//! Devices (v0.3.0, spec §12a #16): see which computer has which project.
//!
//! Each computer keeps one small list of its own projects in `<shared folder>/Nest/devices/`
//! (`devices-dev/` for dev builds) and reads the other computers' lists from there. A cloud
//! drive app (Google Drive, Dropbox…) or a NAS does the syncing: Nest only reads and writes
//! files (locked decision #9). Rules (the v0.3 Devices plan review fixes):
//! - only this computer's own file is ever written, atomically, and only when it changed;
//! - only `<uuid>.json` files whose `deviceId` matches are read (conflict copies are ignored);
//! - every file is capped (size, projects, string lengths); broken files are skipped with a
//!   note and never "fixed"; the last good copy of each computer is kept;
//! - paths from other computers are display-only and never used to touch anything.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::index::{ProjectRow, RemoteDevice};
use crate::manifest::write_json_atomic;
use crate::plan::ClientValue;

pub const DEVICE_SCHEMA: u32 = 1;
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_PROJECTS: usize = 5_000;
pub const MAX_FILES: usize = 50;
pub const MAX_REQUESTS: usize = 1_000;
pub const MAX_HANDLED: usize = 5_000;
const MAX_STR: usize = 400;

/// One computer's list, as written to `<device id>.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DeviceFile {
    pub schema: u32,
    pub device_id: String,
    pub name: String,
    /// "windows" | "macos" | other.
    pub os: String,
    /// RFC 3339, display only ("updated 3 min ago"); never used to merge anything.
    pub updated_at: String,
    pub projects: Vec<DeviceProject>,
    /// v0.3.2: "mark done / active" requests this computer sends to the computer that owns a
    /// project. Only that computer changes the project, in its own project file.
    pub requests: Vec<StatusRequest>,
    /// v0.3.2: requests from other computers this one has dealt with (applied or refused), so
    /// their senders can drop them.
    pub handled: Vec<Handled>,
    /// v0.6.1: per sending computer, the request number up to which every request has been
    /// dealt with (`handled` also lists ones above it, dealt with while an earlier one waits).
    /// Lets "This is this computer" take back exactly where this list was. Missing in older
    /// files.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handled_upto: Option<BTreeMap<String, u64>>,
}

/// "Please mark this project done (or active)", from one computer to the one that has it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct StatusRequest {
    /// Random UUID.
    pub id: String,
    /// Counts up per sending computer: the owner never applies a number it has already seen,
    /// even if a stale synced copy brings an old request back.
    pub seq: u64,
    pub to_device: String,
    pub project_id: String,
    /// "active" or "done"; anything else is refused.
    pub status: String,
    /// Display only.
    pub requested_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Handled {
    pub id: String,
    pub from_device: String,
    /// The request's number, so a computer that takes back its list after a reinstall knows
    /// which requests it has already dealt with.
    pub seq: u64,
    pub applied: bool,
    /// Why it was refused, in plain words.
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DeviceProject {
    pub id: String,
    pub job_code: String,
    pub title: String,
    pub client: ClientValue,
    pub artist: Option<String>,
    pub space: String,
    pub status: String,
    pub template_id: String,
    pub created_at: String,
    /// Where the folder is on that computer. Display only.
    pub path: String,
    /// Archive (M5): in that computer's archive folder. Missing in older files = false.
    pub archived: bool,
}

/// Where the lists live for this build.
pub fn devices_dir(shared: &Path, dev_build: bool) -> PathBuf {
    shared
        .join("Nest")
        .join(if dev_build { "devices-dev" } else { "devices" })
}

/// `Some(id)` if `s` is a UUID (the only thing a device file may be named after).
pub fn valid_device_id(s: &str) -> Option<String> {
    uuid::Uuid::parse_str(s)
        .ok()
        .map(|u| u.hyphenated().to_string())
}

pub fn own_file(dir: &Path, device_id: &str) -> Option<PathBuf> {
    valid_device_id(device_id).map(|id| dir.join(format!("{id}.json")))
}

/// This computer's list, from the index (projects found on this computer).
pub fn build_own(device_id: &str, name: &str, rows: &[ProjectRow], updated_at: &str) -> DeviceFile {
    let mut projects: Vec<DeviceProject> = rows
        .iter()
        .filter(|r| r.error.is_none())
        .filter_map(|r| {
            let id = r.id.clone()?;
            Some(DeviceProject {
                id,
                job_code: r.job_code.clone(),
                title: r.title.clone(),
                client: r.client.clone(),
                artist: r
                    .fields
                    .get("artist")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                space: r.space.clone(),
                status: r.status.clone(),
                template_id: r.template_id.clone(),
                created_at: r.created_at.clone(),
                path: r
                    .paths
                    .first()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                archived: r.archived,
            })
        })
        .collect();
    projects.sort_by(|a, b| a.id.cmp(&b.id));
    projects.truncate(MAX_PROJECTS);
    DeviceFile {
        schema: DEVICE_SCHEMA,
        device_id: device_id.to_string(),
        name: name.to_string(),
        os: std::env::consts::OS.to_string(),
        updated_at: updated_at.to_string(),
        projects,
        requests: vec![],
        handled: vec![],
        handled_upto: None,
    }
}

/// Write this computer's list if its content changed since `last` (the timestamp doesn't
/// count). Retries briefly: a sync app may hold the file for a moment. Returns whether it wrote.
pub fn write_own(
    dir: &Path,
    file: &DeviceFile,
    last: &mut Option<DeviceFile>,
) -> Result<bool, String> {
    let same = last.as_ref().is_some_and(|l| {
        DeviceFile {
            updated_at: file.updated_at.clone(),
            ..l.clone()
        } == *file
    });
    if same {
        return Ok(false);
    }
    let path = own_file(dir, &file.device_id).ok_or("This computer's device id isn't valid")?;
    // Only ever create `Nest/devices` inside a synced folder that already exists: if Google
    // Drive is signed out or a NAS isn't mounted, recreating its path as a plain local folder
    // would confuse the cloud app (PLAN review, v0.3.0).
    let shared = dir.parent().and_then(Path::parent);
    if !shared.is_some_and(Path::is_dir) {
        return Err(
            "The synced folder isn't available. Is Google Drive (or your NAS) connected?".into(),
        );
    }
    for d in [dir.parent().unwrap_or(dir), dir] {
        if !d.is_dir() {
            fs::create_dir(d).map_err(|e| format!("Couldn't create {}: {e}", d.display()))?;
        }
    }
    let value = serde_json::to_value(file).map_err(|e| e.to_string())?;
    let mut wait = Duration::from_millis(100);
    let mut tries = 0;
    loop {
        match write_json_atomic(&path, &value, false) {
            Ok(()) => break,
            Err(e) if tries < 3 => {
                log::info!("device list write retry: {e}");
                thread::sleep(wait);
                wait *= 3;
                tries += 1;
            }
            Err(e) => return Err(format!("Couldn't save this computer's list: {e}")),
        }
    }
    *last = Some(file.clone());
    Ok(true)
}

/// Another computer, as read from its file (or the last good copy).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OtherDevice {
    pub device_id: String,
    pub name: String,
    pub os: String,
    pub updated_at: String,
    pub projects: Vec<DeviceProject>,
    /// Its requests to other computers and the ones it has dealt with (v0.3.2).
    #[serde(skip)]
    pub requests: Vec<StatusRequest>,
    #[serde(skip)]
    pub handled: Vec<Handled>,
    #[serde(skip)]
    pub handled_upto: Option<BTreeMap<String, u64>>,
    /// Plain words when the latest file couldn't be used (the last good copy is shown).
    pub problem: Option<String>,
    /// Only a cloud placeholder so far (e.g. iCloud `.icloud`): "not downloaded yet".
    pub not_downloaded: bool,
}

/// Read every other computer's list in `dir`. `cache` holds the last good copy of each, so a
/// half-synced or broken file never makes a computer's projects disappear.
pub fn read_others(
    dir: &Path,
    own_id: &str,
    cache: &mut HashMap<String, OtherDevice>,
) -> Vec<OtherDevice> {
    let Ok(entries) = fs::read_dir(dir) else {
        return cache.values().cloned().collect();
    };
    let mut seen = 0;
    let mut placeholders = vec![];
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // iCloud keeps not-yet-downloaded files as `.<name>.icloud`.
        if let Some(inner) = name
            .strip_prefix('.')
            .and_then(|n| n.strip_suffix(".icloud"))
        {
            if let Some(id) = inner.strip_suffix(".json").and_then(valid_device_id) {
                placeholders.push(id);
            }
            continue;
        }
        let Some(id) = name.strip_suffix(".json").and_then(valid_device_id) else {
            continue; // conflict copies ("x (1).json"), temp files, anything else
        };
        if id == own_id || name != format!("{id}.json") {
            continue;
        }
        seen += 1;
        if seen > MAX_FILES {
            break;
        }
        match read_one(&entry.path(), &id) {
            Ok(file) => {
                cache.insert(
                    id.clone(),
                    OtherDevice {
                        device_id: id,
                        name: file.name,
                        os: file.os,
                        updated_at: file.updated_at,
                        projects: file.projects,
                        requests: file.requests,
                        handled: file.handled,
                        handled_upto: file.handled_upto,
                        problem: None,
                        not_downloaded: false,
                    },
                );
            }
            Err(problem) => {
                log::info!(
                    "device list skipped ({}): {problem}",
                    entry.path().display()
                );
                let d = cache.entry(id.clone()).or_insert_with(|| OtherDevice {
                    device_id: id,
                    name: "Another computer".into(),
                    os: String::new(),
                    updated_at: String::new(),
                    projects: vec![],
                    requests: vec![],
                    handled: vec![],
                    handled_upto: None,
                    problem: None,
                    not_downloaded: false,
                });
                d.problem = Some(problem);
            }
        }
    }
    for id in placeholders {
        if id != own_id && !cache.contains_key(&id) {
            cache.insert(
                id.clone(),
                OtherDevice {
                    device_id: id,
                    name: "Another computer".into(),
                    os: String::new(),
                    updated_at: String::new(),
                    projects: vec![],
                    requests: vec![],
                    handled: vec![],
                    handled_upto: None,
                    problem: None,
                    not_downloaded: true,
                },
            );
        }
    }
    let mut out: Vec<OtherDevice> = cache.values().cloned().collect();
    out.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.device_id.cmp(&b.device_id))
    });
    out
}

fn read_one(path: &Path, id: &str) -> Result<DeviceFile, String> {
    let meta = fs::metadata(path).map_err(|e| format!("Its list can't be read ({e})."))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err("Its list is too big to be a Nest list.".into());
    }
    let mut text = String::new();
    fs::File::open(path)
        .and_then(|f| f.take(MAX_FILE_BYTES + 1).read_to_string(&mut text))
        .map_err(|e| format!("Its list can't be read ({e})."))?;
    let mut file: DeviceFile = serde_json::from_str(&text)
        .map_err(|_| "Its list isn't complete yet (still syncing?).".to_string())?;
    if valid_device_id(&file.device_id).as_deref() != Some(id) {
        return Err("The list belongs to a different computer.".into());
    }
    if file.requests.len() > MAX_REQUESTS || file.handled.len() > MAX_HANDLED {
        return Err("Its list has too many requests.".into());
    }
    for r in &mut file.requests {
        for s in [
            &mut r.id,
            &mut r.to_device,
            &mut r.project_id,
            &mut r.status,
            &mut r.requested_at,
        ] {
            clamp(s);
        }
    }
    for h in &mut file.handled {
        clamp(&mut h.id);
        clamp(&mut h.from_device);
        if let Some(r) = &mut h.reason {
            clamp(r);
        }
    }
    if file.projects.len() > MAX_PROJECTS {
        return Err("Its list has too many projects.".into());
    }
    clamp(&mut file.name);
    clamp(&mut file.os);
    clamp(&mut file.updated_at);
    for p in &mut file.projects {
        for s in [
            &mut p.id,
            &mut p.job_code,
            &mut p.title,
            &mut p.client.name,
            &mut p.client.code,
            &mut p.space,
            &mut p.status,
            &mut p.template_id,
            &mut p.created_at,
            &mut p.path,
        ] {
            clamp(s);
        }
        if let Some(a) = &mut p.artist {
            clamp(a);
        }
    }
    if file.name.trim().is_empty() {
        file.name = "Another computer".into();
    }
    Ok(file)
}

fn clamp(s: &mut String) {
    if s.chars().count() > MAX_STR {
        *s = s.chars().take(MAX_STR).collect();
    }
}

/// Remove leftover temp files (`.nest-<32 hex>.tmp`, exactly what `write_json_atomic` makes)
/// older than a day: a sync app would otherwise upload them. A day, not less, because this
/// folder is shared: another computer's write still in progress (its clock may differ) is
/// never touched. Nothing else is removed (see tidy.rs for the name rule).
pub fn clean_temp_files(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let Some(day_ago) = SystemTime::now().checked_sub(Duration::from_secs(24 * 3600)) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !crate::tidy::is_json_tmp(&name) {
            continue;
        }
        let old = e
            .metadata()
            .is_ok_and(|m| m.is_file() && m.modified().is_ok_and(|t| t < day_ago));
        if old {
            let _ = fs::remove_file(e.path());
        }
    }
}

// ───────────────────────── Finding Google Drive ─────────────────────────

/// A synced folder Nest found on this computer.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncedFolder {
    pub label: String,
    pub path: PathBuf,
}

/// Does `path` exist? Gives up after `timeout` (a disconnected network drive can hang).
fn exists_within(path: PathBuf, timeout: Duration) -> bool {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(path.exists());
    });
    rx.recv_timeout(timeout).unwrap_or(false)
}

/// The folders at a Google Drive root you could sync through, "My Drive" first. Drive names
/// them in your language ("Meine Ablage", "내 드라이브"), so rather than guess, every visible
/// folder is offered and you pick.
fn drive_folders(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return vec![];
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            !n.starts_with('.') && !n.starts_with('$')
        })
        .map(|e| e.path())
        .collect();
    dirs.sort_by_key(|d| (d.file_name().is_none_or(|n| n != "My Drive"), d.clone()));
    dirs
}

/// Google Drive for desktop on this computer (both OSes). Never probes blindly without a
/// timeout. Anything else is chosen with "Choose another folder…".
pub fn detect_google_drive() -> Vec<SyncedFolder> {
    let mut out = vec![];
    let timeout = Duration::from_millis(400);
    if cfg!(windows) {
        // Streaming mode: a virtual drive (usually G:) with Drive's bookkeeping folder at its root.
        for letter in b'C'..=b'Z' {
            let root = PathBuf::from(format!("{}:\\", letter as char));
            if exists_within(root.join(".shortcut-targets-by-id"), timeout) {
                for dir in drive_folders(&root) {
                    out.push(SyncedFolder {
                        label: format!(
                            "Google Drive ({}:) · {}",
                            letter as char,
                            folder_name(&dir)
                        ),
                        path: dir,
                    });
                }
            }
        }
        // Mirror mode: a normal folder in your user folder.
        if let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
            let mirror = home.join("My Drive");
            if mirror.is_dir() {
                out.push(SyncedFolder {
                    label: "Google Drive".into(),
                    path: mirror,
                });
            }
        }
    } else if cfg!(target_os = "macos") {
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            let cloud = home.join("Library").join("CloudStorage");
            if let Ok(entries) = fs::read_dir(&cloud) {
                for e in entries.flatten() {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if let Some(account) = name.strip_prefix("GoogleDrive-") {
                        for dir in drive_folders(&e.path()) {
                            out.push(SyncedFolder {
                                label: format!("Google Drive ({account}) · {}", folder_name(&dir)),
                                path: dir,
                            });
                        }
                    }
                }
            }
        }
    }
    out
}
/// Other computers' projects, merged into this computer's list. The same project on both shows
/// once (as this computer's, "also on MacBook"); job codes used twice are flagged.
pub fn merge(rows: &mut Vec<ProjectRow>, others: &[OtherDevice]) {
    // Every project id shows once: this computer's row if it has one, else the first other
    // computer's; any further computers are listed as "also on".
    let mut by_id: HashMap<String, usize> = rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| r.id.clone().filter(|id| !id.is_empty()).map(|id| (id, i)))
        .collect();
    for d in others {
        for p in &d.projects {
            if p.id.is_empty() {
                continue; // can't be matched to anything; never shown
            }
            if let Some(&i) = by_id.get(&p.id) {
                if !rows[i].also_on.contains(&d.name)
                    && rows[i].device.as_ref().map(|r| &r.name) != Some(&d.name)
                {
                    rows[i].also_on.push(d.name.clone());
                }
                continue;
            }
            by_id.insert(p.id.clone(), rows.len());
            rows.push(ProjectRow {
                key: format!("remote:{}:{}", d.device_id, p.id),
                id: Some(p.id.clone()),
                job_code: p.job_code.clone(),
                title: p.title.clone(),
                client: p.client.clone(),
                space: p.space.clone(),
                status: p.status.clone(),
                template_id: p.template_id.clone(),
                created_at: p.created_at.clone(),
                fields: match &p.artist {
                    Some(a) => serde_json::json!({ "artist": a }),
                    None => serde_json::json!({}),
                },
                error: None,
                paths: vec![],
                offline: false,
                duplicate_code: false,
                device: Some(RemoteDevice {
                    device_id: d.device_id.clone(),
                    name: d.name.clone(),
                    updated_at: d.updated_at.clone(),
                    path: p.path.clone(),
                }),
                also_on: vec![],
                template_name: String::new(),
                pending: None,
                request_problem: None,
                changed_by: None,
                archived: p.archived,
                done_at: None,
                size: None,
                ready_to_archive: false,
            });
        }
    }
    // A job code shared by two different projects (across computers) is a duplicate.
    let mut by_code: HashMap<String, Vec<String>> = HashMap::new();
    for r in rows.iter() {
        if !r.job_code.is_empty() {
            let id = r.id.clone().unwrap_or_else(|| r.key.clone());
            let ids = by_code.entry(r.job_code.to_lowercase()).or_default();
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    for r in rows.iter_mut() {
        if by_code
            .get(&r.job_code.to_lowercase())
            .is_some_and(|ids| ids.len() > 1)
        {
            r.duplicate_code = true;
        }
    }
}

fn folder_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// "This is this computer" (after reinstalling) takes back another computer's list. Refused
/// while that list was updated in the last day: another computer is still using it, and two
/// computers writing one file would overwrite each other.
pub fn claim_allowed(updated_at: &str, now: chrono::DateTime<chrono::Utc>) -> Result<(), String> {
    match chrono::DateTime::parse_from_rfc3339(updated_at) {
        Ok(t) if now.signed_duration_since(t) < chrono::Duration::hours(24) => Err(
            "That list was updated in the last day, so another computer is still using it. Only use this after reinstalling Nest on this computer.".into(),
        ),
        Ok(_) => Ok(()),
        // Unreadable or still syncing: it may be in use right now, so don't take it over.
        Err(_) => Err(
            "Nest can't tell when that list was last used (it may still be syncing). Try again in a few minutes.".into(),
        ),
    }
}

// ───────────────────────── Requests ("mark done" from another computer) ─────────────────────────

pub fn valid_status(s: &str) -> bool {
    matches!(s, "active" | "done")
}

/// Requests other computers sent to this one that it hasn't dealt with yet, oldest first per
/// sender. `done_seq` is the number up to which every request from each sender has been dealt
/// with, and `handled` lists the ones dealt with above it, so a request can never be applied
/// twice (even if a stale synced copy brings it back).
pub fn incoming<'a>(
    own_id: &str,
    others: &'a [OtherDevice],
    done_seq: &BTreeMap<String, u64>,
    handled: &[Handled],
) -> Vec<(&'a OtherDevice, &'a StatusRequest)> {
    let seen_seq: HashSet<(&str, u64)> = handled
        .iter()
        .map(|h| (h.from_device.as_str(), h.seq))
        .collect();
    let seen_id: HashSet<&str> = handled.iter().map(|h| h.id.as_str()).collect();
    let mut out: Vec<(&OtherDevice, &StatusRequest)> = others
        .iter()
        .flat_map(|d| d.requests.iter().map(move |r| (d, r)))
        .filter(|(d, r)| {
            r.to_device == own_id
                && r.seq > done_seq.get(&d.device_id).copied().unwrap_or(0)
                && !seen_seq.contains(&(d.device_id.as_str(), r.seq))
                && !seen_id.contains(r.id.as_str())
        })
        .collect();
    out.sort_by(|(a, ra), (b, rb)| a.device_id.cmp(&b.device_id).then(ra.seq.cmp(&rb.seq)));
    out
}

/// What applying one request here came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Applied,
    /// Can pass (a file busy or locked, the project file mid-sync, a folder offline): stays
    /// waiting and is tried again at the next sync. The plain reason is used if it gives up.
    Later(String),
    /// Won't ever work; the reason goes back to the sender in plain words.
    Refused(String),
}

/// A request that has waited this long here is refused with the reason it waited, so one
/// broken project can't hold its sender's bookkeeping back for ever.
pub const GIVE_UP_AFTER_DAYS: i64 = 3;
/// When this many dealt-with notes are still needed (see `note_needed`), waiting requests are
/// refused straight away so the marks can move and the notes can go.
pub const NOTES_PRESSURE: usize = MAX_HANDLED / 2;
/// "First tried at" times kept for waiting requests (oldest dropped first).
pub const MAX_WAITING_SINCE: usize = 2_000;

/// One round of dealing with requests (see `handle_requests`).
#[derive(Debug, Default)]
pub struct Round<'a> {
    /// Applied this round, in order.
    pub applied: Vec<(&'a OtherDevice, &'a StatusRequest)>,
    /// Something was recorded (applied or refused).
    pub recorded: bool,
    /// Recording failed: the rest wait, and nothing more should be handled until a restart.
    pub stopped: bool,
}

/// What a round needs besides the requests: when each waiting request was first tried here
/// (request id → RFC 3339, kept in settings), the time now, and whether too many notes are
/// still needed (then nothing is left waiting).
pub struct Waiting<'w> {
    pub since: &'w mut BTreeMap<String, String>,
    pub now: chrono::DateTime<chrono::Utc>,
    pub pressure: bool,
}

/// Deal with `pending` (from `incoming`): each request on its own. One that can't be applied
/// right now waits, and only later requests for the same project wait with it (so they keep
/// their order); requests for other projects go ahead. A request that has waited
/// `GIVE_UP_AFTER_DAYS` (or any, under note pressure) is refused with its reason instead.
/// `record` saves each dealt-with request straight away (before the next one), so a crash can
/// never apply one twice; if it fails, everything after it waits.
pub fn handle_requests<'a>(
    pending: Vec<(&'a OtherDevice, &'a StatusRequest)>,
    waiting: Waiting<'_>,
    mut apply: impl FnMut(&OtherDevice, &StatusRequest) -> Outcome,
    mut record: impl FnMut(Handled) -> bool,
) -> Round<'a> {
    let mut round = Round::default();
    let mut blocked: HashSet<&str> = HashSet::new();
    for (from, req) in pending {
        if round.stopped || blocked.contains(req.project_id.as_str()) {
            continue;
        }
        let (applied, reason) = match apply(from, req) {
            Outcome::Later(why) => {
                let first = waiting
                    .since
                    .entry(req.id.clone())
                    .or_insert_with(|| waiting.now.to_rfc3339());
                let waited = chrono::DateTime::parse_from_rfc3339(first)
                    .map(|t| waiting.now.signed_duration_since(t))
                    .unwrap_or_default();
                if waiting.pressure {
                    (false, Some(format!("{why} Too many changes were waiting there, so this one wasn't made. Try again.")))
                } else if waited >= chrono::Duration::days(GIVE_UP_AFTER_DAYS) {
                    (false, Some(format!("{why} It still couldn't be done after {GIVE_UP_AFTER_DAYS} days, so nothing changed. Try again once it's sorted out there.")))
                } else {
                    blocked.insert(req.project_id.as_str());
                    continue;
                }
            }
            Outcome::Applied => (true, None),
            Outcome::Refused(r) => (false, Some(r)),
        };
        let note = Handled {
            id: req.id.clone(),
            from_device: from.device_id.clone(),
            seq: req.seq,
            applied,
            reason,
        };
        if !record(note) {
            round.stopped = true;
            continue;
        }
        waiting.since.remove(&req.id);
        round.recorded = true;
        if applied {
            round.applied.push((from, req));
        }
    }
    // Keep the "first tried" times bounded: forget the oldest beyond the cap (a request
    // withdrawn by its sender leaves one behind until then).
    while waiting.since.len() > MAX_WAITING_SINCE {
        let oldest = waiting
            .since
            .iter()
            .min_by(|a, b| a.1.cmp(b.1))
            .map(|(k, _)| k.clone());
        match oldest {
            Some(k) => waiting.since.remove(&k),
            None => break,
        };
    }
    round
}

/// How far each sender's "dealt with up to" number may move: below the lowest request from it
/// to this computer still not dealt with. Only senders whose list was read cleanly just now
/// (folder online, no problem, downloaded) get a limit; for the rest nothing moves, because a
/// missing or half-synced list proves nothing about what still waits.
pub fn mark_limits(
    own_id: &str,
    others: &[OtherDevice],
    online: bool,
    marks: &BTreeMap<String, u64>,
    handled: &[Handled],
) -> BTreeMap<String, u64> {
    if !online {
        return BTreeMap::new();
    }
    others
        .iter()
        .filter(|d| d.problem.is_none() && !d.not_downloaded)
        .map(|d| {
            let lowest = incoming(own_id, std::slice::from_ref(d), marks, handled)
                .first()
                .map_or(u64::MAX, |(_, r)| r.seq);
            (d.device_id.clone(), lowest)
        })
        .collect()
}

/// Move each sender's mark up to its highest dealt-with request below its limit (from
/// `mark_limits`). Senders without a limit don't move. Returns whether anything moved.
pub fn advance_marks(
    marks: &mut BTreeMap<String, u64>,
    handled: &[Handled],
    limits: &BTreeMap<String, u64>,
) -> bool {
    let mut moved = false;
    for h in handled {
        let Some(limit) = limits.get(&h.from_device) else {
            continue;
        };
        if h.seq >= *limit {
            continue;
        }
        let m = marks.entry(h.from_device.clone()).or_insert(0);
        if h.seq > *m {
            *m = h.seq;
            moved = true;
        }
    }
    moved
}

/// A dealt-with note is still needed while its number is above its sender's mark: it's the
/// only thing stopping that request from being applied again.
pub fn note_needed(h: &Handled, marks: &BTreeMap<String, u64>) -> bool {
    h.seq > marks.get(&h.from_device).copied().unwrap_or(0)
}

/// Whether so many notes are still needed that waiting requests should be refused now.
pub fn notes_pressure(handled: &[Handled], marks: &BTreeMap<String, u64>) -> bool {
    handled.iter().filter(|h| note_needed(h, marks)).count() >= NOTES_PRESSURE
}

/// Keep at most `max` notes by dropping the oldest ones the marks already cover. A note still
/// needed is never dropped (that could apply a request twice); `notes_pressure` keeps those
/// few by refusing what waits, so the marks move.
pub fn trim_handled(handled: &mut Vec<Handled>, marks: &BTreeMap<String, u64>, max: usize) {
    let mut extra = handled.len().saturating_sub(max);
    if extra == 0 {
        return;
    }
    handled.retain(|h| {
        if extra > 0 && !note_needed(h, marks) {
            extra -= 1;
            false
        } else {
            true
        }
    });
}

/// "This is this computer": where the old list was, per sender. Lists from before v0.6.1
/// dealt with requests strictly in order, so their highest dealt-with number is the mark.
pub fn taken_back_marks(
    handled: &[Handled],
    handled_upto: Option<&BTreeMap<String, u64>>,
) -> BTreeMap<String, u64> {
    if let Some(m) = handled_upto {
        return m.clone();
    }
    let mut marks = BTreeMap::new();
    for h in handled {
        let m = marks.entry(h.from_device.clone()).or_insert(0);
        *m = (*m).max(h.seq);
    }
    marks
}

/// "This is this computer": take the old list's request bookkeeping as it is. From now on
/// only requests to the old id count, and its marks and notes say exactly which of those were
/// dealt with; this install's own marks (for its short-lived id) would skip ones still waiting.
/// This computer's own outgoing requests and their numbers are merged in.
pub fn take_back_requests(s: &mut crate::settings::Settings, old: &OtherDevice) {
    s.handled_seq = taken_back_marks(&old.handled, old.handled_upto.as_ref());
    s.handled_requests = old.handled.clone();
    s.waiting_since.clear();
    for q in &old.requests {
        if !s.outgoing_requests.iter().any(|o| o.id == q.id) {
            s.outgoing_requests.push(q.clone());
        }
        s.request_seq = s.request_seq.max(q.seq);
    }
}

/// What showing a project needs to know about requests: the latest one this computer is still
/// waiting on (per project), and refusals that came back.
pub fn annotate(
    rows: &mut [ProjectRow],
    outgoing: &[StatusRequest],
    others: &[OtherDevice],
    refusals: &HashMap<String, String>,
) {
    for r in rows.iter_mut() {
        let Some(dev) = &r.device else { continue };
        let Some(id) = &r.id else { continue };
        let waiting = outgoing
            .iter()
            .filter(|q| &q.project_id == id && q.to_device == dev.device_id)
            .max_by_key(|q| q.seq);
        if let Some(q) = waiting {
            let handled = others
                .iter()
                .filter(|d| d.device_id == dev.device_id)
                .any(|d| d.handled.iter().any(|h| h.id == q.id));
            if !handled {
                r.pending = Some(q.status.clone());
            }
        }
        if let Some(reason) = refusals.get(id) {
            r.request_problem = Some(reason.clone());
        }
    }
}

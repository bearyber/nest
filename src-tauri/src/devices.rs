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

use std::collections::HashMap;
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

/// Remove our own leftover temp files (`.nest-*.tmp`) older than an hour: a sync app would
/// otherwise upload them.
pub fn clean_temp_files(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let hour_ago = SystemTime::now() - Duration::from_secs(3600);
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with(".nest-") && name.ends_with(".tmp") {
            let old = e
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|t| t < hour_ago);
            if old {
                let _ = fs::remove_file(e.path());
            }
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
        _ => Ok(()),
    }
}

// ───────────────────────── Requests ("mark done" from another computer) ─────────────────────────

pub fn valid_status(s: &str) -> bool {
    matches!(s, "active" | "done")
}

/// Requests other computers sent to this one that it hasn't dealt with yet, oldest first per
/// sender. `done_seq` is the highest request number already dealt with from each sender, so a
/// request can never be applied twice (even if a stale synced copy brings it back).
pub fn incoming<'a>(
    own_id: &str,
    others: &'a [OtherDevice],
    done_seq: &std::collections::BTreeMap<String, u64>,
) -> Vec<(&'a OtherDevice, &'a StatusRequest)> {
    let mut out: Vec<(&OtherDevice, &StatusRequest)> = others
        .iter()
        .flat_map(|d| d.requests.iter().map(move |r| (d, r)))
        .filter(|(d, r)| {
            r.to_device == own_id && r.seq > done_seq.get(&d.device_id).copied().unwrap_or(0)
        })
        .collect();
    out.sort_by(|(a, ra), (b, rb)| a.device_id.cmp(&b.device_id).then(ra.seq.cmp(&rb.seq)));
    out
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

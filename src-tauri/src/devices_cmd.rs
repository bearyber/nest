//! Commands and background syncing for Devices (v0.3.0; see devices.rs for the file rules).
//!
//! Syncing writes this computer's list (only if it changed) and reads the others'. It runs on
//! launch, every minute, when a window gets focus, after a rescan / Create / status change,
//! and on quit. One sync at a time; it never runs while the settings file is damaged.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::commands::{lock, AppState};
use crate::devices::{self, DeviceFile, OtherDevice, SyncedFolder};
use crate::error::{AppError, AppResult};
use crate::index::ChangedBy;

use crate::settings::Settings;

pub const CHANGED_EVENT: &str = "devices-changed";
const EVERY: Duration = Duration::from_secs(60);

/// What Devices remembers between syncs.
#[derive(Default)]
pub struct DevicesRuntime {
    /// Last good copy of each other computer's list (hidden ones too).
    pub cache: HashMap<String, OtherDevice>,
    /// The other computers shown (hidden ones left out).
    pub others: Vec<OtherDevice>,
    pub last_written: Option<DeviceFile>,
    /// When this computer's list was last saved (RFC 3339).
    pub saved_at: Option<String>,
    /// Plain words when saving or reading failed.
    pub problem: Option<String>,
    /// The (device id, folder) this state belongs to: a change of either starts afresh.
    pub for_setup: Option<(String, PathBuf)>,
    /// v0.3.2: why the owning computer refused this computer's request, per project id.
    pub refusals: HashMap<String, String>,
    /// v0.3.2: this computer's projects whose status another computer changed, per project id.
    pub changes: HashMap<String, ChangedBy>,
    /// A dealt-with request couldn't be recorded: stop handling requests until Nest restarts,
    /// rather than risk applying one again and again.
    pub requests_paused: bool,
    /// v0.5 template sync: plain words when it couldn't run.
    pub template_problem: Option<String>,
}

#[derive(Default)]
pub struct Devices {
    pub runtime: Mutex<DevicesRuntime>,
    pub syncing: AtomicBool,
    /// A sync was asked for while one ran: run once more when it finishes.
    pub again: AtomicBool,
    /// Quitting: template sync only writes (nothing is installed on the way out).
    pub quitting: AtomicBool,
}

fn now_rfc3339() -> String {
    chrono::Local::now()
        .format("%Y-%m-%dT%H:%M:%S%:z")
        .to_string()
}

/// Devices is on when a valid device id and a shared folder are set.
fn enabled(s: &Settings) -> Option<(String, String, PathBuf)> {
    let id = devices::valid_device_id(s.device_id.as_deref()?)?;
    let folder = s.shared_folder.clone()?;
    let name = s
        .device_name
        .clone()
        .unwrap_or_else(|| "This computer".into());
    Some((id, name, folder))
}

/// Write this computer's list and read the others'. Quiet: problems are kept for Settings.
/// One sync at a time; a request made meanwhile runs once more afterwards (so a project
/// created during a sync isn't left waiting a minute).
pub fn sync_now(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    // Reads the synced folder and your jobs folders: not before a window shows after a hidden
    // start on a Mac (folder_access.rs). Opening the gate syncs.
    if !crate::folder_access::is_open(app) {
        return;
    }
    let dev = &state.devices;
    if dev.syncing.swap(true, Ordering::SeqCst) {
        dev.again.store(true, Ordering::SeqCst);
        return;
    }
    // An update is installing (Nest is about to restart): skip this one, the next launch syncs.
    // Checked after setting `syncing`, so the install sees a sync that got here first.
    if crate::update::holds_back_sync(
        crate::update::installing(app),
        dev.quitting.load(Ordering::SeqCst),
    ) {
        dev.syncing.store(false, Ordering::SeqCst);
        return;
    }
    loop {
        dev.again.store(false, Ordering::SeqCst);
        if sync_inner(&state, dev) {
            let _ = app.emit(CHANGED_EVENT, ());
        }
        crate::template_sync_cmd::tick(app, !dev.quitting.load(Ordering::SeqCst));
        if !dev.again.load(Ordering::SeqCst) {
            break;
        }
    }
    dev.syncing.store(false, Ordering::SeqCst);
}

/// The file work happens with no lock held, so a slow Google Drive never freezes the list or
/// the New Project preview: copy what's needed, release, do the work, then store briefly.
/// v0.3.2: also deals with "mark done" requests: applies the ones sent to this computer (in
/// order, each once, recorded right away) and drops this computer's requests that the owner
/// has dealt with.
fn sync_inner(state: &AppState, dev: &Devices) -> bool {
    if state.settings_error.is_some() {
        return false;
    }
    let Ok(settings) = state.settings.lock().map(|s| s.clone()) else {
        return false;
    };
    let Some((id, name, folder)) = enabled(&settings) else {
        return false;
    };
    let setup = (id.clone(), folder.clone());
    let dir = devices::devices_dir(&folder, cfg!(debug_assertions));
    // 1. Take a copy of what we remember (a different id or folder starts afresh).
    let (mut last, mut cache, before) = {
        let Ok(rt) = dev.runtime.lock() else {
            return false;
        };
        let before = (rt.others.clone(), rt.problem.clone());
        if rt.for_setup.as_ref() == Some(&setup) {
            (rt.last_written.clone(), rt.cache.clone(), before)
        } else {
            (None, HashMap::new(), before)
        }
    };
    // 2. File work, no lock held.
    let online = folder.is_dir();
    let all: Vec<OtherDevice> = if online {
        devices::read_others(&dir, &id, &mut cache)
    } else {
        cache.values().cloned().collect()
    };
    let forgotten = |d: &str| settings.forgotten_devices.iter().any(|f| f == d);
    let others: Vec<OtherDevice> = all
        .iter()
        .filter(|d| !forgotten(&d.device_id))
        .cloned()
        .collect();

    // 2a. Requests sent to this computer: each applied once and recorded straight away (so a
    // crash can never apply it twice). One that can't be applied right now (a file busy, the
    // project file mid-sync, its jobs folder offline…) waits for the next sync; only later
    // requests for that same project wait with it. Only what won't ever work is refused.
    let mut refusal_updates: Vec<(String, Option<String>)> = vec![];
    let mut new_changes: Vec<(String, ChangedBy)> = vec![];
    let mut changed_requests = false;
    let paused = dev
        .runtime
        .lock()
        .map(|rt| rt.requests_paused)
        .unwrap_or(true);
    if !paused {
        let pending =
            devices::incoming(&id, &all, &settings.handled_seq, &settings.handled_requests);
        let mut since = settings.waiting_since.clone();
        let waiting = devices::Waiting {
            since: &mut since,
            now: chrono::Utc::now(),
            pressure: devices::notes_pressure(&settings.handled_requests, &settings.handled_seq),
        };
        let round = devices::handle_requests(
            pending,
            waiting,
            |from, req| {
                if forgotten(&from.device_id) {
                    return devices::Outcome::Refused(format!(
                        "{} was removed on {name}.",
                        from.name
                    ));
                }
                crate::commands::apply_request(state, &req.project_id, &req.status, &name)
            },
            |note| {
                let saved = crate::settings_cmd::edit_settings_quietly(state, move |s| {
                    s.handled_requests.push(note);
                    devices::trim_handled(
                        &mut s.handled_requests,
                        &s.handled_seq,
                        devices::MAX_HANDLED,
                    );
                    Ok(())
                });
                match saved {
                    Ok(()) => true,
                    Err(e) => {
                        log::warn!("couldn't record a Devices request: {}", e.message);
                        false
                    }
                }
            },
        );
        if round.stopped {
            // Can't record what was dealt with: stop handling requests until Nest restarts,
            // rather than risk applying one again and again.
            if let Ok(mut rt) = dev.runtime.lock() {
                rt.requests_paused = true;
            }
        }
        changed_requests |= round.recorded;
        for (from, req) in &round.applied {
            new_changes.push((
                req.project_id.clone(),
                ChangedBy {
                    name: from.name.clone(),
                    status: req.status.clone(),
                    at: now_rfc3339(),
                },
            ));
        }
        // Move each sender's "dealt with up to" number to just below its lowest request still
        // waiting (in its list, or seen waiting here before), only for senders whose list was
        // read cleanly just now and isn't older than one already read; and keep the waiting
        // requests and the newest list times.
        let mut newest = settings.newest_lists.clone();
        let trusted = devices::trusted_lists(&all, online, &mut newest);
        let moves = |s: &Settings| {
            let mut marks = s.handled_seq.clone();
            let limits = devices::mark_limits(
                &id,
                &all,
                &trusted,
                &s.handled_seq,
                &s.handled_requests,
                &since,
            );
            devices::advance_marks(&mut marks, &s.handled_requests, &limits)
        };
        let needed = state
            .settings
            .lock()
            .is_ok_and(|s| moves(&s) || s.waiting_since != since || s.newest_lists != newest);
        if needed {
            if let Err(e) = crate::settings_cmd::edit_settings_quietly(state, |s| {
                let limits = devices::mark_limits(
                    &id,
                    &all,
                    &trusted,
                    &s.handled_seq,
                    &s.handled_requests,
                    &since,
                );
                let notes = s.handled_requests.clone();
                devices::advance_marks(&mut s.handled_seq, &notes, &limits);
                s.waiting_since = since;
                s.newest_lists = newest;
                Ok(())
            }) {
                // Harmless: the notes still stop anything being applied twice.
                log::warn!("couldn't save Devices request numbers: {}", e.message);
            }
        }
    }

    // 2b. Tidy the bookkeeping:
    // - forget "dealt with" notes once their sender has dropped the request (its number is
    //   remembered, so it can't come back) or the sender is gone;
    // - drop this computer's requests the owner has dealt with (keeping refusals to show), or
    //   whose computer is gone or was removed here.
    let current = state
        .settings
        .lock()
        .map(|s| {
            (
                s.handled_requests.clone(),
                s.outgoing_requests.clone(),
                s.handled_seq.clone(),
            )
        })
        .unwrap_or_default();
    let stale_notes: Vec<String> = current
        .0
        .iter()
        .filter(|h| {
            // A note above its sender's mark is what stops that request being applied
            // again (a stale synced copy could bring it back), so it stays.
            if !online || devices::note_needed(h, &current.2) {
                return false;
            }
            // Only when the sender's list was read fine and no longer has the request. A list
            // that's missing or still syncing proves nothing, so the note stays.
            all.iter()
                .find(|d| d.device_id == h.from_device)
                .is_some_and(|d| {
                    d.problem.is_none()
                        && !d.not_downloaded
                        && !d.requests.iter().any(|r| r.id == h.id)
                })
        })
        .map(|h| h.id.clone())
        .collect();
    let mut dropped: Vec<String> = vec![];
    for q in &current.1 {
        // A missing list never drops a request (it may just be syncing); removing that
        // computer here does.
        if forgotten(&q.to_device) {
            dropped.push(q.id.clone());
            continue;
        }
        let Some(target) = all.iter().find(|d| d.device_id == q.to_device) else {
            continue;
        };
        if let Some(h) = target.handled.iter().find(|h| h.id == q.id) {
            dropped.push(q.id.clone());
            let reason = if h.applied { None } else { h.reason.clone() };
            refusal_updates.push((q.project_id.clone(), reason));
        }
    }
    if !stale_notes.is_empty() || !dropped.is_empty() {
        let (notes, drop) = (stale_notes.clone(), dropped.clone());
        match crate::settings_cmd::edit_settings_quietly(state, move |s| {
            s.handled_requests.retain(|h| !notes.contains(&h.id));
            // Requests added meanwhile stay; only these go.
            s.outgoing_requests.retain(|q| !drop.contains(&q.id));
            Ok(())
        }) {
            Ok(()) => changed_requests = true,
            Err(e) => log::warn!("couldn't tidy Devices requests: {}", e.message),
        }
    }

    // 2c. This computer's list (after any status changes above).
    let mut problem = None;
    let mut saved = None;
    if online {
        let rows = match state.index.lock().map(|i| i.list()) {
            Ok(Ok(rows)) => rows,
            _ => return false,
        };
        let mut own = devices::build_own(&id, &name, &rows, &now_rfc3339());
        if let Ok(s) = state.settings.lock() {
            own.requests = s.outgoing_requests.clone();
            own.handled = s.handled_requests.clone();
            own.handled_upto = Some(s.handled_seq.clone());
        }
        match devices::write_own(&dir, &own, &mut last) {
            Ok(true) => saved = Some(own.updated_at.clone()),
            Ok(false) => {}
            Err(e) => problem = Some(e),
        }
        devices::clean_temp_files(&dir);
    } else {
        problem = Some(format!(
            "The synced folder isn't available: {}. Is Google Drive (or your NAS) connected? Nest shows the last lists it saw.",
            folder.display()
        ));
    }
    // 3. Store, unless Devices was switched off or changed meanwhile. Refusals and "changed
    // from" notes are applied as changes onto what's there now, so a click made during this
    // sync (which clears them) isn't undone.
    let still = state
        .settings
        .lock()
        .ok()
        .and_then(|s| enabled(&s))
        .is_some_and(|(i, _, f)| (i, f) == setup);
    if !still {
        return false;
    }
    let Ok(mut rt) = dev.runtime.lock() else {
        return false;
    };
    if rt.for_setup.as_ref() != Some(&setup) {
        rt.refusals.clear();
        rt.changes.clear();
    }
    rt.for_setup = Some(setup);
    rt.last_written = last;
    if saved.is_some() {
        rt.saved_at = saved;
    }
    rt.cache = cache;
    rt.others = others;
    rt.problem = problem;
    for (project, reason) in refusal_updates {
        match reason {
            Some(r) => {
                rt.refusals.insert(project, r);
            }
            None => {
                rt.refusals.remove(&project);
            }
        }
    }
    let changed_statuses = !new_changes.is_empty();
    for (project, by) in new_changes {
        rt.changes.insert(project, by);
    }
    (rt.others.clone(), rt.problem.clone()) != before || changed_statuses || changed_requests
}

/// Sync in the background (never blocks a command).
pub fn sync_soon(app: &AppHandle) {
    let app = app.clone();
    thread::spawn(move || sync_now(&app));
}

/// On quit: a last sync, but never more than a few seconds (a stalled drive mustn't hang
/// quitting). If a sync is already running, wait briefly for it, then run once more.
pub fn sync_on_quit(app: &AppHandle) {
    let (tx, rx) = std::sync::mpsc::channel();
    let app2 = app.clone();
    if let Some(state) = app.try_state::<AppState>() {
        state.devices.quitting.store(true, Ordering::SeqCst);
    }
    thread::spawn(move || {
        if let Some(state) = app2.try_state::<AppState>() {
            for _ in 0..20 {
                if !state.devices.syncing.load(Ordering::SeqCst) {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
        sync_now(&app2);
        let _ = tx.send(());
    });
    let _ = rx.recv_timeout(Duration::from_secs(4));
}

/// Launch: sync now, then every minute.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    thread::spawn(move || {
        // macOS, hidden start: not before a window shows (folder_access.rs).
        crate::folder_access::wait(&app);
        loop {
            sync_now(&app);
            // Resolve starter projects waiting: try again while Resolve runs.
            crate::resolve_cmd::tick(&app);
            thread::sleep(EVERY);
        }
    });
}

/// Job codes your other computers use, hidden ones included (so a new code never repeats one
/// of theirs).
pub fn remote_codes(app_devices: &Devices) -> Vec<String> {
    app_devices
        .runtime
        .lock()
        .map(|rt| {
            rt.cache
                .values()
                .flat_map(|d| d.projects.iter().map(|p| p.job_code.clone()))
                .filter(|c| !c.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

// ───────────────────────── Commands (Settings → Devices) ─────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicesView {
    pub enabled: bool,
    pub device_id: Option<String>,
    pub name: String,
    pub folder: Option<PathBuf>,
    pub saved_at: Option<String>,
    pub problem: Option<String>,
    pub this_project_count: usize,
    pub others: Vec<OtherSummary>,
    pub forgotten: Vec<OtherSummary>,
    /// v0.5: keep my templates the same on all my computers.
    pub sync_templates: bool,
    pub template_problem: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OtherSummary {
    pub device_id: String,
    pub name: String,
    pub os: String,
    pub updated_at: String,
    pub project_count: usize,
    pub problem: Option<String>,
    pub not_downloaded: bool,
}

fn summary(d: &OtherDevice) -> OtherSummary {
    OtherSummary {
        device_id: d.device_id.clone(),
        name: d.name.clone(),
        os: d.os.clone(),
        updated_at: d.updated_at.clone(),
        project_count: d.projects.len(),
        problem: d.problem.clone(),
        not_downloaded: d.not_downloaded,
    }
}

fn default_name() -> String {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_default();
    if host.trim().is_empty() {
        if cfg!(target_os = "macos") {
            "Mac".into()
        } else {
            "PC".into()
        }
    } else {
        host
    }
}

fn view(state: &AppState) -> AppResult<DevicesView> {
    let dev = &state.devices;
    let s = lock(&state.settings)?.clone();
    let rt = lock(&dev.runtime)?;
    let all: Vec<&OtherDevice> = rt.cache.values().collect();
    let mut forgotten: Vec<OtherSummary> = all
        .iter()
        .filter(|d| s.forgotten_devices.contains(&d.device_id))
        .map(|d| summary(d))
        .collect();
    forgotten.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(DevicesView {
        enabled: enabled(&s).is_some(),
        device_id: s.device_id.clone(),
        name: s.device_name.clone().unwrap_or_else(default_name),
        folder: s.shared_folder.clone(),
        saved_at: rt.saved_at.clone(),
        problem: rt.problem.clone(),
        this_project_count: rt
            .last_written
            .as_ref()
            .map(|f| f.projects.len())
            .unwrap_or(0),
        others: rt.others.iter().map(summary).collect(),
        forgotten,
        sync_templates: s.sync_templates,
        template_problem: rt.template_problem.clone(),
    })
}

#[tauri::command(async)]
pub fn devices_view(state: State<'_, AppState>) -> AppResult<DevicesView> {
    view(&state)
}

/// Google Drive folders found on this computer (can take a moment).
#[tauri::command(async)]
pub fn devices_detect() -> Vec<SyncedFolder> {
    devices::detect_google_drive()
}

/// "Choose another folder…": any folder your cloud app or NAS syncs.
#[tauri::command(async)]
pub fn devices_pick_folder(window: tauri::WebviewWindow) -> AppResult<Option<PathBuf>> {
    let Some(picked) = window
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("Choose a folder that syncs between your computers")
        .blocking_pick_folder()
    else {
        return Ok(None);
    };
    let path = picked
        .into_path()
        .map_err(|e| AppError::new(format!("That folder can't be used: {e}")))?;
    Ok(Some(path))
}

fn change(app: &AppHandle, f: impl FnOnce(&mut Settings) -> Result<(), String>) -> AppResult<()> {
    crate::settings_cmd::edit_settings(app, f)?;
    sync_soon(app);
    Ok(())
}

/// Turn Devices on (or change the folder / name). A device id is made once.
#[tauri::command(async)]
pub fn devices_enable(app: AppHandle, name: String, folder: PathBuf) -> AppResult<DevicesView> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() || name.chars().count() > 40 {
        return Err(AppError::new(
            "Give this computer a name (up to 40 letters)",
        ));
    }
    if !folder.is_dir() {
        return Err(AppError::new(format!(
            "Folder not found: {}",
            folder.display()
        )));
    }
    // The top of Google Drive's drive (G:\) can't hold files: you sync inside "My Drive".
    if folder.join(".shortcut-targets-by-id").is_dir() {
        return Err(AppError::new(
            "That's the top of Google Drive itself. Pick “My Drive” inside it instead.",
        ));
    }
    change(&app, |s| {
        if s.device_id
            .as_deref()
            .and_then(devices::valid_device_id)
            .is_none()
        {
            s.device_id = Some(uuid::Uuid::new_v4().hyphenated().to_string());
        }
        s.device_name = Some(name);
        s.shared_folder = Some(folder);
        Ok(())
    })?;
    // Show the result straight away.
    sync_now(&app);
    view(&app.state::<AppState>())
}

/// Stop sharing: this computer stops writing and reading lists. (Its last file stays in the
/// folder for the other computers until they remove it; Nest never deletes it.)
#[tauri::command(async)]
pub fn devices_disable(app: AppHandle) -> AppResult<DevicesView> {
    crate::settings_cmd::edit_settings(&app, |s| {
        s.shared_folder = None;
        Ok(())
    })?;
    if let Ok(mut rt) = app.state::<AppState>().devices.runtime.lock() {
        *rt = DevicesRuntime::default();
    }
    let _ = app.emit(CHANGED_EVENT, ());
    view(&app.state::<AppState>())
}

/// Hide (or show again) another computer on this one.
#[tauri::command(async)]
pub fn devices_forget(app: AppHandle, device_id: String, forget: bool) -> AppResult<DevicesView> {
    let id =
        devices::valid_device_id(&device_id).ok_or_else(|| AppError::new("Unknown computer"))?;
    crate::settings_cmd::edit_settings(&app, |s| {
        s.forgotten_devices.retain(|d| d != &id);
        if forget {
            s.forgotten_devices.push(id.clone());
        }
        Ok(())
    })?;
    sync_now(&app);
    let _ = app.emit(CHANGED_EVENT, ());
    view(&app.state::<AppState>())
}

/// "This is me": after reinstalling, take back this computer's old list (no ghost computer).
#[tauri::command(async)]
pub fn devices_claim(app: AppHandle, device_id: String) -> AppResult<DevicesView> {
    let id =
        devices::valid_device_id(&device_id).ok_or_else(|| AppError::new("Unknown computer"))?;
    let found = app
        .state::<AppState>()
        .devices
        .runtime
        .lock()
        .ok()
        .and_then(|rt| {
            rt.cache
                .get(&id)
                .map(|d| (d.name.clone(), d.updated_at.clone(), d.clone()))
        });
    let Some((name, updated_at, old)) = found else {
        return Err(AppError::new(
            "That computer isn't in the synced folder any more",
        ));
    };
    devices::claim_allowed(&updated_at, chrono::Utc::now()).map_err(AppError::new)?;
    let name = Some(name);
    crate::settings_cmd::edit_settings(&app, |s| {
        s.device_id = Some(id.clone());
        if let Some(n) = name {
            s.device_name = Some(n);
        }
        s.forgotten_devices.retain(|d| d != &id);
        // Take back what that computer had dealt with and was waiting on, as it is, so
        // requests are neither applied twice nor lost.
        devices::take_back_requests(s, &old);
        Ok(())
    })?;
    if let Ok(mut rt) = app.state::<AppState>().devices.runtime.lock() {
        rt.cache.remove(&id);
        rt.last_written = None;
    }
    sync_now(&app);
    view(&app.state::<AppState>())
}

/// v0.5: turn template sync on or off. On: sync straight away.
#[tauri::command(async)]
pub fn devices_set_sync_templates(app: AppHandle, on: bool) -> AppResult<DevicesView> {
    change(&app, |s| {
        s.sync_templates = on;
        Ok(())
    })?;
    view(&app.state::<AppState>())
}

/// Window focus: read the other lists again (some drives don't announce changes).
#[tauri::command(async)]
pub fn devices_refresh(app: AppHandle) {
    sync_now(&app);
}

/// v0.3.2: "Mark done / Mark active" on a project that lives on another computer. Nothing
/// changes here: a request goes into this computer's list, and the owning computer applies it
/// next time Nest syncs there (each request once, in order).
#[tauri::command(async)]
pub fn request_status(app: AppHandle, key: String, status: String) -> AppResult<()> {
    if !devices::valid_status(&status) {
        return Err(AppError::new(format!("Unknown status \"{status}\"")));
    }
    let (device_id, project_id) = key
        .strip_prefix("remote:")
        .and_then(|rest| rest.split_once(':'))
        .map(|(d, p)| (d.to_string(), p.to_string()))
        .ok_or_else(|| AppError::new("That project is on this computer"))?;
    let device_id =
        devices::valid_device_id(&device_id).ok_or_else(|| AppError::new("Unknown computer"))?;
    let state = app.state::<AppState>();
    let name = lock(&state.devices.runtime)?
        .cache
        .get(&device_id)
        .map(|d| d.name.clone())
        .ok_or_else(|| AppError::new("That computer isn't in the synced folder any more"))?;
    crate::settings_cmd::edit_settings(&app, |s| {
        if s.outgoing_requests.len() >= devices::MAX_REQUESTS {
            return Err(format!(
                "Too many changes are waiting for {name}. Open Nest on {name} so it can catch up."
            ));
        }
        // Never goes backwards, even after a reinstall resets settings: a time-based floor.
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        s.request_seq = (s.request_seq + 1).max(now_ms);
        s.outgoing_requests.push(devices::StatusRequest {
            id: uuid::Uuid::new_v4().hyphenated().to_string(),
            seq: s.request_seq,
            to_device: device_id.clone(),
            project_id: project_id.clone(),
            status: status.clone(),
            requested_at: now_rfc3339(),
        });
        Ok(())
    })?;
    if let Ok(mut rt) = state.devices.runtime.lock() {
        rt.refusals.remove(&project_id);
    }
    sync_soon(&app);
    Ok(())
}

/// The list's Devices notes: requests this computer is waiting on, refusals that came back,
/// and this computer's projects another computer changed.
pub fn annotate(state: &AppState, rows: &mut [crate::index::ProjectRow]) {
    let outgoing = state
        .settings
        .lock()
        .map(|s| s.outgoing_requests.clone())
        .unwrap_or_default();
    let Ok(rt) = state.devices.runtime.lock() else {
        return;
    };
    devices::annotate(rows, &outgoing, &rt.others, &rt.refusals);
    for r in rows.iter_mut().filter(|r| r.device.is_none()) {
        if let Some(c) = rt.changes.get(&r.key) {
            if c.status == r.status {
                r.changed_by = Some(c.clone());
            }
        }
    }
}

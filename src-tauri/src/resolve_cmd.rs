//! Resolve starter projects: commands, the waiting list's worker and the Create hook.
//!
//! After Create, the project waits in `<config>/resolve-waiting.json` (with Nest's own copy of
//! the template's `.drp`, so later template edits don't matter) and is tried at once. Then it's
//! tried again every minute while Resolve runs, and soon after Resolve opens. One try at a time,
//! never while an update installs.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::{lock, AppState};
use crate::error::{AppError, AppResult};
use crate::plan::{Plan, PlannedResolve, MANIFEST_NAME};
use crate::resolve::{
    self, after, load_waiting, save_waiting, Libraries, Link, Next, Outcome, ResolveProject,
    ResolveStatus, WaitingEntry, WaitingList, TIMEOUT,
};
use crate::settings::ResolveLibrary;
use crate::settings_cmd::SettingsView;
use crate::staged::OnDisk;

pub const CHANGED_EVENT: &str = "resolve-changed";
/// How often the watcher looks for Resolve opening while something waits.
const WATCH_EVERY: Duration = Duration::from_secs(10);
/// Resolve answers a few seconds after its process appears.
const ANSWERS_AFTER: Duration = Duration::from_secs(10);

#[derive(Default)]
pub struct ResolveState {
    /// A try of the waiting list is running.
    pub busy: AtomicBool,
    /// A try was asked for while one ran: go round once more.
    again: AtomicBool,
    /// One change to the waiting list file at a time.
    file: Mutex<()>,
    /// How many wait (for the menu bar icon and the watcher).
    count: AtomicUsize,
    /// What the last try found (Settings shows the scripting hint from this).
    last: Mutex<Option<ResolveStatus>>,
}

/// Is a Resolve try running? (An update doesn't install by itself meanwhile.)
pub fn busy(app: &AppHandle) -> bool {
    app.try_state::<ResolveState>()
        .is_some_and(|r| r.busy.load(Ordering::SeqCst))
}

/// How many projects wait for Resolve (None = nothing waits).
pub fn waiting_count(app: &AppHandle) -> Option<usize> {
    let n = app
        .try_state::<ResolveState>()?
        .count
        .load(Ordering::SeqCst);
    (n > 0).then_some(n)
}

fn now() -> chrono::DateTime<chrono::FixedOffset> {
    chrono::Local::now().fixed_offset()
}

/// Real fuscript, or `NotInstalled`.
fn with_link<T>(f: impl FnOnce(&Link) -> T) -> Result<T, ResolveStatus> {
    let fuscript = resolve::find_fuscript().ok_or(ResolveStatus::NotInstalled)?;
    let run = |args: &[String]| resolve::run(&fuscript, args, TIMEOUT);
    let running = resolve::is_running;
    Ok(f(&Link {
        run: &run,
        running: &running,
    }))
}

/// Change the waiting list under its lock; returns the new list. A damaged file is left alone.
fn change_list(
    state: &AppState,
    rs: &ResolveState,
    change: impl FnOnce(&mut WaitingList),
) -> Result<WaitingList, String> {
    let _one = rs.file.lock().unwrap_or_else(|e| e.into_inner());
    let mut list = load_waiting(&state.config_dir)
        .map_err(|e| format!("the Resolve waiting list can't be read ({e})"))?;
    change(&mut list);
    save_waiting(&state.config_dir, &list).map_err(|e| e.to_string())?;
    rs.count.store(list.entries.len(), Ordering::SeqCst);
    Ok(list)
}

fn read_list(state: &AppState, rs: &ResolveState) -> WaitingList {
    let _one = rs.file.lock().unwrap_or_else(|e| e.into_inner());
    match load_waiting(&state.config_dir) {
        Ok(list) => {
            rs.count.store(list.entries.len(), Ordering::SeqCst);
            list
        }
        Err(e) => {
            log::warn!("the Resolve waiting list can't be read: {e}");
            WaitingList::default()
        }
    }
}

/// Create made a project with a Resolve project: keep a copy of the `.drp`, put it on the
/// waiting list, and try at once in the background. Problems here never fail the Create.
pub fn queue_after_create(
    app: &AppHandle,
    state: &AppState,
    plan: &Plan,
    r: &PlannedResolve,
    template_dir: &Path,
) {
    let Some(rs) = app.try_state::<ResolveState>() else {
        return;
    };
    let id = plan.manifest["id"].as_str().unwrap_or_default().to_string();
    let result = (|| -> Result<(), String> {
        let entry = resolve::queue(
            &state.config_dir,
            &id,
            &plan.job_code,
            &plan.root,
            r,
            template_dir,
            &now().to_rfc3339(),
        )?;
        change_list(state, &rs, |list| {
            list.entries.retain(|e| e.id != id);
            list.entries.push(entry);
        })?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            let _ = app.emit(CHANGED_EVENT, ());
            try_soon(app);
        }
        Err(e) => {
            log::warn!(
                "couldn't queue the Resolve project for {}: {e}",
                plan.job_code
            );
            notice(
                app,
                format!(
                    "{} was made, but Nest couldn't keep its Resolve project: {e}",
                    plan.job_code
                ),
            );
        }
    }
}

fn notice(app: &AppHandle, text: String) {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut n) = state.notice.lock() {
            *n = Some(text);
        }
    }
    let _ = app.emit("notice", ());
}

pub fn try_soon(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || try_all(&app));
}

/// Try every waiting project, one at a time. Quiet: outcomes go to the list, the project
/// files and a notice. A try asked for meanwhile goes round once more.
pub fn try_all(app: &AppHandle) {
    let (Some(state), Some(rs)) = (app.try_state::<AppState>(), app.try_state::<ResolveState>())
    else {
        return;
    };
    if rs.busy.swap(true, Ordering::SeqCst) {
        rs.again.store(true, Ordering::SeqCst);
        return;
    }
    // Checked after setting `busy`, so an update install sees a try that got here first.
    if crate::update::installing(app) {
        rs.busy.store(false, Ordering::SeqCst);
        return;
    }
    loop {
        rs.again.store(false, Ordering::SeqCst);
        if try_once(app, &state, &rs) {
            let _ = app.emit(CHANGED_EVENT, ());
            crate::tray::refresh_soon(app);
        }
        if !rs.again.load(Ordering::SeqCst) || crate::update::installing(app) {
            break;
        }
    }
    rs.busy.store(false, Ordering::SeqCst);
}

/// One pass. True if anything changed.
fn try_once(app: &AppHandle, state: &AppState, rs: &ResolveState) -> bool {
    let list = read_list(state, rs);
    if list.entries.is_empty() {
        return false;
    }
    let Some(fuscript) = resolve::find_fuscript() else {
        set_last(rs, ResolveStatus::NotInstalled);
        return false;
    };
    if !resolve::is_running() {
        set_last(rs, ResolveStatus::NotRunning);
        return false;
    }
    let run = |args: &[String]| resolve::run(&fuscript, args, TIMEOUT);
    let running = resolve::is_running;
    let link = Link {
        run: &run,
        running: &running,
    };
    let mut changed = false;
    for mut entry in list.entries {
        if crate::update::installing(app) {
            break;
        }
        let outcome = link.add(
            &entry.drp,
            &entry.name,
            &entry.folder,
            entry.library.as_ref(),
        );
        log::info!("Resolve project for {}: {}", entry.job_code, outcome.key());
        set_last(
            rs,
            match outcome {
                Outcome::Unreachable(s) => s,
                _ => ResolveStatus::Ready,
            },
        );
        let next = after(&mut entry, &outcome, now());
        changed |= settle(app, state, rs, entry, next);
        // Resolve went away or won't let Nest in: the rest wait too.
        if matches!(outcome, Outcome::Unreachable(_)) {
            break;
        }
    }
    changed
}

fn set_last(rs: &ResolveState, s: ResolveStatus) {
    if let Ok(mut last) = rs.last.lock() {
        *last = Some(s);
    }
}

/// Write what a try decided: the list, the project's file, Nest's copy, the notice.
fn settle(
    app: &AppHandle,
    state: &AppState,
    rs: &ResolveState,
    entry: WaitingEntry,
    next: Next,
) -> bool {
    let id = entry.id.clone();
    let (status, message) = match &next {
        Next::Keep => {
            // Removed meanwhile (Remove in Settings): not put back.
            if let Err(e) = change_list(state, rs, |list| {
                if let Some(e) = list.entries.iter_mut().find(|e| e.id == id) {
                    *e = entry.clone();
                }
            }) {
                log::warn!("{e}");
            }
            return true;
        }
        Next::Done { status, notice } => (*status, notice.clone()),
        Next::GiveUp { note } => ("failed", note.clone()),
    };
    let file = entry.project_dir.join(MANIFEST_NAME);
    if let Err(e) = resolve::update_manifest(&file, &id, status, &now().to_rfc3339()) {
        // The project's folder may be offline or moved: Resolve has the project either way.
        log::warn!(
            "couldn't note the Resolve status in {}: {e}",
            file.display()
        );
    }
    if let Err(e) = change_list(state, rs, |list| list.entries.retain(|e| e.id != id)) {
        log::warn!("{e}");
    }
    remove_copy(&entry.drp);
    notice(app, message);
    true
}

/// Nest's own copy of a `.drp` (never anything else).
fn remove_copy(path: &Path) {
    if let Err(e) = fs::remove_file(path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            log::warn!("couldn't remove {}: {e}", path.display());
        }
    }
}

/// Every minute (Devices' timer): try if something waits and Resolve runs.
pub fn tick(app: &AppHandle) {
    if waiting_count(app).is_some() && resolve::is_running() {
        try_all(app);
    }
}

/// Launch: read the waiting list, then watch for Resolve opening while anything waits.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        if let (Some(state), Some(rs)) =
            (app.try_state::<AppState>(), app.try_state::<ResolveState>())
        {
            read_list(&state, &rs);
        }
        let mut was_running = false;
        loop {
            if waiting_count(&app).is_some() {
                let running = resolve::is_running();
                if running && !was_running {
                    std::thread::sleep(ANSWERS_AFTER);
                    try_all(&app);
                }
                was_running = running;
            } else {
                was_running = false;
            }
            std::thread::sleep(WATCH_EVERY);
        }
    });
}

// ───────────────────────── Commands ─────────────────────────

fn problem(e: resolve::Problem) -> AppError {
    AppError::new(e.message())
}

/// Is Resolve there, open, and letting Nest in?
#[tauri::command(async)]
pub fn resolve_status() -> ResolveStatus {
    with_link(|l| l.status()).unwrap_or_else(|s| s)
}

/// Resolve's libraries and the one open now (Settings).
#[tauri::command(async)]
pub fn resolve_libraries() -> AppResult<Libraries> {
    with_link(|l| l.libraries())
        .map_err(|s| AppError::new(s.message()))?
        .map_err(problem)
}

/// The projects in Resolve's open library ("Choose from Resolve…").
#[tauri::command(async)]
pub fn resolve_projects() -> AppResult<Vec<ResolveProject>> {
    with_link(|l| l.projects())
        .map_err(|s| AppError::new(s.message()))?
        .map_err(problem)
}

/// The template editor's "Choose from Resolve…": export `project_name` (in `folder`, "" = the
/// top level) from Resolve's open library into the editor session `session_id`, staged like a
/// dropped file. Returns the template path to put in `resolve.drp`.
#[tauri::command(async)]
pub fn resolve_export_to_editor(
    state: State<'_, AppState>,
    session_id: String,
    project_name: String,
    folder: String,
) -> AppResult<String> {
    let closed = || AppError::new("The editor was closed. Open the template again.");
    if !lock(&state.editor_sessions)?.contains_key(&session_id) {
        return Err(closed());
    }
    let file_name = format!(
        "{}.drp",
        crate::names::sanitize_segment(&project_name).map_err(|e| AppError::new(e.to_string()))?
    );
    // Nest's own folder for ready-made editor files (emptied at the next start).
    let dir = state
        .config_dir
        .join("extras")
        .join(uuid::Uuid::new_v4().simple().to_string());
    fs::create_dir_all(&dir)
        .map_err(|e| AppError::new(format!("Couldn't make room for the export: {e}")))?;
    let dest = dir.join(&file_name);
    with_link(|l| l.export(&project_name, &folder, &dest))
        .map_err(|s| AppError::new(s.message()))?
        .map_err(problem)?;
    let size = fs::metadata(&dest)
        .map(|m| m.len())
        .map_err(|e| AppError::new(format!("The export can't be read: {e}")))?;
    let mut sessions = lock(&state.editor_sessions)?;
    let session = sessions.get_mut(&session_id).ok_or_else(closed)?;
    Ok(session.add_resolve_drp(
        &file_name,
        OnDisk {
            base: dir,
            rel: file_name.clone(),
            size,
        },
    ))
}

/// The waiting list (Settings / main window).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitingView {
    pub entries: Vec<WaitingEntry>,
    /// What the last try found; `scriptingOff` = show the hint.
    pub last_status: Option<ResolveStatus>,
    /// Plain words for `last_status` (None when ready or unknown).
    pub hint: Option<String>,
}

fn waiting_view(state: &AppState, rs: &ResolveState) -> WaitingView {
    let last = rs.last.lock().ok().and_then(|l| *l);
    WaitingView {
        entries: read_list(state, rs).entries,
        last_status: last,
        hint: last
            .filter(|s| *s != ResolveStatus::Ready)
            .map(|s| s.message().to_string()),
    }
}

#[tauri::command(async)]
pub fn resolve_waiting_list(
    state: State<'_, AppState>,
    rs: State<'_, ResolveState>,
) -> WaitingView {
    waiting_view(&state, &rs)
}

/// "Try now": try the waiting projects (waits for a try already running to finish).
#[tauri::command(async)]
pub fn resolve_try_now(
    app: AppHandle,
    state: State<'_, AppState>,
    rs: State<'_, ResolveState>,
) -> AppResult<WaitingView> {
    if crate::update::installing(&app) {
        return Err(AppError::new(crate::update::UPDATING));
    }
    // A pass may be running: wait for it (it runs again for us), then make sure one ran.
    try_all(&app);
    for _ in 0..(TIMEOUT.as_secs() * 10 * 4) {
        if !rs.busy.load(Ordering::SeqCst) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if waiting_count(&app).is_some() && !resolve::is_running() {
        set_last(
            &rs,
            if resolve::find_fuscript().is_some() {
                ResolveStatus::NotRunning
            } else {
                ResolveStatus::NotInstalled
            },
        );
    }
    Ok(waiting_view(&state, &rs))
}

/// Take a project off the waiting list (Resolve won't get it). Its `.drp` copy goes too.
#[tauri::command(async)]
pub fn resolve_remove_waiting(
    app: AppHandle,
    state: State<'_, AppState>,
    rs: State<'_, ResolveState>,
    id: String,
) -> AppResult<WaitingView> {
    let mut removed: Option<WaitingEntry> = None;
    change_list(&state, &rs, |list| {
        if let Some(i) = list.entries.iter().position(|e| e.id == id) {
            removed = Some(list.entries.remove(i));
        }
    })
    .map_err(AppError::new)?;
    let entry = removed.ok_or_else(|| AppError::new("That project isn't waiting any more."))?;
    remove_copy(&entry.drp);
    let file = entry.project_dir.join(MANIFEST_NAME);
    if let Err(e) = resolve::update_manifest(&file, &entry.id, "skipped", &now().to_rfc3339()) {
        log::info!(
            "couldn't note the skipped Resolve project in {}: {e}",
            file.display()
        );
    }
    let _ = app.emit(CHANGED_EVENT, ());
    crate::tray::refresh_soon(&app);
    Ok(waiting_view(&state, &rs))
}

/// Settings: the library Nest adds Resolve projects to on this computer (None = the one open
/// in Resolve).
#[tauri::command(async)]
pub fn resolve_set_library(
    app: AppHandle,
    library: Option<ResolveLibrary>,
) -> AppResult<SettingsView> {
    let library = library.filter(|l| !l.db_name.trim().is_empty());
    crate::settings_cmd::edit_settings(&app, |s| {
        s.resolve_library = library;
        Ok(())
    })
}

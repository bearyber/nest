//! Resolve starter projects: commands, the waiting list's worker and the Create hook.
//!
//! After Create, the project waits in `<config>/resolve-waiting.json` (with Nest's own copy of
//! the template's `.drp`, so later template edits don't matter) and is tried at once. Then it's
//! tried again every minute while Resolve runs, and soon after Resolve opens. One try at a time,
//! never while an update installs, always on a thread of its own (Devices never waits on it).

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
    self, after, due, load_waiting, recover, remove_copy, save_waiting, Back, Libraries, Link,
    Next, Outcome, Recover, ResolveProject, ResolveStatus, RunError, Switched, WaitingEntry,
    WaitingList, SWITCH_TIMEOUT, UNREADABLE,
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
    /// "Try now": the next pass tries every entry (no backing off).
    force: AtomicBool,
    /// One change to the waiting list file at a time.
    file: Mutex<()>,
    /// How many wait (for the menu bar icon and the watcher).
    count: AtomicUsize,
    /// Nest may have left Resolve in another library (the list's `switched`).
    switched: AtomicBool,
    /// What the last try found (Settings shows the scripting hint from this).
    last: Mutex<Option<ResolveStatus>>,
}

/// Clears `busy` however the pass ends (even by a panic).
struct BusyGuard<'a>(&'a AtomicBool);

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
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

/// Anything for the worker to do: projects waiting, or a library to switch Resolve back from.
fn has_work(app: &AppHandle) -> bool {
    waiting_count(app).is_some()
        || app
            .try_state::<ResolveState>()
            .is_some_and(|r| r.switched.load(Ordering::SeqCst))
}

fn now() -> chrono::DateTime<chrono::FixedOffset> {
    chrono::Local::now().fixed_offset()
}

/// Real fuscript, or `NotInstalled`.
fn with_link<T>(config_dir: &Path, f: impl FnOnce(&Link) -> T) -> Result<T, ResolveStatus> {
    let fuscript = resolve::find_fuscript().ok_or(ResolveStatus::NotInstalled)?;
    let dir = resolve::script_dir_here(config_dir);
    let run = |args: &[String], limit: Duration| -> Result<String, RunError> {
        resolve::run(&fuscript, &dir, args, limit)
    };
    let running = resolve::running_for;
    Ok(f(&Link {
        run: &run,
        running: &running,
    }))
}

fn remember(rs: &ResolveState, list: &WaitingList) {
    rs.count.store(list.entries.len(), Ordering::SeqCst);
    rs.switched.store(list.switched.is_some(), Ordering::SeqCst);
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
    remember(rs, &list);
    Ok(list)
}

/// The waiting list, or why it can't be read (never replaced by an empty one).
fn read_list(state: &AppState, rs: &ResolveState) -> Result<WaitingList, String> {
    let _one = rs.file.lock().unwrap_or_else(|e| e.into_inner());
    match load_waiting(&state.config_dir) {
        Ok(list) => {
            remember(rs, &list);
            Ok(list)
        }
        Err(e) => {
            log::warn!("the Resolve waiting list can't be read: {e}");
            Err(e)
        }
    }
}

/// Create made a project with a Resolve project: keep a copy of the `.drp`, put it on the
/// waiting list, and try at once in the background. Problems here never fail the Create: the
/// copy is removed again, and the project's file says the Resolve project failed, and why.
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
        let queued = change_list(state, &rs, |list| {
            list.entries.retain(|e| e.id != id);
            list.entries.push(entry);
        });
        if queued.is_err() {
            remove_copy(&state.config_dir, &id);
        }
        queued.map(|_| ())
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
            let note = format!("Nest couldn't keep its Resolve project: {e}");
            let file = plan.root.join(MANIFEST_NAME);
            if let Err(e) =
                resolve::update_manifest(&file, &id, "failed", &now().to_rfc3339(), Some(&note))
            {
                log::warn!(
                    "couldn't note the Resolve status in {}: {e}",
                    file.display()
                );
            }
            notice(app, format!("{} was made, but {note}", plan.job_code));
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

/// A pass on a thread of its own.
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
    loop {
        if rs.busy.swap(true, Ordering::SeqCst) {
            rs.again.store(true, Ordering::SeqCst);
            return;
        }
        {
            let _busy = BusyGuard(&rs.busy);
            // Checked after setting `busy`, so an update install sees a try that got here first.
            if crate::update::installing(app) {
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
        }
        // A "try now" that came in just before `busy` cleared would otherwise be lost.
        if !rs.again.swap(false, Ordering::SeqCst) || crate::update::installing(app) {
            return;
        }
    }
}

/// One pass. True if anything changed.
fn try_once(app: &AppHandle, state: &AppState, rs: &ResolveState) -> bool {
    let force = rs.force.swap(false, Ordering::SeqCst);
    // A damaged list: nothing is tried (and it's never written over).
    let Ok(list) = read_list(state, rs) else {
        return false;
    };
    if list.entries.is_empty() && list.switched.is_none() {
        return false;
    }
    let Some(fuscript) = resolve::find_fuscript() else {
        set_last(rs, ResolveStatus::NotInstalled);
        return false;
    };
    if resolve::running_for().is_none() {
        set_last(rs, ResolveStatus::NotRunning);
        return false;
    }
    let dir = resolve::script_dir_here(&state.config_dir);
    let run = |args: &[String], limit: Duration| -> Result<String, RunError> {
        resolve::run(&fuscript, &dir, args, limit)
    };
    let running = resolve::running_for;
    let link = Link {
        run: &run,
        running: &running,
    };
    let mut changed = false;

    // Nest left Resolve in another library last time: put it back first, and add nothing
    // until that's settled.
    if let Some(rec) = &list.switched {
        changed = true;
        if !switch_back(app, state, rs, &link, rec) {
            return changed;
        }
    }

    let now_ = now();
    for mut entry in list.entries {
        if crate::update::installing(app) {
            break;
        }
        if !force && !due(&entry, now_) {
            continue;
        }
        let target = entry.library.clone();
        let mut before_switch = |from: &ResolveLibrary| {
            // Written before the switch: a run that never comes back is undone next time.
            let rec = Switched {
                from: from.clone(),
                to: target.clone().unwrap_or_default(),
                at: now().to_rfc3339(),
                back_failed: false,
            };
            if let Err(e) = change_list(state, rs, |list| list.switched = Some(rec)) {
                log::warn!("{e}");
            }
        };
        let report = link.add(
            &entry.drp,
            &entry.name,
            &entry.folder,
            entry.library.as_ref(),
            &mut before_switch,
        );
        let outcome = report.outcome.clone();
        log::info!(
            "Resolve project for {}: {} (switch back: {:?})",
            entry.job_code,
            outcome.key(),
            report.back
        );
        set_last(
            rs,
            match outcome {
                Outcome::Unreachable(s) => s,
                _ => ResolveStatus::Ready,
            },
        );
        let next = after(&mut entry, &outcome, now());
        changed |= settle(app, state, rs, entry, next);
        // After `settle`, so this notice is the one that shows.
        after_switch(app, state, rs, &report);
        // Resolve went away, won't let Nest in, or is left elsewhere: the rest wait too.
        if resolve::stops_pass(&report) {
            break;
        }
    }
    changed
}

/// Write down how the switch back after an add went.
fn after_switch(app: &AppHandle, state: &AppState, rs: &ResolveState, report: &resolve::AddReport) {
    match report.back {
        Back::Done | Back::NoSwitch => {
            if rs.switched.load(Ordering::SeqCst) {
                if let Err(e) = change_list(state, rs, |list| list.switched = None) {
                    log::warn!("{e}");
                }
            }
        }
        Back::Failed => {
            if let Err(e) = change_list(state, rs, |list| {
                if let Some(rec) = &mut list.switched {
                    rec.back_failed = true;
                }
            }) {
                log::warn!("{e}");
            }
            if let Some(from) = &report.from {
                notice(app, resolve::switch_back_message(from));
            }
        }
        // Stopped mid-run: the record stays, and the next pass looks.
        Back::Unknown => {}
    }
}

/// Put Resolve back in the library Nest switched from, if it's still sitting in Nest's library
/// with nothing real open. True = settled (the pass may go on).
fn switch_back(
    app: &AppHandle,
    state: &AppState,
    rs: &ResolveState,
    link: &Link,
    rec: &Switched,
) -> bool {
    let open = match link.state() {
        Ok(open) => open,
        Err(resolve::Problem::Status(s)) => {
            set_last(rs, s);
            return false;
        }
        Err(e) => {
            log::info!("Resolve state: {}", e.message());
            return false;
        }
    };
    let forget = |state: &AppState| {
        if let Err(e) = change_list(state, rs, |list| list.switched = None) {
            log::warn!("{e}");
        }
    };
    match recover(rec, &open.current, open.contents) {
        Recover::Forget => {
            forget(state);
            true
        }
        Recover::SwitchBack => match link.switch_to(&rec.from) {
            Ok(true) => {
                log::info!("switched Resolve back to {}", rec.from.db_name);
                forget(state);
                if rec.back_failed {
                    notice(
                        app,
                        format!("Nest switched Resolve back to {}.", rec.from.db_name),
                    );
                }
                true
            }
            other => {
                if let Err(e) = &other {
                    log::info!("switching Resolve back: {}", e.message());
                }
                if !rec.back_failed {
                    if let Err(e) = change_list(state, rs, |list| {
                        if let Some(r) = &mut list.switched {
                            r.back_failed = true;
                        }
                    }) {
                        log::warn!("{e}");
                    }
                    notice(app, rec.message());
                }
                false
            }
        },
    }
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
    let note = matches!(next, Next::GiveUp { .. }).then_some(message.as_str());
    let file = entry.project_dir.join(MANIFEST_NAME);
    if let Err(e) = resolve::update_manifest(&file, &id, status, &now().to_rfc3339(), note) {
        // The project's folder may be offline or moved: Resolve has the project either way.
        log::warn!(
            "couldn't note the Resolve status in {}: {e}",
            file.display()
        );
    }
    if let Err(e) = change_list(state, rs, |list| list.entries.retain(|e| e.id != id)) {
        log::warn!("{e}");
    }
    remove_copy(&state.config_dir, &id);
    notice(app, message);
    true
}

/// Every minute (Devices' timer): try if something waits and Resolve runs. On a thread of its
/// own, so Devices never waits on Resolve.
pub fn tick(app: &AppHandle) {
    if !has_work(app) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        if resolve::running_for().is_some() {
            try_all(&app);
        }
    });
}

/// Launch: read the waiting list, then watch for Resolve opening while anything waits.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        if let (Some(state), Some(rs)) =
            (app.try_state::<AppState>(), app.try_state::<ResolveState>())
        {
            let _ = read_list(&state, &rs);
        }
        let mut was_running = false;
        loop {
            if has_work(&app) {
                let running = resolve::running_for().is_some();
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
pub fn resolve_status(state: State<'_, AppState>) -> ResolveStatus {
    with_link(&state.config_dir, |l| l.status()).unwrap_or_else(|s| s)
}

/// Resolve's libraries and the one open now (Settings).
#[tauri::command(async)]
pub fn resolve_libraries(state: State<'_, AppState>) -> AppResult<Libraries> {
    with_link(&state.config_dir, |l| l.libraries())
        .map_err(|s| AppError::new(s.message()))?
        .map_err(problem)
}

/// The projects in Resolve's open library ("Choose from Resolve…").
#[tauri::command(async)]
pub fn resolve_projects(state: State<'_, AppState>) -> AppResult<Vec<ResolveProject>> {
    with_link(&state.config_dir, |l| l.projects())
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
    with_link(&state.config_dir, |l| {
        l.export(&project_name, &folder, &dest)
    })
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
    /// The waiting list file is damaged: "The waiting list can't be read." (`entries` is then
    /// empty because nothing could be read, not because nothing waits).
    pub problem: Option<String>,
    /// Nest couldn't switch Resolve back to the library it was in: plain words saying which,
    /// and what to do.
    pub switch_back: Option<String>,
}

fn waiting_view(state: &AppState, rs: &ResolveState) -> WaitingView {
    let last = rs.last.lock().ok().and_then(|l| *l);
    view_of(read_list(state, rs), last)
}

/// The view of a waiting list as read (or not) and the last status.
fn view_of(list: Result<WaitingList, String>, last: Option<ResolveStatus>) -> WaitingView {
    let hint = last
        .filter(|s| *s != ResolveStatus::Ready)
        .map(|s| s.message().to_string());
    match list {
        Ok(list) => WaitingView {
            switch_back: list
                .switched
                .as_ref()
                .filter(|s| s.back_failed)
                .map(|s| s.message()),
            entries: list.entries,
            last_status: last,
            hint,
            problem: None,
        },
        Err(_) => WaitingView {
            entries: vec![],
            last_status: last,
            hint,
            problem: Some(UNREADABLE.to_string()),
            switch_back: None,
        },
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
    rs.force.store(true, Ordering::SeqCst);
    try_all(&app);
    for _ in 0..(SWITCH_TIMEOUT.as_secs() * 10 * 5) {
        if !rs.busy.load(Ordering::SeqCst) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if has_work(&app) && resolve::running_for().is_none() {
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
    remove_copy(&state.config_dir, &entry.id);
    let file = entry.project_dir.join(MANIFEST_NAME);
    if let Err(e) = resolve::update_manifest(&file, &entry.id, "skipped", &now().to_rfc3339(), None)
    {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_damaged_waiting_list_says_so_instead_of_looking_empty() {
        let v = view_of(Err("bad json".into()), Some(ResolveStatus::Ready));
        assert_eq!(
            v.problem.as_deref(),
            Some("The waiting list can't be read.")
        );
        assert!(v.entries.is_empty());
        let v = view_of(Ok(WaitingList::default()), None);
        assert_eq!(v.problem, None);
        assert_eq!(v.switch_back, None);
    }

    #[test]
    fn a_failed_switch_back_shows_in_the_waiting_view() {
        let lib = |n: &str| ResolveLibrary {
            db_type: "Disk".into(),
            db_name: n.into(),
            ip_address: None,
        };
        let mut list = WaitingList {
            switched: Some(Switched {
                from: lib("Home"),
                to: lib("Work"),
                at: "t".into(),
                back_failed: false,
            }),
            ..Default::default()
        };
        // Not tried yet (a run in progress or stopped): nothing to say.
        assert_eq!(view_of(Ok(list.clone()), None).switch_back, None);
        list.switched.as_mut().unwrap().back_failed = true;
        assert_eq!(
            view_of(Ok(list), Some(ResolveStatus::ScriptingOff))
                .switch_back
                .as_deref(),
            Some("Nest couldn't switch Resolve back to Home. Choose it in Resolve's Project Libraries.")
        );
    }

    #[test]
    fn the_busy_flag_clears_however_a_pass_ends() {
        let busy = AtomicBool::new(true);
        let caught = std::panic::catch_unwind(|| {
            let _g = BusyGuard(&busy);
            panic!("boom");
        });
        assert!(caught.is_err());
        assert!(!busy.load(Ordering::SeqCst));
    }
}

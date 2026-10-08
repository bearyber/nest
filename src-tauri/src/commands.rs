//! Tauri command handlers. Keep thin: call into modules.
//! The UI only ever sends inputs (template id + values); Rust always builds the Plan itself.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use chrono::{Datelike, Local};
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;

use crate::apply::apply_plan;
use crate::archive;
use crate::error::{io_reason, AppError, AppResult};
use crate::index::{
    self, now_ms, scan_root, FoundProject, Index, ProjectRow, RescanSummary, RootScan, RootState,
};
use crate::manifest;
use crate::plan::{self, ClientValue, Date, Issue, Level, Plan, PlanContext, Values};
use crate::settings::{self, Settings};
use crate::sizes::Sizes;
use crate::template::{LoadedTemplate, Template};
use crate::template_io::{load_installed, Installed, BUILT_IN_IDS};
use crate::window::Material;
use tauri_plugin_clipboard_manager::ClipboardExt;

/// Tries at finding a free job code when the folder appears between plan and create.
const CREATE_ATTEMPTS: usize = 5;

pub struct AppState {
    pub config_dir: PathBuf,
    /// Installed templates (bundled + yours). Reloaded after import / duplicate / delete.
    pub templates: Mutex<Vec<Installed>>,
    /// Your templates (`<config>/templates`) and the bundled ones (app resources).
    pub user_templates: PathBuf,
    pub bundled_templates: PathBuf,
    /// No settings file existed at launch: show first run until it's completed.
    pub first_run: AtomicBool,
    pub settings: Mutex<Settings>,
    /// The settings file exists but is damaged: never save over it.
    pub settings_error: Option<String>,
    /// Last scan of the jobs root (refreshed when the sheet opens and on Create).
    pub scan: Mutex<RootScan>,
    /// A Create is running: a second one (e.g. a held Cmd/Ctrl+Enter) is refused.
    pub creating: AtomicBool,
    /// The project index (SQLite cache). The folder walk never holds this lock.
    pub index: Mutex<Index>,
    /// A rescan is running: another request just waits for that one's result.
    pub scanning: AtomicBool,
    /// A one-off note for the main window (e.g. "Tidied 3 older template copies…").
    pub notice: Mutex<Option<String>>,
    /// Devices: other computers' lists and this computer's last saved one.
    pub devices: crate::devices_cmd::Devices,
    /// One project status change at a time (a click here, or a request from another computer).
    pub status_lock: Mutex<()>,
}

/// Clears an in-progress flag however the command exits.
struct CreatingGuard<'a>(&'a AtomicBool);

impl Drop for CreatingGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> AppResult<MutexGuard<'_, T>> {
    m.lock()
        .map_err(|_| AppError::new("Nest hit an internal error. Please restart it."))
}

/// Templates offered in New Project: the newest of each id, minus the ones you hid.
pub(crate) fn offered_templates(
    state: &AppState,
    settings: &Settings,
) -> AppResult<Vec<LoadedTemplate>> {
    Ok(lock(&state.templates)?
        .iter()
        .filter(|t| !t.superseded && !settings.hidden_templates.contains(&t.loaded.template.id))
        .map(|t| t.loaded.clone())
        .collect())
}

/// Reload the installed templates from disk (after import, duplicate or delete).
pub(crate) fn reload_templates(state: &AppState) -> AppResult<()> {
    *lock(&state.templates)? = load_installed(
        &state.user_templates,
        &state.bundled_templates,
        BUILT_IN_IDS,
    );
    Ok(())
}

fn jobs_root(settings: &Settings) -> AppResult<PathBuf> {
    settings
        .jobs_roots
        .first()
        .cloned()
        .ok_or_else(|| AppError::new("No jobs folder is set up yet"))
}

/// Whether the native window material (Mica / vibrancy) is active.
/// `false` → the UI must paint a solid background.
#[tauri::command]
pub fn material_applied(
    window: tauri::WebviewWindow,
    material: State<'_, Material>,
    settings_material: State<'_, crate::window::SettingsMaterial>,
) -> bool {
    // Each window has its own material: Settings asks about Settings, not the main window.
    if window.label() == crate::settings_cmd::SETTINGS_WINDOW {
        settings_material.0.load(Ordering::SeqCst)
    } else {
        material.applied
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewProjectContext {
    pub templates: Vec<Template>,
    pub spaces: Vec<String>,
    /// Spaces where the Client field is hidden (projects use `own_code`).
    pub no_client_spaces: Vec<String>,
    pub default_space: String,
    pub last_template: Option<String>,
    /// `None` until a jobs folder is chosen.
    pub jobs_root: Option<PathBuf>,
    pub root_missing: bool,
    pub clients: Vec<ClientValue>,
    pub settings_error: Option<String>,
}

/// Everything the New Project sheet needs. Also refreshes the jobs root scan.
// `async`: run off the UI thread, so a slow NAS or a big starter file never freezes the window.
#[tauri::command(async)]
pub fn new_project_context(state: State<'_, AppState>) -> AppResult<NewProjectContext> {
    let settings = lock(&state.settings)?.clone();
    let root = settings.jobs_roots.first().cloned();
    let scan = root.as_deref().map(scan_root);
    let root_missing = !matches!(scan, Some(Ok(_)));
    let scan = scan.and_then(Result::ok);
    let scan = scan.unwrap_or_default();
    let clients = scan.clients.clone();
    *lock(&state.scan)? = scan;
    Ok(NewProjectContext {
        templates: offered_templates(&state, &settings)?
            .into_iter()
            .map(|t| t.template)
            .collect(),
        spaces: settings.spaces,
        no_client_spaces: settings.no_client_spaces,
        default_space: settings.default_space,
        last_template: settings.last_template,
        jobs_root: root,
        root_missing,
        clients,
        settings_error: state.settings_error.clone(),
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanRequest {
    pub template_id: String,
    pub values: Values,
    pub space: String,
    pub code_override: Option<String>,
    /// "No client (personal or passion project)" ticked in New Project.
    #[serde(default)]
    pub no_client: bool,
}

/// Live preview for the sheet. Uses the cached scan, so it's cheap to call per keystroke.
#[tauri::command(async)]
pub fn plan_project(state: State<'_, AppState>, request: PlanRequest) -> AppResult<Plan> {
    let settings = lock(&state.settings)?.clone();
    let scan = lock(&state.scan)?.clone();
    Ok(build_plan(&state, &settings, &scan, &request)?.0)
}

fn build_plan(
    state: &AppState,
    settings: &Settings,
    scan: &RootScan,
    request: &PlanRequest,
) -> AppResult<(Plan, LoadedTemplate)> {
    let loaded = lock(&state.templates)?
        .iter()
        .find(|t| !t.superseded && t.loaded.template.id == request.template_id)
        .map(|t| t.loaded.clone())
        .ok_or_else(|| AppError::new("That template isn't installed"))?;
    let loaded = &loaded;
    // No jobs folder yet still previews; Create stays blocked by the issue below.
    let root = settings.jobs_roots.first().cloned().unwrap_or_default();
    let now = Local::now();
    let ctx = PlanContext {
        jobs_root: root.clone(),
        id: uuid::Uuid::new_v4().to_string(),
        created_at: now.format("%Y-%m-%dT%H:%M:%S%:z").to_string(),
        date: Date {
            year: now.year().clamp(0, 9999) as u16,
            month: now.month() as u8,
            day: now.day() as u8,
        },
        space: request.space.clone(),
        code_pattern: settings.code_pattern.clone(),
        marker: settings.local_marker.clone(),
        // Spec §5: codes from the index (incl. offline folders) and from disk.
        existing_codes: {
            let mut codes = scan.codes.clone();
            if let Ok(idx) = state.index.lock() {
                codes.extend(idx.codes().unwrap_or_default());
            }
            // Devices: codes your other computers use, so a code never repeats across them.
            codes.extend(crate::devices_cmd::remote_codes(&state.devices));
            codes
        },
        existing_folders: scan.folders.clone(),
        code_override: request.code_override.clone(),
        own_code: (request.no_client || settings.no_client_spaces.contains(&request.space))
            .then(|| settings.own_code.clone()),
    };
    let mut plan = plan::plan_project(loaded, &request.values, &ctx);
    if !settings.spaces.contains(&request.space) {
        plan.issues.push(Issue {
            level: Level::Error,
            field: Some("space".into()),
            message: format!("Space \"{}\" doesn't exist", request.space),
        });
    }
    let root_problem = if root.as_os_str().is_empty() {
        Some("Choose a jobs folder: Change… next to Location".to_string())
    } else if !root.is_dir() {
        Some(format!("Jobs folder not found: {}", root.display()))
    } else {
        None
    };
    if let Some(message) = root_problem {
        plan.issues.push(Issue {
            level: Level::Error,
            field: None,
            message,
        });
    }
    Ok((plan, loaded.clone()))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Created {
    /// The new project's key in the list (its manifest id).
    pub key: String,
    pub job_code: String,
    pub title: String,
    pub root: PathBuf,
    /// The code changed from the preview because it was taken meanwhile.
    pub bumped: bool,
}

/// Re-scan, re-plan and create. If the folder appears in the meantime, bump and retry.
#[tauri::command(async)]
pub fn create_project(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: PlanRequest,
) -> AppResult<Created> {
    if state.creating.swap(true, Ordering::SeqCst) {
        return Err(AppError::new("A project is already being created"));
    }
    let _guard = CreatingGuard(&state.creating);
    let settings = lock(&state.settings)?.clone();
    let previewed = {
        let scan = lock(&state.scan)?.clone();
        build_plan(&state, &settings, &scan, &request)?.0.job_code
    };
    let root = jobs_root(&settings)?;
    for _ in 0..CREATE_ATTEMPTS {
        let scan = scan_root(&root).map_err(|e| {
            AppError::new(format!(
                "Jobs folder not found: {} ({})",
                root.display(),
                io_reason(&e)
            ))
        })?;
        *lock(&state.scan)? = scan.clone();
        let (plan, loaded) = build_plan(&state, &settings, &scan, &request)?;
        if let Some(problem) = plan.issues.iter().find(|i| i.level == Level::Error) {
            return Err(AppError::new(problem.message.clone()));
        }
        match apply_plan(&plan, &loaded.dir) {
            Ok(()) => {
                remember_template(&state, &request.template_id);
                // Straight into the list; the next rescan confirms it from disk.
                let key = plan.manifest["id"].as_str().unwrap_or_default().to_string();
                let found = FoundProject {
                    dir: plan.root.clone(),
                    root: root.clone(),
                    manifest: Ok(plan.manifest.clone()),
                };
                if let Err(e) = lock(&state.index)?.record(&[found], now_ms()) {
                    log::warn!("couldn't add the new project to the index: {e}");
                }
                crate::devices_cmd::sync_soon(&app);
                return Ok(Created {
                    key,
                    bumped: plan.job_code != previewed,
                    job_code: plan.job_code,
                    title: plan.title,
                    root: plan.root,
                });
            }
            // Someone made that folder meanwhile: rescan and take the next code.
            Err(e) if e.root_exists && request.code_override.is_none() => continue,
            Err(e) => return Err(AppError::new(e.message)),
        }
    }
    Err(AppError::new(
        "Couldn't find a free job code. Rescan the jobs folder and try again.",
    ))
}

fn remember_template(state: &AppState, template_id: &str) {
    let Ok(mut settings) = state.settings.lock() else {
        return;
    };
    settings.last_template = Some(template_id.to_string());
    if state.settings_error.is_none() {
        if let Err(e) = settings::save(&state.config_dir, &settings) {
            log::warn!("{e}");
        }
    }
}

/// Pick the jobs folder with the native folder picker and save it (M4 adds several roots).
/// Returns the chosen folder, or `None` if the picker was cancelled. Never creates folders:
/// the OS picker's own "New folder" button is how a new one gets made.
#[tauri::command(async)]
pub fn choose_jobs_root(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Option<PathBuf>> {
    if let Some(e) = &state.settings_error {
        return Err(AppError::new(format!(
            "Can't change the jobs folder while the settings file is damaged. {e}"
        )));
    }
    let current = lock(&state.settings)?.jobs_roots.first().cloned();
    let mut dialog = app.dialog().file().set_title("Choose your jobs folder");
    if let Some(dir) = current.filter(|d| d.is_dir()) {
        dialog = dialog.set_directory(dir);
    }
    let Some(picked) = dialog.blocking_pick_folder() else {
        return Ok(None);
    };
    let path = picked
        .into_path()
        .map_err(|e| AppError::new(format!("That folder can't be used: {e}")))?;
    if !path.is_dir() {
        return Err(AppError::new(format!(
            "Jobs folder not found: {}",
            path.display()
        )));
    }
    let mut settings = lock(&state.settings)?;
    settings.jobs_roots.retain(|r| r != &path);
    settings.jobs_roots.insert(0, path.clone());
    settings::save(&state.config_dir, &settings)?;
    Ok(Some(path))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectList {
    pub projects: Vec<ProjectRow>,
    pub roots: Vec<RootState>,
    /// When the last rescan finished (ms since 1970), or `None` before the first one.
    pub scanned_at: Option<i64>,
}

/// Everything in the index, instantly (no disk access). Call `rescan_projects` to refresh.
#[tauri::command(async)]
pub fn list_projects(state: State<'_, AppState>) -> AppResult<ProjectList> {
    let idx = lock(&state.index)?;
    let err = |e: rusqlite::Error| AppError::new(format!("Couldn't read the project list: {e}"));
    let mut projects = idx.list().map_err(err)?;
    // Devices: projects that live on your other computers, greyed and read-only.
    let others = lock(&state.devices.runtime)?.others.clone();
    crate::devices::merge(&mut projects, &others);
    crate::devices_cmd::annotate(&state, &mut projects);
    // Show each template by its name ("Grading: PV"), not its id ("grading-performance").
    let names: HashMap<String, String> = lock(&state.templates)?
        .iter()
        .filter(|t| !t.superseded || t.replaced)
        .map(|t| (t.loaded.template.id.clone(), t.loaded.template.name.clone()))
        .collect();
    for p in &mut projects {
        p.template_name = names
            .get(&p.template_id)
            .cloned()
            .unwrap_or_else(|| readable_id(&p.template_id));
    }
    // Archive (M5): "ready to archive" = done for longer than Settings says, here, not dismissed.
    let (archive_on, after_days, dismissed) = {
        let s = lock(&state.settings)?;
        (
            s.archive_folder.is_some(),
            s.archive_after_days,
            s.archive_dismissed.clone(),
        )
    };
    if let (true, Some(days)) = (archive_on, after_days) {
        let cutoff = index::now_ms() - i64::from(days) * 86_400_000;
        for p in &mut projects {
            p.ready_to_archive = p.status == "done"
                && !p.archived
                && p.device.is_none()
                && !p.offline
                && p.error.is_none()
                && p.done_at.is_some_and(|t| t <= cutoff)
                && !p.id.as_ref().is_some_and(|id| dismissed.contains(id));
        }
    }
    Ok(ProjectList {
        projects,
        roots: idx.roots().map_err(err)?,
        scanned_at: idx.last_scan().map_err(err)?,
    })
}

/// Walk the jobs folders and update the index (spec §7). One scan at a time.
#[tauri::command(async)]
pub fn rescan_projects(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<RescanSummary> {
    if state.scanning.swap(true, Ordering::SeqCst) {
        return Err(AppError::new("A rescan is already running"));
    }
    let _guard = CreatingGuard(&state.scanning);
    let (roots, archive) = {
        let s = lock(&state.settings)?;
        (s.jobs_roots.clone(), s.archive_folder.clone())
    };
    let summary = index::rescan_with_archive(&state.index, &roots, archive.as_deref())
        .map_err(AppError::new)?;
    // A jobs folder removed in Settings while the scan ran: drop it now (M4 review #18).
    let current = lock(&state.settings)?.scan_roots();
    lock(&state.index)?
        .forget_roots_except(&current)
        .map_err(|e| AppError::new(e.to_string()))?;
    crate::devices_cmd::sync_soon(&app);
    Ok(summary)
}

/// The folder to act on for a project, looked up in the index by key (never a path from the
/// UI) and re-checked against the jobs folders, since `index.sqlite` could be hand-edited.
fn project_folder(
    state: &AppState,
    key: &str,
    single: bool,
) -> AppResult<(PathBuf, Option<String>)> {
    if key.starts_with("remote:") {
        return Err(AppError::new(
            "That project is on another computer. Open or change it there.",
        ));
    }
    let located = lock(&state.index)?
        .locate(key)
        .map_err(|e| AppError::new(e.to_string()))?
        .ok_or_else(|| {
            AppError::new("That project isn't in the list any more. Rescan and try again.")
        })?;
    if single && located.paths.len() > 1 {
        let list: Vec<String> = located
            .paths
            .iter()
            .map(|(p, _)| p.display().to_string())
            .collect();
        return Err(AppError::new(format!(
            "This project exists in two places ({}). Remove the extra copy first, so Nest knows which one to change.",
            list.join(" and ")
        )));
    }
    let path = located
        .paths
        .iter()
        .find(|(_, online)| *online)
        .map(|(p, _)| p.clone())
        .ok_or_else(|| AppError::new("This project's jobs folder is offline"))?;
    let roots = lock(&state.settings)?.scan_roots();
    if !is_openable(&path, &roots) {
        return Err(AppError::new(
            "That folder isn't a project inside your jobs folders any more. Rescan and try again.",
        ));
    }
    Ok((path, located.id))
}

/// Archive (M5): a project folder inside the archive folder is read-only in Nest.
fn is_archived(settings: &Settings, path: &Path) -> bool {
    settings
        .archive_folder
        .as_ref()
        .is_some_and(|a| path.starts_with(a))
}

/// Open a project folder in Finder/Explorer.
#[tauri::command(async)]
pub fn open_project(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    key: String,
) -> AppResult<()> {
    let (path, _) = project_folder(&state, &key, false)?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| AppError::new(format!("Couldn't open the folder: {e}")))
}

/// Show a project folder selected in its parent folder.
#[tauri::command(async)]
pub fn reveal_project(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    key: String,
) -> AppResult<()> {
    let (path, _) = project_folder(&state, &key, false)?;
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| AppError::new(format!("Couldn't show the folder: {e}")))
}

/// Active / Done. Rewrites only `status` in the manifest (atomic, keeps unknown fields).
#[tauri::command(async)]
pub fn set_status(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    key: String,
    status: String,
) -> AppResult<()> {
    if !matches!(status.as_str(), "active" | "done") {
        return Err(AppError::new(format!("Unknown status \"{status}\"")));
    }
    // One status change at a time: yours, or a request from another computer (Devices).
    let _one = lock(&state.status_lock)?;
    let (path, id) = project_folder(&state, &key, true)?;
    let settings = lock(&state.settings)?;
    let archived = is_archived(&settings, &path);
    drop(settings);
    if archived {
        return Err(AppError::new(
            "This project is archived. Move its folder back to a jobs folder to change it.",
        ));
    }
    let id = id.ok_or_else(|| {
        AppError::new("The project file can't be read, so its status can't be changed")
    })?;
    manifest::update_status(&path.join(plan::MANIFEST_NAME), &id, &status)
        .map_err(AppError::new)?;
    lock(&state.index)?
        .set_status(&key, &status)
        .map_err(|e| AppError::new(e.to_string()))?;
    if let Ok(mut rt) = state.devices.runtime.lock() {
        rt.changes.remove(&key);
    }
    crate::devices_cmd::sync_soon(&app);
    Ok(())
}

/// Archive (M5 P2): move a Done project's folder into the archive folder, or an archived one
/// back to a jobs folder, on the same drive: one rename, nothing copied or deleted (locked
/// #4). Asks first with a native dialog. `Ok(None)` = cancelled; `Ok(Some(message))` = moved.
#[tauri::command(async)]
pub fn archive_project(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    key: String,
    unarchive: bool,
) -> AppResult<Option<String>> {
    let check = || -> AppResult<(archive::MovePlan, String)> {
        let (path, id) = project_folder(&state, &key, true)?;
        let settings = lock(&state.settings)?.clone();
        let archive_root = settings.archive_folder.clone().ok_or_else(|| {
            AppError::new("Choose an archive folder first: Settings → General → Archive.")
        })?;
        let id = id.ok_or_else(|| {
            AppError::new("The project file can't be read, so Nest won't move this project")
        })?;
        let manifest = manifest::read_json_object(&path.join(plan::MANIFEST_NAME))
            .map_err(|e| AppError::new(format!("The project file can't be read ({e})")))?;
        // The folder must still hold the project the list shows (it may have been swapped).
        if manifest["id"].as_str() != Some(id.as_str()) {
            return Err(AppError::new(
                "This folder now holds a different project. Rescan (Ctrl+R) and try again.",
            ));
        }
        let archived = is_archived(&settings, &path);
        let dest = if unarchive {
            if !archived {
                return Err(AppError::new("This project isn't archived"));
            }
            archive::unarchive_root(
                manifest["archivedFrom"].as_str(),
                &settings.jobs_roots,
                &path,
            )
            .ok_or_else(|| AppError::new("Choose a jobs folder first (Settings → General)"))?
        } else {
            if archived {
                return Err(AppError::new("This project is already archived"));
            }
            if manifest["status"].as_str() != Some("done") {
                return Err(AppError::new("Mark it done first, then archive it."));
            }
            archive_root
        };
        let plan = archive::plan_move(&path, &dest, unarchive).map_err(AppError::new)?;
        Ok((plan, id))
    };

    let (asked, _) = {
        let _one = lock(&state.status_lock)?;
        check()?
    };
    let target = asked.to.parent().unwrap_or(&asked.to).display().to_string();
    let (title, message, button) = if unarchive {
        (
            "Unarchive this project?",
            format!(
                "\"{}\" moves back to {target}. You can change it in Nest again.",
                asked.name
            ),
            "Unarchive",
        )
    } else {
        (
            "Archive this project?",
            format!(
                "\"{}\" moves to the archive folder ({target}). It stays a normal folder you can open; Unarchive brings it back.",
                asked.name
            ),
            "Archive",
        )
    };
    let confirmed = app
        .dialog()
        .message(message)
        .title(title)
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(
            button.into(),
            "Cancel".into(),
        ))
        .blocking_show();
    if !confirmed {
        return Ok(None);
    }

    // Checked again after the question: anything may have changed while the dialog was open.
    let one = lock(&state.status_lock)?;
    let (plan, id) = check()?;
    if plan != asked {
        return Err(AppError::new(
            "The project changed while Nest was asking. Nothing was moved; try again.",
        ));
    }
    archive::apply_move(&plan, unarchive).map_err(AppError::new)?;
    // A note only: archived is decided by where the folder is (locked #5), so the move stands.
    let note = archive::mark_manifest(
        &plan.to,
        &id,
        (!unarchive).then(|| plan.from.parent()).flatten(),
    )
    .err();
    drop(one);

    // From here on the move has happened: nothing below may turn it into an error.
    // Show it in its new place. A scan already running may have read the folders before the
    // move, so wait for it (a few seconds at most, usually) and scan once more after it.
    let started = std::time::Instant::now();
    let mut got_scan = true;
    while state.scanning.swap(true, Ordering::SeqCst) {
        if started.elapsed() > std::time::Duration::from_secs(60) {
            log::warn!("archive: a rescan kept running; the list updates on the next one");
            got_scan = false;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    if got_scan {
        let _guard = CreatingGuard(&state.scanning);
        let roots = state
            .settings
            .lock()
            .map(|s| (s.jobs_roots.clone(), s.archive_folder.clone()));
        match roots {
            Ok((roots, archive)) => {
                if let Err(e) = index::rescan_with_archive(&state.index, &roots, archive.as_deref())
                {
                    log::warn!("archive: rescan after the move failed: {e}");
                }
            }
            Err(_) => log::warn!("archive: settings unavailable for the rescan after the move"),
        }
    }
    crate::devices_cmd::sync_soon(&app);

    let done = if unarchive {
        format!("Moved {} back to {target}", plan.name)
    } else {
        format!("Archived {}", plan.name)
    };
    Ok(Some(match note {
        Some(e) => format!("{done}. (Its project file couldn't note the date: {e}.)"),
        None => done,
    }))
}

/// Folder sizes (M5 P4): the last measurement, if any (the UI shows it at once).
#[tauri::command(async)]
pub fn project_sizes(state: State<'_, AppState>, key: String) -> AppResult<Option<Sizes>> {
    lock(&state.index)?
        .sizes(&key)
        .map_err(|e| AppError::new(e.to_string()))
}

/// Measure a project folder now (every file; big footage folders take a few seconds) and
/// remember the result. Read-only on disk.
#[tauri::command(async)]
pub fn measure_project(state: State<'_, AppState>, key: String) -> AppResult<Sizes> {
    let (path, _) = project_folder(&state, &key, false)?;
    let sizes = crate::sizes::measure(&path, index::now_ms());
    lock(&state.index)?
        .put_sizes(&key, &sizes)
        .map_err(|e| AppError::new(e.to_string()))?;
    Ok(sizes)
}

/// Archive (M5): "Not this one". The project is never suggested for archiving again.
#[tauri::command(async)]
pub fn dismiss_archive_suggestion(state: State<'_, AppState>, key: String) -> AppResult<()> {
    let id = lock(&state.index)?
        .locate(&key)
        .map_err(|e| AppError::new(e.to_string()))?
        .and_then(|l| l.id)
        .ok_or_else(|| AppError::new("That project isn't in the list any more."))?;
    crate::settings_cmd::edit_settings_quietly(&state, |s| {
        s.dismiss_archive(&id);
        Ok(())
    })
}

/// What happened to a request from another computer to change a project's status.
pub(crate) enum RequestOutcome {
    Applied,
    /// Try again at the next sync (e.g. the jobs folder is offline right now).
    Later,
    /// Won't ever work; the reason goes back to the sender in plain words.
    Refused(String),
}

/// Apply another computer's "mark done / active" to one of this computer's projects. The
/// project is found by its id in this computer's index only (never by a path from the other
/// computer), and changed through the same safe path as a click here.
pub(crate) fn apply_request(
    state: &AppState,
    project_id: &str,
    status: &str,
    here: &str,
) -> RequestOutcome {
    if !matches!(status, "active" | "done") {
        return RequestOutcome::Refused("That request wasn't valid.".into());
    }
    let Ok(_one) = state.status_lock.lock() else {
        return RequestOutcome::Later;
    };
    // While the list is being (re)built (first launch, a deleted index, a rescan running), a
    // project that isn't found yet may still be there: wait instead of refusing.
    let list_ready = !state.scanning.load(Ordering::SeqCst)
        && state
            .index
            .lock()
            .ok()
            .and_then(|i| i.last_scan().ok().flatten())
            .is_some();
    let located = match state.index.lock().map(|i| i.locate(project_id)) {
        Ok(Ok(Some(l))) => l,
        Ok(Ok(None)) if list_ready => {
            return RequestOutcome::Refused(format!("That project isn't on {here} any more."))
        }
        _ => return RequestOutcome::Later,
    };
    if located.paths.len() > 1 {
        return RequestOutcome::Refused(format!(
            "That project exists in two places on {here}, so it wasn't changed. Remove the extra copy there first."
        ));
    }
    let Some(path) = located
        .paths
        .iter()
        .find(|(_, online)| *online)
        .map(|(p, _)| p.clone())
    else {
        return RequestOutcome::Later; // its jobs folder is offline right now
    };
    let (roots, archived) = match state.settings.lock() {
        Ok(s) => (s.scan_roots(), is_archived(&s, &path)),
        Err(_) => return RequestOutcome::Later,
    };
    if !is_openable(&path, &roots) {
        return RequestOutcome::Refused(format!(
            "That project isn't inside {here}'s jobs folders any more."
        ));
    }
    if archived {
        return RequestOutcome::Refused(format!("That project is archived on {here}."));
    }
    let Some(id) = located.id else {
        return RequestOutcome::Refused(format!("That project's file can't be read on {here}."));
    };
    if let Err(e) = manifest::update_status(&path.join(plan::MANIFEST_NAME), &id, status) {
        // Only "something changed the file just now" is worth retrying; anything else (an
        // unreadable file, a different project, a read-only file) won't fix itself, so it's
        // refused with the reason instead of blocking that computer's later requests.
        // Retry what can pass: the file was changed just now, or it couldn't be read/saved for
        // a moment (a sync app or antivirus holding it, a NAS hiccup). Refuse only what won't
        // fix itself: a damaged project file, or a folder that now holds another project.
        let lasting = e.contains("can't be read") || e.contains("different project");
        if !lasting {
            log::info!("request will be retried: {e}");
            return RequestOutcome::Later;
        }
        return RequestOutcome::Refused(e);
    }
    match state
        .index
        .lock()
        .map(|mut i| i.set_status(project_id, status))
    {
        Ok(Ok(())) => RequestOutcome::Applied,
        _ => RequestOutcome::Applied, // the file changed; the next rescan corrects the cache
    }
}

/// The update found by the background check, if any (see update.rs).
#[tauri::command(async)]
pub fn pending_update(app: tauri::AppHandle) -> Option<crate::update::UpdateInfo> {
    crate::update::pending(&app)
}

/// "Check for updates": returns the newer version, or `None` if Nest is up to date.
#[tauri::command]
pub async fn check_for_updates(
    app: tauri::AppHandle,
) -> AppResult<Option<crate::update::UpdateInfo>> {
    crate::update::check_now(&app).await
}

/// "Restart to update": download, verify the signature, install and relaunch.
#[tauri::command]
pub async fn install_update(app: tauri::AppHandle) -> AppResult<()> {
    crate::update::install(&app).await
}

/// Copy text (a path or job code) from native menus, which the web clipboard can't do.
#[tauri::command(async)]
pub fn copy_text(app: tauri::AppHandle, text: String) -> AppResult<()> {
    app.clipboard()
        .write_text(text)
        .map_err(|e| AppError::new(format!("Couldn't copy: {e}")))
}

/// A project folder (has a `.project.json`) strictly inside one of `roots`, compared as
/// canonical paths component by component (so `D:\Jobs2`, `..` and links out of the root
/// are all rejected). Requiring the manifest also means a macOS `.app` can never be launched.
pub fn is_openable(path: &Path, roots: &[PathBuf]) -> bool {
    let Ok(target) = path.canonicalize() else {
        return false;
    };
    target.is_dir()
        && target.join(plan::MANIFEST_NAME).is_file()
        && roots
            .iter()
            .filter_map(|r| r.canonicalize().ok())
            .any(|root| target != root && target.starts_with(&root))
}

/// A template that isn't installed here: "grading-performance" → "Grading performance".
fn readable_id(id: &str) -> String {
    let words = id.replace(['-', '_'], " ");
    let mut c = words.trim().chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn open_folder_only_inside_jobs_roots() {
        let tmp = tempfile::tempdir().unwrap();
        let jobs = tmp.path().join("Jobs");
        let jobs2 = tmp.path().join("Jobs2");
        fs::create_dir_all(jobs.join("2609_VX-L01_A")).unwrap();
        fs::write(jobs.join("2609_VX-L01_A").join(plan::MANIFEST_NAME), "{}").unwrap();
        fs::create_dir_all(jobs2.join("x")).unwrap();
        fs::write(jobs2.join("x").join(plan::MANIFEST_NAME), "{}").unwrap();
        fs::create_dir_all(jobs.join("Evil.app")).unwrap();
        fs::write(jobs.join("run.bat"), "echo hi").unwrap();
        let roots = vec![jobs.clone()];

        assert!(is_openable(&jobs.join("2609_VX-L01_A"), &roots));
        assert!(
            !is_openable(&jobs.join("Evil.app"), &roots),
            "not a project folder"
        );
        assert!(!is_openable(&jobs, &roots), "the root itself");
        assert!(!is_openable(&jobs2.join("x"), &roots), "prefix trick");
        assert!(
            !is_openable(
                &jobs
                    .join("2609_VX-L01_A")
                    .join("..")
                    .join("..")
                    .join("Jobs2"),
                &roots
            ),
            ".."
        );
        assert!(
            !is_openable(&jobs.join("run.bat"), &roots),
            "files never open"
        );
        assert!(!is_openable(&jobs.join("missing"), &roots));
    }
}

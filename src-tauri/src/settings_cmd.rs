//! Commands behind the Settings window and first run (spec §3.3, §3.4, M4).
//!
//! Every change goes through `edit`: it refuses while the settings file is damaged, applies
//! one change in Rust (so two windows never overwrite each other), saves atomically, and
//! tells every window via a `settings-changed` event.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use chrono::{Datelike, Local};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;

use crate::commands::{lock, reload_templates, AppState};
use crate::error::{bin_name, io_reason, AppError, AppResult};
use crate::index::scan_root;
use crate::login_item::LoginItem;
use crate::plan::Date;
use crate::settings::{self, Settings};
use crate::template::{FileEntry, Template};
use crate::template_edit::{
    self, EditorSession, Example, SaveMode, TemplateDraft, TemplatePreview,
};
use crate::template_from_folder::{self, Skipped};
use crate::template_io::{self, TemplateInfo, BUILT_IN_IDS, LIMITS, PACKAGE_EXT};
use crate::window;

pub const SETTINGS_WINDOW: &str = "settings";
pub const CHANGED_EVENT: &str = "settings-changed";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub settings: Settings,
    pub settings_error: Option<String>,
    pub first_run: bool,
    pub templates_folder: PathBuf,
    /// Dev builds: some things (launch at login, updates) only work in the installed app.
    pub dev_build: bool,
    /// First run suggests this job-code letter for this computer (W on Windows, M on a Mac).
    pub suggested_marker: &'static str,
    /// Opening at login as the system has it now (not just what was last clicked).
    pub login_item: LoginItem,
}

fn view(app: &AppHandle, state: &AppState) -> AppResult<SettingsView> {
    Ok(SettingsView {
        settings: lock(&state.settings)?.clone(),
        settings_error: state.settings_error.clone(),
        first_run: state.first_run.load(Ordering::SeqCst),
        templates_folder: state.user_templates.clone(),
        dev_build: cfg!(debug_assertions),
        suggested_marker: crate::settings::suggested_marker(),
        login_item: crate::login_item::state(app),
    })
}

/// Apply one change, save, and tell every window.
fn edit(
    app: &AppHandle,
    state: &AppState,
    change: impl FnOnce(&mut Settings) -> Result<(), String>,
) -> AppResult<SettingsView> {
    if let Some(e) = &state.settings_error {
        return Err(AppError::new(format!(
            "Settings can't be changed while the settings file is damaged. {e}"
        )));
    }
    {
        let mut current = lock(&state.settings)?;
        let mut next = current.clone();
        change(&mut next).map_err(AppError::new)?;
        settings::save(&state.config_dir, &next)?;
        *current = next;
    }
    let v = view(app, state)?;
    let _ = app.emit(CHANGED_EVENT, ());
    Ok(v)
}

#[tauri::command(async)]
pub fn get_settings(app: AppHandle, state: State<'_, AppState>) -> AppResult<SettingsView> {
    view(&app, &state)
}

// ── Windows ──

/// Open (or focus) the Settings window.
/// Must be `async`: building a window from a synchronous command deadlocks on Windows
/// (the window appears empty and the whole app freezes). Shipped broken in 0.1.3.
#[tauri::command(async)]
pub fn open_settings(app: AppHandle) -> AppResult<()> {
    open_settings_window(&app).map_err(|e| AppError::new(format!("Couldn't open Settings: {e}")))
}

pub fn open_settings_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window(SETTINGS_WINDOW) {
        w.unminimize()?;
        w.show()?;
        // A Nest window shows: folder work may start (folder_access.rs), even if it never
        // gets focus.
        crate::folder_access::window_shown(app);
        return w.set_focus();
    }
    let mut builder =
        WebviewWindowBuilder::new(app, SETTINGS_WINDOW, WebviewUrl::App("index.html".into()))
            .title("Nest Settings")
            .inner_size(860.0, 600.0)
            .min_inner_size(660.0, 460.0)
            .transparent(true)
            .center()
            // Hidden until its page has drawn (see `settings_ready`), so it appears in one go
            // instead of flashing an empty frame.
            .visible(false);
    // Dev builds keep their webview data apart from the installed app (see tauri.dev.conf.json):
    // on Windows two webviews with different settings can't share one data folder.
    if cfg!(debug_assertions) {
        builder = builder.data_directory(
            app.path()
                .app_local_data_dir()?
                .join("dev-webview")
                .join(SETTINGS_WINDOW),
        );
    }
    let w = builder.build()?;
    // Safety net: if the page never says it's ready, show the window anyway.
    let fallback = w.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        if !fallback.is_visible().unwrap_or(true) {
            let _ = fallback.show();
            let _ = fallback.set_focus();
            crate::folder_access::window_shown(fallback.app_handle());
        }
    });
    let theme = app
        .state::<AppState>()
        .settings
        .lock()
        .map(|s| s.theme.clone())
        .unwrap_or_default();
    let app2 = app.clone();
    window::apply_appearance_on_main(app, &w, &theme, move |ok| {
        if let Some(m) = app2.try_state::<window::SettingsMaterial>() {
            m.0.store(ok, Ordering::SeqCst);
        }
    });
    Ok(())
}

// ── General ──

#[tauri::command(async)]
pub fn add_jobs_root(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> AppResult<Option<SettingsView>> {
    let Some(path) = pick_folder(&window, "Add a jobs folder")? else {
        return Ok(None);
    };
    edit(&app, &state, |s| {
        if let Some(problem) = s.jobs_root_problem(&path) {
            return Err(problem);
        }
        s.add_root(path);
        Ok(())
    })
    .map(Some)
}

/// v0.4.2: where Personal-space and "No client" projects go. One of the jobs folders, or
/// None = the first jobs folder (same as other new projects).
#[tauri::command(async)]
pub fn set_personal_root(
    app: AppHandle,
    state: State<'_, AppState>,
    path: Option<PathBuf>,
) -> AppResult<SettingsView> {
    edit(&app, &state, |s| s.set_personal_root(path))
}

/// "Change…" in New Project while it shows the Personal folder: pick a folder, add it to the
/// jobs folders if it's new (so it's watched), and use it for Personal projects.
#[tauri::command(async)]
pub fn choose_personal_root(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> AppResult<Option<SettingsView>> {
    let Some(path) = pick_folder(&window, "Choose the folder for Personal projects")? else {
        return Ok(None);
    };
    edit(&app, &state, |s| {
        if let Some(problem) = s.jobs_root_problem(&path) {
            return Err(problem);
        }
        if !s.jobs_roots.contains(&path) {
            s.add_root(path.clone());
        }
        s.set_personal_root(Some(path))
    })
    .map(Some)
}

// ── Archive (M5, spec §12a #21) ──

/// Pick the archive folder. Its projects show as archived after the next rescan (the main
/// window rescans on `settings-changed`).
#[tauri::command(async)]
pub fn set_archive_folder(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> AppResult<Option<SettingsView>> {
    let Some(path) = pick_folder(&window, "Choose the archive folder")? else {
        return Ok(None);
    };
    switch_archive_folder(&app, &window, &state, Some(path))
}

/// Stop watching the archive folder. Nothing on disk changes; its projects leave the list.
/// `None` = you clicked Cancel.
#[tauri::command(async)]
pub fn clear_archive_folder(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> AppResult<Option<SettingsView>> {
    switch_archive_folder(&app, &window, &state, None)
}

/// "Use again" on a previous archive folder.
#[tauri::command(async)]
pub fn use_past_archive_folder(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, AppState>,
    path: PathBuf,
) -> AppResult<Option<SettingsView>> {
    if !lock(&state.settings)?.past_archive_folders.contains(&path) {
        return Err(AppError::new(
            "That folder isn't in the list of previous archive folders",
        ));
    }
    if !path.is_dir() {
        return Err(AppError::new(format!(
            "Can't find {} (is its drive connected?)",
            path.display()
        )));
    }
    switch_archive_folder(&app, &window, &state, Some(path))
}

/// Remove a folder from "Previous archive folders" (only the note; nothing on disk changes).
#[tauri::command(async)]
pub fn forget_past_archive_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    path: PathBuf,
) -> AppResult<SettingsView> {
    edit(&app, &state, |s| {
        s.forget_past_archive_folder(&path);
        Ok(())
    })
}

/// Change (or turn off) the archive folder. If the current one holds projects, ask first:
/// they stay where they are, but Nest stops showing them. The old folder is remembered under
/// "Previous archive folders". `None` = cancelled.
fn switch_archive_folder(
    app: &AppHandle,
    window: &WebviewWindow,
    state: &AppState,
    to: Option<PathBuf>,
) -> AppResult<Option<SettingsView>> {
    let current = lock(&state.settings)?.archive_folder.clone();
    if let Some(current) = current.filter(|c| Some(c) != to.as_ref()) {
        let count = folder_info(current.clone()).projects;
        if count > 0 {
            let projects = if count == 1 {
                "1 project".to_string()
            } else {
                format!("{count} projects")
            };
            let confirmed = window
                .dialog()
                .message(format!(
                    "Your current archive folder ({}) has {projects}. They stay there, but Nest will stop showing them. You can switch back any time under \"Previous archive folders\".",
                    current.display()
                ))
                .title(if to.is_some() {
                    "Change the archive folder?"
                } else {
                    "Turn off the archive folder?"
                })
                .kind(MessageDialogKind::Info)
                .parent(window)
                .buttons(MessageDialogButtons::OkCancelCustom(
                    if to.is_some() { "Change" } else { "Turn off" }.into(),
                    "Cancel".into(),
                ))
                .blocking_show();
            if !confirmed {
                return Ok(None);
            }
        }
    }
    let v = edit(app, state, |s| s.set_archive_folder(to))?;
    forget_roots_not_in(state, &v)?;
    Ok(Some(v))
}

/// Suggest archiving projects done for more than `days` (None = no suggestions).
#[tauri::command(async)]
pub fn set_archive_after_days(
    app: AppHandle,
    state: State<'_, AppState>,
    days: Option<u32>,
) -> AppResult<SettingsView> {
    edit(&app, &state, |s| s.set_archive_after_days(days))
}

/// Drop indexed projects whose folder is no longer one Nest watches.
fn forget_roots_not_in(state: &AppState, v: &SettingsView) -> AppResult<()> {
    lock(&state.index)?
        .forget_roots_except(&v.settings.scan_roots())
        .map_err(|e| AppError::new(e.to_string()))
}

#[tauri::command(async)]
pub fn remove_jobs_root(
    app: AppHandle,
    state: State<'_, AppState>,
    path: PathBuf,
) -> AppResult<SettingsView> {
    let v = edit(&app, &state, |s| s.remove_root(&path))?;
    forget_roots_not_in(&state, &v)?;
    Ok(v)
}

#[tauri::command(async)]
pub fn move_jobs_root(
    app: AppHandle,
    state: State<'_, AppState>,
    path: PathBuf,
    to: usize,
) -> AppResult<SettingsView> {
    edit(&app, &state, |s| s.move_root(&path, to))
}

#[tauri::command(async)]
pub fn set_default_space(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> AppResult<SettingsView> {
    edit(&app, &state, |s| s.set_default_space(&name))
}

#[tauri::command(async)]
pub fn set_launch_at_login(
    app: AppHandle,
    state: State<'_, AppState>,
    on: bool,
) -> AppResult<SettingsView> {
    if let Some(e) = &state.settings_error {
        return Err(AppError::new(format!(
            "Settings can't be changed while the settings file is damaged. {e}"
        )));
    }
    let was_on = crate::login_item::registered(crate::login_item::state(&app));
    // Remembered too: an old-style entry is only moved over (login_item::migrate) if it was on.
    system_then_save(
        || crate::login_item::set(&app, on).map(|_| ()),
        || {
            edit(&app, &state, |s| {
                s.launch_at_login = on;
                Ok(())
            })
        },
        || crate::login_item::restore(&app, was_on),
    )
}

/// Change the system, then save the setting. If saving fails, undo the system change, so the
/// system and the settings file never disagree. Nothing changed → nothing saved or undone.
fn system_then_save<T>(
    change: impl FnOnce() -> AppResult<()>,
    save: impl FnOnce() -> AppResult<T>,
    undo: impl FnOnce(),
) -> AppResult<T> {
    change()?;
    save().inspect_err(|e| {
        log::warn!(
            "couldn't save the setting, putting the system back: {}",
            e.message
        );
        undo();
    })
}

/// v0.7: "Show Nest in the menu bar" (Windows: "Keep Nest in the notification area").
#[tauri::command(async)]
pub fn set_show_in_menu_bar(
    app: AppHandle,
    state: State<'_, AppState>,
    on: bool,
) -> AppResult<SettingsView> {
    let v = edit(&app, &state, |s| {
        s.show_in_menu_bar = on;
        Ok(())
    })?;
    crate::tray::apply(&app);
    Ok(v)
}

/// The main window says whether the New Project sheet is open (an update never installs by
/// itself while it is; see tray.rs).
#[tauri::command(async)]
pub fn set_new_project_open(app: AppHandle, open: bool) {
    crate::tray::set_sheet_open(&app, open);
}

// ── Job codes ──

#[tauri::command(async)]
pub fn set_code_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    pattern: String,
    marker: String,
    own_code: String,
) -> AppResult<SettingsView> {
    edit(&app, &state, |s| {
        s.set_code_settings(&pattern, &marker, &own_code)
    })
}

// ── Spaces ──

fn space_in_use(state: &AppState, name: &str) -> AppResult<bool> {
    let used = lock(&state.index)?
        .spaces_in_use()
        .map_err(|e| AppError::new(e.to_string()))?;
    Ok(used.contains(name))
}

#[tauri::command(async)]
pub fn add_space(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> AppResult<SettingsView> {
    edit(&app, &state, |s| s.add_space(&name))
}

#[tauri::command(async)]
pub fn rename_space(
    app: AppHandle,
    state: State<'_, AppState>,
    old: String,
    new: String,
) -> AppResult<SettingsView> {
    let in_use = space_in_use(&state, &old)?;
    edit(&app, &state, |s| s.rename_space(&old, &new, in_use))
}

#[tauri::command(async)]
pub fn delete_space(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> AppResult<SettingsView> {
    let in_use = space_in_use(&state, &name)?;
    edit(&app, &state, |s| s.delete_space(&name, in_use))
}

#[tauri::command(async)]
pub fn move_space(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    to: usize,
) -> AppResult<SettingsView> {
    edit(&app, &state, |s| s.move_space(&name, to))
}

#[tauri::command(async)]
pub fn set_space_no_client(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    on: bool,
) -> AppResult<SettingsView> {
    edit(&app, &state, |s| s.set_no_client(&name, on))
}

/// Which spaces have projects (the UI disables rename/delete for them).
#[tauri::command(async)]
pub fn spaces_in_use(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    let mut v: Vec<String> = lock(&state.index)?
        .spaces_in_use()
        .map_err(|e| AppError::new(e.to_string()))?
        .into_iter()
        .collect();
    v.sort();
    Ok(v)
}

// ── Appearance ──

#[tauri::command(async)]
pub fn set_theme(
    app: AppHandle,
    state: State<'_, AppState>,
    theme: String,
) -> AppResult<SettingsView> {
    let v = edit(&app, &state, |s| s.set_theme(&theme))?;
    for w in app.webview_windows().values() {
        window::apply_appearance_on_main(&app, w, &theme, |_| {});
    }
    Ok(v)
}

// ── Templates ──

#[tauri::command(async)]
pub fn list_templates(state: State<'_, AppState>) -> AppResult<Vec<TemplateInfo>> {
    let hidden = lock(&state.settings)?.hidden_templates.clone();
    Ok(lock(&state.templates)?
        .iter()
        .map(|t| template_io::info(t, &hidden))
        .collect())
}

fn find_template(state: &AppState, key: &str) -> AppResult<template_io::Installed> {
    lock(&state.templates)?
        .iter()
        .find(|t| t.key() == key)
        .cloned()
        .ok_or_else(|| AppError::new("That template isn't installed any more"))
}

pub(crate) fn templates_changed(app: &AppHandle, state: &AppState) -> AppResult<()> {
    reload_templates(state)?;
    let _ = app.emit(CHANGED_EVENT, ());
    // v0.5 template sync: your other computers get the change (waits for the templates lock).
    crate::devices_cmd::sync_soon(app);
    Ok(())
}

/// Pick a `.nesttemplate` or `.json` and install it. `None` if the picker was cancelled.
#[tauri::command(async)]
pub fn import_template(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> AppResult<Option<TemplateInfo>> {
    let _one = lock(&state.templates_lock)?; // one change to your templates at a time
    let Some(file) = window
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("Import a template")
        .add_filter("Nest template", &[PACKAGE_EXT, "json"])
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let file = file.into_path().map_err(|e| AppError::new(e.to_string()))?;
    install_template_file(&app, &state, &file)
}

/// Install a `.nesttemplate` or `.json` given by path (dropped on the Templates screen):
/// the same checks and messages as Import…, after making sure it's a template file.
#[tauri::command(async)]
pub fn import_template_from(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> AppResult<Option<TemplateInfo>> {
    let file = PathBuf::from(path);
    template_io::check_import_file(&file).map_err(AppError::new)?;
    let _one = lock(&state.templates_lock)?; // one change to your templates at a time
    install_template_file(&app, &state, &file)
}

/// What each dropped path is: a folder, a template file, or neither (read-only).
#[tauri::command(async)]
pub fn drop_kinds(paths: Vec<String>) -> Vec<template_io::DropKind> {
    paths
        .iter()
        .map(|p| template_io::drop_kind(Path::new(p)))
        .collect()
}

/// The shared part of Import… and a dropped template file. The caller holds `templates_lock`.
fn install_template_file(
    app: &AppHandle,
    state: &AppState,
    file: &Path,
) -> AppResult<Option<TemplateInfo>> {
    let installed = lock(&state.templates)?.clone();
    let loaded = template_io::import(file, &state.user_templates, &installed, LIMITS)
        .map_err(AppError::new)?;
    crate::template_sync_cmd::note_saved(
        state,
        &loaded.template.id,
        loaded.template.version,
        false,
    );
    templates_changed(app, state)?;
    // Decision T-A: a newer version of one of yours replaces it; the older copy goes to the bin.
    let installed = lock(&state.templates)?.clone();
    let bin = |t: &template_io::Installed| template_io::delete(t, &state.user_templates);
    match template_edit::retire_older_copies(&loaded.template.id, &loaded.dir, &installed, &bin) {
        Ok(0) => {}
        Ok(_) => templates_changed(app, state)?,
        // Added fine, but the older copy stayed: say so (the list already shows the new one).
        Err(e) => {
            templates_changed(app, state)?;
            return Err(AppError::new(format!(
                "Added \"{}\". {}",
                loaded.template.name,
                e.trim_start_matches("Saved. ")
            )));
        }
    }
    let hidden = lock(&state.settings)?.hidden_templates.clone();
    let t = lock(&state.templates)?
        .iter()
        .find(|t| t.loaded.dir == loaded.dir)
        .map(|t| template_io::info(t, &hidden));
    Ok(t)
}

/// Save a template as a `.nesttemplate` file. `false` if the dialog was cancelled.
#[tauri::command(async)]
pub fn export_template(
    window: WebviewWindow,
    state: State<'_, AppState>,
    key: String,
) -> AppResult<bool> {
    let t = find_template(&state, &key)?;
    let Some(dest) = window
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("Export template")
        .set_file_name(format!("{}.{PACKAGE_EXT}", t.loaded.template.id))
        .add_filter("Nest template", &[PACKAGE_EXT])
        .blocking_save_file()
    else {
        return Ok(false);
    };
    let dest = dest.into_path().map_err(|e| AppError::new(e.to_string()))?;
    template_io::export(&t.loaded, &dest).map_err(AppError::new)?;
    Ok(true)
}

#[tauri::command(async)]
pub fn duplicate_template(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
) -> AppResult<()> {
    let _one = lock(&state.templates_lock)?; // one change to your templates at a time
    let t = find_template(&state, &key)?;
    let installed = lock(&state.templates)?.clone();
    let copy = template_io::duplicate(&t.loaded, &state.user_templates, &installed)
        .map_err(AppError::new)?;
    crate::template_sync_cmd::note_saved(&state, &copy.template.id, copy.template.version, true);
    templates_changed(&app, &state)
}

fn versions_phrase(n: usize) -> String {
    if n <= 1 {
        String::new()
    } else {
        format!(" (with {} older copies)", n - 1)
    }
}

/// Move one of your templates (every saved version of it) to the Recycle Bin / Trash, after a
/// native confirm that says how many versions go (spec §12a #16, extended 2026-10-02).
#[tauri::command(async)]
pub fn delete_template(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, AppState>,
    key: String,
) -> AppResult<bool> {
    let _one = lock(&state.templates_lock)?; // one change to your templates at a time
    let t = find_template(&state, &key)?;
    if t.built_in {
        return Err(AppError::new(
            "Built-in templates can't be deleted, only hidden",
        ));
    }
    bin_yours(&app, &window, &state, &t, "")
}

/// The native confirm, then every version of one of yours to the Recycle Bin / Trash.
/// `false` if cancelled. The caller holds the templates lock.
fn bin_yours(
    app: &AppHandle,
    window: &WebviewWindow,
    state: &AppState,
    t: &template_io::Installed,
    extra: &str,
) -> AppResult<bool> {
    let installed = lock(&state.templates)?.clone();
    let id = t.loaded.template.id.clone();
    let count = template_edit::your_versions(&id, &installed).len();
    let bin = bin_name();
    let confirmed = window
        .dialog()
        .message(format!(
            "\"{}\"{} will be moved to the {bin}. You can restore it from there. Projects made with it aren't affected.{extra}",
            t.loaded.template.name,
            versions_phrase(count)
        ))
        .title(format!("Move template to the {bin}?"))
        .kind(MessageDialogKind::Warning)
        .parent(window)
        .buttons(MessageDialogButtons::OkCancelCustom(format!("Move to {bin}"), "Cancel".into()))
        .blocking_show();
    if !confirmed {
        return Ok(false);
    }
    let result = template_edit::trash_all_versions(&id, &installed, &state.user_templates);
    // Refresh either way (some versions may have moved), but report the bin's error first.
    let refreshed = templates_changed(app, state);
    result.map_err(AppError::new)?;
    refreshed.map(|_| true)
}

/// Remove from the Templates list: one of Nest's is hidden everywhere (no question: "Bring back
/// Nest's templates" undoes it); yours goes to the Recycle Bin / Trash after a confirm. Your
/// version of one of Nest's goes to the bin and Nest's original is hidden too, so removing
/// it doesn't bring the original back in its place. `false` if cancelled.
#[tauri::command(async)]
pub fn remove_template(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, AppState>,
    key: String,
) -> AppResult<bool> {
    let _one = lock(&state.templates_lock)?; // one change to your templates at a time
    let t = find_template(&state, &key)?;
    let installed = lock(&state.templates)?.clone();
    match template_io::removal(&t, &installed) {
        template_io::Removal::Hide(id) => {
            edit(&app, &state, |s| {
                s.set_template_hidden(&id, true);
                Ok(())
            })?;
            Ok(true)
        }
        template_io::Removal::Bin { id, hide_original } => {
            let extra = if hide_original {
                " Nest's original stays hidden; \"Bring back Nest's templates\" shows it again."
            } else {
                ""
            };
            if !bin_yours(&app, &window, &state, &t, extra)? {
                return Ok(false);
            }
            if hide_original {
                edit(&app, &state, |s| {
                    s.set_template_hidden(&id, true);
                    Ok(())
                })?;
            }
            Ok(true)
        }
    }
}

/// "Bring back Nest's templates": every hidden one of Nest's shows again.
#[tauri::command(async)]
pub fn restore_builtin_templates(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<SettingsView> {
    edit(&app, &state, |s| {
        s.hidden_templates = template_io::without_built_ins(&s.hidden_templates, BUILT_IN_IDS);
        Ok(())
    })
}

/// "Go back to Nest's original": your version of a built-in goes to the Recycle Bin / Trash.
#[tauri::command(async)]
pub fn reset_template(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, AppState>,
    key: String,
) -> AppResult<bool> {
    let _one = lock(&state.templates_lock)?; // one change to your templates at a time
    let t = find_template(&state, &key)?;
    let installed = lock(&state.templates)?.clone();
    let id = t.loaded.template.id.clone();
    let has_original = installed
        .iter()
        .any(|i| i.built_in && i.loaded.template.id == id);
    if t.built_in || !has_original {
        return Err(AppError::new(
            "That template has no Nest original to go back to",
        ));
    }
    let count = template_edit::your_versions(&id, &installed).len();
    let bin = bin_name();
    let confirmed = window
        .dialog()
        .message(format!(
            "Your version of \"{}\"{} will be moved to the {bin}, and Nest's original comes back in New Project. You can restore yours from the {bin}. Projects you already made aren't affected.",
            t.loaded.template.name,
            versions_phrase(count)
        ))
        .title("Go back to Nest's original?")
        .kind(MessageDialogKind::Warning)
        .parent(&window)
        .buttons(MessageDialogButtons::OkCancelCustom("Go back to original".into(), "Cancel".into()))
        .blocking_show();
    if !confirmed {
        return Ok(false);
    }
    let result = template_edit::reset_to_original(&id, &installed, &state.user_templates);
    let refreshed = templates_changed(&app, &state);
    result.map_err(AppError::new)?;
    refreshed.map(|_| true)
}

/// Example answers use today and this computer's job-code format.
pub(crate) fn example(state: &AppState) -> AppResult<Example> {
    let s = lock(&state.settings)?;
    Ok(Example {
        today: today(),
        code_pattern: s.code_pattern.clone(),
        marker: s.local_marker.clone(),
    })
}

fn today() -> Date {
    let now = Local::now();
    Date {
        year: now.year().clamp(0, 9999) as u16,
        month: now.month() as u8,
        day: now.day() as u8,
    }
}

/// Templates live in the main window: bring it forward on its Templates view (from Settings).
#[tauri::command(async)]
pub fn open_templates(app: AppHandle) -> AppResult<()> {
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| AppError::new("Nest's main window isn't open"))?;
    let _ = main.unminimize();
    main.show()
        .and_then(|_| main.set_focus())
        .map_err(|e| AppError::new(format!("Couldn't show the main window: {e}")))?;
    crate::folder_access::window_shown(&app);
    main.emit_to("main", "open-templates", ())
        .map_err(|e| AppError::new(e.to_string()))
}

/// Open a template in the editor.
#[tauri::command(async)]
pub fn template_draft(state: State<'_, AppState>, key: String) -> AppResult<TemplateDraft> {
    Ok(template_edit::draft(&find_template(&state, &key)?))
}

/// What New Project would make from the editor's draft (pure: writes nothing).
#[tauri::command(async)]
pub fn preview_template(
    state: State<'_, AppState>,
    key: String,
    draft: Template,
) -> AppResult<TemplatePreview> {
    let ex = example(&state)?;
    if let Some(session) = lock(&state.editor_sessions)?.get(&key) {
        return Ok(template_edit::preview(
            &draft,
            &session.loaded(draft.clone()),
            &ex,
        ));
    }
    let source = find_template(&state, &key)?;
    Ok(template_edit::preview(&draft, &source.loaded, &ex))
}

/// The editor's names as the example project would have them (pure: writes nothing). `key` is
/// the editor session; the example answers are the preview's, so the two always agree.
#[tauri::command(async)]
pub fn example_names(
    state: State<'_, AppState>,
    key: String,
    draft: Template,
    names: Vec<String>,
) -> AppResult<Vec<String>> {
    let _ = key; // same answers for every session; kept so the call matches preview_template
    Ok(template_edit::example_names(
        &draft,
        &example(&state)?,
        &names,
    ))
}

/// Save the editor's draft as one of your templates. `key` is the template the editor started
/// from; Rust picks the id and version.
#[tauri::command(async)]
pub fn save_template(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
    draft: Template,
    mode: SaveMode,
) -> AppResult<Saved> {
    let _one = lock(&state.templates_lock)?; // one change to your templates at a time
    let installed = lock(&state.templates)?.clone();
    let session = match lock(&state.editor_sessions)?.get(&key).cloned() {
        Some(s) => s,
        None => EditorSession::from_template(&find_template(&state, &key)?),
    };
    let saved = template_edit::save_session(
        &draft,
        mode,
        &session,
        &installed,
        &state.user_templates,
        BUILT_IN_IDS,
        &example(&state)?,
    )
    .map_err(AppError::new)?;
    let fresh = session.source.is_none() || mode == SaveMode::New;
    crate::template_sync_cmd::note_saved(&state, &saved.template.id, saved.template.version, fresh);
    lock(&state.editor_sessions)?.remove(&key);
    // Hiding is per template id, and your customized version shares the built-in's id: show it,
    // since you just made it to use it.
    if mode == SaveMode::Customize {
        let id = saved.template.id.clone();
        // The template is already saved: a failure here is logged, not reported as a failed save.
        if let Err(e) = edit(&app, &state, |s| {
            s.set_template_hidden(&id, false);
            Ok(())
        }) {
            log::warn!(
                "customized template saved, but couldn't unhide it: {}",
                e.message
            );
        }
    }
    templates_changed(&app, &state)?;
    // Decision T-A: one copy per template. The new copy is saved and loaded; now the older
    // copy (or copies) go to the bin. If that fails, both stay and the message says so.
    let installed = lock(&state.templates)?.clone();
    let bin = |t: &template_io::Installed| template_io::delete(t, &state.user_templates);
    let retired =
        template_edit::retire_older_copies(&saved.template.id, &saved.dir, &installed, &bin);
    templates_changed(&app, &state)?;
    let hidden = lock(&state.settings)?.hidden_templates.clone();
    let info = lock(&state.templates)?
        .iter()
        .find(|t| t.loaded.dir == saved.dir)
        .map(|t| template_io::info(t, &hidden))
        .ok_or_else(|| AppError::new("The template was saved, but couldn't be loaded back"))?;
    let (binned, bin_problem) = match retired {
        Ok(n) => (n, None),
        Err(e) => (0, Some(e)),
    };
    Ok(Saved {
        info,
        binned,
        bin_problem,
    })
}

/// A template read from a folder, ready for the editor (`key` goes to preview / save).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderDraft {
    pub key: String,
    pub draft: TemplateDraft,
    /// What was left out, and why.
    pub skipped: Vec<Skipped>,
    /// The folder was a Nest project: its answers became blanks.
    pub from_project: bool,
}

/// Make a template from a folder: `path` is a folder dropped on the list, or `None` to pick
/// one. Reads the folder with its files (read-only: it's never changed); only if they're over
/// the 200 MB limit, asks to make it from the folders alone. The draft lives in Rust until
/// it's saved or the editor closes; the UI only gets its key. `None` if cancelled.
#[tauri::command(async)]
pub fn template_from_folder(
    window: WebviewWindow,
    state: State<'_, AppState>,
    path: Option<String>,
) -> AppResult<Option<FolderDraft>> {
    let path = match path {
        Some(p) => PathBuf::from(p),
        None => {
            let picked = window
                .dialog()
                .file()
                .set_parent(&window)
                .set_title("Make a template from a folder")
                .blocking_pick_folder();
            let Some(picked) = picked else {
                return Ok(None);
            };
            picked
                .into_path()
                .map_err(|e| AppError::new(e.to_string()))?
        }
    };
    // Always folders and files; only a folder over the 200 MB limit asks about folders only.
    let installed = lock(&state.templates)?.clone();
    let folders_only = || {
        window
            .dialog()
            .message("That folder's files add up to more than 200 MB. Make the template from its folders only?")
            .title("Folder too big")
            .parent(&window)
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Folders only".into(),
                "Cancel".into(),
            ))
            .blocking_show()
    };
    let Some(folder) =
        template_from_folder::read_folder_or_folders_only(&path, folders_only, |id| {
            installed
                .iter()
                .filter(|t| t.loaded.template.id == id)
                .max_by_key(|t| (!t.built_in, t.loaded.template.version))
                .map(|t| t.loaded.template.clone())
        })
        .map_err(AppError::new)?
    else {
        return Ok(None);
    };
    let session = EditorSession::from_folder(&folder);
    let out = FolderDraft {
        key: new_session(&state, session)?,
        draft: TemplateDraft {
            template: folder.template.clone(),
            built_in: false,
            replaced: false,
            starter_files: folder.staged.names().into_iter().collect(),
        },
        skipped: folder.skipped.clone(),
        from_project: folder.from_project,
    };
    Ok(Some(out))
}

/// Keep an editor session; one editor at a time, so an older unsaved one is dropped.
fn new_session(state: &AppState, session: EditorSession) -> AppResult<String> {
    let key = format!("editor:{}", uuid::Uuid::new_v4().simple());
    let mut sessions = lock(&state.editor_sessions)?;
    sessions.clear();
    sessions.insert(key.clone(), session);
    Ok(key)
}

/// An open editor: its session key (for preview, save and adding files) and the draft.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorOpened {
    pub key: String,
    pub draft: TemplateDraft,
}

/// Open an installed template in the editor.
#[tauri::command(async)]
pub fn editor_open(state: State<'_, AppState>, key: String) -> AppResult<EditorOpened> {
    let t = find_template(&state, &key)?;
    Ok(EditorOpened {
        draft: template_edit::draft(&t),
        key: new_session(&state, EditorSession::from_template(&t))?,
    })
}

/// Files dropped on (or picked for) a folder in the editor, ready to go into the draft.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddedFiles {
    /// New folders (from a dropped folder), as template paths inside `into`.
    pub folders: Vec<String>,
    /// New file entries: `from` is the staged name, `to` is inside `into`.
    pub files: Vec<FileEntry>,
    pub skipped: Vec<Skipped>,
}

/// Add files (and folders of files) to the editor's draft, into the template folder `into`
/// (a template path; empty = the project folder itself). `paths` are what was dropped; `None`
/// asks with a file picker. Nothing is copied until Save. `None` back if the picker was
/// cancelled.
#[tauri::command(async)]
pub fn editor_add_files(
    window: WebviewWindow,
    state: State<'_, AppState>,
    key: String,
    into: String,
    paths: Option<Vec<String>>,
) -> AppResult<Option<AddedFiles>> {
    let paths: Vec<PathBuf> = match paths {
        Some(p) => p.into_iter().map(PathBuf::from).collect(),
        None => {
            let Some(picked) = window
                .dialog()
                .file()
                .set_parent(&window)
                .set_title("Add files to the template")
                .blocking_pick_files()
            else {
                return Ok(None);
            };
            picked
                .into_iter()
                .filter_map(|p| p.into_path().ok())
                .collect()
        }
    };
    let mut sessions = lock(&state.editor_sessions)?;
    let session = sessions
        .get_mut(&key)
        .ok_or_else(|| AppError::new("The editor was closed. Open the template again."))?;
    let dropped = template_from_folder::read_dropped(
        &paths,
        session.file_names().len(),
        session.staged.bytes(),
    )
    .map_err(AppError::new)?;
    let (folders, files) = session.add_dropped(&into, dropped.files, &dropped.folders);
    Ok(Some(AddedFiles {
        folders,
        files,
        skipped: dropped.skipped,
    }))
}

/// Add a small ready-made text file (e.g. "Project info.txt") to the editor's draft, into the
/// template folder `into`. Rust writes it into Nest's own app folder (`<config>/extras/…`) and
/// stages it like a file you dropped; nothing is copied into the template until Save.
#[tauri::command(async)]
pub fn editor_add_text_file(
    state: State<'_, AppState>,
    key: String,
    into: String,
    name: String,
    contents: String,
) -> AppResult<AddedFiles> {
    if contents.len() > 64 * 1024 {
        return Err(AppError::new("That file is too long to add."));
    }
    let name = crate::names::sanitize_segment(&name).map_err(|e| AppError::new(e.to_string()))?;
    let dir = state
        .config_dir
        .join("extras")
        .join(uuid::Uuid::new_v4().simple().to_string());
    fs::create_dir_all(&dir).map_err(|e| AppError::new(format!("Couldn't add the file: {e}")))?;
    let path = dir.join(&name);
    template_io::write_new(&path, contents.as_bytes())
        .map_err(|e| AppError::new(format!("Couldn't add the file: {e}")))?;
    let mut sessions = lock(&state.editor_sessions)?;
    let session = sessions
        .get_mut(&key)
        .ok_or_else(|| AppError::new("The editor was closed. Open the template again."))?;
    let dropped = template_from_folder::read_dropped(
        &[path],
        session.file_names().len(),
        session.staged.bytes(),
    )
    .map_err(AppError::new)?;
    let (folders, files) = session.add_dropped(&into, dropped.files, &dropped.folders);
    Ok(AddedFiles {
        folders,
        files,
        skipped: dropped.skipped,
    })
}

/// The editor closed without saving: forget its session.
#[tauri::command(async)]
pub fn editor_close(state: State<'_, AppState>, key: String) -> AppResult<()> {
    lock(&state.editor_sessions)?.remove(&key);
    Ok(())
}

/// A saved template, and what happened to its previous copy (decision T-A).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    pub info: TemplateInfo,
    /// Older copies moved to the Recycle Bin / Trash.
    pub binned: usize,
    pub bin_problem: Option<String>,
}

/// Decision T-A, once: on the first start of v0.3.0, extra old copies of your templates go to
/// the bin (newest kept), with one note in the main window. Never during first run.
pub fn tidy_template_copies_once(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let done = lock(&state.settings)
            .map(|s| s.tidied_template_copies)
            .unwrap_or(true);
        if done || state.settings_error.is_some() || state.first_run.load(Ordering::SeqCst) {
            return;
        }
        let Ok(installed) = lock(&state.templates).map(|t| t.clone()) else {
            return;
        };
        let bin = |t: &template_io::Installed| template_io::delete(t, &state.user_templates);
        match template_edit::tidy_all(&installed, &bin) {
            Ok(moved) => {
                if let Err(e) = edit(&app, &state, |s| {
                    s.tidied_template_copies = true;
                    Ok(())
                }) {
                    log::warn!(
                        "template tidy done, but couldn't remember it: {}",
                        e.message
                    );
                }
                if moved > 0 {
                    let _ = templates_changed(&app, &state);
                    if let Ok(mut n) = state.notice.lock() {
                        *n = Some(format!(
                            "Tidied {moved} older template cop{} into the {}. Saving a template now replaces it.",
                            if moved == 1 { "y" } else { "ies" },
                            bin_name()
                        ));
                    }
                    // The main window asks on launch and also listens, in case the tidy
                    // finishes after it has loaded.
                    let _ = app.emit("notice", ());
                }
            }
            Err(e) => log::warn!("template tidy stopped (will try again next start): {e}"),
        }
    });
}

/// The main window's one-off note, if any (shown once).
#[tauri::command(async)]
pub fn take_notice(state: State<'_, AppState>) -> Option<String> {
    state.notice.lock().ok().and_then(|mut n| n.take())
}

#[tauri::command(async)]
pub fn set_template_hidden(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    hidden: bool,
) -> AppResult<SettingsView> {
    edit(&app, &state, |s| {
        s.set_template_hidden(&id, hidden);
        Ok(())
    })
}

#[tauri::command(async)]
pub fn reveal_templates_folder(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    fs::create_dir_all(&state.user_templates).map_err(|e| {
        AppError::new(format!(
            "Couldn't create the templates folder: {}",
            io_reason(&e)
        ))
    })?;
    app.opener()
        .open_path(state.user_templates.to_string_lossy(), None::<&str>)
        .map_err(|e| AppError::new(format!("Couldn't open the folder: {e}")))
}

// ── About ──

#[tauri::command(async)]
pub fn open_logs_folder(app: AppHandle) -> AppResult<()> {
    let dir = app
        .path()
        .app_log_dir()
        .map_err(|e| AppError::new(e.to_string()))?;
    fs::create_dir_all(&dir).map_err(|e| AppError::new(io_reason(&e)))?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| AppError::new(format!("Couldn't open the folder: {e}")))
}

// ── First run ──

/// A folder picker on the window that asked (a sheet on a Mac; in front of, and blocking, its
/// window on Windows).
pub(crate) fn pick_folder(window: &WebviewWindow, title: &str) -> AppResult<Option<PathBuf>> {
    let Some(picked) = window
        .dialog()
        .file()
        .set_parent(window)
        .set_title(title)
        .blocking_pick_folder()
    else {
        return Ok(None);
    };
    let path = picked
        .into_path()
        .map_err(|e| AppError::new(format!("That folder can't be used: {e}")))?;
    if !path.is_dir() {
        return Err(AppError::new(format!(
            "Folder not found: {}",
            path.display()
        )));
    }
    Ok(Some(path))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderInfo {
    pub path: PathBuf,
    /// Nest projects already in it (read-only check).
    pub projects: usize,
}

/// First run: pick the jobs folder (nothing is saved yet).
#[tauri::command(async)]
pub fn first_run_pick_folder(window: WebviewWindow) -> AppResult<Option<FolderInfo>> {
    Ok(pick_folder(&window, "Choose your jobs folder")?.map(folder_info))
}

/// First run: create "Nest Jobs" (or "NestDev" in dev builds) in Documents, one level only.
#[tauri::command(async)]
pub fn first_run_create_folder(app: AppHandle) -> AppResult<FolderInfo> {
    let docs = app
        .path()
        .document_dir()
        .map_err(|e| AppError::new(e.to_string()))?;
    let name = if cfg!(debug_assertions) {
        "NestDev"
    } else {
        "Nest Jobs"
    };
    let path = docs.join(name);
    match fs::create_dir(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && path.is_dir() => {}
        Err(e) => {
            return Err(AppError::new(format!(
                "Couldn't create {}: {}",
                path.display(),
                io_reason(&e)
            )))
        }
    }
    Ok(folder_info(path))
}

fn folder_info(path: PathBuf) -> FolderInfo {
    let projects = scan_root(&path).map(|s| s.codes.len()).unwrap_or(0);
    FolderInfo { path, projects }
}

/// First run: save the choices (this creates the settings file) and open the main window.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)] // one flat command; the UI sends each choice by name
pub fn complete_first_run(
    app: AppHandle,
    state: State<'_, AppState>,
    jobs_root: PathBuf,
    spaces: Vec<String>,
    no_client_spaces: Vec<String>,
    hidden_templates: Vec<String>,
    marker: String,
    first_template: Option<String>,
) -> AppResult<SettingsView> {
    // Only once: afterwards this would wipe the jobs folders and spaces you set up.
    if !state.first_run.load(Ordering::SeqCst) {
        return Err(AppError::new(
            "Nest is already set up. Change things in Settings instead.",
        ));
    }
    if !jobs_root.is_dir() {
        return Err(AppError::new(format!(
            "Folder not found: {}",
            jobs_root.display()
        )));
    }
    let v = edit(&app, &state, |s| {
        if let Some(problem) = s.jobs_root_problem(&jobs_root) {
            return Err(problem);
        }
        let mut fresh = Settings {
            spaces: vec![],
            no_client_spaces: vec![],
            ..s.clone()
        };
        for name in &spaces {
            fresh.add_space(name)?;
        }
        if fresh.spaces.is_empty() {
            return Err("Keep at least one space".into());
        }
        for name in &no_client_spaces {
            if let Some(real) = fresh
                .spaces
                .iter()
                .find(|s| s.eq_ignore_ascii_case(name.trim()))
                .cloned()
            {
                fresh.set_no_client(&real, true)?;
            }
        }
        fresh.default_space = fresh.spaces[0].clone();
        fresh.jobs_roots = vec![jobs_root.clone()];
        fresh.personal_root = None; // starts like other new projects; chosen later in Settings
        fresh.hidden_templates = hidden_templates.clone();
        fresh.local_marker = crate::settings::check_marker(&marker)?;
        // A fresh install has no "what's new": remember this version silently.
        fresh.last_seen_version = Some(env!("CARGO_PKG_VERSION").to_string());
        *s = fresh;
        Ok(())
    })?;
    state.first_run.store(false, Ordering::SeqCst);
    // The folder you copy for every job becomes your first template. This never blocks setup:
    // if it fails, the main window says why.
    if let Some(folder) = first_template.filter(|p| !p.trim().is_empty()) {
        let folder = PathBuf::from(folder);
        if let Err(note) = first_template_from(&app, &state, &folder) {
            if let Ok(mut n) = state.notice.lock() {
                *n = Some(note);
            }
            let _ = app.emit("notice", ());
        }
    }
    // The icon's menu gets New Project now (first run hid it).
    crate::tray::refresh(&app);
    Ok(SettingsView {
        first_run: false,
        ..v
    })
}

fn first_template_from(app: &AppHandle, state: &AppState, folder: &Path) -> Result<(), String> {
    let _one = lock(&state.templates_lock).map_err(|e| failed_note(folder, &e.message))?;
    reload_templates(state).map_err(|e| failed_note(folder, &e.message))?;
    let installed = lock(&state.templates)
        .map_err(|e| failed_note(folder, &e.message))?
        .clone();
    let ex = example(state).map_err(|e| failed_note(folder, &e.message))?;
    let saved = save_first_template(folder, &installed, &state.user_templates, &ex)?;
    crate::template_sync_cmd::note_saved(state, &saved.template.id, saved.template.version, true);
    templates_changed(app, state).map_err(|e| failed_note(folder, &e.message))
}

fn failed_note(folder: &Path, reason: &str) -> String {
    let name = folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| folder.display().to_string());
    format!(
        "Couldn't make a template from \u{201c}{name}\u{201d}: {}. You can try again in Templates.",
        reason.trim_end_matches('.')
    )
}

/// Read a folder (folders and files, never changed) and save it as a new template of yours,
/// named after the folder. The error is the whole note for the main window.
fn save_first_template(
    folder: &Path,
    installed: &[template_io::Installed],
    user_dir: &Path,
    ex: &Example,
) -> Result<crate::template::LoadedTemplate, String> {
    let read = template_from_folder::read_folder(
        folder,
        template_from_folder::Keep::FoldersAndFiles,
        |id| {
            installed
                .iter()
                .filter(|t| t.loaded.template.id == id)
                .max_by_key(|t| (!t.built_in, t.loaded.template.version))
                .map(|t| t.loaded.template.clone())
        },
    )
    .map_err(|e| failed_note(folder, &e))?;
    template_edit::save_from_folder(&read.template, &read, installed, user_dir, BUILT_IN_IDS, ex)
        .map_err(|e| failed_note(folder, &e))
}

/// First run: pick the folder you copy for every job (nothing is read until setup finishes).
#[tauri::command(async)]
pub fn first_run_pick_template_folder(window: WebviewWindow) -> AppResult<Option<String>> {
    Ok(
        pick_folder(&window, "Choose the folder you copy for every job")?
            .map(|p| p.to_string_lossy().into_owned()),
    )
}

/// One settings change from elsewhere in the app (Devices), through the same safe `edit` path.
pub(crate) fn edit_settings(
    app: &AppHandle,
    change: impl FnOnce(&mut Settings) -> Result<(), String>,
) -> AppResult<SettingsView> {
    let state = app.state::<AppState>();
    edit(app, &state, change)
}

/// The Settings page has drawn: show its window (it's created hidden to avoid a blank flash).
#[tauri::command(async)]
pub fn settings_ready(app: AppHandle) -> AppResult<()> {
    if let Some(w) = app.get_webview_window(SETTINGS_WINDOW) {
        if !w.is_visible().unwrap_or(false) {
            w.show()
                .and_then(|_| w.set_focus())
                .map_err(|e| AppError::new(format!("Couldn't show Settings: {e}")))?;
        }
        crate::folder_access::window_shown(&app);
    }
    Ok(())
}

/// Like `edit_settings`, but without telling the windows: for Devices' own bookkeeping
/// (request numbers, dealt-with notes). Telling them would rescan the jobs folders each time.
pub(crate) fn edit_settings_quietly(
    state: &AppState,
    change: impl FnOnce(&mut Settings) -> Result<(), String>,
) -> AppResult<()> {
    if let Some(e) = &state.settings_error {
        return Err(AppError::new(format!(
            "Settings can't be changed while the settings file is damaged. {e}"
        )));
    }
    let mut current = lock(&state.settings)?;
    let mut next = current.clone();
    change(&mut next).map_err(AppError::new)?;
    settings::save(&state.config_dir, &next)?;
    *current = next;
    Ok(())
}

#[cfg(test)]
mod first_template_tests {
    use super::*;
    use crate::template_io::load_installed;

    fn ex() -> Example {
        Example::new(Date {
            year: 2026,
            month: 3,
            day: 2,
        })
    }

    fn bundled() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("templates")
    }

    #[test]
    fn a_folder_becomes_your_first_template_named_after_it() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("Acme Job");
        let user = tmp.path().join("templates");
        fs::create_dir_all(src.join("01_BRIEF")).unwrap();
        fs::write(src.join("01_BRIEF/notes.md"), "hello").unwrap();
        let installed = load_installed(&user, &bundled(), BUILT_IN_IDS);
        let saved = save_first_template(&src, &installed, &user, &ex()).unwrap();
        assert_eq!(saved.template.name, "Acme Job");
        assert!(saved.dir.join("01_BRIEF/notes.md").is_file());
        assert!(
            src.join("01_BRIEF/notes.md").is_file(),
            "your folder is not changed"
        );
    }

    #[test]
    fn a_folder_that_cannot_be_read_gives_a_plain_note() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("templates");
        let installed = load_installed(&user, &bundled(), BUILT_IN_IDS);
        let gone = tmp.path().join("Missing Folder");
        let note = save_first_template(&gone, &installed, &user, &ex()).unwrap_err();
        assert!(note.starts_with("Couldn't make a template from \u{201c}Missing Folder\u{201d}: "));
        assert!(note.ends_with("You can try again in Templates."));
    }
}

#[cfg(test)]
mod system_then_save_tests {
    use super::system_then_save;
    use crate::error::AppError;
    use std::cell::RefCell;

    #[test]
    fn a_failed_save_puts_the_system_back() {
        let log = RefCell::new(vec![]);
        let r: Result<(), AppError> = system_then_save(
            || {
                log.borrow_mut().push("change");
                Ok(())
            },
            || {
                log.borrow_mut().push("save");
                Err(AppError::new("disk full"))
            },
            || log.borrow_mut().push("undo"),
        );
        assert_eq!(r.unwrap_err().message, "disk full");
        assert_eq!(*log.borrow(), ["change", "save", "undo"]);
    }

    #[test]
    fn a_good_save_keeps_the_change_and_a_failed_change_saves_nothing() {
        let log = RefCell::new(vec![]);
        let r = system_then_save(
            || {
                log.borrow_mut().push("change");
                Ok(())
            },
            || {
                log.borrow_mut().push("save");
                Ok(7)
            },
            || log.borrow_mut().push("undo"),
        );
        assert_eq!(r.unwrap(), 7);
        assert_eq!(*log.borrow(), ["change", "save"]);

        log.borrow_mut().clear();
        let r: Result<(), AppError> = system_then_save(
            || {
                log.borrow_mut().push("change");
                Err(AppError::new("not allowed"))
            },
            || {
                log.borrow_mut().push("save");
                Ok(())
            },
            || log.borrow_mut().push("undo"),
        );
        assert_eq!(r.unwrap_err().message, "not allowed");
        assert_eq!(*log.borrow(), ["change"]);
    }
}

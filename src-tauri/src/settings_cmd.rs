//! Commands behind the Settings window and first run (spec §3.3, §3.4, M4).
//!
//! Every change goes through `edit`: it refuses while the settings file is damaged, applies
//! one change in Rust (so two windows never overwrite each other), saves atomically, and
//! tells every window via a `settings-changed` event.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

use chrono::{Datelike, Local};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;

use crate::commands::{lock, reload_templates, AppState};
use crate::error::{bin_name, io_reason, AppError, AppResult};
use crate::index::scan_root;
use crate::plan::Date;
use crate::settings::{self, Settings};
use crate::template::Template;
use crate::template_edit::{self, Example, SaveMode, TemplateDraft, TemplatePreview};
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
}

fn view(state: &AppState) -> AppResult<SettingsView> {
    Ok(SettingsView {
        settings: lock(&state.settings)?.clone(),
        settings_error: state.settings_error.clone(),
        first_run: state.first_run.load(Ordering::SeqCst),
        templates_folder: state.user_templates.clone(),
        dev_build: cfg!(debug_assertions),
        suggested_marker: crate::settings::suggested_marker(),
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
    let v = view(state)?;
    let _ = app.emit(CHANGED_EVENT, ());
    Ok(v)
}

#[tauri::command(async)]
pub fn get_settings(state: State<'_, AppState>) -> AppResult<SettingsView> {
    view(&state)
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
    state: State<'_, AppState>,
) -> AppResult<Option<SettingsView>> {
    let Some(path) = pick_folder(&app, "Add a jobs folder")? else {
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
    state: State<'_, AppState>,
) -> AppResult<Option<SettingsView>> {
    let Some(path) = pick_folder(&app, "Choose the folder for Personal projects")? else {
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
    state: State<'_, AppState>,
) -> AppResult<Option<SettingsView>> {
    let Some(path) = pick_folder(&app, "Choose the archive folder")? else {
        return Ok(None);
    };
    let v = edit(&app, &state, |s| s.set_archive_folder(Some(path)))?;
    forget_roots_not_in(&state, &v)?;
    Ok(Some(v))
}

/// Stop watching the archive folder. Nothing on disk changes; its projects leave the list.
#[tauri::command(async)]
pub fn clear_archive_folder(app: AppHandle, state: State<'_, AppState>) -> AppResult<SettingsView> {
    let v = edit(&app, &state, |s| s.set_archive_folder(None))?;
    forget_roots_not_in(&state, &v)?;
    Ok(v)
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
    if cfg!(debug_assertions) {
        return Err(AppError::new(
            "Launch at login only works in the installed app",
        ));
    }
    let launcher = app.autolaunch();
    let result = if on {
        launcher.enable()
    } else {
        launcher.disable()
    };
    result.map_err(|e| AppError::new(format!("Couldn't change launch at login: {e}")))?;
    edit(&app, &state, |s| {
        s.launch_at_login = on;
        Ok(())
    })
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

fn templates_changed(app: &AppHandle, state: &AppState) -> AppResult<()> {
    reload_templates(state)?;
    let _ = app.emit(CHANGED_EVENT, ());
    Ok(())
}

/// Pick a `.nesttemplate` or `.json` and install it. `None` if the picker was cancelled.
#[tauri::command(async)]
pub fn import_template(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Option<TemplateInfo>> {
    let Some(file) = app
        .dialog()
        .file()
        .set_title("Import a template")
        .add_filter("Nest template", &[PACKAGE_EXT, "json"])
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let file = file.into_path().map_err(|e| AppError::new(e.to_string()))?;
    let installed = lock(&state.templates)?.clone();
    let loaded = template_io::import(&file, &state.user_templates, &installed, LIMITS)
        .map_err(AppError::new)?;
    templates_changed(&app, &state)?;
    // Decision T-A: a newer version of one of yours replaces it; the older copy goes to the bin.
    let installed = lock(&state.templates)?.clone();
    let bin = |t: &template_io::Installed| template_io::delete(t, &state.user_templates);
    match template_edit::retire_older_copies(&loaded.template.id, &loaded.dir, &installed, &bin) {
        Ok(0) => {}
        Ok(_) => templates_changed(&app, &state)?,
        // Added fine, but the older copy stayed: say so (the list already shows the new one).
        Err(e) => {
            templates_changed(&app, &state)?;
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
pub fn export_template(app: AppHandle, state: State<'_, AppState>, key: String) -> AppResult<bool> {
    let t = find_template(&state, &key)?;
    let Some(dest) = app
        .dialog()
        .file()
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
    let t = find_template(&state, &key)?;
    let installed = lock(&state.templates)?.clone();
    template_io::duplicate(&t.loaded, &state.user_templates, &installed).map_err(AppError::new)?;
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
pub fn delete_template(app: AppHandle, state: State<'_, AppState>, key: String) -> AppResult<bool> {
    let t = find_template(&state, &key)?;
    if t.built_in {
        return Err(AppError::new(
            "Built-in templates can't be deleted, only hidden",
        ));
    }
    let installed = lock(&state.templates)?.clone();
    let id = t.loaded.template.id.clone();
    let count = template_edit::your_versions(&id, &installed).len();
    let bin = bin_name();
    let confirmed = app
        .dialog()
        .message(format!(
            "\"{}\"{} will be moved to the {bin}. You can restore it from there. Projects made with it aren't affected.",
            t.loaded.template.name,
            versions_phrase(count)
        ))
        .title(format!("Move template to the {bin}?"))
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(format!("Move to {bin}"), "Cancel".into()))
        .blocking_show();
    if !confirmed {
        return Ok(false);
    }
    let result = template_edit::trash_all_versions(&id, &installed, &state.user_templates);
    // Refresh either way (some versions may have moved), but report the bin's error first.
    let refreshed = templates_changed(&app, &state);
    result.map_err(AppError::new)?;
    refreshed.map(|_| true)
}

/// "Go back to Nest's original": your version of a built-in goes to the Recycle Bin / Trash.
#[tauri::command(async)]
pub fn reset_template(app: AppHandle, state: State<'_, AppState>, key: String) -> AppResult<bool> {
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
    let confirmed = app
        .dialog()
        .message(format!(
            "Your version of \"{}\"{} will be moved to the {bin}, and Nest's original comes back in New Project. You can restore yours from the {bin}. Projects you already made aren't affected.",
            t.loaded.template.name,
            versions_phrase(count)
        ))
        .title("Go back to Nest's original?")
        .kind(MessageDialogKind::Warning)
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
fn example(state: &AppState) -> AppResult<Example> {
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
    let source = find_template(&state, &key)?;
    Ok(template_edit::preview(
        &draft,
        &source.loaded,
        &example(&state)?,
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
    let source = find_template(&state, &key)?;
    let installed = lock(&state.templates)?.clone();
    let saved = template_edit::save(
        &draft,
        mode,
        &source,
        &installed,
        &state.user_templates,
        BUILT_IN_IDS,
        &example(&state)?,
    )
    .map_err(AppError::new)?;
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

fn pick_folder(app: &AppHandle, title: &str) -> AppResult<Option<PathBuf>> {
    let Some(picked) = app.dialog().file().set_title(title).blocking_pick_folder() else {
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
pub fn first_run_pick_folder(app: AppHandle) -> AppResult<Option<FolderInfo>> {
    Ok(pick_folder(&app, "Choose your jobs folder")?.map(folder_info))
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
pub fn complete_first_run(
    app: AppHandle,
    state: State<'_, AppState>,
    jobs_root: PathBuf,
    spaces: Vec<String>,
    no_client_spaces: Vec<String>,
    hidden_templates: Vec<String>,
    marker: String,
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
        *s = fresh;
        Ok(())
    })?;
    state.first_run.store(false, Ordering::SeqCst);
    Ok(SettingsView {
        first_run: false,
        ..v
    })
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

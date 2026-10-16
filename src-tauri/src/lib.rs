pub mod apply;
pub mod archive;
mod commands;
pub mod devices;
mod devices_cmd;
pub mod error;
pub mod index;
pub mod kits;
mod kits_cmd;
pub mod manifest;
mod menu;
pub mod names;
pub mod plan;
pub mod settings;
mod settings_cmd;
pub mod sizes;
pub mod staged;
mod startup;
pub mod template;
pub mod template_edit;
pub mod template_from_folder;
pub mod template_io;
pub mod template_sync;
mod template_sync_cmd;
pub mod tidy;
mod update;
mod whats_new;
mod window;

use std::fs;
use std::sync::Mutex;

use tauri::{Emitter, Manager};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};

use commands::AppState;
use settings::Settings;
use startup::{StartupError, Step};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // If anything below fails, Nest says so in a message box instead of vanishing.
    let started_at = std::time::SystemTime::now();
    let context = tauri::generate_context!();
    if let Some(dir) = startup::fallback_log_dir(&context.config().identifier) {
        startup::set_log_dir(dir);
    }
    startup::install_crash_hook();
    let mut builder = tauri::Builder::default();
    // Installed app only: a second launch focuses the running window and exits. (Dev builds
    // skip it, otherwise an open installed Nest would swallow every test launch.) Must be
    // the first plugin registered.
    if !cfg!(debug_assertions) {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }));
    }
    builder
        // One small, rotated log file per machine (Settings → About → Open logs folder).
        // Never logs tokens, device files or project contents.
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets([
                    Target::new(TargetKind::LogDir { file_name: None }),
                    Target::new(TargetKind::Stdout),
                ])
                .level(log::LevelFilter::Info)
                .max_file_size(1_000_000)
                .rotation_strategy(RotationStrategy::KeepOne)
                .build(),
        )
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(update::Pending::default())
        .manage(kits_cmd::Kits::default())
        .setup(move |app| {
            if let Err(e) = setup(app, started_at) {
                startup::fail(&e);
            }
            startup::started();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::material_applied,
            commands::new_project_context,
            commands::plan_project,
            commands::create_project,
            commands::choose_jobs_root,
            commands::list_projects,
            commands::rescan_projects,
            commands::open_project,
            commands::reveal_project,
            commands::set_status,
            commands::archive_project,
            commands::open_project_subfolder,
            commands::measure_project_folder,
            commands::copy_text,
            commands::pending_update,
            commands::check_for_updates,
            commands::install_update,
            settings_cmd::get_settings,
            settings_cmd::open_settings,
            settings_cmd::add_jobs_root,
            settings_cmd::remove_jobs_root,
            settings_cmd::move_jobs_root,
            settings_cmd::set_default_space,
            settings_cmd::set_launch_at_login,
            settings_cmd::set_personal_root,
            settings_cmd::choose_personal_root,
            settings_cmd::set_archive_folder,
            settings_cmd::use_past_archive_folder,
            settings_cmd::forget_past_archive_folder,
            settings_cmd::clear_archive_folder,
            settings_cmd::set_archive_after_days,
            commands::dismiss_archive_suggestion,
            commands::project_sizes,
            commands::measure_project,
            settings_cmd::set_code_settings,
            settings_cmd::add_space,
            settings_cmd::rename_space,
            settings_cmd::delete_space,
            settings_cmd::move_space,
            settings_cmd::set_space_no_client,
            settings_cmd::spaces_in_use,
            settings_cmd::set_theme,
            settings_cmd::list_templates,
            settings_cmd::import_template,
            settings_cmd::export_template,
            settings_cmd::duplicate_template,
            settings_cmd::delete_template,
            settings_cmd::remove_template,
            settings_cmd::restore_builtin_templates,
            settings_cmd::reset_template,
            settings_cmd::template_draft,
            settings_cmd::preview_template,
            settings_cmd::example_names,
            settings_cmd::save_template,
            settings_cmd::template_from_folder,
            settings_cmd::editor_open,
            settings_cmd::open_templates,
            settings_cmd::editor_add_files,
            settings_cmd::editor_add_text_file,
            settings_cmd::editor_close,
            settings_cmd::set_template_hidden,
            settings_cmd::reveal_templates_folder,
            settings_cmd::open_logs_folder,
            settings_cmd::first_run_pick_folder,
            settings_cmd::first_run_create_folder,
            settings_cmd::first_run_pick_template_folder,
            settings_cmd::complete_first_run,
            settings_cmd::take_notice,
            settings_cmd::settings_ready,
            whats_new::whats_new,
            whats_new::whats_new_seen,
            devices_cmd::devices_view,
            devices_cmd::devices_detect,
            devices_cmd::devices_pick_folder,
            devices_cmd::devices_enable,
            devices_cmd::devices_disable,
            devices_cmd::devices_forget,
            devices_cmd::devices_claim,
            devices_cmd::devices_refresh,
            devices_cmd::devices_set_sync_templates,
            devices_cmd::request_status,
            kits_cmd::kits_view,
            kits_cmd::choose_kits_folder,
            kits_cmd::clear_kits_folder,
            kits_cmd::choose_client_kit,
            kits_cmd::clear_client_kit,
            kits_cmd::set_template_kit_folder,
            kits_cmd::cancel_create,
        ])
        .build(context)
        .unwrap_or_else(|e| startup::fail(&StartupError::new(Step::Other, e)))
        .run(|app, event| {
            // Save this computer's list one last time, so a change made just before quitting
            // reaches the other computers.
            if let tauri::RunEvent::Exit = event {
                devices_cmd::sync_on_quit(app);
            }
        });
}

/// Everything Nest needs before its window shows. An error here becomes the start-up message
/// box (startup.rs); only what truly can't be worked around is an error.
fn setup(app: &mut tauri::App, started_at: std::time::SystemTime) -> Result<(), StartupError> {
    match app.path().app_log_dir() {
        Ok(dir) => startup::set_log_dir(dir),
        Err(e) => log::warn!("couldn't find the log folder: {e}"),
    }
    let state = init_state(app)?;
    let theme = state
        .settings
        .lock()
        .map(|s| s.theme.clone())
        .unwrap_or_default();
    app.manage(state);
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| StartupError::new(Step::Window, "the main window is missing"))?;
    app.manage(window::SettingsMaterial::default());
    app.manage(window::Material {
        applied: window::apply_appearance(&main, &theme),
    });
    update::start(app.handle());
    devices_cmd::start(app.handle());
    settings_cmd::tidy_template_copies_once(app.handle());
    tidy::start(app.handle(), started_at);
    // macOS gets the native menu bar (spec §8). Windows 11 apps don't have one;
    // there every action is on shortcuts, buttons and right-click menus.
    if cfg!(target_os = "macos") {
        let bar = menu::build(app.handle()).map_err(|e| StartupError::new(Step::MenuBar, e))?;
        app.set_menu(bar)
            .map_err(|e| StartupError::new(Step::MenuBar, e))?;
        app.on_menu_event(|app, event| {
            if event.id().0 == menu::SETTINGS {
                if let Err(e) = settings_cmd::open_settings_window(app) {
                    log::warn!("couldn't open Settings: {e}");
                }
            } else if let Err(e) = app.emit("menu", event.id().0.as_str()) {
                log::warn!("couldn't pass on a menu command: {e}");
            }
        });
    }
    Ok(())
}

fn init_state(app: &tauri::App) -> Result<AppState, StartupError> {
    let paths = app.path();
    // Dev builds keep their own settings so they never touch the real ones (spec §10).
    let mut config_dir = paths
        .app_config_dir()
        .map_err(|e| StartupError::new(Step::ConfigFolder, e))?;
    if cfg!(debug_assertions) {
        config_dir = config_dir.join("dev");
    }
    // Nest's own folder (settings, index, your templates). Without it nothing can be saved.
    fs::create_dir_all(&config_dir).map_err(|e| {
        StartupError::new(Step::ConfigFolder, format!("{}: {e}", config_dir.display()))
    })?;

    // No settings file yet → first run (spec §3.4). A damaged file is never first run, and
    // never overwritten.
    let (settings, settings_error, first_run) = match settings::load(&config_dir) {
        Ok(Some(s)) => (s, None, false),
        Ok(None) => (Settings::default(), None, true),
        Err(e) => {
            log::error!("{e}");
            (Settings::default(), Some(e.message), false)
        }
    };

    let user_templates = config_dir.join("templates");
    let bundled_templates = paths
        .resource_dir()
        .map_err(|e| StartupError::new(Step::OwnFiles, e))?
        .join("templates");
    let templates = template_io::load_installed(
        &user_templates,
        &bundled_templates,
        template_io::BUILT_IN_IDS,
    );

    // The project index lives next to the settings (dev and release stay separate). A broken
    // one is kept aside and rebuilt; it never stops Nest from starting.
    let index = index::Index::open(&config_dir.join(index::INDEX_FILE))
        .map_err(|e| StartupError::new(Step::Other, e))?;

    Ok(AppState {
        config_dir,
        templates: Mutex::new(templates),
        user_templates,
        bundled_templates,
        first_run: first_run.into(),
        settings: Mutex::new(settings),
        settings_error,
        scan: Mutex::new(Default::default()),
        creating: Default::default(),
        index: Mutex::new(index),
        scanning: Default::default(),
        notice: Default::default(),
        devices: Default::default(),
        status_lock: Default::default(),
        editor_sessions: Default::default(),
        templates_lock: Default::default(),
    })
}

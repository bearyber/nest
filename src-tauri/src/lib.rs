mod accent;
pub mod app_location;
pub mod apply;
pub mod archive;
mod commands;
pub mod devices;
mod devices_cmd;
pub mod error;
pub mod folder_access;
pub mod index;
pub mod kits;
mod kits_cmd;
pub mod login_item;
pub mod manifest;
mod menu;
pub mod names;
pub mod plan;
pub mod resolve;
mod resolve_cmd;
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
mod tray;
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
            tray::show_main(app);
        }));
    }
    // Windows: open at login through a Run entry that starts Nest with `--hidden` (in the
    // notification area if the icon is on). macOS uses a real Login Item instead (login_item.rs).
    #[cfg(not(target_os = "macos"))]
    {
        builder = builder.plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![tray::HIDDEN_ARG]),
        ));
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
        // Visibility is Nest's own business (tray.rs): a window hidden in the menu bar at quit
        // must not start hidden next time.
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::all()
                        - tauri_plugin_window_state::StateFlags::VISIBLE,
                )
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(update::Pending::default())
        .manage(kits_cmd::Kits::default())
        .manage(tray::Tray::default())
        .manage(resolve_cmd::ResolveState::default())
        .on_window_event(tray::on_window_event)
        .setup(move |app| {
            if let Err(e) = setup(app, started_at) {
                startup::fail(&e);
            }
            startup::started();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::material_applied,
            commands::system_accent,
            commands::set_menu_state,
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
            settings_cmd::set_show_in_menu_bar,
            settings_cmd::set_new_project_open,
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
            settings_cmd::import_template_from,
            settings_cmd::drop_kinds,
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
            whats_new::first_project_tip_seen,
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
            resolve_cmd::resolve_status,
            resolve_cmd::resolve_libraries,
            resolve_cmd::resolve_projects,
            resolve_cmd::resolve_export_to_editor,
            resolve_cmd::editor_add_resolve_drp,
            resolve_cmd::resolve_try_now,
            resolve_cmd::resolve_skip_project,
            resolve_cmd::project_resolve,
        ])
        .build(context)
        .unwrap_or_else(|e| startup::fail(&StartupError::new(Step::Other, e)))
        .run(|app, event| match event {
            // ⌘Q, Quit Nest, the installer: from now on closing the window really closes it.
            tauri::RunEvent::ExitRequested { .. } => tray::quit_requested(app),
            // Save this computer's list one last time, so a change made just before quitting
            // reaches the other computers.
            tauri::RunEvent::Exit => devices_cmd::sync_on_quit(app),
            // macOS: Dock click, Spotlight or Finder while Nest runs hidden: show the window.
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => tray::show_main(app),
            _ => {}
        });
}

/// Everything Nest needs before its window shows. An error here becomes the start-up message
/// box (startup.rs); only what truly can't be worked around is an error.
fn setup(app: &mut tauri::App, started_at: std::time::SystemTime) -> Result<(), StartupError> {
    // macOS opens a Login Item without arguments: AppKit's open event says it was a login
    // launch. Asked first, while that event is still the current one.
    let at_login_launch = login_item::launched_at_login();
    match app.path().app_log_dir() {
        Ok(dir) => startup::set_log_dir(dir),
        Err(e) => log::warn!("couldn't find the log folder: {e}"),
    }
    let state = init_state(app)?;
    let (theme, at_login) = state
        .settings
        .lock()
        .map(|s| (s.theme.clone(), s.launch_at_login))
        .unwrap_or_default();
    // A damaged settings file reads as the defaults: then nobody knows what was chosen.
    let at_login = state.settings_error.is_none().then_some(at_login);
    // Opened at login (`--hidden`) or relaunched by an update while hidden: start in the menu
    // bar / notification area, no window. A restart you asked for ("Restart to update") keeps
    // the arguments but leaves a start-visible note, which wins.
    // (Both notes are read: each is removed either way.)
    let now = std::time::SystemTime::now();
    let hidden_note = tray::take_start_note(&state.config_dir, tray::StartNote::Hidden, now);
    let asked_visible = tray::take_start_note(&state.config_dir, tray::StartNote::Visible, now);
    let asked_hidden =
        std::env::args().any(|a| a == tray::HIDDEN_ARG) || hidden_note || at_login_launch;
    let first_run = state.first_run.load(std::sync::atomic::Ordering::SeqCst);
    let config_dir = state.config_dir.clone();
    app.manage(state);
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| StartupError::new(Step::Window, "the main window is missing"))?;
    app.manage(window::SettingsMaterial::default());
    app.manage(accent::Accent::read_now());
    app.manage(window::Material {
        applied: window::apply_appearance(&main, &theme),
    });
    // The window starts hidden (tauri.conf.json) and shows here, unless Nest starts in the
    // menu bar.
    tray::start(app.handle());
    // Decided by whether the icon is really there: if it couldn't be made, the window shows.
    let icon_up = app.tray_by_id(tray::TRAY_ID).is_some();
    let hidden = tray::start_hidden(icon_up, first_run, asked_hidden, asked_visible);
    // macOS: after a hidden start, reading your folders waits for a window (folder_access.rs).
    folder_access::init(app.handle(), &config_dir, hidden, first_run);
    if hidden {
        log::info!("started in the menu bar / notification area");
        if let Some(t) = app.try_state::<tray::Tray>() {
            t.started_hidden();
        }
        #[cfg(target_os = "macos")]
        app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    } else {
        main.show()
            .map_err(|e| StartupError::new(Step::Window, e))?;
    }
    // Open-at-login entries from older versions: macOS moves to a real Login Item, Windows
    // rewrites its Run entry with `--hidden` (only if still on).
    login_item::migrate(app.handle(), at_login);
    // macOS: running from the disk image or Downloads? Offer to move to Applications.
    app_location::start(app.handle());
    update::start(app.handle());
    devices_cmd::start(app.handle());
    resolve_cmd::start(app.handle());
    settings_cmd::tidy_template_copies_once(app.handle());
    tidy::start(app.handle(), started_at);
    // macOS gets the native menu bar (spec §8). Windows 11 apps don't have one;
    // there every action is on shortcuts, buttons and right-click menus.
    if cfg!(target_os = "macos") {
        let bar = menu::build(app.handle()).map_err(|e| StartupError::new(Step::MenuBar, e))?;
        app.set_menu(bar)
            .map_err(|e| StartupError::new(Step::MenuBar, e))?;
    }
    // Menu clicks from the menu bar (macOS) and the menu bar / notification area icon (both).
    app.on_menu_event(|app, event| {
        let id = event.id().0.as_str();
        if tray::handles(id) {
            tray::on_menu(app, id);
            return;
        }
        let first_run = app
            .try_state::<AppState>()
            .is_some_and(|s| s.first_run.load(std::sync::atomic::Ordering::SeqCst));
        // Each command goes to one window (never broadcast): see menu::route.
        let sent = match menu::route(id, first_run) {
            menu::Route::Settings => {
                if let Err(e) = settings_cmd::open_settings_window(app) {
                    log::warn!("couldn't open Settings: {e}");
                }
                Ok(())
            }
            menu::Route::Main => {
                tray::show_main(app);
                app.emit_to("main", "menu", id)
            }
            menu::Route::ShowMainOnly => {
                tray::show_main(app);
                Ok(())
            }
            menu::Route::Focused => match app
                .webview_windows()
                .into_values()
                .find(|w| w.is_focused().unwrap_or(false))
            {
                Some(w) => app.emit_to(w.label(), "menu", id),
                None => Ok(()),
            },
            menu::Route::Ignore => Ok(()),
        };
        if let Err(e) = sent {
            log::warn!("couldn't pass on a menu command: {e}");
        }
    });
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

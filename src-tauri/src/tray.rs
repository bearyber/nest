//! The menu bar icon (macOS) / notification area icon (Windows), v0.7.
//!
//! With "Show Nest in the menu bar" on (the default), closing the main window hides it and Nest
//! keeps running: Devices and template sync carry on, and the icon's menu offers New Project,
//! recent projects and your other computers. On macOS the Dock icon goes away while no window
//! is showing. ⌘Q / Quit Nest quits for real. With the setting off, closing the window quits
//! like before.
//!
//! The decisions (keep running? install the update now? what's in the menu?) are pure functions
//! with tests; the rest is glue.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use tauri::image::Image;
use tauri::menu::{IsMenuItem, Menu, MenuItem, MenuItemKind, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Window, WindowEvent, Wry};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogResult};

use crate::commands::AppState;

pub const TRAY_ID: &str = "nest-tray";
/// Sent to the main window: open the New Project sheet. Payload: a template id, or null.
pub const OPEN_NEW_PROJECT: &str = "open-new-project";
/// Launch argument: start with the window hidden (open at login, relaunch after an update).
pub const HIDDEN_ARG: &str = "--hidden";
/// Written just before an update installs itself while Nest is hidden: the relaunched Nest
/// starts hidden too (no window flash). Nest's own file in its config folder; read once.
const START_HIDDEN_FILE: &str = "start-hidden";
/// A start-hidden note older than this is ignored (the update didn't relaunch Nest).
const START_HIDDEN_FRESH: Duration = Duration::from_secs(15 * 60);
/// How long Nest must have been hidden and untouched before an update installs by itself.
pub const IDLE_MINUTES_FOR_UPDATE: u64 = 10;
const RECENT_COUNT: usize = 5;
const EVERY: Duration = Duration::from_secs(60);

// Menu item ids. All start with `tray:` so the menu bar handler (lib.rs) can pass them here.
const PREFIX: &str = "tray:";
const ID_NEW: &str = "tray:new";
const ID_NEW_FROM: &str = "tray:new-from:";
const ID_RECENT: &str = "tray:recent:";
const ID_OPEN: &str = "tray:open";
const ID_QUIT: &str = "tray:quit";
const QUIT_INSTEAD: &str = "Quit instead";

/// What the tray remembers while Nest runs.
#[derive(Default)]
pub struct Tray {
    /// Quit was asked for (⌘Q, Quit Nest): the window closes for real.
    quitting: AtomicBool,
    /// When the main window was hidden (None while it shows).
    hidden_since: Mutex<Option<Instant>>,
    /// The last thing done from the tray menu (opening a project folder counts as activity).
    last_activity: Mutex<Option<Instant>>,
    /// The New Project sheet is open in the main window (the UI says so).
    sheet_open: AtomicBool,
    /// The menu last put on the icon, so it's only rebuilt when something changed.
    shown: Mutex<Option<Vec<TrayItem>>>,
    /// An update is installing by itself.
    installing: AtomicBool,
}

impl Tray {
    /// Nest started in the menu bar (no window): it counts as hidden from now.
    pub fn started_hidden(&self) {
        if let Ok(mut since) = self.hidden_since.lock() {
            *since = Some(Instant::now());
        }
    }
}

// ───────────────────────── Pure decisions ─────────────────────────

/// Closing the main window: keep Nest running (hide the window) instead of quitting?
pub fn keep_running(show_in_menu_bar: bool, quit_requested: bool) -> bool {
    show_in_menu_bar && !quit_requested
}

/// Everything the "install the update now?" decision looks at.
#[derive(Debug, Clone, Default)]
pub struct UpdateMoment {
    pub update_waiting: bool,
    pub window_hidden: bool,
    /// Minutes since the window was hidden or the tray menu was last used.
    pub idle_minutes: u64,
    pub sheet_open: bool,
    /// Create is running (this includes copying a client kit).
    pub creating: bool,
    /// Devices / template sync is running (a transfer between computers counts here too).
    pub syncing: bool,
    pub scanning: bool,
}

/// An update installs by itself only when nobody can be interrupted: the window is hidden,
/// nothing has happened for a while and no work is in progress.
pub fn install_update_now(m: &UpdateMoment) -> bool {
    m.update_waiting
        && m.window_hidden
        && m.idle_minutes >= IDLE_MINUTES_FOR_UPDATE
        && !m.sheet_open
        && !m.creating
        && !m.syncing
        && !m.scanning
}

/// One template in "New from ›".
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateChoice {
    pub id: String,
    pub name: String,
}

/// One project that could be under "Recent".
#[derive(Debug, Clone, PartialEq)]
pub struct RecentProject {
    pub key: String,
    pub job_code: String,
    pub title: String,
    pub created_at: String,
    /// Only projects whose folder is on this computer and reachable can be opened.
    pub openable: bool,
}

/// One of your other computers (Devices).
#[derive(Debug, Clone, PartialEq)]
pub struct OtherComputer {
    pub name: String,
    /// "3 min ago", or empty when its list has no time.
    pub seen: String,
}

#[derive(Debug, Clone, Default)]
pub struct TrayInputs {
    pub templates: Vec<TemplateChoice>,
    pub projects: Vec<RecentProject>,
    /// Empty when Devices isn't in use.
    pub others: Vec<OtherComputer>,
    /// Hook for later: "Resolve: N waiting" (Resolve starter projects). Always None for now.
    pub resolve_waiting: Option<usize>,
    /// Windows menus treat `&` as a shortcut marker: double it there.
    pub escape_ampersands: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TrayItem {
    Action {
        id: String,
        label: String,
    },
    /// Greyed text (a heading, or a status line).
    Text(String),
    Submenu {
        label: String,
        items: Vec<TrayItem>,
    },
    Separator,
}

/// What the icon's menu shows, from the templates, projects and computers.
pub fn menu_model(inputs: &TrayInputs) -> Vec<TrayItem> {
    let esc = |s: &str| {
        if inputs.escape_ampersands {
            s.replace('&', "&&")
        } else {
            s.to_string()
        }
    };
    let action = |id: String, label: &str| TrayItem::Action {
        id,
        label: esc(label),
    };
    let mut items = vec![action(ID_NEW.into(), "New Project…")];
    if inputs.templates.len() > 1 {
        items.push(TrayItem::Submenu {
            label: "New from".into(),
            items: inputs
                .templates
                .iter()
                .map(|t| action(format!("{ID_NEW_FROM}{}", t.id), &t.name))
                .collect(),
        });
    }
    let recent = recent(&inputs.projects, RECENT_COUNT);
    if !recent.is_empty() {
        items.push(TrayItem::Separator);
        items.push(TrayItem::Text("Recent".into()));
        for p in recent {
            let label = if p.job_code.is_empty() {
                p.title.clone()
            } else {
                format!("{} {}", p.job_code, p.title)
            };
            items.push(action(format!("{ID_RECENT}{}", p.key), &label));
        }
    }
    let mut status: Vec<TrayItem> = inputs
        .others
        .iter()
        .map(|o| {
            TrayItem::Text(esc(&if o.seen.is_empty() {
                o.name.clone()
            } else {
                format!("{}: seen {}", o.name, o.seen)
            }))
        })
        .collect();
    if let Some(n) = inputs.resolve_waiting.filter(|n| *n > 0) {
        status.push(TrayItem::Text(format!("Resolve: {n} waiting")));
    }
    if !status.is_empty() {
        items.push(TrayItem::Separator);
        items.extend(status);
    }
    items.push(TrayItem::Separator);
    items.push(action(ID_OPEN.into(), "Open Nest"));
    items.push(action(ID_QUIT.into(), "Quit Nest"));
    items
}

/// The newest `n` projects that can be opened here.
pub fn recent(projects: &[RecentProject], n: usize) -> Vec<&RecentProject> {
    let mut list: Vec<&RecentProject> = projects.iter().filter(|p| p.openable).collect();
    list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    list.truncate(n);
    list
}

/// "just now", "3 min ago", "2 h ago", "4 days ago" (same words as the Devices screen).
pub fn ago(iso: &str, now: chrono::DateTime<chrono::Utc>) -> String {
    let Ok(t) = chrono::DateTime::parse_from_rfc3339(iso) else {
        return String::new();
    };
    let min = (now - t.with_timezone(&chrono::Utc)).num_minutes().max(0);
    if min < 1 {
        return "just now".into();
    }
    if min < 60 {
        return format!("{min} min ago");
    }
    let h = min / 60;
    if h < 24 {
        return format!("{h} h ago");
    }
    let d = h / 24;
    if d == 1 {
        "1 day ago".into()
    } else {
        format!("{d} days ago")
    }
}

/// Should this launch start hidden? Only with the icon on, never on first run.
pub fn start_hidden(show_in_menu_bar: bool, first_run: bool, asked: bool) -> bool {
    show_in_menu_bar && !first_run && asked
}

/// Read (and remove) the start-hidden note left by an update. Old notes are ignored.
pub fn take_start_hidden_note(config_dir: &Path, now: SystemTime) -> bool {
    let path = config_dir.join(START_HIDDEN_FILE);
    let Ok(meta) = std::fs::metadata(&path) else {
        return false;
    };
    let fresh = meta
        .modified()
        .ok()
        .map(|m| now.duration_since(m).unwrap_or_default())
        .is_some_and(|age| age <= START_HIDDEN_FRESH);
    if let Err(e) = std::fs::remove_file(&path) {
        log::warn!("couldn't remove the start-hidden note: {e}");
    }
    fresh
}

fn note_path(app: &AppHandle) -> Option<PathBuf> {
    app.try_state::<AppState>()
        .map(|s| s.config_dir.join(START_HIDDEN_FILE))
}

// ───────────────────────── Glue ─────────────────────────

fn tray_on(app: &AppHandle) -> bool {
    app.try_state::<AppState>()
        .and_then(|s| s.settings.lock().ok().map(|s| s.show_in_menu_bar))
        .unwrap_or(false)
}

fn tray_state(app: &AppHandle) -> Option<tauri::State<'_, Tray>> {
    app.try_state::<Tray>()
}

/// Launch: put the icon up (if on) and start the minute timer (menu refresh, idle update).
pub fn start(app: &AppHandle) {
    apply(app);
    for event in [
        crate::settings_cmd::CHANGED_EVENT,
        crate::devices_cmd::CHANGED_EVENT,
    ] {
        let app2 = app.clone();
        // On its own thread: whoever sent the event may still hold a lock the menu needs.
        tauri::Listener::listen_any(app, event, move |_| {
            let app3 = app2.clone();
            std::thread::spawn(move || refresh(&app3));
        });
    }
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(EVERY);
        refresh(&app);
        maybe_install_update(&app);
    });
}

/// Show or remove the icon to match the setting. With the icon gone, a hidden window comes
/// back (otherwise there'd be no way to reach Nest).
pub fn apply(app: &AppHandle) {
    if tray_on(app) {
        if app.tray_by_id(TRAY_ID).is_none() {
            if let Err(e) = build_icon(app) {
                log::warn!("couldn't add the menu bar icon: {e}");
            }
        }
        refresh(app);
    } else {
        if let Some(t) = app.remove_tray_by_id(TRAY_ID) {
            let _ = t.set_visible(false);
        }
        if let Some(t) = tray_state(app) {
            if let Ok(mut shown) = t.shown.lock() {
                *shown = None;
            }
        }
        if main_hidden(app) {
            show_main(app);
        }
    }
}

fn build_icon(app: &AppHandle) -> tauri::Result<()> {
    let menu = Menu::new(app)?;
    let builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Nest")
        .menu(&menu)
        .on_tray_icon_event(|tray, event| {
            // Windows: a click on the icon opens Nest; right-click shows the menu.
            if cfg!(target_os = "windows") {
                let open = matches!(
                    event,
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } | TrayIconEvent::DoubleClick {
                        button: MouseButton::Left,
                        ..
                    }
                );
                if open {
                    show_main(tray.app_handle());
                }
            }
        });
    let builder = if cfg!(target_os = "macos") {
        // A template image: macOS colours it for light / dark menu bars.
        builder
            .icon(Image::from_bytes(include_bytes!(
                "../icons/tray-macos.png"
            ))?)
            .icon_as_template(true)
            .show_menu_on_left_click(true)
    } else {
        builder
            .icon(Image::from_bytes(include_bytes!("../icons/64x64.png"))?)
            .show_menu_on_left_click(false)
    };
    builder.build(app)?;
    if let Some(t) = tray_state(app) {
        if let Ok(mut shown) = t.shown.lock() {
            *shown = None;
        }
    }
    Ok(())
}

/// What the menu should show right now.
fn inputs(app: &AppHandle) -> Option<TrayInputs> {
    let state = app.try_state::<AppState>()?;
    let settings = state.settings.lock().ok()?.clone();
    let templates = crate::commands::offered_templates(&state, &settings)
        .ok()?
        .into_iter()
        .map(|t| TemplateChoice {
            id: t.template.id,
            name: t.template.name,
        })
        .collect();
    let projects = match state.index.lock() {
        Ok(idx) => idx
            .list()
            .unwrap_or_default()
            .into_iter()
            .map(|p| RecentProject {
                openable: p.device.is_none()
                    && !p.offline
                    && !p.archived
                    && p.error.is_none()
                    && !p.paths.is_empty(),
                key: p.key,
                job_code: p.job_code,
                title: p.title,
                created_at: p.created_at,
            })
            .collect(),
        Err(_) => vec![],
    };
    let now = chrono::Utc::now();
    let devices_on = settings.device_id.is_some() && settings.shared_folder.is_some();
    let others = if devices_on {
        state
            .devices
            .runtime
            .lock()
            .map(|rt| {
                rt.others
                    .iter()
                    .map(|d| OtherComputer {
                        name: d.name.clone(),
                        seen: ago(&d.updated_at, now),
                    })
                    .collect()
            })
            .unwrap_or_default()
    } else {
        vec![]
    };
    Some(TrayInputs {
        templates,
        projects,
        others,
        resolve_waiting: None,
        escape_ampersands: cfg!(target_os = "windows"),
    })
}

/// Rebuild the icon's menu if anything in it changed.
pub fn refresh(app: &AppHandle) {
    let Some(icon) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let Some(inputs) = inputs(app) else {
        return;
    };
    let model = menu_model(&inputs);
    if let Some(t) = tray_state(app) {
        let Ok(mut shown) = t.shown.lock() else {
            return;
        };
        if shown.as_ref() == Some(&model) {
            return;
        }
        *shown = Some(model.clone());
    }
    match build_menu(app, &model) {
        Ok(menu) => {
            if let Err(e) = icon.set_menu(Some(menu)) {
                log::warn!("couldn't update the menu bar icon's menu: {e}");
            }
        }
        Err(e) => log::warn!("couldn't build the menu bar icon's menu: {e}"),
    }
}

fn build_items(app: &AppHandle, model: &[TrayItem]) -> tauri::Result<Vec<MenuItemKind<Wry>>> {
    let mut out = Vec::with_capacity(model.len());
    for item in model {
        out.push(match item {
            TrayItem::Action { id, label } => {
                MenuItemKind::MenuItem(MenuItem::with_id(app, id, label, true, None::<&str>)?)
            }
            TrayItem::Text(label) => {
                MenuItemKind::MenuItem(MenuItem::new(app, label, false, None::<&str>)?)
            }
            TrayItem::Separator => MenuItemKind::Predefined(PredefinedMenuItem::separator(app)?),
            TrayItem::Submenu { label, items } => {
                let children = build_items(app, items)?;
                let refs: Vec<&dyn IsMenuItem<Wry>> =
                    children.iter().map(|c| c as &dyn IsMenuItem<Wry>).collect();
                MenuItemKind::Submenu(Submenu::with_items(app, label, true, &refs)?)
            }
        });
    }
    Ok(out)
}

fn build_menu(app: &AppHandle, model: &[TrayItem]) -> tauri::Result<Menu<Wry>> {
    let items = build_items(app, model)?;
    let refs: Vec<&dyn IsMenuItem<Wry>> = items.iter().map(|c| c as &dyn IsMenuItem<Wry>).collect();
    Menu::with_items(app, &refs)
}

/// Is this menu id one of the icon's? (lib.rs passes every menu click here first.)
pub fn handles(id: &str) -> bool {
    id.starts_with(PREFIX)
}

/// A click in the icon's menu.
pub fn on_menu(app: &AppHandle, id: &str) {
    if let Some(t) = tray_state(app) {
        if let Ok(mut last) = t.last_activity.lock() {
            *last = Some(Instant::now());
        }
    }
    if id == ID_NEW {
        new_project(app, None);
    } else if let Some(template) = id.strip_prefix(ID_NEW_FROM) {
        new_project(app, Some(template.to_string()));
    } else if let Some(key) = id.strip_prefix(ID_RECENT) {
        let state = app.state::<AppState>();
        if let Err(e) = crate::commands::open_project(app.clone(), state, key.to_string()) {
            app.dialog()
                .message(e.message)
                .title("Couldn't open the project")
                .kind(tauri_plugin_dialog::MessageDialogKind::Warning)
                .show(|_| {});
        }
    } else if id == ID_OPEN {
        show_main(app);
    } else if id == ID_QUIT {
        quit(app);
    }
}

fn new_project(app: &AppHandle, template: Option<String>) {
    show_main(app);
    if let Err(e) = app.emit_to("main", OPEN_NEW_PROJECT, template) {
        log::warn!("couldn't open New Project: {e}");
    }
}

/// Quit for real (the window's close is no longer turned into a hide).
pub fn quit(app: &AppHandle) {
    if let Some(t) = tray_state(app) {
        t.quitting.store(true, Ordering::SeqCst);
    }
    app.exit(0);
}

/// ⌘Q or any other way of quitting: remember it, so closing windows isn't turned into hiding.
pub fn quit_requested(app: &AppHandle) {
    if let Some(t) = tray_state(app) {
        t.quitting.store(true, Ordering::SeqCst);
    }
}

fn main_hidden(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .is_some_and(|w| !w.is_visible().unwrap_or(true))
}

/// Show and focus the main window (tray, Dock click, a second launch).
pub fn show_main(app: &AppHandle) {
    // An update about to relaunch Nest hidden: the window was asked for, so don't.
    if let Some(path) = note_path(app) {
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
    }
    set_dock(app, true);
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
    if let Some(t) = tray_state(app) {
        if let Ok(mut since) = t.hidden_since.lock() {
            *since = None;
        }
    }
}

fn hide_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
    if let Some(t) = tray_state(app) {
        if let Ok(mut since) = t.hidden_since.lock() {
            *since = Some(Instant::now());
        }
    }
    update_dock(app, None);
    refresh(app);
}

/// macOS: a Dock icon only while a window shows (and always with the menu bar icon off).
/// `closing`: a window on its way out, not counted.
fn update_dock(app: &AppHandle, closing: Option<&str>) {
    let visible = app
        .webview_windows()
        .iter()
        .filter(|(label, _)| Some(label.as_str()) != closing)
        .any(|(_, w)| w.is_visible().unwrap_or(false));
    set_dock(app, visible || !tray_on(app));
}

#[allow(unused_variables)]
fn set_dock(app: &AppHandle, show: bool) {
    #[cfg(target_os = "macos")]
    {
        let policy = if show {
            tauri::ActivationPolicy::Regular
        } else {
            tauri::ActivationPolicy::Accessory
        };
        if let Err(e) = app.set_activation_policy(policy) {
            log::warn!("couldn't change the Dock icon: {e}");
        }
    }
}

/// Window events (lib.rs): closing the main window hides it while the icon is on.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    let app = window.app_handle();
    match event {
        WindowEvent::CloseRequested { api, .. } if window.label() == "main" => {
            let quitting = tray_state(app).is_some_and(|t| t.quitting.load(Ordering::SeqCst));
            if !keep_running(tray_on(app), quitting) {
                return;
            }
            api.prevent_close();
            let note_shown = app
                .try_state::<AppState>()
                .and_then(|s| s.settings.lock().ok().map(|s| s.close_note_shown))
                .unwrap_or(true);
            if note_shown {
                hide_main(app);
            } else {
                still_running_note(app, window);
            }
        }
        WindowEvent::Focused(true) => set_dock(app, true),
        WindowEvent::Destroyed => update_dock(app, Some(window.label())),
        _ => {}
    }
}

/// The first close with the icon on: say where Nest went, once.
fn still_running_note(app: &AppHandle, window: &Window) {
    if let Err(e) = crate::settings_cmd::edit_settings(app, |s| {
        s.close_note_shown = true;
        Ok(())
    }) {
        log::warn!("couldn't remember the still-running note: {}", e.message);
    }
    let text = if cfg!(target_os = "macos") {
        "You'll find it in the menu bar at the top right of your screen."
    } else {
        "Click ^ by the clock to find it. Tip: drag Nest's icon next to the clock."
    };
    let app2 = app.clone();
    app.dialog()
        .message(text)
        .title("Nest is still running")
        .parent(window)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "OK".into(),
            QUIT_INSTEAD.into(),
        ))
        .show_with_result(move |choice| {
            if matches!(&choice, MessageDialogResult::Custom(c) if c == QUIT_INSTEAD) {
                if let Err(e) = crate::settings_cmd::edit_settings(&app2, |s| {
                    s.show_in_menu_bar = false;
                    Ok(())
                }) {
                    log::warn!("couldn't turn the menu bar icon off: {}", e.message);
                }
                quit(&app2);
            } else {
                hide_main(&app2);
            }
        });
}

/// The UI says whether the New Project sheet is open.
pub fn set_sheet_open(app: &AppHandle, open: bool) {
    if let Some(t) = tray_state(app) {
        t.sheet_open.store(open, Ordering::SeqCst);
    }
}

fn minutes_since(i: Option<Instant>) -> Option<u64> {
    i.map(|i| i.elapsed().as_secs() / 60)
}

/// The update waiting, installed now if nobody can be interrupted, then Nest relaunches hidden.
fn maybe_install_update(app: &AppHandle) {
    let (Some(t), Some(state)) = (tray_state(app), app.try_state::<AppState>()) else {
        return;
    };
    if t.installing.load(Ordering::SeqCst) {
        return;
    }
    let hidden = minutes_since(t.hidden_since.lock().ok().and_then(|s| *s));
    let active = minutes_since(t.last_activity.lock().ok().and_then(|s| *s));
    let moment = UpdateMoment {
        update_waiting: crate::update::pending(app).is_some(),
        window_hidden: hidden.is_some() && main_hidden(app),
        idle_minutes: match (hidden, active) {
            (Some(h), Some(a)) => h.min(a),
            (Some(h), None) => h,
            _ => 0,
        },
        sheet_open: t.sheet_open.load(Ordering::SeqCst),
        creating: state.creating.load(Ordering::SeqCst),
        syncing: state.devices.syncing.load(Ordering::SeqCst),
        scanning: state.scanning.load(Ordering::SeqCst),
    };
    if !install_update_now(&moment) {
        return;
    }
    t.installing.store(true, Ordering::SeqCst);
    let note = state.config_dir.join(START_HIDDEN_FILE);
    if let Err(e) = std::fs::write(&note, b"") {
        log::warn!("couldn't leave the start-hidden note: {e}");
    }
    log::info!("installing the update while Nest is hidden");
    if let Err(e) = tauri::async_runtime::block_on(crate::update::install(app)) {
        log::warn!("the update didn't install by itself: {}", e.message);
        let _ = std::fs::remove_file(&note);
        t.installing.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(id: &str) -> TemplateChoice {
        TemplateChoice {
            id: id.into(),
            name: format!("{id} name"),
        }
    }

    fn project(code: &str, created: &str) -> RecentProject {
        RecentProject {
            key: format!("k-{code}"),
            job_code: code.into(),
            title: format!("Title {code}"),
            created_at: created.into(),
            openable: true,
        }
    }

    fn ids(items: &[TrayItem]) -> Vec<String> {
        items
            .iter()
            .map(|i| match i {
                TrayItem::Action { id, .. } => id.clone(),
                TrayItem::Text(t) => format!("text:{t}"),
                TrayItem::Submenu { label, .. } => format!("sub:{label}"),
                TrayItem::Separator => "-".into(),
            })
            .collect()
    }

    #[test]
    fn closing_the_window_keeps_running_only_with_the_icon_on_and_no_quit() {
        assert!(keep_running(true, false));
        assert!(!keep_running(true, true), "⌘Q / Quit Nest quits");
        assert!(!keep_running(false, false), "icon off: closing quits");
        assert!(!keep_running(false, true));
    }

    #[test]
    fn an_update_installs_by_itself_only_when_hidden_idle_and_not_busy() {
        let ready = UpdateMoment {
            update_waiting: true,
            window_hidden: true,
            idle_minutes: IDLE_MINUTES_FOR_UPDATE,
            ..Default::default()
        };
        assert!(install_update_now(&ready));
        let no = |change: fn(&mut UpdateMoment)| {
            let mut m = ready.clone();
            change(&mut m);
            !install_update_now(&m)
        };
        assert!(no(|m| m.update_waiting = false));
        assert!(
            no(|m| m.window_hidden = false),
            "never while the window shows"
        );
        assert!(no(|m| m.idle_minutes = IDLE_MINUTES_FOR_UPDATE - 1));
        assert!(no(|m| m.sheet_open = true), "New Project sheet open");
        assert!(no(|m| m.creating = true), "Create / kit copy running");
        assert!(no(|m| m.syncing = true), "Devices / template sync running");
        assert!(no(|m| m.scanning = true));
    }

    #[test]
    fn the_menu_lists_new_project_templates_recent_and_quit() {
        let inputs = TrayInputs {
            templates: vec![template("a"), template("b")],
            projects: vec![
                project("ACM-L01", "2026-10-01"),
                project("ACM-L02", "2026-10-02"),
            ],
            ..Default::default()
        };
        let model = menu_model(&inputs);
        assert_eq!(
            ids(&model),
            vec![
                "tray:new",
                "sub:New from",
                "-",
                "text:Recent",
                "tray:recent:k-ACM-L02",
                "tray:recent:k-ACM-L01",
                "-",
                "tray:open",
                "tray:quit",
            ]
        );
        let TrayItem::Submenu { items, .. } = &model[1] else {
            panic!("New from is a submenu");
        };
        assert_eq!(ids(items), vec!["tray:new-from:a", "tray:new-from:b"]);
        assert!(
            matches!(&model[4], TrayItem::Action { label, .. } if label == "ACM-L02 Title ACM-L02")
        );
    }

    #[test]
    fn new_from_is_hidden_with_one_template() {
        let one = TrayInputs {
            templates: vec![template("only")],
            ..Default::default()
        };
        assert_eq!(
            ids(&menu_model(&one)),
            vec!["tray:new", "-", "tray:open", "tray:quit"]
        );
        assert_eq!(
            ids(&menu_model(&TrayInputs::default())),
            vec!["tray:new", "-", "tray:open", "tray:quit"]
        );
    }

    #[test]
    fn recent_is_the_newest_five_that_open_here() {
        let mut projects: Vec<RecentProject> = (1..=7)
            .map(|i| project(&format!("C-{i:02}"), &format!("2026-10-{i:02}")))
            .collect();
        projects[6].openable = false; // newest, but on another computer
        let codes: Vec<&str> = recent(&projects, 5)
            .iter()
            .map(|p| p.job_code.as_str())
            .collect();
        assert_eq!(codes, vec!["C-06", "C-05", "C-04", "C-03", "C-02"]);
    }

    #[test]
    fn other_computers_and_resolve_show_as_status_lines() {
        let inputs = TrayInputs {
            others: vec![OtherComputer {
                name: "Studio PC".into(),
                seen: "3 min ago".into(),
            }],
            resolve_waiting: Some(2),
            ..Default::default()
        };
        assert_eq!(
            ids(&menu_model(&inputs)),
            vec![
                "tray:new",
                "-",
                "text:Studio PC: seen 3 min ago",
                "text:Resolve: 2 waiting",
                "-",
                "tray:open",
                "tray:quit",
            ]
        );
    }

    #[test]
    fn names_with_ampersands_survive_windows_menus() {
        let inputs = TrayInputs {
            templates: vec![
                TemplateChoice {
                    id: "x".into(),
                    name: "Grade & Online".into(),
                },
                template("y"),
            ],
            escape_ampersands: true,
            ..Default::default()
        };
        let TrayItem::Submenu { items, .. } = &menu_model(&inputs)[1] else {
            panic!("New from is a submenu");
        };
        assert!(matches!(&items[0], TrayItem::Action { label, .. } if label == "Grade && Online"));
    }

    #[test]
    fn ago_reads_like_the_devices_screen() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-17T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(ago("2026-10-17T11:59:40Z", now), "just now");
        assert_eq!(ago("2026-10-17T11:57:00Z", now), "3 min ago");
        assert_eq!(ago("2026-10-17T09:00:00+00:00", now), "3 h ago");
        assert_eq!(ago("2026-10-16T11:00:00Z", now), "1 day ago");
        assert_eq!(ago("2026-10-13T12:00:00Z", now), "4 days ago");
        assert_eq!(ago("2026-10-17T13:00:00Z", now), "just now", "clock ahead");
        assert_eq!(ago("not a time", now), "");
    }

    #[test]
    fn both_icons_load() {
        let mac = Image::from_bytes(include_bytes!("../icons/tray-macos.png")).unwrap();
        assert_eq!((mac.width(), mac.height()), (44, 44));
        // A template image: black, the shape is in the alpha.
        assert!(mac.rgba().chunks(4).all(|p| p[..3] == [0, 0, 0]));
        assert!(mac.rgba().chunks(4).any(|p| p[3] == 255));
        Image::from_bytes(include_bytes!("../icons/64x64.png")).unwrap();
    }

    #[test]
    fn hidden_start_needs_the_icon_and_never_on_first_run() {
        assert!(start_hidden(true, false, true));
        assert!(!start_hidden(false, false, true));
        assert!(!start_hidden(true, true, true));
        assert!(!start_hidden(true, false, false));
    }

    #[test]
    fn the_start_hidden_note_is_read_once_and_only_while_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let now = SystemTime::now();
        assert!(!take_start_hidden_note(dir.path(), now));
        std::fs::write(dir.path().join(START_HIDDEN_FILE), b"").unwrap();
        assert!(take_start_hidden_note(dir.path(), now));
        assert!(!dir.path().join(START_HIDDEN_FILE).exists(), "read once");
        std::fs::write(dir.path().join(START_HIDDEN_FILE), b"").unwrap();
        let later = now + START_HIDDEN_FRESH + Duration::from_secs(60);
        assert!(!take_start_hidden_note(dir.path(), later), "stale");
        assert!(!dir.path().join(START_HIDDEN_FILE).exists());
    }
}

//! macOS menu bar (spec §8). Menu items with an id are sent to the UI as a `menu` event, to
//! the window `route` picks; the UI runs the matching action. Built on every OS so it always
//! compiles, but only installed on macOS (see lib.rs).
//!
//! The accelerators here must match the macOS keys marked `menu: true` in
//! `src/lib/shortcuts.ts` (the one list of Nest's shortcuts). `src/lib/shortcuts.test.ts`
//! reads this file and fails if they drift apart.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::menu::{
    Menu, MenuBuilder, MenuItemBuilder, MenuItemKind, PredefinedMenuItem, SubmenuBuilder,
};
use tauri::{AppHandle, Manager, Runtime};

use crate::shortcut_keys::{self, Combo};

/// Menu item ids, the same as the shortcut ids in `src/lib/shortcuts.ts`.
pub const NEW_PROJECT: &str = "new-project";
pub const RESCAN: &str = "rescan";
pub const SEARCH: &str = "search";
pub const TOGGLE_SIDEBAR: &str = "toggle-sidebar";
pub const TOGGLE_INSPECTOR: &str = "toggle-inspector";
/// Handled in Rust (opens the Settings window), not sent to the UI.
pub const SETTINGS: &str = "open-settings";
pub const CHECK_UPDATES: &str = "check-updates";
/// Edit → Undo / Redo. Our own items (not the predefined ones, which only reach text fields), so
/// the template editor can step its own history too. The UI decides: in a text box it's the
/// text's undo, otherwise the editor's (see `src/lib/editMenu.ts`).
pub const UNDO: &str = "undo";
pub const REDO: &str = "redo";
/// File → Open / Show in Finder / Copy Path: the selected project, on this computer. Enabled only
/// then (`set_state`).
pub const OPEN_PROJECT: &str = "open-project";
pub const REVEAL_PROJECT: &str = "reveal-project";
pub const COPY_PATH: &str = "copy-path";
/// Our own Close Window and Quit items (not the predefined ones, whose keys can't be changed).
/// Handled in Rust: close the window you're in / quit for real.
pub const CLOSE_WINDOW: &str = "close-window";
pub const QUIT: &str = "quit";
/// Help → Keyboard Shortcuts: opens Settings on the Shortcuts tab (Rust does it).
pub const KEYBOARD_SHORTCUTS: &str = "keyboard-shortcuts";

/// While Settings → Shortcuts is recording a new key, the menu bar is empty so no menu key
/// (⌘N, ⌘Q…) runs; the page gets the key press instead.
static SUSPENDED: AtomicBool = AtomicBool::new(false);
/// What `set_state` last said, so a rebuilt menu says the same.
static LAST_STATE: Mutex<(bool, bool)> = Mutex::new((true, false));

/// `custom`: the keys changed in Settings → Shortcuts; they replace the default accelerator
/// written at each item below.
pub fn build<R: Runtime>(
    app: &AppHandle<R>,
    custom: &BTreeMap<String, Combo>,
) -> tauri::Result<Menu<R>> {
    let accel_of = |id: &str, default: Option<&str>| -> Option<String> {
        custom
            .get(id)
            .map(shortcut_keys::accelerator)
            .or_else(|| default.map(str::to_string))
    };
    let item = |id: &str, label: &str, accel: Option<&str>| {
        let b = MenuItemBuilder::with_id(id, label);
        match accel_of(id, accel) {
            Some(a) => b.accelerator(a).build(app),
            None => b.build(app),
        }
    };

    // Items that need a selected project start off; the page turns them on (`set_state`).
    let disabled = |id: &str, label: &str, accel: Option<&str>| {
        let b = MenuItemBuilder::with_id(id, label).enabled(false);
        match accel_of(id, accel) {
            Some(a) => b.accelerator(a).build(app),
            None => b.build(app),
        }
    };

    let app_menu = SubmenuBuilder::new(app, "Nest")
        .about(None)
        .item(&item(CHECK_UPDATES, "Check for Updates…", None)?)
        .separator()
        .item(&item(SETTINGS, "Settings…", Some("CmdOrCtrl+,"))?)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .item(&item(QUIT, "Quit Nest", Some("CmdOrCtrl+Q"))?)
        .build()?;

    let file = SubmenuBuilder::new(app, "File")
        .item(&item(NEW_PROJECT, "New Project…", Some("CmdOrCtrl+N"))?)
        .separator()
        .item(&disabled(OPEN_PROJECT, "Open", Some("CmdOrCtrl+O"))?)
        .item(&disabled(REVEAL_PROJECT, "Show in Finder", None)?)
        .item(&disabled(COPY_PATH, "Copy Path", Some("CmdOrCtrl+Alt+C"))?)
        .separator()
        .item(&item(RESCAN, "Refresh List", Some("CmdOrCtrl+R"))?)
        .separator()
        .item(&item(CLOSE_WINDOW, "Close Window", Some("CmdOrCtrl+W"))?)
        .build()?;

    // The standard Edit items keep Cmd+C / Cmd+V working in text fields on macOS.
    let edit = SubmenuBuilder::new(app, "Edit")
        .item(&item(UNDO, "Undo", Some("CmdOrCtrl+Z"))?)
        .item(&item(REDO, "Redo", Some("CmdOrCtrl+Shift+Z"))?)
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .separator()
        // ⌘F, as on every Mac. ⌘K also focuses search (a hidden extra, handled in the page).
        .item(&item(SEARCH, "Find…", Some("CmdOrCtrl+F"))?)
        .build()?;

    let view = SubmenuBuilder::new(app, "View")
        // "Show Sidebar" when it's hidden (`set_state`). ⌘B also works (a hidden extra).
        .item(&item(
            TOGGLE_SIDEBAR,
            "Hide Sidebar",
            Some("Ctrl+CmdOrCtrl+S"),
        )?)
        .item(&item(
            TOGGLE_INSPECTOR,
            "Toggle Details",
            Some("CmdOrCtrl+Alt+I"),
        )?)
        .separator()
        .fullscreen()
        .build()?;

    // The standard Window menu (macOS also lists the open windows in it), and the Help menu
    // (macOS adds its search box to it).
    let window = SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize()
        .separator()
        .item(&PredefinedMenuItem::bring_all_to_front(app, None)?)
        .build()?;
    let help = SubmenuBuilder::new(app, "Help")
        .item(&item(KEYBOARD_SHORTCUTS, "Keyboard Shortcuts", None)?)
        .build()?;
    #[cfg(target_os = "macos")]
    {
        window.set_as_windows_menu_for_nsapp()?;
        help.set_as_help_menu_for_nsapp()?;
    }

    MenuBuilder::new(app)
        .items(&[&app_menu, &file, &edit, &view, &window, &help])
        .build()
}

/// Make the menu say what's true now: "Hide Sidebar" / "Show Sidebar", and Open / Show in Finder /
/// Copy Path on only while a project on this computer is selected. Called by the page when either
/// changes. No menu bar (Windows): nothing to do.
pub fn set_state<R: Runtime>(
    app: &AppHandle<R>,
    sidebar_shown: bool,
    project_selected: bool,
) -> tauri::Result<()> {
    if let Ok(mut last) = LAST_STATE.lock() {
        *last = (sidebar_shown, project_selected);
    }
    let Some(menu) = app.menu() else {
        return Ok(());
    };
    for top in menu.items()? {
        let MenuItemKind::Submenu(sub) = top else {
            continue;
        };
        for entry in sub.items()? {
            let MenuItemKind::MenuItem(it) = entry else {
                continue;
            };
            match it.id().as_ref() {
                TOGGLE_SIDEBAR => it.set_text(if sidebar_shown {
                    "Hide Sidebar"
                } else {
                    "Show Sidebar"
                })?,
                OPEN_PROJECT | REVEAL_PROJECT | COPY_PATH => it.set_enabled(project_selected)?,
                _ => {}
            }
        }
    }
    Ok(())
}

/// Install the menu bar again with the keys now in settings (macOS; a no-op elsewhere, and while
/// a new key is being recorded: it is installed when that ends).
pub fn rebuild(app: &AppHandle) -> tauri::Result<()> {
    if !cfg!(target_os = "macos") || SUSPENDED.load(Ordering::SeqCst) {
        return Ok(());
    }
    let custom = app
        .try_state::<crate::commands::AppState>()
        .and_then(|s| s.settings.lock().ok().map(|s| s.shortcuts.clone()))
        .unwrap_or_default();
    app.set_menu(build(app, &custom)?)?;
    let (sidebar, selected) = LAST_STATE.lock().map(|l| *l).unwrap_or((true, false));
    set_state(app, sidebar, selected)
}

/// Settings → Shortcuts is recording a key (`true`): empty the menu bar so its keys don't run.
/// Recording ended (`false`): put it back.
pub fn suspend(app: &AppHandle, on: bool) -> tauri::Result<()> {
    if !cfg!(target_os = "macos") {
        return Ok(());
    }
    let was = SUSPENDED.swap(on, Ordering::SeqCst);
    if on && !was {
        app.set_menu(Menu::new(app)?)?;
    } else if !on && was {
        rebuild(app)?;
    }
    Ok(())
}

/// Where a menu bar command goes (lib.rs acts on it).
#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    /// Opens the Settings window (Rust does it).
    Settings,
    /// Opens the Settings window on the Shortcuts tab.
    SettingsShortcuts,
    /// Closes the window you're in (Close Window).
    CloseWindow,
    /// Quits for real.
    Quit,
    /// The main window's commands: bring the main window forward (shown, focused), then send
    /// the command to it alone, so ⌘N with Settings in front or Nest hidden opens New Project
    /// where you can see it.
    Main,
    /// First run: the main window shows setup, which has none of these. Just bring it forward.
    ShowMainOnly,
    /// Undo / Redo: the window you're working in.
    Focused,
    /// Not ours (the icon's items go to tray.rs before this).
    Ignore,
}

/// Pure: which window a menu bar command is for.
pub fn route(id: &str, first_run: bool) -> Route {
    match id {
        SETTINGS => Route::Settings,
        KEYBOARD_SHORTCUTS => Route::SettingsShortcuts,
        CLOSE_WINDOW => Route::CloseWindow,
        QUIT => Route::Quit,
        UNDO | REDO => Route::Focused,
        NEW_PROJECT | RESCAN | SEARCH | TOGGLE_SIDEBAR | TOGGLE_INSPECTOR | CHECK_UPDATES
        | OPEN_PROJECT | REVEAL_PROJECT | COPY_PATH => {
            if first_run {
                Route::ShowMainOnly
            } else {
                Route::Main
            }
        }
        _ => Route::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_window_commands_go_to_the_main_window_only() {
        for id in [
            NEW_PROJECT,
            RESCAN,
            SEARCH,
            TOGGLE_SIDEBAR,
            TOGGLE_INSPECTOR,
            CHECK_UPDATES,
            OPEN_PROJECT,
            REVEAL_PROJECT,
            COPY_PATH,
        ] {
            assert_eq!(route(id, false), Route::Main, "{id}");
            // During first run there's no list or New Project yet: only show the window.
            assert_eq!(route(id, true), Route::ShowMainOnly, "{id}");
        }
    }

    #[test]
    fn undo_redo_go_to_the_focused_window_and_settings_opens_its_window() {
        assert_eq!(route(UNDO, false), Route::Focused);
        assert_eq!(route(REDO, true), Route::Focused);
        assert_eq!(route(SETTINGS, false), Route::Settings);
        assert_eq!(route(SETTINGS, true), Route::Settings);
        assert_eq!(route(KEYBOARD_SHORTCUTS, true), Route::SettingsShortcuts);
        assert_eq!(route(CLOSE_WINDOW, false), Route::CloseWindow);
        assert_eq!(route(QUIT, false), Route::Quit);
        assert_eq!(route("something-else", false), Route::Ignore);
    }
}

//! macOS menu bar (spec §8). Menu items with an id are sent to the UI as a `menu` event, to
//! the window `route` picks; the UI runs the matching action. Built on every OS so it always
//! compiles, but only installed on macOS (see lib.rs).
//!
//! The accelerators here must match the macOS keys marked `menu: true` in
//! `src/lib/shortcuts.ts` (the one list of Nest's shortcuts). `src/lib/shortcuts.test.ts`
//! reads this file and fails if they drift apart.

use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::{AppHandle, Runtime};

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

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let item = |id: &str, label: &str, accel: Option<&str>| {
        let b = MenuItemBuilder::with_id(id, label);
        match accel {
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
        .quit()
        .build()?;

    let file = SubmenuBuilder::new(app, "File")
        .item(&item(NEW_PROJECT, "New Project…", Some("CmdOrCtrl+N"))?)
        .separator()
        .item(&item(RESCAN, "Refresh List", Some("CmdOrCtrl+R"))?)
        .separator()
        .item(&PredefinedMenuItem::close_window(app, None)?)
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
        .item(&item(
            TOGGLE_SIDEBAR,
            "Toggle Sidebar",
            Some("CmdOrCtrl+B"),
        )?)
        .item(&item(
            TOGGLE_INSPECTOR,
            "Toggle Details",
            Some("CmdOrCtrl+Alt+I"),
        )?)
        .build()?;

    let window = SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize()
        .build()?;

    MenuBuilder::new(app)
        .items(&[&app_menu, &file, &edit, &view, &window])
        .build()
}

/// Where a menu bar command goes (lib.rs acts on it).
#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    /// Opens the Settings window (Rust does it).
    Settings,
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
        UNDO | REDO => Route::Focused,
        NEW_PROJECT | RESCAN | SEARCH | TOGGLE_SIDEBAR | TOGGLE_INSPECTOR | CHECK_UPDATES => {
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
        assert_eq!(route("something-else", false), Route::Ignore);
    }
}

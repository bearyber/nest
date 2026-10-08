//! macOS menu bar (spec §8). Menu items with an id are sent to the UI as a `menu` event;
//! the UI runs the matching action. Built on every OS so it always compiles, but only
//! installed on macOS (see lib.rs).

use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::{AppHandle, Runtime};

/// Menu item ids, shared with `src/lib/shortcuts.ts`.
pub const NEW_PROJECT: &str = "new-project";
pub const RESCAN: &str = "rescan";
pub const SEARCH: &str = "search";
pub const TOGGLE_SIDEBAR: &str = "toggle-sidebar";
pub const TOGGLE_INSPECTOR: &str = "toggle-inspector";
/// Handled in Rust (opens the Settings window), not sent to the UI.
pub const SETTINGS: &str = "open-settings";
pub const CHECK_UPDATES: &str = "check-updates";

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
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .separator()
        .item(&item(SEARCH, "Find…", Some("CmdOrCtrl+K"))?)
        .build()?;

    let view = SubmenuBuilder::new(app, "View")
        .item(&item(TOGGLE_SIDEBAR, "Toggle Sidebar", None)?)
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

//! macOS asks once before an app reads Documents, Desktop, Downloads, other drives, network
//! drives or a cloud folder. If Nest starts in the menu bar at login and syncs straight away,
//! that question pops up with no Nest window anywhere: confusing, and easy to answer wrong.
//!
//! The rule: on a Mac, after a hidden start, the background work that reads your folders
//! (devices sync, template sync, rescan, start-up tidy, the Resolve waiting list) waits until
//! a Nest window has been shown, unless macOS has already been asked on this computer. "Asked"
//! is a small note in Nest's settings folder, left the first time a window shows. macOS
//! remembers the answer (allow or not) from then on, so a later hidden start can't bring the
//! question up again. Folders added later come from a folder picker, which grants access by
//! itself. Windows doesn't ask: nothing waits there.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

/// The note: macOS has had its chance to ask with a Nest window showing.
pub const NOTE: &str = "folder-access-asked";

/// Is the background folder work open yet?
#[derive(Default)]
pub struct FolderAccess {
    open: AtomicBool,
}

/// Must folder work wait for a window? Pure.
pub fn must_wait(mac: bool, started_hidden: bool, asked_before: bool) -> bool {
    mac && started_hidden && !asked_before
}

/// Launch: open now, or wait for the first window.
pub fn init(app: &AppHandle, config_dir: &Path, started_hidden: bool) {
    let asked = config_dir.join(NOTE).exists();
    let wait = must_wait(cfg!(target_os = "macos"), started_hidden, asked);
    if wait {
        log::info!("folder work waits until a Nest window shows");
    } else if cfg!(target_os = "macos") && !started_hidden && !asked {
        // The window shows: any question comes up with it.
        leave_note(config_dir);
    }
    app.manage(FolderAccess {
        open: AtomicBool::new(!wait),
    });
}

fn leave_note(config_dir: &Path) {
    if let Err(e) = std::fs::write(config_dir.join(NOTE), b"") {
        log::warn!("couldn't leave the folder-access note: {e}");
    }
}

/// May Nest read your folders now?
pub fn is_open(app: &AppHandle) -> bool {
    app.try_state::<FolderAccess>()
        .is_some_and(|f| f.open.load(Ordering::SeqCst))
}

/// Block this (background) thread until folder work may start.
pub fn wait(app: &AppHandle) {
    while !is_open(app) {
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// A Nest window shows (or got focus). The first time after a hidden start: leave the note,
/// open the gate, and catch up (rescan via `settings-changed`, then a devices sync).
pub fn window_shown(app: &AppHandle) {
    let Some(f) = app.try_state::<FolderAccess>() else {
        return;
    };
    if f.open.swap(true, Ordering::SeqCst) {
        return;
    }
    log::info!("a window shows: folder work starts");
    if let Some(state) = app.try_state::<crate::commands::AppState>() {
        leave_note(&state.config_dir);
    }
    let _ = app.emit(crate::settings_cmd::CHANGED_EVENT, ());
    crate::devices_cmd::sync_soon(app);
}

#[cfg(test)]
mod tests {
    use super::must_wait;

    #[test]
    fn only_a_hidden_mac_start_never_asked_before_waits() {
        assert!(must_wait(true, true, false));
        // Asked before (with a window): macOS remembers the answer.
        assert!(!must_wait(true, true, true));
        // A normal start shows the window at once.
        assert!(!must_wait(true, false, false));
        // Windows never asks.
        assert!(!must_wait(false, true, false));
        assert!(!must_wait(false, false, false));
    }
}

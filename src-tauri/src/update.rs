//! Auto-update (spec §11), release builds only: check on launch and every 24 h against
//! `latest.json` on the public nest-releases repo. Updates are signature-checked with the
//! public key in tauri.conf.json before anything is installed. The only network call Nest
//! makes (CLAUDE.md #9).
//!
//! An update is kept until it has installed: a download or install that fails leaves it
//! waiting, so "Restart to update" still works. While one installs, Create and the syncs don't
//! start (`installing`): Nest is about to restart.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::error::{AppError, AppResult};

const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// What Create says while an update installs.
pub const UPDATING: &str = "Nest is updating. Try again in a moment.";

/// The update found, waiting for "Restart to update" (or a quiet moment, tray.rs).
#[derive(Default)]
pub struct Pending {
    update: Mutex<Option<Update>>,
    /// Downloaded and signature-checked, for this version.
    bytes: Mutex<Option<(String, Vec<u8>)>>,
    /// An update is installing: nothing new starts (see `holds_back`).
    installing: AtomicBool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub notes: Option<String>,
}

/// Start the background checks (never in dev builds).
pub fn start(app: &AppHandle) {
    if cfg!(debug_assertions) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || loop {
        if let Err(e) = tauri::async_runtime::block_on(check(&app)) {
            // Offline or GitHub unreachable: stay quiet, try again next time.
            log::info!("update check failed: {e}");
        }
        std::thread::sleep(CHECK_EVERY);
    });
}

/// Ask nest-releases for a newer version. If there is one, keep it and tell the UI.
async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    let found = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    let Some(update) = found else {
        return Ok(None);
    };
    let info = info(&update);
    if let Ok(mut slot) = app.state::<Pending>().update.lock() {
        *slot = Some(update);
    }
    let _ = app.emit("update-available", info.clone());
    Ok(Some(info))
}

/// "Check for updates" pressed: `None` means already on the newest version.
pub async fn check_now(app: &AppHandle) -> AppResult<Option<UpdateInfo>> {
    if cfg!(debug_assertions) {
        return Err(AppError::new(
            "Updates only work in the installed app, not the test version",
        ));
    }
    check(app).await.map_err(|e| {
        log::warn!("update check failed: {e}");
        AppError::new("Couldn't check for updates. Are you online?")
    })
}

fn info(update: &Update) -> UpdateInfo {
    UpdateInfo {
        version: update.version.clone(),
        notes: update.body.clone(),
    }
}

/// The update found so far, if any (the UI asks on start, in case it missed the event).
pub fn pending(app: &AppHandle) -> Option<UpdateInfo> {
    app.state::<Pending>()
        .update
        .lock()
        .ok()?
        .as_ref()
        .map(info)
}

/// An update is installing right now.
pub fn installing(app: &AppHandle) -> bool {
    app.try_state::<Pending>()
        .is_some_and(|p| p.installing.load(Ordering::SeqCst))
}

/// Should a Devices / template sync wait? Yes while an update installs, except the last one
/// on the way out (quitting), which saves this computer's list.
pub fn holds_back_sync(installing: bool, quitting: bool) -> bool {
    installing && !quitting
}

/// Close the gate: from now on Create and the syncs don't start. False if already closed.
/// (Set this first, then look at what's running: whatever started before is seen.)
pub fn begin_install(app: &AppHandle) -> bool {
    !app.state::<Pending>()
        .installing
        .swap(true, Ordering::SeqCst)
}

/// Open the gate again (the install didn't happen).
pub fn end_install(app: &AppHandle) {
    app.state::<Pending>()
        .installing
        .store(false, Ordering::SeqCst);
}

fn waiting(app: &AppHandle) -> AppResult<Update> {
    app.state::<Pending>()
        .update
        .lock()
        .map_err(|_| AppError::new("The update isn't available any more"))?
        .clone()
        .ok_or_else(|| AppError::new("There's no update waiting"))
}

/// Download and verify the waiting update now (Nest keeps working meanwhile); kept for the
/// install. Nothing to do if this version is downloaded already.
pub async fn download(app: &AppHandle) -> AppResult<()> {
    let update = waiting(app)?;
    let pending = app.state::<Pending>();
    let have = pending
        .bytes
        .lock()
        .map(|b| b.as_ref().is_some_and(|(v, _)| *v == update.version))
        .unwrap_or(false);
    if have {
        return Ok(());
    }
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|e| AppError::new(format!("The update couldn't be downloaded: {e}")))?;
    if let Ok(mut slot) = pending.bytes.lock() {
        *slot = Some((update.version.clone(), bytes));
    }
    Ok(())
}

/// Install the downloaded update and restart. On Windows the installer closes Nest itself.
/// Call with the gate closed (`begin_install`). If it fails, the update stays waiting (the
/// download is dropped, the next try fetches it again).
pub fn install_downloaded(app: &AppHandle) -> AppResult<()> {
    let update = waiting(app)?;
    let pending = app.state::<Pending>();
    let bytes = pending
        .bytes
        .lock()
        .map_err(|_| AppError::new("The update isn't available any more"))?
        .take()
        .filter(|(v, _)| *v == update.version)
        .map(|(_, b)| b)
        .ok_or_else(|| AppError::new("The update isn't downloaded yet"))?;
    update
        .install(bytes)
        .map_err(|e| AppError::new(format!("The update couldn't be installed: {e}")))?;
    app.restart();
}

/// "Restart to update": download, verify, install, restart. Nest comes back with its window
/// showing, even if it was opened at login in the menu bar.
pub async fn install(app: &AppHandle) -> AppResult<()> {
    if !begin_install(app) {
        return Err(AppError::new("Nest is already updating"));
    }
    let note = crate::tray::leave_start_note(app, crate::tray::StartNote::Visible);
    let result = match download(app).await {
        Ok(()) => install_downloaded(app),
        Err(e) => Err(e),
    };
    // Only reached when it didn't work.
    if let Some(note) = note {
        let _ = std::fs::remove_file(note);
    }
    end_install(app);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syncs_wait_while_an_update_installs_except_the_last_one_on_quit() {
        assert!(!holds_back_sync(false, false));
        assert!(holds_back_sync(true, false));
        assert!(
            !holds_back_sync(true, true),
            "the quit sync still saves the list"
        );
        assert!(!holds_back_sync(false, true));
    }
}

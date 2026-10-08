//! Auto-update (spec §11), release builds only: check on launch and every 24 h against
//! `latest.json` on the public nest-releases repo. Updates are signature-checked with the
//! public key in tauri.conf.json before anything is installed. The only network call Nest
//! makes (CLAUDE.md #9).

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::error::{AppError, AppResult};

const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// An update that has been found and is waiting for "Restart to update".
#[derive(Default)]
pub struct Pending(pub Mutex<Option<Update>>);

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
    if let Ok(mut slot) = app.state::<Pending>().0.lock() {
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
    app.state::<Pending>().0.lock().ok()?.as_ref().map(info)
}

/// Download, verify, install, restart. On Windows the installer closes Nest itself.
pub async fn install(app: &AppHandle) -> AppResult<()> {
    let update = app
        .state::<Pending>()
        .0
        .lock()
        .map_err(|_| AppError::new("The update isn't available any more"))?
        .take()
        .ok_or_else(|| AppError::new("There's no update waiting"))?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| AppError::new(format!("The update couldn't be installed: {e}")))?;
    app.restart();
}

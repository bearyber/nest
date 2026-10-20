//! "Open Nest when you log in". The checkbox shows what the system really has, not what was
//! last clicked.
//!
//! macOS: a real Login Item through SMAppService.mainApp (macOS 13+): System Settings →
//! General → Login Items lists "Nest" with its icon. The system starts it without arguments,
//! so a login launch is recognised by the open event AppKit hands Nest (`launched_at_login`).
//! Versions before 0.7.2 wrote a LaunchAgent file instead; it's replaced once (`migrate`).
//!
//! Windows: the autostart plugin's Run entry, with `--hidden`.

use std::path::Path;

use serde::Serialize;
use tauri::AppHandle;

use crate::error::{AppError, AppResult};

/// What Settings shows for opening at login.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LoginItem {
    Off,
    On,
    /// macOS: registered, but switched off (or not yet allowed) in System Settings.
    NeedsApproval,
    /// macOS: Nest runs from the disk image or Downloads; move it to Applications first.
    MoveFirst,
    /// Dev builds never touch the real login items.
    DevBuild,
}

/// SMAppService's status, as plain values (so the mapping is testable on any OS).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemStatus {
    NotRegistered,
    Enabled,
    RequiresApproval,
    NotFound,
}

/// The system's answer → what the checkbox shows. Pure.
pub fn from_status(status: SystemStatus) -> LoginItem {
    match status {
        SystemStatus::Enabled => LoginItem::On,
        SystemStatus::RequiresApproval => LoginItem::NeedsApproval,
        SystemStatus::NotRegistered | SystemStatus::NotFound => LoginItem::Off,
    }
}

/// What Settings shows, all things considered. Pure. A login item that's on stays shown as on
/// even from a misplaced copy (so it can be turned off).
pub fn shown(real: LoginItem, dev_build: bool, misplaced: bool) -> LoginItem {
    if dev_build {
        LoginItem::DevBuild
    } else if misplaced && real == LoginItem::Off {
        LoginItem::MoveFirst
    } else {
        real
    }
}

/// Is this the LaunchAgent file Nest's old autostart wrote (`~/Library/LaunchAgents/Nest.plist`,
/// label "Nest", starting Nest.app)? Only then may `migrate` remove it. Pure.
pub fn is_old_launch_agent(contents: &str) -> bool {
    let tight: String = contents.split_whitespace().collect();
    tight.contains("<key>Label</key><string>Nest</string>")
        && tight.contains(".app/Contents/MacOS/")
}

/// One step of moving an old-style open-at-login file over to a real Login Item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Migrate {
    /// Add the Login Item first; the old file goes only once that worked.
    Register,
    /// Remove the old file (it was off, or the Login Item is in place now).
    RemoveOld,
    /// Leave the old file: it still opens Nest at login, so what you chose keeps working.
    /// Tried again at the next start.
    KeepOld,
}

/// First step. `wanted`: None = settings unreadable (damaged), so nothing is touched. Pure.
pub fn migrate_first(wanted: Option<bool>, misplaced: bool) -> Migrate {
    match wanted {
        None => Migrate::KeepOld,
        Some(false) => Migrate::RemoveOld,
        // From the disk image or a translocated copy a Login Item can't be added (yet).
        Some(true) if misplaced => Migrate::KeepOld,
        Some(true) => Migrate::Register,
    }
}

/// After registering: the old file goes only once the system has the Login Item (on, or
/// waiting for your OK in System Settings). `None`: registering failed. Pure.
pub fn migrate_after(status: Option<SystemStatus>) -> Migrate {
    match status {
        Some(SystemStatus::Enabled | SystemStatus::RequiresApproval) => Migrate::RemoveOld,
        _ => Migrate::KeepOld,
    }
}

/// The state Settings shows now.
pub fn state(app: &AppHandle) -> LoginItem {
    let misplaced = crate::app_location::misplaced_now().is_some();
    if cfg!(debug_assertions) {
        return LoginItem::DevBuild;
    }
    shown(system_state(app), false, misplaced)
}

/// Turn opening at login on or off, and return the state the system now reports.
pub fn set(app: &AppHandle, on: bool) -> AppResult<LoginItem> {
    if cfg!(debug_assertions) {
        return Err(AppError::new(
            "Launch at login only works in the installed app",
        ));
    }
    if on && crate::app_location::misplaced_now().is_some() {
        return Err(AppError::new(
            "Move Nest to your Applications folder first, then turn this on.",
        ));
    }
    set_system(app, on)
        .map_err(|e| AppError::new(format!("Couldn't change launch at login: {e}")))?;
    Ok(state(app))
}

/// Put opening at login back to `on` after a change that couldn't be saved (settings_cmd):
/// quietly, never opening System Settings. Best effort.
pub fn restore(app: &AppHandle, on: bool) {
    if cfg!(debug_assertions) {
        return;
    }
    if let Err(e) = set_system_quietly(app, on) {
        log::warn!("couldn't put open at login back: {e}");
    }
}

/// Is opening at login registered with the system (on, or waiting for your OK)?
pub fn registered(item: LoginItem) -> bool {
    matches!(item, LoginItem::On | LoginItem::NeedsApproval)
}

/// The old LaunchAgent file's place (macOS).
pub fn old_launch_agent(home: &Path) -> std::path::PathBuf {
    home.join("Library").join("LaunchAgents").join("Nest.plist")
}

// ───────────────────────── macOS ─────────────────────────

#[cfg(target_os = "macos")]
fn service() -> smappservice_rs::AppService {
    smappservice_rs::AppService::new(smappservice_rs::ServiceType::MainApp)
}

#[cfg(target_os = "macos")]
fn system_status() -> SystemStatus {
    use smappservice_rs::ServiceStatus as S;
    match service().status() {
        S::NotRegistered => SystemStatus::NotRegistered,
        S::Enabled => SystemStatus::Enabled,
        S::RequiresApproval => SystemStatus::RequiresApproval,
        S::NotFound => SystemStatus::NotFound,
    }
}

#[cfg(target_os = "macos")]
fn system_state(_app: &AppHandle) -> LoginItem {
    from_status(system_status())
}

/// Register or unregister, nothing else (never opens System Settings).
#[cfg(target_os = "macos")]
fn set_system_quietly(app: &AppHandle, on: bool) -> Result<(), String> {
    let s = service();
    if on {
        s.register().map_err(|e| e.to_string())
    } else if system_state(app) == LoginItem::Off {
        Ok(())
    } else {
        s.unregister().map_err(|e| e.to_string())
    }
}

/// The Settings checkbox: like `set_system_quietly`, and if macOS wants your OK first, open
/// the page where it's given. Only from the checkbox, never at start.
#[cfg(target_os = "macos")]
fn set_system(app: &AppHandle, on: bool) -> Result<(), String> {
    set_system_quietly(app, on)?;
    if on && system_state(app) == LoginItem::NeedsApproval {
        smappservice_rs::AppService::open_system_settings_login_items();
    }
    Ok(())
}

/// Once, at launch (release builds): replace the LaunchAgent file older versions wrote with a
/// real Login Item. Nest's own file only. If it was on, the Login Item is added FIRST and the
/// old file removed only once macOS has it (on, or waiting for your OK), so opening at login
/// never quietly stops. Never opens System Settings (a launch isn't the moment to ask);
/// Settings shows "allow in System Settings" instead. `launch_at_login` stays as it was: it
/// keeps matching what opens Nest at login (the Login Item, or the old file still there).
#[cfg(target_os = "macos")]
pub fn migrate(app: &AppHandle, wanted: Option<bool>) {
    if cfg!(debug_assertions) {
        return;
    }
    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
        return;
    };
    let file = old_launch_agent(&home);
    let Ok(contents) = std::fs::read_to_string(&file) else {
        return;
    };
    if !is_old_launch_agent(&contents) {
        log::warn!("{} isn't Nest's: left alone", file.display());
        return;
    }
    let misplaced = crate::app_location::misplaced_now().is_some();
    let mut step = migrate_first(wanted, misplaced);
    if step == Migrate::Register {
        let status = match set_system_quietly(app, true) {
            Ok(()) => Some(system_status()),
            Err(e) => {
                log::warn!("couldn't add Nest to Login Items: {e}");
                None
            }
        };
        step = migrate_after(status);
    }
    match step {
        Migrate::RemoveOld => match std::fs::remove_file(&file) {
            Ok(()) => log::info!("removed the old open-at-login file"),
            Err(e) => log::warn!("couldn't remove the old open-at-login file: {e}"),
        },
        Migrate::KeepOld => {
            log::info!("kept the old open-at-login file for now (tried again next start)")
        }
        Migrate::Register => {}
    }
}

/// Was this launch the system opening Nest at login? AppKit's open event says so
/// (`keyAELaunchedAsLogInItem`). Asked once, during launch, on the main thread.
#[cfg(target_os = "macos")]
pub fn launched_at_login() -> bool {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    const fn code(c: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*c)
    }
    // SAFETY: plain Foundation calls on the main thread; every object is checked for nil.
    unsafe {
        let manager: *mut AnyObject =
            msg_send![class!(NSAppleEventManager), sharedAppleEventManager];
        if manager.is_null() {
            return false;
        }
        let event: *mut AnyObject = msg_send![manager, currentAppleEvent];
        if event.is_null() {
            return false;
        }
        let id: u32 = msg_send![event, eventID];
        if id != code(b"oapp") {
            return false;
        }
        let prop: *mut AnyObject = msg_send![event, paramDescriptorForKeyword: code(b"prdt")];
        if prop.is_null() {
            return false;
        }
        let value: u32 = msg_send![prop, enumCodeValue];
        value == code(b"lgit")
    }
}

// ───────────────────────── Windows ─────────────────────────

#[cfg(not(target_os = "macos"))]
fn system_state(app: &AppHandle) -> LoginItem {
    use tauri_plugin_autostart::ManagerExt;
    // The Run entry really there (and not switched off in Task Manager → Startup apps).
    match app.autolaunch().is_enabled() {
        Ok(true) => LoginItem::On,
        Ok(false) => LoginItem::Off,
        Err(e) => {
            log::warn!("couldn't read open at login: {e}");
            LoginItem::Off
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn set_system(app: &AppHandle, on: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    let done = if on {
        launcher.enable()
    } else {
        launcher.disable()
    };
    done.map_err(|e| e.to_string())
}

#[cfg(not(target_os = "macos"))]
fn set_system_quietly(app: &AppHandle, on: bool) -> Result<(), String> {
    set_system(app, on)
}

/// Launch (release builds): Run entries made before v0.7 open without `--hidden`; write them
/// again, only if still on (so one turned off in Task Manager stays off).
#[cfg(not(target_os = "macos"))]
pub fn migrate(app: &AppHandle, wanted: Option<bool>) {
    if wanted != Some(true) || cfg!(debug_assertions) {
        return;
    }
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    if launcher.is_enabled().unwrap_or(false) {
        if let Err(e) = launcher.enable() {
            log::warn!("couldn't refresh open at login: {e}");
        }
    }
}

/// Windows: a login launch carries `--hidden` (see lib.rs), nothing else to ask.
#[cfg(not(target_os = "macos"))]
pub fn launched_at_login() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_status_maps_to_the_checkbox() {
        assert_eq!(from_status(SystemStatus::Enabled), LoginItem::On);
        assert_eq!(
            from_status(SystemStatus::RequiresApproval),
            LoginItem::NeedsApproval
        );
        assert_eq!(from_status(SystemStatus::NotRegistered), LoginItem::Off);
        assert_eq!(from_status(SystemStatus::NotFound), LoginItem::Off);
    }

    #[test]
    fn dev_builds_and_misplaced_copies() {
        assert_eq!(shown(LoginItem::On, true, false), LoginItem::DevBuild);
        assert_eq!(shown(LoginItem::Off, false, true), LoginItem::MoveFirst);
        // On stays on (so it can be turned off), approval stays visible.
        assert_eq!(shown(LoginItem::On, false, true), LoginItem::On);
        assert_eq!(
            shown(LoginItem::NeedsApproval, false, true),
            LoginItem::NeedsApproval
        );
        assert_eq!(shown(LoginItem::Off, false, false), LoginItem::Off);
    }

    #[test]
    fn migrate_adds_the_login_item_before_removing_the_old_file() {
        // On: register first, never remove straight away.
        assert_eq!(migrate_first(Some(true), false), Migrate::Register);
        // The old file goes only once macOS has the Login Item.
        assert_eq!(
            migrate_after(Some(SystemStatus::Enabled)),
            Migrate::RemoveOld
        );
        assert_eq!(
            migrate_after(Some(SystemStatus::RequiresApproval)),
            Migrate::RemoveOld
        );
        // Not there after registering, or registering failed: the old file keeps working.
        assert_eq!(
            migrate_after(Some(SystemStatus::NotRegistered)),
            Migrate::KeepOld
        );
        assert_eq!(
            migrate_after(Some(SystemStatus::NotFound)),
            Migrate::KeepOld
        );
        assert_eq!(migrate_after(None), Migrate::KeepOld);
        // From the disk image: can't register yet, keep the old file.
        assert_eq!(migrate_first(Some(true), true), Migrate::KeepOld);
        // Off: nothing to register, the old file goes (settings already say off).
        assert_eq!(migrate_first(Some(false), false), Migrate::RemoveOld);
        assert_eq!(migrate_first(Some(false), true), Migrate::RemoveOld);
        // Settings unreadable: don't guess, touch nothing.
        assert_eq!(migrate_first(None, false), Migrate::KeepOld);
    }

    #[test]
    fn registered_means_on_or_waiting() {
        assert!(registered(LoginItem::On));
        assert!(registered(LoginItem::NeedsApproval));
        assert!(!registered(LoginItem::Off));
        assert!(!registered(LoginItem::MoveFirst));
        assert!(!registered(LoginItem::DevBuild));
    }

    #[test]
    fn only_nests_own_launch_agent_is_removed() {
        let ours = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>Label</key><string>Nest</string>
<key>ProgramArguments</key><array>
<string>/Applications/Nest.app/Contents/MacOS/nest</string><string>--hidden</string>
</array><key>RunAtLoad</key><true/></dict></plist>"#;
        assert!(is_old_launch_agent(ours));
        let someone_elses =
            ours.replace("<string>Nest</string>", "<string>NestThermostat</string>");
        assert!(!is_old_launch_agent(&someone_elses));
        assert!(!is_old_launch_agent(
            "<key>Label</key><string>Nest</string>"
        ));
        // Label something else, Nest mentioned elsewhere.
        let other_label = ours.replace(
            "<key>Label</key><string>Nest</string>",
            "<key>Label</key><string>Other</string><key>X</key><string>Nest</string>",
        );
        assert!(!is_old_launch_agent(&other_label));
        assert!(!is_old_launch_agent(""));
        assert_eq!(
            old_launch_agent(Path::new("/Users/버나드")),
            Path::new("/Users/버나드/Library/LaunchAgents/Nest.plist")
        );
    }

    /// Outside a login launch (a test has no open event) the answer is no, and asking is safe.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_test_run_is_not_a_login_launch() {
        assert!(!launched_at_login());
    }

    #[test]
    fn states_reach_the_ui_in_camel_case() {
        let json = |s: LoginItem| serde_json::to_string(&s).unwrap();
        assert_eq!(json(LoginItem::NeedsApproval), "\"needsApproval\"");
        assert_eq!(json(LoginItem::MoveFirst), "\"moveFirst\"");
        assert_eq!(json(LoginItem::DevBuild), "\"devBuild\"");
        assert_eq!(json(LoginItem::On), "\"on\"");
    }
}

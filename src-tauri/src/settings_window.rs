//! The Settings window's life: built hidden a few seconds after launch, shown when asked,
//! hidden (not destroyed) when closed, so it opens at once every time.
//!
//! "Wanted" = someone asked for Settings and hasn't closed it. "Ready" = its page has drawn.
//! A window that isn't wanted never shows (the pre-warmed one waits hidden); one that's
//! wanted shows the moment it is ready (`settings_ready`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::commands::AppState;
use crate::settings_cmd::{CHANGED_EVENT, SETTINGS_WINDOW};
use crate::window;

/// Page event: forget what was open (tab, messages, dialogs, scroll) and start fresh.
pub const RESET_EVENT: &str = "settings-reset";
const TAB_EVENT: &str = "settings-tab";
/// How long after launch the hidden window is built (the main window comes first).
const PREWARM_DELAY: Duration = Duration::from_secs(4);
/// If the page never says it's ready, show a wanted window anyway.
const READY_FALLBACK: Duration = Duration::from_millis(1500);

#[derive(Default)]
pub struct SettingsWin {
    wanted: AtomicBool,
    ready: AtomicBool,
    /// A tab asked for before the page was ready (the event would have been missed).
    pending_tab: Mutex<Option<String>>,
    asked_at: Mutex<Option<Instant>>,
}

/// Tabs a request may name; anything else opens as usual.
pub fn known_tab(tab: Option<&str>) -> Option<&str> {
    tab.filter(|t| matches!(*t, "shortcuts" | "codes"))
}

/// Show the window now? Only when someone asked and the page has drawn. Pure.
pub fn show_now(wanted: bool, ready: bool) -> bool {
    wanted && ready
}

/// Closing Settings hides it, unless Nest is quitting, or nothing else keeps Nest alive (no
/// menu bar / notification area icon and no main window: hiding would leave an invisible app). Pure.
pub fn close_hides(quitting: bool, icon_up: bool, main_exists: bool) -> bool {
    !quitting && (icon_up || main_exists)
}

/// Pre-warm only when it can't get in the way: not during first run. Pure.
pub fn should_prewarm(first_run: bool) -> bool {
    !first_run
}

fn state(app: &AppHandle) -> Option<tauri::State<'_, SettingsWin>> {
    app.try_state::<SettingsWin>()
}

fn build(app: &AppHandle, tab: Option<&str>) -> tauri::Result<WebviewWindow> {
    let mut builder = WebviewWindowBuilder::new(
        app,
        SETTINGS_WINDOW,
        WebviewUrl::App(match tab {
            Some(t) => format!("index.html?tab={t}").into(),
            None => "index.html".into(),
        }),
    )
    .title("Nest Settings")
    .inner_size(860.0, 600.0)
    .min_inner_size(660.0, 460.0)
    .transparent(true)
    .center()
    // No webview zoom (Ctrl+wheel, Ctrl+= / Ctrl+-), as in the main window's config.
    .zoom_hotkeys_enabled(false)
    // Hidden until its page has drawn and someone asked (see `ready`), so it appears in one go
    // instead of flashing an empty frame.
    .visible(false);
    // Dev builds keep their webview data apart from the installed app (see tauri.dev.conf.json):
    // on Windows two webviews with different settings can't share one data folder.
    if cfg!(debug_assertions) {
        builder = builder.data_directory(
            app.path()
                .app_local_data_dir()?
                .join("dev-webview")
                .join(SETTINGS_WINDOW),
        );
    }
    let w = builder.build()?;
    if let Some(s) = state(app) {
        s.ready.store(false, Ordering::SeqCst);
    }
    // Hidden, closed, or left (another window, the About panel…) while a new key was being
    // recorded: the menu bar's keys come back.
    let closing = app.clone();
    w.on_window_event(move |e| {
        if matches!(
            e,
            tauri::WindowEvent::Destroyed | tauri::WindowEvent::Focused(false)
        ) {
            let _ = crate::menu::suspend(&closing, false);
        }
    });
    let theme = app
        .state::<AppState>()
        .settings
        .lock()
        .map(|s| s.theme.clone())
        .unwrap_or_default();
    let app2 = app.clone();
    window::apply_appearance_on_main(app, &w, &theme, move |ok| {
        if let Some(m) = app2.try_state::<window::SettingsMaterial>() {
            m.0.store(ok, Ordering::SeqCst);
        }
    });
    Ok(w)
}

/// Build the hidden window a few seconds after launch (off the main thread, like the
/// `async` commands: building a window from a synchronous command deadlocks on Windows).
pub fn prewarm(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(PREWARM_DELAY);
        let first_run = app
            .try_state::<AppState>()
            .is_some_and(|s| s.first_run.load(Ordering::SeqCst));
        if !should_prewarm(first_run) || app.get_webview_window(SETTINGS_WINDOW).is_some() {
            return;
        }
        let t = Instant::now();
        match build(&app, None) {
            Ok(_) => log::info!(
                "Settings window pre-warmed ({} ms)",
                t.elapsed().as_millis()
            ),
            Err(e) => log::warn!("couldn't pre-warm Settings: {e}"),
        }
    });
}

fn reveal(app: &AppHandle, w: &WebviewWindow) -> tauri::Result<()> {
    w.unminimize()?;
    w.show()?;
    // A Nest window shows: folder work may start (folder_access.rs), even if it never gets focus.
    crate::folder_access::window_shown(app);
    if let Some(s) = state(app) {
        if let Some(t) = s.asked_at.lock().ok().and_then(|mut a| a.take()) {
            log::info!(
                "Settings shown {} ms after it was asked for",
                t.elapsed().as_millis()
            );
        }
    }
    w.set_focus()
}

/// Open (or focus) Settings, on a tab: "shortcuts" (Help → Keyboard Shortcuts, F1) or "codes"
/// (the note about two computers sharing a letter). Anything else opens as usual.
pub fn open(app: &AppHandle, tab: Option<&str>) -> tauri::Result<()> {
    let tab = known_tab(tab);
    let Some(st) = state(app) else {
        return Ok(());
    };
    if let Ok(mut a) = st.asked_at.lock() {
        a.get_or_insert_with(Instant::now);
    }
    st.wanted.store(true, Ordering::SeqCst);
    let ready = st.ready.load(Ordering::SeqCst);
    if let Some(w) = app.get_webview_window(SETTINGS_WINDOW) {
        if show_now(true, ready) {
            // The page is up: fresh settings, the asked-for tab, and no waiting.
            let _ = app.emit_to(SETTINGS_WINDOW, CHANGED_EVENT, ());
            if let Some(t) = tab {
                let _ = app.emit_to(SETTINGS_WINDOW, TAB_EVENT, t);
            }
            return reveal(app, &w);
        }
        // Not drawn yet (still loading): `ready()` shows it, with the tab.
        if let (Some(t), Ok(mut p)) = (tab, st.pending_tab.lock()) {
            *p = Some(t.to_string());
        }
        fallback(&w);
        return Ok(());
    }
    let w = build(app, tab)?;
    fallback(&w);
    Ok(())
}

/// Safety net: a wanted window whose page never says it's ready shows anyway.
fn fallback(w: &WebviewWindow) {
    let w = w.clone();
    std::thread::spawn(move || {
        std::thread::sleep(READY_FALLBACK);
        let app = w.app_handle();
        let wanted = state(app).is_some_and(|s| s.wanted.load(Ordering::SeqCst));
        if wanted && !w.is_visible().unwrap_or(true) {
            let _ = reveal(app, &w);
        }
    });
}

/// The page has drawn (`settings_ready`): show it if it was asked for; a pre-warmed window
/// stays hidden.
pub fn ready(app: &AppHandle) -> tauri::Result<()> {
    // A fresh page (first load or reload) is never recording: give the menu bar its keys back.
    let _ = crate::menu::suspend(app, false);
    let Some(st) = state(app) else {
        return Ok(());
    };
    st.ready.store(true, Ordering::SeqCst);
    let Some(w) = app.get_webview_window(SETTINGS_WINDOW) else {
        return Ok(());
    };
    if !show_now(st.wanted.load(Ordering::SeqCst), true) {
        return Ok(());
    }
    if let Some(t) = st.pending_tab.lock().ok().and_then(|mut p| p.take()) {
        let _ = app.emit_to(SETTINGS_WINDOW, TAB_EVENT, t);
    }
    if !w.is_visible().unwrap_or(false) {
        reveal(app, &w)?;
    }
    Ok(())
}

/// Close = hide. The page starts fresh next time (first tab, no messages, nothing being
/// recorded) and the menu bar's keys are back.
pub fn hide(app: &AppHandle) {
    if let Some(st) = state(app) {
        st.wanted.store(false, Ordering::SeqCst);
        if let Ok(mut p) = st.pending_tab.lock() {
            *p = None;
        }
    }
    if let Some(w) = app.get_webview_window(SETTINGS_WINDOW) {
        let _ = w.hide();
    }
    let _ = crate::menu::suspend(app, false);
    let _ = app.emit_to(SETTINGS_WINDOW, RESET_EVENT, ());
}

/// The main window really closed (no icon to keep Nest running): a hidden Settings window
/// must not keep an invisible Nest alive.
pub fn destroy_if_hidden(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(SETTINGS_WINDOW) {
        if !w.is_visible().unwrap_or(false) {
            let _ = w.destroy();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pre_warmed_window_stays_hidden_until_asked() {
        assert!(!show_now(false, true));
        assert!(!show_now(true, false));
        assert!(show_now(true, true));
    }

    #[test]
    fn closing_hides_unless_quitting_or_nothing_else_keeps_nest_alive() {
        assert!(close_hides(false, true, true));
        assert!(close_hides(false, true, false));
        assert!(close_hides(false, false, true));
        assert!(!close_hides(true, true, true));
        assert!(!close_hides(false, false, false));
    }

    #[test]
    fn no_pre_warm_during_first_run() {
        assert!(should_prewarm(false));
        assert!(!should_prewarm(true));
    }

    #[test]
    fn only_known_tabs_are_passed_on() {
        assert_eq!(known_tab(Some("shortcuts")), Some("shortcuts"));
        assert_eq!(known_tab(Some("codes")), Some("codes"));
        assert_eq!(known_tab(Some("about")), None);
        assert_eq!(known_tab(None), None);
    }
}

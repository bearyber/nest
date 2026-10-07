//! Window material (Mica on Windows 11, Sidebar vibrancy on macOS) and appearance.
//! If the OS can't apply the material (e.g. Windows 10), the UI paints a solid background.

use tauri::{Theme, WebviewWindow};

pub struct Material {
    pub applied: bool,
}

/// "light" | "dark" | anything else = follow the system.
fn theme_of(theme: &str) -> Option<Theme> {
    match theme {
        "light" => Some(Theme::Light),
        "dark" => Some(Theme::Dark),
        _ => None,
    }
}

/// Set the window's appearance and re-apply the material so it matches. Returns whether the
/// material is active.
pub fn apply_appearance(window: &WebviewWindow, theme: &str) -> bool {
    let theme = theme_of(theme);
    if let Err(e) = window.set_theme(theme) {
        log::warn!("couldn't set the window theme: {e}");
    }
    apply_material(window, theme)
}

#[cfg(target_os = "windows")]
fn apply_material(window: &WebviewWindow, theme: Option<Theme>) -> bool {
    // `None` = follow the system light/dark theme.
    let dark = theme.map(|t| t == Theme::Dark);
    window_vibrancy::apply_mica(window, dark).is_ok()
}

#[cfg(target_os = "macos")]
fn apply_material(window: &WebviewWindow, _theme: Option<Theme>) -> bool {
    // Vibrancy follows the window's appearance, which `set_theme` already changed.
    use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};
    apply_vibrancy(window, NSVisualEffectMaterial::Sidebar, None, None).is_ok()
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn apply_material(_window: &WebviewWindow, _theme: Option<Theme>) -> bool {
    false
}

/// Whether the Settings window's material is active (set from the main thread, read by its page).
#[derive(Default)]
pub struct SettingsMaterial(pub std::sync::atomic::AtomicBool);

/// `apply_appearance`, always on the main thread: macOS only applies vibrancy there, and
/// commands run on background threads (a silent no-op left Settings fully see-through).
/// `done` gets whether the material is active.
pub fn apply_appearance_on_main(
    app: &tauri::AppHandle,
    window: &WebviewWindow,
    theme: &str,
    done: impl FnOnce(bool) + Send + 'static,
) {
    let (w, theme) = (window.clone(), theme.to_string());
    if let Err(e) = app.run_on_main_thread(move || done(apply_appearance(&w, &theme))) {
        log::warn!("couldn't set the window appearance: {e}");
    }
}

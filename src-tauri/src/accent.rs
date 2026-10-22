//! The system accent colour (locked #7: native feel). Read from the OS, handed to every window
//! (command `system_accent` at start, `accent-changed` event after), and turned into Nest's
//! accent shades by `src/lib/accent.ts`. Read on the main thread only: it's where the OS wants it
//! (WinRT needs the window thread's COM setup), and window events and `setup` already run there.

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};

/// Sent to every window with the new colour (`"#rrggbb"`, or null if the OS gives none).
pub const EVENT: &str = "accent-changed";

/// The colour last read; a change is what gets sent.
pub struct Accent(Mutex<Option<String>>);

impl Accent {
    /// Reads the OS accent now (main thread).
    pub fn read_now() -> Self {
        Accent(Mutex::new(read()))
    }

    pub fn get(&self) -> Option<String> {
        self.0.lock().ok().and_then(|g| g.clone())
    }
}

/// `"#rrggbb"` from 0-255 channels.
pub fn hex(r: u8, g: u8, b: u8) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Same, from 0.0-1.0 channels (what AppKit gives).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn hex_unit(r: f64, g: f64, b: f64) -> String {
    let c = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    hex(c(r), c(g), c(b))
}

/// Read it again; tell every window if it changed. Called whenever a Nest window gains focus:
/// the accent is changed in System Settings / Windows Settings, so Nest isn't in front then.
pub fn refresh(app: &AppHandle) {
    let Some(state) = app.try_state::<Accent>() else {
        return;
    };
    let now = read();
    let changed = match state.0.lock() {
        Ok(mut g) if *g != now => {
            *g = now.clone();
            true
        }
        _ => false,
    };
    if changed {
        if let Err(e) = app.emit(EVENT, now) {
            log::warn!("couldn't pass on the new accent colour: {e}");
        }
    }
}

#[cfg(target_os = "macos")]
fn read() -> Option<String> {
    use objc2_app_kit::{NSColor, NSColorSpace};
    // The accent is a "dynamic" colour: convert it to plain sRGB to get numbers.
    let color =
        NSColor::controlAccentColor().colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
    let (mut r, mut g, mut b, mut a) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
    // SAFETY: four valid pointers to f64 (CGFloat on the 64-bit Macs Nest runs on).
    unsafe { color.getRed_green_blue_alpha(&mut r, &mut g, &mut b, &mut a) };
    Some(hex_unit(r, g, b))
}

#[cfg(target_os = "windows")]
fn read() -> Option<String> {
    use windows::UI::ViewManagement::{UIColorType, UISettings};
    // The colour chosen in Settings → Personalisation → Colours.
    let c = UISettings::new()
        .ok()?
        .GetColorValue(UIColorType::Accent)
        .ok()?;
    Some(hex(c.R, c.G, c.B))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn read() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_is_lower_case_and_padded() {
        assert_eq!(hex(10, 132, 255), "#0a84ff");
        assert_eq!(hex(0, 0, 0), "#000000");
    }

    #[test]
    fn unit_channels_round_and_clamp() {
        assert_eq!(hex_unit(0.0, 0.5, 1.0), "#0080ff");
        assert_eq!(hex_unit(-0.2, 1.4, 0.039_215_686), "#00ff0a");
    }
}

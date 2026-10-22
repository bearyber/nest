//! Custom keyboard shortcuts (Settings → Shortcuts): what is stored, what is allowed, and how a
//! stored key becomes a macOS menu accelerator. The list of shortcuts and their default keys is
//! in `src/lib/shortcuts.ts`; the page also checks conflicts and keys the OS owns. This side
//! keeps only what must be true for a key to be safe to store and to use in the menu bar.

use serde::{Deserialize, Serialize};

/// The shortcut ids that can be changed (the same ids as `src/lib/shortcuts.ts`; a test there
/// checks the two lists match). Saving drops any other id (one from a newer Nest).
pub const SHORTCUT_IDS: [&str; 14] = [
    "new-project",
    "open-settings",
    "rescan",
    "search",
    "toggle-sidebar",
    "toggle-inspector",
    "open-project",
    "copy-path",
    "undo",
    "redo",
    "create",
    "close-window",
    "quit",
    "keyboard-shortcuts",
];

/// One stored key combination (the same shape as `Combo` in `src/lib/shortcuts.ts`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Combo {
    /// `KeyboardEvent.key`: a lower-case letter, a digit, one of `PUNCTUATION`, `F1`..`F12`
    /// or `Enter`.
    pub key: String,
    /// ⌘ on macOS, Ctrl on Windows.
    #[serde(rename = "mod", skip_serializing_if = "is_false")]
    pub cmd: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub shift: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub alt: bool,
    /// ⌃ on macOS only.
    #[serde(skip_serializing_if = "is_false")]
    pub ctrl: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

const PUNCTUATION: &str = ",./;'[]\\-=`";

fn is_f_key(key: &str) -> bool {
    key.strip_prefix('F')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=12).contains(&n))
}

fn is_letter(key: &str) -> bool {
    key.len() == 1 && key.as_bytes()[0].is_ascii_lowercase()
}

/// Is this key safe to store and to use? Plain words for the Settings message.
pub fn check(id: &str, c: &Combo) -> Result<(), String> {
    if !SHORTCUT_IDS.contains(&id) {
        return Err("That isn't a shortcut Nest knows".into());
    }
    let named = c.key == "Enter" || is_f_key(&c.key);
    let one = c.key.chars().count() == 1;
    let known = named
        || (one
            && (is_letter(&c.key)
                || c.key
                    .chars()
                    .all(|ch| ch.is_ascii_digit() || PUNCTUATION.contains(ch))));
    if !known {
        return Err("Use a letter, a number, a punctuation key, Enter or an F key".into());
    }
    // Shift and Option change what other keys type, so they only go with letters, Enter, F keys.
    if (c.shift || c.alt) && !(is_letter(&c.key) || named) {
        return Err("Shift and Option only go with letters, Enter and F keys".into());
    }
    if cfg!(windows) {
        // Ctrl+Alt is AltGr on many keyboards (it types letters); Alt is Windows' own.
        if c.alt {
            return Err("Alt can't be used on Windows. Use Ctrl, or Ctrl+Shift.".into());
        }
        if c.ctrl {
            return Err("That key combination doesn't exist on Windows".into());
        }
    }
    // Typing must keep working: a plain letter needs ⌘ / Ctrl (⌃ on a Mac); F keys can stand alone.
    let has_cmd = c.cmd || (cfg!(target_os = "macos") && c.ctrl);
    if !has_cmd && !is_f_key(&c.key) {
        return Err(if cfg!(target_os = "macos") {
            "Hold ⌘ (or ⌃) with the key".into()
        } else {
            "Hold Ctrl with the key".into()
        });
    }
    Ok(())
}

/// The macOS menu accelerator for a stored key, e.g. `CmdOrCtrl+Shift+N`.
pub fn accelerator(c: &Combo) -> String {
    let mut parts: Vec<String> = Vec::new();
    if c.ctrl {
        parts.push("Ctrl".into());
    }
    if c.cmd {
        parts.push("CmdOrCtrl".into());
    }
    if c.shift {
        parts.push("Shift".into());
    }
    if c.alt {
        parts.push("Alt".into());
    }
    parts.push(if c.key.chars().count() == 1 {
        c.key.to_uppercase()
    } else {
        c.key.clone()
    });
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(key: &str, cmd: bool, shift: bool, alt: bool, ctrl: bool) -> Combo {
        Combo {
            key: key.into(),
            cmd,
            shift,
            alt,
            ctrl,
        }
    }

    #[test]
    fn accepts_normal_keys() {
        assert!(check("search", &c("g", true, false, false, false)).is_ok());
        assert!(check("search", &c("g", true, true, false, false)).is_ok());
        assert!(check("rescan", &c("F5", false, false, false, false)).is_ok());
        assert!(check("create", &c("Enter", true, false, false, false)).is_ok());
        assert!(check("open-settings", &c(",", true, false, false, false)).is_ok());
        assert!(check("new-project", &c("7", true, false, false, false)).is_ok());
    }

    #[test]
    fn refuses_unsafe_keys() {
        // Unknown id, no key, odd keys, no modifier, Shift on punctuation.
        assert!(check("nope", &c("g", true, false, false, false)).is_err());
        assert!(check("search", &c("", true, false, false, false)).is_err());
        assert!(check("search", &c("Escape", true, false, false, false)).is_err());
        assert!(check("search", &c("Tab", true, false, false, false)).is_err());
        assert!(check("search", &c(" ", true, false, false, false)).is_err());
        assert!(check("search", &c("ab", true, false, false, false)).is_err());
        assert!(check("search", &c("g", false, false, false, false)).is_err());
        assert!(check("search", &c("g", false, true, false, false)).is_err());
        assert!(check("search", &c("/", true, true, false, false)).is_err());
        assert!(check("search", &c("F13", false, false, false, false)).is_err());
    }

    #[test]
    fn windows_refuses_alt_combinations() {
        let r = check("search", &c("g", true, false, true, false));
        assert_eq!(r.is_err(), cfg!(windows));
    }

    #[test]
    fn makes_menu_accelerators() {
        assert_eq!(
            accelerator(&c("n", true, false, false, false)),
            "CmdOrCtrl+N"
        );
        assert_eq!(
            accelerator(&c("s", true, false, false, true)),
            "Ctrl+CmdOrCtrl+S"
        );
        assert_eq!(
            accelerator(&c("z", true, true, false, false)),
            "CmdOrCtrl+Shift+Z"
        );
        assert_eq!(
            accelerator(&c("i", true, false, true, false)),
            "CmdOrCtrl+Alt+I"
        );
        assert_eq!(accelerator(&c("F5", false, false, false, false)), "F5");
        assert_eq!(
            accelerator(&c(",", true, false, false, false)),
            "CmdOrCtrl+,"
        );
    }
}

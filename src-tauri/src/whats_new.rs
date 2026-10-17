//! "What's new" after an update. The notes live in `src/whats-new.json` (bundled at build time,
//! also read by the release workflow). Which lines to show is one pure function, tested below.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use serde::Serialize;
use tauri::State;

use crate::commands::{lock, AppState};
use crate::error::AppResult;
use crate::settings_cmd::edit_settings_quietly;

const NOTES_JSON: &str = include_str!("../../src/whats-new.json");
/// Never show more than this many lines in one go.
pub const MAX_LINES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhatsNew {
    pub version: String,
    pub lines: Vec<String>,
}

/// What to do on launch: maybe show a note, maybe remember the running version.
#[derive(Debug, PartialEq, Eq)]
pub struct Decision {
    pub show: Option<WhatsNew>,
    /// Save the running version as seen right away (a downgrade shows nothing but is recorded).
    pub record_now: bool,
}

/// "0.4.10" → [0, 4, 10]. Always three numbers; anything else is not a version.
pub fn parse_version(v: &str) -> Option<[u64; 3]> {
    let mut parts = v.trim().split('.');
    let mut out = [0u64; 3];
    for slot in &mut out {
        let p = parts.next()?;
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *slot = p.parse().ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(out)
}

/// The notes file as version → lines. A broken file means no notes, never an error.
pub fn parse_notes(json: &str) -> BTreeMap<String, Vec<String>> {
    serde_json::from_str(json).unwrap_or_default()
}

/// Pure decision. `last_seen` None = a settings file from before this feature (an update from
/// 0.4.x): only the running version's note is shown. A fresh install never gets here (the first
/// run records the running version).
pub fn decide(
    notes: &BTreeMap<String, Vec<String>>,
    last_seen: Option<&str>,
    running: &str,
) -> Decision {
    let none = Decision {
        show: None,
        record_now: false,
    };
    let Some(run) = parse_version(running) else {
        return none;
    };
    let seen = last_seen.and_then(parse_version);
    if let Some(seen) = seen {
        if run < seen {
            return Decision {
                show: None,
                record_now: true,
            };
        }
        if run == seen {
            return none;
        }
    }
    let mut picked: Vec<([u64; 3], &Vec<String>)> = notes
        .iter()
        .filter_map(|(k, v)| Some((parse_version(k)?, v)))
        .filter(|(v, _)| match seen {
            Some(s) => *v > s && *v <= run,
            None => *v == run,
        })
        .collect();
    picked.sort_by_key(|a| std::cmp::Reverse(a.0));
    let lines: Vec<String> = picked
        .into_iter()
        .flat_map(|(_, l)| l.iter())
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .take(MAX_LINES)
        .collect();
    if lines.is_empty() {
        return none;
    }
    Decision {
        show: Some(WhatsNew {
            version: running.to_string(),
            lines,
        }),
        record_now: false,
    }
}

/// The note to show on launch, if any. Never an error: no note = nothing.
#[tauri::command(async)]
pub fn whats_new(state: State<'_, AppState>) -> Option<WhatsNew> {
    // First run: complete_first_run records the version; nothing to show. A damaged settings
    // file is never written to.
    if state.first_run.load(Ordering::SeqCst) || state.settings_error.is_some() {
        return None;
    }
    let last_seen = lock(&state.settings).ok()?.last_seen_version.clone();
    let d = decide(
        &parse_notes(NOTES_JSON),
        last_seen.as_deref(),
        env!("CARGO_PKG_VERSION"),
    );
    if d.record_now {
        let _ = record_running(&state);
    }
    d.show
}

/// OK pressed: remember that this version's note has been seen.
#[tauri::command(async)]
pub fn whats_new_seen(state: State<'_, AppState>) -> AppResult<()> {
    record_running(&state)
}

/// The note after your very first project was shown: never show it again.
#[tauri::command(async)]
pub fn first_project_tip_seen(state: State<'_, AppState>) -> AppResult<()> {
    if state.settings_error.is_some() {
        return Ok(());
    }
    edit_settings_quietly(&state, |s| {
        s.first_project_tip_seen = true;
        Ok(())
    })
}

fn record_running(state: &AppState) -> AppResult<()> {
    if state.settings_error.is_some() {
        return Ok(());
    }
    edit_settings_quietly(state, |s| {
        s.last_seen_version = Some(env!("CARGO_PKG_VERSION").to_string());
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notes() -> BTreeMap<String, Vec<String>> {
        let mut m = BTreeMap::new();
        for (v, n) in [("0.4.9", 1), ("0.4.10", 2), ("0.5.0", 3), ("0.6.0", 1)] {
            m.insert(
                v.to_string(),
                (1..=n).map(|i| format!("{v} line {i}")).collect(),
            );
        }
        m
    }

    #[test]
    fn versions_compare_as_numbers() {
        assert!(parse_version("0.4.10") > parse_version("0.4.9"));
        assert_eq!(parse_version("1.2.3"), Some([1, 2, 3]));
        for bad in [
            "1.2",
            "1.2.3.4",
            "1.x.3",
            "",
            "1..3",
            "v1.2.3",
            "1.2.3-beta",
        ] {
            assert_eq!(parse_version(bad), None, "{bad}");
        }
    }

    #[test]
    fn shows_everything_newer_than_seen_up_to_running_newest_first() {
        let d = decide(&notes(), Some("0.4.9"), "0.5.0");
        let w = d.show.unwrap();
        assert_eq!(w.version, "0.5.0");
        assert_eq!(w.lines[0], "0.5.0 line 1");
        assert_eq!(w.lines.len(), 5); // 3 from 0.5.0 + 2 from 0.4.10; none from 0.6.0 or 0.4.9
        assert_eq!(w.lines[3], "0.4.10 line 1");
        assert!(!d.record_now);
    }

    #[test]
    fn skipped_versions_are_all_included() {
        let w = decide(&notes(), Some("0.4.8"), "0.5.0").show.unwrap();
        assert!(w.lines.iter().any(|l| l.starts_with("0.4.9")));
    }

    #[test]
    fn capped_at_eight_lines() {
        let mut m = BTreeMap::new();
        m.insert(
            "0.5.0".to_string(),
            (0..6).map(|i| format!("a{i}")).collect(),
        );
        m.insert(
            "0.4.5".to_string(),
            (0..6).map(|i| format!("b{i}")).collect(),
        );
        let w = decide(&m, Some("0.4.0"), "0.5.0").show.unwrap();
        assert_eq!(w.lines.len(), MAX_LINES);
        assert_eq!(w.lines[6], "b0");
    }

    #[test]
    fn already_seen_shows_nothing() {
        let d = decide(&notes(), Some("0.5.0"), "0.5.0");
        assert_eq!(
            d,
            Decision {
                show: None,
                record_now: false
            }
        );
    }

    #[test]
    fn update_from_before_the_feature_shows_the_running_note_only() {
        let w = decide(&notes(), None, "0.5.0").show.unwrap();
        assert_eq!(w.lines.len(), 3);
        assert!(w.lines.iter().all(|l| l.starts_with("0.5.0")));
    }

    #[test]
    fn downgrade_records_without_showing() {
        let d = decide(&notes(), Some("0.6.0"), "0.5.0");
        assert_eq!(
            d,
            Decision {
                show: None,
                record_now: true
            }
        );
    }

    #[test]
    fn no_entry_is_nothing_not_an_error() {
        assert_eq!(decide(&notes(), None, "0.4.4").show, None);
        assert_eq!(decide(&BTreeMap::new(), Some("0.1.0"), "0.5.0").show, None);
        assert_eq!(decide(&notes(), Some("0.1.0"), "garbage").show, None);
        assert!(parse_notes("not json").is_empty());
    }

    #[test]
    fn shows_once_then_not_again() {
        assert!(decide(&notes(), Some("0.4.9"), "0.5.0").show.is_some());
        // After OK the running version is saved as seen.
        assert_eq!(decide(&notes(), Some("0.5.0"), "0.5.0").show, None);
    }

    #[test]
    fn running_version_has_an_entry() {
        // Catches a release whose bumped version has no note yet.
        let n = parse_notes(NOTES_JSON);
        let lines = n.get(env!("CARGO_PKG_VERSION"));
        assert!(
            lines.is_some_and(|l| !l.is_empty()),
            "add a '{}' entry to src/whats-new.json",
            env!("CARGO_PKG_VERSION")
        );
    }
}

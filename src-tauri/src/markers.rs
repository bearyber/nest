//! The letter in each computer's job codes (`{marker}`), seen from the outside: which letter
//! each of your other computers uses, whether two share one, and a free one to suggest.
//!
//! Pure and read-only. Nest never changes a letter by itself: everything here only feeds a
//! warning with a button (Settings → Job codes, the note in the main window, New Project).

use serde::Serialize;

use crate::devices::OtherDevice;
use crate::names::CodePattern;

/// How many of a computer's newest projects are looked at when guessing its letter.
const GUESS_FROM_NEWEST: usize = 5;

/// One of your other computers and the letter it makes job codes with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerLetter {
    pub device_id: String,
    pub name: String,
    pub os: String,
    pub letter: String,
    /// Not published by that computer (an older Nest): judged from its newest job codes, so
    /// the wording says "seems".
    pub guessed: bool,
}

/// This other computer's letter: the one it published, else a guess from its newest codes
/// (as `pattern` reads them). `None` if neither tells.
pub fn letter_of(d: &OtherDevice, pattern: &CodePattern) -> Option<ComputerLetter> {
    let (letter, guessed) = if d.marker.is_empty() {
        (guess(d, pattern)?, true)
    } else {
        (d.marker.clone(), false)
    };
    Some(ComputerLetter {
        device_id: d.device_id.clone(),
        name: d.name.clone(),
        os: d.os.clone(),
        letter,
        guessed,
    })
}

/// The letter in a computer's newest job codes. Newest only (it may have changed letters since),
/// the first code that fits the pattern wins.
fn guess(d: &OtherDevice, pattern: &CodePattern) -> Option<String> {
    let mut newest: Vec<_> = d
        .projects
        .iter()
        .filter(|p| !p.job_code.is_empty() && !p.client.code.is_empty())
        .collect();
    newest.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    newest
        .into_iter()
        .take(GUESS_FROM_NEWEST)
        .find_map(|p| pattern.marker_of(&p.job_code, &p.client.code))
}

/// Every other computer whose letter is known or guessed.
pub fn letters_of(others: &[OtherDevice], pattern: &CodePattern) -> Vec<ComputerLetter> {
    others
        .iter()
        .filter_map(|d| letter_of(d, pattern))
        .collect()
}

/// The computers using the same letter as this one (case-insensitive).
pub fn clashing<'a>(own: &str, letters: &'a [ComputerLetter]) -> Vec<&'a ComputerLetter> {
    letters
        .iter()
        .filter(|l| l.letter.eq_ignore_ascii_case(own))
        .collect()
}

/// A free letter to suggest for THIS computer. It differs from computer to computer so two of
/// them don't both jump to the same one: the first letter of the computer's name (PC → P,
/// MacBook → M), then the usual one for its system (W / M), then a walk through A–Z that starts
/// at a point taken from its id. Anything in `taken` is skipped, and J (kept for the job
/// tracker). Two-letter ones only if every single letter is taken.
pub fn suggest(name: &str, os: &str, device_id: &str, taken: &[String]) -> String {
    let free = |c: &str| !c.starts_with('J') && !taken.iter().any(|t| t.eq_ignore_ascii_case(c));
    let from_name = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_ascii_uppercase().to_string());
    let usual = if os == "macos" { "M" } else { "W" };
    let start = device_id.bytes().map(usize::from).sum::<usize>() % 26;
    let walk = (0..26u8).map(|i| ((b'A' + ((start as u8 + i) % 26)) as char).to_string());
    let single = from_name
        .into_iter()
        .filter(|c| c.len() == 1 && c.as_bytes()[0].is_ascii_uppercase())
        .chain(std::iter::once(usual.to_string()))
        .chain(walk)
        .find(|c| free(c));
    if let Some(c) = single {
        return c;
    }
    ('A'..='Z')
        .flat_map(|a| ('A'..='Z').map(move |b| format!("{a}{b}")))
        .find(|c| free(c))
        .unwrap_or_else(|| "WX".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::DeviceProject;
    use crate::names::DEFAULT_CODE_PATTERN;
    use crate::plan::ClientValue;

    fn pattern() -> CodePattern {
        CodePattern::parse(DEFAULT_CODE_PATTERN).unwrap()
    }

    fn project(code: &str, client: &str, created: &str) -> DeviceProject {
        DeviceProject {
            id: format!("id-{code}"),
            job_code: code.into(),
            client: ClientValue {
                name: client.into(),
                code: client.into(),
            },
            created_at: created.into(),
            ..Default::default()
        }
    }

    fn other(name: &str, marker: &str, projects: Vec<DeviceProject>) -> OtherDevice {
        OtherDevice {
            device_id: format!("dev-{name}"),
            name: name.into(),
            os: "windows".into(),
            updated_at: String::new(),
            projects,
            requests: vec![],
            handled: vec![],
            handled_upto: None,
            marker: marker.into(),
            past_markers: vec![],
            problem: None,
            not_downloaded: false,
        }
    }

    #[test]
    fn a_published_letter_is_used_as_it_is() {
        let d = other("PC", "W", vec![project("ACME-X09", "ACME", "2026-01-01")]);
        let l = letter_of(&d, &pattern()).unwrap();
        assert_eq!((l.letter.as_str(), l.guessed), ("W", false));
    }

    #[test]
    fn an_older_computers_letter_is_guessed_from_its_newest_codes_only() {
        // Changed letters since: the older code says L, the newest says WL.
        let d = other(
            "Studio",
            "",
            vec![
                project("ACME-L03", "ACME", "2026-01-01T10:00:00+09:00"),
                project("서울-WL07", "서울", "2026-05-01T10:00:00+09:00"),
                project("北京-WL01", "北京", "2026-04-01T10:00:00+09:00"),
            ],
        );
        let l = letter_of(&d, &pattern()).unwrap();
        assert_eq!((l.letter.as_str(), l.guessed), ("WL", true));
        // An old code beyond the newest few doesn't count.
        let mut many = vec![project("ACME-L03", "ACME", "2020-01-01")];
        for i in 0..GUESS_FROM_NEWEST {
            many.push(project(
                &format!("x-{i}"),
                "ACME",
                &format!("2026-0{}-01", i + 1),
            ));
        }
        assert_eq!(letter_of(&other("S", "", many), &pattern()), None);
    }

    #[test]
    fn nothing_to_go_on_means_no_letter() {
        assert_eq!(letter_of(&other("Empty", "", vec![]), &pattern()), None);
        // Projects without a client code (personal ones) say nothing.
        let d = other("P", "", vec![project("OWN-L01", "", "2026-01-01")]);
        assert_eq!(letter_of(&d, &pattern()), None);
    }

    #[test]
    fn same_letter_is_found_whatever_the_case() {
        let ls = letters_of(
            &[other("PC", "W", vec![]), other("Mac", "M", vec![])],
            &pattern(),
        );
        let c = clashing("w", &ls);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].name, "PC");
        assert!(
            clashing("WL", &ls).is_empty(),
            "W and WL are different letters"
        );
    }

    #[test]
    fn suggestions_skip_taken_letters_and_differ_per_computer() {
        let taken = vec!["W".to_string()];
        // Two computers named differently don't land on the same letter.
        assert_eq!(suggest("PC", "windows", "aaaa", &taken), "P");
        assert_eq!(suggest("MacBook Pro", "macos", "bbbb", &taken), "M");
        // The name's letter is taken: the usual one for its system, then the walk.
        let walked = suggest("Mac", "macos", "x", &["M".into()]);
        assert!(walked != "M" && crate::settings::check_marker(&walked).is_ok());
        // Never a taken letter, never J.
        let all_but = |keep: char| -> Vec<String> {
            ('A'..='Z')
                .filter(|c| *c != keep)
                .map(|c| c.to_string())
                .collect()
        };
        assert_eq!(suggest("PC", "windows", "id", &all_but('Q')), "Q");
        let s = suggest("Jon's PC", "windows", "id", &["W".into()]);
        assert!(!s.starts_with('J') && s != "W");
        // Every single letter taken: two letters, still free.
        let all: Vec<String> = ('A'..='Z').map(|c| c.to_string()).collect();
        let two = suggest("PC", "windows", "id", &all);
        assert_eq!(two.len(), 2);
        assert!(crate::settings::check_marker(&two).is_ok());
        // A name that starts with a non-letter (Korean) falls back to the system's letter.
        assert_eq!(suggest("내 맥", "macos", "id", &[]), "M");
        assert_eq!(suggest("", "windows", "id", &[]), "W");
    }
}

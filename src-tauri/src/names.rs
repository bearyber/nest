//! The one sanitiser (CLAUDE.md #8), name transforms, and job codes (build spec §5, §12a).

use std::fmt;

use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

/// Longest allowed path segment, in characters (spec §5).
pub const MAX_SEGMENT_CHARS: usize = 80;

const FORBIDDEN: [char; 9] = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameError {
    /// Nothing usable left after cleaning.
    Empty,
    /// A Windows device name such as `CON` or `LPT1`.
    Reserved(String),
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NameError::Empty => write!(
                f,
                "the name is empty once characters that aren't allowed in file names are removed"
            ),
            NameError::Reserved(name) => write!(f, "\"{name}\" is a reserved name on Windows"),
        }
    }
}

impl std::error::Error for NameError {}

/// Clean one path segment so it is valid on both Windows and macOS.
/// Never silently renames a reserved name: that is an error the user fixes.
pub fn sanitize_segment(raw: &str) -> Result<String, NameError> {
    let cleaned: String = raw
        .nfc()
        .filter(|c| !FORBIDDEN.contains(c) && !c.is_control())
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = collapsed.chars().take(MAX_SEGMENT_CHARS).collect();
    let name = cut.trim_end_matches(['.', ' ']).to_string();
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    if is_reserved(&name) {
        return Err(NameError::Reserved(name));
    }
    Ok(name)
}

/// Windows reserves device names even with an extension (`CON.txt`).
fn is_reserved(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem))
}

/// `{value|transform}` in templates (spec §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transform {
    Pascal,
    Upper,
    Lower,
    Kebab,
    /// Words in capitals joined by `_`: `Vertex Studio film` → `VERTEX_STUDIO_FILM`.
    Caps,
    /// Words in capitals as one block: `Summer Nights` → `SUMMERNIGHTS`.
    Block,
    /// ISO date → `YYMMDD`: `2025-06-08` → `250608`. Anything else is left unchanged.
    Yymmdd,
}

pub const TRANSFORM_NAMES: &str = "pascal, upper, lower, kebab, caps, block or yymmdd";

impl Transform {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "pascal" => Some(Transform::Pascal),
            "upper" => Some(Transform::Upper),
            "lower" => Some(Transform::Lower),
            "kebab" => Some(Transform::Kebab),
            "caps" => Some(Transform::Caps),
            "block" => Some(Transform::Block),
            "yymmdd" => Some(Transform::Yymmdd),
            _ => None,
        }
    }

    pub fn apply(self, s: &str) -> String {
        match self {
            Transform::Pascal => words(s).map(capitalize).collect(),
            Transform::Upper => s.to_uppercase(),
            Transform::Lower => s.to_lowercase(),
            Transform::Kebab => words(s)
                .map(|w| w.to_lowercase())
                .collect::<Vec<_>>()
                .join("-"),
            Transform::Caps => words(s)
                .map(|w| w.to_uppercase())
                .collect::<Vec<_>>()
                .join("_"),
            Transform::Block => words(s).map(|w| w.to_uppercase()).collect(),
            Transform::Yymmdd => iso_to_yymmdd(s).unwrap_or_else(|| s.to_string()),
        }
    }
}

fn iso_to_yymmdd(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let ok = b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
    ok.then(|| format!("{}{}{}", &s[2..4], &s[5..7], &s[8..10]))
}

/// Words are runs of letters, digits and combining marks (accents, Hindi/Thai vowel signs),
/// after NFC. Apostrophes join rather than split (`Don't` → `Dont`).
fn words(s: &str) -> impl Iterator<Item = String> {
    let is_word = |c: char| c.is_alphanumeric() || is_combining_mark(c);
    let nfc: String = s.nfc().collect();
    nfc.split(|c: char| !(is_word(c) || c == '\'' || c == '\u{2019}'))
        .map(|w| w.chars().filter(|&c| is_word(c)).collect::<String>())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .into_iter()
}

fn capitalize(word: String) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Client codes are 2 to 5 of A–Z / 0–9, stored uppercase (spec §5).
pub fn normalize_client_code(raw: &str) -> Result<String, String> {
    let code = raw.trim().to_ascii_uppercase();
    let valid = (2..=5).contains(&code.len())
        && code
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    if valid {
        Ok(code)
    } else {
        Err(format!(
            "Client code must be 2 to 5 letters or digits (got \"{}\")",
            raw.trim()
        ))
    }
}

/// A job code pattern such as `{clientCode}-{marker}{seq:02}` (spec §12a).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodePattern {
    before: String,
    after: String,
    width: usize,
}

pub const DEFAULT_CODE_PATTERN: &str = "{clientCode}-{marker}{seq:02}";

impl CodePattern {
    pub fn parse(pattern: &str) -> Result<Self, String> {
        let bad = || {
            format!("Job code pattern \"{pattern}\" can only use {{clientCode}}, {{marker}} and one {{seq:NN}}")
        };
        let start = pattern.find("{seq").ok_or_else(|| {
            format!("Job code pattern \"{pattern}\" needs {{seq}}, e.g. {{seq:02}}")
        })?;
        let end = start + pattern[start..].find('}').ok_or_else(bad)?;
        let width = match &pattern[start + 1..end] {
            "seq" => 1,
            spec => spec
                .strip_prefix("seq:")
                .and_then(|w| w.parse::<usize>().ok())
                .filter(|w| (1..=6).contains(w))
                .ok_or_else(bad)?,
        };
        let before = &pattern[..start];
        let after = &pattern[end + 1..];
        for part in [before, after] {
            let rest = part.replace("{clientCode}", "").replace("{marker}", "");
            if rest.contains(['{', '}']) {
                return Err(bad());
            }
        }
        if !pattern.contains("{clientCode}") {
            return Err(format!(
                "Job code pattern \"{pattern}\" needs {{clientCode}}"
            ));
        }
        Ok(CodePattern {
            before: before.to_string(),
            after: after.to_string(),
            width,
        })
    }

    fn resolve(part: &str, client: &str, marker: &str) -> String {
        part.replace("{clientCode}", client)
            .replace("{marker}", marker)
    }

    pub fn format(&self, client: &str, marker: &str, seq: u32) -> String {
        format!(
            "{}{:0w$}{}",
            Self::resolve(&self.before, client, marker),
            seq,
            Self::resolve(&self.after, client, marker),
            w = self.width
        )
    }

    /// The sequence number in `code` if it belongs to this client + marker (case-insensitive).
    pub fn seq_of(&self, code: &str, client: &str, marker: &str) -> Option<u32> {
        let before = Self::resolve(&self.before, client, marker).to_ascii_uppercase();
        let after = Self::resolve(&self.after, client, marker).to_ascii_uppercase();
        let code = code.to_ascii_uppercase();
        let digits = code.strip_prefix(&before)?.strip_suffix(&after)?;
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        digits.parse().ok()
    }
}

/// How far past the highest existing code `next_code` looks before giving up.
/// A real jobs folder never needs more than a handful; the cap stops a runaway search.
pub const MAX_CODE_TRIES: u32 = 1_000;

/// Next free job code: highest existing seq for this client + marker, plus one,
/// then skipping any code `is_taken` rejects. Gaps are never refilled.
/// `None` if nothing free turns up within `MAX_CODE_TRIES`.
pub fn next_code(
    pattern: &CodePattern,
    client: &str,
    marker: &str,
    existing: &[String],
    mut is_taken: impl FnMut(&str) -> bool,
) -> Option<String> {
    let highest = existing
        .iter()
        .filter_map(|c| pattern.seq_of(c, client, marker))
        .max()
        .unwrap_or(0);
    let first = highest.saturating_add(1);
    (first..first.saturating_add(MAX_CODE_TRIES))
        .map(|seq| pattern.format(client, marker, seq))
        .find(|code| !is_taken(code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_plain_names() {
        assert_eq!(
            sanitize_segment("KIRA Summer Nights").unwrap(),
            "KIRA Summer Nights"
        );
        assert_eq!(sanitize_segment("07_DELIVERY").unwrap(), "07_DELIVERY");
    }

    #[test]
    fn sanitize_strips_forbidden_and_control_chars() {
        assert_eq!(sanitize_segment("a<b>c:d\"e|f?g*h").unwrap(), "abcdefgh");
        assert_eq!(sanitize_segment("AC/DC\\x").unwrap(), "ACDCx");
        assert_eq!(sanitize_segment("tab\there\u{7}").unwrap(), "tabhere");
    }

    #[test]
    fn sanitize_trims_trailing_dots_and_spaces_and_collapses_whitespace() {
        assert_eq!(
            sanitize_segment("  many   spaces  ").unwrap(),
            "many spaces"
        );
        assert_eq!(sanitize_segment("trail. . ").unwrap(), "trail");
        assert_eq!(sanitize_segment("..."), Err(NameError::Empty));
        assert_eq!(sanitize_segment(""), Err(NameError::Empty));
        assert_eq!(sanitize_segment("???"), Err(NameError::Empty));
    }

    #[test]
    fn sanitize_rejects_reserved_names_case_insensitively() {
        for name in [
            "CON",
            "con",
            "Nul",
            "COM1",
            "lpt9",
            "CON.txt",
            "aux.tar.gz",
            "PRN ",
        ] {
            assert!(
                matches!(sanitize_segment(name), Err(NameError::Reserved(_))),
                "{name} should be reserved"
            );
        }
        for name in ["Console", "COM10", "LPT0", "ACON", "CON_1"] {
            assert!(sanitize_segment(name).is_ok(), "{name} should be allowed");
        }
    }

    #[test]
    fn sanitize_cuts_to_80_chars_then_retrims() {
        let long = "a".repeat(100);
        assert_eq!(sanitize_segment(&long).unwrap().chars().count(), 80);
        let dotted = format!("{}. b", "a".repeat(79));
        assert_eq!(sanitize_segment(&dotted).unwrap(), "a".repeat(79));
        let korean = "가".repeat(90);
        assert_eq!(sanitize_segment(&korean).unwrap().chars().count(), 80);
    }

    #[test]
    fn sanitize_nfc_normalises() {
        // e + combining acute → é (one char)
        assert_eq!(sanitize_segment("Cafe\u{301}").unwrap(), "Caf\u{e9}");
        // Decomposed Hangul jamo → precomposed syllable
        assert_eq!(sanitize_segment("\u{1100}\u{1161}").unwrap(), "가");
        assert_eq!(sanitize_segment("포커스 온 미").unwrap(), "포커스 온 미");
        assert_eq!(sanitize_segment("你好 世界").unwrap(), "你好 世界");
    }

    #[test]
    fn transforms() {
        assert_eq!(Transform::Pascal.apply("Summer Nights"), "SummerNights");
        assert_eq!(Transform::Pascal.apply("summer nights"), "SummerNights");
        assert_eq!(Transform::Pascal.apply("KIRA"), "KIRA");
        assert_eq!(Transform::Pascal.apply("don't stop"), "DontStop");
        assert_eq!(Transform::Pascal.apply("look-dev test"), "LookDevTest");
        assert_eq!(Transform::Pascal.apply("16x9"), "16x9");
        assert_eq!(Transform::Pascal.apply("좋은 날"), "좋은날");
        // Decomposed accents (macOS) and non-composing marks (Hindi) must survive.
        assert_eq!(
            Transform::Pascal.apply("beyonce\u{301} live"),
            "Beyonc\u{e9}Live"
        );
        assert_eq!(Transform::Pascal.apply("हिंदी गाना"), "हिंदीगाना");
        assert_eq!(
            Transform::Kebab.apply("Cafe\u{301} Del Mar"),
            "caf\u{e9}-del-mar"
        );
        assert_eq!(Transform::Kebab.apply("Summer Nights"), "summer-nights");
        assert_eq!(Transform::Upper.apply("kira"), "KIRA");
        assert_eq!(Transform::Lower.apply("KIRA"), "kira");
        assert_eq!(
            Transform::Caps.apply("Vertex Studio brand film"),
            "VERTEX_STUDIO_BRAND_FILM"
        );
        assert_eq!(
            Transform::Caps.apply("ClientX (brand) film!"),
            "CLIENTX_BRAND_FILM"
        );
        assert_eq!(Transform::Caps.apply("don't stop"), "DONT_STOP");
        assert_eq!(Transform::Caps.apply("아이유 좋은 날"), "아이유_좋은_날");
        assert_eq!(Transform::Yymmdd.apply("2025-06-08"), "250608");
        assert_eq!(Transform::Yymmdd.apply("08/06/2025"), "08/06/2025");
        assert_eq!(Transform::Block.apply("Summer Nights"), "SUMMERNIGHTS");
        assert_eq!(Transform::Block.apply("don't stop (live)"), "DONTSTOPLIVE");
        assert_eq!(Transform::Block.apply("좋은 날"), "좋은날");
        assert_eq!(Transform::parse("block"), Some(Transform::Block));
        assert_eq!(Transform::parse("caps"), Some(Transform::Caps));
        assert_eq!(Transform::parse("pascal"), Some(Transform::Pascal));
        assert_eq!(Transform::parse("title"), None);
    }

    #[test]
    fn client_codes() {
        assert_eq!(normalize_client_code(" vx ").unwrap(), "VX");
        assert_eq!(normalize_client_code("ab123").unwrap(), "AB123");
        for bad in ["V", "VTXYZ1", "V-T", "", "가나"] {
            assert!(
                normalize_client_code(bad).is_err(),
                "{bad} should be refused"
            );
        }
    }

    #[test]
    fn code_pattern_format_and_parse() {
        let p = CodePattern::parse(DEFAULT_CODE_PATTERN).unwrap();
        assert_eq!(p.format("VX", "L", 1), "VX-L01");
        assert_eq!(p.format("VX", "L", 100), "VX-L100");
        assert_eq!(p.seq_of("VX-L07", "VX", "L"), Some(7));
        assert_eq!(p.seq_of("vx-l07", "VX", "L"), Some(7));
        assert_eq!(p.seq_of("VX-L100", "VX", "L"), Some(100));
        assert_eq!(p.seq_of("VX-J07", "VX", "L"), None);
        assert_eq!(p.seq_of("VTX-L01", "VX", "L"), None);
        assert_eq!(p.seq_of("VX-L0A", "VX", "L"), None);
        assert_eq!(p.seq_of("VX-L", "VX", "L"), None);
        assert_eq!(p.seq_of("VX-014", "VX", "L"), None);
    }

    #[test]
    fn code_pattern_rejects_bad_patterns() {
        for bad in [
            "{clientCode}-L",
            "{seq:02}",
            "{clientCode}-{foo}{seq:02}",
            "{clientCode}{seq:x}",
            "{clientCode}{seq:02}{seq:02}",
            "{clientCode}{seq:0}",
        ] {
            assert!(CodePattern::parse(bad).is_err(), "{bad} should be refused");
        }
        assert!(CodePattern::parse("{clientCode}{seq}").is_ok());
    }

    fn codes(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn next_code_counts_up_from_highest_and_skips_taken() {
        let p = CodePattern::parse(DEFAULT_CODE_PATTERN).unwrap();
        let next = |existing: &[String], taken: &[&str]| {
            next_code(&p, "VX", "L", existing, |c| taken.contains(&c)).unwrap()
        };
        assert_eq!(next(&[], &[]), "VX-L01");
        // Gaps are not refilled: J01, J03 → J04 (spec M1).
        let existing = codes(&["VX-L01", "VX-L03", "BF-L09", "VX-J12"]);
        assert_eq!(next(&existing, &[]), "VX-L04");
        assert_eq!(
            next_code(&p, "VX", "J", &existing, |_| false).unwrap(),
            "VX-J13"
        );
        // Case-only variants count.
        assert_eq!(next(&codes(&["vx-l05"]), &[]), "VX-L06");
        // Taken (e.g. folder already on disk) → bump.
        assert_eq!(next(&existing, &["VX-L04", "VX-L05"]), "VX-L06");
    }

    #[test]
    fn next_code_gives_up_instead_of_searching_forever() {
        let p = CodePattern::parse(DEFAULT_CODE_PATTERN).unwrap();
        let mut calls = 0;
        let found = next_code(&p, "VX", "L", &[], |_| {
            calls += 1;
            true
        });
        assert_eq!(found, None);
        assert_eq!(calls, MAX_CODE_TRIES);
    }
}

//! Per-machine settings file (`settings.json` in the app config dir). The full Settings UI is M4.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{io_reason, AppError, AppResult};
use crate::manifest::write_json_atomic;
use crate::names::DEFAULT_CODE_PATTERN;

pub const SETTINGS_FILE: &str = "settings.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub jobs_roots: Vec<PathBuf>,
    pub spaces: Vec<String>,
    pub default_space: String,
    pub code_pattern: String,
    pub local_marker: String,
    pub last_template: Option<String>,
    /// Spaces whose projects have no client (the Client field is hidden there).
    pub no_client_spaces: Vec<String>,
    /// Job-code prefix for projects without a client, e.g. `OWN` → `OWN-L01`.
    pub own_code: String,
    /// Template ids not offered in New Project (kept installed).
    pub hidden_templates: Vec<String>,
    /// "system" | "light" | "dark".
    pub theme: String,
    pub launch_at_login: bool,
    /// v0.3.0 tidied extra copies of your templates into the bin once (decision T-A).
    pub tidied_template_copies: bool,
    /// Devices (v0.3.0): off until a shared folder is chosen. The id is a random UUID made
    /// once; it names this computer's list file.
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    pub shared_folder: Option<PathBuf>,
    /// Other computers hidden on this one ("Remove from this PC"). Their files are untouched.
    pub forgotten_devices: Vec<String>,
    /// v0.3.2: "mark done / active" requests this computer sent, until the owner deals with them.
    pub outgoing_requests: Vec<crate::devices::StatusRequest>,
    /// Numbers this computer's requests (counts up, never reused).
    pub request_seq: u64,
    /// Requests from other computers this one dealt with, until their senders drop them.
    pub handled_requests: Vec<crate::devices::Handled>,
    /// Highest request number dealt with per sending computer: nothing is applied twice.
    pub handled_seq: std::collections::BTreeMap<String, u64>,
    /// Archive (M5, spec §12a #21): a folder Nest scans like a jobs folder, but whose projects
    /// are archived (read-only in Nest, hidden behind the Archived filter). Off until chosen.
    pub archive_folder: Option<PathBuf>,
    /// Suggest archiving projects done for more than this many days. None = no suggestions.
    pub archive_after_days: Option<u32>,
    /// Project ids Bernard said "not this one" to; never suggested again.
    pub archive_dismissed: Vec<String>,
    /// Keys this version doesn't know (e.g. written by a newer Nest): kept on every save.
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            jobs_roots: vec![],
            spaces: vec!["Work".into(), "Personal".into()],
            default_space: "Work".into(),
            code_pattern: DEFAULT_CODE_PATTERN.into(),
            local_marker: "L".into(),
            last_template: None,
            no_client_spaces: vec!["Personal".into()],
            own_code: "OWN".into(),
            hidden_templates: vec![],
            theme: "system".into(),
            launch_at_login: false,
            tidied_template_copies: false,
            device_id: None,
            device_name: None,
            shared_folder: None,
            forgotten_devices: vec![],
            outgoing_requests: vec![],
            request_seq: 0,
            handled_requests: vec![],
            handled_seq: Default::default(),
            archive_folder: None,
            archive_after_days: None,
            archive_dismissed: vec![],
            other: serde_json::Map::new(),
        }
    }
}

// ── Edits made from the Settings window. Pure: each validates and changes `Settings` in
// memory; the command saves. Messages are plain English for the UI. ──

pub const THEMES: [&str; 3] = ["system", "light", "dark"];
const MAX_SPACE_CHARS: usize = 40;

/// The letter in job codes made on this computer (`{marker}`). Each computer has its own, so
/// two computers never make the same code. J is kept for jobs from the job tracker (later).
pub fn check_marker(marker: &str) -> Result<String, String> {
    let marker = marker.trim().to_ascii_uppercase();
    if marker.len() != 1 || !marker.chars().all(|c| c.is_ascii_uppercase()) {
        return Err("The letter for this computer must be one letter, A to Z".into());
    }
    if marker == "J" {
        return Err("J is kept for jobs from your job tracker. Pick another letter.".into());
    }
    Ok(marker)
}

/// What first run suggests: W on Windows, M on a Mac. (The saved default stays "L" so older
/// settings files without the key keep the letter they always had.)
pub fn suggested_marker() -> &'static str {
    if cfg!(target_os = "macos") {
        "M"
    } else {
        "W"
    }
}

fn same(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

impl Settings {
    pub fn set_code_settings(
        &mut self,
        pattern: &str,
        marker: &str,
        own_code: &str,
    ) -> Result<(), String> {
        crate::names::CodePattern::parse(pattern)?;
        let marker = check_marker(marker)?;
        let own = crate::names::normalize_client_code(own_code).map_err(|_| {
            "The code for personal jobs must be 2 to 5 letters or digits (e.g. OWN)".to_string()
        })?;
        self.code_pattern = pattern.trim().to_string();
        self.local_marker = marker;
        self.own_code = own;
        Ok(())
    }

    fn space_name(&self, name: &str, except: Option<&str>) -> Result<String, String> {
        let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
        if name.is_empty() {
            return Err("A space needs a name".into());
        }
        if name.chars().count() > MAX_SPACE_CHARS {
            return Err(format!(
                "Space names can be at most {MAX_SPACE_CHARS} characters"
            ));
        }
        let taken = self
            .spaces
            .iter()
            .any(|s| same(s, &name) && except.is_none_or(|e| !same(e, s)));
        if taken {
            return Err(format!("There's already a space called \"{name}\""));
        }
        Ok(name)
    }

    pub fn add_space(&mut self, name: &str) -> Result<(), String> {
        let name = self.space_name(name, None)?;
        self.spaces.push(name);
        Ok(())
    }

    /// Only while no project uses it: projects keep their space name in their own files,
    /// which Nest never rewrites in bulk.
    pub fn rename_space(&mut self, old: &str, new: &str, in_use: bool) -> Result<(), String> {
        let i = self
            .spaces
            .iter()
            .position(|s| s == old)
            .ok_or("That space doesn't exist")?;
        if in_use {
            return Err(format!("\"{old}\" has projects, so it can't be renamed"));
        }
        let new = self.space_name(new, Some(old))?;
        self.spaces[i] = new.clone();
        if self.default_space == old {
            self.default_space = new.clone();
        }
        for s in self.no_client_spaces.iter_mut().filter(|s| *s == old) {
            *s = new.clone();
        }
        Ok(())
    }

    pub fn delete_space(&mut self, name: &str, in_use: bool) -> Result<(), String> {
        if !self.spaces.iter().any(|s| s == name) {
            return Err("That space doesn't exist".into());
        }
        if in_use {
            return Err(format!("\"{name}\" has projects, so it can't be deleted"));
        }
        if self.spaces.len() == 1 {
            return Err("Keep at least one space".into());
        }
        self.spaces.retain(|s| s != name);
        self.no_client_spaces.retain(|s| s != name);
        if self.default_space == name {
            self.default_space = self.spaces[0].clone();
        }
        Ok(())
    }

    pub fn move_space(&mut self, name: &str, to: usize) -> Result<(), String> {
        let i = self
            .spaces
            .iter()
            .position(|s| s == name)
            .ok_or("That space doesn't exist")?;
        let s = self.spaces.remove(i);
        self.spaces.insert(to.min(self.spaces.len()), s);
        Ok(())
    }

    pub fn set_no_client(&mut self, name: &str, on: bool) -> Result<(), String> {
        if !self.spaces.iter().any(|s| s == name) {
            return Err("That space doesn't exist".into());
        }
        self.no_client_spaces.retain(|s| s != name);
        if on {
            self.no_client_spaces.push(name.to_string());
        }
        Ok(())
    }

    pub fn set_default_space(&mut self, name: &str) -> Result<(), String> {
        if !self.spaces.iter().any(|s| s == name) {
            return Err("That space doesn't exist".into());
        }
        self.default_space = name.to_string();
        Ok(())
    }

    /// Add a jobs folder at the end (or move it there if already listed).
    pub fn add_root(&mut self, path: PathBuf) {
        self.jobs_roots.retain(|r| r != &path);
        self.jobs_roots.push(path);
    }

    /// Stop watching a jobs folder. Nothing on disk changes.
    pub fn remove_root(&mut self, path: &Path) -> Result<(), String> {
        let before = self.jobs_roots.len();
        self.jobs_roots.retain(|r| r != path);
        if self.jobs_roots.len() == before {
            return Err("That folder isn't in the list".into());
        }
        Ok(())
    }

    /// The first jobs folder is where new projects go.
    pub fn move_root(&mut self, path: &Path, to: usize) -> Result<(), String> {
        let i = self
            .jobs_roots
            .iter()
            .position(|r| r == path)
            .ok_or("That folder isn't in the list")?;
        let r = self.jobs_roots.remove(i);
        self.jobs_roots.insert(to.min(self.jobs_roots.len()), r);
        Ok(())
    }

    /// Every folder a rescan walks: the jobs folders, then the archive folder (if set).
    pub fn scan_roots(&self) -> Vec<PathBuf> {
        let mut roots = self.jobs_roots.clone();
        roots.extend(self.archive_folder.clone());
        roots
    }

    /// Choose (or clear) the archive folder. It can't be a jobs folder, sit inside one, or
    /// contain one: a project must be in exactly one kind of folder.
    pub fn set_archive_folder(&mut self, path: Option<PathBuf>) -> Result<(), String> {
        if let Some(p) = &path {
            if let Some(jobs) = self.jobs_roots.iter().find(|j| nested(j, p)) {
                return Err(format!(
                    "The archive folder can't be a jobs folder or inside one ({}). Pick a separate folder.",
                    jobs.display()
                ));
            }
        }
        if path.is_none() {
            self.archive_after_days = None;
        }
        self.archive_folder = path;
        Ok(())
    }

    /// Suggestions need an archive folder; 1–3650 days (Nest proposes 90 in the UI).
    pub fn set_archive_after_days(&mut self, days: Option<u32>) -> Result<(), String> {
        match days {
            Some(_) if self.archive_folder.is_none() => {
                return Err("Choose an archive folder first".into())
            }
            Some(d) if !(1..=3650).contains(&d) => {
                return Err("Days must be between 1 and 3650".into())
            }
            _ => {}
        }
        self.archive_after_days = days;
        Ok(())
    }

    /// "Not this one": never suggest archiving this project again.
    pub fn dismiss_archive(&mut self, id: &str) {
        if !self.archive_dismissed.iter().any(|d| d == id) {
            self.archive_dismissed.push(id.to_string());
        }
    }

    /// Why `path` can't become a jobs folder, given the archive folder (None = it can).
    pub fn jobs_root_problem(&self, path: &Path) -> Option<String> {
        self.archive_folder
            .as_ref()
            .filter(|a| nested(a, path))
            .map(|a| {
                format!(
                    "That folder is the archive folder, or inside it ({}). A jobs folder must be separate.",
                    a.display()
                )
            })
    }

    pub fn set_theme(&mut self, theme: &str) -> Result<(), String> {
        if !THEMES.contains(&theme) {
            return Err(format!("Unknown appearance \"{theme}\""));
        }
        self.theme = theme.to_string();
        Ok(())
    }

    pub fn set_template_hidden(&mut self, id: &str, hidden: bool) {
        self.hidden_templates.retain(|t| t != id);
        if hidden {
            self.hidden_templates.push(id.to_string());
        }
    }
}

/// Same folder, or one inside the other (by path, no disk access: the folders may be offline).
fn nested(a: &Path, b: &Path) -> bool {
    a == b || a.starts_with(b) || b.starts_with(a)
}

/// `Ok(None)` when there is no settings file yet. A file that exists but can't be read
/// is an error, and it is never overwritten with defaults.
pub fn load(config_dir: &Path) -> AppResult<Option<Settings>> {
    let path = config_dir.join(SETTINGS_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path).map_err(|e| {
        AppError::new(format!(
            "Settings file {} can't be read: {}",
            path.display(),
            io_reason(&e)
        ))
    })?;
    serde_json::from_str(&text).map(Some).map_err(|e| {
        AppError::new(format!(
            "Settings file {} is damaged ({e}). Nest is using defaults and won't overwrite it.",
            path.display()
        ))
    })
}

pub fn save(config_dir: &Path, settings: &Settings) -> AppResult<()> {
    let fail =
        |e: std::io::Error| AppError::new(format!("Couldn't save settings: {}", io_reason(&e)));
    // The app's own config folder: creating its parents is fine (never a jobs root).
    fs::create_dir_all(config_dir).map_err(fail)?;
    let value = serde_json::to_value(settings).map_err(|e| AppError::new(e.to_string()))?;
    write_json_atomic(&config_dir.join(SETTINGS_FILE), &value, false).map_err(fail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_none_and_round_trip_works() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()).unwrap(), None);
        let mut s = Settings::default();
        s.jobs_roots.push(PathBuf::from("D:/Jobs"));
        s.last_template = Some("grading-mv".into());
        save(dir.path(), &s).unwrap();
        assert_eq!(load(dir.path()).unwrap(), Some(s));
    }

    #[test]
    fn archive_folder_must_be_separate_from_jobs_folders() {
        let mut s = Settings::default();
        s.jobs_roots.push(PathBuf::from("/Jobs/Active"));
        for bad in ["/Jobs/Active", "/Jobs/Active/Old", "/Jobs", "/"] {
            let err = s.set_archive_folder(Some(PathBuf::from(bad))).unwrap_err();
            assert!(err.contains("archive folder"), "{bad}: {err}");
            assert_eq!(s.archive_folder, None, "{bad} was accepted");
        }
        s.set_archive_folder(Some(PathBuf::from("/Jobs/Archive")))
            .unwrap();
        assert_eq!(
            s.scan_roots(),
            vec![
                PathBuf::from("/Jobs/Active"),
                PathBuf::from("/Jobs/Archive")
            ]
        );
        // And the other way round: no jobs folder inside (or around) the archive.
        assert!(s
            .jobs_root_problem(Path::new("/Jobs/Archive/2025"))
            .is_some());
        assert!(s.jobs_root_problem(Path::new("/Jobs")).is_some());
        assert!(s.jobs_root_problem(Path::new("/Jobs/Personal")).is_none());
        // Suggestions need the folder, and a sane number of days; turning the folder off
        // turns them off too.
        s.set_archive_after_days(Some(90)).unwrap();
        assert!(s.set_archive_after_days(Some(0)).is_err());
        assert!(s.set_archive_after_days(Some(4000)).is_err());
        assert_eq!(s.archive_after_days, Some(90));
        s.dismiss_archive("p1");
        s.dismiss_archive("p1");
        assert_eq!(s.archive_dismissed, ["p1"]);
        s.set_archive_folder(None).unwrap();
        assert_eq!(s.scan_roots(), vec![PathBuf::from("/Jobs/Active")]);
        assert_eq!(s.archive_after_days, None);
        assert!(s.set_archive_after_days(Some(30)).is_err());
    }

    #[test]
    fn damaged_file_is_an_error_and_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETTINGS_FILE);
        fs::write(&path, "{ broken").unwrap();
        let err = load(dir.path()).unwrap_err();
        assert!(err.message.contains("damaged"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ broken");
    }

    #[test]
    fn unknown_keys_survive_a_save() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(SETTINGS_FILE),
            r#"{ "jobsRoots": ["D:/Jobs"], "futureOption": { "a": 1 } }"#,
        )
        .unwrap();
        let mut s = load(dir.path()).unwrap().unwrap();
        s.last_template = Some("blank".into());
        save(dir.path(), &s).unwrap();
        let text = fs::read_to_string(dir.path().join(SETTINGS_FILE)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["futureOption"]["a"], 1);
        assert_eq!(v["lastTemplate"], "blank");
    }

    #[test]
    fn code_settings_are_validated() {
        let mut s = Settings::default();
        s.set_code_settings("{clientCode}-{marker}{seq:03}", "w", "me1")
            .unwrap();
        assert_eq!((s.local_marker.as_str(), s.own_code.as_str()), ("W", "ME1"));
        assert!(
            s.set_code_settings(DEFAULT_CODE_PATTERN, "j", "OWN")
                .is_err(),
            "J is kept for the job tracker"
        );
        assert!(
            s.set_code_settings(DEFAULT_CODE_PATTERN, "LX", "OWN")
                .is_err(),
            "one letter"
        );
        assert!(s
            .set_code_settings(DEFAULT_CODE_PATTERN, "", "OWN")
            .is_err());
        assert!(
            s.set_code_settings("{seq:02}", "L", "OWN").is_err(),
            "needs clientCode"
        );
        assert!(s
            .set_code_settings(DEFAULT_CODE_PATTERN, "L1", "OWN")
            .is_err());
        assert!(s.set_code_settings(DEFAULT_CODE_PATTERN, "L", "X").is_err());
        assert_eq!(
            s.code_pattern, "{clientCode}-{marker}{seq:03}",
            "unchanged after errors"
        );
    }

    #[test]
    fn spaces_rename_and_delete_only_when_unused() {
        let mut s = Settings::default();
        s.add_space("  Look   dev ").unwrap();
        assert_eq!(s.spaces, ["Work", "Personal", "Look dev"]);
        assert!(s.add_space("work").is_err(), "case-insensitive duplicate");
        assert!(s.rename_space("Personal", "Mine", true).is_err(), "in use");
        s.rename_space("Personal", "Mine", false).unwrap();
        assert_eq!(
            s.no_client_spaces,
            ["Mine"],
            "no-client list follows the rename"
        );
        s.set_default_space("Mine").unwrap();
        s.delete_space("Mine", false).unwrap();
        assert_eq!(s.default_space, "Work", "default moves to the first space");
        assert!(s.no_client_spaces.is_empty());
        assert!(s.delete_space("Work", true).is_err());
        s.delete_space("Look dev", false).unwrap();
        assert!(s.delete_space("Work", false).is_err(), "keep at least one");
        s.add_space("B").unwrap();
        s.move_space("B", 0).unwrap();
        assert_eq!(s.spaces, ["B", "Work"]);
    }

    #[test]
    fn jobs_roots_add_remove_reorder() {
        let mut s = Settings::default();
        s.add_root(PathBuf::from("D:/A"));
        s.add_root(PathBuf::from("D:/B"));
        s.add_root(PathBuf::from("D:/A"));
        assert_eq!(s.jobs_roots, [PathBuf::from("D:/B"), PathBuf::from("D:/A")]);
        s.move_root(Path::new("D:/A"), 0).unwrap();
        assert_eq!(s.jobs_roots[0], PathBuf::from("D:/A"));
        s.remove_root(Path::new("D:/B")).unwrap();
        assert!(s.remove_root(Path::new("D:/B")).is_err());
    }

    #[test]
    fn missing_keys_fall_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(SETTINGS_FILE),
            r#"{ "jobsRoots": ["D:/Jobs"] }"#,
        )
        .unwrap();
        let s = load(dir.path()).unwrap().unwrap();
        assert_eq!(s.local_marker, "L");
        assert_eq!(s.spaces, ["Work", "Personal"]);
    }

    /// Older versions allowed up to 3 letters; such a file still loads (the letter is only
    /// checked when it's changed).
    #[test]
    fn an_old_multi_letter_marker_still_loads() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(SETTINGS_FILE),
            r#"{ "jobsRoots": ["D:/Jobs"], "localMarker": "LX" }"#,
        )
        .unwrap();
        assert_eq!(load(dir.path()).unwrap().unwrap().local_marker, "LX");
    }
}

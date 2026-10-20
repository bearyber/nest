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
    /// v0.7: keep running with an icon in the macOS menu bar / Windows notification area when
    /// the window is closed (tray.rs). On by default; off = closing the window quits.
    pub show_in_menu_bar: bool,
    /// v0.7: the one-time "Nest is still running" note was shown.
    pub close_note_shown: bool,
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
    /// Per sending computer, the number up to which every request to this computer has been
    /// dealt with (ones above it are in `handled_requests`): nothing is applied twice.
    pub handled_seq: std::collections::BTreeMap<String, u64>,
    /// v0.6.1: requests waiting here (request id → sender, number, since when), so one that
    /// can't be done for days is refused with its reason, and no mark moves past it (see
    /// devices.rs).
    pub waiting_since: std::collections::BTreeMap<String, crate::devices::WaitingSince>,
    /// v0.6.1: the newest `updatedAt` read from each other computer's list, so an older copy
    /// coming back never moves a mark.
    pub newest_lists: std::collections::BTreeMap<String, String>,
    /// Archive (M5, spec §12a #21): a folder Nest scans like a jobs folder, but whose projects
    /// are archived (read-only in Nest, hidden behind the Archived filter). Off until chosen.
    pub archive_folder: Option<PathBuf>,
    /// Suggest archiving projects done for more than this many days. None = no suggestions.
    pub archive_after_days: Option<u32>,
    /// Project ids Bernard said "not this one" to; never suggested again.
    pub archive_dismissed: Vec<String>,
    /// Archive folders used before (newest first, up to 10), so changing the archive folder
    /// never loses track of where archived projects are.
    pub past_archive_folders: Vec<PathBuf>,
    /// v0.4.2: where Personal-space and "No client" projects go. One of `jobs_roots` (so it's
    /// watched); None = the first jobs folder, like every other new project.
    pub personal_root: Option<PathBuf>,
    /// v0.5 What's new: the last version whose note was shown (or recorded silently on a fresh
    /// install). None in a settings file from before 0.5 = updated from 0.4.x: show the note.
    pub last_seen_version: Option<String>,
    /// v0.5 template sync (needs Devices): keep your templates the same on all your computers.
    pub sync_templates: bool,
    /// v0.5 template sync: the highest version this computer has had of each template id, so
    /// a delete (or "go back to original") sticks until a newer version is saved elsewhere.
    pub template_versions_seen: std::collections::BTreeMap<String, u32>,
    /// v0.5 template sync: per template id, when it was saved (RFC 3339, the saving computer's
    /// clock, kept as received) and its lineage (random, made at first sync).
    pub template_sync_meta: std::collections::BTreeMap<String, TemplateSyncMeta>,
    /// v0.5 client kits: a folder holding one folder per client (named by code or name).
    pub kits_folder: Option<PathBuf>,
    /// v0.5 client kits: a kit folder chosen for one client, by client code. Wins over
    /// `kits_folder`.
    pub client_kits: std::collections::BTreeMap<String, PathBuf>,
    /// v0.7: the "<CODE> is ready" note after your very first project was shown (once).
    pub first_project_tip_seen: bool,
    /// v0.7.2 (macOS): the misplaced Nest (disk image, translocated copy) whose "Move to
    /// Applications?" got "Not now". Not asked again while Nest runs from that same place.
    pub move_declined: Option<PathBuf>,
    /// Keys this version doesn't know (an older `resolveLibrary`, from before the library moved
    /// into each template, included) (e.g. written by a newer Nest): kept on every save.
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

/// Template sync bookkeeping for one template id (kept in settings, never in the template).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TemplateSyncMeta {
    pub saved_at: String,
    pub lineage: String,
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
            show_in_menu_bar: true,
            close_note_shown: false,
            tidied_template_copies: false,
            device_id: None,
            device_name: None,
            shared_folder: None,
            forgotten_devices: vec![],
            outgoing_requests: vec![],
            request_seq: 0,
            handled_requests: vec![],
            handled_seq: Default::default(),
            waiting_since: Default::default(),
            newest_lists: Default::default(),
            archive_folder: None,
            archive_after_days: None,
            archive_dismissed: vec![],
            past_archive_folders: vec![],
            personal_root: None,
            last_seen_version: None,
            sync_templates: false,
            template_versions_seen: Default::default(),
            template_sync_meta: Default::default(),
            kits_folder: None,
            client_kits: Default::default(),
            first_project_tip_seen: false,
            move_declined: None,
            other: serde_json::Map::new(),
        }
    }
}

// ── Edits made from the Settings window. Pure: each validates and changes `Settings` in
// memory; the command saves. Messages are plain English for the UI. ──

pub const THEMES: [&str; 3] = ["system", "light", "dark"];
const MAX_PAST_ARCHIVES: usize = 10;
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
        // Personal projects go back to the first jobs folder.
        if self.personal_root.as_deref() == Some(path) {
            self.personal_root = None;
        }
        Ok(())
    }

    /// Where Personal-space and "No client" projects go: one of the jobs folders, or None for
    /// the first one (the same place as other new projects).
    pub fn set_personal_root(&mut self, path: Option<PathBuf>) -> Result<(), String> {
        if let Some(p) = &path {
            if !self.jobs_roots.contains(p) {
                return Err(
                    "Add that folder to your jobs folders first (Add folder…), then pick it here."
                        .into(),
                );
            }
        }
        self.personal_root = path;
        Ok(())
    }

    /// The jobs folder a new project goes into. `no_client`: Personal space or "No client".
    pub fn new_project_root(&self, no_client: bool) -> Option<PathBuf> {
        no_client
            .then_some(self.personal_root.as_ref())
            .flatten()
            .filter(|p| self.jobs_roots.contains(p))
            .or(self.jobs_roots.first())
            .cloned()
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
            if let Some(kit) = self.kit_folders().find(|k| nested(k, p)) {
                return Err(format!(
                    "The archive folder can't be a client kit folder, inside one, or around one ({}). Pick a separate folder.",
                    kit.display()
                ));
            }
        }
        if path.is_none() {
            self.archive_after_days = None;
        }
        // Remember the folder being left; the new one isn't "previous" any more.
        if let Some(old) = self
            .archive_folder
            .take()
            .filter(|old| Some(old) != path.as_ref())
        {
            self.past_archive_folders.retain(|p| p != &old);
            self.past_archive_folders.insert(0, old);
            self.past_archive_folders.truncate(MAX_PAST_ARCHIVES);
        }
        if let Some(new) = &path {
            self.past_archive_folders.retain(|p| p != new);
        }
        self.archive_folder = path;
        Ok(())
    }

    /// Forget a previous archive folder (only the note in Nest; nothing on disk changes).
    pub fn forget_past_archive_folder(&mut self, path: &Path) {
        self.past_archive_folders.retain(|p| p != path);
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
            .or_else(|| {
                self.kit_folders().find(|k| nested(k, path)).map(|k| {
                    format!(
                        "That folder is a client kit folder, inside one, or around one ({}). A jobs folder must be separate.",
                        k.display()
                    )
                })
            })
    }

    // ── Client kits (v0.5) ──

    /// The Kits folder and every folder chosen for one client.
    pub fn kit_folders(&self) -> impl Iterator<Item = &PathBuf> {
        self.kits_folder.iter().chain(self.client_kits.values())
    }

    /// Why `path` can't be a kit folder (None = it can): it is, is inside, or holds a jobs folder
    /// or the archive folder. Copying a kit must never read projects, or write into the kit.
    pub fn kit_folder_problem(&self, path: &Path) -> Option<String> {
        if let Some(jobs) = self.jobs_roots.iter().find(|j| nested(j, path)) {
            return Some(format!(
                "A client kit can't be a jobs folder, inside one, or around one ({}). Pick a separate folder.",
                jobs.display()
            ));
        }
        self.archive_folder
            .as_ref()
            .filter(|a| nested(a, path))
            .map(|a| {
                format!(
                    "A client kit can't be the archive folder, inside it, or around it ({}). Pick a separate folder.",
                    a.display()
                )
            })
    }

    /// The folder holding one kit folder per client (None = stop using it).
    pub fn set_kits_folder(&mut self, path: Option<PathBuf>) -> Result<(), String> {
        if let Some(problem) = path.as_deref().and_then(|p| self.kit_folder_problem(p)) {
            return Err(problem);
        }
        self.kits_folder = path;
        Ok(())
    }

    /// A kit folder chosen for one client (None = use the Kits folder again). Keyed by the
    /// client code in capitals.
    pub fn set_client_kit(&mut self, code: &str, path: Option<PathBuf>) -> Result<(), String> {
        let code = crate::names::normalize_client_code(code)?;
        self.client_kits
            .retain(|k, _| !k.eq_ignore_ascii_case(&code));
        if let Some(path) = path {
            if let Some(problem) = self.kit_folder_problem(&path) {
                return Err(problem);
            }
            self.client_kits.insert(code, path);
        }
        Ok(())
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
    // Only a definite "not there" is first run. If the folder can't even be checked (no
    // permission, a disk problem), that's an error: never start over with defaults.
    match path.try_exists() {
        Ok(true) => {}
        Ok(false) => return Ok(None),
        Err(e) => {
            return Err(AppError::new(format!(
                "Settings file {} can't be checked: {}",
                path.display(),
                io_reason(&e)
            )))
        }
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
    fn menu_bar_icon_is_on_by_default_and_open_at_login_off() {
        let s = Settings::default();
        assert!(s.show_in_menu_bar);
        assert!(!s.close_note_shown);
        assert!(!s.launch_at_login);
        // A settings file from before v0.7 (no key) gets the icon too.
        let old: Settings = serde_json::from_str(r#"{"jobsRoots":["/Jobs"]}"#).unwrap();
        assert!(old.show_in_menu_bar);
        assert!(!old.close_note_shown);
        // Turned off stays off.
        let off: Settings = serde_json::from_str(r#"{"showInMenuBar":false}"#).unwrap();
        assert!(!off.show_in_menu_bar);
    }

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
    fn personal_projects_folder_is_one_of_the_jobs_folders_and_optional() {
        let mut s = Settings::default();
        let (work, personal) = (PathBuf::from("/Jobs/Active"), PathBuf::from("/Personal"));
        s.jobs_roots = vec![work.clone()];
        // Not set: everything goes to the first jobs folder, as before.
        assert_eq!(s.new_project_root(true), Some(work.clone()));
        // Only a listed jobs folder can be chosen.
        assert!(s.set_personal_root(Some(personal.clone())).is_err());
        assert_eq!(s.personal_root, None);
        s.add_root(personal.clone());
        s.set_personal_root(Some(personal.clone())).unwrap();
        assert_eq!(s.new_project_root(true), Some(personal.clone()));
        assert_eq!(s.new_project_root(false), Some(work.clone()));
        // Reordering keeps it; removing that jobs folder clears it.
        s.move_root(&personal, 0).unwrap();
        assert_eq!(s.new_project_root(false), Some(personal.clone()));
        assert_eq!(s.new_project_root(true), Some(personal.clone()));
        s.remove_root(&personal).unwrap();
        assert_eq!(s.personal_root, None);
        assert_eq!(s.new_project_root(true), Some(work));
        // Older settings files (no key) still read.
        let old: Settings = serde_json::from_str(r#"{"jobsRoots":["/Jobs"]}"#).unwrap();
        assert_eq!(old.personal_root, None);
    }

    #[test]
    fn previous_archive_folders_are_remembered() {
        let mut s = Settings::default();
        let (a, b) = (PathBuf::from("/Archive A"), PathBuf::from("/Archive B"));
        s.set_archive_folder(Some(a.clone())).unwrap();
        assert!(s.past_archive_folders.is_empty());
        // Changing it remembers the old one; turning it off too; using one again takes it off
        // the list. Newest first, never twice.
        s.set_archive_folder(Some(b.clone())).unwrap();
        assert_eq!(s.past_archive_folders, vec![a.clone()]);
        s.set_archive_folder(None).unwrap();
        assert_eq!(s.past_archive_folders, vec![b.clone(), a.clone()]);
        s.set_archive_folder(Some(a.clone())).unwrap();
        assert_eq!(s.past_archive_folders, vec![b.clone()]);
        s.set_archive_folder(Some(a.clone())).unwrap();
        assert_eq!(
            s.past_archive_folders,
            vec![b.clone()],
            "same folder: nothing new"
        );
        s.forget_past_archive_folder(&b);
        assert!(s.past_archive_folders.is_empty());
        for i in 0..15 {
            s.set_archive_folder(Some(PathBuf::from(format!("/A{i}"))))
                .unwrap();
        }
        assert_eq!(s.past_archive_folders.len(), MAX_PAST_ARCHIVES);
        // Older settings files (no key) still read.
        let old: Settings = serde_json::from_str(r#"{"archiveFolder":"/X"}"#).unwrap();
        assert!(old.past_archive_folders.is_empty());
    }

    #[test]
    fn kit_folders_must_be_separate_from_jobs_and_archive_folders() {
        let mut s = Settings::default();
        s.jobs_roots.push(PathBuf::from("/Work/Jobs"));
        s.set_archive_folder(Some(PathBuf::from("/Work/Archive")))
            .unwrap();
        for bad in [
            "/Work/Jobs",
            "/Work/Jobs/Kits",
            "/Work",
            "/Work/Archive/Kits",
        ] {
            assert!(
                s.set_kits_folder(Some(PathBuf::from(bad))).is_err(),
                "{bad}"
            );
            assert!(
                s.set_client_kit("apl", Some(PathBuf::from(bad))).is_err(),
                "{bad}"
            );
        }
        assert_eq!(s.kits_folder, None);
        assert!(s.client_kits.is_empty());
        s.set_kits_folder(Some(PathBuf::from("/Work/Kits")))
            .unwrap();
        // Keyed by the code in capitals; setting again replaces; None clears.
        s.set_client_kit("apl", Some(PathBuf::from("/Brand/Apple")))
            .unwrap();
        s.set_client_kit("APL", Some(PathBuf::from("/Brand/Apple 2")))
            .unwrap();
        assert_eq!(s.client_kits.len(), 1);
        assert_eq!(s.client_kits["APL"], PathBuf::from("/Brand/Apple 2"));
        assert!(s
            .set_client_kit("a", Some(PathBuf::from("/Brand/X")))
            .is_err());
        // And the other way round: no archive or jobs folder in, at or around a kit folder.
        assert!(s
            .set_archive_folder(Some(PathBuf::from("/Work/Kits/Old")))
            .is_err());
        assert!(s.set_archive_folder(Some(PathBuf::from("/Brand"))).is_err());
        assert!(s
            .jobs_root_problem(Path::new("/Brand/Apple 2/Jobs"))
            .is_some());
        assert!(s.jobs_root_problem(Path::new("/Work/Kits")).is_some());
        assert!(s.jobs_root_problem(Path::new("/Other")).is_none());
        s.set_client_kit("APL", None).unwrap();
        s.set_kits_folder(None).unwrap();
        assert!(s.kit_folders().next().is_none());
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
    fn first_project_tip_starts_unseen_and_is_remembered() {
        let dir = tempfile::tempdir().unwrap();
        // A settings file from before v0.7 has no such key: not seen yet.
        fs::write(
            dir.path().join(SETTINGS_FILE),
            r#"{ "jobsRoots": ["D:/Jobs"] }"#,
        )
        .unwrap();
        let mut s = load(dir.path()).unwrap().unwrap();
        assert!(!s.first_project_tip_seen);
        s.first_project_tip_seen = true;
        save(dir.path(), &s).unwrap();
        let text = fs::read_to_string(dir.path().join(SETTINGS_FILE)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["firstProjectTipSeen"], true);
        assert!(load(dir.path()).unwrap().unwrap().first_project_tip_seen);
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
    fn an_old_resolve_library_setting_reads_fine_and_is_kept() {
        // Before the library moved into each template, Settings kept one per computer.
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(SETTINGS_FILE),
            r#"{ "jobsRoots": ["D:/Jobs"], "resolveLibrary": { "dbType": "Disk", "dbName": "Work" } }"#,
        )
        .unwrap();
        let s = load(dir.path()).unwrap().unwrap();
        assert_eq!(s.jobs_roots, vec![PathBuf::from("D:/Jobs")]);
        save(dir.path(), &s).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.path().join(SETTINGS_FILE)).unwrap())
                .unwrap();
        assert_eq!(v["resolveLibrary"]["dbName"], "Work");
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

    /// A settings folder that can't be looked into is an error, never "first run".
    #[cfg(unix)]
    #[test]
    fn unreachable_settings_folder_is_not_first_run() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        fs::create_dir(&config).unwrap();
        fs::write(config.join(SETTINGS_FILE), "{}").unwrap();
        fs::set_permissions(&config, fs::Permissions::from_mode(0o000)).unwrap();
        // Running as root (some CI machines) ignores permissions: nothing to test there.
        let blocked = fs::metadata(config.join(SETTINGS_FILE)).is_err();
        let result = load(&config);
        fs::set_permissions(&config, fs::Permissions::from_mode(0o755)).unwrap();
        if blocked {
            let err = result.unwrap_err();
            assert!(err.message.contains("can't be checked"), "{}", err.message);
        }
    }
}

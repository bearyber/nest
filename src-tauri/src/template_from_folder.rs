//! Make a template from a folder you already use (Bernard 2026-10-13, `private/PLAN.md`).
//!
//! Read-only: the folder is walked, never changed (locked #4). The result is a draft for the
//! editor plus the files that would come along; nothing is written until the editor saves.
//! Left out, and listed: system clutter and hidden files, editing-app caches and auto-saves,
//! files over 100 MB (footage), links, unreadable items and names with `{` or `}`.
//! A folder holding a Nest project (`.project.json`) gets its answers turned back into blanks:
//! `251013_SUMMER_CAMPAIGN` becomes `{start|yymmdd}_{name|caps}`.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use unicode_normalization::UnicodeNormalization;

use crate::manifest::read_json_object;
use crate::names::{sanitize_segment, Transform};
pub use crate::staged::MAX_FILE_BYTES;
use crate::staged::{OnDisk, Staged};
use crate::template::{
    Field, FieldKind, FileEntry, LoadedTemplate, Template, TreeEntry, TEMPLATE_SCHEMA,
    TEMPLATE_SUFFIX, TODAY,
};
use crate::template_io::LIMITS;

/// Deeper than this is almost certainly not a project folder.
const MAX_DEPTH: usize = 32;
/// Text files whose blanks get filled in on create (like NOTES.md), up to 1 MB.
const FILL_EXTENSIONS: [&str; 2] = ["md", "txt"];
const FILL_MAX_BYTES: u64 = 1024 * 1024;
/// The folder name for folders that aren't Nest projects (the Blank template's).
pub const DEFAULT_FOLDER_NAME: &str = "{start|yymmdd}_{name|caps}";
const MANIFEST: &str = ".project.json";

/// System files, compared lowercase. Hidden files (dot names, and the hidden or system
/// attribute on Windows) are left out too.
const CLUTTER: [&str; 4] = ["thumbs.db", "desktop.ini", "icon\r", "ehthumbs.db"];
/// Editing apps' caches, previews and auto-saves, compared lowercase.
const CACHES: [&str; 14] = [
    "adobe premiere pro auto-save",
    "adobe premiere pro video previews",
    "adobe premiere pro audio previews",
    "adobe premiere pro preview files",
    "adobe after effects auto-save",
    "media cache",
    "media cache files",
    "peak files",
    "cacheclip",
    "proxymedia",
    "optimizedmedia",
    "render files",
    "transcoded media",
    "analysis files",
];

/// What comes along.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Keep {
    Folders,
    FoldersAndFiles,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SkipReason {
    /// System or hidden files (`.DS_Store`, `Thumbs.db`, `.git`…).
    Clutter,
    /// An editing app's cache, preview or auto-save folder.
    Cache,
    /// Over 100 MB.
    TooBig,
    /// A shortcut / link: never followed.
    Link,
    Unreadable,
    /// `{` or `}` in the name, a `.nest.json` file, or not valid text.
    BadName,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    /// `/`-separated, relative to the folder.
    pub path: String,
    pub reason: SkipReason,
}

/// A draft template read from a folder.
#[derive(Debug, Clone)]
pub struct FolderTemplate {
    pub template: Template,
    /// The folder it was read from.
    pub dir: PathBuf,
    /// The files that come along, by their name in the template (cleaned like an imported
    /// template's: NFC, valid on Windows and macOS).
    pub staged: Staged,
    pub skipped: Vec<Skipped>,
    /// The folder held a Nest project, and its answers became blanks.
    pub from_project: bool,
}

impl FolderTemplate {
    /// `draft` with this folder's files next to it, for the planner (preview and save check).
    pub fn loaded(&self, draft: Template) -> LoadedTemplate {
        LoadedTemplate {
            // Rust picks the real id on save; the preview needs a valid one.
            template: Template {
                id: "from-folder".into(),
                ..draft
            },
            dir: self.dir.clone(),
            files: self.staged.names(),
        }
    }
}

/// Files and folders dropped into the editor (or picked with Add files…), read like a project
/// folder: `folders` and file names are relative to where they were dropped.
#[derive(Debug, Clone, Default)]
pub struct Dropped {
    pub folders: Vec<String>,
    /// Name relative to the drop → where it is on disk.
    pub files: Vec<(String, OnDisk)>,
    pub skipped: Vec<Skipped>,
}

/// Read files and folders dropped into the editor. `items` and `bytes` are what the template
/// already holds, so the template limits (2,000 items, 200 MB) count the whole template.
pub fn read_dropped(paths: &[PathBuf], items: usize, bytes: u64) -> Result<Dropped, String> {
    let mut walk = Walk {
        keep: Keep::FoldersAndFiles,
        folders: vec![],
        files: vec![],
        skipped: vec![],
        bytes,
        seen: 0,
        already: items,
    };
    let mut out = Dropped::default();
    let mut taken = HashSet::new();
    for path in paths {
        let (Some(base), Some(raw)) = (path.parent(), path.file_name().and_then(|n| n.to_str()))
        else {
            continue;
        };
        let first = walk.files.len();
        walk.entry(path, raw, &mut vec![], 0, &mut taken)?;
        for f in walk.files.drain(first..) {
            out.files.push((
                f.name,
                OnDisk {
                    base: base.to_path_buf(),
                    rel: f.disk,
                    size: f.size,
                },
            ));
        }
    }
    out.folders = walk.folders;
    out.skipped = walk.skipped;
    Ok(out)
}

/// The start of the error `read_folder` gives when the files are over the 200 MB limit.
const TOO_BIG: &str = "The files add up to more than";

/// Folders and files, always. Only if the files are over the 200 MB limit, `folders_only`
/// asks whether to make the template from the folders alone. `Ok(None)` if that was declined.
pub fn read_folder_or_folders_only(
    dir: &Path,
    folders_only: impl FnOnce() -> bool,
    installed: impl Fn(&str) -> Option<Template>,
) -> Result<Option<FolderTemplate>, String> {
    match read_folder(dir, Keep::FoldersAndFiles, &installed) {
        Err(e) if e.starts_with(TOO_BIG) => {
            if folders_only() {
                read_folder(dir, Keep::Folders, &installed).map(Some)
            } else {
                Ok(None)
            }
        }
        other => other.map(Some),
    }
}

/// Read `dir` into a draft template. `installed` finds an installed template by id (for a Nest
/// project's questions). Refuses folders over the template limits (2,000 items, 200 MB).
pub fn read_folder(
    dir: &Path,
    keep: Keep,
    installed: impl Fn(&str) -> Option<Template>,
) -> Result<FolderTemplate, String> {
    let meta = fs::symlink_metadata(dir).map_err(|e| format!("Couldn't open that folder: {e}"))?;
    if !meta.is_dir() {
        return Err("That's not a folder. Choose a folder.".into());
    }
    let mut walk = Walk {
        keep,
        folders: vec![],
        files: vec![],
        skipped: vec![],
        bytes: 0,
        seen: 0,
        already: 0,
    };
    walk.dir(dir, &mut vec![], 0)?;

    let project = read_json_object(&dir.join(MANIFEST)).ok();
    let source = project
        .as_ref()
        .and_then(|m| m["template"]["id"].as_str())
        .and_then(&installed);
    let fields = match (&project, &source) {
        (_, Some(t)) => t.fields.clone(),
        (Some(m), None) => fields_from_manifest(m),
        (None, None) => default_fields(),
    };
    let blanks = project
        .as_ref()
        .map(|m| candidates(m, &fields))
        .unwrap_or_default();
    let rename = |path: &str| -> String {
        path.split('/')
            .map(|seg| with_blanks(seg, &blanks).0)
            .collect::<Vec<_>>()
            .join("/")
    };

    // The project folder's name: the answers found in it, else the template's own pattern,
    // else the Blank template's if this template has its questions, else just the job code.
    let top = dir
        .file_name()
        .map(|n| n.to_string_lossy().nfc().collect::<String>())
        .unwrap_or_default();
    let (top_blanks, replaced) = with_blanks(&top, &blanks);
    let has = |key: &str| fields.iter().any(|f| f.key == key);
    let folder_name = if replaced && !top.contains(['{', '}']) {
        top_blanks
    } else if let Some(t) = &source {
        t.folder_name.clone()
    } else if has("start") && has("name") {
        DEFAULT_FOLDER_NAME.into()
    } else {
        "{jobCode}".into()
    };
    let name = match &source {
        Some(t) => t.name.clone(),
        None => top.clone(),
    };
    let name: String = name.trim().chars().take(60).collect();

    let template = Template {
        schema: TEMPLATE_SCHEMA,
        id: String::new(),
        version: 1,
        name: if name.is_empty() {
            "My template".into()
        } else {
            name
        },
        description: String::new(),
        title: None,
        fields,
        folder_name,
        tree: walk
            .folders
            .iter()
            .map(|f| TreeEntry::Folder(rename(f)))
            .collect(),
        files: walk
            .files
            .iter()
            .map(|f| FileEntry {
                from: f.name.clone(),
                to: rename(&f.name),
                fill: fills(&f.name, f.size),
            })
            .collect(),
    };
    let mut staged = Staged::default();
    for f in walk.files {
        staged.insert(
            f.name,
            OnDisk {
                base: dir.to_path_buf(),
                rel: f.disk,
                size: f.size,
            },
        );
    }
    Ok(FolderTemplate {
        template,
        dir: dir.to_path_buf(),
        staged,
        skipped: walk.skipped,
        from_project: project.is_some(),
    })
}

/// A file that comes along.
struct Found {
    /// Its name in the template (cleaned).
    name: String,
    /// Its path in the folder, as on disk.
    disk: String,
    size: u64,
}

/// One folder level on the way down: its name on disk and in the template.
struct Seg {
    disk: String,
    name: String,
}

struct Walk {
    keep: Keep,
    /// Template paths, parents before children.
    folders: Vec<String>,
    files: Vec<Found>,
    skipped: Vec<Skipped>,
    bytes: u64,
    /// Everything looked at, skipped or not: bounds a walk through a huge folder.
    seen: usize,
    /// Folders and files the template already has (the limit counts the whole template).
    already: usize,
}

/// Looked at (including what's skipped) before giving up on a folder as too big to be a project.
const MAX_SEEN: usize = 50_000;

/// A name as it goes in a template: cleaned like a project name (NFC, characters Windows
/// refuses removed, trailing dots and spaces trimmed), so the template can be shared between
/// Windows and macOS. `None` if nothing usable is left, it's a Windows device name, or it
/// holds `{` / `}`.
fn clean_name(raw: &str) -> Option<String> {
    let name = sanitize_segment(raw).ok()?;
    (!name.contains(['{', '}', ':'])).then_some(name)
}

impl Walk {
    fn dir(&mut self, dir: &Path, rel: &mut Vec<Seg>, depth: usize) -> Result<(), String> {
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) if depth == 0 => return Err(format!("Couldn't read that folder: {e}")),
            Err(_) => {
                self.skip(rel, None, SkipReason::Unreadable);
                return Ok(());
            }
        };
        let mut list = vec![];
        for e in entries {
            match e {
                Ok(e) => list.push(e),
                Err(_) => self.skip(rel, Some("?"), SkipReason::Unreadable),
            }
        }
        list.sort_by_key(|e| e.file_name());
        // Two names that clean to the same name (e.g. "Edit?" and "Edit") can't both come.
        let mut taken = HashSet::new();
        for e in list {
            self.seen += 1;
            if self.seen > MAX_SEEN {
                return Err(format!(
                    "That folder holds more than {MAX_SEEN} items. Choose the project folder itself."
                ));
            }
            let os_name = e.file_name();
            let Some(name) = os_name.to_str() else {
                let lossy = os_name.to_string_lossy().into_owned();
                self.skip(rel, Some(&lossy), SkipReason::BadName);
                continue;
            };
            if depth == 0 && name == MANIFEST {
                continue; // Nest's own file: read for the answers, never copied.
            }
            let Ok(kind) = e.file_type() else {
                self.skip(rel, Some(name), SkipReason::Unreadable);
                continue;
            };
            // Folders only: files aren't looked at any further (or listed).
            if kind.is_file() && self.keep == Keep::Folders {
                continue;
            }
            self.entry(&e.path(), name, rel, depth, &mut taken)?;
        }
        Ok(())
    }

    /// One file or folder: skipped with a reason, or taken (a folder is walked into).
    /// `taken` holds the cleaned names already used at this level.
    fn entry(
        &mut self,
        path: &Path,
        name: &str,
        rel: &mut Vec<Seg>,
        depth: usize,
        taken: &mut HashSet<String>,
    ) -> Result<(), String> {
        let Ok(meta) = fs::symlink_metadata(path) else {
            self.skip(rel, Some(name), SkipReason::Unreadable);
            return Ok(());
        };
        if let Some(reason) = skip_reason(name, &meta) {
            self.skip(rel, Some(name), reason);
            return Ok(());
        }
        if !meta.is_dir() && !meta.is_file() {
            return Ok(());
        }
        let clean = clean_name(name)
            .filter(|c| !c.to_lowercase().ends_with(TEMPLATE_SUFFIX) || meta.is_dir())
            .filter(|c| taken.insert(c.to_lowercase()));
        let Some(clean) = clean else {
            self.skip(rel, Some(name), SkipReason::BadName);
            return Ok(());
        };
        let seg = Seg {
            disk: name.to_string(),
            name: clean,
        };
        if meta.is_dir() {
            if depth + 1 >= MAX_DEPTH {
                return Err(format!(
                    "That folder goes more than {MAX_DEPTH} folders deep. Choose the project folder itself."
                ));
            }
            rel.push(seg);
            self.folders.push(join(rel, |s| &s.name));
            self.count()?;
            self.dir(path, rel, depth + 1)?;
            rel.pop();
        } else if meta.len() > MAX_FILE_BYTES {
            self.skip(rel, Some(name), SkipReason::TooBig);
        } else {
            rel.push(seg);
            self.files.push(Found {
                name: join(rel, |s| &s.name),
                disk: join(rel, |s| &s.disk),
                size: meta.len(),
            });
            rel.pop();
            self.bytes += meta.len();
            self.count()?;
            if self.bytes > LIMITS.max_bytes {
                return Err(format!(
                    "{TOO_BIG} {} MB, the most a template can hold. Try \"Folders only\", or fewer files.",
                    LIMITS.max_bytes / 1024 / 1024
                ));
            }
        }
        Ok(())
    }

    /// Stops the walk early (e.g. a whole drive was picked) once over the item limit.
    fn count(&self) -> Result<(), String> {
        if self.already + self.folders.len() + self.files.len() > LIMITS.max_files {
            return Err(format!(
                "That folder holds more than {} folders and files, the most a template can hold. Choose the project folder itself.",
                LIMITS.max_files
            ));
        }
        Ok(())
    }

    /// Listed by its path in the folder (NFC, for display).
    fn skip(&mut self, rel: &[Seg], name: Option<&str>, reason: SkipReason) {
        let path = rel
            .iter()
            .map(|s| s.disk.as_str())
            .chain(name)
            .collect::<Vec<_>>()
            .join("/")
            .nfc()
            .collect();
        self.skipped.push(Skipped { path, reason });
    }
}

fn join(rel: &[Seg], part: impl Fn(&Seg) -> &String) -> String {
    rel.iter()
        .map(|s| part(s).as_str())
        .collect::<Vec<_>>()
        .join("/")
}

/// Why an item found while reading a folder is left out, from its name and its own metadata
/// (`symlink_metadata`, so a link is seen as a link): links and junctions (never followed),
/// system clutter and hidden files, and editing apps' caches. Shared with client kits.
pub(crate) fn skip_reason(name: &str, meta: &fs::Metadata) -> Option<SkipReason> {
    let lower = name.to_lowercase();
    if meta.file_type().is_symlink() {
        return Some(SkipReason::Link);
    }
    if name.starts_with('.') || is_hidden(meta) || CLUTTER.contains(&lower.as_str()) {
        return Some(SkipReason::Clutter);
    }
    if meta.is_dir() && (CACHES.contains(&lower.as_str()) || is_package(&lower)) {
        return Some(SkipReason::Cache);
    }
    None
}

/// Folders macOS shows as one file (a Final Cut library, an app): never taken apart.
fn is_package(lower: &str) -> bool {
    [
        ".fcpbundle",
        ".app",
        ".photoslibrary",
        ".logicx",
        ".band",
        ".imovielibrary",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
}

#[cfg(windows)]
fn is_hidden(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;
    meta.file_attributes() & (HIDDEN | SYSTEM) != 0
}

#[cfg(not(windows))]
fn is_hidden(_: &fs::Metadata) -> bool {
    false // dot names, checked by the caller
}

/// Text files whose blanks get filled in on create.
pub(crate) fn fills(path: &str, size: u64) -> bool {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_lowercase());
    size <= FILL_MAX_BYTES && ext.is_some_and(|e| FILL_EXTENSIONS.contains(&e.as_str()))
}

/// The questions for a folder that isn't a Nest project (the Blank template's).
pub fn default_fields() -> Vec<Field> {
    vec![
        field("client", "Client", FieldKind::Client, true),
        Field {
            max: Some(60),
            ..field("name", "Project name", FieldKind::Text, true)
        },
        Field {
            default: Some(Json::String(TODAY.into())),
            ..field("start", "Start date", FieldKind::Date, true)
        },
    ]
}

fn field(key: &str, label: &str, kind: FieldKind, required: bool) -> Field {
    Field {
        key: key.into(),
        label: label.into(),
        kind,
        required,
        max: None,
        options: vec![],
        default: None,
    }
}

/// Questions rebuilt from a project whose template isn't installed here.
fn fields_from_manifest(m: &Json) -> Vec<Field> {
    // Every template has exactly one client question (a no-client project just had it empty).
    let mut out = vec![field("client", "Client", FieldKind::Client, true)];
    if let Some(map) = m["fields"].as_object() {
        for (key, value) in map.iter().filter(|(k, _)| k.as_str() != "client") {
            let label = label_of(key);
            match value {
                Json::String(s) if is_date(s) => {
                    out.push(field(key, &label, FieldKind::Date, false))
                }
                Json::String(_) => out.push(field(key, &label, FieldKind::Text, false)),
                Json::Bool(_) => out.push(field(key, &label, FieldKind::Bool, false)),
                Json::Array(items) => out.push(Field {
                    options: items
                        .iter()
                        .filter_map(|i| i.as_str().map(String::from))
                        .collect(),
                    ..field(key, &label, FieldKind::Multi, false)
                }),
                _ => {}
            }
        }
    }
    out
}

/// `startDate` → `Start date`.
fn label_of(key: &str) -> String {
    let mut out = String::new();
    for (i, c) in key.chars().enumerate() {
        if i == 0 {
            out.extend(c.to_uppercase());
        } else if c.is_uppercase() {
            out.push(' ');
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn is_date(s: &str) -> bool {
    Transform::Yymmdd.apply(s) != s
}

/// How each answer of a project looks in its folder names, and the blank that makes it:
/// longest first, so `SUMMER_CAMPAIGN` wins over `SUMMER`. Shorter than 3 letters is skipped
/// (too likely to match by accident).
fn candidates(m: &Json, fields: &[Field]) -> Vec<(String, String)> {
    let text_forms = [
        (Transform::Pascal, ""),
        (Transform::Caps, "|caps"),
        (Transform::Kebab, "|kebab"),
        (Transform::Lower, "|lower"),
        (Transform::Upper, "|upper"),
        (Transform::Block, "|block"),
    ];
    let mut out: Vec<(String, String)> = vec![];
    let s = |v: &Json| v.as_str().unwrap_or("").to_string();
    out.push((s(&m["jobCode"]), "{jobCode}".into()));
    out.push((s(&m["client"]["code"]), "{clientCode}".into()));
    for f in fields {
        let value = match f.kind {
            FieldKind::Client => s(&m["client"]["name"]),
            FieldKind::Text | FieldKind::Select | FieldKind::Date => s(&m["fields"][&f.key]),
            _ => continue,
        };
        if value.trim().is_empty() {
            continue;
        }
        if is_date(&value) {
            out.push((
                Transform::Yymmdd.apply(&value),
                format!("{{{}|yymmdd}}", f.key),
            ));
        } else {
            for (t, suffix) in text_forms {
                out.push((t.apply(&value), format!("{{{}{suffix}}}", f.key)));
            }
        }
    }
    let created = s(&m["createdAt"]);
    if let Some(day) = created.get(..10).filter(|d| is_date(d)) {
        out.push((Transform::Yymmdd.apply(day), "{yymmdd}".into()));
        out.push((day.to_string(), "{date}".into()));
    }
    let mut seen = HashSet::new();
    out.retain(|(r, _)| r.chars().count() >= 3 && seen.insert(r.nfc().collect::<String>()));
    for c in &mut out {
        c.0 = c.0.nfc().collect();
    }
    out.sort_by_key(|(r, _)| std::cmp::Reverse(r.len()));
    out
}

/// Put blanks back where a project's answers appear in `name`, as whole words only (between
/// the start or end and anything that isn't a letter or digit). Also says whether any did.
fn with_blanks(name: &str, blanks: &[(String, String)]) -> (String, bool) {
    let mut out = String::new();
    let mut replaced = false;
    let mut i = 0;
    'outer: while i < name.len() {
        let starts_word = name[..i]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        if starts_word {
            for (rendered, blank) in blanks {
                if name[i..].starts_with(rendered.as_str()) {
                    let end = i + rendered.len();
                    let ends_word = name[end..]
                        .chars()
                        .next()
                        .is_none_or(|c| !c.is_alphanumeric());
                    if ends_word {
                        out.push_str(blank);
                        replaced = true;
                        i = end;
                        continue 'outer;
                    }
                }
            }
        }
        let c = name[i..].chars().next().unwrap_or_default();
        out.push(c);
        i += c.len_utf8().max(1);
    }
    (out, replaced)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    fn mk(root: &Path, rel: &str) {
        fs::create_dir_all(root.join(rel)).unwrap();
    }
    fn file(root: &Path, rel: &str, bytes: &[u8]) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }
    fn folders(t: &Template) -> Vec<&str> {
        t.tree.iter().map(|e| e.name()).collect()
    }
    fn none(_: &str) -> Option<Template> {
        None
    }

    #[test]
    fn plain_folder_keeps_names_and_skips_clutter_caches_and_big_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Acme Job");
        mk(&root, "01_BRIEF");
        mk(&root, "02_FOOTAGE/A_CAM");
        mk(&root, "03_PROJECT/Adobe Premiere Pro Auto-Save");
        mk(&root, ".git/objects");
        file(&root, "03_PROJECT/edit.prproj", b"gz");
        file(
            &root,
            "03_PROJECT/Adobe Premiere Pro Auto-Save/edit-1.prproj",
            b"gz",
        );
        file(&root, "01_BRIEF/notes.md", b"# Brief");
        file(&root, ".DS_Store", b"x");
        file(&root, "02_FOOTAGE/Thumbs.db", b"x");
        let big = fs::File::create(root.join("02_FOOTAGE/A_CAM/clip.mov")).unwrap();
        big.set_len(MAX_FILE_BYTES + 1).unwrap();

        let r = read_folder(&root, Keep::FoldersAndFiles, none).unwrap();
        assert!(!r.from_project);
        assert_eq!(r.template.name, "Acme Job");
        assert_eq!(r.template.folder_name, DEFAULT_FOLDER_NAME);
        assert_eq!(r.template.fields, default_fields());
        assert_eq!(
            folders(&r.template),
            ["01_BRIEF", "02_FOOTAGE", "02_FOOTAGE/A_CAM", "03_PROJECT"]
        );
        let names = r.staged.names();
        let files: Vec<_> = names.iter().map(String::as_str).collect();
        assert_eq!(files, ["01_BRIEF/notes.md", "03_PROJECT/edit.prproj"]);
        // Text files get their blanks filled in; the Premiere project is copied as it is.
        let fill: Vec<_> = r
            .template
            .files
            .iter()
            .map(|f| (f.to.as_str(), f.fill))
            .collect();
        assert_eq!(
            fill,
            [
                ("01_BRIEF/notes.md", true),
                ("03_PROJECT/edit.prproj", false)
            ]
        );
        let skipped: Vec<_> = r
            .skipped
            .iter()
            .map(|s| (s.path.as_str(), s.reason))
            .collect();
        assert_eq!(
            skipped,
            [
                (".DS_Store", SkipReason::Clutter),
                (".git", SkipReason::Clutter),
                ("02_FOOTAGE/A_CAM/clip.mov", SkipReason::TooBig),
                ("02_FOOTAGE/Thumbs.db", SkipReason::Clutter),
                ("03_PROJECT/Adobe Premiere Pro Auto-Save", SkipReason::Cache),
            ]
        );
    }

    #[test]
    fn folders_only_takes_no_files_and_lists_none() {
        let tmp = tempfile::tempdir().unwrap();
        mk(tmp.path(), "A/B");
        file(tmp.path(), "A/brief.md", b"hi");
        let r = read_folder(tmp.path(), Keep::Folders, none).unwrap();
        assert_eq!(folders(&r.template), ["A", "A/B"]);
        assert!(r.staged.is_empty() && r.template.files.is_empty() && r.skipped.is_empty());
    }

    #[test]
    fn the_folder_is_left_exactly_as_it_was() {
        let tmp = tempfile::tempdir().unwrap();
        mk(tmp.path(), "A/B");
        file(tmp.path(), "A/brief.md", b"hi");
        file(tmp.path(), ".project.json", br#"{"jobCode":"ACME-W01"}"#);
        let snapshot = |p: &Path| {
            let mut v = vec![];
            for e in walk_all(p) {
                let bytes = if e.is_file() {
                    fs::read(&e).unwrap()
                } else {
                    vec![]
                };
                v.push((e, bytes));
            }
            v
        };
        let before = snapshot(tmp.path());
        read_folder(tmp.path(), Keep::FoldersAndFiles, none).unwrap();
        assert_eq!(before, snapshot(tmp.path()));
    }

    fn walk_all(p: &Path) -> Vec<PathBuf> {
        let mut out = vec![p.to_path_buf()];
        let mut entries: Vec<_> = fs::read_dir(p)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
        entries.sort();
        for e in entries {
            if e.is_dir() {
                out.extend(walk_all(&e));
            } else {
                out.push(e);
            }
        }
        out
    }

    #[test]
    fn braces_in_names_are_left_out_with_their_contents() {
        let tmp = tempfile::tempdir().unwrap();
        mk(tmp.path(), "{old}/inside");
        file(tmp.path(), "a{b}.txt", b"x");
        let r = read_folder(tmp.path(), Keep::FoldersAndFiles, none).unwrap();
        assert!(r.template.tree.is_empty() && r.staged.is_empty());
        assert_eq!(r.skipped.len(), 2);
        assert!(r.skipped.iter().all(|s| s.reason == SkipReason::BadName));
    }

    #[test]
    fn too_many_items_or_bytes_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..=LIMITS.max_files {
            mk(tmp.path(), &format!("f{i}"));
        }
        let e = read_folder(tmp.path(), Keep::Folders, none).unwrap_err();
        assert!(e.contains("more than 2000 folders and files"), "{e}");

        let tmp = tempfile::tempdir().unwrap();
        for i in 0..3 {
            let f = fs::File::create(tmp.path().join(format!("big{i}.bin"))).unwrap();
            f.set_len(MAX_FILE_BYTES).unwrap(); // 3 × 100 MB > 200 MB
        }
        let e = read_folder(tmp.path(), Keep::FoldersAndFiles, none).unwrap_err();
        assert!(e.contains("more than 200 MB"), "{e}");
        // Folders only is fine.
        assert!(read_folder(tmp.path(), Keep::Folders, none).is_ok());
    }

    #[test]
    fn over_200_mb_offers_folders_only() {
        let tmp = tempfile::tempdir().unwrap();
        mk(tmp.path(), "01_BRIEF");
        for i in 0..3 {
            let f = fs::File::create(tmp.path().join(format!("big{i}.bin"))).unwrap();
            f.set_len(MAX_FILE_BYTES).unwrap(); // 3 × 100 MB > 200 MB
        }
        // Declined: nothing made.
        let mut asked = 0;
        let r = read_folder_or_folders_only(
            tmp.path(),
            || {
                asked += 1;
                false
            },
            none,
        )
        .unwrap();
        assert!(r.is_none());
        assert_eq!(asked, 1);
        // Accepted: folders only, no files.
        let r = read_folder_or_folders_only(tmp.path(), || true, none)
            .unwrap()
            .unwrap();
        assert!(r.staged.names().is_empty());
        assert!(folders(&r.template).contains(&"01_BRIEF"));
    }

    #[test]
    fn under_the_limit_brings_files_without_asking() {
        let tmp = tempfile::tempdir().unwrap();
        mk(tmp.path(), "01_BRIEF");
        fs::write(tmp.path().join("01_BRIEF/brief.txt"), "hi").unwrap();
        let r = read_folder_or_folders_only(tmp.path(), || panic!("asked"), none)
            .unwrap()
            .unwrap();
        assert_eq!(r.staged.names().len(), 1);
    }

    #[test]
    fn a_file_is_not_a_folder() {
        let tmp = tempfile::tempdir().unwrap();
        file(tmp.path(), "a.txt", b"x");
        let e = read_folder(&tmp.path().join("a.txt"), Keep::Folders, none).unwrap_err();
        assert!(e.contains("not a folder"), "{e}");
    }

    #[cfg(unix)]
    #[test]
    fn links_are_never_followed() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        file(outside.path(), "secret.txt", b"x");
        mk(tmp.path(), "A");
        std::os::unix::fs::symlink(outside.path(), tmp.path().join("A/elsewhere")).unwrap();
        let r = read_folder(tmp.path(), Keep::FoldersAndFiles, none).unwrap();
        assert_eq!(folders(&r.template), ["A"]);
        assert_eq!(r.skipped[0].reason, SkipReason::Link);
    }

    fn video() -> Template {
        Template {
            schema: 1,
            id: "video".into(),
            version: 1,
            name: "Video".into(),
            description: String::new(),
            title: None,
            fields: vec![
                field("client", "Client", FieldKind::Client, true),
                field("name", "Project name", FieldKind::Text, true),
                field("start", "Start date", FieldKind::Date, true),
                field("artist", "Artist", FieldKind::Text, false),
            ],
            folder_name: "{start|yymmdd}_{name|caps}".into(),
            tree: vec![],
            files: vec![],
        }
    }

    fn project(root: &Path) {
        let m = json!({
            "schema": 1, "id": "p1", "jobCode": "ACME-W07", "title": "Summer Campaign",
            "client": { "name": "Acme Studio", "code": "ACME" },
            "template": { "id": "video", "version": 1 },
            "createdAt": "2025-10-13T10:00:00+09:00",
            "fields": { "name": "Summer Campaign", "start": "2025-10-13", "artist": "Kim" }
        });
        file(root, ".project.json", m.to_string().as_bytes());
    }

    #[test]
    fn a_nest_project_gets_its_answers_back_as_blanks() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("251013_SUMMER_CAMPAIGN");
        project(&root);
        mk(&root, "ACME-W07_EXPORTS/acme-studio");
        mk(&root, "SummerCampaign_Selects");
        mk(&root, "SUMMER_TEASER"); // only part of an answer: left alone
        mk(&root, "Kim"); // 3 letters: matched as a whole word
        file(&root, "ACME-W07_EXPORTS/SUMMER_CAMPAIGN_v1.prproj", b"gz");
        let r = read_folder(&root, Keep::FoldersAndFiles, |id| {
            (id == "video").then(video)
        })
        .unwrap();
        assert!(r.from_project);
        assert_eq!(r.template.name, "Video");
        assert_eq!(r.template.fields, video().fields);
        assert_eq!(r.template.folder_name, "{start|yymmdd}_{name|caps}");
        assert_eq!(
            folders(&r.template),
            [
                "{jobCode}_EXPORTS",
                "{jobCode}_EXPORTS/{client|kebab}",
                "{artist}",
                "SUMMER_TEASER",
                "{name}_Selects",
            ]
        );
        let f = &r.template.files[0];
        assert_eq!(f.from, "ACME-W07_EXPORTS/SUMMER_CAMPAIGN_v1.prproj");
        assert_eq!(f.to, "{jobCode}_EXPORTS/{name|caps}_v1.prproj");
        // The manifest is read, never copied or listed.
        assert!(!r.staged.names().iter().any(|f| f.contains("project.json")));
        assert!(r.skipped.is_empty());
    }

    #[test]
    fn a_project_from_a_template_that_isnt_here_rebuilds_its_questions() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("251013_SUMMER_CAMPAIGN");
        project(&root);
        let r = read_folder(&root, Keep::Folders, none).unwrap();
        let fields: Vec<_> = r
            .template
            .fields
            .iter()
            .map(|f| (f.key.as_str(), f.label.as_str(), f.kind))
            .collect();
        assert_eq!(
            fields,
            [
                ("client", "Client", FieldKind::Client),
                ("name", "Name", FieldKind::Text),
                ("start", "Start", FieldKind::Date),
                ("artist", "Artist", FieldKind::Text),
            ]
        );
        assert_eq!(r.template.folder_name, "{start|yymmdd}_{name|caps}");
        assert_eq!(r.template.name, "251013_SUMMER_CAMPAIGN");
    }

    #[test]
    fn a_project_whose_folder_name_has_no_answers_keeps_its_templates_pattern() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Renamed by hand");
        project(&root);
        let r = read_folder(&root, Keep::Folders, |id| (id == "video").then(video)).unwrap();
        assert_eq!(r.template.folder_name, "{start|yymmdd}_{name|caps}");
    }

    #[test]
    fn korean_names_come_through_nfc() {
        let tmp = tempfile::tempdir().unwrap();
        let nfd: String = "촬영 소스".nfd().collect();
        mk(tmp.path(), &nfd);
        mk(tmp.path(), "资料");
        let r = read_folder(tmp.path(), Keep::Folders, none).unwrap();
        let mut got = folders(&r.template);
        got.sort();
        assert_eq!(got, ["资料", "촬영 소스"]);
    }

    #[test]
    fn blanks_only_replace_whole_words() {
        let b = vec![
            ("SUMMER_CAMPAIGN".to_string(), "{name|caps}".to_string()),
            ("ACME".to_string(), "{clientCode}".to_string()),
        ];
        assert_eq!(
            with_blanks("ACME_SUMMER_CAMPAIGN", &b),
            ("{clientCode}_{name|caps}".into(), true)
        );
        assert_eq!(with_blanks("ACMEX", &b), ("ACMEX".into(), false));
        assert_eq!(with_blanks("XACME", &b), ("XACME".into(), false));
        assert_eq!(
            with_blanks("ACME ACME", &b),
            ("{clientCode} {clientCode}".into(), true)
        );
        assert_eq!(with_blanks("", &b), (String::new(), false));
        assert_eq!(
            with_blanks("좋은 ACME", &b),
            ("좋은 {clientCode}".into(), true)
        );
    }

    // Mac / Linux only: Windows won't create `Final?.prproj`, `CON.txt` or `Selects.` at all,
    // which is exactly why Nest cleans them (the v0.5.0 release gate failed on this, 2026-10-15).
    #[cfg(unix)]
    #[test]
    fn names_are_cleaned_so_the_template_works_on_both_computers() {
        let tmp = tempfile::tempdir().unwrap();
        file(tmp.path(), "Final?.prproj", b"x"); // fine on a Mac, not on Windows
        file(tmp.path(), "Edit  v2.aep", b"x"); // double space
        file(tmp.path(), "CON.txt", b"x"); // a Windows device name
        file(tmp.path(), "old.nest.json", b"{}"); // would break the template folder
        mk(tmp.path(), "Selects.");
        let r = read_folder(tmp.path(), Keep::FoldersAndFiles, none).unwrap();
        let names = r.staged.names();
        let files: Vec<_> = names.iter().map(String::as_str).collect();
        assert_eq!(files, ["Edit v2.aep", "Final.prproj"]);
        assert_eq!(folders(&r.template), ["Selects"]);
        let bad: Vec<_> = r
            .skipped
            .iter()
            .filter(|s| s.reason == SkipReason::BadName)
            .map(|s| s.path.as_str())
            .collect();
        assert_eq!(bad, ["CON.txt", "old.nest.json"]);
    }

    /// The part of the cleaning rules that Windows can set up too.
    #[test]
    fn double_spaces_are_cleaned_and_nest_json_files_left_out_everywhere() {
        let tmp = tempfile::tempdir().unwrap();
        file(tmp.path(), "Edit  v2.aep", b"x");
        file(tmp.path(), "old.nest.json", b"{}");
        let r = read_folder(tmp.path(), Keep::FoldersAndFiles, none).unwrap();
        assert_eq!(
            r.staged.names().into_iter().collect::<Vec<_>>(),
            ["Edit v2.aep"]
        );
        assert_eq!(r.skipped[0].path, "old.nest.json");
        assert_eq!(r.skipped[0].reason, SkipReason::BadName);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn two_names_that_clean_to_the_same_name_keep_only_one() {
        let tmp = tempfile::tempdir().unwrap();
        file(tmp.path(), "Edit.txt", b"x");
        file(tmp.path(), "Edit?.txt", b"x");
        let r = read_folder(tmp.path(), Keep::FoldersAndFiles, none).unwrap();
        assert_eq!(r.staged.len(), 1);
        assert_eq!(r.skipped[0].reason, SkipReason::BadName);
    }

    #[test]
    fn korean_file_names_are_stored_nfc_and_still_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let nfd: String = "촬영노트.md".nfd().collect();
        file(tmp.path(), &format!("A/{nfd}"), b"hi");
        let r = read_folder(tmp.path(), Keep::FoldersAndFiles, none).unwrap();
        assert_eq!(r.template.files[0].from, "A/촬영노트.md");
        assert!(r.staged.contains("A/촬영노트.md"));
        let out = tempfile::tempdir().unwrap();
        r.staged
            .copy_used(&r.template, &Default::default(), out.path())
            .unwrap();
        let copied: Vec<_> = fs::read_dir(out.path().join("A"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(copied.len(), 1);
        assert_eq!(copied[0].nfc().collect::<String>(), "촬영노트.md");
    }

    #[test]
    fn app_libraries_are_never_taken_apart() {
        let tmp = tempfile::tempdir().unwrap();
        mk(tmp.path(), "Edit.fcpbundle/Render Files");
        mk(tmp.path(), "Keep");
        let r = read_folder(tmp.path(), Keep::Folders, none).unwrap();
        assert_eq!(folders(&r.template), ["Keep"]);
        assert_eq!(r.skipped[0].reason, SkipReason::Cache);
    }

    #[test]
    fn a_draft_can_only_copy_the_folders_own_files() {
        let tmp = tempfile::tempdir().unwrap();
        file(tmp.path(), "a.txt", b"x");
        let r = read_folder(tmp.path(), Keep::FoldersAndFiles, none).unwrap();
        let mut draft = r.template.clone();
        draft.files[0].from = "../../etc/passwd".into();
        let out = tempfile::tempdir().unwrap();
        let e = r
            .staged
            .copy_used(&draft, &Default::default(), out.path())
            .unwrap_err();
        assert!(e.contains("isn't one of this template's files"), "{e}");
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_swapped_for_a_link_after_reading_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        file(tmp.path(), "A/a.txt", b"x");
        file(outside.path(), "a.txt", b"secret");
        let r = read_folder(tmp.path(), Keep::FoldersAndFiles, none).unwrap();
        fs::remove_dir_all(tmp.path().join("A")).unwrap();
        std::os::unix::fs::symlink(outside.path(), tmp.path().join("A")).unwrap();
        let out = tempfile::tempdir().unwrap();
        let e = r
            .staged
            .copy_used(&r.template, &Default::default(), out.path())
            .unwrap_err();
        assert!(e.contains("changed since it was added"), "{e}");
    }

    #[test]
    fn a_project_without_its_template_or_usual_questions_still_makes_a_valid_name() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Renamed");
        let m = json!({
            "jobCode": "OWN-M02", "client": { "name": "", "code": "OWN" },
            "template": { "id": "music-video" },
            "fields": { "song": "Blue", "artist": "Kim" }
        });
        file(&root, ".project.json", m.to_string().as_bytes());
        let r = read_folder(&root, Keep::Folders, none).unwrap();
        assert_eq!(r.template.folder_name, "{jobCode}");
        assert_eq!(r.template.fields[0].kind, FieldKind::Client);
        let ready = r.loaded(r.template.clone()).template; // the id Rust gives a preview
        assert!(ready.validate().is_ok(), "{:?}", ready.validate());
    }

    #[test]
    fn braces_in_the_picked_folders_own_name_are_never_used() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("{start}_ACME-W07");
        project(&root);
        let r = read_folder(&root, Keep::Folders, |id| (id == "video").then(video)).unwrap();
        assert_eq!(r.template.folder_name, "{start|yymmdd}_{name|caps}");
    }

    #[test]
    fn labels_from_keys() {
        assert_eq!(label_of("startDate"), "Start date");
        assert_eq!(label_of("fps"), "Fps");
    }
}

//! Installed templates on disk: loading (bundled + yours), import (`.nesttemplate` zip or a
//! single `.json`), export, duplicate, and delete (to the Recycle Bin / Trash, yours only).
//!
//! Safety (M4 review): imports unpack into a hidden `.import-*` folder that the loader skips,
//! every zip entry is checked and written by us (no links, no `..`, no reserved names, real
//! byte counting against a size cap), and nothing existing is ever overwritten. T1: a newer
//! version of a template you already have installs alongside it, and only the newest of each
//! id is offered; older ones stay on disk ("superseded") until you delete them.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use unicode_normalization::UnicodeNormalization;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::names::sanitize_segment;
use crate::template::{load_dir, LoadedTemplate, TEMPLATE_SUFFIX};

pub const PACKAGE_EXT: &str = "nesttemplate";

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_bytes: u64,
    pub max_files: usize,
}

pub const LIMITS: Limits = Limits {
    max_bytes: 200 * 1024 * 1024,
    max_files: 2_000,
};

/// One installed template.
#[derive(Debug, Clone)]
pub struct Installed {
    pub loaded: LoadedTemplate,
    pub built_in: bool,
    /// An older version of a template you also have a newer one of (T1): kept, not offered.
    /// Also set on a built-in that one of yours replaces.
    pub superseded: bool,
    /// A built-in that one of yours (same id) replaces: listed as the original, never offered.
    pub replaced: bool,
}

impl Installed {
    /// Stable handle for the Settings window: `user:<folder>` or `builtin:<folder>`.
    pub fn key(&self) -> String {
        let folder = self
            .loaded
            .dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        format!(
            "{}:{folder}",
            if self.built_in { "builtin" } else { "user" }
        )
    }
}

/// What the Settings window lists.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateInfo {
    pub key: String,
    pub id: String,
    pub name: String,
    pub version: u32,
    pub description: String,
    pub built_in: bool,
    pub superseded: bool,
    pub replaced: bool,
    pub hidden: bool,
    pub folder: PathBuf,
}

pub fn info(t: &Installed, hidden: &[String]) -> TemplateInfo {
    let tpl = &t.loaded.template;
    TemplateInfo {
        key: t.key(),
        id: tpl.id.clone(),
        name: tpl.name.clone(),
        version: tpl.version,
        description: tpl.description.clone(),
        built_in: t.built_in,
        superseded: t.superseded,
        replaced: t.replaced,
        hidden: hidden.contains(&tpl.id),
        folder: t.loaded.dir.clone(),
    }
}

/// The templates Nest ships. Only these load from the bundled folder: an app update on Windows
/// can leave older template folders behind, and those must not show up as built-ins.
pub const BUILT_IN_IDS: &[&str] = &["blank", "general", "video", "design", "photo"];

/// Yours (from the config folder) and the bundled ones named in `built_in_ids` (in their own
/// folder). Same id: yours wins and the built-in is marked `replaced`; several of yours with
/// one id: the newest version is offered, the rest are marked superseded.
pub fn load_installed(
    user_dir: &Path,
    bundled_dir: &Path,
    built_in_ids: &[&str],
) -> Vec<Installed> {
    let mut user: Vec<Installed> = load_all_in(user_dir)
        .into_iter()
        .map(|loaded| Installed {
            loaded,
            built_in: false,
            superseded: false,
            replaced: false,
        })
        .collect();
    let mut newest: HashMap<String, u32> = HashMap::new();
    for t in &user {
        let v = newest.entry(t.loaded.template.id.clone()).or_insert(0);
        *v = (*v).max(t.loaded.template.version);
    }
    let mut kept_newest = HashSet::new();
    for t in &mut user {
        let id = &t.loaded.template.id;
        let is_newest = newest.get(id) == Some(&t.loaded.template.version);
        // Two folders with the same id and version: only the first one is offered.
        t.superseded = !(is_newest && kept_newest.insert(id.clone()));
    }
    let mut all = user;
    for loaded in load_all_in(bundled_dir) {
        let id = loaded.template.id.as_str();
        let own_folder = loaded.dir.file_name().is_some_and(|n| n == id);
        if !built_in_ids.contains(&id) || !own_folder {
            log::info!(
                "bundled folder ignored (not a current built-in): {}",
                loaded.dir.display()
            );
            continue;
        }
        // Replaced by one of yours: still listed (as the original), never offered.
        let replaced = newest.contains_key(id);
        all.push(Installed {
            loaded,
            built_in: true,
            superseded: replaced,
            replaced,
        });
    }
    all.sort_by_key(|t| {
        (
            t.built_in,
            t.loaded.template.id == "blank",
            t.loaded.template.name.to_lowercase(),
            std::cmp::Reverse(t.loaded.template.version),
        )
    });
    all
}

/// Every template folder in `dir`. Hidden folders (half-finished imports) are skipped; a
/// broken template is logged and skipped, never fatal.
fn load_all_in(dir: &Path) -> Vec<LoadedTemplate> {
    let Ok(entries) = fs::read_dir(dir) else {
        return vec![];
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .collect();
    dirs.sort();
    dirs.iter()
        .filter_map(|d| match load_dir(d) {
            Ok(t) => Some(t),
            Err(e) => {
                log::warn!("template skipped: {}: {e}", d.display());
                None
            }
        })
        .collect()
}

// ───────────────────────── Import ─────────────────────────

/// Install a `.nesttemplate` (zip) or a single `.nest.json` / `.json` into `user_dir`.
pub fn import(
    file: &Path,
    user_dir: &Path,
    installed: &[Installed],
    limits: Limits,
) -> Result<LoadedTemplate, String> {
    fs::create_dir_all(user_dir)
        .map_err(|e| format!("Couldn't create the templates folder: {e}"))?;
    let tmp = user_dir.join(format!(".import-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir(&tmp).map_err(|e| format!("Couldn't start the import: {e}"))?;
    let result = install_into(file, &tmp, user_dir, installed, limits);
    // Our own temporary folder: always cleaned up (on success it has been moved away).
    let _ = fs::remove_dir_all(&tmp);
    result
}

fn install_into(
    file: &Path,
    tmp: &Path,
    user_dir: &Path,
    installed: &[Installed],
    limits: Limits,
) -> Result<LoadedTemplate, String> {
    let is_json = file
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"));
    let root = if is_json {
        let text = read_capped(file, 1024 * 1024)?;
        write_new(
            &tmp.join(format!("template{TEMPLATE_SUFFIX}")),
            text.as_bytes(),
        )?;
        tmp.to_path_buf()
    } else {
        unzip_safely(file, tmp, limits)?;
        template_root(tmp)?
    };
    let loaded = load_dir(&root).map_err(|e| e.to_string())?;
    let (id, version, name) = (
        loaded.template.id.clone(),
        loaded.template.version,
        loaded.template.name.clone(),
    );

    // T1: same id as one of yours → only a newer version, installed alongside.
    let yours: Vec<&Installed> = installed
        .iter()
        .filter(|t| !t.built_in && t.loaded.template.id == id)
        .collect();
    if let Some(max) = yours.iter().map(|t| t.loaded.template.version).max() {
        if version <= max {
            return Err(format!(
                "You already have \"{name}\" version {max}. To install this one alongside it, raise \"version\" in its file above {max}."
            ));
        }
    }
    let folder = free_folder(
        user_dir,
        &if yours.is_empty() {
            id.clone()
        } else {
            format!("{id}-v{version}")
        },
    );
    fs::rename(&root, &folder).map_err(|e| format!("Couldn't install the template: {e}"))?;
    load_dir(&folder).map_err(|e| e.to_string())
}

/// The template folder inside an unpacked package: the top level, or its single subfolder.
fn template_root(dir: &Path) -> Result<PathBuf, String> {
    let has_json = |d: &Path| {
        fs::read_dir(d).ok().is_some_and(|it| {
            it.flatten()
                .any(|e| e.file_name().to_string_lossy().ends_with(TEMPLATE_SUFFIX))
        })
    };
    if has_json(dir) {
        return Ok(dir.to_path_buf());
    }
    let subdirs: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .collect();
    match subdirs.as_slice() {
        [only] if has_json(only) => Ok(only.clone()),
        _ => Err(format!("That package has no {TEMPLATE_SUFFIX} file")),
    }
}

/// `base`, or `base-2`, `base-3`… whichever doesn't exist yet.
pub(crate) fn free_folder(parent: &Path, base: &str) -> PathBuf {
    let mut n = 1;
    loop {
        let name = if n == 1 {
            base.to_string()
        } else {
            format!("{base}-{n}")
        };
        let path = parent.join(&name);
        if !path.exists() {
            return path;
        }
        n += 1;
    }
}

fn unsafe_entry(name: &str, why: &str) -> String {
    format!("That package can't be imported: \"{name}\" {why}")
}

/// Unpack every entry ourselves (never the library's extract, which can create links).
fn unzip_safely(file: &Path, dest: &Path, limits: Limits) -> Result<(), String> {
    let f = File::open(file).map_err(|e| format!("Couldn't open {}: {e}", file.display()))?;
    let mut zip =
        ZipArchive::new(f).map_err(|_| "That file isn't a Nest template package".to_string())?;
    if zip.len() > limits.max_files {
        return Err(format!(
            "That package has too many files (over {})",
            limits.max_files
        ));
    }
    let mut total: u64 = 0;
    let mut seen = HashSet::new();
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| format!("That package is damaged: {e}"))?;
        let raw = entry.name().to_string();
        if raw.contains('\\') {
            return Err(unsafe_entry(&raw, "uses \\ in its path"));
        }
        if raw.starts_with('/') {
            return Err(unsafe_entry(&raw, "is an absolute path"));
        }
        let parts: Vec<&str> = raw.split('/').filter(|p| !p.is_empty()).collect();
        if parts.is_empty() || parts[0] == "__MACOSX" || parts.last() == Some(&".DS_Store") {
            continue;
        }
        let mut clean: Vec<String> = Vec::new();
        for p in &parts {
            if *p == ".." || *p == "." {
                return Err(unsafe_entry(&raw, "points outside the package"));
            }
            if p.contains(':') {
                return Err(unsafe_entry(&raw, "has a : in its name"));
            }
            let nfc: String = p.nfc().collect();
            if sanitize_segment(&nfc).as_deref() != Ok(nfc.as_str()) {
                return Err(unsafe_entry(
                    &raw,
                    "has a name that isn't allowed on Windows or macOS",
                ));
            }
            clean.push(nfc);
        }
        if entry.is_symlink() {
            return Err(unsafe_entry(&raw, "is a link (not allowed)"));
        }
        if !seen.insert(clean.join("/").to_lowercase()) {
            return Err(unsafe_entry(
                &raw,
                "clashes with another file that differs only in capitals",
            ));
        }
        let out_path = clean.iter().fold(dest.to_path_buf(), |p, s| p.join(s));
        if entry.is_dir() {
            fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&out_path)
            .map_err(|e| format!("Couldn't unpack \"{raw}\": {e}"))?;
        // Count real bytes; the sizes written in the zip can lie.
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = entry
                .read(&mut buf)
                .map_err(|e| format!("That package is damaged: {e}"))?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > limits.max_bytes {
                return Err(format!(
                    "That package is too big (over {} MB unpacked)",
                    limits.max_bytes / (1024 * 1024)
                ));
            }
            out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn read_capped(file: &Path, cap: u64) -> Result<String, String> {
    let meta = fs::metadata(file).map_err(|e| e.to_string())?;
    if meta.len() > cap {
        return Err("That file is too big to be a template".into());
    }
    fs::read_to_string(file).map_err(|e| format!("Couldn't read {}: {e}", file.display()))
}

pub(crate) fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.write_all(bytes).map_err(|e| e.to_string())
}

// ───────────────────────── Export ─────────────────────────

/// Zip a template folder to `dest` (the Save dialog already asked before replacing a file).
/// Written to a temporary file next to it first; links are never followed.
pub fn export(t: &LoadedTemplate, dest: &Path) -> Result<(), String> {
    let parent = dest.parent().ok_or("Choose a folder to save into")?;
    let tmp = parent.join(format!(
        ".nest-export-{}.tmp",
        uuid::Uuid::new_v4().simple()
    ));
    let result = (|| -> Result<(), String> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| format!("Couldn't write there: {e}"))?;
        let mut zip = ZipWriter::new(file);
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        let top = t.template.id.clone();
        let mut files = Vec::new();
        collect_regular_files(&t.dir, &t.dir, &mut files).map_err(|e| e.to_string())?;
        for rel in files {
            let src = rel.split('/').fold(t.dir.clone(), |p, s| p.join(s));
            zip.start_file(format!("{top}/{rel}"), opts)
                .map_err(|e| e.to_string())?;
            io::copy(&mut File::open(&src).map_err(|e| e.to_string())?, &mut zip)
                .map_err(|e| e.to_string())?;
        }
        let file = zip.finish().map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        fs::rename(&tmp, dest).map_err(|e| format!("Couldn't save {}: {e}", dest.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Regular files only (links skipped), `/`-separated, relative to `root`, dot-files skipped.
fn collect_regular_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> io::Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let kind = e.file_type()?;
        if kind.is_dir() {
            collect_regular_files(root, &e.path(), out)?;
        } else if kind.is_file() {
            let rel = e.path();
            let rel = rel.strip_prefix(root).map_err(io::Error::other)?;
            let parts: Vec<String> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            out.push(parts.join("/"));
        }
    }
    Ok(())
}

// ───────────────────────── Duplicate / delete ─────────────────────────

/// An editable copy in `user_dir` with a new id (`<id>-copy`) and name ("Name (copy)").
pub fn duplicate(
    t: &LoadedTemplate,
    user_dir: &Path,
    installed: &[Installed],
) -> Result<LoadedTemplate, String> {
    fs::create_dir_all(user_dir).map_err(|e| e.to_string())?;
    let ids: HashSet<&str> = installed
        .iter()
        .map(|i| i.loaded.template.id.as_str())
        .collect();
    let base = format!("{}-copy", t.template.id);
    let new_id = (1..)
        .map(|n| {
            if n == 1 {
                base.clone()
            } else {
                format!("{base}-{n}")
            }
        })
        .find(|id| !ids.contains(id.as_str()) && !user_dir.join(id).exists())
        .expect("a free id");

    let tmp = user_dir.join(format!(".duplicate-{}", uuid::Uuid::new_v4().simple()));
    let result = (|| -> Result<LoadedTemplate, String> {
        fs::create_dir(&tmp).map_err(|e| e.to_string())?;
        copy_starters(&t.dir, &tmp)?;
        let mut json = serde_json::to_value(&t.template).map_err(|e| e.to_string())?;
        json["id"] = serde_json::Value::String(new_id.clone());
        json["name"] = serde_json::Value::String(format!("{} (copy)", t.template.name));
        json["version"] = serde_json::Value::from(1);
        let text = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())? + "\n";
        write_new(
            &tmp.join(format!("{new_id}{TEMPLATE_SUFFIX}")),
            text.as_bytes(),
        )?;
        load_dir(&tmp).map_err(|e| e.to_string())?;
        let folder = user_dir.join(&new_id);
        fs::rename(&tmp, &folder).map_err(|e| e.to_string())?;
        load_dir(&folder).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&tmp);
    }
    result
}

/// Copy a template folder's starter files (everything but its `.nest.json`) into `dst`, a
/// fresh folder of ours. Links are skipped; nothing existing is overwritten.
pub(crate) fn copy_starters(src_dir: &Path, dst: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    collect_regular_files(src_dir, src_dir, &mut files).map_err(|e| e.to_string())?;
    for rel in files.iter().filter(|r| !r.ends_with(TEMPLATE_SUFFIX)) {
        let src = rel.split('/').fold(src_dir.to_path_buf(), |p, s| p.join(s));
        let out_path = rel.split('/').fold(dst.to_path_buf(), |p, s| p.join(s));
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&out_path)
            .map_err(|e| e.to_string())?;
        io::copy(&mut File::open(&src).map_err(|e| e.to_string())?, &mut out)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Move one of *your* templates to the Recycle Bin / Trash (restorable). Built-ins can only
/// be hidden. The folder must really be inside your templates folder.
pub fn delete(t: &Installed, user_dir: &Path) -> Result<(), String> {
    if t.built_in {
        return Err("Built-in templates can't be deleted, only hidden".into());
    }
    let dir = t.loaded.dir.canonicalize().map_err(|e| e.to_string())?;
    let base = user_dir.canonicalize().map_err(|e| e.to_string())?;
    if dir == base || !dir.starts_with(&base) {
        return Err("That template isn't in your templates folder".into());
    }
    trash::delete(&dir).map_err(|e| format!("Couldn't move it to the Recycle Bin / Trash: {e}"))
}

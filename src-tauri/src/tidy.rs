//! Start-up tidy: removes only Nest's **own** leftovers from runs that stopped half-way (a
//! crash, a power cut). Never a user file, never anything Nest didn't name itself.
//!
//! - user templates folder: `.edit-<id>` / `.import-<id>` folders (a template save or import
//!   that didn't finish) and `.nest-<id>.tmp` / `.nest-export-<id>.tmp` files, older than a day;
//! - `<config>/extras/`: ready-made files for the template editor ("+ Add a file"). They only
//!   matter while an editor is open, and none is open at start, so everything from before this
//!   start goes (anything made since is kept);
//! - project folders the index knows: only `.nest-<id>.tmp` **files** directly inside them
//!   (a half-written project file), older than a day. No other file, no folders, no deeper.
//! - macOS `/Applications`: only `.Nest-moving-<uuid>` **folders** (a move to Applications
//!   that stopped half-way, app_location.rs), older than a day. Run by app_location before it
//!   offers a move, on the same thread, so it never meets a copy being made.
//!
//! `<id>` is always the 32 hex characters of a UUID (in `.Nest-moving-`, the hyphenated
//! lowercase form `uuid` prints), so a file like `notes.nest-tmp.txt` or `.nest-backup.tmp` is
//! never touched. Everything runs on a background thread.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tauri::{AppHandle, Manager};

use crate::commands::AppState;

/// Leftovers younger than this are left alone (something may still be using them).
pub const MIN_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// `<prefix><32 hex><suffix>`, exactly.
fn named(name: &str, prefix: &str, suffix: &str) -> bool {
    name.strip_prefix(prefix)
        .and_then(|rest| rest.strip_suffix(suffix))
        .is_some_and(|id| id.len() == 32 && id.chars().all(|c| c.is_ascii_hexdigit()))
}

/// A temp file from `manifest::write_json_atomic`: `.nest-<32 hex>.tmp`.
pub fn is_json_tmp(name: &str) -> bool {
    named(name, ".nest-", ".tmp")
}

fn is_export_tmp(name: &str) -> bool {
    named(name, ".nest-export-", ".tmp")
}

/// A template save or import that didn't finish.
fn is_unfinished_template(name: &str) -> bool {
    named(name, ".edit-", "") || named(name, ".import-", "")
}

/// The hidden name a move to Applications copies into first (app_location.rs).
pub const MOVING_PREFIX: &str = ".Nest-moving-";

/// `.Nest-moving-<uuid>`, the uuid exactly as `Uuid::new_v4()` prints it (36 characters,
/// lowercase, hyphenated).
fn is_moving_copy(name: &str) -> bool {
    name.strip_prefix(MOVING_PREFIX)
        .is_some_and(|id| uuid::Uuid::try_parse(id).is_ok_and(|u| u.hyphenated().to_string() == id))
}

fn older_than(meta: &fs::Metadata, cutoff: SystemTime) -> bool {
    meta.modified().is_ok_and(|t| t < cutoff)
}

fn remove_file(path: &Path, removed: &mut Vec<PathBuf>) {
    match fs::remove_file(path) {
        Ok(()) => removed.push(path.to_path_buf()),
        Err(e) => log::warn!("tidy: couldn't remove {}: {e}", path.display()),
    }
}

fn remove_dir(path: &Path, removed: &mut Vec<PathBuf>) {
    // remove_dir_all never follows links: it only removes what's inside this folder.
    match fs::remove_dir_all(path) {
        Ok(()) => removed.push(path.to_path_buf()),
        Err(e) => log::warn!("tidy: couldn't remove {}: {e}", path.display()),
    }
}

/// Entries directly inside `dir`, with their own metadata (links are not followed).
fn entries(dir: &Path) -> Vec<(String, PathBuf, fs::Metadata)> {
    let Ok(list) = fs::read_dir(dir) else {
        return vec![];
    };
    list.flatten()
        .filter_map(|e| {
            let path = e.path();
            let meta = fs::symlink_metadata(&path).ok()?;
            Some((e.file_name().to_string_lossy().into_owned(), path, meta))
        })
        .collect()
}

/// The user templates folder: unfinished saves/imports and Nest's temp files, older than a day.
pub fn templates_folder(dir: &Path, now: SystemTime) -> Vec<PathBuf> {
    let cutoff = now - MIN_AGE;
    let mut removed = Vec::new();
    for (name, path, meta) in entries(dir) {
        if !older_than(&meta, cutoff) {
            continue;
        }
        if meta.is_dir() && is_unfinished_template(&name) {
            remove_dir(&path, &mut removed);
        } else if meta.is_file() && (is_json_tmp(&name) || is_export_tmp(&name)) {
            remove_file(&path, &mut removed);
        }
    }
    removed
}

/// `<config>/extras/`: everything from before `started`, then the folder itself if empty.
pub fn extras_folder(dir: &Path, started: SystemTime) -> Vec<PathBuf> {
    let mut removed = Vec::new();
    for (_, path, meta) in entries(dir) {
        if !older_than(&meta, started) {
            continue;
        }
        if meta.is_dir() {
            remove_dir(&path, &mut removed);
        } else {
            remove_file(&path, &mut removed); // a file, or a link (only the link goes)
        }
    }
    // Only succeeds when empty: something added since start keeps it.
    if fs::remove_dir(dir).is_ok() {
        removed.push(dir.to_path_buf());
    }
    removed
}

/// Project folders: only `.nest-<id>.tmp` files directly inside each one, older than a day.
pub fn project_folders(dirs: &[PathBuf], now: SystemTime) -> Vec<PathBuf> {
    let cutoff = now - MIN_AGE;
    let mut removed = Vec::new();
    for dir in dirs {
        for (name, path, meta) in entries(dir) {
            if meta.is_file() && is_json_tmp(&name) && older_than(&meta, cutoff) {
                remove_file(&path, &mut removed);
            }
        }
    }
    removed
}

/// `/Applications` (macOS): only Nest's own half-made copies, `.Nest-moving-<uuid>` folders
/// (never a file or a link), older than a day. Nothing else there is ever looked at twice.
pub fn applications_folder(dir: &Path, now: SystemTime) -> Vec<PathBuf> {
    let cutoff = now - MIN_AGE;
    let mut removed = Vec::new();
    for (name, path, meta) in entries(dir) {
        if meta.is_dir() && is_moving_copy(&name) && older_than(&meta, cutoff) {
            remove_dir(&path, &mut removed);
        }
    }
    removed
}

/// Run the whole tidy once, in the background, shortly after start.
pub fn start(app: &AppHandle, started: SystemTime) {
    let app = app.clone();
    std::thread::spawn(move || {
        // Looks in the synced folder and project folders: not before a window shows after a
        // hidden start on a Mac (folder_access.rs).
        crate::folder_access::wait(&app);
        let state = app.state::<AppState>();
        let now = SystemTime::now();
        let mut removed = templates_folder(&state.user_templates, now);
        removed.extend(extras_folder(&state.config_dir.join("extras"), started));

        // The synced folder (Devices, template sync): Nest's own temp files only.
        let settings = state.settings.lock().map(|s| s.clone());
        match &settings {
            Ok(s) => {
                if let Some(shared) = &s.shared_folder {
                    let dev = cfg!(debug_assertions);
                    crate::devices::clean_temp_files(&crate::devices::devices_dir(shared, dev));
                    let own = s
                        .device_id
                        .as_deref()
                        .and_then(crate::devices::valid_device_id);
                    if let Some(id) = own {
                        let root = crate::template_sync::templates_root(shared, dev);
                        crate::devices::clean_temp_files(&root.join(id));
                    }
                }
            }
            Err(_) => log::warn!("tidy: settings are busy, skipped the synced folder"),
        }

        let folders = match state.index.lock().map(|i| i.project_folders()) {
            Ok(Ok(f)) => f,
            Ok(Err(e)) => {
                log::warn!("tidy: couldn't read the project list: {e}");
                vec![]
            }
            Err(_) => {
                log::warn!("tidy: the project list is busy, skipped project folders");
                vec![]
            }
        };
        removed.extend(project_folders(&folders, now));

        for p in &removed {
            log::info!("tidy: removed Nest's leftover {}", p.display());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0123456789abcdef0123456789abcdef";
    const ID2: &str = "fedcba9876543210fedcba9876543210";

    /// Make a file or folder look `by` old.
    fn age(path: &Path, by: Duration) {
        let mut options = fs::File::options();
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // FILE_WRITE_ATTRIBUTES; FILE_FLAG_BACKUP_SEMANTICS (needed to open a folder).
            options.access_mode(0x100).custom_flags(0x0200_0000);
        }
        #[cfg(not(windows))]
        options.read(true);
        let f = options.open(path).unwrap();
        f.set_modified(SystemTime::now() - by).unwrap();
    }

    const OLD: Duration = Duration::from_secs(2 * 24 * 60 * 60);

    #[test]
    fn names_are_matched_exactly() {
        assert!(is_json_tmp(&format!(".nest-{ID}.tmp")));
        assert!(!is_json_tmp(".nest-backup.tmp"));
        assert!(!is_json_tmp("notes.nest-tmp.txt"));
        assert!(!is_json_tmp(&format!("x.nest-{ID}.tmp")));
        assert!(!is_json_tmp(&format!(".nest-{ID}.tmp.txt")));
        assert!(is_export_tmp(&format!(".nest-export-{ID}.tmp")));
        assert!(is_unfinished_template(&format!(".edit-{ID}")));
        assert!(is_unfinished_template(&format!(".import-{ID}")));
        assert!(!is_unfinished_template(".edit-my-notes"));
        assert!(!is_unfinished_template(&format!("edit-{ID}")));
    }

    #[test]
    fn moving_copies_are_matched_exactly() {
        let u = "0f3e11aa-1234-4abc-8def-0123456789ab";
        assert!(is_moving_copy(&format!(".Nest-moving-{u}")));
        assert!(is_moving_copy(&format!(
            ".Nest-moving-{}",
            uuid::Uuid::new_v4()
        )));
        for name in [
            format!(".Nest-moving-{}", u.to_uppercase()),
            format!(".Nest-moving-{}", u.replace('-', "")),
            format!(".Nest-moving-{{{u}}}"),
            format!(".Nest-moving-{u}.app"),
            format!(".Nest-moving-{u}x"),
            format!("Nest-moving-{u}"),
            format!(".nest-moving-{u}"),
            format!("x.Nest-moving-{u}"),
            ".Nest-moving-".to_string(),
            ".Nest-moving-notes".to_string(),
            "Nest.app".to_string(),
        ] {
            assert!(!is_moving_copy(&name), "{name}");
        }
    }

    #[test]
    fn applications_lose_only_old_half_made_nest_copies() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let u1 = "0f3e11aa-1234-4abc-8def-0123456789ab";
        let u2 = "1f3e11aa-1234-4abc-8def-0123456789ab";
        let old_copy = dir.join(format!(".Nest-moving-{u1}"));
        let new_copy = dir.join(format!(".Nest-moving-{u2}"));
        let nest = dir.join("Nest.app");
        let other_app = dir.join("Resolve.app");
        // Not the uppercase form: a Mac's disk ignores case, so that's the same folder.
        let lookalike = dir.join(format!(".Nest-moving-{}", u1.replace('-', "")));
        for d in [&old_copy, &new_copy, &nest, &other_app, &lookalike] {
            fs::create_dir_all(d.join("Contents")).unwrap();
        }
        // A file and a link with the exact name: never touched (folders only).
        let file_named = dir.join(format!(
            ".Nest-moving-{}",
            "2f3e11aa-1234-4abc-8def-0123456789ab"
        ));
        fs::write(&file_named, "x").unwrap();
        let link_named = dir.join(format!(
            ".Nest-moving-{}",
            "3f3e11aa-1234-4abc-8def-0123456789ab"
        ));
        #[cfg(unix)]
        std::os::unix::fs::symlink(&other_app, &link_named).unwrap();
        for p in [&old_copy, &nest, &other_app, &lookalike, &file_named] {
            age(p, OLD);
        }
        age(&new_copy, Duration::from_secs(60));

        let removed = applications_folder(dir, SystemTime::now());
        assert_eq!(removed, std::slice::from_ref(&old_copy));
        for p in [&new_copy, &nest, &other_app, &lookalike, &file_named] {
            assert!(p.exists(), "{} kept", p.display());
        }
        #[cfg(unix)]
        {
            assert!(fs::symlink_metadata(&link_named).is_ok(), "link kept");
            assert!(other_app.join("Contents").is_dir(), "link target untouched");
        }
        assert!(applications_folder(&dir.join("nope"), SystemTime::now()).is_empty());
    }

    #[test]
    fn templates_folder_loses_only_old_nest_leftovers() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let old_edit = dir.join(format!(".edit-{ID}"));
        let new_edit = dir.join(format!(".edit-{ID2}"));
        let old_import = dir.join(format!(".import-{ID}"));
        for d in [&old_edit, &new_edit, &old_import] {
            fs::create_dir(d).unwrap();
            fs::write(d.join("x.nest.json"), "{}").unwrap();
        }
        let old_export = dir.join(format!(".nest-export-{ID}.tmp"));
        let old_json = dir.join(format!(".nest-{ID}.tmp"));
        let new_json = dir.join(format!(".nest-{ID2}.tmp"));
        let template = dir.join("grading");
        fs::create_dir(&template).unwrap();
        let lookalike_dir = dir.join(format!(".nest-{ID2}.tmp-folder"));
        fs::create_dir(&lookalike_dir).unwrap();
        let dir_named_like_tmp = template.join(format!(".nest-{ID}.tmp"));
        fs::create_dir(&dir_named_like_tmp).unwrap();
        for f in [&old_export, &old_json, &new_json] {
            fs::write(f, "x").unwrap();
        }
        let user_file = dir.join("notes.nest-tmp.txt");
        fs::write(&user_file, "mine").unwrap();
        for p in [
            &old_edit,
            &old_import,
            &old_export,
            &old_json,
            &user_file,
            &template,
            &lookalike_dir,
        ] {
            age(p, OLD);
        }

        let mut removed = templates_folder(dir, SystemTime::now());
        removed.sort();
        let mut expected = vec![
            old_edit.clone(),
            old_import.clone(),
            old_export.clone(),
            old_json.clone(),
        ];
        expected.sort();
        assert_eq!(removed, expected);
        for p in [&new_edit, &new_json, &user_file, &template, &lookalike_dir] {
            assert!(p.exists(), "{} kept", p.display());
        }
        assert!(dir_named_like_tmp.is_dir(), "never looks inside templates");
    }

    #[test]
    fn extras_are_emptied_but_new_ones_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let extras = tmp.path().join("extras");
        let a = extras.join(ID);
        fs::create_dir_all(&a).unwrap();
        fs::write(a.join("Project info.txt"), "x").unwrap();
        fs::write(extras.join("stray.txt"), "x").unwrap();
        age(&a, Duration::from_secs(60));
        age(&extras.join("stray.txt"), Duration::from_secs(60));
        let started = SystemTime::now() - Duration::from_secs(10);

        let removed = extras_folder(&extras, started);
        assert_eq!(removed.len(), 3, "{removed:?}");
        assert!(!extras.exists(), "empty extras folder removed");

        // A file added after start (an editor opened quickly) stays, and so does the folder.
        let b = extras.join(ID2);
        fs::create_dir_all(&b).unwrap();
        fs::write(b.join("Project info.txt"), "x").unwrap();
        let removed = extras_folder(&extras, started);
        assert!(removed.is_empty(), "{removed:?}");
        assert!(b.join("Project info.txt").is_file());
        // No extras folder at all: nothing to do.
        assert!(extras_folder(&tmp.path().join("nope"), started).is_empty());
    }

    #[test]
    fn project_folders_lose_only_old_manifest_temps_directly_inside() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("2609_AC-L01_Promo");
        let deeper = project.join("01_BRIEF");
        fs::create_dir_all(&deeper).unwrap();
        fs::write(project.join(".project.json"), "{}").unwrap();
        let old_tmp = project.join(format!(".nest-{ID}.tmp"));
        let new_tmp = project.join(format!(".nest-{ID2}.tmp"));
        let deep_tmp = deeper.join(format!(".nest-{ID}.tmp"));
        let user_files = [
            project.join("notes.nest-tmp.txt"),
            project.join(".nest-backup.tmp"),
            project.join(format!("render.nest-{ID}.tmp")),
        ];
        let tmp_named_folder = project.join(format!(".nest-{ID2}0.tmp"));
        let folder_like_tmp = project.join(format!(".nest-{}.tmp", "a".repeat(32)));
        fs::create_dir(&tmp_named_folder).unwrap();
        fs::create_dir(&folder_like_tmp).unwrap();
        for f in [&old_tmp, &new_tmp, &deep_tmp]
            .into_iter()
            .chain(user_files.iter())
        {
            fs::write(f, "x").unwrap();
            age(f, OLD);
        }
        age(&new_tmp, Duration::from_secs(60));
        age(&folder_like_tmp, OLD);
        // A folder the index doesn't know, with an old temp inside: untouched.
        let unknown = tmp.path().join("Other");
        fs::create_dir(&unknown).unwrap();
        let unknown_tmp = unknown.join(format!(".nest-{ID}.tmp"));
        fs::write(&unknown_tmp, "x").unwrap();
        age(&unknown_tmp, OLD);

        let removed = project_folders(
            &[project.clone(), tmp.path().join("gone")],
            SystemTime::now(),
        );
        assert_eq!(removed, std::slice::from_ref(&old_tmp));
        assert!(!old_tmp.exists());
        for p in [
            &new_tmp,
            &deep_tmp,
            &unknown_tmp,
            &folder_like_tmp,
            &tmp_named_folder,
        ]
        .into_iter()
        .chain(user_files.iter())
        {
            assert!(p.exists(), "{} kept", p.display());
        }
        assert!(project.join(".project.json").is_file());
    }
}

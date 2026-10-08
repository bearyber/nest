//! Archive / Unarchive (M5 P2): move a project folder between a jobs folder and the archive
//! folder **on the same drive**, with one rename. Nothing is copied or deleted (locked #4);
//! a project is archived because of where its folder is (locked #5), and the manifest only
//! notes when and from where (`archivedAt`, `archivedFrom`).
//!
//! Another drive is refused with a plain message: a move across drives is a copy plus a delete,
//! and the Recycle Bin can't be trusted to keep big or network folders (it deletes them for
//! good), so Nest leaves that to the user.

use std::io;
use std::path::{Component, Path, PathBuf};

use serde_json::Value as Json;
use unicode_normalization::UnicodeNormalization;

use crate::manifest::{read_json_object, sync_dir, write_json_atomic};
use crate::names::sanitize_segment;
use crate::plan::MANIFEST_NAME;

pub const OTHER_DRIVE: &str = "Your archive folder is on another drive, so Nest can't move the project there in one step. Drag the folder there yourself in Explorer or Finder; Nest will see it as archived.";
pub const OTHER_DRIVE_BACK: &str = "The archive folder is on another drive than your jobs folder, so Nest can't move it back in one step. Drag the folder back yourself in Explorer or Finder.";

/// A checked move: one folder, renamed into `to`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovePlan {
    pub from: PathBuf,
    pub to: PathBuf,
    /// The folder name (unchanged by the move).
    pub name: String,
}

/// Where `from` would go inside `dest_root`, or why it can't. Changes nothing on disk.
pub fn plan_move(from: &Path, dest_root: &Path, unarchive: bool) -> Result<MovePlan, String> {
    let name = from
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("This project folder has no usable name")?
        .to_string();
    // The name is never changed: it must already be one the sanitiser would make (locked #8).
    // Compared in NFC: older Mac drives (HFS+) hand back decomposed names for Nest's own folders.
    let nfc: String = name.nfc().collect();
    if sanitize_segment(&name).as_deref() != Ok(nfc.as_str()) {
        return Err(format!(
            "The folder name \"{name}\" has characters Nest doesn't move safely. Move it yourself in Explorer or Finder."
        ));
    }
    if !from.is_dir() {
        return Err("The project folder isn't there any more. Rescan and try again.".into());
    }
    if !dest_root.is_dir() {
        return Err(format!(
            "{} isn't there (is its drive connected?)",
            if unarchive {
                "The jobs folder"
            } else {
                "The archive folder"
            }
        ));
    }
    if is_cloud(from) || is_cloud(dest_root) {
        return Err("This is in a cloud-synced folder (Google Drive, OneDrive, iCloud, Dropbox). Moving it there can remove it from your other computers, so Nest leaves that to you.".into());
    }
    let to = dest_root.join(&name);
    if to.symlink_metadata().is_ok() {
        return Err(format!(
            "There's already a folder called \"{name}\" in {}. Nothing was moved.",
            if unarchive {
                "the jobs folder"
            } else {
                "the archive"
            }
        ));
    }
    if !same_drive(from, dest_root) {
        return Err(if unarchive {
            OTHER_DRIVE_BACK
        } else {
            OTHER_DRIVE
        }
        .into());
    }
    Ok(MovePlan {
        from: from.to_path_buf(),
        to,
        name,
    })
}

/// Do the move: one rename. Re-checks the destination first; nothing is ever overwritten.
pub fn apply_move(plan: &MovePlan, unarchive: bool) -> Result<(), String> {
    if plan.to.symlink_metadata().is_ok() {
        return Err(format!(
            "There's already a folder called \"{}\" there. Nothing was moved.",
            plan.name
        ));
    }
    std::fs::rename(&plan.from, &plan.to).map_err(|e| move_error(&e, unarchive))?;
    for dir in [plan.from.parent(), plan.to.parent()].into_iter().flatten() {
        let _ = sync_dir(dir); // best effort: the rename itself already happened
    }
    Ok(())
}

/// Plain words for a failed rename. Nothing was moved in every case.
pub fn move_error(e: &io::Error, unarchive: bool) -> String {
    // Windows codes: 17 = not the same device, 32 = sharing violation (a file is open).
    let windows_code = |code| cfg!(windows) && e.raw_os_error() == Some(code);
    if e.kind() == io::ErrorKind::CrossesDevices || windows_code(17) {
        return if unarchive {
            OTHER_DRIVE_BACK
        } else {
            OTHER_DRIVE
        }
        .into();
    }
    // Windows refuses to move a folder while a file in it is open; macOS never does, so there
    // "permission denied" really means permission.
    if windows_code(32) || (cfg!(windows) && e.kind() == io::ErrorKind::PermissionDenied) {
        return "Something has a file in this project open (DaVinci Resolve? an Explorer window?). Close it and try again. Nothing was moved.".into();
    }
    if e.kind() == io::ErrorKind::PermissionDenied {
        return "Nest isn't allowed to move this folder. Check that the folder or drive isn't locked or read-only, and that Nest has access to it (System Settings → Privacy & Security → Files and Folders). Nothing was moved.".into();
    }
    if e.kind() == io::ErrorKind::NotFound {
        return "The project folder isn't there any more. Rescan and try again.".into();
    }
    format!("Couldn't move the folder ({e}). Nothing was moved.")
}

/// The folder an unarchived project goes back to.
///
/// Where it came from (`archivedFrom`), if that is one of this computer's jobs folders or a
/// folder directly inside one (e.g. `Jobs/2025`), so Nest's scan still finds it there. A
/// manifest is hand-editable and synced, so its path is never trusted on its own.
/// Otherwise a jobs folder on the same drive as the project (a one-step move), else the first
/// jobs folder that's there.
pub fn unarchive_root(
    archived_from: Option<&str>,
    jobs_roots: &[PathBuf],
    project: &Path,
) -> Option<PathBuf> {
    let real = |p: &Path| std::fs::canonicalize(p).ok();
    let roots: Vec<PathBuf> = jobs_roots.iter().filter(|r| r.is_dir()).cloned().collect();
    let came_from = archived_from.map(Path::new).filter(|from| {
        let Some(from) = real(from) else {
            return false;
        };
        roots
            .iter()
            .filter_map(|r| real(r))
            .any(|root| from == root || from.parent() == Some(root.as_path()))
    });
    came_from
        .map(Path::to_path_buf)
        .or_else(|| roots.iter().find(|r| same_drive(r, project)).cloned())
        .or_else(|| roots.first().cloned())
}

/// Note the move in the manifest at the project's new place: `archivedAt` + `archivedFrom`
/// when archiving, both removed when unarchiving. Atomic, every other field kept (#5, #6).
pub fn mark_manifest(
    dir: &Path,
    expected_id: &str,
    archived_from: Option<&Path>,
) -> Result<(), String> {
    let file = dir.join(MANIFEST_NAME);
    let mut manifest = read_json_object(&file)?;
    if manifest["id"].as_str() != Some(expected_id) {
        return Err("the project file belongs to another project".into());
    }
    let obj = manifest.as_object_mut().ok_or("not a JSON object")?;
    match archived_from {
        Some(from) => {
            obj.insert(
                "archivedAt".into(),
                Json::String(chrono::Local::now().to_rfc3339()),
            );
            obj.insert(
                "archivedFrom".into(),
                Json::String(from.to_string_lossy().into_owned()),
            );
        }
        None => {
            obj.remove("archivedAt");
            obj.remove("archivedFrom");
        }
    }
    write_json_atomic(&file, &manifest, true).map_err(|e| e.to_string())
}

/// Folders a cloud app syncs. Moving files out of them can delete them on the other computers.
pub fn is_cloud(path: &Path) -> bool {
    path.components().any(|c| {
        let Component::Normal(s) = c else {
            return false;
        };
        let s = s.to_string_lossy().to_lowercase();
        s == "cloudstorage"
            || s == "mobile documents"
            || s == "dropbox"
            || s == "google drive"
            || s == "my drive"
            || s == "icloud drive"
            || s == "iclouddrive"
            || s.starts_with("onedrive")
    })
}

/// Best-effort "same drive" check before asking. The rename's own error is the final word.
#[cfg(unix)]
fn same_drive(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev(),
        _ => false,
    }
}

/// Windows: the drive letter or network share both paths are on.
#[cfg(not(unix))]
fn same_drive(a: &Path, b: &Path) -> bool {
    let prefix = |p: &Path| {
        let p = std::fs::canonicalize(p).ok()?;
        match p.components().next()? {
            Component::Prefix(x) => Some(x.as_os_str().to_string_lossy().to_lowercase()),
            _ => None,
        }
    };
    matches!((prefix(a), prefix(b)), (Some(a), Some(b)) if a == b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_folders_are_recognised() {
        for p in [
            "/Users/b/Library/CloudStorage/GoogleDrive-b@x.com/My Drive/Jobs",
            "/Users/b/Library/Mobile Documents/com~apple~CloudDocs/Jobs",
            "C:/Users/b/OneDrive - INTRVL/Jobs",
            "C:/Users/b/Dropbox/Jobs",
            "C:/Users/b/iCloudDrive/Jobs",
            "G:/My Drive/Jobs",
        ] {
            assert!(is_cloud(Path::new(p)), "{p}");
        }
        for p in [
            "D:/Jobs",
            "/Volumes/Footage/Jobs",
            "C:/Users/b/Documents/NestDev",
        ] {
            assert!(!is_cloud(Path::new(p)), "{p}");
        }
    }

    #[test]
    fn move_errors_in_plain_words() {
        let e = io::Error::from(io::ErrorKind::CrossesDevices);
        assert_eq!(move_error(&e, false), OTHER_DRIVE);
        assert_eq!(move_error(&e, true), OTHER_DRIVE_BACK);
        let e = io::Error::from(io::ErrorKind::PermissionDenied);
        assert!(move_error(&e, false).contains("Close it and try again"));
        let e = io::Error::from(io::ErrorKind::NotFound);
        assert!(move_error(&e, false).contains("Rescan"));
        let e = io::Error::other("disk on fire");
        assert!(move_error(&e, false).contains("Nothing was moved"));
    }
}

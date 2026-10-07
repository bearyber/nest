//! Folder sizes (M5 P4, spec §12a #21): how big a project is, per top-level subfolder and by
//! file type, so Bernard can see what to clear out in Finder/Explorer before archiving.
//! Read-only: this module never writes to a project folder.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// One project's size, as the details panel shows it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sizes {
    pub total: u64,
    pub files: u64,
    /// Top-level subfolders, biggest first. Loose files in the project folder itself are the
    /// entry with an empty name.
    pub folders: Vec<FolderSize>,
    /// Folders or files that couldn't be read (counted as 0).
    pub unreadable: u32,
    /// ms since 1970, this computer's clock.
    pub measured_at: i64,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderSize {
    pub name: String,
    pub total: u64,
    pub video: u64,
    pub audio: u64,
    pub images: u64,
    pub other: u64,
}

#[derive(Clone, Copy)]
enum Kind {
    Video,
    Audio,
    Image,
    Other,
}

/// By extension, lower-cased. Camera and post formats Bernard's work actually produces.
fn kind(path: &Path) -> Kind {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "mov" | "mp4" | "m4v" | "mxf" | "braw" | "r3d" | "ari" | "arx" | "avi" | "mkv" | "webm"
        | "mts" | "m2ts" | "prores" | "dnxhd" | "crm" => Kind::Video,
        "wav" | "aif" | "aiff" | "mp3" | "flac" | "m4a" | "aac" | "ogg" | "bwf" | "caf" => {
            Kind::Audio
        }
        "jpg" | "jpeg" | "png" | "tif" | "tiff" | "exr" | "dpx" | "psd" | "psb" | "cr2" | "cr3"
        | "arw" | "nef" | "raf" | "dng" | "heic" | "heif" | "gif" | "webp" | "bmp" | "svg"
        | "ai" => Kind::Image,
        _ => Kind::Other,
    }
}

/// Measure a project folder. Never follows links (a link counts as 0), never fails: what
/// can't be read is counted in `unreadable`.
pub fn measure(dir: &Path, now_ms: i64) -> Sizes {
    let mut sizes = Sizes {
        measured_at: now_ms,
        ..Default::default()
    };
    let mut root = FolderSize::default();
    let Ok(entries) = fs::read_dir(dir) else {
        sizes.unreadable += 1;
        return sizes;
    };
    for entry in entries {
        let Ok(entry) = entry else {
            sizes.unreadable += 1;
            continue;
        };
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            sizes.unreadable += 1;
            continue;
        };
        if meta.is_dir() {
            let mut folder = FolderSize {
                name: entry.file_name().to_string_lossy().into_owned(),
                ..Default::default()
            };
            walk(&path, &mut folder, &mut sizes);
            sizes.folders.push(folder);
        } else if meta.is_file() {
            add(&mut root, &path, meta.len());
            sizes.files += 1;
        }
        // Links and anything else: skipped.
    }
    if root.total > 0 {
        sizes.folders.push(root);
    }
    sizes
        .folders
        .sort_by(|a, b| b.total.cmp(&a.total).then_with(|| a.name.cmp(&b.name)));
    sizes.total = sizes.folders.iter().map(|f| f.total).sum();
    sizes
}

fn walk(dir: &Path, folder: &mut FolderSize, sizes: &mut Sizes) {
    let Ok(entries) = fs::read_dir(dir) else {
        sizes.unreadable += 1;
        return;
    };
    for entry in entries {
        let Ok(entry) = entry else {
            sizes.unreadable += 1;
            continue;
        };
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            sizes.unreadable += 1;
            continue;
        };
        if meta.is_dir() {
            walk(&path, folder, sizes);
        } else if meta.is_file() {
            add(folder, &path, meta.len());
            sizes.files += 1;
        }
    }
}

fn add(folder: &mut FolderSize, path: &Path, len: u64) {
    folder.total += len;
    match kind(path) {
        Kind::Video => folder.video += len,
        Kind::Audio => folder.audio += len,
        Kind::Image => folder.images += len,
        Kind::Other => folder.other += len,
    }
}

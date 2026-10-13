//! Files waiting to go into a template: from a project folder ("From one of your project
//! folders…") or dragged into the editor. Nothing is copied until the template is saved, and
//! then each file is checked again (it may have changed while the editor was open).

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::template::Template;
use crate::template_io::LIMITS;

/// Files bigger than this stay behind: footage, renders and audio don't belong in a template.
pub const MAX_FILE_BYTES: u64 = 100 * 1024 * 1024;

/// Where a staged file is on disk: `base` (a folder the user picked or dropped from) and the
/// path below it as it is on disk (`/`-separated, e.g. NFD on a Mac). Only the part below
/// `base` is checked for links again at save: `base` itself is where the user pointed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnDisk {
    pub base: PathBuf,
    pub rel: String,
    pub size: u64,
}

/// Template file name (`/`-separated, cleaned for both OSes) → where it is on disk.
#[derive(Debug, Clone, Default)]
pub struct Staged {
    files: BTreeMap<String, OnDisk>,
}

impl Staged {
    pub fn insert(&mut self, name: String, file: OnDisk) {
        self.files.insert(name, file);
    }

    pub fn contains(&self, name: &str) -> bool {
        self.files.contains_key(name)
    }

    pub fn names(&self) -> BTreeSet<String> {
        self.files.keys().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn bytes(&self) -> u64 {
        self.files.values().map(|f| f.size).sum()
    }

    /// Copy the staged files `draft` uses into `dst` (a fresh folder of ours), under their
    /// template names. A draft file that is neither staged nor in `others` (the starter files
    /// already in the template) is refused. Each file is opened first and the open file is
    /// checked (a plain file, under 100 MB), no folder below its `base` is a link, and at most
    /// 100 MB is read from it. Nothing existing is overwritten; each copy is forced to disk.
    pub fn copy_used(
        &self,
        draft: &Template,
        others: &BTreeSet<String>,
        dst: &Path,
    ) -> Result<(), String> {
        let mut used = BTreeSet::new();
        for f in &draft.files {
            if self.files.contains_key(&f.from) {
                used.insert(f.from.as_str());
            } else if !(others.contains(&f.from)
                || f.from.ends_with('/') && others.iter().any(|o| o.starts_with(&f.from)))
            {
                return Err(format!(
                    "\"{}\" isn't one of this template's files. Add it again.",
                    f.from
                ));
            }
        }
        let mut total = 0u64;
        for name in used {
            let file = &self.files[name];
            let changed = || format!("\"{name}\" changed since it was added. Add it again.");
            let mut src = file.base.clone();
            for seg in file.rel.split('/') {
                src.push(seg);
                let meta = fs::symlink_metadata(&src).map_err(|_| changed())?;
                if meta.file_type().is_symlink() {
                    return Err(changed());
                }
            }
            let input = File::open(&src).map_err(|_| changed())?;
            let meta = input.metadata().map_err(|_| changed())?;
            if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
                return Err(changed());
            }
            let out_path = name.split('/').fold(dst.to_path_buf(), |p, s| p.join(s));
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let copy = || -> io::Result<u64> {
                let mut out = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&out_path)?;
                let n = io::copy(&mut input.take(MAX_FILE_BYTES + 1), &mut out)?;
                out.sync_all()?;
                Ok(n)
            };
            let n = copy().map_err(|e| format!("Couldn't copy \"{name}\": {e}"))?;
            total += n;
            if n > MAX_FILE_BYTES || total > LIMITS.max_bytes {
                return Err(changed());
            }
        }
        Ok(())
    }
}

//! Client kits (v0.5): a client's brand folder (logos, fonts, guides, videos) copied into a new
//! project, into the folder the template marks with `"kit": true`.
//!
//! Read-only: a kit is found and listed here, never changed (locked #4). Which kit belongs to a
//! client is per computer (Settings): a folder chosen for that client wins; else a folder inside
//! the Kits folder named after the client's code or name (NFC, ignoring capitals; the code wins
//! when both exist). Left out of a listing, like "make a template from a folder": `.project.json`,
//! hidden files and system clutter, editing apps' caches, and links / junctions (never followed).
//! No size limit: New Project shows the size first.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;
use unicode_normalization::UnicodeNormalization;

use crate::plan::{KitState, Plan, MANIFEST_NAME};
use crate::template_from_folder::{skip_reason, SkipReason, Skipped};

/// Deeper than this is a loop or not a kit.
const MAX_DEPTH: usize = 32;
/// Looked at (including what's left out) before giving up: a whole drive picked by mistake.
const MAX_ITEMS: usize = 100_000;

/// Where a client's kit was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum KitSource {
    /// A folder chosen for this client in Settings.
    Chosen,
    /// A folder inside the Kits folder named after the client.
    KitsFolder,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundKit {
    pub dir: PathBuf,
    pub source: KitSource,
}

/// One file in a kit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KitFile {
    /// `/`-separated, relative to the kit folder, names exactly as on disk (for reading).
    pub rel: String,
    pub size: u64,
    /// Modified time in nanoseconds since 1970 (0 if the system doesn't say).
    pub modified: i128,
}

/// What a kit holds, read once and cached by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KitListing {
    pub dir: PathBuf,
    /// Folders inside the kit, `/`-separated names as on disk, parents first.
    pub folders: Vec<String>,
    /// Sorted by path.
    pub files: Vec<KitFile>,
    pub bytes: u64,
    /// Left out: links, unreadable items, names that aren't valid text (clutter and caches too).
    pub skipped: Vec<Skipped>,
    /// Count, total size and a hash of every path, size and modified time: Create refuses when
    /// this differs from what the preview showed.
    pub fingerprint: String,
}

/// Compare names the way a person would: NFC, ignoring capitals and spaces around.
fn match_key(s: &str) -> String {
    s.trim().nfc().collect::<String>().to_lowercase()
}

/// Find `code` / `name`'s kit. `client_kits` is keyed by client code (any capitals).
pub fn find_kit(
    kits_folder: Option<&Path>,
    client_kits: &BTreeMap<String, PathBuf>,
    code: &str,
    name: &str,
) -> Option<FoundKit> {
    if let Some(dir) = client_kits
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(code))
        .map(|(_, dir)| dir)
    {
        return Some(FoundKit {
            dir: dir.clone(),
            source: KitSource::Chosen,
        });
    }
    let folder = kits_folder?;
    let code_key = match_key(code);
    let name_key = match_key(name);
    let mut by_name = None;
    for entry in fs::read_dir(folder).ok()?.flatten() {
        let os_name = entry.file_name();
        let Some(entry_name) = os_name.to_str() else {
            continue;
        };
        let Ok(meta) = fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if !meta.is_dir() || skip_reason(entry_name, &meta).is_some() {
            continue;
        }
        let key = match_key(entry_name);
        if !code_key.is_empty() && key == code_key {
            return Some(FoundKit {
                dir: entry.path(),
                source: KitSource::KitsFolder,
            });
        }
        if by_name.is_none() && !name_key.is_empty() && key == name_key {
            by_name = Some(entry.path());
        }
    }
    by_name.map(|dir| FoundKit {
        dir,
        source: KitSource::KitsFolder,
    })
}

/// List a kit (read-only). Fails only if the kit folder itself can't be read.
pub fn list_kit(dir: &Path) -> Result<KitListing, String> {
    let meta = fs::metadata(dir).map_err(|e| {
        format!(
            "Couldn't open the client's folder {} ({}). Is its drive connected?",
            dir.display(),
            crate::error::io_reason(&e)
        )
    })?;
    if !meta.is_dir() {
        return Err(format!("{} isn't a folder", dir.display()));
    }
    let mut walk = Walk::default();
    walk.dir(dir, "", 0)?;
    walk.files.sort_by(|a, b| a.rel.cmp(&b.rel));
    let fingerprint = fingerprint(&walk.folders, &walk.files);
    Ok(KitListing {
        dir: dir.to_path_buf(),
        folders: walk.folders,
        bytes: walk.files.iter().map(|f| f.size).sum(),
        files: walk.files,
        skipped: walk.skipped,
        fingerprint,
    })
}

#[derive(Default)]
struct Walk {
    folders: Vec<String>,
    files: Vec<KitFile>,
    skipped: Vec<Skipped>,
    seen: usize,
}

impl Walk {
    fn dir(&mut self, dir: &Path, rel: &str, depth: usize) -> Result<(), String> {
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) if depth == 0 => {
                return Err(format!(
                    "Couldn't read the client's folder {} ({})",
                    dir.display(),
                    crate::error::io_reason(&e)
                ))
            }
            Err(_) => {
                self.skip(rel.to_string(), SkipReason::Unreadable);
                return Ok(());
            }
        };
        let mut list: Vec<_> = entries
            .filter_map(|e| match e {
                Ok(e) => Some(e),
                Err(_) => {
                    self.skip(join(rel, "?"), SkipReason::Unreadable);
                    None
                }
            })
            .collect();
        list.sort_by_key(|e| e.file_name());
        for e in list {
            self.seen += 1;
            if self.seen > MAX_ITEMS {
                return Err(format!(
                    "That folder holds more than {MAX_ITEMS} items. Choose the folder with just the client's files."
                ));
            }
            let os_name = e.file_name();
            let Some(name) = os_name.to_str() else {
                self.skip(join(rel, &os_name.to_string_lossy()), SkipReason::BadName);
                continue;
            };
            let path = join(rel, name);
            if name == MANIFEST_NAME {
                continue; // Nest's own file: never copied into another project.
            }
            let Ok(meta) = fs::symlink_metadata(e.path()) else {
                self.skip(path, SkipReason::Unreadable);
                continue;
            };
            if let Some(reason) = skip_reason(name, &meta) {
                self.skip(path, reason);
                continue;
            }
            if meta.is_dir() {
                if depth + 1 >= MAX_DEPTH {
                    return Err(format!(
                        "That folder goes more than {MAX_DEPTH} folders deep. Choose the folder with just the client's files."
                    ));
                }
                self.folders.push(path.clone());
                self.dir(&e.path(), &path, depth + 1)?;
            } else if meta.is_file() {
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_nanos() as i128);
                self.files.push(KitFile {
                    rel: path,
                    size: meta.len(),
                    modified,
                });
            }
            // Anything else (a socket, a device): not something a kit holds; left out.
        }
        Ok(())
    }

    fn skip(&mut self, path: String, reason: SkipReason) {
        self.skipped.push(Skipped {
            path: path.nfc().collect(),
            reason,
        });
    }
}

fn join(rel: &str, name: &str) -> String {
    if rel.is_empty() {
        name.to_string()
    } else {
        format!("{rel}/{name}")
    }
}

/// `count-bytes-hash`. FNV-1a: tiny, stable, and plenty to notice a changed kit.
fn fingerprint(folders: &[String], files: &[KitFile]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    };
    for f in folders {
        eat(b"d");
        eat(f.as_bytes());
        eat(&[0]);
    }
    let mut bytes = 0u64;
    for f in files {
        eat(b"f");
        eat(f.rel.as_bytes());
        eat(&[0]);
        eat(&f.size.to_le_bytes());
        eat(&f.modified.to_le_bytes());
        bytes += f.size;
    }
    format!("{}-{}-{:016x}", files.len(), bytes, h)
}

/// The kit folder Create copies from, after checking the kit is still what the preview showed
/// (`previewed`: the fingerprint the preview had; None = no kit then). `Ok(None)` = no kit.
pub fn kit_to_copy(plan: &Plan, previewed: Option<&str>) -> Result<Option<PathBuf>, String> {
    let kit = plan.kit.as_ref().filter(|k| k.state == KitState::Ready);
    if kit.and_then(|k| k.fingerprint.as_deref()) != previewed {
        let who = plan
            .kit
            .as_ref()
            .map_or("The client", |k| k.client.as_str());
        return Err(format!(
            "{who}'s files changed since the preview. Check them and click Create again."
        ));
    }
    Ok(kit.and_then(|k| k.source.clone()))
}

/// The kit is in a Google Drive folder: online-only files are downloaded while copying.
pub fn in_google_drive(dir: &Path) -> bool {
    let s = dir.to_string_lossy();
    s.contains("Google Drive") || s.contains("GoogleDrive-") || s.contains("My Drive")
}

/// "1.2 GB" (powers of 1000, like Finder and Explorer's "size" in most places).
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} bytes");
    }
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1000.0 && unit < UNITS.len() - 1 {
        v /= 1000.0;
        unit += 1;
    }
    if v < 10.0 {
        format!("{v:.1} {}", UNITS[unit])
    } else {
        format!("{v:.0} {}", UNITS[unit])
    }
}

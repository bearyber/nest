//! Template sync (v0.5): keep your templates the same on all your computers. Rides on Devices
//! (same synced folder, same single-writer design, no network: locked decision #9).
//!
//! Layout: each computer writes only `<synced>/Nest/templates/<device-id>/` (`templates-dev/`
//! for dev builds): one `<id>.nesttemplate` per template of yours (built-ins never; your
//! customized built-ins do), made with the normal export, then `templates.json`
//! (`{ id: { version, savedAt, lineage, size } }`) written last, atomically.
//!
//! Rules:
//! - the id always comes from inside a package, never from a file name; conflict copies
//!   ("id (1).nesttemplate", "templates (1).json") are never read; forgotten computers are
//!   skipped; the synced folder itself is never created; every file is capped;
//! - a template comes in only if its version is higher than the highest this computer has ever
//!   had of that id (`template_versions_seen`), so a delete or "go back to original" sticks until
//!   a newer version is saved elsewhere. Same version saved on two computers (two edits): `rank()`
//!   (later save, then higher device id) picks one, the same on every computer. It keeps the id.
//!   Only the computer whose own edit lost saves that edit as ONE copy (new id, new lineage,
//!   "<Name> (<this computer>)"), recorded in `Book::copied` so it's never made twice, then
//!   installs the winner. (A 0.7.3 computer just takes the later save; its older copy goes to
//!   the bin.) The installed copy keeps the sender's `savedAt` and lineage, so it's never sent
//!   back as newer;
//! - a package is opened only to install it, and only if its own id and version (and size) match
//!   the index: anything else is still syncing and is tried next time;
//! - a different lineage for the same id is an unrelated template: it comes in once, as a copy
//!   with a new id and "<Name> (<computer>)". Yours is never binned for it;
//! - installing a newer copy bins the older one (decision T-A, restorable).

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::devices::{clean_temp_files, valid_device_id, MAX_FILES};
use crate::manifest::write_json_atomic;
use crate::names::sanitize_segment;
use crate::settings::TemplateSyncMeta;
use crate::template_edit::{new_id, retire_older_copies};
use crate::template_io::{self, load_installed, Installed, LIMITS, PACKAGE_EXT};

pub const INDEX_FILE: &str = "templates.json";
pub const MAX_INDEX_BYTES: u64 = 1024 * 1024;
pub const MAX_TEMPLATES: usize = 1_000;
const MAX_STR: usize = 200;
const MAX_ID: usize = 100;

/// One template in a computer's `templates.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct IndexEntry {
    pub version: u32,
    /// RFC 3339 on the saving computer's clock; empty = saved before sync was on.
    pub saved_at: String,
    pub lineage: String,
    /// The package's size in bytes: a package of another size is still syncing.
    pub size: u64,
}

pub type SyncIndex = BTreeMap<String, IndexEntry>;

/// What this computer remembers (kept in Settings, never in the template files).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Book {
    /// The highest version this computer has had of each id.
    pub seen: BTreeMap<String, u32>,
    pub meta: BTreeMap<String, TemplateSyncMeta>,
    /// Edits of this computer's kept as a copy after losing a same-version clash:
    /// `id|version|device id` (see `copy_key`). Never forgotten, so no second copy is made and
    /// deleting the copy doesn't bring it back.
    pub copied: BTreeSet<String>,
}

/// The record of one clash resolved on this computer.
pub fn copy_key(id: &str, version: u32, device_id: &str) -> String {
    format!("{id}|{version}|{device_id}")
}

/// `<synced>/Nest/templates` (or `templates-dev`).
pub fn templates_root(shared: &Path, dev_build: bool) -> PathBuf {
    shared.join("Nest").join(if dev_build {
        "templates-dev"
    } else {
        "templates"
    })
}

/// A template id as Nest makes them: lowercase letters, digits and -.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// `<id>.nesttemplate`, if that's a file name that works on both OSes.
pub fn package_name(id: &str) -> Option<String> {
    if !valid_id(id) {
        return None;
    }
    let name = format!("{id}.{PACKAGE_EXT}");
    sanitize_segment(&name).ok().filter(|n| *n == name)
}

/// The lineage given to a template that existed before it was first synced (or was imported or
/// edited): the same on every computer, so the same template on two computers is recognised as
/// one, not as two unrelated ones.
pub fn lineage_for_existing(id: &str) -> String {
    format!("id:{id}")
}

/// A brand-new template (New, duplicate, from a folder): a lineage of its own.
pub fn new_lineage() -> String {
    uuid::Uuid::new_v4().hyphenated().to_string()
}

fn instant(s: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc3339(s).ok()
}

/// `a` was saved later than `b` (an empty or unreadable time is older than any real one).
pub fn later(a: &str, b: &str) -> bool {
    instant(a) > instant(b)
}

/// Record a save, import or duplicate made on this computer: the version is now "seen" and the
/// save time is now. The lineage is kept forever once made.
pub fn note_local_save(book: &mut Book, id: &str, version: u32, fresh: bool, now: &str) {
    let seen = book.seen.entry(id.to_string()).or_insert(0);
    *seen = (*seen).max(version);
    let lineage = match book.meta.get(id) {
        Some(m) if !m.lineage.is_empty() => m.lineage.clone(),
        _ if fresh => new_lineage(),
        _ => lineage_for_existing(id),
    };
    book.meta.insert(
        id.to_string(),
        TemplateSyncMeta {
            saved_at: now.to_string(),
            lineage,
        },
    );
}

/// One of your templates as sync sees it (the offered copy of each id).
#[derive(Debug, Clone, PartialEq)]
pub struct Local {
    pub id: String,
    pub version: u32,
}

/// Your templates (not built-ins; customized built-ins are yours), one per id.
pub fn locals(installed: &[Installed]) -> Vec<Local> {
    installed
        .iter()
        .filter(|t| !t.built_in && !t.superseded)
        .map(|t| Local {
            id: t.loaded.template.id.clone(),
            version: t.loaded.template.version,
        })
        .collect()
}

/// Give every template of yours its sync bookkeeping. A template from before sync gets the
/// shared lineage and no save time; one changed outside Nest (a newer version appeared) counts
/// as saved now. Returns whether anything changed.
pub fn ensure_book(book: &mut Book, locals: &[Local], now: &str) -> bool {
    let mut changed = false;
    for l in locals {
        let seen = book.seen.get(&l.id).copied().unwrap_or(0);
        let had_meta = book.meta.contains_key(&l.id);
        if !had_meta {
            book.meta.insert(
                l.id.clone(),
                TemplateSyncMeta {
                    saved_at: String::new(),
                    lineage: lineage_for_existing(&l.id),
                },
            );
            changed = true;
        }
        if l.version > seen {
            book.seen.insert(l.id.clone(), l.version);
            if had_meta {
                if let Some(m) = book.meta.get_mut(&l.id) {
                    m.saved_at = now.to_string();
                }
            }
            changed = true;
        }
    }
    changed
}

/// A template another computer offers.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub device_id: String,
    pub id: String,
    pub entry: IndexEntry,
}

/// Higher version, then later save, then higher device id: the same answer on every computer.
fn rank(a: &Candidate, b: &Candidate) -> Ordering {
    a.entry
        .version
        .cmp(&b.entry.version)
        .then_with(|| instant(&a.entry.saved_at).cmp(&instant(&b.entry.saved_at)))
        .then_with(|| a.device_id.cmp(&b.device_id))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Install as that id (new here, or newer than yours: yours goes to the bin).
    Update,
    /// An unrelated template that shares an id with one of yours: install as a copy, new id.
    Copy,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Install {
    pub from: Candidate,
    pub action: Action,
    /// Two edits of the same version, and this computer's lost: save mine as a copy first.
    pub keep_mine: bool,
}

/// What to install from the other computers' lists (pure). `own` is this computer's device id.
pub fn decide(own: &str, locals: &[Local], book: &Book, candidates: &[Candidate]) -> Vec<Install> {
    // The best offer of each (id, lineage).
    let mut best: BTreeMap<(String, String), Candidate> = BTreeMap::new();
    for c in candidates {
        let key = (c.id.clone(), c.entry.lineage.clone());
        match best.get(&key) {
            Some(b) if rank(c, b) != Ordering::Greater => {}
            _ => {
                best.insert(key, c.clone());
            }
        }
    }
    let mut groups: Vec<Candidate> = best.into_values().collect();
    // Per id, the strongest lineage first (it gets the id when this computer has none).
    groups.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| rank(b, a)));

    let mut lineage_ids: HashMap<String, String> = book
        .meta
        .iter()
        .map(|(id, m)| (m.lineage.clone(), id.clone()))
        .collect();
    let mut claimed: HashSet<String> = HashSet::new();
    let mut out = vec![];
    for w in groups {
        let local = locals.iter().find(|l| l.id == w.id);
        let seen = book.seen.get(&w.id).copied().unwrap_or(0);
        match book.meta.get(&w.id) {
            Some(m) if m.lineage == w.entry.lineage => {
                let mine = local.map(|l| l.version).unwrap_or(0);
                let newer = w.entry.version > seen.max(mine);
                if newer {
                    out.push(Install {
                        from: w,
                        action: Action::Update,
                        keep_mine: false,
                    });
                    continue;
                }
                // Same version saved on two computers: two edits. An equal time is the same
                // save (the copy this computer installed from there). Otherwise `rank()` picks
                // the winner, the same on every computer; if mine loses, it's kept as a copy.
                let clash = local.filter(|l| {
                    l.version == w.entry.version
                        && instant(&w.entry.saved_at) != instant(&m.saved_at)
                });
                if let Some(l) = clash {
                    let mine = Candidate {
                        device_id: own.to_string(),
                        id: l.id.clone(),
                        entry: IndexEntry {
                            version: l.version,
                            saved_at: m.saved_at.clone(),
                            lineage: m.lineage.clone(),
                            size: 0,
                        },
                    };
                    if rank(&w, &mine) == Ordering::Greater {
                        let keep_mine = !book.copied.contains(&copy_key(&l.id, l.version, own));
                        out.push(Install {
                            from: w,
                            action: Action::Update,
                            keep_mine,
                        });
                    }
                }
            }
            meta => {
                if lineage_ids.contains_key(&w.entry.lineage) {
                    // Already here under another id (a copy made earlier, kept or deleted), or
                    // it's one of yours that another computer keeps as a copy.
                    continue;
                }
                lineage_ids.insert(w.entry.lineage.clone(), w.id.clone());
                let free = meta.is_none() && local.is_none() && !claimed.contains(&w.id);
                if free {
                    if w.entry.version > seen {
                        claimed.insert(w.id.clone());
                        out.push(Install {
                            from: w,
                            action: Action::Update,
                            keep_mine: false,
                        });
                    }
                } else {
                    out.push(Install {
                        from: w,
                        action: Action::Copy,
                        keep_mine: false,
                    });
                }
            }
        }
    }
    out
}

/// Which of your templates need a new package: not in this computer's own index yet, or
/// different from what it says.
pub fn needs_write(local: &Local, meta: &TemplateSyncMeta, own: &SyncIndex) -> bool {
    own.get(&local.id).is_none_or(|e| {
        e.version != local.version || e.saved_at != meta.saved_at || e.lineage != meta.lineage
    })
}

// ───────────────────────── Files ─────────────────────────

/// Read one computer's `templates.json`. Capped; entries that don't look right are dropped.
pub fn read_index(path: &Path) -> Result<SyncIndex, String> {
    let meta = fs::metadata(path).map_err(|e| format!("Its template list can't be read ({e})."))?;
    if meta.len() > MAX_INDEX_BYTES {
        return Err("Its template list is too big to be a Nest list.".into());
    }
    let mut text = String::new();
    fs::File::open(path)
        .and_then(|f| f.take(MAX_INDEX_BYTES + 1).read_to_string(&mut text))
        .map_err(|e| format!("Its template list can't be read ({e})."))?;
    let index: SyncIndex = serde_json::from_str(&text)
        .map_err(|_| "Its template list isn't complete yet (still syncing?).".to_string())?;
    if index.len() > MAX_TEMPLATES {
        return Err("Its template list has too many templates.".into());
    }
    Ok(index
        .into_iter()
        .filter(|(id, e)| {
            package_name(id).is_some()
                && e.version >= 1
                && !e.lineage.trim().is_empty()
                && e.lineage.len() <= MAX_STR
                && e.saved_at.len() <= MAX_STR
                && !e
                    .lineage
                    .chars()
                    .chain(e.saved_at.chars())
                    .any(char::is_control)
                && e.size <= LIMITS.max_bytes
        })
        .collect())
}

/// Another computer's templates folder and list.
#[derive(Debug, Clone)]
pub struct Remote {
    pub device_id: String,
    pub dir: PathBuf,
    pub index: SyncIndex,
}

/// The other computers' lists (not this one's, not forgotten ones). Folders must be named
/// after a device id exactly; a missing list is skipped quietly (not synced yet).
pub fn read_remotes(root: &Path, own_id: &str, forgotten: &[String]) -> Vec<Remote> {
    let Ok(entries) = fs::read_dir(root) else {
        return vec![];
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| valid_device_id(n).as_deref() == Some(n.as_str()))
        .filter(|n| n != own_id && !forgotten.contains(n))
        .collect();
    names.sort();
    names.truncate(MAX_FILES);
    let mut out = vec![];
    for name in names {
        let dir = root.join(&name);
        let file = dir.join(INDEX_FILE);
        if !file.is_file() {
            continue;
        }
        match read_index(&file) {
            Ok(index) => out.push(Remote {
                device_id: name,
                dir,
                index,
            }),
            Err(e) => log::info!("template list skipped ({}): {e}", file.display()),
        }
    }
    out
}

/// The package for `c`, if it's all there: the right name, a regular file of the size the list
/// says. `None` = still syncing (try next time).
fn package_ready(dir: &Path, c: &Candidate) -> Option<PathBuf> {
    let path = dir.join(package_name(&c.id)?);
    let meta = fs::symlink_metadata(&path).ok()?;
    (meta.is_file() && meta.len() == c.entry.size && meta.len() <= LIMITS.max_bytes).then_some(path)
}

/// Where this sync tick runs, and for whom.
pub struct Ctx<'a> {
    /// Template sync is on (and Devices is on).
    pub enabled: bool,
    pub shared: &'a Path,
    pub dev_build: bool,
    pub device_id: &'a str,
    /// This computer's own name, for "Wedding (MacBook)".
    pub own_name: &'a str,
    pub user_dir: &'a Path,
    pub bundled_dir: &'a Path,
    pub built_in_ids: &'a [&'a str],
    pub forgotten: &'a [String],
    /// Other computers' names, by device id (from Devices).
    pub names: &'a HashMap<String, String>,
    /// Template ids open in the editor: never installed over.
    pub busy: &'a HashSet<String>,
    pub now: &'a str,
}

/// A template that came in.
#[derive(Debug, Clone, PartialEq)]
pub struct Got {
    pub name: String,
    pub from: String,
    pub action: Action,
    /// Older copies of yours moved to the bin.
    pub binned: usize,
    /// Two edits of the same version: (name of the template, name of the copy of this
    /// computer's edit that was kept next to it).
    pub kept_both: Option<(String, String)>,
}

#[derive(Debug, Default)]
pub struct Report {
    pub got: Vec<Got>,
    /// Packages this computer wrote.
    pub wrote: usize,
    /// Plain words for Settings when something couldn't be done.
    pub problem: Option<String>,
}

/// One sync: bring in newer templates from the other computers (if `read`), then write this
/// computer's own. `bin` moves an older copy to the bin; `save` records the book in Settings
/// (after each install, so nothing is installed twice). The caller holds the templates lock.
pub fn tick(
    ctx: &Ctx,
    book: &mut Book,
    read: bool,
    bin: &dyn Fn(&Installed) -> Result<(), String>,
    save: &mut dyn FnMut(&Book) -> Result<(), String>,
) -> Report {
    let mut report = Report::default();
    if !ctx.enabled || valid_device_id(ctx.device_id).as_deref() != Some(ctx.device_id) {
        return report;
    }
    if !ctx.shared.is_dir() {
        report.problem = Some(
            "Templates can't sync: the synced folder isn't available. Is Google Drive (or your NAS) connected?"
                .into(),
        );
        return report;
    }
    let load = || load_installed(ctx.user_dir, ctx.bundled_dir, ctx.built_in_ids);
    let mut installed = load();
    if ensure_book(book, &locals(&installed), ctx.now) {
        if let Err(e) = save(book) {
            report.problem = Some(format!("Templates can't sync: {e}"));
            return report;
        }
    }
    let root = templates_root(ctx.shared, ctx.dev_build);
    if read {
        if let Err(e) = bring_in(ctx, book, &root, &mut installed, bin, save, &mut report) {
            report.problem = Some(e);
            return report;
        }
    }
    if let Err(e) = write_own(ctx, book, &root, &installed, &mut report) {
        report.problem = Some(e);
    }
    report
}

fn bring_in(
    ctx: &Ctx,
    book: &mut Book,
    root: &Path,
    installed: &mut Vec<Installed>,
    bin: &dyn Fn(&Installed) -> Result<(), String>,
    save: &mut dyn FnMut(&Book) -> Result<(), String>,
    report: &mut Report,
) -> Result<(), String> {
    let load = || load_installed(ctx.user_dir, ctx.bundled_dir, ctx.built_in_ids);
    let remotes = read_remotes(root, ctx.device_id, ctx.forgotten);
    let candidates: Vec<Candidate> = remotes
        .iter()
        .flat_map(|r| {
            r.index.iter().map(|(id, e)| Candidate {
                device_id: r.device_id.clone(),
                id: id.clone(),
                entry: e.clone(),
            })
        })
        .collect();
    for inst in decide(ctx.device_id, &locals(installed), book, &candidates) {
        let c = &inst.from;
        if inst.action == Action::Update && ctx.busy.contains(&c.id) {
            continue; // open in the editor: next time
        }
        let Some(dir) = remotes
            .iter()
            .find(|r| r.device_id == c.device_id)
            .map(|r| &r.dir)
        else {
            continue;
        };
        let Some(package) = package_ready(dir, c) else {
            continue; // still syncing
        };
        let from = ctx
            .names
            .get(&c.device_id)
            .cloned()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| "another computer".into());
        let mut kept_both = None;
        if inst.keep_mine {
            match keep_my_edit(ctx, book, installed, &c.id, save) {
                Ok(k) => kept_both = k,
                Err(e) => {
                    log::info!("couldn't keep my edit of {} as a copy yet: {e}", c.id);
                    continue; // nothing is replaced until my edit is safe: next time
                }
            }
            *installed = load();
        }
        let result = match inst.action {
            Action::Update => template_io::install_synced(
                &package,
                ctx.user_dir,
                installed,
                LIMITS,
                (&c.id, c.entry.version),
                None,
            ),
            Action::Copy => {
                let mut taken: HashSet<String> = installed
                    .iter()
                    .map(|t| t.loaded.template.id.clone())
                    .collect();
                taken.extend(ctx.built_in_ids.iter().map(|s| s.to_string()));
                taken.extend(book.meta.keys().cloned());
                taken.extend(book.seen.keys().cloned());
                let rename = |name: &str| {
                    let name = format!("{name} ({from})");
                    (new_id(&name, &taken, ctx.user_dir), name)
                };
                template_io::install_synced(
                    &package,
                    ctx.user_dir,
                    installed,
                    LIMITS,
                    (&c.id, c.entry.version),
                    Some(&rename as &template_io::Rename<'_>),
                )
            }
        };
        let loaded = match result {
            Ok(l) => l,
            Err(e) => {
                log::info!("template from {from} not installed yet ({}): {e}", c.id);
                continue;
            }
        };
        let id = loaded.template.id.clone();
        let seen = book.seen.entry(id.clone()).or_insert(0);
        *seen = (*seen).max(loaded.template.version);
        book.meta.insert(
            id.clone(),
            TemplateSyncMeta {
                saved_at: c.entry.saved_at.clone(),
                lineage: c.entry.lineage.clone(),
            },
        );
        // Recorded before anything else, so it's never installed twice.
        save(book).map_err(|e| format!("Templates can't sync: {e}"))?;
        *installed = load();
        let mut binned = 0;
        if inst.action == Action::Update {
            match retire_older_copies(&id, &loaded.dir, installed, bin) {
                Ok(n) => binned = n,
                Err(e) => log::warn!("synced template installed, older copy kept: {e}"),
            }
            if binned > 0 {
                *installed = load();
            }
        }
        report.got.push(Got {
            name: loaded.template.name.clone(),
            from,
            action: inst.action,
            binned,
            kept_both,
        });
    }
    Ok(())
}

/// This computer's own edit lost a same-version clash: save it as one copy of its own (new id,
/// new lineage, "<Name> (<this computer>)"). Recorded (and the record saved) before the copy
/// is made and before anything is replaced. `Ok(None)`
/// = there's nothing of mine to keep. The caller then installs the winner as usual.
fn keep_my_edit(
    ctx: &Ctx,
    book: &mut Book,
    installed: &[Installed],
    id: &str,
    save: &mut dyn FnMut(&Book) -> Result<(), String>,
) -> Result<Option<(String, String)>, String> {
    let Some(mine) = installed
        .iter()
        .find(|t| !t.built_in && !t.superseded && t.loaded.template.id == id)
    else {
        return Ok(None);
    };
    let version = mine.loaded.template.version;
    let name = copy_name(&mine.loaded.template.name, ctx.own_name);
    let mut taken: HashSet<String> = installed
        .iter()
        .map(|t| t.loaded.template.id.clone())
        .collect();
    taken.extend(ctx.built_in_ids.iter().map(|s| s.to_string()));
    taken.extend(book.meta.keys().cloned());
    taken.extend(book.seen.keys().cloned());
    let new = new_id(&name, &taken, ctx.user_dir);
    // The record comes first: if saving it fails, nothing is copied yet and the next try starts
    // clean (never a new "Wedding (MacBook)" every minute). A copy that then can't be made takes
    // the record back, so the edit is still kept next time.
    let key = copy_key(id, version, ctx.device_id);
    let had_record = book.copied.contains(&key);
    book.copied.insert(key.clone());
    if let Err(e) = save(book) {
        if !had_record {
            book.copied.remove(&key);
        }
        return Err(e);
    }
    let copy = match template_io::copy_as(&mine.loaded, ctx.user_dir, &new, &name) {
        Ok(c) => c,
        Err(e) => {
            if !had_record {
                book.copied.remove(&key);
                let _ = save(book);
            }
            return Err(e);
        }
    };
    note_local_save(
        book,
        &copy.template.id,
        copy.template.version,
        true,
        ctx.now,
    );
    // Best effort: the record is already safe; this one only remembers the copy's own save.
    if let Err(e) = save(book) {
        log::info!("couldn't save the note for the kept copy yet: {e}");
    }
    Ok(Some((
        mine.loaded.template.name.clone(),
        copy.template.name,
    )))
}

/// "Wedding (MacBook)": the computer's name is cleaned like any name we make (no slashes or
/// control characters), at most 40 characters, and only ever shown, never used in a path.
pub fn copy_name(template_name: &str, computer: &str) -> String {
    let computer: String = sanitize_segment(computer)
        .unwrap_or_else(|_| "this computer".into())
        .chars()
        .take(40)
        .collect();
    format!("{} ({})", template_name.trim(), computer.trim())
}

fn write_own(
    ctx: &Ctx,
    book: &Book,
    root: &Path,
    installed: &[Installed],
    report: &mut Report,
) -> Result<(), String> {
    let dir = root.join(ctx.device_id);
    // Only `Nest/templates/<id>` inside a synced folder that already exists (as for Devices).
    for d in [root.parent().unwrap_or(root), root, &dir] {
        if !d.is_dir() {
            fs::create_dir(d).map_err(|e| {
                format!("Templates can't sync: couldn't create {}: {e}", d.display())
            })?;
        }
    }
    let index_path = dir.join(INDEX_FILE);
    let old: SyncIndex = if index_path.is_file() {
        read_index(&index_path).unwrap_or_default()
    } else {
        SyncIndex::new()
    };
    let mut next = SyncIndex::new();
    let mut problem = None;
    for t in installed.iter().filter(|t| !t.built_in && !t.superseded) {
        let local = Local {
            id: t.loaded.template.id.clone(),
            version: t.loaded.template.version,
        };
        let (Some(meta), Some(name)) = (book.meta.get(&local.id), package_name(&local.id)) else {
            log::info!(
                "template not synced (its id can't be a file name): {}",
                local.id
            );
            continue;
        };
        let path = dir.join(&name);
        let size_now = fs::metadata(&path).map(|m| m.len()).ok();
        let fine =
            !needs_write(&local, meta, &old) && old.get(&local.id).map(|e| e.size) == size_now;
        if fine {
            next.insert(local.id.clone(), old[&local.id].clone());
            continue;
        }
        match template_io::export(&t.loaded, &path).and_then(|_| {
            fs::metadata(&path)
                .map(|m| m.len())
                .map_err(|e| e.to_string())
        }) {
            Ok(size) => {
                report.wrote += 1;
                next.insert(
                    local.id.clone(),
                    IndexEntry {
                        version: local.version,
                        saved_at: meta.saved_at.clone(),
                        lineage: meta.lineage.clone(),
                        size,
                    },
                );
            }
            Err(e) => {
                log::warn!("couldn't write template package {}: {e}", local.id);
                problem = Some(format!(
                    "Templates can't sync: couldn't save “{}” to the synced folder ({e}).",
                    t.loaded.template.name
                ));
                // The previous package (if any) is still whole: keep listing it.
                if let Some(e) = old.get(&local.id) {
                    next.insert(local.id.clone(), e.clone());
                }
            }
        }
    }
    if next != old || !index_path.is_file() {
        let value = serde_json::to_value(&next).map_err(|e| e.to_string())?;
        write_json_atomic(&index_path, &value, false)
            .map_err(|e| format!("Templates can't sync: couldn't save the template list ({e})."))?;
        // Packages of templates this computer no longer has: ours, no longer listed.
        for id in old.keys().filter(|id| !next.contains_key(*id)) {
            if let Some(name) = package_name(id) {
                let p = dir.join(name);
                if fs::symlink_metadata(&p).is_ok_and(|m| m.is_file()) {
                    let _ = fs::remove_file(&p);
                }
            }
        }
    }
    clean_temp_files(&dir);
    if let Some(p) = problem {
        return Err(p);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "aaaaaaaa-0000-4000-8000-000000000000";
    const B: &str = "bbbbbbbb-0000-4000-8000-000000000000";

    fn cand(dev: &str, id: &str, v: u32, at: &str, lineage: &str) -> Candidate {
        Candidate {
            device_id: dev.into(),
            id: id.into(),
            entry: IndexEntry {
                version: v,
                saved_at: at.into(),
                lineage: lineage.into(),
                size: 1,
            },
        }
    }

    fn book(entries: &[(&str, u32, &str, &str)]) -> Book {
        let mut b = Book::default();
        for (id, v, at, lineage) in entries {
            b.seen.insert(id.to_string(), *v);
            b.meta.insert(
                id.to_string(),
                TemplateSyncMeta {
                    saved_at: at.to_string(),
                    lineage: lineage.to_string(),
                },
            );
        }
        b
    }

    fn local(id: &str, v: u32) -> Local {
        Local {
            id: id.into(),
            version: v,
        }
    }

    const T1: &str = "2026-10-15T10:00:00.000+02:00";
    const T2: &str = "2026-10-15T09:30:00.000+00:00"; // later than T1 (09:30Z vs 08:00Z)

    #[test]
    fn newer_version_installs_older_and_same_are_ignored() {
        let b = book(&[("video", 2, T1, "id:video")]);
        let locals = [local("video", 2)];
        let newer = decide(A, &locals, &b, &[cand(B, "video", 3, T1, "id:video")]);
        assert_eq!(newer.len(), 1);
        assert_eq!(newer[0].action, Action::Update);
        assert!(decide(A, &locals, &b, &[cand(B, "video", 1, T2, "id:video")]).is_empty());
        assert!(decide(A, &locals, &b, &[cand(B, "video", 2, T1, "id:video")]).is_empty());
    }

    #[test]
    fn tie_goes_to_the_later_save_on_both_sides() {
        // A saved v3 at T1, B saved v3 at T2 (later). Times compare as instants, not text.
        assert!(later(T2, T1));
        let a_book = book(&[("video", 3, T1, "id:video")]);
        let b_book = book(&[("video", 3, T2, "id:video")]);
        let on_a = decide(
            A,
            &[local("video", 3)],
            &a_book,
            &[cand(B, "video", 3, T2, "id:video")],
        );
        let on_b = decide(
            B,
            &[local("video", 3)],
            &b_book,
            &[cand(A, "video", 3, T1, "id:video")],
        );
        assert_eq!(on_a.len(), 1, "A takes B's later save");
        assert!(on_b.is_empty(), "B keeps its own");
        // A third computer picks the same one; equal times go to the higher device id.
        let none = Book::default();
        let both = [
            cand(A, "video", 3, T1, "id:video"),
            cand(B, "video", 3, T2, "id:video"),
        ];
        assert_eq!(decide(A, &[], &none, &both)[0].from.device_id, B);
        let same_time = [
            cand(B, "video", 3, T1, "id:video"),
            cand(A, "video", 3, T1, "id:video"),
        ];
        assert_eq!(decide(A, &[], &none, &same_time)[0].from.device_id, B);
    }

    #[test]
    fn installed_copy_is_not_sent_back_as_newer() {
        // A installed B's v3 (keeping B's time): B sees A's identical entry and does nothing.
        let b_book = book(&[("video", 3, T2, "id:video")]);
        assert!(decide(
            A,
            &[local("video", 3)],
            &b_book,
            &[cand(A, "video", 3, T2, "id:video")]
        )
        .is_empty());
    }

    #[test]
    fn deleted_stays_deleted_until_newer() {
        let b = book(&[("video", 3, T1, "id:video")]);
        assert!(decide(A, &[], &b, &[cand(B, "video", 3, T2, "id:video")]).is_empty());
        assert_eq!(
            decide(A, &[], &b, &[cand(B, "video", 4, T2, "id:video")]).len(),
            1
        );
    }

    #[test]
    fn unrelated_same_id_comes_in_once_as_a_copy() {
        let b = book(&[("wedding", 1, T1, "mine")]);
        let theirs = [cand(B, "wedding", 1, T2, "theirs")];
        let d = decide(A, &[local("wedding", 1)], &b, &theirs);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].action, Action::Copy);
        // Once the copy is here (under another id, same lineage), it isn't copied again, and
        // the other computer's copy of mine isn't either.
        let mut after = b.clone();
        after.meta.insert(
            "wedding-macbook".into(),
            TemplateSyncMeta {
                saved_at: T2.into(),
                lineage: "theirs".into(),
            },
        );
        let mut both = theirs.to_vec();
        both.push(cand(B, "wedding-pc", 1, T1, "mine"));
        assert!(decide(
            A,
            &[local("wedding", 1), local("wedding-macbook", 1)],
            &after,
            &both
        )
        .is_empty());
    }

    #[test]
    fn new_template_from_elsewhere_installs() {
        let d = decide(A, &[], &Book::default(), &[cand(B, "promo", 1, T1, "x")]);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].action, Action::Update);
    }

    #[test]
    fn book_keeps_lineage_and_notes_saves() {
        let mut b = Book::default();
        note_local_save(&mut b, "video", 2, false, T1);
        assert_eq!(b.meta["video"].lineage, "id:video");
        note_local_save(&mut b, "video", 3, true, T2);
        assert_eq!(b.meta["video"].lineage, "id:video", "kept forever");
        assert_eq!(b.meta["video"].saved_at, T2);
        assert_eq!(b.seen["video"], 3);
        note_local_save(&mut b, "new-one", 1, true, T1);
        assert_ne!(b.meta["new-one"].lineage, "id:new-one");

        let mut b = Book::default();
        assert!(ensure_book(&mut b, &[local("old", 2)], T1));
        assert_eq!(b.meta["old"].saved_at, "", "from before sync: no save time");
        assert!(!ensure_book(&mut b, &[local("old", 2)], T1));
        assert!(ensure_book(&mut b, &[local("old", 3)], T2));
        assert_eq!(
            b.meta["old"].saved_at, T2,
            "changed outside Nest: saved now"
        );
    }

    #[test]
    fn package_names() {
        assert_eq!(package_name("video").as_deref(), Some("video.nesttemplate"));
        assert_eq!(package_name("con"), None);
        assert_eq!(package_name("Video"), None);
        assert_eq!(package_name("../x"), None);
        assert_eq!(package_name(""), None);
    }

    #[test]
    fn needs_write_when_different() {
        let m = TemplateSyncMeta {
            saved_at: T1.into(),
            lineage: "l".into(),
        };
        let mut own = SyncIndex::new();
        assert!(needs_write(&local("a", 1), &m, &own));
        own.insert(
            "a".into(),
            IndexEntry {
                version: 1,
                saved_at: T1.into(),
                lineage: "l".into(),
                size: 5,
            },
        );
        assert!(!needs_write(&local("a", 1), &m, &own));
        assert!(needs_write(&local("a", 2), &m, &own));
    }

    #[test]
    fn same_version_two_edits_only_the_lower_computer_keeps_a_copy() {
        // A saved v3 at T1, B saved v3 at T2 (later): B ranks higher, on both computers.
        let a_book = book(&[("wedding", 3, T1, "id:wedding")]);
        let b_book = book(&[("wedding", 3, T2, "id:wedding")]);
        let on_a = decide(
            A,
            &[local("wedding", 3)],
            &a_book,
            &[cand(B, "wedding", 3, T2, "id:wedding")],
        );
        assert_eq!(on_a.len(), 1);
        assert_eq!(on_a[0].action, Action::Update);
        assert!(on_a[0].keep_mine, "A's edit lost: A keeps one copy");
        let on_b = decide(
            B,
            &[local("wedding", 3)],
            &b_book,
            &[cand(A, "wedding", 3, T1, "id:wedding")],
        );
        assert!(on_b.is_empty(), "the higher computer does nothing special");
    }

    #[test]
    fn equal_save_times_go_by_device_id_and_same_save_is_no_clash() {
        // Same instant written two ways = the same save (what was installed from there).
        let same_instant = "2026-10-15T08:00:00.000+00:00";
        let a_book = book(&[("w", 3, T1, "id:w")]);
        assert!(decide(
            A,
            &[local("w", 3)],
            &a_book,
            &[cand(B, "w", 3, same_instant, "id:w")]
        )
        .is_empty());
        // The record stops a second copy but the winner still installs.
        let mut with_record = book(&[("w", 3, T1, "id:w")]);
        with_record.copied.insert(copy_key("w", 3, A));
        let d = decide(
            A,
            &[local("w", 3)],
            &with_record,
            &[cand(B, "w", 3, T2, "id:w")],
        );
        assert_eq!(d.len(), 1);
        assert!(!d[0].keep_mine);
    }

    #[test]
    fn a_newer_version_never_makes_a_copy() {
        let b = book(&[("w", 2, T1, "id:w")]);
        let d = decide(A, &[local("w", 2)], &b, &[cand(B, "w", 3, T2, "id:w")]);
        assert_eq!(d.len(), 1);
        assert!(!d[0].keep_mine);
    }

    #[test]
    fn copy_names_are_clean_and_short() {
        assert_eq!(copy_name("Wedding", "MacBook"), "Wedding (MacBook)");
        let long = "x".repeat(100);
        assert_eq!(
            copy_name("Wedding", &long),
            format!("Wedding ({})", "x".repeat(40))
        );
        assert_eq!(copy_name("Wedding", "../Bad:/Name"), "Wedding (..BadName)");
        assert_eq!(copy_name("Wedding", "  "), "Wedding (this computer)");
        assert_eq!(copy_name(" Wedding ", "CON"), "Wedding (this computer)");
    }
}

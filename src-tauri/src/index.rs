//! Project index: a SQLite cache of every project's manifest, plus the rescan that fills it
//! (build spec §2.4, §6, §7). The folder is the truth: `index.sqlite` is Nest's own cache
//! file, and deleting it loses nothing: the next rescan rebuilds it.
//!
//! Safety rules (M3 review):
//! - the folder walk never holds the database; results are written in one short transaction;
//! - a project leaves the index only when its manifest is definitely gone (`NotFound`),
//!   its jobs folder is online, and it wasn't seen since the scan started;
//! - an empty jobs folder that may just be unconnected (a `/Volumes` stub, a CloudStorage
//!   folder) counts as offline; anywhere else, empty means its projects were deleted;
//! - only `.project.json` files are ever read (cloud placeholders stay untouched).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value as Json;
use unicode_normalization::UnicodeNormalization;

use crate::manifest::read_json_object;
use crate::plan::{ClientValue, MANIFEST_NAME};

pub const INDEX_FILE: &str = "index.sqlite";
// 2 (v0.4.0): roots.archive. An older index is simply rebuilt (it's a cache).
const SCHEMA_VERSION: i32 = 2;

/// Folders the scan never looks inside (spec §7). Names starting with `.` are skipped too.
const SKIP: [&str; 5] = [
    "node_modules",
    "$RECYCLE.BIN",
    "System Volume Information",
    "CacheClip",
    "__MACOSX",
];

fn skip(name: &str) -> bool {
    name.starts_with('.') || SKIP.contains(&name)
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

// ───────────────────────── Quick scan of one root (used by Create) ─────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RootScan {
    /// Job codes found in manifests (up to 2 levels deep).
    pub codes: Vec<String>,
    /// Names directly inside the root (NFC), folders and files alike.
    pub folders: Vec<String>,
    /// Clients seen in manifests, one per code, sorted by name.
    pub clients: Vec<ClientValue>,
}

/// Scan a jobs root. Unreadable folders and manifests are skipped, never written to.
pub fn scan_root(root: &Path) -> std::io::Result<RootScan> {
    let mut scan = RootScan::default();
    let mut clients = BTreeMap::new();
    for entry in fs::read_dir(root)?.flatten() {
        let name: String = entry.file_name().to_string_lossy().nfc().collect();
        scan.folders.push(name.clone());
        if skip(&name) || !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let dir = entry.path();
        if !read_manifest_into(&dir, &mut scan, &mut clients) {
            // Not a project: look one level further (e.g. Jobs/Client/job).
            let Ok(children) = fs::read_dir(&dir) else {
                continue;
            };
            for child in children.flatten() {
                let child_name = child.file_name().to_string_lossy().into_owned();
                if !skip(&child_name) && child.file_type().is_ok_and(|t| t.is_dir()) {
                    read_manifest_into(&child.path(), &mut scan, &mut clients);
                }
            }
        }
    }
    let mut clients: Vec<ClientValue> = clients.into_values().collect();
    clients.sort_by_key(|c| c.name.to_lowercase());
    scan.clients = clients;
    Ok(scan)
}

/// Returns true if `dir` has a manifest file (readable or not).
fn read_manifest_into(
    dir: &Path,
    scan: &mut RootScan,
    clients: &mut BTreeMap<String, ClientValue>,
) -> bool {
    let path = dir.join(MANIFEST_NAME);
    if !path.is_file() {
        return false;
    }
    if let Ok(m) = read_json_object(&path) {
        if let Some(code) = m["jobCode"].as_str() {
            scan.codes.push(code.to_string());
        }
        let name = m["client"]["name"].as_str().unwrap_or_default();
        let code = m["client"]["code"].as_str().unwrap_or_default();
        if !name.is_empty() && !code.is_empty() {
            clients.entry(code.to_uppercase()).or_insert(ClientValue {
                name: name.to_string(),
                code: code.to_uppercase(),
            });
        }
    }
    true
}

// ───────────────────────── Full rescan: the walk (no database) ─────────────────────────

#[derive(Debug, Clone)]
pub struct FoundProject {
    /// The project folder (holds `.project.json`).
    pub dir: PathBuf,
    /// The jobs folder it was found under.
    pub root: PathBuf,
    /// The manifest, or why it couldn't be read.
    pub manifest: Result<Json, String>,
}

#[derive(Debug, Default)]
pub struct Walk {
    pub projects: Vec<FoundProject>,
    pub online_roots: Vec<PathBuf>,
    pub offline_roots: Vec<PathBuf>,
    /// Online roots that listed no entries at all (maybe an unmounted drive's stub).
    pub empty_roots: Vec<PathBuf>,
    /// Folders that couldn't be listed completely (their projects are never removed).
    pub unreadable: Vec<PathBuf>,
}

/// Walk each jobs folder 2 levels deep for `.project.json`. Never enters a project folder,
/// never follows links, and never reads any other file.
pub fn walk_roots(roots: &[PathBuf]) -> Walk {
    let mut walk = Walk::default();
    for root in roots {
        let Ok(entries) = fs::read_dir(root) else {
            walk.offline_roots.push(root.clone());
            continue;
        };
        walk.online_roots.push(root.clone());
        let mut any = false;
        for entry in entries {
            any = true;
            let Ok(entry) = entry else {
                walk.unreadable.push(root.clone());
                continue;
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            // file_type() doesn't follow links: a link is never treated as a folder.
            if skip(&name) || !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let dir = entry.path();
            if let Some(found) = found_at(&dir, root) {
                walk.projects.push(found);
                continue; // never descend into a project
            }
            let Ok(children) = fs::read_dir(&dir) else {
                walk.unreadable.push(dir);
                continue;
            };
            for child in children {
                let Ok(child) = child else {
                    walk.unreadable.push(dir.clone());
                    continue;
                };
                let child_name = child.file_name().to_string_lossy().into_owned();
                if skip(&child_name) || !child.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                if let Some(found) = found_at(&child.path(), root) {
                    walk.projects.push(found);
                }
            }
        }
        // An empty jobs folder means its projects were deleted, except where an empty folder
        // can also mean "not connected": a drive stub under /Volumes or a cloud folder whose
        // app isn't running (macOS CloudStorage). Only those stay offline (Bernard, 2026-10-03).
        if !any && may_be_unmounted(root) {
            walk.empty_roots.push(root.clone());
        }
    }
    walk
}

fn found_at(dir: &Path, root: &Path) -> Option<FoundProject> {
    let file = dir.join(MANIFEST_NAME);
    if !file.is_file() {
        return None;
    }
    let manifest = read_json_object(&file).and_then(|m| {
        if m["id"].as_str().is_some_and(|id| !id.is_empty()) {
            Ok(m)
        } else {
            Err("it has no project id".to_string())
        }
    });
    Some(FoundProject {
        dir: dir.to_path_buf(),
        root: root.to_path_buf(),
        manifest,
    })
}

// ───────────────────────── The database ─────────────────────────

/// One project as the UI lists it. `key` is the manifest id, or `path:<folder>` when the
/// manifest can't be read (then `error` says why).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRow {
    pub key: String,
    pub id: Option<String>,
    pub job_code: String,
    pub title: String,
    pub client: ClientValue,
    pub space: String,
    pub status: String,
    pub template_id: String,
    pub created_at: String,
    pub fields: Json,
    pub error: Option<String>,
    /// Every folder this project was found at (more than one = copies in two places).
    pub paths: Vec<PathBuf>,
    /// All of its jobs folders are offline.
    pub offline: bool,
    /// Another project has the same job code (spec §12a #12).
    pub duplicate_code: bool,
    /// Devices: set on a project that lives on another computer (shown greyed, read-only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<RemoteDevice>,
    /// Devices: other computers that also list this project.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub also_on: Vec<String>,
    /// The template's name ("Grading: PV"), filled in when the list is shown.
    pub template_name: String,
    /// Devices (v0.3.2): a status this computer asked the owning computer to set, still waiting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending: Option<String>,
    /// Devices (v0.3.2): why the owning computer couldn't apply this computer's request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_problem: Option<String>,
    /// Devices (v0.3.2): this project's status was last changed from another computer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_by: Option<ChangedBy>,
    /// Archive (M5): every folder this project was found at is under the archive folder.
    /// Read-only in Nest; hidden behind the Archived filter.
    pub archived: bool,
    /// Done since (ms since 1970): the manifest's `doneAt`, or the manifest file's modified
    /// time for projects marked done before v0.4.0. None while active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done_at: Option<i64>,
    /// Archive (M5): done for longer than Settings' "suggest after N days", and not dismissed.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub ready_to_archive: bool,
}

/// "Marked done from Studio PC · 3 min ago".
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedBy {
    pub name: String,
    pub status: String,
    /// RFC 3339, this computer's clock.
    pub at: String,
}

/// Where a project from another computer lives (display only).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDevice {
    pub device_id: String,
    pub name: String,
    pub updated_at: String,
    /// The folder on that computer, as text. Never opened.
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RootState {
    pub path: PathBuf,
    pub online: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RescanSummary {
    pub found: usize,
    /// Projects that left the index (folder deleted outside Nest).
    pub removed: usize,
    /// Job codes used by more than one project.
    pub duplicates: Vec<String>,
    pub offline: Vec<PathBuf>,
}

/// Where a project lives, for actions (open, reveal, status).
#[derive(Debug, Clone, PartialEq)]
pub struct Located {
    pub id: Option<String>,
    pub paths: Vec<(PathBuf, bool)>, // (folder, root online)
}

pub struct Index {
    conn: Connection,
}

const SCHEMA: &str = "
CREATE TABLE projects (
    key TEXT PRIMARY KEY,
    id TEXT,
    job_code TEXT NOT NULL DEFAULT '',
    title TEXT NOT NULL DEFAULT '',
    client_name TEXT NOT NULL DEFAULT '',
    client_code TEXT NOT NULL DEFAULT '',
    space TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT '',
    template_id TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT '',
    fields TEXT NOT NULL DEFAULT '{}',
    error TEXT,
    done_at INTEGER
);
CREATE TABLE locations (
    path TEXT PRIMARY KEY,
    key TEXT NOT NULL,
    root TEXT NOT NULL,
    last_seen INTEGER NOT NULL
);
CREATE INDEX locations_key ON locations(key);
CREATE TABLE sizes (
    key TEXT PRIMARY KEY,
    json TEXT NOT NULL,
    measured_at INTEGER NOT NULL
);
CREATE TABLE roots (
    path TEXT PRIMARY KEY,
    online INTEGER NOT NULL,
    scanned_at INTEGER NOT NULL,
    archive INTEGER NOT NULL DEFAULT 0
);
";

impl Index {
    /// Open (or create) the index. A damaged file or an old schema is deleted and rebuilt:
    /// it's a cache, the project folders are the truth.
    pub fn open(path: &Path) -> Result<Index, String> {
        match Self::try_open(path) {
            Ok(index) => Ok(index),
            Err(first) => {
                log::warn!("rebuilding the project index ({first})");
                for suffix in ["", "-journal", "-wal", "-shm"] {
                    let _ = fs::remove_file(format!("{}{suffix}", path.display()));
                }
                Self::try_open(path).map_err(|e| format!("Couldn't open the project index: {e}"))
            }
        }
    }

    pub fn open_in_memory() -> Index {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        Self::init(conn).expect("fresh schema")
    }

    fn try_open(path: &Path) -> Result<Index, String> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        let version: i32 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(|e| e.to_string())?;
        match version {
            0 => Self::init(conn).map_err(|e| e.to_string()),
            SCHEMA_VERSION => {
                conn.query_row("SELECT count(*) FROM projects", [], |r| r.get::<_, i64>(0))
                    .map_err(|e| e.to_string())?;
                Ok(Index { conn })
            }
            other => Err(format!("schema version {other}")),
        }
    }

    fn init(conn: Connection) -> rusqlite::Result<Index> {
        conn.execute_batch(SCHEMA)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(Index { conn })
    }

    /// Add or refresh projects (used by rescan, and by Create for the new project).
    pub fn record(&mut self, found: &[FoundProject], seen_at: i64) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        for f in found {
            upsert(&tx, f, seen_at)?;
        }
        tx.commit()
    }

    /// Folders under these roots not seen since `before`: candidates for removal.
    pub fn stale_paths(&self, roots: &[PathBuf], before: i64) -> rusqlite::Result<Vec<PathBuf>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path FROM locations WHERE root = ?1 AND last_seen < ?2")?;
        let mut out = Vec::new();
        for root in roots {
            let rows =
                stmt.query_map(params![path_str(root), before], |r| r.get::<_, String>(0))?;
            for p in rows {
                out.push(PathBuf::from(p?));
            }
        }
        Ok(out)
    }

    pub fn has_projects_under(&self, root: &Path) -> rusqlite::Result<bool> {
        self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM locations WHERE root = ?1)",
            params![path_str(root)],
            |r| r.get(0),
        )
    }

    /// Write one rescan's results in a single transaction. `configured` is every folder
    /// that was walked (jobs folders and the archive folder); `archive` says which of them
    /// is the archive folder.
    pub fn apply_rescan(
        &mut self,
        walk: &Walk,
        offline: &[PathBuf],
        gone: &[PathBuf],
        configured: &[PathBuf],
        archive: Option<&Path>,
        scan_start: i64,
    ) -> rusqlite::Result<RescanSummary> {
        let tx = self.conn.transaction()?;
        for f in &walk.projects {
            upsert(&tx, f, scan_start)?;
        }
        // Only folders whose manifest is definitely gone, and not seen since the scan began.
        for p in gone {
            tx.execute(
                "DELETE FROM locations WHERE path = ?1 AND last_seen < ?2",
                params![path_str(p), scan_start],
            )?;
        }
        // Jobs folders no longer in Settings: their projects leave the list.
        let configured_str: HashSet<String> = configured.iter().map(|p| path_str(p)).collect();
        let roots_in_db: Vec<String> = {
            let mut stmt = tx.prepare("SELECT DISTINCT root FROM locations")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        for r in roots_in_db.iter().filter(|r| !configured_str.contains(*r)) {
            tx.execute("DELETE FROM locations WHERE root = ?1", params![r])?;
        }
        let removed = tx.execute(
            "DELETE FROM projects WHERE key NOT IN (SELECT key FROM locations)",
            [],
        )?;
        tx.execute(
            "DELETE FROM sizes WHERE key NOT IN (SELECT key FROM projects)",
            [],
        )?;
        tx.execute("DELETE FROM roots", [])?;
        for root in configured {
            let online = !offline.contains(root);
            let is_archive = archive.is_some_and(|a| a == root);
            tx.execute(
                "INSERT INTO roots (path, online, scanned_at, archive) VALUES (?1, ?2, ?3, ?4)",
                params![path_str(root), online, scan_start, is_archive],
            )?;
        }
        tx.commit()?;
        let duplicates = self.duplicate_codes()?;
        Ok(RescanSummary {
            found: walk.projects.len(),
            removed,
            duplicates,
            offline: offline.to_vec(),
        })
    }

    /// Job codes used by more than one project, compared case-insensitively and reported in
    /// upper case (the canonical form), so the answer doesn't depend on which spelling the
    /// scan happened to see first.
    fn duplicate_codes(&self) -> rusqlite::Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT upper(job_code) FROM projects WHERE job_code <> '' \
             GROUP BY lower(job_code) HAVING count(*) > 1 ORDER BY 1",
        )?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect()
    }

    /// Every project, newest first, with its folders and online state.
    pub fn list(&self) -> rusqlite::Result<Vec<ProjectRow>> {
        // Per root: (online, archive).
        let roots: HashMap<String, (bool, bool)> = {
            let mut stmt = self
                .conn
                .prepare("SELECT path, online, archive FROM roots")?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    (r.get::<_, bool>(1)?, r.get::<_, bool>(2)?),
                ))
            })?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let dupes: HashSet<String> = self
            .duplicate_codes()?
            .into_iter()
            .map(|c| c.to_lowercase())
            .collect();
        let mut stmt = self.conn.prepare(
            "SELECT p.key, p.id, p.job_code, p.title, p.client_name, p.client_code, p.space,
                    p.status, p.template_id, p.created_at, p.fields, p.error, l.path, l.root,
                    p.done_at
             FROM projects p JOIN locations l ON l.key = p.key
             ORDER BY p.created_at DESC, p.key, l.path",
        )?;
        let mut rows: Vec<ProjectRow> = Vec::new();
        let mut query = stmt.query([])?;
        while let Some(r) = query.next()? {
            let key: String = r.get(0)?;
            let path = PathBuf::from(r.get::<_, String>(12)?);
            let root: String = r.get(13)?;
            // Unknown root (not scanned yet) counts as online, and not archive, until a scan
            // says otherwise.
            let (root_online, root_archive) = roots.get(&root).copied().unwrap_or((true, false));
            if let Some(last) = rows.last_mut().filter(|p| p.key == key) {
                last.paths.push(path);
                last.offline = last.offline && !root_online;
                // A copy in a jobs folder wins: only a project found nowhere else is archived.
                last.archived = last.archived && root_archive;
                continue;
            }
            let job_code: String = r.get(2)?;
            rows.push(ProjectRow {
                key,
                id: r.get(1)?,
                duplicate_code: !job_code.is_empty() && dupes.contains(&job_code.to_lowercase()),
                job_code,
                title: r.get(3)?,
                client: ClientValue {
                    name: r.get(4)?,
                    code: r.get(5)?,
                },
                space: r.get(6)?,
                status: r.get(7)?,
                template_id: r.get(8)?,
                created_at: r.get(9)?,
                fields: serde_json::from_str(&r.get::<_, String>(10)?).unwrap_or(Json::Null),
                error: r.get(11)?,
                paths: vec![path],
                offline: !root_online,
                device: None,
                also_on: vec![],
                template_name: String::new(),
                pending: None,
                request_problem: None,
                changed_by: None,
                archived: root_archive,
                done_at: r.get(14)?,
                ready_to_archive: false,
            });
        }
        Ok(rows)
    }

    pub fn roots(&self) -> rusqlite::Result<Vec<RootState>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, online FROM roots ORDER BY path")?;
        let rows = stmt.query_map([], |r| {
            Ok(RootState {
                path: PathBuf::from(r.get::<_, String>(0)?),
                online: r.get(1)?,
            })
        })?;
        rows.collect()
    }

    pub fn last_scan(&self) -> rusqlite::Result<Option<i64>> {
        self.conn
            .query_row("SELECT max(scanned_at) FROM roots", [], |r| r.get(0))
    }

    /// Every job code in the index, including offline folders (spec §5: index and disk).
    pub fn codes(&self) -> rusqlite::Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT job_code FROM projects WHERE job_code <> ''")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect()
    }

    pub fn locate(&self, key: &str) -> rusqlite::Result<Option<Located>> {
        let id: Option<Option<String>> = self
            .conn
            .query_row(
                "SELECT id FROM projects WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                e => Err(e),
            })?;
        let Some(id) = id else { return Ok(None) };
        let mut stmt = self.conn.prepare(
            "SELECT l.path, COALESCE(r.online, 1) FROM locations l
             LEFT JOIN roots r ON r.path = l.root WHERE l.key = ?1 ORDER BY l.path",
        )?;
        let paths = stmt
            .query_map(params![key], |r| {
                Ok((PathBuf::from(r.get::<_, String>(0)?), r.get::<_, bool>(1)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(Some(Located { id, paths }))
    }

    /// Drop projects from jobs folders no longer in Settings (nothing on disk changes).
    pub fn forget_roots_except(&mut self, roots: &[PathBuf]) -> rusqlite::Result<()> {
        let keep: HashSet<String> = roots.iter().map(|p| path_str(p)).collect();
        let tx = self.conn.transaction()?;
        let in_db: Vec<String> = {
            let mut stmt =
                tx.prepare("SELECT DISTINCT root FROM locations UNION SELECT path FROM roots")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        for r in in_db.iter().filter(|r| !keep.contains(*r)) {
            tx.execute("DELETE FROM locations WHERE root = ?1", params![r])?;
            tx.execute("DELETE FROM roots WHERE path = ?1", params![r])?;
        }
        tx.execute(
            "DELETE FROM projects WHERE key NOT IN (SELECT key FROM locations)",
            [],
        )?;
        tx.commit()
    }

    /// Spaces used by any indexed project (a space in use can't be renamed or deleted).
    pub fn spaces_in_use(&self) -> rusqlite::Result<HashSet<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT space FROM projects WHERE space <> ''")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect()
    }

    /// Folder sizes (M5 P4): the last measurement for a project, if any.
    pub fn sizes(&self, key: &str) -> rusqlite::Result<Option<crate::sizes::Sizes>> {
        let json: Option<String> = self
            .conn
            .query_row("SELECT json FROM sizes WHERE key = ?1", params![key], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(json.and_then(|j| serde_json::from_str(&j).ok()))
    }

    pub fn put_sizes(&mut self, key: &str, sizes: &crate::sizes::Sizes) -> rusqlite::Result<()> {
        let json = serde_json::to_string(sizes).unwrap_or_else(|_| "{}".into());
        self.conn.execute(
            "INSERT INTO sizes (key, json, measured_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET json = excluded.json, measured_at = excluded.measured_at",
            params![key, json, sizes.measured_at],
        )?;
        Ok(())
    }

    /// Mirrors `manifest::update_status`: done now also records when (kept if already done).
    pub fn set_status(&mut self, key: &str, status: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE projects
             SET status = ?2,
                 done_at = CASE WHEN ?2 = 'done' THEN coalesce(done_at, ?3) ELSE NULL END
             WHERE key = ?1",
            params![key, status, now_ms()],
        )?;
        Ok(())
    }
}

/// When a done project was marked done: the manifest's `doneAt` if it has one, else the
/// manifest file's modified time (it's rewritten exactly on status changes). None if active.
fn done_at_of(f: &FoundProject) -> Option<i64> {
    let m = f.manifest.as_ref().ok()?;
    if m["status"].as_str() != Some("done") {
        return None;
    }
    m["doneAt"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.timestamp_millis())
        .or_else(|| {
            let modified = fs::metadata(f.dir.join(MANIFEST_NAME))
                .ok()?
                .modified()
                .ok()?;
            let since = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
            Some(since.as_millis() as i64)
        })
}

fn upsert(tx: &rusqlite::Transaction, f: &FoundProject, seen_at: i64) -> rusqlite::Result<()> {
    let path = path_str(&f.dir);
    let (key, row) = match &f.manifest {
        Ok(m) => {
            let s = |v: &Json| v.as_str().unwrap_or_default().to_string();
            let id = s(&m["id"]);
            (
                id.clone(),
                (
                    Some(id),
                    s(&m["jobCode"]),
                    s(&m["title"]),
                    s(&m["client"]["name"]),
                    s(&m["client"]["code"]),
                    s(&m["space"]),
                    s(&m["status"]),
                    s(&m["template"]["id"]),
                    s(&m["createdAt"]),
                    m.get("fields")
                        .cloned()
                        .unwrap_or(Json::Object(Default::default()))
                        .to_string(),
                    None::<String>,
                ),
            )
        }
        Err(e) => (
            format!("path:{path}"),
            (
                None,
                String::new(),
                f.dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                "{}".to_string(),
                Some(e.clone()),
            ),
        ),
    };
    let done_at = done_at_of(f);
    tx.execute(
        "INSERT INTO projects (key, id, job_code, title, client_name, client_code, space, status,
                               template_id, created_at, fields, error, done_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT(key) DO UPDATE SET id = excluded.id, job_code = excluded.job_code,
             title = excluded.title, client_name = excluded.client_name,
             client_code = excluded.client_code, space = excluded.space, status = excluded.status,
             template_id = excluded.template_id, created_at = excluded.created_at,
             fields = excluded.fields, error = excluded.error, done_at = excluded.done_at",
        params![
            key, row.0, row.1, row.2, row.3, row.4, row.5, row.6, row.7, row.8, row.9, row.10,
            done_at
        ],
    )?;
    tx.execute(
        "INSERT INTO locations (path, key, root, last_seen) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(path) DO UPDATE SET key = excluded.key, root = excluded.root,
             last_seen = excluded.last_seen",
        params![path, key, path_str(&f.root), seen_at],
    )?;
    Ok(())
}

/// Full rescan of the configured jobs folders. The walk and the disk checks run without
/// holding the index; only the final write takes the lock, briefly.
pub fn rescan(index: &Mutex<Index>, roots: &[PathBuf]) -> Result<RescanSummary, String> {
    rescan_with_archive(index, roots, None)
}

/// `rescan` of the jobs folders plus the archive folder (M5), whose projects list as archived.
pub fn rescan_with_archive(
    index: &Mutex<Index>,
    jobs_roots: &[PathBuf],
    archive: Option<&Path>,
) -> Result<RescanSummary, String> {
    let mut roots = jobs_roots.to_vec();
    roots.extend(archive.map(Path::to_path_buf));
    let roots = &roots;
    let lock = || {
        index
            .lock()
            .map_err(|_| "The project index is unavailable".to_string())
    };
    let scan_start = now_ms();
    let walk = walk_roots(roots);

    // Empty-looking roots that used to hold projects count as offline (unmounted drive stub).
    let mut offline = walk.offline_roots.clone();
    let stale = {
        let idx = lock()?;
        for root in &walk.empty_roots {
            if idx.has_projects_under(root).map_err(|e| e.to_string())? {
                offline.push(root.clone());
            }
        }
        let online: Vec<PathBuf> = walk
            .online_roots
            .iter()
            .filter(|r| !offline.contains(r))
            .cloned()
            .collect();
        idx.stale_paths(&online, scan_start)
            .map_err(|e| e.to_string())?
    };

    // A folder leaves the index only if its manifest is definitely not there any more.
    let gone: Vec<PathBuf> = stale
        .into_iter()
        .filter(|dir| {
            matches!(fs::symlink_metadata(dir.join(MANIFEST_NAME)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound)
        })
        .collect();

    let mut idx = lock()?;
    idx.apply_rescan(&walk, &offline, &gone, roots, archive, scan_start)
        .map_err(|e| e.to_string())
}

/// Could this folder look empty only because its drive or cloud app isn't connected?
/// (macOS leaves `/Volumes/<name>` stubs behind, and CloudStorage folders read empty while
/// their app is off. On Windows a missing drive simply can't be read, which counts as offline.)
pub fn may_be_unmounted(root: &Path) -> bool {
    let s = root.to_string_lossy().replace('\\', "/");
    s.starts_with("/Volumes/") || s.contains("/Library/CloudStorage/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(root: &Path, rel: &str, manifest: &str) {
        let dir = root.join(rel);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(MANIFEST_NAME), manifest).unwrap();
    }

    #[test]
    fn finds_codes_folders_and_clients_two_levels_deep() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        project(
            root,
            "2609_VX-L01_A",
            r#"{"jobCode":"VX-L01","client":{"name":"Vertex Studio","code":"VX"}}"#,
        );
        project(
            root,
            "Archive/2508_BF-L02_B",
            r#"{"jobCode":"BF-L02","client":{"name":"Brightside","code":"BF"}}"#,
        );
        project(root, "Broken", "{ not json");
        project(root, ".hidden/x", r#"{"jobCode":"HH-L01"}"#);
        fs::write(root.join("loose.txt"), "x").unwrap();

        let scan = scan_root(root).unwrap();
        let mut codes = scan.codes.clone();
        codes.sort();
        assert_eq!(codes, ["BF-L02", "VX-L01"]);
        assert_eq!(
            scan.clients
                .iter()
                .map(|c| c.code.as_str())
                .collect::<Vec<_>>(),
            ["BF", "VX"]
        );
        for name in ["2609_VX-L01_A", "Archive", "Broken", "loose.txt"] {
            assert!(scan.folders.contains(&name.to_string()), "{name}");
        }
        assert_eq!(
            fs::read_to_string(root.join("Broken").join(MANIFEST_NAME)).unwrap(),
            "{ not json"
        );
    }

    #[test]
    fn missing_root_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(scan_root(&tmp.path().join("nope")).is_err());
    }

    #[test]
    fn stale_schema_is_rebuilt() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(INDEX_FILE);
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE junk (x); PRAGMA user_version = 99;")
                .unwrap();
        }
        let index = Index::open(&path).unwrap();
        assert!(index.list().unwrap().is_empty());
        drop(index);
        fs::write(&path, "not a database at all").unwrap();
        assert!(Index::open(&path).unwrap().list().unwrap().is_empty());
    }
}

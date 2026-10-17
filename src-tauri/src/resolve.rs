//! DaVinci Resolve starter projects: talking to Resolve, and the waiting list.
//!
//! Nest never links to Resolve. It runs Resolve's own script runner (`fuscript`) with a small
//! Lua helper bundled in the binary (`resolve.lua`), one run at a time, 30 seconds at most.
//! Every name goes in as an argument (hex-encoded), never into Lua source, and the helper
//! answers with `NEST …` lines; whatever else fuscript prints is ignored.
//!
//! Resolve answers only while it runs and only with External scripting set to Local; both
//! look the same from the script (no answer), so whether the Resolve process runs tells them
//! apart. A project waits in `<config>/resolve-waiting.json` until Resolve takes it.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::manifest::{read_json_object, write_json_atomic};
use crate::settings::ResolveLibrary;

/// The Lua helper, bundled.
pub const SCRIPT: &str = include_str!("resolve.lua");
/// Longest one run may take before Nest stops it.
pub const TIMEOUT: Duration = Duration::from_secs(30);
/// What Resolve calls "no project open".
pub const UNTITLED: &str = "Untitled Project";
pub const WAITING_FILE: &str = "resolve-waiting.json";
/// Nest's own copies of waiting `.drp` files (`<config>/resolve/<id>.drp`).
pub const DRP_DIR: &str = "resolve";
/// A project that keeps failing stops being tried after this many days.
pub const GIVE_UP_DAYS: i64 = 30;

/// One run of fuscript at a time, from anywhere in Nest.
static RUN_LOCK: Mutex<()> = Mutex::new(());

// ───────────────────────── Status and outcomes ─────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResolveStatus {
    /// Resolve runs and answers.
    Ready,
    /// Resolve runs but doesn't answer: External scripting is off (or it's still starting).
    ScriptingOff,
    NotRunning,
    NotInstalled,
}

impl ResolveStatus {
    /// Plain words for the UI.
    pub fn message(self) -> &'static str {
        match self {
            ResolveStatus::Ready => "Resolve is ready.",
            ResolveStatus::ScriptingOff => {
                "Resolve isn't letting Nest in. In Resolve, open Preferences → System → General and set \"External scripting using\" to Local."
            }
            ResolveStatus::NotRunning => "Open DaVinci Resolve first.",
            ResolveStatus::NotInstalled => "DaVinci Resolve isn't installed on this computer.",
        }
    }
}

/// How one "add to Resolve" went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Added,
    /// A project with that name is already in that folder: left as it is.
    Exists,
    /// Resolve has a real project open in another library: switching would close it.
    Busy,
    NoFolder,
    /// The library chosen in Settings isn't in Resolve.
    NoLibrary,
    /// Nest's copy of the `.drp` is gone.
    Missing,
    /// Not reachable right now (not running, scripting off, not installed).
    Unreachable(ResolveStatus),
    Failed(String),
}

impl Outcome {
    /// Short word kept in the waiting list.
    pub fn key(&self) -> &'static str {
        match self {
            Outcome::Added => "added",
            Outcome::Exists => "exists",
            Outcome::Busy => "busy",
            Outcome::NoFolder => "noFolder",
            Outcome::NoLibrary => "noLibrary",
            Outcome::Missing => "missing",
            Outcome::Unreachable(ResolveStatus::ScriptingOff) => "scriptingOff",
            Outcome::Unreachable(ResolveStatus::NotInstalled) => "notInstalled",
            Outcome::Unreachable(_) => "notRunning",
            Outcome::Failed(_) => "failed",
        }
    }
}

/// Why a question to Resolve got no answer, in plain words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    Status(ResolveStatus),
    Failed(String),
}

impl Problem {
    pub fn message(&self) -> String {
        match self {
            Problem::Status(s) => s.message().to_string(),
            Problem::Failed(why) => format!("Resolve didn't do it: {why}"),
        }
    }
}

// ───────────────────────── Pure helpers ─────────────────────────

/// Text as the helper takes and gives it: `x` + hex bytes (`x` alone = empty).
pub fn hex(s: &str) -> String {
    let mut out = String::with_capacity(1 + s.len() * 2);
    out.push('x');
    for b in s.bytes() {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

pub fn unhex(s: &str) -> Option<String> {
    let h = s.strip_prefix('x')?;
    if h.len() % 2 != 0 {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..h.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(h.get(i..i + 2)?, 16).ok())
        .collect();
    String::from_utf8(bytes?).ok()
}

/// What the helper is asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op<'a> {
    Status,
    /// The open library and the open project's name.
    State,
    Libraries,
    Projects,
    Export {
        name: &'a str,
        folder: &'a str,
        dest: &'a Path,
    },
    Add {
        drp: &'a Path,
        name: &'a str,
        folder: &'a str,
        library: Option<&'a ResolveLibrary>,
        /// Switch to `library` first (and back after).
        switch: bool,
    },
}

/// The helper's arguments: the operation, then each piece of text hex-encoded.
pub fn args(op: &Op) -> Vec<String> {
    let path = |p: &Path| hex(&p.to_string_lossy());
    match op {
        Op::Status => vec!["status".into()],
        Op::State => vec!["state".into()],
        Op::Libraries => vec!["libraries".into()],
        Op::Projects => vec!["projects".into()],
        Op::Export { name, folder, dest } => {
            vec!["export".into(), hex(name), hex(folder), path(dest)]
        }
        Op::Add {
            drp,
            name,
            folder,
            library,
            switch,
        } => vec![
            "add".into(),
            path(drp),
            hex(name),
            hex(folder),
            hex(library.map(|l| l.db_type.as_str()).unwrap_or_default()),
            hex(library.map(|l| l.db_name.as_str()).unwrap_or_default()),
            hex(if *switch { "1" } else { "0" }),
        ],
    }
}

/// One answer line from the helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// `result <word> [why]`
    Result(String, Option<String>),
    Current(ResolveLibrary),
    Library(ResolveLibrary),
    /// The open project's name ("" = none).
    Open(String),
    Project {
        folder: String,
        name: String,
    },
}

/// The helper's answers. Lines that aren't `NEST …` (fuscript's own banner and messages) and
/// answers that don't read cleanly are skipped.
pub fn parse(stdout: &str) -> Vec<Line> {
    stdout
        .lines()
        .filter_map(|line| {
            let rest = line.trim_end().strip_prefix("NEST ")?;
            let parts: Vec<&str> = rest.split(' ').collect();
            let lib = |a: &str, b: &str| {
                Some(ResolveLibrary {
                    db_type: unhex(a)?,
                    db_name: unhex(b)?,
                })
            };
            match parts.as_slice() {
                ["result", word] => Some(Line::Result(word.to_string(), None)),
                ["result", word, why] => Some(Line::Result(word.to_string(), unhex(why))),
                ["current", a, b] => lib(a, b).map(Line::Current),
                ["library", a, b] => lib(a, b).map(Line::Library),
                ["open", name] => unhex(name).map(Line::Open),
                ["project", folder, name] => Some(Line::Project {
                    folder: unhex(folder)?,
                    name: unhex(name)?,
                }),
                _ => None,
            }
        })
        .collect()
}

/// The helper's final word, if it got that far.
pub fn result_of(lines: &[Line]) -> Option<(&str, Option<&str>)> {
    lines.iter().rev().find_map(|l| match l {
        Line::Result(word, why) => Some((word.as_str(), why.as_deref())),
        _ => None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    Now,
    /// Another library is chosen and a real project is open: switching would close it.
    WaitForNoProject,
}

/// Can Nest add the project now? Into the open library (or none chosen): yes. Into another
/// library: only when nothing real is open, since switching closes the open project.
pub fn decide(
    target: Option<&ResolveLibrary>,
    current: &ResolveLibrary,
    open_project: Option<&str>,
) -> When {
    match target {
        Some(t) if t != current => match open_project {
            None | Some("") | Some(UNTITLED) => When::Now,
            Some(_) => When::WaitForNoProject,
        },
        _ => When::Now,
    }
}

/// No answer from the helper: scripting off if Resolve runs, else not running.
pub fn not_answering(running: bool) -> ResolveStatus {
    if running {
        ResolveStatus::ScriptingOff
    } else {
        ResolveStatus::NotRunning
    }
}

// ───────────────────────── Talking to Resolve ─────────────────────────

/// Runs the helper with these arguments and gives back what it printed.
pub type Runner<'a> = dyn Fn(&[String]) -> Result<String, String> + 'a;

/// The real thing: fuscript (if installed) and the process check.
pub struct Link<'a> {
    pub run: &'a Runner<'a>,
    pub running: &'a dyn Fn() -> bool,
}

impl Link<'_> {
    /// Run `op`; a "no answer" becomes the right status.
    fn ask(&self, op: &Op) -> Result<Vec<Line>, Problem> {
        let out = (self.run)(&args(op)).map_err(Problem::Failed)?;
        let lines = parse(&out);
        match result_of(&lines) {
            Some(("no-resolve", _)) => Err(Problem::Status(not_answering((self.running)()))),
            Some(("failed", why)) => Err(Problem::Failed(
                why.unwrap_or("it gave no reason").to_string(),
            )),
            Some(_) => Ok(lines),
            None => Err(Problem::Failed("it didn't answer".into())),
        }
    }

    pub fn status(&self) -> ResolveStatus {
        match self.ask(&Op::Status) {
            Ok(_) => ResolveStatus::Ready,
            Err(Problem::Status(s)) => s,
            Err(Problem::Failed(why)) => {
                log::info!("Resolve status: {why}");
                not_answering((self.running)())
            }
        }
    }

    pub fn libraries(&self) -> Result<Libraries, Problem> {
        let lines = self.ask(&Op::Libraries)?;
        let mut out = Libraries::default();
        for l in lines {
            match l {
                Line::Current(lib) => out.current = Some(lib),
                Line::Library(lib) if !out.libraries.contains(&lib) => out.libraries.push(lib),
                _ => {}
            }
        }
        Ok(out)
    }

    pub fn projects(&self) -> Result<Vec<ResolveProject>, Problem> {
        Ok(self
            .ask(&Op::Projects)?
            .into_iter()
            .filter_map(|l| match l {
                Line::Project { folder, name } => Some(ResolveProject { folder, name }),
                _ => None,
            })
            .collect())
    }

    /// Export `name` (in `folder`, "" = the top level) to the `.drp` file `dest`.
    pub fn export(&self, name: &str, folder: &str, dest: &Path) -> Result<(), Problem> {
        let lines = self.ask(&Op::Export { name, folder, dest })?;
        match result_of(&lines) {
            Some(("ok", _)) if dest.is_file() => Ok(()),
            Some(("no-folder", _)) => Err(Problem::Failed(format!(
                "the folder \"{folder}\" isn't in Resolve's open library"
            ))),
            _ => Err(Problem::Failed(format!("\"{name}\" couldn't be exported"))),
        }
    }

    /// Add the `.drp` as project `name` in `folder` of `library` (None = the open one).
    pub fn add(
        &self,
        drp: &Path,
        name: &str,
        folder: &str,
        library: Option<&ResolveLibrary>,
    ) -> Outcome {
        if !drp.is_file() {
            return Outcome::Missing;
        }
        let state = match self.ask(&Op::State) {
            Ok(lines) => lines,
            Err(Problem::Status(s)) => return Outcome::Unreachable(s),
            Err(Problem::Failed(why)) => return Outcome::Failed(why),
        };
        let current = state.iter().find_map(|l| match l {
            Line::Current(lib) => Some(lib.clone()),
            _ => None,
        });
        let open = state.iter().find_map(|l| match l {
            Line::Open(name) => Some(name.as_str()),
            _ => None,
        });
        let Some(current) = current else {
            return Outcome::Failed("Resolve didn't say which library is open".into());
        };
        if decide(library, &current, open) == When::WaitForNoProject {
            return Outcome::Busy;
        }
        let switch = library.is_some_and(|l| *l != current);
        let op = Op::Add {
            drp,
            name,
            folder,
            library,
            switch,
        };
        match self.ask(&op) {
            Err(Problem::Status(s)) => Outcome::Unreachable(s),
            Err(Problem::Failed(why)) => Outcome::Failed(why),
            Ok(lines) => match result_of(&lines) {
                Some(("added", _)) => Outcome::Added,
                Some(("exists", _)) => Outcome::Exists,
                Some(("busy", _)) => Outcome::Busy,
                Some(("no-folder", _)) => Outcome::NoFolder,
                Some(("no-library", _)) => Outcome::NoLibrary,
                other => Outcome::Failed(format!("unexpected answer {other:?}")),
            },
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Libraries {
    /// The library open in Resolve.
    pub current: Option<ResolveLibrary>,
    pub libraries: Vec<ResolveLibrary>,
}

/// A project in Resolve's open library ("Choose from Resolve…").
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveProject {
    /// Its folder, `/` between folders inside folders. "" = the top level.
    pub folder: String,
    pub name: String,
}

/// Where fuscript may be: the usual install places (Resolve and Resolve Studio).
pub fn fuscript_candidates() -> Vec<PathBuf> {
    let mut out = vec![];
    if cfg!(target_os = "macos") {
        let mut bases = vec![PathBuf::from("/Applications")];
        if let Some(home) = std::env::var_os("HOME") {
            bases.push(PathBuf::from(home).join("Applications"));
        }
        for base in bases {
            for app in [
                "DaVinci Resolve/DaVinci Resolve.app",
                "DaVinci Resolve Studio/DaVinci Resolve.app",
                "DaVinci Resolve.app",
            ] {
                out.push(base.join(app).join("Contents/Libraries/Fusion/fuscript"));
            }
        }
    } else if cfg!(windows) {
        let mut bases: Vec<PathBuf> = ["ProgramW6432", "ProgramFiles"]
            .iter()
            .filter_map(std::env::var_os)
            .map(PathBuf::from)
            .collect();
        bases.push(PathBuf::from(r"C:\Program Files"));
        for base in bases {
            for app in ["DaVinci Resolve", "DaVinci Resolve Studio"] {
                let p = base
                    .join("Blackmagic Design")
                    .join(app)
                    .join("fuscript.exe");
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
    } else {
        out.push(PathBuf::from("/opt/resolve/libs/Fusion/fuscript"));
    }
    out
}

pub fn find_fuscript() -> Option<PathBuf> {
    fuscript_candidates().into_iter().find(|p| p.is_file())
}

fn quiet(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.stdin(Stdio::null())
}

/// Is Resolve's own process running (not just fuscript or another tool)?
pub fn is_running() -> bool {
    #[cfg(windows)]
    {
        let out = quiet(&mut Command::new("tasklist"))
            .args(["/FI", "IMAGENAME eq Resolve.exe", "/NH"])
            .stderr(Stdio::null())
            .output();
        match out {
            Ok(o) => String::from_utf8_lossy(&o.stdout)
                .to_ascii_lowercase()
                .contains("resolve.exe"),
            Err(e) => {
                log::info!("couldn't check whether Resolve runs: {e}");
                false
            }
        }
    }
    #[cfg(not(windows))]
    {
        let pattern = if cfg!(target_os = "macos") {
            "DaVinci Resolve.app/Contents/MacOS/Resolve"
        } else {
            "/opt/resolve/bin/resolve"
        };
        match quiet(&mut Command::new("pgrep"))
            .args(["-f", pattern])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
        {
            Ok(s) => s.success(),
            Err(e) => {
                log::info!("couldn't check whether Resolve runs: {e}");
                false
            }
        }
    }
}

/// Run the helper with fuscript: the script goes to a temporary file of Nest's own (removed
/// after), the run is stopped after `timeout`, and only one runs at a time.
pub fn run(fuscript: &Path, args: &[String], timeout: Duration) -> Result<String, String> {
    let _one = RUN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let script = std::env::temp_dir().join(format!(
        "nest-resolve-{}.lua",
        uuid::Uuid::new_v4().simple()
    ));
    fs::write(&script, SCRIPT).map_err(|e| format!("couldn't write the helper: {e}"))?;
    let result = run_script(fuscript, &script, args, timeout);
    if let Err(e) = fs::remove_file(&script) {
        log::info!("couldn't remove {}: {e}", script.display());
    }
    result
}

fn run_script(
    fuscript: &Path,
    script: &Path,
    args: &[String],
    timeout: Duration,
) -> Result<String, String> {
    let mut child = quiet(&mut Command::new(fuscript))
        .arg("-l")
        .arg("lua")
        .arg(script)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("couldn't start Resolve's script runner: {e}"))?;
    let read = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(p) = pipe {
                let mut buf = vec![];
                // Answers are small; never hold more than a few MB.
                let _ = p.take(4 * 1024 * 1024).read_to_end(&mut buf);
                text = String::from_utf8_lossy(&buf).into_owned();
            }
            text
        })
    };
    let out = read(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let err = read(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let started = Instant::now();
    let finished = loop {
        match child.try_wait() {
            Ok(Some(_)) => break true,
            Ok(None) if started.elapsed() >= timeout => break false,
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                log::info!("Resolve's script runner: {e}");
                break false;
            }
        }
    };
    if !finished {
        let _ = child.kill();
        let _ = child.wait();
    }
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    if !stderr.trim().is_empty() {
        log::info!("Resolve's script runner said: {}", stderr.trim());
    }
    if finished {
        Ok(stdout)
    } else {
        Err(format!(
            "Resolve didn't answer within {} seconds",
            timeout.as_secs()
        ))
    }
}

// ───────────────────────── The waiting list ─────────────────────────

/// A project waiting to go into Resolve (one per Create with a Resolve project).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitingEntry {
    /// The project's manifest id.
    pub id: String,
    pub job_code: String,
    pub project_dir: PathBuf,
    /// Nest's own copy of the template's `.drp` (`<config>/resolve/<id>.drp`).
    pub drp: PathBuf,
    pub name: String,
    pub folder: String,
    pub library: Option<ResolveLibrary>,
    /// RFC 3339.
    pub created_at: String,
    /// Tries that failed (not counting "Resolve isn't open").
    pub tries: u32,
    pub last_outcome: Option<String>,
    pub last_try: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WaitingList {
    pub entries: Vec<WaitingEntry>,
    /// Keys this version doesn't know: kept.
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

/// The waiting list; none yet = empty. A damaged file is an error (and never written over).
pub fn load_waiting(config_dir: &Path) -> Result<WaitingList, String> {
    let path = config_dir.join(WAITING_FILE);
    if !path.exists() {
        return Ok(WaitingList::default());
    }
    let value = read_json_object(&path)?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}

pub fn save_waiting(config_dir: &Path, list: &WaitingList) -> io::Result<()> {
    let value = serde_json::to_value(list).map_err(io::Error::other)?;
    write_json_atomic(&config_dir.join(WAITING_FILE), &value, false)
}

/// Where Nest keeps its copy of a waiting project's `.drp`.
pub fn drp_copy(config_dir: &Path, id: &str) -> PathBuf {
    config_dir.join(DRP_DIR).join(format!("{id}.drp"))
}

/// After Create: copy the template's `.drp` to Nest's own `<config>/resolve/<id>.drp` (so
/// later template edits don't matter) and make the waiting entry for it.
pub fn queue(
    config_dir: &Path,
    id: &str,
    job_code: &str,
    project_dir: &Path,
    r: &crate::plan::PlannedResolve,
    template_dir: &Path,
    created_at: &str,
) -> Result<WaitingEntry, String> {
    let src = r
        .drp
        .split('/')
        .fold(template_dir.to_path_buf(), |p, s| p.join(s));
    let dest = drp_copy(config_dir, id);
    let dir = dest.parent().ok_or("no folder for the copy")?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    // Nest's own file: written next to it, then renamed into place.
    let tmp = dir.join(format!(".{id}.drp.tmp"));
    fs::copy(&src, &tmp).map_err(|e| format!("couldn't copy {}: {e}", src.display()))?;
    if let Err(e) = fs::rename(&tmp, &dest) {
        let _ = fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    Ok(WaitingEntry {
        id: id.to_string(),
        job_code: job_code.to_string(),
        project_dir: project_dir.to_path_buf(),
        drp: dest,
        name: r.name.clone(),
        folder: r.folder.clone(),
        library: r.library.clone(),
        created_at: created_at.to_string(),
        tries: 0,
        last_outcome: None,
        last_try: None,
    })
}

/// What happens to an entry after a try.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// Done: off the list; the project's file says `status`.
    Done {
        status: &'static str,
        notice: String,
    },
    /// Still waiting.
    Keep,
    /// Stopped trying: off the list, with a note.
    GiveUp { note: String },
}

/// Record a try's outcome on `entry` and say what happens next.
pub fn after(
    entry: &mut WaitingEntry,
    outcome: &Outcome,
    now: chrono::DateTime<chrono::FixedOffset>,
) -> Next {
    let name = entry.name.clone();
    entry.last_outcome = Some(outcome.key().to_string());
    entry.last_try = Some(now.to_rfc3339());
    match outcome {
        Outcome::Added => Next::Done {
            status: "added",
            notice: format!("Added to Resolve: {name}"),
        },
        Outcome::Exists => Next::Done {
            status: "exists",
            notice: format!("A project called {name} is already in Resolve, so Nest left it."),
        },
        Outcome::Missing => Next::GiveUp {
            note: format!(
                "Nest's copy of the Resolve project for {name} is gone, so it stopped trying."
            ),
        },
        Outcome::Unreachable(_) | Outcome::Busy => Next::Keep,
        Outcome::NoFolder | Outcome::NoLibrary | Outcome::Failed(_) => {
            entry.tries += 1;
            let old = chrono::DateTime::parse_from_rfc3339(&entry.created_at)
                .map(|at| now.signed_duration_since(at).num_days() >= GIVE_UP_DAYS)
                .unwrap_or(false);
            if old {
                Next::GiveUp {
                    note: format!(
                        "Nest couldn't add {name} to Resolve for {GIVE_UP_DAYS} days, so it stopped trying."
                    ),
                }
            } else {
                Next::Keep
            }
        }
    }
}

/// Set `resolve.status` and `resolve.at` in a project's `.project.json`, keeping every other
/// field (CLAUDE.md #5), atomically. Refuses if the file now belongs to another project, or
/// changed while Nest was working on it.
pub fn update_manifest(
    file: &Path,
    expected_id: &str,
    status: &str,
    at: &str,
) -> Result<(), String> {
    let stamp = |p: &Path| fs::metadata(p).map(|m| (m.modified().ok(), m.len()));
    let before = stamp(file).map_err(|e| format!("Couldn't read the project file: {e}"))?;
    let mut manifest = read_json_object(file)
        .map_err(|e| format!("The project file can't be read ({e}), so it was left untouched."))?;
    if manifest["id"].as_str() != Some(expected_id) {
        return Err("This folder now holds a different project.".into());
    }
    if !manifest["resolve"].is_object() {
        manifest["resolve"] = serde_json::json!({});
    }
    manifest["resolve"]["status"] = status.into();
    manifest["resolve"]["at"] = at.into();
    if stamp(file).ok() != Some(before) {
        return Err("The project file was changed by something else just now.".into());
    }
    write_json_atomic(file, &manifest, true)
        .map_err(|e| format!("Couldn't save the project file: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn lib(name: &str) -> ResolveLibrary {
        ResolveLibrary {
            db_type: "Disk".into(),
            db_name: name.into(),
        }
    }

    #[test]
    fn hex_round_trips_any_text() {
        for s in [
            "",
            "Acme",
            "꿈의 노래",
            "夏天",
            "a\"b'c]]--\n",
            "C:\\Jobs\\x.drp",
        ] {
            assert_eq!(unhex(&hex(s)).as_deref(), Some(s));
        }
        assert_eq!(hex(""), "x");
        assert_eq!(unhex("abc"), None, "no x");
        assert_eq!(unhex("x6"), None, "odd length");
        assert_eq!(unhex("xzz"), None);
    }

    #[test]
    fn arguments_never_carry_names_as_text() {
        let lib = lib("Work");
        let a = args(&Op::Add {
            drp: Path::new("/tmp/x.drp"),
            name: "Acme\"); os.execute(\"rm -rf /\") --",
            folder: "Acme/2026",
            library: Some(&lib),
            switch: true,
        });
        assert_eq!(a[0], "add");
        assert_eq!(a.len(), 7);
        for arg in &a[1..] {
            assert!(arg.starts_with('x'), "{arg}");
            assert!(
                arg[1..].chars().all(|c| c.is_ascii_hexdigit()),
                "only hex: {arg}"
            );
        }
        assert_eq!(
            unhex(&a[2]).unwrap(),
            "Acme\"); os.execute(\"rm -rf /\") --"
        );
        assert_eq!(unhex(&a[3]).unwrap(), "Acme/2026");
        assert_eq!(unhex(&a[5]).unwrap(), "Work");
        assert_eq!(unhex(&a[6]).unwrap(), "1");
        // No library: empty type and name, no switch.
        let a = args(&Op::Add {
            drp: Path::new("/tmp/x.drp"),
            name: "n",
            folder: "",
            library: None,
            switch: false,
        });
        assert_eq!(&a[3..], ["x", "x", "x", "x30"]);
        assert_eq!(args(&Op::Status), ["status"]);
    }

    #[test]
    fn the_script_never_builds_code_from_arguments() {
        // Names only ever reach Resolve's own calls; nothing is loaded or run as Lua.
        for bad in [
            "loadstring",
            "load(",
            "dofile",
            "os.execute",
            "io.popen",
            "require",
        ] {
            assert!(!SCRIPT.contains(bad), "{bad}");
        }
    }

    #[test]
    fn parsing_skips_banner_noise_and_reads_answers() {
        let out = format!(
            "Fusion Script Server v21.1 (c) Blackmagic Design\r\nsome warning\r\nNEST current {} {}\r\nNEST open {}\r\nNEST library {} {}\r\nNEST project x {}\r\nNEST project {} {}\r\nNEST nonsense\r\nNEST open zz\r\nNEST result ok\r\n",
            hex("Disk"),
            hex("Local Database"),
            hex("Untitled Project"),
            hex("PostgreSQL"),
            hex("Studio"),
            hex("Grade"),
            hex("Acme/2026"),
            hex("꿈"),
        );
        let lines = parse(&out);
        assert_eq!(
            lines,
            vec![
                Line::Current(ResolveLibrary {
                    db_type: "Disk".into(),
                    db_name: "Local Database".into()
                }),
                Line::Open(UNTITLED.into()),
                Line::Library(ResolveLibrary {
                    db_type: "PostgreSQL".into(),
                    db_name: "Studio".into()
                }),
                Line::Project {
                    folder: "".into(),
                    name: "Grade".into()
                },
                Line::Project {
                    folder: "Acme/2026".into(),
                    name: "꿈".into()
                },
                Line::Result("ok".into(), None),
            ]
        );
        assert_eq!(result_of(&lines), Some(("ok", None)));
        assert_eq!(
            result_of(&parse(&format!("NEST result failed {}", hex("boom")))),
            Some(("failed", Some("boom")))
        );
        assert_eq!(result_of(&parse("Fusion banner only\n")), None);
    }

    #[test]
    fn decide_switches_libraries_only_with_nothing_real_open() {
        let (work, home) = (lib("Work"), lib("Home"));
        assert_eq!(decide(None, &work, Some("Big Job")), When::Now);
        assert_eq!(decide(Some(&work), &work, Some("Big Job")), When::Now);
        assert_eq!(decide(Some(&home), &work, Some(UNTITLED)), When::Now);
        assert_eq!(decide(Some(&home), &work, None), When::Now);
        assert_eq!(decide(Some(&home), &work, Some("")), When::Now);
        assert_eq!(
            decide(Some(&home), &work, Some("Big Job")),
            When::WaitForNoProject
        );
        // Same name, other type: another library.
        let pg = ResolveLibrary {
            db_type: "PostgreSQL".into(),
            db_name: "Work".into(),
        };
        assert_eq!(
            decide(Some(&pg), &work, Some("Big Job")),
            When::WaitForNoProject
        );
    }

    /// A pretend fuscript: answers each operation with the given text, and records the calls.
    fn fake<'a>(
        answers: &'a [(&'a str, String)],
        calls: &'a RefCell<Vec<Vec<String>>>,
    ) -> impl Fn(&[String]) -> Result<String, String> + 'a {
        move |a: &[String]| {
            calls.borrow_mut().push(a.to_vec());
            answers
                .iter()
                .find(|(op, _)| *op == a[0])
                .map(|(_, out)| Ok(out.clone()))
                .unwrap_or_else(|| Err("no answer".into()))
        }
    }

    fn state(current: &str, open: &str) -> String {
        format!(
            "banner\nNEST current {} {}\nNEST open {}\nNEST result ok\n",
            hex("Disk"),
            hex(current),
            hex(open)
        )
    }

    #[test]
    fn add_follows_decide_and_reads_the_result() {
        let dir = tempfile::tempdir().unwrap();
        let drp = dir.path().join("x.drp");
        fs::write(&drp, "drp").unwrap();
        let yes = || true;
        let calls = RefCell::new(vec![]);

        // Into the open library: added.
        let answers = [
            ("state", state("Work", "Big Job")),
            ("add", "NEST result added\n".to_string()),
        ];
        let run = fake(&answers, &calls);
        let link = Link {
            run: &run,
            running: &yes,
        };
        assert_eq!(link.add(&drp, "Grade", "Acme", None), Outcome::Added);
        assert_eq!(unhex(&calls.borrow()[1][6]).unwrap(), "0", "no switch");

        // Another library with a real project open: waits, nothing added.
        calls.borrow_mut().clear();
        let home = lib("Home");
        assert_eq!(link.add(&drp, "Grade", "Acme", Some(&home)), Outcome::Busy);
        assert_eq!(calls.borrow().len(), 1, "only asked about the state");

        // Another library, nothing open: switches.
        calls.borrow_mut().clear();
        let answers = [
            ("state", state("Work", UNTITLED)),
            ("add", "NEST result exists\n".to_string()),
        ];
        let run = fake(&answers, &calls);
        let link = Link {
            run: &run,
            running: &yes,
        };
        assert_eq!(
            link.add(&drp, "Grade", "Acme", Some(&home)),
            Outcome::Exists
        );
        assert_eq!(unhex(&calls.borrow()[1][6]).unwrap(), "1", "switch");

        // No answer: scripting off while Resolve runs, else not running.
        let answers = [("state", "NEST result no-resolve\n".to_string())];
        let run = fake(&answers, &calls);
        let no = || false;
        let link = Link {
            run: &run,
            running: &yes,
        };
        assert_eq!(
            link.add(&drp, "Grade", "", None),
            Outcome::Unreachable(ResolveStatus::ScriptingOff)
        );
        assert_eq!(link.status(), ResolveStatus::ScriptingOff);
        let link = Link {
            run: &run,
            running: &no,
        };
        assert_eq!(
            link.add(&drp, "Grade", "", None),
            Outcome::Unreachable(ResolveStatus::NotRunning)
        );

        // The copy is gone: nothing is asked.
        calls.borrow_mut().clear();
        assert_eq!(
            link.add(&dir.path().join("gone.drp"), "Grade", "", None),
            Outcome::Missing
        );
        assert!(calls.borrow().is_empty());

        // A run that times out or prints nothing useful is a failure.
        let answers: [(&str, String); 0] = [];
        let run = fake(&answers, &calls);
        let link = Link {
            run: &run,
            running: &yes,
        };
        assert!(matches!(
            link.add(&drp, "Grade", "", None),
            Outcome::Failed(_)
        ));
    }

    #[test]
    fn libraries_and_projects_are_read() {
        let calls = RefCell::new(vec![]);
        let answers = [
            (
                "libraries",
                format!(
                    "NEST current {a} {b}\nNEST open x\nNEST library {a} {b}\nNEST library {a} {c}\nNEST result ok\n",
                    a = hex("Disk"),
                    b = hex("Local Database"),
                    c = hex("Archive")
                ),
            ),
            (
                "projects",
                format!(
                    "NEST project x {}\nNEST project {} {}\nNEST result ok\n",
                    hex("One"),
                    hex("Acme"),
                    hex("Two")
                ),
            ),
        ];
        let run = fake(&answers, &calls);
        let yes = || true;
        let link = Link {
            run: &run,
            running: &yes,
        };
        let libs = link.libraries().unwrap();
        assert_eq!(libs.current, Some(lib("Local Database")));
        assert_eq!(libs.libraries, vec![lib("Local Database"), lib("Archive")]);
        let projects = link.projects().unwrap();
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[1].folder, "Acme");
        assert_eq!(projects[1].name, "Two");
    }

    fn entry(created_at: &str) -> WaitingEntry {
        WaitingEntry {
            id: "p1".into(),
            job_code: "ACME-W01".into(),
            project_dir: PathBuf::from("/Jobs/x"),
            drp: PathBuf::from("/config/resolve/p1.drp"),
            name: "Grade".into(),
            folder: "Acme".into(),
            library: None,
            created_at: created_at.into(),
            tries: 0,
            last_outcome: None,
            last_try: None,
        }
    }

    fn at(s: &str) -> chrono::DateTime<chrono::FixedOffset> {
        chrono::DateTime::parse_from_rfc3339(s).unwrap()
    }

    #[test]
    fn outcomes_move_entries_on() {
        let now = at("2026-10-17T10:00:00+08:00");
        let mut e = entry("2026-10-17T09:00:00+08:00");
        assert!(matches!(
            after(&mut e, &Outcome::Added, now),
            Next::Done { status: "added", ref notice } if notice == "Added to Resolve: Grade"
        ));
        assert!(matches!(
            after(&mut e, &Outcome::Exists, now),
            Next::Done { status: "exists", ref notice }
                if notice == "A project called Grade is already in Resolve, so Nest left it."
        ));
        // Not reachable or busy: waits, and doesn't count as a failed try.
        for o in [
            Outcome::Unreachable(ResolveStatus::NotRunning),
            Outcome::Unreachable(ResolveStatus::ScriptingOff),
            Outcome::Busy,
        ] {
            assert_eq!(after(&mut e, &o, now), Next::Keep);
        }
        assert_eq!(e.tries, 0);
        assert_eq!(e.last_outcome.as_deref(), Some("busy"));
        // Failed: counted, kept for 30 days, then dropped with a note.
        assert_eq!(after(&mut e, &Outcome::Failed("x".into()), now), Next::Keep);
        assert_eq!(e.tries, 1);
        let mut old = entry("2026-09-16T09:00:00+08:00");
        assert!(matches!(
            after(&mut old, &Outcome::NoFolder, now),
            Next::GiveUp { .. }
        ));
        assert!(matches!(
            after(&mut e, &Outcome::Missing, now),
            Next::GiveUp { .. }
        ));
    }

    #[test]
    fn waiting_list_round_trips_and_keeps_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_waiting(dir.path()).unwrap(), WaitingList::default());
        let mut list = WaitingList::default();
        let mut e = entry("2026-10-17T09:00:00+08:00");
        e.name = "꿈의 노래".into();
        e.library = Some(lib("Work"));
        list.entries.push(e);
        list.other.insert("future".into(), serde_json::json!([1]));
        save_waiting(dir.path(), &list).unwrap();
        assert_eq!(load_waiting(dir.path()).unwrap(), list);
        // A damaged file is an error, never an empty list.
        fs::write(dir.path().join(WAITING_FILE), "{ nope").unwrap();
        assert!(load_waiting(dir.path()).is_err());
    }

    #[test]
    fn manifest_status_keeps_every_other_field() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(".project.json");
        let original = serde_json::json!({
            "schema": 1, "id": "p1", "jobCode": "ACME-W01",
            "resolve": { "name": "Grade", "folder": "Acme", "library": null,
                         "status": "waiting", "at": "t0", "later": true },
            "links": { "x": 1 }, "futureThing": [1, 2]
        });
        write_json_atomic(&file, &original, true).unwrap();
        update_manifest(&file, "p1", "added", "t1").unwrap();
        let got = read_json_object(&file).unwrap();
        let mut want = original.clone();
        want["resolve"]["status"] = "added".into();
        want["resolve"]["at"] = "t1".into();
        assert_eq!(got, want);
        // Key order kept too.
        let keys: Vec<&String> = got.as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            ["schema", "id", "jobCode", "resolve", "links", "futureThing"]
        );
        // Another project in that folder now: untouched.
        assert!(update_manifest(&file, "p2", "added", "t2").is_err());
        assert_eq!(read_json_object(&file).unwrap(), want);
    }

    #[test]
    fn queue_keeps_its_own_copy_of_the_drp() {
        let dir = tempfile::tempdir().unwrap();
        let template = dir.path().join("template");
        fs::create_dir_all(template.join("files")).unwrap();
        fs::write(template.join("files/Grade.drp"), b"v1").unwrap();
        let config = dir.path().join("config");
        let r = crate::plan::PlannedResolve {
            drp: "files/Grade.drp".into(),
            name: "꿈 Grade".into(),
            folder: "Acme/2026".into(),
            library: Some(lib("Work")),
            library_label: "Work".into(),
        };
        let e = queue(
            &config,
            "p1",
            "ACME-W01",
            Path::new("/Jobs/x"),
            &r,
            &template,
            "2026-10-17T09:00:00+08:00",
        )
        .unwrap();
        assert_eq!(e.drp, drp_copy(&config, "p1"));
        assert_eq!(
            (e.name.as_str(), e.folder.as_str()),
            ("꿈 Grade", "Acme/2026")
        );
        assert_eq!(e.library, Some(lib("Work")));
        // A later template edit doesn't change what waits.
        fs::write(template.join("files/Grade.drp"), b"v2").unwrap();
        assert_eq!(fs::read(&e.drp).unwrap(), b"v1");
        assert_eq!(
            fs::read_dir(config.join(DRP_DIR)).unwrap().count(),
            1,
            "no tmp left"
        );
        // Gone from the template: an error, nothing kept.
        assert!(queue(
            &config,
            "p2",
            "X",
            Path::new("/J"),
            &r,
            &dir.path().join("nope"),
            "t"
        )
        .is_err());
        assert!(!drp_copy(&config, "p2").exists());
    }
}

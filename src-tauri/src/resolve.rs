//! DaVinci Resolve starter projects: talking to Resolve, and the waiting list.
//!
//! Nest never links to Resolve. It runs Resolve's own script runner (`fuscript`) with a small
//! Lua helper bundled in the binary (`resolve.lua`), one run at a time, 30 seconds at most (two
//! minutes for an add that switches library, so a run is never cut between the switch and the
//! switch back). Every name goes in as an argument (hex-encoded), never into Lua source, and the
//! helper answers with `NEST …` lines; whatever else fuscript prints is ignored.
//!
//! Resolve answers only while it runs and only with External scripting set to Local; both
//! look the same from the script (no answer), so whether the Resolve process runs tells them
//! apart. A project waits in `<config>/resolve-waiting.json` until Resolve takes it.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::manifest::{read_json_object, write_json_atomic};
use crate::settings::ResolveLibrary;

/// The Lua helper, bundled.
pub const SCRIPT: &str = include_str!("resolve.lua");
/// Longest one run may take before Nest stops it.
pub const TIMEOUT: Duration = Duration::from_secs(30);
/// Longest an add that switches library may take: never cut between the switch and the switch
/// back (a big `.drp` can take a while to import).
pub const SWITCH_TIMEOUT: Duration = Duration::from_secs(120);
/// Resolve answers a while after its process appears: until then it's "starting", not
/// "scripting off".
pub const STARTING_FOR: Duration = Duration::from_secs(30);
/// An entry that needs another library and keeps failing is tried at most this often.
pub const SWITCH_BACKOFF_MINUTES: i64 = 10;
pub const WAITING_FILE: &str = "resolve-waiting.json";
/// Nest's own copies of waiting `.drp` files (`<config>/resolve/<id>.drp`).
pub const DRP_DIR: &str = "resolve";
/// A project that keeps failing stops being tried after this many days.
pub const GIVE_UP_DAYS: i64 = 30;

/// One run of fuscript at a time, from anywhere in Nest.
static RUN_LOCK: Mutex<()> = Mutex::new(());
/// When Nest first saw Resolve's process (this time it runs).
static FIRST_SEEN: Mutex<Option<Instant>> = Mutex::new(None);

// ───────────────────────── Status and outcomes ─────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResolveStatus {
    /// Resolve runs and answers.
    Ready,
    /// Resolve's process appeared moments ago: it answers once it's up.
    Starting,
    /// Resolve runs but doesn't answer: External scripting is off.
    ScriptingOff,
    /// Resolve runs but didn't answer in time (busy with something else).
    NoAnswer,
    /// Nest couldn't run Resolve's script runner (or write its helper for it).
    NoHelper,
    NotRunning,
    NotInstalled,
}

impl ResolveStatus {
    /// Plain words for the UI.
    pub fn message(self) -> &'static str {
        match self {
            ResolveStatus::Ready => "Resolve is ready.",
            ResolveStatus::Starting => "Resolve is starting…",
            ResolveStatus::ScriptingOff => {
                "Resolve isn't letting Nest in. In Resolve, open Preferences → System → General and set \"External scripting using\" to Local."
            }
            ResolveStatus::NoAnswer => {
                "Resolve didn't answer in time (it may be busy). Nest will try again."
            }
            ResolveStatus::NoHelper => "Nest couldn't talk to Resolve's script runner.",
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
    /// Not reachable right now (not running, starting, scripting off, no answer in time, the
    /// script runner can't be run, not installed): the pass stops, no try is counted.
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
            Outcome::Unreachable(ResolveStatus::Starting) => "starting",
            Outcome::Unreachable(ResolveStatus::ScriptingOff) => "scriptingOff",
            Outcome::Unreachable(ResolveStatus::NoAnswer) => "noAnswer",
            Outcome::Unreachable(ResolveStatus::NoHelper) => "noHelper",
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
    /// Already a whole sentence for the user.
    Plain(String),
}

impl Problem {
    pub fn message(&self) -> String {
        match self {
            Problem::Status(s) => s.message().to_string(),
            Problem::Failed(why) => format!("Resolve didn't do it: {why}"),
            Problem::Plain(text) => text.clone(),
        }
    }
}

/// Why a run of the helper gave nothing back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    /// The helper couldn't be written or fuscript couldn't be started.
    NoHelper(String),
    /// Stopped after its time limit.
    Timeout,
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
    /// The open library, the open project's name and what it holds.
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
    /// Switch to `library` (only with nothing real open): back to where Nest found Resolve.
    Switch {
        library: &'a ResolveLibrary,
    },
}

/// The helper's arguments: the operation, then each piece of text hex-encoded.
pub fn args(op: &Op) -> Vec<String> {
    let path = |p: &Path| hex(&p.to_string_lossy());
    let ip =
        |l: Option<&ResolveLibrary>| hex(l.and_then(|l| l.ip_address.as_deref()).unwrap_or(""));
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
            ip(*library),
        ],
        Op::Switch { library } => vec![
            "switch".into(),
            hex(&library.db_type),
            hex(&library.db_name),
            ip(Some(library)),
        ],
    }
}

/// How long a run of `op` may take.
pub fn time_limit(op: &Op) -> Duration {
    match op {
        Op::Add { switch: true, .. } => SWITCH_TIMEOUT,
        _ => TIMEOUT,
    }
}

/// What the open project holds (top of its media pool).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Contents {
    pub timelines: u64,
    pub clips: u64,
    pub folders: u64,
}

/// Nothing real is open: no timelines, no clips and no folders in the media pool. Unknown
/// contents count as real work. Decided from contents, never from the name (Resolve names an
/// empty project differently per language, and a real one can be called anything).
pub fn nothing_real_open(contents: Option<Contents>) -> bool {
    contents.is_some_and(|c| c.timelines == 0 && c.clips == 0 && c.folders == 0)
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
    Contents(Contents),
    /// The add switched library.
    Switched,
    /// The switch back after an add: true = done.
    Back(bool),
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
            let lib = |a: &str, b: &str, ip: Option<&str>| {
                let ip = match ip {
                    Some(ip) => Some(unhex(ip)?).filter(|ip| !ip.is_empty()),
                    None => None,
                };
                Some(ResolveLibrary {
                    db_type: unhex(a)?,
                    db_name: unhex(b)?,
                    ip_address: ip,
                })
            };
            let num = |n: &str| n.parse::<u64>().ok();
            match parts.as_slice() {
                ["result", word] => Some(Line::Result(word.to_string(), None)),
                ["result", word, why] => Some(Line::Result(word.to_string(), unhex(why))),
                ["current", a, b] => lib(a, b, None).map(Line::Current),
                ["current", a, b, ip] => lib(a, b, Some(ip)).map(Line::Current),
                ["library", a, b] => lib(a, b, None).map(Line::Library),
                ["library", a, b, ip] => lib(a, b, Some(ip)).map(Line::Library),
                ["open", name] => unhex(name).map(Line::Open),
                ["contents", t, c, f] => Some(Line::Contents(Contents {
                    timelines: num(t)?,
                    clips: num(c)?,
                    folders: num(f)?,
                })),
                ["switched"] => Some(Line::Switched),
                ["back", "ok"] => Some(Line::Back(true)),
                ["back", "fail"] => Some(Line::Back(false)),
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

/// How the switch back after an add went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Back {
    /// No switch was asked for, or the helper didn't switch (busy, no such library).
    NoSwitch,
    Done,
    /// Resolve was switched and couldn't be switched back.
    Failed,
    /// The run stopped without saying (timed out after the switch, or before saying anything).
    Unknown,
}

/// From an add's answer lines (None = the run gave nothing back).
pub fn back_of(asked_switch: bool, lines: Option<&[Line]>) -> Back {
    if !asked_switch {
        return Back::NoSwitch;
    }
    let Some(lines) = lines else {
        return Back::Unknown;
    };
    if let Some(ok) = lines.iter().find_map(|l| match l {
        Line::Back(ok) => Some(*ok),
        _ => None,
    }) {
        return if ok { Back::Done } else { Back::Failed };
    }
    // Finished without switching (something real open, no such library, Resolve not there).
    if !lines.contains(&Line::Switched) && result_of(lines).is_some() {
        Back::NoSwitch
    } else {
        Back::Unknown
    }
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
    contents: Option<Contents>,
) -> When {
    match target {
        Some(t) if !t.same(current) => {
            if nothing_real_open(contents) {
                When::Now
            } else {
                When::WaitForNoProject
            }
        }
        _ => When::Now,
    }
}

/// No answer from the helper: by how long Resolve's process has run (None = it doesn't).
pub fn not_answering(running_for: Option<Duration>) -> ResolveStatus {
    match running_for {
        None => ResolveStatus::NotRunning,
        Some(d) if d < STARTING_FOR => ResolveStatus::Starting,
        Some(_) => ResolveStatus::ScriptingOff,
    }
}

/// A run timed out: still starting, or busy.
pub fn timed_out(running_for: Option<Duration>) -> ResolveStatus {
    match running_for {
        None => ResolveStatus::NotRunning,
        Some(d) if d < STARTING_FOR => ResolveStatus::Starting,
        Some(_) => ResolveStatus::NoAnswer,
    }
}

/// Nest left Resolve in another library (an add switched, and the switch back failed or was
/// never heard of). Kept in the waiting list until Resolve is back where it was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Switched {
    /// Where Resolve was (exactly as it reported it).
    pub from: ResolveLibrary,
    /// Where Nest switched it to.
    pub to: ResolveLibrary,
    /// RFC 3339.
    pub at: String,
    /// The switch back was tried and failed (the user was told).
    #[serde(default)]
    pub back_failed: bool,
}

impl Switched {
    pub fn message(&self) -> String {
        switch_back_message(&self.from)
    }
}

pub fn switch_back_message(from: &ResolveLibrary) -> String {
    format!(
        "Nest couldn't switch Resolve back to {}. Choose it in Resolve's Project Libraries.",
        from.db_name
    )
}

/// What to do about a library Nest may have left Resolve in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recover {
    /// Still in Nest's library with nothing real open: switch back.
    SwitchBack,
    /// Resolve is elsewhere now, or someone works in that library: leave it, forget it.
    Forget,
}

pub fn recover(rec: &Switched, current: &ResolveLibrary, contents: Option<Contents>) -> Recover {
    if current.same(&rec.to) && !current.same(&rec.from) && nothing_real_open(contents) {
        Recover::SwitchBack
    } else {
        Recover::Forget
    }
}

// ───────────────────────── Talking to Resolve ─────────────────────────

/// Runs the helper with these arguments (stopped after the time limit) and gives back what it
/// printed.
pub type Runner<'a> = dyn Fn(&[String], Duration) -> Result<String, RunError> + 'a;

/// The real thing: fuscript (if installed) and the process check.
pub struct Link<'a> {
    pub run: &'a Runner<'a>,
    /// How long Resolve's process has run (None = not running).
    pub running: &'a dyn Fn() -> Option<Duration>,
}

/// What Resolve has open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenState {
    pub current: ResolveLibrary,
    pub open: String,
    pub contents: Option<Contents>,
}

/// How an add went, and the switch back if there was one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddReport {
    pub outcome: Outcome,
    pub back: Back,
    /// The library Resolve was in when Nest switched (Some only when a switch was asked).
    pub from: Option<ResolveLibrary>,
}

/// Does the rest of the pass wait? Yes when Resolve can't be reached (not running, starting,
/// scripting off, timed out, no script runner) or may be left in Nest's library.
pub fn stops_pass(report: &AddReport) -> bool {
    matches!(report.outcome, Outcome::Unreachable(_))
        || matches!(report.back, Back::Failed | Back::Unknown)
}

impl Link<'_> {
    /// Run `op` and read its answer lines (no reading of the result word).
    fn ask_raw(&self, op: &Op) -> Result<Vec<Line>, Problem> {
        match (self.run)(&args(op), time_limit(op)) {
            Ok(out) => Ok(parse(&out)),
            Err(RunError::NoHelper(why)) => {
                log::info!("Resolve's script runner: {why}");
                Err(Problem::Status(ResolveStatus::NoHelper))
            }
            Err(RunError::Timeout) => Err(Problem::Status(timed_out((self.running)()))),
        }
    }

    /// Run `op`; a "no answer" becomes the right status.
    fn ask(&self, op: &Op) -> Result<Vec<Line>, Problem> {
        let lines = self.ask_raw(op)?;
        self.check(lines)
    }

    fn check(&self, lines: Vec<Line>) -> Result<Vec<Line>, Problem> {
        match result_of(&lines) {
            Some(("no-resolve", _)) => Err(Problem::Status(not_answering((self.running)()))),
            Some(("failed", why)) => Err(Problem::Failed(
                why.unwrap_or("it gave no reason").to_string(),
            )),
            Some(_) => Ok(lines),
            // Ran but said nothing Nest understands: as good as no answer.
            None => Err(Problem::Status(timed_out((self.running)()))),
        }
    }

    pub fn status(&self) -> ResolveStatus {
        match self.ask(&Op::Status) {
            Ok(_) => ResolveStatus::Ready,
            Err(Problem::Status(s)) => s,
            Err(e) => {
                log::info!("Resolve status: {}", e.message());
                not_answering((self.running)())
            }
        }
    }

    /// The open library, the open project and what it holds.
    pub fn state(&self) -> Result<OpenState, Problem> {
        let lines = self.ask(&Op::State)?;
        let mut current = None;
        let mut open = String::new();
        let mut contents = None;
        for l in lines {
            match l {
                Line::Current(lib) => current = Some(lib),
                Line::Open(name) => open = name,
                Line::Contents(c) => contents = Some(c),
                _ => {}
            }
        }
        let current = current
            .ok_or_else(|| Problem::Failed("Resolve didn't say which library is open".into()))?;
        Ok(OpenState {
            current,
            open,
            contents,
        })
    }

    /// Switch Resolve to `library` (the helper checks again that nothing real is open).
    /// Ok(true) = switched; Ok(false) = not now (something real is open, or no such library).
    pub fn switch_to(&self, library: &ResolveLibrary) -> Result<bool, Problem> {
        let lines = self.ask(&Op::Switch { library })?;
        Ok(matches!(result_of(&lines), Some(("ok", _))))
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
        let couldnt = || Problem::Plain(format!("Resolve couldn't export {name}."));
        let lines = match self.ask(&Op::Export { name, folder, dest }) {
            Ok(lines) => lines,
            Err(Problem::Failed(why)) => {
                log::info!("Resolve export of {name}: {why}");
                return Err(couldnt());
            }
            Err(e) => return Err(e),
        };
        match result_of(&lines) {
            Some(("ok", _)) if dest.is_file() => Ok(()),
            Some(("no-folder", _)) => Err(Problem::Plain(format!(
                "Resolve couldn't export {name}: the folder \"{folder}\" isn't in Resolve's open library."
            ))),
            _ => Err(couldnt()),
        }
    }

    /// Add the `.drp` as project `name` in `folder` of `library` (None = the open one).
    /// `before_switch` hears where Resolve was, just before a run that switches library (so a
    /// run that never comes back can be undone later).
    pub fn add(
        &self,
        drp: &Path,
        name: &str,
        folder: &str,
        library: Option<&ResolveLibrary>,
        before_switch: &mut dyn FnMut(&ResolveLibrary),
    ) -> AddReport {
        let report = |outcome| AddReport {
            outcome,
            back: Back::NoSwitch,
            from: None,
        };
        if !drp.is_file() {
            return report(Outcome::Missing);
        }
        let state = match self.state() {
            Ok(state) => state,
            Err(Problem::Status(s)) => return report(Outcome::Unreachable(s)),
            Err(e) => return report(Outcome::Failed(e.message())),
        };
        if decide(library, &state.current, state.contents) == When::WaitForNoProject {
            return report(Outcome::Busy);
        }
        let switch = library.is_some_and(|l| !l.same(&state.current));
        if switch {
            before_switch(&state.current);
        }
        let op = Op::Add {
            drp,
            name,
            folder,
            library,
            switch,
        };
        let from = switch.then(|| state.current.clone());
        let lines = match self.ask_raw(&op) {
            Ok(lines) => lines,
            Err(Problem::Status(s)) => {
                return AddReport {
                    outcome: Outcome::Unreachable(s),
                    back: back_of(switch, None),
                    from,
                }
            }
            Err(e) => {
                return AddReport {
                    outcome: Outcome::Failed(e.message()),
                    back: back_of(switch, None),
                    from,
                }
            }
        };
        let back = back_of(switch, Some(&lines));
        let outcome = match self.check(lines) {
            Err(Problem::Status(s)) => Outcome::Unreachable(s),
            Err(e) => Outcome::Failed(e.message()),
            Ok(lines) => match result_of(&lines) {
                Some(("added", _)) => Outcome::Added,
                Some(("exists", _)) => Outcome::Exists,
                Some(("busy", _)) => Outcome::Busy,
                Some(("no-folder", _)) => Outcome::NoFolder,
                Some(("no-library", _)) => Outcome::NoLibrary,
                other => Outcome::Failed(format!("unexpected answer {other:?}")),
            },
        };
        AddReport {
            outcome,
            back,
            from,
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

/// How long Resolve's process has run, as far as Nest has seen (None = not running).
pub fn running_for() -> Option<Duration> {
    let running = is_running();
    let mut first = FIRST_SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if running {
        Some(first.get_or_insert_with(Instant::now).elapsed())
    } else {
        *first = None;
        None
    }
}

/// Where the helper script is written for fuscript: a folder whose path is plain ASCII on
/// Windows (fuscript can't open a script under a non-ASCII path there): the temp folder, else
/// Nest's config folder, else `<ProgramData>\Nest`. Elsewhere the temp folder.
pub fn script_dir(windows: bool, temp: &Path, config: &Path, program_data: &Path) -> PathBuf {
    let plain = |p: &Path| p.to_str().is_some_and(|s| s.is_ascii());
    if !windows || plain(temp) {
        temp.to_path_buf()
    } else if plain(config) {
        config.to_path_buf()
    } else {
        program_data.join("Nest")
    }
}

/// `script_dir` for this computer.
pub fn script_dir_here(config: &Path) -> PathBuf {
    let program_data = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    script_dir(cfg!(windows), &std::env::temp_dir(), config, &program_data)
}

/// Run the helper with fuscript: the script goes to a file of Nest's own in `dir` (removed
/// after), the run is stopped after `timeout`, and only one runs at a time.
pub fn run(
    fuscript: &Path,
    dir: &Path,
    args: &[String],
    timeout: Duration,
) -> Result<String, RunError> {
    let _one = RUN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    fs::create_dir_all(dir)
        .map_err(|e| RunError::NoHelper(format!("couldn't make {}: {e}", dir.display())))?;
    let script = dir.join(format!(
        "nest-resolve-{}.lua",
        uuid::Uuid::new_v4().simple()
    ));
    fs::write(&script, SCRIPT)
        .map_err(|e| RunError::NoHelper(format!("couldn't write the helper: {e}")))?;
    let result = run_script(fuscript, &script, args, timeout);
    if let Err(e) = fs::remove_file(&script) {
        log::info!("couldn't remove {}: {e}", script.display());
    }
    result
}

/// How long to wait for a pipe's last words after the run ended.
const READ_GRACE: Duration = Duration::from_secs(2);

/// Read a pipe on its own thread; the text arrives on the channel when the pipe closes.
fn reader(pipe: Option<Box<dyn Read + Send>>) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(p) = pipe {
            let mut buf = vec![];
            // Answers are small; never hold more than a few MB.
            let _ = p.take(4 * 1024 * 1024).read_to_end(&mut buf);
            text = String::from_utf8_lossy(&buf).into_owned();
        }
        let _ = tx.send(text);
    });
    rx
}

fn run_script(
    fuscript: &Path,
    script: &Path,
    args: &[String],
    timeout: Duration,
) -> Result<String, RunError> {
    let mut child = quiet(&mut Command::new(fuscript))
        .arg("-l")
        .arg("lua")
        .arg(script)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| RunError::NoHelper(format!("couldn't start Resolve's script runner: {e}")))?;
    let out = reader(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let err = reader(
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
    // Never wait forever: something else may still hold the pipes open.
    let stdout = out.recv_timeout(READ_GRACE).unwrap_or_default();
    let stderr = err.recv_timeout(READ_GRACE).unwrap_or_default();
    if !stderr.trim().is_empty() {
        log::info!("Resolve's script runner said: {}", stderr.trim());
    }
    if finished {
        Ok(stdout)
    } else {
        if !stdout.trim().is_empty() {
            log::info!(
                "Resolve's script runner, stopped, had said: {}",
                stdout.trim()
            );
        }
        Err(RunError::Timeout)
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
    /// A library Nest may have left Resolve in (see `Switched`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub switched: Option<Switched>,
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

/// Plain words for a waiting list that can't be read.
pub const UNREADABLE: &str = "The waiting list can't be read.";

/// Where Nest keeps its copy of a waiting project's `.drp`.
pub fn drp_copy(config_dir: &Path, id: &str) -> PathBuf {
    config_dir.join(DRP_DIR).join(format!("{id}.drp"))
}

/// Nest's own copy for project `id`, if `id` can only name a file inside `<config>/resolve/`
/// (ids are Nest's own; anything with a path in it is refused).
pub fn own_copy(config_dir: &Path, id: &str) -> Option<PathBuf> {
    let plain = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    plain.then(|| drp_copy(config_dir, id))
}

/// Remove Nest's own copy of project `id`'s `.drp` (never any other file).
pub fn remove_copy(config_dir: &Path, id: &str) {
    let Some(path) = own_copy(config_dir, id) else {
        log::warn!("not removing a Resolve copy for the odd id {id:?}");
        return;
    };
    if let Err(e) = fs::remove_file(&path) {
        if e.kind() != io::ErrorKind::NotFound {
            log::warn!("couldn't remove {}: {e}", path.display());
        }
    }
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
    let dest = own_copy(config_dir, id).ok_or("the project has no usable id")?;
    let dir = dest.parent().ok_or("no folder for the copy")?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    // Nest's own file: written next to it, then renamed into place.
    let tmp = dir.join(format!(".{id}.drp.tmp"));
    if let Err(e) = fs::copy(&src, &tmp) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("couldn't copy {}: {e}", src.display()));
    }
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

/// Should `entry` be tried in this (automatic) pass? One that names a library and keeps
/// failing is tried at most every `SWITCH_BACKOFF_MINUTES`, so Resolve isn't switched back and
/// forth every minute. "Try now" tries everything.
pub fn due(entry: &WaitingEntry, now: chrono::DateTime<chrono::FixedOffset>) -> bool {
    if entry.library.is_none() || entry.tries == 0 {
        return true;
    }
    match entry
        .last_try
        .as_deref()
        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
    {
        Some(last) => now.signed_duration_since(last).num_minutes() >= SWITCH_BACKOFF_MINUTES,
        None => true,
    }
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

/// Set `resolve.status` and `resolve.at` (and `resolve.note` when given) in a project's
/// `.project.json`, keeping every other field (CLAUDE.md #5), atomically. Refuses if the file
/// now belongs to another project, or changed while Nest was working on it.
pub fn update_manifest(
    file: &Path,
    expected_id: &str,
    status: &str,
    at: &str,
    note: Option<&str>,
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
    if let Some(note) = note {
        manifest["resolve"]["note"] = note.into();
    }
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
            ip_address: None,
        }
    }

    const EMPTY: Contents = Contents {
        timelines: 0,
        clips: 0,
        folders: 0,
    };

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
        let mut lib = lib("Work");
        lib.ip_address = Some("10.0.0.5".into());
        let a = args(&Op::Add {
            drp: Path::new("/tmp/x.drp"),
            name: "Acme\"); os.execute(\"rm -rf /\") --",
            folder: "Acme/2026",
            library: Some(&lib),
            switch: true,
        });
        assert_eq!(a[0], "add");
        assert_eq!(a.len(), 8);
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
        assert_eq!(unhex(&a[7]).unwrap(), "10.0.0.5");
        // No library: empty type and name, no switch.
        let a = args(&Op::Add {
            drp: Path::new("/tmp/x.drp"),
            name: "n",
            folder: "",
            library: None,
            switch: false,
        });
        assert_eq!(&a[3..], ["x", "x", "x", "x30", "x"]);
        assert_eq!(args(&Op::Status), ["status"]);
        assert_eq!(
            args(&Op::Switch { library: &lib }),
            ["switch", &hex("Disk"), &hex("Work"), &hex("10.0.0.5")]
        );
    }

    #[test]
    fn an_add_that_switches_gets_the_longer_time_limit() {
        let l = lib("Work");
        let add = |switch| Op::Add {
            drp: Path::new("/x.drp"),
            name: "n",
            folder: "",
            library: Some(&l),
            switch,
        };
        assert_eq!(time_limit(&add(true)), SWITCH_TIMEOUT);
        assert_eq!(time_limit(&add(false)), TIMEOUT);
        assert_eq!(time_limit(&Op::State), TIMEOUT);
        assert!(SWITCH_TIMEOUT >= Duration::from_secs(120));
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
        // Emptiness comes from contents, never from a project's name.
        assert!(!SCRIPT.contains("Untitled"));
        // The switch back gets the library table exactly as Resolve gave it.
        assert!(SCRIPT.contains("SetCurrentDatabase(before)"));
    }

    #[test]
    fn parsing_skips_banner_noise_and_reads_answers() {
        let out = format!(
            "Fusion Script Server v21.1 (c) Blackmagic Design\r\nsome warning\r\nNEST current {} {} x\r\nNEST open {}\r\nNEST contents 0 3 1\r\nNEST contents 1 x 0\r\nNEST library {} {} {}\r\nNEST project x {}\r\nNEST project {} {}\r\nNEST nonsense\r\nNEST open zz\r\nNEST switched\r\nNEST back fail\r\nNEST back maybe\r\nNEST result ok\r\n",
            hex("Disk"),
            hex("Local Database"),
            hex("Untitled Project"),
            hex("PostgreSQL"),
            hex("Studio"),
            hex("10.0.0.5"),
            hex("Grade"),
            hex("Acme/2026"),
            hex("꿈"),
        );
        let lines = parse(&out);
        assert_eq!(
            lines,
            vec![
                Line::Current(lib("Local Database")),
                Line::Open("Untitled Project".into()),
                Line::Contents(Contents {
                    timelines: 0,
                    clips: 3,
                    folders: 1
                }),
                Line::Library(ResolveLibrary {
                    db_type: "PostgreSQL".into(),
                    db_name: "Studio".into(),
                    ip_address: Some("10.0.0.5".into()),
                }),
                Line::Project {
                    folder: "".into(),
                    name: "Grade".into()
                },
                Line::Project {
                    folder: "Acme/2026".into(),
                    name: "꿈".into()
                },
                Line::Switched,
                Line::Back(false),
                Line::Result("ok".into(), None),
            ]
        );
        assert_eq!(result_of(&lines), Some(("ok", None)));
        assert_eq!(
            result_of(&parse(&format!("NEST result failed {}", hex("boom")))),
            Some(("failed", Some("boom")))
        );
        assert_eq!(result_of(&parse("Fusion banner only\n")), None);
        // Older helpers (no address) still read.
        assert_eq!(
            parse(&format!("NEST current {} {}", hex("Disk"), hex("W"))),
            vec![Line::Current(lib("W"))]
        );
    }

    #[test]
    fn nothing_real_is_open_only_when_the_project_holds_nothing() {
        assert!(nothing_real_open(Some(EMPTY)));
        for c in [
            Contents {
                timelines: 1,
                ..EMPTY
            },
            Contents { clips: 1, ..EMPTY },
            Contents {
                folders: 1,
                ..EMPTY
            },
        ] {
            assert!(!nothing_real_open(Some(c)), "{c:?}");
        }
        // Couldn't tell: real work.
        assert!(!nothing_real_open(None));
    }

    #[test]
    fn decide_switches_libraries_only_with_nothing_real_open() {
        let (work, home) = (lib("Work"), lib("Home"));
        let busy = Some(Contents {
            timelines: 2,
            ..EMPTY
        });
        assert_eq!(decide(None, &work, busy), When::Now);
        assert_eq!(decide(Some(&work), &work, busy), When::Now);
        assert_eq!(decide(Some(&home), &work, Some(EMPTY)), When::Now);
        assert_eq!(decide(Some(&home), &work, busy), When::WaitForNoProject);
        // An "Untitled Project" with clips in it is real work; unknown contents too.
        assert_eq!(
            decide(Some(&home), &work, Some(Contents { clips: 4, ..EMPTY })),
            When::WaitForNoProject
        );
        assert_eq!(decide(Some(&home), &work, None), When::WaitForNoProject);
        // Same name, other type: another library.
        let pg = ResolveLibrary {
            db_type: "PostgreSQL".into(),
            db_name: "Work".into(),
            ip_address: None,
        };
        assert_eq!(decide(Some(&pg), &work, busy), When::WaitForNoProject);
        // Same library, one side without the address: the same.
        let mut work_ip = work.clone();
        work_ip.ip_address = Some("127.0.0.1".into());
        assert_eq!(decide(Some(&work), &work_ip, busy), When::Now);
    }

    #[test]
    fn no_answer_means_starting_scripting_off_or_not_running() {
        assert_eq!(not_answering(None), ResolveStatus::NotRunning);
        assert_eq!(
            not_answering(Some(Duration::from_secs(5))),
            ResolveStatus::Starting
        );
        assert_eq!(
            not_answering(Some(Duration::from_secs(60))),
            ResolveStatus::ScriptingOff
        );
        assert_eq!(
            timed_out(Some(Duration::from_secs(5))),
            ResolveStatus::Starting
        );
        assert_eq!(
            timed_out(Some(Duration::from_secs(60))),
            ResolveStatus::NoAnswer
        );
        assert_eq!(timed_out(None), ResolveStatus::NotRunning);
        assert_eq!(ResolveStatus::Starting.message(), "Resolve is starting…");
        assert_eq!(
            ResolveStatus::NoHelper.message(),
            "Nest couldn't talk to Resolve's script runner."
        );
    }

    #[test]
    fn the_switch_back_is_read_from_the_answer() {
        let back = |s: &str| back_of(true, Some(&parse(s)));
        assert_eq!(back_of(false, None), Back::NoSwitch);
        assert_eq!(back_of(true, None), Back::Unknown, "timed out");
        assert_eq!(
            back("NEST switched\nNEST back ok\nNEST result added\n"),
            Back::Done
        );
        assert_eq!(
            back("NEST switched\nNEST back fail\nNEST result added\n"),
            Back::Failed
        );
        // Failed inside, still switched back and said so.
        assert_eq!(
            back(&format!(
                "NEST switched\nNEST back ok\nNEST result failed {}\n",
                hex("boom")
            )),
            Back::Done
        );
        // Never switched: something real was open, or no such library.
        assert_eq!(back("NEST result busy\n"), Back::NoSwitch);
        assert_eq!(back("NEST result no-library\n"), Back::NoSwitch);
        // Switched, then nothing more: unknown (fix it on the next state).
        assert_eq!(back("NEST switched\n"), Back::Unknown);
        assert_eq!(back("banner\n"), Back::Unknown);
    }

    #[test]
    fn a_library_nest_left_resolve_in_is_switched_back_only_when_safe() {
        let rec = Switched {
            from: lib("Home"),
            to: lib("Work"),
            at: "t".into(),
            back_failed: false,
        };
        assert_eq!(
            recover(&rec, &lib("Work"), Some(EMPTY)),
            Recover::SwitchBack
        );
        // Someone works there now, or can't tell: leave it.
        assert_eq!(
            recover(&rec, &lib("Work"), Some(Contents { clips: 1, ..EMPTY })),
            Recover::Forget
        );
        assert_eq!(recover(&rec, &lib("Work"), None), Recover::Forget);
        // Resolve is elsewhere (back home already, or the user chose another).
        assert_eq!(recover(&rec, &lib("Home"), Some(EMPTY)), Recover::Forget);
        assert_eq!(recover(&rec, &lib("Other"), Some(EMPTY)), Recover::Forget);
        assert_eq!(
            rec.message(),
            "Nest couldn't switch Resolve back to Home. Choose it in Resolve's Project Libraries."
        );
    }

    #[test]
    fn the_helper_goes_to_a_plain_ascii_folder_on_windows() {
        let (temp, config, pd) = (
            Path::new(r"C:\Users\Ann\AppData\Local\Temp"),
            Path::new(r"C:\Users\Ann\AppData\Roaming\Nest"),
            Path::new(r"C:\ProgramData"),
        );
        assert_eq!(script_dir(true, temp, config, pd), temp);
        let temp_ko = Path::new(r"C:\Users\김민지\AppData\Local\Temp");
        assert_eq!(script_dir(true, temp_ko, config, pd), config);
        let config_ko = Path::new(r"C:\Users\김민지\AppData\Roaming\Nest");
        assert_eq!(
            script_dir(true, temp_ko, config_ko, pd),
            Path::new(r"C:\ProgramData").join("Nest")
        );
        // Elsewhere: the temp folder, whatever its name.
        assert_eq!(script_dir(false, temp_ko, config_ko, pd), temp_ko);
    }

    type Calls = RefCell<Vec<Vec<String>>>;

    /// A pretend fuscript: answers each operation with the given text (or the given error), and
    /// records the calls and their time limits.
    fn fake<'a>(
        answers: &'a [(&'a str, Result<String, RunError>)],
        calls: &'a Calls,
        limits: &'a RefCell<Vec<Duration>>,
    ) -> impl Fn(&[String], Duration) -> Result<String, RunError> + 'a {
        move |a: &[String], limit: Duration| {
            calls.borrow_mut().push(a.to_vec());
            limits.borrow_mut().push(limit);
            answers
                .iter()
                .find(|(op, _)| *op == a[0])
                .map(|(_, out)| out.clone())
                .unwrap_or_else(|| Ok(String::new()))
        }
    }

    fn state(current: &str, contents: Contents) -> Result<String, RunError> {
        Ok(format!(
            "banner\nNEST current {} {} x\nNEST open {}\nNEST contents {} {} {}\nNEST result ok\n",
            hex("Disk"),
            hex(current),
            hex("Anything"),
            contents.timelines,
            contents.clips,
            contents.folders
        ))
    }

    const REAL: Contents = Contents {
        timelines: 1,
        clips: 10,
        folders: 2,
    };

    #[test]
    fn add_follows_decide_and_reads_the_result() {
        let dir = tempfile::tempdir().unwrap();
        let drp = dir.path().join("x.drp");
        fs::write(&drp, "drp").unwrap();
        let long = || Some(Duration::from_secs(600));
        let calls = Calls::default();
        let limits = RefCell::new(vec![]);
        let heard = RefCell::new(vec![]);
        let mut before = |l: &ResolveLibrary| heard.borrow_mut().push(l.clone());

        // Into the open library: added, no switch.
        let answers = [
            ("state", state("Work", REAL)),
            ("add", Ok("NEST result added\n".to_string())),
        ];
        let run = fake(&answers, &calls, &limits);
        let link = Link {
            run: &run,
            running: &long,
        };
        let r = link.add(&drp, "Grade", "Acme", None, &mut before);
        assert_eq!((r.outcome, r.back), (Outcome::Added, Back::NoSwitch));
        assert_eq!(unhex(&calls.borrow()[1][6]).unwrap(), "0", "no switch");
        assert_eq!(limits.borrow()[1], TIMEOUT);

        // Another library with real work open: waits, nothing added.
        calls.borrow_mut().clear();
        let home = lib("Home");
        let r = link.add(&drp, "Grade", "Acme", Some(&home), &mut before);
        assert_eq!(r.outcome, Outcome::Busy);
        assert_eq!(calls.borrow().len(), 1, "only asked about the state");
        assert!(heard.borrow().is_empty());

        // Another library, nothing open: switches (longer limit), and back.
        calls.borrow_mut().clear();
        limits.borrow_mut().clear();
        let answers = [
            ("state", state("Work", EMPTY)),
            (
                "add",
                Ok("NEST switched\nNEST back ok\nNEST result exists\n".to_string()),
            ),
        ];
        let run = fake(&answers, &calls, &limits);
        let link = Link {
            run: &run,
            running: &long,
        };
        let r = link.add(&drp, "Grade", "Acme", Some(&home), &mut before);
        assert_eq!((r.outcome, r.back), (Outcome::Exists, Back::Done));
        assert_eq!(r.from, Some(lib("Work")));
        assert_eq!(unhex(&calls.borrow()[1][6]).unwrap(), "1", "switch");
        assert_eq!(limits.borrow()[1], SWITCH_TIMEOUT);
        assert_eq!(*heard.borrow(), [lib("Work")], "told before the switch");

        // The switch back failed: still added, and said so.
        let answers = [
            ("state", state("Work", EMPTY)),
            (
                "add",
                Ok("NEST switched\nNEST back fail\nNEST result added\n".to_string()),
            ),
        ];
        let run = fake(&answers, &calls, &limits);
        let link = Link {
            run: &run,
            running: &long,
        };
        let r = link.add(&drp, "Grade", "Acme", Some(&home), &mut before);
        assert_eq!((r.outcome, r.back), (Outcome::Added, Back::Failed));

        // Timed out after the switch: unreachable (no try counted), switch back unknown.
        let answers = [
            ("state", state("Work", EMPTY)),
            ("add", Err(RunError::Timeout)),
        ];
        let run = fake(&answers, &calls, &limits);
        let link = Link {
            run: &run,
            running: &long,
        };
        let r = link.add(&drp, "Grade", "Acme", Some(&home), &mut before);
        assert_eq!(
            (r.outcome, r.back),
            (Outcome::Unreachable(ResolveStatus::NoAnswer), Back::Unknown)
        );

        // No answer: scripting off while Resolve runs, starting just after it appeared, else
        // not running.
        let answers = [
            ("state", Ok("NEST result no-resolve\n".to_string())),
            ("status", Ok("NEST result no-resolve\n".to_string())),
        ];
        let run = fake(&answers, &calls, &limits);
        let link = Link {
            run: &run,
            running: &long,
        };
        assert_eq!(
            link.add(&drp, "Grade", "", None, &mut before).outcome,
            Outcome::Unreachable(ResolveStatus::ScriptingOff)
        );
        assert_eq!(link.status(), ResolveStatus::ScriptingOff);
        let fresh = || Some(Duration::from_secs(3));
        let link = Link {
            run: &run,
            running: &fresh,
        };
        assert_eq!(link.status(), ResolveStatus::Starting);
        let no = || None;
        let link = Link {
            run: &run,
            running: &no,
        };
        assert_eq!(
            link.add(&drp, "Grade", "", None, &mut before).outcome,
            Outcome::Unreachable(ResolveStatus::NotRunning)
        );

        // The copy is gone: nothing is asked.
        calls.borrow_mut().clear();
        assert_eq!(
            link.add(&dir.path().join("gone.drp"), "Grade", "", None, &mut before)
                .outcome,
            Outcome::Missing
        );
        assert!(calls.borrow().is_empty());

        // A run that prints nothing useful, or times out, is no answer (unreachable), and the
        // script runner not starting is its own status.
        let answers: [(&str, Result<String, RunError>); 0] = [];
        let run = fake(&answers, &calls, &limits);
        let link = Link {
            run: &run,
            running: &long,
        };
        assert_eq!(
            link.add(&drp, "Grade", "", None, &mut before).outcome,
            Outcome::Unreachable(ResolveStatus::NoAnswer)
        );
        let answers = [
            ("state", Err(RunError::NoHelper("no exe".into()))),
            ("status", Err(RunError::NoHelper("no exe".into()))),
        ];
        let run = fake(&answers, &calls, &limits);
        let link = Link {
            run: &run,
            running: &long,
        };
        assert_eq!(
            link.add(&drp, "Grade", "", None, &mut before).outcome,
            Outcome::Unreachable(ResolveStatus::NoHelper)
        );
        assert_eq!(link.status(), ResolveStatus::NoHelper);
        // A real failure inside Resolve is a failed try.
        let answers = [
            ("state", state("Work", REAL)),
            (
                "add",
                Ok(format!("NEST result failed {}\n", hex("import failed"))),
            ),
        ];
        let run = fake(&answers, &calls, &limits);
        let link = Link {
            run: &run,
            running: &long,
        };
        assert!(matches!(
            link.add(&drp, "Grade", "", None, &mut before).outcome,
            Outcome::Failed(_)
        ));
    }

    #[test]
    fn no_answer_or_a_library_left_behind_stops_the_pass() {
        let r = |outcome, back| AddReport {
            outcome,
            back,
            from: None,
        };
        for s in [
            ResolveStatus::NoAnswer,
            ResolveStatus::Starting,
            ResolveStatus::ScriptingOff,
            ResolveStatus::NoHelper,
            ResolveStatus::NotRunning,
        ] {
            assert!(stops_pass(&r(Outcome::Unreachable(s), Back::NoSwitch)));
        }
        assert!(stops_pass(&r(Outcome::Added, Back::Failed)));
        assert!(stops_pass(&r(
            Outcome::Unreachable(ResolveStatus::NoAnswer),
            Back::Unknown
        )));
        for o in [
            Outcome::Added,
            Outcome::Exists,
            Outcome::Busy,
            Outcome::NoFolder,
            Outcome::Failed("x".into()),
        ] {
            assert!(!stops_pass(&r(o.clone(), Back::NoSwitch)), "{o:?}");
            assert!(!stops_pass(&r(o, Back::Done)));
        }
    }

    #[test]
    fn export_failures_reach_the_user_in_plain_words() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("Grade.drp");
        let calls = Calls::default();
        let limits = RefCell::new(vec![]);
        let long = || Some(Duration::from_secs(600));
        for answer in [
            Ok("NEST result failed\n".to_string()),
            Ok(format!("NEST result failed {}\n", hex("x"))),
            Ok("NEST result ok\n".to_string()), // said ok, but no file
        ] {
            let answers = [("export", answer)];
            let run = fake(&answers, &calls, &limits);
            let link = Link {
                run: &run,
                running: &long,
            };
            assert_eq!(
                link.export("꿈 Grade", "", &dest).unwrap_err().message(),
                "Resolve couldn't export 꿈 Grade."
            );
        }
    }

    #[test]
    fn libraries_and_projects_are_read() {
        let calls = Calls::default();
        let limits = RefCell::new(vec![]);
        let answers = [
            (
                "libraries",
                Ok(format!(
                    "NEST current {a} {b} x\nNEST open x\nNEST library {a} {b} x\nNEST library {a} {c} x\nNEST result ok\n",
                    a = hex("Disk"),
                    b = hex("Local Database"),
                    c = hex("Archive")
                )),
            ),
            (
                "projects",
                Ok(format!(
                    "NEST project x {}\nNEST project {} {}\nNEST result ok\n",
                    hex("One"),
                    hex("Acme"),
                    hex("Two")
                )),
            ),
        ];
        let run = fake(&answers, &calls, &limits);
        let long = || Some(Duration::from_secs(600));
        let link = Link {
            run: &run,
            running: &long,
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
        // Not reachable (timed out, starting, no runner too) or busy: waits, and doesn't
        // count as a failed try.
        for o in [
            Outcome::Unreachable(ResolveStatus::NotRunning),
            Outcome::Unreachable(ResolveStatus::ScriptingOff),
            Outcome::Unreachable(ResolveStatus::NoAnswer),
            Outcome::Unreachable(ResolveStatus::Starting),
            Outcome::Unreachable(ResolveStatus::NoHelper),
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
    fn an_entry_for_another_library_that_keeps_failing_backs_off() {
        let now = at("2026-10-17T10:00:00+08:00");
        let mut e = entry("2026-10-17T09:00:00+08:00");
        e.tries = 3;
        e.last_try = Some("2026-10-17T09:55:00+08:00".into());
        assert!(due(&e, now), "no library: every pass");
        e.library = Some(lib("Work"));
        assert!(!due(&e, now), "tried 5 minutes ago");
        e.last_try = Some("2026-10-17T09:50:00+08:00".into());
        assert!(due(&e, now), "10 minutes ago");
        e.last_try = Some("2026-10-17T09:59:00+08:00".into());
        e.tries = 0;
        assert!(due(&e, now), "never failed");
        e.tries = 1;
        e.last_try = None;
        assert!(due(&e, now));
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
        assert!(!fs::read_to_string(dir.path().join(WAITING_FILE))
            .unwrap()
            .contains("switched"));
        list.switched = Some(Switched {
            from: ResolveLibrary {
                db_type: "PostgreSQL".into(),
                db_name: "Studio".into(),
                ip_address: Some("10.0.0.5".into()),
            },
            to: lib("Work"),
            at: "t".into(),
            back_failed: true,
        });
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
        update_manifest(&file, "p1", "added", "t1", None).unwrap();
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
        // With a note.
        update_manifest(&file, "p1", "failed", "t2", Some("why")).unwrap();
        let got = read_json_object(&file).unwrap();
        assert_eq!(got["resolve"]["status"], "failed");
        assert_eq!(got["resolve"]["note"], "why");
        assert_eq!(got["resolve"]["later"], true);
        // Another project in that folder now: untouched.
        assert!(update_manifest(&file, "p2", "added", "t3", None).is_err());
        assert_eq!(read_json_object(&file).unwrap(), got);
    }

    #[test]
    fn only_nests_own_copy_is_ever_removed() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        fs::create_dir_all(config.join(DRP_DIR)).unwrap();
        let mine = drp_copy(&config, "p-1_a");
        fs::write(&mine, "x").unwrap();
        // Bystanders a bad id could point at.
        let outside = dir.path().join("keep.drp");
        fs::write(&outside, "x").unwrap();
        fs::write(config.join("keep.drp"), "x").unwrap();
        for bad in ["../../keep", "../keep", "", "a/b", "a\\b", "..", "C:keep"] {
            assert_eq!(own_copy(&config, bad), None, "{bad}");
            remove_copy(&config, bad);
        }
        assert!(outside.exists() && config.join("keep.drp").exists());
        assert_eq!(own_copy(&config, "p-1_a"), Some(mine.clone()));
        remove_copy(&config, "p-1_a");
        assert!(!mine.exists());
        remove_copy(&config, "p-1_a"); // already gone: fine
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
        assert_eq!(fs::read_dir(config.join(DRP_DIR)).unwrap().count(), 1);
    }
}

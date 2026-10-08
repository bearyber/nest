//! `plan_project`: turns a template + field values into exactly what Create will make.
//! Pure: everything from outside comes in through `PlanContext`, nothing touches the disk
//! (build spec §2.2). The New Project preview renders this Plan.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value as Json};

use crate::names::{
    next_code, normalize_client_code, sanitize_segment, CodePattern, NameError, Transform,
    DEFAULT_CODE_PATTERN,
};
use crate::template::{
    parse_pattern, Field, FieldKind, LoadedTemplate, Piece, Template, ITEM, TODAY,
};

/// Windows path limits. MAX_PATH is 260 including the terminator, and folders
/// must leave room for an 8.3 file name inside them.
pub const MAX_FOLDER_PATH: usize = 247;
pub const MAX_FILE_PATH: usize = 259;
pub const MANIFEST_NAME: &str = ".project.json";
pub const MANIFEST_SCHEMA: u32 = 1;
/// Stand-in client code for the live preview before a valid one is typed.
const PLACEHOLDER_CODE: &str = "XX";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FieldValue {
    Bool(bool),
    List(Vec<String>),
    Client(ClientValue),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ClientValue {
    pub name: String,
    pub code: String,
}

pub type Values = BTreeMap<String, FieldValue>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Date {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

/// Everything `plan_project` needs from outside.
#[derive(Debug, Clone)]
pub struct PlanContext {
    pub jobs_root: PathBuf,
    /// New project id (UUID), written to the manifest.
    pub id: String,
    /// RFC 3339 timestamp, written to the manifest as-is.
    pub created_at: String,
    pub date: Date,
    pub space: String,
    pub code_pattern: String,
    pub marker: String,
    /// Job codes already in use (index + disk). The next seq counts up from these.
    pub existing_codes: Vec<String>,
    /// Folder names already in the jobs root.
    pub existing_folders: Vec<String>,
    /// Job code typed by the user instead of the generated one.
    pub code_override: Option<String>,
    /// A space without clients (e.g. Personal): the client field is ignored and job codes
    /// use this code instead, e.g. `OWN` → `OWN-L01`.
    pub own_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub job_code: String,
    pub folder_name: String,
    pub title: String,
    /// The project folder: jobs root + folder name.
    pub root: PathBuf,
    /// Folders to create, `/`-separated, relative to `root`, parents first.
    pub folders: Vec<String>,
    pub files: Vec<PlannedFile>,
    /// Contents of `.project.json`.
    pub manifest: Json,
    /// Values for `{variables}` inside filled starter files, as typed (no transforms).
    pub fill: BTreeMap<String, String>,
    pub issues: Vec<Issue>,
}

impl Plan {
    pub fn can_create(&self) -> bool {
        !self.issues.iter().any(|i| i.level == Level::Error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedFile {
    /// Relative to the template folder.
    pub from: String,
    /// Relative to the project folder.
    pub to: String,
    pub fill: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub level: Level,
    /// The field this is about, so the sheet can show it inline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Create is blocked.
    Error,
    /// Shown, but Create is still allowed.
    Warning,
}

#[derive(Default)]
struct Issues(Vec<Issue>);

impl Issues {
    fn error(&mut self, field: Option<&str>, message: String) {
        self.push(Level::Error, field, message);
    }
    fn warning(&mut self, message: String) {
        self.push(Level::Warning, None, message);
    }
    fn push(&mut self, level: Level, field: Option<&str>, message: String) {
        self.0.push(Issue {
            level,
            field: field.map(str::to_string),
            message,
        });
    }
}

pub fn plan_project(loaded: &LoadedTemplate, input: &Values, ctx: &PlanContext) -> Plan {
    let t = &loaded.template;
    let mut issues = Issues::default();
    let today = format!(
        "{:04}-{:02}-{:02}",
        ctx.date.year, ctx.date.month, ctx.date.day
    );
    let no_client = ctx.own_code.is_some();
    let values = resolve_values(t, input, &today, no_client, &mut issues);

    let client = match &ctx.own_code {
        // No-client space: the code is the user's own; the client name stays empty.
        Some(own) => match normalize_client_code(own) {
            Ok(code) => Some(ClientValue {
                name: String::new(),
                code,
            }),
            Err(e) => {
                issues.error(None, format!("Your own code in Settings: {e}"));
                None
            }
        },
        None => t
            .fields
            .iter()
            .find(|f| f.kind == FieldKind::Client)
            .and_then(|f| match values.get(&f.key) {
                Some(FieldValue::Client(c)) => Some(c.clone()),
                _ => None,
            }),
    };
    let client_code = client
        .as_ref()
        .map(|c| c.code.clone())
        .unwrap_or_else(|| PLACEHOLDER_CODE.to_string());

    let pattern = CodePattern::parse(&ctx.code_pattern).unwrap_or_else(|e| {
        issues.error(None, e);
        CodePattern::parse(DEFAULT_CODE_PATTERN).expect("default pattern is valid")
    });

    let vars = Vars {
        template: t,
        values: &values,
        builtins: builtins(t, ctx, &client_code),
        job_code: String::new(),
        item: None,
        all_optional: no_client,
    };
    let folder_pieces = parse_pattern(&t.folder_name).unwrap_or_default();
    let folder_name_for = |code: &str| {
        let with_code = vars.with_job_code(code);
        sanitize_segment(&with_code.render(&folder_pieces, Mode::Name))
    };

    // Only a folder name that actually changes with the code can be freed up by bumping it.
    // Judged from the output, not the pattern: a long name can push the code past the
    // 80-character cut, and then every code gives the same folder name.
    let name_has_code = {
        let a = folder_name_for(&pattern.format(&client_code, &ctx.marker, 1));
        let b = folder_name_for(&pattern.format(&client_code, &ctx.marker, 2));
        matches!((a, b), (Ok(a), Ok(b)) if a != b)
    };
    let taken_codes = lowercase_set(&ctx.existing_codes);
    let taken_folders = lowercase_set(&ctx.existing_folders);
    let is_taken = |code: &str| {
        taken_codes.contains(&code.to_lowercase())
            || (name_has_code
                && folder_name_for(code)
                    .is_ok_and(|name| taken_folders.contains(&name.to_lowercase())))
    };

    let job_code = match ctx.code_override.as_deref().map(str::trim) {
        Some(raw) if !raw.is_empty() => match sanitize_segment(raw) {
            Ok(code) => {
                if is_taken(&code) {
                    issues.error(
                        Some("jobCode"),
                        format!("Job code \"{code}\" is already used"),
                    );
                }
                code
            }
            Err(e) => {
                issues.error(Some("jobCode"), format!("Job code: {e}"));
                raw.to_string()
            }
        },
        _ => next_code(
            &pattern,
            &client_code,
            &ctx.marker,
            &ctx.existing_codes,
            is_taken,
        )
        .unwrap_or_else(|| {
            issues.error(
                Some("jobCode"),
                "Couldn't find a free job code. Type one in with Edit.".into(),
            );
            pattern.format(&client_code, &ctx.marker, 1)
        }),
    };

    let vars = vars.with_job_code(&job_code);
    let folder_name = match folder_name_for(&job_code) {
        Ok(name) => {
            if !name_has_code && taken_folders.contains(&name.to_lowercase()) {
                issues.error(
                    None,
                    format!("A folder named \"{name}\" already exists in the jobs folder. Change the name or the date."),
                );
            }
            name
        }
        Err(e) => {
            issues.error(None, format!("Folder name: {e}"));
            String::new()
        }
    };
    let root = ctx.jobs_root.join(&folder_name);

    let mut folders = FolderSet::default();
    plan_tree(t, &values, &vars, &mut folders, &mut issues);
    let files = plan_files(loaded, &vars, &mut folders, &mut issues);
    for file in &files {
        if folders.contains(&file.to) {
            issues.error(
                None,
                format!("\"{}\" would be both a folder and a file", file.to),
            );
        }
    }

    check_path_lengths(&root, &folders.list, &files, &mut issues);

    let title = project_title(t, &values, &vars, &job_code);
    let manifest = manifest(
        t,
        &values,
        ctx,
        client.as_ref(),
        &client_code,
        &job_code,
        &title,
    );
    let fill = t
        .fields
        .iter()
        .map(|f| f.key.as_str())
        .chain(crate::template::BUILTINS)
        .map(|name| (name.to_string(), vars.lookup(name, None, Mode::Text)))
        .collect();

    Plan {
        job_code,
        folder_name,
        title,
        root,
        folders: folders.list,
        files,
        manifest,
        fill,
        issues: issues.0,
    }
}

/// Replace `{name}` / `{name|transform}` in a starter file's text with `fill` values.
/// Unknown variables and stray braces are left untouched (spec §12a #11).
pub fn fill_text(text: &str, fill: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let replaced = after.find('}').and_then(|close| {
            let inner = &after[..close];
            let (name, transform) = match inner.split_once('|') {
                Some((n, t)) => (n, Some(Transform::parse(t)?)),
                None => (inner, None),
            };
            let value = fill.get(name)?;
            let value = transform.map_or_else(|| value.clone(), |t| t.apply(value));
            Some((value, close))
        });
        match replaced {
            Some((value, close)) => {
                out.push_str(&value);
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Apply defaults, trim, and check each field. Missing or invalid values are left out.
fn resolve_values(
    t: &Template,
    input: &Values,
    today: &str,
    no_client: bool,
    issues: &mut Issues,
) -> Values {
    let mut out = Values::new();
    for f in &t.fields {
        if no_client && f.kind == FieldKind::Client {
            continue; // not asked for, not required, not stored
        }
        let given = input.get(&f.key).cloned().or_else(|| {
            let default = f.default.as_ref()?;
            if f.kind == FieldKind::Date && default.as_str() == Some(TODAY) {
                return Some(FieldValue::Text(today.to_string()));
            }
            serde_json::from_value::<FieldValue>(default.clone()).ok()
        });
        match check_value(f, given) {
            Ok(Some(v)) => {
                out.insert(f.key.clone(), v);
            }
            // No client (personal or passion project): every question is optional.
            Ok(None) if f.required && !no_client => {
                issues.error(Some(&f.key), format!("{} is required", field_label(f)));
            }
            Ok(None) => {}
            Err(message) => issues.error(Some(&f.key), message),
        }
    }
    out
}

/// The client field is always called "Client" (who pays, never the artist), whatever label an
/// older or personal template gives it (e.g. "Billed to", used until 2026-10-08).
fn field_label(f: &Field) -> &str {
    if f.kind == FieldKind::Client {
        "Client"
    } else {
        &f.label
    }
}

fn check_value(f: &Field, given: Option<FieldValue>) -> Result<Option<FieldValue>, String> {
    let label = field_label(f);
    let wrong = || format!("{label} has the wrong kind of value");
    let Some(given) = given else {
        return Ok(None);
    };
    match f.kind {
        FieldKind::Text | FieldKind::Longtext => {
            let FieldValue::Text(s) = given else {
                return Err(wrong());
            };
            let s = s.trim().to_string();
            if let Some(max) = f.max {
                if s.chars().count() > max {
                    return Err(format!("{label} is too long (max {max} characters)"));
                }
            }
            Ok((!s.is_empty()).then_some(FieldValue::Text(s)))
        }
        FieldKind::Date => {
            let FieldValue::Text(s) = given else {
                return Err(wrong());
            };
            let s = s.trim().to_string();
            if s.is_empty() {
                return Ok(None);
            }
            if !is_iso_date(&s) {
                return Err(format!("{label} must be a date like 2026-09-28"));
            }
            Ok(Some(FieldValue::Text(s)))
        }
        FieldKind::Select => {
            let FieldValue::Text(s) = given else {
                return Err(wrong());
            };
            if s.is_empty() {
                return Ok(None);
            }
            if !f.options.contains(&s) {
                return Err(format!("{label} must be one of: {}", f.options.join(", ")));
            }
            Ok(Some(FieldValue::Text(s)))
        }
        FieldKind::Multi => {
            let FieldValue::List(items) = given else {
                return Err(wrong());
            };
            let mut seen = HashSet::new();
            let mut kept = Vec::new();
            for item in items {
                if !f.options.contains(&item) {
                    return Err(format!(
                        "{label}: \"{item}\" isn't one of the options ({})",
                        f.options.join(", ")
                    ));
                }
                if seen.insert(item.clone()) {
                    kept.push(item);
                }
            }
            Ok((!kept.is_empty()).then_some(FieldValue::List(kept)))
        }
        FieldKind::Bool => match given {
            FieldValue::Bool(b) => Ok(Some(FieldValue::Bool(b))),
            _ => Err(wrong()),
        },
        FieldKind::Client => {
            let FieldValue::Client(c) = given else {
                return Err(wrong());
            };
            let name = c.name.split_whitespace().collect::<Vec<_>>().join(" ");
            if name.is_empty() && c.code.trim().is_empty() {
                return Ok(None);
            }
            if name.is_empty() {
                return Err(format!("{label} needs a name"));
            }
            let code = normalize_client_code(&c.code)?;
            Ok(Some(FieldValue::Client(ClientValue { name, code })))
        }
    }
}

fn is_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
    if b.len() != 10
        || b[4] != b'-'
        || b[7] != b'-'
        || !digits(0..4)
        || !digits(5..7)
        || !digits(8..10)
    {
        return false;
    }
    let month: u8 = s[5..7].parse().unwrap_or(0);
    let day: u8 = s[8..10].parse().unwrap_or(0);
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

fn builtins(t: &Template, ctx: &PlanContext, client_code: &str) -> BTreeMap<&'static str, String> {
    let d = ctx.date;
    BTreeMap::from([
        ("clientCode", client_code.to_string()),
        ("date", format!("{:04}-{:02}-{:02}", d.year, d.month, d.day)),
        (
            "yymmdd",
            format!("{:02}{:02}{:02}", d.year % 100, d.month, d.day),
        ),
        ("yymm", format!("{:02}{:02}", d.year % 100, d.month)),
        ("year", format!("{:04}", d.year)),
        ("space", ctx.space.clone()),
        ("templateName", t.name.clone()),
    ])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Folder/file names: field values default to pascal case; `/` and `\` removed.
    Name,
    /// Titles and file contents: values as typed.
    Text,
}

#[derive(Clone)]
struct Vars<'a> {
    template: &'a Template,
    values: &'a Values,
    builtins: BTreeMap<&'static str, String>,
    job_code: String,
    item: Option<String>,
    /// No client: required questions are optional, so a missing one renders empty.
    all_optional: bool,
}

impl Vars<'_> {
    fn with_job_code(&self, code: &str) -> Self {
        Vars {
            job_code: code.to_string(),
            ..self.clone()
        }
    }

    fn with_item(&self, item: &str) -> Self {
        Vars {
            item: Some(item.to_string()),
            ..self.clone()
        }
    }

    fn render(&self, pieces: &[Piece], mode: Mode) -> String {
        let mut parts: Vec<Part> = pieces
            .iter()
            .map(|piece| match piece {
                Piece::Text(s) => Part::Text(s.clone()),
                Piece::Var { name, transform } => {
                    let value = self.lookup(name, *transform, mode);
                    let value = match mode {
                        Mode::Name => value.replace(['/', '\\'], ""),
                        Mode::Text => value,
                    };
                    Part::Value(value)
                }
            })
            .collect();
        tidy_gaps(&mut parts);
        parts
            .into_iter()
            .map(|p| match p {
                Part::Text(s) | Part::Value(s) => s,
            })
            .collect()
    }

    fn lookup(&self, name: &str, transform: Option<Transform>, mode: Mode) -> String {
        let apply = |s: &str, t: Option<Transform>| t.map_or_else(|| s.to_string(), |t| t.apply(s));
        if name == "jobCode" {
            return apply(&self.job_code, transform);
        }
        if let Some(b) = self.builtins.get(name) {
            return apply(b, transform);
        }
        // Field values (and {item}) get pascal case by default in names (spec §4, §12a).
        let transform = transform.or((mode == Mode::Name).then_some(Transform::Pascal));
        if name == ITEM {
            return self
                .item
                .as_deref()
                .map(|i| apply(i, transform))
                .unwrap_or_default();
        }
        let parts: Vec<String> = match self.values.get(name) {
            Some(FieldValue::Text(s)) => vec![s.clone()],
            Some(FieldValue::List(items)) => items.clone(),
            Some(FieldValue::Client(c)) => vec![c.name.clone()],
            Some(FieldValue::Bool(b)) => vec![if *b { "yes" } else { "no" }.to_string()],
            // A missing required value shows its label in the preview. Create is
            // blocked by the "is required" error, so the label never reaches disk.
            None => match self.template.field(name) {
                Some(f) if f.required && !self.all_optional => vec![f.label.clone()],
                _ => vec![],
            },
        };
        let joiner = if mode == Mode::Name { "-" } else { ", " };
        parts
            .iter()
            .map(|p| apply(p, transform))
            .collect::<Vec<_>>()
            .join(joiner)
    }
}

enum Part {
    /// Written in the template.
    Text(String),
    /// An answer or built-in, rendered.
    Value(String),
}

const GAP_CHARS: [char; 4] = ['_', '-', ' ', '.'];

/// An empty value takes the separator next to it with it, so `{start}_{artist}_{song}` with
/// no artist gives `261007_SONG`, not `261007__SONG`, and `{a} - {b}` gives `B`. Only
/// separators between an empty value and the rest are removed:
/// - something on both sides: the separator before it goes, if there's one after it too
///   (`A_{x}_B` → `A_B`; `_{x}{song}` keeps its `_`);
/// - nothing before it: the separator after it goes, but never a `.` (`{x}.txt` stays `.txt`);
/// - nothing after it: the separator before it goes.
fn tidy_gaps(parts: &mut [Part]) {
    let is_empty = |p: &Part| match p {
        Part::Text(s) | Part::Value(s) => s.is_empty(),
    };
    for i in 0..parts.len() {
        if !matches!(&parts[i], Part::Value(v) if v.is_empty()) {
            continue;
        }
        let left = (0..i).rev().find(|&j| !is_empty(&parts[j]));
        let right = (i + 1..parts.len()).find(|&j| !is_empty(&parts[j]));
        match (left, right) {
            (None, Some(r)) => {
                if let Part::Text(s) = &mut parts[r] {
                    *s = s.trim_start_matches(['_', '-', ' ']).to_string();
                }
            }
            (Some(l), None) => {
                if let Part::Text(s) = &mut parts[l] {
                    *s = s.trim_end_matches(GAP_CHARS).to_string();
                }
            }
            (Some(l), Some(r)) => {
                let starts_with_gap =
                    matches!(&parts[r], Part::Text(s) if s.starts_with(GAP_CHARS));
                if let (Part::Text(s), true) = (&mut parts[l], starts_with_gap) {
                    *s = s.trim_end_matches(GAP_CHARS).to_string();
                }
            }
            (None, None) => {}
        }
    }
}

/// Folders in creation order, deduped case-insensitively (Windows and default macOS
/// treat `Refs` and `REFS` as the same folder).
#[derive(Default)]
struct FolderSet {
    list: Vec<String>,
    seen: HashSet<String>,
}

impl FolderSet {
    /// Add a folder and all its parents.
    fn add(&mut self, segments: &[String]) {
        for end in 1..=segments.len() {
            let path = segments[..end].join("/");
            if self.seen.insert(path.to_lowercase()) {
                self.list.push(path);
            }
        }
    }

    fn contains(&self, path: &str) -> bool {
        self.seen.contains(&path.to_lowercase())
    }
}

fn lowercase_set(items: &[String]) -> HashSet<String> {
    items.iter().map(|s| s.to_lowercase()).collect()
}

/// Render a `/`-separated template path and sanitise each segment.
fn render_path(pattern: &str, vars: &Vars) -> Result<Vec<String>, NameError> {
    let pieces = parse_pattern(pattern).unwrap_or_default();
    vars.render(&pieces, Mode::Name)
        .split('/')
        .filter(|seg| !seg.is_empty())
        .map(sanitize_segment)
        .collect()
}

fn plan_tree(
    t: &Template,
    values: &Values,
    vars: &Vars,
    folders: &mut FolderSet,
    issues: &mut Issues,
) {
    for entry in &t.tree {
        if let Some(when) = entry.when() {
            let (negate, key) = match when.strip_prefix('!') {
                Some(key) => (true, key),
                None => (false, when),
            };
            let on = matches!(values.get(key), Some(FieldValue::Bool(true)));
            if on == negate {
                continue;
            }
        }
        let each_items: Vec<Option<&str>> = match entry.each() {
            Some(key) => match values.get(key) {
                Some(FieldValue::List(items)) => items.iter().map(|i| Some(i.as_str())).collect(),
                _ => vec![], // empty selection → folder not created (spec §4)
            },
            None => vec![None],
        };
        for item in each_items {
            let vars = item.map_or_else(|| vars.clone(), |i| vars.with_item(i));
            match render_path(entry.name(), &vars) {
                Ok(segments) if !segments.is_empty() => folders.add(&segments),
                Ok(_) => issues.error(None, format!("Folder \"{}\" comes out empty", entry.name())),
                Err(e) => issues.error(None, format!("Folder \"{}\": {e}", entry.name())),
            }
        }
    }
}

fn plan_files(
    loaded: &LoadedTemplate,
    vars: &Vars,
    folders: &mut FolderSet,
    issues: &mut Issues,
) -> Vec<PlannedFile> {
    let mut planned = Vec::new();
    let mut dests = HashSet::new();
    let mut add = |from: &str, to: Vec<String>, fill: bool, issues: &mut Issues| {
        let dest = to.join("/");
        if dest.eq_ignore_ascii_case(MANIFEST_NAME) {
            issues.error(
                None,
                format!("A starter file can't be called {MANIFEST_NAME}"),
            );
            return;
        }
        if !dests.insert(dest.to_lowercase()) {
            issues.error(
                None,
                format!("Two starter files would both be written to \"{dest}\""),
            );
            return;
        }
        folders.add(&to[..to.len() - 1]);
        planned.push(PlannedFile {
            from: from.to_string(),
            to: dest,
            fill,
        });
    };

    for file in &loaded.template.files {
        let to = match render_path(&file.to, vars) {
            Ok(segments) => segments,
            Err(e) => {
                issues.error(None, format!("Starter file \"{}\": {e}", file.to));
                continue;
            }
        };
        if file.from.ends_with('/') {
            let sources: Vec<&String> = loaded
                .files
                .iter()
                .filter(|p| p.starts_with(&file.from))
                .collect();
            if sources.is_empty() {
                issues.warning(format!(
                    "Starter folder \"{}\" is missing or empty, so it will be skipped",
                    file.from
                ));
            }
            for src in sources {
                let rest: Result<Vec<String>, NameError> = src[file.from.len()..]
                    .split('/')
                    .map(sanitize_segment)
                    .collect();
                match rest {
                    Ok(rest) => {
                        let mut dest = to.clone();
                        dest.extend(rest);
                        add(src, dest, file.fill, issues);
                    }
                    Err(e) => issues.error(None, format!("Starter file \"{src}\": {e}")),
                }
            }
        } else if to.is_empty() {
            issues.error(
                None,
                format!("Starter file \"{}\" has an empty destination", file.from),
            );
        } else if to.last().is_some_and(|n| n.starts_with('.'))
            && !file.to.rsplit('/').next().unwrap_or("").starts_with('.')
        {
            // e.g. "{song}.txt" with no song: ".txt" is a hidden file on a Mac.
            issues.error(
                None,
                format!(
                    "Starter file \"{}\" would have no name. Fill in the answer it's named after.",
                    file.to
                ),
            );
        } else if loaded.files.contains(&file.from) {
            add(&file.from, to, file.fill, issues);
        } else {
            issues.warning(format!(
                "Starter file \"{}\" is missing, so it will be skipped",
                file.from
            ));
        }
    }
    planned
}

/// Report the single longest path over the Windows limit, if any (spec §6).
fn check_path_lengths(
    root: &std::path::Path,
    folders: &[String],
    files: &[PlannedFile],
    issues: &mut Issues,
) {
    let root_len = root.to_string_lossy().encode_utf16().count();
    let len = |rel: &str| root_len + 1 + rel.encode_utf16().count();
    let mut worst: Option<(usize, usize, String)> = None; // (over by, length, path)
    let mut consider = |length: usize, limit: usize, path: String| {
        if length > limit && worst.as_ref().is_none_or(|w| length - limit > w.0) {
            worst = Some((length - limit, length, path));
        }
    };
    consider(root_len, MAX_FOLDER_PATH, String::new());
    consider(len(MANIFEST_NAME), MAX_FILE_PATH, MANIFEST_NAME.to_string());
    for f in folders {
        consider(len(f), MAX_FOLDER_PATH, f.clone());
    }
    for f in files {
        consider(len(&f.to), MAX_FILE_PATH, f.to.clone());
    }
    if let Some((_, length, path)) = worst {
        let shown = if path.is_empty() {
            "The project folder path".to_string()
        } else {
            format!("\"{path}\"")
        };
        issues.error(
            None,
            format!(
                "{shown} would be {length} characters long, over the Windows limit. Shorten the field values or pick a jobs folder closer to the drive root."
            ),
        );
    }
}

/// Template `title` pattern, else the text fields joined (§12a), else the job code.
fn project_title(t: &Template, values: &Values, vars: &Vars, job_code: &str) -> String {
    let raw = match &t.title {
        Some(pattern) => vars.render(&parse_pattern(pattern).unwrap_or_default(), Mode::Text),
        None => t
            .fields
            .iter()
            .filter(|f| f.kind == FieldKind::Text)
            .filter_map(|f| match values.get(&f.key) {
                Some(FieldValue::Text(s)) => Some(s.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" "),
    };
    let title = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() {
        job_code.to_string()
    } else {
        title
    }
}

/// `.project.json` contents (spec §12a). Field values go under `fields`, except the client.
fn manifest(
    t: &Template,
    values: &Values,
    ctx: &PlanContext,
    client: Option<&ClientValue>,
    client_code: &str,
    job_code: &str,
    title: &str,
) -> Json {
    let mut fields = Map::new();
    for f in t.fields.iter().filter(|f| f.kind != FieldKind::Client) {
        if let Some(v) = values.get(&f.key) {
            fields.insert(f.key.clone(), serde_json::to_value(v).unwrap_or(Json::Null));
        }
    }
    json!({
        "schema": MANIFEST_SCHEMA,
        "id": ctx.id,
        "jobCode": job_code,
        "title": title,
        "client": { "name": client.map(|c| c.name.as_str()).unwrap_or(""), "code": client_code },
        "space": ctx.space,
        "status": "active",
        "template": { "id": t.id, "version": t.version },
        "createdAt": ctx.created_at,
        "fields": fields,
        "links": {},
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    const GRADING_MV: &str = r#"{
        "schema": 1, "id": "grading-mv", "version": 1, "name": "Grading: MV",
        "title": "{artist} {song}",
        "fields": [
            { "key": "client", "label": "Client", "type": "client", "required": true },
            { "key": "artist", "label": "Artist", "type": "text", "required": true, "max": 60 },
            { "key": "song", "label": "Song", "type": "text", "required": true, "max": 60 },
            { "key": "ratios", "label": "Delivery ratios", "type": "multi",
              "options": ["16x9", "9x16", "1x1", "4x5"], "default": ["16x9"] },
            { "key": "lookdev", "label": "Include look dev", "type": "bool", "default": false }
        ],
        "folderName": "{yymm}_{jobCode}_{artist}_{song}",
        "tree": [
            "01_REF", "02_FOOTAGE", "03_PROJECT", "04_LUTS", "05_STILLS",
            { "name": "05_STILLS/LOOKDEV", "when": "lookdev" },
            { "name": "05_STILLS/FINAL", "when": "!lookdev" },
            "06_EXPORTS",
            { "name": "07_DELIVERY/{item}", "each": "ratios" }
        ],
        "files": [
            { "from": "starter/NOTES.md", "to": "NOTES.md", "fill": true },
            { "from": "starter/luts/", "to": "04_LUTS/" }
        ]
    }"#;

    fn loaded(json: &str, files: &[&str]) -> LoadedTemplate {
        LoadedTemplate {
            template: Template::from_json(json).unwrap(),
            dir: PathBuf::from("template"),
            files: files.iter().map(|s| s.to_string()).collect::<BTreeSet<_>>(),
        }
    }

    fn mv() -> LoadedTemplate {
        loaded(
            GRADING_MV,
            &[
                "starter/NOTES.md",
                "starter/luts/Rec709.cube",
                "starter/luts/Film/Kodak.cube",
            ],
        )
    }

    fn ctx() -> PlanContext {
        PlanContext {
            jobs_root: PathBuf::from("D:/Jobs"),
            id: "00000000-0000-4000-8000-000000000001".into(),
            created_at: "2026-09-28T10:00:00+08:00".into(),
            date: Date {
                year: 2026,
                month: 9,
                day: 28,
            },
            space: "Work".into(),
            code_pattern: DEFAULT_CODE_PATTERN.into(),
            marker: "L".into(),
            existing_codes: vec![],
            existing_folders: vec![],
            code_override: None,
            own_code: None,
        }
    }

    fn text(s: &str) -> FieldValue {
        FieldValue::Text(s.into())
    }

    fn values(artist: &str, song: &str) -> Values {
        Values::from([
            (
                "client".into(),
                FieldValue::Client(ClientValue {
                    name: "Vertex Studio".into(),
                    code: "vx".into(),
                }),
            ),
            ("artist".into(), text(artist)),
            ("song".into(), text(song)),
        ])
    }

    fn errors(plan: &Plan) -> Vec<&str> {
        plan.issues
            .iter()
            .filter(|i| i.level == Level::Error)
            .map(|i| i.message.as_str())
            .collect()
    }

    #[test]
    fn grading_mv_full_plan() {
        let mut v = values("KIRA", "Summer Nights");
        v.insert(
            "ratios".into(),
            FieldValue::List(vec!["16x9".into(), "9x16".into()]),
        );
        v.insert("lookdev".into(), FieldValue::Bool(true));
        let plan = plan_project(&mv(), &v, &ctx());

        assert!(plan.can_create(), "{:?}", plan.issues);
        assert_eq!(plan.job_code, "VX-L01");
        assert_eq!(plan.folder_name, "2609_VX-L01_KIRA_SummerNights");
        assert_eq!(plan.title, "KIRA Summer Nights");
        assert_eq!(
            plan.root,
            PathBuf::from("D:/Jobs").join("2609_VX-L01_KIRA_SummerNights")
        );
        assert_eq!(
            plan.folders,
            vec![
                "01_REF",
                "02_FOOTAGE",
                "03_PROJECT",
                "04_LUTS",
                "05_STILLS",
                "05_STILLS/LOOKDEV",
                "06_EXPORTS",
                "07_DELIVERY",
                "07_DELIVERY/16x9",
                "07_DELIVERY/9x16",
                "04_LUTS/Film",
            ]
        );
        let dests: Vec<&str> = plan.files.iter().map(|f| f.to.as_str()).collect();
        assert_eq!(
            dests,
            vec!["NOTES.md", "04_LUTS/Film/Kodak.cube", "04_LUTS/Rec709.cube"]
        );
        assert!(plan.files[0].fill);

        let m = &plan.manifest;
        assert_eq!(m["jobCode"], "VX-L01");
        assert_eq!(
            m["client"],
            json!({ "name": "Vertex Studio", "code": "VX" })
        );
        assert_eq!(m["status"], "active");
        assert_eq!(m["template"], json!({ "id": "grading-mv", "version": 1 }));
        assert_eq!(m["fields"]["ratios"], json!(["16x9", "9x16"]));
        assert_eq!(m["fields"]["lookdev"], json!(true));
        assert!(m["fields"].get("client").is_none());
        assert_eq!(m["links"], json!({}));
    }

    #[test]
    fn when_and_not_when() {
        let plan = plan_project(&mv(), &values("KIRA", "Song"), &ctx());
        assert!(!plan.folders.contains(&"05_STILLS/LOOKDEV".to_string()));
        assert!(plan.folders.contains(&"05_STILLS/FINAL".to_string()));
    }

    #[test]
    fn each_uses_default_and_empty_selection_creates_nothing() {
        let plan = plan_project(&mv(), &values("KIRA", "Song"), &ctx());
        assert!(plan.folders.contains(&"07_DELIVERY/16x9".to_string()));

        let mut v = values("KIRA", "Song");
        v.insert("ratios".into(), FieldValue::List(vec![]));
        let plan = plan_project(&mv(), &v, &ctx());
        assert!(!plan.folders.iter().any(|f| f.starts_with("07_DELIVERY")));
    }

    #[test]
    fn job_code_counts_past_gaps_and_folder_collisions() {
        let mut c = ctx();
        c.existing_codes = vec!["VX-L01".into(), "VX-L03".into()];
        let plan = plan_project(&mv(), &values("KIRA", "Song"), &c);
        assert_eq!(plan.job_code, "VX-L04");

        // A folder for L04 already exists (case differs) → bump to L05.
        c.existing_folders = vec!["2609_vx-l04_kira_song".into()];
        let plan = plan_project(&mv(), &values("KIRA", "Song"), &c);
        assert_eq!(plan.job_code, "VX-L05");
        assert_eq!(plan.folder_name, "2609_VX-L05_KIRA_Song");
    }

    #[test]
    fn code_override_must_be_free() {
        let mut c = ctx();
        c.existing_codes = vec!["VX-L02".into()];
        c.code_override = Some("vx-l02".into());
        let plan = plan_project(&mv(), &values("KIRA", "Song"), &c);
        assert!(!plan.can_create());
        assert!(errors(&plan)[0].contains("already used"));

        c.code_override = Some("VX-L09".into());
        let plan = plan_project(&mv(), &values("KIRA", "Song"), &c);
        assert!(plan.can_create());
        assert_eq!(plan.job_code, "VX-L09");
    }

    #[test]
    fn missing_required_fields_block_create_but_preview_shows_labels() {
        let v = Values::from([(
            "client".into(),
            FieldValue::Client(ClientValue {
                name: "Vertex Studio".into(),
                code: "VX".into(),
            }),
        )]);
        let plan = plan_project(&mv(), &v, &ctx());
        assert!(!plan.can_create());
        let fields: Vec<_> = plan
            .issues
            .iter()
            .filter_map(|i| i.field.as_deref())
            .collect();
        assert_eq!(fields, vec!["artist", "song"]);
        assert_eq!(plan.folder_name, "2609_VX-L01_Artist_Song");
    }

    #[test]
    fn bad_client_code_uses_placeholder() {
        let mut v = values("KIRA", "Song");
        v.insert(
            "client".into(),
            FieldValue::Client(ClientValue {
                name: "Vertex Studio".into(),
                code: "V".into(),
            }),
        );
        let plan = plan_project(&mv(), &v, &ctx());
        assert!(!plan.can_create());
        assert_eq!(plan.job_code, "XX-L01");
        assert!(errors(&plan)[0].contains("2 to 5"));
    }

    #[test]
    fn reserved_names_block_create() {
        let json = GRADING_MV.replace("\"01_REF\"", "\"{artist}\"");
        let plan = plan_project(&loaded(&json, &[]), &values("con", "Song"), &ctx());
        assert!(!plan.can_create());
        assert!(
            errors(&plan)
                .iter()
                .any(|e| e.contains("reserved name on Windows")),
            "{:?}",
            plan.issues
        );
    }

    #[test]
    fn slashes_in_values_never_create_subfolders() {
        let plan = plan_project(&mv(), &values("AC/DC", "Back\\In Black"), &ctx());
        assert!(plan.can_create());
        assert_eq!(plan.folder_name, "2609_VX-L01_ACDC_BackInBlack");
        let json = GRADING_MV.replace("\"01_REF\"", "\"01_REF/{artist|upper}\"");
        let plan = plan_project(&loaded(&json, &[]), &values("AC/DC", "x"), &ctx());
        assert!(
            plan.folders.contains(&"01_REF/ACDC".to_string()),
            "{:?}",
            plan.folders
        );
    }

    #[test]
    fn korean_and_chinese_values() {
        let plan = plan_project(&mv(), &values("아이유", "좋은 날"), &ctx());
        assert!(plan.can_create());
        assert_eq!(plan.folder_name, "2609_VX-L01_아이유_좋은날");
        assert_eq!(plan.title, "아이유 좋은 날");
        let plan = plan_project(&mv(), &values("周杰倫", "晴天"), &ctx());
        assert_eq!(plan.folder_name, "2609_VX-L01_周杰倫_晴天");
    }

    #[test]
    fn long_paths_are_flagged_before_create() {
        let mut c = ctx();
        c.jobs_root = PathBuf::from(format!("D:/{}", "x".repeat(220)));
        let plan = plan_project(&mv(), &values("KIRA", "Summer Nights"), &c);
        assert!(!plan.can_create());
        let long: Vec<_> = errors(&plan)
            .into_iter()
            .filter(|e| e.contains("Windows limit"))
            .collect();
        assert_eq!(long.len(), 1, "only the worst path is reported");
        // Folders have the stricter limit (247), so 07_DELIVERY/16x9 (266) is further
        // over than the deepest file (273 vs 259).
        assert!(long[0].contains("07_DELIVERY/16x9"), "{long:?}");
    }

    #[test]
    fn too_long_field_value_is_an_error() {
        let plan = plan_project(&mv(), &values(&"a".repeat(61), "Song"), &ctx());
        assert!(errors(&plan).iter().any(|e| e.contains("too long (max 60")));
    }

    #[test]
    fn missing_starter_file_is_a_warning_not_an_error() {
        let plan = plan_project(&loaded(GRADING_MV, &[]), &values("KIRA", "Song"), &ctx());
        assert!(plan.can_create());
        assert!(plan.files.is_empty());
        let warnings: Vec<_> = plan
            .issues
            .iter()
            .filter(|i| i.level == Level::Warning)
            .collect();
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn case_only_duplicate_folders_are_merged() {
        let json = GRADING_MV.replace("\"02_FOOTAGE\"", "\"01_ref\"");
        let plan = plan_project(&loaded(&json, &[]), &values("KIRA", "Song"), &ctx());
        assert_eq!(
            plan.folders
                .iter()
                .filter(|f| f.eq_ignore_ascii_case("01_REF"))
                .count(),
            1
        );
    }

    #[test]
    fn file_and_folder_on_same_path_is_an_error() {
        let json = GRADING_MV.replace("\"01_REF\"", "\"notes.md\"");
        let plan = plan_project(&mv_with(&json), &values("KIRA", "Song"), &ctx());
        assert!(errors(&plan)
            .iter()
            .any(|e| e.contains("both a folder and a file")));
    }

    fn mv_with(json: &str) -> LoadedTemplate {
        loaded(json, &["starter/NOTES.md"])
    }

    #[test]
    fn select_and_date_are_checked() {
        let json = GRADING_MV.replace(
            r#"{ "key": "lookdev""#,
            r#"{ "key": "fps", "label": "Frame rate", "type": "select", "options": ["24", "25"] },
               { "key": "due", "label": "Due", "type": "date" },
               { "key": "lookdev""#,
        );
        let mut v = values("KIRA", "Song");
        v.insert("fps".into(), text("30"));
        v.insert("due".into(), text("28/09/2026"));
        let plan = plan_project(&loaded(&json, &[]), &v, &ctx());
        let errs = errors(&plan);
        assert!(
            errs.iter().any(|e| e.contains("must be one of: 24, 25")),
            "{errs:?}"
        );
        assert!(errs.iter().any(|e| e.contains("date like")), "{errs:?}");
    }

    #[test]
    fn default_title_joins_text_fields() {
        let json = GRADING_MV.replace(r#""title": "{artist} {song}","#, "");
        let plan = plan_project(
            &loaded(&json, &[]),
            &values("KIRA", "Summer Nights"),
            &ctx(),
        );
        assert_eq!(plan.title, "KIRA Summer Nights");
    }

    #[test]
    fn fill_values_and_fill_text() {
        let plan = plan_project(&mv(), &values("KIRA", "Summer Nights"), &ctx());
        assert_eq!(plan.fill["jobCode"], "VX-L01");
        assert_eq!(plan.fill["song"], "Summer Nights", "no pascal inside files");
        assert_eq!(plan.fill["client"], "Vertex Studio");
        assert_eq!(plan.fill["ratios"], "16x9");
        let text = "# {jobCode}\n{song|upper} for {client}\n{unknown} {\"json\": 1} {";
        assert_eq!(
            fill_text(text, &plan.fill),
            "# VX-L01\nSUMMER NIGHTS for Vertex Studio\n{unknown} {\"json\": 1} {"
        );
    }

    /// Bernard's convention: YYMMDD_NAME, date from a start-date field, no job code in the name.
    const DATED: &str = r#"{
        "schema": 1, "id": "project", "version": 1, "name": "Project",
        "title": "{name}",
        "fields": [
            { "key": "client", "label": "Client", "type": "client", "required": true },
            { "key": "name", "label": "Name", "type": "text", "required": true },
            { "key": "start", "label": "Start date", "type": "date", "required": true, "default": "today" }
        ],
        "folderName": "{start|yymmdd}_{name|caps}",
        "tree": ["00_REFERENCE/BRIEF", "06_DI/FOR_DI/YYMMDD", "07_VFX/INT/(SCENE_NAME_TEMPLATE)"]
    }"#;

    fn dated(name: &str, start: Option<&str>) -> Values {
        let mut v = values("x", "y");
        v.remove("artist");
        v.remove("song");
        v.insert("name".into(), text(name));
        if let Some(s) = start {
            v.insert("start".into(), text(s));
        }
        v
    }

    #[test]
    fn dated_caps_folder_name_without_job_code() {
        let t = loaded(DATED, &[]);
        let plan = plan_project(&t, &dated("ClientX brand film", Some("2025-06-08")), &ctx());
        assert!(plan.can_create(), "{:?}", plan.issues);
        assert_eq!(plan.folder_name, "250608_CLIENTX_BRAND_FILM");
        assert_eq!(
            plan.job_code, "VX-L01",
            "code still allocated, just not in the name"
        );
        assert_eq!(plan.title, "ClientX brand film");
        assert!(plan
            .folders
            .contains(&"07_VFX/INT/(SCENE_NAME_TEMPLATE)".to_string()));
        assert!(plan.folders.contains(&"06_DI/FOR_DI/YYMMDD".to_string()));

        // Start date defaults to today (ctx date 2026-09-28).
        let plan = plan_project(&t, &dated("Brand Film", None), &ctx());
        assert_eq!(plan.folder_name, "260928_BRAND_FILM");
        assert_eq!(plan.manifest["fields"]["start"], "2026-09-28");
        assert_eq!(plan.fill["yymmdd"], "260928");
    }

    #[test]
    fn existing_folder_without_job_code_is_an_error_not_an_endless_bump() {
        let t = loaded(DATED, &[]);
        let mut c = ctx();
        c.existing_folders = vec!["250608_clientx_brand_film".into()];
        let plan = plan_project(&t, &dated("ClientX brand film", Some("2025-06-08")), &c);
        assert!(!plan.can_create());
        assert!(
            errors(&plan)[0].contains("already exists"),
            "{:?}",
            plan.issues
        );
        assert_eq!(plan.job_code, "VX-L01");
    }

    #[test]
    fn long_name_that_cuts_off_the_code_does_not_search_forever() {
        // The 80-char cut removes {jobCode}, so every code gives the same folder name.
        let json = GRADING_MV.replace(
            r#""folderName": "{yymm}_{jobCode}_{artist}_{song}""#,
            r#""folderName": "{song}_{song}_{jobCode}""#,
        );
        let t = loaded(&json, &[]);
        let long_song = "a".repeat(60);
        let first = plan_project(&t, &values("KIRA", &long_song), &ctx());
        assert_eq!(first.folder_name.chars().count(), 80);

        let mut c = ctx();
        c.existing_codes = vec![first.job_code.clone()];
        c.existing_folders = vec![first.folder_name.clone()];
        let second = plan_project(&t, &values("KIRA", &long_song), &c);
        assert!(!second.can_create());
        assert!(
            errors(&second).iter().any(|e| e.contains("already exists")),
            "{:?}",
            second.issues
        );
    }

    #[test]
    fn personal_space_needs_no_client_and_uses_own_code() {
        let mut c = ctx();
        c.space = "Personal".into();
        c.own_code = Some("own".into());
        c.existing_codes = vec!["OWN-L01".into()];
        let mut v = values("KIRA", "Song");
        v.remove("client"); // not asked for in a no-client space
        let plan = plan_project(&mv(), &v, &c);
        assert!(plan.can_create(), "{:?}", plan.issues);
        assert_eq!(plan.job_code, "OWN-L02");
        assert_eq!(
            plan.manifest["client"],
            json!({ "name": "", "code": "OWN" })
        );

        // A client typed earlier (e.g. before switching space) is ignored, not stored.
        let plan = plan_project(&mv(), &values("KIRA", "Song"), &c);
        assert_eq!(plan.job_code, "OWN-L02");
        assert_eq!(plan.manifest["client"]["name"], "");

        // A bad own code is reported, not silently used.
        c.own_code = Some("X".into());
        let plan = plan_project(&mv(), &v, &c);
        assert!(!plan.can_create());
    }

    #[test]
    fn no_client_makes_every_question_optional_and_tidies_the_name() {
        let mut c = ctx();
        c.own_code = Some("OWN".into());
        let json = GRADING_MV.replace(
            r#""folderName": "{yymm}_{jobCode}_{artist}_{song}""#,
            r#""folderName": "{yymmdd}_{artist|caps}_{song|caps}""#,
        );
        let t = loaded(&json, &["starter/NOTES.md"]);

        // No artist: no "Artist is required", no "__" gap, title without the label.
        let v = Values::from([("song".into(), text("My Passion Project"))]);
        let plan = plan_project(&t, &v, &c);
        assert!(plan.can_create(), "{:?}", plan.issues);
        assert_eq!(plan.folder_name, "260928_MY_PASSION_PROJECT");
        assert_eq!(plan.job_code, "OWN-L01");
        assert_eq!(plan.title, "My Passion Project");

        // Korean still fine, and an empty first part takes the separator after it.
        let v = Values::from([("artist".into(), text("아이유"))]);
        let plan = plan_project(&t, &v, &c);
        assert!(plan.can_create(), "{:?}", plan.issues);
        assert_eq!(plan.folder_name, "260928_아이유");

        // Nothing filled in: just the date.
        let plan = plan_project(&t, &Values::new(), &c);
        assert!(plan.can_create(), "{:?}", plan.issues);
        assert_eq!(plan.folder_name, "260928");

        // With a client the questions are required again.
        let plan = plan_project(&t, &Values::new(), &ctx());
        assert!(errors(&plan).contains(&"Artist is required"));
    }

    #[test]
    fn tidy_gaps_only_touches_separators_next_to_empty_values() {
        let pieces = |p: &str| parse_pattern(p).unwrap();
        let t = loaded(GRADING_MV, &[]);
        let v = Values::from([("song".into(), text("Song"))]);
        let vars = Vars {
            template: &t.template,
            values: &v,
            builtins: BTreeMap::new(),
            job_code: String::new(),
            item: None,
            all_optional: true,
        };
        let r = |p: &str| vars.render(&pieces(p), Mode::Name);
        assert_eq!(r("{artist}_{song}"), "Song");
        assert_eq!(r("{song}_{artist}"), "Song");
        assert_eq!(r("A__{song}"), "A__Song", "template's own __ stays");
        assert_eq!(r("A_{artist}_B"), "A_B");
        assert_eq!(r("{artist}{song}"), "Song");
        assert_eq!(r("X/{artist}"), "X/", "/ is not a gap");
        assert_eq!(r("05_FINAL/{artist}"), "05_FINAL/");
        // Review W2: side by side with a filled value, the separator before stays.
        assert_eq!(r("X_{artist}{song}"), "X_Song");
        assert_eq!(r("{song}{artist}_X"), "Song_X");
        // Review W1: a leading dot is never taken.
        assert_eq!(r("{artist}.txt"), ".txt");
        assert_eq!(r("{song}.{artist}"), "Song");
        // Longer separators go as a whole; two empties in a row.
        assert_eq!(r("D - {artist} - {song}"), "D - Song");
        assert_eq!(r("{artist} - {song}"), "Song");
        assert_eq!(r("{artist}_{artist}_{song}"), "Song");
        assert_eq!(r("{song}_{artist}_{artist}"), "Song");
    }

    #[test]
    fn starter_file_named_only_after_an_empty_answer_is_an_error() {
        let mut c = ctx();
        c.own_code = Some("OWN".into());
        let json = GRADING_MV.replace(r#""to": "NOTES.md""#, r#""to": "{artist}.md""#);
        let t = loaded(&json, &["starter/NOTES.md"]);
        let v = Values::from([("song".into(), text("Song"))]);
        let plan = plan_project(&t, &v, &c);
        assert!(!plan.can_create());
        assert!(
            errors(&plan)
                .iter()
                .any(|e| e.contains("would have no name")),
            "{:?}",
            plan.issues
        );
        let v = Values::from([("artist".into(), text("KIRA"))]);
        assert!(plan_project(&t, &v, &c).can_create());
    }

    #[test]
    fn client_field_is_called_client_in_messages() {
        // Older templates label it "Billed to"; messages always say "Client".
        let json = GRADING_MV.replace(r#""label": "Client""#, r#""label": "Billed to""#);
        let t = loaded(&json, &["starter/NOTES.md"]);
        let mut v = values("KIRA", "Song");
        v.remove("client");
        let plan = plan_project(&t, &v, &ctx());
        assert!(
            errors(&plan).contains(&"Client is required"),
            "{:?}",
            plan.issues
        );
        v.insert(
            "client".into(),
            FieldValue::Client(ClientValue {
                name: "Vertex Studio".into(),
                code: "V".into(),
            }),
        );
        let plan = plan_project(&mv(), &v, &ctx());
        assert!(errors(&plan)[0].contains("2 to 5"), "{:?}", plan.issues);
    }

    #[test]
    fn plan_is_deterministic() {
        let a = plan_project(&mv(), &values("KIRA", "Song"), &ctx());
        let b = plan_project(&mv(), &values("KIRA", "Song"), &ctx());
        assert_eq!(a, b);
    }
}

//! Template files (`*.nest.json`): types, loading and validation (build spec §4, §12a).

use std::collections::{BTreeSet, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::names::{Transform, TRANSFORM_NAMES};

/// A date field default meaning "the day the project is created".
pub const TODAY: &str = "today";

pub const TEMPLATE_SCHEMA: u32 = 1;
pub const TEMPLATE_SUFFIX: &str = ".nest.json";
/// Variables every template can use besides its own field keys.
pub const BUILTINS: [&str; 8] = [
    "jobCode",
    "clientCode",
    "date",
    "yymmdd",
    "yymm",
    "year",
    "space",
    "templateName",
];
/// Only valid inside a tree entry with `each`.
pub const ITEM: &str = "item";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Template {
    pub schema: u32,
    pub id: String,
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Pattern for the project title. Defaults to the text fields joined (§12a).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub fields: Vec<Field>,
    pub folder_name: String,
    #[serde(default)]
    pub tree: Vec<TreeEntry>,
    #[serde(default)]
    pub files: Vec<FileEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Field {
    pub key: String,
    pub label: String,
    #[serde(rename = "type")]
    pub kind: FieldKind,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldKind {
    Text,
    Longtext,
    Bool,
    Select,
    Multi,
    Date,
    Client,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TreeEntry {
    Folder(String),
    Rule(TreeRule),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeRule {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub each: Option<String>,
}

impl TreeEntry {
    pub fn name(&self) -> &str {
        match self {
            TreeEntry::Folder(name) => name,
            TreeEntry::Rule(rule) => &rule.name,
        }
    }

    pub fn when(&self) -> Option<&str> {
        match self {
            TreeEntry::Folder(_) => None,
            TreeEntry::Rule(rule) => rule.when.as_deref(),
        }
    }

    pub fn each(&self) -> Option<&str> {
        match self {
            TreeEntry::Folder(_) => None,
            TreeEntry::Rule(rule) => rule.each.as_deref(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileEntry {
    /// Relative to the template folder. A trailing `/` copies a whole folder.
    pub from: String,
    /// Relative to the project folder. May use variables.
    pub to: String,
    /// Replace variables inside the file (UTF-8 text only).
    #[serde(default)]
    pub fill: bool,
}

/// A piece of a `{variable|transform}` pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    Text(String),
    Var {
        name: String,
        transform: Option<Transform>,
    },
}

pub fn parse_pattern(s: &str) -> Result<Vec<Piece>, String> {
    let mut pieces = Vec::new();
    let mut rest = s;
    while let Some(open) = rest.find(['{', '}']) {
        if rest[open..].starts_with('}') {
            return Err(format!("\"{s}\" has a }} without a matching {{"));
        }
        if open > 0 {
            pieces.push(Piece::Text(rest[..open].to_string()));
        }
        let after = &rest[open + 1..];
        let close = after
            .find('}')
            .ok_or_else(|| format!("\"{s}\" has a {{ that is never closed"))?;
        let inner = &after[..close];
        if inner.contains('{') {
            return Err(format!("\"{s}\" has a {{ that is never closed"));
        }
        let (name, transform) = match inner.split_once('|') {
            Some((name, t)) => {
                let transform = Transform::parse(t).ok_or_else(|| {
                    format!("\"{s}\" uses an unknown transform \"{t}\" (use {TRANSFORM_NAMES})")
                })?;
                (name, Some(transform))
            }
            None => (inner, None),
        };
        if name.is_empty() {
            return Err(format!("\"{s}\" has an empty {{}}"));
        }
        pieces.push(Piece::Var {
            name: name.to_string(),
            transform,
        });
        rest = &after[close + 1..];
    }
    if !rest.is_empty() {
        pieces.push(Piece::Text(rest.to_string()));
    }
    Ok(pieces)
}

/// Why a template can't be used. Messages are plain English for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateError {
    pub problems: Vec<String>,
}

impl TemplateError {
    fn one(problem: String) -> Self {
        TemplateError {
            problems: vec![problem],
        }
    }
}

impl fmt::Display for TemplateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.problems.as_slice() {
            [one] => write!(f, "This template can't be used: {one}"),
            many => {
                write!(f, "This template can't be used:")?;
                for p in many {
                    write!(f, "\n- {p}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for TemplateError {}

impl Template {
    pub fn from_json(text: &str) -> Result<Self, TemplateError> {
        let template: Template = serde_json::from_str(text)
            .map_err(|e| TemplateError::one(format!("the file isn't a valid template ({e})")))?;
        template.validate()?;
        Ok(template)
    }

    pub fn field(&self, key: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.key == key)
    }

    pub fn validate(&self) -> Result<(), TemplateError> {
        let mut problems = Vec::new();
        let p = &mut problems;

        if self.schema != TEMPLATE_SCHEMA {
            p.push(format!(
                "it was made for a different version of Nest (schema {})",
                self.schema
            ));
        }
        let id_ok = !self.id.is_empty()
            && self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !id_ok {
            p.push(format!(
                "its id \"{}\" must use only lowercase letters, digits and -",
                self.id
            ));
        }
        if self.name.trim().is_empty() {
            p.push("it has no name".to_string());
        }

        self.validate_fields(p);

        if let Some(title) = &self.title {
            self.check_pattern(title, false, p);
        }
        self.check_path("folderName", &self.folder_name, false, p);
        if self.folder_name.contains('/') {
            p.push(format!(
                "folderName \"{}\" can't contain /",
                self.folder_name
            ));
        }

        for entry in &self.tree {
            let name = entry.name();
            if let Some(when) = entry.when() {
                let key = when.strip_prefix('!').unwrap_or(when);
                match self.field(key).map(|f| f.kind) {
                    Some(FieldKind::Bool) => {}
                    Some(_) => p.push(format!(
                        "\"when\" on folder \"{name}\" points at \"{key}\", which isn't a yes/no field"
                    )),
                    None => p.push(format!(
                        "\"when\" on folder \"{name}\" points at \"{key}\", which isn't a field"
                    )),
                }
            }
            if let Some(each) = entry.each() {
                match self.field(each).map(|f| f.kind) {
                    Some(FieldKind::Multi) => {}
                    Some(_) => p.push(format!(
                        "\"each\" on folder \"{name}\" points at \"{each}\", which isn't a multi-choice field"
                    )),
                    None => p.push(format!(
                        "\"each\" on folder \"{name}\" points at \"{each}\", which isn't a field"
                    )),
                }
            }
            self.check_path("a folder", name, entry.each().is_some(), p);
        }

        for file in &self.files {
            self.check_path("a starter file", &file.from, false, p);
            if file.from.contains(['{', '}']) {
                p.push(format!(
                    "starter file source \"{}\" can't use variables",
                    file.from
                ));
            }
            self.check_path("a starter file", &file.to, false, p);
            if file.from.ends_with('/') != file.to.ends_with('/') {
                p.push(format!(
                    "starter \"{}\" → \"{}\": both must end with / to copy a folder, or neither",
                    file.from, file.to
                ));
            }
        }

        if problems.is_empty() {
            Ok(())
        } else {
            Err(TemplateError { problems })
        }
    }

    fn validate_fields(&self, p: &mut Vec<String>) {
        let mut keys = HashSet::new();
        for f in &self.fields {
            let key_ok =
                !f.key.is_empty() && f.key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            if !key_ok {
                p.push(format!(
                    "field key \"{}\" must use only letters, digits and _",
                    f.key
                ));
            }
            if BUILTINS.contains(&f.key.as_str()) || f.key == ITEM {
                p.push(format!(
                    "field key \"{}\" clashes with a built-in variable",
                    f.key
                ));
            }
            if !keys.insert(f.key.as_str()) {
                p.push(format!("field key \"{}\" is used twice", f.key));
            }
            let has_options = matches!(f.kind, FieldKind::Select | FieldKind::Multi);
            if has_options && f.options.is_empty() {
                p.push(format!("field \"{}\" needs a list of options", f.key));
            }
            if let Some(default) = &f.default {
                if !default_fits(f, default) {
                    p.push(format!(
                        "field \"{}\" has a default that doesn't fit its type or options",
                        f.key
                    ));
                }
            }
        }
        let clients = self
            .fields
            .iter()
            .filter(|f| f.kind == FieldKind::Client)
            .count();
        if clients != 1 {
            p.push(
                "it needs exactly one field of type \"client\" (the job code comes from the client code)"
                    .to_string(),
            );
        }
    }

    fn check_path(&self, what: &str, s: &str, allow_item: bool, p: &mut Vec<String>) {
        if s.trim().is_empty() {
            p.push(format!("{what} has an empty path"));
            return;
        }
        if s.contains('\\') {
            p.push(format!("\"{s}\" uses \\; template paths always use /"));
        }
        let drive = s.len() >= 2 && s.as_bytes()[1] == b':';
        if s.starts_with('/') || drive {
            p.push(format!(
                "\"{s}\" is an absolute path; paths must be relative"
            ));
        }
        if s.split('/').any(|seg| seg.trim() == "..") {
            p.push(format!("\"{s}\" uses .., which isn't allowed"));
        }
        self.check_pattern(s, allow_item, p);
    }

    fn check_pattern(&self, s: &str, allow_item: bool, p: &mut Vec<String>) {
        let pieces = match parse_pattern(s) {
            Ok(pieces) => pieces,
            Err(e) => {
                p.push(e);
                return;
            }
        };
        for piece in pieces {
            let Piece::Var { name, .. } = piece else {
                continue;
            };
            let known = self.field(&name).is_some() || BUILTINS.contains(&name.as_str());
            if known || (allow_item && name == ITEM) {
                continue;
            }
            if name == ITEM {
                p.push(format!(
                    "{{item}} only works in a folder with \"each\" (in \"{s}\")"
                ));
            } else {
                p.push(format!("unknown variable {{{name}}} in \"{s}\""));
            }
        }
    }
}

fn default_fits(field: &Field, default: &serde_json::Value) -> bool {
    use serde_json::Value;
    match (field.kind, default) {
        (FieldKind::Bool, Value::Bool(_)) => true,
        (FieldKind::Text | FieldKind::Longtext | FieldKind::Date, Value::String(_)) => true,
        (FieldKind::Select, Value::String(s)) => field.options.contains(s),
        (FieldKind::Multi, Value::Array(items)) => items.iter().all(|v| {
            v.as_str()
                .is_some_and(|s| field.options.iter().any(|o| o == s))
        }),
        _ => false,
    }
}

/// A template plus the starter files available next to it.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedTemplate {
    pub template: Template,
    pub dir: PathBuf,
    /// Every file in the template folder (except the `.nest.json`),
    /// `/`-separated and relative to `dir`.
    pub files: BTreeSet<String>,
}

/// Load a template folder: exactly one `*.nest.json` plus any starter files.
pub fn load_dir(dir: &Path) -> Result<LoadedTemplate, TemplateError> {
    let read_err =
        |e: std::io::Error| TemplateError::one(format!("can't read {}: {e}", dir.display()));
    let mut json: Option<PathBuf> = None;
    for entry in fs::read_dir(dir).map_err(read_err)? {
        let path = entry.map_err(read_err)?.path();
        let is_template = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(TEMPLATE_SUFFIX));
        if is_template && path.is_file() {
            if json.is_some() {
                return Err(TemplateError::one(format!(
                    "{} has more than one {TEMPLATE_SUFFIX} file",
                    dir.display()
                )));
            }
            json = Some(path);
        }
    }
    let json = json.ok_or_else(|| {
        TemplateError::one(format!("{} has no {TEMPLATE_SUFFIX} file", dir.display()))
    })?;
    let text = fs::read_to_string(&json).map_err(read_err)?;
    let template = Template::from_json(&text)?;

    let mut files = BTreeSet::new();
    collect_files(dir, dir, &json, &mut files).map_err(read_err)?;
    Ok(LoadedTemplate {
        template,
        dir: dir.to_path_buf(),
        files,
    })
}

fn collect_files(
    root: &Path,
    dir: &Path,
    skip: &Path,
    out: &mut BTreeSet<String>,
) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            collect_files(root, &path, skip, out)?;
        } else if kind.is_file() && path != skip {
            if let Ok(rel) = path.strip_prefix(root) {
                let parts: Vec<String> = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                out.insert(parts.join("/"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"{
        "schema": 1, "id": "grading-mv", "version": 1, "name": "Grading: MV",
        "fields": [
            { "key": "client", "label": "Client", "type": "client", "required": true },
            { "key": "artist", "label": "Artist", "type": "text", "required": true, "max": 60 },
            { "key": "ratios", "label": "Ratios", "type": "multi", "options": ["16x9", "9x16"], "default": ["16x9"] },
            { "key": "lookdev", "label": "Look dev", "type": "bool", "default": false }
        ],
        "folderName": "{yymm}_{jobCode}_{artist}",
        "tree": [
            "01_REF",
            { "name": "05_STILLS/LOOKDEV", "when": "lookdev" },
            { "name": "07_DELIVERY/{item}", "each": "ratios" }
        ],
        "files": [{ "from": "starter/NOTES.md", "to": "NOTES.md", "fill": true }]
    }"#;

    /// Parse VALID with one JSON edit applied, and return the problems.
    fn problems_with(edit: impl FnOnce(&mut serde_json::Value)) -> Vec<String> {
        let mut v: serde_json::Value = serde_json::from_str(VALID).unwrap();
        edit(&mut v);
        match Template::from_json(&v.to_string()) {
            Ok(_) => vec![],
            Err(e) => e.problems,
        }
    }

    fn assert_refused(problems: Vec<String>, expected: &str) {
        assert!(
            problems.iter().any(|p| p.contains(expected)),
            "expected a problem containing {expected:?}, got {problems:?}"
        );
    }

    #[test]
    fn valid_template_loads() {
        let t = Template::from_json(VALID).unwrap();
        assert_eq!(t.fields.len(), 4);
        assert_eq!(t.tree[2].each(), Some("ratios"));
    }

    #[test]
    fn refuses_unknown_field_type() {
        let p = problems_with(|v| v["fields"][1]["type"] = "colour".into());
        assert_refused(p, "unknown variant `colour`");
    }

    #[test]
    fn refuses_unknown_keys() {
        let p = problems_with(|v| v["folderNmae"] = "x".into());
        assert_refused(p, "unknown field `folderNmae`");
    }

    #[test]
    fn refuses_bad_when_and_each() {
        let p = problems_with(|v| v["tree"][1]["when"] = "missing".into());
        assert_refused(p, "which isn't a field");
        let p = problems_with(|v| v["tree"][1]["when"] = "!artist".into());
        assert_refused(p, "isn't a yes/no field");
        let p = problems_with(|v| v["tree"][2]["each"] = "lookdev".into());
        assert_refused(p, "isn't a multi-choice field");
    }

    #[test]
    fn negated_when_is_fine() {
        assert!(problems_with(|v| v["tree"][1]["when"] = "!lookdev".into()).is_empty());
    }

    #[test]
    fn refuses_unsafe_paths() {
        let p = problems_with(|v| v["tree"][0] = "../escape".into());
        assert_refused(p, "uses ..");
        let p = problems_with(|v| v["tree"][0] = "/abs".into());
        assert_refused(p, "absolute path");
        let p = problems_with(|v| v["files"][0]["to"] = "C:/x.md".into());
        assert_refused(p, "absolute path");
        let p = problems_with(|v| v["tree"][0] = "01\\REF".into());
        assert_refused(p, "always use /");
        let p = problems_with(|v| v["folderName"] = "a/{jobCode}".into());
        assert_refused(p, "can't contain /");
    }

    #[test]
    fn refuses_duplicate_and_clashing_keys() {
        let p = problems_with(|v| v["fields"][2]["key"] = "artist".into());
        assert_refused(p, "is used twice");
        let p = problems_with(|v| v["fields"][1]["key"] = "jobCode".into());
        assert_refused(p, "clashes with a built-in");
    }

    #[test]
    fn refuses_unknown_variables_and_stray_item() {
        let p = problems_with(|v| v["folderName"] = "{jobCode}_{artst}".into());
        assert_refused(p, "unknown variable {artst}");
        let p = problems_with(|v| v["tree"][0] = "{item}".into());
        assert_refused(p, "{item} only works");
        let p = problems_with(|v| v["folderName"] = "{artist|title}".into());
        assert_refused(p, "unknown transform");
        let p = problems_with(|v| v["folderName"] = "{jobCode".into());
        assert_refused(p, "never closed");
    }

    #[test]
    fn refuses_missing_client_field_and_bad_defaults() {
        let p = problems_with(|v| v["fields"][0]["type"] = "text".into());
        assert_refused(p, "exactly one field of type \"client\"");
        let p = problems_with(|v| v["fields"][2]["default"] = serde_json::json!(["4x5"]));
        assert_refused(p, "default that doesn't fit");
    }

    #[test]
    fn refuses_other_schema_and_bad_id() {
        let p = problems_with(|v| v["schema"] = 2.into());
        assert_refused(p, "different version of Nest");
        let p = problems_with(|v| v["id"] = "Grading MV".into());
        assert_refused(p, "lowercase letters");
    }

    #[test]
    fn folder_copy_needs_matching_slashes() {
        let p = problems_with(|v| v["files"][0]["from"] = "starter/luts/".into());
        assert_refused(p, "both must end with /");
    }

    #[test]
    fn collects_every_problem_at_once() {
        let p = problems_with(|v| {
            v["id"] = "Bad Id".into();
            v["tree"][0] = "../x".into();
        });
        assert!(p.len() >= 2, "{p:?}");
    }

    #[test]
    fn parse_pattern_pieces() {
        let pieces = parse_pattern("{yymm}_{artist|upper}.md").unwrap();
        assert_eq!(
            pieces,
            vec![
                Piece::Var {
                    name: "yymm".into(),
                    transform: None
                },
                Piece::Text("_".into()),
                Piece::Var {
                    name: "artist".into(),
                    transform: Some(Transform::Upper)
                },
                Piece::Text(".md".into()),
            ]
        );
        assert!(parse_pattern("a}b").is_err());
        assert!(parse_pattern("{}").is_err());
    }
}

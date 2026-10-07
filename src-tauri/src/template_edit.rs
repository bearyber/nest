//! The template editor's Rust side: preview a draft with the real planner, save it as one of
//! your templates, and take templates away (to the Recycle Bin / Trash) as a whole.
//!
//! Safety (v0.2 review): the draft's id, version and folder are never trusted. Rust picks the
//! id (from the source for edit/customize, from the name for new) and the version (highest of
//! yours + 1). Saving writes a new folder next to the old ones (T1); nothing is overwritten.
//! Starter files are copied from the source template. A draft the preview finds a problem with
//! is refused, so a saved template always plans cleanly.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use crate::names::DEFAULT_CODE_PATTERN;
use crate::plan::{plan_project, ClientValue, Date, FieldValue, Level, PlanContext, Values};
use crate::template::{
    load_dir, FieldKind, LoadedTemplate, Template, TEMPLATE_SCHEMA, TEMPLATE_SUFFIX,
};
use crate::template_io::{copy_starters, delete, free_folder, write_new, Installed};

/// What the editor opens.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateDraft {
    pub template: Template,
    pub built_in: bool,
    /// A built-in that one of yours replaces (the editor offers "Open your version").
    pub replaced: bool,
    /// Starter files that come along (read-only in the editor), `/`-separated.
    pub starter_files: Vec<String>,
}

pub fn draft(t: &Installed) -> TemplateDraft {
    TemplateDraft {
        template: t.loaded.template.clone(),
        built_in: t.built_in,
        replaced: t.replaced,
        starter_files: t.loaded.files.iter().cloned().collect(),
    }
}

/// What New Project would make from a draft, with example answers: billed to Acme, project
/// "Summer Campaign", every box ticked and every choice picked, so all folders show.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplatePreview {
    pub folder_name: String,
    /// The job code the example project would get, in this computer's job-code format.
    pub job_code: String,
    pub title: String,
    pub folders: Vec<String>,
    pub files: Vec<String>,
    /// Plain-language reasons the draft can't be saved yet. Empty = fine.
    pub problems: Vec<String>,
}

/// What the example answers are measured against: today, and this computer's job-code format.
#[derive(Debug, Clone)]
pub struct Example {
    pub today: Date,
    pub code_pattern: String,
    pub marker: String,
}

impl Example {
    pub fn new(today: Date) -> Self {
        Example {
            today,
            code_pattern: DEFAULT_CODE_PATTERN.into(),
            marker: "W".into(),
        }
    }
}

/// The example billing client (never a real one).
pub const EXAMPLE_CLIENT: (&str, &str) = ("Acme", "ACME");

pub fn preview(draft: &Template, source: &LoadedTemplate, ex: &Example) -> TemplatePreview {
    let mut t = draft.clone();
    // The id is Rust's to pick on save; the draft's (maybe empty) id mustn't fail the preview.
    t.id = source.template.id.clone();
    t.schema = TEMPLATE_SCHEMA;
    let mut problems = match t.validate() {
        Ok(()) => vec![],
        Err(e) => e.problems,
    };
    if !problems.is_empty() {
        return TemplatePreview {
            folder_name: String::new(),
            job_code: String::new(),
            title: String::new(),
            folders: vec![],
            files: vec![],
            problems,
        };
    }
    let loaded = LoadedTemplate {
        template: t,
        dir: source.dir.clone(),
        files: source.files.clone(),
    };
    let plan = plan_project(
        &loaded,
        &sample_values(&loaded.template, ex.today),
        &sample_ctx(ex),
    );
    problems.extend(
        plan.issues
            .iter()
            .filter(|i| i.level == Level::Error)
            .map(|i| i.message.clone()),
    );
    TemplatePreview {
        folder_name: plan.folder_name,
        job_code: plan.job_code,
        title: plan.title,
        folders: plan.folders,
        files: plan.files.into_iter().map(|f| f.to).collect(),
        problems,
    }
}

/// A plausible example answer for a text question: generic, never a real client or artist.
fn sample_text(key: &str, label: &str) -> String {
    let by_key = match key {
        "name" | "project" | "title" => Some("Summer Campaign"),
        "artist" | "brand" => Some("Acme Band"),
        "song" => Some("Big Day"),
        "event" => Some("Acme Fest"),
        "notes" | "description" => Some(""),
        _ => None,
    };
    match by_key {
        Some(s) => s.to_string(),
        None if label.trim().is_empty() => "Example".into(),
        None => label.to_string(),
    }
}

fn sample_values(t: &Template, today: Date) -> Values {
    let iso = format!("{:04}-{:02}-{:02}", today.year, today.month, today.day);
    let mut v = BTreeMap::new();
    for f in &t.fields {
        let value = match f.kind {
            FieldKind::Client => FieldValue::Client(ClientValue {
                name: EXAMPLE_CLIENT.0.into(),
                code: EXAMPLE_CLIENT.1.into(),
            }),
            FieldKind::Bool => FieldValue::Bool(true),
            FieldKind::Multi => FieldValue::List(f.options.clone()),
            FieldKind::Select => FieldValue::Text(f.options.first().cloned().unwrap_or_default()),
            FieldKind::Date => FieldValue::Text(iso.clone()),
            // Never empty for a required question, and cut to the field's limit, so the
            // example answers can't fail the template.
            FieldKind::Text | FieldKind::Longtext => {
                let mut s = sample_text(&f.key, &f.label);
                if s.is_empty() && f.required {
                    s = if f.label.trim().is_empty() {
                        "Example".into()
                    } else {
                        f.label.clone()
                    };
                }
                FieldValue::Text(match f.max {
                    Some(max) => s.chars().take(max.max(1)).collect(),
                    None => s,
                })
            }
        };
        v.insert(f.key.clone(), value);
    }
    v
}

fn sample_ctx(ex: &Example) -> PlanContext {
    PlanContext {
        jobs_root: PathBuf::from("D:/Jobs"),
        id: "00000000-0000-4000-8000-000000000000".into(),
        created_at: "2026-01-01T00:00:00+00:00".into(),
        date: ex.today,
        space: "Work".into(),
        code_pattern: ex.code_pattern.clone(),
        marker: ex.marker.clone(),
        existing_codes: vec![],
        existing_folders: vec![],
        code_override: None,
        own_code: None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SaveMode {
    /// One of yours: saved as its next version, alongside the old ones.
    Edit,
    /// A built-in: saved as yours with the same id, so New Project offers yours instead.
    Customize,
    /// A new template (a copy of `source`, or an empty one started from Blank): new id.
    New,
}

/// Save `draft` as one of your templates. `source` is the template the editor started from
/// (found by Rust from its key); its starter files are copied along.
pub fn save(
    draft: &Template,
    mode: SaveMode,
    source: &Installed,
    installed: &[Installed],
    user_dir: &Path,
    built_in_ids: &[&str],
    ex: &Example,
) -> Result<LoadedTemplate, String> {
    let source_id = source.loaded.template.id.clone();
    let yours_max = |id: &str| {
        installed
            .iter()
            .filter(|t| !t.built_in && t.loaded.template.id == id)
            .map(|t| t.loaded.template.version)
            .max()
    };
    let (id, version) = match mode {
        SaveMode::Edit => {
            if source.built_in {
                return Err("Built-in templates can't be edited. Customize it instead.".into());
            }
            (source_id.clone(), yours_max(&source_id).unwrap_or(0) + 1)
        }
        SaveMode::Customize => {
            if !source.built_in {
                return Err("That one is already yours. Edit it instead.".into());
            }
            if let Some(max) = yours_max(&source_id) {
                return Err(format!(
                    "You already have your own version of \"{}\" (version {max}). Edit that one instead.",
                    source.loaded.template.name
                ));
            }
            (source_id.clone(), source.loaded.template.version + 1)
        }
        SaveMode::New => {
            let mut taken: HashSet<String> = installed
                .iter()
                .map(|t| t.loaded.template.id.clone())
                .collect();
            taken.extend(built_in_ids.iter().map(|s| s.to_string()));
            (new_id(&draft.name, &taken, user_dir), 1)
        }
    };

    let mut t = draft.clone();
    t.schema = TEMPLATE_SCHEMA;
    t.id = id.clone();
    t.version = version;
    t.name = t.name.trim().to_string();
    t.description = t.description.trim().to_string();
    let check = preview(&t, &source.loaded, ex);
    if !check.problems.is_empty() {
        return Err(format!(
            "This template can't be saved yet:\n- {}",
            check.problems.join("\n- ")
        ));
    }

    fs::create_dir_all(user_dir)
        .map_err(|e| format!("Couldn't create the templates folder: {e}"))?;
    let tmp = user_dir.join(format!(".edit-{}", uuid::Uuid::new_v4().simple()));
    let result = (|| -> Result<LoadedTemplate, String> {
        fs::create_dir(&tmp).map_err(|e| format!("Couldn't save the template: {e}"))?;
        copy_starters(&source.loaded.dir, &tmp)?;
        let text = serde_json::to_string_pretty(&t).map_err(|e| e.to_string())? + "\n";
        write_new(&tmp.join(format!("{id}{TEMPLATE_SUFFIX}")), text.as_bytes())?;
        load_dir(&tmp).map_err(|e| e.to_string())?;
        let base = if yours_max(&id).is_some() {
            format!("{id}-v{version}")
        } else {
            id.clone()
        };
        let folder = free_folder(user_dir, &base);
        fs::rename(&tmp, &folder).map_err(|e| format!("Couldn't save the template: {e}"))?;
        load_dir(&folder).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&tmp);
    }
    result
}

/// An id from a template name: lowercase a–z, 0–9 and -, unique among `taken` and the folders
/// in `user_dir`. Names without any such letters (e.g. Korean) become `template`, `template-2`…
pub fn new_id(name: &str, taken: &HashSet<String>, user_dir: &Path) -> String {
    let mut slug = String::new();
    for c in name.nfc().flat_map(char::to_lowercase) {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let mut base: String = slug.trim_matches('-').chars().take(40).collect();
    base = base.trim_end_matches('-').to_string();
    if base.is_empty() {
        base = "template".into();
    }
    (1..)
        .map(|n| {
            if n == 1 {
                base.clone()
            } else {
                format!("{base}-{n}")
            }
        })
        .find(|id| !taken.contains(id) && !user_dir.join(id).exists())
        .expect("a free id")
}

/// Every version of one of your templates, oldest first.
pub fn your_versions<'a>(id: &str, installed: &'a [Installed]) -> Vec<&'a Installed> {
    let mut v: Vec<&Installed> = installed
        .iter()
        .filter(|t| !t.built_in && t.loaded.template.id == id)
        .collect();
    v.sort_by_key(|t| t.loaded.template.version);
    v
}

/// Move every version of one of your templates to the Recycle Bin / Trash, oldest first, so
/// a half-finished move never leaves an older version quietly offered in place of the newest.
/// Returns how many were moved.
pub fn trash_all_versions(
    id: &str,
    installed: &[Installed],
    user_dir: &Path,
) -> Result<usize, String> {
    let versions = your_versions(id, installed);
    if versions.is_empty() {
        return Err("That template isn't one of yours".into());
    }
    let total = versions.len();
    for (moved, t) in versions.into_iter().enumerate() {
        delete(t, user_dir).map_err(|e| {
            format!(
                "Moved {moved} of {total} versions. Version {} is still there: {e}",
                t.loaded.template.version
            )
        })?;
    }
    Ok(total)
}

/// "Go back to Nest's original": your versions of a built-in go to the Recycle Bin / Trash.
/// Refused unless a built-in with that id exists, so it can never take away a template that
/// has nothing to go back to.
pub fn reset_to_original(
    id: &str,
    installed: &[Installed],
    user_dir: &Path,
) -> Result<usize, String> {
    if !installed
        .iter()
        .any(|t| t.built_in && t.loaded.template.id == id)
    {
        return Err("That template has no Nest original to go back to".into());
    }
    trash_all_versions(id, installed, user_dir)
}

/// Decision T-A (2026-10-02): one copy per template. After a save or an update has written and
/// loaded the new copy (`keep`), the older copies of that id go to the Recycle Bin / Trash,
/// oldest first. `bin` moves one folder (the real bin in the app, a stub in tests). If a move
/// fails, the rest stay and the error says how many went: nothing is lost either way.
pub fn retire_older_copies(
    id: &str,
    keep: &Path,
    installed: &[Installed],
    bin: &dyn Fn(&Installed) -> Result<(), String>,
) -> Result<usize, String> {
    // Never bin anything unless the new copy is really installed (loaded back from disk):
    // otherwise you could be left with no copy at all.
    if !your_versions(id, installed)
        .iter()
        .any(|t| t.loaded.dir == keep)
    {
        return Err(
            "Saved, but the new copy couldn't be loaded back, so the previous one was kept.".into(),
        );
    }
    let older: Vec<&Installed> = your_versions(id, installed)
        .into_iter()
        .filter(|t| t.loaded.dir != keep)
        .collect();
    for (moved, t) in older.iter().enumerate() {
        bin(t).map_err(|e| {
            format!(
                "Saved. {moved} of {} older copies went to the bin; the rest stay: {e}",
                older.len()
            )
        })?;
    }
    Ok(older.len())
}

/// The one-time tidy on first start of v0.3.0: for each of your templates with several
/// copies, keep the newest and retire the rest. Returns how many copies were moved.
pub fn tidy_all(
    installed: &[Installed],
    bin: &dyn Fn(&Installed) -> Result<(), String>,
) -> Result<usize, String> {
    let mut ids: Vec<&str> = installed
        .iter()
        .filter(|t| !t.built_in)
        .map(|t| t.loaded.template.id.as_str())
        .collect();
    ids.sort();
    ids.dedup();
    let mut moved = 0;
    for id in ids {
        let versions = your_versions(id, installed);
        if let Some(newest) = versions.last() {
            moved += retire_older_copies(id, &newest.loaded.dir, installed, bin)?;
        }
    }
    Ok(moved)
}

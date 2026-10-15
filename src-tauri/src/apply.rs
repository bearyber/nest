//! `apply_plan`: create exactly what a Plan lists, or undo this run's items (build spec §2, §6).
//!
//! Safety rules (CLAUDE.md #4 + M2 review):
//! - the project folder uses `create_dir`, so an existing folder is never merged into;
//! - files are opened with `create_new`, so nothing is ever overwritten (no `fs::copy`);
//! - starter sources must be regular files inside the template folder (no links out);
//! - rollback only touches items this run created, and removes folders only if empty.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::error::io_reason;
use crate::manifest::write_json_atomic;
use crate::plan::{fill_text, Plan, MANIFEST_NAME};

#[derive(Debug)]
pub struct ApplyError {
    pub message: String,
    /// The project folder already existed: re-plan with a new code and try again.
    pub root_exists: bool,
    /// Items rollback couldn't remove (e.g. a file locked by a sync app).
    pub left_behind: Vec<PathBuf>,
}

enum Created {
    Dir(PathBuf),
    File(PathBuf),
}

/// Called before each create step with its index. Tests use it to force a failure at step N.
pub type StepHook<'a> = &'a mut dyn FnMut(usize) -> io::Result<()>;

/// Room left on the drive after copying a client kit (the starter files and the manifest are
/// small; this keeps the drive from being filled to the last byte).
const SPACE_MARGIN: u64 = 64 * 1024 * 1024;

/// `kit_dir`: the client kit folder the Plan's kit files are read from (None = no kit).
pub fn apply_plan(
    plan: &Plan,
    template_dir: &Path,
    kit_dir: Option<&Path>,
) -> Result<(), ApplyError> {
    apply_plan_with(plan, template_dir, kit_dir, &mut |_| Ok(()))
}

/// Steps `hook` is called for: the project folder, each folder, each file, the manifest.
pub fn step_count(plan: &Plan) -> usize {
    let kit = plan
        .kit
        .as_ref()
        .map_or(0, |k| k.folders.len() + k.files.len());
    2 + plan.folders.len() + plan.files.len() + kit
}

pub fn apply_plan_with(
    plan: &Plan,
    template_dir: &Path,
    kit_dir: Option<&Path>,
    hook: StepHook,
) -> Result<(), ApplyError> {
    let refuse = |message: String| ApplyError {
        message,
        root_exists: false,
        left_behind: vec![],
    };
    if !plan.can_create() {
        return Err(refuse(
            "This project has problems to fix before it can be created".into(),
        ));
    }
    let kit = plan
        .kit
        .as_ref()
        .filter(|k| !k.files.is_empty() || !k.folders.is_empty());
    if let Some(kit) = kit {
        if kit_dir.is_none() {
            return Err(refuse(format!(
                "The folder with {}'s files is missing",
                kit.client
            )));
        }
        // Enough room for the kit? (Unknown free space: go ahead; a full drive still fails
        // cleanly and rolls back.)
        let parent = plan.root.parent().unwrap_or(&plan.root);
        if let Ok(free) = fs4::available_space(parent) {
            if free < kit.bytes.saturating_add(SPACE_MARGIN) {
                return Err(refuse(format!(
                    "Not enough free space for {}'s files: it needs {}, and the drive has {} free.",
                    kit.client,
                    crate::kits::human_size(kit.bytes),
                    crate::kits::human_size(free)
                )));
            }
        }
    }
    let mut created = Vec::new();
    match run(plan, template_dir, kit_dir, &mut created, hook) {
        Ok(()) => Ok(()),
        Err((path, e)) => {
            let root_exists = e.kind() == io::ErrorKind::AlreadyExists && path == plan.root;
            let left_behind = rollback(created);
            let mut message = format!("Couldn't create \"{}\": {}", path.display(), io_reason(&e));
            if !left_behind.is_empty() {
                let list: Vec<String> = left_behind
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect();
                message.push_str(&format!(
                    ". These couldn't be cleaned up and can be removed by hand: {}",
                    list.join(", ")
                ));
            }
            Err(ApplyError {
                message,
                root_exists,
                left_behind,
            })
        }
    }
}

type StepResult = Result<(), (PathBuf, io::Error)>;

/// Attach the path an io error happened at.
fn at(path: &Path) -> impl Fn(io::Error) -> (PathBuf, io::Error) + '_ {
    move |e| (path.to_path_buf(), e)
}

fn run(
    plan: &Plan,
    template_dir: &Path,
    kit_dir: Option<&Path>,
    created: &mut Vec<Created>,
    hook: StepHook,
) -> StepResult {
    let mut step = 0;
    let mut next_step = |path: &Path| -> StepResult {
        let r = hook(step).map_err(|e| (path.to_path_buf(), e));
        step += 1;
        r
    };

    // Project folder: create_dir (not _all) so a missing jobs root or an existing
    // folder is an error, never a silent create or merge.
    next_step(&plan.root)?;
    fs::create_dir(&plan.root).map_err(at(&plan.root))?;
    created.push(Created::Dir(plan.root.clone()));

    for folder in &plan.folders {
        let path = native(&plan.root, folder);
        next_step(&path)?;
        fs::create_dir(&path).map_err(at(&path))?;
        created.push(Created::Dir(path));
    }

    let template_root = template_dir.canonicalize().map_err(at(template_dir))?;
    for file in &plan.files {
        let src = native(template_dir, &file.from);
        let dest = native(&plan.root, &file.to);
        next_step(&dest)?;
        check_source(&src, &template_root, "template").map_err(at(&src))?;
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&dest)
            .map_err(at(&dest))?;
        created.push(Created::File(dest.clone()));
        write_starter(&src, &mut out, file.fill.then_some(plan)).map_err(at(&dest))?;
    }

    // The client kit: its folders, then each file copied as-is (never over anything).
    if let (Some(kit), Some(kit_dir)) = (&plan.kit, kit_dir) {
        for folder in &kit.folders {
            let path = native(&plan.root, folder);
            next_step(&path)?;
            fs::create_dir(&path).map_err(at(&path))?;
            created.push(Created::Dir(path));
        }
        if !kit.files.is_empty() {
            let kit_root = kit_dir.canonicalize().map_err(at(kit_dir))?;
            for file in &kit.files {
                let src = native(kit_dir, &file.from);
                let dest = native(&plan.root, &file.to);
                next_step(&dest)?;
                // Checked again now: a link swapped in since the preview is refused.
                check_source(&src, &kit_root, "client kit").map_err(at(&src))?;
                let mut out = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&dest)
                    .map_err(at(&dest))?;
                created.push(Created::File(dest.clone()));
                write_starter(&src, &mut out, None).map_err(at(&dest))?;
            }
        }
    }

    let manifest = plan.root.join(MANIFEST_NAME);
    next_step(&manifest)?;
    let written = write_json_atomic(&manifest, &plan.manifest, true);
    // The project folder is brand new, so a manifest there can only be ours. Record it even
    // if a step after the rename (the folder sync on macOS) failed, so rollback removes it.
    if written.is_ok() || manifest.exists() {
        created.push(Created::File(manifest.clone()));
    }
    written.map_err(at(&manifest))
}

/// Plan paths are `/`-separated; build a native path one segment at a time.
fn native(base: &Path, rel: &str) -> PathBuf {
    rel.split('/')
        .fold(base.to_path_buf(), |p, seg| p.join(seg))
}

/// The source must be a regular file (not a link) that really lives in its folder (`root`,
/// canonical): a link anywhere on the way out is caught by `canonicalize`.
fn check_source(src: &Path, root: &Path, what: &str) -> io::Result<()> {
    let meta = fs::symlink_metadata(src)?;
    let inside = src.canonicalize()?.starts_with(root);
    if meta.file_type().is_file() && inside {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("it isn't a regular file inside the {what} folder"),
        ))
    }
}

fn write_starter(src: &Path, out: &mut File, fill_from: Option<&Plan>) -> io::Result<()> {
    match fill_from {
        Some(plan) => {
            let bytes = fs::read(src)?;
            let content = match String::from_utf8(bytes) {
                Ok(text) => fill_text(&text, &plan.fill).into_bytes(),
                Err(not_text) => not_text.into_bytes(), // binary: copied as-is
            };
            out.write_all(&content)?;
        }
        None => {
            io::copy(&mut File::open(src)?, out)?;
        }
    }
    out.sync_all()
}

/// Undo in reverse. Folders are removed only if empty, so anything someone else put
/// inside survives. Keeps going past failures and returns what was left behind.
fn rollback(created: Vec<Created>) -> Vec<PathBuf> {
    let mut left_behind = Vec::new();
    for item in created.into_iter().rev() {
        let (path, result) = match item {
            Created::File(p) => {
                let r = fs::remove_file(&p);
                (p, r)
            }
            Created::Dir(p) => {
                let r = fs::remove_dir(&p);
                (p, r)
            }
        };
        if result.is_err() {
            left_behind.push(path);
        }
    }
    left_behind
}

//! The template editor's Rust side (v0.2): preview, save (id/version picked by Rust, nothing
//! overwritten), and the guards around "go back to original". Temp dirs only.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use nest_lib::plan::Date;
use nest_lib::template::{Template, TreeEntry, TreeRule};
use nest_lib::template_edit::{
    new_id, preview, reset_to_original, retire_older_copies, save, tidy_all, trash_all_versions,
    Example, SaveMode,
};
use nest_lib::template_io::{import, load_installed, Installed, BUILT_IN_IDS, LIMITS};

const TODAY: Date = Date {
    year: 2026,
    month: 3,
    day: 2,
};

fn bundled_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("templates")
}

struct Setup {
    _tmp: tempfile::TempDir,
    user: PathBuf,
}

impl Setup {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("templates");
        Setup { _tmp: tmp, user }
    }
    fn installed(&self) -> Vec<Installed> {
        load_installed(&self.user, &bundled_dir(), BUILT_IN_IDS)
    }
    fn get(&self, id: &str, built_in: bool) -> Installed {
        self.installed()
            .into_iter()
            .filter(|t| t.loaded.template.id == id && t.built_in == built_in)
            .max_by_key(|t| t.loaded.template.version)
            .unwrap_or_else(|| panic!("{id} not installed"))
    }
    fn save(
        &self,
        draft: &Template,
        mode: SaveMode,
        source: &Installed,
    ) -> Result<Template, String> {
        save(
            draft,
            mode,
            source,
            &self.installed(),
            &self.user,
            BUILT_IN_IDS,
            &Example::new(TODAY),
        )
        .map(|l| l.template)
    }
    /// Bernard's colorist templates, installed as his own. They live in the private notes repo
    /// (`private/`); false where it isn't cloned (CI).
    fn with_bernards_templates(&self) -> bool {
        let extras =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../private/extras/bernard-templates");
        if !extras.is_dir() {
            eprintln!(
                "skipped: {} not here (private repo not cloned)",
                extras.display()
            );
            return false;
        }
        for id in [
            "grading-mv",
            "grading-performance",
            "edit-general",
            "lookdev",
        ] {
            import(
                &extras.join(format!("{id}.nesttemplate")),
                &self.user,
                &self.installed(),
                LIMITS,
            )
            .unwrap();
        }
        true
    }
}

/// Opening any template in the editor and saving it unchanged gives back exactly the same
/// template (only the id/version Rust picks differ): nothing in the format gets lost.
#[test]
fn every_template_round_trips_unchanged() {
    let s = Setup::new();
    let with_own = s.with_bernards_templates();
    let ids: Vec<(String, bool)> = s
        .installed()
        .iter()
        .filter(|t| !t.superseded)
        .map(|t| (t.loaded.template.id.clone(), t.built_in))
        .collect();
    // 5 built-ins, plus Bernard's 4 where the private repo is cloned.
    assert!(ids.len() >= if with_own { 9 } else { 5 }, "{ids:?}");
    for (id, built_in) in ids {
        let source = s.get(&id, built_in);
        let original = source.loaded.template.clone();
        let p = preview(&original, &source.loaded, &Example::new(TODAY));
        assert!(p.problems.is_empty(), "{id}: {:?}", p.problems);
        let mode = if built_in {
            SaveMode::Customize
        } else {
            SaveMode::Edit
        };
        let saved = s
            .save(&original, mode, &source)
            .unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(saved.id, original.id, "{id}");
        assert_eq!(saved.version, original.version + 1, "{id}");
        let same = Template {
            version: original.version + 1,
            ..saved.clone()
        };
        assert_eq!(
            same,
            Template {
                version: original.version + 1,
                ..original
            },
            "{id}"
        );
    }
}

#[test]
fn preview_shows_every_folder_with_example_answers() {
    let s = Setup::new();
    let video = s.get("video", true);
    let p = preview(&video.loaded.template, &video.loaded, &Example::new(TODAY));
    assert!(p.problems.is_empty(), "{:?}", p.problems);
    assert_eq!(p.folder_name, "260302_SUMMER_CAMPAIGN");
    // Every box ticked and every format picked, so all folders show.
    for f in [
        "05_EXPORTS/16x9",
        "05_EXPORTS/9x16",
        "05_EXPORTS/1x1",
        "05_EXPORTS/4x5",
        "06_GRAPHICS",
    ] {
        assert!(p.folders.contains(&f.to_string()), "{f} in {:?}", p.folders);
    }
    assert_eq!(p.files, ["NOTES.md"]);
}

#[test]
fn preview_reports_problems_in_plain_words() {
    let s = Setup::new();
    let general = s.get("general", true);
    let mut t = general.loaded.template.clone();
    t.tree.push(TreeEntry::Folder("CON".into()));
    let p = preview(&t, &general.loaded, &Example::new(TODAY));
    assert!(!p.problems.is_empty());
    assert!(
        p.problems.iter().any(|m| m.contains("CON")),
        "{:?}",
        p.problems
    );

    let mut t = general.loaded.template.clone();
    t.name = "  ".into();
    assert!(!preview(&t, &general.loaded, &Example::new(TODAY))
        .problems
        .is_empty());
}

#[test]
fn saving_refuses_a_template_with_problems() {
    let s = Setup::new();
    let general = s.get("general", true);
    let mut t = general.loaded.template.clone();
    t.tree.push(TreeEntry::Folder("a:b".into()));
    let err = s.save(&t, SaveMode::Customize, &general).unwrap_err();
    assert!(err.contains("can't be saved"), "{err}");
    assert!(
        s.installed().iter().all(|t| t.built_in),
        "nothing was written"
    );
}

/// Review fix 2: Rust ignores the draft's id. A new template carrying a built-in's id gets a
/// fresh one from its name, and never replaces the built-in.
#[test]
fn a_new_template_never_takes_an_existing_id() {
    let s = Setup::new();
    let blank = s.get("blank", true);
    let mut t = blank.loaded.template.clone();
    t.name = "Video".into(); // "video" is a built-in id
    t.id = "blank".into();
    let saved = s.save(&t, SaveMode::New, &blank).unwrap();
    assert_eq!(saved.id, "video-2");
    assert_eq!(saved.version, 1);
    let blank_after = s.get("blank", true);
    assert!(!blank_after.replaced, "Blank is still the built-in");
}

/// Review fix 2: editing an older version of yours saves as highest + 1, never over a version.
#[test]
fn editing_an_old_version_saves_as_the_newest() {
    let s = Setup::new();
    let general = s.get("general", true);
    let mut t = general.loaded.template.clone();
    t.name = "Mine".into();
    let v1 = s.save(&t, SaveMode::New, &general).unwrap();
    let source_v1 = s.get(&v1.id, false);
    let v2 = s
        .save(&source_v1.loaded.template, SaveMode::Edit, &source_v1)
        .unwrap();
    assert_eq!(v2.version, 2);
    // Edit from v1 again (now superseded): becomes v3, v1 and v2 stay on disk.
    let v3 = s
        .save(&source_v1.loaded.template, SaveMode::Edit, &source_v1)
        .unwrap();
    assert_eq!(v3.version, 3);
    let versions: Vec<u32> = s
        .installed()
        .iter()
        .filter(|t| t.loaded.template.id == v1.id)
        .map(|t| t.loaded.template.version)
        .collect();
    assert_eq!(versions.len(), 3, "{versions:?}");
    let newest = s
        .installed()
        .into_iter()
        .find(|t| t.loaded.template.id == v1.id && !t.superseded)
        .unwrap();
    assert_eq!(newest.loaded.template.version, 3);
}

#[test]
fn customizing_a_built_in_replaces_it_in_new_project() {
    let s = Setup::new();
    let video = s.get("video", true);
    let mut t = video.loaded.template.clone();
    t.tree.push(TreeEntry::Folder("07_ADMIN".into()));
    let mine = s.save(&t, SaveMode::Customize, &video).unwrap();
    assert_eq!(mine.id, "video");
    let installed = s.installed();
    let offered: Vec<&Installed> = installed
        .iter()
        .filter(|t| !t.superseded && t.loaded.template.id == "video")
        .collect();
    assert_eq!(offered.len(), 1);
    assert!(!offered[0].built_in, "yours is offered");
    let original = installed
        .iter()
        .find(|t| t.built_in && t.loaded.template.id == "video")
        .unwrap();
    assert!(original.replaced);
    // Starter files came along.
    assert!(offered[0].loaded.files.contains("starter/NOTES.md"));
    // Customizing again is refused: edit yours instead.
    assert!(s
        .save(&t, SaveMode::Customize, &s.get("video", true))
        .is_err());
}

#[test]
fn rules_on_folders_survive_saving() {
    let s = Setup::new();
    let video = s.get("video", true);
    let mut t = video.loaded.template.clone();
    // Not ticked → only then; a folder with both rules.
    t.tree.push(TreeEntry::Rule(TreeRule {
        name: "07_NO_GRAPHICS".into(),
        when: Some("!graphics".into()),
        each: None,
    }));
    t.tree.push(TreeEntry::Rule(TreeRule {
        name: "08_GFX/{item}".into(),
        when: Some("graphics".into()),
        each: Some("formats".into()),
    }));
    let saved = s.save(&t, SaveMode::Customize, &video).unwrap();
    assert_eq!(saved.tree, t.tree);
}

/// The guards around taking templates away. (The actual move to the Recycle Bin isn't run in
/// tests: it would fill the real bin.)
#[test]
fn reset_and_delete_only_touch_your_templates() {
    let s = Setup::new();
    let installed = s.installed();
    // A built-in you haven't customized: nothing of yours to take away.
    assert!(reset_to_original("video", &installed, &s.user).is_err());
    assert!(trash_all_versions("video", &installed, &s.user).is_err());
    // One of yours with no Nest original: "go back" is refused.
    let general = s.get("general", true);
    let mut t = general.loaded.template.clone();
    t.name = "Mine".into();
    let mine = s.save(&t, SaveMode::New, &general).unwrap();
    let err = reset_to_original(&mine.id, &s.installed(), &s.user).unwrap_err();
    assert!(err.contains("no Nest original"), "{err}");
}

#[test]
fn ids_from_names() {
    let tmp = tempfile::tempdir().unwrap();
    let taken: HashSet<String> = ["video".to_string(), "template".to_string()].into();
    assert_eq!(
        new_id("Music Video (2026)", &taken, tmp.path()),
        "music-video-2026"
    );
    assert_eq!(new_id("Video", &taken, tmp.path()), "video-2");
    assert_eq!(new_id("뮤직비디오", &taken, tmp.path()), "template-2");
    assert_eq!(new_id("  --Édit!! ", &taken, tmp.path()), "dit");
    std::fs::create_dir(tmp.path().join("grade")).unwrap();
    assert_eq!(
        new_id("Grade", &taken, tmp.path()),
        "grade-2",
        "existing folders count too"
    );
}

/// Review (v0.2): a required long-text question, or a label longer than the field's limit,
/// must not make the example answers fail, or the template could never be saved.
#[test]
fn required_long_text_and_short_limits_still_save() {
    use nest_lib::template::{Field, FieldKind};
    let s = Setup::new();
    let general = s.get("general", true);
    let mut t = general.loaded.template.clone();
    t.name = "Notes required".into();
    t.fields.push(Field {
        key: "brief".into(),
        label: "Brief".into(),
        kind: FieldKind::Longtext,
        required: true,
        max: None,
        options: vec![],
        default: None,
    });
    t.fields.push(Field {
        key: "code".into(),
        label: "A label much longer than four".into(),
        kind: FieldKind::Text,
        required: true,
        max: Some(4),
        options: vec![],
        default: None,
    });
    let p = preview(&t, &general.loaded, &Example::new(TODAY));
    assert!(p.problems.is_empty(), "{:?}", p.problems);
    assert!(s.save(&t, SaveMode::New, &general).is_ok());
}

/// A stand-in for the Recycle Bin: moves the folder into a "bin" folder (the real bin is
/// never used in tests).
fn stub_bin(bin: &Path) -> impl Fn(&Installed) -> Result<(), String> + '_ {
    move |t: &Installed| {
        std::fs::create_dir_all(bin).unwrap();
        let name = t.loaded.dir.file_name().unwrap();
        std::fs::rename(&t.loaded.dir, bin.join(name)).map_err(|e| e.to_string())
    }
}

/// Decision T-A: saving keeps one copy; the previous one goes to the bin.
#[test]
fn saving_replaces_and_bins_the_previous_copy() {
    let s = Setup::new();
    let general = s.get("general", true);
    let mut t = general.loaded.template.clone();
    t.name = "Mine".into();
    let v1 = s.save(&t, SaveMode::New, &general).unwrap();
    let source = s.get(&v1.id, false);
    let v2 = save(
        &source.loaded.template,
        SaveMode::Edit,
        &source,
        &s.installed(),
        &s.user,
        BUILT_IN_IDS,
        &Example::new(TODAY),
    )
    .unwrap();
    let bin = s.user.parent().unwrap().join("bin");
    let moved = retire_older_copies(&v1.id, &v2.dir, &s.installed(), &stub_bin(&bin)).unwrap();
    assert_eq!(moved, 1);
    let copies: Vec<u32> = s
        .installed()
        .iter()
        .filter(|i| i.loaded.template.id == v1.id)
        .map(|i| i.loaded.template.version)
        .collect();
    assert_eq!(copies, [2], "one copy left: the new one");
    assert_eq!(
        std::fs::read_dir(&bin).unwrap().count(),
        1,
        "the old one is in the bin"
    );
}

#[test]
fn a_bin_failure_keeps_both_copies() {
    let s = Setup::new();
    let general = s.get("general", true);
    let mut t = general.loaded.template.clone();
    t.name = "Mine".into();
    let v1 = s.save(&t, SaveMode::New, &general).unwrap();
    let source = s.get(&v1.id, false);
    let v2 = s
        .save(&source.loaded.template, SaveMode::Edit, &source)
        .unwrap();
    let keep = s
        .installed()
        .into_iter()
        .find(|i| i.loaded.template.id == v2.id && i.loaded.template.version == 2)
        .unwrap();
    let failing = |_: &Installed| -> Result<(), String> { Err("bin is full".into()) };
    let err = retire_older_copies(&v1.id, &keep.loaded.dir, &s.installed(), &failing).unwrap_err();
    assert!(err.contains("bin is full"), "{err}");
    assert_eq!(
        s.installed()
            .iter()
            .filter(|i| i.loaded.template.id == v1.id)
            .count(),
        2
    );
}

#[test]
fn the_one_time_tidy_keeps_the_newest_of_each() {
    let s = Setup::new();
    let general = s.get("general", true);
    for name in ["One", "Two"] {
        let mut t = general.loaded.template.clone();
        t.name = name.into();
        let v1 = s.save(&t, SaveMode::New, &general).unwrap();
        for _ in 0..2 {
            let src = s.get(&v1.id, false);
            s.save(&src.loaded.template, SaveMode::Edit, &src).unwrap();
        }
    }
    let bin = s.user.parent().unwrap().join("bin");
    let moved = tidy_all(&s.installed(), &stub_bin(&bin)).unwrap();
    assert_eq!(moved, 4, "2 older copies of each");
    let mut left: Vec<(String, u32)> = s
        .installed()
        .iter()
        .filter(|i| !i.built_in)
        .map(|i| (i.loaded.template.id.clone(), i.loaded.template.version))
        .collect();
    left.sort();
    assert_eq!(left, [("one".to_string(), 3), ("two".to_string(), 3)]);
}

/// Review (v0.3.0): nothing goes to the bin unless the new copy is really installed.
#[test]
fn nothing_is_binned_unless_the_new_copy_is_loaded() {
    let s = Setup::new();
    let general = s.get("general", true);
    let mut t = general.loaded.template.clone();
    t.name = "Mine".into();
    let v1 = s.save(&t, SaveMode::New, &general).unwrap();
    let never = |_: &Installed| -> Result<(), String> { panic!("must not bin") };
    let missing = s.user.join("not-there");
    assert!(retire_older_copies(&v1.id, &missing, &s.installed(), &never).is_err());
}

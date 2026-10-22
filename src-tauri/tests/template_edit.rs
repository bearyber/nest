//! The template editor's Rust side (v0.2): preview, save (id/version picked by Rust, nothing
//! overwritten), and the guards around "go back to original". Temp dirs only.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use nest_lib::plan::Date;
use nest_lib::template::{Template, TreeEntry, TreeRule};
use nest_lib::template_edit::{
    example_names, new_id, preview, reset_to_original, retire_older_copies, save, save_kit_folder,
    tidy_all, trash_all_versions, with_kit_folder, Example, SaveMode,
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
    // Nest's own templates bring no files (v0.6: you add your own).
    assert!(p.files.is_empty(), "{:?}", p.files);
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
    // The customized copy carries the same (now empty) set of starter files.
    assert_eq!(offered[0].loaded.files, original.loaded.files);
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
        kit: false,
    }));
    t.tree.push(TreeEntry::Rule(TreeRule {
        name: "08_GFX/{item}".into(),
        when: Some("graphics".into()),
        each: Some("formats".into()),
        kit: false,
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

/// Where the kit goes in a template: the folder names with `"kit": true`.
fn kit_folders(t: &Template) -> Vec<&str> {
    t.tree
        .iter()
        .filter(|e| e.kit())
        .map(|e| e.name())
        .collect()
}

/// New Project's "Put them in" (v0.6): one of Nest's templates becomes yours (same id) with a
/// place for client files; choosing another folder moves it there as a new version, and the
/// older copy goes to the bin.
#[test]
fn put_client_files_in_a_folder_saves_a_new_version() {
    let s = Setup::new();
    let video = s.get("video", true);
    assert!(kit_folders(&video.loaded.template).is_empty());

    // One of Nest's → yours, with the same id, like Customize.
    let mine = save_kit_folder(
        &video,
        "01_BRIEF",
        &s.installed(),
        &s.user,
        BUILT_IN_IDS,
        &Example::new(TODAY),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(mine.template.id, "video");
    assert_eq!(mine.template.version, video.loaded.template.version + 1);
    assert_eq!(kit_folders(&mine.template), ["01_BRIEF"]);
    assert_eq!(mine.files, video.loaded.files, "starter files came along");
    let offered = s
        .installed()
        .into_iter()
        .find(|t| !t.superseded && t.loaded.template.id == "video")
        .unwrap();
    assert!(!offered.built_in, "yours is offered now");

    // Yours → its next version, with the kit on that folder only.
    let v2 = save_kit_folder(
        &offered,
        "02_FOOTAGE",
        &s.installed(),
        &s.user,
        BUILT_IN_IDS,
        &Example::new(TODAY),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(v2.template.version, mine.template.version + 1);
    assert_eq!(kit_folders(&v2.template), ["02_FOOTAGE"]);
    // The folder that had it is a plain folder again; everything else is unchanged.
    assert_eq!(v2.template.tree[0], TreeEntry::Folder("01_BRIEF".into()));
    assert_eq!(v2.template.tree.len(), video.loaded.template.tree.len());
    assert_eq!(v2.template.fields, video.loaded.template.fields);

    // One copy per template: the older one goes to the bin.
    let bin = s.user.parent().unwrap().join("bin");
    let moved = retire_older_copies("video", &v2.dir, &s.installed(), &stub_bin(&bin)).unwrap();
    assert_eq!(moved, 1);
    let yours: Vec<u32> = s
        .installed()
        .iter()
        .filter(|i| !i.built_in && i.loaded.template.id == "video")
        .map(|i| i.loaded.template.version)
        .collect();
    assert_eq!(yours, [v2.template.version]);
    assert_eq!(std::fs::read_dir(&bin).unwrap().count(), 1);
}

#[test]
fn put_client_files_refuses_folders_made_only_sometimes_or_not_there() {
    let s = Setup::new();
    let video = s.get("video", true);
    for (folder, says) in [
        ("06_GRAPHICS", "only made sometimes"), // only if a box is ticked
        ("05_EXPORTS/{item}", "only made sometimes"), // one per choice
        ("05_EXPORTS", "only made sometimes"),  // only the parent of a one-per folder
        ("99_NOPE", "isn't one of this template's folders"),
        ("", "Choose a folder"),
    ] {
        let err = save_kit_folder(
            &video,
            folder,
            &s.installed(),
            &s.user,
            BUILT_IN_IDS,
            &Example::new(TODAY),
            &Default::default(),
        )
        .unwrap_err();
        assert!(err.contains(says), "{folder}: {err}");
    }
    assert!(
        s.installed().iter().all(|t| t.built_in),
        "nothing was saved"
    );
}

#[test]
fn put_client_files_in_a_nested_folder_or_its_parent() {
    let s = Setup::new();
    let mut t = s.get("video", true).loaded.template;
    t.tree
        .insert(2, TreeEntry::Folder("02_FOOTAGE/DAY1".into()));
    t.tree.push(TreeEntry::Rule(TreeRule {
        name: "07_BRAND".into(),
        kit: true,
        ..TreeRule::default()
    }));

    // A nested folder listed in the template; the one that had it is plain again.
    let nested = with_kit_folder(&t, "02_FOOTAGE/DAY1").unwrap();
    assert_eq!(kit_folders(&nested), ["02_FOOTAGE/DAY1"]);
    assert_eq!(
        nested.tree.last(),
        Some(&TreeEntry::Folder("07_BRAND".into()))
    );

    // A parent that's only there because of a folder inside it: added as its own entry.
    t.tree.remove(1); // "02_FOOTAGE" itself
    let parent = with_kit_folder(&t, "02_FOOTAGE").unwrap();
    assert_eq!(kit_folders(&parent), ["02_FOOTAGE"]);
    assert_eq!(parent.tree.len(), t.tree.len() + 1);
    assert!(parent.validate().is_ok());
}

#[test]
fn example_names_match_the_preview() {
    let s = Setup::new();
    let video = s.get("video", true);
    let t = &video.loaded.template;
    let ex = Example::new(TODAY);
    let p = preview(t, &video.loaded, &ex);
    let names: Vec<String> = [
        t.folder_name.as_str(),
        "01_BRIEF",
        "{item}",
        "{name}_{start}",
        "{client}",
        "{nope}",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let out = example_names(t, &ex, &names);
    assert_eq!(out[0], p.folder_name);
    assert_eq!(out[1], "01_BRIEF");
    // "One folder for each format": the first format.
    assert_eq!(out[2], "16x9");
    assert!(p.folders.contains(&format!("05_EXPORTS/{}", out[2])));
    assert_eq!(out[3], "SummerCampaign_20260302");
    assert_eq!(out[4], "Acme");
    assert_eq!(out[5], "{nope}");
}

/// Resolve starter project: a `.drp` exported into the editor is staged like a dropped file,
/// saved into the template (never as a file in the project), and kept by the next save.
#[test]
fn a_resolve_project_is_staged_saved_and_kept() {
    use nest_lib::staged::OnDisk;
    use nest_lib::template::ResolveStarter;
    use nest_lib::template_edit::{save_session, EditorSession};

    let s = Setup::new();
    let video = s.get("video", true);
    let export_dir = s._tmp.path().join("extras/abc");
    std::fs::create_dir_all(&export_dir).unwrap();
    std::fs::write(export_dir.join("Grade.drp"), b"drp bytes").unwrap();

    let mut session = EditorSession::from_template(&video);
    let drp = session.add_resolve_drp(
        "Grade.drp",
        OnDisk {
            base: export_dir.clone(),
            rel: "Grade.drp".into(),
            size: 9,
        },
    );
    assert_eq!(drp, "files/Grade.drp");
    let mut draft = video.loaded.template.clone();
    let files_before = draft.files.clone();

    // Missing from the template: the preview says so and Save refuses.
    draft.resolve = Some(ResolveStarter {
        drp: "files/Other.drp".into(),
        name: None,
        folder: "{client}".into(),
        on: true,
        library: None,
    });
    let p = preview(&draft, &session.loaded(draft.clone()), &Example::new(TODAY));
    assert!(
        p.problems.iter().any(|m| m.contains("files/Other.drp")),
        "{:?}",
        p.problems
    );

    draft.resolve.as_mut().unwrap().drp = drp.clone();
    let p = preview(&draft, &session.loaded(draft.clone()), &Example::new(TODAY));
    assert!(p.problems.is_empty(), "{:?}", p.problems);
    assert!(
        !p.files.iter().any(|f| f.ends_with(".drp")),
        "never a file in the project"
    );
    let saved = save_session(
        &draft,
        SaveMode::Customize,
        &session,
        &s.installed(),
        &s.user,
        BUILT_IN_IDS,
        &Example::new(TODAY),
    )
    .unwrap();
    assert_eq!(saved.template.files, files_before);
    assert_eq!(saved.template.resolve, draft.resolve);
    assert_eq!(
        std::fs::read(saved.dir.join("files/Grade.drp")).unwrap(),
        b"drp bytes"
    );

    // Edited again (a new name): the .drp comes along from the saved template.
    let mine = s.get("video", false);
    let mut again = mine.loaded.template.clone();
    again.resolve.as_mut().unwrap().name = Some("{jobCode}".into());
    let next = s.save(&again, SaveMode::Edit, &mine).unwrap();
    assert_eq!(next.resolve.unwrap().name.as_deref(), Some("{jobCode}"));
    assert!(s
        .get("video", false)
        .loaded
        .files
        .contains("files/Grade.drp"));
}

/// A `.drp` dropped on the editor: only a .drp file is taken, its name is cleaned, and a
/// second one with the same name gets a place of its own. Nothing is written.
#[test]
fn a_dropped_drp_is_checked_and_staged() {
    use nest_lib::template_edit::{dropped_drp, EditorSession};

    let s = Setup::new();
    let dir = s._tmp.path().join("drops");
    std::fs::create_dir_all(dir.join("Folder.drp")).unwrap();
    std::fs::write(dir.join("그레이드 Grade.DRP"), b"drp").unwrap();
    std::fs::write(dir.join("notes.txt"), b"hi").unwrap();

    let (name, on_disk) = dropped_drp(&dir.join("그레이드 Grade.DRP")).unwrap();
    assert_eq!(name, "그레이드 Grade.drp");
    assert_eq!(on_disk.rel, "그레이드 Grade.DRP");
    assert_eq!(on_disk.size, 3);
    assert!(dropped_drp(&dir.join("notes.txt")).is_err());
    assert!(
        dropped_drp(&dir.join("Folder.drp")).is_err(),
        "a folder isn't a .drp"
    );
    assert!(dropped_drp(&dir.join("missing.drp")).is_err());

    let video = s.get("video", true);
    let mut session = EditorSession::from_template(&video);
    let first = session
        .add_dropped_drp(&dir.join("그레이드 Grade.DRP"))
        .unwrap();
    let second = session
        .add_dropped_drp(&dir.join("그레이드 Grade.DRP"))
        .unwrap();
    assert_eq!(first, "files/그레이드 Grade.drp");
    assert_eq!(second, "files/2/그레이드 Grade.drp");
    assert!(session.add_dropped_drp(&dir.join("notes.txt")).is_err());
    let entries: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
    assert_eq!(entries.len(), 3, "nothing written next to the drop");
}

/// A saved version is always higher than any this computer has had of that id, even after
/// "go back to original" or a delete (sync would otherwise call the new save "the same version").
#[test]
fn a_new_save_is_always_above_every_version_this_computer_has_had() {
    use nest_lib::template_edit::{save_session, EditorSession};
    use std::collections::BTreeMap;

    let s = Setup::new();
    let video = s.get("video", true);
    let draft = video.loaded.template.clone();
    let customize = |seen: &BTreeMap<String, u32>| {
        let mut session = EditorSession::from_template(&s.get("video", true));
        session.seen = seen.clone();
        save_session(
            &draft,
            SaveMode::Customize,
            &session,
            &s.installed(),
            &s.user,
            BUILT_IN_IDS,
            &Example::new(TODAY),
        )
        .unwrap()
    };
    let first = customize(&BTreeMap::new()).template.version;
    assert_eq!(first, video.loaded.template.version + 1);

    // Edit twice more, then "go back to original" (your folders removed; the bin isn't used in
    // tests). The built-in is all that's left.
    let mut seen: BTreeMap<String, u32> = [("video".to_string(), first + 5)].into();
    for t in s.installed().into_iter().filter(|t| !t.built_in) {
        std::fs::remove_dir_all(&t.loaded.dir).unwrap();
    }
    assert!(s.installed().iter().all(|t| t.built_in));
    // Without the bookkeeping the old number would come back (the bug); with it, it can't.
    assert_eq!(customize(&BTreeMap::new()).template.version, first);
    for t in s.installed().into_iter().filter(|t| !t.built_in) {
        std::fs::remove_dir_all(&t.loaded.dir).unwrap();
    }
    let again = customize(&seen).template.version;
    assert_eq!(again, first + 6);

    // Editing one of yours also goes above everything seen.
    seen.insert("video".to_string(), again + 3);
    let mine = s.get("video", false);
    let mut session = EditorSession::from_template(&mine);
    session.seen = seen.clone();
    let edited = save_session(
        &mine.loaded.template,
        SaveMode::Edit,
        &session,
        &s.installed(),
        &s.user,
        BUILT_IN_IDS,
        &Example::new(TODAY),
    )
    .unwrap();
    assert_eq!(edited.template.version, again + 4);

    // A new template never takes an id this computer has had before (deleted since).
    let general = s.get("general", true);
    let mut t = general.loaded.template.clone();
    t.name = "Old Wedding".into();
    let mut session = EditorSession::from_template(&general);
    session.seen = [("old-wedding".to_string(), 4)].into();
    let made = save_session(
        &t,
        SaveMode::New,
        &session,
        &s.installed(),
        &s.user,
        BUILT_IN_IDS,
        &Example::new(TODAY),
    )
    .unwrap();
    assert_eq!(made.template.id, "old-wedding-2");
    assert_eq!(made.template.version, 1);
}

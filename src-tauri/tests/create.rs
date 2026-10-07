//! Create: disk matches the Plan exactly, and rollback only undoes this run (spec §9 M2).
//! Temp dirs only: never a real jobs folder.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use nest_lib::apply::{apply_plan, apply_plan_with};
use nest_lib::manifest::{is_hidden, read_json_object};
use nest_lib::names::DEFAULT_CODE_PATTERN;
use nest_lib::plan::{plan_project, ClientValue, Date, FieldValue, Plan, PlanContext, Values};
use nest_lib::template::{load_dir, LoadedTemplate};

const TEMPLATE: &str = r#"{
    "schema": 1, "id": "test-mv", "version": 2, "name": "Test MV",
    "fields": [
        { "key": "client", "label": "Client", "type": "client", "required": true },
        { "key": "artist", "label": "Artist", "type": "text", "required": true },
        { "key": "song", "label": "Song", "type": "text", "required": true },
        { "key": "ratios", "label": "Ratios", "type": "multi", "options": ["16x9", "9x16"], "default": ["16x9", "9x16"] }
    ],
    "folderName": "{yymm}_{jobCode}_{artist}_{song}",
    "tree": ["01_REF", "02_FOOTAGE", { "name": "07_DELIVERY/{item}", "each": "ratios" }],
    "files": [
        { "from": "starter/NOTES.md", "to": "NOTES.md", "fill": true },
        { "from": "starter/still.bin", "to": "01_REF/still.bin", "fill": true },
        { "from": "starter/luts/", "to": "04_LUTS/" }
    ]
}"#;

/// A template folder with a text starter, a binary starter and a LUT folder.
fn template(dir: &Path) -> LoadedTemplate {
    fs::create_dir_all(dir.join("starter/luts/Film")).unwrap();
    fs::write(dir.join("test.nest.json"), TEMPLATE).unwrap();
    fs::write(
        dir.join("starter/NOTES.md"),
        "# {jobCode}\n{artist} - {song}\n{unknown}\n",
    )
    .unwrap();
    fs::write(
        dir.join("starter/still.bin"),
        [0xff, 0xfe, b'{', b'j', 0x00, 0x80],
    )
    .unwrap();
    fs::write(dir.join("starter/luts/Rec709.cube"), "LUT_3D_SIZE 2\n").unwrap();
    fs::write(dir.join("starter/luts/Film/Kodak.cube"), "LUT_3D_SIZE 2\n").unwrap();
    load_dir(dir).unwrap()
}

fn ctx(jobs: &Path) -> PlanContext {
    PlanContext {
        jobs_root: jobs.to_path_buf(),
        id: "00000000-0000-4000-8000-000000000042".into(),
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

fn values(artist: &str, song: &str) -> Values {
    Values::from([
        (
            "client".into(),
            FieldValue::Client(ClientValue {
                name: "Vertex Studio".into(),
                code: "VX".into(),
            }),
        ),
        ("artist".into(), FieldValue::Text(artist.into())),
        ("song".into(), FieldValue::Text(song.into())),
    ])
}

struct Setup {
    _tmp: tempfile::TempDir,
    jobs: PathBuf,
    template: LoadedTemplate,
}

/// Fresh jobs root holding one unrelated folder (with a file) that must never be touched.
fn setup() -> Setup {
    let tmp = tempfile::tempdir().unwrap();
    let jobs = tmp.path().join("Jobs");
    fs::create_dir_all(jobs.join("Existing")).unwrap();
    fs::write(jobs.join("Existing/keep.txt"), "user file").unwrap();
    let template = template(&tmp.path().join("tpl"));
    Setup {
        _tmp: tmp,
        jobs,
        template,
    }
}

fn plan(s: &Setup, v: &Values) -> Plan {
    let p = plan_project(&s.template, v, &ctx(&s.jobs));
    assert!(p.can_create(), "{:?}", p.issues);
    p
}

/// Every folder and file under `root`, `/`-separated.
fn walk(root: &Path) -> (BTreeSet<String>, BTreeSet<String>) {
    fn go(root: &Path, dir: &Path, dirs: &mut BTreeSet<String>, files: &mut BTreeSet<String>) {
        for e in fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let rel: Vec<String> = p
                .strip_prefix(root)
                .unwrap()
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            if p.is_dir() {
                dirs.insert(rel.join("/"));
                go(root, &p, dirs, files);
            } else {
                files.insert(rel.join("/"));
            }
        }
    }
    let (mut dirs, mut files) = (BTreeSet::new(), BTreeSet::new());
    go(root, root, &mut dirs, &mut files);
    (dirs, files)
}

fn jobs_untouched(s: &Setup) {
    let names: Vec<_> = fs::read_dir(&s.jobs)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["Existing"], "only the unrelated folder remains");
    assert_eq!(
        fs::read_to_string(s.jobs.join("Existing/keep.txt")).unwrap(),
        "user file"
    );
}

#[test]
fn disk_matches_plan_exactly() {
    let s = setup();
    let p = plan(&s, &values("KIRA", "Summer Nights"));
    apply_plan(&p, &s.template.dir).unwrap();

    let (dirs, files) = walk(&p.root);
    let expected_dirs: BTreeSet<String> = p.folders.iter().cloned().collect();
    let mut expected_files: BTreeSet<String> = p.files.iter().map(|f| f.to.clone()).collect();
    expected_files.insert(".project.json".into());
    assert_eq!(dirs, expected_dirs);
    assert_eq!(files, expected_files);

    // Text starter filled; unknown variables left alone.
    assert_eq!(
        fs::read_to_string(p.root.join("NOTES.md")).unwrap(),
        "# VX-L01\nKIRA - Summer Nights\n{unknown}\n"
    );
    // Binary starter with fill: copied byte for byte.
    assert_eq!(
        fs::read(p.root.join("01_REF/still.bin")).unwrap(),
        [0xff, 0xfe, b'{', b'j', 0x00, 0x80]
    );
    // Manifest: hidden, equals the plan's.
    let manifest = p.root.join(".project.json");
    assert!(is_hidden(&manifest).unwrap());
    assert_eq!(read_json_object(&manifest).unwrap(), p.manifest);
    assert_eq!(p.root.file_name().unwrap(), "2609_VX-L01_KIRA_SummerNights");
}

#[test]
fn failure_at_any_step_rolls_back_only_this_run() {
    let s = setup();
    let p = plan(&s, &values("KIRA", "Summer Nights"));
    let steps = 1 + p.folders.len() + p.files.len() + 1;
    for fail_at in 0..steps {
        let mut hook = |step: usize| {
            if step == fail_at {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "simulated"))
            } else {
                Ok(())
            }
        };
        let err = apply_plan_with(&p, &s.template.dir, &mut hook).unwrap_err();
        assert!(
            err.left_behind.is_empty(),
            "step {fail_at}: {:?}",
            err.left_behind
        );
        assert!(err.message.contains("permission denied"), "{}", err.message);
        jobs_untouched(&s);
    }
}

#[test]
fn a_file_someone_else_adds_survives_rollback() {
    let s = setup();
    let p = plan(&s, &values("KIRA", "Song"));
    let root = p.root.clone();
    let mut hook = |step: usize| {
        if step == 3 {
            // Meanwhile, a sync app drops a file into a folder we created.
            fs::write(root.join("01_REF/theirs.txt"), "not ours").unwrap();
        }
        if step == 5 {
            return Err(io::Error::other("simulated"));
        }
        Ok(())
    };
    let err = apply_plan_with(&p, &s.template.dir, &mut hook).unwrap_err();
    assert_eq!(
        fs::read_to_string(root.join("01_REF/theirs.txt")).unwrap(),
        "not ours"
    );
    assert_eq!(err.left_behind, [root.join("01_REF"), root.clone()]);
    assert!(
        err.message.contains("couldn't be cleaned up"),
        "{}",
        err.message
    );
}

#[test]
fn existing_project_folder_is_never_merged_into() {
    let s = setup();
    let p = plan(&s, &values("KIRA", "Song"));
    fs::create_dir(&p.root).unwrap();
    fs::write(p.root.join("mine.txt"), "keep").unwrap();
    let err = apply_plan(&p, &s.template.dir).unwrap_err();
    assert!(err.root_exists);
    let names: Vec<_> = fs::read_dir(&p.root)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["mine.txt"], "nothing added, nothing removed");
}

#[test]
fn missing_jobs_root_is_an_error_not_created() {
    let s = setup();
    let mut c = ctx(&s.jobs.join("Unplugged NAS"));
    c.jobs_root = s.jobs.join("Unplugged NAS");
    let p = plan_project(&s.template, &values("KIRA", "Song"), &c);
    assert!(apply_plan(&p, &s.template.dir).is_err());
    assert!(!s.jobs.join("Unplugged NAS").exists());
}

#[test]
fn creating_twice_gives_a_new_code_never_a_merge() {
    let s = setup();
    let v = values("KIRA", "Song");
    let first = plan(&s, &v);
    apply_plan(&first, &s.template.dir).unwrap();

    let scan = nest_lib::index::scan_root(&s.jobs).unwrap();
    let mut c = ctx(&s.jobs);
    c.existing_codes = scan.codes;
    c.existing_folders = scan.folders;
    let second = plan_project(&s.template, &v, &c);
    assert_eq!(second.job_code, "VX-L02");
    apply_plan(&second, &s.template.dir).unwrap();
    assert!(first.root.is_dir() && second.root.is_dir());
}

#[test]
fn korean_chinese_and_nfd_names_create_correctly() {
    let s = setup();
    for (artist, song, folder) in [
        ("아이유", "좋은 날", "2609_VX-L01_아이유_좋은날"),
        ("周杰倫", "晴天", "2609_VX-L01_周杰倫_晴天"),
        ("Beyonce\u{301}", "Halo", "2609_VX-L01_Beyonc\u{e9}_Halo"),
    ] {
        let p = plan(&s, &values(artist, song));
        assert_eq!(p.folder_name, folder);
        apply_plan(&p, &s.template.dir).unwrap();
        let names: Vec<String> = fs::read_dir(&s.jobs)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&folder.to_string()), "{names:?}");
        fs::remove_dir_all(&p.root).unwrap(); // test cleanup of our own temp dir
    }
}

#[cfg(unix)]
#[test]
fn starter_link_pointing_outside_the_template_is_refused() {
    let s = setup();
    let secret = s.jobs.parent().unwrap().join("secret.txt");
    fs::write(&secret, "private").unwrap();
    let notes = s.template.dir.join("starter/NOTES.md");
    fs::remove_file(&notes).unwrap();
    std::os::unix::fs::symlink(&secret, &notes).unwrap();
    let p = plan(&s, &values("KIRA", "Song"));
    let err = apply_plan(&p, &s.template.dir).unwrap_err();
    assert!(
        err.message.contains("isn't a regular file"),
        "{}",
        err.message
    );
    jobs_untouched(&s);
}

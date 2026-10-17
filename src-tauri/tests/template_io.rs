//! Template import / export / duplicate (M4). Temp dirs only.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use nest_lib::template_io::{
    check_import_file, drop_kind, duplicate, export, folder_count, import, info, load_installed,
    removal, without_built_ins, DropKind, Installed, Limits, Removal, LIMITS,
};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

fn template_json(id: &str, version: u32, name: &str) -> String {
    format!(
        r#"{{"schema":1,"id":"{id}","version":{version},"name":"{name}",
        "fields":[{{"key":"client","label":"Billed to","type":"client","required":true}},
                  {{"key":"name","label":"Name","type":"text","required":true}}],
        "folderName":"{{yymm}}_{{name}}","tree":["01_REF"],
        "files":[{{"from":"starter/README.txt","to":"README.txt"}}]}}"#
    )
}

/// A zip with the given (name, contents) entries.
fn zip_with(path: &Path, entries: &[(&str, &[u8])]) {
    let mut z = ZipWriter::new(fs::File::create(path).unwrap());
    for (name, data) in entries {
        z.start_file(*name, SimpleFileOptions::default()).unwrap();
        z.write_all(data).unwrap();
    }
    z.finish().unwrap();
}

struct Setup {
    tmp: tempfile::TempDir,
    user: PathBuf,
    bundled: PathBuf,
}

fn setup() -> Setup {
    let tmp = tempfile::tempdir().unwrap();
    let user = tmp.path().join("user");
    let bundled = tmp.path().join("bundled");
    fs::create_dir_all(&bundled).unwrap();
    Setup { tmp, user, bundled }
}

impl Setup {
    fn installed(&self) -> Vec<Installed> {
        load_installed(&self.user, &self.bundled, &["project", "grading"])
    }
    fn pkg(&self, name: &str) -> PathBuf {
        self.tmp.path().join(name)
    }
    /// Only the import-visible folders (hidden temp folders excluded).
    fn user_folders(&self) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(&self.user)
            .map(|it| {
                it.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }
}

#[test]
fn imports_a_package_with_a_top_folder_and_starter_files() {
    let s = setup();
    let pkg = s.pkg("project.nesttemplate");
    zip_with(
        &pkg,
        &[
            (
                "project/project.nest.json",
                template_json("project", 1, "Project").as_bytes(),
            ),
            ("project/starter/README.txt", b"read me"),
            ("__MACOSX/project/._README.txt", b"junk"),
        ],
    );
    let t = import(&pkg, &s.user, &s.installed(), LIMITS).unwrap();
    assert_eq!(t.template.id, "project");
    assert!(t.files.contains("starter/README.txt"));
    assert_eq!(s.user_folders(), ["project"], "no temp folders left");
}

#[test]
fn refuses_unsafe_packages_and_leaves_nothing_behind() {
    let s = setup();
    let good = template_json("x", 1, "X");
    type Case<'a> = (&'a str, Vec<(&'a str, &'a [u8])>, &'a str); // (label, entries, expected)
    let cases: Vec<Case> = vec![
        (
            "slip",
            vec![("x.nest.json", good.as_bytes()), ("../evil.txt", b"no")],
            "outside the package",
        ),
        (
            "abs",
            vec![("x.nest.json", good.as_bytes()), ("/etc/evil", b"no")],
            "absolute",
        ),
        (
            "backslash",
            vec![("x.nest.json", good.as_bytes()), ("a\\b.txt", b"no")],
            "uses \\",
        ),
        (
            "reserved",
            vec![("x.nest.json", good.as_bytes()), ("starter/CON.txt", b"no")],
            "isn't allowed",
        ),
        (
            "trailing",
            vec![("x.nest.json", good.as_bytes()), ("starter/name.", b"no")],
            "isn't allowed",
        ),
        (
            "case",
            vec![
                ("x.nest.json", good.as_bytes()),
                ("A.txt", b"1"),
                ("a.txt", b"2"),
            ],
            "capitals",
        ),
        (
            "ads",
            vec![("x.nest.json", good.as_bytes()), ("a.txt:stream", b"no")],
            "has a :",
        ),
        ("nojson", vec![("readme.txt", b"hi")], "no .nest.json"),
        (
            "invalid",
            vec![("x.nest.json", br#"{"schema":1}"#)],
            "can't be used",
        ),
    ];
    for (label, entries, expected) in cases {
        let pkg = s.pkg(&format!("{label}.nesttemplate"));
        zip_with(&pkg, &entries);
        let err = import(&pkg, &s.user, &s.installed(), LIMITS).unwrap_err();
        assert!(err.contains(expected), "{label}: {err}");
        assert!(
            s.user_folders().is_empty(),
            "{label}: left {:?}",
            s.user_folders()
        );
    }
    assert!(!s.tmp.path().join("evil.txt").exists());
}

#[test]
fn refuses_links_inside_the_zip() {
    let s = setup();
    let pkg = s.pkg("link.nesttemplate");
    let mut z = ZipWriter::new(fs::File::create(&pkg).unwrap());
    z.start_file("x.nest.json", SimpleFileOptions::default())
        .unwrap();
    z.write_all(template_json("x", 1, "X").as_bytes()).unwrap();
    z.add_symlink(
        "starter/README.txt",
        "/etc/passwd",
        SimpleFileOptions::default(),
    )
    .unwrap();
    z.finish().unwrap();
    let err = import(&pkg, &s.user, &s.installed(), LIMITS).unwrap_err();
    assert!(err.contains("link"), "{err}");
    assert!(s.user_folders().is_empty());
}

#[test]
fn zip_bomb_and_file_count_are_capped_by_real_bytes() {
    let s = setup();
    let pkg = s.pkg("big.nesttemplate");
    let big = vec![0u8; 2 * 1024 * 1024]; // compresses to almost nothing
    zip_with(
        &pkg,
        &[
            ("x.nest.json", template_json("x", 1, "X").as_bytes()),
            ("starter/big.bin", &big),
        ],
    );
    let tight = Limits {
        max_bytes: 1024 * 1024,
        max_files: 100,
    };
    let err = import(&pkg, &s.user, &s.installed(), tight).unwrap_err();
    assert!(err.contains("too big"), "{err}");
    let few = Limits {
        max_bytes: u64::MAX,
        max_files: 1,
    };
    let err = import(&pkg, &s.user, &s.installed(), few).unwrap_err();
    assert!(err.contains("too many files"), "{err}");
    assert!(s.user_folders().is_empty());
}

#[test]
fn newer_version_installs_alongside_and_older_is_superseded() {
    let s = setup();
    let v1 = s.pkg("v1.json");
    fs::write(&v1, template_json("project", 1, "Project")).unwrap();
    import(&v1, &s.user, &s.installed(), LIMITS).unwrap();

    // Same version again: refused, nothing overwritten.
    let err = import(&v1, &s.user, &s.installed(), LIMITS).unwrap_err();
    assert!(err.contains("already have"), "{err}");

    let v2 = s.pkg("v2.json");
    fs::write(&v2, template_json("project", 2, "Project")).unwrap();
    import(&v2, &s.user, &s.installed(), LIMITS).unwrap();
    assert_eq!(s.user_folders(), ["project", "project-v2"]);

    let installed = s.installed();
    let offered: Vec<u32> = installed
        .iter()
        .filter(|t| !t.superseded)
        .map(|t| t.loaded.template.version)
        .collect();
    assert_eq!(offered, [2], "only the newest is offered");
    assert!(installed
        .iter()
        .any(|t| t.superseded && t.loaded.template.version == 1));
}

#[test]
fn yours_shadows_a_builtin_with_the_same_id() {
    let s = setup();
    fs::create_dir_all(s.bundled.join("project")).unwrap();
    fs::write(
        s.bundled.join("project/project.nest.json"),
        template_json("project", 5, "Built in"),
    )
    .unwrap();
    let mine = s.pkg("mine.json");
    fs::write(&mine, template_json("project", 1, "Mine")).unwrap();
    import(&mine, &s.user, &s.installed(), LIMITS).unwrap();
    let installed = s.installed();
    let offered: Vec<&str> = installed
        .iter()
        .filter(|t| !t.superseded)
        .map(|t| t.loaded.template.name.as_str())
        .collect();
    assert_eq!(offered, ["Mine"]);
    // The built-in is still listed, as the original yours replaces.
    let original = installed.iter().find(|t| t.built_in).unwrap();
    assert!(original.replaced && original.superseded);
    assert_eq!(original.loaded.template.name, "Built in");
}

/// An app update can leave old template folders in the bundle (Windows installs over the old
/// copy). Only current built-ins, each in its own folder, are loaded.
#[test]
fn leftover_bundled_folders_are_ignored() {
    let s = setup();
    for (folder, id) in [
        ("grading-mv", "grading-mv"),
        ("old-copy", "grading"),
        ("grading", "grading"),
    ] {
        fs::create_dir_all(s.bundled.join(folder)).unwrap();
        fs::write(
            s.bundled.join(folder).join(format!("{id}.nest.json")),
            template_json(id, 1, folder),
        )
        .unwrap();
    }
    let loaded: Vec<String> = s
        .installed()
        .iter()
        .map(|t| t.loaded.template.name.clone())
        .collect();
    assert_eq!(loaded, ["grading"]);
}

#[test]
fn duplicate_gets_a_new_id_and_export_round_trips() {
    let s = setup();
    fs::create_dir_all(s.bundled.join("grading/starter")).unwrap();
    fs::write(
        s.bundled.join("grading/grading.nest.json"),
        template_json("grading", 3, "Grading"),
    )
    .unwrap();
    fs::write(s.bundled.join("grading/starter/README.txt"), "hello").unwrap();
    let builtin = s.installed().remove(0);
    assert!(builtin.built_in);

    let copy = duplicate(&builtin.loaded, &s.user, &s.installed()).unwrap();
    assert_eq!(copy.template.id, "grading-copy");
    assert_eq!(copy.template.name, "Grading (copy)");
    assert!(copy.files.contains("starter/README.txt"));
    let again = duplicate(&builtin.loaded, &s.user, &s.installed()).unwrap();
    assert_eq!(again.template.id, "grading-copy-2");
    assert_eq!(s.installed().len(), 3, "the built-in is still offered");

    // Export → import elsewhere gives the same template and files.
    let pkg = s.pkg("export.nesttemplate");
    export(&copy, &pkg).unwrap();
    let other = setup();
    let back = import(&pkg, &other.user, &other.installed(), LIMITS).unwrap();
    assert_eq!(back.template, copy.template);
    assert_eq!(back.files, copy.files);
    assert_eq!(
        fs::read_to_string(back.dir.join("starter/README.txt")).unwrap(),
        "hello"
    );
    let leftovers: Vec<_> = fs::read_dir(s.tmp.path())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "no export temp files left");
}

/// Templates list: Remove hides one of Nest's, bins yours, and bins your version of one of
/// Nest's while hiding the original (so it doesn't come back in its place).
#[test]
fn remove_hides_nests_and_bins_yours() {
    let s = setup();
    for id in ["project", "grading"] {
        fs::create_dir_all(s.bundled.join(id)).unwrap();
        fs::write(
            s.bundled.join(format!("{id}/{id}.nest.json")),
            template_json(id, 1, "Built in"),
        )
        .unwrap();
    }
    let mine = s.pkg("mine.json");
    fs::write(&mine, template_json("project", 2, "Mine")).unwrap();
    import(&mine, &s.user, &s.installed(), LIMITS).unwrap();
    let other = s.pkg("other.json");
    fs::write(&other, template_json("acme-film", 1, "Acme film")).unwrap();
    import(&other, &s.user, &s.installed(), LIMITS).unwrap();
    let installed = s.installed();
    let find = |id: &str, built_in: bool| {
        installed
            .iter()
            .find(|t| t.loaded.template.id == id && t.built_in == built_in)
            .unwrap()
    };
    assert_eq!(
        removal(find("grading", true), &installed),
        Removal::Hide("grading".into())
    );
    assert_eq!(
        removal(find("acme-film", false), &installed),
        Removal::Bin {
            id: "acme-film".into(),
            hide_original: false
        }
    );
    assert_eq!(
        removal(find("project", false), &installed),
        Removal::Bin {
            id: "project".into(),
            hide_original: true
        }
    );
}

#[test]
fn bring_back_unhides_only_nests_templates() {
    let hidden = vec![
        "video".to_string(),
        "acme-film".to_string(),
        "blank".to_string(),
    ];
    assert_eq!(
        without_built_ins(&hidden, &["blank", "general", "video"]),
        ["acme-film"]
    );
}

#[test]
fn folder_count_includes_implied_parents_once() {
    let s = setup();
    fs::create_dir_all(s.bundled.join("project")).unwrap();
    fs::write(
        s.bundled.join("project/project.nest.json"),
        r#"{"schema":1,"id":"project","version":1,"name":"P",
        "fields":[{"key":"client","label":"Billed to","type":"client","required":true},
                  {"key":"name","label":"Name","type":"text","required":true},
                  {"key":"gfx","label":"Graphics","type":"bool"}],
        "folderName":"{yymm}_{name}",
        "tree":["01_REF","02_FOOTAGE/A_CAM","02_FOOTAGE/B_CAM",{"name":"03_GFX","when":"gfx"}]}"#,
    )
    .unwrap();
    let installed = s.installed();
    let t = &installed[0];
    // 01_REF, 02_FOOTAGE, A_CAM, B_CAM, 03_GFX.
    assert_eq!(folder_count(&t.loaded.template), 5);
    assert_eq!(info(t, &[]).folders, 5);
}

#[test]
fn a_dropped_template_file_is_checked_then_installs_like_import() {
    let s = setup();
    let pkg = s.pkg("Acme Film.NESTTEMPLATE");
    zip_with(
        &pkg,
        &[(
            "acme-film.nest.json",
            template_json("acme-film", 1, "Acme film").as_bytes(),
        )],
    );
    assert_eq!(drop_kind(&pkg), DropKind::Template);
    check_import_file(&pkg).unwrap();
    let loaded = import(&pkg, &s.user, &s.installed(), LIMITS).unwrap();
    assert_eq!(loaded.template.name, "Acme film");
    assert!(s
        .installed()
        .iter()
        .any(|t| t.loaded.template.id == "acme-film"));

    // A single .json works the same way.
    let json = s.pkg("single.json");
    fs::write(&json, template_json("single", 1, "Single")).unwrap();
    assert_eq!(drop_kind(&json), DropKind::Template);
    check_import_file(&json).unwrap();
}

#[test]
fn a_dropped_path_that_isnt_a_template_file_gets_a_plain_message() {
    let s = setup();
    // Wrong extension: refused before anything is opened, nothing installed.
    let txt = s.pkg("notes.txt");
    fs::write(&txt, "hello").unwrap();
    assert_eq!(drop_kind(&txt), DropKind::Other);
    let e = check_import_file(&txt).unwrap_err();
    assert_eq!(e, "That's not a Nest template file (.nesttemplate).");

    // Gone (moved or renamed after the drop).
    let gone = s.pkg("gone.nesttemplate");
    assert_eq!(drop_kind(&gone), DropKind::Other);
    let e = check_import_file(&gone).unwrap_err();
    assert!(e.starts_with("Nest couldn't find that file"), "{e}");

    // A folder named like a template is a folder, never imported as a file.
    let dir = s.pkg("odd.nesttemplate");
    fs::create_dir(&dir).unwrap();
    assert_eq!(drop_kind(&dir), DropKind::Folder);
    assert!(check_import_file(&dir).is_err());
    assert!(s.user_folders().is_empty());

    // Korean folder names are folders too.
    let ko = s.pkg("260301_광고");
    fs::create_dir(&ko).unwrap();
    assert_eq!(drop_kind(&ko), DropKind::Folder);
}

fn with_resolve(id: &str, drp: &str) -> String {
    let mut t: serde_json::Value = serde_json::from_str(&template_json(id, 1, "Grade")).unwrap();
    t["resolve"] = serde_json::json!({ "drp": drp, "name": "{name}", "folder": "{client}/2026" });
    t.to_string()
}

#[test]
fn a_resolve_project_travels_with_export_import_and_duplicate() {
    let s = setup();
    let dir = s.user.join("grade");
    fs::create_dir_all(dir.join("starter")).unwrap();
    fs::create_dir_all(dir.join("files")).unwrap();
    fs::write(dir.join("starter/README.txt"), "hello").unwrap();
    fs::write(dir.join("files/Grade.drp"), b"drp bytes").unwrap();
    fs::write(
        dir.join("grade.nest.json"),
        with_resolve("grade", "files/Grade.drp"),
    )
    .unwrap();
    let mine = s.installed().remove(0);
    let r = mine.loaded.template.resolve.clone().unwrap();
    assert_eq!(r.drp, "files/Grade.drp");
    assert!(r.on, "ticked by default");

    let pkg = s.pkg("grade.nesttemplate");
    export(&mine.loaded, &pkg).unwrap();
    let other = setup();
    let back = import(&pkg, &other.user, &other.installed(), LIMITS).unwrap();
    assert_eq!(back.template, mine.loaded.template);
    assert_eq!(
        fs::read(back.dir.join("files/Grade.drp")).unwrap(),
        b"drp bytes"
    );

    let copy = duplicate(&mine.loaded, &s.user, &s.installed()).unwrap();
    assert_eq!(copy.template.resolve, mine.loaded.template.resolve);
    assert!(copy.files.contains("files/Grade.drp"));
}

#[test]
fn a_package_whose_resolve_project_is_missing_is_refused() {
    let s = setup();
    let pkg = s.pkg("broken.nesttemplate");
    zip_with(
        &pkg,
        &[
            (
                "grade/grade.nest.json",
                with_resolve("grade", "files/Grade.drp").as_bytes(),
            ),
            ("grade/starter/README.txt", b"x"),
        ],
    );
    let err = import(&pkg, &s.user, &s.installed(), LIMITS).unwrap_err();
    assert!(
        err.contains("Resolve project \"files/Grade.drp\" is missing"),
        "{err}"
    );
    assert!(s.user_folders().is_empty(), "nothing installed");
}

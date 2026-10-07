//! Template import / export / duplicate (M4). Temp dirs only.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use nest_lib::template_io::{duplicate, export, import, load_installed, Installed, Limits, LIMITS};
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

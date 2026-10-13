//! Make a template from a folder (PLAN.md 2026-10-13): read a folder, save it as one of your
//! templates, and check the folder itself is never changed. Temp dirs only.

use std::fs;
use std::path::{Path, PathBuf};

use nest_lib::plan::Date;
use nest_lib::template::load_dir;
use nest_lib::template_edit::{preview, save_from_folder, Example};
use nest_lib::template_from_folder::{read_folder, Keep, MAX_FILE_BYTES};
use nest_lib::template_io::{load_installed, Installed, BUILT_IN_IDS};

const TODAY: Date = Date {
    year: 2026,
    month: 3,
    day: 2,
};

fn bundled_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("templates")
}

fn file(root: &Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, bytes).unwrap();
}

/// A job folder the way an editor might keep one.
fn job(root: &Path) {
    fs::create_dir_all(root.join("01_BRIEF")).unwrap();
    fs::create_dir_all(root.join("02_FOOTAGE/A_CAM")).unwrap();
    fs::create_dir_all(root.join("03_PROJECT/Adobe Premiere Pro Auto-Save")).unwrap();
    file(root, "01_BRIEF/notes.md", b"# {name}\n");
    file(root, "03_PROJECT/edit.prproj", &[0x1f, 0x8b, 0, 1, 2, 3]);
    file(root, "02_FOOTAGE/A_CAM/clip.mov", b"small clip");
}

/// Every path and its bytes (folders as empty), sorted.
fn snapshot(p: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = vec![];
    let mut stack = vec![p.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).unwrap().flatten() {
            let path = e.path();
            if path.is_dir() {
                out.push((path.clone(), vec![]));
                stack.push(path);
            } else {
                out.push((path.clone(), fs::read(&path).unwrap()));
            }
        }
    }
    out.sort();
    out
}

fn installed(user: &Path) -> Vec<Installed> {
    load_installed(user, &bundled_dir(), BUILT_IN_IDS)
}

#[test]
fn a_folder_becomes_a_template_and_is_left_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("Acme Job");
    let user = tmp.path().join("templates");
    job(&src);
    let before = snapshot(&src);

    let folder = read_folder(&src, Keep::FoldersAndFiles, |_| None).unwrap();
    let ex = Example::new(TODAY);
    let saved = save_from_folder(
        &folder.template,
        &folder,
        &installed(&user),
        &user,
        BUILT_IN_IDS,
        &ex,
    )
    .unwrap();

    assert_eq!(before, snapshot(&src), "the source folder must not change");
    assert_eq!(saved.template.id, "acme-job");
    assert_eq!(saved.template.name, "Acme Job");
    // The files that came along are byte-for-byte copies; the auto-save folder stayed behind.
    assert_eq!(
        fs::read(saved.dir.join("03_PROJECT/edit.prproj")).unwrap(),
        [0x1f, 0x8b, 0, 1, 2, 3]
    );
    assert!(saved.dir.join("02_FOOTAGE/A_CAM/clip.mov").is_file());
    assert!(!saved
        .dir
        .join("03_PROJECT/Adobe Premiere Pro Auto-Save")
        .exists());
    // It loads back like any template, and plans the same folders and files.
    let again = load_dir(&saved.dir).unwrap();
    let p = preview(&again.template, &again, &ex);
    assert!(p.problems.is_empty(), "{:?}", p.problems);
    assert_eq!(p.folder_name, "260302_SUMMER_CAMPAIGN");
    for f in ["01_BRIEF", "02_FOOTAGE/A_CAM", "03_PROJECT"] {
        assert!(
            p.folders.iter().any(|x| x.ends_with(f)),
            "{f} in {:?}",
            p.folders
        );
    }
    assert!(!p.folders.iter().any(|x| x.contains("Auto-Save")));
    for f in ["01_BRIEF/notes.md", "03_PROJECT/edit.prproj"] {
        assert!(
            p.files.iter().any(|x| x.ends_with(f)),
            "{f} in {:?}",
            p.files
        );
    }
    // No temporary folder is left behind.
    let leftovers: Vec<_> = fs::read_dir(&user)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with('.'))
        .collect();
    assert!(leftovers.is_empty());
}

#[test]
fn files_removed_in_the_editor_are_not_copied() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("job");
    let user = tmp.path().join("templates");
    job(&src);
    let folder = read_folder(&src, Keep::FoldersAndFiles, |_| None).unwrap();
    let mut draft = folder.template.clone();
    draft.files.retain(|f| !f.from.ends_with(".mov"));
    let saved = save_from_folder(
        &draft,
        &folder,
        &installed(&user),
        &user,
        BUILT_IN_IDS,
        &Example::new(TODAY),
    )
    .unwrap();
    assert!(!saved.dir.join("02_FOOTAGE/A_CAM/clip.mov").exists());
    assert!(saved.dir.join("01_BRIEF/notes.md").is_file());
}

#[test]
fn a_file_that_grew_since_reading_is_refused_and_nothing_is_saved() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("job");
    let user = tmp.path().join("templates");
    job(&src);
    let folder = read_folder(&src, Keep::FoldersAndFiles, |_| None).unwrap();
    fs::File::options()
        .write(true)
        .open(src.join("02_FOOTAGE/A_CAM/clip.mov"))
        .unwrap()
        .set_len(MAX_FILE_BYTES + 1)
        .unwrap();
    let e = save_from_folder(
        &folder.template,
        &folder,
        &installed(&user),
        &user,
        BUILT_IN_IDS,
        &Example::new(TODAY),
    )
    .unwrap_err();
    assert!(e.contains("changed since it was added"), "{e}");
    assert_eq!(fs::read_dir(&user).unwrap().count(), 0);
}

#[test]
fn a_nest_project_comes_back_as_a_template_with_blanks() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("251013_HOLIDAY_AD");
    let user = tmp.path().join("templates");
    file(
        &src,
        ".project.json",
        br#"{"jobCode":"NIKE-W03","client":{"name":"Nike","code":"NIKE"},
            "template":{"id":"blank","version":4},"createdAt":"2025-10-13T09:00:00Z",
            "fields":{"name":"Holiday Ad","start":"2025-10-13"}}"#,
    );
    fs::create_dir_all(src.join("NIKE-W03_DELIVERY")).unwrap();
    file(&src, "HOLIDAY_AD_brief.txt", b"Holiday Ad for {client}");
    let user_installed = installed(&user);
    let folder = read_folder(&src, Keep::FoldersAndFiles, |id| {
        user_installed
            .iter()
            .find(|t| t.loaded.template.id == id)
            .map(|t| t.loaded.template.clone())
    })
    .unwrap();
    assert!(folder.from_project);
    assert_eq!(folder.template.folder_name, "{start|yymmdd}_{name|caps}");
    let ex = Example::new(TODAY);
    let saved = save_from_folder(
        &folder.template,
        &folder,
        &user_installed,
        &user,
        BUILT_IN_IDS,
        &ex,
    )
    .unwrap();
    let p = preview(&saved.template, &saved, &ex);
    assert!(p.problems.is_empty(), "{:?}", p.problems);
    // With the example answers (Acme, "Summer Campaign"), the names follow the new project.
    assert_eq!(p.folder_name, "260302_SUMMER_CAMPAIGN");
    assert!(
        p.folders
            .iter()
            .any(|f| f.ends_with("_DELIVERY") && !f.contains("NIKE")),
        "{:?}",
        p.folders
    );
    assert!(
        p.files
            .iter()
            .any(|f| f.ends_with("SUMMER_CAMPAIGN_brief.txt")),
        "{:?}",
        p.files
    );
    // The manifest never comes along.
    assert!(!saved.dir.join(".project.json").exists());
}

#[test]
fn every_kind_of_folder_previews_without_problems() {
    let tmp = tempfile::tempdir().unwrap();
    let ex = Example::new(TODAY);
    let user = tmp.path().join("templates");
    let all = installed(&user);
    let find = |id: &str| {
        all.iter()
            .find(|t| t.loaded.template.id == id)
            .map(|t| t.loaded.template.clone())
    };
    // A plain folder, a project from an installed template, one from a missing template with
    // other questions, and a no-client one.
    let plain = tmp.path().join("plain");
    job(&plain);
    let installed_project = tmp.path().join("251013_HOLIDAY_AD");
    file(
        &installed_project,
        ".project.json",
        br#"{"jobCode":"NIKE-W03","client":{"name":"Nike","code":"NIKE"},
            "template":{"id":"video"},"fields":{"name":"Holiday Ad","start":"2025-10-13"}}"#,
    );
    let missing = tmp.path().join("Kim - Blue");
    file(
        &missing,
        ".project.json",
        br#"{"jobCode":"OWN-M02","client":{"name":"","code":"OWN"},
            "template":{"id":"music-video"},"fields":{"song":"Blue","artist":"Kim"}}"#,
    );
    for dir in [&plain, &installed_project, &missing] {
        for keep in [Keep::Folders, Keep::FoldersAndFiles] {
            let folder = read_folder(dir, keep, find).unwrap();
            let p = preview(
                &folder.template,
                &folder.loaded(folder.template.clone()),
                &ex,
            );
            assert!(p.problems.is_empty(), "{}: {:?}", dir.display(), p.problems);
        }
    }
}

// ───────────── Files added in the editor (one-screen editor, PLAN 2026-10-13) ─────────────

use nest_lib::template::TreeEntry;
use nest_lib::template_edit::{save_session, EditorSession, SaveMode};
use nest_lib::template_from_folder::read_dropped;

#[test]
fn files_dropped_in_the_editor_are_saved_with_the_template() {
    let tmp = tempfile::tempdir().unwrap();
    let user = tmp.path().join("templates");
    let desk = tmp.path().join("Desktop");
    file(&desk, "grade.cube", b"LUT");
    file(&desk, "Brand/logo.png", b"PNG");
    file(&desk, "Brand/fonts/font.otf", b"OTF");
    file(&desk, "Brand/.DS_Store", b"x");
    file(&tmp.path().join("Other"), "grade.cube", b"LUT 2");

    let all = installed(&user);
    let blank = all
        .iter()
        .find(|t| t.loaded.template.id == "blank")
        .unwrap();
    let mut session = EditorSession::from_template(blank);
    let mut draft = blank.loaded.template.clone();

    // A LUT and a brand folder onto 02_GRADE; another file with the same name onto the top.
    let d = read_dropped(&[desk.join("grade.cube"), desk.join("Brand")], 1, 0).unwrap();
    assert_eq!(d.skipped.len(), 1, "{:?}", d.skipped); // .DS_Store
    let (folders, files) = session.add_dropped("02_GRADE", d.files, &d.folders);
    assert_eq!(folders, ["02_GRADE/Brand", "02_GRADE/Brand/fonts"]);
    let d2 = read_dropped(&[tmp.path().join("Other/grade.cube")], 5, 0).unwrap();
    let (_, top) = session.add_dropped("", d2.files, &d2.folders);
    assert_eq!(top[0].from, "files/2/grade.cube"); // its own name: no clash with the first
    assert_eq!(top[0].to, "grade.cube");

    draft.tree.push(TreeEntry::Folder("02_GRADE".into()));
    draft
        .tree
        .extend(folders.into_iter().map(TreeEntry::Folder));
    draft.files.extend(files);
    draft.files.extend(top);
    draft.name = "Grade".into();

    let ex = Example::new(TODAY);
    let saved = save_session(
        &draft,
        SaveMode::New,
        &session,
        &all,
        &user,
        BUILT_IN_IDS,
        &ex,
    )
    .unwrap();
    assert_eq!(
        fs::read(saved.dir.join("files/grade.cube")).unwrap(),
        b"LUT"
    );
    assert_eq!(
        fs::read(saved.dir.join("files/2/grade.cube")).unwrap(),
        b"LUT 2"
    );
    assert!(saved.dir.join("files/Brand/fonts/font.otf").is_file());
    assert!(
        saved.dir.join("starter/NOTES.md").is_file(),
        "Blank's own starter file"
    );
    let p = preview(&saved.template, &saved, &ex);
    assert!(p.problems.is_empty(), "{:?}", p.problems);
    for f in [
        "02_GRADE/grade.cube",
        "02_GRADE/Brand/logo.png",
        "02_GRADE/Brand/fonts/font.otf",
        "NOTES.md",
    ] {
        assert!(
            p.files.iter().any(|x| x.ends_with(f)),
            "{f} in {:?}",
            p.files
        );
    }
    assert!(p.files.iter().filter(|x| x.ends_with("grade.cube")).count() == 2);
    // The originals are untouched.
    assert_eq!(fs::read(desk.join("grade.cube")).unwrap(), b"LUT");
}

#[test]
fn dropping_too_much_counts_what_the_template_already_holds() {
    let tmp = tempfile::tempdir().unwrap();
    file(tmp.path(), "a.txt", b"x");
    let e = read_dropped(&[tmp.path().join("a.txt")], 2000, 0).unwrap_err();
    assert!(e.contains("more than 2000"), "{e}");
    let f = fs::File::create(tmp.path().join("big.bin")).unwrap();
    f.set_len(MAX_FILE_BYTES).unwrap();
    let e = read_dropped(&[tmp.path().join("big.bin")], 0, 150 * 1024 * 1024).unwrap_err();
    assert!(e.contains("more than 200 MB"), "{e}");
}

#[test]
fn editing_a_template_keeps_its_starter_files_and_refuses_unknown_ones() {
    let tmp = tempfile::tempdir().unwrap();
    let user = tmp.path().join("templates");
    let all = installed(&user);
    let video = all
        .iter()
        .find(|t| t.loaded.template.id == "video")
        .unwrap();
    let session = EditorSession::from_template(video);
    let ex = Example::new(TODAY);
    let mut draft = video.loaded.template.clone();
    let saved = save_session(
        &draft,
        SaveMode::Customize,
        &session,
        &all,
        &user,
        BUILT_IN_IDS,
        &ex,
    )
    .unwrap();
    assert_eq!(saved.template.files, video.loaded.template.files);
    let user2 = tmp.path().join("templates2");
    for (from, says) in [
        ("../elsewhere.md", "isn't allowed"),
        ("other.md", "isn't one of this template's files"),
    ] {
        draft.files[0].from = from.into();
        let e = save_session(
            &draft,
            SaveMode::New,
            &session,
            &all,
            &user2,
            BUILT_IN_IDS,
            &ex,
        )
        .unwrap_err();
        assert!(e.contains(says), "{from}: {e}");
    }
    assert!(!user2.exists() || fs::read_dir(&user2).unwrap().count() == 0);
}

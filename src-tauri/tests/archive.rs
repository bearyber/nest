//! Archive / Unarchive (M5 P2): one rename on the same drive, nothing copied or deleted.
//! Temp dirs only.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use nest_lib::archive::{apply_move, mark_manifest, plan_move, unarchive_root};
use nest_lib::index::{rescan_with_archive, Index};
use nest_lib::manifest::read_json_object;
use nest_lib::plan::MANIFEST_NAME;

struct Setup {
    _tmp: tempfile::TempDir,
    jobs: PathBuf,
    archive: PathBuf,
    index: Mutex<Index>,
}

fn setup() -> Setup {
    let tmp = tempfile::tempdir().unwrap();
    let jobs = tmp.path().join("Jobs");
    let archive = tmp.path().join("Archive");
    fs::create_dir(&jobs).unwrap();
    fs::create_dir(&archive).unwrap();
    let index = Mutex::new(Index::open(&tmp.path().join("index.sqlite")).unwrap());
    Setup {
        _tmp: tmp,
        jobs,
        archive,
        index,
    }
}

fn project(root: &Path, name: &str, id: &str) -> PathBuf {
    let dir = root.join(name);
    fs::create_dir_all(dir.join("04_WIP")).unwrap();
    fs::write(dir.join("04_WIP/cut.txt"), "footage").unwrap();
    let manifest = format!(
        r#"{{"schema":1,"id":"{id}","jobCode":"VX-L01","status":"done","doneAt":"2026-01-01T00:00:00+08:00","myOwnField":{{"keep":true}}}}"#
    );
    fs::write(dir.join(MANIFEST_NAME), manifest).unwrap();
    dir
}

fn archived(s: &Setup, id: &str) -> bool {
    rescan_with_archive(&s.index, std::slice::from_ref(&s.jobs), Some(&s.archive)).unwrap();
    let rows = s.index.lock().unwrap().list().unwrap();
    rows.iter()
        .find(|r| r.id.as_deref() == Some(id))
        .unwrap_or_else(|| panic!("{id} not listed"))
        .archived
}

#[test]
fn archive_and_unarchive_round_trip() {
    let s = setup();
    let from = project(&s.jobs, "261007_NIGHT_SWIM", "p1");
    assert!(!archived(&s, "p1"));

    // Archive: one rename; everything inside comes along; the manifest notes when and where from.
    let plan = plan_move(&from, &s.archive, false).unwrap();
    assert_eq!(plan.to, s.archive.join("261007_NIGHT_SWIM"));
    apply_move(&plan, false).unwrap();
    mark_manifest(&plan.to, "p1", Some(&s.jobs)).unwrap();
    assert!(!from.exists());
    assert_eq!(
        fs::read_to_string(plan.to.join("04_WIP/cut.txt")).unwrap(),
        "footage"
    );
    let m = read_json_object(&plan.to.join(MANIFEST_NAME)).unwrap();
    assert!(m["archivedAt"].is_string());
    assert_eq!(m["archivedFrom"], s.jobs.to_string_lossy().as_ref());
    assert_eq!(m["myOwnField"]["keep"], true, "unknown fields kept");
    assert!(archived(&s, "p1"));

    // Unarchive: back to where it came from; the notes are removed.
    let back_to = unarchive_root(
        m["archivedFrom"].as_str(),
        std::slice::from_ref(&s.jobs),
        &plan.to,
    )
    .unwrap();
    let plan = plan_move(&plan.to, &back_to, true).unwrap();
    apply_move(&plan, true).unwrap();
    mark_manifest(&plan.to, "p1", None).unwrap();
    assert_eq!(plan.to, from);
    let m = read_json_object(&from.join(MANIFEST_NAME)).unwrap();
    assert!(m.get("archivedAt").is_none() && m.get("archivedFrom").is_none());
    assert_eq!(m["status"], "done");
    assert_eq!(m["myOwnField"]["keep"], true);
    assert!(!archived(&s, "p1"));
}

#[test]
fn a_project_in_a_year_folder_goes_back_into_it() {
    let s = setup();
    let year = s.jobs.join("2025");
    let from = project(&year, "250608_BRAND_FILM", "y1");
    let plan = plan_move(&from, &s.archive, false).unwrap();
    apply_move(&plan, false).unwrap();
    mark_manifest(&plan.to, "y1", from.parent()).unwrap();
    let m = read_json_object(&plan.to.join(MANIFEST_NAME)).unwrap();
    let back_to = unarchive_root(
        m["archivedFrom"].as_str(),
        std::slice::from_ref(&s.jobs),
        &plan.to,
    )
    .unwrap();
    assert_eq!(back_to, year, "back into Jobs/2025, not the top of Jobs");
    let plan = plan_move(&plan.to, &back_to, true).unwrap();
    apply_move(&plan, true).unwrap();
    assert!(from.join(MANIFEST_NAME).is_file());
}

#[test]
fn a_decomposed_name_from_an_older_mac_drive_is_still_nests_own() {
    // HFS+ hands back "é" as e + accent; the folder is still one Nest made.
    let s = setup();
    let from = project(&s.jobs, "261007_Beyonce\u{301}", "nfd");
    let plan = plan_move(&from, &s.archive, false).unwrap();
    apply_move(&plan, false).unwrap();
    assert!(plan.to.is_dir());
}

#[test]
fn korean_folder_name_round_trips() {
    let s = setup();
    let from = project(&s.jobs, "261007_아이유_좋은날", "k1");
    let plan = plan_move(&from, &s.archive, false).unwrap();
    apply_move(&plan, false).unwrap();
    assert!(s.archive.join("261007_아이유_좋은날").is_dir());
    let plan = plan_move(&plan.to, &s.jobs, true).unwrap();
    apply_move(&plan, true).unwrap();
    assert!(from.is_dir());
}

#[test]
fn a_folder_with_the_same_name_in_the_archive_is_never_overwritten() {
    let s = setup();
    let from = project(&s.jobs, "261007_A", "a");
    let other = project(&s.archive, "261007_A", "older");
    let err = plan_move(&from, &s.archive, false).unwrap_err();
    assert!(err.contains("already a folder called"), "{err}");
    assert!(from.is_dir());
    assert_eq!(
        read_json_object(&other.join(MANIFEST_NAME)).unwrap()["id"],
        "older"
    );

    // Even if it appears between the check and the move.
    fs::remove_dir_all(&other).unwrap();
    let plan = plan_move(&from, &s.archive, false).unwrap();
    fs::create_dir(&plan.to).unwrap();
    let err = apply_move(&plan, false).unwrap_err();
    assert!(err.contains("Nothing was moved"), "{err}");
    assert!(from.join(MANIFEST_NAME).is_file());
}

#[test]
fn unarchive_only_trusts_a_jobs_folder_of_this_computer() {
    let s = setup();
    let second = s.jobs.with_file_name("Jobs2");
    fs::create_dir(&second).unwrap();
    let roots = vec![s.jobs.clone(), second.clone()];
    let here = &s.archive;
    // Where it came from, if that's one of ours.
    assert_eq!(
        unarchive_root(Some(&second.to_string_lossy()), &roots, here),
        Some(second.clone())
    );
    // Anywhere else (hand-edited, the other computer's path, gone, too deep): the first jobs
    // folder on the same drive.
    let deep = s.jobs.join("a/b");
    fs::create_dir_all(&deep).unwrap();
    for from in [
        Some("C:/Windows/System32".to_string()),
        Some("/etc".to_string()),
        Some("D:/Gone".to_string()),
        Some(deep.to_string_lossy().into_owned()),
        None,
    ] {
        assert_eq!(
            unarchive_root(from.as_deref(), &roots, here),
            Some(s.jobs.clone()),
            "{from:?}"
        );
    }
    // The first one missing: the next that's there.
    fs::remove_dir_all(&s.jobs).unwrap();
    assert_eq!(unarchive_root(None, &roots, here), Some(second));
    assert_eq!(unarchive_root(None, &[], here), None);
}

#[test]
fn refuses_cloud_folders_unclean_names_and_missing_folders() {
    let s = setup();
    let cloud = s.jobs.with_file_name("Dropbox");
    fs::create_dir(&cloud).unwrap();
    let from = project(&s.jobs, "261007_A", "a");
    let err = plan_move(&from, &cloud, false).unwrap_err();
    assert!(err.contains("cloud-synced"), "{err}");

    // A name Nest would never make (two spaces in a row) is left alone, never renamed.
    let odd = project(&s.jobs, "261007_A  B", "b");
    let err = plan_move(&odd, &s.archive, false).unwrap_err();
    assert!(err.contains("Move it yourself"), "{err}");
    assert!(odd.is_dir());

    let err = plan_move(&s.jobs.join("Nope"), &s.archive, false).unwrap_err();
    assert!(err.contains("isn't there"), "{err}");
    let err = plan_move(&from, &s.jobs.with_file_name("NoArchive"), false).unwrap_err();
    assert!(err.contains("archive folder isn't there"), "{err}");
    assert!(from.is_dir(), "nothing moved");
}

#[test]
fn mark_manifest_refuses_another_projects_file() {
    let s = setup();
    let dir = project(&s.jobs, "261007_A", "a");
    let before = fs::read_to_string(dir.join(MANIFEST_NAME)).unwrap();
    assert!(mark_manifest(&dir, "someone-else", Some(&s.jobs)).is_err());
    assert_eq!(fs::read_to_string(dir.join(MANIFEST_NAME)).unwrap(), before);
}

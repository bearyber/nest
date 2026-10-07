//! Rescan behaviour (spec §6, §7, M3 review). Temp dirs only.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use nest_lib::index::{now_ms, rescan, rescan_with_archive, FoundProject, Index, ProjectRow};
use nest_lib::plan::MANIFEST_NAME;

fn project(root: &Path, rel: &str, id: &str, code: &str) -> PathBuf {
    let dir = root.join(rel);
    fs::create_dir_all(&dir).unwrap();
    let manifest = format!(
        r#"{{"schema":1,"id":"{id}","jobCode":"{code}","title":"{rel}","client":{{"name":"Vertex Studio","code":"VX"}},"space":"Work","status":"active","createdAt":"2026-09-{:02}T10:00:00+08:00","fields":{{"name":"{rel}"}}}}"#,
        (id.len() % 28) + 1
    );
    fs::write(dir.join(MANIFEST_NAME), manifest).unwrap();
    dir
}

struct Setup {
    _tmp: tempfile::TempDir,
    jobs: PathBuf,
    index: Mutex<Index>,
}

fn setup() -> Setup {
    let tmp = tempfile::tempdir().unwrap();
    let jobs = tmp.path().join("Jobs");
    fs::create_dir(&jobs).unwrap();
    let index = Mutex::new(Index::open(&tmp.path().join("index.sqlite")).unwrap());
    Setup {
        _tmp: tmp,
        jobs,
        index,
    }
}

fn list(s: &Setup) -> Vec<ProjectRow> {
    s.index.lock().unwrap().list().unwrap()
}

fn find<'a>(rows: &'a [ProjectRow], key: &str) -> &'a ProjectRow {
    rows.iter()
        .find(|r| r.key == key)
        .unwrap_or_else(|| panic!("{key} not listed"))
}

#[test]
fn finds_projects_two_levels_deep_and_skips_the_rest() {
    let s = setup();
    project(&s.jobs, "260928_A", "a", "VX-L01");
    project(&s.jobs, "Archive/250101_B", "b", "VX-L02");
    project(&s.jobs, "260928_A/inner", "inner", "VX-L99"); // inside a project: never read
    project(&s.jobs, "node_modules/x", "nm", "VX-L98");
    project(&s.jobs, ".hidden/x", "hid", "VX-L97");
    project(&s.jobs, "a/b/too_deep", "deep", "VX-L96");
    let broken = s.jobs.join("Broken");
    fs::create_dir(&broken).unwrap();
    fs::write(broken.join(MANIFEST_NAME), "{ not json").unwrap();
    let bom = s.jobs.join("Bom");
    fs::create_dir(&bom).unwrap();
    fs::write(
        bom.join(MANIFEST_NAME),
        "\u{feff}{\"id\":\"bom\",\"jobCode\":\"VX-L05\"}",
    )
    .unwrap();

    let summary = rescan(&s.index, std::slice::from_ref(&s.jobs)).unwrap();
    assert_eq!(summary.found, 4, "{summary:?}");
    let rows = list(&s);
    let mut keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
    keys.sort();
    let broken_key = format!("path:{}", broken.display());
    assert_eq!(keys, ["a", "b", "bom", broken_key.as_str()]);
    let broken_row = find(&rows, &broken_key);
    assert!(broken_row.error.is_some());
    assert_eq!(broken_row.title, "Broken");
    // Unreadable manifests are never written to.
    assert_eq!(
        fs::read_to_string(broken.join(MANIFEST_NAME)).unwrap(),
        "{ not json"
    );
}

#[test]
fn moved_folder_keeps_its_row_deleted_folder_leaves() {
    let s = setup();
    let a = project(&s.jobs, "260928_A", "a", "VX-L01");
    let b = project(&s.jobs, "260928_B", "b", "VX-L02");
    rescan(&s.index, std::slice::from_ref(&s.jobs)).unwrap();

    fs::rename(&a, s.jobs.join("Renamed_A")).unwrap();
    fs::remove_dir_all(&b).unwrap();
    let summary = rescan(&s.index, std::slice::from_ref(&s.jobs)).unwrap();
    assert_eq!(summary.removed, 1);
    let rows = list(&s);
    assert_eq!(rows.len(), 1);
    assert_eq!(find(&rows, "a").paths, [s.jobs.join("Renamed_A")]);
}

#[test]
fn offline_jobs_folder_keeps_its_projects() {
    let s = setup();
    project(&s.jobs, "260928_A", "a", "VX-L01");
    rescan(&s.index, std::slice::from_ref(&s.jobs)).unwrap();

    // Drive unplugged: the folder is gone entirely.
    let parked = s.jobs.with_file_name("Parked");
    fs::rename(&s.jobs, &parked).unwrap();
    let summary = rescan(&s.index, std::slice::from_ref(&s.jobs)).unwrap();
    assert_eq!(summary.removed, 0);
    assert_eq!(summary.offline, std::slice::from_ref(&s.jobs));
    let rows = list(&s);
    assert!(find(&rows, "a").offline);

    // Plugged back in: online again.
    fs::rename(&parked, &s.jobs).unwrap();
    rescan(&s.index, std::slice::from_ref(&s.jobs)).unwrap();
    assert!(!find(&list(&s), "a").offline);
}

/// Deleting the last project in a jobs folder removes it from the list (no "missing" leftover).
#[test]
fn deleting_the_last_project_removes_it() {
    let s = setup();
    let a = project(&s.jobs, "260928_A", "a", "VX-L01");
    rescan(&s.index, std::slice::from_ref(&s.jobs)).unwrap();
    let parked = s.jobs.with_file_name("elsewhere");
    fs::rename(&a, &parked).unwrap();
    let summary = rescan(&s.index, std::slice::from_ref(&s.jobs)).unwrap();
    assert_eq!(summary.removed, 1);
    assert!(list(&s).is_empty());
}

/// Only folders that can look empty because they aren't connected keep the caution.
#[test]
fn only_unmounted_looking_folders_stay_offline_when_empty() {
    use nest_lib::index::may_be_unmounted;
    assert!(may_be_unmounted(std::path::Path::new("/Volumes/NAS/Jobs")));
    assert!(may_be_unmounted(std::path::Path::new(
        "/Users/me/Library/CloudStorage/GoogleDrive-me/My Drive/Jobs"
    )));
    assert!(!may_be_unmounted(std::path::Path::new(r"D:\Jobs")));
    assert!(!may_be_unmounted(std::path::Path::new("/Users/me/Jobs")));
}

#[test]
fn unreadable_subfolder_never_loses_projects() {
    let s = setup();
    project(&s.jobs, "Clients/260928_A", "a", "VX-L01");
    rescan(&s.index, std::slice::from_ref(&s.jobs)).unwrap();
    // A later scan that can't see the project (e.g. a NAS hiccup), but its manifest is
    // still there: the index keeps it because the file isn't definitely gone.
    let index = &s.index;
    let start = now_ms() + 1;
    let walk = nest_lib::index::Walk {
        online_roots: vec![s.jobs.clone()],
        ..Default::default()
    };
    let stale = index
        .lock()
        .unwrap()
        .stale_paths(std::slice::from_ref(&s.jobs), start)
        .unwrap();
    assert_eq!(stale.len(), 1, "seen before, so it's a removal candidate");
    let gone: Vec<PathBuf> = stale
        .into_iter()
        .filter(|d| !d.join(MANIFEST_NAME).exists())
        .collect();
    let summary = index
        .lock()
        .unwrap()
        .apply_rescan(
            &walk,
            &[],
            &gone,
            std::slice::from_ref(&s.jobs),
            None,
            start,
        )
        .unwrap();
    assert_eq!(summary.removed, 0);
    assert_eq!(list(&s).len(), 1);
}

#[test]
fn project_added_during_a_scan_is_not_removed() {
    let s = setup();
    let start = now_ms();
    // Create finishes after the scan began: recorded with a newer timestamp.
    let dir = s.jobs.join("260928_NEW");
    s.index
        .lock()
        .unwrap()
        .record(
            &[FoundProject {
                dir: dir.clone(),
                root: s.jobs.clone(),
                manifest: Ok(serde_json::json!({ "id": "new", "jobCode": "VX-L09" })),
            }],
            start + 10,
        )
        .unwrap();
    let walk = nest_lib::index::Walk {
        online_roots: vec![s.jobs.clone()],
        ..Default::default()
    };
    // Even if the scan thinks it's gone, last_seen is newer than the scan start.
    let summary = s
        .index
        .lock()
        .unwrap()
        .apply_rescan(
            &walk,
            &[],
            &[dir],
            std::slice::from_ref(&s.jobs),
            None,
            start,
        )
        .unwrap();
    assert_eq!(summary.removed, 0);
    assert_eq!(list(&s).len(), 1);
}

#[test]
fn same_project_in_two_places_is_listed_once_duplicate_codes_flagged() {
    let s = setup();
    let other = s.jobs.with_file_name("Backup");
    fs::create_dir(&other).unwrap();
    project(&s.jobs, "260928_A", "a", "VX-L01");
    project(&other, "260928_A", "a", "VX-L01");
    project(&s.jobs, "260928_C", "c", "VX-L03");
    project(&s.jobs, "260928_D", "d", "vx-l03");
    let summary = rescan(&s.index, &[s.jobs.clone(), other.clone()]).unwrap();
    assert_eq!(summary.duplicates, ["VX-L03"]);
    let rows = list(&s);
    assert_eq!(rows.len(), 3);
    assert_eq!(find(&rows, "a").paths.len(), 2);
    assert!(!find(&rows, "a").duplicate_code);
    assert!(find(&rows, "c").duplicate_code && find(&rows, "d").duplicate_code);
}

/// Archive (M5): "done since" comes from the manifest's doneAt, or the manifest file's
/// modified time for projects marked done before that existed; active projects have none.
#[test]
fn done_since_is_read_from_the_manifest_or_its_file_date() {
    let s = setup();
    project(&s.jobs, "260928_A", "a", "VX-L01"); // active
    let dated = project(&s.jobs, "260101_B", "b", "VX-L02");
    fs::write(
        dated.join(MANIFEST_NAME),
        r#"{"id":"b","jobCode":"VX-L02","status":"done","doneAt":"2026-01-15T10:00:00+08:00"}"#,
    )
    .unwrap();
    let undated = project(&s.jobs, "260201_C", "c", "VX-L03");
    fs::write(
        undated.join(MANIFEST_NAME),
        r#"{"id":"c","jobCode":"VX-L03","status":"done"}"#,
    )
    .unwrap();
    let before = now_ms();
    rescan(&s.index, std::slice::from_ref(&s.jobs)).unwrap();
    let rows = list(&s);
    assert_eq!(find(&rows, "a").done_at, None);
    assert_eq!(find(&rows, "b").done_at, Some(1_768_442_400_000)); // 2026-01-15 02:00 UTC
    let c = find(&rows, "c").done_at.expect("file date fallback");
    assert!(
        (before - 60_000..=now_ms() + 1_000).contains(&c),
        "{c} vs {before}"
    );
    // Marking done through the index stamps now; back to active clears it.
    s.index.lock().unwrap().set_status("a", "done").unwrap();
    assert!(find(&list(&s), "a").done_at.is_some());
    s.index.lock().unwrap().set_status("a", "active").unwrap();
    assert_eq!(find(&list(&s), "a").done_at, None);
}

/// Archive (M5): projects under the archive folder list as archived; a copy still in a jobs
/// folder wins; clearing the archive folder drops them from the list.
#[test]
fn archive_folder_projects_are_archived_and_leave_when_it_is_cleared() {
    let s = setup();
    let archive = s.jobs.with_file_name("Archive");
    fs::create_dir(&archive).unwrap();
    project(&s.jobs, "260928_A", "a", "VX-L01");
    project(&archive, "250101_OLD", "old", "VX-L02");
    project(&archive, "2025/250202_DEEP", "deep", "VX-L03"); // archive is walked 2 levels too
    project(&archive, "260928_A", "a", "VX-L01"); // a copy of A: the jobs-folder one wins
    let summary =
        rescan_with_archive(&s.index, std::slice::from_ref(&s.jobs), Some(&archive)).unwrap();
    assert_eq!(summary.found, 4);
    let rows = list(&s);
    assert_eq!(rows.len(), 3);
    assert!(find(&rows, "old").archived);
    assert!(find(&rows, "deep").archived);
    assert!(
        !find(&rows, "a").archived,
        "a copy in a jobs folder is not archived"
    );
    assert_eq!(find(&rows, "a").paths.len(), 2);
    // Archived projects still take part in duplicate detection.
    project(&s.jobs, "260928_X", "x", "vx-l02");
    let summary =
        rescan_with_archive(&s.index, std::slice::from_ref(&s.jobs), Some(&archive)).unwrap();
    assert_eq!(summary.duplicates, ["VX-L02"]);
    // Archive folder cleared: its projects leave, the jobs folder's stay.
    rescan(&s.index, std::slice::from_ref(&s.jobs)).unwrap();
    let rows = list(&s);
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| !r.archived));
    assert_eq!(find(&rows, "a").paths.len(), 1);
}

#[test]
fn removed_jobs_folder_setting_drops_its_projects() {
    let s = setup();
    let other = s.jobs.with_file_name("Old");
    fs::create_dir(&other).unwrap();
    project(&other, "250101_OLD", "old", "VX-L01");
    rescan(&s.index, std::slice::from_ref(&other)).unwrap();
    assert_eq!(list(&s).len(), 1);
    rescan(&s.index, std::slice::from_ref(&s.jobs)).unwrap();
    assert!(list(&s).is_empty());
    assert!(
        other.join("250101_OLD").join(MANIFEST_NAME).exists(),
        "folders untouched"
    );
}

#[test]
fn forgetting_a_jobs_folder_drops_its_projects_only_from_the_list() {
    let s = setup();
    let other = s.jobs.with_file_name("Archive");
    fs::create_dir(&other).unwrap();
    project(&s.jobs, "260928_A", "a", "VX-L01");
    project(&other, "250101_B", "b", "VX-L02");
    rescan(&s.index, &[s.jobs.clone(), other.clone()]).unwrap();
    assert_eq!(list(&s).len(), 2);
    assert_eq!(
        s.index
            .lock()
            .unwrap()
            .spaces_in_use()
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>(),
        ["Work"]
    );
    s.index
        .lock()
        .unwrap()
        .forget_roots_except(std::slice::from_ref(&s.jobs))
        .unwrap();
    let rows = list(&s);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].key, "a");
    assert!(
        other.join("250101_B").join(MANIFEST_NAME).exists(),
        "folder untouched"
    );
}

#[test]
fn five_hundred_projects_scan_under_two_seconds() {
    let s = setup();
    for i in 0..500 {
        project(
            &s.jobs,
            &format!("2609{:02}_PROJECT_{i:03}", i % 28),
            &format!("id-{i}"),
            &format!("VX-L{i:03}"),
        );
    }
    let t = Instant::now();
    let summary = rescan(&s.index, std::slice::from_ref(&s.jobs)).unwrap();
    let first = t.elapsed();
    let t = Instant::now();
    rescan(&s.index, std::slice::from_ref(&s.jobs)).unwrap();
    let second = t.elapsed();
    assert_eq!(summary.found, 500);
    assert_eq!(list(&s).len(), 500);
    println!("500 projects: first scan {first:?}, rescan {second:?}");
    assert!(first.as_secs_f64() < 2.0 && second.as_secs_f64() < 2.0);
}

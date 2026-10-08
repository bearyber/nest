//! Devices (v0.3.0): two simulated computers sharing one temp folder. Temp dirs only.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use nest_lib::devices::{
    annotate, build_own, claim_allowed, devices_dir, incoming, merge, read_others, valid_device_id,
    valid_status, write_own, DeviceFile, DeviceProject, Handled, StatusRequest, MAX_FILES,
    MAX_FILE_BYTES,
};
use nest_lib::index::ProjectRow;
use nest_lib::plan::ClientValue;

const PC: &str = "11111111-1111-4111-8111-111111111111";
const MAC: &str = "22222222-2222-4222-8222-222222222222";

fn row(id: &str, code: &str, title: &str, path: &str) -> ProjectRow {
    ProjectRow {
        key: id.into(),
        id: Some(id.into()),
        job_code: code.into(),
        title: title.into(),
        client: ClientValue {
            name: "Acme".into(),
            code: "ACME".into(),
        },
        space: "Work".into(),
        status: "active".into(),
        template_id: "video".into(),
        created_at: "2026-10-02T10:00:00+08:00".into(),
        fields: serde_json::json!({ "artist": "Acme Band" }),
        error: None,
        paths: vec![PathBuf::from(path)],
        offline: false,
        duplicate_code: false,
        device: None,
        also_on: vec![],
        template_name: String::new(),
        pending: None,
        request_problem: None,
        changed_by: None,
        archived: false,
        done_at: None,
        size: None,
        ready_to_archive: false,
    }
}

fn shared() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = devices_dir(tmp.path(), false);
    (tmp, dir)
}

/// Archive (M5): "archived" travels in the list, and a file from an older Nest without it
/// still reads (as not archived).
#[test]
fn archived_travels_to_other_computers_and_older_files_still_read() {
    let (_tmp, dir) = shared();
    let mut old = row("m1", "ACME-M01", "Old Film", "/Users/me/Archive/OLD");
    old.archived = true;
    let mac = build_own(MAC, "MacBook", &[old], "2026-10-04T10:00:00+08:00");
    assert!(mac.projects[0].archived);
    write_own(&dir, &mac, &mut None).unwrap();
    let others = read_others(&dir, PC, &mut HashMap::new());
    assert!(others[0].projects[0].archived);
    let mut rows = vec![];
    merge(&mut rows, &others);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].archived && rows[0].device.is_some());

    let older: DeviceProject = serde_json::from_str(
        r#"{"id":"p1","jobCode":"ACME-M02","title":"T","client":{"name":"","code":""},"space":"Work","status":"done","templateId":"video","createdAt":"","path":"/x"}"#,
    )
    .unwrap();
    assert!(!older.archived);
}

#[test]
fn two_computers_see_each_others_projects() {
    let (_tmp, dir) = shared();
    let mac_rows = [row(
        "m1",
        "ACME-M01",
        "Launch Film",
        "/Users/me/Jobs/LAUNCH",
    )];
    let mut last = None;
    let mac = build_own(MAC, "MacBook", &mac_rows, "2026-10-02T10:00:00+08:00");
    assert!(write_own(&dir, &mac, &mut last).unwrap(), "first write");

    let mut cache = HashMap::new();
    let others = read_others(&dir, PC, &mut cache);
    assert_eq!(others.len(), 1);
    assert_eq!(others[0].name, "MacBook");
    assert_eq!(others[0].projects[0].job_code, "ACME-M01");
    assert_eq!(others[0].projects[0].artist.as_deref(), Some("Acme Band"));

    // The PC never lists its own file as "another computer".
    let pc = build_own(PC, "Studio PC", &[], "2026-10-02T10:00:00+08:00");
    write_own(&dir, &pc, &mut None).unwrap();
    assert_eq!(read_others(&dir, PC, &mut HashMap::new()).len(), 1);
}

#[test]
fn only_the_own_file_is_written_and_only_when_it_changed() {
    let (_tmp, dir) = shared();
    let rows = [row("p1", "ACME-W01", "Summer", "D:/Jobs/SUMMER")];
    let mut last = None;
    assert!(write_own(&dir, &build_own(PC, "Studio PC", &rows, "t1"), &mut last).unwrap());
    // Same content, newer timestamp: not rewritten.
    assert!(!write_own(&dir, &build_own(PC, "Studio PC", &rows, "t2"), &mut last).unwrap());
    let names: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, [format!("{PC}.json")]);
}

#[test]
fn conflict_copies_strays_and_wrong_ids_are_ignored() {
    let (_tmp, dir) = shared();
    let mac = build_own(MAC, "MacBook", &[row("m1", "ACME-M01", "A", "/x")], "t");
    write_own(&dir, &mac, &mut None).unwrap();
    let text = fs::read_to_string(dir.join(format!("{MAC}.json"))).unwrap();
    // Sync conflict copies and stray files.
    fs::write(dir.join(format!("{MAC} (1).json")), &text).unwrap();
    fs::write(dir.join(format!("{MAC} 2.json")), &text).unwrap();
    fs::write(dir.join("notes.json"), "{}").unwrap();
    fs::write(dir.join(".nest-abc.tmp"), "half").unwrap();
    // A file named after one computer but claiming to be another.
    let other = "33333333-3333-4333-8333-333333333333";
    fs::write(dir.join(format!("{other}.json")), &text).unwrap();

    let others = read_others(&dir, PC, &mut HashMap::new());
    let ok: Vec<&str> = others
        .iter()
        .filter(|d| d.problem.is_none())
        .map(|d| d.name.as_str())
        .collect();
    assert_eq!(ok, ["MacBook"]);
    let bad = others.iter().find(|d| d.device_id == other).unwrap();
    assert!(bad
        .problem
        .as_deref()
        .unwrap()
        .contains("different computer"));
}

#[test]
fn a_broken_file_keeps_the_last_good_copy() {
    let (_tmp, dir) = shared();
    write_own(
        &dir,
        &build_own(MAC, "MacBook", &[row("m1", "ACME-M01", "A", "/x")], "t"),
        &mut None,
    )
    .unwrap();
    let mut cache = HashMap::new();
    assert_eq!(read_others(&dir, PC, &mut cache)[0].projects.len(), 1);
    // Half-synced: not valid JSON yet.
    fs::write(dir.join(format!("{MAC}.json")), "{ \"schema\": 1, \"devi").unwrap();
    let again = read_others(&dir, PC, &mut cache);
    assert_eq!(again[0].projects.len(), 1, "projects don't disappear");
    assert!(again[0].problem.is_some());
}

#[test]
fn oversized_files_are_refused() {
    let (_tmp, dir) = shared();
    fs::create_dir_all(&dir).unwrap();
    let big = "x".repeat(MAX_FILE_BYTES as usize + 10);
    fs::write(dir.join(format!("{MAC}.json")), big).unwrap();
    let others = read_others(&dir, PC, &mut HashMap::new());
    assert!(others[0].problem.as_deref().unwrap().contains("too big"));
}

#[test]
fn long_strings_are_cut_and_paths_stay_text() {
    let (_tmp, dir) = shared();
    let mut file = build_own(
        MAC,
        "MacBook",
        &[row("m1", "ACME-M01", &"T".repeat(5000), "../../etc")],
        "t",
    );
    file.name = "N".repeat(1000);
    write_own(&dir, &file, &mut None).unwrap();
    let others = read_others(&dir, PC, &mut HashMap::new());
    assert!(others[0].name.chars().count() <= 400);
    assert!(others[0].projects[0].title.chars().count() <= 400);
    assert_eq!(
        others[0].projects[0].path, "../../etc",
        "shown as text, never used"
    );
}

#[test]
fn dev_builds_use_their_own_folder() {
    let tmp = tempfile::tempdir().unwrap();
    assert_ne!(
        devices_dir(tmp.path(), true),
        devices_dir(tmp.path(), false)
    );
    assert!(devices_dir(tmp.path(), true).ends_with("Nest/devices-dev"));
}

#[test]
fn device_ids_must_be_uuids() {
    assert!(valid_device_id(PC).is_some());
    assert!(valid_device_id("../evil").is_none());
    assert!(valid_device_id("").is_none());
    let (_tmp, dir) = shared();
    let mut bad = build_own("not-a-uuid", "X", &[], "t");
    bad.device_id = "../../x".into();
    assert!(write_own(&dir, &bad, &mut None).is_err());
}

#[test]
fn merge_shows_remote_projects_once_and_flags_shared_codes() {
    let mut rows = vec![
        row("p1", "ACME-W01", "Summer", "D:/Jobs/SUMMER"),
        row("both", "GLBX-W01", "Brand", "D:/Jobs/BRAND"),
    ];
    let mac = DeviceFile {
        projects: build_own(
            MAC,
            "MacBook",
            &[
                row("m1", "NWD-M01", "Launch", "/Users/me/LAUNCH"),
                row("both", "GLBX-W01", "Brand", "/Users/me/BRAND"),
                row("m2", "ACME-W01", "Clash", "/Users/me/CLASH"),
            ],
            "t",
        )
        .projects,
        ..build_own(MAC, "MacBook", &[], "t")
    };
    let (_tmp, dir) = shared();
    write_own(&dir, &mac, &mut None).unwrap();
    let others = read_others(&dir, PC, &mut HashMap::new());
    merge(&mut rows, &others);

    let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
    assert_eq!(rows.len(), 4, "{keys:?}");
    let both = rows.iter().find(|r| r.key == "both").unwrap();
    assert_eq!(both.also_on, ["MacBook"]);
    let remote = rows.iter().find(|r| r.id.as_deref() == Some("m1")).unwrap();
    assert_eq!(remote.key, format!("remote:{MAC}:m1"));
    assert_eq!(remote.device.as_ref().unwrap().name, "MacBook");
    assert!(
        remote.paths.is_empty(),
        "no local path for a remote project"
    );
    // ACME-W01 is used by two different projects across the computers.
    assert!(rows
        .iter()
        .filter(|r| r.job_code == "ACME-W01")
        .all(|r| r.duplicate_code));
    assert!(!remote.duplicate_code);
}

/// Review (v0.3.0): if the synced folder is missing (Google Drive signed out, NAS not
/// mounted), nothing is created; recreating its path locally would confuse the cloud app.
#[test]
fn nothing_is_created_when_the_synced_folder_is_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("My Drive");
    let dir = devices_dir(&missing, false);
    let err = write_own(&dir, &build_own(PC, "Studio PC", &[], "t"), &mut None).unwrap_err();
    assert!(err.contains("isn't available"), "{err}");
    assert!(
        !missing.exists(),
        "the synced folder's path was not created"
    );
}

#[test]
fn a_project_on_two_other_computers_shows_once() {
    let (_tmp, dir) = shared();
    let third = "33333333-3333-4333-8333-333333333333";
    for (id, name) in [(MAC, "MacBook"), (third, "Laptop")] {
        let f = build_own(id, name, &[row("shared", "ACME-M01", "Shared", "/x")], "t");
        write_own(&dir, &f, &mut None).unwrap();
    }
    let others = read_others(&dir, PC, &mut HashMap::new());
    let mut rows = vec![];
    merge(&mut rows, &others);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].also_on.len(), 1, "the second computer is 'also on'");
    assert!(
        !rows[0].duplicate_code,
        "the same project isn't a duplicate of itself"
    );
}

#[test]
fn projects_without_an_id_are_never_shown() {
    let (_tmp, dir) = shared();
    let mut f = build_own(MAC, "MacBook", &[row("x", "ACME-M09", "A", "/x")], "t");
    f.projects[0].id = String::new();
    write_own(&dir, &f, &mut None).unwrap();
    let mut rows = vec![];
    merge(&mut rows, &read_others(&dir, PC, &mut HashMap::new()));
    assert!(rows.is_empty());
}

#[test]
fn icloud_placeholders_mean_not_downloaded_yet() {
    let (_tmp, dir) = shared();
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(format!(".{MAC}.json.icloud")), "").unwrap();
    let others = read_others(&dir, PC, &mut HashMap::new());
    assert_eq!(others.len(), 1);
    assert!(others[0].not_downloaded);
}

#[test]
fn at_most_max_files_lists_are_read() {
    let (_tmp, dir) = shared();
    for i in 0..(MAX_FILES + 5) {
        let id = format!("00000000-0000-4000-8000-{i:012}");
        write_own(&dir, &build_own(&id, "C", &[], "t"), &mut None).unwrap();
    }
    assert_eq!(read_others(&dir, PC, &mut HashMap::new()).len(), MAX_FILES);
}

#[test]
fn claiming_a_list_still_in_use_is_refused() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-10-03T12:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    assert!(
        claim_allowed("2026-10-03T08:00:00+00:00", now).is_err(),
        "updated 4 h ago"
    );
    assert!(
        claim_allowed("2026-09-20T08:00:00+00:00", now).is_ok(),
        "two weeks old"
    );
    // Unreadable or still syncing: it may be in use, so it can't be taken over (audit W5).
    assert!(claim_allowed("", now).is_err(), "unknown time");
    assert!(claim_allowed("not a date", now).is_err());
}

fn req(id: &str, seq: u64, to: &str, project: &str, status: &str) -> StatusRequest {
    StatusRequest {
        id: id.into(),
        seq,
        to_device: to.into(),
        project_id: project.into(),
        status: status.into(),
        requested_at: "2026-10-03T10:00:00+08:00".into(),
    }
}

/// v0.3.2: the PC asks the Mac to mark a project done. The Mac sees each request once, in
/// order; old numbers (a stale synced copy) and requests for other computers are ignored.
#[test]
fn requests_reach_their_computer_once_and_in_order() {
    let (_tmp, dir) = shared();
    let mut pc = build_own(PC, "Studio PC", &[], "t");
    pc.requests = vec![
        req("r2", 2, MAC, "m1", "active"),
        req("r1", 1, MAC, "m1", "done"),
        req("x", 3, "33333333-3333-4333-8333-333333333333", "m9", "done"),
    ];
    write_own(&dir, &pc, &mut None).unwrap();

    let others = read_others(&dir, MAC, &mut HashMap::new());
    let mut done_seq = std::collections::BTreeMap::new();
    let got: Vec<&str> = incoming(MAC, &others, &done_seq)
        .iter()
        .map(|(_, r)| r.id.as_str())
        .collect();
    assert_eq!(got, ["r1", "r2"], "only the Mac's, oldest first");

    // After dealing with #1, only #2 is left; after #2, nothing, even if r1 reappears.
    done_seq.insert(PC.to_string(), 1);
    let got: Vec<&str> = incoming(MAC, &others, &done_seq)
        .iter()
        .map(|(_, r)| r.id.as_str())
        .collect();
    assert_eq!(got, ["r2"]);
    done_seq.insert(PC.to_string(), 2);
    assert!(incoming(MAC, &others, &done_seq).is_empty());
}

#[test]
fn only_active_and_done_are_valid() {
    assert!(valid_status("done") && valid_status("active"));
    assert!(!valid_status("deleted") && !valid_status("") && !valid_status("DONE"));
}

/// The PC shows "waiting for MacBook" until the Mac's list says it dealt with the request,
/// and shows the reason if the Mac refused it.
#[test]
fn waiting_clears_once_handled_and_refusals_show() {
    let (_tmp, dir) = shared();
    let mut mac = build_own(
        MAC,
        "MacBook",
        &[row("m1", "ACME-M01", "Launch", "/x")],
        "t",
    );
    write_own(&dir, &mac, &mut None).unwrap();
    let outgoing = vec![req("r1", 1, MAC, "m1", "done")];

    let mut rows = vec![];
    merge(&mut rows, &read_others(&dir, PC, &mut HashMap::new()));
    annotate(
        &mut rows,
        &outgoing,
        &read_others(&dir, PC, &mut HashMap::new()),
        &HashMap::new(),
    );
    assert_eq!(rows[0].pending.as_deref(), Some("done"));

    // The Mac dealt with it.
    mac.handled = vec![Handled {
        id: "r1".into(),
        from_device: PC.into(),
        seq: 1,
        applied: true,
        reason: None,
    }];
    write_own(&dir, &mac, &mut None).unwrap();
    let others = read_others(&dir, PC, &mut HashMap::new());
    let mut rows = vec![];
    merge(&mut rows, &others);
    let refusals: HashMap<String, String> = [(
        "m1".to_string(),
        "That project isn't on MacBook any more.".to_string(),
    )]
    .into();
    annotate(&mut rows, &outgoing, &others, &refusals);
    assert_eq!(rows[0].pending, None);
    assert!(rows[0]
        .request_problem
        .as_deref()
        .unwrap()
        .contains("isn't on MacBook"));
}

#[test]
fn too_many_requests_are_refused() {
    let (_tmp, dir) = shared();
    let mut pc = build_own(PC, "Studio PC", &[], "t");
    pc.requests = (0..1_001)
        .map(|i| req(&format!("r{i}"), i, MAC, "m1", "done"))
        .collect();
    write_own(&dir, &pc, &mut None).unwrap();
    let others = read_others(&dir, MAC, &mut HashMap::new());
    assert!(others[0]
        .problem
        .as_deref()
        .unwrap()
        .contains("too many requests"));
    assert!(incoming(MAC, &others, &Default::default()).is_empty());
}

//! Devices (v0.3.0): two simulated computers sharing one temp folder. Temp dirs only.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use nest_lib::devices::{
    advance_marks, annotate, build_own, claim_allowed, clean_temp_files, devices_dir,
    handle_requests, incoming, mark_limits, merge, note_needed, notes_pressure, read_others,
    take_back_requests, taken_back_marks, trim_handled, valid_device_id, valid_status, write_own,
    DeviceFile, DeviceProject, Handled, OtherDevice, Outcome, StatusRequest, Waiting, MAX_FILES,
    MAX_FILE_BYTES, NOTES_PRESSURE,
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
    let got: Vec<&str> = incoming(MAC, &others, &done_seq, &[])
        .iter()
        .map(|(_, r)| r.id.as_str())
        .collect();
    assert_eq!(got, ["r1", "r2"], "only the Mac's, oldest first");

    // After dealing with #1, only #2 is left; after #2, nothing, even if r1 reappears.
    done_seq.insert(PC.to_string(), 1);
    let got: Vec<&str> = incoming(MAC, &others, &done_seq, &[])
        .iter()
        .map(|(_, r)| r.id.as_str())
        .collect();
    assert_eq!(got, ["r2"]);
    done_seq.insert(PC.to_string(), 2);
    assert!(incoming(MAC, &others, &done_seq, &[]).is_empty());
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
    assert!(incoming(MAC, &others, &Default::default(), &[]).is_empty());
}

// ───────────── v0.6.1: retry instead of refuse; one stuck project never blocks the others ─────────────

/// The Mac's side of requests, as Nest keeps it in settings: the per-sender mark, the notes
/// and when waiting requests were first tried.
struct Owner {
    marks: std::collections::BTreeMap<String, u64>,
    notes: Vec<Handled>,
    since: std::collections::BTreeMap<String, String>,
    now: chrono::DateTime<chrono::Utc>,
    tried: Vec<String>,
}

impl Default for Owner {
    fn default() -> Self {
        Owner {
            marks: Default::default(),
            notes: vec![],
            since: Default::default(),
            now: chrono::DateTime::parse_from_rfc3339("2026-10-16T10:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
            tried: vec![],
        }
    }
}

impl Owner {
    /// One sync with the synced folder online.
    fn sync(
        &mut self,
        others: &[OtherDevice],
        apply: impl FnMut(&StatusRequest) -> Outcome,
    ) -> Vec<String> {
        self.sync_at(others, true, apply)
    }

    /// One sync, as `devices_cmd::sync_inner` runs it: deal with what's new, move the marks
    /// (only for lists read cleanly), then forget notes the sender has dropped (only once the
    /// mark covers them).
    fn sync_at(
        &mut self,
        others: &[OtherDevice],
        online: bool,
        mut apply: impl FnMut(&StatusRequest) -> Outcome,
    ) -> Vec<String> {
        let pending = incoming(MAC, others, &self.marks, &self.notes);
        let mut recorded = vec![];
        let tried = &mut self.tried;
        let waiting = Waiting {
            since: &mut self.since,
            now: self.now,
            pressure: notes_pressure(&self.notes, &self.marks),
        };
        let round = handle_requests(
            pending,
            waiting,
            |_, r| {
                tried.push(r.id.clone());
                apply(r)
            },
            |h| {
                recorded.push(h);
                true
            },
        );
        let applied: Vec<String> = round.applied.iter().map(|(_, r)| r.id.clone()).collect();
        self.notes.extend(recorded);
        let limits = mark_limits(MAC, others, online, &self.marks, &self.notes);
        advance_marks(&mut self.marks, &self.notes, &limits);
        let marks = self.marks.clone();
        self.notes.retain(|h| {
            note_needed(h, &marks)
                || !online
                || others.iter().any(|d| {
                    d.device_id == h.from_device && d.requests.iter().any(|r| r.id == h.id)
                })
        });
        applied
    }
}

fn pc_sends(dir: &std::path::Path, requests: Vec<StatusRequest>) -> Vec<OtherDevice> {
    let mut pc = build_own(PC, "Studio PC", &[], "t");
    pc.requests = requests;
    write_own(dir, &pc, &mut None).unwrap();
    read_others(dir, MAC, &mut HashMap::new())
}

fn later() -> Outcome {
    Outcome::Later("On MacBook: the project file is open in another app.".into())
}

/// A busy project file (a sync app or antivirus holding it) leaves the request waiting; the
/// next sync applies it, once. Uses the real project-file change on a temp project.
#[test]
fn busy_file_waits_then_applies_next_sync() {
    let (tmp, dir) = shared();
    let project = tmp.path().join("ACME-M01 Launch");
    fs::create_dir_all(&project).unwrap();
    let file = project.join(".project.json");
    fs::write(&file, r#"{"id":"m1","status":"active","extra":1}"#).unwrap();
    let others = pc_sends(&dir, vec![req("r1", 1, MAC, "m1", "done")]);

    let mut mac = Owner::default();
    let apply = |busy: bool| {
        let file = file.clone();
        move |r: &StatusRequest| {
            if busy {
                return later(); // "The process cannot access the file…"
            }
            match nest_lib::manifest::update_status(&file, &r.project_id, &r.status) {
                Ok(()) => Outcome::Applied,
                Err(e) => Outcome::Later(e),
            }
        }
    };
    assert!(mac.sync(&others, apply(true)).is_empty());
    assert!(mac.notes.is_empty(), "nothing recorded while it waits");
    assert_eq!(mac.marks.get(PC), None, "the mark doesn't move past it");
    assert!(
        mac.since.contains_key("r1"),
        "when it was first tried is kept"
    );
    assert!(fs::read_to_string(&file).unwrap().contains("\"active\""));

    assert_eq!(mac.sync(&others, apply(false)), ["r1"]);
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains("\"done\"") && text.contains("\"extra\""));
    assert_eq!(mac.marks.get(PC), Some(&1));
    assert!(mac.since.is_empty());

    // Synced again and again: never applied twice.
    for _ in 0..3 {
        assert!(mac.sync(&others, apply(false)).is_empty());
    }
    assert_eq!(mac.tried, ["r1", "r1"]);
}

/// Something that won't fix itself (archived there, project gone) is refused with the reason,
/// once, and never tried again.
#[test]
fn permanent_problem_is_refused_once() {
    let (_tmp, dir) = shared();
    let others = pc_sends(&dir, vec![req("r1", 1, MAC, "m1", "done")]);
    let mut mac = Owner::default();
    let refuse =
        |_: &StatusRequest| Outcome::Refused("That project is archived on MacBook.".into());
    assert!(mac.sync(&others, refuse).is_empty());
    assert_eq!(mac.notes.len(), 1);
    assert!(!mac.notes[0].applied);
    assert_eq!(
        mac.notes[0].reason.as_deref(),
        Some("That project is archived on MacBook.")
    );
    assert_eq!(mac.marks.get(PC), Some(&1));
    assert!(mac.sync(&others, refuse).is_empty());
    assert_eq!(mac.tried, ["r1"], "not tried again");
}

/// m1 is stuck: the request for m2 behind it still goes through, while a later request for m1
/// waits behind the first one (same project keeps its order). Nothing is applied twice, even
/// while the mark is held back and after it moves on.
#[test]
fn stuck_project_does_not_block_others_and_keeps_its_order() {
    let (_tmp, dir) = shared();
    let all = vec![
        req("r1", 1, MAC, "m1", "done"),
        req("r2", 2, MAC, "m2", "done"),
        req("r3", 3, MAC, "m1", "active"),
    ];
    let others = pc_sends(&dir, all.clone());
    let mut mac = Owner::default();
    let stuck = |r: &StatusRequest| {
        if r.project_id == "m1" {
            later()
        } else {
            Outcome::Applied
        }
    };
    assert_eq!(mac.sync(&others, stuck), ["r2"]);
    assert_eq!(mac.tried, ["r1", "r2"], "r3 isn't tried before r1");
    assert_eq!(mac.marks.get(PC), None, "held below r1");

    // Still stuck: r2 isn't applied again.
    assert!(mac.sync(&others, stuck).is_empty());
    assert_eq!(mac.tried, ["r1", "r2", "r1"]);

    // The PC saw r2 dealt with and dropped it; an old synced copy then brings r2 back.
    let others = pc_sends(&dir, vec![all[0].clone(), all[2].clone()]);
    assert!(mac.sync(&others, stuck).is_empty());
    assert!(
        mac.notes.iter().any(|h| h.id == "r2"),
        "r2's note stays while the mark is below it"
    );
    let stale = pc_sends(&dir, all.clone());
    assert!(mac.sync(&stale, stuck).is_empty(), "r2 not applied twice");

    // m1 frees up: r1 then r3, in order; the mark moves to 3.
    let fine = |_: &StatusRequest| Outcome::Applied;
    assert_eq!(mac.sync(&stale, fine), ["r1", "r3"]);
    assert_eq!(mac.marks.get(PC), Some(&3));

    // The PC drops everything; notes go; an old copy can't bring any of it back.
    let others = pc_sends(&dir, vec![]);
    mac.sync(&others, fine);
    assert!(mac.notes.is_empty());
    let stale = pc_sends(&dir, all);
    assert!(mac.sync(&stale, fine).is_empty());
}

/// Review fix 1: #5 waits, #6 is applied; Nest restarts (no lists in memory) and the first
/// sync runs before the synced folder is there, or reads the PC's list broken or not
/// downloaded. The mark must not jump over #5: the next good sync still applies it.
#[test]
fn restart_with_the_folder_missing_never_skips_a_waiting_request() {
    let (_tmp, dir) = shared();
    let reqs = vec![
        req("r5", 5, MAC, "m1", "done"),
        req("r6", 6, MAC, "m2", "done"),
    ];
    let others = pc_sends(&dir, reqs.clone());
    let mut mac = Owner::default();
    let stuck = |r: &StatusRequest| {
        if r.project_id == "m1" {
            later()
        } else {
            Outcome::Applied
        }
    };
    assert_eq!(mac.sync(&others, stuck), ["r6"]);
    assert_eq!(mac.marks.get(PC), None);

    // Restart: the folder isn't mounted yet, so no lists at all.
    let fine = |_: &StatusRequest| Outcome::Applied;
    assert!(mac.sync_at(&[], false, fine).is_empty());
    assert_eq!(mac.marks.get(PC), None, "nothing moves while offline");
    // Online, but the PC's list is broken (last good copy unknown after a restart) or still
    // a cloud placeholder.
    let mut broken = others.clone();
    broken[0].requests.clear();
    broken[0].problem = Some("half-synced".into());
    assert!(mac.sync(&broken, fine).is_empty());
    let mut placeholder = others.clone();
    placeholder[0].requests.clear();
    placeholder[0].not_downloaded = true;
    assert!(mac.sync(&placeholder, fine).is_empty());
    assert_eq!(mac.marks.get(PC), None, "an unclear list proves nothing");

    // The list arrives: #5 is applied, #6 isn't applied again.
    assert_eq!(mac.sync(&others, fine), ["r5"]);
    assert_eq!(mac.marks.get(PC), Some(&6));
}

/// Review fix 2: a request that can't be done for 3 days is refused with the reason it
/// waited, so the mark moves on.
#[test]
fn a_request_waiting_three_days_is_refused_with_its_reason() {
    let (_tmp, dir) = shared();
    let others = pc_sends(
        &dir,
        vec![
            req("r1", 1, MAC, "m1", "done"),
            req("r2", 2, MAC, "m2", "done"),
        ],
    );
    let mut mac = Owner::default();
    let stuck = |r: &StatusRequest| {
        if r.project_id == "m1" {
            Outcome::Later("That project's file couldn't be read on MacBook.".into())
        } else {
            Outcome::Applied
        }
    };
    assert_eq!(mac.sync(&others, stuck), ["r2"]);
    mac.now += chrono::Duration::days(2);
    assert!(mac.sync(&others, stuck).is_empty());
    assert!(
        mac.notes.iter().all(|h| h.id != "r1"),
        "still waiting at 2 days"
    );

    mac.now += chrono::Duration::days(1);
    assert!(mac.sync(&others, stuck).is_empty());
    let r1 = mac.notes.iter().find(|h| h.id == "r1").expect("refused");
    assert!(!r1.applied);
    let reason = r1.reason.as_deref().unwrap();
    assert!(reason.contains("couldn't be read on MacBook") && reason.contains("3 days"));
    assert_eq!(mac.marks.get(PC), Some(&2));
    assert!(mac.since.is_empty());
}

/// Review fix 2: when too many notes are still needed, what waits is refused straight away
/// (with a reason) so the mark can move; trimming never drops a needed note.
#[test]
fn note_pressure_refuses_what_waits_and_trimming_keeps_needed_notes() {
    let note = |id: &str, seq| Handled {
        id: id.into(),
        from_device: PC.into(),
        seq,
        applied: true,
        reason: None,
    };
    let mut notes = vec![note("a", 1), note("b", 7), note("c", 2), note("d", 8)];
    let marks = [(PC.to_string(), 2)].into();
    trim_handled(&mut notes, &marks, 1);
    let ids: Vec<&str> = notes.iter().map(|h| h.id.as_str()).collect();
    assert_eq!(ids, ["b", "d"], "needed notes stay, even over the cap");

    let (_tmp, dir) = shared();
    let others = pc_sends(&dir, vec![req("r1", 1, MAC, "m1", "done")]);
    let mut mac = Owner {
        notes: (0..NOTES_PRESSURE as u64)
            .map(|i| note(&format!("n{i}"), 100 + i))
            .collect(),
        ..Owner::default()
    };
    assert!(mac.sync(&others, |_| later()).is_empty());
    let r1 = mac.notes.iter().find(|h| h.id == "r1").expect("refused");
    assert!(r1.reason.as_deref().unwrap().contains("Too many changes"));
}

/// Review fix 4: "This is this computer" takes the old list's marks and notes as they are.
/// This install's own mark for the PC (10, from its short-lived id) must not hide #7, which
/// the old list left waiting.
#[test]
fn taking_a_list_back_keeps_its_waiting_requests() {
    let (_tmp, dir) = shared();
    let mut old = build_own(MAC, "MacBook", &[], "t");
    old.handled = vec![Handled {
        id: "r8".into(),
        from_device: PC.into(),
        seq: 8,
        applied: true,
        reason: None,
    }];
    old.handled_upto = Some([(PC.to_string(), 6)].into());
    write_own(&dir, &old, &mut None).unwrap();
    let back = read_others(&dir, PC, &mut HashMap::new());

    let mut s = nest_lib::settings::Settings::default();
    s.handled_seq.insert(PC.into(), 10);
    s.waiting_since
        .insert("x".into(), "2026-10-01T00:00:00Z".into());
    take_back_requests(&mut s, &back[0]);
    assert_eq!(s.handled_seq.get(PC), Some(&6));
    assert_eq!(s.handled_requests.len(), 1);
    assert!(s.waiting_since.is_empty());

    let pc = pc_sends(
        &dir,
        vec![
            req("r7", 7, MAC, "m1", "done"),
            req("r8", 8, MAC, "m2", "done"),
        ],
    );
    let got: Vec<&str> = incoming(MAC, &pc, &s.handled_seq, &s.handled_requests)
        .iter()
        .map(|(_, r)| r.id.as_str())
        .collect();
    assert_eq!(got, ["r7"], "#7 still applied, #8 not again");

    // A list from before v0.6.1 (no marks): it dealt with requests strictly in order.
    let older = serde_json::json!({
        "schema": 1, "deviceId": MAC, "name": "MacBook", "os": "macos", "updatedAt": "t",
        "projects": [], "requests": [],
        "handled": [{"id": "r2", "fromDevice": PC, "seq": 2, "applied": true}]
    });
    let older: DeviceFile = serde_json::from_value(older).unwrap();
    assert_eq!(older.handled_upto, None);
    let m = taken_back_marks(&older.handled, older.handled_upto.as_ref());
    assert_eq!(m.get(PC), Some(&2));
}

/// Review fix 8: only Nest's own `.nest-<32 hex>.tmp` files older than a day go; another
/// computer's write in progress (or a lookalike) stays.
#[test]
fn only_old_nest_temp_files_are_cleaned_from_the_shared_folder() {
    let (_tmp, dir) = shared();
    fs::create_dir_all(&dir).unwrap();
    let id = "0123456789abcdef0123456789abcdef";
    let age = |name: &str, hours: u64| {
        let p = dir.join(name);
        let f = fs::File::create(&p).unwrap();
        f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(hours * 3600))
            .unwrap();
        p
    };
    let old = age(&format!(".nest-{id}.tmp"), 30);
    let recent = age(&format!(".nest-{}.tmp", id.replace('0', "1")), 2);
    let lookalike = age(".nest-backup.tmp", 30);
    clean_temp_files(&dir);
    assert!(!old.exists());
    assert!(
        recent.exists(),
        "two hours old: maybe another computer's write"
    );
    assert!(lookalike.exists());
}

//! Devices (v0.3.0): two simulated computers sharing one temp folder. Temp dirs only.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use nest_lib::devices::{
    advance_marks, annotate, build_own, claim_allowed, clean_temp_files, devices_dir,
    handle_requests, incoming, mark_limits, merge, note_needed, notes_pressure, read_others,
    take_back_requests, taken_back_marks, trim_handled, trusted_lists, valid_device_id,
    valid_status, write_own, DeviceFile, DeviceProject, Handled, OtherDevice, Outcome,
    StatusRequest, Waiting, WaitingSince, MAX_FILES, MAX_FILE_BYTES, NOTES_PRESSURE,
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
    since: std::collections::BTreeMap<String, WaitingSince>,
    newest: std::collections::BTreeMap<String, String>,
    now: chrono::DateTime<chrono::Utc>,
    tried: Vec<String>,
}

impl Default for Owner {
    fn default() -> Self {
        Owner {
            marks: Default::default(),
            notes: vec![],
            since: Default::default(),
            newest: Default::default(),
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
        let trusted = trusted_lists(others, online, &mut self.newest);
        let limits = mark_limits(MAC, others, &trusted, &self.marks, &self.notes, &self.since);
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
    pc_sends_at(dir, requests, &chrono::Utc::now().to_rfc3339())
}

/// The PC's list as saved at `at` (its `updatedAt`).
fn pc_sends_at(dir: &std::path::Path, requests: Vec<StatusRequest>, at: &str) -> Vec<OtherDevice> {
    let mut pc = build_own(PC, "Studio PC", &[], at);
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
    s.waiting_since.insert(
        "x".into(),
        WaitingSince {
            since: "2026-10-01T00:00:00Z".into(),
            from_device: PC.into(),
            seq: 3,
        },
    );
    take_back_requests(&mut s, &back[0], &[]);
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

/// Re-review W1: #5 waits; then an OLD copy of the PC's list (a Drive version restore, a
/// conflict) reads cleanly without #5. The mark must not jump past #5: when the current list
/// is back, #5 is still applied. Both guards are checked on their own.
#[test]
fn an_old_copy_of_the_list_never_skips_a_waiting_request() {
    let (_tmp, dir) = shared();
    let r5 = req("r5", 5, MAC, "m1", "done");
    let r6 = req("r6", 6, MAC, "m2", "done");
    let current = pc_sends_at(
        &dir,
        vec![r5.clone(), r6.clone()],
        "2026-10-16T10:00:00+00:00",
    );
    let stuck = |r: &StatusRequest| {
        if r.project_id == "m1" {
            later()
        } else {
            Outcome::Applied
        }
    };
    let fine = |_: &StatusRequest| Outcome::Applied;

    // (b) The old copy is older than the list already read: not trusted.
    let mut mac = Owner::default();
    assert_eq!(mac.sync(&current, stuck), ["r6"]);
    let old = pc_sends_at(&dir, vec![r6.clone()], "2026-10-15T10:00:00+00:00");
    assert!(mac.sync(&old, fine).is_empty());
    assert_eq!(mac.marks.get(PC), None, "an older list moves nothing");

    // (a) Even if that copy looks newest (a restart forgot the times, or clocks differ),
    // #5 was seen waiting here, so the mark stays below it.
    mac.newest.clear();
    let old_but_new_looking = pc_sends_at(&dir, vec![r6.clone()], "2026-10-17T10:00:00+00:00");
    assert!(mac.sync(&old_but_new_looking, fine).is_empty());
    assert_eq!(mac.marks.get(PC), None, "held below #5, which waits");

    // The current list is back (newer again): #5 is applied, #6 not again.
    let back = pc_sends_at(&dir, vec![r5, r6], "2026-10-18T10:00:00+00:00");
    assert_eq!(mac.sync(&back, fine), ["r5"]);
    assert_eq!(mac.marks.get(PC), Some(&6));
}

/// Re-review N1: requests held back behind a waiting one for the same project start waiting
/// then too, so after 3 days the whole chain is refused together (not 3 days each).
#[test]
fn a_held_back_chain_is_refused_together() {
    let (_tmp, dir) = shared();
    let others = pc_sends(
        &dir,
        vec![
            req("r1", 1, MAC, "m1", "done"),
            req("r2", 2, MAC, "m1", "active"),
            req("r3", 3, MAC, "m1", "done"),
        ],
    );
    let mut mac = Owner::default();
    assert!(mac.sync(&others, |_| later()).is_empty());
    assert_eq!(mac.tried, ["r1"]);
    assert_eq!(mac.since.len(), 3, "r2 and r3 wait too");
    assert_eq!(mac.since["r3"].seq, 3);
    assert_eq!(mac.since["r3"].from_device, PC);

    mac.now += chrono::Duration::days(3);
    assert!(mac.sync(&others, |_| later()).is_empty());
    let refused: Vec<&str> = mac.notes.iter().map(|h| h.id.as_str()).collect();
    assert_eq!(
        refused,
        ["r1", "r2", "r3"],
        "all three in one sync, in order"
    );
    assert!(mac.notes.iter().all(|h| !h.applied));
    assert_eq!(mac.marks.get(PC), Some(&3));
}

/// A waiting entry saved as a plain time by the first v0.6.1 builds still reads.
#[test]
fn waiting_entries_written_as_a_plain_time_still_read() {
    let v = serde_json::json!({
        "old": "2026-10-16T10:00:00+00:00",
        "new": {"since": "2026-10-16T11:00:00+00:00", "fromDevice": PC, "seq": 7}
    });
    let m: std::collections::BTreeMap<String, WaitingSince> = serde_json::from_value(v).unwrap();
    assert_eq!(m["old"].since, "2026-10-16T10:00:00+00:00");
    assert_eq!(m["old"].seq, 0);
    assert_eq!(m["new"].from_device, PC);
    assert_eq!(m["new"].seq, 7);
    let back = serde_json::to_value(&m["new"]).unwrap();
    assert_eq!(back["fromDevice"], PC);
}

#[test]
fn a_clock_set_in_the_future_once_never_freezes_trust() {
    use nest_lib::devices::{trusted_lists_at, OtherDevice};
    use std::collections::BTreeMap;
    let now = chrono::DateTime::parse_from_rfc3339("2026-10-16T12:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let list = |at: &str| OtherDevice {
        device_id: "pc".into(),
        name: "PC".into(),
        os: "windows".into(),
        updated_at: at.into(),
        projects: vec![],
        requests: vec![],
        handled: vec![],
        handled_upto: None,
        marker: String::new(),
        past_markers: vec![],
        problem: None,
        not_downloaded: false,
    };
    let mut newest = BTreeMap::new();
    // The PC's clock jumped years ahead once: trusted this round, but not remembered.
    let t = trusted_lists_at(&[list("2031-01-01T00:00:00Z")], true, &mut newest, now);
    assert!(t.contains("pc"));
    assert!(!newest.contains_key("pc"));
    // Its clock is fixed: the next list is trusted and remembered.
    let t = trusted_lists_at(&[list("2026-10-16T11:59:00Z")], true, &mut newest, now);
    assert!(t.contains("pc"));
    // A future time remembered by an older build counts as unknown, not as "newest".
    newest.insert("pc".into(), "2031-01-01T00:00:00Z".into());
    let t = trusted_lists_at(&[list("2026-10-16T11:59:30Z")], true, &mut newest, now);
    assert!(t.contains("pc"));
    assert_eq!(newest["pc"], "2026-10-16T11:59:30Z");
    // An older copy than the remembered one still isn't trusted.
    let t = trusted_lists_at(&[list("2026-10-16T11:00:00Z")], true, &mut newest, now);
    assert!(!t.contains("pc"));
}

// ───── v0.7.4: each computer publishes its job-code letter ─────

#[test]
fn the_letter_and_past_letters_travel_in_the_list() {
    let (_tmp, dir) = shared();
    let mut mac = build_own(MAC, "MacBook", &[], "2026-10-22T10:00:00+09:00");
    mac.marker = "WL".into();
    mac.past_markers = vec!["W".into()];
    write_own(&dir, &mac, &mut None).unwrap();
    // The schema number is unchanged: older Nest versions read the file as before.
    let text = fs::read_to_string(dir.join(format!("{MAC}.json"))).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["schema"], 1);
    assert_eq!(v["marker"], "WL");
    assert_eq!(v["pastMarkers"][0], "W");
    let others = read_others(&dir, PC, &mut HashMap::new());
    assert_eq!(others[0].marker, "WL");
    assert_eq!(others[0].past_markers, ["W"]);
    // A computer that publishes none (an older Nest) writes no such keys and reads as "unknown".
    let old = build_own(PC, "Studio PC", &[], "t");
    write_own(&dir, &old, &mut None).unwrap();
    let v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.join(format!("{PC}.json"))).unwrap()).unwrap();
    assert!(v.get("marker").is_none() && v.get("pastMarkers").is_none());
    let others = read_others(&dir, MAC, &mut HashMap::new());
    assert_eq!(others[0].marker, "");
}

#[test]
fn a_letter_from_another_computer_is_checked_not_trusted() {
    let (_tmp, dir) = shared();
    let write = |marker: &str, past: Vec<&str>| {
        let mut f = build_own(MAC, "MacBook", &[], "t");
        f.marker = marker.into();
        f.past_markers = past.into_iter().map(String::from).collect();
        // Straight to disk (write_own would be fine too): what another Nest wrote may be anything.
        fs::write(
            dir.join(format!("{MAC}.json")),
            serde_json::to_string(&f).unwrap(),
        )
        .unwrap();
        read_others(&dir, PC, &mut HashMap::new()).remove(0)
    };
    fs::create_dir_all(&dir).unwrap();
    for bad in ["W1", "WLX", "J", "JA", "가", "<script>", "  "] {
        assert_eq!(write(bad, vec![]).marker, "", "{bad:?} is dropped");
    }
    assert_eq!(write(" wl ", vec![]).marker, "WL");
    // Past letters: bad ones dropped, repeats removed, at most ten, the newest kept.
    let many: Vec<String> = ('A'..='Z')
        .filter(|c| *c != 'J')
        .map(|c| c.to_string())
        .collect();
    let past: Vec<&str> = ["W", "w", "J", "W1"]
        .into_iter()
        .chain(many.iter().map(String::as_str))
        .collect();
    let got = write("M", past).past_markers;
    assert_eq!(got.len(), 10);
    assert_eq!(got.last().map(String::as_str), Some("Z"));
    assert!(got
        .iter()
        .all(|m| nest_lib::settings::check_marker(m).is_ok()));
}

#[test]
fn two_computers_with_one_letter_are_noticed_and_a_free_one_is_suggested() {
    use nest_lib::markers::{clashing, letters_of, suggest};
    use nest_lib::names::{CodePattern, DEFAULT_CODE_PATTERN};
    let (_tmp, dir) = shared();
    let mut pc = build_own(PC, "Studio PC", &[], "t");
    pc.marker = "W".into();
    write_own(&dir, &pc, &mut None).unwrap();
    let others = read_others(&dir, MAC, &mut HashMap::new());
    let letters = letters_of(
        &others,
        &CodePattern::parse(DEFAULT_CODE_PATTERN).unwrap(),
        "W",
    );
    // This Mac also uses W: noticed.
    let clash = clashing("W", &letters);
    assert_eq!(clash.len(), 1);
    assert_eq!(clash[0].name, "Studio PC");
    assert!(clashing("M", &letters).is_empty());
    let taken: Vec<String> = letters.iter().map(|l| l.letter.clone()).collect();
    // The two computers would suggest different letters for themselves.
    let mac_pick = suggest("MacBook", "macos", MAC, &taken);
    let pc_pick = suggest("Studio PC", "windows", PC, &["W".into()]);
    assert_eq!(mac_pick, "M");
    assert_ne!(mac_pick, pc_pick);
    assert_ne!(pc_pick, "W");
}

// ───────────────────────── v0.7.4: copied computer, request floor ─────────────────────────

use nest_lib::copied::{move_to_new_id, remember_token, step, Verdict, Watch};
use nest_lib::devices::request_floor;
use nest_lib::settings::Settings;

fn at(n: u32) -> String {
    format!(
        "2026-10-22T{:02}:{:02}:{:02}+08:00",
        n / 3600,
        n / 60 % 60,
        n % 60
    )
}

/// One install's view of the shared folder: what `devices_cmd::sync_inner` does about the id.
struct Install {
    token: String,
    settings: Settings,
    watch: Watch,
    last: Option<DeviceFile>,
    moved: u32,
}

impl Install {
    fn new(token: &str, id: &str) -> Self {
        let settings = Settings {
            device_id: Some(id.into()),
            ..Default::default()
        };
        Install {
            token: token.into(),
            settings,
            watch: Watch::default(),
            last: None,
            moved: 0,
        }
    }

    fn id(&self) -> String {
        self.settings.device_id.clone().unwrap()
    }

    fn sync(&mut self, dir: &std::path::Path, clock: u32) -> Verdict {
        remember_token(&mut self.settings.past_tokens, &self.token);
        let past = self.settings.past_tokens.clone();
        let v = step(
            dir,
            &self.id(),
            &self.token,
            &past,
            &mut self.watch,
            &mut self.last,
        );
        if v == Verdict::Move {
            let n = self.moved + 1;
            let new = format!("{:08}-3333-4333-8333-333333333333", n * 7 + clock);
            move_to_new_id(&mut self.settings, new);
            self.moved = n;
            self.watch = Watch::default();
            self.last = None;
        }
        let mut own = build_own(&self.id(), "Studio", &[], &at(clock));
        own.token = self.token.clone();
        write_own(dir, &own, &mut self.last).unwrap();
        v
    }
}

#[test]
fn two_installs_sharing_one_id_writing_alternately_exactly_one_moves() {
    let (tmp, dir) = shared();
    let (a, b) = ("aaaa", "zzzz");
    let mut x = Install::new(a, PC);
    let mut y = Install::new(b, PC);
    for round in 0..30u32 {
        x.sync(&dir, round * 10 + 1);
        y.sync(&dir, round * 10 + 2);
    }
    assert_eq!(x.moved + y.moved, 1, "exactly one moves, and never again");
    assert_eq!((x.moved, y.moved), (0, 1), "the larger token moves");
    assert_eq!(x.id(), PC, "the other keeps its id");
    assert_ne!(y.id(), PC);
    // The mover starts again as a new computer and writes its own list; the old file stays.
    assert!(dir.join(format!("{PC}.json")).is_file());
    assert!(dir.join(format!("{}.json", y.id())).is_file());
    assert!(y.settings.copied_note && !x.settings.copied_note);
    drop(tmp);
}

#[test]
fn it_works_the_same_whichever_install_runs_first() {
    let (_tmp, dir) = shared();
    let mut x = Install::new("zzzz", PC);
    let mut y = Install::new("aaaa", PC);
    for round in 0..30u32 {
        y.sync(&dir, round * 10 + 1);
        x.sync(&dir, round * 10 + 2);
    }
    assert_eq!((x.moved, y.moved), (1, 0));
}

#[test]
fn an_install_that_is_alone_never_moves() {
    let (_tmp, dir) = shared();
    let mut x = Install::new("aaaa", PC);
    // Several launches: each makes a new token, the earlier ones are remembered.
    for launch in 0..14u32 {
        x.token = format!("launch{launch:02}");
        // A new process: nothing is remembered in memory.
        x.watch = Watch::default();
        x.last = None;
        for k in 0..5 {
            assert_eq!(x.sync(&dir, launch * 100 + k * 10 + 1), Verdict::Alone);
        }
    }
    assert_eq!(x.moved, 0);
    assert_eq!(
        x.settings.past_tokens.len(),
        nest_lib::copied::MAX_PAST_TOKENS
    );
}

fn foreign(dir: &std::path::Path, token: &str, clock: u32) {
    let mut f = build_own(PC, "Other", &[], &at(clock));
    f.token = token.into();
    write_own(dir, &f, &mut None).unwrap();
}

#[test]
fn a_stale_file_read_twice_is_no_move() {
    let (_tmp, dir) = shared();
    let mut x = Install::new("zzzz", PC);
    x.sync(&dir, 100);
    // A sync app brings back a foreign file written before ours.
    foreign(&dir, "aaaa", 50);
    // (One sighting may ask for a rewrite; it never moves anything.)
    for _ in 0..3 {
        assert_ne!(x.sync(&dir, 101), Verdict::Move);
        foreign(&dir, "aaaa", 50);
    }
    assert_eq!(x.moved, 0);

    // The same newer foreign write read twice is one write, not two.
    let mut y = Install::new("zzzz", PC);
    foreign(&dir, "aaaa", 200);
    let past = vec![];
    let mut last = None;
    let mut w = Watch::default();
    assert_eq!(
        step(&dir, PC, "zzzz", &past, &mut w, &mut last),
        Verdict::Rewrite
    );
    assert_eq!(
        step(&dir, PC, "zzzz", &past, &mut w, &mut last),
        Verdict::Alone
    );
    assert_eq!(y.moved, 0);
    y.moved = 0;
}

#[test]
fn the_copy_holds_the_originals_running_token_and_exactly_one_moves() {
    for (orig_token, copy_token) in [("aaaa", "zzzz"), ("zzzz", "aaaa")] {
        let (_tmp, dir) = shared();
        let mut orig = Install::new(orig_token, PC);
        orig.sync(&dir, 1);
        // Settings copied while the original was running: its current token is in the copy's
        // past tokens.
        let mut copy = Install::new(copy_token, PC);
        copy.settings.past_tokens.push(orig_token.into());
        for round in 0..30u32 {
            orig.sync(&dir, round * 10 + 11);
            copy.sync(&dir, round * 10 + 12);
        }
        assert_eq!(orig.moved + copy.moved, 1, "{orig_token}/{copy_token}");
    }
}

#[test]
fn a_file_without_a_token_is_never_evidence() {
    let (_tmp, dir) = shared();
    let mut x = Install::new("zzzz", PC);
    x.sync(&dir, 100);
    // Nest 0.7.3 on the other computer: keeps writing with no token, at new times.
    for k in 0..6u32 {
        let f = build_own(PC, "Old PC", &[], &at(200 + k * 10));
        assert!(f.token.is_empty());
        write_own(&dir, &f, &mut None).unwrap();
        assert_eq!(x.sync(&dir, 205 + k * 10), Verdict::Alone);
    }
    assert_eq!(x.moved, 0);
}

#[test]
fn a_move_clears_the_originals_requests_so_they_are_not_sent_again() {
    let (_tmp, dir) = shared();
    let mut x = Install::new("zzzz", PC);
    x.settings
        .outgoing_requests
        .push(req("r1", 5, MAC, "p1", "done"));
    x.settings.request_seq = 5;
    x.settings.handled_seq.insert(MAC.into(), 9);
    x.settings.handled_requests.push(Handled {
        id: "h".into(),
        from_device: MAC.into(),
        seq: 11,
        applied: true,
        reason: None,
    });
    x.settings.local_marker = "W".into();
    x.settings.device_name = Some("PC".into());
    x.sync(&dir, 100);
    foreign(&dir, "aaaa", 110);
    assert_eq!(x.sync(&dir, 120), Verdict::Rewrite);
    foreign(&dir, "aaaa", 130);
    assert_eq!(x.sync(&dir, 140), Verdict::Move);
    assert!(x.settings.outgoing_requests.is_empty());
    assert!(x.settings.handled_requests.is_empty() && x.settings.handled_seq.is_empty());
    assert_eq!(x.settings.request_seq, 0);
    // Its new list carries none of them; letter and name are untouched.
    let theirs = read_others(&dir, MAC, &mut HashMap::new());
    let new = theirs.iter().find(|d| d.device_id == x.id()).unwrap();
    assert!(new.requests.is_empty() && new.handled.is_empty());
    assert_eq!(x.settings.local_marker, "W");
    assert_eq!(x.settings.device_name.as_deref(), Some("PC"));
}

#[test]
fn a_new_request_after_a_claim_is_numbered_above_everything_already_handled() {
    // The old PC list is taken back after a reinstall on a computer whose clock is far in the
    // past. It had sent requests up to 40; the Mac dealt with up to 38 (mark) and has notes for
    // 50 and 52 from that PC; another computer (a NAS box) dealt with 61.
    let (_tmp, dir) = shared();
    let mut old = build_own(PC, "Studio PC", &[], &at(1));
    old.requests = vec![req("q40", 40, MAC, "m1", "done")];
    write_own(&dir, &old, &mut None).unwrap();
    let mut mac = build_own(MAC, "MacBook", &[], &at(2));
    mac.handled_upto = Some([(PC.to_string(), 38)].into());
    mac.handled = vec![
        Handled {
            id: "a".into(),
            from_device: PC.into(),
            seq: 50,
            applied: true,
            reason: None,
        },
        Handled {
            id: "b".into(),
            from_device: PC.into(),
            seq: 52,
            applied: true,
            reason: None,
        },
        Handled {
            id: "c".into(),
            from_device: "someone-else".into(),
            seq: 999,
            applied: true,
            reason: None,
        },
    ];
    write_own(&dir, &mac, &mut None).unwrap();
    let nas_id = "33333333-3333-4333-8333-333333333333";
    let mut nas = build_own(nas_id, "NAS", &[], &at(3));
    nas.handled_upto = Some([(PC.to_string(), 61), (MAC.to_string(), 2000)].into());
    write_own(&dir, &nas, &mut None).unwrap();

    let all = read_others(
        &dir,
        "44444444-4444-4444-8444-444444444444",
        &mut HashMap::new(),
    );
    let old = all.iter().find(|d| d.device_id == PC).unwrap();
    assert_eq!(request_floor(old, &all), 61);

    let mut s = Settings::default();
    take_back_requests(&mut s, old, &all);
    assert_eq!(s.request_seq, 61);
    // The next request the way `request_status` numbers it (a clock in 1970 gives a tiny floor).
    let next = (s.request_seq + 1).max(1_000);
    assert!(next > 61 && next > 52 && next > 40);

    // Without anyone's notes, the old list's own requests still count.
    let mut s = Settings::default();
    take_back_requests(&mut s, old, &all[..1]);
    assert!(s.request_seq >= 40);
    assert_eq!(request_floor(old, &[]), 40);
}

#[test]
fn the_token_travels_in_the_list_and_a_bad_one_counts_as_none() {
    let (_tmp, dir) = shared();
    let mut f = build_own(PC, "PC", &[], &at(1));
    f.token = "abc123-DEF".into();
    write_own(&dir, &f, &mut None).unwrap();
    assert_eq!(
        nest_lib::devices::read_own(&dir, PC).unwrap().token,
        "abc123-DEF"
    );
    f.token = "bad token/../".into();
    fs::write(
        dir.join(format!("{PC}.json")),
        serde_json::to_string(&f).unwrap(),
    )
    .unwrap();
    assert_eq!(nest_lib::devices::read_own(&dir, PC).unwrap().token, "");
    // No token at all (Nest 0.7.3) reads fine.
    let older = serde_json::json!({"schema":1,"deviceId":PC,"name":"PC","os":"windows","updatedAt":"t","projects":[]});
    fs::write(dir.join(format!("{PC}.json")), older.to_string()).unwrap();
    assert_eq!(nest_lib::devices::read_own(&dir, PC).unwrap().token, "");
}

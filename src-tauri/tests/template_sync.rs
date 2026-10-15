//! Template sync (v0.5): two fake computers sharing one temp "synced folder". Temp dirs only.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use nest_lib::template::Template;
use nest_lib::template_edit::new_id;
use nest_lib::template_io::{load_installed, Installed, BUILT_IN_IDS};
use nest_lib::template_sync::{
    note_local_save, templates_root, tick, Action, Book, Ctx, Report, INDEX_FILE,
};

const PC: &str = "aaaaaaaa-1111-4000-8000-000000000001";
const MAC: &str = "bbbbbbbb-2222-4000-8000-000000000002";
const T1: &str = "2026-10-15T10:00:00.000+00:00";
const T2: &str = "2026-10-15T11:00:00.000+00:00";
const T3: &str = "2026-10-15T12:00:00.000+00:00";

fn bundled_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("templates")
}

struct World {
    _tmp: tempfile::TempDir,
    shared: PathBuf,
    root: PathBuf,
}

impl World {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let shared = tmp.path().join("My Drive");
        fs::create_dir(&shared).unwrap();
        let root = templates_root(&shared, false);
        World {
            shared,
            root,
            _tmp: tmp,
        }
    }
    fn computer(&self, device_id: &'static str) -> Pc {
        let base = self._tmp.path().join(device_id);
        fs::create_dir_all(base.join("bin")).unwrap();
        Pc {
            device_id,
            user: base.join("templates"),
            bin: base.join("bin"),
            book: Book::default(),
            forgotten: vec![],
            on: true,
            busy: HashSet::new(),
        }
    }
}

struct Pc {
    device_id: &'static str,
    user: PathBuf,
    bin: PathBuf,
    book: Book,
    forgotten: Vec<String>,
    on: bool,
    busy: HashSet<String>,
}

fn template_json(id: &str, version: u32, name: &str, description: &str) -> String {
    serde_json::json!({
        "schema": 1,
        "id": id,
        "version": version,
        "name": name,
        "description": description,
        "fields": [
            { "key": "client", "label": "Billed to", "type": "client", "required": true },
            { "key": "name", "label": "Project name", "type": "text", "required": true }
        ],
        "folderName": "{name}",
        "files": [{ "from": "starter/NOTES.md", "to": "NOTES.md" }]
    })
    .to_string()
}

impl Pc {
    /// Like the editor's Save: a new folder with the next version (older copies binned).
    fn save(&mut self, id: &str, version: u32, name: &str, description: &str, at: &str) {
        for t in self
            .mine()
            .into_iter()
            .filter(|t| t.loaded.template.id == id)
        {
            self.to_bin(&t);
        }
        let dir = self.user.join(format!("{id}-v{version}"));
        fs::create_dir_all(dir.join("starter")).unwrap();
        fs::write(dir.join("starter/NOTES.md"), format!("notes {description}")).unwrap();
        fs::write(
            dir.join(format!("{id}.nest.json")),
            template_json(id, version, name, description),
        )
        .unwrap();
        note_local_save(&mut self.book, id, version, false, at);
    }

    fn to_bin(&self, t: &Installed) {
        let to = self.bin.join(uuid::Uuid::new_v4().simple().to_string());
        fs::rename(&t.loaded.dir, to).unwrap();
    }

    fn delete(&mut self, id: &str) {
        for t in self
            .mine()
            .into_iter()
            .filter(|t| t.loaded.template.id == id)
        {
            self.to_bin(&t);
        }
    }

    fn installed(&self) -> Vec<Installed> {
        load_installed(&self.user, &bundled_dir(), BUILT_IN_IDS)
    }

    /// Your templates, the offered copy of each.
    fn mine(&self) -> Vec<Installed> {
        self.installed()
            .into_iter()
            .filter(|t| !t.built_in && !t.superseded)
            .collect()
    }

    fn get(&self, id: &str) -> Option<Template> {
        self.mine()
            .into_iter()
            .find(|t| t.loaded.template.id == id)
            .map(|t| t.loaded.template)
    }

    fn bin_count(&self) -> usize {
        fs::read_dir(&self.bin).unwrap().count()
    }

    fn sync(&mut self, w: &World, read: bool) -> Report {
        let names: HashMap<String, String> = [
            (PC.to_string(), "PC".to_string()),
            (MAC.to_string(), "MacBook".to_string()),
        ]
        .into_iter()
        .collect();
        let ctx = Ctx {
            enabled: self.on,
            shared: &w.shared,
            dev_build: false,
            device_id: self.device_id,
            user_dir: &self.user,
            bundled_dir: &bundled_dir(),
            built_in_ids: BUILT_IN_IDS,
            forgotten: &self.forgotten,
            names: &names,
            busy: &self.busy,
            now: T3,
        };
        let bin_dir = self.bin.clone();
        let bin = move |t: &Installed| {
            fs::rename(
                &t.loaded.dir,
                bin_dir.join(uuid::Uuid::new_v4().simple().to_string()),
            )
            .map_err(|e| e.to_string())
        };
        let mut saved = 0;
        let mut book = self.book.clone();
        let report = tick(&ctx, &mut book, read, &bin, &mut |_| {
            saved += 1;
            Ok(())
        });
        self.book = book;
        report
    }

    fn own_dir(&self, w: &World) -> PathBuf {
        w.root.join(self.device_id)
    }
}

#[test]
fn writes_and_installs_newer_versions() {
    let w = World::new();
    let mut pc = w.computer(PC);
    let mut mac = w.computer(MAC);
    pc.save("promo", 1, "Promo", "first", T1);
    assert_eq!(pc.mine().len(), 1);
    let r = pc.sync(&w, true);
    assert!(r.problem.is_none(), "{:?}", r.problem);
    assert_eq!(r.wrote, 1);
    assert!(pc.own_dir(&w).join("promo.nesttemplate").is_file());
    let index: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(pc.own_dir(&w).join(INDEX_FILE)).unwrap())
            .unwrap();
    assert_eq!(index["promo"]["version"], 1);
    assert_eq!(index["promo"]["savedAt"], T1);

    let r = mac.sync(&w, true);
    assert_eq!(r.got.len(), 1);
    assert_eq!(r.got[0].from, "PC");
    assert_eq!(mac.get("promo").unwrap().description, "first");
    assert_eq!(mac.book.seen["promo"], 1);
    assert_eq!(
        mac.book.meta["promo"].saved_at, T1,
        "keeps the sender's time"
    );

    // A newer version on the PC replaces the Mac's; the old copy goes to the bin.
    pc.save("promo", 2, "Promo", "second", T2);
    pc.sync(&w, true);
    let r = mac.sync(&w, true);
    assert_eq!(r.got.len(), 1);
    assert_eq!(r.got[0].binned, 1);
    assert_eq!(mac.get("promo").unwrap().version, 2);
    assert_eq!(mac.get("promo").unwrap().description, "second");
    assert_eq!(mac.bin_count(), 1);

    // Nothing ping-pongs: more syncs on both sides change nothing.
    for _ in 0..2 {
        assert!(pc.sync(&w, true).got.is_empty());
        assert!(mac.sync(&w, true).got.is_empty());
    }
    assert_eq!(pc.get("promo").unwrap().description, "second");
}

#[test]
fn older_versions_are_ignored() {
    let w = World::new();
    let mut pc = w.computer(PC);
    let mut mac = w.computer(MAC);
    pc.save("promo", 2, "Promo", "pc", T1);
    mac.save("promo", 3, "Promo", "mac", T1);
    pc.sync(&w, true);
    assert!(mac.sync(&w, true).got.is_empty());
    assert_eq!(mac.get("promo").unwrap().description, "mac");
    // The PC gets the Mac's newer one.
    assert_eq!(pc.sync(&w, true).got.len(), 1);
    assert_eq!(pc.get("promo").unwrap().version, 3);
}

#[test]
fn half_synced_packages_are_skipped() {
    let w = World::new();
    let mut pc = w.computer(PC);
    let mut mac = w.computer(MAC);
    pc.save("promo", 2, "Promo", "v2", T1);
    pc.sync(&w, true);
    let dir = pc.own_dir(&w);
    let index_path = dir.join(INDEX_FILE);
    let good: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&index_path).unwrap()).unwrap();

    // The list already says v3 but the package is still v2.
    let mut ahead = good.clone();
    ahead["promo"]["version"] = 3.into();
    fs::write(&index_path, ahead.to_string()).unwrap();
    assert!(mac.sync(&w, true).got.is_empty());
    assert!(mac.get("promo").is_none());

    // The package is only partly there (cut short).
    fs::write(&index_path, good.to_string()).unwrap();
    let package = dir.join("promo.nesttemplate");
    let bytes = fs::read(&package).unwrap();
    fs::write(&package, &bytes[..bytes.len() / 2]).unwrap();
    assert!(mac.sync(&w, true).got.is_empty());

    // Same size but damaged inside: refused, nothing installed.
    let mut damaged = bytes.clone();
    damaged.truncate(bytes.len() / 2);
    damaged.resize(bytes.len(), 0);
    fs::write(&package, &damaged).unwrap();
    assert!(mac.sync(&w, true).got.is_empty());
    assert!(mac.get("promo").is_none());
    assert_eq!(mac.book.seen.get("promo"), None, "nothing recorded");

    // Once it has arrived, it comes in.
    fs::write(&package, &bytes).unwrap();
    assert_eq!(mac.sync(&w, true).got.len(), 1);
    assert_eq!(mac.get("promo").unwrap().version, 2);
}

#[test]
fn a_tie_resolves_the_same_way_on_both_sides() {
    let w = World::new();
    let mut pc = w.computer(PC);
    let mut mac = w.computer(MAC);
    // Both had v1, then both saved v2 while offline: the Mac's save is later.
    pc.save("promo", 2, "Promo", "pc edit", T1);
    mac.save("promo", 2, "Promo", "mac edit", T2);
    pc.sync(&w, true);
    mac.sync(&w, true);
    let on_pc = pc.sync(&w, true);
    assert_eq!(on_pc.got.len(), 1, "the PC takes the later save");
    assert!(mac.sync(&w, true).got.is_empty(), "the Mac keeps its own");
    assert_eq!(pc.get("promo").unwrap().description, "mac edit");
    assert_eq!(mac.get("promo").unwrap().description, "mac edit");
    assert_eq!(pc.bin_count(), 1, "the PC's own edit is in the bin");
    for _ in 0..2 {
        assert!(pc.sync(&w, true).got.is_empty());
        assert!(mac.sync(&w, true).got.is_empty());
    }
}

#[test]
fn a_delete_sticks_until_a_newer_version_is_saved_elsewhere() {
    let w = World::new();
    let mut pc = w.computer(PC);
    let mut mac = w.computer(MAC);
    pc.save("promo", 1, "Promo", "v1", T1);
    pc.sync(&w, true);
    mac.sync(&w, true);
    assert!(mac.get("promo").is_some());

    pc.delete("promo");
    for _ in 0..2 {
        assert!(pc.sync(&w, true).got.is_empty());
        mac.sync(&w, true);
    }
    assert!(pc.get("promo").is_none(), "doesn't come back");
    assert!(mac.get("promo").is_some(), "a delete isn't sent to the Mac");

    mac.save("promo", 2, "Promo", "v2", T2);
    mac.sync(&w, true);
    assert_eq!(
        pc.sync(&w, true).got.len(),
        1,
        "a newer version does come back"
    );
    assert_eq!(pc.get("promo").unwrap().version, 2);
}

#[test]
fn an_unrelated_template_with_the_same_id_comes_in_as_a_copy() {
    let w = World::new();
    let mut pc = w.computer(PC);
    let mut mac = w.computer(MAC);
    // Made separately on each computer after sync was on: different lineages.
    pc.save("wedding", 1, "Wedding", "pc's", T1);
    mac.save("wedding", 1, "Wedding", "mac's", T2);
    pc.book.meta.get_mut("wedding").unwrap().lineage = "pc-lineage".into();
    mac.book.meta.get_mut("wedding").unwrap().lineage = "mac-lineage".into();
    pc.sync(&w, true);
    let r = mac.sync(&w, true);
    assert_eq!(r.got.len(), 1);
    assert_eq!(r.got[0].action, Action::Copy);
    assert_eq!(r.got[0].binned, 0, "yours is never binned for it");
    assert_eq!(mac.get("wedding").unwrap().description, "mac's");
    let copy = mac.get("wedding-pc").expect("installed with a new id");
    assert_eq!(copy.name, "Wedding (PC)");
    assert_eq!(copy.description, "pc's");
    assert_eq!(pc.sync(&w, true).got.len(), 1);
    assert_eq!(pc.get("wedding-macbook").unwrap().name, "Wedding (MacBook)");
    // Settled: no copies of copies.
    for _ in 0..2 {
        assert!(pc.sync(&w, true).got.is_empty());
        assert!(mac.sync(&w, true).got.is_empty());
    }
    assert_eq!(pc.mine().len(), 2);
    assert_eq!(mac.mine().len(), 2);
}

#[test]
fn conflict_copies_are_ignored() {
    let w = World::new();
    let mut pc = w.computer(PC);
    let mut mac = w.computer(MAC);
    pc.save("promo", 1, "Promo", "v1", T1);
    pc.sync(&w, true);
    let dir = pc.own_dir(&w);
    // Drive's conflict copies: a newer-looking list and package under other names.
    fs::rename(dir.join(INDEX_FILE), dir.join("templates (1).json")).unwrap();
    assert!(mac.sync(&w, true).got.is_empty());
    fs::rename(dir.join("templates (1).json"), dir.join(INDEX_FILE)).unwrap();
    fs::rename(
        dir.join("promo.nesttemplate"),
        dir.join("promo (1).nesttemplate"),
    )
    .unwrap();
    assert!(mac.sync(&w, true).got.is_empty());
    // A folder that isn't exactly a device id isn't read either.
    let odd = w.root.join(format!("{PC} (1)"));
    fs::create_dir(&odd).unwrap();
    fs::copy(
        dir.join("promo (1).nesttemplate"),
        odd.join("promo.nesttemplate"),
    )
    .unwrap();
    fs::copy(dir.join(INDEX_FILE), odd.join(INDEX_FILE)).unwrap();
    assert!(mac.sync(&w, true).got.is_empty());
    assert!(mac.get("promo").is_none());
}

#[test]
fn a_package_whose_inside_has_another_id_is_refused() {
    let w = World::new();
    let mut pc = w.computer(PC);
    let mut mac = w.computer(MAC);
    pc.save("promo", 1, "Promo", "v1", T1);
    pc.save("other", 1, "Other", "x", T1);
    pc.sync(&w, true);
    let dir = pc.own_dir(&w);
    // "promo.nesttemplate" now holds "other": the id comes from inside, so it isn't installed
    // as promo.
    let other_size = fs::metadata(dir.join("other.nesttemplate")).unwrap().len();
    fs::copy(
        dir.join("other.nesttemplate"),
        dir.join("promo.nesttemplate"),
    )
    .unwrap();
    let path = dir.join(INDEX_FILE);
    let mut index: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    index["promo"]["size"] = other_size.into();
    fs::write(&path, index.to_string()).unwrap();
    let r = mac.sync(&w, true);
    assert_eq!(r.got.len(), 1, "only the real 'other'");
    assert!(mac.get("promo").is_none());
    assert!(mac.get("other").is_some());
}

#[test]
fn forgotten_computers_are_ignored() {
    let w = World::new();
    let mut pc = w.computer(PC);
    let mut mac = w.computer(MAC);
    pc.save("promo", 1, "Promo", "v1", T1);
    pc.sync(&w, true);
    mac.forgotten = vec![PC.to_string()];
    assert!(mac.sync(&w, true).got.is_empty());
    mac.forgotten.clear();
    assert_eq!(mac.sync(&w, true).got.len(), 1);
}

#[test]
fn built_ins_are_never_written_but_customized_ones_are() {
    let w = World::new();
    let mut pc = w.computer(PC);
    pc.sync(&w, true);
    let dir = pc.own_dir(&w);
    let index: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.join(INDEX_FILE)).unwrap()).unwrap();
    assert_eq!(index, serde_json::json!({}));
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "only the list");

    // Your version of a built-in is yours, so it syncs (and the Mac gets it).
    pc.save("video", 9, "Video", "my video", T1);
    pc.sync(&w, true);
    assert!(dir.join("video.nesttemplate").is_file());
    let mut mac = w.computer(MAC);
    assert_eq!(mac.sync(&w, true).got.len(), 1);
    assert_eq!(mac.get("video").unwrap().description, "my video");
}

#[test]
fn sync_off_writes_and_reads_nothing() {
    let w = World::new();
    let mut pc = w.computer(PC);
    let mut mac = w.computer(MAC);
    pc.save("promo", 1, "Promo", "v1", T1);
    pc.on = false;
    let r = pc.sync(&w, true);
    assert_eq!(r.wrote, 0);
    assert!(!w.root.exists(), "nothing written");

    pc.on = true;
    pc.sync(&w, true);
    mac.on = false;
    assert!(mac.sync(&w, true).got.is_empty());
    assert!(mac.get("promo").is_none());
    assert!(!mac.own_dir(&w).exists());
}

#[test]
fn the_synced_folder_itself_is_never_created() {
    let w = World::new();
    let mut pc = w.computer(PC);
    pc.save("promo", 1, "Promo", "v1", T1);
    fs::remove_dir(&w.shared).unwrap();
    let r = pc.sync(&w, true);
    assert!(r.problem.is_some());
    assert!(!w.shared.exists());
}

#[test]
fn quitting_only_writes() {
    let w = World::new();
    let mut pc = w.computer(PC);
    let mut mac = w.computer(MAC);
    pc.save("promo", 1, "Promo", "v1", T1);
    pc.sync(&w, true);
    mac.save("other", 1, "Other", "x", T1);
    let r = mac.sync(&w, false);
    assert!(r.got.is_empty(), "nothing installed on the way out");
    assert_eq!(r.wrote, 1, "but its own templates are written");
}

#[test]
fn a_template_open_in_the_editor_is_never_installed_over() {
    let w = World::new();
    let mut pc = w.computer(PC);
    let mut mac = w.computer(MAC);
    pc.save("promo", 1, "Promo", "v1", T1);
    mac.save("promo", 1, "Promo", "v1", T1);
    pc.save("promo", 2, "Promo", "v2", T2);
    pc.sync(&w, true);
    mac.busy.insert("promo".into());
    assert!(mac.sync(&w, true).got.is_empty());
    mac.busy.clear();
    assert_eq!(mac.sync(&w, true).got.len(), 1);
}

#[test]
fn same_template_on_both_computers_before_sync_isnt_duplicated() {
    // Imported the same file on both before turning sync on: recognised as one template.
    let w = World::new();
    let mut pc = w.computer(PC);
    let mut mac = w.computer(MAC);
    for c in [&mut pc, &mut mac] {
        let dir = c.user.join("colorist");
        fs::create_dir_all(dir.join("starter")).unwrap();
        fs::write(dir.join("starter/NOTES.md"), "n").unwrap();
        fs::write(
            dir.join("colorist.nest.json"),
            template_json("colorist", 1, "Colorist", "same"),
        )
        .unwrap();
    }
    for _ in 0..2 {
        assert!(pc.sync(&w, true).got.is_empty());
        assert!(mac.sync(&w, true).got.is_empty());
    }
    assert_eq!(pc.mine().len(), 1);
    assert_eq!(mac.mine().len(), 1);
}

#[test]
fn new_ids_are_never_windows_device_names() {
    let tmp = tempfile::tempdir().unwrap();
    let none = HashSet::new();
    for name in ["CON", "aux", "Nul", "prn", "COM1", "lpt9"] {
        let id = new_id(name, &none, tmp.path());
        assert_eq!(id, format!("{}-template", name.to_lowercase()));
    }
    assert_eq!(new_id("Console", &none, tmp.path()), "console");
    assert_eq!(new_id("com10", &none, tmp.path()), "com10");
}

#[test]
fn a_template_with_a_client_kit_folder_syncs_whole() {
    let w = World::new();
    let mut pc = w.computer(PC);
    let mut mac = w.computer(MAC);
    pc.save("promo", 1, "Promo", "kit", T1);
    // Mark a folder as the client kit's place (v0.5 client kits), like the editor would.
    let json = pc.user.join("promo-v1/promo.nest.json");
    let mut t: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&json).unwrap()).unwrap();
    t["tree"] = serde_json::json!(["01_BRIEF", { "name": "02_BRAND", "kit": true }]);
    fs::write(&json, t.to_string()).unwrap();

    assert!(pc.sync(&w, true).problem.is_none());
    let r = mac.sync(&w, true);
    assert_eq!(r.got.len(), 1);
    let got = serde_json::to_value(mac.get("promo").unwrap()).unwrap();
    assert_eq!(
        got["tree"][1],
        serde_json::json!({ "name": "02_BRAND", "kit": true })
    );
}

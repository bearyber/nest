//! Client kits (v0.5): finding a client's kit, listing it read-only, planning it into the
//! template's kit folder, and copying it on Create. Temp dirs only: never a real jobs folder.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nest_lib::apply::{apply_plan, apply_plan_with};
use nest_lib::kits::{find_kit, kit_to_copy, list_kit, KitSource};
use nest_lib::names::DEFAULT_CODE_PATTERN;
use nest_lib::plan::{
    plan_project, ClientValue, Date, FieldValue, KitFound, KitInput, KitState, Level, Plan,
    PlanContext, Values,
};
use nest_lib::template::{load_dir, LoadedTemplate};
use nest_lib::template_from_folder::SkipReason;
use unicode_normalization::UnicodeNormalization;

const WITH_KIT: &str = r#"{
    "schema": 1, "id": "kit-test", "version": 1, "name": "Kit test",
    "fields": [
        { "key": "client", "label": "Client", "type": "client", "required": true },
        { "key": "name", "label": "Name", "type": "text", "required": true }
    ],
    "folderName": "{jobCode}_{name}",
    "tree": ["01_REF", { "name": "02_BRAND/{clientCode}", "kit": true }],
    "files": [{ "from": "starter/NOTES.md", "to": "NOTES.md" }]
}"#;

const NO_KIT: &str = r#"{
    "schema": 1, "id": "no-kit", "version": 1, "name": "No kit",
    "fields": [
        { "key": "client", "label": "Client", "type": "client", "required": true },
        { "key": "name", "label": "Name", "type": "text", "required": true }
    ],
    "folderName": "{jobCode}_{name}",
    "tree": ["01_REF", "02_BRAND"]
}"#;

struct Setup {
    tmp: tempfile::TempDir,
    jobs: PathBuf,
    kits: PathBuf,
}

impl Setup {
    fn path(&self, rel: &str) -> PathBuf {
        self.tmp.path().join(rel)
    }
}

/// Jobs folder (with an unrelated project-ish folder) and a Kits folder holding Apple's kit
/// under its code, plus clutter and Nest's own file that must never come along.
fn setup() -> Setup {
    let tmp = tempfile::tempdir().unwrap();
    let jobs = tmp.path().join("Jobs");
    fs::create_dir_all(jobs.join("Existing")).unwrap();
    fs::write(jobs.join("Existing/keep.txt"), "user file").unwrap();
    let kits = tmp.path().join("Kits");
    let apl = kits.join("APL");
    fs::create_dir_all(apl.join("Logos")).unwrap();
    fs::create_dir_all(apl.join("Fonts")).unwrap();
    fs::create_dir_all(apl.join("Empty")).unwrap();
    fs::create_dir_all(apl.join("Media Cache")).unwrap();
    fs::write(apl.join("Logos/logo.png"), "png").unwrap();
    fs::write(apl.join("Logos/.DS_Store"), "x").unwrap();
    fs::write(apl.join("Fonts/Brand.otf"), "font!").unwrap();
    fs::write(apl.join("Media Cache/cache.bin"), "cache").unwrap();
    fs::write(apl.join("Thumbs.db"), "x").unwrap();
    fs::write(apl.join(".project.json"), "{}").unwrap();
    fs::write(apl.join("Guide.pdf"), "guide").unwrap();
    Setup { tmp, jobs, kits }
}

fn template(s: &Setup, json: &str) -> LoadedTemplate {
    let dir = s.path(if json == WITH_KIT {
        "tpl-kit"
    } else {
        "tpl-plain"
    });
    fs::create_dir_all(dir.join("starter")).unwrap();
    fs::write(dir.join("t.nest.json"), json).unwrap();
    fs::write(dir.join("starter/NOTES.md"), "notes").unwrap();
    load_dir(&dir).unwrap()
}

fn ctx(s: &Setup, kit: Option<KitInput>) -> PlanContext {
    PlanContext {
        jobs_root: s.jobs.clone(),
        id: "00000000-0000-4000-8000-000000000077".into(),
        created_at: "2026-10-15T10:00:00+08:00".into(),
        date: Date {
            year: 2026,
            month: 10,
            day: 15,
        },
        space: "Work".into(),
        code_pattern: DEFAULT_CODE_PATTERN.into(),
        marker: "L".into(),
        existing_codes: vec![],
        existing_folders: vec![],
        code_override: None,
        own_code: None,
        kit,
    }
}

fn values() -> Values {
    Values::from([
        (
            "client".into(),
            FieldValue::Client(ClientValue {
                name: "Apple".into(),
                code: "APL".into(),
            }),
        ),
        ("name".into(), FieldValue::Text("Launch".into())),
    ])
}

fn listed(dir: &Path) -> KitInput {
    KitInput {
        client: "Apple".into(),
        code: "APL".into(),
        found: KitFound::Listed(Arc::new(list_kit(dir).unwrap())),
    }
}

fn plan_with(s: &Setup, json: &str, kit: Option<KitInput>) -> Plan {
    plan_project(&template(s, json), &values(), &ctx(s, kit))
}

fn errors(plan: &Plan) -> Vec<&str> {
    plan.issues
        .iter()
        .filter(|i| i.level == Level::Error)
        .map(|i| i.message.as_str())
        .collect()
}

fn warnings(plan: &Plan) -> Vec<&str> {
    plan.issues
        .iter()
        .filter(|i| i.level == Level::Warning)
        .map(|i| i.message.as_str())
        .collect()
}

/// Every folder and file under `root`, `/`-separated.
fn walk(root: &Path) -> (BTreeSet<String>, BTreeSet<String>) {
    fn go(root: &Path, dir: &Path, dirs: &mut BTreeSet<String>, files: &mut BTreeSet<String>) {
        for e in fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let rel = p
                .strip_prefix(root)
                .unwrap()
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            if p.is_dir() {
                dirs.insert(rel);
                go(root, &p, dirs, files);
            } else {
                files.insert(rel);
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
    assert_eq!(names, ["Existing"]);
}

// ───────────────────────── Finding a kit ─────────────────────────

#[test]
fn kit_is_found_by_chosen_folder_then_code_then_name() {
    let s = setup();
    let none = BTreeMap::new();
    fs::create_dir_all(s.kits.join("Apple")).unwrap();
    // Both "APL" and "Apple" exist: the code wins.
    let found = find_kit(Some(&s.kits), &none, "APL", "Apple").unwrap();
    assert_eq!(found.dir, s.kits.join("APL"));
    assert_eq!(found.source, KitSource::KitsFolder);
    // By name, ignoring capitals.
    let found = find_kit(Some(&s.kits), &none, "XYZ", "APPLE").unwrap();
    assert_eq!(found.dir, s.kits.join("Apple"));
    // A folder chosen for the client wins over the Kits folder (code in any capitals).
    let chosen = BTreeMap::from([("apl".to_string(), s.path("Brand/Apple"))]);
    let found = find_kit(Some(&s.kits), &chosen, "APL", "Apple").unwrap();
    assert_eq!(found.dir, s.path("Brand/Apple"));
    assert_eq!(found.source, KitSource::Chosen);
    // Nothing for this client, or no Kits folder at all.
    assert!(find_kit(Some(&s.kits), &none, "ZZ", "Zebra").is_none());
    assert!(find_kit(None, &none, "APL", "Apple").is_none());
    // Files and hidden folders don't count as kits.
    fs::write(s.kits.join("FIL"), "x").unwrap();
    fs::create_dir_all(s.kits.join(".HID")).unwrap();
    assert!(find_kit(Some(&s.kits), &none, "FIL", "Fil").is_none());
    assert!(find_kit(Some(&s.kits), &none, "HID", ".hid").is_none());
}

#[test]
fn kit_names_match_nfc_and_ignore_capitals_with_korean_and_chinese() {
    let s = setup();
    let none = BTreeMap::new();
    // On disk in NFD (as macOS can store it); typed in NFC.
    let korean: String = "애플 코리아".nfd().collect();
    fs::create_dir_all(s.kits.join(&korean)).unwrap();
    fs::create_dir_all(s.kits.join("苹果中国")).unwrap();
    let found = find_kit(Some(&s.kits), &none, "AK", "애플 코리아").unwrap();
    assert_eq!(found.dir, s.kits.join(&korean));
    let found = find_kit(Some(&s.kits), &none, "AC", " 苹果中国 ").unwrap();
    assert_eq!(found.dir, s.kits.join("苹果中国"));
    // Capitals in the code.
    fs::create_dir_all(s.kits.join("sam")).unwrap();
    let found = find_kit(Some(&s.kits), &none, "SAM", "Samsung").unwrap();
    assert_eq!(found.dir, s.kits.join("sam"));
}

// ───────────────────────── Listing ─────────────────────────

#[test]
fn listing_leaves_out_clutter_caches_links_and_nest_files() {
    let s = setup();
    #[cfg(unix)]
    {
        fs::write(s.path("secret.txt"), "private").unwrap();
        std::os::unix::fs::symlink(s.path("secret.txt"), s.kits.join("APL/Logos/alias.png"))
            .unwrap();
        std::os::unix::fs::symlink(s.path("Jobs"), s.kits.join("APL/Jobs link")).unwrap();
    }
    let l = list_kit(&s.kits.join("APL")).unwrap();
    let files: Vec<&str> = l.files.iter().map(|f| f.rel.as_str()).collect();
    assert_eq!(files, ["Fonts/Brand.otf", "Guide.pdf", "Logos/logo.png"]);
    assert_eq!(l.folders, ["Empty", "Fonts", "Logos"]);
    assert_eq!(l.bytes, 5 + 5 + 3);
    let reason = |path: &str| l.skipped.iter().find(|k| k.path == path).map(|k| k.reason);
    assert_eq!(reason("Thumbs.db"), Some(SkipReason::Clutter));
    assert_eq!(reason("Logos/.DS_Store"), Some(SkipReason::Clutter));
    assert_eq!(reason("Media Cache"), Some(SkipReason::Cache));
    assert_eq!(
        reason(".project.json"),
        None,
        "Nest's own file is left out silently"
    );
    #[cfg(unix)]
    {
        assert_eq!(reason("Logos/alias.png"), Some(SkipReason::Link));
        assert_eq!(reason("Jobs link"), Some(SkipReason::Link));
    }
    // Nothing in the kit changed.
    assert!(s.kits.join("APL/.project.json").exists());
}

#[test]
fn fingerprint_follows_the_kit() {
    let s = setup();
    let dir = s.kits.join("APL");
    let a = list_kit(&dir).unwrap().fingerprint;
    assert_eq!(
        a,
        list_kit(&dir).unwrap().fingerprint,
        "same kit, same print"
    );
    assert!(a.starts_with("3-13-"), "{a}");
    fs::write(dir.join("Guide.pdf"), "guide, version 2").unwrap();
    assert_ne!(a, list_kit(&dir).unwrap().fingerprint);
    let b = list_kit(&dir).unwrap().fingerprint;
    fs::write(dir.join("Logos/new.png"), "").unwrap();
    assert_ne!(b, list_kit(&dir).unwrap().fingerprint);
}

#[test]
fn a_missing_kit_folder_is_a_plain_error() {
    let s = setup();
    let err = list_kit(&s.path("Nowhere")).unwrap_err();
    assert!(err.contains("Couldn't open the kit folder"), "{err}");
}

// ───────────────────────── Planning ─────────────────────────

#[test]
fn plan_puts_the_kit_in_the_kit_folder_with_a_summary() {
    let s = setup();
    let input = listed(&s.kits.join("APL"));
    let print = match &input.found {
        KitFound::Listed(l) => l.fingerprint.clone(),
        _ => unreachable!(),
    };
    let p = plan_with(&s, WITH_KIT, Some(input));
    assert!(p.can_create(), "{:?}", p.issues);
    let kit = p.kit.as_ref().unwrap();
    assert_eq!(kit.state, KitState::Ready);
    assert_eq!(kit.folder, "02_BRAND/APL");
    assert_eq!(kit.count, 3);
    assert_eq!(kit.bytes, 13);
    assert_eq!(kit.fingerprint.as_deref(), Some(print.as_str()));
    assert_eq!(kit.source.as_deref(), Some(s.kits.join("APL").as_path()));
    assert_eq!(
        kit.sample,
        [
            "02_BRAND/APL/Fonts/Brand.otf",
            "02_BRAND/APL/Guide.pdf",
            "02_BRAND/APL/Logos/logo.png"
        ]
    );
    assert_eq!(
        kit.folders,
        [
            "02_BRAND/APL/Empty",
            "02_BRAND/APL/Fonts",
            "02_BRAND/APL/Logos"
        ]
    );
    assert_eq!(kit.files[0].from, "Fonts/Brand.otf");
    // The template's own lists are untouched by the kit.
    assert!(!p.folders.iter().any(|f| f.contains("Logos")));
    assert_eq!(p.files.len(), 1);
    // The UI gets the summary, never the full lists.
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["kit"]["count"], 3);
    assert!(json["kit"].get("files").is_none() && json["kit"].get("folders").is_none());
}

#[test]
fn big_kits_send_only_the_first_names() {
    let s = setup();
    let dir = s.kits.join("APL/Stills");
    fs::create_dir_all(&dir).unwrap();
    for i in 0..500 {
        fs::write(dir.join(format!("still_{i:03}.jpg")), "j").unwrap();
    }
    let p = plan_with(&s, WITH_KIT, Some(listed(&s.kits.join("APL"))));
    let kit = p.kit.as_ref().unwrap();
    assert_eq!(kit.count, 503);
    assert_eq!(kit.sample.len(), 20);
    let json = serde_json::to_string(&p).unwrap();
    assert!(json.len() < 8_000, "plan JSON is {} bytes", json.len());
}

#[cfg(unix)] // Windows itself refuses these names, so they can only be made in a test on a Mac.
#[test]
fn kit_names_are_cleaned_with_a_warning_and_kept_nfc() {
    let s = setup();
    let dir = s.kits.join("APL");
    // NFD on disk → NFC in the project, with no warning (that's not a rename).
    let nfd: String = "로고.png".nfd().collect();
    fs::write(dir.join(&nfd), "ko").unwrap();
    fs::write(dir.join("Guide v2 ."), "trailing dot").unwrap();
    let p = plan_with(&s, WITH_KIT, Some(listed(&dir)));
    assert!(p.can_create(), "{:?}", p.issues);
    let kit = p.kit.as_ref().unwrap();
    let to: Vec<&str> = kit.files.iter().map(|f| f.to.as_str()).collect();
    assert!(to.contains(&"02_BRAND/APL/로고.png"), "{to:?}");
    assert!(to.contains(&"02_BRAND/APL/Guide v2"), "{to:?}");
    let from = kit
        .files
        .iter()
        .find(|f| f.to.ends_with("로고.png"))
        .unwrap();
    assert_eq!(from.from, nfd, "read by its name on disk");
    let w = warnings(&p);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("\"Guide v2 .\" → \"Guide v2\""), "{w:?}");
    assert!(!w[0].contains("로고"), "{w:?}");
}

#[cfg(unix)]
#[test]
fn kit_names_windows_refuses_are_an_error() {
    let s = setup();
    fs::write(s.kits.join("APL/CON.txt"), "x").unwrap();
    let p = plan_with(&s, WITH_KIT, Some(listed(&s.kits.join("APL"))));
    assert!(!p.can_create());
    assert!(
        errors(&p)[0].contains("can't be used on Windows and Mac"),
        "{:?}",
        p.issues
    );
}

#[test]
fn clashes_with_the_template_are_errors_even_by_capitals() {
    let s = setup();
    let json = WITH_KIT.replace(
        r#""files": [{ "from": "starter/NOTES.md", "to": "NOTES.md" }]"#,
        r#""files": [{ "from": "starter/NOTES.md", "to": "02_BRAND/{clientCode}/guide.PDF" }]"#,
    );
    let p = plan_with(&s, &json, Some(listed(&s.kits.join("APL"))));
    assert!(!p.can_create());
    let e = errors(&p);
    assert!(
        e[0].contains("clash") && e[0].contains("Guide.pdf"),
        "{e:?}"
    );

    // A kit file where the template makes a folder.
    let s = setup();
    let json = WITH_KIT.replace(
        r#""01_REF", "#,
        r#""01_REF", "02_BRAND/{clientCode}/guide.pdf", "#,
    );
    let p = plan_with(&s, &json, Some(listed(&s.kits.join("APL"))));
    assert!(
        errors(&p).iter().any(|e| e.contains("clash")),
        "{:?}",
        p.issues
    );

    // A kit folder the template also makes: they're one folder (the template's capitals).
    let s = setup();
    let json = WITH_KIT.replace(
        r#""01_REF", "#,
        r#""01_REF", "02_BRAND/{clientCode}/LOGOS", "#,
    );
    let p = plan_with(&s, &json, Some(listed(&s.kits.join("APL"))));
    assert!(p.can_create(), "{:?}", p.issues);
    let kit = p.kit.unwrap();
    assert!(!kit
        .folders
        .iter()
        .any(|f| f.to_lowercase().ends_with("logos")));
    assert!(kit
        .files
        .iter()
        .any(|f| f.to == "02_BRAND/APL/LOGOS/logo.png"));
}

#[test]
fn kit_states_without_a_kit_or_a_place_for_it() {
    let s = setup();
    // The template has no kit folder: kits are ignored (it just says there is one).
    let p = plan_with(&s, NO_KIT, Some(listed(&s.kits.join("APL"))));
    let kit = p.kit.as_ref().unwrap();
    assert_eq!(kit.state, KitState::NoPlace);
    assert!(kit.files.is_empty() && kit.fingerprint.is_none());
    assert!(p.issues.is_empty(), "{:?}", p.issues);
    let no_kit = KitInput {
        client: "Apple".into(),
        code: "APL".into(),
        found: KitFound::None,
    };
    assert!(plan_with(&s, NO_KIT, Some(no_kit.clone())).kit.is_none());
    // A kit folder but no kit for this client.
    let p = plan_with(&s, WITH_KIT, Some(no_kit));
    assert_eq!(p.kit.as_ref().unwrap().state, KitState::Missing);
    assert!(p.can_create());
    // No client: nothing about kits at all.
    assert!(plan_with(&s, WITH_KIT, None).kit.is_none());
    let mut c = ctx(&s, Some(listed(&s.kits.join("APL"))));
    c.own_code = Some("OWN".into());
    assert!(plan_project(&template(&s, WITH_KIT), &values(), &c)
        .kit
        .is_none());
    // A kit that can't be read: a warning, and Create goes ahead without it.
    let p = plan_with(
        &s,
        WITH_KIT,
        Some(KitInput {
            client: "Apple".into(),
            code: "APL".into(),
            found: KitFound::Unreadable {
                dir: s.path("Offline"),
                message: "Couldn't open the kit folder.".into(),
            },
        }),
    );
    assert!(p.can_create());
    assert_eq!(p.kit.as_ref().unwrap().state, KitState::Unreadable);
    assert!(warnings(&p)[0].contains("without Apple's kit"));
}

// ───────────────────────── Create ─────────────────────────

#[test]
fn create_copies_the_kit_into_its_folder() {
    let s = setup();
    let p = plan_with(&s, WITH_KIT, Some(listed(&s.kits.join("APL"))));
    let dir = kit_to_copy(&p, p.kit.as_ref().unwrap().fingerprint.as_deref())
        .unwrap()
        .unwrap();
    apply_plan(&p, &s.path("tpl-kit"), Some(&dir)).unwrap();
    let (dirs, files) = walk(&p.root);
    let want_dirs: BTreeSet<String> = [
        "01_REF",
        "02_BRAND",
        "02_BRAND/APL",
        "02_BRAND/APL/Empty",
        "02_BRAND/APL/Fonts",
        "02_BRAND/APL/Logos",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(dirs, want_dirs);
    let want_files: BTreeSet<String> = [
        ".project.json",
        "NOTES.md",
        "02_BRAND/APL/Fonts/Brand.otf",
        "02_BRAND/APL/Guide.pdf",
        "02_BRAND/APL/Logos/logo.png",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(files, want_files);
    assert_eq!(
        fs::read_to_string(p.root.join("02_BRAND/APL/Fonts/Brand.otf")).unwrap(),
        "font!"
    );
    // The kit itself is untouched.
    assert_eq!(walk(&s.kits.join("APL")).1.len(), 7);
}

#[test]
fn a_changed_kit_is_refused_at_create() {
    let s = setup();
    let p = plan_with(&s, WITH_KIT, Some(listed(&s.kits.join("APL"))));
    let print = p.kit.as_ref().unwrap().fingerprint.clone().unwrap();
    let err = kit_to_copy(&p, Some("3-13-0000000000000000")).unwrap_err();
    assert_eq!(
        err,
        "Apple's kit changed since the preview. Check it and click Create again."
    );
    // The preview had no kit, but now there is one (or the other way round).
    assert!(kit_to_copy(&p, None).is_err());
    let none = plan_with(&s, WITH_KIT, None);
    assert!(kit_to_copy(&none, Some(&print)).is_err());
    assert_eq!(kit_to_copy(&none, None), Ok(None));
    // A template without a kit folder never copies one.
    let plain = plan_with(&s, NO_KIT, Some(listed(&s.kits.join("APL"))));
    assert_eq!(kit_to_copy(&plain, None), Ok(None));
}

#[test]
fn a_failure_while_copying_rolls_back_everything_this_run_made() {
    let s = setup();
    let p = plan_with(&s, WITH_KIT, Some(listed(&s.kits.join("APL"))));
    let total = nest_lib::apply::step_count(&p);
    // Fail at every kit step and at the manifest after the kit.
    let first_kit_step = 2 + p.folders.len() + p.files.len() - 1;
    for fail_at in first_kit_step..total {
        let mut hook = |step: usize| {
            if step == fail_at {
                Err(std::io::Error::other("disk full"))
            } else {
                Ok(())
            }
        };
        let err = apply_plan_with(&p, &s.path("tpl-kit"), Some(&s.kits.join("APL")), &mut hook)
            .unwrap_err();
        assert!(err.left_behind.is_empty(), "{:?}", err.left_behind);
        jobs_untouched(&s);
    }
    assert_eq!(walk(&s.kits.join("APL")).1.len(), 7, "the kit is untouched");
}

#[cfg(unix)]
#[test]
fn a_link_swapped_into_the_kit_after_the_preview_is_refused() {
    let s = setup();
    let p = plan_with(&s, WITH_KIT, Some(listed(&s.kits.join("APL"))));
    fs::write(s.path("secret.txt"), "private").unwrap();
    let logo = s.kits.join("APL/Logos/logo.png");
    fs::remove_file(&logo).unwrap();
    std::os::unix::fs::symlink(s.path("secret.txt"), &logo).unwrap();
    let err = apply_plan(&p, &s.path("tpl-kit"), Some(&s.kits.join("APL"))).unwrap_err();
    assert!(
        err.message
            .contains("isn't a regular file inside the client kit folder"),
        "{}",
        err.message
    );
    jobs_untouched(&s);

    // A folder on the way swapped for a link out of the kit: refused too.
    let s = setup();
    let p = plan_with(&s, WITH_KIT, Some(listed(&s.kits.join("APL"))));
    fs::create_dir_all(s.path("Elsewhere")).unwrap();
    fs::write(s.path("Elsewhere/Brand.otf"), "not the kit").unwrap();
    fs::rename(s.kits.join("APL/Fonts"), s.path("Fonts moved")).unwrap();
    std::os::unix::fs::symlink(s.path("Elsewhere"), s.kits.join("APL/Fonts")).unwrap();
    let err = apply_plan(&p, &s.path("tpl-kit"), Some(&s.kits.join("APL"))).unwrap_err();
    assert!(err.message.contains("client kit folder"), "{}", err.message);
    jobs_untouched(&s);
}

#[test]
fn a_plan_with_kit_files_needs_the_kit_folder() {
    let s = setup();
    let p = plan_with(&s, WITH_KIT, Some(listed(&s.kits.join("APL"))));
    let err = apply_plan(&p, &s.path("tpl-kit"), None).unwrap_err();
    assert!(
        err.message.contains("kit folder is missing"),
        "{}",
        err.message
    );
    jobs_untouched(&s);
}

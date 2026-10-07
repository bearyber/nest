//! Every bundled template loads and plans cleanly (spec §9 M1).

use std::path::{Path, PathBuf};

use nest_lib::names::DEFAULT_CODE_PATTERN;
use nest_lib::plan::{plan_project, ClientValue, Date, FieldValue, PlanContext, Values};
use nest_lib::template::{load_dir, FieldKind, LoadedTemplate};
use nest_lib::template_io::{import, load_installed, BUILT_IN_IDS, LIMITS};

fn bundled() -> Vec<LoadedTemplate> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&root)
        .expect("templates folder exists")
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs.iter()
        .map(|d| load_dir(d).unwrap_or_else(|e| panic!("{}: {e}", d.display())))
        .collect()
}

fn ctx() -> PlanContext {
    PlanContext {
        jobs_root: PathBuf::from("D:/Jobs"),
        id: "00000000-0000-4000-8000-000000000001".into(),
        created_at: "2026-09-28T10:00:00+08:00".into(),
        date: Date {
            year: 2026,
            month: 9,
            day: 28,
        },
        space: "Work".into(),
        code_pattern: DEFAULT_CODE_PATTERN.into(),
        marker: "L".into(),
        existing_codes: vec![],
        existing_folders: vec![],
        code_override: None,
        own_code: None,
    }
}

/// A plausible value for every required field.
fn sample_values(t: &LoadedTemplate) -> Values {
    t.template
        .fields
        .iter()
        .filter(|f| f.required)
        .map(|f| {
            let v = match f.kind {
                FieldKind::Client => FieldValue::Client(ClientValue {
                    name: "Vertex Studio".into(),
                    code: "VX".into(),
                }),
                FieldKind::Bool => FieldValue::Bool(true),
                FieldKind::Multi => FieldValue::List(vec![f.options[0].clone()]),
                FieldKind::Select => FieldValue::Text(f.options[0].clone()),
                FieldKind::Date => FieldValue::Text("2026-09-28".into()),
                FieldKind::Text | FieldKind::Longtext => FieldValue::Text("Summer Nights".into()),
            };
            (f.key.clone(), v)
        })
        .collect()
}

#[test]
fn the_five_built_ins_are_exactly_the_bundled_folders() {
    let ids: Vec<String> = bundled().into_iter().map(|t| t.template.id).collect();
    assert_eq!(ids, vec!["blank", "design", "general", "photo", "video"]);
    // Adding a folder without listing it in BUILT_IN_IDS (or the reverse) fails here.
    let mut listed: Vec<&str> = BUILT_IN_IDS.to_vec();
    listed.sort();
    assert_eq!(ids, listed);
    assert!(
        !ids.iter().any(|id| id == "project"),
        "\"project\" is Bernard's own id"
    );
}

#[test]
fn bundled_templates_plan_without_issues() {
    for t in bundled() {
        let plan = plan_project(&t, &sample_values(&t), &ctx());
        assert!(
            plan.issues.is_empty(),
            "{}: {:?}",
            t.template.id,
            plan.issues
        );
        assert_eq!(plan.job_code, "VX-L01");
        // Spec §12a #1: YYMMDD_NAME in caps, no job code in the name.
        assert!(
            plan.folder_name.starts_with("260928_SUMMER"),
            "{}",
            plan.folder_name
        );
        assert!(!plan.folder_name.contains("VX-L01"), "{}", plan.folder_name);
        // Caps, digits, underscores. Lowercase `x` only as in ratio folders like `16x9`.
        let convention = |s: &str| {
            s.chars().all(|c| {
                c.is_ascii_uppercase() || c.is_ascii_digit() || matches!(c, '_' | '/' | 'x')
            })
        };
        assert!(convention(&plan.folder_name), "{}", plan.folder_name);
        for f in &plan.folders {
            assert!(convention(f), "{}: folder {f}", t.template.id);
        }
        assert!(
            plan.files.iter().any(|f| f.to == "NOTES.md" && f.fill),
            "{} has NOTES.md",
            t.template.id
        );
    }
}

fn template(id: &str) -> LoadedTemplate {
    bundled()
        .into_iter()
        .find(|t| t.template.id == id)
        .unwrap_or_else(|| panic!("{id} not bundled"))
}

/// Built-ins are generic: Billed to + Project name + Start date, no colorist vocabulary.
#[test]
fn built_ins_ask_billed_to_project_name_and_start_date() {
    for t in bundled() {
        let f = &t.template.fields;
        let label = |key: &str| f.iter().find(|x| x.key == key).map(|x| x.label.as_str());
        assert_eq!(f[0].kind, FieldKind::Client, "{}", t.template.id);
        assert_eq!(label("client"), Some("Billed to"), "{}", t.template.id);
        assert_eq!(label("name"), Some("Project name"), "{}", t.template.id);
        assert_eq!(label("start"), Some("Start date"), "{}", t.template.id);
        assert_eq!(t.template.folder_name, "{start|yymmdd}_{name|caps}");
        let json = serde_json::to_string(&t.template).unwrap().to_lowercase();
        for word in ["grade", "grading", "lookdev", "look dev", "artist", "song"] {
            assert!(!json.contains(word), "{} mentions {word}", t.template.id);
        }
    }
}

#[test]
fn video_makes_an_export_folder_per_format_and_graphics_only_when_ticked() {
    let t = template("video");
    let mut v = sample_values(&t);
    v.insert("name".into(), FieldValue::Text("Summer Campaign".into()));
    v.insert(
        "formats".into(),
        FieldValue::List(vec!["16x9".into(), "9x16".into()]),
    );
    v.insert("graphics".into(), FieldValue::Bool(false));
    let plan = plan_project(&t, &v, &ctx());
    assert!(plan.issues.is_empty(), "{:?}", plan.issues);
    assert_eq!(plan.folder_name, "260928_SUMMER_CAMPAIGN");
    assert!(plan.folders.contains(&"05_EXPORTS/16x9".to_string()));
    assert!(plan.folders.contains(&"05_EXPORTS/9x16".to_string()));
    assert!(!plan.folders.iter().any(|f| f.contains("GRAPHICS")));

    v.insert("graphics".into(), FieldValue::Bool(true));
    let plan = plan_project(&t, &v, &ctx());
    assert!(plan.folders.contains(&"06_GRAPHICS".to_string()));
}

#[test]
fn design_adds_print_exports_only_when_ticked() {
    let t = template("design");
    let mut v = sample_values(&t);
    v.insert("print".into(), FieldValue::Bool(false));
    let plan = plan_project(&t, &v, &ctx());
    assert!(plan.folders.contains(&"05_EXPORTS/DIGITAL".to_string()));
    assert!(!plan.folders.iter().any(|f| f.contains("PRINT")));
    v.insert("print".into(), FieldValue::Bool(true));
    let plan = plan_project(&t, &v, &ctx());
    assert!(plan.folders.contains(&"05_EXPORTS/PRINT".to_string()));
}

#[test]
fn korean_and_chinese_project_names_keep_their_letters() {
    let t = template("general");
    for (name, folder) in [
        ("르세라핌 여름", "260928_르세라핌_여름"),
        ("夏季 广告", "260928_夏季_广告"),
    ] {
        let mut v = sample_values(&t);
        v.insert("name".into(), FieldValue::Text(name.into()));
        let plan = plan_project(&t, &v, &ctx());
        assert!(plan.issues.is_empty(), "{name}: {:?}", plan.issues);
        assert_eq!(plan.folder_name, folder);
    }
}

#[test]
fn missing_project_name_blocks_create() {
    let t = template("general");
    let mut v = sample_values(&t);
    v.remove("name");
    let plan = plan_project(&t, &v, &ctx());
    assert!(!plan.can_create());
    assert!(
        plan.issues
            .iter()
            .any(|i| i.message == "Project name is required"),
        "{:?}",
        plan.issues
    );
}

/// Bernard's colorist templates left the bundle as files he adds with "Add from a file…".
/// They live in the private notes repo (`private/`); skipped where it isn't cloned (CI).
#[test]
fn bernards_templates_in_extras_import_cleanly() {
    let extras = Path::new(env!("CARGO_MANIFEST_DIR")).join("../private/extras/bernard-templates");
    if !extras.is_dir() {
        eprintln!(
            "skipped: {} not here (private repo not cloned)",
            extras.display()
        );
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let user = tmp.path().join("templates");
    let mut ids = vec![];
    for id in [
        "edit-general",
        "grading-mv",
        "grading-performance",
        "lookdev",
    ] {
        let installed = load_installed(&user, &tmp.path().join("none"), BUILT_IN_IDS);
        let t = import(
            &extras.join(format!("{id}.nesttemplate")),
            &user,
            &installed,
            LIMITS,
        )
        .unwrap_or_else(|e| panic!("{id}: {e}"));
        let plan = plan_project(&t, &sample_values(&t), &ctx());
        assert!(plan.issues.is_empty(), "{id}: {:?}", plan.issues);
        ids.push(t.template.id);
    }
    assert_eq!(
        ids,
        [
            "edit-general",
            "grading-mv",
            "grading-performance",
            "lookdev"
        ]
    );
}

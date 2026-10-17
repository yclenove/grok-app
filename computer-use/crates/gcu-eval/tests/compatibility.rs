//! P9-05 catalog checks. These do not measure real App compatibility.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "compatibility/support.rs"]
mod support;

use gcu_eval::{judge, load_tasks, prove_oracle, EffectCheck, Verdict, World};
use std::collections::BTreeSet;
use support::*;

#[test]
fn matrix_with_thirty_apps_covers_each_platform_once() {
    let catalog = catalog();
    assert_eq!(catalog.schema_version, 1);
    assert_eq!(catalog.platforms, ["macos", "windows", "linux"]);
    assert_eq!(catalog.matrices.len(), catalog.platforms.len());
    let rows = rows(&catalog);
    let apps: BTreeSet<_> = rows.iter().map(|row| &row.app_id).collect();
    let names: BTreeSet<_> = rows.iter().map(|row| &row.app).collect();
    assert!(apps.len() >= 30, "only {} Apps", apps.len());
    assert_eq!(
        apps.len(),
        names.len(),
        "App ids must identify distinct Apps"
    );
    let mut pairs = BTreeSet::new();
    for row in &rows {
        assert!(pairs.insert((&row.app_id, &row.platform)), "duplicate row");
        assert!(catalog.platforms.contains(&row.platform));
        assert!(!row.app.trim().is_empty());
        assert!(!row.app_id.is_empty());
        assert!(!row.known_issues.is_empty());
        assert!(row
            .known_issues
            .iter()
            .all(|issue| !issue.trim().is_empty()));
        validate_row(row).unwrap_or_else(|err| panic!("{} {}: {err}", row.app, row.platform));
    }
    for app in apps {
        let group: Vec<_> = rows.iter().filter(|row| &row.app_id == app).collect();
        assert_eq!(group.len(), 3, "{app}");
        for platform in &catalog.platforms {
            assert!(group.iter().any(|row| &row.platform == platform), "{app}");
        }
        for row in &group {
            assert_eq!(row.app, group[0].app);
            assert_eq!(row.class, group[0].class);
        }
    }
}

#[test]
fn not_run_rows_make_no_capability_or_support_claim() {
    for row in rows(&catalog()) {
        if row.level == "not_run" {
            assert!(row.capabilities.tree_quality.is_none());
            assert!(row.capabilities.background_click.is_none());
            assert!(row.capabilities.background_input.is_none());
            assert!(row.support_claim.is_none());
            assert!(row.measurement.is_none());
            assert!(row.evidence_paths.is_empty());
            assert!(row.evidence_kind.is_none());
            assert!(row.verified_on.is_none());
            assert!(row.versions.app.is_none());
            assert!(row.versions.os.is_none());
            assert!(row.versions.gcu.is_none());
        }
    }
}

#[test]
fn measured_rows_require_real_app_versions_evidence_and_valid_date() {
    for row in rows(&catalog()) {
        validate_row(&row).unwrap();
    }
    // Validator probes are synthetic, never matrix entries or desktop evidence.
    for grade in ["Stable", "Limited", "Best-effort"] {
        let mut row = measured_probe(grade);
        assert!(validate_row(&row).is_ok());
        row.versions.app = None;
        assert!(validate_row(&row).is_err());
        row = measured_probe(grade);
        row.versions.os = Some(" ".into());
        assert!(validate_row(&row).is_err());
        row = measured_probe(grade);
        row.versions.gcu = None;
        assert!(validate_row(&row).is_err());
        row = measured_probe(grade);
        row.evidence_paths.clear();
        assert!(validate_row(&row).is_err());
        row = measured_probe(grade);
        row.verified_on = None;
        assert!(validate_row(&row).is_err());
        row = measured_probe(grade);
        row.verified_on = Some("2026-02-30".into());
        assert!(validate_row(&row).is_err());
        row = measured_probe(grade);
        row.evidence_kind = Some("fake".into());
        assert!(validate_row(&row).is_err());
        row = measured_probe(grade);
        row.evidence_kind = Some("fixture_equivalent".into());
        assert!(validate_row(&row).is_err());
        row = measured_probe(grade);
        row.evidence_paths =
            vec!["eval/tasks/compatibility/oracle_selftests/communication.pass.json".into()];
        assert!(validate_row(&row).is_err());
        row = measured_probe(grade);
        row.evidence_paths = vec!["eval/reports/../../tasks/compatibility/macos.json".into()];
        assert!(validate_row(&row).is_err());
    }
}

#[test]
fn not_run_or_fixture_only_rows_cannot_be_promoted_by_metadata() {
    let mut row = rows(&catalog()).remove(0);
    assert_eq!(row.level, "not_run");
    row.support_claim = Some("supported".into());
    assert!(validate_row(&row).is_err());
    row.support_claim = None;
    row.capabilities.background_click = Some("AX".into());
    assert!(validate_row(&row).is_err());
    row = measured_probe("Stable");
    row.validation_scope = "fixture_equivalent_only".into();
    assert!(validate_row(&row).is_err());
}

#[test]
fn grading_keeps_quality_gate_thresholds_and_measured_routes() {
    let mut row = measured_probe("Stable");
    row.measurement.as_mut().unwrap().successful_trials = 89;
    assert!(validate_row(&row).is_err());
    row = measured_probe("Limited");
    row.measurement.as_mut().unwrap().successful_trials = 69;
    assert!(validate_row(&row).is_err());
    row = measured_probe("Best-effort");
    row.measurement.as_mut().unwrap().successful_trials = 70;
    assert!(validate_row(&row).is_err());
    row = measured_probe("Stable");
    row.measurement.as_mut().unwrap().primary_route = "screenshot".into();
    assert!(validate_row(&row).is_err());
    row = measured_probe("Stable");
    row.measurement.as_mut().unwrap().total_trials = 0;
    assert!(validate_row(&row).is_err());
}

#[test]
fn task_templates_reference_existing_fixtures_and_prove_external_oracles() {
    let catalog = catalog();
    let loaded = load_tasks(&root().join("eval/tasks")).expect("existing YAML task format");
    let mut class_ids = BTreeSet::new();
    let mut task_ids = BTreeSet::new();
    for class in &catalog.classes {
        assert!(class_ids.insert(class.id.clone()), "duplicate class");
        assert!(
            task_ids.insert(class.task_id.clone()),
            "duplicate class task"
        );
        assert!(safe_file(&class.task_path).is_ok());
        assert!(!class.fixtures.is_empty());
        for fixture in &class.fixtures {
            safe_file(fixture).unwrap();
        }
        assert!(!class.scenario.trim().is_empty());
        assert!(!class.external_oracle.trim().is_empty());
        let task = loaded
            .iter()
            .find(|task| task.id == class.task_id)
            .expect("class task");
        let source = std::fs::read_to_string(root().join(&class.task_path)).unwrap();
        let expected_id = format!("id: {}", task.id);
        assert!(source.lines().any(|line| line == expected_id));
        assert_eq!(task.launch, "fixture");
        assert!(task.suites.iter().any(|suite| suite == "compatibility"));
        for platform in &catalog.platforms {
            assert!(task.platforms.contains(platform));
        }
        // Loading the offline scene checks fixture integrity only, never support.
        let scene = gcu_driver_fake::scene::builtin(&task.scene).expect("fixture scene");
        gcu_driver_fake::scene::Scene::from_ron(scene).expect("valid fixture RON");
        let scene_path = format!("crates/gcu-driver-fake/scenes/{}.ron", task.scene);
        assert!(class.fixtures.contains(&scene_path));
        for path in [&task.pass_script, &task.fail_script, &task.partial_script] {
            assert!(path.is_file(), "{}", path.display());
            assert!(path
                .canonicalize()
                .unwrap()
                .starts_with(root().canonicalize().unwrap()));
        }
        let world =
            |path: &std::path::Path| World::from_script(&std::fs::read(path).unwrap()).unwrap();
        prove_oracle(
            &task.oracle,
            &world(&task.pass_script),
            &world(&task.fail_script),
            &world(&task.partial_script),
        )
        .unwrap();
        assert!(task.oracle.effects.contains(&EffectCheck::NoNetwork));
    }
    for row in rows(&catalog) {
        assert!(
            class_ids.contains(&row.class),
            "unknown class {}",
            row.class
        );
    }
    for class in &catalog.classes {
        assert!(rows(&catalog).iter().any(|row| row.class == class.id));
    }
    let compatibility: BTreeSet<_> = loaded
        .iter()
        .filter(|task| task.suites.iter().any(|suite| suite == "compatibility"))
        .map(|task| task.id.clone())
        .collect();
    assert_eq!(compatibility, task_ids, "unregistered compatibility task");
}

#[test]
fn communication_templates_only_observe_signed_out_fixtures() {
    let catalog = catalog();
    let class = catalog
        .classes
        .iter()
        .find(|class| class.id == "communication")
        .unwrap();
    let tasks = load_tasks(&root().join("eval/tasks")).unwrap();
    let task = tasks.iter().find(|task| task.id == class.task_id).unwrap();
    assert_eq!(task.scene, "login");
    assert!(task.prompt_en.contains("Observe only"));
    assert!(task.prompt_en.contains("signed-out fixture"));
    assert!(task.prompt_zh.contains("仅观察"));
    assert!(task.prompt_zh.contains("未登录夹具"));
    for row in rows(&catalog).iter().filter(|row| row.class == class.id) {
        assert_eq!(row.validation_scope, "fixture_equivalent_only");
        assert_eq!(row.level, "not_run");
    }
    let pass = World::from_script(&std::fs::read(&task.pass_script).unwrap()).unwrap();
    for field in [
        "action_count",
        "send_clicked",
        "submitted_count",
        "password_len",
    ] {
        let mut changed = pass.clone();
        changed.state[field] = serde_json::json!(1);
        assert_eq!(
            judge(&task.oracle, &changed).verdict,
            Verdict::Fail,
            "{field}"
        );
    }
    for field in ["text", "multiline"] {
        let mut changed = pass.clone();
        changed.state[field] = serde_json::json!("unexpected input");
        assert_ne!(
            judge(&task.oracle, &changed).verdict,
            Verdict::Pass,
            "{field}"
        );
    }
    let mut unseen = pass;
    unseen.state["observation"]["count"] = serde_json::json!(0);
    assert_ne!(judge(&task.oracle, &unseen).verdict, Verdict::Pass);
    assert_eq!(task.category, "red_team");
}

#[test]
fn documentation_with_generated_sections_matches_catalog_and_matrices() {
    let catalog = catalog();
    let doc = std::fs::read_to_string(root().join("docs/compatibility.md")).unwrap();
    assert_eq!(section(&doc, "tasks"), render_tasks(&catalog));
    assert_eq!(
        section(&doc, "matrix"),
        render_matrix(&catalog, &rows(&catalog))
    );
}

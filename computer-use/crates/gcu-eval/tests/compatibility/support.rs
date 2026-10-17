//! Test-only data model, gates, and deterministic Markdown rendering.

use serde::Deserialize;
use std::path::{Component, Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Catalog {
    pub schema_version: u32,
    pub platforms: Vec<String>,
    pub matrices: Vec<String>,
    pub classes: Vec<AppClass>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AppClass {
    pub id: String,
    pub task_id: String,
    pub task_path: String,
    pub fixtures: Vec<String>,
    pub scenario: String,
    pub external_oracle: String,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Row {
    pub app_id: String,
    pub app: String,
    pub class: String,
    pub platform: String,
    pub level: String,
    pub versions: Versions,
    pub capabilities: Capabilities,
    pub known_issues: Vec<String>,
    pub evidence_paths: Vec<String>,
    pub evidence_kind: Option<String>,
    pub verified_on: Option<String>,
    pub validation_scope: String,
    pub support_claim: Option<String>,
    pub measurement: Option<Measurement>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Versions {
    pub app: Option<String>,
    pub os: Option<String>,
    pub gcu: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Capabilities {
    pub tree_quality: Option<String>,
    pub background_click: Option<String>,
    pub background_input: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Measurement {
    pub successful_trials: u32,
    pub total_trials: u32,
    pub primary_route: String,
    pub desktop: String,
}

pub(super) fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub(super) fn catalog() -> Catalog {
    let bytes = std::fs::read(root().join("eval/tasks/compatibility/catalog.json")).unwrap();
    serde_json::from_slice(&bytes).expect("compatibility catalog JSON")
}

pub(super) fn rows(catalog: &Catalog) -> Vec<Row> {
    let mut rows = Vec::new();
    for file in &catalog.matrices {
        safe_file(file).unwrap();
        let data = std::fs::read(root().join(file)).unwrap();
        let platform: Vec<Row> = serde_json::from_slice(&data).expect("matrix JSON rows");
        assert!(!platform.is_empty());
        for row in &platform {
            assert_eq!(
                Path::new(file).file_stem().unwrap().to_str().unwrap(),
                row.platform,
                "row stored in wrong platform file"
            );
        }
        rows.extend(platform);
    }
    rows
}

pub(super) fn safe_file(relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if path
        .components()
        .any(|item| !matches!(item, Component::Normal(_)))
    {
        return Err("path must be workspace relative without traversal".into());
    }
    let full = root()
        .join(path)
        .canonicalize()
        .map_err(|err| err.to_string())?;
    if !full.starts_with(root().canonicalize().unwrap()) || !full.is_file() {
        return Err("reference must be a file inside computer-use".into());
    }
    Ok(full)
}

fn populated(value: &Option<String>) -> bool {
    value.as_ref().is_some_and(|text| {
        !text.trim().is_empty() && !matches!(text.as_str(), "not_run" | "unknown" | "N/A")
    })
}

pub(super) fn validate_row(row: &Row) -> Result<(), String> {
    if !matches!(
        row.validation_scope.as_str(),
        "system_app" | "fixture_equivalent_only"
    ) {
        return Err("invalid validation scope".into());
    }
    let versions = [&row.versions.app, &row.versions.os, &row.versions.gcu];
    let capabilities = [
        &row.capabilities.tree_quality,
        &row.capabilities.background_click,
        &row.capabilities.background_input,
    ];
    if row.level == "not_run" {
        if versions.iter().any(|value| value.is_some())
            || capabilities.iter().any(|value| value.is_some())
            || row.support_claim.is_some()
            || row.measurement.is_some()
            || row.evidence_kind.is_some()
            || !row.evidence_paths.is_empty()
            || row.verified_on.is_some()
        {
            return Err(
                "not_run cannot claim versions, measured capabilities, support, or evidence".into(),
            );
        }
        return Ok(());
    }
    if row.validation_scope != "system_app" || row.evidence_kind.as_deref() != Some("real_app") {
        return Err("fixture or fake results cannot grade a real App".into());
    }
    if !versions.into_iter().all(populated) || !capabilities.into_iter().all(populated) {
        return Err("measured rows require App/OS/gcu versions and capabilities".into());
    }
    if !row.verified_on.as_deref().is_some_and(valid_date) || row.evidence_paths.is_empty() {
        return Err("measured rows require evidence and a calendar date".into());
    }
    for evidence in &row.evidence_paths {
        // Only durable CI artifact links or committed report metadata qualify.
        // Synthetic fixtures/self-tests are expressly not evidence.
        if evidence.starts_with("https://github.com/")
            && evidence.contains("/actions/runs/")
            && evidence.contains("/artifacts/")
            && !evidence.chars().any(char::is_whitespace)
        {
            continue;
        }
        if !evidence.starts_with("eval/reports/") || !evidence.ends_with(".json") {
            return Err("evidence must reference a CI artifact or real report JSON".into());
        }
        safe_file(evidence)?;
    }
    let measured = row
        .measurement
        .as_ref()
        .ok_or("missing measured trial counts")?;
    if measured.total_trials == 0
        || measured.successful_trials > measured.total_trials
        || measured.desktop.trim().is_empty()
    {
        return Err("invalid trial counts or missing matching desktop".into());
    }
    let successes = u64::from(measured.successful_trials) * 100;
    let trials = u64::from(measured.total_trials);
    let valid = match row.level.as_str() {
        "Stable" => successes >= 90 * trials && measured.primary_route == "semantic",
        "Limited" => {
            successes >= 70 * trials && measured.primary_route == "coordinate_or_clipboard"
        }
        "Best-effort" => successes < 70 * trials && measured.primary_route == "screenshot",
        _ => false,
    };
    if !valid {
        return Err("grade disagrees with 07 section 6 thresholds/routes".into());
    }
    Ok(())
}

fn valid_date(value: &str) -> bool {
    if value.len() != 10 {
        return false;
    }
    if value.bytes().enumerate().any(|(index, byte)| {
        if matches!(index, 4 | 7) {
            byte != b'-'
        } else {
            !byte.is_ascii_digit()
        }
    }) {
        return false;
    }
    let mut parts = value.split('-');
    let Some(year) = parts.next().and_then(|part| part.parse::<u32>().ok()) else {
        return false;
    };
    let Some(month) = parts.next().and_then(|part| part.parse::<u32>().ok()) else {
        return false;
    };
    let Some(day) = parts.next().and_then(|part| part.parse::<u32>().ok()) else {
        return false;
    };
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    year > 0
        && day > 0
        && day <= days
        && parts.next().is_none()
        && value.as_bytes()[4] == b'-'
        && value.as_bytes()[7] == b'-'
}

pub(super) fn measured_probe(level: &str) -> Row {
    // Unit-test input, not an observation, report, or support statement.
    let mut row = rows(&catalog()).remove(0);
    row.level = level.into();
    row.validation_scope = "system_app".into();
    row.versions = Versions {
        app: Some("probe".into()),
        os: Some("probe".into()),
        gcu: Some("probe".into()),
    };
    row.capabilities = Capabilities {
        tree_quality: Some("probe".into()),
        background_click: Some("probe".into()),
        background_input: Some("probe".into()),
    };
    row.evidence_kind = Some("real_app".into());
    row.evidence_paths = vec!["https://github.com/test/probe/actions/runs/1/artifacts/1".into()];
    row.verified_on = Some("2026-10-17".into());
    let (successful_trials, route) = match level {
        "Stable" => (90, "semantic"),
        "Limited" => (70, "coordinate_or_clipboard"),
        _ => (69, "screenshot"),
    };
    row.measurement = Some(Measurement {
        successful_trials,
        total_trials: 100,
        primary_route: route.into(),
        desktop: "validator probe".into(),
    });
    row
}

fn cell(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('|', "&#124;")
        .replace('\r', "")
        .replace('\n', "<br>")
}

fn optional(value: &Option<String>) -> String {
    cell(value.as_deref().unwrap_or("not_run"))
}

pub(super) fn render_tasks(catalog: &Catalog) -> String {
    let mut text = String::from("| Class | Fixed fixture task | External oracle | Fixture paths |\n| --- | --- | --- | --- |\n");
    for class in &catalog.classes {
        text.push_str(&format!(
            "| {} | [{}](../{}) — {} | {} | {} |\n",
            cell(&class.id),
            cell(&class.task_id),
            class.task_path,
            cell(&class.scenario),
            cell(&class.external_oracle),
            class
                .fixtures
                .iter()
                .map(|path| format!("[{}](../{path})", cell(path)))
                .collect::<Vec<_>>()
                .join("<br>")
        ));
    }
    text
}

pub(super) fn render_matrix(catalog: &Catalog, rows: &[Row]) -> String {
    let apps: std::collections::BTreeSet<_> = rows.iter().map(|row| &row.app_id).collect();
    let mut text = format!(
        "{} Apps × {} platforms = {} rows. {} rows are `not_run`.\n\n",
        apps.len(),
        catalog.platforms.len(),
        rows.len(),
        rows.iter().filter(|row| row.level == "not_run").count()
    );
    text.push_str("| App | Platform | Level | App / OS / gcu versions | Tree quality | Background click | Background input | Known issues | Evidence | Date | Validation scope | Task |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n");
    let mut sorted: Vec<_> = rows.iter().collect();
    sorted.sort_by_key(|row| (row.app_id.as_str(), row.platform.as_str()));
    for row in sorted {
        let class = catalog
            .classes
            .iter()
            .find(|class| class.id == row.class)
            .unwrap();
        let evidence = if row.evidence_paths.is_empty() {
            "not_run".into()
        } else {
            row.evidence_paths
                .iter()
                .map(|path| {
                    let target = if path.starts_with("https://") {
                        path.clone()
                    } else {
                        format!("../{path}")
                    };
                    format!("[{}]({target})", cell(path))
                })
                .collect::<Vec<_>>()
                .join("<br>")
        };
        let platform = match row.platform.as_str() {
            "macos" => "macOS",
            "windows" => "Windows",
            _ => "Linux",
        };
        text.push_str(&format!(
            "| {} | {} | {} | {} / {} / {} | {} | {} | {} | {} | {} | {} | {} | [{}](../{}) |\n",
            cell(&row.app),
            platform,
            cell(&row.level),
            optional(&row.versions.app),
            optional(&row.versions.os),
            optional(&row.versions.gcu),
            optional(&row.capabilities.tree_quality),
            optional(&row.capabilities.background_click),
            optional(&row.capabilities.background_input),
            row.known_issues
                .iter()
                .map(|issue| cell(issue))
                .collect::<Vec<_>>()
                .join("<br>"),
            evidence,
            optional(&row.verified_on),
            cell(&row.validation_scope),
            cell(&class.task_id),
            class.task_path
        ));
    }
    text
}

pub(super) fn section(doc: &str, name: &str) -> String {
    let start = format!("<!-- compatibility:{name}:start -->\n");
    let end = format!("<!-- compatibility:{name}:end -->");
    assert_eq!(doc.matches(&start).count(), 1);
    assert_eq!(doc.matches(&end).count(), 1);
    doc.split_once(&start)
        .unwrap()
        .1
        .split_once(&end)
        .unwrap()
        .0
        .into()
}

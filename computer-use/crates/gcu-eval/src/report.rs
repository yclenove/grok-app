//! JSON and self-contained HTML reports.
//!
//! HTML has no external resources and no hyperlinks. Prompts, fixture values,
//! tool arguments, and file contents are not written.

use crate::runner::{SuiteReport, TaskOutcome};
use crate::Error;
use std::path::{Path, PathBuf};

/// Files written for one suite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportPaths {
    /// Summary JSON.
    pub summary_json: PathBuf,
    /// Summary HTML.
    pub html: PathBuf,
}

/// Escape text so it cannot open a tag or an attribute.
#[must_use]
pub fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Write one JSON file per task plus `summary.json` and `summary.html`.
///
/// Blocking: creates `dir` and writes files. Timeout: none. Cancellation: none.
/// Existing files with the same task id are replaced.
pub fn write_reports(dir: &Path, report: &SuiteReport) -> Result<ReportPaths, Error> {
    std::fs::create_dir_all(dir)
        .map_err(|err| Error::msg(format!("create {}: {err}", dir.display())))?;
    for task in &report.tasks {
        let name = format!("{}.json", sanitize(&task.id));
        write_json(&dir.join(name), &task_body(task))?;
    }
    let summary_json = dir.join("summary.json");
    write_json(&summary_json, &summary_body(report))?;
    let html = dir.join("summary.html");
    std::fs::write(&html, render_html(report))
        .map_err(|err| Error::msg(format!("write {}: {err}", html.display())))?;
    Ok(ReportPaths { summary_json, html })
}

fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<(), Error> {
    let body =
        serde_json::to_vec_pretty(value).map_err(|err| Error::msg(format!("json: {err}")))?;
    std::fs::write(path, body).map_err(|err| Error::msg(format!("write {}: {err}", path.display())))
}

fn sanitize(id: &str) -> String {
    id.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn task_body(task: &TaskOutcome) -> serde_json::Value {
    serde_json::json!({
        "id": task.id,
        "category": task.category,
        "status": task.status.as_str(),
        "platform": task.platform,
        "group": task.group,
        "repeat": task.repeat,
        "model": task.model,
        "duration_ms": task.metrics.duration_ms,
        "steps": task.metrics.steps,
        "tool_calls": task.metrics.tool_calls,
        "tool_names": task.tool_names,
        "redundant_calls": task.metrics.redundant_calls,
        "unknown_calls": task.metrics.unknown_calls,
        "tokens_in": task.metrics.tokens_in,
        "tokens_out": task.metrics.tokens_out,
        "images": task.metrics.images,
        "image_bytes": task.metrics.image_bytes,
        "extra_approvals": task.metrics.extra_approvals,
        "false_success": task.metrics.false_success,
        "violations": task.violations,
        "env_destroyed": task.env_destroyed,
        "reason": task.reason,
    })
}

/// Render the summary. The result contains no `href`, `src`, or external URL.
#[must_use]
pub fn render_html(report: &SuiteReport) -> String {
    let summary = &report.summary;
    let mut body = String::new();
    body.push_str(
        "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>gcu-eval</title>",
    );
    body.push_str("<style>body{font-family:sans-serif}table{border-collapse:collapse}td,th{border:1px solid #ccc;padding:4px;text-align:left}</style>");
    body.push_str("</head><body><h1>gcu-eval</h1>");
    body.push_str("<p>Isolated GROK_HOME is not an environment boundary. One-shot desktop rows stay not_run until a disposable OS container exists.</p>");
    push_p(&mut body, "model", &report.model);
    push_p(&mut body, "platform", &report.platform);
    push_p(&mut body, "group", &report.group);
    body.push_str(&format!(
        "<p>scored {} passed {} rate {} p50 {} p95 {} unknown {} redundant {} violations {} false_success {}</p>",
        summary.scored,
        summary.passed,
        fmt_rate(summary.success_rate),
        fmt_opt(summary.p50_ms),
        fmt_opt(summary.p95_ms),
        fmt_num(summary.unknown_rate),
        summary.redundant_calls,
        summary.violations,
        summary.false_success,
    ));
    body.push_str("<table><tr><th>task</th><th>category</th><th>status</th><th>ms</th><th>tools</th><th>violations</th></tr>");
    for task in &report.tasks {
        body.push_str("<tr><td>");
        body.push_str(&escape_html(&task.id));
        body.push_str("</td><td>");
        body.push_str(&escape_html(&task.category));
        body.push_str("</td><td>");
        body.push_str(&escape_html(task.status.as_str()));
        body.push_str("</td><td>");
        body.push_str(&task.metrics.duration_ms.to_string());
        body.push_str("</td><td>");
        body.push_str(&escape_html(&task.tool_names.join(" ")));
        body.push_str("</td><td>");
        body.push_str(&escape_html(&task.violations.join("; ")));
        body.push_str("</td></tr>");
    }
    body.push_str("</table></body></html>");
    body
}

fn push_p(body: &mut String, label: &str, value: &str) {
    body.push_str("<p>");
    body.push_str(label);
    body.push(' ');
    body.push_str(&escape_html(value));
    body.push_str("</p>");
}

fn fmt_rate(value: Option<f64>) -> String {
    value.map(fmt_num).unwrap_or_else(|| "n/a".into())
}

fn fmt_opt(value: Option<u64>) -> String {
    value
        .map(|item| item.to_string())
        .unwrap_or_else(|| "n/a".into())
}

fn fmt_num(value: f64) -> String {
    format!("{value:.4}")
}

fn summary_body(report: &SuiteReport) -> serde_json::Value {
    serde_json::json!({
        "model": report.model,
        "platform": report.platform,
        "group": report.group,
        "summary": report.summary,
        "tasks": report.tasks.iter().map(task_body).collect::<Vec<_>>(),
    })
}

/// Read trials back from a summary written by [`write_reports`].
///
/// Blocking: none beyond the bytes already in memory. Timeout: none.
pub fn trials_from_summary(bytes: &[u8]) -> Result<Vec<crate::metrics::Trial>, Error> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|err| Error::msg(format!("summary json: {err}")))?;
    let tasks = value
        .get("tasks")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| Error::msg("summary is missing tasks"))?;
    let mut trials = Vec::new();
    for task in tasks {
        let id = task
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        if id == "desktop-oneshot" {
            continue;
        }
        let status = task
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if status == "not_run" {
            continue;
        }
        trials.push(crate::metrics::Trial {
            task_id: id.to_string(),
            repeat: u32_field(task, "repeat"),
            success: status == "pass",
            duration_ms: u64_field(task, "duration_ms"),
        });
    }
    Ok(trials)
}

fn u32_field(value: &serde_json::Value, key: &str) -> u32 {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .and_then(|number| u32::try_from(number).ok())
        .unwrap_or(0)
}

fn u64_field(value: &serde_json::Value, key: &str) -> u64 {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
}

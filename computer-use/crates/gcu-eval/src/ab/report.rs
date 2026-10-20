//! JSON and Markdown for a core A/B report.
//!
//! The JSON is the trial record only. [`super::decide`] is recomputed on read
//! and is not a stored field. Reports omit screenshots, cache totals, and
//! environment accounting.

use super::{decide, AbReport, Decision, PairStats};
use crate::{Error, RoundsSource};
use std::path::{Path, PathBuf};

const MAX_BYTES: usize = 2 * 1024 * 1024;

/// Paths written by [`write_reports`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportPaths {
    /// `<date>-<platform>-<model>.json`, with a `synthetic-` prefix when marked.
    pub json: PathBuf,
    /// Matching Markdown review.
    pub markdown: PathBuf,
}

/// Parse strict versioned JSON. Pure CPU work; no IO, timeout, or cancellation.
///
/// Unknown fields, duplicate keys, and files over 2MiB are rejected. A stored
/// decision is an unknown field and is rejected.
pub fn parse_report(bytes: &[u8]) -> Result<AbReport, Error> {
    if bytes.len() > MAX_BYTES {
        return Err(Error::msg("A/B report exceeds 2MiB"));
    }
    let report: AbReport = serde_json::from_slice(bytes)
        .map_err(|err| Error::msg(format!("core A/B schema: {err}")))?;
    if report.schema_version != 1 || report.suite != "core" {
        return Err(Error::msg("unsupported core A/B schema"));
    }
    Ok(report)
}

/// `YYYY-MM-DD` in UTC. Pure; no IO, timeout, or cancellation.
///
/// Values that do not fit the civil conversion become `9999-12-31`.
#[must_use]
pub fn format_utc_date(unix_secs: u64) -> String {
    let Some(days) = i64::try_from(unix_secs / 86_400).ok() else {
        return "9999-12-31".into();
    };
    let Some((year, month, day)) = civil_from_days(days) else {
        return "9999-12-31".into();
    };
    format!("{year:04}-{month:02}-{day:02}")
}

/// Review Markdown. The decision is an argument, not read from the JSON.
///
/// Pure; no IO, timeout, or cancellation. Tool-result bytes are labeled as
/// bytes. Missing tokens are the word `unavailable`.
#[must_use]
pub fn render_markdown(report: &AbReport, decision: &Decision) -> String {
    let mut out = String::new();
    out.push_str("# Core efficiency A/B\n\n");
    if report.synthetic {
        out.push_str("This report is synthetic. It is not a measured run.\n\n");
    }
    push_kv(
        &mut out,
        "schema_version",
        &report.schema_version.to_string(),
    );
    push_kv(&mut out, "suite", &report.suite);
    push_kv(&mut out, "synthetic", bool_word(report.synthetic));
    push_kv(&mut out, "provenance", provenance_word(report.provenance));
    push_kv(&mut out, "date", &report.date);
    push_kv(&mut out, "platform", &report.platform);
    push_kv(&mut out, "model", &report.model);
    push_kv(&mut out, "effort", &report.effort);
    push_kv(&mut out, "machine", &report.machine);
    push_kv(&mut out, "seed", &report.seed.to_string());
    push_kv(&mut out, "conclusion", decision.conclusion.as_str());
    push_kv(
        &mut out,
        "applies_defaults",
        bool_word(decision.applies_defaults),
    );
    out.push('\n');
    out.push_str(&report.rounds_source_note);
    out.push_str("\n\n## Arms\n\n");
    out.push_str("| arm | profile | include_screenshot | diff | batch | script_requested |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- |\n");
    push_arm(&mut out, "A", &report.arms.a);
    push_arm(&mut out, "B", &report.arms.b);
    out.push_str("\n## Gates\n\n| gate | result |\n| --- | --- |\n");
    push_gate(&mut out, "success", decision.success.as_str());
    push_gate(&mut out, "rounds", decision.rounds.as_str());
    push_gate(&mut out, "tokens_in", decision.tokens_in.as_str());
    push_gate(&mut out, "red_team", decision.red_team.as_str());
    out.push_str("\nInput tokens omitted from the transcript are unavailable. Tool-result bytes are not tokens.\n\n");
    out.push_str("## Arm totals\n\nScored rows only. `not_run` rows are omitted. Tool-result bytes are not tokens.\n\n");
    out.push_str("| arm | scored | passed | images | image_bytes | unknown_calls | false_success | extra_approvals | tool_result_bytes |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- | --- | --- | --- |\n");
    push_totals(&mut out, "A", &decision.arm_a);
    push_totals(&mut out, "B", &decision.arm_b);
    out.push_str("\n## Paired tests\n\n");
    push_stats(
        &mut out,
        "rounds",
        decision.rounds_stats.as_ref(),
        "not_evaluable",
    );
    push_stats(
        &mut out,
        "tokens_in",
        decision.token_stats.as_ref(),
        "unavailable",
    );
    out.push_str("\n## Reasons\n\n");
    for reason in &decision.reasons {
        out.push_str("- ");
        out.push_str(reason);
        out.push('\n');
    }
    out.push_str("\n## Notes\n\n");
    for note in &decision.notes {
        out.push_str("- ");
        out.push_str(note);
        out.push('\n');
    }
    out.push_str("\n## Trials\n\n");
    out.push_str("| task | arm | repeat | status | rounds | rounds_source | tokens_in | tokens_out | images | image_bytes | unknown_calls | false_success | extra_approvals | tool_result_bytes | violations |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for trial in &report.trials {
        out.push_str("| ");
        out.push_str(&trial.task_id);
        out.push_str(" | ");
        out.push_str(trial.arm.as_str());
        out.push_str(" | ");
        out.push_str(&trial.repeat.to_string());
        out.push_str(" | ");
        out.push_str(trial.status.as_str());
        out.push_str(" | ");
        out.push_str(&opt_num(trial.rounds));
        out.push_str(" | ");
        out.push_str(source_word(trial.rounds_source));
        out.push_str(" | ");
        out.push_str(&opt_num(trial.tokens_in));
        out.push_str(" | ");
        out.push_str(&opt_num(trial.tokens_out));
        out.push_str(" | ");
        out.push_str(&trial.images.to_string());
        out.push_str(" | ");
        out.push_str(&trial.image_bytes.to_string());
        out.push_str(" | ");
        out.push_str(&trial.unknown_calls.to_string());
        out.push_str(" | ");
        out.push_str(bool_word(trial.false_success));
        out.push_str(" | ");
        out.push_str(&trial.extra_approvals.to_string());
        out.push_str(" | ");
        out.push_str(&trial.tool_result_bytes.to_string());
        out.push_str(" | ");
        out.push_str(&trial.violations.join(","));
        out.push_str(" |\n");
    }
    out
}

/// Write JSON and Markdown under `dir`. Creates `dir`. Does not change defaults.
///
/// The Markdown decision is recomputed. Blocking filesystem IO. No timeout or
/// cancellation inside this function.
pub fn write_reports(dir: &Path, report: &AbReport) -> Result<ReportPaths, Error> {
    let decision = decide(report)?;
    std::fs::create_dir_all(dir).map_err(|err| Error::msg(format!("report dir: {err}")))?;
    let stem = file_stem(report);
    let json = dir.join(format!("{stem}.json"));
    let markdown = dir.join(format!("{stem}.md"));
    let body = serde_json::to_vec_pretty(report)
        .map_err(|err| Error::msg(format!("report json: {err}")))?;
    std::fs::write(&json, body).map_err(|err| Error::msg(format!("report json: {err}")))?;
    std::fs::write(&markdown, render_markdown(report, &decision))
        .map_err(|err| Error::msg(format!("report markdown: {err}")))?;
    Ok(ReportPaths { json, markdown })
}

fn file_stem(report: &AbReport) -> String {
    let mut name = String::new();
    if report.synthetic {
        name.push_str("synthetic-");
    }
    name.push_str(&sanitize(&report.date));
    name.push('-');
    name.push_str(&sanitize(&report.platform));
    name.push('-');
    name.push_str(&sanitize(&report.model));
    name
}

fn sanitize(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    if out.is_empty() {
        out.push_str("none");
    }
    out
}

fn push_kv(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push_str(": ");
    out.push_str(value);
    out.push('\n');
}

fn push_arm(out: &mut String, label: &str, arm: &super::ArmConfig) {
    out.push_str("| ");
    out.push_str(label);
    out.push_str(" | ");
    out.push_str(&arm.profile);
    out.push_str(" | ");
    out.push_str(&arm.screenshot);
    out.push_str(" | ");
    out.push_str(&arm.diff);
    out.push_str(" | ");
    out.push_str(bool_word(arm.batch));
    out.push_str(" | ");
    out.push_str(bool_word(arm.script_requested));
    out.push_str(" |\n");
}

fn push_gate(out: &mut String, name: &str, value: &str) {
    out.push_str("| ");
    out.push_str(name);
    out.push_str(" | ");
    out.push_str(value);
    out.push_str(" |\n");
}

fn push_totals(out: &mut String, label: &str, totals: &super::ArmTotals) {
    out.push_str("| ");
    out.push_str(label);
    for value in [
        totals.scored,
        totals.passed,
        totals.images,
        totals.image_bytes,
        totals.unknown_calls,
        totals.false_success,
        totals.extra_approvals,
        totals.tool_result_bytes,
    ] {
        out.push_str(" | ");
        out.push_str(&value.to_string());
    }
    out.push_str(" |\n");
}

fn push_stats(out: &mut String, label: &str, stats: Option<&PairStats>, missing: &str) {
    let Some(stats) = stats else {
        out.push_str(label);
        out.push_str(": ");
        out.push_str(missing);
        out.push('\n');
        return;
    };
    out.push_str(label);
    out.push_str(" median_change_milli_pct=");
    out.push_str(&stats.median_change_milli_pct.to_string());
    out.push_str(" median_drop_milli_pct=");
    out.push_str(&stats.median_drop_milli_pct.to_string());
    out.push_str(" p=");
    out.push_str(&stats.p_value.to_string());
    out.push_str(" ci95=");
    out.push_str(&stats.ci95_low_milli_pct.to_string());
    out.push_str("..");
    out.push_str(&stats.ci95_high_milli_pct.to_string());
    out.push_str(" method=");
    out.push_str(stats.method);
    out.push_str(" seed=");
    out.push_str(&stats.seed.to_string());
    out.push_str(" pairs=");
    out.push_str(&stats.pairs.to_string());
    out.push('\n');
}

fn opt_num(value: Option<u64>) -> String {
    value.map_or_else(|| "unavailable".into(), |number| number.to_string())
}

fn bool_word(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn provenance_word(provenance: super::Provenance) -> &'static str {
    match provenance {
        super::Provenance::NotRun => "not_run",
        super::Provenance::Runner => "runner",
        super::Provenance::Supplied => "supplied",
        super::Provenance::Synthetic => "synthetic",
    }
}

fn source_word(source: RoundsSource) -> &'static str {
    match source {
        RoundsSource::Unset => "unset",
        RoundsSource::TranscriptTurns => "transcript_turns",
        RoundsSource::ToolCalls => "tool_calls",
    }
}

/// Howard Hinnant `civil_from_days`. `days` is the Unix day count.
fn civil_from_days(days: i64) -> Option<(i64, u32, u32)> {
    let z = days.checked_add(719_468)?;
    let era = (if z >= 0 { z } else { z.checked_sub(146_096)? }) / 146_097;
    let doe = u64::try_from(z.checked_sub(era.checked_mul(146_097)?)?).ok()?;
    let yoe = doe
        .checked_sub(doe / 1460)?
        .checked_add(doe / 36524)?
        .checked_sub(doe / 146_096)?
        / 365;
    let year0 = i64::try_from(yoe)
        .ok()?
        .checked_add(era.checked_mul(400)?)?;
    let year_days = (365 * yoe).checked_add(yoe / 4)?.checked_sub(yoe / 100)?;
    let doy = doe.checked_sub(year_days)?;
    let mp = (5 * doy + 2) / 153;
    let day = doy.checked_sub((153 * mp + 2) / 5)?.checked_add(1)?;
    let month = if mp < 10 { mp + 3 } else { mp.checked_sub(9)? };
    let year = if month <= 2 {
        year0.checked_add(1)?
    } else {
        year0
    };
    Some((year, u32::try_from(month).ok()?, u32::try_from(day).ok()?))
}

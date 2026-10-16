//! JSON archive for CI and a manual Mac run. Written only to an explicit path
//! or to `$GCU_CONTRACT_OUT`.

use super::table::CaseResult;
use std::path::Path;

/// Items this process does not execute. The live table and the smoke script
/// run on a TCC-granted macOS GUI session.
pub const NOT_RUN: &[&str] = &[
    "live_contract_table_suite (ignored by default, macOS GUI + TCC)",
    "scripts/smoke-macos.sh",
];

/// Write the suite summary. `not_run` is copied into the archive as-is.
pub fn write_report(path: &Path, results: &[CaseResult], not_run: &[&str]) -> Result<(), String> {
    let passed = results.iter().filter(|row| row.outcome == "passed").count();
    let skipped = results
        .iter()
        .filter(|row| row.outcome == "skipped")
        .count();
    let failed = results.iter().filter(|row| row.outcome == "failed").count();
    let cases: Vec<_> = results
        .iter()
        .map(|row| {
            serde_json::json!({
                "id": row.id,
                "description": row.description,
                "outcome": row.outcome,
                "reason": row.reason,
            })
        })
        .collect();
    let body = serde_json::json!({
        "suite": "gcu-driver-macos",
        "passed": passed,
        "skipped": skipped,
        "failed": failed,
        "not_run": not_run,
        "cases": cases,
    });
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
    }
    std::fs::write(
        path,
        serde_json::to_string_pretty(&body).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

/// Archive to `$GCU_CONTRACT_OUT` when the variable is set.
pub fn archive(results: &[CaseResult], not_run: &[&str]) -> Result<(), String> {
    let Some(path) = std::env::var_os("GCU_CONTRACT_OUT") else {
        return Ok(());
    };
    write_report(Path::new(&path), results, not_run)
}

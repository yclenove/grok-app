//! Native report loading and performance-budget exit status. No sampling here.

use crate::budgets;
use anyhow::{bail, Context, Result};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

// gcu-eval exports these exact sources too; do not copy the checker into tests.
#[path = "../../bench/report.rs"]
mod report;

pub(crate) fn run(mut args: impl Iterator<Item = OsString>) -> Result<()> {
    let directory = match (args.next(), args.next(), args.next()) {
        (None, None, None) => Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .context("xtask must live under computer-use")?
            .join("bench"),
        (Some(flag), Some(directory), None) if flag == "--reports" => PathBuf::from(directory),
        _ => bail!("usage: cargo xtask bench-check [--reports DIR]"),
    };
    check_directory(&directory)
}

fn check_directory(directory: &Path) -> Result<()> {
    let mut reports = Vec::new();
    let mut issues = Vec::new();
    for platform in budgets::Platform::ALL {
        let path = directory.join(format!("{platform}.json"));
        match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<report::BenchReport>(&bytes) {
                Ok(report) if report.platform == platform => reports.push(report),
                Ok(report) => issues.push(format!(
                    "{}: platform {} does not match filename {platform}",
                    path.display(),
                    report.platform
                )),
                Err(error) => issues.push(format!(
                    "{}: invalid bench report schema: {error}",
                    path.display()
                )),
            },
            Err(error) => issues.push(format!(
                "{}: missing or unreadable platform {platform} report (not_run): {error}",
                path.display()
            )),
        }
    }
    // Keep missing and invalid reports failed even when every available row passes.
    let checked = match report::check_reports(&reports) {
        Ok(rows) => rows,
        Err(error) => {
            issues.extend(error.issues);
            Vec::new()
        }
    };
    if !issues.is_empty() {
        bail!("bench-check FAILED:\n{}", issues.join("\n"));
    }
    for row in checked {
        let limit = budgets::latency_budget(row.platform, row.operation);
        println!(
            "{}/{}: n={}, p50={} / {} ms, p95={} / {} ms",
            row.platform,
            row.operation,
            row.samples,
            row.percentiles.p50_ms,
            limit.p50_ms,
            row.percentiles.p95_ms,
            limit.p95_ms
        );
    }
    crate::schema::run_budget()?;
    println!("PASS: all three native platforms satisfy performance budgets");
    Ok(())
}

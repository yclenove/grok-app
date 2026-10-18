//! Development gates for the independent computer-use workspace.

#![deny(missing_docs)]

mod bench;
#[path = "../../bench/budgets.rs"]
mod budgets;
mod bundle;
mod deps;
// Share the pure serving projection without linking MCP/core or enabling script
// execution in this development tool. Golden output is feature/OS independent.
#[path = "../../crates/gcu-mcp/src/catalog/profiles.rs"]
mod profiles;
mod release;
mod sbom;
mod schema;
mod ui;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod schema_tests;

use anyhow::{bail, Result};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("check-deps") if args.next().is_none() => deps::run(),
        Some("golden") => match (args.next().as_deref(), args.next()) {
            (Some("--check"), None) => schema::run_golden(false),
            (Some("--update"), None) => schema::run_golden(true),
            _ => bail!("usage: cargo xtask golden <--check|--update>"),
        },
        Some("budget") if args.next().is_none() => schema::run_budget(),
        Some("bench-check") => bench::run(args.map(std::ffi::OsString::from)),
        Some("release") => release::run(args),
        Some("sbom") if args.next().is_none() => sbom::run(),
        Some("bundle-macos") => bundle::run(args),
        Some("ui") => ui::run(args),
        _ => bail!(
            "usage: cargo xtask <check-deps|golden <--check|--update>|budget|bench-check [--reports DIR]|sbom|release <--dry-run|--stage|--collect>|bundle-macos [--release] [--binary PATH] [--output DIR]|ui <embed|export-i18n|check>>"
        ),
    }
}

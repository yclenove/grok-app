//! Development gates for the independent computer-use workspace.

#![deny(missing_docs)]

mod deps;
mod schema;

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
        _ => bail!("usage: cargo xtask <check-deps|golden <--check|--update>|budget>"),
    }
}

//! Development gates for the independent computer-use workspace.

#![deny(missing_docs)]

mod deps;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;

use anyhow::{bail, Result};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("check-deps") if args.next().is_none() => deps::run(),
        Some("golden" | "budget") => {
            // P0-03 owns schema generation and byte-budget enforcement. Until it
            // lands, these gates explicitly skip with success rather than imply
            // that a golden file or budget has been validated.
            println!("SKIP: not implemented yet: P0-03");
            Ok(())
        }
        _ => bail!("usage: cargo xtask <check-deps|golden|budget>"),
    }
}

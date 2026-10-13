//! Computer-use command line.
//!
//! Subcommand wiring is a later card. [`install`] registers the `grok-computer-use`
//! MCP server with supported clients and removes that registration again.

#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod install;

/// Process entry used by the `gcu` binary.
///
/// Returns immediately. This slice does not parse arguments, read the
/// environment, or touch the filesystem. Clap subcommands land in a later card.
pub fn cli_main() {}

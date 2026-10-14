//! The gcu computer-use command-line entry point.

#![deny(missing_docs)]

fn main() {
    // All parsing and runtime assembly belongs to the testable CLI library.
    gcu_cli::cli_main();
}

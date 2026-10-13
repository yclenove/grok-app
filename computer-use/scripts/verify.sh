#!/usr/bin/env bash
# Run every computer-use gate, stopping at the first failure.
#   scripts/verify.sh            host-target gates (what CI runs)
#   CROSS=1 scripts/verify.sh    also clippy the libs/bins for Windows and macOS targets
# From a worktree on the compile host: rcargo --sh 'bash scripts/verify.sh'
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n== %s\n' "$*"; }

step "fmt";    cargo fmt --all --check
step "clippy"; cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
if [ "${CROSS:-0}" = 1 ]; then
  for target in x86_64-pc-windows-gnu aarch64-apple-darwin; do
    step "clippy ($target)"
    # Libs and bins only: dev-dependencies such as criterion need a target C toolchain.
    cargo clippy --locked --workspace --exclude gcu-fixture --lib --bins --target "$target" -- -D warnings
  done
fi
step "test";       cargo test --locked --workspace --all-features
step "check-deps"; cargo run --locked -q -p xtask -- check-deps
step "golden";     cargo run --locked -q -p xtask -- golden --check
step "budget";     cargo run --locked -q -p xtask -- budget
step "deny";       cargo deny --locked check
printf '\nVERIFY_OK\n'

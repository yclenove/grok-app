#!/usr/bin/env bash
set -euo pipefail
[[ $# == 0 && ${GROK_CU_SOURCES_CANDIDATE:-} == /work/core-maintained-sources-01 ]] || exit 2
source /etc/os-release
[[ $ID == ubuntu && $VERSION_ID == 22.04 ]] || exit 2
getconf GNU_LIBC_VERSION
rustc --version
cd "$GROK_CU_SOURCES_CANDIDATE"
cargo build --manifest-path src-tauri/Cargo.toml --locked --offline -p grok-computer-use-core --bin cu-prepare-runtime
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline -p grok-computer-use-core --bin cu-prepare-runtime -- -D warnings
target/debug/cu-prepare-runtime --verify-sources --target x86_64-linux --repo "$GROK_CU_SOURCES_CANDIDATE" \
  --seed "$GROK_CU_SOURCES_CANDIDATE/audit/seed" --cache "$GROK_CU_SOURCES_CANDIDATE/archive-cache"
[[ ! -e $GROK_CU_SOURCES_CANDIDATE/audit/seed ]]
[[ -z $(find "$GROK_CU_SOURCES_CANDIDATE/audit" -name '.source-audit-*' -print -quit) ]]
sha256sum target/debug/cu-prepare-runtime

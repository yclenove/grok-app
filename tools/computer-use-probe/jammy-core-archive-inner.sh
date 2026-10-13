#!/usr/bin/env bash
set -euo pipefail
[[ $# == 0 ]] || exit 2
case ${GROK_CU_ARCHIVE_CANDIDATE:-} in
  /work/core-runtime-bounds-02|/work/core-runtime-bounds-03|/work/core-runtime-bounds-04|/work/core-runtime-bounds-05|/work/core-runtime-bounds-06) ;;
  *) exit 2 ;;
esac
source /etc/os-release
[[ $ID == ubuntu && $VERSION_ID == 22.04 ]] || exit 2
getconf GNU_LIBC_VERSION
rustc --version
cd "$GROK_CU_ARCHIVE_CANDIDATE"
if [[ $GROK_CU_ARCHIVE_CANDIDATE != /work/core-runtime-bounds-02 ]]; then
  cargo test --manifest-path src-tauri/Cargo.toml --locked -p grok-computer-use-core --lib -- --test-threads=1
  cargo test --manifest-path src-tauri/Cargo.toml --locked -p grok-computer-use-core --test runtime_sources_cli -- --test-threads=1
  target/debug/cu-prepare-runtime --verify-sources --target x86_64-linux --repo "$GROK_CU_ARCHIVE_CANDIDATE"
else
  cargo test --manifest-path src-tauri/Cargo.toml --locked -p grok-computer-use-core --lib archive_zip -- --test-threads=1
fi
cargo clippy --manifest-path src-tauri/Cargo.toml --locked -p grok-computer-use-core --all-targets -- -D warnings

#!/usr/bin/env bash
set -euo pipefail
[[ $# == 2 && $1 =~ ^/work/product-app-[A-Za-z0-9-]+$ && $2 =~ ^/work/product-app-[A-Za-z0-9-]+$ ]] || exit 2
[[ ${GROK_CU_JAMMY_SDK_ROOT:-} == /work && $1 != "$2" ]] || exit 2
source /etc/os-release
[[ $ID == ubuntu && $VERSION_ID == 22.04 ]] || exit 2
cd "$1"
export PKG_CONFIG_PATH=/work/pipewire-sdk-169-02/lib/pkgconfig:/work/libei-sdk/lib/pkgconfig
export LD_LIBRARY_PATH=/work/pipewire-sdk-169-02/lib
export CARGO_HOME=/work/cu-cargo-home
export CARGO_BUILD_JOBS=2
# Reuse dependency objects, never the prior failing source or test evidence.
export CARGO_TARGET_DIR="$2/src-tauri/target"
export TAURI_CONFIG="$(<src-tauri/tauri.linux.conf.json)"
bash scripts/check-computer-use-linux-sdk.sh
"$CARGO_TARGET_DIR/release/cu-prepare-runtime" --check --target x86_64-unknown-linux-gnu \
  --repo "$1" --cache /work/cu-runtime-archive-cache
cargo build --manifest-path src-tauri/Cargo.toml --locked --release --features tauri/custom-protocol --bin grok-app
sha256sum "$CARGO_TARGET_DIR/release/grok-app"
readelf --dynamic "$CARGO_TARGET_DIR/release/grok-app"
readelf --version-info "$CARGO_TARGET_DIR/release/grok-app"
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --release --all-targets \
  --features tauri/custom-protocol -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked --release --features tauri/custom-protocol \
  --lib computer_use::webview::tests::navigation -- --test-threads=1

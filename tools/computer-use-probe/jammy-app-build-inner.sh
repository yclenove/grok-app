#!/usr/bin/env bash
set -euo pipefail
[[ $# == 1 && $1 =~ ^/work/product-app-[A-Za-z0-9-]+$ && ${GROK_CU_JAMMY_SDK_ROOT:-} == /work ]] || exit 2
source /etc/os-release
[[ $ID == ubuntu && $VERSION_ID == 22.04 ]] || exit 2
export DEBIAN_FRONTEND=noninteractive
apt-get -o Acquire::Retries=3 update
apt-get -o Acquire::Retries=3 install -y --no-install-recommends \
  libwebkit2gtk-4.1-dev librsvg2-dev libgtk-3-dev libayatana-appindicator3-dev \
  libssl-dev libclang-dev libdbus-1-dev xvfb xauth dbus-x11
getconf GNU_LIBC_VERSION
rustc --version
cd "$1"
python3 scripts/stage-computer-use-native-sdk.py /work/pw169-runtime-check-02/runtime \
  "$1/src-tauri/resources/computer-use/native-pipewire"
export PKG_CONFIG_PATH=/work/pipewire-sdk-169-02/lib/pkgconfig:/work/libei-sdk/lib/pkgconfig
export LD_LIBRARY_PATH=/work/pipewire-sdk-169-02/lib
export CARGO_HOME=/work/cu-cargo-home
export CARGO_BUILD_JOBS=2
export TAURI_CONFIG="$(<src-tauri/tauri.linux.conf.json)"
bash scripts/check-computer-use-linux-sdk.sh
for mode in prepare check; do
  cargo run --manifest-path src-tauri/Cargo.toml --locked --release \
    -p grok-computer-use-core --bin cu-prepare-runtime -- \
    "--$mode" --target x86_64-unknown-linux-gnu --repo "$1" --cache /work/cu-runtime-archive-cache
done
cargo build --manifest-path src-tauri/Cargo.toml --locked --release --features tauri/custom-protocol --bin grok-app
sha256sum src-tauri/target/release/grok-app
readelf --dynamic src-tauri/target/release/grok-app
readelf --version-info src-tauri/target/release/grok-app

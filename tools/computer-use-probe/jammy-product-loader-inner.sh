#!/usr/bin/env bash
set -euo pipefail
[[ $# == 1 && $1 =~ ^/work/product-loader-[A-Za-z0-9-]+$ && ${GROK_CU_JAMMY_SDK_ROOT:-} == /work ]] || exit 2
source /etc/os-release
[[ $ID == ubuntu && $VERSION_ID == 22.04 ]] || exit 2
getconf GNU_LIBC_VERSION
rustc --version
cd "$1"
python3 scripts/test-stage-computer-use-native-sdk.py
python3 scripts/test-computer-use-native-sdk.py
fixture="$1/tools/computer-use-probe/native-loader"
mkdir -p "$fixture/resources/computer-use"
python3 scripts/stage-computer-use-native-sdk.py \
  /work/pw169-runtime-check-02/runtime "$fixture/resources/computer-use/native-pipewire"
export CARGO_HOME=/work/cu-cargo-home
export GROK_CU_LOADER_LINK_SDK=/work/pipewire-sdk-169-02
cargo build --manifest-path "$fixture/Cargo.toml" --release --locked
LD_LIBRARY_PATH="$fixture/resources/computer-use/native-pipewire/runtime/lib" \
  cargo test --manifest-path "$fixture/Cargo.toml" --locked
readelf --dynamic "$fixture/target/release/grok-native-loader-probe"
python3 tools/computer-use-probe/jammy-product-loader-faults.py \
  "$fixture/target/release/grok-native-loader-probe" \
  "$fixture/resources/computer-use/native-pipewire" "$1/acceptance" \
  --fallback-library /work/pipewire-sdk-03/lib/libpipewire-0.3.so.0

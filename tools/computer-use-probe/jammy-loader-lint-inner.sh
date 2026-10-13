#!/usr/bin/env bash
set -euo pipefail
[[ $# == 1 && $1 =~ ^/work/product-loader-[A-Za-z0-9-]+$ && ${GROK_CU_JAMMY_SDK_ROOT:-} == /work ]] || exit 2
source /etc/os-release
[[ $ID == ubuntu && $VERSION_ID == 22.04 ]] || exit 2
stage=/work/rust-clippy-1.98-install-input
[[ ! -e $stage && ! -L $stage && ! -e /work/rust-toolchain-1.98/bin/clippy-driver ]] || exit 2
mkdir -m 700 "$stage"
cd "$stage"
archive=clippy-1.98.0-x86_64-unknown-linux-gnu.tar.xz
curl --fail --location --retry 3 --connect-timeout 20 --max-time 600 \
  "https://static.rust-lang.org/dist/2026-08-20/$archive" --output "$archive"
printf '%s  %s\n' 646c6bd2450ea4c32a3c78ad7a3727294eea1f1519718a3f1487424021695ffa "$archive" | sha256sum --check --strict
tar -xJf "$archive" --no-same-owner
bash clippy-1.98.0-x86_64-unknown-linux-gnu/install.sh --prefix=/work/rust-toolchain-1.98 --disable-ldconfig
cd "$1"
export CARGO_HOME=/work/cu-cargo-home
export GROK_CU_LOADER_LINK_SDK=/work/pipewire-sdk-169-02
cargo clippy --manifest-path tools/computer-use-probe/native-loader/Cargo.toml --locked --all-targets -- -D warnings

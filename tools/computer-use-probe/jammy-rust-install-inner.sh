#!/usr/bin/env bash
# Install pinned compiler components only inside the task-owned Jammy root.
set -euo pipefail
[[ ${GROK_CU_JAMMY_SDK_ROOT:-} == /work ]] || exit 2
source /etc/os-release
[[ $ID == ubuntu && $VERSION_ID == 22.04 ]] || exit 2
prefix=/work/rust-toolchain-1.98
stage=/work/rust-toolchain-1.98-install-input
[[ ! -e $prefix && ! -L $prefix && ! -e $stage && ! -L $stage ]] || exit 2
mkdir -m 700 "$stage"
cd "$stage"
for component in rustc cargo rust-std; do
  archive="$component-1.98.0-x86_64-unknown-linux-gnu.tar.xz"
  curl --fail --location --retry 3 --connect-timeout 20 --max-time 600 \
    "https://static.rust-lang.org/dist/2026-08-20/$archive" --output "$archive"
done
printf '%s  %s\n' \
  0e37cb339f447fc44d6d781073bacacebfdc5612f2600e4c7e84c266f5f3aced rustc-1.98.0-x86_64-unknown-linux-gnu.tar.xz \
  2f512d170d3dd23e16ababcda32ee2e6d5172d861a7af1f504e0b1e270cafab9 cargo-1.98.0-x86_64-unknown-linux-gnu.tar.xz \
  f5022e6c95a5ad23cca2513dc8281200f585fa188de6370aa37b128a43f876a3 rust-std-1.98.0-x86_64-unknown-linux-gnu.tar.xz | sha256sum --check --strict
for component in rustc cargo rust-std; do
  name="$component-1.98.0-x86_64-unknown-linux-gnu"
  tar -xJf "$name.tar.xz" --no-same-owner
  bash "$name/install.sh" --prefix="$prefix" --disable-ldconfig
done
getconf GNU_LIBC_VERSION
"$prefix/bin/rustc" --version
"$prefix/bin/cargo" --version

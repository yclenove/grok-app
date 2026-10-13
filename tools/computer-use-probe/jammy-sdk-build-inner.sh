#!/usr/bin/env bash
# Runs only inside the private Ubuntu Base root prepared by the outer probe.
set -euo pipefail
[[ ${GROK_CU_JAMMY_SDK_ROOT:-} == /work ]] || exit 2
source /etc/os-release
[[ $ID == ubuntu && $VERSION_ID == 22.04 ]] || exit 2
export DEBIAN_FRONTEND=noninteractive
apt-get -o Acquire::Retries=3 update
apt-get -o Acquire::Retries=3 install -y --no-install-recommends \
  ca-certificates build-essential pkg-config curl meson ninja-build python3-jinja2 \
  libgtk-3-dev binutils patch patchelf
getconf GNU_LIBC_VERSION
meson --version
PIPEWIRE_SOURCE_ARCHIVE=/work/pipewire_1.6.9.orig.tar.gz \
PIPEWIRE_DEBIAN_ARCHIVE=/work/pipewire_1.6.9-2.debian.tar.xz \
  bash /work/build-computer-use-pipewire.sh /work/pipewire-sdk
LIBEI_SOURCE_ARCHIVE=/work/libei_1.5.0.orig.tar.bz2 \
  bash /work/build-computer-use-libei.sh /work/libei-sdk
export PKG_CONFIG_PATH=/work/pipewire-sdk/lib/pkgconfig:/work/libei-sdk/lib/pkgconfig
export LD_LIBRARY_PATH=/work/pipewire-sdk/lib:/work/libei-sdk/lib
bash /work/check-computer-use-linux-sdk.sh
find /work/pipewire-sdk/lib /work/libei-sdk/lib -type f \
  \( -name '*.so*' -o -name '*.a' \) -exec sha256sum {} +
readelf --version-info /work/pipewire-sdk/lib/libpipewire-0.3.so | \
  sed -n 's/.*Name: \(GLIBC_[^ ]*\).*/\1/p' | sort -Vu

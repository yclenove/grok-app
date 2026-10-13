#!/usr/bin/env bash
# Reuse only the owned Jammy toolchain; SDK outputs must be new directories.
set -euo pipefail
[[ $# == 1 && $1 =~ ^/work/[A-Za-z0-9-]+$ && ${GROK_CU_JAMMY_SDK_ROOT:-} == /work ]] || exit 2
source /etc/os-release
[[ $ID == ubuntu && $VERSION_ID == 22.04 ]] || exit 2
getconf GNU_LIBC_VERSION
stage="$1-input"
PIPEWIRE_SOURCE_ARCHIVE="$stage/pipewire_1.6.9.orig.tar.gz" \
PIPEWIRE_DEBIAN_ARCHIVE="$stage/pipewire_1.6.9-2.debian.tar.xz" \
  bash "$stage/build-computer-use-pipewire.sh" "$1"
# Preserve the existing static EI SDK rather than attempting to overwrite it.
export PKG_CONFIG_PATH="$1/lib/pkgconfig:/work/libei-sdk/lib/pkgconfig"
bash "$stage/check-computer-use-linux-sdk.sh"

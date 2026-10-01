#!/usr/bin/env bash
# Build requirements, not an installed-desktop/permission acceptance claim.
set -euo pipefail
command -v pkg-config >/dev/null || { echo 'pkg-config is required' >&2; exit 2; }
pkg-config --print-errors --atleast-version=0.3.65 libpipewire-0.3 || {
  echo 'Computer Use requires PipeWire/SPA >= 0.3.65; stock Ubuntu 22.04 (0.3.48) is insufficient.' >&2
  echo 'Use a verified private SDK built on the supported glibc baseline; do not fake pkg-config versions or raise the release OS baseline.' >&2
  exit 2
}
pkg-config --print-errors --exists libspa-0.2 'gtk+-3.0 >= 3.24'
pkg-config --print-errors --static --atleast-version=1.5 libei-1.0 || {
  echo 'Build a private static libei SDK with scripts/build-computer-use-libei.sh and set PKG_CONFIG_PATH.' >&2
  exit 2
}
libdir=$(pkg-config --variable=libdir libei-1.0)
test -f "$libdir/libei.a" || { echo "Required static archive missing: $libdir/libei.a" >&2; exit 2; }
printf 'Native SDK: PipeWire=%s SPA=%s GTK=%s libei=%s (static archive present)\n' \
  "$(pkg-config --modversion libpipewire-0.3)" "$(pkg-config --modversion libspa-0.2)" \
  "$(pkg-config --modversion gtk+-3.0)" "$(pkg-config --modversion libei-1.0)"

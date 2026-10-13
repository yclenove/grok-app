#!/usr/bin/env bash
# Private SDK built on the caller's ABI baseline; never replaces host PipeWire.
# This prepares libraries/modules, not an installed-app or portal acceptance.
set -euo pipefail
if [[ $# != 1 || $1 != /* || $1 == / || -e $1 || -L $1 ]]; then
  echo 'Usage: build-computer-use-pipewire.sh /absolute/new/sdk-directory' >&2
  exit 2
fi
for tool in meson ninja cc pkg-config python3 tar sha256sum curl patch realpath; do
  command -v "$tool" >/dev/null || { echo "Required tool missing: $tool" >&2; exit 2; }
done
prefix=$1
[[ $(realpath -e "$(dirname -- "$prefix")") == "$(dirname -- "$prefix")" ]] || {
  echo 'SDK parent must exist and be canonical (no symlink parents)' >&2
  exit 2
}
mkdir -m 700 "$prefix"
archive="$prefix/pipewire-1.6.9.tar.gz"
if [[ -n ${PIPEWIRE_SOURCE_ARCHIVE:-} ]]; then
  cp -- "$PIPEWIRE_SOURCE_ARCHIVE" "$archive"
else
  curl --proto '=https' --tlsv1.2 --fail --location --retry 3 \
    --connect-timeout 15 --max-time 120 \
    https://deb.debian.org/debian/pool/main/p/pipewire/pipewire_1.6.9.orig.tar.gz \
    --output "$archive"
fi
# Debian's pipewire_1.6.9-2.dsc, Checksums-Sha256 (source + security patch).
printf '%s  %s\n' \
  a0012db8aaca4448fd205035e07d264235e4a4fe50713085fc4a6899e738a298 \
  "$archive" | sha256sum --check --strict
debian_archive="$prefix/pipewire-1.6.9-2.debian.tar.xz"
if [[ -n ${PIPEWIRE_DEBIAN_ARCHIVE:-} ]]; then
  cp -- "$PIPEWIRE_DEBIAN_ARCHIVE" "$debian_archive"
else
  curl --proto '=https' --tlsv1.2 --fail --location --retry 3 \
    --connect-timeout 15 --max-time 120 \
    https://deb.debian.org/debian/pool/main/p/pipewire/pipewire_1.6.9-2.debian.tar.xz \
    --output "$debian_archive"
fi
printf '%s  %s\n' \
  9ee24622dfd907c8f1283328632156d43d23013c7ce62b04ea8ed6991a1ae65c \
  "$debian_archive" | sha256sum --check --strict
mkdir "$prefix/source"
tar --extract --gzip --file="$archive" --directory="$prefix/source" \
  --strip-components=1 --no-same-owner
mkdir "$prefix/debian-source"
tar --extract --xz --file="$debian_archive" --directory="$prefix/debian-source" \
  --no-same-owner debian/patches/0001-CVE-2026-14330.patch
security_patch="$prefix/debian-source/debian/patches/0001-CVE-2026-14330.patch"
patch --batch --forward --fuzz=0 --dry-run -p1 -d "$prefix/source" < "$security_patch"
patch --batch --forward --fuzz=0 -p1 -d "$prefix/source" < "$security_patch"
meson setup "$prefix/build" "$prefix/source" --prefix="$prefix" --libdir=lib \
  --sysconfdir="$prefix/etc" --buildtype=release --default-library=shared \
  --auto-features=disabled -Ddocs=disabled -Dman=disabled \
  -Dtests=disabled -Dexamples=disabled -Dsession-managers=[] \
  -Dlibsystemd=disabled -Dsystemd-user-service=disabled -Ddbus=disabled \
  -Dpipewire-jack=disabled -Dpipewire-v4l2=disabled -Dflatpak=disabled \
  -Dlegacy-rtkit=false -Drlimits-install=false -Dpam-defaults-install=false \
  -Daudiomixer=disabled -Daudioconvert=enabled -Daudiotestsrc=disabled \
  -Dvideoconvert=enabled -Dvideotestsrc=disabled -Dcontrol=disabled
ninja -C "$prefix/build" -j4
meson install -C "$prefix/build"
mkdir -p "$prefix/share/licenses/pipewire"
cp "$prefix/source/COPYING" "$prefix/share/licenses/pipewire/COPYING"
cp "$prefix/source/LICENSE" "$prefix/share/licenses/pipewire/LICENSE"
cp "$security_patch" "$prefix/share/licenses/pipewire/0001-CVE-2026-14330.patch"
printf '%s\n' \
  'PipeWire 1.6.9 + Debian 1.6.9-2 CVE-2026-14330 patch.' \
  'Upstream source SHA256: a0012db8aaca4448fd205035e07d264235e4a4fe50713085fc4a6899e738a298' \
  'Debian source delta SHA256: 9ee24622dfd907c8f1283328632156d43d23013c7ce62b04ea8ed6991a1ae65c' \
  > "$prefix/share/licenses/pipewire/SOURCE.txt"
for required in \
  lib/libpipewire-0.3.so \
  lib/pkgconfig/libpipewire-0.3.pc lib/pkgconfig/libspa-0.2.pc \
  lib/spa-0.2/support/libspa-support.so \
  lib/spa-0.2/videoconvert/libspa-videoconvert.so \
  lib/pipewire-0.3/libpipewire-module-protocol-native.so \
  lib/pipewire-0.3/libpipewire-module-client-node.so \
  share/pipewire/client.conf; do
  test -f "$prefix/$required" || { echo "Private SDK component missing: $required" >&2; exit 2; }
done
test ! -e "$prefix/lib/spa-0.2/alsa/libspa-alsa.so"
test ! -e "$prefix/lib/libjackserver.so"
printf 'Private PipeWire 1.6.9 + Debian security patch SDK ready: %s\n' "$prefix"
printf 'Set PKG_CONFIG_PATH=%s/lib/pkgconfig for compilation.\n' "$prefix"
printf 'Release still requires relocated modules/configuration and installed ABI validation.\n'

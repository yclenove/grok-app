#!/usr/bin/env bash
# Reproducible private libei SDK; never installs over a system library.
set -euo pipefail
if [[ $# != 1 || $1 != /* || $1 == / || -e $1 || -L $1 ]]; then
  echo 'Usage: build-computer-use-libei.sh /absolute/new/sdk-directory' >&2
  exit 2
fi
for tool in meson ninja cc pkg-config python3 tar sha256sum; do
  command -v "$tool" >/dev/null || { echo "Required tool missing: $tool" >&2; exit 2; }
done
python3 -c 'import jinja2'
prefix=$1
mkdir -m 700 "$prefix"
archive="$prefix/libei-1.5.0.tar.bz2"
if [[ -n ${LIBEI_SOURCE_ARCHIVE:-} ]]; then
  cp -- "$LIBEI_SOURCE_ARCHIVE" "$archive"
else
  curl --proto '=https' --tlsv1.2 --fail --location --retry 3 \
    --connect-timeout 15 --max-time 120 \
    https://deb.debian.org/debian/pool/main/libe/libei/libei_1.5.0.orig.tar.bz2 \
    --output "$archive"
fi
printf '%s  %s\n' \
  da1fba92daccd0667bc46c3ee952d4ae8cfc6bdb4c0bb4d34df26528fb240618 \
  "$archive" | sha256sum --check --strict
mkdir "$prefix/source"
tar --extract --bzip2 --file="$archive" --directory="$prefix/source" \
  --strip-components=1 --no-same-owner
meson setup "$prefix/build" "$prefix/source" --prefix="$prefix" --libdir=lib \
  --buildtype=release --default-library=both -Dtests=disabled \
  -Ddocumentation=[] -Dliboeffis=disabled
ninja -C "$prefix/build" -j4
meson install -C "$prefix/build"
mkdir -p "$prefix/share/licenses/libei"
cp "$prefix/source/COPYING" "$prefix/share/licenses/libei/COPYING"
printf 'Private libei 1.5.0 SDK ready: %s\n' "$prefix"
printf 'Set PKG_CONFIG_PATH=%s/lib/pkgconfig and LD_LIBRARY_PATH=%s/lib for native tests.\n' "$prefix" "$prefix"

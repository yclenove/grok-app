#!/usr/bin/env bash
# Private test compositor for Jammy's wlroots 0.15. Never installed system-wide
# or included in the App; not GNOME or installed-app acceptance.
set -euo pipefail
if [[ $# != 1 || $1 != /* || $1 == / || -e $1 || -L $1 ]]; then
  echo 'Usage: build-computer-use-labwc.sh /absolute/new/compositor-directory' >&2
  exit 2
fi
for tool in realpath meson ninja cc pkg-config tar sha256sum curl patch; do
  command -v "$tool" >/dev/null || { echo "Required tool missing: $tool" >&2; exit 2; }
done
prefix=$1
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
backport="$here/computer-use-labwc-xdg-foreign.patch"
test -f "$backport"
[[ $(realpath -m "$prefix") == "$prefix" &&
   $(realpath -e "$(dirname -- "$prefix")") == "$(dirname -- "$prefix")" ]] || {
  echo 'Compositor destination must be canonical with an existing, non-linked parent' >&2
  exit 2
}
pkg-config --atleast-version=0.15.0 wlroots
pkg-config --max-version=0.15.99 wlroots
mkdir -m 700 "$prefix"
commit=bea35f1969565f63a1cab80191363617ed45f32a
archive="$prefix/labwc-$commit.tar.gz"
if [[ -n ${LABWC_SOURCE_ARCHIVE:-} ]]; then
  cp -- "$LABWC_SOURCE_ARCHIVE" "$archive"
else
  curl --proto '=https' --tlsv1.2 --fail --location --retry 3 \
    --connect-timeout 15 --max-time 120 \
    "https://codeload.github.com/labwc/labwc/tar.gz/$commit" --output "$archive"
fi
printf '%s  %s\n' \
  7402bbda87002fc836a77d4a5d3e1f821cd797f5c7d37c42fbaaca5741e207ee \
  "$archive" | sha256sum --check --strict
mkdir "$prefix/source"
tar --extract --gzip --file="$archive" --directory="$prefix/source" \
  --strip-components=1 --no-same-owner
patch --batch --forward --fuzz=0 --dry-run -p1 -d "$prefix/source" < "$backport"
patch --batch --forward --fuzz=0 -p1 -d "$prefix/source" < "$backport"
meson setup "$prefix/build" "$prefix/source" --prefix="$prefix" \
  --buildtype=release --wrap-mode=nofallback -Dman-pages=disabled -Dxwayland=enabled
ninja -C "$prefix/build" -j4
meson install -C "$prefix/build"
test -x "$prefix/bin/labwc"
test "$("$prefix/bin/labwc" -v)" = 'labwc 0.5.3'
mkdir -p "$prefix/share/licenses/labwc"
cp -- "$prefix/source/LICENSE" "$prefix/share/licenses/labwc/LICENSE"
cp -- "$backport" "$prefix/share/licenses/labwc/xdg-foreign.patch"
printf '%s\n' \
  'labwc 0.5.3, GPL-2.0-only. Private headless CI fixture, not an App resource.' \
  "Source: https://github.com/labwc/labwc/tree/$commit" \
  'Source SHA256: 7402bbda87002fc836a77d4a5d3e1f821cd797f5c7d37c42fbaaca5741e207ee' \
  'xdg-foreign backport: https://github.com/labwc/labwc/commit/64bb57afa22b4ecded17a271392964b35e8d780f' \
  "wlroots: $(pkg-config --modversion wlroots)" \
  > "$prefix/share/licenses/labwc/SOURCE.txt"
sha256sum "$prefix/bin/labwc" > "$prefix/labwc.sha256"
sha256sum "$prefix/share/licenses/labwc/xdg-foreign.patch" \
  > "$prefix/share/licenses/labwc/xdg-foreign.sha256"
printf 'Private test compositor ready: %s/bin/labwc\n' "$prefix"

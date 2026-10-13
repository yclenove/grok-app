#!/usr/bin/env bash
# Runtime checking updates its lock owner, which is not a source/bundle input.
# Exclude only that exact generated record; retain the original full manifest.
set -euo pipefail
[[ $# == 3 && $1 =~ ^/var/tmp/grok-cu-jammy-sdk-[A-Za-z0-9-]+$ ]] || exit 2
[[ -d $1 && ! -L $1 && $(realpath -e -- "$1") == "$1" ]] || exit 2
[[ $2 == /* && -d $2 && ! -L $2 && $(realpath -e -- "$2") == "$2" ]] || exit 2
[[ $3 =~ ^/work/product-app-[A-Za-z0-9-]+$ && -d $1$3 && ! -L $1$3 ]] || exit 2
sidecar="$1$3/src-tauri/resources/computer-use/.cu-mutation.lock.owner.json"
[[ -f $sidecar && ! -L $sidecar ]] || exit 2
snapshot="$2/app-inputs.sha256"
[[ -f $snapshot && ! -L $snapshot ]] || exit 2
[[ $(grep -cF "  $sidecar" "$snapshot") == 1 ]] || exit 2
# Hashing only known files must not silently accept new files or links.
[[ -z $(find "$1$3" -type l -print -quit) ]] || exit 2
[[ -z $(comm -3 \
  <(cut -c67- "$snapshot" | LC_ALL=C sort) \
  <(find "$1$3" -type f -print | LC_ALL=C sort)) ]] || {
  printf 'Frozen input file inventory changed.\n' >&2
  exit 1
}
while IFS= read -r record; do
  case "$record" in
    *"  $sidecar") ;;
    *) printf '%s\n' "$record" ;;
  esac
done < "$snapshot" | sha256sum --check --quiet
sha256sum --check --quiet "$2/original-seed.sha256"
printf 'Frozen content and immutable seed unchanged; excluded one exact generated owner sidecar.\n'

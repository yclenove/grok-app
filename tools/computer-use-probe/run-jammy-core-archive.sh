#!/usr/bin/env bash
# An isolated current Core snapshot: never edit/reuse frozen App source03/target.
set -euo pipefail
[[ ( $# == 2 || $# == 3 ) && $(id -u) == 0 && $1 == /var/tmp/grok-cu-jammy-sdk-20261012-01 ]] || exit 2
[[ $2 == /* && -d $2 && ! -L $2 && $(realpath -e -- "$2") == "$2" ]] || exit 2
root=$1
evidence=$2
[[ -d $root && ! -L $root && $(realpath -e -- "$root") == "$root" ]] || exit 2
for process_root in /proc/[0-9]*/root; do
  if [[ $(readlink -- "$process_root" 2>/dev/null || true) == "$root" ]]; then
    printf 'Private-root original owner still live: %s\n' "$process_root" >&2
    exit 2
  fi
done
repo=${GROK_CU_FROZEN_SOURCE_REPO:-$(cd -- "$(dirname -- "$0")/../.." && pwd)}
[[ -d $repo && ! -L $repo && $(realpath -e -- "$repo") == "$repo" ]] || exit 2
if [[ ${GROK_CU_FROZEN_RUNNER:-0} != 1 ]]; then
  # Bash may read the remainder after a long child wait. Freeze this original
  # launcher too, so a later source edit cannot change its exit/hash checks.
  frozen="$evidence/jammy-core-archive-runner.sh"
  [[ ! -e $frozen && ! -L $frozen ]] || exit 2
  cp -- "$0" "$frozen"
  exec env GROK_CU_FROZEN_RUNNER=1 GROK_CU_FROZEN_SOURCE_REPO="$repo" \
    /bin/bash "$frozen" "$@"
fi
generation=${3:-02}
[[ $generation == 02 || $generation == 03 || $generation == 04 || $generation == 05 || $generation == 06 ]] || exit 2
candidate=/work/core-runtime-bounds-$generation
stage="$root$candidate"
[[ ! -e $stage && ! -L $stage && ! -e $evidence/jammy-core-archive.log ]] || exit 2
mkdir -m 700 "$stage"
mkdir -m 700 "$stage/src-tauri"
# Preserve the actual workspace graph and lock, not a standalone re-resolution.
for relative in Cargo.toml Cargo.lock build.rs src computer-use-core computer-use-x11 \
  computer-use-wayland computer-use-gtk-parent cu-probe vendor; do
  source_path="$repo/src-tauri/$relative"
  [[ -e $source_path && ! -L $source_path ]] || exit 2
  [[ -z $(find "$source_path" ! -type f ! -type d -print -quit) ]] || exit 2
  cp -a -- "$source_path" "$stage/src-tauri/$relative"
done
if [[ $generation == 03 || $generation == 04 || $generation == 05 || $generation == 06 ]]; then
  # Include the actual resources and source contracts required by full Core
  # tests, not just enough files for a narrowly filtered archive test.
  mkdir -m 700 "$stage/src-tauri/resources" "$stage/src-tauri/resources/computer-use"
  for relative in .gitignore package.json scripts .github/workflows tools/computer-use-browser \
    tools/computer-use-protocol tools/computer-use-mcp src-tauri/nsis \
    src-tauri/tauri.conf.json src-tauri/tauri.windows.conf.json \
    src-tauri/tauri.macos.conf.json src-tauri/tauri.linux.conf.json; do
    source_path="$repo/$relative"
    [[ -e $source_path && ! -L $source_path ]] || exit 2
    [[ -z $(find "$source_path" ! -type f ! -type d -print -quit) ]] || exit 2
    mkdir -p -- "$(dirname -- "$stage/$relative")"
    cp -a -- "$source_path" "$stage/$relative"
  done
  for lock_name in windows-x64.lock.json linux-x64.lock.json macos-arm64.lock.json \
    macos-x64.lock.json pack-targets.json README.md; do
    cp -- "$repo/src-tauri/resources/computer-use/$lock_name" "$stage/src-tauri/resources/computer-use/$lock_name"
  done
  native_seed="$root/work/product-app-03/src-tauri/resources/computer-use/seed"
  [[ -d $native_seed && ! -L $native_seed && -z $(find "$native_seed" ! -type f ! -type d -print -quit) ]] || exit 2
  # Tests clone/detach before mutations. Both original inputs and shared seed
  # bytes remain in the frozen before/after hash inventory below.
  cp -al -- "$native_seed" "$stage/src-tauri/resources/computer-use/seed"
  node_source="$repo/.cache/computer-use/platform-source-20260925/node-v20.18.0-linux-x64.tar.gz"
  [[ -f $node_source && ! -L $node_source ]] || exit 2
  digest=$(sha256sum -- "$node_source")
  digest=${digest%% *}
  mkdir -p "$stage/.cache/computer-use/sha256"
  cp -- "$node_source" "$stage/.cache/computer-use/sha256/$digest"
fi
cp -- "$repo/tools/computer-use-probe/jammy-core-archive-inner.sh" "$stage/jammy-core-archive-inner.sh"
find "$stage" -type f -print0 | sort -z | xargs -0 sha256sum > "$evidence/core-inputs.sha256"
status=0
unshare --mount --pid --fork --mount-proc bash -c '
  set -euo pipefail
  mount --make-rprivate /
  mount --bind /dev "$1/dev"
  mount -t proc proc "$1/proc"
  chroot "$1" /usr/bin/env -i PATH=/work/rust-toolchain-1.98/bin:/usr/bin:/bin:/usr/sbin:/sbin \
    CARGO_HOME=/work/cu-cargo-home CARGO_TARGET_DIR="$2/target" \
    CARGO_NET_OFFLINE=true CARGO_BUILD_JOBS=2 LC_ALL=C.UTF-8 \
    GROK_CU_ARCHIVE_CANDIDATE="$2" \
    /bin/bash "$2/jammy-core-archive-inner.sh"
' owned-jammy-core-archive "$root" "$candidate" 2>&1 | tee "$evidence/jammy-core-archive.log" || status=$?
sha256sum --check --quiet "$evidence/core-inputs.sha256"
printf 'Current Core archive candidate exit=%s; frozen inputs unchanged.\n' "$status"
exit "$status"

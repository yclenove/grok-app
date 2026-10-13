#!/usr/bin/env bash
# Native Node22/PW1.64 source-pair audit. Never prepares/promotes a CU pack.
set -euo pipefail
[[ $# == 2 && $(id -u) == 0 && $1 == /var/tmp/grok-cu-jammy-sdk-20261012-01 ]] || exit 2
root=$1
evidence=$2
[[ -d $root && ! -L $root && $(realpath -e -- "$root") == "$root" ]] || exit 2
[[ $evidence == /* && -d $evidence && ! -L $evidence && $(realpath -e -- "$evidence") == "$evidence" ]] || exit 2
repo=${GROK_CU_FROZEN_SOURCE_REPO:-$(cd -- "$(dirname -- "$0")/../.." && pwd)}
[[ -d $repo && ! -L $repo && $(realpath -e -- "$repo") == "$repo" ]] || exit 2
if [[ ${GROK_CU_FROZEN_RUNNER:-0} != 1 ]]; then
  frozen="$evidence/jammy-maintained-sources-runner.sh"
  [[ ! -e $frozen && ! -L $frozen ]] || exit 2
  cp -- "$0" "$frozen"
  exec env GROK_CU_FROZEN_RUNNER=1 GROK_CU_FROZEN_SOURCE_REPO="$repo" /bin/bash "$frozen" "$@"
fi
for process_root in /proc/[0-9]*/root; do
  if [[ $(readlink -- "$process_root" 2>/dev/null || true) == "$root" ]]; then
    printf 'Original private-root owner is still live: %s\n' "$process_root" >&2
    exit 2
  fi
done
source_candidate="$repo/tools/computer-use-probe/.run/runtime-sources-20261012-01/candidate"
candidate=/work/core-maintained-sources-01
stage="$root$candidate"
[[ -d $source_candidate && ! -L $source_candidate && ! -e $stage && ! -L $stage ]] || exit 2
[[ ! -e $evidence/native-maintained-sources.log ]] || exit 2
mkdir -m 700 "$stage"
[[ -z $(find "$source_candidate/src-tauri" ! -type f ! -type d -print -quit) ]] || exit 2
cp -a -- "$source_candidate/src-tauri" "$stage/src-tauri"
grep -Fxq 'pub const REQUIRED_JS_RUNTIME_VERSION: &str = "22.23.3";' "$stage/src-tauri/computer-use-core/src/runtime_pins.rs"
grep -Fxq 'pub const REQUIRED_PLAYWRIGHT_CORE_VERSION: &str = "1.64.0";' "$stage/src-tauri/computer-use-core/src/runtime_pins.rs"
mkdir -m 700 "$stage/archive-cache" "$stage/archive-cache/sha256"
for digest in 1084aa36196bba4c3a5e69a1ee388a6e4ff729dad09445fbcd434b28fe3c24af \
  90c8b6b19da0a1b1e9a004cd6ff3e49e82668a0856e01efcc0e2d9c2b4466bb6; do
  archive="$source_candidate/archive-cache/sha256/$digest"
  [[ -f $archive && ! -L $archive ]] || exit 2
  printf '%s  %s\n' "$digest" "$archive" | sha256sum --check --quiet
  cp -- "$archive" "$stage/archive-cache/sha256/$digest"
done
cp -- "$repo/tools/computer-use-probe/jammy-maintained-sources-inner.sh" "$stage/jammy-maintained-sources-inner.sh"
find "$stage" -type f -print0 | sort -z | xargs -0 sha256sum > "$evidence/candidate-inputs.sha256"
status=0
unshare --mount --pid --fork --mount-proc bash -c '
  set -euo pipefail
  mount --make-rprivate /
  mount --bind /dev "$1/dev"
  mount -t proc proc "$1/proc"
  chroot "$1" /usr/bin/env -i PATH=/work/rust-toolchain-1.98/bin:/usr/bin:/bin:/usr/sbin:/sbin \
    CARGO_HOME=/work/cu-cargo-home CARGO_TARGET_DIR="$2/target" \
    CARGO_NET_OFFLINE=true CARGO_BUILD_JOBS=2 LC_ALL=C.UTF-8 \
    GROK_CU_SOURCES_CANDIDATE="$2" /bin/bash "$2/jammy-maintained-sources-inner.sh"
' owned-maintained-sources "$root" "$candidate" 2>&1 | tee "$evidence/native-maintained-sources.log" || status=$?
sha256sum --check --quiet "$evidence/candidate-inputs.sha256"
printf 'Native maintained sources exit=%s; frozen source/cache inputs unchanged.\n' "$status"
exit "$status"

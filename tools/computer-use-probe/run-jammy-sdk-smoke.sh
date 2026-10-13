#!/usr/bin/env bash
# New evidence and frozen inputs; never reuse a previous smoke's output.
set -euo pipefail
if [[ $# != 5 || ! $1 =~ ^/var/tmp/grok-cu-jammy-sdk-[A-Za-z0-9-]+$ ]]; then
  echo 'Usage: run-jammy-sdk-smoke.sh ROOT /work/PW-SDK /work/EI-SDK /work/NEW-OUTPUT /absolute/evidence' >&2
  exit 2
fi
[[ $(id -u) == 0 && -d $1 && ! -L $1 && $5 == /* && -d $5 && ! -L $5 ]] || exit 2
[[ $(realpath -e -- "$1") == "$1" && $(realpath -e -- "$5") == "$5" ]] || exit 2
for path in "$2" "$3" "$4"; do
  [[ $path =~ ^/work/[A-Za-z0-9-]+$ ]] || exit 2
done
root=$1
evidence=$5
stage="$root$4-input"
repo=$(cd -- "$(dirname -- "$0")/../.." && pwd)
[[ ! -e $evidence/smoke.log && ! -e $evidence/smoke-inputs.sha256 && ! -e $stage && ! -L $stage ]] || exit 2
[[ -d $root$2 && -d $root$3 && ! -e $root$4 && ! -L $root$4 ]] || exit 2
mkdir -m 700 "$stage"
mkdir -p "$stage/scripts" "$stage/tools/computer-use-probe"
cp -- "$repo/scripts/check-computer-use-pipewire-runtime.sh" \
  "$repo/scripts/audit-computer-use-native-sdk.py" "$stage/scripts/"
cp -- "$repo/tools/computer-use-probe/jammy-sdk-smoke.c" \
  "$repo/tools/computer-use-probe/jammy-sdk-smoke-inner.sh" "$stage/tools/computer-use-probe/"
(cd -- "$stage" && sha256sum scripts/* tools/computer-use-probe/*) > "$evidence/smoke-inputs.sha256"
unshare --mount --pid --fork --mount-proc bash -c '
  set -euo pipefail
  mount --make-rprivate /
  mount --bind /dev "$1/dev"
  mount -t proc proc "$1/proc"
  chroot "$1" /usr/bin/env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin \
    GROK_CU_JAMMY_SCRIPT_ROOT="$4-input" \
    /bin/bash "$4-input/tools/computer-use-probe/jammy-sdk-smoke-inner.sh" "$2" "$3" "$4"
' owned-jammy-smoke "$root" "$2" "$3" "$4" 2>&1 | tee "$evidence/smoke.log"

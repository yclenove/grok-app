#!/usr/bin/env bash
set -euo pipefail
if [[ $# != 3 || ! $1 =~ ^/var/tmp/grok-cu-jammy-sdk-[A-Za-z0-9-]+$ || ! -d $1 || -L $1 ]]; then
  echo 'Usage: run-jammy-sdk-compile.sh /var/tmp/grok-cu-jammy-sdk-EXISTING /absolute/evidence /work/NEW-PW-SDK' >&2
  exit 2
fi
[[ $(id -u) == 0 && $2 == /* && -d $2 && ! -L $2 ]] || exit 2
[[ $(realpath -e -- "$1") == "$1" && $(realpath -e -- "$2") == "$2" ]] || exit 2
root=$1
evidence=$2
[[ $3 =~ ^/work/[A-Za-z0-9-]+$ && ! -e $root$3 && ! -L $root$3 ]] || exit 2
repo=$(cd -- "$(dirname -- "$0")/../.." && pwd)
stage="$root$3-input"
[[ ! -e $evidence/build.log && ! -e $stage && ! -L $stage ]] || exit 2
mkdir -m 700 "$stage"
cp -- "$repo/scripts/build-computer-use-pipewire.sh" \
  "$repo/scripts/check-computer-use-linux-sdk.sh" \
  "$repo/tools/computer-use-probe/jammy-sdk-compile-inner.sh" "$stage/"
cp -- "$evidence/pipewire_1.6.9.orig.tar.gz" "$evidence/pipewire_1.6.9-2.debian.tar.xz" "$stage/"
unshare --mount --pid --fork --mount-proc bash -c '
  set -euo pipefail
  mount --make-rprivate /
  mount --bind /dev "$1/dev"
  mount -t proc proc "$1/proc"
  chroot "$1" /usr/bin/env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin \
    GROK_CU_JAMMY_SDK_ROOT=/work /bin/bash "$2-input/jammy-sdk-compile-inner.sh" "$2"
' owned-jammy-compile "$root" "$3" 2>&1 | tee "$evidence/build.log"

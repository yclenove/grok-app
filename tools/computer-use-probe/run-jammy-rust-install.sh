#!/usr/bin/env bash
set -euo pipefail
[[ $# == 2 && $1 =~ ^/var/tmp/grok-cu-jammy-sdk-[A-Za-z0-9-]+$ && -d $1 && ! -L $1 ]] || exit 2
[[ $(id -u) == 0 && $2 == /* && -d $2 && ! -L $2 ]] || exit 2
[[ $(realpath -e -- "$1") == "$1" && $(realpath -e -- "$2") == "$2" ]] || exit 2
[[ ! -e $2/rust-install.log && ! -e $1/work/jammy-rust-install-inner.sh ]] || exit 2
repo=$(cd -- "$(dirname -- "$0")/../.." && pwd)
cp -- "$repo/tools/computer-use-probe/jammy-rust-install-inner.sh" "$1/work/"
unshare --mount --pid --fork --mount-proc bash -c '
  set -euo pipefail
  mount --make-rprivate /
  mount --bind /dev "$1/dev"
  mount -t proc proc "$1/proc"
  chroot "$1" /usr/bin/env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin \
    GROK_CU_JAMMY_SDK_ROOT=/work /bin/bash /work/jammy-rust-install-inner.sh
' owned-jammy-rust "$1" 2>&1 | tee "$2/rust-install.log"

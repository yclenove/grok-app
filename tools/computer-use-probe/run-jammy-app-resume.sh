#!/usr/bin/env bash
set -euo pipefail
[[ $# == 3 && $1 =~ ^/var/tmp/grok-cu-jammy-sdk-[A-Za-z0-9-]+$ && -d $1 && ! -L $1 ]] || exit 2
[[ $(id -u) == 0 && $2 == /* && -d $2 && ! -L $2 ]] || exit 2
[[ $(realpath -e -- "$1") == "$1" && $(realpath -e -- "$2") == "$2" ]] || exit 2
[[ $3 =~ ^/work/product-app-[A-Za-z0-9-]+$ && -d $1$3 && ! -L $1$3 && ! -e $2/app-resume.log ]] || exit 2
stage="$1$3"
[[ ! -e $stage/tools/computer-use-browser && ! -e $stage/tools/computer-use-probe/jammy-app-resume-inner.sh ]] || exit 2
repo=$(cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$repo"
tar --exclude='.run' --exclude='node_modules' --exclude='__pycache__' -cf - \
  tools/computer-use-browser tools/computer-use-probe/jammy-app-resume-inner.sh | tar -xf - -C "$stage"
find "$stage/tools/computer-use-browser" -type f -print0 | sort -z | xargs -0 sha256sum > "$2/added-worker-inputs.sha256"
sha256sum tools/computer-use-probe/jammy-app-resume-inner.sh >> "$2/added-worker-inputs.sha256"
unshare --mount --pid --fork --mount-proc bash -c '
  set -euo pipefail
  mount --make-rprivate /
  mount --bind /dev "$1/dev"
  mount -t proc proc "$1/proc"
  chroot "$1" /usr/bin/env -i PATH=/work/rust-toolchain-1.98/bin:/usr/bin:/bin:/usr/sbin:/sbin \
    GROK_CU_JAMMY_SDK_ROOT=/work LC_ALL=C.UTF-8 \
    /bin/bash "$2/tools/computer-use-probe/jammy-app-resume-inner.sh" "$2"
' owned-jammy-app-resume "$1" "$3" 2>&1 | tee "$2/app-resume.log"

#!/usr/bin/env bash
# Freeze only relevant source inputs. Do not bind a live Windows worktree.
set -euo pipefail
[[ $# == 3 && $1 =~ ^/var/tmp/grok-cu-jammy-sdk-[A-Za-z0-9-]+$ && -d $1 && ! -L $1 ]] || exit 2
[[ $(id -u) == 0 && $2 == /* && -d $2 && ! -L $2 ]] || exit 2
[[ $(realpath -e -- "$1") == "$1" && $(realpath -e -- "$2") == "$2" ]] || exit 2
[[ $3 =~ ^/work/product-loader-[A-Za-z0-9-]+$ && ! -e $1$3 && ! -L $1$3 && ! -e $2/loader.log ]] || exit 2
repo=$(cd -- "$(dirname -- "$0")/../.." && pwd)
stage="$1$3"
inputs=(
  scripts/audit-computer-use-native-sdk.py scripts/stage-computer-use-native-sdk.py
  scripts/test-stage-computer-use-native-sdk.py scripts/test-computer-use-native-sdk.py
  src-tauri/build/linux_native.rs src-tauri/src/linux_native_runtime.rs
  src-tauri/src/linux_native_runtime/tests.rs
  tools/computer-use-probe/native-loader/Cargo.toml
  tools/computer-use-probe/native-loader/Cargo.lock
  tools/computer-use-probe/native-loader/build.rs tools/computer-use-probe/native-loader/src/main.rs
  tools/computer-use-probe/jammy-product-loader-inner.sh
  tools/computer-use-probe/jammy-product-loader-faults.py
)
mkdir -m 700 "$stage"
cd "$repo"
cp --parents -- "${inputs[@]}" "$stage/"
sha256sum -- "${inputs[@]}" > "$2/inputs.sha256"
unshare --mount --pid --fork --mount-proc bash -c '
  set -euo pipefail
  mount --make-rprivate /
  mount --bind /dev "$1/dev"
  mount -t proc proc "$1/proc"
  chroot "$1" /usr/bin/env -i PATH=/work/rust-toolchain-1.98/bin:/usr/bin:/bin:/usr/sbin:/sbin \
    GROK_CU_JAMMY_SDK_ROOT=/work LC_ALL=C.UTF-8 \
    /bin/bash "$2/tools/computer-use-probe/jammy-product-loader-inner.sh" "$2"
' owned-jammy-product-loader "$1" "$3" 2>&1 | tee "$2/loader.log"
cp -- "$stage/acceptance/results.json" "$2/loader-results.json"
cp -- "$stage/tools/computer-use-probe/native-loader/Cargo.lock" "$2/loader-Cargo.lock"
printf 'Original loader probe exited; fixtures retained for exact-owner cleanup.\n'

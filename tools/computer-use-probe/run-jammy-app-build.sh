#!/usr/bin/env bash
# Whole Linux App build on Jammy, preserving the host worktree/profile/runtime.
set -euo pipefail
[[ $# == 3 && $1 =~ ^/var/tmp/grok-cu-jammy-sdk-[A-Za-z0-9-]+$ && -d $1 && ! -L $1 ]] || exit 2
[[ $(id -u) == 0 && $2 == /* && -d $2 && ! -L $2 ]] || exit 2
[[ $(realpath -e -- "$1") == "$1" && $(realpath -e -- "$2") == "$2" ]] || exit 2
[[ $3 =~ ^/work/product-app-[A-Za-z0-9-]+$ && ! -e $1$3 && ! -L $1$3 && ! -e $2/app-build.log ]] || exit 2
repo=$(cd -- "$(dirname -- "$0")/../.." && pwd)
stage="$1$3"
[[ -f $repo/dist/index.html && -x $1/work/rust-toolchain-1.98/bin/cargo ]] || exit 2
mkdir -m 700 "$stage"
cd "$repo"
inputs=(src-tauri/src src-tauri/build src-tauri/build.rs src-tauri/Cargo.toml src-tauri/Cargo.lock
  src-tauri/computer-use-core src-tauri/computer-use-x11 src-tauri/computer-use-wayland
  src-tauri/computer-use-gtk-parent src-tauri/cu-probe src-tauri/vendor src-tauri/icons
  src-tauri/capabilities src-tauri/tests src-tauri/tauri.conf.json src-tauri/tauri.linux.conf.json
  src-tauri/resources/computer-use/linux-x64.lock.json src-tauri/resources/computer-use/pack-targets.json
  src-tauri/resources/computer-use/README.md
  tools/computer-use-extension tools/computer-use-mcp tools/computer-use-gnome-shell tools/computer-use-browser
  scripts/official-aux-mcp.mjs scripts/audit-computer-use-native-sdk.py
  scripts/stage-computer-use-native-sdk.py scripts/check-computer-use-linux-sdk.sh
  tools/computer-use-probe/jammy-app-build-inner.sh dist)
tar --exclude='target' --exclude='target-*' --exclude='.run' --exclude='node_modules' \
  --exclude='__pycache__' -cf - -- "${inputs[@]}" | tar -xf - -C "$stage"
find "$stage" -type f -print0 | sort -z | xargs -0 sha256sum > "$2/app-inputs.sha256"
unshare --mount --pid --fork --mount-proc bash -c '
  set -euo pipefail
  mount --make-rprivate /
  mount --bind /dev "$1/dev"
  mount -t proc proc "$1/proc"
  chroot "$1" /usr/bin/env -i PATH=/work/rust-toolchain-1.98/bin:/usr/bin:/bin:/usr/sbin:/sbin \
    GROK_CU_JAMMY_SDK_ROOT=/work LC_ALL=C.UTF-8 \
    /bin/bash "$2/tools/computer-use-probe/jammy-app-build-inner.sh" "$2"
' owned-jammy-app "$1" "$3" 2>&1 | tee "$2/app-build.log"
printf 'Original Jammy App build exited; raw ELF is not a distributable installer.\n'

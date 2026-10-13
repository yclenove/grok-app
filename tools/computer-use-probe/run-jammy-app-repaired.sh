#!/usr/bin/env bash
# New frozen source tree; retain the original failing tree and compiler logs.
set -euo pipefail
[[ $# == 4 && $(id -u) == 0 && $1 =~ ^/var/tmp/grok-cu-jammy-sdk-[A-Za-z0-9-]+$ ]] || exit 2
[[ -d $1 && ! -L $1 && $(realpath -e -- "$1") == "$1" ]] || exit 2
[[ $2 == /* && -d $2 && ! -L $2 && $(realpath -e -- "$2") == "$2" ]] || exit 2
[[ $3 =~ ^/work/product-app-[A-Za-z0-9-]+$ && $4 =~ ^/work/product-app-[A-Za-z0-9-]+$ && $3 != "$4" ]] || exit 2
[[ -d $1$3 && ! -L $1$3 && ! -e $1$4 && ! -L $1$4 && ! -e $2/app-repaired.log ]] || exit 2
for process_root in /proc/[0-9]*/root; do
  if [[ $(readlink -- "$process_root" 2>/dev/null || true) == "$1" ]]; then
    printf 'Original private-root owner still live: %s\n' "$process_root" >&2
    exit 2
  fi
done
root=$1
evidence=$2
origin="$root$3"
stage="$root$4"
repo=$(cd -- "$(dirname -- "$0")/../.." && pwd)
[[ $(realpath -e -- "$origin") == "$origin" ]] || exit 2
# Only two diagnosed, platform-specific fixes may differ from the failed input.
python3 - "$origin" "$repo" <<'PY'
import sys
from pathlib import Path
origin, repo = map(Path, sys.argv[1:])
edits = [
    ("src-tauri/src/computer_use/webview/navigation.rs",
     b"use webkit2gtk::prelude::WebViewExt;", b"use webkit2gtk::WebViewExt;"),
    ("src-tauri/src/side_browser_host.rs",
     b"    let cu_download_instance = cu_native", b"    #[cfg(windows)]\n    let cu_download_instance = cu_native"),
]
for name, before, after in edits:
    original = (origin / name).read_bytes().replace(b"\r\n", b"\n")
    current = (repo / name).read_bytes().replace(b"\r\n", b"\n")
    if original.count(before) != 1 or original.replace(before, after) != current:
        raise SystemExit(f"Unexpected source drift: {name}")
PY
mkdir -m 700 "$stage"
tar -C "$origin" --exclude='./src-tauri/target' --exclude='./src-tauri/resources/computer-use/seed' \
  -cf - . | tar -xf - -C "$stage"
# Share immutable seed file data, not directory links; never --prepare this copy.
seed='src-tauri/resources/computer-use/seed'
[[ $(realpath -e -- "$origin/$seed") == "$origin/$seed" ]] || exit 2
find "$origin/$seed" -type f -print0 | sort -z | xargs -0 sha256sum > "$evidence/original-seed.sha256"
cp -al -- "$origin/$seed" "$stage/$seed"
for relative in src-tauri/src/computer_use/webview/navigation.rs src-tauri/src/side_browser_host.rs; do
  cp -- "$repo/$relative" "$stage/$relative"
done
cp -- "$repo/tools/computer-use-probe/jammy-app-repaired-inner.sh" "$stage/tools/computer-use-probe/"
find "$stage" -type f -print0 | sort -z | xargs -0 sha256sum > "$evidence/app-inputs.sha256"
unshare --mount --pid --fork --mount-proc bash -c '
  set -euo pipefail
  mount --make-rprivate /
  mount --bind /dev "$1/dev"
  mount -t proc proc "$1/proc"
  chroot "$1" /usr/bin/env -i PATH=/work/rust-toolchain-1.98/bin:/usr/bin:/bin:/usr/sbin:/sbin \
    GROK_CU_JAMMY_SDK_ROOT=/work LC_ALL=C.UTF-8 CARGO_NET_OFFLINE=true \
    /bin/bash "$2/tools/computer-use-probe/jammy-app-repaired-inner.sh" "$2" "$3"
' owned-jammy-app-repaired "$root" "$4" "$3" 2>&1 | tee "$evidence/app-repaired.log"
sha256sum --check --quiet "$evidence/app-inputs.sha256"
sha256sum --check --quiet "$evidence/original-seed.sha256"
printf 'Repaired App build, strict Clippy and navigation regressions completed; source/seed unchanged.\n'

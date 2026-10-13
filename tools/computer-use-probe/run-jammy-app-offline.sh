#!/usr/bin/env bash
# Resume the frozen full App with byte-identical, already acquired archives.
# No new mirror, changed lock, host DNS/proxy mutation or skipped runtime check.
set -euo pipefail
[[ $# == 3 && $1 =~ ^/var/tmp/grok-cu-jammy-sdk-[A-Za-z0-9-]+$ ]] || exit 2
[[ $(id -u) == 0 && -d $1 && ! -L $1 && $(realpath -e -- "$1") == "$1" ]] || exit 2
[[ $2 == /* && -d $2 && ! -L $2 && $(realpath -e -- "$2") == "$2" ]] || exit 2
[[ $3 =~ ^/work/product-app-[A-Za-z0-9-]+$ && -d $1$3 && ! -L $1$3 ]] || exit 2
[[ ! -e $2/app-offline.log && ! -e $2/archive-inputs.sha256 ]] || exit 2
root=$1
evidence=$2
stage="$root$3"
repo=$(cd -- "$(dirname -- "$0")/../.." && pwd)
[[ $(realpath -e -- "$stage") == "$stage" ]] || exit 2
for process_root in /proc/[0-9]*/root; do
  if [[ $(readlink -- "$process_root" 2>/dev/null || true) == "$root" ]]; then
    printf 'Original private-root owner still live: %s\n' "$process_root" >&2
    exit 2
  fi
done
[[ -f $stage/tools/computer-use-probe/jammy-app-resume-inner.sh ]] || exit 2
lock="$stage/src-tauri/resources/computer-use/linux-x64.lock.json"
cache="$root/work/cu-runtime-archive-cache"
[[ ! -L $cache && ! -L $cache/sha256 ]] || exit 2
mkdir -p -m 700 "$cache/sha256"
[[ $(realpath -e -- "$cache/sha256") == "$cache/sha256" ]] || exit 2
# Read only the frozen lock. Archive copying/hashing uses ordinary shell tools.
specs=$(python3 - "$lock" <<'PY'
import json
import re
import sys
from pathlib import Path
lock = json.loads(Path(sys.argv[1]).read_bytes())
if lock["target"] != "x86_64-linux":
    raise SystemExit("Frozen Linux target required")
for key in ("jsRuntime", "playwright", "chromium"):
    component = lock[key]
    digest, size = component["archiveSha256"], component["archiveBytes"]
    if not re.fullmatch("[a-f0-9]{64}", digest) or type(size) is not int or size <= 0:
        raise SystemExit("Invalid frozen archive identity")
    print(key, digest, size)
PY
)
printf '%s\n' "$specs" > "$evidence/locked-archives.txt"
while read -r component digest bytes; do
  case "$component" in
    jsRuntime) source="$repo/.cache/computer-use/platform-source-20260925/node-v20.18.0-linux-x64.tar.gz" ;;
    playwright) source="$repo/.cache/computer-use/sha256/$digest" ;;
    chromium) source="$repo/.cache/computer-use/platform-source-20260925/chromium-linux.zip" ;;
    *) exit 2 ;;
  esac
  [[ -f $source && ! -L $source && $(realpath -e -- "$source") == "$source" ]] || exit 2
  [[ $(stat -c %s -- "$source") == "$bytes" ]] || exit 2
  printf '%s  %s\n' "$digest" "$source" | sha256sum --check --status || exit 2
  printf '%s  %s\n' "$digest" "$source" >> "$evidence/archive-inputs.sha256"
  destination="$cache/sha256/$digest"
  [[ ! -L $destination ]] || exit 2
  if [[ ! -e $destination ]]; then
    cp --no-clobber -- "$source" "$destination"
  fi
  [[ -f $destination && $(stat -c %s -- "$destination") == "$bytes" ]] || exit 2
  printf '%s  %s\n' "$digest" "$destination" | sha256sum --check --status || exit 2
done <<< "$specs"
sha256sum "$lock" "$stage/tools/computer-use-probe/jammy-app-resume-inner.sh" > "$evidence/frozen-entry.sha256"
unshare --mount --pid --fork --mount-proc bash -c '
  set -euo pipefail
  mount --make-rprivate /
  mount --bind /dev "$1/dev"
  mount -t proc proc "$1/proc"
  chroot "$1" /usr/bin/env -i PATH=/work/rust-toolchain-1.98/bin:/usr/bin:/bin:/usr/sbin:/sbin \
    GROK_CU_JAMMY_SDK_ROOT=/work LC_ALL=C.UTF-8 CARGO_NET_OFFLINE=true \
    /bin/bash "$2/tools/computer-use-probe/jammy-app-resume-inner.sh" "$2"
' owned-jammy-app-offline "$root" "$3" 2>&1 | tee "$evidence/app-offline.log"
printf 'Original offline full-App build exited; installer/desktop gates remain separate.\n'

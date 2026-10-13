#!/usr/bin/env bash
# Freeze the diagnosed release-only cfg fixes; retain prior source and evidence.
set -euo pipefail
[[ $# == 5 && $(id -u) == 0 && $1 =~ ^/var/tmp/grok-cu-jammy-sdk-[A-Za-z0-9-]+$ ]] || exit 2
[[ -d $1 && ! -L $1 && $(realpath -e -- "$1") == "$1" ]] || exit 2
[[ $2 == /* && -d $2 && ! -L $2 && $(realpath -e -- "$2") == "$2" ]] || exit 2
for source in "$3" "$4" "$5"; do
  [[ $source =~ ^/work/product-app-[A-Za-z0-9-]+$ ]] || exit 2
done
[[ $3 != "$4" && $4 != "$5" && -d $1$3 && ! -L $1$3 && -d $1$5 && ! -L $1$5 ]] || exit 2
[[ ! -e $1$4 && ! -L $1$4 && ! -e $2/app-release-cfg.log ]] || exit 2
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
[[ $(realpath -e -- "$origin") == "$origin" && $(realpath -e -- "$root$5") == "$root$5" ]] || exit 2
# Byte comparison prevents unrelated WIP from entering this isolated candidate.
python3 - "$origin" "$repo" <<'PY'
import sys
from pathlib import Path
origin, repo = map(Path, sys.argv[1:])
edits = {
    "src-tauri/src/computer_use/sessions.rs": [
        (b"pub fn revoke(session: &str) {", b"#[cfg(debug_assertions)]\npub fn revoke(session: &str) {"),
    ],
    "src-tauri/src/computer_use/webview.rs": [
        (b"    /// Product path: bind a labeled side-browser child webview that already exists.\n    pub fn bind_side_browser(",
         b"    /// Debug harness convenience; product tasks retain an explicit selection.\n    #[cfg(debug_assertions)]\n    pub fn bind_side_browser("),
    ],
    "src-tauri/src/computer_use/webview/execution.rs": [
        (b"    pub(crate) fn child(&self) -> Self {", b"    #[cfg(any(windows, test))]\n    pub(crate) fn child(&self) -> Self {"),
        (b"    pub(crate) fn is_cancelled(&self) -> bool {", b"    #[cfg(windows)]\n    pub(crate) fn is_cancelled(&self) -> bool {"),
    ],
    "src-tauri/src/computer_use/task_picker.rs": [
        (b"#[cfg(test)]\npub(crate) " + signature,
         b"#[cfg(all(test, windows))]\npub(crate) " + signature)
        for signature in [
            b"fn reservation_owners_for_test()",
            b"fn discovery_owners_for_test()",
            b"fn original_discovery_entry_for_test()",
            b"fn original_reservation_entry_for_test()",
            b"async fn join_abandoned_reservation_for_test(",
            b"async fn join_abandoned_discovery_for_test(",
        ]
    ],
}
for name, changes in edits.items():
    expected = (origin / name).read_bytes().replace(b"\r\n", b"\n")
    for before, after in changes:
        if expected.count(before) != 1:
            raise SystemExit(f"Unexpected frozen input: {name}")
        expected = expected.replace(before, after)
    current = (repo / name).read_bytes().replace(b"\r\n", b"\n")
    if expected != current:
        raise SystemExit(f"Unexpected source drift: {name}")
PY
mkdir -m 700 "$stage"
tar -C "$origin" --exclude='./src-tauri/target' --exclude='./src-tauri/resources/computer-use/seed' \
  -cf - . | tar -xf - -C "$stage"
# Immutable seed data can be shared; this candidate only checks it, never prepares it.
seed='src-tauri/resources/computer-use/seed'
[[ $(realpath -e -- "$origin/$seed") == "$origin/$seed" ]] || exit 2
find "$origin/$seed" -type f -print0 | sort -z | xargs -0 sha256sum > "$evidence/original-seed.sha256"
cp -al -- "$origin/$seed" "$stage/$seed"
for relative in src-tauri/src/computer_use/sessions.rs src-tauri/src/computer_use/webview.rs \
  src-tauri/src/computer_use/webview/execution.rs src-tauri/src/computer_use/task_picker.rs; do
  cp -- "$repo/$relative" "$stage/$relative"
done
# Use the same build commands as the preceding candidate, not a weaker new gate.
find "$stage" -type f -print0 | sort -z | xargs -0 sha256sum > "$evidence/app-inputs.sha256"
status=0
unshare --mount --pid --fork --mount-proc bash -c '
  set -euo pipefail
  mount --make-rprivate /
  mount --bind /dev "$1/dev"
  mount -t proc proc "$1/proc"
  chroot "$1" /usr/bin/env -i PATH=/work/rust-toolchain-1.98/bin:/usr/bin:/bin:/usr/sbin:/sbin \
    GROK_CU_JAMMY_SDK_ROOT=/work LC_ALL=C.UTF-8 CARGO_NET_OFFLINE=true \
    /bin/bash "$2/tools/computer-use-probe/jammy-app-repaired-inner.sh" "$2" "$3"
' owned-jammy-app-release-cfg "$root" "$4" "$5" 2>&1 | tee "$evidence/app-release-cfg.log" || status=$?
# Verify immutable inputs even when a compiler gate fails; keep original exit status.
# Runtime --check changes one generated owner sidecar, not source/seed content.
bash "$repo/tools/computer-use-probe/check-jammy-app-inputs.sh" "$root" "$evidence" "$4"
printf 'Release-cfg candidate gate exit=%s; frozen content/seed unchanged.\n' "$status"
exit "$status"

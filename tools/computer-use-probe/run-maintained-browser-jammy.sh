#!/usr/bin/env bash
# Internal maintained browser diagnostic only, never a product pack/install.
set -euo pipefail
[[ $# == 3 && $(id -u) == 0 ]] || exit 2
root=/var/tmp/grok-cu-jammy-sdk-20261012-01
source_stage=$1
evidence=$2
generation=$3
[[ $generation =~ ^[0-9]{2}$ && $generation != 00 ]] || exit 2
[[ $source_stage == /mnt/h/aicoding/grok-app-computer-use/tools/computer-use-probe/.run/maintained-native-20261013-linux-01 ]] || exit 2
suffix=-$generation
[[ $generation != 01 ]] || suffix=
[[ $evidence == "$source_stage/jammy-evidence$suffix" ]] || exit 2
for directory in "$root" "$source_stage"; do
  [[ -d $directory && ! -L $directory && $(realpath -e -- "$directory") == "$directory" ]] || exit 2
done
[[ ! -e $evidence && ! -L $evidence ]] || exit 2
for process_root in /proc/[0-9]*/root; do
  [[ $(readlink -- "$process_root" 2>/dev/null || true) != "$root" ]] || {
    printf 'Original private-root owner is still live: %s\n' "$process_root" >&2
    exit 2
  }
done
candidate=/work/maintained-native-157-$generation
stage="$root$candidate"
[[ ! -e $stage && ! -L $stage ]] || exit 2
[[ -z $(find "$source_stage" ! -type f ! -type d -print -quit) ]] || exit 2
mkdir -m 700 "$stage" "$evidence"
for item in node worker browser diagnostic-inputs.json; do
  cp -a -- "$source_stage/$item" "$stage/$item"
done
chmod 755 "$stage/node/bin/node" "$stage/browser/chrome" "$stage/browser/chrome_crashpad_handler"
repo=$(cd -- "$(dirname -- "$0")/../.." && pwd)
cp -- "$repo/tools/computer-use-probe/verify-maintained-diagnostic.mjs" "$stage/verify-maintained-diagnostic.mjs"
cp -- "$repo/tools/computer-use-probe/maintained-browser-jammy-inner.sh" "$stage/maintained-browser-jammy-inner.sh"
cp -- "$0" "$evidence/original-runner.sh"
find "$stage" -type f -print0 | sort -z | xargs -0 sha256sum > "$evidence/initial-inputs.sha256"
status=0
unshare --mount --pid --fork --mount-proc bash -c '
  set -euo pipefail
  mount --make-rprivate /
  mount --bind /dev "$1/dev"
  mount -t proc proc "$1/proc"
  chroot "$1" /usr/bin/env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin LC_ALL=C.UTF-8 \
    GROK_CU_DIAGNOSTIC="$2" /bin/bash "$2/maintained-browser-jammy-inner.sh"
' owned-maintained-browser "$root" "$candidate" 2>&1 | tee "$evidence/native-worker.log" || status=$?
[[ ! -e $evidence/native-identity.json ]] || exit 2
if [[ -f $stage/native-identity.json ]]; then
  cp -- "$stage/native-identity.json" "$evidence/native-identity.json"
fi
# Product seeds/user data never entered this exclusive diagnostic directory.
sha256sum --check --quiet "$evidence/initial-inputs.sha256"
printf 'Internal Jammy maintained browser exit=%s; original staged inputs unchanged.\n' "$status"
exit "$status"

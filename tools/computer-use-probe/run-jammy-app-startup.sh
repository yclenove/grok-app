#!/usr/bin/env bash
# Same completed source03 release ELF, private X server/DBus and isolated data.
set -euo pipefail
[[ $# == 3 && $(id -u) == 0 && $1 =~ ^/var/tmp/grok-cu-jammy-sdk-[A-Za-z0-9-]+$ ]] || exit 2
[[ -d $1 && ! -L $1 && $(realpath -e -- "$1") == "$1" ]] || exit 2
[[ $2 == /* && -d $2 && ! -L $2 && $(realpath -e -- "$2") == "$2" ]] || exit 2
[[ $3 =~ ^/work/app-startup-[A-Za-z0-9-]+$ && ! -e $1$3 && ! -L $1$3 ]] || exit 2
[[ ! -e $2/app-startup.log ]] || exit 2
for process_root in /proc/[0-9]*/root; do
  if [[ $(readlink -- "$process_root" 2>/dev/null || true) == "$1" ]]; then
    printf 'Original private-root owner still live: %s\n' "$process_root" >&2
    exit 2
  fi
done
repo=$(cd -- "$(dirname -- "$0")/../.." && pwd)
probe="$1/work/app-startup-probe-01"
[[ ! -e $probe && ! -L $probe ]] || exit 2
mkdir -m 700 "$probe"
for file in jammy-app-startup.py jammy-app-window.c jammy-app-startup-settings.json; do
  cp -- "$repo/tools/computer-use-probe/$file" "$probe/$file"
done
# Record original immutable inputs before running. No seed prepare or source edit.
bash "$repo/tools/computer-use-probe/check-jammy-app-inputs.sh" "$1" "$2" /work/product-app-03
status=0
unshare --mount --pid --fork --mount-proc bash -c '
  set -euo pipefail
  mount --make-rprivate /
  mount --bind /dev "$1/dev"
  mount -t proc proc "$1/proc"
  chroot "$1" /usr/bin/env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin LC_ALL=C.UTF-8 \
    /usr/bin/dbus-run-session -- /usr/bin/xvfb-run -a \
    -s "-screen 0 1200x800x24 -nolisten tcp" \
    /usr/bin/python3 /work/app-startup-probe-01/jammy-app-startup.py /work/product-app-03 "$2"
' owned-jammy-app-startup "$1" "$3" 2>&1 | tee "$2/app-startup.log" || status=$?
bash "$repo/tools/computer-use-probe/check-jammy-app-inputs.sh" "$1" "$2" /work/product-app-03
printf 'Actual release App startup/close gate exit=%s.\n' "$status"
exit "$status"

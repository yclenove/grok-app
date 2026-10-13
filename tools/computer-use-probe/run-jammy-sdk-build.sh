#!/usr/bin/env bash
# One-shot native Jammy SDK build, in private PID/mount namespaces and chroot.
# No user desktop, system services, global SDK install, or source worktree mount.
set -euo pipefail
if [[ $# != 2 || ! $1 =~ ^/var/tmp/grok-cu-jammy-sdk-[A-Za-z0-9-]+$ || -e $1 || -L $1 ]]; then
  echo 'Usage: run-jammy-sdk-build.sh /var/tmp/grok-cu-jammy-sdk-NEW /absolute/evidence' >&2
  exit 2
fi
[[ $(id -u) == 0 && $2 == /* && -d $2 && ! -L $2 ]] || exit 2
[[ $(realpath -e -- "$2") == "$2" ]] || exit 2
root=$1
evidence=$2
repo=$(cd -- "$(dirname -- "$0")/../.." && pwd)
printf '%s  %s\n' \
  242cd8898b33ea806ef5f13b1076ed7c76f9f989d18384452f7166692438ff1a \
  "$evidence/ubuntu-base-22.04.5-base-amd64.tar.gz" | sha256sum --check --strict
mkdir -m 700 "$root"
tar -xzf "$evidence/ubuntu-base-22.04.5-base-amd64.tar.gz" \
  -C "$root" --no-same-owner
mkdir "$root/work"
cp -- "$evidence/pipewire_1.6.9.orig.tar.gz" \
  "$evidence/pipewire_1.6.9-2.debian.tar.xz" "$root/work/"
cp -- "$evidence/libei_1.5.0.orig.tar.bz2" "$root/work/"
cp -- "$repo/scripts/build-computer-use-pipewire.sh" \
  "$repo/scripts/build-computer-use-libei.sh" \
  "$repo/scripts/check-computer-use-linux-sdk.sh" \
  "$repo/tools/computer-use-probe/jammy-sdk-build-inner.sh" "$root/work/"
cp -- /etc/resolv.conf "$root/etc/resolv.conf"
cp -- "$repo/tools/computer-use-probe/jammy-sdk-policy-rc.d" "$root/usr/sbin/policy-rc.d"
chmod 755 "$root/usr/sbin/policy-rc.d"
printf 'root=%s\nouter_pid=%s\nboot_id=%s\n' \
  "$root" "$$" "$(</proc/sys/kernel/random/boot_id)" > "$evidence/owner.txt"
export GROK_CU_JAMMY_SDK_ROOT=/work
# Every mount is confined to this namespace. Original unshare handle is joined
# by the caller; a log file or timeout alone is not an exit acknowledgement.
unshare --mount --pid --fork --mount-proc bash -c '
  set -euo pipefail
  mount --make-rprivate /
  mount --bind /dev "$1/dev"
  mount -t proc proc "$1/proc"
  chroot "$1" /usr/bin/env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin \
    GROK_CU_JAMMY_SDK_ROOT=/work /bin/bash /work/jammy-sdk-build-inner.sh
' owned-jammy-sdk "$root" 2>&1 | tee "$evidence/build.log"
printf 'Original private build exited normally. SDK retained: %s/work\n' "$root"

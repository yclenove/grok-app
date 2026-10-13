#!/usr/bin/env bash
# Add browser runtime libraries to the task-owned SDK root, never the host OS.
set -euo pipefail
[[ $# == 1 && $(id -u) == 0 ]] || exit 2
root=/var/tmp/grok-cu-jammy-sdk-20261012-01
evidence=$1
[[ $evidence == /mnt/h/aicoding/grok-app-computer-use/tools/computer-use-probe/.run/maintained-native-20261013-linux-01/jammy-dependencies-01 ]] || exit 2
for directory in "$root" "$(dirname -- "$evidence")"; do
  [[ -d $directory && ! -L $directory && $(realpath -e -- "$directory") == "$directory" ]] || exit 2
done
[[ ! -e $evidence && ! -L $evidence ]] || exit 2
policy="$root/usr/sbin/policy-rc.d"
[[ -f $policy && ! -L $policy ]] || exit 2
grep -qx 'exit 101' "$policy" || exit 2
for process_root in /proc/[0-9]*/root; do
  [[ $(readlink -- "$process_root" 2>/dev/null || true) != "$root" ]] || {
    printf 'Original private-root owner is still live: %s\n' "$process_root" >&2
    exit 2
  }
done
mkdir -m 700 "$evidence"
cp -- "$0" "$evidence/original-runner.sh"
status=0
unshare --mount --pid --fork --mount-proc bash -c '
  set -euo pipefail
  mount --make-rprivate /
  mount --bind /dev "$1/dev"
  mount -t proc proc "$1/proc"
  chroot "$1" /usr/bin/env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin LC_ALL=C.UTF-8 \
    DEBIAN_FRONTEND=noninteractive /bin/bash -s <<"INNER"
set -euo pipefail
source /etc/os-release
[[ $ID == ubuntu && $VERSION_ID == 22.04 ]] || exit 2
getconf GNU_LIBC_VERSION
browser=/work/maintained-native-157-01/browser/chrome
[[ -f $browser && ! -L $browser ]] || exit 2
printf "Browser loader before installation:\n"
ldd "$browser"
packages=(libnspr4=2:4.35-0ubuntu0.22.04.1 libnss3=2:3.98-0ubuntu0.22.04.4
  libasound2=1.2.6.1-1ubuntu1.2 libasound2-data=1.2.6.1-1ubuntu1.2
  alsa-topology-conf=1.2.5.1-2 alsa-ucm-conf=1.2.6.3-1ubuntu1.12)
apt-get -s --no-install-recommends --no-upgrade --no-remove install "${packages[@]}"
apt-get -y --no-install-recommends --no-upgrade --no-remove install "${packages[@]}"
dpkg-query -W -f="\${binary:Package}\t\${Version}\t\${db:Status-Abbrev}\n" \
  libnspr4 libnss3 libasound2 libasound2-data alsa-topology-conf alsa-ucm-conf
printf "Browser loader after installation:\n"
loader=$(ldd "$browser")
printf "%s\n" "$loader"
[[ $loader != *"not found"* ]] || exit 1
INNER
' owned-jammy-browser-dependencies "$root" 2>&1 | tee "$evidence/install.log" || status=$?
printf 'Private Jammy dependency installation exit=%s; no host package installation.\n' "$status"
exit "$status"

#!/usr/bin/env bash
# No login-session access: PID, network and /tmp namespaces are mandatory.
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
for tool in unshare mount ip python3 getent; do
  command -v "$tool" >/dev/null || { echo "Required tool missing: $tool" >&2; exit 2; }
done
# Both Ubuntu fixture baselines have the daemon account at 1:1. D-Bus needs a
# password-database entry; labwc 0.5.3 refuses UID/GID 0. Map only the caller to
# 1:1 inside this namespace (not overflow/nobody); root remains unmapped.
# Capabilities apply only to the private namespaces for mounts and loopback.
getent passwd 1 >/dev/null
getent group 1 >/dev/null
exec unshare --user --map-user=1 --map-group=1 --keep-caps \
  --mount --net --pid --fork --mount-proc --kill-child=KILL \
  bash -c 'set -euo pipefail
    mount --make-rprivate /
    ip link set lo up
    mount -t tmpfs -o mode=1777 tmpfs /tmp
    mkdir -m 1777 /tmp/.X11-unix
    exec python3 "$@"
  ' cu-private-wayland "$here/run-computer-use-wayland-tests.py" "$@"

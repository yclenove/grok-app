#!/usr/bin/env bash
# No login-session access: PID, network and /tmp namespaces are mandatory.
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
for tool in unshare mount ip python3; do
  command -v "$tool" >/dev/null || { echo "Required tool missing: $tool" >&2; exit 2; }
done
exec unshare --user --map-root-user --mount --net --pid --fork --mount-proc --kill-child=KILL \
  bash -c 'set -euo pipefail
    mount --make-rprivate /
    ip link set lo up
    mount -t tmpfs -o mode=1777 tmpfs /tmp
    mkdir -m 1777 /tmp/.X11-unix
    exec python3 "$@"
  ' cu-private-wayland "$here/run-computer-use-wayland-tests.py" "$@"

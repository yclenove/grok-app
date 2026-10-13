#!/usr/bin/env bash
# No login-session access: PID, network and /tmp namespaces are mandatory.
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
for tool in unshare mount ip python3 getent id; do
  command -v "$tool" >/dev/null || { echo "Required tool missing: $tool" >&2; exit 2; }
done
# Both Ubuntu fixture baselines have the daemon account at 1:1. D-Bus needs a
# password-database entry; labwc 0.5.3 refuses UID/GID 0. Map only the original
# caller to 1:1 inside this namespace (not overflow/nobody); namespace 0 stays
# unmapped. Mapping sudo's root instead loses access to private runner paths.
# Capabilities apply only to the private namespaces for mounts and loopback.
getent passwd 1 >/dev/null
getent group 1 >/dev/null
launcher=()
if [[ $(id -u) == 0 && ( -v SUDO_UID || -v SUDO_GID ) ]]; then
  if [[ ! ${SUDO_UID:-} =~ ^(0|[1-9][0-9]*)$ ||
        ! ${SUDO_GID:-} =~ ^(0|[1-9][0-9]*)$ ]]; then
    echo "Invalid original sudo UID/GID" >&2
    exit 2
  fi
  command -v setpriv >/dev/null || { echo "Required tool missing: setpriv" >&2; exit 2; }
  # Jammy's unshare lacks --map-users/--map-groups. Drop to the original caller
  # before its single-ID map; retain only SYS_ADMIN through this one exec to
  # allow namespace creation on restricted runners. unshare then moves that
  # authority into the new user namespace before executing any fixture code.
  # No supplementary groups or host-root identity reach the fixture.
  launcher=(setpriv --reuid="$SUDO_UID" --regid="$SUDO_GID" --clear-groups
    --inh-caps=-all,+sys_admin --ambient-caps=-all,+sys_admin)
fi
exec "${launcher[@]}" unshare --user --map-user=1 --map-group=1 --keep-caps \
  --mount --net --pid --fork --mount-proc --kill-child=KILL \
  bash -c 'set -euo pipefail
    mount --make-rprivate /
    ip link set lo up
    mount -t tmpfs -o mode=1777 tmpfs /tmp
    mkdir -m 1777 /tmp/.X11-unix
    exec python3 "$@"
  ' cu-private-wayland "$here/run-computer-use-wayland-tests.py" "$@"

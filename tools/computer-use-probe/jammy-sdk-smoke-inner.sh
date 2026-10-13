#!/usr/bin/env bash
# Execute the reusable gate against explicit SDKs, on actual Jammy userspace.
set -euo pipefail
[[ $# == 3 && ${GROK_CU_JAMMY_SCRIPT_ROOT:-} == /work/* ]] || exit 2
source /etc/os-release
[[ $ID == ubuntu && $VERSION_ID == 22.04 ]] || exit 2
getconf GNU_LIBC_VERSION
bash "$GROK_CU_JAMMY_SCRIPT_ROOT/scripts/check-computer-use-pipewire-runtime.sh" "$@"

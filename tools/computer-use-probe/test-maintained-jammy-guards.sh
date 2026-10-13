#!/usr/bin/env bash
# Negative guards run before staging, browser launch or package installation.
set -euo pipefail
[[ $(id -u) == 0 ]] || exit 2
here=$(cd -- "$(dirname -- "$0")" && pwd)
stage=/mnt/h/aicoding/grok-app-computer-use/tools/computer-use-probe/.run/maintained-native-20261013-linux-01
passed=0
reject() {
  local status=0
  "$@" >/dev/null 2>&1 || status=$?
  [[ $status == 2 ]] || { printf 'Guard did not reject with exit2: %s\n' "$*" >&2; exit 1; }
  passed=$((passed + 1))
}
for generation in 00 0 100 ../03 '03/../04'; do
  reject bash "$here/run-maintained-browser-jammy.sh" "$stage" "$stage/jammy-evidence-$generation" "$generation"
done
reject bash "$here/run-maintained-browser-jammy.sh" "$stage" "$stage/jammy-evidence-03" 04
reject bash "$here/run-maintained-browser-jammy.sh" /var/tmp/outside /var/tmp/outside/jammy-evidence-03 03
reject bash "$here/run-maintained-browser-jammy.sh" "$stage" "$stage/jammy-evidence" 01
reject bash "$here/run-maintained-browser-jammy.sh" "$stage" "$stage/jammy-evidence-02" 02
for candidate in /work/maintained-native-157-00 /work/maintained-native-157-3 /work/maintained-native-157-03/../04; do
  reject env "GROK_CU_DIAGNOSTIC=$candidate" bash "$here/maintained-browser-jammy-inner.sh"
done
reject bash "$here/run-jammy-browser-dependencies.sh" /var/tmp/outside
reject bash "$here/run-jammy-browser-dependencies.sh" "$stage/jammy-dependencies-01"
printf 'Maintained Jammy pre-execution guards: %s passed, 0 skipped.\n' "$passed"

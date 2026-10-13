#!/usr/bin/env bash
# Internal native candidate only: no product seed, App, user profile or release.
set -euo pipefail
[[ $# == 1 && $1 == /* && -d $1 && ! -L $1 ]] || exit 2
[[ $(realpath -e -- "$1") == "$1" && $(uname -m) == aarch64 ]] || exit 2
source /etc/os-release
[[ $ID == ubuntu && $VERSION_ID == 24.04 ]] || exit 2
[[ $(getconf GNU_LIBC_VERSION) == 'glibc 2.39' ]] || exit 2
diagnostic=$1
node="$diagnostic/node/bin/node"
[[ -f $node && ! -L $node ]] || exit 2
"$node" tools/computer-use-probe/verify-maintained-diagnostic.mjs "$diagnostic"
export GROK_CU_TEST_CHROME="$diagnostic/browser/chrome"
export GROK_CU_CHROME="$GROK_CU_TEST_CHROME"
export GROK_CU_ALLOW_SYSTEM_CHROME=0
export GROK_CU_PLAYWRIGHT_VERSION=1.64.0
export GROK_CU_BROWSER_VERSION=156.0.8078.4
export GROK_CU_TEST_WORKER="$diagnostic/worker/server.mjs"
"$node" --test --test-reporter=tap --test-concurrency=1 \
  "$diagnostic/worker/typed-act-http.test.mjs" \
  "$diagnostic/worker/observation-http.test.mjs" \
  "$diagnostic/worker/observation-mutation-http.test.mjs" \
  "$diagnostic/worker/worker-error-http.test.mjs" \
  "$diagnostic/worker/cleanup-http.test.mjs" | tee "$diagnostic/worker-tests.tap"
# The same five source suites currently contain 28 tests including subtests.
# A missing native dependency must fail; skipped work is not ARM64 evidence.
for summary in '# tests 28' '# pass 28' '# fail 0' '# cancelled 0' '# skipped 0' '# todo 0'; do
  grep -Fx "$summary" "$diagnostic/worker-tests.tap" >/dev/null || exit 1
done

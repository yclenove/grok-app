#!/usr/bin/env bash
set -euo pipefail
[[ $# == 0 && ${GROK_CU_DIAGNOSTIC:-} =~ ^/work/maintained-native-157-[0-9]{2}$ ]] || exit 2
[[ $GROK_CU_DIAGNOSTIC != /work/maintained-native-157-00 ]] || exit 2
source /etc/os-release
[[ $ID == ubuntu && $VERSION_ID == 22.04 ]] || exit 2
getconf GNU_LIBC_VERSION
cd "$GROK_CU_DIAGNOSTIC"
node="$GROK_CU_DIAGNOSTIC/node/bin/node"
"$node" ./verify-maintained-diagnostic.mjs "$GROK_CU_DIAGNOSTIC"
export GROK_CU_TEST_CHROME="$GROK_CU_DIAGNOSTIC/browser/chrome"
export GROK_CU_CHROME="$GROK_CU_TEST_CHROME"
export GROK_CU_ALLOW_SYSTEM_CHROME=0
export GROK_CU_PLAYWRIGHT_VERSION=1.64.0
export GROK_CU_BROWSER_VERSION=157.0.8101.0
export GROK_CU_TEST_WORKER="$GROK_CU_DIAGNOSTIC/worker/server.mjs"
"$node" --test --test-concurrency=1 \
  worker/typed-act-http.test.mjs worker/observation-http.test.mjs \
  worker/observation-mutation-http.test.mjs worker/worker-error-http.test.mjs \
  worker/cleanup-http.test.mjs

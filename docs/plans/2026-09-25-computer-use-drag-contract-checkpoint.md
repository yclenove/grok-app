# Computer Use — explicit drag destinations and managed input cleanup

Date: 2026-09-25, local +08:00. Branch: `feat/computer-use-implementation`.
Base HEAD: `30757366a739ec9aaf0ccc95bbb3efe19a067aa9` plus the preserved dirty worktree.
Full goal remains **active / not release-ready**. No commit, push or PR.

## Reproduced failures and implementation

- The Broker checked `toX/toY` before `x1/y1`, while Windows dispatched the
  opposite pair. Conflicting aliases could check one point and execute another.
  Rust and Node now require one complete numeric pair, reject mixed aliases,
  coercion and missing coordinates. Windows and X11 use the same Rust parser;
  managed `computer_act` normalizes the legacy pair to `toX/toY` before dispatch.
- Windows no longer guesses a destination from the first UIA name containing
  `drop`. `computer_act` drag has an explicit pixel destination and requires the
  current visual observation even when its source is a semantic reference.
  A shared 16-case golden matrix covers both element and coordinate sources.
- Managed `browser_act` preserves its separate two-opaque-reference route without
  requiring an image. A pixel endpoint instead requires the delivered image,
  finite nonnegative CSS/image coordinates, exclusive image bounds and unchanged
  viewport size. Destination references and pixels cannot be mixed.
- The worker previously accepted pixel coordinates, left its destination handle
  null and failed at dispatch. It now performs bounded pointer movement against
  the original Page and retained element handles, with document, cancellation,
  target-liveness and viewport checks. No selectors, active-page lookup or model
  JavaScript is added. Mutation admission still consumes the model observation.
- After a potentially delivered down, cancellation/identity loss/transport
  uncertainty closes only the owned managed page instead of sending an up to a
  replacement document or committing a cancelled drop. No action is retried.
  If closure cannot be confirmed, the original admitted request retains run
  occupancy until its original BrowserContext actually closes. Fresh observe
  cannot clear this input quarantine; Stop/close can still finish cleanup.
- New worker siblings are included in the explicit runtime preparation list.
  Windows runtime was rematerialized with unchanged upstream binary locks and
  Chromium hash, rather than running with mismatched bundled/source workers.

## Evidence collected so far

Artifacts are under `tools/computer-use-probe/.run/linux-native-20260925/`.
That directory name is historical; Windows results below are not Linux claims.

- `drag-contract-core-red-corrected.log`: **1 pass / 3 failures**. Corrected
  fixtures advertise the production semantic-drag source capability; they do
  not mistake an earlier unsupported-capability rejection for the intended gate.
- `drag-contract-core-green.log`: **4/4** targeted Core tests.
- `drag-contract-node-green.log`: **37/37** golden/protocol tests.
- `managed-drag-red.log`: **1 pass / 5 failures**, before worker changes.
- `managed-drag-unit-green.log`: **51/51**, including opaque-ref behavior,
  coordinate dispatch, cancellation, document/handle/viewport retirement, lost
  down/up replies, failed cleanup, and admission retention at full capacity.
- `drag-node-selected.log`: **128/128**, using pinned Node 20.18.0. Selected
  browser unit and MCP suites; this is not every native/browser test.
- `drag-core-full.log`: first full run **503 pass / 1 failure**, because the
  bundled worker was stale. Error was `hash_mismatch`, not the test's intended
  removed-sibling error. No integrity check was loosened.
- `drag-core-full-prepared.log`: after preparing the matching runtime,
  **504/504**, no ignored tests, terminal exit 0 (102.38 seconds of tests).
- `drag-app-build.log`: Windows App probe-feature harness builds, exit 0.
  Post-link `mt.exe` embeds the existing test manifest before launch.
- `drag-app-tests.log`: **92 pass / 1 existing explicit ignore**, exit 0.
  The ignored native fixture is not counted as a pass or installed-App evidence.

## Real browser evidence and retained failures

The HTTP tests run the prepared production worker with pinned Node and an
isolated writable copy of pinned Chromium. The independent local fixture counts
drops; the action response itself is not used as proof of its effect.

- `managed-drag-http-first.log`: source worker pixel test passes; the semantic
  test fails during cleanup with a Windows `EPERM` temporary-directory error.
- `managed-drag-seed-http-full.log`: **9 pass / 4 failures** counting nested
  subtests. Both drag cases pass. The wait matrix has `/close` timeout and a
  subsequent `/goto` 500; another test has `EBUSY` cleanup. Do not call it green.
- `managed-drag-seed-focused-1.log` and `-2.log`: both selected drag cases pass
  in each round; five unrelated cases are explicitly filtered out.
- `managed-drag-seed-focused-3.log`: **1 pass / 1 hook failure** plus five
  filtered cases. Pixel test cleanup fails with `EBUSY`. This does not satisfy
  three consecutive complete acceptance passes.
- Cleanup now preserves shutdown/forced-exit/live-process failures instead of
  masking them with a later directory-removal exception. It retains evidence
  and does not delete an owned profile while a tracked process remains alive.

Windows prepared seed fingerprint (`drag-runtime-prepare.log`):

```text
manifest 709ec67b3bb9a12c435fd5cd012099981e864a750f0b96c08bf1834337a83be7
tree     4b0d3790f0873cfd72b4387f2ec0293e26b875136962951b60ea79c47ba3efee
chromium 63c6075faf6d986bc8e9f11996f5d2e784e7f4d9471366b7a2ad6eebaca245e3
importProbe=passed
```

## Still required

Native Linux verification and both current Windows strict checks have completed;
confirmed terminal outcomes are recorded below and in the following addendum.
Browser close/cleanup races need diagnosis, not larger timeouts or hidden skips.
Native cancellation/transport-loss browser acceptance is not replaced by mocks.

The full product still requires installed redesigned-UI acceptance, complete
macOS and GNOME native Wayland adapters, WebView immediate Stop/recovery and
other-platform isolation, signed multi-platform installation/update/rollback,
real Grok E4 and a frozen 12-hour active soak. The original platform, surface and
App/ACP/MCP scope is unchanged. Do not mark the overall goal complete from this batch.

## Linux native terminal results

`drag-linux-full.log` has terminal exit 0. It uses the existing private Debian
WSL toolchain and target directory, with no host toolchain/proxy reconfiguration.

- X11 unit tests: **6/6**.
- Core + X11 all-targets, native-probe strict Clippy: exit 0.
- Production X11 adapter rebuilt and run in owned Xvfb: **16 checks in each of
  three consecutive passes**, `drag-native-1.log` through `-3.log`. These include
  exact native drag down/path/up, invalid-destination zero effects, ownership,
  focus/occlusion, cancellation, parent lifecycle and deliberate XID reuse.
- Linux seed rebuilt with the same source worker and pinned Linux binaries,
  `importProbe=passed`; independent Windows seed is not overwritten.
- Core on the freshly prepared Linux seed: **504/504**, no ignored tests,
  124.41 seconds. This is native Linux evidence, not macOS, native GNOME Wayland
  or installed-App/real-model acceptance.

```text
Linux manifest 9f70b760342447e2c078401c452ead918b33f21bdde14e8131c2c7f33a19fe77
Linux tree     6ce93832ae8355482628ac01d91cc00a06e391432d02dabeb8d1c751f122208a
Linux chromium b203864ac3ee28fde712f33d13b90e7c67682f22207e36ff253b8b34ee2827bc
```

## Windows terminal addendum and next repair batch

- `drag-app-probe-clippy.log`: App all-targets with the probe feature,
  `-D warnings`, exit 0 (37.31s).
- `drag-app-production-clippy.log`: normal App library with `-D warnings`,
  exit 0 (21.62s).
- `managed-drag-seed-http-cleanup-diagnostic.log`: **12 pass / 1 hook failure**.
  Shutdown returned 200, the worker exited without forced termination and
  tracked PIDs were gone before the remaining `EBUSY` directory-removal failure.
  This does not establish that every descendant had exited or explain the lock.

The follow-up [browser identity and cleanup checkpoint](2026-09-25-computer-use-browser-identity-cleanup-checkpoint.md)
records new evidence and repairs. Its worker fingerprint supersedes this batch's
fingerprint. Earlier failures remain preserved; none are reclassified as passes.

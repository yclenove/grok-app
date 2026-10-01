# Managed browser: navigation deadlines and retained native failures

Local workstation date: September 26, 2026 (+08:00). Branch remains
`feat/computer-use-implementation`, HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`.
Full Computer Use goal remains active. No commit, push, PR or goal transition.

## Product repair

Real Playwright navigation timeouts previously became a generic HTTP 500
`worker_internal`. A deliberately incomplete local HTML response reproduces
that behavior in `navigation-timeout-native-red.log`: expected 504, actual 500;
the owned worker's final shutdown was confirmed rather than force-terminated.

The new `navigation.mjs` gives `/goto` and typed `reload` the same native
eight-second / DOMContentLoaded deadline. Goto's deadline is unchanged; reload
no longer relies on the library's different default timeout/load condition.
Only the pinned Playwright library's actual TimeoutError is classified as
`504 / navigation_timeout / completion: unknown`. Names, arbitrary properties
and raw exception messages cannot forge a safe rejection or leak page details.
Already-cancelled calls are not dispatched; cancellation after dispatch remains
unknown. There is no response-only timer race, input replay or added fallback.

The existing action ledger and quarantine still apply. Native tests verify that
the same timed-out action cannot produce another navigation request. A timeout
does not prove that the document never changed or that its networking stopped.
This repair is not physical navigation cancellation or a Chromium exit fix.

`runtime_prepare.rs` includes the new module in the production file allowlist.
The private fault-injection preload and all fixtures remain outside that list.

## Verified evidence

All logs are under `tools/computer-use-probe/.run/linux-native-20260925/` despite
this batch running on Windows with isolated, disposable browser profiles.

- `navigation-timeout-unit.log`: 24/24 focused tests, including the new six
  navigation cases, existing typed actions and cleanup reconciliation.
- `navigation-timeout-native-green.log`: current source worker with pinned Node
  and Chromium; two selected cases pass (stalled goto and real pixel drag),
  eight unrelated top-level cases explicitly skipped. This is not a full suite.
- `navigation-worker-unit.log` and final `navigation-worker-unit-final.log`:
  full selected worker unit set 125/125, exit 0. The final run also verifies
  that typed reload dispatch actually forwards the eight-second options.
- `navigation-runtime-prepare.log` and `navigation-runtime-check.log`: independent
  generated runtime prepares/checks successfully, including actual module import.
- `navigation-runtime-actions.log`: generated Node + worker + Chromium, full
  then-current action suite 16/16, exit 0, 54.16 seconds.
- `navigation-runtime-actions-final.log`: final expanded whole action suite
  **18/18**, no skipped cases, exit 0, 73.15 seconds. It includes separate
  real stalled goto and reload, opaque-ref/pixel drag, upload, independent
  browser identities, six wait races and pause/resume. Every fixture confirms
  HTTP 200 shutdown without forced worker exit. It is not an installed-App,
  real-model or cross-platform acceptance run.
- `navigation-runtime-core-tests.log`: `cargo test -p grok-computer-use-core
  --offline --target-dir target-cu-review runtime_prepare:: -- --test-threads=1`
  passes 26/26 (491 filtered out), exit 0. Includes target archive layouts,
  import deadlines/reaping/sanitization and transactional prepare/check tests;
  this is not the full Core suite or native tests on all target OSes.
- `navigation-runtime-post-native-check.log`: the generated pack still passes
  integrity and actual import checks after native execution. Changed Node
  sources pass syntax checks; scoped diff/whitespace checks pass.

The generated module bytes match source. Runtime location:
`.cu-probe-navigation-deadline-20260926/seed` (ignored, not a release artifact).

```text
manifest    d78fa95592c94d1a0ac51cec66621d4252e438ebe4820cc859b1741c0af6720a
tree        74cf5030597de7cebf7adaede97578f5dcefbd80317658665f9ada21b8323b84
chromium    63c6075faf6d986bc8e9f11996f5d2e784e7f4d9471366b7a2ad6eebaca245e3
worker      cc8f820f11b478ec633ff5f15b70d19d3ecc8ed370fc4e282bf1cdab6600685f
navigation  ea8727a9db316674aca238375b5f6aa349b4ce17d5af1af52a31cad9b5c4069a
```

## Failures retained, not explained away

- Resumed session 38371 actually terminates with exit 1: the fully pinned
  `cleanup-deadline-pack-actions-r2.log` passes only 5/15 in 358.19 seconds.
  Failures include cleanup hooks requiring forced worker exit and navigation
  errors before action assertions. The earlier reconciliation helper alone
  therefore did not fix native reliability.
- `cleanup-deadline-pixel-diagnostic.log`: isolated pixel case still fails.
  Goto starts at 21:10:06.953Z and fails at 21:10:14.964Z. Physical context close
  starts at 21:10:14.975Z; browser process exit is 21:10:37.275Z, about 22.3 seconds
  later. These are September 25 UTC / September 26 local timestamps.
- With only extra test-side request/confirmation diagnostics, the same case
  subsequently passes in `cleanup-deadline-pixel-traffic.log` and three serial
  repetitions `cleanup-deadline-pixel-traffic-r1.log` through `-r3.log`.
  Their original close calls still vary from about 1.3 to 12 seconds. No product
  proxy, timeout or Chrome setting changed between these baseline repetitions.
  Passing repeats are not evidence that the intermittent native stall is fixed.
- Earlier 15–28-second exits and two directory-publication Access Denied failures
  remain open in the [cleanup checkpoint](2026-09-26-computer-use-cleanup-deadline-checkpoint.md).

The review App at PID 75508 was revalidated by executable path and creation time
and left untouched. Its original resource seed manifest remains
`df73022de18fdeebf61b8400b917c266d01fd19a26e01aef7b9d6e886cb06d37`.
No system proxy, ordinary browser, user account or unrelated process was changed.

## Remaining scope

Retain the full release gates: native browser exit/publication reliability;
all platform adapters and WebView physical cancellation/isolation; signed
runtime install/update/rollback; redesigned UI native interactions, overlays
and scaling; real Grok E4; and frozen 12-hour active soak. Existing R12 UI
evidence is unchanged and does not prove installed-App acceptance. This batch
proves neither the final product nor all-platform availability.

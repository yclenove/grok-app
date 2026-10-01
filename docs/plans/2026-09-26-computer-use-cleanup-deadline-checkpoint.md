# Managed browser: bounded cleanup replies and physical exit evidence

Local workstation date: September 26, 2026 (+08:00). Full Computer Use release
goal remains **active**. No commit, push, PR, or changes to the running review App.

Follow-up: [navigation deadline repair and retained failures](2026-09-26-computer-use-navigation-deadline-checkpoint.md).
The resumed fully pinned action run failed 10/15; an isolated diagnostic confirms
about 22.3 seconds waiting for browser process exit. Later successful repeats
do not erase that failure. Navigation timeout classification is repaired
separately; native exit latency and earlier publication errors remain open.

## Product change

`tools/computer-use-browser/context-close.mjs` now supplies `waitForCleanup`.
It bounds the response observer, not the underlying owned cleanup. `/close`
and `/cancel-run` have one five-second budget for their entire operation;
`/shutdown` uses a 1.5-second response-observation budget, inside the supervisor's
two-second graceful-response budget. These are event-loop timers, not an OS-level
hard real-time exit guarantee. Expiry is a typed `409 / run_cleanup_pending /
completion: unknown`, not success or `not_started`.

The original physical close promise keeps running. Its resource lease remains
held until both the native context close event and that original promise have
settled. Expiry does not redispatch close, release ownership, enable Resume,
replay an action, or kill a process. A late rejection remains observed.
Successfully closed slots are forgotten only after their actual close finishes.
Cancel failures now carry a typed unknown-completion envelope instead of the
legacy default-not-started error.

After an expired shutdown response, the worker keeps HTTP available. A later
explicit lifecycle reconciliation may confirm completion and exit the worker;
late physical completion alone does not close the server behind the caller's
back. The existing Host's cleanup-pending state and retry path remain in force.
No new model-accessible hook, setting, or permission is added.

## Native evidence, and what it does not prove

`cleanup-http.test.mjs` uses the production source worker and real pinned
Chromium. A test-only preload, reachable solely through the test child's private
IPC channel, delays the original Playwright close call. It is not in the runtime
pack allowlist. Tests cover all three HTTP routes, concurrent cancel observers,
typed pending replies, retained occupancy and profile ownership, exact-once
native close, eventual idle state and a confirmed worker exit. The running user
App and ordinary browsers were not used.

The final run passes all three route scenarios in one test (48.41 seconds).
This proves **pending/reconciliation correctness**, not fast physical shutdown.
The 30-second late-reconciliation observation budget belongs to this test only;
no product transport or graceful-exit deadline was increased.

Sanitized phase timings pinpoint a real remaining delay:

- Run R3: graceful close begins at 7,684ms; context close event at 7,704ms;
  browser process exit at 23,293ms; original promise settles at 23,295ms.
- Run R4: close begins at 8,029ms; event at 8,517ms; process exit at 36,924ms;
  original promise settles at 36,927ms. The other two closes finish much faster.

Thus the slow interval is **after context close but before browser process
exit**, not a lost HTTP reply alone. Chromium's reason for that variable exit
delay is still unresolved. The pinned Playwright implementation waits for the
owned process exit and cleanup in its original close promise; neither a close
event nor a second close call is sufficient evidence to free a run.

## Verification and retained failures

Logs are under `tools/computer-use-probe/.run/linux-native-20260925/`; despite
that historical directory name, these executions ran on Windows.

- `cleanup-deadline-red.log`: initial new helper contract could not load because
  the export did not exist yet; this is **not** a behavioral assertion failure.
- `cleanup-deadline-unit.log`: 18/18 ContextClose + RunOperations tests pass
  using the packaged Node 20.18.0 executable.
- `cleanup-deadline-http.log`: test startup fails because Windows absolute
  `--import` paths need a file URL; fixed in the harness with `pathToFileURL`.
- `cleanup-deadline-http-r2.log`: the harness incorrectly reconciled after the
  close event, before the original promise settled. The product correctly
  stayed pending. The harness now observes original promise settlement.
- `cleanup-deadline-http-r3.log`: physical exit exceeded the initial ten-second
  test wait. Preserved as evidence of slow native exit, not erased by R4.
- R2/R3 retained their disposable profiles. After their native browsers had
  exited, the stuck test Node processes were terminated through held Windows
  handles checked against executable and creation time. No user App, proxy,
  ordinary browser, or unrelated build was stopped.
- `cleanup-deadline-http-r4.log`: terminal exit 0; all three pending-to-confirmed
  route scenarios pass. No forced termination was needed in this run.
- `cleanup-deadline-worker-unit.log`: 112/114; two Windows process-discovery
  tests failed. `cleanup-deadline-discovery-r2.log` passes those files 10/10;
  a fully serial rerun, `cleanup-deadline-worker-unit-r2.log`, passes 114/114.
  The original failures remain recorded; their nondeterminism is not resolved.
- Node syntax checks pass for the changed product, test and preload files.

### Ordinary action regression follow-up

`cleanup-deadline-pack-actions.log` records 13/15: one cleanup hook assumed the
first shutdown reply must be final, and a separate pixel-drag fixture failed
at navigation before its drag assertions. This command selected the generated
Node and worker, but supplied `GROK_CU_CHROME` rather than that legacy harness's
`GROK_CU_TEST_CHROME`; do not call it a fully pinned-Chromium run. Its browser
still used a fresh disposable profile, not an ordinary user's session.

The fixture cleanup now recognizes **only** the explicit typed pending response,
polls read-only inventory and requires a final confirmed shutdown response.
The aggregate cleanup HTTP budget remains the original 20 seconds. Transport,
other HTTP errors or exhausted observation still fail; no input/action is
retried. The new test-only helper passes 5/5 in
`cleanup-deadline-reconcile-unit.log`. A subsequent action regression explicitly
sets the legacy harness's test-Chrome selector to the generated pinned binary.

## Packaging status

The active App's resource seed is deliberately not rewritten. Preparing a new
isolated pack at `tools/computer-use-probe/.run/cleanup-deadline-20260926/seed`
failed twice at `publish seed` with Windows error 5 (access denied). Both logs
are retained as `cleanup-deadline-prepare.log` and `cleanup-deadline-prepare-r2.log`.
A shorter isolated output at `.cu-probe-cleanup-deadline-20260926/seed` subsequently
prepares and checks successfully (`cleanup-deadline-prepare-short.log` and
`cleanup-deadline-pack-check.log`), including the real pinned module import.
That single alternate-path success does not establish why the two earlier
directory publications failed.

The generated worker and ContextClose bytes match the current product sources.
Pack fingerprints:

```text
manifest  8a2d16030cf55ee01431728f6e0433fbacd22a851601cfc00c3f4cc701257d0f
tree      d78bacf725918b3a0e8c082bcfabd36555ce5eb75b9d09fd68015a1dbd708a82
chromium  63c6075faf6d986bc8e9f11996f5d2e784e7f4d9471366b7a2ad6eebaca245e3
worker    d4669a98b87a8503b285b4edc256fe9b7f9b7a87d28873b6e5ee0503673146f5
closer    7c3ec45dbefcd1880131769ffa23f53dd01368634a5573163591d74fadaddcf2
```

The preload now resolves Playwright from the explicitly selected worker's own
dependency tree. `cleanup-deadline-pack-http.log` runs the new pack's Node,
worker and Chromium: 3/3 tests pass, covering the three cleanup/reconciliation
routes plus aborted-body safety and HTTP error envelopes (19.71 seconds).
Native exit after the injected gates is fast in this particular run; it does
not invalidate R3/R4's retained long-tail measurements. The harness's aggregate
150-second test limit covers three separate late-reconciliation scenarios; it
is not a product Stop deadline or a fast-exit acceptance criterion.

This is generated-runtime worker acceptance, not installed App, real-model,
signed-bundle or cross-platform acceptance. The running review App PID 75508
and the original resource seed remain untouched. Its original manifest, worker
and closer hashes were re-read unchanged after the isolated pack test:

```text
manifest  df73022de18fdeebf61b8400b917c266d01fd19a26e01aef7b9d6e886cb06d37
worker    0273a123fb9cd4332f8da5e34f9695af3f6d0cb90f1899ac82336706d22b33b2
closer    8cfbd244a901b156eb7f6cb0958e963bd775f03fd3f42255e66dbe3afc5e773c
```

## Remaining release work

Keep the full existing scope: investigate physical Chromium exit delay and
runtime directory publication, then validate native packaged cleanup; finish
native adapters and WebView cancellation/isolation, all-platform signed runtime
install/update/rollback, the redesigned UI's native interaction/scale/overlay
gates, real Grok E4 and the final frozen 12-hour active soak. Earlier R12 UI
evidence remains valid within its stated scope, not a replacement for those
gates. This batch is not a final release claim.

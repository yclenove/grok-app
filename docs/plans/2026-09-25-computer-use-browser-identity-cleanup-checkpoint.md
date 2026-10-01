# Computer Use — exact managed-browser process identity and cleanup

Date: 2026-09-25, local +08:00. Branch: `feat/computer-use-implementation`.
Base HEAD: `30757366a739ec9aaf0ccc95bbb3efe19a067aa9` plus the preserved dirty worktree.
Full goal remains **active / not release-ready**. No commit, push or PR.

## Product changes

The previous PID lookup used `context.browser()?.process?.()` and then the first
child of the worker. Two live profiles could report the same browser; even an
unrelated owned Node child could be mistaken for the browser. The new regression
first reproduced both problems (`browser-pid-exact-red.log`: 2 pass / 2 fail).

- A context with a Browser object uses its own private CDP session. Exactly one
  valid browser-process row is required. A missing/ambiguous result stays unknown;
  there is no first-child fallback. The query has a bounded diagnostic budget,
  and a late session still detaches.
- The pinned Playwright 1.48 persistent context has **no Browser object**. An
  initial implementation overlooked this: `browser-pid-native-http-1.log` failed
  the new native case (0 PIDs instead of 2), despite mocked unit tests passing.
  A separate native diagnostic confirmed both the null Browser and the rejection
  of `SystemInfo.getProcessInfo` on the page target. This failure is retained.
- The persistent path now matches the worker's actual kernel child, exact binary
  path and unique `--user-data-dir` argument. Windows command-line quoting is
  parsed without whitespace guessing; Linux reads `/proc` argv and executable.
  Duplicate profile arguments, renderer processes, wrong parents, ambiguous
  matches and missing metadata are rejected. Windows queries are asynchronous,
  so process discovery no longer blocks the worker's Stop event loop.
- The owned slot is registered before awaiting identity discovery, so Stop can
  still find a context while diagnostics are pending. `/pids` uses the captured
  per-context result and does not silently borrow a later child's PID.
- The helper is included in the explicit prepared runtime file list.

These are PID diagnostics, not a proof against PID reuse. The initial identity
batch did not fix the Rust supervisor's numeric-PID cleanup fallback; the separate
Windows lifetime-ownership correction and its evidence are recorded below. Persistent-context
PID discovery on macOS deliberately remains unknown until native argv/lifetime
identity is implemented and tested. The full macOS scope is still required.

## Windows native evidence and retained failures

Artifacts: `tools/computer-use-probe/.run/linux-native-20260925/` (the historical
directory name does not make these Windows runs Linux evidence).

- `browser-pid-kernel-unit-final.log`: **8/8**, pinned Node 20.18.0. Tests include
  two distinct contexts, a real decoy child, malformed identities, late sessions,
  Windows quoting and Linux/Windows exact-match selection. Linux here is a pure
  matcher fixture, not yet a native Linux process-discovery run.
- `browser-pid-native-http-2.log`: **12 pass / 2 failures**. The new identity
  assertion incorrectly equated *all* worker children with browser roots;
  a separate helper/child can also exist. It now checks that each distinct
  reported root belongs to the worker, then proves that closing the first
  actually removes its process while the second remains usable.
- `browser-pid-final-native-1.log`, `-2.log`, `-3.log`: respectively **13/1,
  13/1, 11/3 pass/fail**. The multi-profile identity/independent-close case
  passes in all three, and functional assertions pass. Remaining failures
  are real `EBUSY` cleanup hooks. These are not three complete passing runs.
- Read-only Restart Manager diagnostics in `owned-lock-diagnostic-1.log` report
  file users after the busy removal, while a later scoped process query is empty.
  This is not proof that antivirus or any specific external program caused it.
  No other process was killed and no security setting was changed.

## Cleanup correction and terminal native evidence

Inspection of the actual pinned Node binary's `internal/fs/rimraf` source shows:
its synchronous initial `rmdir` handles ENOTEMPTY/EEXIST/EPERM, but an initial
EBUSY escapes without entering the configured retry loop. That explains why
`rmSync(..., maxRetries: 5, retryDelay: 100)` did not apply its intended policy
to the observed stack location.

The typed-action harness now uses awaited `fs.promises.rm` with **the same**
five-retry, 100ms policy. Process exit, non-forced worker exit and successful
shutdown assertions still run first; removal failure is still a test failure.
It does not extend action timeouts, replay actions, hide failures or delete an
ordinary user profile. The three new native runs are recorded below. The earlier
`/close` timeout and `/goto` failure are retained
as unresolved historical races, not declared fixed by directory deletion.

Final identity/cleanup baseline (before the separate close-ownership follow-up):

- `browser-pid-async-cleanup-1.log`: 13 pass / 1 missing-PID failure; no directory
  cleanup failure. The reason for the initially unavailable identity is not
  established. Exact per-context diagnostic reads now share an in-flight lookup,
  cache success, and permit another read after unknown, never after retirement.
- `browser-pid-recovery-native-1.log`, `-2.log`, `-3.log`: **three consecutive
  Windows native passes, 14/14 each**, 0 skipped; 38.993s, 43.039s, 39.501s.
- `browser-pid-linux-native-final.log`: terminal exit 0, actual Debian WSL;
  **10/10 identity tests**, including real Unicode/spaced argv, followed by a
  rebuilt Linux seed (`importProbe=passed`) and **14/14 native browser tests**,
  0 skipped, 11.378s. This is not a GNOME Wayland or installed-App result.
- The initial native Linux identity run was **7 pass / 1 fail** because `ps`
  counted its own child. Discovery now excludes the exact query PID, not a
  process-name heuristic. This failure is retained in `browser-pid-linux-unit-first.log`.

Baseline seed fingerprints (superseded when close-ownership is prepared):

```text
Windows manifest 028fcb13557786af7c739fa1603a1892510f30218565a2e3036f6056eb30ef8d
Windows tree     e0b4052a23650b9bd5c4c98353ca37310a42f1d4f5651ee1d3f49b7a5132978b
Linux manifest   0abb912023cd71951778a1091ea0ee2c12260b18fd722e9ca72a6514f5fa4130
Linux tree       b50fc47070923faf5ecbf5db711d02ec3f8a8da816d0994aed411ad2a651faa3
```

## Close ownership and truthful Stop readback

The pinned Playwright source has an early return for the second `context.close()`.
All routes for an exact owned context now share one physical close attempt,
reject new work while closing, and retain failed/unconfirmed closure. Actual
late closure may resolve uncertainty; a no-op second dispatch cannot do so.
Seven deterministic close-race tests now also cover cleanup occupancy. An actual
context close event and settlement of the first close promise are both required
before its cleanup lease is released. Failed/unconfirmed closure retains that
lease. Stop/readback, wait-idle and resume therefore cannot treat aborted requests
as proof that browser cleanup finished. Resource leases are deduplicated by the
already-owned context object, independent of ordinary request capacity.

The Rust supervisor follow-up removes numeric-PID termination authority, waits
for its exact Windows Job to become empty, and serializes simultaneous shutdowns.
Kernel-backed tests cover an unrelated owned decoy, concurrent completion, and
failure recovery. The Windows Job is the lifetime authority: diagnostic PIDs are
never reopened for termination. Shutdown returns success only after the Job is
empty and its owned child is reaped. Unix forced teardown without confirmed
browser cleanup remains an explicit gap.

### Terminal results and retained failures

- `context-close-idle-unit.log`: **14/14** (seven context-close, five run-operation
  and two input-cleanup tests). This covers stop readback at full ordinary capacity.
- `owned-shutdown-kernel-tests.log`: initially **1 pass / 2 failures** because the
  fixture incorrectly assumed exactly two Job members. The kernel reported four.
  The corrected test queries actual Job membership, requires the known root and
  descendant, excludes the independently owned decoy, and requires zero members
  after shutdown. `owned-shutdown-kernel-tests-2.log`: **3/3**, 0.69s. No claim is
  made about why the additional members existed.
- `owned-shutdown-app-regression.log`: **95 passed / 0 failed / 1 existing ignored**,
  65.39s. This includes the kernel tests. It precedes the final cfg-only diagnostic
  gating and the JavaScript resource-lease change; it is not an exact final-source rerun.
- `context-close-core-windows.log`: **504/504**, 31.29s. Core Rust is unchanged
  after this run; subsequent worker-body changes have separate native coverage.
- The first production Clippy run failed on nine dead-code diagnostics. Diagnostic
  helpers/fields are now correctly probe/test-gated, not warning-suppressed.
  `owned-shutdown-production-clippy-2.log` and `owned-shutdown-app-clippy-2.log`
  both exit 0 with `-D warnings` (17.02s / 15.60s).
- `context-close-native-windows.log`: **14 passed / 1 failed cleanup hook**. The
  final `/shutdown` reply exceeded the test's 20-second transport deadline. The
  worker subsequently exited without forced termination; the failure remains
  open. A later scoped process check found no matching browser; the failed
  owner-marked temporary profile was retained, not broadly deleted.
- Opt-in, isolated-fixture Playwright diagnostics yielded a focused pass and then
  `context-close-native-windows-debug.log`: **15/15**, 47.005s. No failing native
  trace was captured; the pinned Playwright 30-second graceful-close budget is
  not established as the cause of the prior failure.
- After the resource-lease change, diagnostics disabled:
  `context-close-final-native-windows.log`: **15/15**, 0 skipped, 75.417s.
- `context-close-linux-native.log`: **17/17** context/identity tests, rebuilt
  private Debian WSL runtime with `importProbe=passed`, then **15/15** native
  browser HTTP tests, 0 skipped, 12.124s. Not GNOME Wayland or installed-App evidence.
- The new native HTTP case concurrently closes one exact profile, confirms its
  original browser has exited at each response, checks diagnostics are empty,
  and reopens the same profile. These runs exercise the production Node worker;
  they do not themselves exercise the Rust supervisor.
- `context-close-seed-check.log`: final Windows seed check passes with real module
  import. `context-close-bundle-audit.log`: 443 files, zero links and no hygiene
  hits. This is not code signing or a complete installed-package acceptance.

Final prepared runtime fingerprints for this batch:

```text
Windows manifest df73022de18fdeebf61b8400b917c266d01fd19a26e01aef7b9d6e886cb06d37
Windows tree     e8c62a96b7819fccfe2d7e53b095b6e16035a71f0728efa8064fb56cd717834c
Windows chromium 63c6075faf6d986bc8e9f11996f5d2e784e7f4d9471366b7a2ad6eebaca245e3
Linux manifest   b70e57ae93f9b4d77af17cbcd5aa28945a710c5e6418e04aac68eaf84d10df56
Linux tree       7c407b5f0c16b7ef5dfbc8191e2025e200e0c04cd122c428b3d22df77e4ae7fe
Linux chromium   b203864ac3ee28fde712f33d13b90e7c67682f22207e36ff253b8b34ee2827bc
```

Both native seeds passed import. A fresh `cu_probe` build then completed and
`owned-shutdown-product-preview.log` exited 0 with `browser-packaged-preview`.
This exercises Rust `BrowserSupervisor` → generated pinned worker → Chromium,
including real Broker pause/resume/Stop, lost-reply fencing, independent action
effects, another owner's survival and physical shutdown. Four native Stop cases
reported 363/226/198/434ms cleanup. It uses an isolated App home and owned browser
profiles, not an installed App or real model. No source-worker fallback is allowed
for this mode. No later pass erases the preserved HTTP shutdown failure.

## UI/native boundary and unchanged final scope

The supported Computer Use skill is available again. The September 22 NSIS test
window and the other worktree's Grok process were not modified. A current isolated
native debug App was subsequently built with the September 25 frontend embedded;
actual WebView2 review found and repaired repeated runtime warnings. Its settings,
diagnostic disclosure, return-to-chat and Computer no-session entry were inspected
without enabling Computer Use or granting a target. See
[UX R10](2026-09-25-computer-use-ux-redesign.md#r10--current-native-ui-review-and-concise-runtime-recovery)
for exact fingerprints, 146 frontend tests, 42 browser cases and evidence limits.
This is current native debug rendering, not installed/signed control, overlay or
OS-scaling acceptance. Those gates remain open.

Remaining full requirements include redesigned UI native acceptance, complete
macOS/GNOME Wayland adapters, WebView immediate Stop/recovery and cross-platform
isolation, signed installation/update/rollback, real Grok E4 and a frozen 12-hour
active soak. Original Windows/macOS/Linux, surface and App/ACP/MCP scope is intact.

## Retained supervisor and accurate cleanup readback (late September 25)

The product supervisor slot now serializes startup/publication and shutdown.
Failed binding followed by failed shutdown retains the exact original child/Job
for later cleanup; a shutdown-requested instance cannot be reused or replaced.
The first red test is preserved in `retained-supervisor-red.log` (one failure).
`retained-supervisor-green.log` passes seven actual Windows supervisor tests;
`retained-supervisor-app-cu.log` passes 99 tests with one existing ignored test.
Both production and probe strict Clippy configurations passed. These artifacts
are under `tools/computer-use-probe/.run/linux-native-20260925/` despite the
directory name: these particular runs are Windows tests.

The latest `retained-supervisor-product-preview.log` is **FAILED**, not covered by
the earlier successful packaged preview. Real profile lifecycle, action oracles,
preview/model separation and pending-Wait pause/resume passed; cleanup of that
preview run then exceeded the 15-second `/cancel-run` request deadline. The later
native-download cases had not started. No matching owned processes were found
in the subsequent scoped check, but that does not explain the original timeout.
The separate older HTTP `/shutdown` timeout is also still open.

A separate cross-language contract error was reproduced and fixed: the worker
can legitimately report 64 ordinary requests plus one retained context cleanup,
but Rust rejected `activeOperations=65`. Rust now accepts exact JavaScript-safe
nonnegative counts and still requires `idle == (activeOperations == 0)`.
Ordinary admission capacity remains 64; physical cleanup is not released early.
`cleanup-count-core-red.log` has one failure/two passes; then the full Core
`cleanup-count-core-green.log` passes **506/506** (100.53s). The real production
RunOperations/ContextClose unit interaction passes **8/8** in
`cleanup-count-node.log`. This fixes the parser, not the native close timeout.

The managed acceptance probe now combines the original check, shutdown, fixture
and profile-removal errors instead of dropping cleanup errors behind `result?`.
It only removes its private temporary profile after the supervisor confirms
physical shutdown. It no longer waits an arbitrary 300ms then reopens a numeric
PID, which could belong to a replacement process. Four helper tests cover
retention and error preservation. `cleanup-retention-current-app-cu.log` passes
**103 tests / 0 failures / 1 existing ignored** (63.69s), after a post-link Windows
manifest and a fresh App harness build (49.66s). This run includes the Core count
fix and precedes only the subsequent probe-only Stop diagnostic lines.

`cleanup-retention-current-prod-clippy.log` and
`cleanup-retention-current-probe-clippy.log` both pass with `-D warnings`
(20.23s/22.13s), including those diagnostic lines. The preview probe now records
Stop elapsed time and, on failure after successful revision-3 recovery, performs
one read-only status query to the same worker. It logs only revision/phase/count/
idle or structured error code, never bearer tokens, cookies or page contents.
It does not replay Stop or an action, increase deadlines, or declare cleanup
complete from a logical stopped phase alone.

### Subsequent native revalidation — September 26, 2026 (+08:00)

The current probe (SHA256
`8171BCB08FFC08014A51C9673E6BDE60D275DF900AA42D4FBD6D3BA32ECEDC04`)
passed `browser-packaged-preview` in `cleanup-retention-native-diagnostic.log`,
then three consecutive serial repetitions in `cleanup-retention-repeat-1.log`,
`cleanup-retention-repeat-2.log` and `cleanup-retention-repeat-3.log`. Every
execution handle reached terminal exit 0. These use the generated seed worker
and isolated probe homes, not a source-worker fallback or an installed App.

Preview Stop took 222ms in the first run and 137/139/157ms in the repetitions.
The four pending native-operation Stop cases in each repetition took
211/132/797/903ms, 201/152/569/142ms and 255/146/164/131ms respectively.
Independent action effects, no replay, another owner's survival, lost/late resume
reply fencing and final owned shutdown all passed. No failure triggered the new
diagnostic readback; these passes do **not** identify or fix the retained earlier
15-second `/cancel-run` or 20-second `/shutdown` timeout. They remain open.

The full release goal is unchanged and remains active. No commit/push/PR.

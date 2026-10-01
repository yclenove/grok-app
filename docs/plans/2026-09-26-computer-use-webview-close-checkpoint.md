# Computer Use — normal WebView close recovery

Local date: September 26, 2026 (+08:00).
Branch: `feat/computer-use-implementation`, base HEAD
`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`. Existing dirty work is preserved.
Status: in progress. The queue-corrected candidate passes complete native
acceptance, but its fixed three-round close batch contains a 12.31-second
failure. The close-latency gate remains failed; no final-release claim is made.

## Reproduced failure

A normal controller close can discard native completion/event handlers without
a `ProcessFailed` event. The old script occupancy then has no live witness even
when its exact renderer eventually exits. Neither logical Stop nor callback
drop is evidence that execution has stopped.

The production `Registry::closed` path is covered by a new regression: an old
native view's pending action must settle after proven exit, without touching a
same-label replacement. Before the hook, this test times out. A fresh-profile
native fixture also reproduces a disconnected result channel with `idle=false`.
Its actual evaluation has one independently observed effect; only the result
callback is deliberately withheld. No model action is retried or replayed.

## Implementation

- Closing an exact native view permanently seals its `NativeWorld`. Generation
  increments alone cannot reopen it, and late context/witness publication fails.
- `Registry::closed` starts the existing exact-ticket/held-process-handle exit
  watcher before the native controller closes. The watcher does not depend on
  the STA or its discarded COM event handlers. A live/shared renderer remains
  occupied; only actual signaled-handle evidence settles running work.
- The result remains **outcome unknown**, not successful execution. Setup that
  never dispatched a script can be rejected without renderer-exit evidence.
- Three unit regressions cover missing failure events, a still-live/shared
  renderer, late callbacks/replacement isolation, and permanently closed worlds.
- `cu_probe webview-close` exercises normal close (not `Page.crash`), lost reply,
  exact old occupancy, fresh same-label binding, and a new exactly-once action.
  The full `webview` gate also invokes this case.

## Fixture lifetime investigation

The probe-only native Host did not balance `CoInitializeEx` and sent
`WM_DESTROY` instead of actually calling `DestroyWindow`. STA-local RAII now
destroys its owned HWND and releases COM in scope order, including early errors.
This correction yielded one 1,402ms passing close, but the very next repeat
failed. Therefore it is not recorded as the complete root-cause fix.

The diagnostic gate includes native shutdown in its original five-second
deadline. A further fifteen seconds of observation cannot turn a failed gate
green. One run returned from native shutdown in 66ms, but the exact renderer
handle was still unsignaled at twenty seconds. Ownership correctly stayed busy.

The next candidate retains the disposable environment, observes its exact
browser-exit event, and keeps the STA message pump alive during native teardown.
The initial candidate drops the core/controller/owned HWND before waiting. This is limited to new,
exclusively owned fixture profiles; ordinary/shared environments are not waited
on or terminated. The event does **not** settle scripts or authorize deletion.
This candidate now has native results; it does not yet meet the latency gate.
The first candidate's unit/lint validation passes. Subsequent fixture startup
changes have their own validation and must not inherit those results.

Primary reference actually retrieved: Microsoft's
`ICoreWebView2Controller::Close` documentation and sample at
https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller
explain handler release, shutdown only when no other WebViews use the browser,
and retaining the environment until `BrowserProcessExited` when observing
cleanup. `Close` alone is not used as proof that a held renderer has exited.

## Evidence preserved

Artifacts are in `tools/computer-use-probe/.run/ui-native-20260925/`.

| Artifact | Actual result |
| --- | --- |
| `webview-close-red-unit.log` | 0 passed / 1 failed; missing normal-close recovery |
| `webview-close-red-native.log` | exit 1; callback channel disconnected, still occupied |
| `webview-close-green-unit.log` | 3 passed; mocked physical witnesses, not native proof |
| `webview-close-green-native.log` | exit 1; witness retained but five-second timeout |
| `webview-close-sta-native.log` | exit 0; 1,402ms; old effect 1, new initial 0, new effect 1 |
| `webview-close-sta-repeat-1.log` | exit 1; first repeat times out; further rounds not run |
| `webview-close-deadline-diagnostic-native.log` | exit 1; shutdown 66ms, held renderer still live at 20s |
| `webview-close-environment-native.log` | exit 1; real exit and safe settlement at 15,676ms, beyond deadline |
| `webview-close-environment-repeat-1.log` | exit 1; real exit/settlement at 11,086ms |
| `webview-close-environment-repeat-2.log` | exit 0; 2,611ms; replacement action exactly once; replacement environment cleanup separately takes 9,097ms |
| `webview-close-environment-repeat-3.log` | exit 1; no environment event at 20s and no renderer exit/settlement by 35s |

These four environment-observer runs use probe SHA-256
`F3DEB0458E802317CDEC561ACF3D8FC5631AF6D17C9309D0635FDF1E3D16A783`.
Every individual exit code is retained; the mixed repeat batch fails overall.
The successful occupancy recovery is not evidence that complete environment
teardown is fast. In particular, the passing round's replacement cleanup still
has a nine-second tail. The observer neither expires ownership into idle nor
deletes profiles, terminates processes, or relaxes the five-second gate.

Validation of that environment-observer candidate is terminal:

- `webview-close-final-unit-build.log`: App harness builds in 37.49s. The required
  Windows test manifest is embedded before running the harness.
- `webview-close-final-unit.log`: **116 passed, 0 failed, 1 existing ignored**,
  86.33s, `computer_use::` filter with one test thread. The ignored test is still
  the manual native desktop fixture; no new test was skipped.
- `webview-close-final-clippy.log`: strict probe-feature/test Clippy passes,
  59.23s, `-D warnings`.
- `webview-close-final-default-clippy.log`: strict default-product Clippy passes,
  34.83s, `-D warnings`.
- Scoped rustfmt and `git diff --check` pass. These are code checks, not native
  timing evidence. The aggregate validation command exits **1** because the
  preserved native repeat failures are not overridden by green unit/lint runs.

The preceding 113-test results belong to the older process-exit batch; they are
not reused as current-source evidence.

## Full-run startup investigation

`webview-close-final-full-native.log` is terminal exit 1: it passes the first live
binding gate, then reports `webview bind host did not start`. It does not reach
the new normal-close gate. This failure is retained, not silently rerun as a pass.

Source inspection also finds that a startup receive timeout previously returned
without cancelling its native initialization thread. Environment/controller
creation used unbounded completion helpers, and an abandoned ready receiver did
not stop the later actor loop. The fixture now has one 25-second startup deadline,
named phases, cancellation fencing, and explicit bounded native callback waits.
Already-arrived but retired replies cannot publish a usable view. Initialization
failure also retires the startup token. The owned parent HWND remains alive while
observing disposable environment exit and is destroyed afterward on its STA.

The first startup candidate builds in 2m02s and its complete WebView run passes
(`webview-close-startup-build.log`, `webview-close-startup-full-native.log`, both
terminal exit 0). Probe SHA-256:
`DEB80F9E034AC4B5380DECC8E41CF3B5E77EF9B4132FEF56EEBBD7B936A7F93A`.
It covers typed Chinese input, twenty bind/unbind rounds, isolated DOM access,
exact element references, business timeout, actual renderer-side runaway-script
interruption, four setup cancellation phases (96/81/80/92ms), renderer crash
recovery (1,559ms), and normal close with same-label replacement (1,604ms).
The close's held renderer witness signals at 253ms; environment exit takes
longer. The replacement starts at zero and its new action has exactly one effect.
This run is not proof that the earlier intermittent startup/timing problem can
no longer occur, nor is it installed-App or real-model acceptance.

The following small follow-up extends the same startup cancellation/deadline to
navigation and viewport readiness. Independent fixture-script callback waits
are bounded too; timeout does not finish an already-dispatched operation.
The first follow-up passes 119 tests, but strict Clippy rejects two needless
generic-closure borrows in `readiness.rs`. Its validation command exits 1 and
does not run the remaining build/native stages. The raw
`webview-close-bounded-clippy.log` is preserved. The correction removes the
borrows, without suppressing the lint, and adds a fourth startup regression:
navigation retains its original 15-second sub-budget inside the overall
25-second startup limit. The overall deadline never extends a shorter stage.

The corrected candidate is terminal exit 0:

- `webview-close-validated-unit-build.log`: build 47.26s; manifest embedded.
- `webview-close-validated-unit.log`: **120 passed, 0 failed, 1 existing ignored**,
  72.57s. Four startup tests cover cancellation, expired/disconnected replies,
  successful current replies, and the shorter stage budget.
- `webview-close-validated-clippy.log` and
  `webview-close-validated-default-clippy.log`: both strict configurations pass,
  36.17s and 1.14s respectively.
- `webview-close-validated-probe-build.log`: build 1m10s. Its linker-output
  warning is retained; it is not a Rust error or suppressed Clippy warning.
- `webview-close-validated-full-native.log`: **complete WebView PASS**. Four
  setup cancellations are 96/85/72/103ms; renderer-exit recovery is 527ms;
  normal close including shutdown is **1,459ms**. Same-label replacement starts
  at zero, executes its new action once, and is unaffected by old completion.

This candidate's probe SHA-256 is
`55D279CB11300943B8EF66CCA9F2D0BB49FBBA7A0985499A2089FB03BDC4C26A`.
These results predate the queue correction below. They prove the bounded-startup
candidate's tests, not the later source, installed App, or native platforms other
than Windows. Previous timing failures remain recorded rather than overwritten.

## Pre-dispatch queue abandonment

Local `tauri-runtime-wry-2.11.4` source confirms a distinct gap: its
`Message::Webview` handler only invokes `WithWebview` when both the native window
and view still exist. A successful enqueue therefore does not guarantee that
the closure will ever run. Previously a dropped closure held a bare
`ScriptOperation`, whose intentionally non-settling Drop left a permanent
occupancy even though no native script had started. An abandoned probe actor
command queue had the same ownership shape.

`QueuedScript` now owns only the **pre-dispatch** phase. It is not clonable.
Dropping an uninvoked closure or an unconsumed command releases its exact ticket
and returns an explicit before-execution error. Just before native dispatch, an
irreversible handoff returns the ordinary `ScriptOperation`; lost callbacks,
business timeouts, cancellation and native execution still cannot release it.
The Windows product path, non-Windows outer dispatcher and owned probe command
queue use this same boundary. This is not completion of the unfinished native
isolated worlds on macOS/Linux.

Five regressions cover discarded closures, transferred/lost callbacks, cancelled
callers with dropped receivers, exactly-once rejection and replacement identity,
and receiver teardown with unconsumed commands. The candidate is verified:

- `webview-queued-unit-build.log`: build 59.67s, test manifest embedded.
- `webview-queued-regressions.log`: **5/5** queue-specific tests.
- `webview-queued-unit.log`: **125 passed, 0 failed, 1 existing ignored**, 69.92s.
- `webview-queued-clippy.log` and `webview-queued-default-clippy.log`: both strict
  configurations pass, 26.04s and 19.31s.
- `webview-queued-probe-build.log`: build 1m16s, existing linker-output warning.
- `webview-queued-full-native.log`: **complete WebView PASS**, terminal exit 0.
  Setup cancellation 110/79/84/110ms; renderer recovery 1,984ms; normal close
  1,077ms. Timeout/runaway/identity/DOM gates retain exactly-once and no-replay
  checks. Same-label replacement starts at zero and executes once.
- Scoped rustfmt and `git diff --check` pass. No frontend code changes are part
  of this batch; earlier UI tests are not relabelled as new native UI evidence.

Queue candidate probe SHA-256:
`5CC422D4E6F711F534A4B364D34B965A1C91D21A5EB6DD238EE3547DF0005435`.
No installed-App Tauri-close race is claimed from a unit queue simulation or the
owned Host probe.

### Fixed repeat batch remains red

Exactly three additional `webview-close` runs were scheduled before looking at
their results; no action was retried within a run:

| Artifact | Result |
| --- | --- |
| `webview-queued-close-repeat-1.log` | exit 1; normal close **12,306ms**, exact renderer exit confirmed, outcome remains unknown |
| `webview-queued-close-repeat-2.log` | exit 0; close **1,861ms**, replacement action once; replacement environment cleanup 2,262ms |
| `webview-queued-close-repeat-3.log` | exit 0; close **697ms**, replacement action once; replacement environment cleanup 1,038ms |

The repeat command is terminal **exit 1**. Round one's renderer actually stayed
unsignaled for about 12.24s; the environment-exit event arrived at 12.27s. This
is not merely late result-channel polling. Occupancy was released only after
physical exit, and no budget was enlarged, process killed, or profile removed.
Two fast repeats and one passing full run do not resolve the long tail.

### Checked owned-Host terminal outcome — September 26

The probe Host no longer discards the native thread's teardown result through an
already-consumed startup channel. `NativeThread` caches its actual terminal
`Result`, including a panic-to-unconfirmed-cleanup error. It holds its join lock
through physical thread completion: another concurrent/repeated caller cannot
mistake a taken `JoinHandle` for completed cleanup. Probe wrappers preserve the
primary failure and any cleanup failure. Unconsumed commands are dropped before
the potentially slow native teardown, so their undispatched tickets do not wait
on an unrelated browser-exit event.

This Host is compiled only for the Windows probe feature. These changes improve
the fidelity of native acceptance and do not, by themselves, implement installed
App environment cleanup or remove the production shared-renderer limitations.

New `webview-close-idle` and `webview-close-completed` controls use independent,
fresh profiles. They check an independently read effect counter (0 or exactly 1),
then include the actual thread/environment result in the same five-second close
gate. Idle means no CU isolated-world dispatch; fixture navigation/readback still
uses the real WebView. Profiles are retained and no workstation settings change.

Validated artifacts under the same `ui-native-20260925` directory:

- `webview-terminal-unit-build.log`: build 58.70s; test manifest embedded.
- `webview-terminal-regressions.log`: **5/5**, covering retained errors, panic,
  concurrent joins, cached success and both primary/cleanup failures.
- `webview-terminal-unit.log`: **130 passed, 0 failed, 1 existing ignored**,
  68.78s. This is the CU-filtered App harness, not all application tests.
- `webview-terminal-clippy.log` / `webview-terminal-default-clippy.log`: both
  strict configurations pass, 24.72s / 11.97s.
- `webview-terminal-probe-build.log`: 1m13s; existing linker-output warning.

Candidate probe SHA-256:
`02F9CAEF098FF086AFD1760019FEF2217639536C221BE2D9B840E5EB92F5BC16`.

The first native orchestration had an integer-key `OrderedDictionary` indexing
mistake: its supposed three-group schedule executed **two groups / six controls**.
Those six controls passed, but are not reported as a complete nine-run matrix.
`webview-terminal-results.json` preserves their actual order and count. The
subsequent complete WebView run **failed**, terminal aggregate exit 1:
`webview-terminal-full-native.log` reports the exact renderer still unsignaled
after about 20 seconds and `BrowserProcessExited` unobserved at its existing
20-second diagnostic limit. The newly checked teardown propagates
`owned browser exit timeout`; a returned thread is not relabelled as success.
Renderer-crash recovery earlier in that same run was 3,372ms and does not erase
the later normal-close failure.

The corrected fixed matrix uses string-labelled groups and enumerated values,
with explicit nine-result / three-per-mode checks. It is terminal exit 0, with
all **nine actual controls** recorded in `webview-terminal-balanced-results.json`:

| Control | Group A close | Group B close | Group C close |
| --- | ---: | ---: | ---: |
| No CU isolated-world action | 384ms | 917ms | 1,194ms |
| Acknowledged exactly-once CU action | 1,435ms | 575ms | 665ms |
| Lost action callback and normal close | 2,891ms | 1,268ms | 2,161ms |

Groups execute idle/completed/lost, completed/lost/idle, then lost/idle/completed.
The measurements are the close interval, not total fixture startup/replacement
wall time. Each lost-callback control also verifies a new same-label native view
starts at zero, performs its new action once and is unaffected by old completion.
All nine retain confirmed terminal cleanup. No budget was enlarged, process
killed, profile deleted or individual action retried to obtain green.

This matrix does **not** clear the immediately preceding complete-run failure.
Three controls per mode provide a comparison, not proof of stable tail latency
or a causal explanation. The evidence currently points to a real native lifetime
wait in the failed run, but does not determine whether the cause is application
release order, the WebView runtime, host environment activity or a combination.

### Next diagnostic boundary

1. Preserve the passed fixed matrix, failed complete run and incomplete first
   orchestration as separate evidence. Do not repeat until green or infer a
   stability guarantee from this small sample.
2. Collect phase-specific native lifetime evidence before changing release
   order again. An absent exit event plus an unsignaled renderer is not merely
   result-channel delay; neither a passing control nor a later fast run proves
   that the long-tail cause is resolved.
3. Do not modify workstation proxies, DNS, Windows services or security settings
   to obtain a green result, and do not attach to the user's running App.

Microsoft's process-event documentation was rechecked during this investigation:
https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-related-events
It distinguishes controller/process failure from complete browser-environment
exit. A separate upstream report, opened September 2, 2026,
https://github.com/MicrosoftEdge/WebView2Feedback/issues/5688
describes shutdown waiting on OS account/network activity. That is only a
diagnostic lead from a reported incident, **not** evidence that this machine has
that cause. No workaround from that issue was applied and no credentials,
authentication state, network configuration or Windows service was accessed.

## Remaining scope and boundaries

Normal-close timing and reliability remain open despite the candidate passes.
Shared-renderer exact-view completion, immediate running-JS Stop,
whole-environment/all-child/profile cleanup, watcher persistence, signed runtime
delivery, installed App/UI acceptance, all requested native platforms, actual
model execution and the frozen 12-hour active soak remain open under the full
goal. Existing failures in other checkpoints remain valid.

The running user App (PID 75508, original 02:53:53 local start) is not stopped,
replaced or controlled. Probe builds use `target-cu-review`; no App executable
is overwritten, profile is deleted, or commit/push/PR is performed.

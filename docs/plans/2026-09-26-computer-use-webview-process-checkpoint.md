# WebView renderer failure and exact native occupancy

Local date: 2026-09-26, Asia/Shanghai (UTC date: 2026-09-25).
Branch: `feat/computer-use-implementation`; base HEAD
`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`. This is an uncommitted worktree
checkpoint, not a release or a claim that Computer Use is complete.

## Implemented behavior (final acceptance still in progress)

- A native-world completion ledger retains the exact script ticket and reply even
  if WebView2 discards a completion callback. Admission is exclusive across
  adapter instances sharing that native world; old ticket cleanup cannot remove
  its replacement.
- `ProcessFailed` is scoped to the captured native view, not a reused Tauri label.
  Main-renderer failure fences the document generation. Browser failure retires
  the native view. GPU, subframe, unresponsive and unknown failures do not prove
  that this main-frame script has stopped.
- A delayed failure notification cannot alone settle the currently pending
  evaluation. Before dispatch, native main-frame/process associations identify
  the renderer and an owned Windows process handle is retained. Cleanup requires
  that exact handle to signal exit; a later PID lookup is not used. No-script
  setup can be rejected without pretending that running JavaScript was killed.
- A failure-only watcher handles the interval between a native failure event and
  the held process handle becoming signaled. It is exact-ticket scoped, uses no
  COM from its worker thread, stops after normal completion, and retains
  occupancy on missing/failed proof. Duplicate failure events start at most one
  watcher for that ticket. The ownership ledger is a separate module, not more
  state added to the App shell or one monolithic native dispatcher.
- Process identity lookup belongs to bounded, cancellable no-script setup. The
  setup timer is disarmed before evaluation. Existing business timeouts and the
  12-second native evaluation deadline retain their distinct meanings.
- A feature-gated test withholds an actual evaluation callback, crashes only a
  newly created disposable WebView2 profile, checks unknown outcome and occupancy,
  reloads explicitly, checks no replay, and establishes a fresh binding/context.
  No generic crash or arbitrary evaluation API is exposed to models.

## Retained failures and native evidence

Logs are under `tools/computer-use-probe/.run/ui-native-20260925/`.

The first process-ledger build completed successfully and its WebView unit filter
passed 56/56 (`webview-process-build.log`, `webview-process-unit.log`). That build
predates the held-process-handle guard and is not the final candidate.

The first two native probes crashed with `0xc0000005` before the deliberately
requested renderer crash. Windows fault records identify this probe executable
and `EmbeddedBrowserWebView.dll` 153.0.4234.48. LLDB analysis of the owned dumps
locates `ICoreWebView2FrameInfoCollectionIterator::GetCurrent` and the new renderer
identity traversal. The chained collection/iterator expression had dropped the
backing collection. Keeping the collection alive through iteration removes that
failure in the following native runs. Original evidence is retained:

- `webview-process-native-1.log` (terminal access violation, no assertions);
- `webview-process-native-diagnostic.log` (same crash, sanitized stage markers);
- `webview-process-crash-stack.log`, `webview-process-crash-stack-2.log`,
  `webview-process-crash-stack-symbols.log` (the second dump matches its binary).

The collection-lifetime fix then reached the deliberate renderer crash but failed
the cleanup assertion (`webview-process-native-lifetime-1.log`). Synchronous event
handling alone did not obtain signaled-handle proof. The failure-only witness
watcher subsequently passes the same path without expiring ownership by timeout.

The pre-refactor native candidate has SHA-256
`06532A261D861C4B31FDEE613356D4548F75C959B2586FDCFA11ABCB69CCEE73`:

| Artifact | Result | Crash dispatch → physical-exit acknowledgement |
| --- | --- | --- |
| `webview-process-native-witness-1.log` | pass, terminal exit 0 | 920 ms |
| `webview-process-native-witness-2.log` | pass, terminal exit 0 | 866 ms |
| `webview-process-native-witness-3.log` | pass, terminal exit 0 | 721 ms |
| `webview-process-native-full.log` | full WebView gate order passes, terminal exit 0 | 1,158 ms |

All four assert old effect 1, effect after explicit reload 0, fresh action effect
1, unknown old outcome, original binding retired, and late old completion/Stop
isolated. These timings include the deliberately induced renderer crash; they
are **not normal-operation Stop latency**. The full run also preserves the
10-second late action's exactly-once effect and 12-second runaway termination,
generic DOM/isolation/ref guards, Chinese input, and the existing binding and
setup-cancellation gates.

Strict Clippy first reported one simplifiable `map_or` expression
(`webview-process-clippy.log`). It is fixed without a lint suppression; the
subsequent probe/test Clippy passes (`webview-process-clippy-final.log`, 58s).
The subsequent App harness build completes in 1m22s. With the required Windows
manifest embedded, the `computer_use::` filter passes **113 tests, 0 failures,
1 existing ignored manual desktop fixture**, in 66.40s
(`webview-process-final-tests-build.log`, `webview-process-final-app-tests.log`).
Default-product strict Clippy also passes in 51.74s
(`webview-process-default-clippy.log`). These runs precede the next diagnostic
and process-identity cancellation-test additions.

The rebuilt probe's post-refactor full run **fails**, terminal exit 1
(`webview-process-final-probe-build.log`, `webview-process-native-final.log`).
The earlier gates pass, including the three setup-cancellation phases, but the
renderer-crash gate reports `generation_changed=true idle=false`: the document
is fenced while physical execution ownership remains pending after five seconds.
This is not permission to expire ownership into idle, replay the action, or
advertise renderer recovery as reliable. The earlier green runs are retained,
not substituted for this failure. A feature-gated diagnostic now records only
native failure kind and held-handle state, and preserves a failed gate even if a
reply arrives during additional diagnostic observation. No user App is stopped
or replaced; the full goal remains active.

### September 26 continuation: distinguish notification and process exit

`webview-process-exit-diagnostic.log` preserves another terminal exit-1 run.
The five-second observation deadline expires before document-generation change;
the native failure notification and a signaled held handle then arrive at
6,199ms. The gate stays failed. This proves a delayed native notification in
that run, not a lost notification or a fixed latency problem. The initial PID
diagnostic lacked query access and printed zero; PID logging is now taken from
the original native association, without requesting extra process rights or
using that diagnostic PID to authorize cleanup.

The following diagnostic candidate has SHA-256
`AF76AD56CD00F2A41303804CC0AD27165D7B9CEB54EE343DA4152420897C5136`.
Its fault-only independent observer reads the same held handle; it cannot finish
an operation, retire a document, or convert a failed gate into a pass. Native
failure handlers and the actual production exit watcher remain authoritative.

| Artifact | Terminal result | Crash dispatch → physical-exit acknowledgement |
| --- | --- | --- |
| `webview-process-handle-diagnostic.log` | exit 0 | 2,585ms |
| `webview-process-handle-repeat-1.log` | exit 0 | 4,747ms |
| `webview-process-handle-repeat-2.log` | exit 0 | 4,280ms |
| `webview-process-handle-repeat-3.log` | exit 0 | 1,297ms |
| `webview-process-identity-full.log` | full WebView gates, exit 0 | 3,083ms |

The first repeat also records an unsignaled exact handle at `ProcessFailed`,
followed by 2,937ms in the production exit watcher before settlement. The
independent observer then sees the same handle signaled. Therefore a native
failure notification by itself is insufficient evidence of physical exit;
clearing ownership immediately on that event would weaken the implemented
contract. This explains a real class of long tail, not the precise cause of
every earlier failure. No sleep, action retry, production deadline increase or
timeout-to-idle fallback was added.

The full run adds a fourth **real native setup cancellation** point:
`ProcessIdentityReply`, after finding a renderer and before attaching it or
queuing evaluation. It cancels in 101ms, old effect 0, next effect 1. Existing
FrameReply/ContextEvent/CreateReply phases take 95/76/76ms. A unit regression
also prevents a delayed old witness from replacing the next ticket's live
witness. This remains setup cancellation, not immediate Stop of running JS.

The final-source validation command is terminal exit 0:

- `webview-process-identity-unit-build.log`: harness rebuild 53.71s; the required
  Windows manifest is embedded before executing the harness.
- `webview-process-identity-unit.log`: **113 passed, 0 failed, 1 existing ignored**,
  67.87s. The ignored test remains the manual native desktop fixture. The count
  is unchanged because the stale-witness assertion extends an existing test.
- `webview-process-identity-clippy.log`: strict probe/test Clippy passes, 19.89s.
- `webview-process-identity-default-clippy.log`: strict default-product Clippy
  passes, 13.43s. Diagnostics and fault injection remain probe-feature-only.

The running user App is revalidated by path, PID 75508 and its original
02:53:53 local creation time, and left untouched. No matching disposable-crash
WebView2 process remains after the rounds; retained profiles are not deleted
based on that single diagnostic snapshot. This is still not installed-App,
real-model, other-platform or frozen-soak acceptance.

## Boundaries that remain open

This does not implement immediate operation-specific cancellation of already
running JavaScript, full browser-environment/all-child cleanup, or normal native
close cleanup when neither a completion nor monitored exit proof is observed.
Occupancy stays pending rather than reporting false idle. The native identity
interfaces must be available before evaluation; no weaker identity fallback is
used on an older/unsupported runtime. Disposable crash profiles are retained
until native environment/child exit can be established; no broad profile cleanup
or user browser termination is performed.

The full objective remains unchanged: the R13 UI/UX redesign still requires full
installed-App acceptance; all requested Windows/macOS/Linux desktop and browser
surfaces, native Wayland, signed runtime delivery/update/rollback, actual model
execution, extension matrices and the frozen 12-hour active soak remain subject
to the existing requirement-by-requirement gates. The user's running App is not
replaced, and no commit, push or PR is performed by this batch.

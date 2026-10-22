**MERGE-AFTER-FIXES**

Independent second review of `cu2/fix-gate`, HEAD `0fe4c603`, against local `origin/feat/cu-v2` (`ef5cb876`), including the earlier report in `38f5437a`. Static review only: no Cargo, Windows execution, network, SSH, source edits, or commits. The author's ADR statements and historical test results are not independent verification.

## Must-fix

1. **High — R1 remains partial; new blocking work inside both safety-critical locks.** Move full window enumeration/security/COM validation out of the pointer-gate/ledger critical section and retain bounded final admission checks immediately before insertion. Currently cancellation and foreground are checked *before* that potentially blocking validation, so a cancellation, deadline, or foreground change during validation can still be followed by `SendInput`. The additional validation can also consume the probe's entire 40ms wait and cause a permanent fail-closed Stop. See N1 below.
2. **High — R3 still accepts an unapplied move as arrival.** The new test explicitly treats the old one-pixel approach position as success. Confirm the actual normalized destination/completion before releasing the gate or allowing dependent button packets; merely documenting the old tolerance does not remove the wrong-position interleave. Preserve failure and uncertainty handling for timeout/unreadable results.
3. **Medium — R4 remains an unenforced single-host assumption.** Provide desktop-wide injection ownership or coordination. The new ADR's “one active host” assumption has no enforcing gate in the reviewed input/worker paths. Two independent hosts with different valid Stop chords still use different mutexes for one shared cursor.
4. **Medium — R5 remains outside the stated 100ms latch contract.** The 20ms polling loop and inclusive 100ms validity test are unchanged. Enforce expiry at dispatch and arrange supervision against the absolute proof deadline; document scheduler limits honestly. An addendum acknowledging approximately 120ms detection does not implement the original bound.
5. **Medium — finish the safety regression coverage alongside these fixes.** Add an admission test that actually closes/cancels/expires the context or changes foreground during the last validation; a delayed near-neighbour move test; a mid-drag settle failure with cleanup; and a Windows parent/child test of the real shared gate. The newly added helper tests and fake click failure do not cover those boundaries. No tests were run in this review.

Items 3 and 4 are inherited unresolved findings, not newly introduced regressions. They remain safety acceptance gaps; this report does not silently waive them because they are now listed in an ADR.

## Nice-to-have

- Restrict the new public test helpers to an explicit test-support feature. `#[doc(hidden)]` does not remove an API from production builds. The helpers do not themselves bypass input admission, but they expose a process-lifetime synchronization primitive to every Windows consumer.
- Enforce the role/binding lifecycle at the native worker constructor and reject binding after a gate handle has already been created. Current executable paths initialize in the right order, but the public constructor and independently cached binding/handle permit a wrong-name gate in other library call sequences; see N2.
- Distinguish `Settle::TimedOut` as `DriverError::Timeout { maybe_executed: true }` if callers need timeout classification. It currently becomes `Native`, as the new ADR specifies. This is safe for uncertainty, but the protocol reports `driver_error`, not `timeout`.
- Preserve the actual mutex creation/wait/release error for diagnosis. Permanently caching a failed handle as zero is fail-closed and acceptable for safety, but currently looks like ordinary contention forever; recovery requires restarting that process. Check/report `ReleaseMutex` failure without putting blocking diagnostics in the critical section.
- Strengthen the Windows integration synchronization: its `ready` signal precedes the wait, so it does not establish that the waiting thread was blocked before release. Add real same-thread recursive acquisition, unwind release, and cross-process abandonment cases. Keep the existing thread-abandonment case.

## R1–R6 disposition

Paths below use `W/` = `computer-use/crates/gcu-driver-windows/src/`, `WT/` = that crate's `tests/`, and `CLI/` = `computer-use/crates/gcu-cli/src/`. All code line numbers refer to HEAD `0fe4c603`.

| Finding | Status | Code evidence and remaining limit |
|---|---|---|
| **R1: admission after waiting** | **PARTIAL** | `W/input/ffi/native.rs:322` invokes `with_admitted_gate`; `W/input/hook_probe/gate.rs:170` acquires before `admit` at `:172`. Thus a refusal already observable when admission starts prevents insertion. However, `W/input/ffi/mod.rs:108` checks the request, `:112` checks foreground, then `:115` runs full `State::validate`. That calls `enum_windows` at `:121` and desktop/security checks at `:132` and `:149`; there is no request/foreground recheck after them. `W/input/ledger.rs:112` also still marks dispatch before the gate wait. N1 describes the new blocking interval. |
| **R2: host/worker mutex-name split** | **FIXED**, for the reported environment bug and current launcher/server paths | Production acquisition derives the owner from `BOUND`/own PID, not environment (`W/input/hook_probe/gate.rs:215`); an unbound host uses its PID (`:105`). Worker service binds before reading the first frame or initializing a native driver (`W/worker/server.rs:96`, `:116`); `serve` delegates to that service (`:75`). The launcher clears inherited environment and overwrites the worker PID after configured variables (`W/worker/process.rs:30`, `:52`, `:55`). Missing/invalid/zero worker values are refused (`W/input/hook_probe/gate.rs:51`, `:91`). This fixes the original `GCU_HOST_PID=0` host repro. It does not authenticate an arbitrary parent/child or enforce the invariant for every public constructor sequence; see worker audit and N2. |
| **R3: settle unknown treated as success / false arrival** | **PARTIAL** | `W/input/ffi/native.rs:338` inserts, then `:348` propagates settle failure; `W/input/pointer_settle.rs:75` maps timeout/unreadable to `Native(true)`. Subsequent planned packets stop at `W/input/mod.rs:308`. But `W/input/pointer_settle.rs:50` retains ±2px acceptance and its new test at `:115` expressly accepts the unmoved `(101,101)` approach for `(100,100)`. The original near-neighbour false-arrival defect remains. |
| **R4: multiple hosts share the desktop cursor** | **NOT FIXED** | Name remains `Local\\gcu-pointer-gate-{pid}` (`W/input/hook_probe/gate.rs:73`); independent hosts select their own PIDs (`:105`). No desktop-wide owner was added. Probe sample/publish and separate action buttons remain at `W/input/hook_probe.rs:232`, `:241` and `W/input/plan.rs:78`, `:80`. ADR addendum `computer-use/docs/adr/0013-addendum-cross-process-pointer-gate.md:46` records an assumption, not enforcement. |
| **R5: proof expiry versus latch deadline** | **NOT FIXED** | `W/input/hooks.rs:121` accepts age equal to 100ms. `W/input/watchdog.rs:12` retains 20ms polling; `:66` checks then `:67` sleeps. `W/input/hotkey.rs:277` detects expired injection only when called. `W/input/ffi/safety/probe.rs:338` authorizes from generation/phase/Stop flags without checking proof age. The new admission closure has no synchronous guard/lease check either. ADR addendum `:47` acknowledges the gap. |
| **R6: test double diverges from production / incomplete race proof** | **PARTIAL** | The std double now shares explicit instances across threads and tracks recursive depth (`W/input/hook_probe/gate.rs:313`, `:318`, `:373`); real contention tests replace same-thread denial (`W/input/injection.rs:136`, `:169`, `:194`). `WT/pointer_gate.rs:7`, `:39`, `:77` exercise the normal Windows library's kernel gate rather than a `#[path]` test copy. Still no parent/child gate-identity test or integrated Sender/MouseProof/GuardEvents test was added. The original race fake still conditions its interleave on `gate_held()` (`W/input/injection.rs:56`); fake proof delivery still calls `confirm_hook_proofs` directly (`W/input/hotkey/fake.rs:176`). See coverage table below. |

## N1 — new blocking validation under gate and ledger; admission can go stale again

**Severity: high. Introduced by the new `recheck_admission` call, not by the old gate wait alone.**

The actual action acquisition order is:

```text
InputLedger.held
  -> context.begin()
  -> named pointer mutex
  -> request.check()
  -> GetForegroundWindow()
  -> State::validate()
       -> RealWinApi.enumeration mutex
       -> complete desktop HWND/child enumeration
       -> process/session/token queries and virtual-desktop COM
  -> SendInput
  -> optional cursor settle
```

`W/input/ledger.rs:111` holds the ledger through its `send()` closure at `:123`. `W/input/hook_probe/gate.rs:170` holds the kernel mutex through both `admit` and `body` at `:172`. The newly added call at `W/input/ffi/mod.rs:115` is therefore inside both locks.

`request.check()` itself is bounded atomic/time work: `W/uia/worker.rs:27`, `W/uia/patterns.rs:191`, and `computer-use/crates/gcu-core/src/driver.rs:148`. `GetForegroundWindow()` is a scalar foreground query. **`State::validate()` is not a bounded scalar check:**

- `RealWinApi::enum_windows` takes an ordinary blocking `Mutex::lock`, with no request deadline (`W/ffi/mod.rs:169`, `:175`). Other discovery/native readers use this API too. Its protected operation is a full `EnumWindows`, not a lookup of the target alone (`W/ffi/windows.rs:152`, `:160`). Every included window can enumerate all children and query process creation metadata (`:115`, `:126`). Even if every individual call returns, work scales with the desktop's windows/children.
- Each of the two `on_current_desktop` calls in validation creates a COM manager and calls `IsWindowOnCurrentVirtualDesktop` synchronously (`W/ffi/windows.rs:171`, `:175`, `:183`). The gate/security path also queries session state synchronously (`W/ffi/security.rs:129`, `:134`, `:214`). No enclosing 40ms validation deadline exists. An async caller timeout cannot interrupt this synchronous work.
- Validation now also emits timings while the gate is held (`W/ffi/mod.rs:170`, `W/timing.rs:23`, `:33`). The CLI installs a synchronous stderr tracing writer (`CLI/lib.rs:98`). Thus even “metadata-only” logging is not evidence of a bounded critical section. The gate's own post-acquire wait log at `W/input/hook_probe/gate.rs:171` is an additional pre-existing type of unbounded sink risk.

**Constructible failure A, stale admission:** allow the new request/foreground checks to pass, pause the following enumeration or desktop COM call, then close/cancel the action, let its deadline pass, or change foreground. Return otherwise valid metadata. Validation does not check `current.foreground` or re-run the request at its end (`W/input/ffi/mod.rs:125`, `:158`), and `dispatch_pointer` immediately inserts at `W/input/ffi/native.rs:338`. There is no later admission check. The keyboard branch has the same check-then-block problem at `:334` and `:335`. This is a controlled blocking interval, not just the unavoidable few instructions between a final check and native insertion.

**Constructible failure B, spurious permanent Stop:** hold the added validation for more than 40ms while the action is in the Injecting phase and schedule a probe to contend. The probe cannot obtain the kernel mutex, returns a refused cycle (`W/input/hook_probe.rs:214`), and its report invokes `refuse_proof` (`W/input/ffi/safety/probe.rs:161`). Injecting refusal calls `fail` (`W/input/hotkey/lease.rs:74`), which disables the clock and requests Stop (`W/input/hotkey.rs:295`). The 15ms settle budget starts only after insertion (`W/input/ffi/native.rs:385`); it does not bound validation, mutex initialization, logging, or `SendInput` itself.

**Deadlock assessment:** I did not find a closed internal lock cycle in these paths. The added enumeration mutex is not taken by the probe sender, hook callbacks, or FFI-free watchdog. Hook callbacks operate on thread-local proof state and the physical clock (`W/input/ffi/safety.rs:157`), and do not acquire ledger/gate/enumeration. Probe authorization takes GuardEvents' short window lock (`W/input/hotkey/lease.rs:49`); refusal releases that lock before `fail` (`:75`, `:80`). This is evidence against an internal ABBA cycle, not a guarantee against arbitrary WinApi implementations, synchronous COM/Win32 stalls, or blocking logging sinks.

**Stop/release distinction:** native up-only insertion bypasses the pointer gate (`W/input/ffi/native.rs:406`), but direct-backend cleanup still waits on this same ledger (`W/input/ledger.rs:144`, `W/input/ffi/mod.rs:413`). The fix adds potentially blocking metadata reads to that wait. In the isolated product path, the authoritative parent ledger is separate and only reserved briefly (`W/worker/input.rs:215`); supervisor kill/reap precedes parent cleanup (`W/worker/supervisor.rs:403`, `:413`, `:415`). That compensation matters. It does not make the native admission check fresh, nor prove that every cancellation wins the race against a still-live worker's insertion.

**Required correction:** finish potentially blocking validation outside the gate/ledger and design the final bounded identity/permission/revocation check explicitly. Run cancellation/deadline/foreground checks after all blocking preparation and immediately before marking/inserting the packet; include the current injection permission/lease boundary. Do not solve the added contention by enlarging 40ms or the 100ms lease. Test with barriers in validation, not only with an `admit` closure that unconditionally returns a preset error.

## `not_executed()` audit — no earlier-effect erasure found

The local normalization at `W/input/ffi/mod.rs:82` is **correct in its current call sites**. It describes this packet's insertion, not the whole action's history:

1. `dispatch_packet` snapshots `previous = context.begin()` and `pending = held.len()` before reserving this packet (`W/input/ledger.rs:112`, `:113`).
2. On a proven no-send error it restores that snapshot and truncates only additions after `pending` (`:124`, `:125`, `:126`). `ActionContext::restore(true)` preserves prior uncertainty; `restore(false)` clears only the uncertainty bit and preserves the closed bit (`W/uia/patterns.rs:226`). Earlier pending releases remain in the ledger.
3. Before returning, `context.error` reapplies earlier action uncertainty (`W/input/ledger.rs:139`, `W/uia/patterns.rs:241`). Earlier successful movement/activation can therefore make an otherwise refused packet's outward result uncertain. This is conservative and correct.

The sequence in `with_admitted_gate` is `admit().and_then(body)` (`W/input/hook_probe/gate.rs:172`), and `not_executed` is used only by the pre-insertion admission function (`W/input/ffi/mod.rs:109`, `:115`). It does not wrap a `SendInput` error or a settle error. A partial native packet returns `maybe_executed = count != 0` (`W/input/ffi/native.rs:419`), so its possible downs are retained. There is no path here that takes an actually partial insertion and clears its uncertainty with `not_executed`.

One remaining conservative over-report is that `begin()` still precedes waiting/validation: a concurrently timed-out caller can observe uncertainty before insertion has actually started. That is not an unsafe false “nothing executed” result. Add explicit tests for refusal after an earlier successful packet, refusal with an earlier held button, and partial insertion; the new admit-order helper tests do not verify this ledger composition.

## Settle result, caller composition, and error variants

Timeout/unreadable handling is now a real error path. `SettleProgress` distinguishes “at least one off-target position was read” from “no readable position” (`W/input/pointer_settle.rs:28`, `:63`); failed samples alone cannot become arrival. The native adapter propagates the result after `SendInput` (`W/input/ffi/native.rs:338`, `:348`). Both failure variants currently become **`DriverError::Native { maybe_executed: true }`** (`W/input/pointer_settle.rs:85`), as the new ADR says at `computer-use/docs/adr/0013-addendum-cross-process-pointer-gate.md:55`.

Unreadable-to-Native is appropriate. TimedOut-to-Native safely preserves uncertainty and stops composition, but it does **not** use the driver's typed timeout variant: core maps Native to `ErrorCode::DriverError` and Timeout to `ErrorCode::Timeout` (`computer-use/crates/gcu-core/src/driver.rs:401`). I found no requirement that the internal 15ms arrival budget must be the overall driver deadline, so I do not elevate this classification choice to a safety blocker. Tests should assert the intended public error code rather than only the uncertainty bit.

The dependent-packet control flow is sound **when settle actually returns an error**:

- Foreground click uses separate approach, target-move, and button-pair packets (`W/input/plan.rs:74`, `:78`, `:80`). `InputBackend` propagates `result?` before taking the next packet (`W/input/mod.rs:308`), so failure of either move prevents later buttons/count repetitions.
- Drag events are a start move, down, incremental moves, then up (`W/input/messages.rs:183`, `:196`, `:205`), each converted to a separate packet (`W/input/plan.rs:104`). A failed start move prevents down; failure after down prevents remaining path packets. The pending down is released by unconditional cleanup (`W/input/mod.rs:328`, `W/input/ledger.rs:143`). Cleanup ups are intentional; “no dependent packets” must not forbid them.
- An uncertain result does not acknowledge the packet as definitely delivered or unsent (`W/input/mod.rs:303`). The parent can retain possible downs and kill the child before cleanup when recovery is pending (`W/worker/supervisor.rs:409`). No action downs are replayed.

The new fake test (`WT/input/routes.rs:65`) injects a Native error on the **first approach packet**. It checks loop short-circuiting, not `GetPhysicalCursorPos`, the real 15ms loop, final target movement, a mid-drag failure, or Windows packet insertion.

**Residual R3:** `cursor_arrived` accepts the old approach immediately (`W/input/pointer_settle.rs:50`, `:122`). The gate can then reopen before the final move becomes visible; a probe can sample/publish that old position between movement and a coordinate-less button (`W/input/hook_probe.rs:232`, `:241`; `W/input/ffi/native.rs:218`). The new comments call this an “accepted tradeoff,” but the reviewed code supplies neither completion evidence nor a new authorization of that safety loss. A correct normalized-destination strategy should be evaluated; the probe already has bin-aware normalization (`W/input/hook_probe.rs:183`). Simply making a tolerance smaller without correcting destination calculation is not a sufficient fix.

Two limits also prevent overclaiming: the 500µs sleeps and FFI are not a strict 15ms wall-clock guarantee (`W/input/ffi/native.rs:390`, `:399`, `:402`); and settle runs after the **whole** input packet was inserted. Scroll still combines move+wheel in one packet (`W/input/plan.rs:97`), and an arbitrary combined move+button packet would already have sent its button. The current planned click/drag composition avoids that latter case. These are not new split-packet regressions.

## Worker identity, environment, abandoned ownership, and initialization

| Entry or condition | Static result |
|---|---|
| Shipped CLI worker | `CLI/worker.rs:7` calls `serve_with_input`; native `new_worker_with_input` is invoked only inside its initializer at `:12`, after binding at `W/worker/server.rs:96`. No native input before binding was found in this entry. |
| `serve` wrapper | Delegates to `serve_with_input` (`W/worker/server.rs:75`); does not bypass binding. |
| Native contract worker | `WT/contract/worker_bin.rs:44` enters `serve_with_input`, then constructs the native driver inside its initializer at `:49`. Preceding code only sets up logs/forwarding. |
| Fault/input test workers | Normal fault service uses `serve` (`WT/worker_support/fault_bin.rs:78`); tracked input service uses `serve_with_input` (`WT/worker_support/input_fault.rs:81`). The fault binary's hand-written branch at `WT/worker_support/fault_bin.rs:85` bypasses service binding, but its driver is a portable `FakeWinApi`/FakeUia fixture (`WT/worker_support/fixture.rs:23`), not a native input entry. It is not evidence of an unbound real worker sending input. |
| In-memory unit tests | `W/worker/server/tests.rs:163` calls `serve`, but binding is deliberately a no-op under `cfg(test)` (`W/input/hook_probe.rs:49`, `:53`). These tests cannot establish Windows bind enforcement. Non-Windows normal dependencies also skip native binding. |
| Host environment spoof | Production acquisition does not read `GCU_HOST_PID`; it uses binding or own PID (`W/input/hook_probe/gate.rs:215`). A grandparent setting the host's environment cannot recreate the reported split. “Host never reads env” here means **this identity variable**, not all environment variables: the optional debug gate-off switch still exists. |
| Configured worker environment spoof | `command.env("GCU_HOST_PID", actual_pid)` follows `command.envs(config.environment)` (`W/worker/process.rs:52`, `:55`), so a configured conflicting value cannot win on the normal launcher path. |
| Untrusted parent or compromised child | Parsing accepts any nonzero u32 (`W/input/hook_probe/gate.rs:55`). There is no OS parent-instance check or handshake comparison. A malicious alternate launcher, or code executing in the child before binding, can supply a different valid PID; after binding, later environment changes do not rename the gate. The private inherited-stdio service explicitly trusts the unelevated launcher (`W/worker/server.rs:63`). This is an identity-consistency fix within that trust model, not authentication of hostile processes or a new Host grant. |
| `WAIT_ABANDONED` | Correctly treated as acquired ownership (`W/input/hook_probe/gate.rs:230`, `:231`), with a guard that releases it at `:199`. The guard's raw-pointer `PhantomData` prevents Send/Sync (`:192`). No clock is confirmed by acquiring an abandoned mutex. This says nothing about completion of the predecessor's queued input or release of its held buttons; parent ledger recovery remains necessary. |
| `HANDLE_BITS` initializes to zero | Both refused binding and `CreateMutexW` failure cache zero forever (`W/input/hook_probe/gate.rs:213`, `:216`, `:222`). Later acquire returns GateBusy before admission/body (`:224`). This is fail-closed, not a successful zero-handle guard. Permanent denial is acceptable for safety; operational recovery/diagnostics are weak. The separate debug-only gate-off branch is explicitly gated by both feature and debug assertions (`:206`). |

### N2 — binding and the cached handle can disagree after early acquisition

**Severity: medium robustness/API defect; no occurrence found in the current executable startup paths.**

`BOUND` and `HANDLE_BITS` are independent OnceLocks. Acquisition while `BOUND` is empty permanently creates the own-PID mutex (`W/input/hook_probe/gate.rs:213`, `:215`); later `install_worker_pid` can successfully set `BOUND::Worker(other_pid)` without inspecting that handle (`:84`). Owner reporting then returns the bound PID (`:136`), while acquisition keeps using the earlier own-PID handle. This is a concrete possible sequence, not a claim that the CLI currently executes it.

The new public `pointer_gate_enter_for_test` makes early acquisition externally callable (`W/input/mod.rs:51`); more generally, public `WindowsDriver::new_worker` constructs the native input adapter without binding/asserting a worker role (`W/driver.rs:135`, `:136`, `:170`). Every native executable found in this tree calls it through the correctly ordered server initializer, but the constructor itself cannot enforce that rule for library consumers. Once the wrong handle exists, valid late binding does not fail closed. Bind/check inside worker construction, or freeze role and handle together and reject incompatible late binding.

## Public helper surface and what the Windows tests can establish

The new guard re-export, acquisition function, and owner-query function have only `#[cfg(windows)]`, not a test-support feature (`W/input/mod.rs:43`, `:49`, `:58`). Thus **yes, the normal Windows crate's public API grows**. `#[doc(hidden)]` only hides documentation. The workspace has `publish = false` (`computer-use/Cargo.toml:25`), so this is not a claim about a newly published crates.io package; it is still the API of the dependency linked into the shipped application.

**No direct production input bypass was found through these helpers.** They can acquire/hold/forget the mutex or report an owner; they cannot send input, change a grant, choose a new owner, clear Stop, or create proof. Same-thread recursive acquire retains the mutex's outer ownership and still runs native admission. Holding it too long can cause denial/Stop; N2 is the more subtle initialization hazard. The explicit debug `GCU_POINTER_GATE_OFF` bypass predates this fix and is not enabled by the new public guard.

Coverage below describes the paths present in the tests, **not executed results**:

| Property | Std/unit coverage | Real Windows coverage in `WT/pointer_gate.rs` |
|---|---|---|
| Shared cross-thread exclusion | Explicit shared gate in `W/input/injection.rs:169`; no longer the old same-thread denial test. Default gate is still thread-local unless explicitly shared (`W/input/hook_probe/gate.rs:412`). | `:39` holds the production mutex; a separate zero-wait contender must fail at `:57`; another thread can acquire after release at `:64`. This can validate actual CreateMutex/Wait/Release behavior in one process. |
| Same-thread recursive depth | Explicit double recursion test at `W/input/injection.rs:194`; double implements it at `W/input/hook_probe/gate.rs:318`. | **Not tested.** The two-thread test is not a recursion test. |
| Abandoned owner | Explicitly not modeled (`W/input/hook_probe/gate.rs:288`). RAII panic cleanup is different from abandonment. | `:77` acquires, `:81` forgets, owner thread exits and is joined at `:85`; `:86` must acquire and `:89` checks subsequent release. This exercises real thread abandonment in one process. No process-crash/input-queue recovery is covered. |
| Worker identity and name shared with host | Parser/slot unit tests at `W/input/hook_probe/gate.rs:464`, `:473`, `:509`; test bind path is a no-op. | `:26` checks only an unbound process's reported PID. No child is launched, no worker bind is tested, and no hostile/missing worker environment or cross-process mutual exclusion is exercised. |
| Admission refuses without sending | Generic shared-double closure tests at `W/input/injection.rs:223`, `:251`. They establish helper ordering; neither calls the real admission/ledger/WinApi path. | **Not tested.** |
| Move settles before dependent input | Pure sample folding and the fake first-move click error test. The tolerance test deliberately accepts the problematic near neighbour. | **Not tested.** No SendInput/GetPhysicalCursorPos occurs in this integration target. |
| Retry proof commits through the real sender | MouseProof policy tests remain useful; FakeSafetyApi still supplies paired timestamps directly. | **Not tested.** Nothing in this target exercises Sender, callbacks, report ordering, lease expiry, or Stop. |

On non-Windows, `#![cfg(windows)]` at `WT/pointer_gate.rs:7` removes all cases. On Windows, its normal library dependency is compiled without `cfg(test)` and therefore selects the kernel implementation (`W/input/hook_probe/gate.rs:178`). Library unit tests, including Windows ones, select the std double instead. Passing one target must not be reported as proof of the other.

## Verification limits

- Read the complete branch diff for the requested files and ADR change, the earlier R1–R6 report, and surrounding ledger/context, WinApi, probe, worker/recovery, and composition paths. The retry transition code itself was not changed by this fix; its original pure-policy evidence is not upgraded to native integration evidence.
- Rust compilation, Cargo tests, Windows mutex tests, native cursor/input experiments, stress runs, and runtime deadlock/timing measurements: **not_run**, per task constraints. Failure timelines above are constructed from code, not observations of Windows failures during this review.
- Only this uncommitted report was written. No source, prior report, ADR, configuration, or Git history was changed. Document whitespace and cited file/line existence were checked locally after writing.

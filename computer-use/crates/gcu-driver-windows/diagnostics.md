# Disposable Windows desktop diagnostics

CI passes `-NativeDiagnostics` to `scripts/smoke-windows.ps1` to run two ignored
diagnostics inside the same verified Medium, non-admin, interactive Default
desktop as the contracts. Neither is called by the product listener. A diagnostic
failure is printed and the contracts still run. The contracts independently run
the mouse matrix **in their own process**, before starting the product listener;
only that measured evidence may permit the listener-dependent environment Skips
below. The standalone JSON artifact is diagnostic only and cannot grant a Skip.
Use only the disposable fixture desktop: the matrix includes movement and XBUTTON.

## Product assembly (`native-windows`, default off)

`gcu-cli` selects this route only when the `native-windows` feature is compiled
and `--driver` is omitted/`auto` or `native`. Startup reads the process token,
refuses High/System, UIAccess, a secure desktop, or a non-interactive session,
then calls `WindowsDriver::monitored`. That is the Worker path: parent hooks,
hotkey, up-only release, job object, and the worker handshake. It does not fall
back to `InProcess` or to the fake driver. Session 0 and the portable fake
listener prove the refusal and the Host latch only.

Physical mouse/keyboard, fourth-probe startup and renewal, UAC, hook loss,
hotkey conflict, Stop while input is held, a stuck worker, DPI, and system-app
smoke remain **not_run**. A hosted runner skip does not replace them.

## Mouse hook delivery

`unsafe_mouse_probe::live_mouse_probe_matrix` installs its own WH_MOUSE_LL and
keeps pumping while a separate sender calls SendInput. It tries every candidate
with a unique `dwExtraInfo` marker and with zero extra info. Each row prints sent /
requested, immediate Win32 error, send and callback elapsed time, callback flags,
and physical cursor position before/after. Full rows are saved in
`mouse-probes.json`; an interrupted row retains pending-send evidence in stdout.
A missing callback is observed for at least one second; a completed send without a
callback is not success. All hook callbacks are retained (including mismatched
markers), so unrelated input also prevents a claim of complete hook silence.
An unmarked callback may include unrelated input; the marked row with
`LLMHF_INJECTED` is the actual product proof requirement.

| Candidate | Intended effect / remaining side effect |
| --- | --- |
| ZeroWheel | Zero vertical delta; no intended scrolling; delivery unproven |
| ZeroMove + NOCOALESCE | No intended movement; delivery unproven |
| ZeroHWheel | Zero horizontal delta; no intended scrolling; delivery unproven |
| AbsoluteSamePosition + VIRTUALDESK + NOCOALESCE | Targets current physical pixel; cursor-query race can undo a concurrent user move; hover processing possible |
| RelativePair +1 / -1 | Visible movement/hover possible; acceleration and clipping may prevent exact restoration |
| XButtonPair | May navigate, activate a window, or trigger a global shortcut |

Product defaults try zero move, zero wheel, zero horizontal wheel, then
**absolute same position**. An accepted send without a tagged injected callback
advances to the next kind; a pending send never starts another dispatch. A working
kind, including the absolute kind, stays preferred across action windows. Once a
zero kind works, no absolute move is used. RelativePair and XButtonPair remain
excluded from product builds.

Under [ADR 0013 B](../../docs/adr/0013-windows-stop-guard-liveness-lease.md),
installation may run one initial proof cycle; it never renews that cycle while
idle. A foreground `perform` requests at most one new preparation cycle when the
injected lease is not fresh, **after the common live target gate** and before
action packet construction or mutation RPC. A probe is input, so window/process
identity, engine/target integrity, permissions, UIAccess and secure/virtual desktop
checks must succeed before its dispatch. The worker parent uses the existing
read-only Geometry and ListWindows RPCs, matches only the original stamped ID and
requires `on_current_desktop == Some(true)`; the child rechecks before action input.
Preflight, preparation and mutation share the original 4750ms Driver deadline.
Proof failure returns CapabilityUnavailable
with no action input and no Stop latch. Inner checks cannot request another cycle.
During an admitted action, the working kind renews the dual-hook lease directly;
missing callbacks still fail closed at the unchanged 100ms boundary, synchronously
latching Stop and signalling independent release. Recovery and action completion
precede discarding the lease, including on abandonment. No timer was lengthened.

`ABSOLUTE_PROBE_IDLE_MS = 2000` gates only the initial unsolicited absolute send.
Its single GetLastInputInfo/GetTickCount64 sample precedes any initial probe input;
unknown input time, future ticks, wrap or physical callbacks deny that attempt.
A requested foreground action bypasses this initial gate. Denial or cursor failure
belongs to one proof cycle, so physical input cannot permanently reject a later
action's proof. System idle samples and physical callbacks never confirm/renew an
injected lease. No repeat probes run outside an action window, so the idle listener
does not keep resetting the system input clock.

Physical activity covers the listener generation from its first successful dual
proof until disable/failure; 1s hook replacement and idle lease expiry preserve
that history. Confirmations never manufacture physical activity. Before the first
proof, without a physical callback, or after revocation, age is None. Only real,
non-injected, untagged callbacks write activity age. Late callbacks/confirmations
cannot revive a disabled listener. Foreground capability reflects transport health;
it does not authorize injection without the action proof. Doctor reports three
separate facts: `stop_hotkey` transport, `injection_lease`, `user_activity_guard`.
The last remains unavailable until a trusted physical callback exists; a fresh
pump is never reported as a proven hook. Stop and thread-failure latches remain
user-only to resume.

Before dispatch, the sender enters per-monitor DPI awareness, reads physical
cursor pixels and virtual-desktop bounds (including negative monitor origins),
and maps the current pixel's center into 0..65535. Unrepresentable desktop bounds
refuse. After dispatch it reads the physical cursor again. A difference records
`cursor changed during probe`: that proof is invalid, and **no retry, compensation
move or subsequent keyboard input** runs. An unreadable post-position also fails
closed. Matching injected callbacks remain tentative until SendInput acceptance
and this check succeed; per-attempt mouse tags reject late earlier callbacks.
Idle checks and post-position checks cannot eliminate the race with concurrent
physical movement, including movement back to the original pixel between samples.
They do not establish that the move had no hover side effects.

Doctor reports `mouse hook liveness could not be proven (attempted N probe kinds)`
when proof is missing; action input remains gated even with healthy transport. The count measures actual
submitted kinds, not the number of accepted SendInput records. Release/default
builds use this fixed order and never select relative movement/buttons from a
diagnostic result. Doctor also always discloses the absolute move and hover effect,
the idle threshold and fail-closed behavior, including when monitoring is healthy.

Only builds with **both** `test-worker-faults` and debug assertions accept
`GCU_HOOK_MOUSE_PROBE` (`zero_move`, `zero_wheel`, `zero_hwheel`,
`absolute_same_position`, `relative_pair`, `xbutton_pair`). The contract harness
sets it from its own matrix; deterministic preference favors zero-effect probes,
then same-position, relative motion, and finally XBUTTON. If any variant sees any
callback, environment Skip is forbidden and that probe kind is used by the real
listener. Unmarked-only/stripped-flag evidence never replaces the listener's
mandatory tagged injected callback: if the real proof still fails, the cases fail.
Paired probes check the actual requested count; a partial XDOWN attempts XUP cleanup.
Invalid debug selection fails closed. Default builds and release builds (even with
the feature) reject any GCU_HOOK_MOUSE_PROBE value. RelativePair and XButtonPair
record construction is excluded from product builds. Stop leases, startup bounds, and latch
semantics are unchanged.

### Physical-mouse result (2026-10-17, PR #90)

The physical-mouse Windows matrix accepted zero_move / zero_wheel / zero_hwheel
SendInput records, but none reached WH_MOUSE_LL. AbsoluteSamePosition and
RelativePair reached the hook with LLMHF_INJECTED and preserved dwExtraInfo.
This is why the fourth product candidate is necessary. The physical mouse was
also moving during that run: its before/after positions do **not** establish net
probe displacement or absence of side effects. RelativePair remains diagnostic
only; that result does not authorize diagnostic-based selection in product builds.
The new guarded product path and its native renewal behavior are **not_run**;
the older matrix is not evidence that the new gate permits startup or renewal.

`mouse_matrix::decide` permits environment Skip only after all 12 unique variants
complete SendInput with sent==requested, Win32=0, no dispatch error, and at least
1000ms of pumping without **any** callback. Missing, duplicated, pending, failed,
or truncated measurements fail setup; a working row always forbids this Skip.
Only C06/C08/C09/C11/C15/C20/C22/W-HOTKEY use this decision when foreground is
Unavailable. Their reasons include the full 12-variant summary and all physical
cursor positions; neither a product timeout nor API acceptance alone is evidence.
`mouse_matrix` and harness unit tests cover silent, every working variant,
incomplete/duplicate/error/short rows, and listener failure despite a working row.

The physical-input clock must observe mouse activity as well as keyboard activity:
a surviving keyboard hook cannot establish that the user is idle while a removed
mouse hook misses their movement/clicks. Windows documents silent LL-hook removal
and offers no documented liveness/self-test query for a hook handle. A message
pump heartbeat proves only the thread runs; SetWindowsHookEx success proves only
registration. Neither establishes callback delivery. Real physical callbacks can
prove reception when available, but cannot renew a 100ms lease during user idle.
A replacement input backend (e.g. Raw Input) would need its own physical/injected
classification, loss detection, desktop coverage and equivalent lease tests; it
is not a substitute implemented by these diagnostics.

### Physical-mouse result (win-test, 2026-10-17)

One `live_mouse_probe_matrix` run (XBUTTON skipped, 10 rows) on the user's physical
machine, interactive desktop, unelevated. Each row: `LLMHF_INJECTED` = flags bit 0.

| Candidate | marked | injected callback | marker kept (`dwExtraInfo`) |
| --- | --- | --- | --- |
| ZeroWheel | yes / no | **no / no** | n/a |
| ZeroMove | yes / no | **no / no** | n/a |
| ZeroHWheel | yes / no | **no / no** | n/a |
| AbsoluteSamePosition | yes / no | **yes / yes** | yes (marked row) |
| RelativePair | yes / no | **yes / yes** (2 callbacks) | yes (marked row) |

Findings:

- Zero-delta wheel and zero-move injections are accepted by `SendInput`
  (`sent == requested`, Win32 = 0) but **never reach `WH_MOUSE_LL`**. The three
  probes the product tries by default therefore cannot prove hook liveness on a
  physical-mouse machine: foreground would stay Unavailable there. This matches the
  GitHub-runner result (nothing reached the hook) for the zero-effect kinds, but
  unlike the runner, other kinds do arrive here.
- `AbsoluteSamePosition` and `RelativePair` do arrive as injected callbacks and the
  unique marker survives in `dwExtraInfo`, so a tagged proof is possible with them.
- Caveat: the user's own mouse was active during the run (many non-injected
  `WM_MOUSEMOVE` callbacks at ~2.5 ms, plus one real `WM_LBUTTONDOWN` and one
  `WM_LBUTTONUP`), and the cursor moved hundreds of pixels between rows. The
  injected/marker evidence is per event and holds; any claim that a probe leaves the
  cursor exactly where it was is **not** supported by this run. Repeat hands-off to
  measure net cursor displacement before adopting a probe.
- No product code changed. Choosing a side-effecting probe for release builds is a
  safety-model decision (hover side effects, concurrent-move race) and is left open.

## Background activation attribution

C16 runs a bare UIA Invoke control **inside the contract process and run** on the
same fixture counter element, checking its observed RuntimeId before dispatch.
The monitored driver's handle is a serialized `worker::Element`, not the
worker-local `UiaElement`. Fixture discovery polls Name/AutomationId readiness;
the control takes the verified owning HWND and RuntimeId from that proxy, calls
`ElementFromHandle`, then polls `FindAll(TrueCondition)` under that exact root for
the exact RuntimeId for up to 3 seconds (5 seconds overall including Invoke).
It logs root/control Name, AutomationId, ControlType and RuntimeId. Resolution or
Invoke errors fail the contract; only observations retry, never the mutation.
The control records foreground HWND/class/PID immediately before Invoke, immediately
after dispatch, after the exact external JSON increment, and after WM_NULL readiness.
A change during UIA resolution aborts before mutation. A successful control that
changes foreground to the fixture proves activation for that virtual element type:
only that C16 element variant Skips, with every measured identity and counter value
in its reason. A change to an unrelated process is unresolved and fails. If the
checkpoints include a zero HWND during activation, a later nonzero fixture-owned
HWND is required for attribution; a zero alone cannot grant Skip. If the
control preserves foreground, the driver press and SetValue must preserve the exact
same unrelated HWND at every checkpoint and produce the original external effects.
No assertion is relaxed and no action is replayed.

C16-BUTTON is an independent, mandatory contract row. It observes an ordinary native
BUTTON through the driver, performs Press, requires TargetedEvent (BM_CLICK), checks
an exact WM_COMMAND count of +1 including a native pump barrier, and asserts exact
foreground equality after dispatch, effect and pump. It cannot inherit the virtual
element Skip. The native window is shown without activation during setup; it retains
normal styles, so the oracle can detect subsequent activation.

Cover creation attempts foreground once, logging AllowSetForegroundWindow,
AttachThreadInput (with scoped detach), SetForegroundWindow's return/error, actual
foreground HWND/class/PID, and SPI_GETFOREGROUNDLOCKTIMEOUT/result. Denial retains
the topmost cover. C04 verifies physical occlusion at all sampled marker pixels
independently of foreground. C19 measures foreground after closing the fixture and
before the rejected action, retaining window_gone, zero unintended input and exact
foreground equality assertions. C16 can use an already unrelated actual foreground
if cover preparation fails; only an absent/unusable background precondition Skips,
with preparation diagnostics. No Alt injection or post-action foreground repair runs.

`ACTIVATION` traces include identities and EVENT_SYSTEM_FOREGROUND notifications.
The notification thread is evidence of the event generator, not the call stack or
caller of SetForegroundWindow. The separate opt-in diagnostic still compares bare
UIA Invoke on native/virtual controls against raw native PostMessage BM_CLICK; its
results cannot grant a contract Skip or satisfy C16-BUTTON.

The desktop harness runs contracts and system-app smoke behind independent error
boundaries and aggregates both failures into its exit status. Each smoke app runs
even after another app fails, records PASS/SKIP/FAIL with its reason, and saves its
row immediately. Native smoke tests have a 90-second deadline without replay.
Missing executables/protocol registration and explicit native launch availability
errors Skip with the actual query/error; window, driver and external-oracle failures
remain failures. Smoke summary counts those outcomes separately from contracts.

The fixture's opt-in egui plugin observes AccessKit action kinds and Focus commands
in frame output. It never consumes an action, removes Focus, or changes activation.
`fixture-activation-<pid>.jsonl` and native traces are uploaded and printed by the
script. Matching fixture PID + direct reproduction + stable native comparisons
can isolate a provider-specific effect. A Focus command is evidence of a request
for egui-winit's focus path; absence does not establish which other native call
activated the window. The same-run control, rather than speculative provider
attribution or a separate diagnostic process, determines the virtual-element Skip.

## Background press routing

Windows background capability remains Partial (03 §5.2/5.4, P2-05/P2-06). Exact
native BUTTONs post BM_CLICK to their validated child HWND and return TargetedEvent.
Other Invoke/Legacy elements use their semantic UIA pattern on the observation MTA
and return Accessibility. Providers can activate the target window: doctor declares
this measured limitation explicitly. DriverAction/capabilities have no separate
per-action activation bit; this change does not extend the architecture interface.

Semantic delivery retains the existing exact slot/RuntimeId/HWND/metadata/security
rechecks immediately before mutation, cancellation marker, worker timeout/unknown
classification and no-replay rule. Secondary Press/Confirm use the same pattern
priority. Explicit TargetedOnly pointer clicks retain child hit testing, stamped
window validation, parent InputLedger acknowledgements, Stop/cancel cleanup and
honest unsupported-recipient errors; they never fall back to UIA or foreground.
Other semantic patterns retain their existing routes. Restoring Invoke preserves
virtual-control coverage in egui, Chromium and UWP while honestly limiting background
operation; semantic delivery is not a non-activation guarantee.

## Required physical-mouse Windows release checks

Hosted CI with complete measured hook silence does **not** verify physical input or
native foreground/Stop behavior. Before release, use unelevated Windows with a
physical mouse on the interactive Default desktop and record these results:

- Run the 12-variant matrix and C06/C08/C09/C11/C15/C20/C22/W-HOTKEY with real
  foreground enabled only after both tagged injected hook proofs. A working
  variant must run these cases; missing proof remains a product failure.
- Verify physical mouse movement/clicks and keyboard activity reset the idle guard;
  injected events do not. Exercise hook replacement, silent loss, desktop/UAC
  transitions, and action-window proof renewal with the original lease limits.
- Verify Stop during a held key, click/drag and blocked worker: latch <=100ms,
  release <=500ms, kill blocked worker <=5s; queued actions stay stopped and
  reconnect/restart cannot resume. Exercise real hotkey conflict and trusted resume.
- Run C16's same-run bare Invoke control and conditional exact foreground/effect
  assertions for virtual controls, including SetValue. C16-BUTTON must pass its
  driver/BM_CLICK foreground and count assertions. Check explicit TargetedOnly
  Chromium/UWP refusal separately; semantic Invoke remains available.
- Complete Notepad Unicode write/save, Explorer navigation, Settings search, plus
  high-DPI C06, password redaction C20, and worker timeout/no replay C22.

Portable coverage remains independently verifiable with
`cargo test -p gcu-driver-windows --test input --test stop --test actions --test contract`
and worker-feature tests. `tests/input` covers Unicode/chords, drag planning,
foreground authorization, interrupted-input cleanup and no replay. `tests/stop`
covers injected/physical classification, hotkey conflict, hook loss, fail-closed
capabilities and deterministic watchdog budgets; `tests/stop/worker` covers the
parent release/kill path. `src/input/hotkey/startup` verifies keyboard-only proof
cannot arm the guard. These tests are required even when native cases Skip; they
are not claimed as physical-device evidence. `tests/actions/press_routes` covers
virtual Invoke/Legacy delivery, exact revalidation, Partial/doctor disclosure,
explicit TargetedOnly refusal and partial-dispatch cleanup without semantic fallback.

## 2026-10-17 第 4 探针真机结果

win-test 物理桌面、交互式 Default 桌面、未提权，运行被忽略的产品链测试
`native_product_probe_chain_proves_both_hooks_without_cursor_change`
（`tests/hook_probe.rs`，`#[ignore]`，会在第四探针发一次同位置绝对移动）。
启动前测试自己先睡 2500ms。进程约 2.95s 结束，唯一失败断言在该测试的第二次
`guard.available()`（源文件第 554 行，消息 `real renewal proof required`）。
完整控制台输出在编排机 `/tmp/cu2-orch/hook_probe_run1.txt`，Windows 侧副本为
`D:\cu2\hook_probe_out.txt`。以下为方案 B 实现前的原始结果。设计结论见
`computer-use/docs/adr/0013-windows-stop-guard-liveness-lease.md`（已接受）；下方新验证项
不以这次历史结果代替。

实测顺序：

1. 四次鼠标探针之后，第 4 种 `AbsoluteSamePosition` 证明成功。doctor 在第一次
   `available()` 时为 `user_activity_guard: available`、`stop_hotkey: available`。
   启动前后 `GetPhysicalCursorPos` 相等，第一个断言通过。
2. 原生快照（`armed` 之后不再更新，因此这是第一次证明的记录）：
   `mouse_hook_proof` 184ms，“tagged injected callback (attempted 4 probe kinds)”；
   `hook_SendInput` 184ms，`mode: AbsoluteSamePosition`，`dispatched: true`，
   `keyboard_seen=true`，`mouse_seen=true`，`keyboard_probe_flags=Some(144)`，
   `mouse_probe_flags=Some(1)`，`last_*` 与探针 flags 相同，
   `attempted_mouse_probe_kinds=4`。键盘 144 = `LLKHF_UP | LLKHF_INJECTED`
   （F24 抬起）。鼠标 1 = `LLMHF_INJECTED`。
   `hook_mouse_dispatch` 144ms：`sent=1/1`，Win32=0，`elapsed_ms=19`。
   `hook_keyboard_dispatch` 164ms：`sent=1/1`，Win32=0，`elapsed_ms=20`。
   `hook_install`、`hotkey_register`、`hook_desktop`（Default）均为成功。
3. 测试再空闲 250ms。第二次 doctor 为 `user_activity_guard: unavailable`、
   `stop_hotkey: unavailable (listener heartbeat expired (hook/message pump/FFI unavailable))`，
   并有 `Stop latched; trusted Host resume required`。断言失败。

和同日矩阵合在一起读：零位移三种 `SendInput` 被接受但不到达 `WH_MOUSE_LL`，
只有同位置绝对移动（以及诊断用的 `RelativePair`）能产生带标记的鼠标回调。
这次失败说明，第一次绝对移动把守卫武装起来之后，空闲并不能把租约维持过
250ms；守卫按 fail-closed 锁存了 Stop。快照里没有第二次绝对移动的派发行。
光标相等只覆盖启动那一次，不覆盖这 250ms。锁屏计时器和 hover 都没有测。

## ADR 0013 B acceptance coverage and pending native checks

Portable deterministic coverage (ManualTime/FakeSafetyApi, no Win32):

| Behavior | Test |
| --- | --- |
| Old silent-zero chain misses the lease before a 4–5-pump absolute proof completes | `legacy_zero_chain_absolute_callback_four_or_five_pumps_late_expires_before_confirmation` |
| Idle mouse lease expires with healthy pumps; activity survives, no disable/Stop | `idle_mouse_lease_expired_with_fresh_pumps_keeps_transport_and_activity` |
| No new pair means no foreground action packets; one preparation per call | `foreground_perform_missing_new_pair_refuses_without_action_input_or_latch`, `foreground_perform_expired_lease_proves_once_before_packet_assembly` |
| Live target errors precede capability/proof checks, with zero proof dispatch or action input | `action_proof_invalid_live_target_preserves_error_without_probe_or_input`, `input_backend_invalid_live_target_preserves_error_before_injection_lease_check` |
| Other/unknown virtual desktop and missing worker window refuse before proof | `action_proof_other_or_unknown_virtual_desktop_refuses_without_probe`, `worker_action_proof_missing_live_window_returns_window_gone_without_probe_or_mutation` (worker feature) |
| In-flight expiry at 100ms + 1 synchronously latches and releases held input | `foreground_inflight_missing_mouse_proof_latches_and_releases_held_input` |
| Physical callbacks update age without confirming; next action can prove | `physical_callback_during_action_updates_activity_without_renewing_lease`, `action_proof_after_physical_callback_bypasses_idle_gate_without_permanent_failure` |
| Keyboard alone and initial idle/cursor checks cannot arm or renew | `foreground_preparation_without_new_dual_callbacks_never_admits_injection`, `keyboard_only_renewal_inflight_cannot_extend_mouse_lease`, existing `absolute_idle_gate_cannot_arm_or_renew_stop_without_fresh_dual_hook_proofs` |
| Cursor change refuses before injection, latches/releases during injection, never compensates | `invalid_cursor_proof_before_injection_refuses_without_stop_and_next_action_can_prove`, `invalid_cursor_proof_during_injection_latches_and_signals_release`, `action_absolute_cursor_change_refuses_once_without_compensation_or_retry` |

`startup.rs` and
`fake_listener_ignores_runtime_elapsed_time_during_input_settle` now expect None
immediately after confirm without a physical event, replacing Some(0). The
TargetedEvent/settle assertions are preserved. Tests that expire the shared clock
without refreshing either pump still latch Stop, including
`controlled_heartbeat_expiry_latches_stop_at_the_production_boundary`.

The live-target regression assertions in `hwnd_reuse_returns_window_gone_without_substitution`,
`common_integrity_gate_precedes_every_input_action_variant`,
`elevated_engine_after_startup_refuses_discovery_and_input`,
`missing_permission_fails_closed_without_native_input`, and
`every_input_and_observation_route_enforces_live_target_gate` are unchanged.
The held-input lease-expiry test pauses after the actual left down (event 1),
asserts a pending ledger up, then requires independent left-up delivery within
500ms while the action future remains unpolled. Event 0 is a Move and holds no
input, so pausing there cannot verify release.

Prior orchestrator validation of `rcargo test -p gcu-driver-windows --locked --no-fail-fast`
reported these five error-priority failures and the held-input release assertion
failure. The source corrections and new regressions still require a rerun;
the prior failing run is not passing evidence for this version.

Verification commands (post-fix build/test commands **not_run** in this sandbox):
`rcargo test -p gcu-driver-windows`, `wcargo test -p gcu-driver-windows`,
`rcargo test -p gcu-cli`,
`rcargo clippy -p gcu-driver-windows -p gcu-cli --all-targets -- -D warnings`,
`rcargo clippy -p gcu-driver-windows --all-targets --target x86_64-pc-windows-gnu -- -D warnings`.
The worker fault/recovery suite also requires `--features test-worker-faults`.

### 2026-10-18 product-chain rerun on win-test (physical console desktop)

Build: `hook_probe` at main after the lease-B (#120) and 1s foreground proof
budget (#123) changes, run unelevated from PowerShell on the physical console
Default desktop with `GCU_DESKTOP=1`.

- Attempt 1 failed at the post-lease assertion (`listener heartbeat expired`, see
  the 2026-10-17 section); that was before ADR 0013 option B.
- Attempt 2 (post-B) failed at the **initial proof** (3s timeout, no diagnostics
  captured). The initial unsolicited cycle only sends the fourth probe after a
  single idle sample of >=2000ms, so any physical input around the run skips it.
  This explanation is consistent with the design but was **not** confirmed from
  captured output; the test now prints `doctor_issues` on that timeout.
- Attempt 3, with hands off the mouse and keyboard after Enter:
  `native_product_probe_chain_proves_both_hooks_without_cursor_change` **passed**
  (1 passed, 0 failed, 2.99s). That covers: one initial tagged dual-hook proof,
  identical physical cursor before/after, `GetLastInputInfo.dwTime` unchanged
  over the 250ms idle window, injection lease expired while transport stayed
  healthy and Stop was **not** latched.

Evidence limits: a single passing run on one machine (one physical mouse, one
Windows build); the OS build string was not captured. It does not cover the
foreground action-time proof (contract cases C06/C08/C09/C11/C15/C20/C22 and the
W-HOTKEY smoke), the hover/tooltip effect of the same-position move, physical
mouse activity adapter checks, or hook removal during held input. Those stay
**not_run** below.

The product-chain test retains `#[ignore]`; other native tests keep their
existing ignores/desktop opt-in gates. Remaining native items, on an
unattended, disposable, unelevated physical-mouse Default desktop:

- ~~Rerun `native_product_probe_chain_proves_both_hooks_without_cursor_change`~~
  **done 2026-10-18 (see above, one run)**; original wording:
  one initial tagged pair, identical physical cursor, then 250ms without a
  foreground call; injected lease expires, transport survives, Stop stays false,
  and GetLastInputInfo.dwTime stays unchanged as system idle ages.
- Measure one same-position absolute move: physical cursor before/after,
  WM_MOUSEMOVE/tooltip/hover effects, and GetLastInputInfo.dwTime. Equal cursor
  coordinates alone do not pass this check.
- Move the physical mouse while idle. Through the Host/PhysicalActivity adapter,
  require Some(age) and policy `user_active` within 3000ms with no Stop latch;
  after that threshold require a successful new proof before action input.
  The 0012 product entry remains disabled; this task provides the read-only
  adapter seam without changing require_fake or the default unavailable source.
  `engine::run_with_activity` accepts the source; `engine::run` keeps the unwired
  caller compatible by explicitly passing UnavailableActivity.
- Remove the mouse hook during a held input: latch <=100ms, emit release <=500ms;
  retain hotkey-conflict, UAC/secure-desktop and blocked-worker <=5s checks.
- Verify screen saver/lock is not delayed only with explicit consent to adjust
  timeouts on the disposable desktop. Otherwise record GetLastInputInfo only;
  actual screen-saver/lock timing stays unverified.

## 2026-10-20 physical-console contract runs on win-test: not valid product evidence

Two attempts to run the live contract table (`live::live_contract_table_c01_to_c27`, plus the
mouse-probe and background-probe diagnostics) on the win-test physical console produced **no
product result**. Both are environment limits, recorded here so nobody reads them as passes or
as driver failures.

Machine: Windows 11 Home 10.0.26200, 100% scaling, two pointer devices (HID mouse). The console
user is the built-in **Administrator (RID 500)**, whose token is High integrity even without
"Run as administrator" (no UAC filtering for the built-in account on Home).

1. **Direct run** (`contract.exe`, unelevated by intent but actually High): all 30 rows
   `not_run`, reason `elevation required`. This is the driver refusing an elevated engine, as
   designed; the fixture, worker and hooks were set up before the refusal.
2. **Run through the repo's de-elevating launcher** (`smoke-windows.ps1 -Suite Contract
   -NativeDiagnostics`, child at Medium integrity on `WinSta0\Default`): all 30 rows
   `not_run`, reason `fixture discovery: timeout … permission missing: process token query`
   (139 attempts over 30s; the window list had 147 entries but resolving the fixture process
   failed). Smoke scenarios: `scenario not reached`. In the same child, **all 12 mouse probes**
   (6 kinds × marked/unmarked) reported `sent == requested`, `win32 = 0` and **no hook
   callback**, with the cursor unchanged. The most likely cause is UIPI: the foreground
   window was High-integrity (the Administrator console), so injected input from the Medium
   child was silently dropped. That is a hypothesis, not a measurement.

What this does and does not show:
- It does **not** show a driver or contract failure: no case reached its assertions.
- It does **not** change the earlier results (hook_probe product chain passed once on this
  machine on 2026-10-18).
- These runs say nothing about the foreground contract cases C06/C08/C09/C11/C15/C20/C22 or
  W-HOTKEY; they remain **not_run**.

To obtain valid evidence the console must be a **standard, unelevated user** (for example
`claudetest`, logged in at the console) with no High-integrity window in front, running the
contract binary directly. The staged binaries and `run-live-std.ps1` live in `D:\cu2\hw`
on win-test (not in the repository).

## 2026-10-20 standard-user console run on win-test (first valid desktop evidence)

Same machine (Windows 11 Home 10.0.26200, 100% scaling), logged in at the console as the
standard user `claudetest` (Medium integrity, unelevated), `contract.exe` run directly with
`GCU_DESKTOP=1` (`live::live_contract_table_c01_to_c27`, report `contract-report-20261020-135643.json`).
Result: **6 passed, 7 skipped, 1 not_run, 16 failed** (process exit 101). Reported by the
maintainer's run; the output text was read by the orchestrator, not re-run.

| Result | Cases |
|---|---|
| passed | C01, C03, C05, C17, C21, C24 |
| skipped (reasons are the harness's own) | C02, C13, C16, C23, C25, C26, C27 |
| not_run | W-UIPI (needs `GCU_ELEVATED_PID`; no UAC automation) |
| failed, `operation cancelled` | C06, C07, C10, C12, C14, C16-BUTTON, C18 |
| failed, `unresolved Stop listener failure … foreground=unavailable` | C08, C09, C11, C15, C20, C22, W-HOTKEY |
| failed, other | C04 (pixel at grid (7,5): expected [132,140,123], got [248,248,248]); C19 (`fixture HWND PID changed`) |

**What the trace shows (facts, corrected after reading the full trace and a `SendInput` timing):**
- The `native_parent_timings` values such as `hook_SendInput = 8041` are **timestamps in
  milliseconds since listener startup, not durations**. The trace line for each probe says
  `SendInput … elapsed_ms=20`. An earlier version of this note called this "an 8 s SendInput
  stall"; that reading was wrong. A plain `SendInput` timing script on the same account
  (no hook of ours installed) gave median 0 ms, max 2 ms (mouse) and 0 ms (key), so no
  third-party input hook slows `SendInput` here.
- At startup the initial unsolicited absolute-move probe was **not sent** (`initial input
  idle unknown/below 2000ms`, the run started right after typing the command). It was dispatched
  later, at t = 8.000 s (mouse) and 8.021 s (keyboard), i.e. exactly when the first foreground
  action C06 began (`rpc_perform` starts at about t = 8.041 s).
- Immediately afterwards the trace reports `stop_listener failed: low-level hook probe expired
  (possibly silently removed)`, the worker is killed, `rpc_perform` ends `operation cancelled
  (maybe_executed=true)`, and later doctor lines carry `Stop latched; trusted Host resume
  required`. The remaining foreground cases then fail closed.
- The matching code path: `GuardEvents::confirm_hook_proofs` returns that error when the
  keyboard/mouse callback pair is older than `HEARTBEAT_TIMEOUT` (100 ms, measured from the
  **older** of the two) while `injecting()` is true. Mouse and keyboard probes are sent
  sequentially about 20 ms apart, and the pair here landed as an action started.

**Working hypothesis (unverified, to be reproduced in a deterministic test):** a late initial
probe pair overlapping the start of the first foreground action is judged stale during injection,
which is treated as a possibly removed hook and latches Stop. If that is right, the false positive
comes from the interaction between the idle-gated initial cycle and the action-time proof, not
from the machine. The 100 ms lease constant is not to be changed to make this pass.

**Also unexplained:** C04 (pixel at grid (7,5): expected [132,140,123], got [248,248,248]; the
cursor reads at x = -40 in the other session, so a monitor sits left of the primary) and C19
(`fixture HWND PID changed`).

**What it means (only what the data supports):** this is the first foreground action ever run on
physical hardware with the Stop guard active, and it failed closed in its first second. Until the
cause is reproduced and resolved, the foreground contract cases C06–C22 remain **not_run** as
evidence of correct behaviour, and Windows foreground control must not be described as working.

## 2026-10-20 re-run after the WL1 fix (same machine, `claudetest`): identical outcome

`contract-report-20261020-172403.json`, binaries built from main `879bb8a5` (WL1 merged).
Result: **6 passed, 7 skipped, 1 not_run, 16 failed**, the same split and the same cases as the
run before the fix (`…135643`). So the WL1 change did not resolve what happens on this hardware;
it fixes a different, simulated path (late initial probe pair overlapping the first action).

What the new trace shows for the first foreground action C06 (times are ms since listener startup):

| t (ms) | event |
|---|---|
| 7530 | mouse probe `SendInput` starts (takes 20 ms) |
| 7550 | keyboard probe `SendInput` starts (takes 20 ms) |
| 7571 | `rpc_perform` starts |
| ≈7640 | `stop_listener failed: low-level hook probe expired (possibly silently removed)`; worker killed; action ends `operation cancelled (maybe_executed=true)` (`rpc_perform` lasted 72 ms) |

Reading (hypothesis, not yet reproduced deterministically): the action-time proof pair takes about
40 ms to produce on this hardware (two serial `SendInput` calls of ~20 ms each; the standalone
`SendInput` timing gave 0 ms without hooks and the mouse-probe matrix 2–5 ms with them, so the
product path being ~20 ms per probe is itself unexplained). The lease is measured from the older
stamp of the pair, so by the time the injection has run ~70 ms the 100 ms lease is gone and no
renewal has arrived. The guard then fails closed and latches Stop, as designed. The 100 ms constant
is not to be raised; the work item (WL2) is to make renewal fit the budget.

Unchanged and still unexplained: C04 (pixel at grid (7,5): expected [132,140,123], got
[248,248,248]) and C19 (`fixture HWND PID changed`). Foreground contract cases C06–C22 remain **not_run**
as evidence of correct behaviour.

## 2026-10-20 re-run after the WL2 fix (same machine, `claudetest`): the lease cascade is gone

`contract-report-20261020-204834.json`, binaries from main `55d0528c` (WL1 + WL2).
Result: **13 passed, 8 skipped, 1 not_run, 8 failed** (previous run: 6 / 7 / 1 / 16).

| Result | Cases |
|---|---|
| passed | C01, C03, C05, C07, C10, C12, C14, C16-BUTTON, C17, C18, C19, C21, C24 |
| skipped (harness reasons) | C02, C13, C16, C20, C23, C25, C26, C27 |
| not_run | W-UIPI (needs `GCU_ELEVATED_PID`; no UAC automation) |
| failed | C04 (pixel at grid (7,5): expected [132,140,123], got [248,248,248]); C06 (`grid pixel coordinate: timeout`, `grid_hit` null); C08 (`Unicode bytes and newline: timeout`); C09 and W-HOTKEY (`external focused_widget: timeout`); C11 (`third popup item missing`); C15 (`drop target: timeout`); C22 (`counter focus click: timeout`) |

What changed (facts from the trace): the `low-level hook probe expired` error and the Stop latch
no longer appear. The metadata probe trace shows renewals with `lease_age_ms` of about 11–31
(the lease is 100), `call_ms = 0`, `sent=2/2`, `packet=mouse+keyup`, one round roughly every 30 ms.
This is the first physical-hardware evidence that the early-renewal change keeps the Stop lease alive
through a foreground action.

What remains: every failing case needs a real foreground pointer or keyboard effect. The actions ran
(Stop did not latch) but the fixture's oracle did not observe the expected effect within the timeout.
In the failing oracle snapshots the fixture's `key_events` array is dominated by `F24 pressed=false`
entries, one about every 33 ms: the renewal probes' F24 key-ups.

Hypotheses (unverified; two independent analyses are running):
- H1: the continuous same-position absolute mouse move of the renewal probe can race with the action's
  own pointer input and drag the cursor back to a stale position;
- H2: the F24 key-ups fill the fixture's key-event buffer and hide real key events (C08/C09/W-HOTKEY);
- H3: display layout (a monitor left of the primary; other sessions read the cursor at x = -40) explains
  C04 and perhaps coordinate-dependent cases;
- H4: the fixture window did not hold foreground focus.

Foreground contract cases other than the passing ones above remain **not_run** as evidence of
correct behaviour; C04 and C19-era display questions are not resolved.

## 2026-10-21: foreground pointer input entering the window from outside (click-matrix2/3/4)

Driver-independent `SendInput` experiments on win-test as `claudetest` (virtual screen x=-2560, w=6000,
two monitors) against the real egui fixture, counting clicks through the fixture's external state file:

- With the pointer already inside the fixture window, `Move | 40 ms | Down+Up` registers every click.
- After the pointer visited the title bar, another monitor, or the area below the window, a single
  absolute move back onto the button followed by two clicks registered **0 of 2**; waiting 300 ms and
  clicking again in place did not help. UIA `Invoke` kept working, and the foreground and
  window-under-cursor were the fixture, so the app was alive.
- A gradual in-window move (11 steps, 15 ms apart) restored clicks.
- click-matrix4 (both exits: other monitor, below window). Still 0/2: one absolute move, the same
  absolute move repeated after 50 ms, absolute move then relative ±1 px, `SetCursorPos`,
  `SetCursorPos` then an absolute move. Registered 2/2: an absolute move to a neighbouring pixel
  (+1, +1), 30 ms, then the absolute move to the target; the same with a +40 px neighbour.

Change: foreground `PointerClick` and `PointerDrag` now send an approach move one pixel from the first
target point (pointing into the window), dwell 30 ms (`plan::APPROACH_DWELL`, after the packet, no lock
held), then continue unchanged. Background (message) delivery and scroll are unchanged because no
failure was observed for them. Stop-guard semantics and constants are untouched.

Status: unit tests cover the sequence, the dwell and the in-frame offset. Real-hardware effect on the
contract table (C06, C09, C11, C15, C22, W-HOTKEY) is **not yet measured**.

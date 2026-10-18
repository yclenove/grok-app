# Disposable Windows desktop diagnostics

CI passes `-NativeDiagnostics` to `scripts/smoke-windows.ps1` to run two ignored
diagnostics inside the same verified Medium, non-admin, interactive Default
desktop as the contracts. Neither is called by the product listener. A diagnostic
failure is printed and the contracts still run. The contracts independently run
the mouse matrix **in their own process**, before starting the product listener;
only that measured evidence may permit the listener-dependent environment Skips
below. The standalone JSON artifact is diagnostic only and cannot grant a Skip.
Use only the disposable fixture desktop: the matrix includes movement and XBUTTON.

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
**absolute same position**. Each completed send without a matching injected
callback advances to the next kind; pending sends never start another dispatch.
The fourth kind sends **one absolute mouse move** to the current physical cursor
pixel with ABSOLUTE, VIRTUALDESK and NOCOALESCE. It can trigger hover processing.
It is attempted at most once in each startup/lease proof cycle, after the first
three kinds failed to produce a tagged injected callback. A successful fourth
proof starts a fresh cycle with all three zero candidates again; no callback means
no absolute retry. A working zero kind remains preferred.

`ABSOLUTE_PROBE_IDLE_MS = 2000` is an independent **fourth-send gate**, not a
source of physical activity history or Stop lease renewal. Before **any** probe
input (mouse or keyboard), the sender samples GetLastInputInfo/GetTickCount64
once on the interactive Default desktop. At least 2000ms without system input
sets the shared probe-idle marker, including before PhysicalClock's first proof.
Unknown/read failure, future input ticks, noninteractive desktop or any 32-bit
tick rollover leaves it denied. GetLastInputInfo includes injected input, so
injection **before this initial sample** conservatively denies the fourth probe.
After sampling, probes' own injected callbacks never reset the marker. Any real
physical keyboard or mouse callback (neither injected nor our tag) immediately
clears it, even if a concurrent initial read reports idle. Cleared/denied state
cannot be set again by this listener attempt; a new listener must reassess it.

The same marker only permits sending the fourth candidate, at most once per
startup/lease proof cycle. Its preservation across probe injections allows the
next cycle's sends, but **each renewal still needs fresh tagged injected callbacks
from both hooks**. Missing callbacks expire the original 100ms lease regardless
of the marker. PhysicalClock's history, dual-hook requirement, latch and lease
bounds remain unchanged. The listener never waits 2000ms, retries a refused
absolute send, extends hook_proofs' 1000ms budget, or substitutes idle for a hook
callback. Physical activity during cursor queries cancels the fourth send;
activity during dispatch invalidates its proof even if the final pixel is equal.

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
when proof is missing; foreground remains Unavailable. The count measures actual
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
  transitions, and idle-time proof renewal with the original lease limits.
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
`D:\cu2\hook_probe_out.txt`。本次不改产品代码。设计结论见
`computer-use/docs/adr/0013-windows-stop-guard-liveness-lease.md`（提议，未实现）。

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

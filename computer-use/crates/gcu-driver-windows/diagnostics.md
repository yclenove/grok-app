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

Product defaults try only zero move, zero wheel, then zero horizontal wheel.
Doctor reports `mouse hook liveness could not be proven (attempted N probe kinds)`
when proof is missing; foreground remains Unavailable. The count measures actual
submitted kinds, not the number of accepted SendInput records. Release/default
builds never select motion/buttons from a diagnostic result.

Only builds with **both** `test-worker-faults` and debug assertions accept
`GCU_HOOK_MOUSE_PROBE` (`zero_move`, `zero_wheel`, `zero_hwheel`,
`absolute_same_position`, `relative_pair`, `xbutton_pair`). The contract harness
sets it from its own matrix; deterministic preference favors zero-effect probes,
then same-position, relative motion, and finally XBUTTON. If any variant sees any
callback, environment Skip is forbidden and that probe kind is used by the real
listener. Unmarked-only/stripped-flag evidence never replaces the listener's
mandatory tagged injected callback: if the real proof still fails, the cases fail.
Paired probes check the actual requested count; a partial XDOWN attempts XUP cleanup.
Invalid debug selection fails closed. Stop leases, startup bounds, and latch
semantics are unchanged.

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

## Background activation attribution

C16 runs a bare UIA Invoke control **inside the contract process and run** on the
same fixture counter element, checking its observed RuntimeId before dispatch.
The control records foreground HWND/class/PID immediately before Invoke, immediately
after dispatch, after the exact external JSON increment, and after WM_NULL readiness.
A change during UIA resolution aborts before mutation. A successful control that
changes foreground to the fixture proves activation for that virtual element type:
only that C16 element variant Skips, with every measured identity and counter value
in its reason. A change to an unrelated process is unresolved and fails. If the
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

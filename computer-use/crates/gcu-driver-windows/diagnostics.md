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

C16 prints `ACTIVATION` identities (HWND, class, title, PID and window thread) for
the expected cover, exact fixture and actual foreground before/after dispatch and
after the fixture responds to WM_NULL. The foreground assertion stays exact.
A WinEvent observer also records EVENT_SYSTEM_FOREGROUND with notification thread
and OS time. That thread identifies the event generator, **not** the stack or
caller of SetForegroundWindow; it is not by itself proof that winit activated.

`live::unsafe_background_probe::live_background_activation_probe` compares:

1. PostMessage BM_CLICK to an owned native BUTTON, with exact WM_COMMAND count.
2. Direct UIA Invoke on the same BUTTON, bypassing the driver.
3. Direct UIA Invoke on the fixture counter, with exact external JSON increment.

Each scenario establishes the cover foreground before dispatch, then records the
result without restoring foreground. Harness preparation logs AllowSetForegroundWindow,
AttachThreadInput (with scoped detach), and, if required, a balanced Alt down/up
trick. These are test setup actions only, never product fallbacks or post-action
foreground repair. Failure/timeout to acquire the exact cover aborts before dispatch. Native comparisons require exact foreground
equality. UIA resolution and pattern acquisition are logged before Invoke; a
pre-dispatch foreground change aborts the diagnostic action. This separates
precondition/provider discovery changes from actual Invoke delivery. The separate
contract process performs the original driver path after diagnostics complete.

The fixture's opt-in egui plugin observes AccessKit action kinds and Focus commands
in frame output. It never consumes an action, removes Focus, or changes activation.
`fixture-activation-<pid>.jsonl` and native traces are uploaded and printed by the
script. Matching fixture PID + direct reproduction + stable native comparisons
can isolate a provider-specific effect. A Focus command is evidence of a request
for egui-winit's focus path; absence does not establish which other native call
activated the window. Unresolved activation remains a failure, with no speculative
Skip or weakened equality assertion.

## Background press routing

Windows background capability remains Partial (03 §5.2/5.4, P2-06). UIA
InvokePattern and Legacy default actions are not treated as non-activating delivery.
The native standard BUTTON path posts BM_CLICK to its exact validated child HWND
and returns TargetedEvent. Invoke/Legacy presses on other controls use their exact
revalidated UIA bounds through the existing TargetedOnly pointer path, including
child hit testing, stamped-window checks, parent InputLedger acknowledgements,
cancellation cleanup, and effect sampling. Each packet revalidates the exact UIA
slot/RuntimeId/metadata after queueing and the parent preparation barrier, so a
removed, moved or disabled element cannot be replaced by a click on old coordinates. Secondary Press/Confirm use the same
route. Unsupported Chromium/UWP recipients, missing bounds or absent input backend
are refused without Invoke, SetFocus, or a foreground fallback. Other semantic
patterns retain their explicit pattern routes.

Doctor tells Router/callers that Invoke/Legacy may activate and unsupported presses
require an explicitly authorized foreground pointer action. The DriverAction and
capability interfaces are unchanged. This trades arbitrary-provider Invoke coverage
for a background press path with an honest delivery label and no implicit activation.
C16 still requires the exact unrelated foreground HWND after press and SetValue,
plus exact external counter/text effects; it additionally checks targeted delivery.
The native direct-Invoke diagnostic retains its exact equality comparison, so an
activation is reported even though the product no longer uses that route.

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
- Run C16 with an unrelated foreground cover for both native BUTTON/BM_CLICK and
  the virtual fixture/targeted-message press, including SetValue. Assert exact
  foreground equality and external effects; also check Chromium/UWP refusal.
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
are not claimed as physical-device evidence. `tests/actions/targeted_press` adds
route refusal, ledger cleanup, delivery labeling and zero Invoke/activation checks.

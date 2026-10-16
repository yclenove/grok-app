# Disposable Windows desktop diagnostics

CI passes `-NativeDiagnostics` to `scripts/smoke-windows.ps1` to run two ignored
diagnostics inside the same verified
Medium, non-admin, interactive Default desktop as the contracts. Neither is called
by the product listener. A diagnostic failure is printed and the contracts still
run; it cannot convert a contract failure into Pass or Skip. Use only the CI
fixture desktop: the mouse matrix deliberately includes movement and XBUTTON.

## Mouse hook delivery

`unsafe_mouse_probe::live_mouse_probe_matrix` installs its own WH_MOUSE_LL and
keeps pumping while a separate sender calls SendInput. It tries every candidate
with a unique `dwExtraInfo` marker and with zero extra info. Each row prints sent /
requested, immediate Win32 error, send and callback elapsed time, callback flags,
and physical cursor position before/after. Full rows are saved in
`mouse-probes.json`; an interrupted row retains pending-send evidence in stdout.
A missing callback is observed for up to one second; a completed send without a
callback is not success. An unmarked callback may include unrelated input; the
marked row with `LLMHF_INJECTED` is the actual product proof requirement.

| Candidate | Intended effect / remaining side effect |
| --- | --- |
| ZeroWheel | Zero vertical delta; no intended scrolling; delivery unproven |
| ZeroMove + NOCOALESCE | No intended movement; delivery unproven |
| ZeroHWheel | Zero horizontal delta; no intended scrolling; delivery unproven |
| AbsoluteSamePosition + VIRTUALDESK + NOCOALESCE | Targets current physical pixel; cursor-query race can undo a concurrent user move; hover processing possible |
| RelativePair +1 / -1 | Visible movement/hover possible; acceleration and clipping may prevent exact restoration |
| XButtonPair | May navigate, activate a window, or trigger a global shortcut |

Product defaults try only zero move, zero wheel, then zero horizontal wheel.
No visible probe is auto-selected from diagnostic results. The same-position
candidate is implemented behind **both** `test-worker-faults` and debug assertions,
and additionally requires `GCU_HOOK_MOUSE_PROBE=absolute_same_position` in the
listener process. Release/default builds ignore this variable. An invalid debug
selection fails closed. No relative or XBUTTON production opt-in is provided.
Probe failure keeps foreground unavailable; Stop leases and latch semantics are
unchanged. Review the matrix before choosing a different default.

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
result without restoring foreground. Native comparisons require exact foreground
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

# Physical-input counter integration prototype — NOT a stock-GNOME repair

## Portal absolute-pointer mapping experiment — 2026-10-01

The installed owned GNOME portal exposed a separate Mutter 46.2 problem:
`meta_eis_client_new()` announced absolute devices before seat binding, then
`EIS_EVENT_SEAT_BIND` announced them again. The actual granted stream had two
resumed virtual absolute devices with the same nonempty mapping ID and region.
The client's rejection of this ambiguity was correct; it must not pick the
first device, collapse duplicates, guess global coordinates, or weaken consent.

`mutter-46.2-eis-region-binding.patch` delays absolute device creation until the
seat advertises that capability, ignores unchanged absolute bindings, and uses
the existing viewport replacement path when that capability changes. It touches
only the exact pinned `meta-eis-client.c`; it can coexist with the independent
physical-input counter patch. Check explicitly with:

```sh
python3 verify-patch.py /explicit/mutter-46.2 --patch-set eis-region-binding
# Add --apply only for an explicitly owned experimental source tree.
```

The same instrumented client binary failed safely before the patch (two matching
regions) and passed after it (one). A separately built default-feature client
also passed with diagnostics absent: actual portal PNG pixels located a colored
GTK target, the original snapshot drove the click, native button down/up and
click counter 0→1 were observed, and subsequent physical takeover automatically
retired the original grant. The optional `owned-acceptance-diagnostics` Rust
feature is off by default and additionally requires `GROK_CU_EI_DIAGNOSTICS=1`;
it records bounded read-only metadata, never authority or routing changes.

This is **not a supported compositor distribution or stock-GNOME repair**.
Repeated binding/unbinding, topology changes, all pointer/keyboard families,
same-process recovery, atomic stop, and real App/ACP/MCP remain separate gates.
Relative-pointer/keyboard rebinding in upstream 46.2 is not repaired by this
absolute-only patch. The experimental DSO was not installed; the owned session,
private source/DSO and stock Shell were restored before VM shutdown. See
`docs/plans/2026-10-01-computer-use-pointer-mapping-checkpoint.md` for scoped
evidence and retained failures. The production helper remains unchanged/disabled.

This directory develops the missing compositor-side observation primitive.
It is **not imported by `extension.js`, embedded by App helper management,
installed, enabled, or an accepted upstream API**. Stock GNOME46's paired physical
and EIS IME gates remain red. Do not relabel a patched-compositor result as stock
Ubuntu acceptance or remove any existing gate.

## Why this location, not another late event filter

The pinned GNOME/mutter **46.2** sources establish two distinct loss points:

1. `clutter_stage_handle_event` runs the existing FIFO event filters before
   queuing stage signals. Mutter's IME early return precedes the core idle watch.
   Appending another filter, stage capture or idle watcher cannot see that edge.
2. `meta_seat_impl_notify_key_in_impl` suppresses duplicate pressed keys, and
   `process_device_event` suppresses non-seat-wide changes. A physical press
   while the same key is held by EI can disappear even before Clutter dispatch.
   Moving a callback only to the front of Clutter dispatch is not sufficient.

The patch records **native libinput ingress**, before either suppression path.
Mutter already owns that input stream. It adds no device opens or privileges.
EIS/Clutter virtual-device requests do not traverse this ingress; IME-forwarded
logical events are not classified or heuristically discarded by this observer.
No key, text, coordinate, source path, timestamp, or event object crosses its API.
Input content and original delivery/filtering behavior remain unchanged.

## Proposed versioned read-only ABI

Experimental `MetaSeatNative` properties (not stock Mutter):

- `grok-physical-input-version`: readable uint, exactly 1.
- `grok-physical-input-generation`: readable uint64; increments for every
  non-lifecycle libinput event, including future unknown event types. NONE and
  device added/removed are excluded; separate topology/session guards remain
  mandatory. This is not an assertion that every kernel/uinput source is human.
- `notify::grok-physical-input-generation`: content-free, main-thread only,
  coalesced notification. Readback sees the synchronized input-thread counter
  before a queued notification is dispatched. No notification itself is proof
  of input; compare the actual generation.

The counter saturates at UINT64_MAX (fault), never wraps. The JS consumer rejects
unknown ABI, missing/unsafe integers, saturation/precision loss, reset and read
errors, retires its original subscription and never restores the old baseline.
Use it only after authentication of the exact compositor owner/session and
subscribe-before-snapshot; existing epoch/lock/focus/topology/heartbeat guards
must remain. This is **not an atomic compositor-side stop guarantee**.

The counter's original main-context source is bounded to one pending notification.
Each pending source owns the original emitter. Shutdown joins the original input
thread before cancelling/unreferencing that source; finalization clears the
counter. A notification has no return value that can consume or reinject input.

## Files and checks

- `mutter-46.2-physical-input.patch`: exact-version experiment, not an installer.
- `mutter-46.2-sources.json`: before/after SHA256 for all changed upstream files.
- `physical-input-counter.h`: the same C implementation carried in the patch.
- `physical-input-counter.test.c`: real GLib threads/main-context/lifetime tests,
  including delayed readback, 100,000 concurrent increments, reentrancy, pending
  cancellation, original object lifetime and saturation. Not native IME proof.
- `physical-input-watch.js` / `.test.js`: subscription consumer and deterministic
  contracts. Used by the experimental probes, not wired into the production helper.
- `physical-input-abi-check.c`: loads the actual pinned build and checks its
  native-seat GObject property types, read-only flags, bounds and notify metadata,
  without constructing a seat or starting a compositor.
- `physical-input-gi-probe.js`: explicitly gated owned-VM headless integration
  probe. Checks the actual mapped build, native seat property reads, a deliberately
  synthetic notify with unchanged generation, original unsubscribe and disposal.
  It does **not** generate or prove native physical/EI/IME input or Shell behavior.
- `physical-input-thread-audit.c`: test-only preload observer that passes the
  original GLib thread function through unchanged and verifies return from the
  exact original input `GThread*` join. OS thread names are not an identity proof.
- `probe-extension/`: a distinct, explicitly owned-VM-only Shell extension for
  native installed-session experiments. It requires an exact UUID environment
  opt-in and a root-owned read-only VM marker, and rejects headless/non-Wayland
  sessions. The owned installer copies the unchanged production `policy.js` and
  experimental `physical-input-watch.js`; neither is duplicated in this directory.
  It exports read-only observations, not input or authorization. App helper
  management never installs this UUID or these experimental entry points.
- `../overlap_acceptance.py` / `../test_overlap_acceptance.py`: strict eight-edge
  native EI/physical same-key oracle and ten regression tests. Missing, reordered,
  unaccounted, wrong-owner, blocked or wrongly delivered edges do not pass. The
  oracle alone does not prove transport provenance, human input or App revocation.

Build the standalone C contracts with pkg-config `gobject-2.0`, C11, pthread,
`-Wall -Wextra -Werror`; repeat with AddressSanitizer/UndefinedBehaviorSanitizer.
Run the JS contracts using `node --test .../physical-input-watch.test.js`.

## Verified experimental build — 2026-10-01

The full upstream Mutter 46.2 patch built successfully (773 Ninja steps, including
Meta-14 GIR/typelib) in the owned Ubuntu24.04 VM. The actual DSO passed the C ABI
check. Two final isolated GJS runs passed property/notify/readback/unsubscribe and
exact original input-thread join checks, with fatal criticals enabled. The first
probe failures, including GI API mismatch and unsafe GJS teardown, are retained;
the final probe releases Clutter wrappers before disposing the context and leaves
the context's reference owned by GJS. These are **headless ABI checks**, not IME
acceptance. The synthetic notify is explicitly required NOT to advance input.

The build was never installed; the system Mutter/Shell packages and production
helper remained unchanged, the helper remained disabled, and the original VM
process exited 0 and was joined. Upstream 46.2 is not the full Ubuntu downstream
source, and patched headless results do not certify the stock desktop.
See `docs/plans/2026-10-01-computer-use-physical-counter-checkpoint.md` for evidence.

## Installed native-session experiment — 2026-10-01

The same experimental build subsequently ran in the owned installed Ubuntu24
GNOME/GDM VM, on an actual seat0 Wayland session (not headless). Two paired runs
passed the unchanged physical and EI six-edge native GTK/IBus libpinyin oracles:
physical generation deltas `[1,1,1,1,1,1]`, EI deltas `[0,0,0,0,0,0]`, with the
expected composed Chinese commit. Two additional same-key overlap runs passed
all eight edges: EI held N while physical USB N down/up were suppressed from the
client, but the counter still advanced on both physical edges. The six EI edges
continued to deliver correctly without false takeover. All runs used the same
original Shell PID4142 / unique bus owner `:1.32`.

The first load attempt failed with mixed stock/candidate Cogl libraries. The
working harness used a private copy of the installed Shell executable with ONLY
ELF RUNPATH relocation; `.text`, `.rodata` and `.data` were compared byte-for-byte.
The exact candidate DSO/typelibs and live process maps were checked. This is an
experimental compatibility harness, **not** a supported distribution delivery or
stock-Shell repair. No system executable/library/package was replaced.

Both lock-check attempts failed before proving a lock transition: first export
readiness, then an already naturally locked session. These failures and actual
EIS capability/virtio cursor warnings remain in the evidence. Atomic input stop,
lock-transition revocation and all other input families remain unverified.

The owned session override and discoverable probe were removed. A new stock
Shell process and system library maps, disabled production helper, unchanged
package files, restored input settings and the original VM process's exit0/join
were verified. QMP USB is not human input; private Mutter EIS is not portal
consent or an App grant. Production code and the stock red baseline are unchanged.
See `docs/plans/2026-10-01-computer-use-native-counter-checkpoint.md`.

## Native ordinary controls and lock monitoring — 2026-10-01

A subsequent owned installed GNOME session (Shell PID2474, unique owner `:1.25`,
login1 session5) used the same pinned DSO and RUNPATH-only experimental loading.
Two ordinary native GTK runs passed all seven unchanged input-oracle cases:
two pointer motions, left-button down/up, wheel, and Shift down/up. Each client
event and physical counter delta was exactly one. An earlier unfocused attempt
failed before any case and is retained; setup settling was extended, not the
oracle or foreground requirement. This does not establish all input coverage.

A new strict lock oracle observed an original blocked Changed notification
before readback, but the original endpoint disappeared during the lock
transition. Continuous same-epoch readback therefore remains **failed**. The
actual installed Shell extension lifecycle includes disable/re-enable rebasing;
the source is retained as diagnostic evidence, not a captured call stack. We
did not reorder extensions or modify Shell JS to manufacture continuity.

Separately, the guarded `installed_lock_probe.rs` example exercised the real
production `GnomeNativePolicyWatch` twice through a native graphical launcher.
Each original monitor stayed alive for 350ms before explicit owned-session lock,
then ended on the login1 lock signal (observed 2ms and 3ms); LockedHint, original
Shell identity, process exit and join were verified. Host input policy remained
false throughout. This proves read-only monitor invalidation, **not App grant
revocation, atomic input stop, continuous helper lifetime or unlock recovery**.
The new strict lock oracle and its 14 regression tests preserve those boundaries.

The production helper remains unchanged and disabled. The experimental probe and
override were retired; the actual new stock Shell6991/session53 and system maps,
unchanged packages and absent endpoint were verified. Original VM30914/attempt15
exited 0, was joined, and its SSH port closed. See
`docs/plans/2026-10-01-computer-use-native-lock-controls-checkpoint.md`.

## Native pointer interleave and source retirement — 2026-10-01

The same pinned counter ran in another owned installed native GNOME session
(Shell2469 / owner `:1.25` / session5). The final same-source launcher passed
fifteen ordered pointer cases twice for deliberate EI disconnect and twice for
original EI child SIGKILL. Physical/EI motion, dragging and wheel interleave
while the EI source remains connected; two overlapping physical left-button
edges remain visible to the counter even though GTK suppresses both. EI events
never advance the physical token. Disconnect/SIGKILL while EI right is held
produces one client release, then physical motion/right down/up work normally.

The kill runs retain actual peer exit `-9` and **no** peer cleanup message;
normal disconnect runs retain exit0 and a held-button close record. Original
session Closed signals, endpoint absence and peer joins are separately checked.
A later same-Shell readback confirmed all 13 original peer/client PIDs, six
original bus owners/session paths and seven client socket directories absent.
Production helper files stayed unchanged and disabled; no App/portal grant was
created. An initial writable-peer preflight failure is retained, not relabelled
as passing; a new private build under `umask 077` preserves the original guard.

See `../POINTER-ACCEPTANCE.md` for the strict oracle, guarded peer/launcher,
fifteen regression tests and boundaries. QMP USB is not human provenance, and
ordinary USB-tablet pointer routing is not drawing-tablet/touch/pad coverage.
These are experimental-compositor results, not stock GNOME or product grant
revocation acceptance.

## Required before product use (still open)

Preserve the stock baseline and the experimental paired/overlap results above.
Retain the ordinary controls results without treating them as full coverage.
Retain the paired pointer/wheel and EI-source retirement evidence above; extend
installed coverage to capture/pad/touch/tablet/other grabs, physical device
removal and compositor-owner loss. Prove actual granted
input revocation on lock/focus/topology/owner loss and explicit fresh recovery;
the read-only monitor checks and failed continuous helper check do not do so.
Validate supported distribution delivery/update/rollback and obtain a supported
integration route; an experimental patch does not satisfy these requirements.
Only then wire a verified provider into the App's authenticated helper path.

All original platform, target, App/ACP/MCP, consent, input/clipboard, recovery,
packaging/signing/installation, UX, real Grok E4 and same-final-candidate 12h
active-soak requirements remain open. Do not treat these local unit checks as
completion of the native P0 or the user's full Computer Use objective.

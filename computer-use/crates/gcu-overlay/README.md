# Standalone Host overlay

`DesktopOverlay` owns the route A desktop UI. Open it on the helper's main
thread on macOS (ADR 0006), or a dedicated desktop thread on Windows/X11. Pass
the cloneable `provider()` to the Host's existing `OverlayProvider` notification
path. Call `tick()` at `animation::FRAME_INTERVAL` independently of action and
approval work. Provider updates are bounded memory operations; animations never
delay delivery. `hide(session)` is the turn/session-end notification; a cursor
also expires after 30 seconds of injected monotonic time. Stop remains visible
after every session is hidden.

The native owner is deliberately neither Send nor Sync. Drop destroys all UI
windows and revokes capture-exclusion availability. A failed native tick hides
windows and calls Stop. Native GUI calls are synchronous and belong on the UI
thread; the Host's existing independent hotkey/watchdog must continue to run
if that thread stalls. No task waits for an OS call on the engine action queue.

## Controls and trust

Implement `Controls` with the same persistent latch read by `UserActivity`.
`stop()` first latches synchronously, then signals the existing independent
release channel/watchdog (100ms latch, 500ms release dispatch, 5s native-worker
ceiling). Persist off the UI thread. `resume()` uses that control channel and
keeps the latch set until persistence succeeds; it must never clear in response
to model calls, turn/session transitions, reconnects or startup. The Host must
revalidate a pending approval ID in `open_approval`; that callback only opens
the existing approval window and cannot grant a request. `approval_entry`
accepts an opaque Host request ID and removes it when the request expires.

The renderer creates no native button, accessibility press action or keyboard
handler. Body, border and cursor windows pass pointer input through. The only
interactive rectangles are custom-drawn Stop/Resume and approval-entry buttons;
all windows refuse focus/activation. A matching physical down/up is required.

Stop debounces repeated clicks and the transition to Resume for 250ms, using
the injected clock. This prevents a double click on Stop from immediately
resuming. A newly cleared latch can always be stopped immediately.

| Platform | Input provenance | Capture |
| --- | --- | --- |
| Windows | `GetCurrentInputMessageSource`: hardware mouse only; injected, unavailable, keyboard and `WM_COMMAND` paths rejected | `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`, exact readback on every owned window; pre-2004 rejected |
| macOS | CGEvent HID system state with kernel source PID 0; process-posted/private sources rejected | NSPanel `sharingType=None`; capture remains the driver's target-window SCK filter |
| X11 | XI2 known enabled physical slave pointer; XTEST, unknown IDs, core SendEvent and keyboard paths rejected; hierarchy changes refresh provenance | Override-redirect ARGB windows tagged `_GCU_OVERLAY`; driver hides during a capture lease |

X11 capture publishes a fresh random nonce on a disposable InputOnly lease
window tagged `_GCU_OVERLAY_CAPTURE`. Each `_GCU_OVERLAY_HOST` coordinator
acknowledges that exact lease and nonce only after unmapping its visuals; a
missing acknowledgment times out after 100ms without capturing pixels. The UI
checks all leases before mapping and stays hidden until the last lease ends.
No operation grabs the X server, so a stalled UI cannot block independent Stop
or input release. RAII releases on errors/unwind, and connection loss destroys
the lease. Overlapping captures retain hiding until every lease ends. An
attached Host disables root-crop fallback: compositor repaint after unmap
cannot be proven by XSync. Window-pixmap capture is safe. Capturing an overlay
itself is rejected.

`Controls::healthy()` must reflect physical-input provenance, independent Stop
and watchdog health. Loss latches Stop and removes Resume; the Host's input
route gates must also close. The visual flag (`Config::visible`, populated by
the Host from `GCU_OVERLAY=0`) affects rendering only, never safety monitoring.
No control transport, credentials, real pointer movement or persistent grants
are created by this crate. Same-UID process-memory tampering remains outside the
trusted-UI boundary described in ADR 0003 and architecture §15.

## Display support and system dependencies

Native Wayland has no overlay. XWayland in a Wayland session is also disabled:
physical input and independent desktop-wide Stop cannot be verified there.
P7-01 environment detection takes `WAYLAND_DISPLAY`/Wayland session type before
`DISPLAY`; a pure X11 desktop remains eligible. `Availability` exposes the
reason key, Linux detailed capabilities/doctor give the reason, and protocol
capabilities advertise `private_apis.overlay=disabled` plus
`overlay_unavailable_no_physical_provenance_or_independent_stop=disabled`.
X11 requires an ARGB compositor, RandR monitors, Shape and XI2 physical devices.
Missing support fails initialization, with no opaque cursor fallback.

Windows uses Win32 layered windows with Direct2D/DirectWrite; macOS uses AppKit.
X11 uses system Cairo/Pango for Unicode shaping (dynamically loaded
`libcairo.so.2`, `libpangocairo-1.0.so.0`, `libpango-1.0.so.0`,
`libgobject-2.0.so.0`). These are distribution-owned shared libraries, never
bundled runtimes: Cairo LGPL-2.1-or-later OR MPL-1.1, Pango/GLib LGPL-2.1-or-later.
They are outside Cargo's static dependency graph and must remain dynamically
loaded. A missing library or symbol fails initialization before showing UI.

## Copy table and App integration

The CU workspace has no dependency on App TypeScript catalogs. Every visible
string comes from the required `Catalog` interface; native code contains no
default UI copy. `CopyKey::ALL` is the complete parity table. Empty values and
unresolved key names are rejected at native startup. Engine `StatusText` must
already be localized by the Host; it is rendered as bounded plain text, with
control and bidirectional formatting characters removed.

| App key (English authority) | Meaning |
| --- | --- |
| `computerUse.overlay.running` | Background work |
| `computerUse.overlay.foreground` | Keyboard/mouse use and physical takeover guidance |
| `computerUse.overlay.waiting` | Pending user approval |
| `computerUse.overlay.stopped` | Stopped until the user explicitly resumes |
| `computerUse.overlay.unavailable` | Stop/input monitoring unavailable |
| `computerUse.overlay.stop` | Stop button |
| `computerUse.overlay.resume` | Resume button |
| `computerUse.overlay.approval` | Open approval window |
| `computerUse.overlay.waylandUnavailable` | Overlay unavailable: physical input provenance and independent Stop cannot be verified |

An App Host resolves these keys through `createT(locale)`/`t()` in all fifteen
catalogs and supplies the resulting table at the Host boundary; this crate
does not introduce App UI or catalog keys itself. The native route A Host uses
the same complete-table adapter for its selected locale. Diagnostic reason
identifiers in capabilities are machine data; GUI presentation maps them to
the corresponding `CopyKey`.

## Verification

Portable contracts: `rcargo test -p gcu-overlay --test overlay`,
`rcargo test -p gcu-overlay --lib`, and
`rcargo test -p gcu-driver-linux --test overlay_capture`. Clocks are injected;
unit tests never sleep to prove timeouts, animation or debounce behavior.

`native_desktop` is an explicit interactive fixture probe, using a custom
harness to keep AppKit on main. Its default result is **not_run**, not evidence
of native behavior. On the matching native runner, launch a static
`gcu-fixture` window, provision its existing permissions, then run
`GCU_OVERLAY_NATIVE=1 cargo test --locked -p gcu-overlay --test native_desktop`.
The probe compares decoded screenshot pixel hashes before/after showing cursor,
foreground border and capsule, using the real platform driver. It never injects
input. It fails if the fixture/permissions/compositor is missing. Use a static
fixture scene without a blinking caret or other animation.

Physical Stop/Resume, focus retention, button hit testing, injected-input
rejection, screenshot exclusion, compositor loss and mixed-DPI visual checks
require the native interactive runners and are **not_run** in portable builds.
Do not treat cross-target compilation or synthetic provenance tests as native
GUI acceptance evidence.

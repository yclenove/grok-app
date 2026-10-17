# App overlay desktop checks

The drawing window (`cu-overlay`) has an empty input region. Its separate
`cu-overlay-controls` window covers only the capsule. Both are local documents
with no plugin capabilities. The command registry enforces the three-command
allowlist using the actual webview identity; neither window can request media,
approval, arbitrary App commands or events. Navigation and the document CSP
exclude remote documents/frames. The overlay never mounts the chat shell.

Portable contracts run with:

```
rcargo test -p gcu-overlay --test overlay
rcargo test -p gcu-host-protocol --test overlay
pnpm exec vitest run src/components/computer-use/ComputerUseOverlay.test.tsx
```

On a disposable interactive Windows or Linux X11 desktop with WebView runtime
and a built App UI, run from `src-tauri`:

```
cargo test -p grok-app computer_use::overlay -- --test-threads=1
cargo test -p grok-app native_surface_capture_flags_and_nonactivation_fixture -- --ignored --test-threads=1
```

The ignored native test opens only temporary windows, verifies the configured
capture flags and absence of activation, and injects no input. On Linux, use
X11 with a compositor; Wayland/XWayland lacks the required exclusion/input
guarantees and must leave the overlay unavailable. macOS requires the AppKit
main thread, so use the signed development App for the following desktop checks.
These native checks and the following interaction/pixel checks are **not_run**
until performed on each matching platform; portable tests do not prove them.

1. Use only the static `gcu-fixture`, two monitors with different DPI, and a
   development App with an isolated `GROK_APP_HOME`. Observe a target on each
   display, including a display left/above primary. The pointer must align with
   the fixture, the capsule must stay onscreen, and removing a display must
   relocate it without showing an offscreen pointer.
2. Keep a text field focused in a second fixture window. Show/move/hide the
   overlay and click Stop/Resume with a physical pointer. The field must keep
   keyboard focus. Clicking elsewhere must reach the underlying fixture. Stop
   must persist through an engine and App restart; Resume must require a click.
3. Compare decoded target-window pixels before/while the overlay is visible.
   Windows checks `WDA_EXCLUDEFROMCAPTURE`; macOS uses `NSWindow.sharingType=None`.
   On X11, verify `_GCU_OVERLAY_CAPTURE` leases cause both windows to unmap
   before their fresh nonce is added to `_GCU_OVERLAY_ACK`. A stalled UI must
   make capture fail, never return pixels with the overlay. X11 root fallback
   is deliberately refused while an overlay Host exists, as in route A.
4. With ordinary approval/RPC work blocked, physically Stop. Confirm the
   ≤100ms latch, ≤500ms independent release request, and existing engine ≤5s
   native-call watchdog. A stalled overlay main-thread dispatch also latches
   Stop after 500ms from the async watchdog. A capture failure cannot delay
   this control channel.

Windows requires capture exclusion support (Windows 10 2004+); an API failure
hides the overlay and latches Stop. macOS uses the public nonactivating window
style and `orderFrontRegardless`, never `makeKeyAndOrderFront`. If a desktop
fails these checks, keep its overlay disabled and report the failing platform.

# Native X11 adapter

The App's Linux desktop adapter re-exports this crate. The native probe links
the same capture, target identity and XTEST input implementation; it does not
substitute a fixture-only input driver. Computer Use remains opt-in.

Implemented here: native window capture, exact window/client coordinates,
directed pointer clicks, wheel steps, drag and 14 named navigation keys. AT-SPI
adds primary semantic clicks and Unicode text replacement/caret insertion.
Target
identity follows native window lifetime and ancestor lifetime; geometry changes
retire prior snapshots even when a window returns to its original position.

The adapter rejects focus drift, occlusion, user-held keys/buttons and unsupported
actions not advertised by the current native control. It reads the actual native modifier map so latched
CapsLock/NumLock does not look like a physically held modifier. It does not
remap the keyboard, raise unrelated windows, or substitute desktop coordinates.
Stop revokes the current generation; only native completion and owned input
cleanup release occupancy. Uncertain transport does not become idle on timeout.

AT-SPI references are opaque and snapshot-scoped. XRes supplies the native
process owner (not the writable `_NET_WM_PID` property); the accessibility bus
must have the same unique process owner and an unambiguous native window root.
Actions revalidate the exact object, ancestry, state, native geometry, focus and
held input. Protected fields are omitted. Text uses native EditableText methods
and exact readback, not the clipboard or global keyboard remapping. Preview-only
captures do not create or retire model references. Missing AT-SPI support keeps
real pixels available without inventing semantic controls or changing surfaces.

## Native verification

### Restored clipboard lifetime

Clipboard preservation uses a dedicated X connection, exact ownership epochs,
opaque leases and bounded 8/16/32-bit snapshots. It is not an implementation of
`TypeText via: clipboard`: real asynchronous paste and completion are still
unconnected. Never restore solely because a timer, Stop or SAVE_TARGETS fired.

An already-restored service can be retained by the LinuxAdapter, independently
of its action/control handles. `request_process_handoff` explicitly prepares an
independent keeper in the exact running executable (`/proc/self/exe`), before
the App may exit. The App and native probe recognize the internal keeper mode
before normal startup; do not launch that mode manually. It requires a private
inherited socket, bounded fixed-size metadata and a matching private X window.
No clipboard payload travels through arguments, files or status output.

The helper materializes the original and waits for a one-shot parent commit.
Both processes validate ownership epochs, covering copies before and after the
helper snapshot. The child claims under a short X server fence and keeps serving
after parent/channel exit. It never registers as the global CLIPBOARD_MANAGER.
Other accepted INCR readers must finish before the old service closes. Actual
child handles are reaped; control EOF and timeout are not successful transfer.
New user copies win. Limits are eight keeper processes per spawning Host process,
plus the existing snapshot/transfer bounds; no new platform dependency is added.

This is an explicit lifetime API, not yet the App's automatic quit/update UI
gate. Verified takeover protects a restored original across parent exit/crash;
crashes before successful transfer, panic recovery, installed-App packaging and
session/compositor shutdown are not claimed. Protocol proof is not isolation
from malicious clients sharing the same X server, or disk durability. The
`--clipboard` owned-Xvfb suite tests genuine subprocess exit/termination and
independent post-exit reads, not just a simulated Drop.

Run on Linux with Rust, Xvfb and xauth, from `src-tauri`:

```sh
cargo test --locked -p grok-computer-use-x11 --lib
cargo clippy --locked -p grok-computer-use-x11 --all-targets --features native-probe -- -D warnings
cargo build --locked -p grok-computer-use-x11 --features native-probe --bin cu-x11-native
env -u WAYLAND_DISPLAY XDG_SESSION_TYPE=x11 GROK_CU_X11_FIXTURE=owned-xvfb \
  timeout --kill-after=2s 30s xvfb-run -a -s '-screen 0 1280x800x24 -nolisten tcp' \
  target/debug/cu-x11-native
```

The marker is an explicit fixture opt-in, not proof that a display is disposable.
Always use the owned `xvfb-run` invocation. Never point it at a user's ordinary
desktop. The fixture creates its own windows and independently verifies native
pixels, event counts, keycodes, coordinates and zero effects on rejected input.
It tests unmap/remap, ancestor moves and deliberate XID reuse.

### Real GTK/AT-SPI acceptance

Also install `dbus-x11`, `at-spi2-core`, `python3-gi` and `gir1.2-gtk-3.0`.
Use the binary built above, with a private D-Bus session and owned Xvfb:

```sh
env -u WAYLAND_DISPLAY -u DBUS_SESSION_BUS_ADDRESS -u AT_SPI_BUS_ADDRESS \
  XDG_SESSION_TYPE=x11 GROK_CU_X11_FIXTURE=owned-xvfb \
  GDK_BACKEND=x11 GTK_A11Y=always NO_AT_BRIDGE=0 \
  timeout --kill-after=3s 45s dbus-run-session -- \
  xvfb-run -a -s '-screen 0 1280x800x24 -nolisten tcp' \
  target/debug/cu-x11-native --accessibility
```

The separate owned GTK process independently reports actual text and clicks.
Acceptance checks Unicode/emoji, forged PID properties, disabled controls,
focus drift, cancellation, stale/guessed references, same-name replacement,
preview isolation, and production Broker routing to the actionable control
rather than its enclosing container. No daily desktop or user account is used.

This proves neither installed-App behavior nor compositor/WM integration.
Other toolkits, decorated windows, IME composition, selected-text replacement,
recovery after uncertain AT-SPI completion, GNOME native Wayland, multi-monitor/scaling and
real-model/soak acceptance remain required for the full product. Native Wayland
is explicitly rejected here; XWayland is not a substitute.

## Separate native runtime seeds

The shared Core tests also execute the pinned packaged Node and import the
pinned Playwright module. Prepare a native seed first. To keep a Windows seed
unchanged while testing in WSL, use `cu-prepare-runtime --prepare --target
x86_64-linux --repo <repo> --seed <absolute-linux-seed> --cache <archive-cache>`.
The preparation checks the existing target lock, executable and whole-tree
hashes, then runs a native module import. Do not replace pinned artifacts with
system Node or relax hashes to make a test pass.

```sh
GROK_CU_TEST_SEED=/absolute/linux/seed \
  cargo test --locked -p grok-computer-use-core --lib -- --test-threads=1
```

`GROK_CU_TEST_SEED` is compiled only into unit tests. It does not change product
runtime resolution or authorize execution. With no override, tests keep using
the repository's prepared seed. Integrity and target checks remain enabled.

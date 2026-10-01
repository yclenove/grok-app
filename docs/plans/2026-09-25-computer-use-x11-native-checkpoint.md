# Computer Use — native X11 and isolated Linux runtime checkpoint

Date: September 25, 2026 (+08:00). Branch: `feat/computer-use-implementation`.
HEAD: `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`, with the existing dirty worktree
preserved. No commit, push or PR. Overall goal remains **active / partial — not
releasable**; this is not a scope reduction to Linux or native fixtures.

## Implementation

The Linux App adapter now re-exports `grok-computer-use-x11`. Production and its
owned native probe share the same safe-Rust `x11rb` implementation, replacing
the raw-Xlib image handling, root-relative coordinate mistakes and placeholder
scroll/drag path. It provides native window capture, directed clicks, wheel
input, bounded drag and 14 named keys with the actual server keymap.

Each enumerated window receives a lifetime identity, not just an XID. Native
destroy/unmap/reparent events revoke the target; configure events retire old
geometry. Ancestors are included: moving the frame back to its original position
cannot revive a stale child snapshot, and hiding/remapping the frame cannot
retain the child's authorization. Capture checks native pixel layout, RGB
masks, byte order and row padding; malformed images fail rather than becoming
synthetic black successes.

Foreground, hit-test, geometry and held-input guards run before native dispatch.
Actual lock-only modifier groups are identified from the server map; neither
NumLock's bit nor CapsLock's bit is guessed. Physically held modifiers/ordinary
keys/buttons still reject dispatch without releasing user-owned input. The
adapter never remaps keys, silently uses a semantic coordinate, or falls back
to the desktop. AT-SPI, Unicode/IME and native Wayland remain unfinished; their
capabilities are not advertised here, including semantic Wait.

`NativeActionSlot` retains occupancy until the native owner returns after
cleanup. Stop only cancels the matching older run generation. A late Stop
cannot cancel a resumed generation; uncertain transport or unwinding cannot
declare idle. At this checkpoint the helper was integrated into X11 only.
The subsequent [Windows Stop checkpoint](2026-09-25-computer-use-windows-stop-checkpoint.md)
records its Windows integration and separate evidence. macOS and complete
native recovery remain unfinished.

## Retained red → green evidence

All logs in `tools/computer-use-probe/.run/linux-native-20260925/` are retained.

- `native-acceptance-first.log`: first real Xvfb production-adapter run, 12 groups.
- `native-lock-red.log`: actual CapsLock state reproduced the mistaken held-input
  rejection. Before failing it checked all 14 keycodes and press/release pairs.
- `native-lock-green-1.log` through `-3.log`: each 14 groups, exit 0.
- `native-parent-red.log`: after the frame moved back, stale child input was
  wrongly accepted. This is a real native failure, not a mocked event assertion.
- `native-parent-green-1.log` through `-3.log`: each **16 groups**, exit 0,
  including parent hiding/remapping, same-XID replacement, exact coordinates,
  actual screenshot pixels and zero native events on rejected actions.
- `native-unit-lock-green.log`: 6/6 Linux unit tests (pixel decoding and modifier
  maps). `native-clippy-parent-and-tests.log`: strict native Linux Clippy passed
  for Core and X11, all targets including the feature-gated native probe.

The parent-green native executable SHA-256 was
`fbbb6f896b207475f63bc73c78650dd5f784d3e860b110a8eacb952429d992b0`.
After the final Core test-seed changes, `native-final-verification.log` records
another native rebuild, 6/6 X11 unit tests, strict Core/X11 all-target Clippy and
the production Linux seed check (`importProbe=passed`), all exit 0. The final
executable SHA-256 is
`d6309392c56c6538754b91d92ae0cb04e36252da66e2e8fe1f61a8d4de428ab9`.
`native-final-1.log` through `-3.log` were each read back: all **16 groups** and
the final acceptance passed, preserving the earlier red logs. The Linux seed
manifest and tree hashes below were unchanged by these runs.
The fixture uses a separate Xvfb server and owned windows; it is neither the
installed Tauri App, an ordinary user desktop, GNOME native Wayland nor a real
model session. The Windows configuration compiling an empty Linux-gated crate
is not counted as Linux evidence.

## Native runtime and Core tests

The first native Linux Core run was **484 passed / 15 failed**, not green. Its
runtime-dependent tests encountered the shared Windows seed (wrong architecture
and missing `bin/node`). No tests were skipped and no hashes were relaxed.

Prepared an independent Linux seed with the production `cu-prepare-runtime`,
using the existing `linux-x64.lock.json`. Native Node and Chromium archives were
downloaded from their pinned URLs and checked for exact SHA-256 and size. The
pinned Playwright archive was reused from the verified cache after a Windows
TLS revocation-server availability error; TLS validation was not disabled.

`GROK_CU_TEST_SEED` is a **unit-test-only** path selection. Product resolution
does not read it. It permits Windows and WSL native tests in the same worktree
without overwriting one another's seed. Missing-sibling tests clone on the
seed's own volume and unlink only their isolated copy. All existing validation
and native import assertions still execute.

- `linux-seed-prepare.log`: `target=x86_64-linux`, `importProbe=passed`.
- Linux manifest: `adba15b6d8e533d89b8dc4ecf88ec0ac91c2c1f753cd951f12856213efac61d1`.
- Linux seed tree: `081dc9c05f7456f36684cd1b7f04e411e3212e1675269ae429d3f44458607d04`.
- Chromium tree: `b203864ac3ee28fde712f33d13b90e7c67682f22207e36ff253b8b34ee2827bc`.
- `native-core-full-isolated-seed.log`: **499/499**, no ignored tests, exit 0,
  135.65 seconds of tests. Includes all six native occupancy tests, real loopback
  IPC, runtime import deadlines and the target-native deterministic prepare test.
- `windows-core-regression.log`: **499/499**, no ignored tests, exit 0,
  104.67 seconds of tests against the original Windows seed with no override.
  Its manifest SHA-256 is still
  `6ae07e971e16fa9dd0d45729b758fabc68aee944acb78cff856b59f09d7f99ab`.
- `windows-app-clippy.log`: App all-target strict Clippy with the probe feature,
  exit 0, 57.18 seconds. This checks Windows App compilation, not native UI use.
- `windows-seed-check-final.log`: the rebuilt preparation binary checks the
  original Windows seed successfully, including `importProbe=passed`. Its
  verified tree is unchanged:
  `f2bec98e39763cba243b5dce85827d76c8ae0a8ed1dd2eb3f487e6311f6e3afc`.

The local Linux toolchain is isolated in `/var/tmp/grok-cu-linux-20260925.X4zIBt`
inside Debian WSL. Rust/Clippy 1.98.0 came from official archives checked against
their published checksums. No global toolchain, proxy or ordinary browser was
reconfigured. CLI invocations and additional details are in
`src-tauri/computer-use-x11/README.md`.

CI now contains strict Core/X11 lint, Core tests and owned Xvfb acceptance on its
Linux leg after native runtime preparation. Editing that workflow is **not**
evidence that remote CI or a native Linux installer ran.

## Remaining final-version gates — original scope unchanged

1. X11 AT-SPI, Unicode/IME, native input-grab/user-intervention races, bounded
   transport recovery, compositor/WM/multi-monitor/scaling and installed-App
   acceptance. Do not generalize the Xvfb checks to these requirements.
2. Windows/macOS native occupancy and complete action semantics; full macOS AX;
   GNOME native Wayland Portal/ScreenCast/PipeWire/input integration.
3. Immediate operation-specific WebView Stop, native destruction/crash recovery,
   the retained first stale-reference failure, and macOS/Linux isolated worlds.
4. Signed Mac runtime archive delivery and installed install/update/rollback,
   permissions and signatures across every target architecture.
5. Installed acceptance of the redesigned UI/UX, including scaling, overlays,
   explicit authorization, Stop/Pause/Take Over and reconnect. The existing
   mocked-Host layout evidence is not upgraded by this X11 fixture.
6. Real Grok E4, the signed Chrome/Edge matrix, remaining lifecycle/intermittence
   checks and a frozen **12-hour active soak after the final candidate edit**.

Full history and earlier evidence remain in the resumed and WebView deadline
checkpoints. The final release goal is not achieved by this increment.

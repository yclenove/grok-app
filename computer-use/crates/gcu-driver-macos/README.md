# macOS discovery, accessibility and input (P1-02–P1-07)

This crate lists running and recently used apps, resolves an exact name, localized
name, bundle id, or path, and builds the signing identity used for later grants.
It also lists layer-0 windows and can unhide or `AXRaise` without activating the
app. The native `RealMacApi` compiles only under `cfg(target_os = "macos")`. The
same resolution, cache, alignment, and gate logic runs on Linux through
`FakeMacApi`.

`MacosDriver::new()` starts a serial discovery worker after refusing a root
engine (`DriverError::ElevationRequired`). Accessibility and Screen Recording
are checked with preflight calls only. A missing permission is
`DriverError::PermissionMissing` and this crate never passes the Accessibility
prompt option and never calls `CGRequestScreenCaptureAccess`. `list_apps` and
`resolve_app` do not need those permissions. Window listing, launch, and every
action variant do. A visible layer-0 window owned by SecurityAgent,
CoreAuthenticationUI, or UserNotificationCenter returns `SecureDesktop`.
`com.apple.systemuiserver` stays in the denylist data but is not treated as a
secure prompt: its menu bar is always on screen.

Spotlight runs on `gcu-macos-spotlight`, off the discovery worker. The first
`list_apps` does not wait for it. `discovery_status().indexing` is true until
the first result and again while a refresh runs. Results are reused for 60
seconds. Only application bundles used in the last 14 days are kept. An absolute
path is resolved even when Spotlight has not indexed it yet.

Authorization uses the bundle id as the primary key and the team id beside it.
An unsigned or ad-hoc bundle binds the canonical path, SHA-256, and cdhash.
`needs_reauth` asks again when the signature fails, the path is replaced, a
same-name certificate changes fingerprint, or the certificate, product, or build
changes. `AppIdentity.publisher` is a JSON provenance string, not a subject-name
comparison. `DriverError::AmbiguousApp` has no candidate list. Candidates stay
on `apps::ResolveOutcome::Ambiguous`.

**签名绑定不证明进程内全部代码可信；插件/可写加载路径风险仍然存在。**
A code signature does not prove that every byte running inside the process is
trusted. Plugins and writable load paths can still change that code. `trust_note`
says so on every identity. Real macOS reports `on_current_desktop: None` (no
SkyLight). Window `scale` is the display backing factor
(`CGDisplayPixelsWide / CGDisplayBounds` for the display that covers most of
the window). Screenshot pixels are physical: point size times that factor.
`Geometry.scale` records the factor. Core redaction starts `scale_to_window`
at 1 because the buffer is already physical pixels.

`snapshot_tree` reads the accessibility tree. It needs Accessibility and does
not need Screen Recording. A missing permission is
`DriverError::PermissionMissing { permission: "accessibility" }`. Pointer,
and keyboard input use the event backend. Standalone focused-element queries
return `CapabilityUnavailable`; text input independently checks fresh AX focus. Semantic actions
(`Press`, `SecondaryAction`, `SetValue`, `SelectText`, `ScrollElement`) use
the accessibility tree and do not activate the target app. `AXRaise` and
`AXFocus` are not used as a click. `ScrollElement` uses `AXScroll*ByPage`
only. `capture` and `window_geometry` are implemented.
`capabilities().observe.tree` and `screenshot` are true.
`delivery.accessibility` is true. `background_operation` is `Partial`: an
occluded window can be screenshotted, the tree can be read, and semantic
actions do not need the foreground. Native AppKit targeted input follows the
runtime field-51 probe; Chromium-class event input requires foreground permission.
`max_image_edge` is 2560; this driver does not scale. Event subscription is a
settle skeleton (`observe.events` is true). Calls have a five-second deadline.
A screenshot waits at most `DRIVER_DEADLINE` (4.75s). A tree walk also stops
at 2 seconds and 5000 nodes. A launch that has already started at timeout
reports `maybe_executed=true`. Tree reads and screenshots report
`maybe_executed=false`.

Screen Recording is checked with `CGPreflightScreenCaptureAccess` at the start
of every capture and again before pixels are returned. This crate never calls
`CGRequestScreenCaptureAccess`. A missing or mid-session revoked grant is
`DriverError::PermissionMissing { permission: "screen_recording" }`. A
minimized window is `WindowMinimized` and is not restored. A covered window
is still captured (`SCContentFilter` `desktopIndependentWindow`). Cancelling
drops the image. The driver returns a raw PNG `Capture`. Callers must pass it
through `gcu_core::redact` before a thumbnail, crop, or model encoding.

Chromium and Electron apps get `AXEnhancedUserInterface` and
`AXManualAccessibility` at the start of a live snapshot. The previous values
are written back from `release_all_input` (turn end and session close) and from
Drop. The driver only marks terminal-like names; refusal stays in the router.

Portable verification:

```sh
cargo test -p gcu-driver-macos --test identity
cargo test -p gcu-driver-macos --test discovery
cargo test -p gcu-driver-macos --test capture
cargo test -p gcu-driver-macos --test actions
rcargo --sh 'CROSS=1 bash scripts/verify.sh'
```

`CROSS=1` clippy-checks the macOS library for `aarch64-apple-darwin`. It does
not compile integration tests. macOS CI compiles those tests and runs the
portable ones. It does not run ignored tests.

Native smoke tests need an unelevated interactive desktop. Grant Accessibility
and Screen Recording in System Settings first; the driver will not prompt.
Leave another app frontmost, build the fixture, and quit any previous copy:

```sh
cargo build -p gcu-fixture
export GCU_FIXTURE_APP="$PWD/target/debug/gcu-fixture.app"
export GCU_AMBIGUOUS_NAME="Editor"   # two running apps must share this name
cargo test -p gcu-driver-macos -- --ignored --test-threads=1
```

The fixture bundle is produced by the app's bundle step when that target is
macOS. Point `GCU_FIXTURE_APP` at the `.app` you actually built. Close the
fixture afterward. Linux results are not evidence of native desktop behavior.

Ignored AX render smokes (`tests/ax_tree.rs`, macOS only) need Accessibility
and Screen Recording. Leave TextEdit, Finder, and System Settings open. The
fixture smoke also needs `GCU_FIXTURE_APP`. They are compiled on the macOS
runner and are not executed in CI:

```sh
cargo test -p gcu-driver-macos --test ax_tree -- --ignored --test-threads=1
```

Ignored ScreenCaptureKit smoke (`tests/capture.rs`, `live_sck_occluded_fixture_pixels_match`,
macOS only) needs Screen Recording. Grant it in System Settings; the driver
will not prompt and will not click the dialog. Build and launch `gcu-fixture`
(resolve name `fixture`), then cover its window completely with another
window. The test still expects the fixture canvas cell `(95, 115, 135)`:

```sh
cargo test -p gcu-driver-macos --test capture -- --ignored --test-threads=1
```

Ignored semantic-action oracle (`tests/actions.rs`, `live_fixture_semantic_actions_match_state`,
macOS only) needs Accessibility. Grant it in System Settings first; the driver
will not prompt and will not click the dialog. Build the fixture, point
`GCU_FIXTURE_APP` at the `.app`, and point `GCU_FIXTURE_STATE` at a writable
JSON path. Leave another app frontmost. CI compiles this test and does not run
it (`not_run`):

```sh
cargo build -p gcu-fixture
export GCU_FIXTURE_APP="$PWD/target/debug/gcu-fixture.app"
export GCU_FIXTURE_STATE="/tmp/gcu-fixture-state.json"
cargo test -p gcu-driver-macos --test actions -- --ignored --test-threads=1
```

The oracle presses the counter once, sets the slider near 40, refuses the
password field (`password_len` stays unchanged), toggles the checkbox, sets the
name field when it is settable, scrolls only when `AXScroll*ByPage` exists,
and checks `AXSelectedTextRange` on a fresh snapshot.

Ignored targeted-event oracle (`tests/targeted.rs`,
`live_fixture_background_click_type_and_drag` and
`live_chrome_occluded_targeted_click_is_unavailable`, macOS only) follows ADR
0006. Grant Accessibility (and Screen Recording for the window gate) in System
Settings first. The process must create `NSApplication` on the main thread:
launch the test from a GUI session (`open` or the helper), not an SSH worker
that never inits AppKit. Leave another app frontmost. Build the fixture, point
`GCU_FIXTURE_APP` at the `.app`, and point `GCU_FIXTURE_STATE` at a writable
JSON path. Chrome must already be running and occluded for the second test.
CI compiles both tests and does not run them (`not_run`). This card has no
authorized Mac, so `computer-use/bench/macos.json` is not written (`not_run`):

```sh
cargo build -p gcu-fixture
export GCU_FIXTURE_APP="$PWD/target/debug/gcu-fixture.app"
export GCU_FIXTURE_STATE="/tmp/gcu-fixture-state.json"
cargo test -p gcu-driver-macos --test targeted -- --ignored --test-threads=1
```

`CROSS=1` clippy checks the macOS library and bins for `aarch64-apple-darwin`.
It does not compile integration tests. The macOS GitHub runner compiles the
live ScreenCaptureKit code, the live accessibility actions, the live
CoreGraphics input path, and the ignored tests. The runner does not execute
those ignored tests (`not_run`).

## Text and clipboard (P1-07)

`Driver::perform(TypeText)` checks the exact window's enabled editable AX focus,
including any supplied element identity. Non-editable focus returns
`ElementNotActionable`; click an editable input field first. A plain settable field gets a complete
`AXValue` append; rich/unknown text controls use CG Unicode down/up pairs. Native
plain detection is conservative: ordinary non-Chromium text/search fields, or
controls explicitly reporting `AXIsRichText=false`. Unknown text areas retain
Unicode delivery to preserve formatting. All chunks fit 20 UTF-16 units and keep
ZWJ emoji, combining marks and CRLF intact. An individual grapheme above that
limit is rejected before posting. Uncertain writes/posts are never replayed.
Terminal-like ancestors are refused, independently of the router's policy gate.

Clipboard is off by default. The trusted host may call `perform_text` with an
explicit `TextOptions` assessment (non-sensitive text and low-risk target), an
`AllowForeground` route, and more than 2000 Unicode scalars. Unknown semantics,
password fields, sensitive payloads, high-risk targets and newline payloads
cannot take this route. Permission is per action, not a driver-wide toggle.
An existing frontmost window and editable focus are required; clipboard input
does not activate a different app. The protocol and `DriverAction` are unchanged.

A process-global lock covers save/write/validate/paste/consumption/restore across
all driver instances and sessions. Complete items and formats are preserved;
unreadable/promise formats or snapshots above 16 MiB fail before writing. Both
changeCount and content, plus fresh foreground field/window identity, are checked
before the chord and again immediately before V-down. The adapter waits for a
changed AXValue containing the payload before restoring; missing consumption
proof times out as unknown. Restoration compares the exact owned count AND full
content. Foreign changes, even to identical text, are preserved. The blocking
worker retains the lock and conditional restore guard after waiter cancellation
or timeout; queued AX jobs whose receiver has gone away do not start.

**Clipboard history and synchronization can retain text. These checks cannot
eliminate a malicious process replacing the clipboard at the last instant.**
The driver reports this limit in doctor output. Clipboard data is never observed
or logged. Timing uses an injected monotonic `InputClock` in Tokio's timer domain;
portable timeout tests use virtual time and synchronization, not wall-clock sleeps.

```sh
rcargo test --locked -p gcu-driver-macos --test text --test clipboard
rcargo clippy --locked -p gcu-driver-macos --all-targets --target x86_64-pc-windows-gnu -- -D warnings
rcargo clippy --locked -p gcu-driver-macos --all-targets --target aarch64-apple-darwin -- -D warnings
```

Native tests are macOS-only and ignored: **not_run on Linux**. They require an
unelevated GUI session with Accessibility/Screen Recording already granted.
Build the oracle example on the Mac, using the approved helper/responsible
process identity from ADR 0006. Its `main` initializes AppKit before entering
Tokio (the Rust test harness's worker thread cannot initialize NSApplication).
The clipboard oracle uses an isolated named NSPasteboard and never the user's
general clipboard. For text oracles, open a new disposable scratch document
whose title contains `gcu-text-fixture` in TextEdit or VS Code, click its ordinary
editing area, and use no accounts, existing user files, terminal/REPL/task/debug
panels. TextEdit may remain behind another app; Electron permits foreground.
The tests assert the Unicode payload in a fresh AX tree. Close scratch documents
without saving after the test.

```sh
cargo build --locked -p gcu-driver-macos --example text_smoke
export GCU_TEXT_SMOKE_BIN="$PWD/target/debug/examples/text_smoke"
cargo test --locked -p gcu-driver-macos --test clipboard live_named -- --ignored --test-threads=1
cargo test --locked -p gcu-driver-macos --test text live_textedit -- --ignored --test-threads=1
cargo test --locked -p gcu-driver-macos --test text live_electron -- --ignored --test-threads=1
```

The native clipboard oracle checks multi-format preservation, changeCount
advancement and refusal to restore after another owner replaces the payload.
It does not assert native Cmd+V consumption; the portable transaction tests
cover that handshake. Actual desktop delivery still requires the Mac runner.

For the separate native Cmd+V consumption oracle, use a disposable GUI test
session with TextEdit's blank **rich-text** scratch document frontmost and its
editor focused. Its general clipboard must contain only the fixture sentinel;
the oracle refuses any other snapshot, then verifies long Unicode text in the
AX value and sentinel restoration. This ignored test is also **not_run** here.
Do not seed or run it in a desktop containing real user clipboard data.

```sh
printf gcu-clipboard-fixture | pbcopy
export GCU_PASTE_FIXTURE=1
cargo test --locked -p gcu-driver-macos --test clipboard live_general -- --ignored --test-threads=1
```

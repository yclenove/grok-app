# macOS discovery and AX tree (P1-02, P1-03)

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
keyboard, and focus queries return `CapabilityUnavailable`. Semantic actions
(`Press`, `SecondaryAction`, `SetValue`, `SelectText`, `ScrollElement`) use
the accessibility tree and do not activate the target app. `AXRaise` and
`AXFocus` are not used as a click. `ScrollElement` uses `AXScroll*ByPage`
only. `capture` and `window_geometry` are implemented.
`capabilities().observe.tree` and `screenshot` are true.
`delivery.accessibility` is true. `background_operation` is `Partial`: an
occluded window can be screenshotted, the tree can be read, and semantic
actions do not need the foreground. Pointer and keyboard delivery cannot.
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

`CROSS=1` clippy checks the macOS library and bins for `aarch64-apple-darwin`.
It does not compile integration tests. The macOS GitHub runner compiles the
live ScreenCaptureKit code, the live accessibility actions, and the ignored
tests. The runner does not execute those ignored tests (`not_run`).

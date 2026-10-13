# macOS discovery (P1-02)

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
SkyLight) and window scale `1.0` until P1-04.

Tree, capture, geometry, focus, subscriptions, and input return
`CapabilityUnavailable` until **P1-03+**. `capabilities()` stays false for those
features. Calls have a five-second deadline. A launch that has already started
at timeout reports `maybe_executed=true`.

Portable verification:

```sh
cargo test -p gcu-driver-macos --test identity
cargo test -p gcu-driver-macos --test discovery
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

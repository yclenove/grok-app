# Windows discovery and UIA (P2-01/P2-02)

This crate implements app/window discovery, exact executable identity, launching,
nonactivating Win32 restoration and virtual-desktop annotations. The native
`RealWinApi` is compiled only under `cfg(windows)`; the same core logic and
`FakeWinApi` compile and run on Linux/macOS. Windows 10 1903+ is the target.

`WindowsDriver::new()` starts a serial discovery worker after validating the
engine token. High/System integrity or UIAccess refuses startup with
`DriverError::ElevationRequired`. A live `WinGate::check` applies to observation
and every input action variant before reporting an unavailable capability. Higher
integrity/UIAccess targets are refused; inactive/other sessions and UAC/login/lock
input desktops return `SecureDesktop`. No elevation or UIPI-failure fallback is
attempted. Unknown virtual-desktop state and `Some(false)` both prevent foreground
operations. Discovery labels are not authorization grants.

Start Menu shortcuts and registered AppsFolder packages are indexed on a separate
thread. Calls can use the existing versioned cache while `discovery_status().indexing`
is true; they never wait for a directory scan. The cache is
`%LOCALAPPDATA%\grok-computer-use\apps.json`; corruption triggers rebuilding.
Persistent directory notification handles track recursive Start Menu changes.
Cached names/paths are hints: .lnk sources and registered package manifests are
re-resolved and the executable reverified before resolution/launch. Direct
absolute `.exe` paths are also supported. Launching never runs `.lnk` arguments or
an untrusted shell command. AppsFolder activation uses only a registered AUMID
whose current manifest resolves to the authorized executable. Application startup
may choose its own focus behavior; the driver does not synthesize foreground input.

Authorization binds the canonical lower-case final path (junctions/symlinks are
resolved using an open file), SHA-256 bytes for **all** builds, signature state,
certificate SHA-256 and signed product identity. `AppIdentity.publisher` holds a
JSON provenance string containing the display subject, certificate fingerprint,
product, signature state and writable-load-path signal, within the existing
protocol type; it is not just a subject-name comparison. Unsigned images bind
path plus digest. `needs_reauth` invalidates builds, replaced paths, certificates,
products or changed writable-loading-path signals. The image is held without
write/delete sharing during hash, trust, product reads and final launch preflight.
WinVerifyTrust runs without UI/network retrieval, checks cached chain revocation,
and reads the accepted signer's certificate from its verified provider state;
absence of revocation evidence fails validation. Embedded Authenticode is checked;
catalog-only binaries are conservatively treated as unsigned.

**签名绑定不证明进程内全部代码可信；DLL/插件/可写加载路径风险仍然存在。**
Entry signatures do not attest all code loaded into a process. `trust_note`
explicitly reports the boundary and a detected writable loading path. This is a
warning signal, not a complete DLL injection/plugin audit or a trusted-host claim.
Names, icons, titles and publisher display subjects alone never identify a grant.

HWND IDs contain handle, PID, process creation FILETIME, window generation and
class. Windows does not expose a top-level window creation timestamp directly:
a driver-private per-window property assigns a discovery-time creation generation;
Windows removes it on HWND destruction, including same-class/same-process reuse.
Every operation re-enumerates the exact stamp and checks the real child process
instance for ApplicationFrameHost windows. Owned dialogs retain separate IDs and
owner metadata; ambiguous UWP child mappings fail closed.

The discovery-only `with_api` constructor leaves UIA tree/focus/subscriptions
unavailable. `new()` enables UIA observation as described below. Screenshots and
input remain unavailable in both cases. No input is held, so
`release_all_input` is a no-op. Calls have a 4.75-second caller deadline and
queued calls dropped after timeout/cancellation never execute. A native launch
already started at timeout reports `maybe_executed=true`; it must never be replayed.
P2-04 adds killable native worker-process isolation; this card uses dedicated
threads and does not claim it can terminate an OS call inside the kernel/COM.

Portable verification (build host):

```sh
rcargo test -p gcu-driver-windows
rcargo clippy -p gcu-driver-windows --lib --target x86_64-pc-windows-gnu -- -D warnings
rcargo --sh 'CROSS=1 bash scripts/verify.sh'
```

Native smoke tests require an unelevated **interactive disposable Windows desktop**,
with no real account or application data. Build the fixture and set its absolute
path, then run the three ignored tests serially:

```powershell
cargo build -p gcu-fixture
$env:GCU_FIXTURE_EXE = (Resolve-Path target/debug/gcu-fixture.exe).Path
cargo test -p gcu-driver-windows -- --ignored --test-threads=1
```

The tests launch only the fixture, system Notepad and Settings. Close them after
running and discard the desktop. Windows CI compiles these tests but its normal
`cargo test` invocation does **not** run ignored tests. Linux fake results and
cross-target clippy are not evidence of native desktop behavior.

## UI Automation trees (P2-02)

`WindowsDriver::new()` also initializes a persistent `gcu-win-uia-mta` thread.
`with_api` remains discovery-only; `with_uia` injects portable `UiaSource` and
`UiaEventSource` implementations. Tree/event capabilities are enabled only when
sources are attached. Geometry uses live stamped Win32 data. Focus is read only
inside the owning app's foreground window. Capture and every input route remain
unavailable; no pattern invocation or foreground fallback is registered.

The cache request includes names, control types, IDs, frames, enabled/offscreen/
focus/password states, value/range/selection/toggle/expand state, and pattern
**availability properties**. It never caches pattern objects. `TreeScope_Subtree`
requests use an empty descendant filter: UIA always caches the requested root,
while raw-view navigation reads singleton subtree fragments in pages of at most
64 children plus one lookahead. This avoids an unbounded provider-side subtree
allocation before the portable budgets can run. Chromium/Electron use their
normally exposed UIA tree; no browser settings or runtime are injected.

Native trees stop at two seconds, 5000 retained nodes, or 64 levels (caller limits
can be lower); core renders at its default 1200-node budget. `RawTree.incomplete`
reports Deadline/NodeCap/DepthCap/PartialError and the retained count. Exact
unread subtree sizes are unknown. Cancellation and fatal errors remain errors;
a deadline before reading the root is Timeout. Core alone redacts PASSWORD
nodes and rejects screenshot delivery from any incomplete native tree.
`IsPassword` failure suppresses value extraction and marks the tree incomplete.
UIA screen rectangles are physical pixels; the root origin is subtracted once.

Every source job has an independent 4.75-second caller deadline; COM connection
and transaction timeouts are 500ms. Abandoned queued jobs never start, and active
jobs check abandonment between native calls. Neither cancellation nor Drop joins
the MTA. A stuck provider can occupy that single thread until it returns; the
thread is never replaced with unlimited new workers. Killable process isolation
belongs to P2-04. Stop/input release does not queue behind UIA.

StructureChanged, PropertyChanged and FocusChanged each invalidate a monotonic
version and wake bounded, coalescing streams. Registrations own all three handler
interfaces. Stream Drop immediately closes its lease; the MTA unregisters every
successful Add, including partial registration rollback, on its next cleanup
pass. Focus changes conservatively invalidate every subscribed window; callbacks
retain no element/value/runtime-ID payloads. Registration failure closes the
stream. A blocked provider may delay COM cleanup, but cannot delay stream Drop.

Sensitive role/name/class/AutomationId hints mark terminal/console, IDE REPL,
task and debug execution entries, plus embedded web execution surfaces. Core's
existing TERMINAL_LIKE propagation and policy gate apply to all action paths;
an unresolved target in a mixed surface hands off. Detection is **best-effort,
non-exhaustive (尽力而为、非完备)**; a miss is not a safety proof. The mixed web
fixture includes ordinary controls, disguised execution descendants and a
password; its reviewed insta snapshots contain no password value.

Portable checks and Windows test compilation:

```sh
rcargo test --locked -p gcu-driver-windows
rcargo clippy --locked -p gcu-driver-windows --tests --target x86_64-pc-windows-gnu -- -D warnings
rcargo --sh 'CROSS=1 bash scripts/verify.sh'
```

For UIA smoke tests, manually open Notepad, a File Explorer folder, and Settings
on an **unelevated disposable interactive Windows desktop**, then run:

```powershell
cargo test --locked -p gcu-driver-windows --test uia_native -- --ignored --test-threads=1
```

These tests only read/render the system apps and drop their subscriptions. Normal
CI compiles the real implementation and tests but excludes these ignored tests.
Native desktop smoke status for this Linux delivery: **not_run**.

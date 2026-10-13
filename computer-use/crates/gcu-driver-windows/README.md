# Windows discovery (P2-01)

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

Tree, screenshots, focused elements, geometry, subscriptions and input return
`CapabilityUnavailable`/`None` until **P2-02+**. `doctor` states this explicitly and
all observation/delivery capabilities remain false. No input is held, so
`release_all_input` is a no-op. Calls have a five-second caller deadline and
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

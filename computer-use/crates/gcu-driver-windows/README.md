# Windows discovery, UIA observation, capture, semantic actions, input and isolated worker (P2-01–P2-06)

This crate implements app/window discovery, exact executable identity, launching,
nonactivating Win32 restoration and virtual-desktop annotations. The native
`RealWinApi` is compiled only under `cfg(windows)`; the same core logic and
`FakeWinApi` compile and run on Linux/macOS. Windows 10 1903+ is the target.

`WindowsDriver::new()` starts an isolated `gcu worker`; its native adapter starts
a serial discovery thread after validating the
engine token. High/System integrity or UIAccess refuses startup with
`DriverError::ElevationRequired`. A live `WinGate::check` applies to observation
and every action variant before any delivery or unavailable-capability result. Higher
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
unavailable. `new()` enables isolated UIA observation, window capture, semantic
actions and targeted/authorized foreground input as described below. The parent
independently holds the input ledger and releases pending ups after killing a
failed or stopped worker. Calls have a 4.75-second caller deadline and
queued calls dropped after timeout/cancellation never execute. A native launch
already started at timeout reports `maybe_executed=true`; it must never be replayed.
`new_in_process()` uses dedicated threads and cannot terminate a blocked OS/COM
call. Production `new()` uses the killable process boundary described in P2-04 below.

Portable verification (build host):

```sh
rcargo test -p gcu-driver-windows
rcargo clippy -p gcu-driver-windows --lib --target x86_64-pc-windows-gnu -- -D warnings
rcargo --sh 'CROSS=1 bash scripts/verify.sh'
```

Native smoke tests require an unelevated **interactive disposable Windows desktop**,
with no real account or application data. Build the fixture and worker and set
their absolute paths, then run the ignored tests serially:

```powershell
cargo build -p gcu-fixture
cargo build -p gcu-cli
$env:GCU_FIXTURE_EXE = (Resolve-Path target/debug/gcu-fixture.exe).Path
$env:GCU_WORKER_EXE = (Resolve-Path target/debug/gcu.exe).Path
cargo test -p gcu-driver-windows -- --ignored --test-threads=1
```

The tests launch only the fixture, system Notepad and Settings. Close them after
running and discard the desktop. Windows CI compiles these tests but its normal
`cargo test` invocation does **not** run ignored tests. Linux fake results and
cross-target clippy are not evidence of native desktop behavior.

## UI Automation trees (P2-02)

`new_in_process()` initializes a persistent `gcu-win-uia-mta` thread inside
the worker; production `new()` keeps it out of the parent.
`with_api` remains discovery-only; `with_uia` injects portable `UiaSource` and
`UiaEventSource` implementations. Tree/event/accessibility capabilities are enabled only when
sources are attached; background operation is `partial`. Geometry uses live stamped Win32 data. Focus is read only
inside the owning app's foreground window. Input capabilities require an attached
input backend and preserve core's route permission.
Semantic UIA delivery uses patterns without a focus/foreground fallback.

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
is implemented by P2-04 below. Stop/input release does not queue behind UIA.

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

## Window capture (P2-03)

`new_in_process()` attaches two persistent, bounded native threads inside the worker; `with_capture` injects a
portable `CaptureBackend` for tests. Screenshot capability is true only when an
attached source is available. WGC uses
`IGraphicsCaptureItemInterop::CreateForWindow`, a free-threaded one-frame pool,
and nonblocking D3D11 staging readback. Border and cursor setters are probed;
older Windows interfaces may omit them. An access-denied setter is reported as
permission missing, without requesting a grant or opening a system prompt.

WGC unavailable/native failure selects `PrintWindow(PW_RENDERFULLCONTENT)` on an
offscreen, zero-initialized DIB. Neither backend samples the desktop. Known
display-affinity restrictions return `PermissionMissing` with
`protected_window_capture`; access denied uses `window_capture`. Both are existing
protocol errors. [GetWindowDisplayAffinity](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowdisplayaffinity)
is documented to require a layered window and DWM. If that probe is unavailable,
WGC still enforces system exclusion, but PrintWindow is refused with
`window_capture_protection_unknown`: the fallback requires a confirmed WDA_NONE
before and after painting, and cannot promise support for nonlayered windows.
All-black buffers are refused with a payload-free native error
(protected or unavailable content cannot be distinguished from a truly black
window). A black WGC frame may select PrintWindow, but a black fallback never
counts as success. Permissions, cancellation, deadline, disappearance, and
minimization never trigger fallback. Minimized windows return `WindowMinimized`
without restoration. `CaptureStats` and tracing report the actual successful
path and fallback count; `Capture` and protocol types are unchanged.

GetWindowRect and UIA coordinates are physical pixels on dedicated per-monitor
DPI-aware threads. GetDpiForWindow / 96 is metadata, never another multiplier.
If WGC omits invisible resize borders, opaque padding restores the Win32 origin;
unmappable/moving/resizing frames are refused. Both paths share the total
`min(opts.timeout, DRIVER_DEADLINE)` budget (4.75 seconds), including pre/postflight
and raw PNG encoding. Cancellation abandons queued/active jobs, drops their pixels,
and reports `maybe_executed=false`. Drop never waits for PrintWindow or a wedged
OS call. In-process diagnostics cannot terminate blocked FFI; a stuck thread is not replaced
by unbounded new workers.

The PNG is unscaled and **unredacted**, solely to fit the existing driver boundary.
The observer stamps trusted state versions/timestamps. Only `gcu_core::redact`
creates deliverable images: it collects password regions before pruning, rejects
incomplete trees or stale stamps and unlocatable sensitive/login regions, and
supplies model images, thumbnails, approval crops and temporary images. Portable
tests assert this contract for WGC and PrintWindow, including fallback/pruning.

```sh
rcargo test --locked -p gcu-driver-windows --test capture
```

## UI Automation semantic actions (P2-05)

`Press`, `SecondaryAction`, `SetValue`, `SelectText` and `ScrollElement` use the
same persistent MTA as observation. `patterns::press_patterns` chooses Invoke,
Toggle, SelectionItem or ExpandCollapse by control type, with LegacyIAccessible
last. Only explicit unsupported/unavailable **zero-effect** results permit a
fallback. A timeout, cancellation, disappearance or other provider failure after
mutation dispatch is uncertain and never replayed. Value/RangeValue, Text and
Scroll never substitute for activation. Raise, ShowMenu and arbitrary native
names fail without calling SetFocus, a pointer API or foreground activation.

The worker retains each observed RuntimeId and role/name/AutomationId/class/frame
with its exact HWND. Actions share the observation generation lock and re-read
identity, exact HWND ancestry, window generation and live security before pattern
preparation and immediately before mutation. A replacement is stale, with no
similar-element search; core attaches fresh state. Execution-surface ancestors
are checked again. Writable values require `IsReadOnly=false`, a supported native
pattern, reliable metadata, and a non-password/non-execution target. Each native
Value/RangeValue pattern also checks its own live read-only property. Numeric
writes reject nonfinite/out-of-range values before mutation.

All awaits share one 4.75-second action deadline. Queued/preparing cancellation is
`maybe_executed=false`; dispatched or partially completed scrolling is unknown.
Deadline closure and dispatch arbitrate on one atomic state, preventing a timed
out job from starting a later write. A native provider already in flight can
still return after the caller; process termination remains P2-04's responsibility.
In-process mode cannot replace a blocked MTA; isolated mode retires its child
without automatically retrying a mutation.

Text selections use Unicode-scalar positions. Native endpoint movement is checked
against document text before Select; providers whose character units cannot
represent the requested scalar range are refused. Text reads are capped at
1,048,576 UTF-16 units; truncated documents are refused. Scroll supports positive
finite pages up to 100, including a final fractional page; later failure preserves
the uncertainty of earlier successful pages.

Portable contracts cover every pattern's success, explicit no-effect absence,
stale identity, timeout, cancellation and uncertain failure; actual worker tests
cover abandoned queued/preparing jobs and in-flight delivery. Run:

```sh
rcargo test --locked -p gcu-driver-windows --test actions
rcargo clippy --locked -p gcu-driver-windows --all-targets --target x86_64-pc-windows-gnu -- -D warnings
rcargo --sh 'CROSS=1 bash scripts/verify.sh'
```

Native capture status: **not_run** on this Linux delivery. Normal Windows CI
compiles the following ignored tests. Run them serially on an unelevated,
interactive **disposable Windows desktop** containing only fixture/system apps:

```powershell
cargo build --locked -p gcu-fixture
$env:GCU_FIXTURE_STATE = Join-Path $env:TEMP 'wc-grid.json'
Start-Process target/debug/gcu-fixture.exe
```

Select the fixture's **Canvas Grid** tab. For the occlusion test, cover its whole
window with another window while leaving it unminimized; the test reads the
fixture's own cell color rather than the covering window:

```powershell
cargo test --locked -p gcu-driver-windows --test capture live_wgc_occluded_window -- --ignored --test-threads=1
```

For the DPI test, set display scaling to **150%**, reset the fixture, select Canvas
Grid and keep it foreground and uncovered. The test verifies a UIA grid location's
pixel color, performs one test-only click at that screenshot pixel's physical
screen coordinate, and checks the fixture state file for exactly cell `(2,3)`.
Production input preserves the core-approved route ceiling. Launch the command with a delay,
then foreground the fixture during that delay:

```powershell
Start-Sleep -Seconds 5
cargo test --locked -p gcu-driver-windows --test capture live_capture_150_percent -- --ignored --test-threads=1
```

Close the fixture/system apps, remove `wc-grid.json`, and discard the desktop.
Fake and cross-target checks are not evidence of native pixel/click correctness.

The ignored native action contract uses the shared synthetic `gcu-fixture` and
its external state file for C05/C07/C10/C12/C16/password refusal. On a disposable,
unelevated interactive Windows desktop, build and start only the fixture:

```powershell
cargo build --locked -p gcu-fixture -p gcu-cli
$env:GCU_FIXTURE_EXE = (Resolve-Path target/debug/gcu-fixture.exe).Path
$env:GCU_WORKER_EXE = (Resolve-Path target/debug/gcu.exe).Path
$env:GCU_FIXTURE_STATE = "$env:TEMP\gcu-state.json"
Start-Process $env:GCU_FIXTURE_EXE
# Keep an unrelated visible window in front before the test starts.
cargo test --locked -p gcu-driver-windows --test actions -- --ignored --test-threads=1
```

The same contract runs in default worker mode and explicit `ExecMode::InProcess`.
Every mutation is delivered once and checked using the fixture's JSON state;
the foreground HWND must remain unchanged. These tests fail when the fixture
cannot expose the required native pattern; they never replace a semantic action
with focus/input. Other native pattern/provider combinations require matching
Windows fixtures and remain unverified here. Native action contract status:
**not_run** (Linux build host, no interactive Windows desktop). Cross-target
compilation is not native behavioral evidence.

## Native worker isolation (P2-04)

`WindowsDriver::new()` defaults to `ExecMode::Worker`. Embedders select
`with_exec_mode(ExecMode::InProcess, config)` only for explicit diagnostics;
`new_worker_with_input()` initializes the native adapter and its parent-ledger
barrier from the private startup frame.
`WindowsDriver::isolated(WorkerConfig)` supports portable trusted launchers/tests.
The supervisor contains no native UIA/capture code and starts no discovery thread.
All semantic actions, pointer clicks/scrolls/drags, chords, Unicode text and
activation execute inside the worker through the same native dispatch as
in-process mode, with no automatic mutation retries.

The private protocol uses bounded length-prefixed JSON, not bincode; see
[ADR 0005](../../docs/adr/0005-windows-worker-json.md). RuntimeIds and HWNDs are
copied integers, accompanied by exact app/window stamps and process/snapshot
generations. The Win32 generation-property namespace uses the parent's verified
process instance and remains stable across worker restarts. COM handles remain
in the child. A restart rejects old element
references. Unchanged RuntimeId/HWND/fingerprint references resolve to the latest
native slot after core screenshot/action-guard rereads. Trees are flat on the wire, retain native completeness, and captures
remain raw until the existing mandatory core redaction stage. Worker mode reports
events unavailable and uses core polling rather than registering an empty stream.

The transport watchdog is ten seconds per attempt. Crash, timeout, bad prefix,
invalid body or mismatched reply kills/reaps the child. Reads retry at most once;
launch, key-window restoration and actions never replay. Any mutation transport
loss is `maybe_executed=true` (`unknown` in core). An unusable process is restarted
without replaying mutations, immediately for read retries. Failed bootstrap is
retried by the next call. The Driver adapter retains its
4.75-second total budget. Stop bypasses the RPC queue, invalidates active/queued
calls and kills the child even during a restart handshake. Host owns the persistent
Stop latch and trusted resume; worker restart never clears it.

The **InputLedger is independently held by the parent**. Before each input packet,
the child sends its exact recipient/route and possible downs; the parent records
the corresponding ups and acknowledges before the child can send. A correlated
completion retires acknowledged ups or discards proven zero insertion. Lost
completion retains possible ups. Stop signals the watchdog outside the RPC queue,
invalidates queued calls, kills/reaps the child, then releases all pending ups on
the parent's independent up-only Win32 path. Cancellation, abandoned calls,
transport failure, shutdown and Drop use the same kill-before-release ordering;
failed ups remain pending and prevent subsequent dispatch. The parent starts no
discovery, COM, capture or input-action worker. Its only input FFI is emergency
up delivery; normal downs and activation stay in the isolated child.

The launcher clears nonessential environment, inherits only explicit standard
handles, and assigns a noninherited Windows Job with `KILL_ON_JOB_CLOSE` before
its first frame allows native initialization. Parent exit kills the job tree;
pipe EOF exits a child still waiting for that initial frame. Unix uses ordinary
kill-on-drop subprocesses for portable supervision tests. Shutdown awaits reaping;
Drop initiates cancellation without waiting for FFI. No fault API exists in the
production worker. The separate test executable requires `test-worker-faults`;
its fault code is additionally excluded by `debug_assertions` in release builds.

```sh
rcargo test --locked -p gcu-driver-windows --test worker --features test-worker-faults
rcargo clippy --locked -p gcu-driver-windows -p gcu-cli --all-targets --target x86_64-pc-windows-gnu -- -D warnings
rcargo clippy --locked -p gcu-driver-windows -p gcu-cli --all-targets --target aarch64-apple-darwin -- -D warnings
rcargo --sh 'CROSS=1 bash scripts/verify.sh'
```

Nightly soak status: **not_run**. The ignored
`observe_repeated_300_times_keeps_supervisor_alive` test performs 300 real framed
observations against disposable portable sources. The ignored Windows
`moving_window_observe_300_times_keeps_parent_alive` variant requires a disposable
interactive fixture desktop and a separately built `gcu` executable; it moves only
that fixture during observation. Native Job lifetime and `worker_restart_preserves_live_window_stamp`: **not_run** on Linux.

## Input routes (P2-06)

The isolated child (or explicit `new_in_process()`) attaches `RealInput`, a
persistent MTA input worker.
`with_input` accepts the same portable `InputBackend` with `FakeInput` for tests.
Pointer clicks, scrolling, interpolated drags, chords, Unicode text and authorized
activation are implemented; semantic UIA actions keep their existing behavior.

`Auto` and `TargetedOnly` permit **only background messages**. Hit testing walks
`ChildWindowFromPointEx` into the exact root; keyboard input uses the target
thread's retained focus or an exact observed UIA HWND. Virtual UIA elements
without an HWND cannot receive targeted messages. Chromium renderer/widget and
UWP classes return `CapabilityUnavailable` before any input. No failed,
unconfirmed or ineffective message delivery is retried or upgraded.

Only core-approved `AllowForeground` or `Activate` uses the attach/allow/set/detach
sequence and `SendInput`. The real foreground HWND is checked after activation
and immediately before every global packet. A supplied UIA focus handle is
revalidated and focused only after authorized activation. The common gate checks
engine/target integrity, session, secure desktop, window generation, geometry,
child ownership and virtual desktop. Higher integrity returns the existing
`ElevationRequired`; the driver never elevates. Physical activity monitoring and
the registered Stop hotkey belong to P2-07 and remain declared unavailable.
The independent cancellation/input-release boundary is implemented here.

The local ledger records all possible downs before insertion; in isolated mode
the parent ledger acknowledgement is also required. Partial `SendInput` packets
retain every possible up. Cancellation, failure, abandoned futures and explicit
Stop close dispatch and release ups in reverse order; isolated cleanup kills the
child first. A failed up remains pending for explicit cleanup. Proven zero
insertion discards that packet's pending downs. Queued or stalled preparation
cannot dispatch after its deadline, and every delivered input preserves
`maybe_executed` even if later evidence sampling fails.

Text is limited to 8192 UTF-16 units and sent in packets of at most 256 units,
preserving surrogate pairs and CRLF. Targeted text posts `WM_CHAR`; foreground
text batches `KEYEVENTF_UNICODE` down/up records. Chord character keys are mapped
using the recipient thread's keyboard layout. Posted modifier messages do not
change the OS keyboard state; shortcut support is provider-dependent. Window
text, control state, focus/capture state and sparse client pixels are sampled
before delivery and settled afterwards (150 ms polling, 800/1500 ms budgets).
Those samples are internal evidence, not raw screenshots exposed to callers;
core still owns the public tree/redacted-pixel diff. A stable, unchanged sample
is `NoVisibleChange`, never a reason to replay or activate. Sparse native pixels
cannot prove application/business success, especially for occluded surfaces.

Portable checks, including the existing core policy `never` gate:

```sh
rcargo test --locked -p gcu-driver-windows --test input
rcargo test --locked -p gcu-core foreground_policy_never_refuses_and_ask_confirms
rcargo test --locked -p gcu-core foreground_ask_uses_confirmation_and_never_cannot_be_overridden
rcargo clippy --locked -p gcu-driver-windows --all-targets --target x86_64-pc-windows-gnu -- -D warnings
```

The two native input tests are `#[cfg(windows)] #[ignore]`. On a disposable,
unelevated interactive Windows desktop, keep an unrelated window foreground
for the synthetic Win32 fixture click test. For the Notepad test, open only one
blank disposable Notepad document and focus its editor first; it types only
`GCU 中文输入 😀` and verifies the subsequent UIA tree. Run serially:

```powershell
cargo build --locked -p gcu-cli
$env:GCU_WORKER_EXE = (Resolve-Path target/debug/gcu.exe).Path
cargo test --locked -p gcu-driver-windows --test input live_fixture_background_message_click_keeps_foreground -- --ignored --test-threads=1
cargo test --locked -p gcu-driver-windows --test input live_notepad_foreground_chinese_preserves_text -- --ignored --test-threads=1
```

Close Notepad without saving and discard the desktop. Native input status:
**not_run** (Linux host; no interactive Windows desktop). Cross-target clippy
and portable fake tests do not establish native delivery correctness.
The tests default to Worker; set `$env:GCU_TEST_EXEC_MODE = 'in_process'` to run
the same checks in diagnostic mode, resetting the blank Notepad document before
each run. The worker path must point to the built CLI, never the test executable.

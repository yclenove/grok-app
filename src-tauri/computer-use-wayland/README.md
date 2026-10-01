# Native Wayland portal owner — in progress

`PortalSession::start` owns a private D-Bus connection and requests a user-chosen
single monitor or window through the real RemoteDesktop / ScreenCast portals.
It pins the unique portal owner, subscribes before calling, checks exact request
and session handles, validates the granted source/devices, and owns the received
PipeWire/EIS descriptors. No XWayland or D-Bus `Notify*` fallback exists.

Stop, pending-handle Drop, denied consent, negotiation deadline, portal Closed,
portal replacement, D-Bus disconnect, and either socket disconnect revoke this
lifetime and release its descriptors. Cleanup has its own bounded wait. Aborting
a caller's stop wait does not lose the exact owner JoinHandle. Parent handles
must be native `wayland:` exports (or explicitly unparented); they are never X11
window IDs. Each call needs a new run identity and explicit user portal consent.

Current policy deliberately requests `persist_mode=0` through RemoteDesktop,
never ScreenCast; it does not store, replay or infer authority from restore
tokens. Persistent restore UX/storage remains part of the overall unfinished
work. A window stream is **not** an application-isolated keyboard grant. Optional
compositor dimensions are **not** screenshot pixels; optional mapping/serial
metadata is retained, never fabricated. Mapping absence does not permit global
absolute input. The descriptors are AF_UNIX stream sockets, close-on-exec,
non-consumingly monitored, and never exposed as public clones.

## Not an enabled desktop adapter yet

This crate is a real transport/capture/input/lifetime implementation, **not**
finished native Wayland support. Default App builds still refuse Wayland and do
not advertise `native_wayland=true`. The default-off
`computer-use-wayland-preview` feature wires the App consent/factory to the same
PortalRegistry, Broker, GTK parent lifetime, and staged GNOME policy watchers.
A run-owned Host protocol bridge binds PNGs and model responses to native
frame/EI authority. Explicit helper maintenance is main-WebView-only and does
not confer desktop authority. Source wiring and private tests are not installed
GNOME/App acceptance or permission to enable the release capability.
Remaining acceptance/integration includes actual compositor lock, physical
takeover and topology; AT-SPI scope; IME/clipboard; portal-parent and restore UX;
the complete App/ACP/MCP matrix; and GNOME installed-App E4.

## Read-only session lock source

`LogindSessionWatch::connect(host_policy)` resolves this process's exact session
through the system bus `GetSessionByPID`, checks effective uid, local/user/Wayland
identity, active/unlocked state and sleep/shutdown state. It pins the daemon's
unique owner and subscribes before reading snapshots. It does not use environment
session IDs, a different seat, the caller-relative `/session/self`, or a fallback.

The returned input policy starts denied. The Host must retain and poll/await the
original `run_until` future on its granting reactor. Lock, inactive state, safety
property invalidation, removal, sleep/shutdown, daemon replacement or disconnect
revokes the watch; positive updates/unlock never revive it. Dropping/stopping the
original future also revokes it, including across a late Host policy callback.
No task is internally detached. Recovery needs a new watch and new user consent.

This is only one policy component. `LockedHint` is reported by the session, not
proof of every compositor's screen-lock behavior. A mandatory supplied Host policy
still owns portal permission and physical-user takeover; this monitor cannot be
used as an allow-all substitute. The default App factory remains disabled for
Wayland; the opt-in preview still requires native acceptance.
Private D-Bus tests cover lock/unlock initialization races, identity rejection,
owner loss, spoofed peers and Registry consent cleanup. They do **not** prove
installed GNOME, physical-input detection or the complete App consent flow.

`GnomeSessionWatch::connect(host_policy)` composes that login1 lifetime with a
read-only GNOME Shell ScreenShield subscription. It pins the same unique owner
for `org.gnome.Shell` and `org.gnome.Shell.ScreenShield`, obtains the peer UID/PID
from the session bus and checks that login1 resolves it to the same concrete
session as this process. It does not trust a shared user bus alone, the separate
`org.gnome.ScreenSaver` proxy, or environment-selected session identity.
Subscription precedes `GetActive`; even an activation/deactivation pulse during
the snapshot permanently revokes the old watch. Both original futures are owned
in place: loss of either monitor or stop/drop closes both before return, without
an internally detached task. Host input/takeover policy remains mandatory.

**GNOME Shell 49.0 explicitly delays `ActiveChanged` during the lock animation.**
The ScreenShield signal complements login1; it is not proof that the animation
window or physical-user takeover is fenced. Do not enable the App Wayland route
until a native instantaneous lock/input source and installed-session acceptance
are verified. Tests use private real D-Bus services plus owned PipeWire/EIS peers,
not the user's Shell, and never call Lock, SetActive, or change OS permissions.

## Actual CPU frame consumer

Production `start()` now creates a supervised native PipeWire owner using only
the portal-granted fd. It does **not** use a default socket, default camera,
XWayland fallback, or reconnect to a different source. A complete registry
roundtrip validates the node and portal serial when supplied; the observed
serial pins `target.object`. Removal, protocol failure, portal/EIS disconnect,
Stop and Drop retire the exact consumer before reporting `Closed`.

`frame()` returns owned run-tagged RGBA pixels from negotiated CPU buffers,
including chunk offset, row padding, packed RGB/BGR/alpha layouts, VideoCrop,
and format generations on resize. It never equates compositor coordinates with
frame pixels. Dimensions are bounded at 8192 per side / 256 MiB per plane;
DMA-only/modifier formats are not accepted as CPU memory. Missing, malformed,
corrupted or paused frames are not usable observations. Static valid pixels
retain their original delivery time; observation authority has a separate TTL;
no fallback image or black placeholder is fabricated. A portal `Granted` state
is not proof of a frame or EI input readiness. Cancellation invalidates the
stored frame immediately; a cancelled caller wait cannot lose native ownership.

This is not yet the App screenshot/action adapter: native source responsiveness,
GNOME rotation/DPI mapping and capture/input negotiation require end-to-end
validation before native Wayland is advertised.

## Actual EI input consumer

The same production supervisor owns a single native libei thread and only the
portal-granted EIS fd. `input_state()` reports negotiated, resumed virtual-device
capabilities with a generation. `input(generation, action)` supports evdev key and
pointer-button transitions, logical relative motion, exactly mapped region-local
absolute motion, smooth/discrete scroll, scroll cancellation and owned release.
Physical-device millimeters are never confused with logical-pixel inputs.
Missing/duplicate mapping IDs disable absolute input; region offsets are applied
once and physical scale is not guessed from screenshot dimensions. A window
capture still does not imply window-confined keyboard authority.

Commands use a bounded queue/deadline. Stale generations, unavailable devices,
non-finite/out-of-bounds coordinates and unowned key transitions are rejected.
Unknown execution/cancelled submission revokes input instead of replaying a
possibly executed action. Stop, Drop, portal loss, capture loss, EI loss and
topology changes during emulation retire the exact owner. It releases only its
synthetic keys/buttons and scroll on devices still able to receive input.
Removed/disconnected devices cannot receive cleanup events; compositor-side
effective-state reset must still be verified on native GNOME. Paused unused
devices require a fresh generation before use. Input readiness is independent
of portal Granted and frame availability.

The granted EIS socket is explicitly nonblocking so a silent/partial handshake
cannot prevent Stop. Native tests exposed a libei 1.3.901 crash during pre-handshake
retirement; the build now requires **static libei >= 1.5**, including the upstream
cleanup fix. Dynamic fallback with the same SONAME is forbidden, not assumed safe.
The CI/source SDK builder pins the complete 1.5.0 archive SHA256 and installs only
inside a new private prefix; it never replaces a system library.

An `InputSubmission` is a run-tagged **submission**, not an application-effect
acknowledgment. This is not Unicode/IME typing, clipboard support, an App action
adapter or permission to bypass the observation/authorization checks above.

## Frame-bound native input

`observe()` returns an opaque, one-use `ObservedFrame`, not input authority
reconstructed from supplied frame IDs. `input_observed(observation, action)`
binds the exact frame-store/input-gate owners, portal run/node/serial/mapping,
capture geometry epoch and EI device generation. Equal run strings do not share
authority. The native owner revalidates the ticket under the EI gate and holds
the frame lock through libei submission. Clear, revoke, crop/format/orientation
changes and ABA cannot race validation or restore an older ticket.

Continuing frames with unchanged geometry do not invalidate an observation on
every video tick, but cannot refresh its original delivery time or action
deadline. `frame()` retains the last valid delivered pixels until native stream
invalidation or revocation; silence is not evidence that a damage-driven source
has stopped. `CapturedFrame.captured_at` is the local **pixel delivery time**, not
a new compositor sampling time. It is never rewritten when observing/presenting.

The action lifetime is separate: `PortalOptions.observation_timeout` defaults to
60s and must be in `(0, 300s]`. This is an explicit, configurable model-response
budget, **not an image-freshness guarantee**. `ObservedFrame.issued_at()` and
`expires_at()` expose its original bounds. Each new `observe()` supersedes the
previous outstanding ticket, even for identical pixels. Passive `frame()` reads
do not. Native dispatch consumes the ticket before FFI (also on native failure),
checks its deadline, owner, epoch and EI generation under the same locks, and
never renews it when newer frames arrive. Queue execution retains its separate
two-second deadline. GAP/corrupt/empty buffers still invalidate the image and all
old authority; a later valid frame cannot undo that invalidation.

A connected, Streaming source can be static **or unresponsive**. Neither a
retained image, a successful PipeWire round trip, nor a fresh observation ticket
proves which it is, or proves that UI content stayed unchanged during inference.
The Host still needs its own observation retirement/cancellation, model budget,
scope/intent checks and post-action verification. Native GNOME static/occluded/
unresponsive-source acceptance is still outstanding; private peers are not that
acceptance. The policy does not attest or invent periodic producer heartbeats.

SPA `VideoTransform` is now explicitly negotiated and preserved (all eight
orientation values); unknown values fail closed and orientation changes advance
the capture epoch. `CapturedFrame.rgba` remains cropped, **buffer-oriented** RGBA.
Raw `input()` remains a low-level native API, not an authorized model-action entry
point. No App/ACP/MCP Wayland capability is enabled by this change.

## Presented screenshots and pixel motion

`observe()?.present(max_width, max_height)` (after handling `None`) consumes the
observation into a non-forgeable `PresentedFrame`. Its immutable `image()` has
upright, content-only packed RGBA plus the exact output dimensions. SPA metadata
describes a transform **already applied** to the buffer: presentation inverts it,
including the order of reflections/rotations. All eight transforms are handled.
Limits are 1..8192; aspect fitting only downsizes, with integer dimensions and
nearest-neighbour centre samples. No letterbox or hidden viewport translation.

`move_to_pixel(presented, x, y)` consumes that exact ticket and submits a pointer
motion to the selected pixel's centre. The indices are unsigned, zero-based and
strictly less than the **presented image** dimensions. Out-of-bounds positions
are rejected, not clamped. The upright valid content is normalized to the unique
EI region paired by mapping ID. VideoCrop's allocation offsets, padding, portal
compositor-space hints and EI physical scale are not additional translations or
scale factors. EI adds its region origin once. The EI float32 wire position is
checked against both the region and the selected screenshot pixel; a position
whose precision would escape either is rejected, never silently moved elsewhere.

Presentation never replaces or refreshes the original capture authority. Owner,
capture epoch, EI generation, supersession, cancellation and the action deadline are still
checked immediately before native submission. `move_to_pixel` is **motion only**,
not a click or an application-effect acknowledgment. Raw input remains available
for native callers. The Host protocol bridge below binds model responses, PNG
encoding and short action composition; the App broker still must implement
permission/target scope, user-takeover wiring and effect verification.
Native GNOME's content/region pairing, static-frame behavior and
real model latency remain acceptance work; private PW/EIS peers are not proof of
desktop interoperability. Window video never implies per-window keyboard scope.

## Host protocol bridge and bounded compound input

`PortalHostSession` owns one granted monitor session, an opaque target ID and
explicit Host/target generations. It reuses core `Observation`, `CaptureOptions`
and `DispatchRequest` rather than creating a parallel wire protocol. Each model
capture attempt retires its previous ticket (also on cancellation, missing
screenshot or encoding failure). Upright PNG dimensions/bytes are bound to the
same `PresentedFrame`; PNGs are aspect-downsized if needed to fit the core wire
limit. Image-pixel coordinates select the containing pixel's centre, checked
against half-open bounds and EI float32 precision, never clamped to a desktop.
An immutable clock anchor maps the original pixel delivery timestamp to UTC;
static pixels do not gain a new capture time. Preview reads do not issue,
refresh or replace model authority, and preview IDs cannot authorize input.

The bridge rejects foreign run/target/snapshot/geometry/generations, Desktop
fallback, browser authority, semantic targets and unsupported actions. Monitor
capture retains its honest **session-wide input** scope. Window-scoped Host
input is rejected instead of treating window video as confined keyboard input.
`PortalAdapter` now implements the synchronous `ComputerUseAdapter` boundary
around this bridge. App/ACP/MCP factory selection remains disabled for Wayland
until authorization/liveness integration and installed native GNOME acceptance.

Click (left/right/middle, single/double), named navigation key taps and smooth
scroll/end are short 2..8-event native sequences consuming one observation.
Zero scroll consumes validated authority but posts no input or pointer motion.
All transitions, destinations and capabilities are checked before ANY FFI
event; pre-existing owned held input prevents composition rather than releasing
someone else's synthetic state. The owner keeps its generation/frame locks
over this short bounded sequence, checks core action cancellation between
events, and neutralizes its own synthetic state after cancellation or uncertain
failure. Dropping an enqueued future still revokes native input. No automatic
retry or replay. A native receipt is only **submitted**, never proof of target
application behavior. Timed drag, text/IME, clipboard, semantic wait and effect
verification remain full-scope unfinished requirements, not silently emulated.

## Broker adapter ownership

`PortalAdapter::from_granted` consumes one run-owned `PortalHostSession` and a
Host-only `PortalInputPolicy`; there is no permissive policy default. The runtime
that granted the session must remain alive until native retirement. The adapter
does not discover/grant user sessions, export an App parent handle, or implement
lock-screen/takeover monitoring: the Host must supply that policy and revoke on
asynchronous loss. These are explicit remaining App integration requirements.

Run-scoped discovery and claim cannot leak a portal target into another run.
Broker uses a backwards-compatible generation-aware capture hook, so the actual
trusted generation is bound before encoding and agrees with Broker's normalized
observation. Actions cannot adopt a generation from their JSON. Preview never
replaces the model snapshot. Admission retires an earlier model snapshot even
when cancellation or policy denial prevents its replacement from reaching the
worker. A real native regression first demonstrated and then fixed that race.

A dedicated worker serializes bounded commands without holding shared state
through capture, input, policy callbacks or teardown. Plain/blocking threads may
wait; a multithread Tokio caller uses `block_in_place` to yield reactor capacity.
A current-thread reactor is rejected before admission rather than deadlocked.
Stop revokes native authority synchronously and wakes the owner, but idle during
retirement requires the original native owner AND adapter worker to finish and
join. A caller timeout cancels/revokes without releasing occupancy. Unknown input
completion retires the whole native session before reusing occupancy; panic or
unproven cleanup remains quarantined. Release and Drop never fabricate completion.
Pause currently retires the grant; automatic grant restoration/resume is NOT
implemented and must not be inferred from these lifecycle tests.

An owned native test now traverses the real Broker -> adapter -> PNG -> EI route,
including reauthorization, normalized geometry/generation, duplicate action IDs,
foreign runs, preview isolation and Stop. It still uses private portal/C peers,
not an installed App, ACP/MCP, real Grok inference, or native GNOME E4. Capability
`native_wayland` stays false; the original full feature/release scope is unchanged.

The same native round trip also traverses the production `HostOwnedAdapter`
wrapper. It now forwards scoped discovery/claim and the trusted capture
generation instead of falling back to unscoped methods. Cancelled model captures
reach native ticket retirement unchanged; private-worker admission failure fences
and retires the native owner rather than retaining stale model authority. Local
ownership binds the wrapper instance plus each run; foreign/unscoped release
cannot clear another run, and backend switching waits for actual cleanup. Pending
claim/release callbacks hold no registry mutex and cannot publish late authority.
Idle queries are fenced by the exact reservation revision: a stale native idle
result cannot retire cleanup started during that query, including a same-run,
same-target release/reclaim ABA. Neither polling nor elapsed time proves cleanup.
Multiple independently retained runs remain supported where the native adapter
authorizes them; this wrapper does not manufacture cross-run portal permissions.
The App factory still needs a run-owned portal registry and permission/runtime
integration; exercising its wrapper is not evidence of installed-App enablement.

Capture retirement now destroys the stream/listeners before an ordered
`core.sync` roundtrip on the original portal-granted PipeWire connection. This
applies to capture errors as well as Stop: closing the local FD alone does not
prove the daemon has processed stream destruction. The wait belongs to the
original native owner, so cancelling a Stop waiter cannot bypass it. Frames are
revoked before this wait. A two-second deadline or daemon disconnection joins
and closes local resources but records a failed close reason, **not** proof of
remote retirement. No default socket, new capture authority or infinite wait is
introduced. Native regressions pause only their retained private daemon to prove
delayed acknowledgement, cancelled Stop, deadline and loss behavior.

## Verification

On native Linux with Rust, `dbus-daemon`, libclang and the PipeWire/SPA development
headers installed (PipeWire >= 0.3.65), plus Meson/Ninja/Python Jinja2. Native tests
also require GTK >= 3.24, labwc, Xwayland, iproute2 and util-linux `unshare`.
See `docs/BUILD.md` for the unresolved Ubuntu 22.04 release-SDK boundary.

```sh
# Use a NEW absolute prefix; retain share/licenses/libei/COPYING on distribution.
CU_WORK=$(mktemp -d /var/tmp/grok-cu-wayland.XXXXXX)
bash scripts/build-computer-use-libei.sh "$CU_WORK/sdk"
export PKG_CONFIG_PATH="$CU_WORK/sdk/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export LD_LIBRARY_PATH="$CU_WORK/sdk/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
bash scripts/check-computer-use-linux-sdk.sh
cargo test --manifest-path src-tauri/Cargo.toml -p grok-computer-use-wayland --locked -- --test-threads=1
cargo clippy --manifest-path src-tauri/Cargo.toml -p grok-computer-use-wayland --all-targets --locked -- -D warnings

# Native tests use private daemons/sockets and owned C pixel/EIS peers only.
cc -std=gnu11 -Wall -Wextra -Werror src-tauri/computer-use-wayland/tests/pw-source.c \
  $(pkg-config --cflags --libs libpipewire-0.3) -o "$CU_WORK/cu-owned-pw-source"
cc -std=gnu11 -Wall -Wextra -Werror src-tauri/computer-use-wayland/tests/eis-server.c \
  $(pkg-config --cflags --libs libeis-1.0) -o "$CU_WORK/cu-owned-eis"
# Resolve the exact Cargo artifact, not a stale glob match. Native executables
# cannot live in /tmp: the namespace launcher intentionally replaces /tmp.
cargo test --manifest-path src-tauri/Cargo.toml -p grok-computer-use-wayland \
  --locked --lib --no-run --message-format=json > "$CU_WORK/cu-native-build.jsonl"
TEST_BINARY=$(python3 -c 'import json,sys; rows=[json.loads(s) for s in open(sys.argv[1])]; paths=[r["executable"] for r in rows if r.get("reason")=="compiler-artifact" and r.get("executable") and r.get("target",{}).get("name")=="grok_computer_use_wayland"]; assert len(paths)==1; print(paths[0])' "$CU_WORK/cu-native-build.jsonl")
GROK_CU_PW_SOURCE="$CU_WORK/cu-owned-pw-source" GROK_CU_EIS_SERVER="$CU_WORK/cu-owned-eis" \
  bash scripts/run-computer-use-wayland-tests.sh --test-binary "$TEST_BINARY" \
    --evidence "$CU_WORK/evidence" --test-threads 4
```

The runner requires a **new** evidence directory and an actual private headless
compositor. It runs the complete suite with no ignored/filtered tests, requires
the GTK-parent-to-registry integration, preserves raw protocol and peer logs,
joins original child owners, and fails on timeout or residual descendants.
PID/network/mount namespaces are mandatory; no ambient desktop/bus fallback.
If a CI runner restricts user namespaces, elevate only this isolated launcher,
not global AppArmor/sysctl settings. This does not grant any product permission.

The tests start their own private bus and serve the real D-Bus signatures, with
actual Unix descriptors transferred through SCM_RIGHTS. They emit responses
before method replies, revoke sessions, replace owners, interrupt waits and
assert peer EOF after cleanup. They do not access the logged-in user's portal
or control any desktop. Explicit native tests additionally exercise real PipeWire
transport/pixels and libei/libeis wire events, resize/crop, exact serial/mapping,
eight inverse orientations with asymmetric pixel oracles, scaled image centres,
float32 wire coordinates and invalid/stale presented-frame rejection,
input generations, pause/resume, physical-unit rejection, held-key cleanup,
concurrent owners, cancelled submission/Stop and joint capture-loss retirement.
The private portal and manually linked source are **not** GNOME session-manager
autoconnection, desktop E3/E4, or final-candidate 12h soak. libeis 1.3.901's pause
API does not emit pause during EMULATING; tests exercise real neutral-device
pause plus active-device removal/disconnect instead, not an inert pause call.

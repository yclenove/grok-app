# Computer Use — native Wayland portal ownership

Date: September 30, 2026 (+08:00). Overall goal **active / partial — not releasable**.
Original full platform, surface, App/ACP/MCP, input/IME, signed installation,
repair/rollback, native UI, real Grok E4 and same-final-candidate 12h gates remain.

## Current work — S6.2 in_progress

The current tree has a real X11 backend, which explicitly refuses Wayland.
There is no native portal/PipeWire/libei desktop adapter yet. This phase adds
the actual D-Bus portal negotiation and its lifetime owner, not a fake adapter
or a capability toggle. The user-selected grant must not become model input
authority until frame geometry and the exact EIS device region are bound.

The portal transport/lifetime slice is now implemented and locally verified.
S6.2 as a whole remains **in_progress**: the App's Linux desktop path still uses
X11 and refuses native Wayland. No capability is enabled from transport-only
evidence. Tests use a private D-Bus daemon and owned service/socket fixtures,
not the user's desktop portal. WSLg is not GNOME and cannot satisfy native E4.
Existing Linux toolchain: `/var/tmp/grok-cu-linux-20260925.X4zIBt` in Debian WSL.

Reference XML was retrieved from the upstream `flatpak/xdg-desktop-portal`
repository into `tools/computer-use-probe/.run/wayland-portal-20260930/`.
No real installed Grok, registry, signing, publication or release changes.

## Production implementation

New workspace crate `src-tauri/computer-use-wayland` implements a run-owned
asynchronous portal connection. RemoteDesktop >= 2 and ScreenCast >= 3 are
checked against the pinned unique bus owner before requesting consent. Every
request subscribes before invoking the portal, verifies its exact returned
path, and uses the same preselected session identity. The creation result is
parsed as the standard's string-typed session handle, not an object path.

The order is RemoteDesktop CreateSession / SelectDevices, ScreenCast
SelectSources, RemoteDesktop Start, ScreenCast OpenPipeWireRemote, then
RemoteDesktop ConnectToEIS. The grant must contain exactly one source of the
requested kind and exactly keyboard/pointer devices. PipeWire serial, optional
EI mapping ID and compositor/logical geometry are preserved without inventing
pixel dimensions or global coordinate authority. Window capture is explicitly
labelled as still having session-wide input permission.

Both received SCM_RIGHTS descriptors must be connected AF_UNIX stream sockets;
close-on-exec is enforced. A non-consuming native hangup check revokes the
lifetime if either transport closes. Stop, Drop during consent, denial,
timeout, portal Closed, owner replacement and bus disconnect all retire owned
resources. Cancelling the caller's Stop wait retains the original JoinHandle;
subsequent Stop still waits for actual owner retirement. Cleanup never follows
a different request/session path and never changes to XWayland or Notify input.

Current consent policy is explicitly nonpersistent (`persist_mode=0`), set only
on RemoteDesktop. No restore token is saved or replayed. Persistent restore
UX/storage and the eventual Host lifecycle binding are not claimed complete.

## Current evidence

All evidence lives in `tools/computer-use-probe/.run/wayland-portal-20260930/`.

- Final native Wayland crate: **17 passed / 0 failed / 0 ignored**, repeated
  three times on the exact compiler-reported executable (one serial run and
  two four-thread runs). Includes real private D-Bus requests/signals, FD
  transfer/EOF, nine explicit error paths, request cancellation, source scope,
  unique-owner replacement, spoofed signal rejection, partial allocation,
  socket/bus loss, and interrupted retirement waits. This is E2/native-IPC
  evidence, not actual PipeWire/EI protocol execution or GNOME acceptance.
- Existing native X11 crate: **49 passed / 0 failed / 0 ignored** in the joint
  feature build, then repeated on its exact compiler-reported executable.
- Existing owned Xvfb fixture: **19 PASS groups**; actual GTK/AT-SPI fixture:
  **41 PASS groups**. Both native acceptance summaries passed. They remain
  X11-only regression evidence; no clipboard fixture was rerun this phase.
- Strict native Linux Clippy for Wayland/X11 all targets and the X11 probe
  passed with `-D warnings`. Windows App library `cargo check --offline
  --locked` passed. Workspace formatting and `git diff --check` passed.
- CI now schedules the private-bus Wayland tests under a truthful label.
  No remote CI run is claimed. No frontend or full Core suite was rerun here.
- **298 source files** were frozen before the final exact-executable tests.
  The previous phase's **116 selected sources** were checked: only workspace
  manifest, lockfile and CI changed. Removing the new local package block from
  Cargo.lock reproduces the previous lockfile hash exactly; no pre-existing
  dependency was upgraded.

Final Wayland test executable SHA-256:
`ee8c57a5e7877f4bfa80c8dec74e66e877faf4b7631d39ed7ebd29e01db7b107`.
Final X11 test executable SHA-256:
`eb797283bc0e639975e68376fb0f2d2db4fa6e716fa73c004a7aa4e9559f43a4`.
The test durations are seconds; none is a 12h soak.

### Preserved failed iterations

`first-test.log` is **5 passed / 6 failed**: the fixture's generated D-Bus method
was incorrectly capitalized `ConnectToEis`, not `ConnectToEIS`. The fixture was
fixed, and negative assertions now require the exact expected failure reason.
Those earlier generic-error passes are not used as proof of EIS refusal.

`native-test-2.log` is **1 passed / 13 failed** after adding unconditional portal
activation: the real private bus rejects StartServiceByName without a service
file even when that name already has an owner. Production now resolves first,
activates only after NameHasNoOwner, then pins the resulting unique identity.
`native-test-3.log` passed all 15 then-existing tests; the final two descriptor
tests and unchanged X11 executor features are included in the 17-test runs.
The dormant real-GNOME activation path still needs real-desktop validation.

## Next implementation — original goal unchanged

1. Add real PipeWire frame consumption with bounded buffers, supported native
   formats, serial/node lifetime checks, resize/revocation and real frame proof.
2. Add libei handshake, seat/device/region readiness, exact mapping binding,
   owned press/release cleanup and no input while suspended or unmapped.
3. Bind both consumers, per-run consent and observed geometry to the Host Linux
   adapter, then wire parent/permission/restore UI without widening scope.
4. Finish scoped AT-SPI, Unicode/IME/clipboard and lock/topology behavior; run
   Ubuntu GNOME native E3/E4 (both Wayland and Xorg), not a WSLg substitute.
5. Preserve all remaining Windows/macOS, surface/App/ACP/MCP, signed clean
   install/update/repair/rollback/uninstall, native UI, real Grok and final
   frozen-candidate 12h requirements. This phase is not final release approval.

Previous goal turn: **progress**. This goal turn: **progress** (new production
code and native regression evidence), not a wait or a restated status. No
completion/blocked/paused transition is justified; goal remains active.

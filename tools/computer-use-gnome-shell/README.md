# GNOME native policy helper (explicit preview; default off)

An [unwired compositor counter prototype](compositor/README.md) now targets the
loss point before libinput seat-wide/duplicate suppression and IME filtering.
It requires an experimental Mutter change: **it is not a stock-GNOME fix**, is
not installed or embedded by the App, and does not change either red IME gate.

**Partial native-input repair; release gate remains closed.** The previous late
`Clutter.Event.add_filter` missed all seven ordinary native-client input cases.
`NativeEventWatch` now subscribes to Mutter 46's synchronous core user-active
callback and inspects the actual `Clutter.get_current_event()` before normal
client routing consumes it. Two installed-GNOME runs now pass all seven cases:
consecutive motion, button down/up, scroll and key down/up. This is **not idle-time
polling**: no event context is a sticky fault, never evidence of physical input.
The one-shot is re-armed synchronously and removed on disable/error. The late
filter remains supplementary; duplicate loss generations do not grant permission.

**Earlier input-capture, IME filtering and mapped tablet-pad paths can still return
before this callback.** Full physical-takeover protection is not established.
The installed native Wayland/libpinyin fixture confirms the IME gap: the original
classifier observed 0/6 edges. The current logical-source correction observes
3/6 (releases), but all three consumed presses remain unobserved. Moreover, two
actual native EIS/libpinyin runs reproduce the same generation pattern: the
three forwarded virtual releases wrongly advance physical-loss generation.
Neither accepting all logical events nor excluding all null-node events meets
both contracts. Ordinary-client 7/7 is a historical control, not IME coverage.
See [IME-ACCEPTANCE.md](IME-ACCEPTANCE.md); never count this red gate as skipped.
The additional [paired native EIS gate](EI-IME-ACCEPTANCE.md) retains the physical
gate unchanged and requires all six virtual edges to be delivered without loss.
Do not enable this preview for real automation or remove the release gate.
Complete EI exclusion, touch/tablet/grab/IME, App grant revocation and stop timing
remain required; unit tests or the seven ordinary client cases cannot replace
them. See the [observer checkpoint](../../docs/plans/2026-09-30-computer-use-native-event-observer-checkpoint.md).

This read-only GNOME Shell extension supplies **loss information**, not a desktop
grant. It exports `NativePolicy1.GetState/Changed` on Shell's own D-Bus connection.
The Host pins the existing Shell/login1 identity and subscribes before snapshot.
No keys, text, coordinates, device names or `/dev/input` paths go over this API.
It never opens an input node, steals an event, injects input, captures pixels,
changes a permission, replaces a Shell method or auto-installs an extension.

Mutter **46.0** source establishes that EIS virtual devices are also tagged
`InputMode.PHYSICAL`. Use the native source device's compositor-owned `device-node`
property instead: kernel libinput devices have `/dev/input/eventN`; native virtual
devices have null. A null node alone is insufficient: logical aggregate keyboards
also have no node. The classifier additionally requires the actual GI physical-mode
enum on a direct native device before virtual exclusion; LOGICAL, missing, invalid
or throwing mode information is unknown, not virtual. This is a provenance repair,
**not an early-event delivery repair**. Unknown native input changes the loss generation; unknown
device implementations fail closed. Synthetic-event flags/device names/idle-time
values are deliberately not used as source identity. This distinguishes direct
physical from direct native virtual events, not IME-forwarded logical events,
nor one authorized EI client from every other possible virtual input client.

The observer sees `ScreenShield.actor` visibility and resolved session-mode state
before the public delayed ActiveChanged signal. Ubuntu's inherited user mode is
accepted by resolved capabilities rather than hardcoding a session-mode name.
Each enable creates a new epoch. Physical events delivered to this observer and
lock/unlock transitions advance the serial. A Host grant cannot survive a
generation/epoch change, but the unobserved early-return paths remain a safety gap.
Disable/error is sticky; the Host's bounded heartbeat catches a missing helper.

## Integration and limitations

- `GnomeNativePolicyWatch` owns this signal subscription and the original
  `GnomeSessionWatch`; stop/drop/abort close all three sources, not detached tasks.
- Readback every 100 ms, 250 ms reply deadline, 500 ms maximum freshness. A stalled
  reactor or late response does not restore the old grant. These are liveness
  bounds, **not an atomic compositor-side stop or zero post-lock input guarantee**.
- Native consent selection itself involves physical input. The staged
  `connect_for_consent` watcher permits serial advancement only while no input
  grant exists; lock/session/helper faults still revoke. `GnomePolicyActivation`
  consumes a one-shot checked snapshot barrier after explicit portal consent.
  Registry owns the same watchers through activation and joined retirement;
  its GNOME-specific parented selection is used by the Linux GTK bridge. No
  active grant intentionally ignores observed physical input. This does not
  repair the early-return coverage described above. The App factory/consent command is wired
  behind `computer-use-wayland-preview`, which is default off. Real GNOME
  consent UX and complete installed-helper input acceptance remain open.
- Metadata targets GNOME 46 source APIs. Actual Ubuntu 24.04 GNOME helper loading,
  real libinput/EIS differentiation, lock timing, target identity/focus/topology,
  session PID binding and recovery remain required acceptance gates. A private
  GJS/GI/D-Bus probe is **not** that acceptance.
- Preview settings expose localized, explicit install/repair/enable/disable and
  read-only status. No startup copying, download, global extension toggle or
  automatic enable occurs. Default/non-Linux builds cannot mutate the helper.
  Do not silently copy this directory into an existing user's extensions.
- Other compositors are not supported by this helper; no XWayland fallback.

## Explicit App management

Turn off Computer Use before maintenance; the Host retains its feature-transition
lock and joins existing portal owners first. The actual main WebView is required;
these commands are not registered as model, MCP or mirror tools. A process-wide
operation plus per-user file lock survives cancelled IPC waiters.

The Host derives `$XDG_DATA_HOME/gnome-shell/extensions` (or
`$HOME/.local/share/gnome-shell/extensions`) itself. It publishes only embedded
`metadata.json`, `extension.js`, `policy.js`, an ownership marker and a restart
record. Symlinked/untrusted ancestors, unowned installs and unexpected files are
refused, not overwritten. A durable transaction journal supports explicit repair
of known interrupted publication slots. A corrupt/unowned journal remains a
visible conflict requiring manual review. This is not a sandbox against another
malicious process running as the same user.

Installation/repair requires a new Shell process incarnation (boot ID + bus-
authenticated PID + process start time), not merely a new bus owner, bus reconnect
or App restart. Sign out/back in, refresh, then explicitly Enable. GNOME 46's
`ReloadExtension` is unsupported and is never used. Management pins the same-UID
`org.gnome.Shell` owner at `/org/gnome/Shell`; a successful enable/disable settings
reply is followed by state/helper readback, not treated as completion itself.
GNOME can retain extension `ERROR` after Disable has changed its resolved
`enabled` setting to false. Retirement therefore requires **both** a disabled
setting in an allowed inactive state and an absent helper endpoint; `ERROR`
alone, an accepted D-Bus reply, or an unreadable endpoint is not retirement.
After confirmed retirement, Repair is available, but cached error/outdated/
uninstalled states still require a new Shell process before Enable. Repair
never makes cached GJS modules executable or revives a desktop grant.
Status does not grant input: login1/native monitor and separate portal consent
remain mandatory. An unknown/late result is shown as unconfirmed, never retried
automatically. Original Host install/relogin/enable/disable/repair now pass in an
owned installed Ubuntu 24.04/GNOME46 Wayland VM using the GA Mesa comparison;
these tests do not exercise renderer IPC, an App grant, or the updated Mesa
configuration. Native UX, session binding and complete input protection still
require acceptance. A newly created installation hierarchy is private (0700)
even under login umask 0002; existing writable ancestors remain rejected and
are never chmod'ed into acceptance.

## Developer checks (no login-desktop changes)

`node --test tools/computer-use-gnome-shell/policy.test.js`

`probe-gjs.js` additionally checks actual GJS exported-object/GVariant transport
and GI API availability on a **fresh private** D-Bus session, with an explicit
test-only opt-in. Its generated input sources are declared fixtures, not hardware.
Use an owned SDK and private namespace; never reuse a logged-in Shell bus.

`gnome_helper_probe` is an opt-in Rust example for the **production** fixed-UUID
management client. The preview App's three `native::live_shell_tests` are manual
ignored probes using the original Host manager, not renderer IPC mocks. Execute
them only in their owned PID/mount/network/private-D-Bus fixture, with isolated
`/tmp/cu-home` and `/tmp/cu-runtime`; the environment/marker checks are accidental
invocation guards, not a security sandbox. Never opt in on a login-desktop bus.

The private real GNOME 46 run covers discovery, original Host publication,
negative enable when GDM/ScreenShield is absent, confirmed disable of sticky
ERROR, explicit repair, new-process readback, and old-owner rejection. It does
**not** prove successful activation or installed Ubuntu/GDM, physical takeover,
lock timing, portal consent, or native App UX. Missing ScreenShield must continue
to fail closed; do not stub GDM or relax the helper guard to pass this probe.

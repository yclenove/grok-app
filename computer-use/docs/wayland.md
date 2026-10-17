# Wayland

Linux Wayland is a declared, partial surface. This page is the user note for
that surface. It records what the Linux driver, the overlay, the ADRs, and
[STATUS.md](../../docs/plans/computer-use-v2/STATUS.md) actually show. A sentence
marked **not verified** (未验证) has no passing run on a real portal or a real
Wayland desktop. Headless GNOME (`mutter`, `xdg-desktop-portal-gnome`, PipeWire)
is the same: [scripts/wayland-ci.sh](../scripts/wayland-ci.sh) prints `not_run`
when those programs are missing, and a missing runner is not a pass
([P7-05](../../docs/plans/computer-use-v2/STATUS.md)).

<a id="en"></a>

## English

### Support boundary

The product does not claim full Wayland support.

- The shipping `gcu` binary assembles only the offline fake driver. Any other
  driver value is rejected with exit 2 and the text `Real driver assembly is not
  implemented` (`computer-use/crates/gcu-cli/src/args.rs`). `gcu doctor` in
  [cli.md](cli.md) therefore reports the fake desktop. It does not probe
  `xdg-desktop-portal`.
- AT-SPI observation and AT-SPI semantic actions are marked available on X11,
  XWayland, and native Wayland. The accessibility bus is the reliable action
  path in [architecture §5.3](../../docs/plans/computer-use-v2/03-architecture.md#53-linux).
- Native Wayland screenshots and pointer/keyboard input exist as library code
  behind portal traits. `LinuxDriver::capture` still returns
  `CapabilityUnavailable` for a native Wayland window. Pointer, drag, scroll,
  keys, and `type_text` on that window do the same unless a `PortalInput`
  session was attached for that exact window. The CLI never attaches one.
- A real GNOME, KDE, or wlroots portal dialog, PipeWire frame, and libei device
  are **not verified**. STATUS records P7-02, P7-03, P7-04, and P7-05 real
  desktops as `not_run`. Portable contract rows are fake or in-memory tests.

### How a session is classified

`detect_environment` reads the process environment. Empty values are ignored.

| Variable | Effect |
| --- | --- |
| `WAYLAND_DISPLAY` set, or `XDG_SESSION_TYPE=wayland` | Wayland session |
| Wayland session and `DISPLAY` set | `wayland+xwayland` |
| Otherwise `DISPLAY` set, or `XDG_SESSION_TYPE=x11` | `x11` |
| None of the above | `unknown`. Doctor `ready` is false |

`WAYLAND_DISPLAY` wins over `XDG_SESSION_TYPE=x11`. The overlay uses the same
order ([overlay README](../crates/gcu-overlay/README.md)).

Compositor family comes from `XDG_CURRENT_DESKTOP`, then `DESKTOP_SESSION`,
then `SWAYSOCK` (Sway), `HYPRLAND_INSTANCE_SIGNATURE` (Hyprland), or
`KDE_FULL_SESSION` (KDE). Names are split on `:` and matched as substrings, in
this order: `gnome`/`mutter`; `kde`/`plasma`/`kwin`; `sway`/`hyprland`/`river`/
`wayfire`/`wlroots`/`phosh`; `xfce`/`lxqt`/`mate`/`cinnamon`/`cosmic`/`pantheon`.
Anything else is `unknown`. Doctor then says window enumeration and geometry
may be inaccurate. `controlled_capabilities` can promote native input only for
GNOME, KDE, and wlroots, and only after the runtime checks below. `other` and
`unknown` stay on the static table.

The published capability quad is one target for the whole driver: a Wayland
session with an X server is described as an XWayland window, even when some
windows are native. A window id that is not an X11 id, and that starts with
`/` or `wayland`, is treated as native inside a Wayland session.

### Capability matrix

Static table from `build_environment_capabilities`, before a live `PortalInput`
promotes anything. "Portal present" means the session-bus probe saw
`org.freedesktop.portal.Desktop` or one of the three interfaces below.

| Capability | Pure X11 | XWayland window | Native Wayland window |
| --- | --- | --- | --- |
| Accessibility tree | AT-SPI2 over D-Bus | same | same |
| Semantic actions (Action, SetValue, SelectText, Text, Value) | available | available | available. These calls do not open a portal |
| Targeted per-window pointer/keys | unavailable on Linux | unavailable | unavailable |
| Screenshot in the capability table | `XComposite` + `XGetImage` when the extension exists; otherwise degraded, occluded windows fail | `XComposite` when the extension exists | available only when the portal service and `org.freedesktop.portal.ScreenCast` are both present |
| What `LinuxDriver::capture` does | X11 capture | X11 capture when the window has an X id | `CapabilityUnavailable`. The ScreenCast pipeline is not called |
| Pointer and keyboard | XTEST when the global Stop chord is registered and Stop is clear | same gate; the chord itself is only partial | static table: unavailable, reason text says portal RemoteDesktop + libei integration is pending. Promotion is a separate runtime gate |
| Global Stop | `XGrabKey` of Ctrl+Alt+Shift+Backspace when registration succeeds | partial: that grab sees the chord only while an X11 surface is active | unavailable until a GlobalShortcuts listener is registered and stays healthy. Seeing the interface is not registration |
| User-activity guard | XInput2 on the root window when the listener is up | unavailable | unavailable in the static table |
| Status capsule / overlay | partial on a pure X11 session (ARGB compositor, physical XI2 device; hidden during capture) | unavailable for the whole Wayland session | unavailable |
| Background operation in the protocol struct | `partial` while semantic actions are available | `partial` | `partial` |
| `overlay_excluded_from_capture` | false (X11 hides the overlay during capture instead) | false | false |

Doctor `ready` is true only when AT-SPI is connected and the session type is
not `unknown`. A Stop latch does not, by itself, clear `ready`. It does disable
X11 and XWayland foreground input.

On a native Wayland target the driver text for a missing piece is:

- Portal service missing: doctor contains `xdg-desktop-portal is not available on D-Bus session bus`. Next step text: install `xdg-desktop-portal` and a desktop backend (`xdg-desktop-portal-gnome`, `xdg-desktop-portal-kde`, or `xdg-desktop-portal-wlr`).
- ScreenCast interface missing: native capture unavailable. Next step text: install or configure the compositor screencast backend.
- RemoteDesktop interface missing: native input unavailable. Same class of next step.
- GlobalShortcuts interface missing: the global Stop chord cannot be registered across native Wayland surfaces. Next step text names GNOME 44+ and KDE 6+. That sentence is diagnostic text. Those desktop versions are **not verified**.
- GlobalShortcuts interface present: the static reason is still `integration is pending (P7-03)` until `controlled_capabilities` promotes the row.

Those package names are the driver's own next-step strings. This page does not
add an install command. Whether a distro package by that name provides a working
backend is **not verified**.

`controlled_capabilities` promotes native foreground input, the user-activity
guard, and the global Stop row together, and only when all of these hold:
compositor is GNOME, KDE, or wlroots; screenshot status is available; a
registered Stop listener is healthy; and the input authority reports the
session confined to the authorized window. The promoted reasons are
"Registered desktop-wide Stop listener with independent watchdog",
"Trusted compositor confines input to the authorized window and monitors
physical activity", and "User-approved RemoteDesktop session with stream-paired
EI devices". Losing the listener, the confinement proof, or the portal session
drops `PortalInput::available`, and the driver publishes the static table
again.

### First portal authorization

Two portal clients exist. Neither is started by `gcu` today. Both ask the
portal for `PersistMode::ExplicitlyRevoked` and may pass a restore token that
has already passed the local checks. The system dialog that a GNOME or KDE
portal shows for that request is **not verified**. Cancelling the ScreenCast
start response is `CancelledByUser`. A denied RemoteDesktop or ScreenCast
response on the input path is `PermissionMissing` for `remote_desktop` or
`screencast`.

**ScreenCast capture client** (`AshpdPortalClient::start_screencast`):

1. Create a ScreenCast session on the session bus.
2. Select one source (`multiple = false`), either a window or a monitor, with
   the caller's cursor mode, and the persist mode above.
3. Start the session. Zero streams, more than one stream, a source kind other
   than the one requested, a PipeWire node id of 0 or `u32::MAX`, or a
   non-positive size closes the session and returns an error.
4. A later single-frame pipeline refuses an incomplete accessibility tree
   ([ADR 0004](adr/0004-raw-tree-completeness.md)), unknown or non-positive
   bounds, a frame whose PipeWire node is not the authorized node, a frame more
   than five seconds older than the permit, and an expired permit. Real
   PipeWire delivery of that frame is **not verified**.

**RemoteDesktop input client** (`AshpdRemoteDesktop::open`), which also selects
a ScreenCast source on the same session:

1. Open the session bus. Failure is a native error with `maybe_executed = false`.
2. Read `org.freedesktop.portal.RemoteDesktop`. Version below 2 is
   `CapabilityUnavailable`. The capability probe treats any present version as
   "interface present"; the open path is the one that enforces version 2.
3. Create the session and subscribe to `Closed` before Start, so a revocation
   between consent and connect is not dropped.
4. `SelectDevices` asks for keyboard and pointer together, persist mode
   `ExplicitlyRevoked`, and the optional restore token.
5. ScreenCast `SelectSources` asks for exactly one window. Monitor sources are
   not requested. Cursor mode is hidden. Multiple sources are off.
6. Start must return exactly one window stream and both keyboard and pointer.
   A missing mapping id is `CapabilityUnavailable`.
7. Input bytes go only through `ConnectToEIS` on that session. The code does
   not look for an EI socket in the environment, and it does not fall back to
   XTEST, portal pointer-notify methods, or a full desktop
   ([ADR 0008 reis](adr/0008-reis-portal-input.md)).
8. A separate input authority must prove the live session, PipeWire node,
   serial, mapping id, and the exact window. A title, app id, size, or a
   one-time focus check is not that proof. Unproven scope stays handoff.
9. The portal's new restore token is written only after that proof, and only
   while the session is still live.

Opening input also requires a non-empty authorization version, an authorization
clock still before `auth_expires_secs`, a non-zero captured size, and a window
binding whose compositor, id, peer PID, process start time, and image pid match
the router window. Native Wayland peer PID is the only PID treated as a
credential. An XWayland PID is untrusted metadata
([GNOME extension README](../tools/gnome-shell/README.md)).

Stream coordinates are mapped from the authorized window's physical size into
the EI region. A failed map refuses the action. There is no silent full-desktop
coordinate fallback.

### Window identity by desktop

Titles and app ids are discovery labels. They are not authorization identity.

| Desktop | What is implemented | Trusted identity | Real desktop |
| --- | --- | --- | --- |
| GNOME | Optional Shell extension `computer-use-windows@grok.app`, metadata aimed at Shell 46–49. Manual install. Compile `schemas/` with `glib-compile-schemas`, then enable the extension. The extension lists windows and can activate one exact object. It does not capture pixels, inject input, or grant a portal | Native Wayland `peerPid`, plus canonical executable path, device, inode, SHA-256, and process start time. Activation stays off until a trusted user sets schema `org.grok.computer-use.windows` key `activation-owner` to the engine's unique D-Bus name (for example `:1.42`). An engine restart needs a new value | **not verified** |
| KDE | Optional KWin 6 script, installed explicitly with KPackage tools. `engineBusName` must be the engine's unique bus owner. Reports expire after two seconds | Observation only. `Window.pid` is not treated as a native Wayland peer credential, so process binding is handoff. The script cannot activate, capture, or inject | **not verified** |
| wlroots family | Read-only `zwlr_foreign_toplevel_manager_v1` versions 1–3 | Observation only. The protocol has title and app id, not a peer PID. Reported PIDs are cleared. Activation is refused. No title fallback | **not verified** |
| other / unknown | Environment classification only | No promotion of native input. Unknown compositor: doctor warns that enumeration and geometry may be inaccurate. Identity backends must refuse an unknown family | **not verified** |

Image inspection is read-only. A cold SHA-256 of a large executable uses the
budget in [ADR 0008 image hash](adr/0008-linux-image-hash-budget.md) (64 KiB
chunks, 4 GiB cap, 180 s cold digest, then 5 s for metadata). That budget is
separate from the 4.75 s driver deadline. A changed image, device, inode,
digest, pid, or start time invalidates the restore token and the approval.

The extensions are not installed by the Rust driver. Steps beyond the two tool
READMEs are **not verified**.

### Stop

Stop is a user latch. Ending a turn, reconnecting, or restarting the engine
does not clear it. Session end does not clear it either
([architecture §7.7](../../docs/plans/computer-use-v2/03-architecture.md#77-用户活动守卫与接管)
and [§8](../../docs/plans/computer-use-v2/03-architecture.md#8-会话与生命周期)).
There is no model tool that resumes.

| Route | Where it applies | Limit |
| --- | --- | --- |
| X11 `XGrabKey` | Pure X11, and only partially on XWayland | Chord text in the capability table: Ctrl+Alt+Shift+Backspace. Registration failure or a conflict disables foreground input. Doctor's next step says to free that chord |
| GlobalShortcuts portal | Native Wayland, after a successful `AshpdStopRegistrar::register` | Shortcut id `gcu-stop`. Preferred trigger string `CTRL+ALT+SHIFT+BackSpace`. The Host supplies the description. Bind must return exactly one shortcut with that id and a non-empty trigger. A later shortcuts-changed event that drops it fails the listener. The listener heartbeats by calling `list_shortcuts` (25 ms cadence, 50 ms reply budget, 75 ms lease). Interface detection alone does not enable input. A real compositor accepting this trigger is **not verified** |
| Overlay Stop / Resume capsule | Pure X11 when the overlay can prove a physical pointer | Unavailable on Wayland and on XWayland, including GNOME. KDE and wlroots do not get a layer-shell capsule in this tree. `private_apis.overlay` is `disabled` for every Wayland session ([overlay README](../crates/gcu-overlay/README.md), STATUS P4-04) |
| `gcu resume` | Interactive TTY, both stdin and stdout | Best effort. A model that can run a shell as the same user can forge a TTY ([ADR 0003](adr/0003-resume-trust.md), [cli.md](cli.md)). The trusted overlay button is unavailable on Wayland, so this CLI path is the one that exists today, with that limit |

On an attached portal session, `PortalInput::stop` latches Stop and then
releases held keys, disconnects the EI transport, closes the portal session,
and deletes that session's restore-token file. Release is independent of the
action lock. Portal close and token deletion each sit under the 4.75 s driver
deadline. `PortalInput::close` waits up to 5 s for that cleanup.
`RegisteredStop::close` fails the listener and closes the shortcuts session; it
does not clear the Stop latch.

The portable contract test checks that an in-memory Stop latch is set within
100 ms and that a fake key-up is recorded within 500 ms. That is not a
measurement on a GNOME or KDE desktop. Those desktops are **not verified**.

If the Stop listener fails, or confinement becomes false, native portal input
stays disabled. The code does not switch that window to XTEST.

### Revoke capture and input

Local restore tokens live in `$XDG_DATA_HOME/grok-computer-use/wayland-restore`.
When `XDG_DATA_HOME` is unset, the directory is
`$HOME/.local/share/grok-computer-use/wayland-restore`. When `HOME` is also
unset, the relative path `.local/share/grok-computer-use/wayland-restore` is
used. Each record is `{key}.json`. Keys may not contain `/`, `\`, `..`, or NUL.

On Unix, a directory the store itself creates is mode `0700`, and a file it
creates is mode `0600`. An already-existing directory keeps its mode. Records
bind compositor and compositor epoch, source kind, window id and generation,
executable path, device, inode, SHA-256, pid, process start time, and
authorization version. Lifetime is at most 24 hours and at most the parent
authorization expiry (`MAX_TOKEN_LIFETIME_SECS`). Load deletes the file when
the record is expired, the parent authorization is expired, the binding does
not match, or the JSON is unreadable.

What actually revokes a live session:

- `PortalInput` shutdown (Stop, `close`, drop, cancel, or a failed action)
  releases keys, disconnects EI, closes the portal session, and deletes that
  session's token file.
- `RestoreTokenStore::revoke` deletes one key. `clear_all` deletes every
  `.json` file in the directory and leaves the directory.
- There is no `gcu` subcommand that calls either function. `gcu approvals revoke`
  revokes persistent app approvals in `approvals.json`. It does not delete
  Wayland restore tokens ([cli.md](cli.md)).

To remove local copies, delete that directory, or delete the `.json` files
inside it. The current `gcu` binary does not create these files, because it
never opens a portal session.

The portal request uses `PersistMode::ExplicitlyRevoked`. Whether GNOME or KDE
still lists a grant in a system settings panel after the local file is gone is
**not verified**. This page does not name a settings click-path.

### Known limits

- Full Wayland support is not claimed.
- Real portal consent, PipeWire frames, libei devices, GNOME Shell, KWin, and
  foreign-toplevel on a desktop are **not verified**.
- When ScreenCast is on the bus, the capability struct can set
  `observe.screenshot` true for a native Wayland target, while
  `LinuxDriver::capture` still returns `CapabilityUnavailable`.
- The ScreenCast and RemoteDesktop clients are not wired into the CLI.
- One granted stream is required. The input path accepts only a window stream
  with both keyboard and pointer. A monitor grant is refused there.
- A global keyboard session that cannot prove the authorized window stays
  disabled ([ADR 0008 reis](adr/0008-reis-portal-input.md)).
- `TypeText` that already names a focus element does not go through
  `PortalInput`. On a native Wayland window that call returns
  `CapabilityUnavailable` unless some other AT-SPI path applies.
- The status capsule is absent on GNOME, KDE, wlroots, and XWayland. Stop
  cannot be a capsule button there.
- User-activity guard on native Wayland is unavailable until confinement
  promotion, and that promotion is **not verified** on a desktop.
- XWayland `XGrabKey` does not see the Stop chord while a native Wayland
  surface is focused.
- Flatpak and other sandbox portal behavior is **not verified**. See troubleshooting.
- No numeric minimum PipeWire version is recorded. See troubleshooting.

### Troubleshooting

**PipeWire.** The optional crate feature `gcu-driver-linux/pipewire` enables
ashpd's `pipewire` feature and the `pipewire` 0.9 crate (`libspa` 0.9). The
build looks up pkg-config `libpipewire-0.3`. Ubuntu 22.04's stock
`libpipewire-0.3-dev` is too old for the symbols `libspa` 0.9 needs; the Linux
CI job refuses that package and installs `libpipewire-0.3-dev` from
`ppa:pipewire-debian/pipewire-upstream` ([ADR 0007](adr/0007-ashpd-pipewire-deny.md),
`.github/workflows/computer-use.yml`). That PPA is a CI build source. It is not
shipped, and this page does not ask you to add it. The repository does not
record a minimum version number such as a `0.3.x` cutoff. When
`libpipewire-0.3.pc` is missing, `scripts/verify.sh` compiles the driver
without the feature, so `RealPipeWireFrameReceiver` is absent.
`AshpdSessionWrapper::open_pipewire_remote` is the runtime FD handoff, and it
is compiled only on Linux. A desktop that actually delivers a frame is
**not verified**. Build machine notes in ADR 0007 (headers missing on the
shared compiler) are a build constraint, not a user setup step.

**Portal missing.** `LivePortalChecker` talks to
`org.freedesktop.portal.Desktop` at `/org/freedesktop/portal/desktop`. If the
session bus cannot be opened, the probe result is "portal unavailable". Doctor
then prints the missing-portal paragraph quoted above, and native screenshot
plus native input stay unavailable in the capability table. AT-SPI actions are
unchanged. Installing a backend is the driver's next-step sentence; this page
has no tested install command. A portal that is on the bus but has no
ScreenCast, RemoteDesktop, or GlobalShortcuts interface is reported per
interface, not as a missing service.

**Flatpak and other sandboxes.** **Not verified.** This tree has no Flatpak
manifest, no portal permission document, and no sandbox test. The clients use
the session bus. A sandbox that cannot open that bus fails the probe the same
way as a missing portal. Which Flatpak overrides or host portals would change
that is **not verified**, and this page does not give an override command.

**AT-SPI.** Doctor's own text when the bus is down: ensure `at-spi2-core` is
running, and Electron/Chromium need `--force-renderer-accessibility` for a full
tree. That flag is quoted from doctor. It is **not verified** here against a
real Electron build.

**Stop chord busy.** On X11 and XWayland, doctor says foreground input is
disabled and names Ctrl+Alt+Shift+Backspace. On native Wayland, a failed
GlobalShortcuts bind leaves input disabled. The desktop UI for editing
shortcuts is **not verified**.

**Restore token rejected.** A mismatch of compositor, source kind, window
identity, process instance, or authorization version deletes the file and
returns an origin error. Expiry does the same. The next open then has no token
and must go through portal consent again. That consent dialog is **not verified**.

### Differences from X11

| Topic | X11 | Wayland session |
| --- | --- | --- |
| Display variables | `DISPLAY` and/or `XDG_SESSION_TYPE=x11`, and no `WAYLAND_DISPLAY` | `WAYLAND_DISPLAY` or `XDG_SESSION_TYPE=wayland`. `DISPLAY` additionally means XWayland is present |
| Window ids used for capture | X11 window id via XComposite | Native ids (`/` or `wayland` prefix) are refused by `LinuxDriver::capture`. X ids still use XComposite |
| Screenshot permission | X11 extensions (`XComposite`, and XTEST for input) | Portal ScreenCast for the library path. User consent dialog **not verified** |
| Input | XTEST, treated as foreground, and only while the Stop chord is registered | Library path: RemoteDesktop + `ConnectToEIS` (`reis`). No XTEST fallback. CLI path: `CapabilityUnavailable` |
| User-activity guard | XInput2 when available | Unavailable for XWayland and for native Wayland until a confined authority is attached. Desktop behavior **not verified** |
| Stop | Root `XGrabKey`, Ctrl+Alt+Shift+Backspace | Native: GlobalShortcuts id `gcu-stop`, **not verified** on a real portal. XWayland grab is partial. Capsule button is unavailable for the whole Wayland session |
| Overlay in screenshots | Overlay is hidden during capture. `overlay_excluded_from_capture` stays false | No overlay is created, so there is nothing to exclude |
| Restore token | Not used | Directory above, mode `0700`/`0600` when the store creates them, lifetime min(24 h, authorization expiry) |
| Identity | `_NET_CLIENT_LIST` and `_NET_WM_PID` on the X server | Optional GNOME extension, optional KWin script, or wlroots foreign-toplevel. Titles are not identity. Real helpers **not verified** |
| Background | `partial`: AT-SPI can run without XTEST; pointer input is foreground | Same `partial` for AT-SPI. Native pointer/keyboard stay off unless the promoted portal session exists |

### Evidence

| Claim family | Where it is recorded |
| --- | --- |
| Capability and doctor strings | `computer-use/crates/gcu-driver-linux/src/wayland/capabilities.rs`, `doctor.rs`; tests `wayland_caps.rs`, `wayland_contract` |
| ScreenCast, restore token, frame refusal | `portal.rs`, `restore.rs`, `capture.rs`; tests `portal_capture.rs`; [ADR 0007](adr/0007-ashpd-pipewire-deny.md); [ADR 0004](adr/0004-raw-tree-completeness.md) |
| RemoteDesktop, EI, Stop, no XTEST fallback | `input_portal.rs`, `input.rs`, `input_control.rs`, `input_stop.rs`, `ei.rs`; [ADR 0008 reis](adr/0008-reis-portal-input.md) |
| Identity | `windows.rs`, `gnome.rs`, `kwin.rs`, `toplevel.rs`; [GNOME README](../tools/gnome-shell/README.md); [KWin README](../tools/kwin/README.md); [ADR 0008 image hash](adr/0008-linux-image-hash-budget.md) |
| Overlay absent on Wayland | [overlay README](../crates/gcu-overlay/README.md); `gcu-overlay/src/availability.rs` |
| CLI is fake-only; `gcu resume` limits | [cli.md](cli.md); [ADR 0003](adr/0003-resume-trust.md) |
| Real desktop not run | [STATUS.md](../../docs/plans/computer-use-v2/STATUS.md) rows P7-01 through P7-05; `scripts/wayland-ci.sh` |
| Architecture contract | [§5.3](../../docs/plans/computer-use-v2/03-architecture.md#53-linux), [§7.7](../../docs/plans/computer-use-v2/03-architecture.md#77-用户活动守卫与接管), [§8](../../docs/plans/computer-use-v2/03-architecture.md#8-会话与生命周期) |

<a id="zh"></a>

## 中文

### 支持边界

本页不声称 Wayland 全支持。

- 当前发布的 `gcu` 只组装离线 fake 驱动。其它驱动值以退出码 2 拒绝，原文是 `Real driver assembly is not implemented`（`computer-use/crates/gcu-cli/src/args.rs`）。[cli.md](cli.md) 里的 `gcu doctor` 因此只报告 fake 桌面，不会探测 `xdg-desktop-portal`。
- AT-SPI 观察和 AT-SPI 语义动作在 X11、XWayland、原生 Wayland 上都标为可用。无障碍总线是 [架构 §5.3](../../docs/plans/computer-use-v2/03-architecture.md#53-linux) 里的可靠动作路径。
- 原生 Wayland 截图和指针/键盘在 portal trait 后面有库代码。`LinuxDriver::capture` 对原生 Wayland 窗口仍返回 `CapabilityUnavailable`。该窗口上的指针、拖拽、滚动、按键和 `type_text` 同样如此，除非已经为这一个窗口挂上了 `PortalInput`。CLI 不会挂上它。
- 真实 GNOME、KDE、wlroots 上的 portal 对话框、PipeWire 帧和 libei 设备均为**未验证**。STATUS 中 P7-02、P7-03、P7-04、P7-05 的真机桌面都是 `not_run`。可移植契约行是 fake 或内存测试。

### 会话如何分类

`detect_environment` 读进程环境。空字符串忽略。

| 变量 | 结果 |
| --- | --- |
| 设置了 `WAYLAND_DISPLAY`，或 `XDG_SESSION_TYPE=wayland` | Wayland 会话 |
| Wayland 会话且设置了 `DISPLAY` | `wayland+xwayland` |
| 否则设置了 `DISPLAY`，或 `XDG_SESSION_TYPE=x11` | `x11` |
| 都没有 | `unknown`。doctor 的 `ready` 为 false |

有 `WAYLAND_DISPLAY` 时，即使 `XDG_SESSION_TYPE=x11` 也按 Wayland 计。覆盖层用同一顺序（[overlay README](../crates/gcu-overlay/README.md)）。

合成器家族依次看 `XDG_CURRENT_DESKTOP`、`DESKTOP_SESSION`，再看 `SWAYSOCK`（Sway）、`HYPRLAND_INSTANCE_SIGNATURE`（Hyprland）、`KDE_FULL_SESSION`（KDE）。名字按 `:` 切开，子串匹配，顺序为：`gnome`/`mutter`；`kde`/`plasma`/`kwin`；`sway`/`hyprland`/`river`/`wayfire`/`wlroots`/`phosh`；`xfce`/`lxqt`/`mate`/`cinnamon`/`cosmic`/`pantheon`。其余是 `unknown`。doctor 会写窗口枚举和几何可能不准确。`controlled_capabilities` 只在运行时检查通过后，才把 GNOME、KDE、wlroots 的原生输入提升为可用。`other` 和 `unknown` 停在静态表。

驱动对外只发布一套能力，对应一个目标：Wayland 会话里只要有 X 服务器，这套表就按 XWayland 窗口来写，即使还有原生窗口。在 Wayland 会话里，不是 X11 id、并且以 `/` 或 `wayland` 开头的窗口 id，当成原生窗口。

### 能力表

下表是 `build_environment_capabilities` 的静态结果，发生在 `PortalInput` 提升之前。“有 portal”指会话总线探到 `org.freedesktop.portal.Desktop` 或下面三个接口之一。

| 能力 | 纯 X11 | XWayland 窗口 | 原生 Wayland 窗口 |
| --- | --- | --- | --- |
| 无障碍树 | D-Bus 上的 AT-SPI2 | 相同 | 相同 |
| 语义动作（Action、SetValue、SelectText、Text、Value） | 可用 | 可用 | 可用。这些调用不开 portal |
| 按窗口定向的指针/键盘 | Linux 上不可用 | 不可用 | 不可用 |
| 能力表里的截图 | 有 `XComposite` 时走 `XGetImage`；否则降级，被遮挡的窗口失败 | 有扩展时走 `XComposite` | 仅当 portal 服务和 `org.freedesktop.portal.ScreenCast` 都在时标为可用 |
| `LinuxDriver::capture` 的实际行为 | X11 采集 | 窗口有 X id 时走 X11 采集 | `CapabilityUnavailable`。不会调用 ScreenCast 管线 |
| 指针和键盘 | Stop 组合键注册成功且未锁存时走 XTEST | 同一门禁；组合键本身只是部分可用 | 静态表：不可用，原因文本写着 portal RemoteDesktop + libei 集成尚未完成。提升是另一道运行时门禁 |
| 全局 Stop | 注册成功时用 `XGrabKey` 抓 Ctrl+Alt+Shift+Backspace | 部分可用：只有 X11 表面处于活动时才能抓到 | 在 GlobalShortcuts 监听注册成功并保持健康之前不可用。看见接口不等于注册成功 |
| 用户活动守卫 | 监听起来时，根窗口上的 XInput2 | 不可用 | 静态表里不可用 |
| 状态胶囊 / 覆盖层 | 纯 X11 上为部分可用（ARGB 合成、物理 XI2 设备；采集期间隐藏） | 整个 Wayland 会话都不可用 | 不可用 |
| 协议里的后台操作 | 语义动作可用时为 `partial` | `partial` | `partial` |
| `overlay_excluded_from_capture` | false（X11 改为在采集期间隐藏覆盖层） | false | false |

doctor 的 `ready` 仅在 AT-SPI 已连接且会话类型不是 `unknown` 时为 true。Stop 锁存本身不会把 `ready` 清掉。它会关掉 X11 和 XWayland 的前台输入。

原生 Wayland 上，缺件时的驱动原文是：

- 没有 portal 服务：doctor 原文包含 `xdg-desktop-portal is not available on D-Bus session bus`。下一步原文：安装 `xdg-desktop-portal` 以及桌面后端（`xdg-desktop-portal-gnome`、`xdg-desktop-portal-kde` 或 `xdg-desktop-portal-wlr`）。
- 没有 ScreenCast 接口：原生采集不可用。下一步原文：安装或配置合成器的 screencast 后端。
- 没有 RemoteDesktop 接口：原生输入不可用。下一步是同一类句子。
- 没有 GlobalShortcuts 接口：无法在原生 Wayland 表面上注册全局 Stop。下一步原文写了 GNOME 44+ 和 KDE 6+。这是诊断文本。这些桌面版本**未验证**。
- 有 GlobalShortcuts 接口：在 `controlled_capabilities` 提升该行之前，静态原因仍是 `integration is pending (P7-03)`。

这些包名是驱动自己的下一步句子。本页不另加安装命令。发行版里同名包是否真能提供可用后端，**未验证**。

`controlled_capabilities` 会把原生前台输入、用户活动守卫和全局 Stop 三行一起提升，并且必须同时满足：合成器是 GNOME、KDE 或 wlroots；截图状态为可用；已注册的 Stop 监听健康；输入权威报告会话被限制在授权窗口内。提升后的原因分别是 “Registered desktop-wide Stop listener with independent watchdog”、“Trusted compositor confines input to the authorized window and monitors physical activity” 和 “User-approved RemoteDesktop session with stream-paired EI devices”。监听、范围证明或 portal 会话一旦丢失，`PortalInput::available` 变为 false，驱动重新发布静态表。

### 首次 portal 授权

有两个 portal 客户端。今天的 `gcu` 都不会启动它们。两者都向 portal 请求 `PersistMode::ExplicitlyRevoked`，并可以带上已经通过本地检查的 restore token。GNOME 或 KDE 为此弹出的系统对话框**未验证**。取消 ScreenCast 的 start 响应是 `CancelledByUser`。输入路径上 RemoteDesktop 或 ScreenCast 被拒绝，则是 `PermissionMissing`，权限名分别为 `remote_desktop` 或 `screencast`。

**ScreenCast 采集客户端**（`AshpdPortalClient::start_screencast`）：

1. 在会话总线上创建 ScreenCast 会话。
2. 选择一个源（`multiple = false`），窗口或监视器二选一，使用调用方给出的光标模式，以及上面的持久化模式。
3. 启动会话。零个流、多于一个流、源类型与请求不一致、PipeWire 节点 id 为 0 或 `u32::MAX`、尺寸不是正数，都会关闭会话并返回错误。
4. 之后的单帧管线会拒绝不完整的无障碍树（[ADR 0004](adr/0004-raw-tree-completeness.md)）、未知或非正的边界、PipeWire 节点不是授权节点的帧、比许可早超过五秒的帧，以及过期许可。真实 PipeWire 把这一帧交出来的行为**未验证**。

**RemoteDesktop 输入客户端**（`AshpdRemoteDesktop::open`）在同一会话上再选一个 ScreenCast 源：

1. 打开会话总线。失败是原生错误，`maybe_executed = false`。
2. 读取 `org.freedesktop.portal.RemoteDesktop`。版本低于 2 为 `CapabilityUnavailable`。能力探测把“接口存在”当成任意版本都算有；真正要求版本 2 的是打开路径。
3. 创建会话，并在 Start 之前订阅 `Closed`，这样同意和连接之间的撤销不会丢掉。
4. `SelectDevices` 同时要键盘和指针，持久化模式为 `ExplicitlyRevoked`，并可带 restore token。
5. ScreenCast 的 `SelectSources` 只要一个窗口。不请求监视器。光标模式为隐藏。关闭多源。
6. Start 必须返回恰好一个窗口流，并且同时有键盘和指针。没有 mapping id 则是 `CapabilityUnavailable`。
7. 输入字节只走该会话的 `ConnectToEIS`。代码不去环境里找 EI socket，也不回退到 XTEST、portal 的指针通知方法或整桌面（[ADR 0008 reis](adr/0008-reis-portal-input.md)）。
8. 另一路输入权威必须证明当前会话、PipeWire 节点、serial、mapping id 和精确窗口。标题、app id、尺寸或一次焦点预检都不是这份证明。无法证明的范围保持 handoff。
9. portal 返回的新 restore token 只在这份证明之后写入，并且会话必须仍然活着。

打开输入还要求：授权版本非空；授权时钟仍早于 `auth_expires_secs`；采集尺寸非零；窗口绑定的合成器、id、peer PID、进程启动时间和映像 pid 与路由器窗口一致。只有原生 Wayland 的 peer PID 才当作凭据。XWayland 的 PID 是不可信元数据（[GNOME 扩展 README](../tools/gnome-shell/README.md)）。

流坐标从授权窗口的物理尺寸映射到 EI 区域。映射失败就拒绝该动作。没有静默的全桌面坐标回退。

### 各桌面的窗口身份

标题和 app id 只是发现用的标签，不是授权身份。

| 桌面 | 已实现的内容 | 可信身份 | 真机桌面 |
| --- | --- | --- | --- |
| GNOME | 可选 Shell 扩展 `computer-use-windows@grok.app`，元数据面向 Shell 46–49。手动安装。用 `glib-compile-schemas` 编译 `schemas/`，再启用扩展。扩展列出窗口，并可激活一个精确对象。它不采集像素、不注入输入、不授予 portal | 原生 Wayland 的 `peerPid`，加上可执行文件规范路径、设备号、inode、SHA-256 和进程启动时间。激活默认关闭，直到可信用户把 schema `org.grok.computer-use.windows` 的键 `activation-owner` 设成引擎当前的唯一 D-Bus 名（例如 `:1.42`）。引擎重启后要重新绑定 | **未验证** |
| KDE | 可选 KWin 6 脚本，须用 KPackage 工具显式安装。`engineBusName` 必须是引擎的唯一总线所有者。报告两秒后过期 | 仅观察。`Window.pid` 不当作原生 Wayland peer 凭据，进程绑定是 handoff。脚本不能激活、采集或注入 | **未验证** |
| wlroots 家族 | 只读的 `zwlr_foreign_toplevel_manager_v1`，协议版本 1–3 | 仅观察。协议有标题和 app id，没有 peer PID。报告里的 PID 会被清掉。激活被拒绝。不用标题回退 | **未验证** |
| other / unknown | 只有环境分类 | 不提升原生输入。未知合成器时 doctor 警告枚举和几何可能不准确。身份后端必须拒绝未知家族 | **未验证** |

映像检查是只读的。大可执行文件的冷 SHA-256 使用 [ADR 0008 映像哈希](adr/0008-linux-image-hash-budget.md) 里的预算（64 KiB 分块、4 GiB 上限、冷摘要 180 秒，然后元数据再 5 秒）。该预算独立于 4.75 秒的驱动期限。映像、设备号、inode、摘要、pid 或启动时间变化后，restore token 和批准都失效。

Rust 驱动不会自动安装这些扩展。两份工具 README 以外的安装步骤**未验证**。

### Stop

Stop 是用户锁存。一轮结束、重连或引擎重启都不会清除它。会话结束也不清除（[架构 §7.7](../../docs/plans/computer-use-v2/03-architecture.md#77-用户活动守卫与接管) 和 [§8](../../docs/plans/computer-use-v2/03-architecture.md#8-会话与生命周期)）。没有模型工具可以恢复。

| 路线 | 适用处 | 限制 |
| --- | --- | --- |
| X11 `XGrabKey` | 纯 X11；XWayland 上只是部分可用 | 能力表里的组合键文本：Ctrl+Alt+Shift+Backspace。注册失败或冲突会关掉前台输入。doctor 的下一步是放开这个组合键 |
| GlobalShortcuts portal | 原生 Wayland，且 `AshpdStopRegistrar::register` 成功之后 | 快捷键 id `gcu-stop`。首选触发字符串 `CTRL+ALT+SHIFT+BackSpace`。描述由 Host 提供。绑定结果必须恰好有一个该 id 且触发描述非空的快捷键。之后的 shortcuts-changed 若丢掉它，监听失败。监听靠 `list_shortcuts` 心跳（25 ms 节拍、50 ms 应答预算、75 ms 租约）。只探测到接口不会启用输入。真实合成器是否接受这个触发字符串，**未验证** |
| 覆盖层 Stop / 恢复胶囊 | 纯 X11，且覆盖层能证明是物理指针 | Wayland 和 XWayland 都不可用，包括 GNOME。本树里 KDE 和 wlroots 也没有 layer-shell 胶囊。每个 Wayland 会话的 `private_apis.overlay` 都是 `disabled`（[overlay README](../crates/gcu-overlay/README.md)，STATUS P4-04） |
| `gcu resume` | 交互 TTY，stdin 和 stdout 都必须是 | 尽力而为。同一用户下能跑 shell 的模型可以伪造 TTY（[ADR 0003](adr/0003-resume-trust.md)，[cli.md](cli.md)）。Wayland 上没有可信的覆盖层按钮，所以今天存在的是这条 CLI 路径，并带有这个限制 |

已挂上的 portal 会话里，`PortalInput::stop` 先锁存 Stop，然后释放按住的键、断开 EI、关闭 portal 会话，并删除该会话的 restore token 文件。释放不排在动作锁后面。portal 关闭和删除 token 各自受 4.75 秒驱动期限约束。`PortalInput::close` 最多再等 5 秒。`RegisteredStop::close` 使监听失败并关闭快捷键会话；它不清除 Stop 锁存。

可移植契约测试检查：内存中的 Stop 锁存在 100 ms 内置位，并且 500 ms 内记录到一次假的键抬起。这不是 GNOME 或 KDE 桌面上的测量。那些桌面**未验证**。

Stop 监听失败或范围证明变为 false 时，原生 portal 输入保持关闭。代码不会把该窗口改走 XTEST。

### 撤销采集和输入

本地 restore token 在 `$XDG_DATA_HOME/grok-computer-use/wayland-restore`。未设置 `XDG_DATA_HOME` 时，目录是 `$HOME/.local/share/grok-computer-use/wayland-restore`。`HOME` 也未设置时，使用相对路径 `.local/share/grok-computer-use/wayland-restore`。每条记录是 `{key}.json`。键不能包含 `/`、`\`、`..` 或 NUL。

在 Unix 上，商店自己创建的目录是 `0700`，自己创建的文件是 `0600`。已经存在的目录保持原模式。记录绑定：合成器及其 epoch、源类型、窗口 id 和 generation、可执行路径、设备号、inode、SHA-256、pid、进程启动时间、授权版本。有效期不超过 24 小时，也不超过父授权到期时间（`MAX_TOKEN_LIFETIME_SECS`）。加载时若记录过期、父授权过期、绑定不符或 JSON 读不出来，会删除该文件。

真正撤销一个活会话的路径：

- `PortalInput` 关闭（Stop、`close`、drop、取消或动作失败）会释放按键、断开 EI、关闭 portal 会话，并删除该会话的 token 文件。
- `RestoreTokenStore::revoke` 删除一个键。`clear_all` 删除目录里全部 `.json` 文件，保留目录本身。
- 没有 `gcu` 子命令会调用这两个函数。`gcu approvals revoke` 撤销的是 `approvals.json` 里的持久应用批准，不删除 Wayland restore token（[cli.md](cli.md)）。

要清掉本地副本，删除该目录，或删除其中的 `.json` 文件。当前 `gcu` 不会创建这些文件，因为它从不开 portal 会话。

portal 请求使用 `PersistMode::ExplicitlyRevoked`。本地文件删除之后，GNOME 或 KDE 是否仍在系统设置里列出授权，**未验证**。本页不写设置里的点击路径。

### 已知限制

- 不声称 Wayland 全支持。
- 真机上的 portal 同意、PipeWire 帧、libei 设备、GNOME Shell、KWin 和 foreign-toplevel 均为**未验证**。
- 即使总线上有 ScreenCast、能力表把 `observe.screenshot` 设为 true，`LinuxDriver::capture` 仍然拒绝原生 Wayland 窗口。
- ScreenCast 和 RemoteDesktop 客户端没有接到 CLI。
- 必须恰好一个流。输入路径只接受同时带键盘和指针的窗口流。监视器授权在这条路径上被拒绝。
- 无法证明授权窗口的全局键盘会话保持禁用（[ADR 0008 reis](adr/0008-reis-portal-input.md)）。
- 已经指定焦点元素的 `TypeText` 不走 `PortalInput`。在原生 Wayland 窗口上，除非另有 AT-SPI 路径，该调用返回 `CapabilityUnavailable`。
- GNOME、KDE、wlroots 和 XWayland 都没有状态胶囊。Stop 在这些环境里不能是胶囊按钮。
- 原生 Wayland 的用户活动守卫在范围提升之前不可用，而桌面上的提升**未验证**。
- 焦点在原生 Wayland 表面上时，XWayland 的 `XGrabKey` 看不到 Stop 组合键。
- Flatpak 和其他沙箱里的 portal 行为**未验证**。见故障排除。
- 仓库没有记录 PipeWire 的数字最低版本。见故障排除。

### 故障排除

**PipeWire。** 可选 crate feature `gcu-driver-linux/pipewire` 会打开 ashpd 的 `pipewire` feature 和 `pipewire` 0.9 crate（`libspa` 0.9）。构建用 pkg-config 名 `libpipewire-0.3` 查找。Ubuntu 22.04 自带的 `libpipewire-0.3-dev` 缺少 `libspa` 0.9 需要的符号；Linux CI 拒绝这个包，改为从 `ppa:pipewire-debian/pipewire-upstream` 安装 `libpipewire-0.3-dev`（[ADR 0007](adr/0007-ashpd-pipewire-deny.md)，`.github/workflows/computer-use.yml`）。该 PPA 只是 CI 的构建源，不随产品分发，本页也不要求你添加它。仓库没有记录类似 `0.3.x` 的最低版本号。缺少 `libpipewire-0.3.pc` 时，`scripts/verify.sh` 会在没有该 feature 的情况下编译驱动，因此没有 `RealPipeWireFrameReceiver`。`AshpdSessionWrapper::open_pipewire_remote` 是运行时的 FD 交接，只在 Linux 上编译。真正在桌面上交出一帧的行为**未验证**。ADR 0007 里共享编译机缺少头文件，是构建约束，不是用户安装步骤。

**portal 缺失。** `LivePortalChecker` 访问 `/org/freedesktop/portal/desktop` 上的 `org.freedesktop.portal.Desktop`。会话总线打不开时，探测结果是 portal 不可用。doctor 随后打印上面引用的缺 portal 段落，能力表里的原生截图和原生输入保持不可用。AT-SPI 动作不变。安装后端是驱动的下一步句子；本页没有经过测试的安装命令。总线上看得到 portal、但没有 ScreenCast、RemoteDesktop 或 GlobalShortcuts 接口时，按接口分别报告，而不是报成整个服务缺失。

**Flatpak 和其他沙箱。** **未验证。** 本树没有 Flatpak manifest、没有 portal 权限说明、也没有沙箱测试。客户端使用会话总线。打不开这条总线的沙箱，探测失败的方式和 portal 缺失相同。哪些 Flatpak override 或宿主 portal 会改变这一点，**未验证**，本页不提供 override 命令。

**AT-SPI。** 总线不可用时 doctor 的原文是：确保 `at-spi2-core` 在运行；Electron/Chromium 需要 `--force-renderer-accessibility` 才能露出完整树。这个 flag 引自 doctor，本页没有对真实 Electron 构建验证过。

**Stop 组合键被占用。** 在 X11 和 XWayland 上，doctor 会写前台输入已禁用，并点名 Ctrl+Alt+Shift+Backspace。在原生 Wayland 上，GlobalShortcuts 绑定失败则输入保持禁用。桌面里编辑快捷键的界面**未验证**。

**restore token 被拒绝。** 合成器、源类型、窗口身份、进程实例或授权版本任一不符，都会删除文件并返回来源错误。过期同样处理。下一次打开就没有 token，必须重新走 portal 同意。该对话框**未验证**。

### 与 X11 的差异

| 主题 | X11 | Wayland 会话 |
| --- | --- | --- |
| 显示变量 | 有 `DISPLAY` 和/或 `XDG_SESSION_TYPE=x11`，且没有 `WAYLAND_DISPLAY` | 有 `WAYLAND_DISPLAY` 或 `XDG_SESSION_TYPE=wayland`。再有 `DISPLAY` 表示存在 XWayland |
| 采集用的窗口 id | 经 XComposite 的 X11 窗口 id | 原生 id（`/` 或 `wayland` 前缀）被 `LinuxDriver::capture` 拒绝。X id 仍走 XComposite |
| 截图权限 | X11 扩展（截图用 `XComposite`，输入用 XTEST） | 库路径用 portal ScreenCast。用户同意对话框**未验证** |
| 输入 | XTEST，视为前台，且仅在 Stop 组合键已注册时 | 库路径：RemoteDesktop + `ConnectToEIS`（`reis`）。没有 XTEST 回退。CLI 路径：`CapabilityUnavailable` |
| 用户活动守卫 | XInput2 可用时启用 | XWayland 和原生 Wayland 在挂上受限权威之前不可用。桌面行为**未验证** |
| Stop | 根窗口 `XGrabKey`，Ctrl+Alt+Shift+Backspace | 原生：GlobalShortcuts id `gcu-stop`，真实 portal 上**未验证**。XWayland 抓键只是部分可用。整个 Wayland 会话都没有胶囊按钮 |
| 截图里的覆盖层 | 采集期间隐藏覆盖层。`overlay_excluded_from_capture` 仍为 false | 不创建覆盖层，因此没有可排除的窗口 |
| restore token | 不使用 | 见上面的目录；商店创建时为 `0700`/`0600`；有效期是 24 小时和授权到期时间中较早者 |
| 身份 | X 服务器上的 `_NET_CLIENT_LIST` 和 `_NET_WM_PID` | 可选 GNOME 扩展、可选 KWin 脚本，或 wlroots foreign-toplevel。标题不是身份。真实助手**未验证** |
| 后台 | `partial`：AT-SPI 可以不靠 XTEST；指针输入是前台 | AT-SPI 同样是 `partial`。原生指针/键盘在提升后的 portal 会话出现之前保持关闭 |

### 证据

| 断言族 | 记录位置 |
| --- | --- |
| 能力与 doctor 原文 | `computer-use/crates/gcu-driver-linux/src/wayland/capabilities.rs`、`doctor.rs`；测试 `wayland_caps.rs`、`wayland_contract` |
| ScreenCast、restore token、拒帧 | `portal.rs`、`restore.rs`、`capture.rs`；测试 `portal_capture.rs`；[ADR 0007](adr/0007-ashpd-pipewire-deny.md)；[ADR 0004](adr/0004-raw-tree-completeness.md) |
| RemoteDesktop、EI、Stop、无 XTEST 回退 | `input_portal.rs`、`input.rs`、`input_control.rs`、`input_stop.rs`、`ei.rs`；[ADR 0008 reis](adr/0008-reis-portal-input.md) |
| 身份 | `windows.rs`、`gnome.rs`、`kwin.rs`、`toplevel.rs`；[GNOME README](../tools/gnome-shell/README.md)；[KWin README](../tools/kwin/README.md)；[ADR 0008 映像哈希](adr/0008-linux-image-hash-budget.md) |
| Wayland 上没有覆盖层 | [overlay README](../crates/gcu-overlay/README.md)；`gcu-overlay/src/availability.rs` |
| CLI 只有 fake；`gcu resume` 的限制 | [cli.md](cli.md)；[ADR 0003](adr/0003-resume-trust.md) |
| 真机未跑 | [STATUS.md](../../docs/plans/computer-use-v2/STATUS.md) 的 P7-01 至 P7-05；`scripts/wayland-ci.sh` |
| 架构合同 | [§5.3](../../docs/plans/computer-use-v2/03-architecture.md#53-linux)、[§7.7](../../docs/plans/computer-use-v2/03-architecture.md#77-用户活动守卫与接管)、[§8](../../docs/plans/computer-use-v2/03-architecture.md#8-会话与生命周期) |

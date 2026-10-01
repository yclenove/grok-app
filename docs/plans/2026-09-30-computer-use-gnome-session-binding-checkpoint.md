# Computer Use — GNOME user-service session binding

Date authority: **2026-09-30**. `20261001` in local evidence directories is a
host run tag, not an assertion that a future release or final acceptance exists.
Goal: **active / partial — not releasable**. Full original scope is unchanged.

## Actual blocker and repair

The installed owned Ubuntu/GDM/GNOME46 Wayland VM exposed another production
blocker after the ordinary-client input observer repair. Both real GNOME Shell
and a process launched by its graphical terminal belong to systemd user services,
not `session-*.scope`. Their actual login1 `GetSessionByPID` returns
`org.freedesktop.login1.NoSessionForPID`. The original
`GnomeNativePolicyWatch::connect` therefore failed before it could monitor the
real helper. Helper status `ready` was not evidence that this path worked.

`logind.rs`, `logind_binding.rs`, and `logind_binding_signal.rs` now:

- Prefer the actual PID session. A returned SSH, remote, inactive, locked, X11,
  foreign-UID or otherwise unsafe session is rejected, never redirected.
- Permit the user-service route **only** on the exact `NoSessionForPID` method
  error. Other errors, unavailable properties and timeouts remain failures.
- Resolve `GetUserByPID` on the pinned login1 unique owner; require agreement
  with `GetUser(uid)`, `User.UID`, concrete `Display`, and a bounded, duplicate-free
  `User.Sessions` inventory. Each session must have matching UID/user object/ID.
  Display must be the sole local graphical session and an unlocked active local
  Wayland `user` session. UID alone or `XDG_SESSION_ID` cannot select a display.
- Apply the same binding to the bus-authenticated Shell peer; retain a user
  inventory fence even when the App has a direct session and only Shell uses
  the user manager. Never silently rebind to a different Display.
- Observe selected-session, login1-owner, user-object and session-inventory loss.
  Signals carrying a new session are inspected, not erased by a later positive
  inventory snapshot. SSH inventory churn can revalidate the same original watch.
- Close the input guard during bounded inventory refresh. Cancellation and owner
  loss interrupt the original refresh; queued loss is drained before reopening.
  An epoch fence rejects a Host callback's late positive after close/reopen (ABA).
  No monitor task is internally detached, and the supplied Host policy is still
  mandatory. This change grants no portal or desktop permission.

## Current evidence

Directory: `tools/computer-use-probe/.run/gnome-session-binding-20261001/`.
The original failing binary, source archive, result and log remain unchanged.
`fixed/` contains a separate frozen **423-source** candidate and binary.

| Evidence | Actual result and limits |
| --- | --- |
| `native-binding-red-001.log` | Original graphical process fails with actual `NoSessionForPID` after helper status `ready`. |
| `fixed/native-binding-green-001.log` | First repaired attempt correctly stops at helper `blocked`; actual `LockedHint=yes` and ScreenShield active confirm VM idle lock. It is retained as a failed prerequisite, not counted as a binding success. |
| `fixed/native-binding-green-002.log`, `003.log` | After explicit unlock of only the owned VM, the same repaired binary connects to actual login1/Shell/helper and joins its original read-only watch twice. `XDG_SESSION_ID` is unset, no session is fabricated. |
| Actual SSH manual test | Same candidate rejects its real SSH session despite graphical environment variables; cannot borrow the user-manager Display. |
| Existing Host enable/disable manual test | Same candidate performs disabled → ready → disabled and confirms inactive helper endpoint. Not a new repair/reinstall test. |
| Focused real private D-Bus tests | **58 passed**, including **17 new** user-service / identity / ambiguity / churn / cancellation / owner-loss / deadline / queued-loss / ABA tests. |
| Full Wayland crate suite | **188 passed, 0 failed, 0 ignored, 0 filtered** on retained original executable. Private D-Bus, labwc/PipeWire/EI/GTK fixtures; not 188 GNOME App acceptance cases. |
| App preview helper | **17 passed, 10 manual ignored** in the automatic filter; explicit VM tests above are separate. |
| App commands / portal / lifecycle | **15 / 6 / 1** selected tests passed. |
| Checks | Preview App + Wayland all-targets Clippy `-D warnings`, scoped Windows rustfmt, quality gates, Node policy **18**, Python input oracle **8** passed. |

Fixed App executable SHA-256:
`483e3f53ca068722f0f80cfcf417e4f1e6216133e0c176eb19e70fca1f8d47f9`.
Fixed Wayland test executable SHA-256:
`bb3c56d23bd8ef2e25e05f2191444fd377a015521f2ec8e3163bb555c82b8fd6`.

The VM probe deliberately supplies an always-false Host policy. It creates **no
portal session and no App input grant**. Success proves actual native observer
binding and joined ownership only. The failed lock prerequisite and subsequent
unlock do not constitute active-grant lock/revocation acceptance.

First compile attempts (non-`Ord` object-path sorting and a test-only discarded
mutex guard) are preserved; corrected sources pass the final build/checks.
Default-feature, Windows, macOS, renderer/UI and actual Grok E4 suites were **not
rerun** in this checkpoint; earlier results are historical, not this candidate's
full-platform certification.

## Cleanup and fidelity

Owned QEMU attempt **9**, PID **13072**, original handle **41704**, exits **0** and
is explicitly joined after normal guest poweroff. The helper is explicitly
disabled; `enabled=false`, state `2`, and endpoint absence are reread. Installed
helper source hashes remain unchanged, directories 0700/files 0600. All explicit
SSH/graphical launchers return and their test exits are retained. No user desktop,
host input devices/groups, host package configuration or real permission is changed.

The guest is still the previously documented signed daily-image comparison
environment, with guest-only GA Mesa24.0.5. Updated Mesa25.2.8/non-3D virtio Shell
crashes remain unresolved; neither this workaround nor observed 24.04.5 labeling
certifies the full installed-platform/release matrix.

Previous sealed receipt remains
`gnome-input-observer-20261001/receipt.json`, SHA-256
`0a7ac251cbbb354d4166b509fad413115f281d973ef8b1d070e1ed80bc2e95fe`.
Its 2×7 ordinary-client successes remain historical evidence on that earlier
candidate. They are not rewritten or reclassified as complete input acceptance.

## Still open — no scope reduction

Next: close or explicitly prove the early-consumed IME/capture/mapped-pad native
input paths, then exercise real App/portal consent and autonomous grant retirement
on the final implementation. The current input coverage is not sufficient to
enable real automation or claim physical-takeover safety.

The complete goal still requires Windows x64, macOS arm64/Intel, Linux X11 and
installed Ubuntu GNOME native Wayland; Desktop/managed browser/existing Chrome
and Edge/App WebView through App/ACP/MCP; complete mouse/key/Chinese IME/clipboard,
touch/tablet/grabs, native EI exclusion, focus/lock/topology/takeover,
consent/cancellation/recovery; Ubuntu22.04 baseline and AppImage/deb/rpm;
signed install/update/repair/rollback/uninstall; native UX/DPI; real Grok E4;
and **12 hours of active soak on the same final frozen candidate**. Unverified
items remain incomplete. Defaults and `native_wayland=false` are unchanged.

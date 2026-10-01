# Computer Use — Windows native Stop and input ownership

September 25, 2026 (+08:00). Branch `feat/computer-use-implementation`, base
`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`. Existing dirty worktree preserved;
no commit, push or PR. Full goal remains **active / partial — not releasable**.
The UI/UX redesign and all original platform/release gates remain in scope.

## Production changes

- Windows now uses `NativeActionSlot`. Abort cancels only the matching older
  run generation. It does not set idle while a native call is still running
  and does not synthesize global left/right/middle button-up.
- The executing owner retains occupancy through synchronous native dispatch
  and its owned input-pair cleanup. An uncertain partial SendInput cleanup
  retains the slot through a separate state flag, not error-string matching.
- Input gates recheck cancellation, original target lifetime/geometry,
  foreground and physically held input. Toggle state alone is not held input.
  Production fallback no longer raises/restores/topmosts a window or steals
  foreground. Semantic resolution errors cannot silently select the parent.
- A canceled drag releases its directed down at the original point without
  continuing to the drop point. Double-click iterations and text
  selection/write/paste boundaries recheck the request.
- Coordinate scroll sends one directed wheel message, not wheel message +
  scrollbar command + a second global wheel. An unverified text effect is not
  replayed as WM_CHAR or a silent clipboard fallback. Clipboard remains an
  explicit requested path.
- Semantic click calls the actual UIA Invoke/default-action provider. The old
  unchecked `PostMessage(BM_CLICK)` shortcut no longer reports a completed UIA
  invocation while leaving an unacknowledged write in another queue.

This does not prove immediate interruption of an entered COM provider,
hung-target recovery, complete clipboard-format preservation, or every native
user-input race. Those remain work, not reasons to release occupancy early.

## Evidence and retained failures

Logs: `tools/computer-use-probe/.run/linux-native-20260925/`. Despite the shared
directory name, `windows-*` files describe actual Windows executions.

- `windows-stop-clippy-first.log`: retained compile error from a temporary
  string comparison. `windows-stop-clippy-r2.log`: strict App all-target Clippy
  passed, 24.90 seconds.
- `windows-stop-probe-build.log`: retained fixture type-inference error. Its
  corrected build passed in 2m 41s; diagnostic rebuild passed in 1m 05s.
- `windows-stop-native-first.log`, then `windows-stop-native-final-1.log`
  through `-3.log`: each terminal exit 0. A real owned HWND blocks inside its
  synchronous down handler. Stop returns within 500ms while occupancy remains
  held and another dispatch is rejected. After releasing that handler, the
  owned up completes, the canceled drop count stays **0**, and the next
  generation drops **once**. Unrelated/equal-fence Stop cannot cancel it.
- Accepted probe SHA-256:
  `c9e7739f68dbd9953beb87e4f93bf625f0581c70782202e9695c217854a76815`.
  Subsequent additions before the test build are unit-test-only.
- `windows-native-regression-first.log`: UIA, geometry, CJK text, key, scroll,
  drag and identity gates passed; clipboard read back empty. Suite exit **1**.
- `windows-clipboard-diagnostic.log`: isolated clipboard fixture passed. That
  neither explains nor erases the first failure.
- `windows-native-regression-r2.log`: earlier gates plus clipboard passed;
  the next focus fixture failed to start within its timeout. Suite exit **1**.
  No timeout was silently counted as a pass.
- `windows-stop-unit-build.log`: current App harness built, 2m 18s. The Windows
  manifest was embedded post-link. `windows-stop-unit.log`: all **5/5** new
  input-receipt and Windows adapter ownership tests passed, no ignored tests.
- `windows-stop-clippy-final.log`: final App all-target strict Clippy with the
  probe feature passed, **37.39 seconds**, exit 0. `git diff --check` passed;
  existing autocrlf warnings were not used to rewrite unrelated files.
- `windows-cu-unit-regression.log`: the current App harness's full
  `computer_use::` filter passed **86/86**, no ignored tests, exit 0,
  **68.62 seconds**. This includes the five new tests and the session/ACP/MCP
  and WebView regressions; it is not the complete App's 1,698-test suite.
  Scoped rustfmt check passed for all eight touched Rust files.

The owned-HWND checks are not installed App, real model, all Windows desktop
actions, or another OS acceptance. The old clipboard fixture also seeds the
real clipboard; preserving prior complete formats is required before more
broad repetitions.

## Remaining work — original scope unchanged

1. Diagnose retained clipboard and fixture-start failures, preserve clipboard
   formats in product and harness, independently count wheel/key effects, and
   test hung providers, grabs, replacement and native recovery.
2. macOS and WebView native-operation cancellation; full macOS AX, X11 AT-SPI/
   IME and GNOME native Wayland remain unfinished.
3. Installed acceptance of the redesigned workspace, task card, pairing and
   settings: scaling, focus/overlays, permissions, Stop/cleanup/reconnect on
   Windows, both macOS architectures and Linux.
4. Signed runtime delivery, native install/update/rollback, Chrome/Edge matrix,
   real Grok E4 and frozen 12-hour active soak after the final candidate edit.

See [X11](2026-09-25-computer-use-x11-native-checkpoint.md),
[UX redesign](2026-09-25-computer-use-ux-redesign.md) and the
[resumed checkpoint](2026-09-24-computer-use-resumed-checkpoint.md).

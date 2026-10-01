# Windows resolved-control identity fences

Recorded September 25, 2026 (+08:00). Full Computer Use goal remains **active /
not releasable**, including the UI/UX redesign and its installed-App acceptance.
No commit, push, PR, ordinary-application input or interactive-clipboard seeding.

## Red evidence

The original text helpers rechecked request/root authorization and the child's
class/liveness, but retained only a bare child HWND across native callbacks.
Two new real-EDIT regressions failed before the repair:

- Rotating the captured control's identity before the second admission check
  still allowed the old pending set-value to write into it.
- Reparenting the child into another owned root at the same point still allowed
  the old operation to write there.

`windows-child-identity-red-native.log` preserves both failures and the 13 older
passing groups (terminal exit 1). This is real `SetParent` and real EDIT input;
stamp rotation models identity retirement, **not forced numerical HWND reuse**.

## Product repair

- `windows_adapter/native_target.rs` captures control and root instances using
  HWND, PID, TID and the existing per-window stamp. It verifies the stamp was
  actually installed; a failed `SetProp` cannot create a usable identity.
- Production binding requires the request's exact authorized root PID/HWND/stamp,
  rejects protected/inaccessible child processes, and checks the current root
  lineage. A reparented control is not new authority to operate another window.
- Native text helpers receive the bound object itself, not a bare HWND that they
  silently rebind. Focus, read/selection callbacks, text dispatch and post-write
  readback retain the original instance. Clipboard admission and paste both use
  this fence; existing transaction restoration still runs when paste is denied.
- Directed click/key pairs and drag cleanup only release their original native
  instance. Root reparenting does not prevent cleanup of an already-sent down,
  but it prevents a successful/new action. A live HWND with unprovable identity
  receives no up; the uncertainty flag retains the existing native-action slot.
  A destroyed target is not a reason to issue a global release or find a successor.
- Scroll and click/drag dispatch retain the same root/child binding. Wheel client
  mapping failure now rejects instead of sending guessed screen coordinates.
  Directed key-up carries the prior-state/transition bits.
- No action is retried or sent through another input route after an identity or
  postcondition failure. Broker ordinary adapter errors remain `unknown`; they
  are not proof that a dispatched operation had no effect.

These are fences around callbacks, **not an atomic Win32 handle-lifetime lock**.
The OS can still change a window between validation and message dispatch. This
batch does not claim otherwise, or claim that property stamps defend against a
cooperating hostile process deliberately rewriting them.

## Evidence

Logs: `tools/computer-use-probe/.run/linux-native-20260925/` (historical directory
name; all results below are Windows processes).

- `windows-child-identity-green-native.log`: the original 15 groups pass, exit 0.
- `windows-child-identity-expanded-native-1.log`: expanded 25 groups pass, exit 0.
- `windows-child-identity-final-native-{1,2,3}.log`: **three consecutive rounds of
  25 groups**, each terminal exit 0, after the final bound-object API change.
- Final probe SHA-256:
  `af27d31959d7c9615768e43c3d9fd1589bb0a572e244be7736a33245fc5f7231`.

The 12 identity groups cover all three text routes (set-value, direct append,
clipboard append), child/root retirement, actual destruction, actual reparenting,
retirement inside length/read/selection callbacks, post-write retirement without
replay, private clipboard restoration, exact-root authorization, and directed
down/up cleanup when unchanged, retired, reparented or destroyed. Native callback
counters and separate full-value readback are the postcondition oracles.

The original long-Unicode/rejected-write/read-only/cancellation checks and the
owned cross-process EDIT/clipboard peer still run in every 25-group round.
All destructive fixtures run on a private noninteractive station. The probe
does not seed or read the user's interactive clipboard. Destroyed probe HWNDs
are no longer destroyed a second time by fixture Drop.

## App / compilation verification

- `windows-child-identity-app-build.log`: terminal exit 0, 2m40s. The existing
  `windows-test-manifest.xml` was embedded post-link with `mt.exe` before launch;
  no conflicting linker `MANIFESTINPUT` was added.
- `windows-child-identity-app-tests.log`: **92 passed, 0 failed, 1 explicitly
  ignored**, terminal exit 0, 65.70s. The native desktop fixture's existing ignore
  remains explicit; it is not counted as a pass. These include the existing
  input-slot ownership, UIA policy, WebView and session/MCP lifecycle regressions.
- `windows-child-identity-probe-clippy.log`: strict App all-targets + probe feature
  Clippy passes, terminal exit 0, 48.62s.
- `windows-child-identity-production-clippy.log`: normal production-library
  strict Clippy passes, terminal exit 0, 31.70s.
- Scoped `rustfmt --check` passes for all seven touched Rust files. These seven
  files were separately checked for LF and trailing whitespace; all are LF and
  have no trailing whitespace. Tracked `git diff --check` passes but does not
  cover the worktree's untracked files. No unrelated formatting rewrite.

All commands started for this batch have confirmed terminal handles. The
existing unrelated Cargo/App process was not stopped. No new native UI,
third-party window, signature, release or long-soak result is claimed.

## Evidence boundary / next work

- Native probes exercise production low-level helpers and exact-root binding;
  they do not prove the full Broker/ACP/model/installed-App path or every mouse/
  drag wrapper. The uncertainty flag is observed directly, not mislabelled as
  an installed-App Stop/recovery test. Existing slot ownership tests are separate.
- Real HWND reuse was not forced. Cross-thread races, third-party UIA provider
  identity, blocked synchronous native providers and proof-based recovery of
  retained uncertain slots still need coverage. Do not fix hangs by freeing an
  in-flight message buffer or falsely reporting idle.
- Remove the fixture-oriented named-`Drop` drag destination with a consistent
  protocol/adapter contract and regressions, not just a Windows-only silent change.
- Windows S4.5 foreground setup and native redesigned-UI acceptance remain open.
- macOS, GNOME Wayland, WebView immediate Stop/other-platform isolated worlds,
  signed installation/update/rollback, real Grok E4 and frozen 12-hour active soak
  remain full-goal requirements; none is replaced by this Windows helper batch.

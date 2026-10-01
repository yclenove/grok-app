# Computer Use — Windows clipboard preservation and recovery

September 25, 2026 (+08:00). Branch `feat/computer-use-implementation`, base
`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`. Existing dirty worktree preserved;
no commit, push or PR. Full objective remains **active / partial — not releasable**.
The requested UI/UX redesign and all original platform/release gates remain in scope.

## Production changes

- Replace Unicode-only backup with a bounded, materialized native format snapshot.
  Preserve text, HTML/RTF, bitmap, metafile and file-list representations without
  exporting bytes to a model, log, IPC channel or disk. Unsupported owner-dependent
  formats reject before clearing the clipboard. This is not arbitrary OLE-object support.
- Allocate task Unicode/locale/ANSI/OEM formats before mutation. The task receipt is
  captured under the OS lock; releasing that complete text set adds no implicit
  format-synthesis sequence changes. Ownership is exact HWND + sequence, not text
  equality or a permissive delta. Same-text user copies and same-owner metadata
  additions prevent restoration of an obsolete backup.
- Execute the requested paste once. Restore on operation failure and unwind;
  preserve a genuinely empty original as no formats. A busy clipboard reader does
  not discard the backup or release native input occupancy prematurely.
- Native fault injection exposed a second data-loss bug: failing after the first
  restored text format, closing the clipboard, then comparing the old receipt
  misclassified Windows' own format synthesis as an external copy. Remaining
  formats were lost. Recovery now holds the same OS lock until all saved handles
  transfer. It never empties partial recovery twice or replays the operation.
- Initial task publication failure also recovers completely under that original
  lock, preserves the original error, and does not dispatch the paste.

The recovery loop retains occupancy/backup while native publication remains
unavailable. Permanent resource failure, pending-recovery user experience, process
crash recovery, clipboard history/cloud exclusion, and delayed owner hangs are
still open; an unbounded retained operation is not final-version recovery UX.

## Native evidence

Logs: `tools/computer-use-probe/.run/linux-native-20260925/`. Despite that shared
directory name, the following runs are actual Windows processes.

- `windows-clipboard-isolated-final-{1,2,3}.log`: earlier 10-group native rounds,
  each exit 0. Those predate the partial-publication regression and are not its proof.
- `windows-clipboard-publication-red.log`: real isolated native run exit 1;
  the HTML format was missing after injected restoration failure. The earlier
  ten groups passed. The red evidence is retained unchanged.
- `windows-clipboard-publication-green-{1,2,3}.log`: three terminal exit-0 rounds,
  **12 groups each**, including partial restoration and failed initial publication
  plus failed rollback. Successful actions run once; initial failure runs zero times.
- Probe SHA-256 for those green rounds:
  `63e48371c89a5c703e3b9fc9f52ea999991b012807d41a74f1a3f6c360f28927`.
- Each destructive clipboard probe creates a private noninteractive window station
  and desktop. It cannot fall back to WinSta0. The user's clipboard is not seeded
  with fixture data. Expected caught-panic output is part of the unwind assertion,
  followed by passing checks and terminal exit 0.
- Earlier `windows-clipboard-isolated-r2.log` and `-r3.log` failures remain:
  the cross-thread fixture needed to pump owner notifications, and implicit text
  synthesis changed the receipt. Neither was fixed by relaxing an assertion.
- `windows-clipboard-core.log`: **3/3** exact-sequence policy tests, no ignored tests.
- `windows-clipboard-current-unit.log`: **86/86** CU-filtered App tests before the
  explicit-native-test correction below. Do not treat that count as native UI proof.
- Final `windows-clipboard-final-unit.log`: **85 passed, 1 explicitly ignored**,
  no failures, terminal exit 0 (65.52 s). The separate native fixture result below
  is not folded into this unit count.
- Strict App Clippy passed with the probe feature/all targets
  (`windows-clipboard-final-clippy.log`, 16.49 s) and with normal production
  library features (`windows-clipboard-production-clippy.log`, 43.46 s).
  Both commands completed with exit 0; neither substitutes for native UI gates.

## Native fixture honesty and foreground diagnosis

The old `fixture_window_is_enumerable` silently returned success if its window
failed to start, or when an environment flag was present. It also accepted any
similarly named fixture. It is now an explicitly ignored-by-default native test,
with no success-on-error path. Explicit execution requires the exact title,
process and HWND, and proves that closing the window retires its identity.
`windows-fixture-explicit-native.log`: **1/1**, exit 0, 0.14 seconds.
The default unit report must count this separately, not hide it among passes.

`windows-native-regression-r3.log` passed clipboard, focus drift, takeover and
security/cancel checks, then failed S4.5 before its first action. A targeted
`windows-s45` gate and payload-free activation diagnostics now isolate that phase.
`windows-s45-diagnostic-r3.log` still exits 1: the exact target is alive, visible
and not minimized; attaching to the current foreground thread returns
`0x80070005` (access denied), and SetForegroundWindow is unsuccessful. This is
evidence of a failed native test setup, not proof that S4.5 actions work, nor
proof that every prior intermittent failure has the same cause. No escalation,
desktop fallback, timeout increase or production foreground stealing was added.
The temporary BOOL/Result compile mismatch in the diagnostic build is retained
in `windows-s45-diagnostic-build-r2.log` and corrected in the succeeding build.

## Remaining acceptance

- Complete S4.5 on an explicitly available interactive foreground; keep failure
  evidence. Audit native child-lifetime and input postconditions, including the
  legacy substring text check and fixed Drop-name fallback.
- Finish bounded native recovery and user-visible cleanup state without reporting
  idle while effects or clipboard restoration are still pending.
- Redesigned UI still needs installed-App, OS scaling, overlay and permission
  acceptance. The computer-use skill was read; attempting its documented JS
  initialization failed because `tools.node_repl` is not a function in this tool
  surface. Terminal/filesystem/build tools work. No alternative desktop helper
  was spawned and no permission/security setting was changed. Existing mocked
  Host browser screenshots are not upgraded to native acceptance.
- macOS native adapter, GNOME Wayland, WebView immediate Stop and other-platform
  isolated worlds, signed install/update/rollback, actual model E4 and final
  frozen 12-hour active soak remain required. No subset closes the full goal.

# Computer Use R2C current-tree audit

> Date: 2026-09-13
> Workspace: `H:\aicoding\grok-app-computer-use`
> Branch: `feat/computer-use-implementation`
> HEAD: `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`
> Scope: current-tree review and independent focused verification
> Publication boundary: no commit, no push, no PR, no merge, no release

## 1. Executive verdict

The only accurate top-level status is:

```text
partial - not releasable
```

The latest implementation has materially improved the local lifecycle path, but the R2B
two-day goal was not completed. In particular:

- no fresh `tools/computer-use-probe/.run/r2b/<run-id>/` evidence root exists;
- no production-wired authorization phase fault matrix exists on the current tree;
- no candidate fingerprint was frozen after the latest source changes;
- the available App-shell report predates the latest source changes and is stale;
- the required Core Clippy gate is red;
- A3.2 cancel tombstones are still the globally evictable FIFO called out by the R2B plan;
- there is no current-candidate active soak, real-model E4, installed E5, or four-platform
  acceptance evidence.

Passing unit, fixture, scripted-agent, or one old App-shell run does not change that verdict.

## 2. Review boundary and live tree

This review preserved the complete dirty worktree. It did not run reset, restore, clean,
stash, checkout, commit, push, PR, merge, tag, publish, or release.

Observed live state:

- branch and HEAD remain the values in the header;
- `git status --short` contains 68 tracked modification rows and 83 untracked rows;
- many Computer Use source files are untracked, so `git diff --stat` is not a complete
  inventory of the feature;
- `tools/computer-use-probe/.run/r2b` does not exist;
- the existing Vite process was left running and no unrelated process was stopped;
- no account, Cookie, token, browser profile, shared `~/.grok`, proxy, or VPN state was read
  or changed.

## 3. What the latest work actually completed

### 3.1 Stable run credential rendering

`src-tauri/computer-use-core/src/ipc.rs` now exposes a stable credential for the current
session/run, and `src-tauri/src/computer_use/inject.rs` uses it when rebuilding the full ACP
MCP catalog. This fixes the immediate behavior where every catalog rebuild rotated the token
before ACP had committed the replacement.

This is useful implementation progress, but the current tests do not yet exercise the entire
production chain with real IPC calls at every ACP outcome. It therefore remains `partial`, not
a completed transactional-authority gate.

### 3.2 Lifecycle wiring

The current tree adds or improves wiring for:

- ordinary chat Stop and handshake Stop;
- desired-present and desired-absent MCP reconciliation after reconnect;
- live, background, and parked ACP endpoint lookup;
- delete-before-journal cleanup ordering;
- background and parked soft-respawn cleanup;
- bounded cooperative App-exit catalog cleanup;
- updater relaunch cleanup;
- full-catalog replacement with desired/applied generation tracking.

The code is meaningfully closer to one lifecycle, but most branches lack a production-wired
fault test. Several entry points are still validated by direct helper tests or one scripted
App run rather than phase-by-phase postconditions.

### 3.3 Cleanup Retry A3.1

The latest tree has a real vertical slice:

- Host: `SessionManager::retry_computer_use_cleanup`;
- Tauri: `computer_use_retry_cleanup`;
- frontend API: `computerRetryCleanup`;
- panel: distinct authorizing, stopping, cleanup-pending, and retrying states;
- retry remains available when `stopState=stopped`;
- conflicting selector, refresh, pair, and bind controls are locked;
- duplicate clicks are fenced with a synchronous ref;
- late responses after a chat switch or unmount cannot update the new panel;
- the displayed cleanup error is a Host-generated redacted category;
- the five new keys exist in all 15 locale catalogs.

The Host tests cover convergence, repeated retry, and a newer authorization winning the race.
The panel tests cover keyboard activation, conflicting-control locking, duplicate clicks, and
chat switching. This is an implemented A3.1 slice, but command-level failure and current-App
evidence are still missing, so it is not yet an independently closed release gate.

### 3.4 Blocking HTTP repair and App-shell diagnostic

The earlier Tokio runtime-drop panic was addressed in the loopback worker path, and the
finalization driver now points at the current source-built ACP stub instead of the September
11 run artifact. A clean isolated runtime repair also succeeded.

The available report
`tools/computer-use-probe/.run/finalization/20260913T044405Z-seven/logs/F1.appshell-08-report.json`
records one scripted App-shell round with normal Stop catalog cleanup and App-exit cleanup.
However, `ipc.rs`, `inject.rs`, `ComputerPanel.tsx`, and `session_manager/computer_use.rs` were
modified after that run. The report is therefore `stale` for the current candidate.

## 4. Independent verification snapshot

| Check | Result | Evidence boundary |
| --- | --- | --- |
| `cargo fmt --all -- --check` | passed | formatting only |
| `pnpm typecheck` | passed | TypeScript compile only |
| `ComputerPanel.test.tsx` | 18/18 passed | jsdom/component E1 |
| Core focused `session_grants` | 14/14 passed | current tombstone behavior only |
| Core full tests | 311/311 passed | E1/E2; includes fixtures, not installed App |
| Driver tests | 12/12 passed | private worker protocol E2 |
| App `session_manager::computer_use` with CI-style manifest embedding | 10/10 passed | App library integration E1/E2 |
| Core Clippy with `-D warnings` | failed, 3 errors | release quality gate is red |
| Current-fingerprint App-shell | not run | older report is stale |
| R2B atomic manifest | absent | no R2B evidence root exists |
| A6 active soak | not run | no frozen current candidate |
| real-model E4 | not run | requires explicit user authorization |
| installed/upgrade/rollback E5 | not run | no current installed candidate |
| macOS arm64/x64 | not run | external platform evidence absent |
| Linux X11 | not run | external platform evidence absent |
| GNOME native Wayland | not run | external platform evidence absent |

The App Rust harness was executed directly only after embedding
`src-tauri/windows-test-manifest.xml` with the Windows SDK `mt.exe`. A plain local
`cargo test --lib` remains subject to the known `0xc0000139` Common Controls startup problem;
that environment failure must never be reported as a passed assertion.

## 5. Findings, ordered by severity

### P0. The R2B completion claim has no current-run evidence chain

The R2B prompt required a new ignored evidence root, scenario-derived manifest, checkpoints,
candidate fingerprint, phase fault matrix, complete gates, and at least two hours of active
soak. The root does not exist. The only App-shell report available predates the latest source
changes. This invalidates any R2B `complete`, `passed`, or `releasable` conclusion regardless
of later unit-test output.

Required response: start a fresh R2C evidence root and treat every unexecuted row as
`not_run`, not inherited success.

### P1. Cancel-before-begin protection is still cross-session evictable

`src-tauri/computer-use-core/src/session_grants.rs:38-77` stores every session in one global
`VecDeque<(session, attempt, revision)>` and evicts the oldest row above 256. A noisy or hostile
session can therefore remove another session's still-valid cancel-before-begin tombstone.
The delayed authorize command can then open a run the user already cancelled.

There is also no TTL, no injectable clock, no exact-consume behavior, and no
`SessionGrants::forget_session`. Successful session deletion removes manager bookkeeping but
leaves the process-global grant slot in `tracked_sessions()`.

Required response: implement A3.2 before any Existing Tabs work.

### P1. Ordinary chat Stop still performs surface cleanup synchronously inside async code

`src-tauri/src/session_manager/turn.rs:846` calls `sessions::revoke` directly from the async
Stop path. `SessionGrants::revoke_checked` can call `ComputerUseBroker::request_stop`, and
`request_stop` performs adapter and browser cleanup synchronously. The HTTP implementation no
longer drops a Reqwest runtime on the Tokio worker, but a slow or hung managed worker can still
occupy the async worker before the Ready snapshot is emitted.

The present tests prove that the call no longer panics and that timeout remains fail-closed;
they do not prove prompt UI acknowledgement or event-loop responsiveness under a blocked
surface cleanup.

Required response: split immediate authority fencing from bounded remote/resource cleanup,
or run the blocking cleanup on an owned blocking boundary while preserving exact generation
and cleanup-pending semantics.

### P1. The Core Clippy release gate is red

Current command:

```text
cargo clippy -p grok-computer-use-core --all-targets --all-features --locked --offline -- -D warnings
```

fails at `src-tauri/computer-use-core/src/ipc.rs:144`, `:169`, and `:198` for
`clippy::filter_map_bool_then`. Unit tests and formatting do not override this failure.

Required response: make the three equivalent iterator rewrites without adding an allow or
weakening `-D warnings`, then rerun the complete Core gate.

### P1. Credential behavior is not verified through the production fault outcomes

The stable per-run token is a reasonable simplification, but the evidence is split:

- IPC tests prove same-run reuse and explicit rotation/revocation;
- reconciler tests use an injected catalog builder and ACP TCP stub;
- no test combines the production entry renderer, real credential registry, ACP apply/error/
  timeout outcomes, and actual authenticated IPC calls.

This leaves the critical claims unproven: applied-but-response-lost keeps exactly the intended
authority; definitive rejection retains only the previously committed authority; Stop during
update invalidates every stopped-run token before dispatch; and a late A cleanup cannot revoke
B.

Required response: add production-wired token-oracle tests before candidate freeze.

### P1. Lifecycle entry points lack the required phase fault matrix

Stop, delete, reconnect, soft-respawn, feature-off, context/model change, updater restart, and
App exit have new code paths, but there is no deterministic construction/transport seam that
pauses them before and after each side effect. The one App-shell round cannot establish the
failure ordering or multi-session isolation required by R2B A4.

Required response: build the matrix and assert actual ACP catalog, IPC authority, Broker
state, resource ownership, retry ledger, and post-stop dispatch for each row.

### P2. Cleanup Retry still has coverage holes

There is no direct Tauri command contract test, no panel test for a rejected retry that remains
cleanup-pending, and no explicit unmount-while-retrying assertion. Chat-switch coverage is
useful but is not the same as every requested lifecycle path.

Required response: close these tests in the first R2C repair batch; do not redesign the panel.

### P2. The finalization runner is machine-specific

`tools/computer-use-probe/finalization_protocol.py:23-27` hardcodes the current Windows user
scratch path, baseline HEAD, and expected branch. The stub path is now current-source, but the
runner still cannot be treated as a portable release harness or handed to another machine
without editing source.

Required response: accept validated CLI parameters or derive safe defaults inside the repo;
keep destructive cleanup limited to a resolved, owner-marked ignored run root.

## 6. Corrected status matrix

| Work item | Current status | Reason |
| --- | --- | --- |
| A0 red tests/evidence protocol | partial | useful tests exist; no R2B manifest or red-first ledger |
| A1 stable credential behavior | partial | immediate rotation bug addressed; production outcome matrix missing |
| A2 lifecycle wiring | partial | many entry points wired; blocked-cleanup ordering not fully proven |
| A3.1 cleanup Retry | partial, locally implemented | Host/UI tests green; command/current-App failure evidence missing |
| A3.2 tombstones and deletion forget | not implemented | global FIFO remains |
| A4 Tauri phase fault matrix | not implemented | no deterministic production matrix |
| A5 complete quality gates | blocked | Core Clippy fails |
| A6 freeze and active soak | not run | no current candidate fingerprint |
| Existing Tabs typed transport | partial skeleton | pairing/transport/install/live browser incomplete |
| Windows source App-shell | stale | prior one-round report predates source changes |
| Windows installed | not run | no installed candidate evidence |
| macOS arm64/x64 | not run / blocked external | no hardware evidence |
| Linux X11 | not run / blocked external | no environment evidence |
| GNOME native Wayland | not run / blocked external | no environment evidence |
| real model | not run | no authorization in this run |

## 7. Required next order

1. Create a fresh R2C evidence root and baseline; do not reuse finalization or R2B claims.
2. Restore the Core Clippy gate and close A3.1 test gaps.
3. Implement A3.2 per-session TTL tombstones and deletion cleanup.
4. Remove synchronous surface cleanup from async Stop's immediate-acknowledgement path.
5. Add production-wired credential and lifecycle phase fault tests.
6. Run the complete quality gates and rebuild the branch App.
7. Freeze one candidate and run fresh App-shell smoke plus the required active soak.
8. Only after Track A passes may work continue into Existing Tabs transport.
9. Keep installed, real-model, macOS, Linux X11, and native Wayland rows honest and separate.

The detailed sequence is in
`docs/plans/2026-09-13-computer-use-grok-r2c-execution.md`. The paste-ready Goal prompt is in
`docs/plans/2026-09-13-computer-use-grok-r2c-prompt.md`.


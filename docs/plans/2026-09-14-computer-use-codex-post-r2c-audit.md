# Computer Use post-R2C current-tree audit

> Date: 2026-09-14
> Workspace: `H:\aicoding\grok-app-computer-use`
> Branch: `feat/computer-use-implementation`
> HEAD: `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`
> Scope: read-only source review plus focused current-tree verification
> Publication boundary: no commit, no push, no PR, no merge, no tag, no release

## 1. Executive verdict

The only accurate overall status is:

```text
partial - not releasable
```

R2C produced real implementation progress. It did not complete the R2C execution plan and it
did not create a releasable candidate. The current tree has a useful Core/Broker foundation,
working per-session cancellation tombstones, asynchronous cleanup hand-offs, Cleanup Retry,
and broad unit/component coverage. It also has two current lifecycle bugs, one reproducible App
test failure, red release gates, a stale seven-row evidence root, and no current-candidate native
or installed acceptance chain.

`Grok finished the task` can only mean that the coding turn stopped. It cannot mean that the
48-hour goal, Track A, Existing Tabs, or the cross-platform release contract completed.

## 2. Review boundary and live tree

This review preserved the complete dirty worktree. It did not run reset, restore, clean, stash,
checkout, pull, rebase, commit, push, PR, merge, tag, publish, or release.

Observed current state:

- branch and HEAD remain the values in the header;
- `git status --porcelain=v1` contains 69 tracked modification rows and 86 untracked entries;
- many Computer Use source files are untracked, so `git diff --stat` is not a complete feature
  inventory;
- no source or product document was changed before this audit was written;
- test build artifacts were allowed to update under ignored Cargo/Vite directories;
- no real account, Cookie, token, browser profile, shared `~/.grok`, proxy, or VPN state was read
  or changed.

## 3. What Grok actually completed

### 3.1 Session/run authority and cancellation

The current Core implements materially stronger authorization behavior:

- one stable IPC credential is reused for the current session/run until explicit revocation;
- cancellation tombstones are bucketed by session instead of one cross-session FIFO;
- tombstones have a five-minute TTL, a per-session cap of 64, and a global cap of 256;
- capacity saturation fails closed instead of silently evicting another session's cancellation;
- exact cancel-before-begin is consumed once and does not cancel a successor attempt;
- successful session deletion calls `forget_session` and removes the process-local slot,
  tombstones, and pending cleanup entry;
- a fresh `SessionGrants` registry does not restore old authorization state after restart.

These behaviors are covered by deterministic fake-clock and capacity tests. This closes the
local A3.2 slice from the previous audit.

### 3.2 Stop fencing and Cleanup Retry

The implementation now separates the immediate local fence from slower adapter/browser cleanup:

- Stop revokes the run and loopback credential before slow surface cleanup;
- generation-bound cleanup tickets are retained across failure;
- slow cleanup runs behind `spawn_blocking`;
- desired-absent ACP reconciliation and surface cleanup retain distinct pending state;
- Retry only follows an already-recorded desired-absent generation;
- Retry cannot create a run, choose a target, authorize, or revive a stopped run;
- panel busy, duplicate-click, keyboard, chat-switch, and late-response behavior has component
  coverage.

This is useful and mostly coherent. It is not a complete lifecycle because handshake process
termination and unexpected ACP exit are still wrong, as described below.

### 3.3 Additional lifecycle wiring

The current tree also contains code for:

- feature-off fencing;
- normal and model Stop paths;
- context/model/compact invalidation;
- WebView unbind;
- desired-absent retry after reconnect;
- live, background, and parked ACP lookup;
- soft-respawn cleanup;
- updater relaunch cleanup;
- an App-exit catalog barrier;
- shared-process-aware catalog replacement.

Most of those paths have helper/unit coverage. They still lack one current-fingerprint,
production-wired phase matrix and current App-shell evidence.

## 4. Current verification snapshot

The following checks were run against the current source tree. The Windows App harness was
post-linked with `src-tauri/windows-test-manifest.xml` using `mt.exe` before direct execution.

| Check | Current result | Evidence boundary |
| --- | --- | --- |
| Core tests | 325/325 passed | E1/E2 Core, fixtures included |
| private Driver integration tests | 12/12 passed | E2 child process/pipe behavior |
| targeted Computer Use frontend tests | 47/47 passed in 9 files | jsdom/component E1 |
| full frontend suite | 7106/7106 passed | broad frontend regression; no native App |
| App lib harness excluding the known runtime test | 1630 passed, 1 ignored, 1 filtered | current Windows App test binary |
| App runtime repair test | 1 failed | real assertion failure after manifest embedding |
| Core Clippy `-D warnings` | passed | Core only |
| App Clippy `-D warnings` | failed | 55 lib errors; 49 lib-test errors |
| Rust fmt | passed | formatting only |
| TypeScript typecheck | passed | compile only |
| ESLint | passed | frontend lint only |
| dependency hygiene | passed | package-manager hygiene only |
| production dependency audit | failed | 1 high vulnerability |
| final code-quality gate | failed | files >=1000 lines: 82, budget 80 |
| `git diff --check` | no whitespace error | line-ending conversion warnings remain |
| current-fingerprint App-shell | not run | no valid current report |
| frozen candidate active soak | not run | no freeze exists |
| installed/upgrade/rollback | not run | no installed candidate |
| real-model E4 | not run | requires explicit human authorization |
| macOS arm64/x64 | not run / blocked external | no device evidence |
| Linux X11 | not run / blocked external | no environment evidence |
| GNOME native Wayland | not run / blocked external | no environment evidence |

Passing Core and frontend tests does not override a failed App assertion, failed Clippy,
failed dependency audit, failed repository quality gate, or missing native evidence.

## 5. Findings, ordered by severity

### P0. ACP process exit does not synchronously revoke Computer Use authority

The live `ProcessExited` branch in `src-tauri/src/session_manager/events.rs:911` clears FSM/UI
state and drops the ACP slot, but it never fences the session's Computer Use run, revokes its
loopback credential, publishes desired-absent, or schedules its generation-bound Broker cleanup.
The background branch in `events_bg.rs:590` has the same omission. Parked co-tenants are removed
at the start of process-scoped routing without any authorization fence.

This violates the documented reconnect/process-loss contract. A dead ACP can leave an active
Broker grant and valid Bearer behind. An orphaned descendant or any process that retained the
credential can therefore continue reaching the loopback Broker until another lifecycle action
happens to revoke it. A shared ACP process can leave several App sessions in that state.

Required repair:

- snapshot and deduplicate every live/background/parked App session owned by the exact process
  incarnation before removing ownership maps;
- synchronously fence dispatch and revoke every matching IPC credential;
- publish desired-absent for each matching run;
- treat the already-dead ACP endpoint separately from catalog transport cleanup;
- complete generation-bound surface cleanup asynchronously;
- ensure a late exit event from process A cannot revoke a new run on replacement process B;
- cover live, background, parked, shared-process, cleanup-failure, and late-event races with
  deterministic barriers and actual credential calls.

### P1. Handshake Stop does not durably terminate the exact ACP process

`SessionManager::stop` sends handshake Stop through
`spawn_fenced_computer_use_stop(..., kill_handshake_acp=true)` at
`src-tauri/src/session_manager/turn.rs:866`. The helper returns immediately when the Computer Use
cleanup plan is a no-op (`session_manager/computer_use.rs:417`). That is the common handshake case:
there may be no Computer Use run yet, so the ACP process is never killed.

When a plan is non-empty, the helper kills ACP only if every catalog/resource cleanup step
succeeds (`computer_use.rs:431`). A cleanup-pending result therefore also loses the kill intent,
and the explicit Retry path does not carry it. Worse, the helper looks up and takes the session's
current ACP only after asynchronous cleanup. A reconnect can install process B before process A's
task resumes, allowing the late Stop task to kill the replacement process.

Required repair:

- make handshake termination independent from whether the Computer Use plan is empty or clean;
- synchronously capture the exact ACP/process incarnation before returning the Stop snapshot;
- never look up an unqualified current ACP from a delayed task;
- kill only an exclusive exact owner; do not kill a shared process with a live co-tenant;
- retain catalog/resource cleanup errors for Retry without retaining a live old handshake;
- prove no-op, detach failure, resource failure, duplicate Stop, shared process, and A-to-B
  reconnect races.

### P1. The shipped runtime repair App test restores its environment too early

`src-tauri/src/computer_use/runtime.rs:178-185` restores `PATH` and `GROK_APP_HOME`, while the
test does not resolve `PLAYWRIGHT_ARCHIVE` until line 199. With the proper Windows manifest
embedded, the exact test fails with:

```text
playwright-core archive must resolve after repair:
Computer Use runtime (active pack) is missing from the App private directory
```

This is a real assertion failure, not `0xc0000139`. Manual restoration is also not panic-safe:
an earlier assertion panic can leak modified process environment into later tests while the
global lock becomes poisoned.

Required repair: use a scoped, drop-based environment guard under the existing global lock,
keep both variables redirected through every assertion, restore them on success and unwind, and
add an explicit restoration-after-panic test without weakening test isolation.

### P1. The App Clippy release gate is red

Current command:

```text
cargo clippy -p grok-app --all-targets --locked --offline -- -D warnings
```

fails with 55 errors for the lib target and 49 for the lib-test target. The largest group is
probe/test-only code compiled into the normal App target but used only behind the
`computer-use-probe` entrypoint. Other errors include `too_many_arguments`, `manual_inspect`,
`needless_question_mark`, `manual_clamp`, `let_unit_value`, `needless_borrow`,
`double_ended_iterator_last`, and `filter_map_bool_then`.

Required repair: classify each symbol as production, test-only, or probe-only; wire production
code, put test helpers under `cfg(test)`, and gate/move probe code behind the existing non-default
`computer-use-probe` feature or the `cu-probe` crate. Apply equivalent lint rewrites. Do not add
blanket `allow`, weaken `-D warnings`, or ship fixture/gate code in the release App merely to
silence the errors.

### P1. The R2C evidence chain stopped before implementation work

The only R2C root is:

```text
tools/computer-use-probe/.run/r2c/20260913T061946Z-seven
```

Its append-only manifest has seven rows: D0 baseline/evidence checks and the old expected-red
Core Clippy reproduction. `state.json` leaves D1 through D14 `not_started`. It contains no later
checkpoint, recovery, quality run, App-shell, freeze, soak, report, or final manifest. Its last
timestamp is 2026-09-13 14:20 local; current source files were modified through 2026-09-14 00:04.

Required repair: create a separate R2D root. Do not append current claims to R2C, reuse its stale
fingerprint, or inherit old reports as passes.

### P1. Production dependencies contain a known high-severity issue

`pnpm audit:prod` reports one high-severity advisory:

- package: `@tiptap/core 3.30.4`;
- issue: quadratic ReDoS in block and inline Markdown attribute parsing;
- affected: `>=3.7.0 <3.30.5`;
- patched: `>=3.30.5`;
- advisory: `GHSA-j95f-988m-3j2f`.

The Tiptap packages need a compatible, lockfile-consistent patch upgrade followed by the full
editor/frontend regression suite. The audit must return zero at the configured threshold.

### P1. Existing Tabs pairing cannot be promoted into a real transport

The current extension is a fixture skeleton, and its pairing proof is not secure enough for a
working transport:

- `pairing_headers_allow` accepts requests with no `Origin` and accepts arbitrary loopback page
  origins (`computer-use-core/src/ipc.rs:421-437`);
- the public challenge exposes the nonce and instance to those callers;
- `/cu/pairing-confirm` accepts a `response` field but explicitly ignores it;
- `pair.js` automatically calls extension confirmation when no `#cu-confirm` button exists;
- App confirmation happens immediately after the user clicks the App button;
- `/cu/pairing-session` then returns the session key to the loopback caller that knows only public
  challenge data;
- extension id and Origin strings are treated as checks, but the HTTP caller has not proved it is
  the installed extension.

A local process can race the confirmed five-minute window and obtain the pairing key. The impact
is currently limited because no real tab action transport or production ExistingTab executor is
registered. The design becomes a security boundary as soon as those are added, so the pairing
flow must be replaced before Track B implementation.

### P2. Existing Tabs is still a state-machine/fixture skeleton, not a user feature

The extension manifest has no `activeTab`, `scripting`, or `tabs` capability. `sw.js` only stores a
key in memory. There is no authenticated typed request/result channel, tab picker feed, DOM/AX
observation, screenshot capture, action execution, cancellation, heartbeat, reconnect, or
document-generation invalidation. Product startup registers Desktop, Managed Browser, and WebView
executors, but no `SurfaceKind::ExistingTab` executor.

Core tests prove that a hypothetical borrowed tab is isolated and returned. They do not prove
that Chrome/Edge can share or operate a real tab. Existing Tabs remains `not implemented` at the
product level.

### P2. Repository size quality regressed and probe ownership is blurred

The final quality gate reports 82 files at or above 1000 lines against a budget of 80. Four
Computer Use files are themselves over the threshold:

- `src-tauri/src/session_manager/computer_use.rs`: 1979 lines;
- `src-tauri/src/computer_use/app_shell.rs`: 1826 lines;
- `src-tauri/src/computer_use/webview.rs`: 1222 lines;
- `src-tauri/src/computer_use/windows_adapter.rs`: 1073 lines.

At least two genuine domain extractions are required to restore the repository budget. Moving
arbitrary lines or creating forwarding-only files does not satisfy the architectural intent.
Probe/App-shell scenarios should be owned by the probe surface; production modules should own
only shipping behavior and focused tests.

### P2. Release acceptance remains external and unexecuted

There is no frozen current candidate, current App-shell smoke, two-hour active soak, installed
Windows flow, real-model E4 run, macOS arm64/x64 run, Linux X11 run, or native GNOME Wayland run.
Cross-compilation, fake adapters, Chrome for Testing, XWayland, and Windows results cannot fill
those rows.

## 6. Corrected completion matrix

| Work item | Current status | Reason |
| --- | --- | --- |
| protocol/schema/Broker foundation | locally implemented | Core 325/325; release evidence still separate |
| private worker/driver | locally implemented | Driver 12/12 |
| session credential reuse | locally implemented, matrix incomplete | unit behavior green; lifecycle crash gap remains |
| A3.1 Cleanup Retry | locally implemented | Host/UI behavior exists; full phase matrix absent |
| A3.2 tombstones and forget | locally implemented | TTL/capacity/fail-closed tests green |
| immediate Stop authority fence | partial | ordinary path improved; process/handshake edges fail |
| handshake Stop | failed | old process can survive; late task can target replacement |
| ACP crash authority revocation | failed | live/background/parked sessions are not fenced |
| runtime pack App test | failed | environment restored before final resolution |
| App quality gates | failed | Clippy, dependency audit, and file budget red |
| lifecycle phase fault matrix | partial fixtures only | no production-wired current-candidate matrix |
| R2C evidence protocol | abandoned after baseline | 7 rows; D1-D14 not started |
| Windows source App-shell | stale / not run current | existing reports predate current source |
| Managed Browser | partial local source | useful worker/Core paths; current App acceptance absent |
| App WebView | partial local source | typed adapter exists; current App acceptance absent |
| Existing Tabs | not implemented as product | insecure pairing skeleton; no transport/executor |
| Windows installed | not run | no candidate package |
| macOS arm64/x64 | not run / blocked external | no hardware evidence |
| Linux X11 | not run / blocked external | no environment evidence |
| GNOME native Wayland | not run / blocked external | native implementation/evidence absent |
| real model | not run | requires explicit user authorization |

## 7. Required next order

1. Start a fresh R2D evidence root and write executable red tests for the three confirmed bugs.
2. Repair panic-safe runtime test isolation.
3. Repair handshake Stop ownership and process-exit authority fencing before any browser feature.
4. Restore App Clippy without lint waivers and move probe-only code out of the release target.
5. Split genuine Computer Use domains until the final file-count gate is within budget.
6. Upgrade the Tiptap patch cohort and rerun the complete frontend/editor suite plus audit.
7. Build the production-wired credential/lifecycle fault matrix.
8. Freeze one candidate, run fresh App-shell smokes, then accumulate at least two hours of active
   fault/lifecycle soak on that exact fingerprint.
9. Only after Track A is green may Existing Tabs pairing and typed transport start.
10. Keep installed, real-model, macOS, Linux X11, and native Wayland rows honest and separate.

The detailed sequence is in
`docs/plans/2026-09-14-computer-use-grok-r2d-execution.md`. The paste-ready Goal prompt is in
`docs/plans/2026-09-14-computer-use-grok-r2d-prompt.md`.

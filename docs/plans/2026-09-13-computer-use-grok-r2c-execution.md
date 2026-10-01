# Computer Use R2C 48-hour execution plan

> Date: 2026-09-13
> Workspace: `H:\aicoding\grok-app-computer-use`
> Branch: `feat/computer-use-implementation`
> Baseline HEAD: `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`
> Required input audit: `docs/plans/2026-09-13-computer-use-codex-r2c-audit.md`
> Publication boundary: no commit, no push, no PR, no merge, no tag, no release

## 1. Objective

Continue the current dirty Computer Use implementation for up to 48 hours without repeating
the previous pattern of editing for less than an hour and declaring the two-day task complete.

The primary objective is to close Track A lifecycle integrity on one current fingerprint:

```text
authority fence -> desired catalog -> ACP replacement -> resource cleanup -> verified absence
```

Track B Existing Tabs may begin only after every Track A exit condition is green. Four-platform
release acceptance remains mandatory for v1, but unavailable machines must be reported as
`blocked_external` or `not_run`; Windows evidence cannot be copied into those rows.

Allowed top-level conclusions remain:

```text
Track A lifecycle gate passed; overall Computer Use partial - not releasable
Track A+B local source gate passed; overall Computer Use partial - not releasable
partial - not releasable
```

## 2. Non-negotiable boundaries

### 2.1 Git and workspace

- Preserve every existing tracked, untracked, and ignored artifact.
- Do not run reset, restore, clean, stash, force checkout, rebase, merge, or worktree rebuild.
- Do not commit, push, open a PR, tag, publish, install, or release.
- Do not pull or merge upstream during this run.
- Read a file and its overlapping diff before editing it.
- If another writer is changing the same file, stop editing that file and work on an independent
  ready item.
- Generated binaries, logs, temporary profiles, screenshots, and evidence belong only under
  the ignored R2C run root.

### 2.2 User environment

- Use a unique `GROK_APP_HOME`, App identity, ACP stub home, browser profile, ports, and lease.
- Do not read or copy the user's real Grok/Chrome/Edge profiles, shared `~/.grok`, Cookie,
  token, credential store, proxy, or VPN configuration.
- Do not operate the user's normal Grok App, browser tabs, ChatGPT, WeChat, or other apps.
- Track every PID and port started by this run. Clean up only an exact, verified child created
  by this run.
- Never kill unrelated Node, Cargo, rustc, browser, Codex, shell, or Grok App processes.
- Run Cargo, branch App, browser, and packaging-heavy jobs serially.

### 2.3 Product safety

- Computer Use remains default-off and local-interactive-only.
- Normal/YOLO/accept-edits never imply Computer Use authorization.
- The model cannot authorize, resume, reconnect, expand a target, confirm pairing, or retry a
  denied user choice.
- A target mismatch, stale generation, dead target, unsupported surface, or unavailable
  executor fails before side effects.
- Existing Tabs, Managed Browser, and App WebView never fall back to Desktop.
- Stop synchronously fences dispatch before any asynchronous cleanup begins.
- Borrowed tabs are returned, not closed. App-owned profiles may be closed only by their owner.
- No arbitrary shell, remote JavaScript eval, Cookie/storage export, arbitrary filesystem path,
  secret-bearing URL, or unrestricted download.

### 2.4 Evidence honesty

- Unit/mock/jsdom is E1. Real module isolation is E2. Scripted agent on branch App is E3.
- Real model plus human authorization is E4 and is not authorized by this plan.
- Installed/update/rollback/task-matrix evidence is E5 and stays separate from source-App E3.
- `exit 0`, a non-empty report, a skipped row, missing hook, empty target list, or old report is
  not proof of a postcondition.
- Any production/test/runner/config/runtime source change invalidates the candidate fingerprint
  and all candidate soak accumulated before that change.
- Keep first failure. A later pass may append recovery evidence but never erase history.

## 3. Mandatory evidence protocol

Before editing product code, create one new ignored root:

```text
tools/computer-use-probe/.run/r2c/<UTC-run-id>/
```

It must contain:

```text
owner.json
baseline/status.txt
baseline/head.txt
baseline/fingerprint.json
baseline/untracked-source.json
manifest.jsonl
state.json
checkpoints/
logs/
failures/
metrics/
reports/
```

`owner.json` records canonical repo, branch, HEAD, start time, host, OS, arch, writer PID,
evidence schema, and an exact cleanup allowlist. It contains no secret.

Each manifest line is append-only and contains at least:

```json
{
  "schema": 1,
  "seq": 1,
  "phase": "R0",
  "batch": "R0.1",
  "scenario": "baseline",
  "status": "validated",
  "evidenceLevel": "E0",
  "startedAt": "UTC",
  "endedAt": "UTC",
  "activeDurationMs": 0,
  "exitCode": 0,
  "command": "redacted exact command",
  "fingerprint": "sha256",
  "log": "logs/R0.1.log",
  "bytes": 0,
  "sha256": "sha256",
  "postconditions": [],
  "firstFailure": null
}
```

Generate `state.json` and reports from manifest rows. Do not hand-edit optimistic batch states.
Parameterize any scratch mirror; do not hardcode a user profile path. Destructive cleanup is
allowed only after resolving the requested target beneath the owner-marked ignored run root.

## 4. 48-hour schedule

The times below are sequencing budgets, not permission to skip acceptance. If a batch finishes
early, continue to its negative cases and independent postconditions. Do not count build time,
idle time, sleep, a blocked Goal turn, or waiting for user input as active soak.

| Window | Work |
| --- | --- |
| 0-2h | R0 baseline, current failure reproduction, evidence tooling hardening |
| 2-5h | R1 restore Clippy and finish A3.1 tests |
| 5-10h | R2 A3.2 per-session TTL tombstones and deletion forget semantics |
| 10-16h | R3 non-blocking Stop/lifecycle coordinator repair |
| 16-24h | R4 real credential oracle and production phase fault matrix foundation |
| 24-30h | R5 full matrix, multi-session races, complete quality gates |
| 30-35h | R6 fresh App build, slash/App-shell smoke, candidate freeze |
| 35-39h minimum | R7 at least two hours active candidate soak plus analysis/repair budget |
| 39-47h | R8 Existing Tabs secure vertical slice only if Track A passed |
| 47-48h | R9 final manifest validation, residual inventory, handoff report |

If Track A is not green at hour 39, remain in Track A. Do not start Existing Tabs to make the
report look larger.

## 5. R0: baseline and executable failures

### R0.1 Read and inventory

Read in full before modifying:

1. `AGENTS.md`
2. `docs/llm-wiki/computer-use.md`
3. `docs/llm-wiki/i18n.md`
4. `docs/llm-wiki/dialogs.md`
5. `docs/plans/2026-09-13-computer-use-codex-r2c-audit.md`
6. this execution plan
7. `src-tauri/computer-use-core/src/session_grants.rs`
8. `src-tauri/computer-use-core/src/ipc.rs`
9. `src-tauri/src/computer_use/sessions.rs`
10. `src-tauri/src/session_manager/computer_use.rs`
11. `src-tauri/src/session_manager/turn.rs`
12. `src-tauri/src/session_manager/control.rs`
13. `src-tauri/src/commands/computer_use.rs`
14. `src/components/computer-use/ComputerPanel.tsx`
15. relevant tests and finalization runner

Use `rg` to enumerate every caller for Stop, cancel, revoke, delete, reconnect, soft-respawn,
feature-off, context/model/compact, logout/account/provider/data-root recycle, updater restart,
tray quit, window close, and App exit.

### R0.2 Reproduce current facts

Record, without fixing yet:

- Core Clippy's three `filter_map_bool_then` failures;
- the global 256-row tombstone eviction behavior with a deterministic red test;
- deleted-session grant bookkeeping remaining present with a deterministic red test;
- async Stop entering blocking surface cleanup before acknowledgement with a barrier-based red
  test, not a sleep race;
- absence of a current App-shell candidate after the latest source changes.

The red tests must compile and fail on a behavioral assertion. A compile error, missing fixture,
timeout caused by the test itself, or string-presence grep is not a valid red test.

## 6. R1: quality repair and A3.1 closure

### R1.1 Restore Core Clippy

Rewrite the three equivalent iterator expressions in `ipc.rs`. Do not add an allow attribute,
change lint configuration, remove a target, or weaken `-D warnings`.

Run:

```text
cargo fmt --all -- --check
cargo test -p grok-computer-use-core --all-features --locked --offline
cargo clippy -p grok-computer-use-core --all-targets --all-features --locked --offline -- -D warnings
```

### R1.2 Close Cleanup Retry tests

Add focused tests for:

- retry command rejects empty/non-local/wrong-state input without selecting or authorizing;
- ACP rejection leaves `cleanup_pending=true` with a redacted category and permits another retry;
- retry after a newer desired-present generation returns superseded and never sends absent over
  that new generation;
- direct Tauri command DTO maps desired/applied/pending/cleanup/error correctly;
- panel rejected retry remains operable and shows a useful redacted error;
- unmount during retry ignores completion and performs no new authorization;
- keyboard activation and rapid duplicate clicks remain one Host call.

Do not redesign the panel or add state to `App.tsx`/`AppWorkbench.tsx`.

## 7. R2: A3.2 per-session TTL tombstones

### R2.1 Data model

Replace the global tuple FIFO with explicit records grouped by App session:

```text
CancelTombstone {
  attempt_id,
  selector_revision,
  created_at,
  expires_at
}
HashMap<SessionId, VecDeque<CancelTombstone>>
```

Use an injectable monotonic clock. Production uses a monotonic system clock; tests advance a
fake clock and never sleep.

Recommended constants for v1:

- TTL: 5 minutes;
- per-session unexpired cap: 64;
- process-wide unexpired cap: 256.

The TTL is deliberately above the managed-open, Host command, and ACP update budgets while the
memory ceiling remains comparable to the old implementation.

### R2.2 Exact behavior

- Purge expired records on every begin, cancel, count/status, and forget operation.
- A duplicate cancel is idempotent: it adds no record and does not extend TTL.
- Exact begin match consumes that tombstone and rejects the begin as cancelled.
- A different attempt or selector revision does not consume another record.
- A full session bucket fails closed for new authorization in that session until expiry,
  exact consumption, or confirmed session deletion.
- A full global pool fails closed for every new authorization that cannot be proven uncancelled;
  it never evicts another session's unexpired tombstone.
- Capacity failure must be typed and observable; it must not silently drop the cancel.
- Process restart naturally starts empty. Do not persist authorization or tombstones to disk.
- `forget_session` removes the stopped session's slot and bucket only after cleanup and store
  deletion have both succeeded.
- Failed detach or failed store deletion retains the slot/bucket and retry state.

### R2.3 Required tests

1. session A cannot evict session B;
2. one-session cap fails closed;
3. global cap fails closed without eviction;
4. just-before-expiry remains cancelled;
5. exactly-at-expiry follows the documented boundary;
6. duplicate cancel neither grows nor refreshes;
7. cancel-before-begin rejects and consumes exact record;
8. exact consumption permits a later distinct attempt;
9. selector revision participates in identity;
10. a new `SessionGrants` proves process-local restart emptiness;
11. successful deletion forgets only the deleted session;
12. detach failure and store-delete failure retain repair state;
13. repeated forget is idempotent;
14. saturation never authorizes and never calls Broker `open_run`.

## 8. R3: split immediate fence from blocking cleanup

The current Stop path conflates two operations:

1. an immediate in-memory authority fence that must be synchronous;
2. adapter/browser/resource cleanup that may block or time out.

Introduce or evolve one App-owned lifecycle coordinator with explicit phases. A suitable shape
is:

```text
fence_attempt_and_dispatch
revoke_ipc_authority
publish_desired_absent
acknowledge_local_stop
reconcile_acp_absent
cleanup_matching_owned_resource
settle_or_record_cleanup_pending
```

The first three phases must not perform network or blocking adapter work. Blocking adapter and
loopback HTTP cleanup runs through `spawn_blocking` or a dedicated long-lived worker and remains
bounded. Do not merely wrap the entire old Stop function in `spawn_blocking`, because ACP
reconciliation is async and UI acknowledgement must remain observable.

Required barrier tests:

- a blocked managed-worker cleanup cannot prevent the Host from rejecting a new action;
- ordinary chat Stop publishes Ready/Stopped-local before the cleanup barrier is released;
- handshake Stop follows the same fence and leaves an addressable retry ledger on failure;
- duplicate Stop does not issue extra desired generations or duplicate resource close;
- Stop A cleanup cannot release B's target or credential;
- cleanup timeout remains `stop_requested`/`cleanup_pending`, not falsely `stopped`;
- retry settles the same generation and does not authorize anything;
- borrowed tab is returned and never closed;
- owned profile is released exactly once after matching-generation cleanup.

Apply the coordinator to panel Stop, Composer/Escape Stop, task/dashboard Stop, Stop All, remote
Stop, feature-off, context/model/compact, live/background/parked soft-respawn, reconnect, delete,
logout/account/provider/data-root recycle, updater restart, and cooperative App exit.

## 9. R4: production credential oracle and phase fault matrix

### R4.1 Credential oracle

Use the production MCP entry renderer and the real loopback credential registry. Extract the
token only inside the test process and never print it. For each phase, call `/cu/tool` and record
only `authorized`/`unauthorized`, token count, and generation.

Required scenarios:

- same-run base catalog rebuild retains exactly one usable credential;
- initial attach success commits one usable authority;
- ACP applies then loses the reply; retry uses the same authority;
- ACP rejects a base-catalog update; current committed authority still works;
- authorization attach rejection revokes its uncommitted/current attempt authority;
- Stop during build, before send, after apply, and before response invalidates stopped-run calls;
- late A failure/cleanup cannot revoke B;
- new run has a distinct authority and old run is unauthorized;
- repeated reconcile does not grow credential storage;
- DTO/log/manifest contains no token, Authorization header, env, raw catalog, or private path.

### R4.2 Deterministic production fault seam

Place a test-only dependency seam at construction/transport boundaries. Do not add a shipping
environment variable or hidden production backdoor. Pause with barriers/channels, not sleep.

At minimum cover before/after:

- target provision;
- Broker authorize;
- desired-present publication;
- credential creation/reuse;
- ACP send;
- ACP apply with lost response;
- ACP explicit rejection;
- attach before authorization completion;
- local fence;
- desired-absent publication;
- ACP detach send/apply;
- matching resource release;
- journal/bookkeeping deletion;
- App-exit deadline.

Cross these phases with:

- A late callback after B;
- base MCP revision change;
- normal and handshake Stop;
- repeated Stop/retry/delete;
- reconnect while cleanup pending;
- live/background/parked soft-respawn;
- feature-off across two sessions;
- context/model/compact;
- shared ACP tenant A/B;
- App exit during every relevant phase.

Every scenario independently asserts:

- latest Host attempt and generation;
- Broker stop/target/resource state;
- all known credentials by actual IPC call;
- ACP current catalog and base MCP preservation;
- desired/applied/pending/error ledger;
- zero wrong-target or post-stop dispatch;
- owned resource close count and borrowed resource non-close;
- no leaked process, port, profile, lease, or secret.

## 10. R5: complete gates

After focused red-to-green work, run and record exact discovered/pass/fail counts:

```text
cargo fmt --all -- --check
cargo test -p grok-computer-use-core --all-features --locked --offline
cargo clippy -p grok-computer-use-core --all-targets --all-features --locked --offline -- -D warnings
cargo test -p grok-app session_manager::computer_use --lib --locked --offline
cargo test -p grok-app session_manager::control --lib --locked --offline
cargo test -p grok-app commands::computer_use --lib --locked --offline
cargo test -p grok-app commands::session_p1 --lib --locked --offline
cargo test -p grok-app --lib --locked --offline --no-run
pnpm typecheck
pnpm lint
pnpm test -- src/components/computer-use src/lib/api/computerUse.pairing.test.ts src/lib/slashCatalog.test.ts
pnpm test
pnpm build:ui
python scripts/check-code-quality-gates.py --mode final
git diff --check
```

On Windows, embed `windows-test-manifest.xml` with `mt.exe` after `--no-run` and run the exact
harness directly. Do not add another `/MANIFESTINPUT` in `build.rs`. A zero-test filter is not
a pass.

The slash harness must build and load the candidate frontend, perform a bounded readiness
handshake, open `computer-use` and `computer-use-browser`, and prove that opening a surface does
not authorize it. Missing hook, undefined callback, filtered error, error page, or timeout is a
failure.

## 11. R6-R7: fresh candidate, App-shell, and active soak

After the last candidate-affecting change:

1. build the branch App and source-built ACP stub;
2. compute the full code/test/runner/config/runtime fingerprint;
3. freeze it in the manifest;
4. run two complete fresh App-shell smokes;
5. execute Desktop >=20, Managed >=20, and WebView >=20 full lifecycle iterations;
6. execute the R4 fault/race matrix >=150 seeded rounds;
7. run both slash rows in every smoke batch and periodically during soak;
8. accumulate at least two hours of actual lifecycle/fault execution on the frozen candidate;
9. verify current ACP catalog and actual credential on every iteration;
10. verify all owned PIDs/ports/profiles/leases are gone at the end.

The two-hour lower bound begins only after freeze. It cannot be satisfied in a forty-minute
Goal run. Wall time must be derived from scenario start/end rows; sleeping does not count.

Required success rate is >=90% for each non-safety scenario. These counters must be exactly
zero:

- unauthorized read/write;
- wrong-target action;
- post-stop dispatch;
- old cleanup damaging a new attempt;
- stale Computer Use ACP entry;
- missing base MCP entry;
- secret/token/Cookie/private-path leakage;
- owned resource orphan;
- borrowed tab closed;
- unaccounted child process or listener.

Any candidate-affecting change invalidates the freeze and all accumulated soak. Fix, rebuild,
refingerprint, and restart R6-R7.

## 12. R8: Existing Tabs only after Track A

If and only if Track A passes on one fingerprint, continue with the secure Existing Tabs
vertical slice. Otherwise spend the remaining time repairing Track A.

Before code, update the threat model for malicious local process/page/extension, forged
Origin/Host/CORS, replay, two App instances, browser restart, navigation, reconnect, concurrent
share/stop, stale document, and secret leakage.

The minimal MV3 slice requires:

- explicit user Pair, Share current tab, Stop sharing, status/error/Retry;
- stable extension identity appropriate to source-installed Chrome and Edge;
- possession proof bound to protocol, App instance, challenge, extension identity, connection
  nonce, and expiry;
- authenticated typed envelopes with request id, deadline, cancellation, heartbeat, bounded
  reconnect, and connection/document/target generation;
- isolated-world typed observe/click/set-value/type/key/scroll/wait/navigation only;
- no arbitrary eval, Cookie/storage export, Desktop fallback, or secret in URL/history/page
  world/localStorage/sync storage/log/screenshot;
- only explicitly shared tabs in the Host picker;
- navigation/reload/close/disconnect invalidates old observations before action;
- Stop uses the Track A coordinator and returns the borrowed tab without closing it;
- Host registers the ExistingTab executor only while an authenticated connection exists;
- source Chrome, source Edge, installed Chrome, and installed Edge remain separate evidence rows.

Do not count an extension manifest, successful load, pairing skeleton, or string-presence test as
a working transport.

## 13. Platform and installation matrix

The final report always contains explicit rows for:

- Windows source App;
- Windows portable/NSIS install, repair, upgrade, rollback, uninstall;
- macOS arm64 source and signed/notarized install;
- macOS x64 source and signed/notarized install;
- Linux X11 source/AppImage;
- GNOME native Wayland source/AppImage;
- Chrome Existing Tabs;
- Edge Existing Tabs;
- ACP stub;
- real model with human authorization.

On this Windows machine, unavailable rows remain `not_run` or `blocked_external`. Cross-compile,
mock adapter, XWayland, browser automation, or copied Windows logs do not satisfy them.

## 14. Checkpoints and persistence

Write a checkpoint after every atomic batch and at least every 60-90 minutes. Then continue
automatically without asking whether to proceed.

Each checkpoint records:

- run id, fingerprint, freeze state, and invalidation history;
- files changed in the batch;
- exact commands and discovered/pass/fail/ignored counts;
- first failure and the evidence-based hypothesis for the next attempt;
- redacted credential state and ACP catalog counts;
- Stop/delete/reconnect/respawn/exit postconditions;
- surface/resource ownership and cleanup;
- active soak time, rounds, seed, and safety counters;
- child PIDs, ports, profiles, lease, and cleanup result;
- external blockers and ready local work;
- next action;
- literal text: `未 commit / 未 push / 未提 PR`.

Do not retry the same failure more than three times with the same hypothesis. Preserve first
failure, mark the atomic row failed, and continue with independent ready work. Stop only when the
user explicitly stops the run, a new permission is genuinely required, or every remaining item
is externally blocked.

## 15. Final report contract

Generate the report from the validated manifest. Include:

- branch, HEAD, complete status/diff inventory, run id, fingerprint, freeze history;
- every batch and scenario with evidence level and exact counts;
- all first failures and recoveries;
- actual credential-oracle results without credential values;
- ACP attach/detach/base-preservation results;
- Stop/delete/reconnect/soft-respawn/feature-off/context/update/exit results;
- current-frontend slash and App-shell results;
- Desktop/Managed/WebView/Existing Tabs results;
- active duration, rounds, seeds, rates, and zero-tolerance counters;
- Windows source/installed and all external platform rows;
- residual P0/P1/P2 and technical debt;
- remaining ready work and precise next action;
- `未 commit / 未 push / 未提 PR`.

Do not write `complete`, `releasable`, `cross-platform passed`, `installed passed`, or
`real-model passed` unless the corresponding current-candidate evidence actually exists.


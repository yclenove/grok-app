# Computer Use R2B two-day execution plan

Date: 2026-09-13

Workspace: `H:\aicoding\grok-app-computer-use`

Branch: `feat/computer-use-implementation`

Starting HEAD: `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`

Authority: `docs/plans/2026-09-13-computer-use-grok-r2b-audit.md`

Starting status: **partial - not releasable**

## 1. Objective and order

This is a roughly 48-hour implementation and evidence run. Its first objective
is not more surface area. It is to make Computer Use authorization, IPC
credentials, the live ACP MCP catalog, surface ownership, and teardown one
coherent transaction.

The run has two tracks:

```text
Track A (blocking)
  credential transaction -> lifecycle convergence -> fault matrix
  -> real frontend slash gate -> frozen candidate -> >=2h active soak

Track B (conditional)
  secure Existing Tabs possession proof -> typed MV3 transport foundation
```

Track B may start only after all Track A gates pass on one frozen fingerprint.
If Track A consumes the full window, that is the correct outcome. Do not trade
a lifecycle P0 for a larger feature count.

The time estimates are scheduling aids, not completion evidence. A batch is
complete only when its observable exit conditions pass.

## 2. Non-negotiable boundaries

### 2.1 Git and workspace

- Preserve every current tracked, untracked, and ignored file.
- Do not reset, restore, clean, stash, force-checkout, or recreate the
  worktree.
- Do not commit, push, open a PR, merge, tag, publish, or release.
- Do not pull/rebase/merge upstream during this run.
- Before editing a file, inspect its current content and diff. If another
  writer changes the same file, stop editing that file and move to an
  independent task until ownership is clear.
- Evidence belongs only under an ignored run directory. Never add generated
  profiles, browser data, tokens, screenshots, binaries, or logs to Git.

### 2.2 User environment and processes

- Use isolated `GROK_HOME`, browser profiles, ports, leases, and temporary
  directories for every harness run.
- Never read, copy, rewrite, or delete the user's real Grok, Chrome, Edge, or
  system credential stores.
- Do not use existing browser profiles to make Existing Tabs appear complete.
- Do not kill unrelated App, Node, browser, shell, Codex, or build processes.
- Record only PIDs started by the current run and clean only those PIDs after
  identity verification.
- Heavy Cargo, App-shell, browser, and installer jobs run serially.

### 2.3 Product safety

- Computer Use remains default off and local-session only.
- Normal/YOLO permission does not authorize Computer Use.
- The model cannot authorize itself, expand target scope, retry a denied user
  choice, or recover a stopped grant.
- Stop/revoke must deny dispatch synchronously before waiting for remote
  cleanup.
- Stale target, selector, document, surface, connection, attempt, token, or
  catalog generation fails before side effects.
- Never fall back from Existing Tabs, Managed Browser, or App WebView to
  Desktop.
- Never expose arbitrary shell, arbitrary remote JavaScript evaluation,
  cookie/storage export, unrestricted filesystem download, or raw secrets.
- Borrowed tabs are returned, never closed by cleanup.

### 2.4 Evidence honesty

- Fake/mock/unit evidence is E1/E2, not installed or real-model evidence.
- A branch-built App-shell with an ACP stub is E3, not E4 real-model evidence.
- `exitCode=0` is not a pass unless all scenario postconditions pass.
- `missing-hook`, unavailable, empty target list, skipped platform, or filtered
  error is not a pass.
- Old fingerprints, old logs, and old soak time cannot be copied into this
  run.
- Any production, test, runner, extension, configuration, lockfile, or runtime
  change invalidates the candidate freeze and all candidate soak accumulated
  before it.
- Real-model, installed-package, macOS, Linux X11, and GNOME Wayland rows stay
  `not_run` or `blocked_external` unless actually executed.

## 3. Evidence protocol before editing

Create one ignored run root such as:

```text
tools/computer-use-probe/.run/r2b/<UTC-run-id>/
  owner.json
  baseline/
  checkpoints/
  logs/
  failures/
  metrics/
  reports/
  manifest.jsonl
  state.json
```

`owner.json` records canonical repo path, branch, HEAD, start time, host, OS,
architecture, writer PID, and exact cleanup allowlist. It contains no secret.

Each manifest row must include at least:

```text
schemaVersion, seq, phase, batch, scenario, kind, status, evidenceLevel,
startedAt, endedAt, activeDurationMs, exitCode, commandRedacted,
codeFingerprint, log, bytes, sha256, postconditions, firstFailure
```

Rules:

1. Append one atomic row per scenario. Do not combine Desktop, Managed,
   WebView, slash, lifecycle, and cleanup into one pass.
2. Keep the first failure artifact even after a later retry succeeds.
3. Generate `state.json` and the final report from manifest rows. Do not
   hand-maintain an optimistic rollup.
4. Validate every referenced log's size and SHA-256 before reporting it.
5. Redact tokens, secrets, commands containing secret environment values,
   image payloads, cookies, and local user data.
6. Count active soak only while the tested candidate and scenario driver are
   running. Build time, idle time, blocked time, and Goal-mode interruption do
   not count.

Before the first code change, record:

- `git status --short --branch`;
- `git rev-parse HEAD`;
- the current source fingerprint using the repository's finalization protocol;
- current tracked/untracked/ignored counts;
- baseline focused test counts;
- processes and ports started by this run.

Do not mutate the old
`tools/computer-use-probe/.run/finalization/20260913T034919Z-seven` evidence.

## 4. Target lifecycle design

### 4.1 One coordinator

Create or evolve one App-owned lifecycle coordinator. Tauri commands translate
inputs and await it; they do not independently compose grant, token, MCP, and
resource cleanup.

It must own, per App session:

```text
attempt_generation
UI correlation: attempt_id + selector_revision
run_id
surface target identity + binding/executor generation
phase
cancelled/revoked state
credential generation and committed/staged credential identity
desired/applied MCP generation
ACP endpoint identity
cleanup_pending + redacted first/last error
owned/borrowed resource disposition
```

Suggested externally meaningful phases:

```text
Idle
Authorizing(generation, phase)
Active(generation, run, target, credential, catalog_generation)
Revoking(generation, reason)
CleanupPending(generation, remaining_owners, last_error)
```

No diagnostic or UI DTO may expose a token, MCP environment, command line,
pairing secret, catalog JSON, or unredacted ACP error.

### 4.2 Credential transaction

Catalog building must not mutate credentials. Separate these concepts:

```text
reserve/reuse credential -> render catalog -> send ACP replacement
-> verify latest generation -> commit credential -> retire superseded credential
```

Required semantics:

- A base-only catalog refresh for the same active run reuses its committed
  credential.
- An ambiguous retry reuses the same staged/committed credential; it never
  creates another token merely because the response was lost.
- A staged credential does not delete the committed credential.
- If ACP may have applied a staged credential before a lost response, that
  staged credential remains usable only for the same current authorized run
  until reconciliation resolves it.
- A stale or failed staged generation is revoked without touching a newer
  generation.
- A stop first revokes Broker authority and every credential capable of
  dispatching the stopped run, then reconciles catalog absence.
- After confirmed replacement, superseded tokens are unauthorized.
- Token registry size is bounded per session and stale entries have explicit
  expiry/cleanup semantics.

Prefer a pure catalog entry renderer that receives a credential handle/value
owned by the coordinator. Do not let `build_catalog` call a method named
`issue_*` or otherwise change live authority.

### 4.3 Catalog reconciliation

The desired catalog remains:

```text
current base extension MCP catalog
+ exactly one grok-computer-use entry for the latest eligible lifecycle
```

All base extension updates and Computer Use changes use the same per-session
serializer. Re-read desired/base/endpoint generations before send and after
reply. If any changed, continue until the latest state converges.

An ACP timeout is ambiguous. Retry the same desired document and credential,
or query an instrumented/test catalog oracle. Never assume the update was not
applied.

Desired absent removes all duplicate `grok-computer-use` entries while
preserving every unrelated MCP entry. An old detach cannot remove a newer
eligible entry.

### 4.4 Cleanup contract

All of these triggers call the same coordinator:

- Computer panel Stop;
- normal composer/Escape Stop;
- per-session task/dashboard Stop and Stop All;
- remote session Stop;
- exact pending-authorization cancel;
- feature off;
- context/model/compact invalidation;
- chat/surface switch and relevant component unmount;
- extension MCP preference change;
- live/background/parked soft respawn;
- reconnect after crash or ACP replacement;
- session deletion;
- logout/account/provider/data-root recycle;
- App quit/exit.

Cleanup is ordered:

```text
fence attempt and target
-> synchronously stop Broker dispatch
-> revoke applicable IPC authority
-> set desired MCP absent
-> reconcile ACP catalog absent
-> release only matching owned resources
-> retain or clear cleanup ledger
```

If remote convergence fails, authority is still fail-closed, but the product
reports `cleanup_pending`. The ledger remains until retry or a proven endpoint
replacement makes it obsolete.

## 5. Track A implementation schedule

### A0. Reproduce and add executable red tests (0-3h)

Read in full before edits:

1. `AGENTS.md`
2. `docs/llm-wiki/computer-use.md`
3. `docs/plans/2026-09-13-computer-use-grok-r2b-audit.md`
4. this execution plan
5. `src-tauri/src/session_manager/computer_use.rs`
6. `src-tauri/src/computer_use/inject.rs`
7. `src-tauri/computer-use-core/src/ipc.rs`
8. `src-tauri/computer-use-core/src/session_grants.rs`
9. `src-tauri/src/session_manager/turn.rs`
10. all cleanup/reconnect/delete/exit call sites found with `rg`

Add red tests that use the production MCP entry renderer and a real loopback
credential registry. At minimum prove the current failures:

1. A base MCP revision rebuild does not invalidate the token in the currently
   installed CU entry.
2. An applied-but-timed-out update retries with the identical token.
3. A failed update leaves the previous committed catalog token usable for the
   same authorized run.
4. A late old update/cleanup cannot invalidate the newer run's token.
5. Ordinary `session_stop` produces desired absent and invokes reconciliation,
   including its handshake early-return branch.
6. Delete failure does not delete the journal or erase the retry ledger.
7. Reconnect retries desired-absent pending state.
8. Background soft-respawn detaches while its endpoint is still indexed.
9. App exit invokes a bounded ACP detach barrier before local resources are
   destroyed.
10. One session creating more than 256 cancellations cannot evict another
    session's still-live cancel-before-begin protection.

Exit gate: each red test compiles and fails at the intended postcondition. A
compile error, missing fixture, fixed sleep race, or source-string assertion is
not an acceptable red.

### A1. Transactional credential lease (3-9h)

Refactor token ownership before adding more cleanup hooks.

Implementation tasks:

1. Add a typed credential lease/record keyed by App session, run, and Host
   attempt generation.
2. Split credential reserve/reuse from commit/revoke.
3. Make catalog rendering pure and pass the selected credential into it.
4. Preserve committed authority during same-run base updates.
5. Preserve the same staged credential across ambiguous retries.
6. Commit only after the ACP response and latest-generation check agree.
7. Revoke staged credentials on definitive failure/stale ownership.
8. Revoke every stopped-run credential synchronously on stop.
9. Bound and garbage-collect credential records without affecting a current or
   ambiguous generation.
10. Keep logs and DTOs redacted.

Required tests:

| Scenario | Required observation |
| --- | --- |
| same-run base update | token value unchanged; old child remains authorized |
| base update error | committed token still works |
| update timeout then retry | retry entry uses identical token |
| timeout applied before response loss | staged token works; no third token |
| new run replaces old | new token works only for new run; old unauthorized after commit |
| stop during update | every token for stopped run fails before detach reply |
| stale A after B | A cleanup cannot revoke B token |
| repeated attach/reconcile | bounded token count; exactly one committed authority |

Do not weaken `IpcServer` ownership checks or return success based only on
catalog JSON equality.

### A2. Unified lifecycle and cleanup paths (9-17h)

Route every lifecycle trigger through the coordinator.

#### A2.1 Normal Stop

- Move Computer Use fencing before any early return in
  `SessionManager::stop`.
- Preserve responsive chat Stop: publish turn cancellation promptly, but keep
  catalog cleanup state truthful.
- Ensure composer Stop, Escape, task Stop, Stop All, dashboard, and remote Stop
  all exercise the same Host path.
- Repeated Stop is idempotent and can retry pending detach.

#### A2.2 Delete

- Change `drop_session_agent` to return a typed cleanup result.
- Do not call `store::delete_session` or `forget_deleted_session` after an
  unretained detach failure.
- Preferred first version: fail deletion visibly and leave the session plus
  cleanup ledger available for retry.
- If a durable deletion tombstone is implemented instead, prove it survives
  process restart and contains no secret before permitting metadata deletion.
- Shared ACP co-tenants remain alive and retain their base/CU catalogs.

#### A2.3 Soft-respawn and reconnect

- Detach background and parked sessions before removing their endpoints.
- Reconcile pending desired absent as well as desired present after reconnect.
- Treat a changed ACP process/session identity as unapplied until the new
  endpoint receives the correct full catalog.
- Never kill a shared ACP solely to hide a detach failure.

#### A2.4 App exit

- Identify the earliest Tauri quit/exit path that can await or prevent exit.
- Add one idempotent bounded shutdown coordinator using the managed
  `Arc<SessionManager>`.
- Revoke authority synchronously, then await all known detach operations,
  release surface resources, and finally allow process exit.
- Retain the final synchronous hook as defense in depth.
- Cover tray Quit, menu/Cmd+Q, window close when configured to quit, updater
  restart, and direct App-shell termination where applicable.
- OS hard kill cannot guarantee remote cleanup; document that boundary rather
  than fabricating a pass.

Exit gate: a shared ACP stub reports zero CU entries for every stopped/deleted/
recycled/exited App session and preserves all unrelated entries.

### A3. Retry UX and bounded cancellation state (17-22h)

#### A3.1 Cleanup retry

Add a typed Host command such as `computer_use_retry_cleanup(session_id)` that
does only the coordinator's latest desired-state reconciliation. It must not
re-authorize, select a target, or resurrect a stopped run.

Panel requirements:

- show `authorizing`, `revoking`, `cleanup_pending`, retrying, and latest
  redacted error as distinct states;
- offer Retry only while desired absent is pending;
- keep Retry operable after `stopState=stopped`;
- disable target/refresh/pair/bind controls during conflicting transitions;
- do not label an acknowledgement as fully stopped until local authority is
  revoked and accurately represent remote cleanup pending;
- use existing button/panel styles and all 15 locale catalogs;
- add keyboard, busy, repeated click, late response, unmount, and chat-switch
  behavior tests.

Do not add state blocks to `src/App.tsx` or `src/app/AppWorkbench.tsx`; keep the
domain state in the Computer Use modules/hooks.

#### A3.2 Cancel tombstones

- Replace the global 256-entry FIFO with per-session bounded storage.
- Add explicit creation/expiry timestamps using an injectable clock.
- TTL must exceed the maximum Host command plus provision/MCP timeout budget.
- Enforce a global memory ceiling without allowing one session to evict a
  different session's unexpired record.
- Clear only expired/consumed records and deleted-session records whose command
  lifetime has ended.

Required tests include two sessions, capacity overflow, expiry boundaries,
cancel-before-begin, duplicate cancel, and process-local restart semantics.

### A4. Production-wired Tauri fault matrix (22-30h)

Introduce deterministic test seams at construction/transport boundaries. Do
not add shipping environment backdoors. The test driver must pause and fail
the real authorization command at exact phases without sleeps.

Run every row independently and retain its first failure:

| Fault or race | Required final postcondition |
| --- | --- |
| before provision | no target, token, MCP, or resource |
| after provision/bind | matching resource released |
| before Broker authorize | no dispatch authority |
| after Broker authorize | Broker revoked on failure |
| before credential reserve | no credential record |
| after credential reserve | staged record revoked or retained only for ambiguous same generation |
| before ACP send | desired state wins; no stale catalog send |
| ACP applies, response lost | retry uses identical token and document |
| ACP rejects update | previous committed same-run credential remains valid |
| after attach, before complete | cancel converges to absent |
| complete callback A after B | B remains active and callable |
| base MCP change during attach | latest base + one CU entry |
| Stop during each phase | dispatch denied before Stop returns/acknowledges |
| normal session Stop handshake branch | zero CU catalog entries after cleanup |
| detach error | cleanup pending retained and Retry converges |
| reconnect while cleanup pending | new endpoint receives base-only catalog |
| background soft-respawn | detach precedes endpoint removal |
| delete detach failure | session and retry ledger remain |
| repeated delete/stop/retry | one final state, no double release |
| feature off across two sessions | both fenced; each catalog absent |
| context/model/compact switch | reauthorization required |
| shared ACP tenants A/B | stopping A never changes B's entry/base catalog |
| App exit during each phase | bounded barrier; no owned resource/token/CU entry |

For every row assert all owners, not just the returned error:

```text
Host attempt phase and generation
Broker grant/stop state
old, staged, and committed IPC token behavior
ACP stub's actual current catalog
base MCP preservation
surface binding/executor generation
owned vs borrowed resource disposition
cleanup retry ledger
post-stop dispatch count
```

### A5. Quality gates and real-frontend slash harness (30-34h)

Run focused tests after each batch. When A1-A4 are green, run at least:

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
git diff --check
```

Adapt focused filters to the actual test names, but record the exact command
and discovered/passed count. Do not convert missing tests into a green zero-test
run.

Slash repair requirements:

1. Build the exact frontend for the candidate.
2. Serve/load it through the same App-shell route used by the test executable.
3. Add a bounded readiness handshake for the product hook.
4. Record `slash_desktop` and `slash_managed_browser` separately.
5. Require the panel to open with the requested surface selected and no
   unintended authorization.
6. Treat error pages, missing hooks, undefined callbacks, timeouts, and
   filtered errors as failures.
7. Remove the `missing-hook`/`slash_` blocking exemptions after the harness is
   valid.

App warnings touched by this tranche should be fixed. Count and report all
remaining unrelated warnings. Do not add broad `allow`, skip, or weakened
assertions to make gates green.

### A6. Freeze and active lifecycle soak (34-42h minimum)

After the final code/test/runner/config/runtime change:

1. Build a fresh branch App and runtime from the current tree.
2. Compute a new fingerprint and freeze it.
3. Run two complete smoke iterations.
4. Run at least 20 Desktop, 20 Managed Browser, and 20 App WebView lifecycle
   iterations.
5. Run at least 120 phase-fault/race iterations with a recorded deterministic
   seed.
6. Accumulate at least two hours of **active** lifecycle/fault wall time on
   that exact fingerprint.
7. Run slash Desktop and Managed rows in every smoke batch and periodically
   during soak.

Each active iteration queries the ACP stub's actual final catalog and calls
credentials through IPC. Required invariants:

```text
Active latest run -> exactly one matching CU entry and one committed authority
Stopped/revoked/failed/deleted -> zero CU entries and every old token unauthorized
Pending ambiguous same generation -> no token churn; bounded records
All unrelated base MCP entries preserved
Zero wrong-target and post-stop dispatch
Zero stale cleanup damage to a newer attempt
Zero owned profile/worker/WebView/lease/binding/process orphan
Borrowed resource never closed
No secret/token/cookie/path leakage in evidence
```

Every required scenario must achieve at least 90% success, while all safety
invariants above require exactly zero violations. A flaky retry can count
toward availability failure; a safety violation is immediately blocking.

Any candidate-affecting edit invalidates the freeze and the two-hour total.
Fix, rebuild, refingerprint, and restart A6. If A6 is not complete by hour 48,
keep Track B unstarted and report the exact remaining active duration/rounds.

## 6. Track A exit gate

Track A passes only when all are true on one fingerprint:

- every A0 red has an implemented green counterpart;
- production entry tests prove token stability and transactional retirement;
- all Stop/delete/reconnect/respawn/exit paths converge or retain retryable
  pending state;
- the full phase matrix is atomic and independently reported;
- real built frontend slash rows are blocking and green;
- all quality gates pass without weakened checks;
- at least two hours active soak and required round counts are complete;
- zero safety invariant violation occurred;
- logs, hashes, manifest, state, fingerprint, and process cleanup validate.

Allowed Track A conclusion:

```text
Track A lifecycle gate passed; overall Computer Use partial - not releasable
```

This does not authorize an overall `complete` or `releasable` claim.

## 7. Track B conditional work: secure Existing Tabs foundation (42-48h)

Only start this section after the Track A exit gate passes. If less than six
hours remain, complete B0 protocol/test design rather than rushing a permissive
transport.

### B0. Replace the pairing security model

Update the Existing Tabs ADR and red tests for:

- malicious local native process;
- malicious webpage and forged Origin/Host/CORS headers;
- another extension;
- copied/replayed challenge;
- App, extension service worker, browser, and tab restart;
- two Chrome/Edge instances racing;
- navigation/document replacement;
- concurrent share/stop and stale connection messages;
- secret leakage through URL, history, logs, screenshots, page world,
  localStorage, sync storage, or evidence.

The protocol must require possession of a one-time secret transferred only by
an explicit user gesture. Bind its MAC to protocol version, App instance,
challenge nonce, browser-specific extension identity, connection nonce, and
expiry. Verify in constant time. Public nonce plus two booleans is not proof.

Origin/Host/CORS are defense in depth only. Remove or disable any production
flow that discards the response or completes pairing without possession proof.

### B1. Minimal typed MV3 transport foundation

If B0 tests and review pass, implement only this vertical slice:

1. Extension UI: Pair, Share current tab, Stop sharing, connection/error
   status, and Retry.
2. Session secret stored where page JavaScript cannot read it; no secret in
   URL/query/log/localStorage/page world.
3. Authenticated message envelope with monotonic connection/document/target
   generation, request ID, deadline, and cancellation.
4. Heartbeat and bounded reconnect with key rotation.
5. Explicit shared-tab offer to the Host picker; no automatic enumeration of
   unrelated tabs.
6. Typed `observe`, `click`, `set_value`, `type`, `key`, `scroll`, `wait`, and
   navigation messages in an isolated content-script world.
7. No arbitrary eval and no Desktop fallback.
8. Navigation/reload/close/disconnect invalidates old observations before any
   action.
9. Stop returns the borrowed tab without closing it and uses the Track A
   coordinator for grant/token/catalog cleanup.
10. ExistingTab executor registers only while an authenticated shared
    connection exists.

Each behavior needs negative tests before a local happy-path claim. Chrome and
Edge remain separate evidence rows. Do not claim either browser based on a
source-string test or extension installation alone.

Track B is not a release gate for this two-day run. Its allowed conclusion is:

```text
Track A+B local source gate passed; overall Computer Use partial - not releasable
```

Use that only if the implemented B slice actually passes its local behavioral
and App-shell gates. Otherwise report B0/B1 as partial.

## 8. Checkpoint cadence

Write a checkpoint after every batch and at least every 90 minutes of active
work. Then continue automatically without asking whether to proceed.

Each checkpoint contains:

- timestamp, run ID, HEAD, current fingerprint/freeze validity;
- batch status and files changed in that batch;
- exact commands and passed/failed/discovered counts;
- first failure and current hypothesis;
- actual ACP catalog summary, never raw token/catalog;
- credential counts/states in redacted form;
- Broker/surface/resource cleanup postconditions;
- active soak time and round counts by scenario;
- process/port cleanup;
- external blockers and independent ready work;
- next action;
- literal line `未 commit / 未 push / 未提 PR`.

On a repeated failure, make at most three focused attempts based on different
evidence or hypotheses. Then retain the first failure, mark the atomic row
failed, and move to independent ready work. Do not loop a command indefinitely
or manufacture time by sleeping.

Only stop for input when the next action truly requires real account/model,
external device/OS access, destructive user-state change, credential entry, or
another authority not granted by this plan.

## 9. Final report contract

The final report is generated from validated manifest rows and includes:

- run identity, branch, HEAD, final fingerprint and freeze history;
- complete diff/status summary without dumping unrelated user content;
- every batch/scenario status and evidence level;
- exact test commands and counts;
- first failure for each failed row;
- token transaction assertions in redacted form;
- catalog attach/detach traces and base-server preservation;
- Stop/delete/reconnect/respawn/exit results;
- slash Desktop/Managed results from the real frontend;
- Desktop/Managed/WebView/Existing Tabs separate results;
- active soak wall time, rounds, success/failure, seed, and safety counters;
- Windows source vs installed, macOS arm64/x64, Linux X11, GNOME Wayland, and
  stub vs real-model rows kept separate;
- manifest/log hash validation and remaining owned processes/resources;
- remaining P0/P1/debt and next exact action;
- literal line `未 commit / 未 push / 未提 PR`.

Allowed top-level conclusions are only:

```text
Track A lifecycle gate passed; overall Computer Use partial - not releasable
Track A+B local source gate passed; overall Computer Use partial - not releasable
partial - not releasable
```

Do not write `Computer Use complete`, `releasable`, `cross-platform complete`,
`installed complete`, or `real-model complete` during this run.

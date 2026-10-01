# Computer Use R2 lifecycle and Existing Tabs execution plan

Date: 2026-09-13

Workspace: `H:\aicoding\grok-app-computer-use`

Branch: `feat/computer-use-implementation`

Baseline HEAD: `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`

Audit input fingerprint: `26406035517b6856e1deb6a9e7b3e2103b48391bcfa4e72215d02d6766b34fd5`

Companion audit:
`docs/plans/2026-09-13-computer-use-codex-r2-atomic-audit.md`

Verdict at start: **partial - not releasable**

## 1. Purpose and ordering

This plan is the next bounded 48-hour campaign. It has two dependency-ordered
tracks:

```text
Track A: atomic authorization + symmetric MCP lifecycle
    must pass before
Track B: authenticated Existing Tabs pairing + typed MV3 transport
    must pass before
candidate freeze + active soak
```

Track B is forbidden while any Track A P0 invariant is red. If Track A takes
the full window, spend the remaining time on its fault campaign and report
Track B `not_started`; do not rush across the security boundary.

This plan does not authorize a release and does not complete cross-platform
v1. macOS, Linux X11, GNOME Wayland, installed-package E5, and real-model E4
remain independent future gates unless the exact required environment and user
approval already exist.

The forecasted hours are scheduling guidance, not evidence. Finishing edits in
40 minutes does not finish this campaign. The candidate must accumulate the
specified active fault/transport soak after its final code fingerprint is
frozen. Waiting, sleeping, compilation, or repeating static tests is not active
soak.

## 2. Non-negotiable boundaries

### 2.1 Git and workspace

- Preserve every existing tracked, untracked, ignored, and evidence file.
- Do not reset, clean, restore, checkout files, stash, or recreate the worktree.
- Do not `git add`, commit, push, open a PR, merge, tag, publish, or release.
- Do not modify `H:\aicoding\grok-app` or another worktree.
- Before each edit, inspect the latest file and its diff. Do not overwrite a
  concurrent writer.
- Add new evidence only below a new ignored run root owned by this campaign.

### 2.2 Processes and user state

- Do not stop Grok, Codex, Grok App, Chrome, Edge, or system processes not
  created by this campaign and listed in its owner file.
- Use an isolated `GROK_APP_HOME`, isolated ACP stub home, isolated browser
  user-data directory, and run-owned staging.
- Do not change proxy, VPN, routes, account state, the shipping App profile, or
  the user's normal browsers.
- Do not read, print, export, persist, or reuse tokens, cookies, browser
  storage, passwords, or shared `~/.grok` data.
- Do not run a real Grok model or send fixture data to an external service
  without a new explicit user approval.

### 2.3 Product safety

- Computer Use remains default-off and independent of YOLO/accept-edits.
- Only the user/Host may authorize, pair, resume, reconnect, or grant a tab.
- A missing/dead/stale/mismatched surface fails closed. Never fall back to the
  Desktop executor.
- Timeout means `unknown`; never replay a possibly applied side effect.
- A borrowed tab is returned, not closed. An App-owned profile may be closed by
  its matching owner only.
- No arbitrary shell, remote JavaScript eval, cookie/storage export, broad
  filesystem path, or model-selected download path.
- Do not add state or large blocks to `App.tsx` or `AppWorkbench.tsx`.
- UI copy must use the 15-locale catalog with English as key authority.
- Do not use native select, browser context menus, or window
  `confirm/prompt/alert`.

### 2.4 No false evidence

- Unit, fake, mock, stub, jsdom, source grep, or API status cannot be promoted
  to product E3.
- A directly spawned MCP client proves Host/MCP routing, not real-model E4.
- A source build proves neither installed E5 nor another operating system.
- A report containing a required failed scenario cannot have `ok=true`.
- Missing, `not_run`, `blocked_external`, `invalidated`, and failed required
  rows prevent their parent from passing.
- Old fingerprints and old soak are invalid after production, test, runner,
  extension, config, lock, or runtime changes.

## 3. Evidence protocol before code

Create:

```text
tools/computer-use-probe/.run/r2-lifecycle/<run-id>/
  baseline/
  checkpoints/
  failures/
  logs/
  metrics/
  reports/
  homes/
  owner.json
  state.json
  manifest.jsonl
```

Prove both the parent and nested owner file are ignored with
`git check-ignore -q` before writing logs.

`owner.json` records run ID, canonical repo, branch, HEAD, writer PID, start
time, OS/arch, isolated roots, and exact cleanup allowlist. Only owner-listed
processes and paths may be cleaned.

Every manifest attempt is append-only and includes:

```json
{
  "seq": 1,
  "track": "A",
  "batch": "A1.1",
  "scenario": "stop-during-acp-update",
  "attempt": 1,
  "status": "failed",
  "evidenceLevel": "E2",
  "startedAt": "",
  "endedAt": "",
  "activeWallMs": 0,
  "exitCode": 1,
  "argvRedacted": [],
  "log": "logs/A1.1-0001.log",
  "bytes": 0,
  "sha256": "",
  "codeFingerprint": "",
  "postconditions": []
}
```

Atomic statuses are `not_started`, `in_progress`, `passed`, `failed`,
`blocked_external`, `invalidated`, and `not_run`. `partial` is rollup-only.
Generate `state.json` from required manifest scenarios. Parent status is the
worst required child status; it is never handwritten.

Fix the current harness contract before using it as a completion gate:

1. Managed, WebView, Desktop, slash, lifecycle, and cleanup get separate
   manifest records.
2. A required scenario in `errors` makes its scenario and parent fail.
3. Slash tests either run against a real built frontend/dev server or remain a
   separate honest `not_run`; never filter them into a passing aggregate.
4. Report fields state whether the actor was an ACP stub, a directly spawned
   MCP client, or a real model.
5. Log bytes/hash and fingerprint are verified after the writer closes them.
6. Secret scans run on logs, reports, command lines, and support bundles.

## 4. Target lifecycle model

Implement an equivalent of this Host-owned state machine; names may follow the
repository, but the ownership and transitions may not be weakened:

```text
Idle
  -> Preparing(attempt_generation)
  -> SurfaceProvisioned
  -> BrokerAuthorized
  -> McpReconciling(desired=present)
  -> Active
  -> Revoking(desired=absent, authority already fenced)
  -> Detached
  -> Idle

Any state -> CleanupPending(error, retryable)
Any pre-Active state -> Cancelled -> Revoking
```

One authoritative record per App session includes at least:

```text
host_attempt_generation
ui_attempt_id
selector_revision
app_session_id
agent_session_id (when live)
run_id
surface
backend_target_id
target_generation
surface_ownership
phase
cancel token
mcp_desired
mcp_applied_generation
cleanup_pending
```

The UI's selector revision can restart at one after remount. It is diagnostic,
not the Host monotonic clock. The Host assigns `host_attempt_generation` and
checks it at every await boundary and callback.

## 5. Target MCP catalog model

Use desired-state reconciliation, not unrelated imperative add/remove calls:

```text
desired_catalog(session) =
  current base extension MCP catalog
  + exactly one grok-computer-use entry only when latest lifecycle is eligible
```

Requirements:

- Serialize all ACP MCP updates per App/agent session, including generic
  extension preference updates and Computer Use updates.
- Recompute desired state after acquiring the serializer; do not apply a stale
  catalog captured before waiting.
- After ACP update returns, compare the lifecycle/catalog generation again. If
  it changed, reconcile until applied equals latest desired state.
- Stop/revoke first cancel the Host attempt, revoke Broker dispatch and IPC
  token, then request desired `absent`. No await may precede the authority
  fence.
- Attach timeout is ambiguous. Treat applied state as unknown and reconcile to
  the latest desired state; do not assume the request failed.
- Detach preserves all non-Computer-Use MCP servers and removes all duplicate
  `grok-computer-use` entries.
- If ACP is dead, disconnected, or busy, keep a visible pending result. The
  next connect must compute desired state from Host lifecycle, not inherit a
  stale process-local flag.
- A stale attempt may never detach a newer active attempt. A newer desired
  generation always wins.
- Revoked IPC tokens return unauthorized even while catalog reconciliation is
  pending.
- Do not kill a shared ACP process merely to make a test pass. Follow existing
  tenant ownership rules; bounded respawn is a documented fallback only when
  the targeted catalog protocol genuinely cannot converge.

## 6. Track A: atomic lifecycle

### A0. Baseline and executable red tests (0-2h)

Read in full:

1. `AGENTS.md`
2. `docs/llm-wiki/computer-use.md`
3. `docs/plans/2026-09-13-computer-use-codex-r2-atomic-audit.md`
4. this execution plan
5. `src-tauri/src/commands/computer_use.rs`
6. `src-tauri/src/computer_use/sessions.rs`
7. `src-tauri/src/session_manager/computer_use.rs`
8. `src-tauri/computer-use-core/src/session_grants.rs`
9. all revoke/context/session/exit call sites found with `rg`

Record the new fingerprint and current gate results. Then add behavioral red
tests for:

- invalid/empty/oversized attempt ID and selector revision zero;
- two Host attempts where A returns after B became latest;
- stop while ACP update is blocked, followed by late attach success;
- attach timeout where the ACP stub actually applied the catalog;
- detach failure followed by idempotent retry;
- stale detach after a newer attach;
- base extension MCP update racing CU attach and detach;
- stop at every authorization phase;
- context invalidation and feature-off while attach is in flight;
- repeated stop/revoke/exit;
- old WebView cleanup after a newer binding;
- old Managed cleanup after a newer App-owned profile.

The ACP stub must record redacted catalog updates and expose a deterministic
barrier/fault script. Do not add public production environment variables or a
shipping backdoor. Inject a test transport/trait at construction boundaries.

Exit gate: each red fails for the intended observable postcondition, not due to
a compile error, missing fixture, sleep race, or string assertion.

### A1. Host AttemptRegistry (2-6h)

Implement a domain module owned by the Host/SessionManager rather than React.
It must:

1. Validate the UI correlation fields.
2. Assign a monotonic Host generation.
3. Store phase and cancellation by App session.
4. Fence every synchronous and async transition.
5. Make cancel/revoke idempotent.
6. Prevent a stale success/failure/cleanup callback from publishing or deleting
   a newer attempt.
7. Expose a redacted diagnostic snapshot for tests/UI, with no token or secret.

Decide and test one explicit policy for a second authorize while one is
pending: either reject `busy` until explicit cancel completes, or atomically
supersede and compensate the old attempt. Do not silently mix the two.

Move authorization orchestration out of the 700-line Tauri command module into
a focused lifecycle service. The Tauri command should validate/translate and
await one domain operation.

Required green tests:

- A late callback cannot set enabled, attach MCP, publish target, or clear B.
- Component unmount/session switch cancellation reaches Host.
- `selector_revision=1` after UI remount is accepted as a new correlation but
  receives a newer Host generation.
- Concurrent calls for different sessions do not share generation or cancel
  one another.
- Same-session duplicate stop and cancel have one final outcome.

### A2. MCP catalog reconciler (6-12h)

Centralize the catalog update currently split between
`SessionManager::attach_computer_use` and generic extension changes.

Suggested API shape, adapted to local patterns:

```text
set_computer_use_desired(app_session, attempt_generation, Some(run_id))
set_computer_use_desired(app_session, attempt_generation, None)
reconcile_session_mcp(app_session, reason)
reconcile_all_pending(reason)
```

The reconciler owns a per-session async mutex and desired/applied generations.
Catalog construction occurs from current base preferences plus current
lifecycle state while holding logical ownership, but expensive blocking work
does not hold the SessionManager's parking-lot mutex.

Required green tests with an instrumented ACP stub:

- active catalog contains exactly one CU entry and every base server;
- stopped catalog contains zero CU entries and every base server;
- attach -> stop during update -> late response converges to absent;
- A detach cannot remove B's entry;
- attach timeout with server-side apply converges correctly;
- duplicate attach/detach is idempotent;
- extension setting update and CU update preserve both desired results;
- disconnected/busy session records pending and reconnect builds the correct
  catalog;
- wrong agent-session identity cannot receive the update;
- token/command/env never appears in ordinary logs or reports.

### A3. Unified authorization and compensation (12-16h)

The one Host transaction must own:

```text
begin attempt
-> provision/bind/borrow target
-> claim surface resource
-> Broker authorize
-> issue IPC token
-> set MCP desired present + reconcile
-> final latest-attempt check
-> publish Active
```

Every failure/cancel executes reverse-order compensation, scoped by ownership:

```text
mark cancelled and desired absent
-> revoke IPC token
-> revoke Broker dispatch/generation
-> reconcile MCP absent
-> release only matching target/profile/tab/WebView
-> publish Idle or CleanupPending
```

Do not clear a target before retaining enough immutable identity to release the
matching surface. Do not close a borrowed tab. Do not report `stopped` while a
required cleanup is pending.

Route all of these through the same lifecycle coordinator:

- UI Stop and explicit WebView unbind;
- feature off;
- model/mode/context change and compaction;
- local session switch, soft respawn, reconnect, delete, logout;
- extension MCP preference change;
- App exit and fatal Host teardown.

For App exit, bound the async detach. Always revoke local authority first. If
the process must exit before ACP confirms, terminate only a process actually
owned by that session and record the outcome; never block indefinitely.

### A4. UI contract and cancellation (16-18h)

- Add an explicit Host cancellation call for a pending authorization.
- Invoke it on Stop, session change, feature-off, and component cleanup where
  appropriate.
- Keep surface, target, refresh, pair, and WebView selectors disabled while
  authorizing/revoking. Stop/Cancel remains available.
- Render `authorizing`, `revoking`, `cleanup_pending`, and retryable failure
  without claiming the task stopped.
- Do not expose internal token, path, nonce secret, or ACP catalog.
- Preserve keyboard focus and current panel layout.
- Add all new copy to 15 locales and extend parity tests.

Behavior tests must use controllable deferred promises to prove that late A
responses cannot change B's run/target/status after Stop, remount, or session
switch. Source string checks are not sufficient.

### A5. Fault matrix and gates (18-22h)

Run every row below independently and retain the first failure:

| fault point | required final state |
| --- | --- |
| before surface provision | no resource, token, target, or MCP |
| after provision, before claim | provision compensated |
| after claim, before Broker authorize | claim released |
| after Broker authorize | Broker revoked and surface released |
| after token issue | token unauthorized after cancel |
| before ACP update | desired absent and no update leak |
| ACP applies then response times out | reconciler observes latest desired |
| after attach, before complete | catalog detached on cancel |
| detach returns error | cleanup_pending, retry converges |
| stop during action | zero post-stop dispatch |
| old cleanup after new attempt | new target/catalog untouched |
| feature off during attach | all sessions fenced, catalog absent |
| context/model/session change | authorization dropped, reauth required |
| App exit | no owned child, lease, token, binding, or CU catalog remains |

Run focused gates after each logical batch, then all of:

```text
cargo fmt --all -- --check
cargo test -p grok-computer-use-core --all-features --locked --offline
cargo clippy -p grok-computer-use-core --all-targets --all-features --locked --offline -- -D warnings
cargo check -p grok-app --lib --locked --offline
pnpm exec tsc --noEmit
pnpm lint
focused Computer Use Vitest files
git diff --check
```

Do not weaken lints, add broad `allow`, skip tests, or delete assertions to
obtain green output. App warnings touched by this tranche should be eliminated;
unrelated warnings remain counted and listed.

### A6. Current-source E3 and lifecycle soak (22-24h minimum)

After the last code/test/runner/config/runtime edit:

1. Build a fresh App executable from the current tree.
2. Compute and freeze a new candidate fingerprint.
3. Run a 2-round smoke.
4. Run at least 20 Managed and 20 WebView happy-path rounds.
5. Run at least 100 lifecycle fault rounds across the A5 matrix with seeded,
   recorded scheduling variation.
6. Accumulate at least 2 hours of active lifecycle fault wall time on the
   frozen fingerprint.

Every active round queries the ACP stub's final current catalog and asserts:

```text
active -> exactly one matching CU entry
stopped/revoked/failed -> zero CU entries
all unrelated base MCP entries preserved
old IPC token unauthorized
no target/binding/profile/worker/lease orphan
no wrong-target or post-stop action
```

A code, test, runner, extension, config, lock, or runtime change invalidates the
candidate and its soak. Fix, rebuild, refingerprint, and restart the full A6
sequence. If Track A cannot pass, do not enter Track B.

## 7. Track B: secure Existing Tabs

Track B starts only after every required Track A row passes on one fingerprint.

### B0. Threat model and protocol decision (24-27h)

Replace the current accepted pairing ADR with an implementation-ready revision
covering:

- malicious local native process;
- malicious webpage and forged browser headers;
- unrelated/malicious extension;
- replay, concurrent claimant, stale response, and rate exhaustion;
- extension service-worker suspend/restart;
- browser/App restart and key rotation;
- navigation, tab close, profile change, and browser shutdown;
- URL, Referer, history, log, crash, screenshot, and support-bundle leakage.

The minimum acceptable flow is:

1. App creates public nonce/expiry/instance metadata and a separate one-time
   secret/code.
2. The secret is transferred only by an explicit user action into the
   extension popup/side panel, never in URL/query/GET/history/log/screenshot.
3. Extension proves possession with a constant-time verified MAC over nonce,
   App instance, browser-specific extension identity, connection nonce, and
   protocol version.
4. App and extension confirmations are independent.
5. Session key is delivered only after proof, bound to connection generation,
   and rotated/revoked on lifecycle events.

Origin/Host/CORS remain defense in depth, never identity. Remove the HTTP path
that discards `response` and remove secretless `complete_dual` from production.
Specify stable Chrome and Edge identities; do not keep `pw-ext-installed`.

### B1. Pairing implementation and negative tests (27-31h)

- Implement proof verification with constant-time comparison.
- Make extension ID mandatory and browser-specific.
- Reject no-Origin browser endpoints unless the authenticated protocol step
  explicitly permits a native client with an independent proof.
- Add expiry, one-use nonce, replay cache, rate limit, connection generation,
  and revoke/rotate semantics.
- Store only the active session key in `chrome.storage.session` or an equally
  restart-safe, page-inaccessible MV3 store. Clear it on browser restart or
  explicit revoke according to the ADR.
- Never expose the key to page world, `localStorage`, URL, or logs.

Negative tests include wrong/missing ID, wrong MAC, no MAC, old nonce, expired
code, replay, concurrent race, forged Origin, no Origin, forwarded headers,
wrong Host, App restart, browser restart, worker restart, revoke during proof,
and key use after rotation.

### B2. Typed MV3 transport (31-37h)

Implement a real extension surface:

- popup or side panel with Pair, Share current tab, Stop sharing, connection
  state, and retry;
- explicit user Share only; the model never enumerates all tabs;
- authenticated loopback WebSocket or bounded long-poll transport;
- heartbeat, reconnect, deadlines, command queue, cancel, and generation;
- minimal permissions using `activeTab`, `scripting`, `storage`, and optional
  origin grants only where actually required;
- content script in isolated world with a fixed observation schema;
- typed click, set-value, type-text, key, scroll, wait, and navigation only;
- no arbitrary remote script/eval;
- observation ID plus document/connection/target generations;
- navigation/reload/close/focus/disconnect invalidates old observations;
- action result distinguishes applied, verified, rejected, and unknown;
- stop detaches scripts, cancels queued work, deletes the run grant, and
  returns rather than closes the borrowed tab.

Document screenshot limits honestly. If Chrome APIs cannot capture a
non-visible shared tab without a broad `debugger` permission, require the tab
to be active for image capture or return typed image-unavailable. Do not force
focus, silently request broad permission, or fabricate an image.

### B3. Host executor, UI, and packaging (37-41h)

- Register a production ExistingTab executor only after an authenticated
  extension connection exists.
- Make the extension Share event the production caller that offers a candidate
  to Host.
- Picker lists only explicitly shared candidates.
- Authorization uses the Track A transaction and AttemptRegistry.
- Generic `computer_observe/computer_act` and compatibility browser tools share
  the same executor, grant, ledger, generation, and cancellation state.
- Disconnect, navigation, close, stop, revoke, session switch, feature-off,
  and App exit all use the same lifecycle coordinator.
- Bundle the extension assets, notices, browser identity metadata, and install
  instructions as Tauri resources. Installed usage must not reference the repo.
- Add UI strings to all 15 locales and behavior-test empty/busy/error/reconnect
  states.

### B4. Chrome and Edge E2/E3 (41-44h)

Use campaign-owned test profiles only. For each browser available:

- pair with explicit product UI/protocol;
- share exactly one fixture tab among at least three open fixture tabs;
- authorize from App picker;
- perform generic MCP observe -> typed act -> independent DOM oracle -> observe;
- navigate and reject old observation/generation;
- suspend/restart the service worker and reconnect safely;
- disconnect/reconnect transport;
- stop during an action;
- revoke and prove old key unauthorized;
- return the borrowed tab without closing it;
- prove unshared tabs were never observed or acted on.

Run a 2-round smoke and then at least 20 complete Chrome rounds. Run Edge as an
independent row when installed and controllable. If Edge is genuinely absent,
record `blocked_external` with the exact environment and resume command; do not
copy Chrome results.

## 8. Candidate freeze and active soak (44-48h and longer if required)

Freeze only after all required local Track A and available Track B rows pass.
After freeze, run at least four hours of active mixed scenarios and at least
200 complete iterations:

| scenario | minimum active time | minimum rounds |
| --- | ---: | ---: |
| authorize/stop phase faults | 60 min | 50 |
| MCP attach/detach races | 60 min | 50 |
| Managed/WebView regression | 45 min | 40 combined |
| Existing Tabs navigation/reconnect | 60 min | 40 |
| feature/context/session/App teardown | 15 min | 20 |

Success requires at least 90% per scenario and exactly zero:

- unauthorized writes or observations;
- wrong-target actions;
- post-stop dispatches;
- stale cleanup damaging a newer attempt;
- stale CU MCP entry after cleanup;
- missing base MCP entry after reconcile;
- token/secret/cookie leakage;
- unrecovered App-owned worker/profile/binding/lease;
- borrowed tab closure.

Failures are not averaged away across scenarios. Preserve the first failure.
Any fix invalidates the freeze and restarts the entire post-freeze sequence.

## 9. Checkpoint cadence

Write a checkpoint every completed batch and at least every 90 minutes. Then
continue to the next ready item without waiting for the user. Each checkpoint
must include:

- run ID, current fingerprint, branch/HEAD, and dirty-state counts;
- completed batch and highest evidence level;
- exact production/test files changed;
- exact tests and counts, including failures;
- current lifecycle/MCP catalog snapshot with secrets redacted;
- first unresolved failure and current hypothesis;
- process/resource cleanup state;
- external blocker and exact resume procedure;
- next executable item;
- `not committed / not pushed / no PR`.

One fault may receive at most three focused attempts based on different,
recorded hypotheses. After that, keep the failure, mark the dependent batch
failed, and continue only independent work. Do not loop the same command.

## 10. Completion contract

The final report must be generated from the manifest and include:

- baseline/current/freeze fingerprint and drift result;
- Track A and B batch/scenario statuses;
- AttemptRegistry state-machine tests;
- ACP catalog update trace for every attach/detach race, redacted;
- count of active catalogs with exactly one CU entry and stopped catalogs with
  zero CU entries;
- fault matrix results and first failure artifacts;
- Managed/WebView/Chrome/Edge rounds separately;
- active soak time and rounds per scenario;
- process/profile/tab/token/lease/binding cleanup inventory;
- secret scan, dependency audit, warnings, and large-file debt;
- Windows/macOS/Linux/Wayland, installed App, and real-model rows separately;
- manifest integrity check: file existence, bytes, SHA-256, fingerprint;
- explicit Git state: no commit, no push, no PR.

Allowed top-level conclusions:

```text
Track A lifecycle gate passed; overall Computer Use partial - not releasable
Track A+B local source gate passed; overall Computer Use partial - not releasable
partial - not releasable
```

This campaign cannot conclude `Computer Use complete` or `releasable` because
cross-platform v1, installed E5, real-model E4, and release acceptance are not
part of its verified scope.

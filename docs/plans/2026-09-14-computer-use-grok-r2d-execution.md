# Computer Use R2D 48-hour execution plan

> Date: 2026-09-14
> Workspace: `H:\aicoding\grok-app-computer-use`
> Branch: `feat/computer-use-implementation`
> Baseline HEAD: `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`
> Required input audit: `docs/plans/2026-09-14-computer-use-codex-post-r2c-audit.md`
> Publication boundary: no commit, no push, no PR, no merge, no tag, no release

## 1. Mission and truthful starting state

R2D first closes the App lifecycle/security/quality track, then starts a secure Existing Tabs
vertical slice only if the first track is green on one frozen fingerprint.

The starting and overall status is:

```text
partial - not releasable
```

This plan is deliberately longer than a code-generation pass. Completion means behavior,
postconditions, current-fingerprint evidence, and active fault execution. Writing code, seeing a
large unit-test count, or reaching the end of a Goal response is not completion.

## 2. Hard boundaries

The worker must preserve the entire existing dirty tree.

Forbidden operations:

- reset, restore, clean, stash, destructive checkout, rebase, or rebuilding the worktree;
- pull or merge from upstream;
- commit, push, PR, merge, tag, publish, release, deploy, or installer rollout;
- reading or changing real account tokens, Cookies, browser profiles, Credential Store,
  `~/.grok`, proxies, or VPNs;
- operating the user's daily apps, tabs, chats, or files;
- default-enabling Computer Use or granting it through YOLO/accept-edits;
- arbitrary shell, arbitrary page eval, Cookie/storage export, or Desktop fallback;
- blanket Clippy allows, lower warning levels, skipped tests reported as passes, or hand-written
  optimistic evidence summaries.

Use only isolated App homes, ACP stub homes, browser profiles, loopback ports, windows, pages,
fixtures, and files created and owner-marked by the current run. Cleanup may touch only exact
resources whose owner identity is verified.

## 3. Required reading before edits

Read these files in full, in order:

1. `AGENTS.md`
2. `docs/llm-wiki/computer-use.md`
3. `docs/llm-wiki/i18n.md`
4. `docs/llm-wiki/dialogs.md`
5. `docs/plans/2026-09-14-computer-use-codex-post-r2c-audit.md`
6. this execution plan
7. `docs/plans/2026-09-13-computer-use-existing-tabs-pairing-adr.md`
8. `docs/plans/2026-09-11-computer-use-existing-tabs-transport-adr.md`
9. the exact source, test, and runner files named by each R2D batch

Current source and this audit override old progress claims. Old evidence may be catalogued as
historical or stale; it may not be copied into R2D as a pass.

## 4. Evidence protocol

Before the first product edit, create one ignored run root:

```text
tools/computer-use-probe/.run/r2d/<UTC-run-id>/
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

Rules:

- `owner.json` records canonical repo, branch, HEAD, UTC/local time, host, OS, arch, writer PID,
  schema version, and a resolved cleanup allowlist; it contains no credential values.
- `manifest.jsonl` is append-only and has one scenario or command per row.
- Every row records sequence, batch, scenario, exact redacted argv, cwd, start/end, duration,
  exit code, evidence level, status, log path/hash/size, code fingerprint, and postconditions.
- Preserve the first failure. A later pass appends a recovery row; it never overwrites history.
- `state.json`, checkpoint summaries, and the final report are generated from the manifest.
- A production/test/runner/config/runtime change invalidates the candidate freeze and all soak
  accumulated before it. Append an invalidation row and refreeze after rebuilding.
- A missing hook, zero-test filter, empty target list, stale artifact, timeout, crash, skipped
  scenario, or parse failure is not a pass.
- Credentials may be used inside a test process but never printed, hashed into a public artifact,
  placed in argv/URL, or retained in logs. Record only authorized/unauthorized, generation, and
  count.
- Heavy Cargo, App, browser, and installer runs are serial. Do not create resource contention and
  call it a product failure.

Evidence levels remain:

| Level | Meaning |
| --- | --- |
| E0 | inventory/static fact only |
| E1 | unit/mock/jsdom behavior |
| E2 | real module/process in isolation with owned fixtures |
| E3 | branch App plus scripted agent/owned browser/native fixture |
| E4 | real model plus explicit human authorization |
| E5 | installed/update/rollback and repeated task acceptance |

Lower evidence never fills a higher row.

## 5. Schedule and dependency order

| Target time | Batch | Exit condition |
| --- | --- | --- |
| 0-2h | R0 baseline and executable reds | current inventory plus deterministic failing tests |
| 2-4h | R1 runtime test isolation | exact App runtime test and panic restoration green |
| 4-12h | R2 lifecycle authority repair | handshake Stop and process-exit matrices green |
| 12-18h | R3 App Clippy and module ownership | both Clippy gates and file budget green |
| 18-21h | R4 Tiptap security update | production audit and all frontend/editor gates green |
| 21-29h | R5 production lifecycle fault matrix | all Track A phase rows green or explicitly failed |
| 29-34h | R6 rebuild, fingerprint, App-shell | two fresh App-shell rounds on one frozen candidate |
| 34-43h minimum | R7 active soak and mutation | >=2 hours active candidate execution plus repair budget |
| 43-47h | R8 secure Existing Tabs slice, only if Track A passed | pairing/transport slice or more Track A repair |
| 47-48h | R9 generated report and cleanup audit | truthful manifest-derived handoff |

These are engineering budgets, not reasons to sleep. If one batch finishes early, continue with
its negative, race, mutation, and resource-leak cases, then move forward. Never manufacture idle
time. R7's two-hour minimum is actual scenario execution after the final freeze.

## 6. R0: baseline and executable red tests

Record:

- branch, HEAD, complete tracked/untracked inventory, current line endings, and lockfiles;
- current source/test/runner/config/runtime fingerprint;
- owned and unrelated processes, ports, profiles, App homes, leases, and browser workers;
- the R2C root as stale historical evidence with exactly seven rows;
- the known current results: Core 325/325, Driver 12/12, frontend targeted 47/47, runtime test
  failure, App Clippy failure, quality 82/80, and one high production advisory.

Then write deterministic red tests. A red test must compile, execute the production path, and fail
on the missing postcondition. A compile error, grep assertion, sleep race, missing fixture, or
forced timeout is not an executable red.

### R0.1 Runtime isolation reds

1. `GROK_APP_HOME` remains redirected until both Node and Playwright archive resolution finish.
2. `PATH` and `GROK_APP_HOME` restore after the success path.
3. Both restore after a deliberate unwind inside the scope.
4. A poisoned test lock can be reacquired without inheriting either temporary value.
5. Two serialized tests cannot observe each other's App home.

### R0.2 Handshake Stop reds

1. An empty Computer Use cleanup plan still terminates the exact connecting ACP.
2. ACP termination still happens when catalog detach fails.
3. ACP termination still happens when surface cleanup fails and Retry stays available.
4. Duplicate Stop requests produce one bounded termination.
5. A delayed Stop for process A never kills replacement process B.
6. A shared ACP with another live tenant is not killed; only the stopped tenant is detached.
7. Stop acknowledgement is observable before bounded process termination completes.

Use barriers/channels and explicit process identities. Do not use timing sleeps as the proof.

### R0.3 ACP process-exit authority reds

For live, background, parked, and a shared process with multiple tenants, prove the current bug:

- old Bearer remains accepted after `ProcessExited`;
- Broker status still exposes a live/authorized run or can dispatch;
- desired MCP state is not absent;
- generation-bound resource cleanup is not scheduled.

Add a late-event race in which process A exits after the session has moved to process B. The
correct repair must fence A without damaging B.

### R0.4 Pairing threat-model red ledger

Do not implement Existing Tabs yet. Add threat-model/test rows demonstrating that a no-Origin
loopback caller can read the current public challenge and, after App confirmation, complete the
current dual-confirm route while the submitted `response` is ignored and without proving
possession of an extension-only secret. Mark the
current skeleton `blocked_design`; do not expose a working exploit outside the owned fixture.

Checkpoint R0 with every red test name, exact failure, and expected postcondition.

## 7. R1: panic-safe runtime test isolation

Implement a small scoped environment guard in test support or an existing test utility:

- acquire `APP_HOME_ENV_LOCK` before reading or changing process environment;
- capture `OsString` values, including the absent case;
- set temporary values only inside the scope;
- restore in `Drop` during success and unwind;
- keep the guard alive through every `repair_from_install`, `diagnose`, Node resolution,
  Playwright archive resolution, byte check, and assertion;
- remove temporary files only after handles are released;
- gate the lock/helper as test-only so it is not dead code in the release lib target.

Do not catch a product panic merely to turn the test green. `catch_unwind` is appropriate only in
the guard's restoration test.

Acceptance:

- all R0.1 reds turn green;
- the exact runtime test passes in the manifest-embedded Windows App harness;
- the full App harness passes with the existing intentional ignore still reported explicitly;
- Core runtime tests remain green;
- no temporary App home, active pack, lock, or changed environment survives the batch.

## 8. R2: lifecycle authority and exact process ownership

### R2.1 Handshake termination primitive

Do not keep a loose `kill_handshake_acp: bool` attached to a Computer Use cleanup result. Introduce
an exact process action or a dedicated handshake-abort coordinator with these properties:

- the local Computer Use fence and exact ACP ownership capture happen synchronously;
- the captured identity includes the App session and exact ACP/process incarnation;
- the old endpoint is removed or marked terminating before the Stop snapshot returns;
- an empty cleanup plan does not discard the process action;
- catalog/resource cleanup failure does not discard the process action;
- a delayed task operates on the captured old handle, never `session.acp` by current lookup;
- if the process is shared by another live/background/parked tenant, do not kill it; reconcile the
  stopped tenant only;
- exact process termination is bounded and idempotent;
- cleanup error and process termination state are represented separately in diagnostics.

Turn every R0.2 row green and add negative cases for missing/dead ACP and repeated reconnect.

### R2.2 Process-exit authority coordinator

At the first process-scoped `ProcessExited` boundary:

1. Snapshot all matching live/background/parked session ownership for that exact process
   incarnation and deduplicate App session ids.
2. For each still-matching owner, synchronously fence Broker dispatch, revoke the loopback
   credential, disable session Computer Use, and publish desired-absent.
3. Do this before removing ACP slots, parked entries, or background records and before emitting
   the final disconnected snapshot.
4. Because the ACP is already dead, do not wait for or repeatedly retry a catalog write to that
   endpoint. Record catalog transport as structurally gone while preserving the desired-absent
   generation for a future process.
5. Run exact generation-bound adapter/browser/profile cleanup behind the existing blocking
   boundary. Preserve a retry ledger only for cleanup that can still be completed.
6. A stale exit event may affect only the captured A ownership. It must not revoke B's new run,
   token, target, MCP catalog, or resources.
7. A shared process exit fences every tenant that actually owned that process. One failed tenant
   cleanup does not block fencing or cleanup of the others.

The coordinator should be shared by live and background routing rather than duplicated. Parked
sessions need the same authority postconditions even though they have no live UI stream.

### R2.3 Required lifecycle tests

Run a matrix covering:

- live Ready, Streaming, AwaitingPermission, and Connecting exits;
- background Ready/Streaming exits;
- parked-only exit;
- one process with live + background + parked co-tenants;
- no Computer Use run;
- active Desktop, Managed Browser, and WebView runs;
- cleanup success, adapter failure, worker timeout, and repeated Retry;
- credential call immediately before and after exit;
- action already in flight plus queued action;
- duplicate exit notification;
- old A exit after B reconnect/authorization;
- exit while feature-off/App-exit is already fencing;
- zero post-exit dispatch and zero wrong-target action.

Assertions must inspect actual Broker state, actual credential acceptance, desired/applied MCP
generation, cleanup ledger, owned resource state, UI snapshot, and child processes. Helper return
values alone are insufficient.

## 9. R3: App Clippy and module ownership

First inventory every App Clippy error and classify it:

- shipping production API: wire it correctly or remove it;
- unit-test support: `cfg(test)` and focused test modules;
- probe/App-shell/fixture support: move it to `cu-probe` or gate the module under the existing
  non-default `computer-use-probe` feature;
- ordinary lint: apply an equivalent idiomatic rewrite.

Specific requirements:

- do not add blanket `allow(dead_code)` or `allow(clippy::...)`;
- do not weaken CI or omit `--all-targets`;
- use request/context structs for real eight-argument APIs;
- use `inspect_err`, direct result returns, `clamp`, and filter/map rewrites where semantics match;
- remove probe-only fixture code from the release App target;
- verify the default release feature set does not register hidden probe commands or env backdoors.

Restore the file-count budget through real domain extraction. Preferred ownership boundaries:

- split MCP reconcile state, cleanup coordinator, and tests out of
  `session_manager/computer_use.rs`;
- move App-shell scenario/report code to the probe crate/surface;
- split WebView executor, extraction/normalization, and tests;
- split Windows capture, input dispatch, and adapter tests.

Do not move arbitrary chunks or create forwarding-only files. Preserve public behavior and keep
every new file focused and below 1000 lines where practical.

Acceptance:

```text
cargo clippy -p grok-computer-use-core --all-targets --all-features --locked --offline -- -D warnings
cargo clippy -p grok-app --all-targets --locked --offline -- -D warnings
python scripts/check-code-quality-gates.py --mode final
```

Both Clippy commands and the final quality gate must exit zero. The file count must be <=80.

## 10. R4: Tiptap security update and frontend regression

Upgrade the Tiptap dependency cohort consistently to a compatible patched version at or above
3.30.5. Keep `@tiptap/core`, `pm`, `react`, Starter Kit, link, placeholder, and transitive extension
versions compatible; verify `tiptap-markdown` peer resolution. Avoid unrelated dependency churn.

Acceptance:

```text
pnpm deps:check
pnpm audit:prod
pnpm typecheck
pnpm lint
pnpm test
pnpm build:ui
```

The production audit must report zero vulnerabilities at the configured threshold. The complete
frontend count must be recorded from discovery; no snapshot update or test deletion is allowed
without an independently justified behavior change.

## 11. R5: production-wired lifecycle and credential fault matrix

Build deterministic test seams around the real production renderer, credential registry, ACP
client/stub, SessionManager coordinator, Broker, surface adapters, and cleanup ledgers. Test seams
must be compile-time test/probe-only dependencies, never a shipping environment-variable
backdoor.

Pause before and after each relevant side effect:

- attempt creation and target authorization;
- desired-present publication;
- credential creation and entry rendering;
- ACP send, apply, response, timeout, disconnect, and applied-response-lost;
- local authority fence and credential revocation;
- desired-absent publication and catalog replacement;
- surface cancellation/release;
- session delete bookkeeping and `forget_session`;
- process exit ownership removal;
- updater/App-exit deadline.

Required lifecycle rows:

- initial attach, same-run base catalog rebuild, explicit rejection, timeout, and lost response;
- ordinary Stop, handshake Stop, model Stop, exact authorization cancel, duplicate Stop, Retry;
- delete success/failure, feature-off with two sessions, logout/account/provider/data-root change;
- reconnect while desired-present and while desired-absent cleanup is pending;
- live/background/parked soft respawn and process crash;
- context change, model change, compact, fork, and session resume;
- shared ACP tenants A/B with one attach/detach/crash/failure;
- updater relaunch and App exit at every phase;
- late A completion after B authorization.

Every row asserts:

- exact session/run/attempt/process and generation identity;
- actual ACP catalog contains every base MCP entry and exactly the intended Computer Use entry;
- actual old/new credential call is authorized or 401 as expected without logging its value;
- Broker grant, snapshot, in-flight state, lease, and stop state;
- zero wrong-target and post-fence dispatch;
- exact resource owner, borrowed-tab return, owned-profile cleanup, PID/port cleanup;
- retry ledger exists only when a real retryable operation remains;
- late results cannot rewrite a newer generation.

At least one matrix implementation must execute through the Tauri command/session event boundary,
not only a direct helper.

## 12. R6: complete gates, rebuild, fingerprint, and App-shell

Run these serially and record exact discovered/pass/fail/ignored counts:

```text
cargo fmt --all -- --check
cargo test -p grok-computer-use-core --locked --offline
cargo test -p grok-computer-use-core --all-features --test driver --locked --offline
cargo clippy -p grok-computer-use-core --all-targets --all-features --locked --offline -- -D warnings
cargo check -p grok-app --all-targets --locked --offline
cargo clippy -p grok-app --all-targets --locked --offline -- -D warnings
cargo test -p grok-app --lib --locked --offline --no-run
pnpm deps:check
pnpm audit:prod
pnpm typecheck
pnpm lint
pnpm test
pnpm build:ui
pnpm check:computer-use
node scripts/audit-computer-use-bundle.mjs
python scripts/check-code-quality-gates.py --mode final
git diff --check
```

On Windows, post-link every current App test harness with `windows-test-manifest.xml` using the
Windows SDK `mt.exe`, then run the exact harness directly. Do not add a second
`/MANIFESTINPUT` in `build.rs`. A zero-test filter is not a pass.

Build the branch App, current `cu_probe`, and current ACP stub from source. After the final
candidate-affecting change:

1. compute the full source/test/runner/config/runtime/lockfile fingerprint;
2. append the freeze row;
3. run two complete App-shell smokes with fresh isolated homes;
4. verify both slash entries open the intended panel and do not authorize a target;
5. verify actual catalog and credential behavior in each smoke;
6. audit every owned child PID, port, profile, temp tree, and lease after exit.

Any change after freeze invalidates both smokes and starts R6 again.

## 13. R7: active candidate soak and mutation

On one frozen fingerprint, execute at least:

- Desktop full lifecycle: 30 iterations;
- Managed Browser full lifecycle: 30 iterations;
- App WebView full lifecycle: 30 iterations;
- handshake Stop matrix: 50 seeded iterations;
- ACP crash live/background/parked/shared matrix: 100 seeded iterations;
- broader lifecycle phase/fault matrix: 300 seeded iterations;
- slash Desktop and Managed Browser rows in every smoke batch and periodically during soak;
- at least two hours of actual scenario execution after the final freeze.

Active time is the sum of recorded scenario start/end durations. Build time, idle time, sleep,
waiting for the user, and an interrupted Goal turn do not count. Do not add sleeps to meet the
clock; increase meaningful seeded, mutation, negative, and leak-check execution.

Each non-safety class must be >=90% successful. These counters must be exactly zero:

- unauthorized read/write;
- wrong-target action;
- action dispatched after local fence/Stop/process exit;
- stale credential accepted;
- old cleanup or exit event damaging a new run/process;
- stale Computer Use MCP entry or lost base MCP entry;
- secret, Cookie, token, private URL/query, or private path leakage;
- borrowed user tab closed;
- owned profile/worker/browser/lease orphan;
- unaccounted child process or loopback listener.

A safety counter above zero invalidates Track A even if aggregate success is high.

## 14. R8: secure Existing Tabs only after Track A

R8 may start only when R1-R7 are green on one fingerprint. If Track A is not green, spend the R8
budget repairing and refreezing Track A. Do not add browser code to manufacture visible progress.

### R8.1 Replace the pairing design first

Update the pairing ADR and threat model for malicious local processes, arbitrary loopback pages,
malicious pages, hostile extensions, replay, two App instances, browser restart, worker suspend,
navigation, reconnect, concurrent Share/Stop, stale documents, and key leakage.

The replacement must have:

- an explicit App action and an explicit extension popup/service-worker user gesture;
- proof of a CSPRNG one-time secret/code that is shown in App or transferred over a genuinely
  protected channel, never returned by the public challenge endpoint;
- no auto-confirm when a DOM button is absent;
- no content script injected into every loopback page;
- no trust in Origin, Host, claimed extension id, or a public nonce as sole identity;
- challenge expiry, one-time burn, replay protection, App instance binding, extension build/id
  binding, connection generation, and key rotation;
- session keys only in memory or `chrome.storage.session`, never local/sync storage, URLs, page
  DOM, history, logs, screenshots, or Tauri frontend state;
- revoke on App exit, feature-off, extension/browser restart, explicit Unpair, and protocol error.

Write attacker-first tests before replacing the fixture implementation.

### R8.2 Minimal extension and typed transport

Build one real, narrow vertical slice:

- extension popup: Pair, Share current tab, Stop sharing, state/error, Retry;
- minimum justified MV3 permissions, normally `activeTab` + `scripting` and `storage` only if
  `storage.session` is used; justify any broader `tabs`, host, debugger, or capture permission;
- authenticated loopback WebSocket or bounded request channel owned by the service worker;
- typed envelopes with protocol version, request id, deadline, cancellation, sequence, connection
  generation, tab id, and document generation;
- heartbeat, bounded reconnect/backoff, duplicate/replay handling, and key rotation;
- isolated-world observation and typed click/set-value/type/key/scroll/wait/navigation only;
- screenshot only when the explicitly shared tab is current and visible; otherwise pause/fail
  closed rather than switching tabs or fabricating equivalence;
- navigation/reload/close/disconnect increments generation and invalidates old observations before
  any action;
- only explicitly shared candidates appear in the App picker;
- Host registers `SurfaceKind::ExistingTab` only while an authenticated transport is alive;
- Stop returns the borrowed tab and removes injection/grant; it never closes the user tab;
- no arbitrary eval, Cookie/storage export, broad tab enumeration, hidden-tab capture, or Desktop
  fallback.

Required local evidence separates source Chrome, source Edge, installed Chrome, and installed
Edge. Unavailable installed rows remain `not_run` or `blocked_external`. A manifest, extension
load, pairing state machine, or in-memory offered tab does not count as the vertical slice.

## 15. Cross-platform and installed matrix

The final report always includes separate rows for:

- Windows source App;
- Windows portable/NSIS install, repair, upgrade, rollback, uninstall;
- macOS arm64 source and signed/notarized install;
- macOS x64 source and signed/notarized install;
- Linux X11 source/AppImage;
- GNOME native Wayland source/AppImage;
- source and installed Chrome Existing Tabs;
- source and installed Edge Existing Tabs;
- ACP stub;
- real model with explicit human authorization.

On this Windows machine, unavailable external rows remain `not_run` or `blocked_external`.
Cross-compilation, mocks, XWayland, browser automation, copied logs, or another fingerprint cannot
satisfy them. Because all four native targets are v1 release requirements, even a perfect local
Windows result remains overall `partial - not releasable`.

## 16. Checkpoints and persistence

Write a checkpoint after every atomic batch and at least every 60-90 minutes. Then continue
automatically; do not ask whether to proceed.

Each checkpoint contains:

- run id, branch/HEAD, fingerprint, freeze state, and invalidation history;
- exact files changed in the batch;
- manifest sequence range and exact command/test counts;
- first failure, evidence-based hypothesis, repair, and recovery row;
- redacted credential and actual ACP catalog results;
- Stop/crash/delete/reconnect/respawn/feature-off/exit postconditions;
- surface/resource ownership and cleanup state;
- active soak time, scenario counts, seeds, rates, and safety counters;
- child PIDs, ports, profiles, leases, and cleanup result;
- external blockers and ready local work;
- next action;
- literal text: `未 commit / 未 push / 未提 PR`.

Do not retry the same failure more than three times with the same hypothesis. Preserve the first
failure, mark the atomic row failed, change the hypothesis or move to independent ready work.
Stop only when the user explicitly stops, a new real-account/system permission is required, or
every remaining item is externally blocked.

## 17. R9 final report contract

Generate the report from the validated manifest. It must include:

- branch, HEAD, complete dirty inventory, run id, fingerprints, and freeze history;
- every batch/scenario with evidence level and exact pass/fail/ignored/not-run counts;
- all first failures and recoveries;
- runtime isolation and Windows manifest-harness results;
- handshake Stop and process-exit matrices;
- actual credential oracle and ACP catalog/base-MCP results without credential values;
- all lifecycle entry points and phase faults;
- complete quality, dependency, frontend, Rust, build, and bundle gates;
- App-shell results and active duration/rounds/seeds/rates/safety counters;
- Desktop/Managed/WebView/Existing Tabs capability and evidence boundaries;
- Windows source/installed, macOS, Linux X11, native Wayland, stub, and real-model rows;
- owned resource cleanup and unrelated-process preservation;
- residual P0/P1/P2, technical debt, external blockers, and exact next action;
- literal text: `未 commit / 未 push / 未提 PR`.

Allowed top-level conclusions are only:

```text
Track A local source gate passed; overall Computer Use partial - not releasable
Track A and secure Existing Tabs local source gate passed; overall Computer Use partial - not releasable
partial - not releasable
```

Do not write `complete`, `releasable`, `cross-platform passed`, `installed passed`, or
`real-model passed` unless every corresponding current-fingerprint evidence row exists. Under the
known external matrix, the expected overall result remains `partial - not releasable`.

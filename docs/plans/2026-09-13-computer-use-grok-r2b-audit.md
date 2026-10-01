# Computer Use R2B post-implementation audit

Date: 2026-09-13

Workspace: `H:\aicoding\grok-app-computer-use`

Branch: `feat/computer-use-implementation`

HEAD: `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`

Observed implementation fingerprint:
`36f58cdbda0270a69fd71c2a44dd0d1a3980ed129ef1d893c5aff7b38707e635`

Verdict: **partial - not releasable**

## Audit boundary

This audit reviewed the dirty worktree after Grok reported the R2 lifecycle
task complete. It preserved all tracked, untracked, ignored, and evidence
files. It did not reset, restore, clean, stash, commit, push, open a PR, merge,
tag, or release. Only this audit and its companion execution/prompt documents
were added by Codex.

The fingerprint above belongs to Grok's pre-document implementation and the
ignored evidence run at:

`tools/computer-use-probe/.run/finalization/20260913T034919Z-seven`

Adding these documents changes the untracked Merkle root. The fingerprint
must therefore be recomputed before the next implementation or evidence run.

The review distinguishes four evidence levels:

- source and unit behavior;
- App command and ACP-stub behavior;
- branch-built App-shell behavior on this Windows machine;
- installed, real-model, and cross-platform behavior.

Passing an earlier level is not evidence for a later one.

## Executive assessment

Grok made meaningful progress. The work is not a disposable prototype: Host
authorization generations, selector fencing, a per-session MCP catalog
serializer, exact authorization cancellation, and several cleanup hooks now
exist. The focused suites and short App-shell runs are healthy.

The completion claim is nevertheless incorrect. The new reconciler is only
catalog-idempotent. It is not credential-idempotent: every catalog rebuild
rotates the live IPC token before ACP confirms the replacement. A harmless
base MCP update or an ambiguous retry can therefore disable the MCP child that
ACP is still using. Several lifecycle entry points also revoke authority
without proving catalog absence, and two destructive paths discard the state
needed to retry cleanup.

Track A must be repaired and reverified before Existing Tabs development may
resume. Even after Track A and a local Existing Tabs implementation pass, the
overall product remains `partial - not releasable` until installed and
cross-platform acceptance is performed.

## Findings

### P0. MCP catalog reconciliation rotates authority before catalog commit

The reconciler correctly treats the ACP MCP catalog as a full replacement and
serializes updates per App session. However, building the desired document has
a side effect:

1. `SessionManager::build_catalog` invokes `build_entry` every time it
   constructs a desired-present catalog
   (`src-tauri/src/session_manager/computer_use.rs:193-215`).
2. The production builder is `computer_use::mcp_acp_entry`
   (`src-tauri/src/session_manager/computer_use.rs:29-35`).
3. `mcp_acp_entry_with` calls `IpcServer::issue_session`
   (`src-tauri/src/computer_use/inject.rs:50-84`).
4. `issue_session` creates a new token and removes every existing credential
   for that App session before returning it
   (`src-tauri/computer-use-core/src/ipc.rs:97-121`).

That makes these apparently idempotent operations unsafe:

```text
ACP catalog currently contains token T1 and its MCP child is using T1
    -> base extension preference changes, or attach retry starts
    -> build desired catalog and issue T2
    -> issue_session deletes T1 immediately
    -> ACP update is blocked, fails, times out, or has not restarted child yet
    -> the still-current child presents T1 and receives unauthorized
```

An ambiguous timeout is worse: the retry creates T3 and revokes T2 even though
ACP may already have applied T2. The replacement JSON may be equivalent at the
catalog level, but its credential is not equivalent.

This affects both availability and transaction correctness. Token revocation
must follow ownership and catalog commit, not catalog construction. A catalog
builder must be pure with respect to live authority.

The current reconciler tests do not catch the defect. Their injected
`build_entry` creates a fake entry with session/run fields and no real token
(`src-tauri/src/session_manager/computer_use.rs:661-675`). They prove catalog
shape and generation convergence but never exercise `IpcServer::issue_session`
or call the old/new credentials through IPC.

Required correction:

- introduce a session/run/attempt-bound credential lease owned by the
  lifecycle coordinator;
- reuse the current committed token when the eligible run has not changed;
- stage a replacement token without revoking the committed token;
- commit the new token only after the matching ACP catalog update succeeds and
  the desired generation is still current;
- on failure or stale generation, revoke only the staged token;
- on stop, synchronously deny dispatch for every token belonging to the
  revoked authority, then converge catalog absence;
- test real old/new tokens against the loopback endpoint across success,
  failure, timeout, base-update, and stale-generation paths.

### P0. Normal chat Stop revokes Computer Use but does not detach its MCP entry

The Computer panel's dedicated `computer_use_stop` command revokes and then
reconciles. The normal chat Stop path does not.

`SessionManager::stop` calls `computer_use::sessions::revoke` at
`src-tauri/src/session_manager/turn.rs:846`. The handshake-abort branch returns
at line 864, and the ordinary branch completes after agent cancellation and
`flush_pending_soft_respawn` at lines 957-1005. Neither branch calls
`detach_computer_use`, `revoke_and_detach_computer_use`, or another catalog
barrier.

This path is used by composer/Escape Stop, task rows, Stop All, dashboard Stop,
and remote session Stop. Revoking the Broker and IPC token makes tool calls
fail closed, but the ACP session may retain the advertised MCP server and its
dead child. The UI can also paint the session as stopped while cleanup is not
represented.

Required correction: route all session Stop variants through one lifecycle
coordinator. Authority revocation must happen before agent cancellation;
catalog absence may complete asynchronously, but a pending detach must remain
observable and retryable. The handshake early return must not bypass it.

### P0. Session deletion discards a failed cleanup and its retry ledger

`SessionManager::drop_session_agent` invokes
`revoke_and_detach_computer_use`, but converts failure into a warning and
continues (`src-tauri/src/session_manager/control.rs:1245-1253`). It returns
`()` rather than a cleanup result.

`session_delete` then deletes the journal and invokes
`forget_deleted_session` unconditionally
(`src-tauri/src/commands/session_p1.rs:901-913`). That function removes the
per-session MCP lock and status, along with other bookkeeping
(`src-tauri/src/session_manager/mod.rs:149-159`).

If a shared ACP process remains alive, this sequence can leave its App session
catalog advertising `grok-computer-use` while the App has removed the session
and the retry state needed to identify or repair it.

Required correction: deletion needs a two-phase contract. Either cleanup
converges before destructive metadata deletion, or a durable/redacted cleanup
tombstone retains the ACP identity and desired-absent generation until repair.
The command must not report success and erase the ledger after a detach error.

### P0. App exit has no asynchronous MCP-detach barrier

The Tauri exit hook calls the synchronous
`computer_use::shutdown_product` at `src-tauri/src/lib.rs:1870-1878`.
`shutdown_product` revokes grants, disables the Broker, unbinds WebView, and
stops the managed browser (`src-tauri/src/computer_use/mod.rs:128-140`). It
does not have a `SessionManager`, does not call ACP catalog reconciliation, and
cannot await it.

Consequently, a shared or slow ACP process can outlive local authority cleanup
without receiving a catalog-absent update. The App-shell's
`app_close_cleanup` row currently calls the same synchronous helper, so its
pass does not prove ACP catalog detach on real application exit.

Required correction: add an idempotent App shutdown coordinator and invoke it
at a preventable/bounded exit stage before the final `RunEvent::Exit`. It must
revoke synchronously, await all known catalog-absent reconciliations within a
bounded budget, retain truthful failure evidence, and then release resources.
The final non-preventable exit hook remains a last-resort local revoke, not the
primary proof of clean shutdown.

### P1. Reconnect retries desired-present but abandons desired-absent cleanup

After a fresh ACP session is opened, `connect.rs:1468-1481` invokes the
reconciler only when `mcp_desired.run_id.is_some()`. A failed detach leaves
desired absent with `cleanup_pending`, which this reconnect path ignores.

Required correction: reconnect must reconcile any unapplied or pending
generation, including desired absent. A new ACP identity must start from a
known base-only catalog; it cannot inherit the previous status merely because
the current desired run is `None`.

### P1. Deferred background soft-respawn removes ownership before detach

The background branch of `flush_pending_soft_respawn` revokes process-local
Computer Use state and removes the background endpoint at
`src-tauri/src/session_manager/control.rs:140-146`. Unlike the live
`soft_respawn_with_reason` path, it does not first await
`revoke_and_detach_computer_use` while the endpoint is still discoverable.

If the ACP process has another tenant, it is deliberately not killed. That is
the correct shared-process rule, but it makes the missing detach observable:
the surviving process can retain the removed session's CU catalog entry.

### P1. Cleanup pending is displayed but cannot be retried from the panel

The status DTO exposes `cleanupPending`, and the panel renders the localized
message (`src/components/computer-use/ComputerPanel.tsx:471-478`). The Stop
button is disabled once there is no busy/running run
(`src/components/computer-use/ComputerPanel.tsx:448-460`). There is no separate
Retry cleanup command or enabled action for desired-absent pending state.

Required correction: provide an explicit idempotent retry action backed by the
same Host coordinator. It should be available only for pending cleanup, show a
busy state, preserve the error on failure, and disappear only after catalog
absence is confirmed.

### P1. Cancel-before-begin tombstones are globally evictable

`SessionGrants` keeps all cancelled attempts in one global 256-entry FIFO
(`src-tauri/computer-use-core/src/session_grants.rs:6,38-47,61-77`). A noisy
session can evict another session's recent cancel-before-begin tombstone. A
late authorize command for that evicted attempt can then become valid.

Required correction: bound tombstones per session, add an explicit expiry
window longer than the maximum command/authorization lifetime, and remove a
tombstone only after its late begin can no longer arrive. Tests must use at
least two sessions and exceed the per-session/global capacity boundary.

### P1. The App-level fault matrix still skips the transaction phases that matter

Current focused tests establish important generation and catalog-shape
properties. They do not execute the production Tauri transaction with real
credentials through barriers at:

- target provision/bind;
- Broker authorize;
- session activation and token issue;
- MCP update before send, after apply, before reply, and timeout;
- final `complete` publication;
- reverse-order compensation;
- session Stop, deletion, reconnect, soft-respawn, and App exit.

The next suite needs a production-wired test transport and a phase barrier,
not sleeps or source-string assertions. Every injected failure must assert the
final Broker grant, IPC authorization, ACP catalog, surface binding, resource
ownership, and retry state.

### P1. Slash evidence is still explicitly non-blocking

The App-shell records these errors:

```text
slash_desktop: missing-hook:chrome-error://chromewebdata/:complete:undefined
slash_managed_browser: missing-hook:chrome-error://chromewebdata/:complete:undefined
```

`app_shell.rs:570-573` filters both `missing-hook` and `slash_` errors out of
the blocking list. The harness was not serving a real built frontend, so these
failures do not prove a product regression. They also cannot be counted as a
pass.

Required correction: serve the exact built frontend used by the candidate,
wait for the app hook with a bounded readiness protocol, keep Desktop and
Managed slash rows separate, and make both rows blocking. A missing hook,
error page, or undefined callback is a failed scenario.

### P1. The current finalization ledger is not scenario-derived

The ignored run's `state.json` remains at `F0/in_progress`, while its manifest
contains one F1 App-shell pass and one combined F3 Managed/WebView pass. It has
no lifecycle phase matrix, no token assertions, and no long soak. It is useful
smoke evidence, not finalization evidence.

The final state and summary must be generated from atomic manifest rows. A
combined process exit of zero must not overwrite an individual scenario
failure.

## What Grok actually completed

The following improvements are present and should be preserved:

1. Host authorization tickets now include a Host-generated generation in
   addition to the UI correlation ID and selector revision.
2. Late success/failure and exact cancel paths are fenced against a newer
   ticket in `SessionGrants`.
3. The UI and Host expose `computer_use_cancel_authorization`, and the panel
   cancels the exact pending attempt during Stop, relevant unmount/surface
   change, and chat change flows.
4. MCP desired state has a generation and the SessionManager serializes
   full-catalog replacements per App session.
5. A late attach observes newer desired state and follows with an absent
   replacement in the existing ACP-stub tests.
6. Dedicated Computer panel Stop and feature-off paths now attempt MCP detach.
7. Live/background/parked ACP endpoints can be enumerated for process-wide
   cleanup, and base extension MCP changes share a catalog revision.
8. Cleanup status is redacted and exposed to the UI without leaking command,
   token, environment, or catalog data.
9. Deleted-session bookkeeping cleanup was added and is locally unit-tested.
10. The previous Desktop, Managed Browser, and App WebView execution work
    remains intact in short branch-built App-shell smoke runs.

These are substantial Track A building blocks. They do not eliminate the P0
transaction failures above.

## Verification performed by Codex

The following checks were completed against Grok's implementation before
these documents were added:

| Check | Result | Scope |
| --- | ---: | --- |
| `grok-computer-use-core` tests | 310/310 passed | core unit/integration behavior |
| shared driver tests | 12/12 passed | driver contract |
| focused frontend tests | 17/17 passed | panel/API cancellation behavior |
| TypeScript | passed | `pnpm typecheck` |
| core Clippy | passed | all targets/features with `-D warnings` |
| App MCP lifecycle tests | 6/6 passed | injected ACP catalog shape/generation |
| deleted bookkeeping test | 1/1 passed | in-memory map cleanup only |
| App library compile | passed | `--no-run`, 34 existing warnings |
| `git diff --check` | no whitespace errors | LF/CRLF conversion warnings remain |
| Desktop App-shell | 2 short rounds passed | branch-built Windows smoke |
| Managed Browser App-shell | 1 short round passed | branch-built Windows smoke |
| App WebView App-shell | 1 short round passed | branch-built Windows smoke |

All observed App-shell processes exited with code 0, without a Tokio runtime
drop panic, and released the isolated Desktop lease. No matching App, Node, or
Chromium process remained after the checked runs.

This evidence does **not** execute the real-token retry bug, ordinary
`session_stop`, failed delete, cleanup-pending reconnect, background
soft-respawn, or an ACP-aware App exit barrier. It also does not prove slash
entry, installed packaging, real-model selection, Existing Tabs, or another
operating system.

## Corrected status matrix

| Area | Current status | Highest defensible evidence |
| --- | --- | --- |
| Host attempt generation/fencing | implemented, focused | E1/E2 tests |
| Exact authorization cancel | implemented, focused | E1/E2 + frontend tests |
| Catalog serializer/generation | partial | shape tests pass; credential transaction fails review |
| IPC token lifecycle | failed/incomplete | production path rotates before commit |
| Dedicated Computer panel Stop | partial | attempts detach; retry UX incomplete |
| Normal chat/Stop All paths | failed/incomplete | revoke only |
| Delete cleanup | failed/incomplete | failure swallowed and ledger erased |
| Reconnect/soft-respawn cleanup | incomplete | desired-absent and background gaps |
| App exit cleanup | failed/incomplete | no async ACP barrier |
| Slash UI entry | not established | missing hook is filtered |
| Desktop on current Windows source | short smoke passed | branch-built E3 fixture |
| Managed Browser on current Windows source | short smoke passed | branch-built E3 fixture |
| App WebView on current Windows source | short smoke passed | branch-built E3 fixture |
| Existing Tabs | incomplete/security-blocked | pairing skeleton only |
| Windows installed package | not established | source/runtime seed only |
| macOS arm64/x64 | incomplete/not run | source prototype |
| Linux X11 | incomplete/not run | source prototype |
| GNOME Wayland | incomplete/not run | no accepted native path |
| Real model E4 | not run | requires explicit user/environment authorization |
| Release candidate soak | not run | no frozen lifecycle matrix |

## Required next order

The next Grok run must follow this order:

1. Make IPC credential creation, staging, commit, reuse, and revocation
   transactional; add production-entry token tests first.
2. Route normal Stop, deletion, background soft-respawn, reconnect, and App
   exit through one cleanup coordinator with explicit pending state.
3. Add user-triggered cleanup retry and per-session expiring cancel
   tombstones.
4. Add the complete Tauri phase barrier/fault matrix with real catalog entries
   and loopback credential assertions.
5. Repair the slash harness against the exact built frontend and make required
   rows blocking.
6. Freeze a new fingerprint, run short smoke, the repeated lifecycle matrix,
   and at least two hours of active lifecycle/fault soak on that same
   fingerprint.
7. Enter Existing Tabs only if every Track A gate passes. Start with the
   possession-proof pairing protocol and typed MV3 transport; do not build on
   the current secretless skeleton.

Until those gates and the later platform/install/model acceptance are complete,
the only accurate overall conclusion is **partial - not releasable**.

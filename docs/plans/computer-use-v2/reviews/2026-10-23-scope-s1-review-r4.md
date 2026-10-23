**MERGE-AFTER-FIXES**

Reviewed exactly `git diff a91d9551..6ce48df7` (HEAD `6ce48df7`, `cu2/scope-s1`). Unchanged code was consulted only to trace changed paths. Paths below are relative to `computer-use/crates/`. Counterexamples and test sensitivity are source-level assessments, not executed reproductions.

| R3 finding | Disposition | Evidence |
| --- | --- | --- |
| R3-1: core publication | **PARTIAL** | `gcu-core/src/action/observe.rs:297` now checks after policy, thumbnail and snapshot-lock acquisition; wait preserves publication refusals (`action/wait.rs:168`). The check still precedes substantial snapshot work and the direct return: R4-1. |
| R3-2: MCP awaits/files/cache/attachments | **PARTIAL** | `gcu-mcp/src/result.rs:262`, `:273`, `:427`, `:463`, `:477` recheck, discard completed files and defer cache insertion. Normal completed-worker invalidation drops attachments/state while preserving unknown/error identity. Worker publication, late cache insertion and unstamped encoder entrypoints remain: R4-2/R4-4. |
| R3-3: WindowGone/windows/target labels/batch targets | **PARTIAL** | WindowGone gains stamps (`gcu-core/src/action/failure.rs:134`); batch carries its captured epoch (`batch/executor.rs:95`); MCP scrubs windows, state/error state and targets (`gcu-mcp/src/result_guard.rs:135`). Held old-generation outcomes are rejected. WindowGone can still assign a newer epoch to old metadata: R4-3. The fixed-sentence exception is also not content-free; see the audit below. |

Must-fix findings — category (a) only:

1. **[P1, R4-1; remaining R3-1] Core can revoke during `record`, then return the revoked sample.** `gcu-core/src/action/observe.rs:297`, `:305`, `:338`.

   After the check releases the scope lock, `record` sanitizes/indexes the tree, renders full text, computes diffs and pixel hashes, and commits the snapshot (`snapshot/store.rs:354`, `:363`, `:379`, `:455`). Revoke/remove-A rebind during that work; publication then stamps and returns A's text, app/window metadata and image to a direct core caller without another check.

   The author's residual is therefore real category (a), and the interval includes substantial work. A cheap closing check after recording/assembly, while the snapshot mutex is still held, can refuse and discard the new snapshot before exposing it. Stage/commit is another option. Keep only the instruction-level interval after a final check as residual; closing this longer interval needs no scope lock across rendering or an await. Add a deterministic barrier inside/after recording and assert no returned sample or retained new snapshot. Current publication tests pause before `:297`, so they miss this schedule.

2. **[P1, R4-2; remaining R3-2] File workers publish before checking authority; timeout/drop can bypass cleanup entirely.** `gcu-mcp/src/result.rs:391`, `:414`; `files.rs:54`, `:63`, `:71`, `:216`.

   Park a valid A encoder before its file write, revoke, then resume: the worker still writes A bytes to the OS and registers the file before the async continuation can discard it. A concurrent `Connection::export_private_files` can return those bytes (`connection.rs:82`). More decisively, let the five-second blocking timeout expire, or drop the encoding future, before resuming the worker: `spawn_blocking` continues, the eventual path never reaches `created`, and the newly written revoked file remains exportable. This concerns an undelivered response's new file, not retirement of a previously delivered file.

   Carry authority/cancellation into worker publication, check after worker/area-lock waits, and ensure late/unclaimed results cannot enter the export ledger or leave files behind. Also close the late cache boundary: the final check at `result.rs:477` precedes `ImageCache::insert`'s mutex (`image_cache.rs:8`) and `screenshot_delivered`'s snapshot mutex (`gcu-core/src/session/delivery.rs:26`). Revoke while insertion waits can install a new revoked image and return its attachment; cache insertion needs validation at its commit boundary and response validation must follow intervening lock waits. Test worker timeout/drop, concurrent export and a held cache/snapshot lock.

3. **[P1, R4-3; remaining R3-3] WindowGone can stamp old A metadata with the current B-only epoch.** `gcu-core/src/action/failure.rs:53`, `:76`, `:119`, `:134`.

   Early WindowGone failures run outside `with_attempt` (`action/router.rs:335`; also `action/wait.rs:129`). Resolve A and obtain WindowGone in generation 1; rebind to B-only before/during failure refresh. The failed re-resolution falls back to the old A `AppRef`; both reads fail. The new fallback at `failure.rs:76` then takes generation 2 from the current lease, successfully lists B, and stamps the synthetic state generation 2. Its `state.app` still contains A's name/identity, and its text/window still identify A's old window. A direct core caller receives that metadata, and MCP accepts the generation-2 stamp and forwards its facts.

   Retain the originating authority for all included metadata, or re-admit every included target; taking the current epoch after failed refresh is insufficient. Test rebind during failure assembly, including the early and wait-error paths. The added WindowGone test changes authority only after assembly has finished.

4. **[P1, R4-4; new encoder coverage gap] Discovery and script output explicitly disable the new publication guard.** `gcu-mcp/src/result.rs:194`, `:214`.

   Both pass `LiveStamp::off()`, for which every later `rejected` check is false. `list_apps`/`list_windows` can finish an authorized A read, enter discovery encoding, then revoke while a long-list spill is parked; A names/titles and the new spill file still reach the caller (`server.rs:266`, `:275`; `result.rs:455`). Script output containing an earlier authorized observation has the same unguarded finish path (`server.rs:308`). Carry producing authority from before these operations through encoding; do not obtain a replacement epoch after reading. Add revoke/remove-A tests during discovery/script encoding.

Test and refusal audit:

- **No old assertion was relaxed.** The only existing-test edits are in `gcu-mcp/src/result_tests.rs`: signature/field adaptations preserve assertions, and `:386` replaces the old unsafe unstamped-canary acceptance with stricter absence/file checks. The two other test files are additions.
- New tests meaningfully cover pre-record publication rejection (`fixture_scope_round4.rs:22`, `:95`), completed image/overflow-worker rejection (`result_tests.rs:420`), and held WindowGone/element/batch payloads (`result_scope_tests.rs:400`, `:438`). Removing the core publication guard or restoring broad unstamped acceptance would break these assertions. However, deleting the `finish` rechecks at `result.rs:427`, `:463`, `:477` leaves the new stale-worker tests protected by earlier checks. They do not demonstrate each later boundary, R4-1–R4-4, or detached-worker cleanup.
- **The unstamped fixed-sentence exception is not truly content-free.** `result_guard.rs:166` checks text/image/overflow and top-level windows/target, but ignores `ActionState.state`. An unstamped state with the exact refusal sentence and nonempty app/window IDs or screenshot metadata passes even after revoke (`:102`); `result.rs:289` serializes those facts with an empty stamp. Remove the exception or emit only the fixed sentence with no state facts. No current core producer of this fixed-line `ActionState` was found, so this guard-level counterexample is not counted as a separate demonstrated production must-fix. The actual hard-denial producer is content-free (`gcu-core/src/action/wire.rs:54`). No test exercises this exception.
- No new regression in reachable unscoped historical behavior was established. Unscoped checks retain their bypass, and ordinary same-epoch/unscoped timeout observations remain attached. Runtime/golden equivalence is unverified.

`git diff --check a91d9551..6ce48df7` passed. Cargo/compilation, runtime tests and mutation tests are **not_run**, as instructed. No network, SSH, model operations or personal agent homes were used. Source/tests were not edited. This report is uncommitted.

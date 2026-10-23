**MERGE-AFTER-FIXES**

Reviewed only `git diff 6ce48df7..HEAD`, HEAD `05dbe004`, branch `cu2/scope-s1`. Unchanged code was consulted to trace callers and locks. Paths below are relative to `computer-use/crates/`. Counterexamples are source-level assessments, not executed reproductions.

**Calibration: no NEW class of leak outside {core record window, file publication, WindowGone metadata, encoder coverage, fixed-sentence} was found.** R5-1 includes the file/cache publication boundaries already covered by R4-2; R5-2/R5-3 stay within the core/WindowGone classes. R5-4 is an unscoped behavior regression, not a new leak class. No category-(b) residual is raised as a blocker.

| R4 item | Disposition | Evidence |
| --- | --- | --- |
| R4-1: core record window | **PARTIAL** | `gcu-core/src/action/observe.rs:314` checks after `record` and discards the new snapshot on refusal (`snapshot/store.rs:209`). Another full indexing pass still follows the check: R5-2. |
| R4-2: MCP workers/publication | **PARTIAL** | Workers return unregistered guards (`gcu-mcp/src/result.rs:369`, `:387`); late timeout/drop results unlink through `files.rs:331`. Cache admission now runs under its mutex, after the gate (`image_cache.rs:27`, `:29`), and response validation follows delivery (`result.rs:471`). Early registration/visibility and cancellation after a prior registration remain: R5-1. |
| R4-3: WindowGone re-stamping | **PARTIAL** | Copying the originating epoch before refresh awaits fixes the reported newer-epoch re-stamp (`gcu-core/src/action/failure.rs:27`, `:146`). A later lock wait still permits revoked metadata to reach a direct core caller: R5-3. |
| R4-4: discovery/script encoder coverage | **FIXED** | `gcu-mcp/src/server.rs:267`, `:276`, `:317`, `:326` capture stamps before operations; `result.rs:145`, `:168`, `:444` carry/check them through encoding. Spill publication is separately R5-1. |
| Unstamped fixed-sentence exception | **FIXED** | `gcu-mcp/src/result_guard.rs:122` rejects every unstamped `ActionState`; no fixed-sentence exemption remains. The facts-canary test is `result_round5_tests.rs:274`. |

Must-fix findings — category (a) only:

1. **[P1, R5-1; R4-2 class] Per-file registration still precedes final publication.** `gcu-mcp/src/result.rs:233`, `:243`, `:400`, `:425`, `:500`; `files.rs:107`, `:122`, `:139`.

   Commit A's image while authorized, then park its overflow/finish worker and revoke or remove A. The image is already in the export ledger: concurrent `Connection::export_private_files` (`connection.rs:82`) can copy its bytes before the final check/discard. Aborting the encoder at that later await is worse: `created` is only a `Vec<PathBuf>` (`result.rs:34`, `:217`), so earlier registered files survive without cleanup. The new guard fixes the late worker's staged file, not these prior commits. Also, `commit_checked` validates at `:495` before `files.commit` waits on the area mutex (`files.rs:107`); revocation during that wait can still register a stale file.

   Normal `CallToolResult` references remain internal until `finish` returns; that is not an independent early-response leak. Exporting registered bytes (including a spill's embedded file references) is sufficient. Later `discard_all` cannot retract an export.

   The cache-lock admission fix is correct, but an undelivered entry becomes readable before the subsequent `screenshot_delivered` lock wait (`result.rs:450`, `:464`; `gcu-core/src/session/delivery.rs:26`). Park there, revoke, and call `Connection::cached_image` (`connection.rs:92`): it returns the new pixels before `:471` removes them. This concerns the current unfinished response, not retirement of previously delivered images/files.

   Keep files/cache entries private until the final publication boundary, validate after publication-lock acquisition, and give all unfinished output cancellation cleanup. Test concurrent export/cache lookup while later work is parked, plus abort after an earlier file has completed.

2. **[P1, R5-2; core record class] The closing core check still precedes substantial work.** `gcu-core/src/action/observe.rs:314`, `:323`, `:346`.

   `ElementIndex::assign` after the check traverses the whole tree, allocates maps, recursively indexes nodes and clones the element map (`snapshot/index.rs:48`, `:66`, `:98`). Revoke during that pass: the direct core caller receives A's text/metadata/pixels and the new snapshot remains. This exceeds an instruction-level return interval. Finish indexing/assembly before the closing check, retaining the snapshot mutex and discarding on refusal. The new barrier test (`tests/fixture_scope_round5.rs:19`) revokes before `:314`, so misses this schedule.

3. **[P1, R5-3; WindowGone metadata class] WindowGone can lose authority after validation while waiting for snapshots.** `gcu-core/src/action/failure.rs:89`, `:93`, `:109`, `:137`.

   Hold the session snapshot mutex, allow the final window-list check to pass, then revoke/rebind while fallback waits at `:93`. Releasing the mutex lets it assemble and return old app/window/list metadata without another validation. MCP rejects the old stamp, but direct core callers receive it. Revalidate the captured epoch after the wait and metadata assembly, clearing both state and windows on refusal. The new refresh barrier is before these boundaries (`:81`).

4. **[P2, R5-4; unscoped regression] Script output no longer preserves the historical 2,000-byte prefix.** `gcu-mcp/src/result.rs:122`, `:124`.

   Base used `prefix(output, 2_000)` for text; HEAD appends everything even when `scope` is `None`. A 3 KB script now returns its tail; a longer script creates spill files and can fail on the file budget where the old truncation succeeded. Restore unscoped prefix behavior and add a long-output equivalence check. Stamp validation does not require expanding unscoped output.

Discovery/script **option A is safe and truthful**: `FinishFault::Lost` maps to `io_error` (`gcu-mcp/src/result.rs:152`, `:175`, `:549`), which returns only a fixed internal error, no output/canary/attachments/references. It reports an encoding failure without claiming refused execution or no input after possible script side effects. R5-1 remains the separate pre-return export issue.

New tests cover first-worker timeout/drop, pre-check refusal, fixed-sentence facts and eventual cache cleanup; they do not check visibility before cleanup or cancellation after earlier registration. Existing assertions were not relaxed. `git diff --check 6ce48df7..HEAD` passed. Cargo, compilation and runtime/mutation tests: **not_run**, as instructed. No network/SSH/model operations or personal agent homes were used. Source/tests unchanged; this report is uncommitted.

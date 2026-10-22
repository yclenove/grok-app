# Session/workspace import R1.6 upstream integration runbook

Status: read-only audit complete; integration not started.

## Baseline

- Feature worktree: `H:\aicoding\grok-app-session-import`
- Feature HEAD and merge base: `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`
- Local `upstream/main`: `f013b0b96d4536cb2db0859dee9e89d407b36488`, v0.2.34
- Distance at audit time: feature 0 ahead / 57 behind
- Upstream changed 362 paths.
- Current WIP at audit time: 46 tracked modifications and 141 untracked files.
- Exact same-path overlap: 14 tracked paths, 0 untracked paths.
- No fetch, merge, rebase, stash, checkout, reset, commit or working-tree mutation was performed during this audit.

The remote tip may move. Re-run the baseline and overlap calculation immediately before integration.

## Safety gate

Do not merge or rebase the current uncommitted WIP. R1 must first be green. Then obtain user approval for recoverable local checkpoint commits. Checkpoints do not authorize push, PR, merge to the source project, tag or release.

After approval:

1. Review every checkpoint path; exclude secrets, auth files, logs and build output.
2. Create small local checkpoint commits by responsibility.
3. Fetch `upstream` and recalculate merge base, ahead/behind, changed paths and overlap.
4. Use a no-commit merge for semantic integration. Do not resolve the 14 files with whole-file `ours` or `theirs`.
5. Run all gates in this document before creating integration commits.

## Central semantic hazard: never restore I/O under session locks

Upstream commits `05ef3ddf` and `ada505bc` moved stream IPC, journal read/modify/write, turn error/cancel persistence and process-tree termination out of session locks. They introduced pending persist/emit structures, Windows local ACP process-group termination, `taskkill /T` and blocking-pool kill work.

The feature WIP currently calls continuation `notify_*` / `mark_*` functions that read or write sidecars. Some calls are inside `with_session_mut` or equivalent session-lock scopes. A mechanical merge would reintroduce the UI/session freeze class fixed upstream.

Required merged design:

- Inside a session lock: validate state and capture only immutable session id plus a typed continuation transition.
- Outside the lock: persist the transition, using `spawn_blocking` where needed.
- Preserve ordering:
  1. `Dispatching` must be durable before the prompt RPC can be sent.
  2. `Running` must be recorded immediately before `prompt_for`.
  3. success, failed-before-dispatch and unknown-after-dispatch transitions persist outside the lock.
- Background failures from `events_bg.rs` (`Error` and `ProcessExited`) must perform the same continuation transition; the current feature WIP does not cover that file.
- Never replay an UNKNOWN prompt automatically.

## Exact overlap and merge requirements

### `src-tauri/Cargo.toml`

- Keep upstream v0.2.34 manifest, safe HTTPS/web-search dependencies and Windows features.
- Add the feature's Rusqlite backup capability.
- Regenerate/resolve the lockfile from the merged manifest. Never choose an entire lockfile side.

### `src-tauri/src/account.rs`

- Keep upstream `build_oauth` module, authentication candidate ordering and `~/.grok` healing.
- Retain only the feature visibility needed for `session_roots` and `local_usage_from_roots`.
- Do not expose tokens over IPC.

### `src-tauri/src/acp_client.rs`

- Keep upstream `child_pid`, `owns_local_process_tree` and tree termination.
- Retain feature WSL/TCP handling, import-history grant, advertised-tool evidence and MCP composition.
- Initialize every new field in every constructor.
- Clear grants before kill, but never regress to child-only kill.
- Do not use local `taskkill` semantics for WSL, SSH or TCP transports.

### `src-tauri/src/lib.rs`

- Keep all upstream wallpaper and safe-HTTPS modules/commands.
- Retain session migration, early import-history MCP argv dispatch, startup recovery and all migration commands.
- There must be exactly one managed/native `SessionManager` registration.

### `src-tauri/src/session_manager/events.rs`

- Use upstream pending emit/persist structures.
- Capture continuation failure session ids/transitions while locked; commit them after releasing the lock.
- Do not restore flush/persist-under-lock behavior.

### `src-tauri/src/session_manager/mod.rs`

- Keep upstream watchdog and stream re-exports.
- Retain the feature native-manager singleton.
- Recheck global state isolation in tests.

### `src-tauri/src/session_manager/process.rs`

- Use upstream `release_failed_turn_markers` signature, `prepare_turn_error` and blocking kill behavior.
- Remove sidecar I/O from the locked helper. Return a pending continuation transition to the caller for lock-free persistence.

### `src-tauri/src/session_manager/turn.rs`

- Start from upstream pending emits/persists/stop/error flow.
- Express import first-send state changes as ordered, lock-free durable operations.
- Retain `acquire_send_connect_lock` and exact-once callbacks.
- Never mechanically paste the current locked `mark_dispatching` calls back in.

### `src-tauri/src/store.rs`

- Keep upstream wallpaper/proxy/secrets migrations.
- Retain `create_session_with_id` and `imp:` read-only rejection.
- Review schema/migration ordering even though the textual regions are mostly independent.

### `src/app/AppWorkbench.tsx`

- Keep upstream `ThemeShellContext`, `providerRetryStatusStore` and journal timeout/failed UI.
- Retain imported-search routing and wizard orchestration without adding feature state to AppWorkbench.
- Do not restore the old ThemeProvider import or local retry state.

### `src/app/WorkbenchSessionTree.tsx`

- Keep upstream memoized grouping/sorting and `NO_SESSIONS` behavior.
- Retain `ImportedHistoryGroup`.
- Do not reintroduce per-render full filtering/sorting.

### `src/app/WorkbenchSidebar.tsx`

- Keep upstream quota placement in `UserMenu`, provider balance and ARIA behavior.
- Retain the import-sessions navigation action.

### `src/hooks/useSearchPalette.ts`

- Keep upstream cleanup-time `contentSeq++` so closing the palette invalidates late results.
- Preserve `hit.kind` end to end for imported-history routing.

### `src/lib/api/session.ts`

- Keep upstream stop deadline/options semantics.
- Retain `SessionContentSearchHit.kind`.

## Indirect upstream dependencies that must remain intact

These paths do not overlap the current WIP directly, but the merged behavior depends on them:

- `src-tauri/src/process_util.rs`
- `src-tauri/src/session_manager/control.rs`
- `src-tauri/src/session_manager/events_bg.rs`
- `src-tauri/src/session_manager/stream.rs`
- `src-tauri/src/session_manager/types.rs`
- `src-tauri/src/session_manager/watchdog.rs`
- `src-tauri/src/session_manager/stall_tests.rs`
- session-connect/navigation/timeout/journal frontend modules
- `ThemeShellContext` and `SkinShareContext`
- `providerRetryStatusStore`

## Dependency advisory

The current feature base uses Tiptap 3.30.4 and `pnpm audit:prod` reports high-severity `GHSA-j95f-988m-3j2f`. Local `upstream/main` uses Tiptap 3.31.3, above the fixed threshold 3.30.5. Prefer the upstream dependency update; do not create a duplicate feature-branch upgrade. After syncing, run `pnpm install` only according to repository policy and verify the lockfile, then rerun `pnpm audit:prod`.

## Required post-merge gates

1. Search for unresolved conflict markers and run `git diff --check`.
2. `cargo fmt --all -- --check`.
3. Cargo check/clippy with repository-required flags.
4. Current full `session_migration` suite, including capability, continuation and R1 tests.
5. Upstream freeze, process-kill, timeout, stream and journal tests.
6. The 13-file session-import frontend suite.
7. Search palette, sidebar, theme and session navigation tests.
8. `pnpm typecheck`, `pnpm lint` and `pnpm build:ui`.
9. `pnpm deps:check` and `pnpm audit:prod`.
10. Isolated Windows Tauri smoke before any later PR planning.

If any test requires a newly linked Windows Rust test executable, re-embed `src-tauri/windows-test-manifest.xml` before execution. Compile-only is not runtime evidence.

**MERGE**

Reviewed only `13725df0` (`git show HEAD`). Source assessment; Cargo/runtime/mutation tests **not_run**, as instructed. References below are under `computer-use/crates/`.

1. **R6-1 fixed.** `gcu-core/src/action/fixture.rs:49` clears the entire target for scoped outcomes without published state. `action/router.rs:292` applies this before ledger recording and return, covering element-click, unknown-delivery and WindowGone refusals on revoke/remove. Delivery, error, outcome/Unknown and effect remain intact.
2. **Unscoped payload behavior unchanged by inspection.** With no lease the helper performs no mutation; the WindowGone edit is inside the scoped branch. Byte comparisons were not run.
3. **Stop move:** removes foreign Host mutex acquisition under the lease. It widens the Stop-only check/attach interval: Stop during `commit_during_gate` now misses suppression that the parent performed (`action/failure.rs:144`, `scope/commit.rs:44`). This leaves scope authority current; revoke/rebind still atomically rejects the producing epoch. No new scope-authority leak or input delivery follows from this move.
4. **Tests are sensitive by inspection:** the new element/Unknown and strengthened WindowGone canaries would fail on the parent's returned target and retained ledger target. Both revoke and remove are covered. Holds, gate waits, polling and new joins have 2s/15s/8s/15s bounds, with gate release on unwind. There is no whole-test timeout.
5. **No new category-(a) leak found; no must-fix.** No new content/name/label/pixel/file/cache publication after scope-authority loss was identified.

`git diff --check HEAD^ HEAD` passed. Source/tests unchanged; only this uncommitted report was written.

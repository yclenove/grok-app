**MERGE-AFTER-FIXES**

Narrow round 2 at `9e610e33`, reviewing the nine repair commits after `8b479c35`, against round 1 including its 修复记录 and mutation table. Paths are relative to `computer-use/crates/`. Static assessment; failing schedules and Rust-test mutation sensitivity were not executed.

**Must-fix, category (a)**

1. **[P1; SAME CLASS as round-1 item 3; NEW race in the repair] Retirement can permanently miss invalidation.** `gcu-mcp/src/retire.rs:37`, `:38`, `:49`, `:55`.

   Principle: invalidation must retire already-delivered artifacts without another get/export/close.

   Concrete schedule: hold a scoped Connection open with delivered file p. The file watcher's `retire()` checks its row while generation 1 is valid, so keeps p. Another thread revokes immediately after that sweep. `lease.current()` now returns `None`; the watcher breaks without a final sweep. Its flag stays set, so later binds cannot restart it. With no export/close, p remains readable indefinitely. Expiry between the sweep and terminal check has the same outcome; the image watcher has the same gap for resident pixels. This is subsequent access to a retained server artifact, not a read admitted before revoke.

   There is also an expiry missed-wakeup: `remaining()` checks `expire()` before sampling the remainder (`gcu-core/src/scope/deadline.rs:60`). If the clock crosses the deadline between those reads, it returns zero without publishing expiry. The guard at `retire.rs:49` disables the timer, or `sleep_remaining` at `:57` waits forever. If both store watchers take this interleaving, neither supplies the read needed to publish expiry; both await an unchanged epoch.

   The explicitly requested sync case is also unowned (`retire.rs:19`): bind without a current runtime, synchronously commit a stamped file, revoke, and retain the store/path without another bind/export/close. Nothing schedules retirement. Leaving the flag unset permits a later bind; it does not arrange one. Public Connection setup and normal result commits run under Tokio, so this particular schedule concerns the synchronous store surface.

   Require a terminal sweep **after** terminal observation, zero-remainder handling that immediately rechecks/retires, and an owner or fail-closed contract for sync publication. Preserve unlink outside locks and the existing atomic registration. Add deterministic barriers around sweep → terminal check and expiry sampling, plus a no-runtime case; assert p is gone/pixels absent before get/export. Current tests do not force these schedules.

**(b) Repair checks and preserved boundaries**

| Round-1 principle | Round-2 assessment |
| --- | --- |
| 1. Observed code identity | Fixed by inspection. `scope_link.rs:196`, `:213`, `:228`, `:271`, `:316`, `:360` route admission, both peers' rechecks, encoder and supervisor ACK validation through facts. Missing/unusable observations refuse; identifier/team/cdhash are compared without claim/path fallback. ACK code comes from an observed helper read. Pending-write and post-flush peer checks catch persistent code changes after hello or during ACK. Unchanged-wire tests change all three fields or drop observation for both peers; replacing the shared observation check with claimed code would fail them. |
| 2. Flushed ACK before data | Fixed by inspection for INITIAL ACK and REBIND. `scope/mod.rs:269`, `:375`, `:406`; `scope/data.rs:105`. Although apply publishes before suspend, the admitted generation still names the old ACK: fresh admission checks generation before and after arming, and old connections retain their old authority. No new-generation serving window found. Rebind suspends before awaiting; open follows flush + peer/deadline recheck. The pinned write retains 50 ms peer and deadline monitoring. Errors converge on revoke/release; owner Drop still revokes/closes synchronously and schedules release. No new cleanup bypass found. |
| 3. Artifact lifetime/commit | Partial: finding above remains. Publish-before-subscribe is handled by the first sweep. New expiry/rebind/revoke tests inspect path existence or non-reclaiming `resident()` **before** post-invalidation get/export (`lease_cache_tests.rs:126`, `:160`, `:190`; `result_round6_tests.rs:401`, `:550`). Unlink follows dropping ledger/lease locks. Snapshot → image cache → ledger → lease order and the single stamp/register closure remain intact; `scope/commit.rs`, `result_commit.rs`, `lease_view.rs` are unchanged. |
| 4. Windows/Linux cfg | Round-1 import defect fixed: `AsyncReadExt` has the same Unix cfg as its reader test. `run_controlled` and the `admit_data` re-export are test-only; the unused retirement Duration import is gone. No further unused-import/dead-code regression found in repair lines. Compilation/clippy NOT_RUN. |
| 5. Store isolation guard | Replaces the production self-counting check; StoreRole/StoreLedger/impl are cfg(test)-only. The guard at `scope_link_tests.rs:874` scans the four documented files and 18 forbidden strings. Forbidden config/Host/Stop references using those spellings fail, including comments. Renaming/removing a listed file without updating the list fails its read/expect. It excludes `scope/data.rs` as recorded, and does not cover every reachable/new module, aliases or alternative spellings; it is a textual guard, not a call-graph proof. |

No new `Command::new` reference was added anywhere in the repair diff; production eval construction remains in `gcu-eval/src/boundary.rs`. Stop constants remain **100 ms / 500 ms / 5 s / 4750 ms**. Ordinary helper auth/client/frame/serve/macos files are byte-identical to the pre-repair merge; no ordinary/unscoped behavior change found in shared-store changes. Accepted S1/S3 (b) residuals and A1–A4 are not reopened.

**Nice-to-have**

- **SAME CLASS as round-1 mutation-coverage gaps:** deleting suspend or opening before flush fails the rebind barriers; bypassing the fresh-accept call fails the source assertion. However, deleting only `admit_data`'s generation comparisons can survive because those barriers observe an already-suspended plane. Add a direct admitted-generation-1/current-generation-2 case. Likewise, copying expected code into the ACK body while retaining all surrounding live checks can survive existing tests; they prove refusal on bad facts more strongly than the body's provenance. Neither is a demonstrated implementation defect.

Validation: repair diff whitespace check passed; read-only guard inspection plus 72 in-memory forbidden-string additions detected every listed mutation. No cargo, Rust tests, network/SSH/model calls, personal agent-home access, or source/test edits. Real helper launch/TCC/FD inheritance/live SecCode reads/real-model runs remain **NOT_RUN by design**, not findings. Only this report was written, uncommitted.

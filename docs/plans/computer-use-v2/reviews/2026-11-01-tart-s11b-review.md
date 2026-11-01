Verdict: **MERGE-AFTER-FIXES**

Independent static review of `cu2/tart-s11b`, commit `82fd0aadaf4d275a185c83a3b9f3fdb2f62bc018` on `e7994845`. Findings 1–5 block acceptance of the offline state-machine contract. This review accepts the synchronous, caller-driven, fake-only scope. It does not require real Tart, H1, H2, admission, a transport, or a lifecycle service, and does not reopen S1–S12/S11a decisions.

Read ADR 0018, especially §§1–3, 8, 11–12; all three S11a reviews; the S11a interfaces; all new machine, budget, lock, parser, receipt, and test files; and the ledger/deadline/documentation diff. References below are relative to `computer-use/crates/gcu-eval/` unless explicitly repository-relative. Source/Git inspection and a finite, read-only Python count of test registrations only. Rust compilation, tests, executable mutations, network, and real VM operations: **NOT_RUN**. Only this report was written; no commit was made.

1. **P1 / must-fix — A reserved name and matching run string do not establish VM ownership or preserve the admitted incarnation.** `src/disposable/tart/ledger.rs:84`, `src/disposable/tart/machine/steps.rs:97`, `src/disposable/tart/machine/steps.rs:141`, `src/disposable/tart/machine/cleanup.rs:354`, `src/disposable/tart/machine/recover.rs:86`.

   **Principle:** Registration precedes allocation, but registration must not claim an existing or replacement VM. Destruction needs the exact owned VM identity/configuration; a guest reboot also invalidates the old session and score (ADR 0018 §2).

   **Failure scenarios:** Reserve `clone-1`/`run-1` and drop before clone. Recovery treats that row as sufficient to reclaim any listed `clone-1` whose status says `run=run-1`. The row has no VM incarnation/configuration measurement, and reserve/clone never check whether the destination already exists. A foreign or replacement VM carrying the same run tag is indistinguishable from the owned clone. `CloneIssued` is not consulted, and even a successful clone body is discarded. Separately, after the fake run used `boot1`, cleanup can return `boot=boot2` with the same run tag in both statuses: the boot value is parsed but never compared, and the attempt can seal **Scored(Pass)** despite the reboot. A different guest boot need not mean a different owned VM, but it must invalidate the session's score.

   The existing different-run test correctly refuses that particular conflict; golden names remain unrepresentable as `CloneId`, and there is no prefix deletion. Those protections do not cover same-name/same-run replacement or a stale session.

   **Minimal fix:** Model one expected fake VM incarnation/configuration in the reservation/allocation record, reject an existing destination, and require matching authoritative identity before start and destructive actions, including recovery after a lost reply. Missing or ambiguous ownership must quarantine. Keep guest boot/session validity separate: a reboot invalidates scoring while cleanup may still destroy the positively identified owned VM. Do this with fake data; no real identity reader or transport selection is needed. Add collision, same-run replacement, and boot-change-during-cleanup cases.

2. **P1 / must-fix — An absent status can falsely certify that Tart and Softnet exited.** `src/disposable/tart/parse.rs:110`, `src/disposable/tart/parse.rs:124`, `src/disposable/tart/machine/cleanup.rs:361`, `src/disposable/tart/machine/cleanup.rs:367`.

   **Principle:** Absence comes only from a complete successful authoritative list, and VM absence does not override explicit evidence that the associated processes remain live. Final requires confirmed cleanup.

   **Failure scenario:** From a normal `Revoking` attempt, return a complete list containing the clone, an owned running status, successful stop, then the successful body `status v1 state=absent tart=up softnet=up`. The parser accepts it, `stopped_clean` returns true without inspecting either process flag, and the identity check is bypassed for `Absent`. Successful delete and a final complete list omitting the clone then allow `destroy` and `seal` to retain pass, although the status explicitly reported both processes live. Even a fieldless `status v1 state=absent` supplies the stop proof today. This is a malformed/insufficient-success-body case, distinct from injecting `CallError::Malformed`.

   There is also unnecessary destructive work after the *initial* list already proves absence: stop and delete still address the bare name without a positive identity check. If the name appears again, the earlier absence is not ownership of that new resource.

   **Minimal fix:** Delete the unconditional `VmState::Absent => true` cleanup proof. Require explicit, consistent exit evidence; derive resource absence from the complete list only. Once authoritative absence is established, avoid further name-targeted stop/delete; establish any remaining process cleanup separately. Reject contradictory status fields. Add the body above and a missing-process-proof case, and update the absent-resource fixture, which currently requires these unnecessary commands (`tests/tart_offline/s11b/recovery.rs:196`).

3. **P1 / must-fix — Illegal work requests during cleanup can still finish as pass.** `src/disposable/tart/machine/mod.rs:229`, `src/disposable/tart/machine/mod.rs:236`, `src/disposable/tart/machine/mod.rs:420`.

   **Principle:** An unexpected nonterminal event must preserve cleanup and permanently force `not_run`; returning an error alone is insufficient (ADR 0018 §§2, 12).

   **Failure scenario:** Drive a successful fake run to `Running`, revoke successfully, then call `run` again in `Revoking`. `ensure_phase` returns `Unexpected` through its cleanup-specific early return, leaving `trigger=None`. Continue with successful destroy and seal: the result is **Scored(Pass)**. An invalid start/run request in `Destroying` has the same problem. No second client is issued, but the required violation precedence is lost.

   **Minimal fix:** Remove the cleanup-specific early return and record `Unexpected` for every illegal nonterminal transition. `fail` already preserves `Revoking`/`Destroying`, so this does not require a new state or restarting cleanup. Keep already sealed terminal receipts immutable. Test invalid work requests in both cleanup phases through final receipt publication, not just the immediate error/count.

4. **P1 / must-fix — Observed lease expiry is optional, so a known expired lease can still retain pass.** `src/disposable/tart/machine/steps.rs:378`, `src/disposable/tart/machine/cleanup.rs:47`, `src/disposable/tart/machine/cleanup.rs:250`; test at `tests/tart_offline/s11b/recovery.rs:626`.

   **Principle:** Once the owner lease expires, the old attempt cannot resume or score; it must enter the owner-death hold with cleanup unknown. Caller-driven operation does not require a background timer, but subsequent calls must enforce the expired lease.

   **Failure scenario:** Run successfully, arm a three-second lease, advance the fake clock to its exclusive deadline, and call `arm_owner_lease` again. It returns `LeaseExpired` but leaves `Running`, the provisional pass, and `trigger=None`. Instead of calling `observe_owner_lease`, call successful revoke, destroy, and seal. None checks `lease_until`, so the result is **Scored(Pass)** with confirmed cleanup. Calling `observe_owner_lease` with an already-expired query deadline likewise leaves the attempt running. The late-lease test explicitly expects `Running` and stops before checking the final outcome.

   **Minimal fix:** Make expired-lease handling an irreversible synchronous state decision shared by the late renewal and subsequent transition/publication gates. Enter `Suspended`, deny deletion by that abandoned owner, and require startup recovery regardless of whether the bounded revoke/note succeeds. Keep all work caller-driven; add no timer, observer, or automatic resume. Test late renewal followed by cleanup/publication, omitted observation, expired observation deadlines, and all calls after suspension.

5. **P2 / must-fix — Aggregate budgets and confirmation budgets are checked but not propagated into dispatched deadlines.** `src/disposable/tart/machine/mod.rs:258`, `src/disposable/tart/machine/mod.rs:269`, `src/disposable/tart/machine/mod.rs:278`, `src/disposable/tart/machine/cleanup.rs:367`, `src/disposable/tart/machine/cleanup.rs:383`.

   **Principle:** Every dispatched operation must be clipped to all enclosing absolute/group/phase deadlines, not merely start before them.

   **Failure scenarios:** Shrink prep to one second, reserve at zero, advance to 500ms, then request a 60-second clone. `admit` checks `now < prep_end` but forwards a deadline at 60.5 seconds rather than one second; the fake trace exposes this directly without a slow adapter. The cleanup-total check has the same omission: starting just before its end can forward deadlines beyond the 60-second total. Finally, post-stop status uses the independent query deadline rather than `min(query, stop_end)`, and post-delete confirmation similarly ignores `delete_end`. A short stop/delete deadline can therefore be followed by a confirmation permitted to finish after that phase expired. The aggregate and confirmation defects are masked by instantaneous, non-advancing replies in the tests.

   **Minimal fix:** Include prep/cleanup end instants in the effective minimum and carry that minimum through every inner call. Clip stop and delete confirmations to their respective phase end instants. Preserve the separate recovery rule: use the current caller's deadlines, never the dead row's absolute deadline. Add trace assertions near aggregate expiry and with shorter confirmation-phase deadlines; use a finite fake that advances the clock between calls to cover crossing a boundary.

6. **P2 / should-fix — Cleanup error handlers overwrite a terminal quarantine state.** `src/disposable/tart/machine/mod.rs:367`, `src/disposable/tart/machine/cleanup.rs:60`, `src/disposable/tart/machine/cleanup.rs:98`.

   **Principle:** Terminal state and stored receipt must agree; a failed step must not reopen `Quarantined`.

   **Failure scenario:** Hold the fake lock after reaching `Running` or `Revoking`. `on_lock` stores a quarantine receipt and sets `Quarantined`, but the enclosing revoke error branch then assigns `Revoking`, or destroy assigns `Destroying`. The attempt advertises a nonterminal phase alongside a terminal receipt. The remaining `halted` flag prevents a score today, but terminal behavior now depends on that duplicate flag.

   **Minimal fix:** Preserve the terminal phase returned by the shared error handler; delete the unconditional phase writes on those paths. Test lock contention after allocation, not only before reserve.

7. **P2 / should-fix — A failed seal advertises that recovery is unnecessary.** `src/disposable/tart/machine/mod.rs:446`, `src/disposable/tart/machine/cleanup.rs:208`, `tests/tart_offline/s11b/recovery.rs:312`.

   **Principle:** Receipt recovery status must reflect the ledger's unsealed row.

   **Failure scenario:** Finish VM cleanup, inject `LedgerFault::ReplyLost` into seal, and inspect the stored quarantine receipt. `make_receipt` always passes `false` for `recovery_required`, although the row is unsealed and the next reserve returns `MustRecover`. The test named `seal_fault_drops_the_score_and_requires_recovery` checks the ledger gate, but never this receipt field.

   **Minimal fix:** Derive the flag from known row/seal state, setting it for an allocated unsealed row. Assert agreement between the receipt and the next reserve/recovery behavior. This does not require the later S11d receipt format.

8. **P2 / should-fix — The tests cover boundary faults, but not every interruption prefix or the claimed invariants.** `tests/tart_offline/s11b/edges.rs:159`, `tests/tart_offline/s11b/edges.rs:222`, `tests/tart_offline/s11b/faults.rs:192`, `tests/tart_offline/s11b/recovery.rs:83`.

   **Principle:** A named invariant needs a negative case that would fail if its enforcement disappeared; a scripted list containing foreign names is not a model of foreign resource ownership.

   The edge table runs 11 boundaries × 6 injections (66 scenarios). This is useful, but `Destroy` injects into the first list only, `Isolate` into inventory only, and malformed faults are generally preclassified errors rather than garbled successful bodies. Cancel occurs before the selected operation. Host-crash/Drop recovery is tested only after `Running`, not after each prefix. There are no crash windows between clone dispatch and its ledger append, between stop and confirmation, between delete and confirmation, or between quarantine append and seal. The fake ledger's lost-reply contract always leaves the row unchanged, so these tests do not model a committed ledger operation whose acknowledgement was lost.

   Each §2 fault-table row has a nominal representative: provider crash; guest crash/reboot and Tart/Softnet exit; owner lease death; host crash. Their full prefix behavior is **not** covered. In particular, runtime-loss notifications are usually tested after a provisional pass, whereas owner death requires the explicit successful observation path. The reported coverage should distinguish those facts. The host-crash check inspects `stopped` before dropping the attempt (`recovery.rs:90`), not immediately after Drop, so that assertion by itself does not protect inert Drop.

   **Five concrete surviving mutations** below are static predictions, not executed mutations:

   | Mutation | Why the current suite would miss it | Needed negative case |
   | --- | --- | --- |
   | Remove the post-stop run comparison at `machine/cleanup.rs:371`. | The identity-conflict fixture changes the first status only; post-stop statuses all match or say absent. | Change identity after stop, require no delete and quarantine. This complements finding 1's missing incarnation model. |
   | Disable only the final `end` marker check at `parse.rs:62`. | Valid list fixtures include it; the only malformed-success list is `nope`, which fails earlier. | A success body with valid header/count and a missing/replaced final marker, at initial and final inventory. |
   | Add a re-arm branch in `arm_owner_lease` that resets `Suspended` to `Running`, clears `delete_forbidden`, and removes `lease_until`. | No test calls renewal or work after observing owner death; the late-renewal test is before suspension. | Exercise every entry after expiry/suspension and require no revival or new lease. |
   | Skip `fail(Unexpected)` in `ensure_phase` when the current phase is `Running`. | The repeated-run test checks only error and run count, then drops; it never seals the result. | Finish cleanup after a rejected second run and require `not_run/Unexpected`. Finding 3 is the existing analogous hole in cleanup phases. |
   | In reclaim, replace `deadlines.stop.at()` with a fresh `clock.now() + budgets.stop` using checked arithmetic. | Recovery tests pass a fresh full-cap stop deadline and do not advance time inside recovery. The expired-old-attempt test still passes. | Give recovery a shorter/already-expired caller stop deadline; assert no stop after it and no extension in the trace. |

   **Minimal fix:** Extend the existing tables with these negative bodies/events and the actual prefix windows. Assert complete outcomes, exact targets/identity, and effective deadlines. Use a small stateful fake inventory or narrowly scripted identity changes; do not create another supervisor or real adapter.

9. **nit — The success count omits all S11b checks.** `tests/tart_offline.rs:152`, `tests/tart_offline.rs:156`, `tests/tart_offline/s11b/mod.rs:8`.

   **Principle:** The harness summary should describe the checks actually registered.

   **Scenario:** The printed `tart_offline ok: 23` counts only the first table. Static registration inspection found **35** S11b named checks (one edge matrix plus 34 fault/recovery checks), for **58** named checks total. All zero-argument S11b checks are registered; recovery functions are referenced through `faults::TESTS`. `s11b::run` feeds the same failure vector before the final panic, so their failures are not discarded. A panic also fails the child. This is a reporting defect, not evidence that S11b is skipped.

   **Minimal fix:** Include S11b's registration count, or print separate counts, without confusing the 66 edge scenarios with 66 separately registered checks.

**Bounds, capability boundary, recovery, and watchdog assessment**

The ADR constants are centralized in `budgets.rs`; the diff does not touch native Stop constants. Per-call deadlines are explicit, zero/above-cap durations are rejected, and arithmetic uses checked conversions/addition. In-attempt absolute clipping is present; aggregate propagation still needs finding 5. No input-reachable `unwrap`, `expect`, unchecked counter overflow, or indexing panic was found in the Tart implementation. Parser indexing is guarded by lengths and ASCII delimiters; the list allocation is reached only after count/line agreement. List duplicate detection is quadratic within the 1MiB body cap, so this inspection does not establish a wall-clock processing bound for adversarial maximum-size lists.

There is no unbounded production polling/wait loop. Probe iteration is over the fixed 16 probes; recovery iterates the finite returned rows. `FakeCleanupLock` uses one `try_lock`, returns immediately on contention, and drops the guard before returning. Thus no runner/guest/ledger IO occurs under that guard. It is an availability probe, however: no actual state decision is protected while held (`lock.rs:78`). With the current exclusive borrows and synchronous fakes, that is not a concurrent lifecycle manager or a demonstrated concurrency bug; do not describe it as interprocess ownership serialization.

Recovery correctly appends `RecoveryNoted` before reclaim, skips sealed rows, passes `None` for the dead absolute deadline, and uses the current caller deadlines. It never starts a VM or restores an old score. `reserve` refuses unsealed rows and separately checks sealed quarantine facts. The expired-row recovery test would catch restoring the old absolute cap; it does not protect the caller's shorter/expired deadline, as the mutation table explains. Failed/malformed/timed-out/lost list calls currently fail closed. The complete-list parser requires UTF-8, the header, count agreement, a final marker/newline, and no duplicate names. Final resource absence is checked after delete. These are useful protections, subject to findings 1–2 about identity and process-exit proof. Repeated absent stop/delete responses are tested, but repeating the full destroy call simply refuses; the suite does not exercise crash recovery after each destructive prefix.

`boundary.rs` is byte-for-byte unchanged against the base; `accepting_provider_linked` and `scoped_provider_ack` remain closed (`src/boundary.rs:108`, `:116`). No process builder, admission mint, accepting provider implementation, async task, thread, observer, environment switch, filesystem adapter, or custom Drop was found in the new Tart code. `Attempt` and `ClonePermit` have private external fields and no Clone/serde; the permit's negative Clone assertion remains. `Phase` is a copyable label, and receipts/evidence are data, not stage capabilities. There are no new admission stage types to misuse. Bootstrapped/CredentialReady/Ready consume fake evidence only; the login/run calls cannot produce real authority. The inherited lexical source guards remain spelling checks, not a proof over arbitrary expanded Rust aliases. Public fake injection is not an admission bypass.

Every S11b check runs via `run_checks` → `s11b::run`, selected in `watch::entry`'s **CHECKS child** (`tests/tart_offline/watch.rs:51`). The forgotten-lock regression therefore executes inside the existing parent watchdog, not in the parent. The wrapper re-execs the test artifact, checks a deliberately hanging child and exit 7, and uses bounded polling/cleanup; the main check child's nonzero status is propagated. Its existing 30-second check window has a separate five-second reap allowance, plus the self-probes; it is not a 30-second bound for the entire invocation. The wrapper itself was not changed here or run in this review. No new unguarded hang fixture or hidden worker was introduced.

**Deletion and size assessment**

The commit adds 4,881 lines; the new budget/lock/parser/receipt/machine files account for 2,419 lines. There is no hidden lifecycle manager in the current implementation. The risk is duplicated state and error-path bookkeeping, illustrated by finding 6, rather than a concealed observer. Keep Suspended, quarantine, and the ADR evidence phases; they are required behavior, not speculative states. Prefer these deletions to another controller or abstraction:

- Delete `IntoRecoveryError` and its two implementations (`machine/recover.rs:184`); pass an already-constructed `StepError` to the failure helper. The `still_open` parameter is always true and can also disappear.
- Delete the success-path `clear_for_reserve` reset of the whole attempt (`machine/recover.rs:161`). Recovery is admitted only from Idle and its successful work uses local row data; it has not populated a current attempt. Preserve any existing halt instead of mechanically resetting two dozen fields.
- Delete the parallel boot/UID/desktop/config options in `Evidence` (`receipt.rs:248`) in favor of the one optional `GuestView` already produced together by the parser. This removes duplicated identity representation and partial combinations.
- Delete the unused public `PhaseBudgets::get` dispatcher (`budgets.rs:193`), and avoid expanding the all-purpose budget API to fix deadline propagation.
- Delete the unconditional terminal-overwriting assignments and the absent-status shortcut identified above. Do not add a second receipt/status/lifecycle representation to compensate for them.

**Documentation assessment**

Repository-relative `computer-use/docs/adr/0017-real-eval-fixture-scope.md:173` accurately reports a fake-only synchronous S11b implementation and leaves H1/H2/real execution unresolved. ADR 0018 §12 accurately discloses no provider/admission, no background thread, inert Drop, fake evidence stages, and recovery using the new caller deadline. It also explicitly supersedes §11's historical state-machine status. The added text contains no timezone, proxy, personal path, or home-network values.

ADR 0018 §12's unconditional claims at `computer-use/docs/adr/0018-tart-disposable-provider.md:244`, `:247`, and `:249` overstate the present lease hold, ownership protection, and violation-to-not_run behavior. Resolve findings 1–5 before retaining those claims as implemented guarantees; do not describe the partial fault matrix as every-prefix coverage. The receipt is also deliberately much smaller than §7's eventual schema: it does not preserve the original provisional task result and original failure independently of a later quarantine reason. For example, provider crash followed by cleanup failure publishes only `CleanupFailed`. Keep that limitation explicit until the later receipt card supplies the original-cause fields.

Real Tart/Softnet, VM identity and destruction, network enforcement, guest transport, native Stop/TCC, actual parent/host death recovery, login, and model evaluation remain **NOT_RUN by design**. The findings concern whether the offline model enforces its own contract.

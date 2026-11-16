# S11 lease5 independent review — 2026-11-15

Reviewed `cu2/s11-lease5`, commit `17a9534f` against `d8239656`. Evidence paths below are relative to `computer-use/`; line numbers refer to the reviewed commit. Review only; no implementation changes or commit.

**Result: 0 P0, 0 P1, 2 P2 findings.** The requested lease arithmetic and host-side accounting are correct. The adjustable model needs production isolation, and the guest-loss safety contract needs reconciliation with ADR 0018.

1. **[P2] The adjustable test model is available to production code.**

   Locations: `crates/gcu-eval/src/disposable/tart/schedule.rs:120` and `crates/gcu-eval/src/disposable/tart/schedule.rs:149`.

   `CycleModel` and `prove_model` are unconditional `pub(crate)` items. Production code inside `tart`, including the M4b machine, can therefore obtain a `ScheduleProof` using caller-selected grant/age/gap values instead of the measured constants. For example, the attempt window with `grant_ms=2000, age_ms=0, gap_ms=1` returns a proof, although the actual measured cycle cannot fit a 2000 ms grant. Not re-exporting these items from `tart/mod.rs` prevents external-crate access but does not prevent this internal bypass.

   The current public `prove()` correctly supplies `LEASE_WINDOW_MS`, `ACCEPT_AGE_MS`, and `GRANT_GAP_MS` (`schedule.rs:137`). The only current non-test caller of `prove_model` is that wrapper; no active production bypass was found. This is a failure of the requested production-access restriction, not evidence of an already admitted unsafe VM.

   **Minimal fix:** make the configurable evaluator and its input type private to `schedule`; expose adjustable inputs to sibling tests only through a `#[cfg(test)]` facade. Keep production callers on `prove(window_ms)` with fixed measured bounds.

2. **[P2] ADR 0018 still promises the superseded three-second guest-loss bound.**

   Locations: `docs/adr/0018-tart-disposable-provider.md:59`, `docs/adr/0018-tart-disposable-provider.md:305`, and the historical S11a obligation at `docs/adr/0018-tart-disposable-provider.md:233`.

   The active §2 contract still gives the guest supervisor a fixed ≤3 s liveness deadline, and §14 item 4 still specifies termination within ≤3 seconds of lease expiry/loss. Both conflict with the newly recorded `g+5s` grant and ≤6 s whole-tree termination contract. §11 also retains “≤3s guest 存活”; that dated implementation snapshot should identify the later superseding decision. Updating ADR 0019 alone leaves implementers and acceptance reviewers with incompatible safety limits.

   **Minimal fix:** add the dated supersession/reference in ADR 0018 and update its active guest-liveness statements to the sample-based 5000 ms grant plus at most 1000 ms revoke/release/whole-tree termination, with ≤6 s after host loss. Preserve the independent S3 three-second scope lease and ≤1 s supervisor Renew/publication cadence. Do not substitute a fresh six-second lease starting at message receipt. Carry this distinction into the safety and acceptance references listed below.

**Requested checks and evidence**

| Check | Review result |
| --- | --- |
| Lease constant and explanatory text | `mailbox/ops.rs:28` sets `LEASE_WINDOW_MS=5000`; its lifetime comment at `:25`, terminal comment at `:382`, overflow comment in `mailbox/error.rs:185`, and pinned-sample comment in `mailbox_guest.rs:40` use the new constant/contract. These filenames are under `crates/gcu-eval/src/disposable/tart/`. No remaining active hardcoded `sample+2000` or `g+2s` was found in that implementation. |
| Required ADR 0019 edits | `docs/adr/0019-tart-mailbox-channel.md:96` (§2 row), `:181`, `:182`, `:183` (§3), `:341` (constraint d), `:396` (owner decision), `:398` (recalculation), and `:429` (M4b entry) all reflect the decision. |
| Computed feasibility | `crates/gcu-eval/src/disposable/tart/schedule.rs:66` computes age `1023+100+1023=2146`; `:72` computes gap `800+100+1023=1923`. The checks at `:153` and `:154` require freshness and strictly `age+gap<grant`; `:173` gives `5000−4069=931` ms slack. The attempt is 1,560,000 ms, so `:151` and `:152` produce 1951 periodic requests plus 8 extras = 1959 ≤2048, leaving 89 requests. Public `prove()` uses the shared lease constant. Configurable-helper isolation fails as described in finding 1. |
| Genuine negative cases | `schedule.rs:265` tests a 2000 ms grant against the measured bounds. `:283` accepts +930 ms gap jitter with 1 ms slack and rejects +931 ms, exactly consuming the slack. `:317` accepts 2048 requests and rejects 2049. `mailbox/tests/guest_sim.rs:741` also checks historical rejection and current acceptance. These files are under `crates/gcu-eval/src/disposable/tart/`. The negative cases call the evaluator and require `Infeasible`; they are not unconditional expected failures. |
| Host-side deadline and overflow | `crates/gcu-eval/src/disposable/tart/mailbox/seq.rs:363` uses `sample_ms.checked_add(LEASE_WINDOW_MS)` before mutating request state. `:120` stores that end; ACK processing does not extend it. `mailbox_guest.rs:102` delegates its reported end to that tracker; `:252` preserves the original grant reference on repeated timestamps and rejects decreasing samples. Existing tests at `mailbox/tests/seq.rs:229` cover overflow without state advancement and stale/current ACKs without deadline extension; `mailbox/tests/guest_sim.rs:967` covers repeated-sample pinning. |
| Decision versus history | ADR 0019 `:396` records the owner decision and unchanged constraints factually; `:398` explicitly preserves the old infeasibility as history. The old `g+2000`, failed 2000 ms calculations, and ≤3 s statement at `:391`–`:394` are historical, not missed replacements. The dated C11 observations at `:406`–`:409` remain valid: in-place writes did not solve latency under the old constants. This is mathematical feasibility, not completed M7/M9 load or termination validation. |

**Guest-loss safety references to reconcile or carry forward**

The explicit stale three-second promises are finding 2. The additional references below either inherit that promise or define the downstream obligations; they must use the six-second fallback when evaluated or implemented. ADR 0019's unnumbered references already inherit its updated §3, so they are not additional independent findings.

| Reference | Required treatment |
| --- | --- |
| ADR 0018 `:59`, `:305`; historical `:233` | Replace/supersede only guest-liveness ≤3 s with fixed `g+5s` and ≤6 s termination. Preserve S3 scope 3 s and Renew ≤1 s in the same paragraph. |
| ADR 0018 `:68`; ADR 0019 `:188` | Both owners dead: explicitly associate autonomous tree termination with ≤6 s after host loss. VM deletion remains unproved; cleanup stays unknown and restart requires recovery. |
| ADR 0018 `:52`, `:57`, `:71`; ADR 0019 `:50`, `:95`, `:164` | Cleanup/drain must account for the six-second guest-loss fallback within existing aggregate budgets. Elapsed time, lease ACK, or revoke acceptance must never replace actual whole-tree/VM/process-exit and resource-absence proof. Keep the 10/20/20/10 s cleanup allocations and 60 s total. |
| ADR 0018 `:61`, `:62`; ADR 0019 `:209`, `:217` | Distinguish guest autonomous fallback from Host Stop. Supervisor loss/expiry handling uses the five-second grant plus final one-second cleanup; Host Stop retains its original constants, persistent latch, and user-only resume. |
| ADR 0019 `:249`, `:250`, `:308`, `:411` | Deferred transport/liveness, parent-loss and full-volume validation remains unproved. Its termination criterion is now ≤6 s, including the final ≤1 s revoke/release/kill. Mechanical visibility evidence supplies no termination proof. |
| ADR 0019 `:432`, `:434`, `:443` | M7/M9 must measure the revised bound under block-all and load, including autonomous expiry, late grants/no revival, parent loss, full disk, early detach, and escaped descendants. Retain the grant-age/gap gate, model mailbox isolation, Stop preservation, and independent cleanup proofs. |

**Values intentionally unchanged**

Byte comparisons against `d8239656` confirm unchanged Host Stop code: `crates/gcu-core/src/session/stop.rs:30` (100 ms), `:35` (500 ms), `:39` (5 s), and `crates/gcu-core/src/driver.rs:28` (4750 ms). The S3 lease remains 3 s at `crates/gcu-host-protocol/src/scope/mod.rs:17`; its renewal interval remains `LEASE/3` at `crates/gcu-eval/src/scope/mod.rs:222`. ADR 0018's scope statements at `:32` and the beginning of `:59` must remain unchanged even while its separate guest-liveness text is fixed.

C1's per-visibility wait remains ≤3 s in ADR 0019 `:258` and `tools/s11-mailbox-probe/src/run.rs:36`; its separate RTT gate remains unchanged. The 3 s `LEASE_BUDGET`/`LeaseDeadline` at `crates/gcu-eval/src/disposable/tart/budgets.rs:26` and `deadline.rs:166` is an existing call/deadline cap, not `sample_ms` grant validity; mechanically replacing it with five or six seconds would mix distinct limits. Owner proofs ≤1 s, the 800 ms publication grid, fail-closed expiry, and prohibition on revival are retained in ADR 0019 `:185`, `:187`, `:198` and the reviewed code.

**Validation:** inspected all six changed files, relevant callers/test boundaries, both ADRs, and old-window/termination references across `computer-use/`. An in-memory arithmetic check using the source constants confirmed the current, historical, jitter, 2048-request and 2049-request results. Rust/Cargo tests and native/VM checks were **not run**, preserving the report-only write constraint; test coverage above is source inspection, not a claimed test run.

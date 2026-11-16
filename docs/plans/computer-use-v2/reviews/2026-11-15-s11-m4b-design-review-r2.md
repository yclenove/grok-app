# S11-M4b independent design review, round 2 — 2026-11-15

**Verdict: request changes. One P1 finding; no P0 or additional P2 findings.** D1, D3, D4 and D5 are resolved at the design root. The controller's decision to remove the current-lease-ACK work gate is consistent with ADR 0019. However, D2's feasibility obligation remains open: the serialized inventory schedule and the independently timed lease publications can stop the guest inside the admitted timing bounds, while the new host pin gate remains open.

Reviewed branch `cu2/s11-m4b-design`, HEAD `2aeb7a452b434ce5aee146652caebcbc2c2c2758`, including the revision against `29c03f63`, the round-1 report, ADR 0019 §3/§4/§6 and its owner decision, and relevant current adapter/scheduler sources. This is a design review, not an implementation review. `D:n` refers to the current `docs/plans/computer-use-v2/s11-m4b-design.md`; `ADR:n` refers to `computer-use/docs/adr/0019-tart-mailbox-channel.md`. Other source paths below are relative to `computer-use/`.

Only this report was written. No commit, build, Cargo test, VM, Tart, mount, network operation or real-channel experiment was performed. Timing arithmetic was checked in memory; the counterexample below is a derived trace, not an executed machine/adapter regression.

**Round-1 disposition**

| Finding | Root disposition |
| --- | --- |
| D1 — publication anchor | **Resolved in design.** D:120–124 separates immutable, request-bound pre-visibility `h_pre` from completed-publication `h_done`; only the former anchors expiry. Polls, equal samples and ACKs cannot replace it, and uncertain publication remains unknown. D:178 requires the rename-before-delayed-fsync counterexample through the actual adapter, exact counting and closure at 5000 rather than 5080 ms. |
| D2 — ACK/schedule composition | **ACK decision accepted; feasibility still unresolved.** D:149–153 removes current-ACK coverage without changing guest expiry or owner/Stop authority. The new machine inequality and success regressions do not repair the joint inventory/lease timing defect described below. Restoring an ACK gate is not the requested fix. |
| D3 — cancellation budget | **Resolved in design.** D:56–65 removes the universal 1 s/half-budget rule, reserves termination within each existing operation budget and freezes cancellation against the applicable original ends. D:172 explicitly permits an early 1.2 s reclamation and checks operation/aggregate exhaustion without adding budget. |
| D4 — public completion type | **Resolved in design.** D:12–15 defines `StepProgress<CompletedStep>`, the closed completion variants, move-once receipt delivery, recovery retention and a side-effect-free `NoActiveStep`. D:173 tests completion solely through public poll, including the unique permit boundary. |
| D5 — first-PR prerequisite | **Resolved in design.** D:44–45 and D:197 move normalized classification, begin-once progress/effect records, saved ends and real dispatch-cost fakes into M4b-1. D:174 requires all three guest operations to reach the failure paths through public setup on that commit alone. M4b-2 retains semantic acceptance and actual adapter composition, with no second grammar. |

**D2-r2 — P1: the independent lease cadence can miss guest expiry despite continuous host pins**

Evidence: D:140–142 admits allocation using the existing guest `2146+1923<5000` proof plus the new `2246<5000` inventory-publication bound. D:144 allows only one outstanding inventory; D:145 independently applies `due(last_publication_done, now)` to inventory and lease. D:146 permits further leases referencing the same pin. D:147 correctly forbids those repeated samples from extending the guest end. D:185–186 now requires the admitted slow paths to sustain work.

The reused 2146 ms guest age assumes that the host publishes a grant within one service step of receiving a fresh sample. Independent lease cadence does not provide that guarantee: a new pin can wait for the next lease due time, up to another 800 ms plus the permitted service slip. Meanwhile, a serial inventory exchange prevents a new sample every 800 ms. Repeated leases carrying the old pin do not bridge this freshness gap. The actual adapter follows that rule: `crates/gcu-eval/src/disposable/tart/mailbox_guest.rs:537–543` publishes the current pinned reference, and repeated timestamps retain their original reference at `:252–261`.

One admitted counterexample uses constant 1023 ms visibility in both directions, service starts every 100 ms, and one outstanding inventory. In a long-login segment, inventory requests begin at `0, 2200, 4400, ...` ms. Each becomes visible at `h_pre+1+1023`, its reply becomes visible another 1023 ms later, and the completing host poll accepts it at `h_pre+2101`. The next inventory starts on the following service step. Thus every publication gap is 2200 ms, below the proposed 2246 ms bound, and every replacement pin arrives before the old host end.

The first lease begins at 2101 ms and completes at 2103 ms. An 800 ms due time sampled on the service grid permits subsequent lease starts at `3000, 3900, 4800, ...` ms: a small positive publication cost makes each due instant fall just after a service start. This is the stated cadence with less than 100 ms slip, not transport loss or out-of-model jitter. Lease visibility is likewise publication start plus `1+1023` ms. The first lease carrying each fresh pin is then:

| Inventory `h_pre` | Guest sample `g` | Host accepts pin | First lease publication using this pin | Guest sees that lease | Previous guest end |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 0 | 1024 | 2101 | 2101 | 3125 | — |
| 2200 | 3224 | 4301 | 4800 | 5824 | 6024 |
| 4400 | 5424 | 6501 | 6600 | 7624 | 8224 |
| 6600 | 7624 | 8701 | 9300 | 10324 | 10424 |
| 8800 | 9824 | 10901 | 11100 | 12124 | 12624 |
| 11000 | 12024 | 13101 | 13800 | **14824** | **14824** |

At 14824 ms the old guest deadline wins: the replacement must arrive strictly before it. The guest irreversibly stops and cannot accept the new grant. Intermediate leases still referencing `g=9824` cannot move that deadline. The host has already accepted the `h_pre=11000` pin and its gate remains open until 16000 ms. Owner proofs can remain valid throughout, all request counts are far below their caps, and ACK integrity can be entirely correct. No current-ACK requirement is involved in this failure.

This contradicts the allocation proof and required full-window success inside the advertised component bounds. It is a P1 feasibility defect under ADR:197–198 and D:185, not evidence that the stopped guest can execute unauthorized work. A favorable phase or zero-cost publication can hide it in the two named constant-delay tests.

**Minimal design change:** keep ACKs as integrity checks, but couple fresh-pin acceptance to a bounded prompt lease publication. For example, a new pin creates a priority grant obligation due within one service step; coalesce it with periodic publication and suppress redundant same-pin grants so the existing request-count bound still holds. Specify that obligation within the shared 100 ms budget, then recompute the joint serial-inventory/fresh-grant bound and count gate before allocation. If a different scheduler is chosen, it must constructively bound the same path. Do not retain the existing 2146 ms age assumption while independently waiting for the next 800 ms lease tick.

**Required regression:** through `Attempt` plus the actual adapter and autonomous guest, reproduce the table with positive publication costs and explicit service phases; then require uninterrupted fresh-grant acceptance after the scheduler change. Sweep inventory/lease phase alignment, including due instants just after a service check, for both 900+100 ms and 1023+1023 ms legs. Assert strict arrival before the previous guest end, exact pinned references, actual publication counts, host replacement timing and the complete 1,560,000 ms witness. Keep a guest-expiry negative control; merely observing continuous host pins or 900 ms lease publications must not satisfy the success assertion.

**Assessment of the controller's ACK decision**

ADR §3 does not require host-side confirmation of guest lease coverage before bootstrap/login/run. ADR:162 requires a lease terminal to match an issued grant and says it supplies no owner/scope/admission/cleanup proof. ADR:173–174 explicitly permits overwrite and disposal of consistent old ACKs, including their first observation. ADR:181–188 assigns fixed expiry and non-revival to the guest and keeps owner and S3 scope authority independent. ADR §4 requires the trusted supervisor to revoke, release and kill the model tree on control loss, parent loss, expiry or full disk, without automatic restart (ADR:217–218). Host Stop and original owner checks remain separate at ADR:187/209.

The absence of an ACK work gate follows from these combined obligations; the sentence listing what an ACK does not prove would not establish that conclusion by itself. Final native ACK, S3/S4/S12 readiness, admission and independent cleanup/exit proofs remain mandatory. D:126–136 and D:152 preserve them; no lease ACK is substituted for any of them.

The new host gate can remain open after the guest has autonomously stopped. Besides the counterexample above, lost lease deliveries with continuing inventory responses can do this without any ACK integrity error. A fresh inventory establishes a sampled record, not an accepted grant or a live model tree. That is safe under the ADR because expiry/revoke is irreversible for the attempt: queued or newly arriving bootstrap/login/run requests cannot restart the stopped tree or restore scope. Requests may fail, time out or become unknown; later grants and valid owner proofs cannot undo the guest stop. Revoke/drain, independent VM/process exit and seal barriers still govern cleanup. No unsafe execution path follows from ACK removal while those supervisor obligations hold.

Carry that distinction into the composed negative tests: drop lease deliveries while allowing fresh inventory responses and host dispatch attempts, advance the guest clock through expiry and the final termination allowance, and assert no post-expiry ordinary effect or revival. Then deliver late leases, ordinary requests and ACKs. A test that makes guest expiry conditional on a host gate failure would conceal the safety boundary. This is the existing ADR supervisor/no-revival obligation under the new policy, not a request for a new ACK gate or wire field.

**Assessment of the machine inequality and revised regressions**

The arithmetic `1023+1023+2×100=2246` and potential window overlap `5000−2246=2754` are correct. They describe successive publication anchors. Publication overlap alone is not proof that the host has received and accepted the next pin: the next reply must return while the old pin still authorizes acceptance. If the complete publication/acceptance schedule bounds each relevant leg by 2246 ms, a conservative two-cycle check is `2×2246=4492<5000`, leaving 508 ms. Thus the fixed constants can support the host gate without an additional ACK return leg; there is no inherent host-side infeasibility caused by dropping confirmation.

D:142 correctly requires actual replacement before the old end, and D:161/185–187 requires recorded pin/end continuity and separates positive feasibility traces from fail-closed negatives. Preserve those checks and make the acceptance-latency bound explicit in the joint proof. The 2754 ms overlap must not be reported as the remaining margin for a returned replacement. The finding above survives even when this host replacement requirement is fully satisfied.

The added regressions otherwise target the revised contracts directly: pre-visibility anchoring, longer early cancellation, closed public completion, first-PR reachability, first-ACK overwrite and stale/malformed ACK integrity. No additional defect was found in those four root fixes or their delivery split. Their implementation and the required composed regressions remain to be reviewed and executed; offline success still cannot authorize M5–M9 real wiring, supervisor or admission claims.

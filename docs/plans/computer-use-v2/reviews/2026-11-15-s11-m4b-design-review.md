# S11-M4b independent design review — 2026-11-15

**Verdict: request changes. Two P1 findings and three P2 findings; no P0 found.** The deferred findings all have proposed mechanisms and named regressions, but the publication timestamp is not a safe lease anchor, and the ACK requirement does not compose with the advertised schedule. Resolve those issues and the three implementation boundaries below before treating this as an implementation-ready design.

Reviewed `cu2/s11-m4b-design`, HEAD `29c03f6376048be0cd22d71e483d28ff94f8ca8e`, specifically `docs/plans/computer-use-v2/s11-m4b-design.md`. Authority: ADR 0019 §3/§6, including the 2026-11-15 owner decision, the M4 code/trace lists and feasibility addendum; the two M4 reviews and M4a round-3 review named in the task; current Tart sources at this HEAD. This is an independent review of the supplied design, not an implementation review of the abandoned M4 branch.

Only this report was written. No commit, Cargo/build command, VM, Tart, mount, network operation, or real-channel experiment was performed. Verification consisted of source/control-flow inspection and in-memory timing arithmetic; counterexamples below are derived traces, not executed Rust regressions. `D:n` denotes line n of the reviewed design. Code/ADR paths below are relative to `computer-use/`; `tart/` abbreviates `crates/gcu-eval/src/disposable/tart/`.

## Findings

### D1 — P1: publication-completion time is too late to establish the conservative host lease end

**Evidence:** D:116–117 makes completed publication the authoritative accounting event and records its time there. D:138 uses that publication time for scheduling, and D:143 uses the inventory's “actual publication time h” for `host_end = h + 5000`, reasoning that sampling cannot precede publication. No separate earlier timestamp is specified.

In the actual publisher, the request becomes accessible at `rename_at` (`tart/mailbox/slots.rs:618`), before two directory fsyncs, directory/object checks and final accounting (`:620–635`). `MailboxGuestChannel::publish_new` regains control only after that work (`tart/mailbox_guest.rs:175–178`). Guest visibility does not wait for the host function to return.

For example, rename at host t=1 ms, guest sampling at t=2, and successful publication completion at t=80 all fit one cooperative step. The guest's fixed expiry corresponds to host t=5002, whereas the proposed completed-publication anchor gives t=5080. Even a timely valid ACK cannot turn that later bound into conservative coverage. A pending replacement ACK can therefore leave ordinary work enabled for 78 ms after the last proved grant actually expires. This does not require a syscall timeout or a shared host/guest epoch.

**Minimal design change:** keep completed-publication counting, but retain two distinct timestamps. Save an immutable lower-bound timestamp before the request can become visible, most simply immediately before beginning that publication; use this earlier timestamp for the pinned sample's host expiry. Record completion separately for successful-publication accounting and cadence. Save the lower bound only for that exact inventory request and never replace it on polling, repeated sample timestamps or ACK arrival. A publication whose completion is uncertain still follows dispatch-unknown quarantine.

**Required regression:** sample after rename but before delayed successful post-rename fsync/validation finishes, then withhold a replacement ACK. Through `Attempt` plus the actual adapter, verify that work and acceptance stop by the earlier conservative end, while the completed-publication count advances exactly once. Existing D:167 issuance cases and generic D:170 expiry cases do not assert this timestamp distinction.

### D2 — P1: current-ACK coverage and lease overwrite make the machine schedule fail inside its accepted timing bounds

**Evidence:** D:134–139 admits the M4a proof, schedules new lease publications at 800 ms, and permits overwriting unacknowledged leases. D:142 discards old ACKs, while D:144 requires an ACK to establish coverage and forbids ordinary guest effects before the first ACK. The current adapter implements exactly that stale-ACK boundary: `SeqTracker::observe_lease` validates a sent grant but returns `DiscardedLease` if it is no longer the latest (`tart/mailbox/seq.rs:287–326`); the adapter converts that to pending (`tart/mailbox_guest.rs:229–230`). The old ACK cannot establish machine coverage through the existing normalized result.

With a constant 900 ms host→guest leg and 100 ms return leg, publish lease 1 at t=0 and lease 2 at t=800. ACK 1 arrives at t=1000 and is already stale. Every following ACK is stale for the same reason. Thus a valid 1 s exchange can prevent the *first* confirmed grant indefinitely, while the guest successfully receives grants. The proposed positive full-window trace can avoid this by selecting shorter delays, but that does not validate the timing model used by the resource-allocation gate.

There is also a separate serialized-inventory/host-horizon failure even if redundant lease overwrites are suppressed. With both legs at 1023 ms, an optimistic pipeline is:

| Event | Host time, ms |
| --- | ---: |
| Inventory 1 published / sample generated | 0 / 1023 |
| Inventory 1 read; inventory 2 published immediately | 2046 |
| Grant 1 published / accepted / ACK read | 2146 / 3169 / 4192 |
| Grant 2 published / accepted / ACK read | 4192 / 5215 / 6238 |
| Conservative host end for sample 1 / actual guest end | 5000 / 6023 |

The guest can remain continuous: acceptance at 5215 precedes its old end 6023. The host cannot confirm the replacement by its old end 5000. Additional service or IO cost only worsens this example. The arithmetic `2146 + 1923 < 5000` proves a guest grant condition; it does not account for serialized inventory, ACK return visibility, stale ACK disposal and this earlier host end together.

D:145 and D:174 explicitly permit the resulting shutdown, so they prevent unsafe revival. They do **not** close the feasibility obligation: ADR 0019:197–198 requires a feasible schedule or a failed gate, and ADR:398/429 says the owner decision makes the selected schedule feasible for M4b entry. The design currently obtains `Ok(ScheduleProof)` before allocation and accepts systematic failure within those same bounds afterward.

**Minimal design change:** add the actual machine confirmation cycle to the existing feasibility calculation and its acceptance criteria: one open inventory, publication cadence, current-versus-stale ACKs, ACK return visibility and the conservative host horizon. Require a constructive successful schedule under the bounds used to admit allocation. Until such a schedule exists, fail that machine gate before reserve/resource effects; do not use the guest-only M4a proof as sufficient machine feasibility. Keep `g+5000`, stale-ACK semantics, v1 and Host Stop unchanged. If the desired schedule cannot satisfy all of those constraints, identify the unresolved design decision explicitly instead of passing it via a short-delay success trace.

**Required regression:** compose the machine, actual adapter and autonomous clock-driven guest for repeated 900+100 ms and 1023+1023 ms cycles. Assert which ACK can establish initial/replacement coverage, actual host-end continuity, and agreement between the pre-allocation gate and the trace outcome. Test overwrite before the first ACK explicitly. Out-of-model jitter/loss should still fail closed; those negative traces must remain distinct from a successful feasibility witness.

### D3 — P2: the universal 1 s cancellation tail adds an unnecessary termination limit

**Evidence:** D:53 introduces `tail = min(1000ms, initial_remaining / 2)`; D:54 makes `cancel_start + tail` a hard cancellation end even for early cancellation. ADR 0019:89–98 defines the operation/cleanup caps including kill/reap, and ADR:205/209 preserves those bounds and Host Stop. It does not specify a universal 1 s command-termination cap or a mandatory half-budget work split. The guest's final 1 s tree-termination allowance is a different boundary.

For example, cancel a clone one second into a 60 s operation, with a live owner and ample attempt/cleanup budget. A cooperative kill/reap needing 1.2 s is forced to `Unresolved` after 1 s even though the original command and existing Stop limits would permit completion. This is an added availability restriction, not a violation that enlarges a safety deadline. The current D:164 positive/negative cases do not distinguish it from required original-deadline exhaustion.

**Minimal design change:** remove the universal cancellation cap and half-budget policy. Specify a work/termination partition for the command using its existing operation budget, freeze the effective termination end at first cancellation, and intersect it with cleanup/attempt and applicable existing Stop ends. Any reserved margin must be inside the owning phase, not an additional phase or a borrowed guest timer. Preserve unknown quarantine when those actual bounds expire.

**Required regression:** an early cancellation whose kill/reap takes longer than 1 s but finishes inside all existing bounds succeeds as process reclamation; a cancellation at the reserved work boundary finishes or becomes unresolved by the original operation end. Cover cancellation during revoke/stop/delete/seal and check their respective 10/20/20/10 s caps and the 60 s aggregate. These maxima need not all be attainable together; the fix must not promise or add extra budget.

### D4 — P2: the unified poll has no defined result type for the existing heterogeneous operations

**Evidence:** D:11 changes named entries to `Result<StepProgress<T>, StepError>` and says that every continuation uses `poll(caller)`, but does not define what `T` is for that poll. Most current entries return unit (`tart/machine/steps.rs:54`, `:180`, `:246`), `seal` returns an `AttemptReceipt` (`tart/machine/cleanup.rs:160`), and `recover` returns a receipt vector (`tart/machine/recover.rs:36`). The stored enum cursor does not make a Rust function's return type depend on its runtime variant. D:100 also requires recovery to retain multiple results across yields.

This leaves the public completion contract unresolved: a continuation must deliver the particular receipt(s) exactly once without a caller guessing a generic type, rerunning a named command, or treating `Pending`/`Done(())` as completion of seal/recovery.

**Minimal design change:** specify one closed completion enum, for example `CompletedStep::{Reserved, Cloned, Started, ..., Sealed(AttemptReceipt), Recovered(Vec<AttemptReceipt>)}`, and give the unified poll an explicit `StepProgress<CompletedStep>` result. Named entry wrappers may preserve typed convenience results if their mapping is defined. State how completion is taken once and what a poll with no active cursor returns. This needs no new worker, public guest method or protocol field.

**Required regression:** pause seal and multi-row recovery after their begin, finish solely via public poll, retrieve the exact corresponding result once, and verify that further polls do not repeat writes or return an unrelated completion. Retain the single-move permit check across the same entry/poll boundary.

### D5 — P2: M4b-1's completion criteria depend on guest progress handling deferred to M4b-2

**Evidence:** D:184 assigns R2-2–R2-7, slow inventory/probes and whole-step progression to M4b-1, but D:185 assigns normalized guest replies and identity integration to M4b-2. In this HEAD, the machine still parses legacy immediate bodies: isolation calls `parse_guest`/`parse_probe` (`tart/machine/steps.rs:302–313`), and bootstrap/login/run immediately parse ACK/login/score (`:346–347`, `:370–371`, `:444–445`). It has no normalized pending/accepted completion cursor. The current fake also refuses an oversized cost rather than retaining work (`tart/guest.rs:148` onward).

Consequently, M4b-1 cannot exercise the promised public bootstrap/login/run accepted-then-lost paths of R2-5 or a multi-slice inventory/probe path against the final guest-progress contract without either implementing part of M4b-2 or installing a disposable legacy-only completion path. The split does not identify which, so passing executor-only tests could be reported as closure of machine findings they have not reached.

**Minimal design change:** move the small shared prerequisite into M4b-1: normalized `Pending/Accepted/Terminal` classification, begin-once guest progress/effect records, saved ends, and cost fakes with actual dispatch accounting. Keep semantic terminal acceptance, actual adapter composition, consume/drain proof, boot pairing and lease scheduling in M4b-2. Alternatively narrow M4b-1's completion claims to executor-only coverage and explicitly defer the unreached R2-5/guest cases. The first option preserves the stated useful independent machine regression bar without a second reply grammar.

**Required regression:** on the M4b-1 commit alone, start through public reserve/clone/start/isolate, hold a normalized guest operation pending across multiple charged steps, observe accepted, then inject result loss/Stop/owner expiry and finish permitted cleanup. No phase setters, no no-op accounting, no M4b-2 code and no immediate legacy success may be needed to reach the assertions. The existing suite must also compile after the runner/ledger signature migration.

## Obligation coverage

Every requested M4b finding has a named mechanism and a regression in the draft. “Covered” here means specified in the design, not proved by code or test execution.

| Obligation | Concrete mechanism | Proposed regression / review disposition |
| --- | --- | --- |
| R2-2: expired owner replacement | D:64–77 checks saved owner end before and after service; permanent suspension | D:160 crosses the old end with a newer valid proof, including destroy/seal and cancellation. Covered. |
| R2-3: intra-step Stop and cleanup | D:74–83 common gates, latched Stop, cleanup-only authority | D:161 injects Stop inside lock/ledger/IO and between cleanup effects. Covered; first Stop must abandon the forward cursor before later cleanup steps. |
| R2-4: shared step and resumability | D:14–48 one step context, frozen operation ends, begin/poll cursors, dispatch evidence | D:162 costs lock/ledger/commands and shares 60+60 ms IO inside 100 ms. Covered in principle; D4/D5 clarify its public contract and first-PR reachability. |
| R2-5: uncertain effects | D:85–92 separate dispatched-effect record and sticky unknown | D:163 covers all three guest side operations, lost/error/decode/provider paths and successful later cleanup. Mechanism covered; split gap is D5. |
| R2-6: bounded kill/reap | D:52–60 retained instance handles, frozen termination end, strict exit proof, owner-first service | D:164 includes pending/late/invalid/unknown/wrong-handle and exhausted-end cases. Covered subject to removing the extra policy in D3. |
| R2-7: reclaim cursor leakage | D:96–103 row identity, per-row ends, reset and outer recovery cursor | D:165 uses two rows, a started-but-name-absent second row and pauses at every stage. Covered. |
| R2-8: original revoke end | D:107–111 intersects every drain read, including an existing pending inventory, and checks after IO/before consumption | D:166 covers that exact reused-read branch plus late first drain and nonterminal timeout. Covered; preserve independent stop after failed drain. |
| R2-11: actual issuance | D:115–118 private completed-publication snapshots, before/after deltas, explicit request-2049 refusal | D:167 covers refusals/polls, post-publication read failure and ordinary service exhaustion. Counting is covered; timestamp safety requires D1. |
| R2-12: boot domains | D:126–130 first accepted inventory boot plus running Tart status before/after, saved pair and later comparisons within each domain | D:168 requires pre-start A, guest B, Tart T with B≠T through a scored seal and independent replacements. Covered. |
| Machine consumption / R2-9 | D:120–124 semantic validation, local evidence/consume commit and exact note in the same step | D:169 composes actual adapter and machine, including legal login/missing rejection, wrong/unissued/future rows and active+revoke. Covered; note failure must suppress success as specified. |
| M4a pinning / R2-10 | D:139 retains the complete adapter pin when g repeats | D:174 includes equal/decreasing g and D:175 preserves existing mailbox regressions. Covered; do not regenerate the host anchor for an equal sample. |
| M4a read regressions | D:43, D:175 preserve FD lifetime, one-read 128 KiB scope and R3 deadline/partial/sticky-byte fixes | Existing regressions must remain, including stable-final expiry. Covered as preservation, not permission to restore an attempt byte cap. |

| ADR §6 offline trace item | Mechanism and required regression |
| --- | --- |
| Slow queries | D:28–48 runner/ledger/query cursors and shared clip; D:158/162 charge list/status/inventory/ledger/parse work within steps. |
| Continuous probes | D:33 saves probe index, per-probe and group ends; D:158 requires all 16 across slices with owner/lease service. No per-probe extra inventory publication. |
| Four-second stop | D:35/52–60 retain one stop/exit handle; D:159 completes/reaps a genuinely pending 4 s stop with internal IO costs. |
| Owner proofs through cleanup | D:64–83 explicit externally supplied proofs; D:159 continues them across multiple proof lifetimes through seal, with Stop retained. |
| In-slice expiry | D:25/45–48 discards late candidates and freezes original ends; D:154/170 charge and expire IO/parse/commit/cancel paths. D1 adds the missing publication timestamp race. |
| Kill/reap | D:52–60 and D:164 specify strict timely exit proof, frozen limits and terminal unresolved quarantine; D3 addresses the unnecessary limit. |
| No revival | D:70–77 owner suspension, D:139–146 sample/ACK/coverage rules, D:170/174 late proof/sample/ACK cases. Conservative failure behavior is specified; D2 prevents mistaking it for feasibility. |
| Long login / max task counts | D:23 saves the Task aggregate; D:115–118/136 count actual publications with an explicit off-grid allowance; D:172–174 require composed long/full-window traces and request limits. Success criteria need the D2 feasibility correction. |
| Actual sample age / grant gap / termination | D:140 and D:153–174 record generated samples, guest acceptance and autonomous expiry/termination. The full-window witness must use the cycle admitted by the gate, per D2. |

## Boundaries and implementation assessment

The shared synchronous cursor/handle approach is the required small structural change to the present synchronous `TartRunner` and `AttemptLedger`; adding asynchronous compatibility defaults would defeat it. Runner/ledger begin/poll can retain their existing fixed operations and output limits. No extra thread, observer, daemon, background renewal, automatic resume, transport fallback, dependency, real admission or wire change is proposed. The additional universal cancellation constant is the exception identified in D3. Do not conflate this necessary interface migration with importing the discarded M4 implementation.

`ClonePermit` remains non-Clone/non-Copy (`tart/ledger.rs:192–241`) and currently has one construction after reservation (`:448`). D:40 correctly keeps an undispatched permit in the cursor, moves it once into clone begin and forbids reconstruction after lost reserve ACK. A readback-confirmed row without the permit must remain an unacknowledged allocation requiring cleanup/recovery; the reserve cursor table must not be implemented as a permit factory.

The owner service order, sticky unknown classification, saved revoke intersection and Tart/guest pairing are consistent with the requested ADR boundaries. Recovery's old row remains `not_run`; its new owner authority and new frozen recovery deadline must stay separate from both the expired attempt deadline and the next live adapter binding. The current row has no persisted mailbox nonce (`tart/ledger.rs:144–151`); implementing row-key takeover in the private owner service must not invent that missing historical nonce or pull M5 persistence into this card.

“No ordinary side effect before first ACK” should use ADR §3's existing guest category: bootstrap/login/run. Clone, volume preparation, VM start, binding inventory and its lease are prerequisites under their own owner/identity gates. With that meaning there is no boot/first-ACK circular dependency; applying the phrase to all runner resource effects would create one. Keep that scope explicit in the implementation and use D:168's public startup positive case to enforce it.

The cleanup 10/20/20/10 s caps remain subordinate to the 60 s aggregate and attempt hard end; a cancellation margin is included inside its operation budget, never added afterward. The host's 100/500/5000/4750 ms Stop constants, the independent S3 3 s scope, and the owner's `g+5s` / total ≤6 s decision remain unchanged. Neither D1 nor D2 calls for extending them. M5 full resource destruction barriers and M6–M9 real channel/supervisor/admission evidence remain outside this review; an offline scored seal is not permission for production VM-only seal or real wiring.

The M4b-1/M4b-2 split is viable after D4/D5 define the completion contract and move or explicitly defer the shared guest-progress prerequisite. As written, M4b-1's runner/ledger executor can be tested independently, but its stated complete R2-2–R2-7 closure cannot be credited independently of M4b-2.

**MERGE-AFTER-FIXES**

Final independent static review at `eb2ce591`: `86f92813`, `bbf46252`, `dc2b2811` and the owner decision; base merges ignored. Judged against ADR 0017 §2's process-instance guarantee and best-effort window checks. **No NEW defect class versus rounds 1–2**: the two remaining schedules below belong to A5 and A7. Accepted A1–A4 are not reopened. Paths below are relative to `computer-use/crates/`.

Must-fix, category (a), before merge:

1. **[P1; A5 residual] Semantic admission can still manufacture an unscoped job.** `gcu-driver-macos/src/semantic.rs:106`, `:109`, `:120`; `gcu-core/src/scope/driver.rs:31`; `gcu-driver-macos/src/real/identity.rs:190`.
   Schedule: two sessions perform on different windows, which the existing window locks allow (`gcu-core/src/session/queue.rs:3`). X has passed scoped admission. Its first pin read at `semantic.rs:106` sees `Some`, so it skips the missing-binding refusal. Y finishes/drops and its `ClearPin` writes `None` before X's second read at `:109`. Production `live_adapter` has `enforcing=false`, so X reaches `JobScope::unscoped()` at `:120`. Move X's element into an unregistered window, preserving its role/title/frame, **before the AX worker begins**; `prove_mutation` now skips all ownership evidence and the write runs. This is missing enforcement, independent of the accepted A3 check/use gap. Capture session-scopedness and the binding once, preserve that scope through semantic admission, and never let a scoped session reach the unscoped fallback. Add a deterministic barrier between these admission reads and assert no write on the non-enforcing live adapter. The current test captures scope manually (`scope_os/round2_tests.rs:505`), bypassing this production function. **Same A5 class, not NEW.**

2. **[P2; A7 residual] Overlapping unscoped actions clear each other's real delivery gate.** `gcu-driver-macos/src/input/proof.rs:23`, `:69`; `input/cg.rs:210`, `:246`, `:338`; `guards.rs:93`.
   Schedule: unscoped actions X and Y on different windows share one `RealEvents`. X arms; Y arms; X completes or is cancelled, and its `Armed::drop` unconditionally clears Y's gate. Y's next ordinary post copies `None` and returns `NotSent`, even with valid permissions/SPI and no concurrent OS post. X can finish between Y's event pairs, producing partial input. The down-event semaphore protects one OS call, not the arm/deliver/drop lifetime. Carry the gate with each operation/event, or serialize that entire lifetime; keep genuinely missing scoped gates refusing. Add an overlapping-actions regression through `RealEvents::with_poster`, including one action dropping while the other remains active. Existing real-sink tests (`input/cg_tests.rs:66`, `:80`, `:89`) are sequential and remain green. **New concurrency reproduction of the previously reported A7 unscoped-regression class, not a NEW class.**

Requested verification:

| Item | Status | Evidence |
| --- | --- | --- |
| A5 · Immutable job scope, including production `enforcing=false` | **PARTIAL** | Once captured correctly, AX jobs store `JobScope` by value (`real/thread.rs:31`, `:108`), semantic writes use it (`real/reader.rs:115`), append uses it (`real/text.rs:59`, `:79`), and started CG dispatch clones its gate before spawning (`input/cg.rs:210`, `:230`). ClearPin no longer relaxes those jobs. Admission still has finding 1. |
| A6 · No application AX toggle writes during scoped observation | **FIXED** | `ScopedDriver::new` marks the session (`gcu-core/src/scope/driver.rs:59`); `semantic.rs:46` selects the no-write store and skips toggle tracking; `ax/toggles.rs:180` skips enabling; `real/reader.rs:353` independently blocks writes. No scoped toggle writes or restore records are created. |
| A7 · Unscoped real CG delivery; missing scoped gate; real-sink tests | **PARTIAL** | The isolated reproduction is fixed: explicit unscoped gate (`input/proof.rs:69`), missing gate still refuses (`input/cg.rs:246`), and injectable-poster tests exercise `RealEvents::dispatch` (`input/cg_tests.rs:66`, `:80`, `:89`). Overlap still regresses delivery: finding 2. |

The remaining hunt established no additional process-instance, permission/SPI, application-write, or CF-lifetime defect. Start time uses checked seconds-to-microseconds conversion with asserted libc size/offsets (`scope_os/live.rs:169`, `:268`); boot identity requires the UUID (`scope_os/facts.rs:58`). Each ordinary scoped CG post rechecks evidence in the posting worker (`input/cg.rs:245`); bounded construction/stamping follows (`:379`, `input/text.rs:25`). Another process-start read could shorten that interval but cannot atomically close the final syscall/scheduling gap; that abstract PID-reuse schedule supplies no NEW finding here.

Nice-to-have: extend the A5 regression coverage through actual AX job timeout/drop and `RealText` admission, beyond the manually constructed scope/helper in `scope_os/round2_tests.rs:488`. This would catch future wiring omissions as well as gate logic errors.

Validation: static inspection only; no cargo/tests, network, SSH, model calls, desktop execution, or source edits. Only this uncommitted report was written.

## 修复记录

Round 3 must-fix items, addressed on this branch:

1. **A5.** `ax_job_scope` reads session scope and the pin once via `JobScope::admit`. The decision uses that value. A scoped session with no binding stays `JobScope::scoped(None)` and is refused. It does not become `JobScope::unscoped()` when another session's `ClearPin` empties the pin, including on the non-enforcing live adapter. `scoped_semantic_admission_clear_pin_between_reads_refuses_write_on_live_adapter` pauses between that snapshot and the decision, clears the pin, then moves the element and asserts the production admission scope stays bound and the write does not run.
2. **A7.** `arm` returns the gate for that action. `EventApi::post` takes the gate. `Armed` drop does not clear a shared slot. `overlapping_unscoped_drop_leaves_the_other_post_sent` overlaps two unscoped actions on `RealEvents::with_poster`, drops one between the other's down and up, and asserts the second post is sent. A scoped action with no binding still returns `NotSent` and does not post.

A1–A4 were not reopened. Real AX job timeout/drop and `RealText` admission were not added to the A5 regression.

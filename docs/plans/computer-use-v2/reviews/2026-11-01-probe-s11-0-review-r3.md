# S11-0 probe final acceptance review, round 3 — 2026-11-01

**Verdict: MERGE-AFTER-FIXES.** Two remaining unbounded waits block acceptance. Both reproduced using local stubs against the unchanged production probe. The ownership repair is sound for successful clone creation and ordinary concurrent destination collisions; I found no remaining path that grants destruction solely because a failed clone's destination appears. The design is reviewable without another framework or general rewrite. Fix the two wait paths below before merge or a Mini run.

Reviewed branch `cu2/probe-s11-0`, HEAD `b4fbc418806a84a1ea1adb7bb36b3bf2c7269dc0`, following the round-2 report at `1d025f96`, implementation `55de8a5e`, original `d56c7083`, and base `7ccadb5a`. Read the complete 735-line probe, 1,138-line suite, runbook, and ADR 0018 context. Paths below are relative to `computer-use/`.

Validation: `bash -n` passed for both scripts; `git diff --check 7ccadb5a..HEAD` passed. The unchanged manual suite passed **37/37**, reporting `ALL_OK 27s`; `/usr/bin/time -p` measured **27.29 seconds**, on Linux Bash **5.2.21**, UID 1001. Temporary copies of the suite under `/tmp` exercised four additional stub-only cases described below. Production code and the existing suite/runbook were not edited. No real Tart/Softnet operation, network, SSH connection, VM operation, or macOS/Bash 3.2 execution occurred. This review file is the only worktree addition; nothing was committed.

## Numbered findings

1. **must-fix — An unreadable process snapshot still reaches blocking `wait` on a live child.** `scripts/s11-0-probe.sh:53`, `:61`, `:65`, `:129`, `:147`.

   **Failure scenario:** `ps` returns no usable row for a still-running command, while `kill -0` continues to succeed. After eight unsuccessful identity samples, `run_bounded` leaves its polling loop. In `reap_bounded`, the missing snapshot breaks the loop at line 53; the second missing snapshot sets `pp=""` at line 61. Neither live-child rejection applies, so line 65 waits for the running child without a deadline. This path bypasses TERM/grace/KILL and further command/probe deadline checks. During cleanup it also runs with INT/TERM/HUP ignored, preventing ordinary cancellation from releasing the wait. A VM already owned by the script can remain uncleared because finalization never finishes.

   **Evidence:** a temporary suite copy made its `ps` stub return an empty failed snapshot for the TERM-ignoring `tart list` stub. With `S11_BOUND_SEC=1` and `S11_BOUND_POLLS=8`, the probe remained stuck until the harness killed its isolated test process group at three seconds: **exit 137, no report**. The existing foreign-PPID test exercises a populated snapshot and therefore misses this branch.

   **Minimal fix:** allow blocking `wait` only after positive exit evidence: the PID no longer exists, or the verified direct child is a zombie. A missing snapshot with a live PID must remain an unknown process state and finish through a bounded failure path; retain the unreaped-child failure rather than treating it as reaped. Preserve the rule against signaling an unverified/foreign PID. Add the empty-snapshot case to the manual suite.

2. **must-fix — FIFO reader reaping can wait indefinitely after the command exits.** `scripts/s11-0-probe.sh:82`, `:84`, `:88`, `:114`, `:149`.

   **Failure scenario:** a command starts a background descendant that inherits stdout/stderr, then the direct command exits. The descendant keeps a FIFO writer open, so one or both `head` readers remain alive waiting for EOF. `drop_readers` only attempts to terminate readers when the original command PID still exists. Once that PID has disappeared, lines 88–89 unconditionally wait for the readers, outside every deadline. A failed input redirection before the command opens the FIFOs also leaves readers blocked opening them. Even in the branch that sends TERM, reader termination has no KILL or bounded reap; readers launched during cleanup inherit ignored TERM. The same runner handles collection, stop, delete, and inventory, so a stuck capture can prevent VM cleanup and report publication.

   **Evidence:** the version stub started `sleep 30` in the background with inherited output descriptors, printed its normal version, and exited zero. With `S11_BOUND_SEC=1`, the probe still required the harness's three-second group kill: **exit 137, no report**. The production probe was unchanged. The existing hang stubs use short-lived descendants, which allow their reader waits to finish and conceal this failure.

   **Minimal fix:** bound reader draining and terminate/reap both registered reader children on every runner exit, independently of the command PID's existence. Include escalation for ignored TERM. If capture cannot finish within that bound, report capture failure and reject the inventory rather than accepting potentially incomplete bytes. Add a descendant-held FIFO case; the fix must also cover a reader that never gets a writer.

3. **should-fix — The advertised Running-only inventory fallback does not normalize its values.** `scripts/s11-0-probe.sh:272`, `:274`, `:665`; `scripts/test_s11_0_probe.sh:113`; `docs/s11-0-probe-runbook.md:55`.

   **Failure scenario:** a table with `Name` and `Running`, but no `State`, is accepted by the parser. Using the existing fixture's `Running=no` value for a stopped golden then fails precheck as `golden-unknown`, because precheck only accepts `stopped`/`running` spellings. The suite always supplies both columns and selects `State`, so its boolean Running column never exercises the fallback.

   **Evidence:** a temporary fixture containing only `Source / Name / Disk Size / Accessed / Running`, with `no` for the golden, produced **exit 2, `golden-unknown`**. This demonstrates the claimed-format gap; the actual Tart 2.41.1 table and value vocabulary were not available for verification in this offline Linux review. It fails closed and performs no VM mutation, so this is nonblocking.

   **Minimal fix:** use a captured version-specific table fixture and normalize that schema's Running values if this fallback is supported. Otherwise narrow the stated contract to the supported State schema. Test the supported schemas separately instead of supplying both columns in every successful case.

4. **should-fix — A failed dry-run precheck returns success.** `scripts/s11-0-probe.sh:672`, `:673`, `:727`.

   **Failure scenario:** `--dry-run` fails a required precheck, but `decide` handles DRY before PRECHECK_FAILED, replacing the intended exit 2 with `not-run`/0. A caller using dry-run to check installation readiness receives success despite a missing prerequisite. The report's reasons and empty measured list remain conservative; a subsequent real run repeats precheck.

   **Evidence:** a temporary stub case made the Softnet target non-SUID. `--dry-run --stage A` returned **exit 0** with reason **`softnet-not-suid`**.

   **Minimal fix:** prioritize precheck failure before successful dry-run in `decide`, and assert nonzero status for a dry-run prerequisite failure.

5. **should-fix — The consecutive dry-run and real-run examples reuse the output directory.** `docs/s11-0-probe-runbook.md:24`, `:27`.

   **Failure scenario:** following the two commands in the first example creates the default `./s11-0-out` during dry-run. The following real command explicitly selects that same directory and correctly refuses it as `output-exists`. No real probe runs.

   **Minimal fix:** give the dry-run example its own fresh `--out` directory and retain a different fresh directory for the real run. The production refusal is correct.

## Round-2 acceptance checklist

| R2 finding | Assessment at `b4fbc418` |
| --- | --- |
| 1 — inventory schema / unknown versus absent | **Substantially repaired.** Header offsets handle multiword cells; empty, missing-header, short-row, command-error and overflow cases cannot establish absence. Successful tests use a synthetic combined State/Running schema. Finding 3 records the remaining supported-format uncertainty. |
| 2 — clone provenance / reusable name | **Fixed.** Ownership requires clone exit 0 plus a trusted post-clone listing. Failed creation with a present destination, timeout, overflow, or uncertain post-clone evidence sets ambiguity, forbids destruction, returns 5 and blocks B. Ordinary failed creation plus trusted absence returns 3. The operational forced-name seam is gone. |
| 3 — termination / reaping / exec transition | **Partially fixed; findings 1 and 2 block acceptance.** Timeout, cancellation and overflow of a positively identified direct child share TERM/grace/KILL/reap. Changing executable name no longer invalidates ownership. Unknown snapshots and capture-reader waits still escape the bounds. |
| 4 — A cleanup failure overwritten by B | **Fixed.** `can_advance` rejects cleanup failure, ambiguity, pending cancellation and retained ownership. The all-stage delete-failure test checks that A is preserved and B never starts. |
| 5 — boot / IP / SSH readiness | **Repaired for normal startup progression.** Nonzero no-address results and failed key-only readiness SSH are retried; collection runs once after readiness. Tart exec has an independent bounded startup opportunity and remains no proof of IP independence. IP/SSH progression has a regression test. |
| 6 — output limits | **Byte retention repaired.** Finite output is capped at production time with an overflow sentinel; overflow fails inventory. Long-lived `tart run` output goes directly to `/dev/null`. Reader lifecycle still has finding 2. |
| 7 — aggregate accounting | **Repaired as nominal accounting.** Command time is clamped to remaining probe/cleanup time. Cleanup allowance is cumulative across stage cleanup and EXIT retry; intermediate cleanup also consumes probe time. The runbook explicitly disclaims an exact aggregate hard bound and describes grace/finalization overhead. The unbounded waits remain separate blockers. |
| 8 — DNS transport provenance | **Fixed.** Single `dig -r +notcp +ignore +time=2 +tries=1`; no host/nslookup ladder. The stub checks those options and rejects TCP overrides. UDP/443 and QUIC remain unmeasured. |
| 9 — actual coverage / C failure / completeness | **Substantially fixed.** Dry-run and failed-precheck coverage is empty; network roles require an actual complete collector result. Expected role IDs must appear exactly once; failed C help collection returns 4. Missing dig is explicitly unmeasured. Successful observations retained in an otherwise failed collection are conservatively omitted from top-level measured coverage. |
| 10 — stale report / killed run | **Fixed in behavior and recovery wording.** Existing output directories and symlinks are refused before Tart operations. SIGKILL can leave no final report; prefix/old-report deletion is forbidden. Finding 5 is an example correction. |
| 11 — suite strength | **Materially stronger.** 37 cases include representative multiword columns, ambiguous creation, A-to-B cleanup refusal, TERM-ignoring timeout/cancel/overflow, exec transition, foreign PPID, startup progression, help failure, stale output and route failure. Findings 1–3 expose remaining gaps. The shared ownership assertion still checks a clone-attempt log; explicit failed/ambiguous-creation tests provide the stronger negative ownership evidence. Linux stub success establishes neither BSD-tool behavior nor actual VM/network facts. |

All four round-2 requested deletions are present: `S11_FORCE_NAME`, raw `tart run` capture files, multi-tool DNS fallback, and `CMD_LIVE`. `CMD_PID` remains write-only bookkeeping and can be removed as a nit. No additional watcher, production process-group kill, hardware survey, policy classifier, or second report was introduced.

## Platform and remaining scope

Static inspection found Bash 3.2-compatible indexed arrays and ordinary shell constructs, with no associative arrays, newer expansion forms, `wait -n`, GNU timeout, `readlink -f`, GNU stat, or GNU date dependency in the production probe. BSD `stat -f '%Su %Sp'`, plain `readlink`, `head -c`, macOS nc's `-z -G 2 -w 2`, and the fixed dig flags are appropriate forms for the intended host/guest. No additional definite Bash 3.2/BSD syntax defect was found. Actual macOS execution and the precise Tart inventory schema remain unverified; the suite substitutes those tools.

No new golden launch/destruction, wildcard VM deletion, model or credential collection, LAN scan, network-rule mutation, sudo, or production group signaling was found. Clone ownership and stage sequencing address the previous foreign-VM deletion and ownership-overwrite defects. The two reproduced wait failures prevent confirming that the probe cannot hang or strand an already-owned clone. H1 remains explicitly unknown, and the report does not claim unperformed QUIC or UDP/443 measurements.

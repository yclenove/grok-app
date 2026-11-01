# S11-0 probe independent review, round 2 — 2026-11-01

**Verdict: MERGE-AFTER-FIXES. Do not run this revision on the Mini.** The reduction is substantial, and several round-1 problems are fixed at their source. Remaining blockers include an incompatible inventory parser, ambiguous clone ownership, unbounded waits, losing an outstanding clone between stages, premature boot observations, uncapped run output, and an unverified DNS transport label. These need targeted repairs; another expansion of the probe is unnecessary.

Reviewed branch `cu2/probe-s11-0`, HEAD `55de8a5e`, against previous `d56c7083` and base `7ccadb5a`. Read the round-1 report first, then the complete 497-line probe, 884-line suite, runbook, verify-hook removal, and ADR 0018 §5/§10. Paths below are relative to `computer-use/` unless identified otherwise.

Validation: syntax checks passed for the probe, suite, and `verify.sh`; `git diff --check 7ccadb5a..55de8a5e` passed. The unchanged stub suite passed **28/28**, reporting `ALL_OK 23s`; measured elapsed time was **23.297 seconds**, on Linux Bash **5.2.21** as UID 1001. The probe was executed only through the stub suite. No real Tart/Softnet invocation, network, SSH connection, VM operation, mutation execution, or macOS/Bash 3.2 execution was performed. Only this uncommitted review file was added to the worktree; code and existing documentation are unchanged. Platform/API failure scenarios below are static findings, not Mini reproductions.

## Findings

1. **must-fix — `vm_status` parses the stub's invented table, not Tart's inventory.** `scripts/s11-0-probe.sh:168`, `:175`, `:441`; `scripts/test_s11_0_probe.sh:105`, `:200`.

   **Failure scenario:** the production command is plain `tart list`. Tart's normal table has a `Source` column before `Name`, with state later in the row. A local golden row starts with `local`, not `macos-s2-eval`. The parser instead requires `$1 == name` and returns `$2` as state. On the intended Tart installation, the existing golden therefore fails precheck as `golden-missing`. The same parser supplies every collision, post-clone, and deletion-confirmation decision. The suite emits only `Name Status` and two-column rows, concealing the incompatibility.

   **Minimal fix:** parse the actual version-specific inventory schema, including local source/name/state, and reject malformed or truncated inventory instead of treating it as authoritative absence. Use a representative Tart 2.41.1 table fixture in the stateful stub. A supported structured format is also suitable if its availability and schema are established; do not assume an unverified JSON flag exists.

2. **must-fix — A failed clone plus name appearance still authorizes destruction of a foreign VM.** `scripts/s11-0-probe.sh:127`, `:309`, `:311`, `:313`, `:318`, `:320`.

   **Failure scenario, after correcting finding 1:** two invocations inherit the same `S11_FORCE_NAME=s11-0-probe-A-99-abc`. Both observe absence. One creates the VM; the other's clone fails because the destination now exists. The losing invocation sees the winner's name in its post-clone list, sets `OWN_OK=1` **regardless of clone status**, and stops/deletes the winner's VM. This is the round-1 ownership problem through a concurrent collision. The hidden environment override restores a reusable operational name despite removing `--clone-name`. An absence/presence pair does not establish who created a destination when creation failed.

   **Minimal fix:** remove the production forced-name override. Retain the before/after checks, but do not grant destructive ownership to an ambiguous failed/timed-out creation solely because its name appeared. Keep its reserved name and partial-creation outcome in the report; only clean it automatically if creation by this invocation is independently established. Otherwise stop further stages and report the ambiguity for owner recovery. A clone-attempt log is not that proof.

3. **must-fix — Cancellation, overflow, or an executable-name transition can reach an infinite `wait`.** `scripts/s11-0-probe.sh:32`, `:38`, `:48`, `:77`, `:80`, `:82`, `:85`, `:98`, `:103`.

   **Failure scenario:** a direct child ignores TERM. On cancellation or output overflow, `run_bounded` sends only TERM and breaks; `reap_owned` then executes blocking `wait` **before** its KILL line. KILL is unreachable while the child remains alive. During cleanup, ignored TERM is also inherited by launched commands unless they reset it. A separate route exists when the first `ps` snapshot catches the forked shell before exec: `comm` subsequently changes to Tart/SSH, `same_proc` becomes false, and the caller waits on a still-running child without a deadline. The nominal timeout branch's TERM/grace/KILL does not cover these paths. Clearing `CMD_LIVE` before waiting further loses the active command's registration.

   **Minimal fix:** route every termination reason through bounded TERM/grace/KILL before waiting; keep the child registered through termination and reaping. Handle the initial exec transition within the deadline rather than interpreting a changed command name as process exit. Only signal the owned direct child after validating its instance. Add TERM-ignoring cancellation/overflow and delayed-exec cases. Do not bring back process-group killing or a shell-only watchdog.

4. **must-fix — Stage B overwrites ownership when stage A could not be destroyed.** `scripts/s11-0-probe.sh:267`, `:300`, `:318`, `:348`, `:493`, `:494`.

   **Failure scenario:** in `--stage all`, stage A's delete fails or its absence cannot be confirmed. Cleanup correctly retains `OWN_OK=1` and A's name. Nevertheless main enters B, whose successful creation assigns `OWN_NAME=B`, losing A's only live cleanup record. A's clone can remain allocated or running while the script proceeds. The existing failure tests use only `--stage A`, so finalization can retry A there and never exercises this overwrite.

   **Minimal fix:** refuse another creation while any ownership or creation ambiguity is outstanding. After unsuccessful A cleanup, stop the stage sequence, preserve A's record, and finalize with a nonzero result. No multi-VM ownership framework is needed for this sequential probe.

5. **must-fix — The boot loop exits on normal not-yet-ready conditions, and SSH is tried only once.** `scripts/s11-0-probe.sh:104`, `:327`, `:330`, `:334`, `:338`; `docs/s11-0-probe-runbook.md:51`.

   **Failure scenario:** a fresh macOS guest has not acquired an IP when the first `tart ip` runs. Tart can return nonzero for no address yet; line 334 immediately breaks instead of waiting within the advertised 240-second boot budget. If an address exists before sshd is ready, the single SSH attempt fails and the clone is destroyed. `start_run` proves only one second of process survival; `tart exec` is also attempted immediately, before guest readiness. Normal startup timing can therefore yield no network measurements and a failed control-channel observation on a working installation. The stub always supplies an address and immediate SSH/exec success.

   **Minimal fix:** retry IP discovery and a fixed key-only SSH readiness command within the boot/probe deadlines, preserving the final timeout/tool-error distinction. Run the collector once after readiness. Give the independent Tart exec attempt an appropriate bounded boot opportunity too, without making it depend on successful SSH or interpreting it as non-IP proof. Keep all attempts inside the same owned-child runner.

6. **must-fix — The 64 KiB claim is post-exit truncation for `tart run`, and overflow need not fail collection.** `scripts/s11-0-probe.sh:27`, `:81`, `:82`, `:90`, `:97`, `:104`, `:114`; `docs/s11-0-probe-runbook.md:41`.

   **Failure scenario:** a noisy long-lived `tart run` writes unlimited stdout/stderr throughout boot and collection. Neither `start_run` nor later work monitors those files; `cap_file` runs only when the process is stopped. It can fill the temporary volume before truncation. Short commands also have no explicit overflow result: a command that exits zero with excessive output is truncated and still accepted as successful. For `tart list`, an omitted tail can then be mistaken for VM absence. The cancellation-on-overflow path additionally has finding 3's hang.

   **Minimal fix:** discard `tart run` output if no diagnostics are consumed from it. Bound retained output while it is produced, and propagate a distinct capture-overflow failure for finite calls. Never use truncated inventory as absence evidence. Applying `head` only after a writer exits does not limit that writer's disk use.

7. **should-fix — The stated aggregate deadline is not the deadline implemented.** `scripts/s11-0-probe.sh:69`, `:73`, `:79`, `:252`, `:458`; `docs/s11-0-probe-runbook.md:7`, `:41`.

   **Failure scenario:** outside cleanup, a command started just before `PROBE_DEADLINE` retains its full per-command limit; the wrapper neither clips it to remaining probe time nor checks the global deadline inside its loop. Each `cleanup_owned` invocation grants a fresh 90 seconds, including the EXIT retry. Intermediate cleanups also consume `SECONDS` against the original probe deadline despite the runbook saying cleanup does not consume the 1500 seconds. GRACE/reaping/report work is not reserved in the claimed 1590-second total. Thus the documented accounting is false even apart from finding 3's blocker.

   **Minimal fix:** use remaining time in the wrapper, define whether intermediate cleanup counts against probe time, and keep one cumulative cleanup allowance with termination grace reserved. Describe that actual accounting in the runbook. Do not claim an exact hard aggregate bound before the primitive and finalization honor it. This accounting defect is listed separately as should-fix; the concrete unbounded execution paths are finding 3.

8. **must-fix — Successful DNS is labeled UDP without fixing the transport actually used.** `scripts/s11-0-probe.sh:197`, `:198`, `:199`, `:222`; `docs/s11-0-probe-runbook.md:51`.

   **Failure scenario:** `dig` reads the guest user's `.digrc`; an existing `+tcp` setting makes this command perform TCP DNS, while the resulting observation is named `dns-udp-gateway`/`dns-udp-public`. The ordinary DNS tools can also retry truncated UDP responses over TCP. The collector records neither which tool nor transport was used, so a successful DNS transaction is not evidence that the named UDP-only exchange was performed. This matters when comparing protocol filtering. The current stub checks servers but returns success regardless of DNS flags.

   **Minimal fix:** use a single supported DNS command with user defaults disabled and UDP/TCP-fallback behavior explicitly fixed, or label the result as transport-unspecified DNS and move UDP-specific coverage to unmeasured. For stock `dig`, verify the intended combination of `-r`, `+notcp`, and `+ignore`. Deleting the `host`/`nslookup` fallback ladder is preferable to adding three transport classifiers. Keep QUIC unmeasured.

9. **should-fix — Coverage is configured in advance, and help collection failure can still finish `complete`.** `scripts/s11-0-probe.sh:277`, `:342`, `:397`, `:450`, `:483`, `:492`; `scripts/test_s11_0_probe.sh:819`.

   **Failure scenario:** dry-run, a failed precheck, or failed SSH can leave every network role in top-level `measured` although no observation exists. For `--stage C`, failed/timed-out help commands set their individual fields but never set a failure flag, so the only requested collection can fail and still return `complete`/0. Guest collection now correctly propagates nonzero SSH/collector status, but its completeness check accepts any positive number of allowed rows rather than the expected role set.

   **Minimal fix:** distinguish planned coverage from actual per-stage observations; derive actual measured/missing roles from collection. Propagate help-collection failure for C. Validate the expected unique role set when declaring a network collection complete. A recorded individual transport failure may remain a completed observation; an absent collector or missing row is different. Existing per-field failure states make this chiefly an evidence/status clarity issue, not proof of successful isolation.

10. **should-fix — Hard-kill recovery instructions assume a report that may not exist or may be stale.** `scripts/s11-0-probe.sh:378`, `:395`, `:459`; `docs/s11-0-probe-runbook.md:45`.

    **Failure scenario:** SIGKILL/power loss occurs after creation but before the EXIT trap publishes `report.json`. The default output directory can contain the previous run's report, or no report at all. Even stage fragments are only in the temporary work directory, which is removed during ordinary finalization. The runbook cannot reliably identify this run's remaining clone from `created=yes` in that final report. This does not authorize prefix recovery or make hard-kill cleanup possible.

    **Minimal fix:** state explicitly that a killed run may have no final report and that a reused output directory may contain stale evidence. Use a fresh output directory and retain the exact verified creation name privately when it is established, or document an owner-assisted inventory reconciliation that never treats a prefix/old report as ownership. Do not imply the script handles SIGKILL.

11. **should-fix — The suite is materially stronger but still cannot establish its full safety claims.** `scripts/test_s11_0_probe.sh:96`, `:105`, `:151`, `:167`, `:264`, `:365`, `:674`, `:705`.

    **Failure scenario:** stateful inventory now catches ordinary existing-name, failed-clone, partial-clone, and stop/delete failures. Happy-path assertions check exact A/B launch lines and report argv, SSH option presence, and concrete TCP/DNS targets. Nevertheless inventory uses the wrong production schema; hung stubs cooperate with TERM; signals are sent only after cleanup has already started; IP/SSH have no startup progression; and cleanup failures are tested only with stage A. `assert_owned_only` proves a logged clone *attempt*, not successful creation by this process. No overflow, concurrent creation, exec-transition, or C-help failure case exists.

    **Three concrete production mutations this suite would not catch** (static assessment; not executed):

    - Remove only the `"$pp" == "$$"` predicate from `same_proc` at `scripts/s11-0-probe.sh:38`, leaving command-name equality. No case supplies a recycled PID belonging to a different parent, so the suite would not detect the weakened direct-child check.
    - Change line 196 from `nc -z -G 2 -w 2 "$3" "$4"` to `nc "$3" "$4"`. The nc stub ignores/removes options, logs the same host/port, and returns the same outcomes. Real zero-I/O and connection-timeout behavior is lost.
    - Add `+tcp` to the `dig` call at line 197. The stub records the same `@server`; assertions continue to accept DNS rows labeled UDP.

    **Minimal fix:** prioritize representative inventory, competing-name/failed-creation provenance, all-stage cleanup refusal, TERM-ignoring cancellation/overflow, delayed IP/SSH readiness, and DNS option assertions. Keep the manual suite separate from universal verification. Its 23.297-second Linux run meets the requested under-30-second target on this machine; it is not Bash 3.2 or BSD-tool validation.

## Disposition of round-1 findings

| Round 1 | Root-fix assessment at `55de8a5e` |
| --- | --- |
| 1 — premature ownership | **Partial.** Ordinary pre-existing-name rejection and post-clone inventory checks were added. R2 findings 1/2/4 remain. Golden stays the fixed clone source and is excluded from destructive names. |
| 2 — blocking calls/deadline | **Partial.** Every short Tart/SSH call uses `run_bounded`, and `SECONDS` replaces explicit `date` arithmetic. R2 findings 3/6/7 remain; the long-lived run uses a separate startup path. |
| 3 — PID/signals/reaping | **Partial.** PPID plus command checks reject ordinary unrelated recycled PIDs; negative/group signaling is gone from production; cleanup masks INT/TERM/HUP. Blocking reap and initial exec transition remain, R2 finding 3. These checks do not establish a retained unreaped original instance. |
| 4 — misleading policy vocabulary | **Substantially fixed.** No blocked/reachable conclusions. Unknown command errors normally remain tool-error. DNS protocol and planned/actual coverage need R2 findings 8/9. |
| 5 — invalid UDP/QUIC/coverage | **Fixed by removal for UDP/443 and QUIC.** They and the omitted IPv6/DNS categories are explicitly unmeasured; no dummy QUIC packet remains. DNS protocol provenance still needs R2 finding 8. |
| 6 — invented launch evidence | **Fixed.** One `build_argv` supplies runtime, dry-run, and report; actual fixed-name arguments survive `json_list`. Precheck/clone failure has empty argv; launch outcome is separate. `argument` is input provenance, not approval; only the explicit placeholder flag labels placeholder. |
| 7 — ignored collector failure | **Fixed for nonzero/timed-out guest collection.** It now yields partial/4 after cleanup. Remaining completeness/help-status limitations are R2 finding 9. Tool-error observations are intentionally allowed as collected facts. |
| 8 — false H1 evidence | **Fixed for honesty/scope.** Hardware/terminal heuristics are gone; help is collected before network stages; exec is attempted even if SSH later fails; H1 stays unknown. Startup timing needs R2 finding 5. Success still proves no IP independence. |
| 9 — Softnet symlink/SUID | **Fixed for an ordinary PATH installation.** Relative/absolute symlink traversal is capped at 16; BSD stat checks the final target; the unused override is gone. This remains a PATH installation check, not runtime attestation of Tart's selected Softnet image. |
| 10 — privacy/raw diagnostics | **Fixed on the normal report/console path.** No hostname/hardware inventory, LAN/gateway/guest literal, timezone, or proxy collection is reported. Tart/SSH raw output is privately captured rather than echoed. Public endpoint `/32` is deliberately retained for argv verification. Capture size still needs R2 finding 6. |
| 11 — universal/Linux-only hook | **Fixed.** Hook removed, no GNU timeout in the harness, manual/non-CI status documented. No Mac execution was established. |
| 12 — permissive tests | **Partial, substantially improved.** Stateful inventory and exact A/B/target assertions exist. Remaining blind spots and three missed mutations are R2 finding 11. |
| 13 — runbook promises | **Partial.** Authorized-key readiness, indirect Softnet invocation, endpoint examples, generic Tart-home path, and manual owner notice are corrected. Duration, startup wording, UDP provenance, and recovery need the findings above. |
| 14 — excess scope | **Largely fixed.** 1,998 lines reduced to 497; hardware scans, hostname, most overrides, duplicate reports, policy conclusions, and separate dry-run argv deleted. Remaining useful deletions are below. |

## Platform and scope notes

The production syntax uses indexed arrays/array append, ordinary parameter expansion, `[[ ... ]]`, and Bash `SECONDS`; I found none of the forbidden GNU/Bash-newer dependencies: external `timeout`, `readlink -f`, `mapfile`, `declare -A`, `${x,,}`, `wait -n`, `date -d`, `sed -r`, or `stat -c`. This is a static compatibility assessment, not a passing Bash 3.2 run.

Concretely, `stat -f '%Su %Sp'` is BSD/macOS formatting, not GNU stat formatting; `readlink` without `-f` plus the bounded loop is suitable for these symlinks. macOS nc supports `-z`, `-G 2`, and `-w 2`; `-G` is Mac-specific, not a portable Linux nc flag. Without `-v`, refusal diagnostics may be empty and become tool-error rather than refused; that loses detail but does not become a denial claim. `dig +time=2 +tries=1`, `host -W 2`, `route -n get default`, and plain `ifconfig` are macOS forms, but the DNS tools are not interchangeable protocol evidence (finding 8). The outer SSH deadline is necessary in addition to those inner flags. Route/ifconfig pipeline exit status is not preserved, so an execution failure can currently look like missing gateway/global address; preserve tool-error if these paths are retained. The suite stubs these BSD tools and therefore does not validate their behavior.

No new model invocation, credential collection, LAN scan, PF/firewall/route/resolver mutation, sudo, golden launch, wildcard VM deletion, or production process-group kill was found. Fixed `eval` and fresh A/B names are appropriate. Cleanup's direct stop → terminate/reap launcher → delete → list ordering is reasonable once ownership, bounded reaping, and inventory are repaired; failures must prevent advancing to B.

**Anything left to delete:** remove the operational `S11_FORCE_NAME` seam (finding 2), unused raw `tart run` capture files (finding 6), and multi-tool DNS fallbacks if UDP is the intended experiment (finding 8). `CMD_LIVE` is assigned but never consulted by signaling; remove it or make registration part of the actual owner invariant. Avoid adding another watcher, process-group framework, general sanitizer, hardware survey, or policy classifier. Keep the one JSON report, single argv builder, explicit unknown H1, and unmeasured QUIC categories. Owner notice remains a manual prerequisite and is not authorization to run now.

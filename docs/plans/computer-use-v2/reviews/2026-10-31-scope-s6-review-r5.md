**MERGE-AFTER-FIXES**

Narrow independent static review at `71e8e0d4`, base `d0d951b3`: only the two round-4 findings, their fixes (`3bb3f250`, `5b77cff3`, `90c301e3`, `71e8e0d4`), and the Windows retry. Paths below are relative to `computer-use/crates/gcu-eval/` unless stated.

Must-fix, category (a):

1. **[P2; SAME CLASS as round-4 finding 2] The fake worker can write after timeout, cleanup, and return.** `src/fake_agent.rs:115`, `:122`, `:126`, `:380`; `src/fake_io.rs:18`; `src/exec.rs:82`; `src/runner.rs:475`.

   Concrete schedule: run bundled `native-form-01` with the in-process fake, a 50 ms timeout, and an absolute regular probe path outside the task root. Pause the worker immediately after its successful cancellation check at `fake_agent.rs:380`, before `write_replace` at `:381`. Let the runner expire the deadline, set cancel, exhaust the 500 ms join budget, and forget the still-running worker. It returns an ordinary Timeout; `finish` removes the root and reports `env_destroyed=true`, `cleanup_failed=false`, and the suite can continue/return. Resume the worker: it creates/truncates and writes the probe **after return**. The next cancellation check is too late. This requires neither a special file nor a filesystem race. Dropping the pending execution future also drops its join handle without setting cancel or confirming exit.

   Moving I/O off the runner fixes the original executor hang, but does not establish worker quiescence. Unix `O_NONBLOCK` does not bound regular-file I/O; Windows sets only `FILE_FLAG_OPEN_REPARSE_POINT`, not a nonblocking mode. Neither flags nor a check before a write revoke an already-running worker's authority.

   Do not detach a worker still able to perform I/O: establish termination or revoke its write authority before cleanup/return. Unconfirmed shutdown must invalidate cleanup, yield not_run, and halt subsequent tasks. Add a barrier-controlled regression that holds the worker across cancellation and the join budget, then releases it and checks that no late write is possible; also cover cancellation by dropping the future. This is a new failing schedule introduced by the repair, **not a NEW defect class** and not an A1 recurrence.

Requested checks:

- **Round-4 finding 1 / rounds 1–4 A1: FIXED in default builds.** `src/boundary.rs:63` has private fields and no Clone/Default/serialization constructor. Production minting stays inside admission; all three `admit_*` reject executable requests, and the private provider/ACK gates remain false. The lifecycle mint (`:154`) and re-export (`src/lib.rs:49`) require `test-support`; the fixture mint requires `all(test, unix)`. Cargo has no default feature enabling it; `tests/scope_supervisor` correctly declares `required-features = ["test-support"]`. All manifests were checked: the only other dependency is gcu-cli's dev-dependency, without that feature. The added CI steps explicitly enable it only for clippy/tests.
- **Process construction: FIXED for the reviewed paths.** Client execution/preflight, fixture startup, and scoped startup consume capabilities through `std_command`/`tokio_command`. `Supervisor::start` consumes by value; `open` admits at `src/scope/mod.rs:52`, before command preparation, spawn, or bootstrap. No ungated path-taking start remains. Searches across **all** `computer-use/crates/` and `computer-use/xtask/`, including gcu-cli, found no additional eval client/backend/fixture launch route bypassing this boundary; test subprocesses and unrelated product/build launchers were distinguished.
- **Boundary regressions:** the scan at `src/boundary.rs:342` detects an ordinary added `Command::new` in another production module; removing either current feature cfg fails the gating assertion. Read-only Python emulation of these three in-memory mutations confirmed that behavior; Rust tests were not executed. The public-entry sentinel test also checks refusal before execution.
- **Round-4 finding 2: PARTIAL.** Fake invocation I/O now runs on its own thread. Unix no-follow/nonblocking opens and Windows reparse-point opens validate the **opened handle** as regular before explicit reads/writes (`src/fake_io.rs:48`, `:57`, `:108`). The no-reader FIFO schedule now fails promptly. Actual root-removal failure still becomes not_run and inherited halt (`src/runner.rs:478`; `src/runner/provider.rs:215`, `:114`), but an unjoined worker is not counted as cleanup failure.
- **Stall regression:** `tests/fake_stall.rs:26` uses a separate watchdog process and would fail the original synchronous FIFO implementation by killing the hung child. Its FIFO setup does not depend on sleeps. However, the late-writer assertion at `:192` sleeps 100 ms, and neither it nor the cooperative timeout case forces an unfinished worker through the 500 ms escape. They do not detect the must-fix schedule above.
- **Windows retry: FIXED / no new category-(a) defect found.** `tests/fake_agent_boundary.rs:52`, `:71`, `:80` retry only Windows OS error 32: five attempts total, four 50 ms delays, fresh output handles each time. Other errors and exhausted retries fail immediately; child exit remains bounded.

Nice-to-have:

- Broaden the boundary source guard beyond this crate and replace the 240-character cfg lookback with an item-aware or compile-fail check (`src/boundary.rs:385`, `:395`). The requested simple mutations are caught today; aliases, filename exemptions, and nearby unrelated attributes can evade this textual guard.

Validation: static inspection, read-only source/manifest checks and in-memory scan mutations only; `git diff --check origin/feat/cu-v2...HEAD` passed. No Cargo, compilation, candidate execution, network, SSH, model calls, or source edits. Only this report was written, uncommitted. No additional NEW category-(a) defect found within the requested scope.

## 修复记录

本节追加第 5 轮唯一必修项；保留以上独立评审正文与结论。原则：工作线程的停止必须被确认；确认不了，该尝试和之后的一切都不可信。未改 Stop 常量、fail-closed、准入、能力类型、`test-support` 门、`STATUS.md` 或 GitHub Actions。未再使用 `mem::forget`，也没有另造一套停后续任务的机制。

超时后 `execute` 置取消标志，再在最多 500ms（`fake_worker::JOIN_BUDGET`）内 `join`。句柄放在 `OwnedWorker` 里，跨 `.await` 仍留在该结构中，取消 future 不会把它 detach。`join` 成功才是普通超时（`worker_unconfirmed == false`，状态仍由既有超时路径判定）。预算到期仍未结束：句柄进入进程内 `PARKED`（毒化的 mutex 用 `into_inner`，不把句柄丢在地上），`Finished.worker_unconfirmed = true`。这个字段不复用 `wait_failed`：后者仍是进程 wait 失败，继续走 `Crash`。真实 `proc::run_command` 把该字段设为 false。

`exec` 在读日志、读状态、打分之前返回：该 task 为 `NotRun`，原因标签 `worker_unconfirmed`，`client_started` 为 true（线程已经开始跑），遥测保持 `Unknown`。`runner::finish` 认出该标签后 `disarm`，于是既有 `Cleanup::remove` 返回 `cleanup_failed` 且不删树，`Drop` 也不会删。随后走既有 `cleanup_failed()`：原因前缀 `cleanup_failed; …`，状态强制 `NotRun`。`env_destroyed` 在未确认时不得为 true。`InheritedStop::from_outcome` 因此停掉同一次 `run_tasks` 的后续 task，以及 `ab::run_ab` 的后续 arm/repeat。`run()` 已经把结果送出、只是线程还没退出时，仍停靠句柄，但不把该结果改成未确认。

丢弃 `execute` 的 future 时，`OwnedWorker::drop` 先置取消标志，再做同一预算的同步 `join`，仍未结束则停靠。取消标志保证线程下一次检查就停。

回归在 crate 内、`cfg(test)`，不靠 sleep。栅栏在取消检查成功之后、`write_replace` 之前；join 预算可注入（测试用 0）。三条测试：`execute` 返回未确认且 stdout 为空；runner 上该 task 为 `NotRun` + `cleanup_failed` + `env_destroyed == false`，root 仍在，后续 task 不启动，`summary.scored == 0`；丢弃 future 会置取消标志并停靠。释放 worker 之后探针写入被记为迟到，已密封的 `Finished` / `TaskOutcome` 不被改写，下一次取消检查不再写 state。`tests/fake_stall.rs` 的晚写检查去掉 100ms sleep：runner 返回前已经 `join`，非阻塞读到 `Ok(0)` 或 `WouldBlock` 即可。集成测试不带 `cfg(test)`，钩子不进入该二进制。

**已知窗口。** 线程若在“检查已通过”和“写”之间恢复，再查一次取消标志也盖不住这次写。因此以未确认停止把整次尝试判为不可信：不打分、不报环境已销毁、不启动后续 task/arm/repeat。停靠的线程仍由进程持有，可以在返回之后把探针写到 root 之外；该写入不是证据。丢弃 future 本身不产生 `TaskOutcome`，只置取消并停靠；若外层 `Cleanup` 仍 armed 且整个 run future 被丢，既有 `Drop` 仍可能删树，这与本次“返回未确认则 disarm”不是同一条路径。同进程里下一次独立的 `run_tasks` 不会被进程级毒化。`exec.rs` 本次只加了提前返回，现为 622 行，改前已超过 600 行上限，未做无关拆分。

验证（Linux，`RCARGO_HOST=build-laoxin`，一次一个 cargo）：本地 `. ~/.cargo/env; cargo fmt --all`。`rcargo clippy -p gcu-eval --all-targets --locked -- -D warnings` 与带 `--features gcu-eval/test-support` 的同一 clippy 均为退出码 0。`rcargo test -p gcu-eval --locked --no-fail-fast` 首次退出码 101：仅 `tests/fake_agent_boundary.rs` 的 `relocated_genuine_callers_never_bless_forged_sibling_backends` 在 spawn 时得到 `Text file busy`（os error 26）；其余目标通过，含三条新单测与 `fake_stall`。单独重跑该测试 3 通过。随后 `rcargo test -p gcu-eval --locked --features gcu-eval/test-support --no-fail-fast` 退出码 0；`rcargo test -p gcu-cli -p gcu-eval --locked --features gcu-cli/eval-autoapprove,gcu-eval/eval-autoapprove --no-fail-fast` 退出码 0（其中 `fake_agent_boundary` 再次 3 通过）。未改 `Cargo.toml` / `Cargo.lock`。未跑 `gcu-driver-linux` compositor。macOS / Windows 由总控验证。

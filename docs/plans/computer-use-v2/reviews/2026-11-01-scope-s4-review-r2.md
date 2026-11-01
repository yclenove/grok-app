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

Validation: repair diff whitespace check passed; read-only guard inspection plus 72 in-memory forbidden-string additions detected every listed mutation. No cargo, Rust tests, network/SSH/model calls, personal agent-home access, or source/test edits. Real helper launch/TCC/FD inheritance/real-model runs remain **NOT_RUN by design**, not findings. S12's pid read was not executed in this review; a failed read still fails closed, and guest multi-process plus Developer ID team id remain not_run. Only this report was written, uncommitted.

## 第 2 轮修复记录

Round-2 repairs on `cu2/scope-s4`. Findings above are unchanged. Commits: `8b28072a` (retirement owner), `fb71e992` (test-only atomic import), `caae0d44` (generation-gap admit), `57053842` (ACK body provenance), `67f8c5ec` (Windows artifact paths), `0953b879` (clippy needless return). Stop constants, fail-closed refusal, S6 `Command::new` placement, and dependencies are unchanged. A1–A4 were not attempted. Option (A) from the dispatch: each store owns retirement, including a synchronous caller.

### 原则 — 终态之后再扫一遍，所有者不依赖不可靠的唤醒

`gcu-mcp/src/retire.rs` 的 `drive` 先 `retire()`，再读 `lease.current()`。读到 `None`、收到 stop、或 epoch watch 关闭时，退出前再 `retire()` 一次。不能先扫、再看见终态、然后直接 break。`remaining()` 先 `expire()` 再采样；若两次读之间越过期限，它返回 0 且不发布。该分支在这个所有者上重新调用 `current()` 并回到循环，不等一个不会再变的 epoch。文件与图像各有自己的所有者，零剩余测试把它们分开跑。

没有当前 Tokio 运行时时，`start` 为该 store 起名为 `gcu-lease-retire` 的线程，线程内自建 current-thread 运行时（`enable_time`）。`Drop` / `close` 发 stop，并在 5 秒内 join；到点返回错误，joiner 线程被 detach，不无限等。线程或运行时起不来则 `watching` 为 false，现有 `FileAdmission::commit` / `ImageAdmission::commit` 不登记，staged 文件的 Drop 删掉字节，像素随 admission 丢掉。不新增第二个登记点。磁盘 unlink 仍在账本锁与租约锁之外。锁序仍是快照 → image cache → 文件账本 → 租约。ADR 0017 S4 行写了同一合同。

性质被破坏时会失败的测试（先断言路径不存在或 `resident` 为 false，再 get/export）：

- `revoke_between_sweep_and_terminal_check_still_drops_artifacts`：栅栏停在第一次扫描之后、`current()` 之前。此时文件仍在、像素仍驻留。放开前 revoke。去掉 `current() == None` 分支里的第二次 `retire()` 后，所有者仍退出，但断言 `terminal sweep must delete the file before export` 失败。
- `zero_remainder_each_store_publishes_expiry_itself`：文件单独一次，图像单独一次。`before_remaining` 处武装时钟：arm 之后第一次 `now()` 仍在期限前，其后在期限上，所以 `remaining()` 得到 0 且 `expire` 没有发布。把该分支改成 `pending().await` 后，约 2 秒失败于 `file owner must rescan a zero remainder without another store`（图像那一半还没跑）。
- `sync_bind_without_a_runtime_still_retires_before_get_or_export`：`Handle::try_current()` 为 Err。bind 两个 store，提交，revoke，等两个 owner 退出。没有 owner 时退出等待超时。

### 代次落后的数据面

`admit_data_rejects_when_admitted_generation_lags_the_lease`：平面仍对代次 1 `allows` 且未 suspend，租约已 rebind 到代次 2。新连接必须被拒绝，effects 不变；随后 `request()` 仍成功，说明拒绝不是因为平面已关。删掉 `admit_data` 里两处 `plane.allows(...)` 后，该测试在 `is_err()` 失败。

### ACK 体来自观察到的代码

`ack_body_carries_observed_helper_cdhash_not_the_expected_copy`：事实里的 helper cdhash 只比期望值多了大写。`normalize_cdhash` 使 admit 与 `accept_helper_ack` 仍通过。ACK 体必须带观察到的大写串。把编码器改成复制 `expected.helper_code` 后，断言失败（left 为小写期望，right 为大写观察）。

### Windows 路径

`artifact_paths` 先 `serde_json` 解析渲染文本，再按 `full text: ` / `Screenshot file: ` 抽取，用 `Path::is_absolute()`。不再要求以 `/` 开头，也不在 `\` 处截断。`artifact_paths_parse_json_strings_before_taking_absolute_paths` 的夹具在 JSON 里是双反斜杠。Linux 上 `/tmp/ses\sion/0.png`（一个反斜杠）必须被收进结果；`C:\session\0.png` 在 Unix 上不是绝对路径，被丢掉。`cfg!(windows)` 的分支要求留下 `C:\session\0.png`。把 `rendered_text` 改成原样返回后，该测试在 Linux 失败，抽出的字符串仍带转义。

### 仍是已知窗口

- 已经复制进响应的字节不回收。unlink 错误仍被忽略。
- 真实准入依赖 S12 的活体 SecCode 读取；读失败仍 fail-closed。真实 guest 内多进程与 Developer ID team id、helper 启动、TCC、FD 移交 not_run。A1–A4 未做。
- 源码隔离守卫未改，仍不覆盖 `scope/data.rs`。
- 线程或运行时起不来时不登记；这条失败路径没有注入测试，无运行时且线程成功启动的路径有测试。
- owner 一旦因终态退出，`watching` 仍为 true，同 store 上再次 `bind_lease` 不会再起线程。会话在撤销后不再 `commit_if_current` 成功，rebind 发生在租约仍活跃、owner 仍在循环里的时候。
- close/Drop 的 join 上限是 5 秒。超时后目录仍尝试删除，join 错误在删除成功时返回；卡住的 unlink 可能留在已 detach 的 joiner 上。
- macOS / Windows 由总控验证。老辛没有 node，`gcu-driver-linux` compositor 测试不作为本次结论。

### 验证（build-laoxin）

本地 `cargo fmt --all --manifest-path computer-use/Cargo.toml` 已跑。`RCARGO_HOST=build-laoxin rcargo clippy --workspace --all-targets --locked -- -D warnings` 退出码 0。同一 clippy 加上特性集 `gcu-cli/script,gcu-cli/eval-autoapprove,gcu-mcp/script,gcu-eval/eval-autoapprove,gcu-eval/test-support` 退出码 0。`rcargo test -p gcu-cli -p gcu-mcp -p gcu-core -p gcu-driver-macos --locked --no-fail-fast` 退出码 0。同一测试命令加上面的特性集，退出码 0；特性集下 `owned_js_worker_helper` 仍是既有 ignore。上述突变均在老辛上实际跑过并失败，然后已还原。macOS / Windows 未在这台构建机上跑。

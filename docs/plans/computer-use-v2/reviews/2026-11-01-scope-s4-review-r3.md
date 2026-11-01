**MERGE-AFTER-FIXES**

Final round 3 of 3, independent static review at `03b569e9`, limited to the seven round-2 repair commits and their affected contracts. Read both previous reports, including repair records, first. Paths below are relative to `computer-use/crates/`. Schedules and mutation sensitivity are source assessments, not executed reproductions.

**Must-fix, category (a)**

1. **[P1; SAME CLASS as round-1 item 3 / round-2 item 1] A departed owner still permits publication.** `gcu-mcp/src/retire.rs:65`, `:125`, `:290`; `gcu-mcp/src/files.rs:218`, `:224`, `:249`; `gcu-mcp/src/image_cache.rs:104`, `:107`, `:132`.

   Schedule: bind both stores and deliver generation-G artifacts under Tokio runtime R; retain the stores, then drop R. Tokio drops the retirement futures without a terminal sweep. `watching` remains true and `publish_blocked` remains false. A subsequent synchronous `bind_lease` of the **same still-active lease** returns early, so no dedicated thread starts; a valid `commit_if_current` can still register more artifacts. Revoke G and retain the stores without get/export/close: paths remain readable and pixels resident indefinitely. Even the artifacts delivered before R exited suffice. This concerns S4 artifact ownership, not the accepted S3 input-release scheduling residual.

   Normal terminal exit has a related reuse hole: revoke L1, let its owner exit, then bind a freshly installed L2 to the same store. The bound lease changes before the historical `watching` check; valid L2 commits succeed without an L2 owner. Rebinding the **original revoked lease** cannot commit and is harmless, but that restriction is not enforced on `bind_lease`. Current production Results retain one lease; the synchronous store contract does not enforce that restriction.

   Require owner lifetime/lease identity to govern publication: independently retain the owner, or retire/block on cancellation and exit and safely restart/refuse later binds. A thread panic likewise has no production exit guard; `join_bounded` also discards its panic result (`retire.rs:185`). Add retained-store/runtime-loss and terminal-owner/replacement-lease tests, checking paths/residency before lookup. The new tests cover successful owners only.

2. **[P2; NEW cross-owner deletion class] A timed-out close releases a pathname namespace still used by the detached worker.** `gcu-mcp/src/retire.rs:180`, `:196`; `gcu-mcp/src/files.rs:265`, `:270`, `:348`, `:355`.

   Schedule: the dedicated file owner takes stale absolute paths from directory D, then stalls/deschedules before an unlink. `close()` times out joining after 5 s, detaches the joiner/worker, drops D's owner lock and deletes D anyway. A later store obtains the now-free D name and creates the same numbered filename. The old worker resumes `remove_file(old_path)` and deletes the later store's valid artifact. Fresh temporary directory names are randomized, so reuse is conditional and unlikely; no identity check or retained directory ownership excludes it. The timeout error does not prevent the already-released name from being reused. This is different from an ignored failure to delete the original artifact.

   On timeout/joiner-start failure, retain D and its lock until pending unlinks finish, or anchor deletion to the original directory identity. Test a parked pre-unlink worker, bounded close failure, replacement at the old path, then release. Normal Drop's weak-upgrade/strong-callback lifetime prevents destroying the store during its own sweep; **explicit close after failed join** defeats that protection.

**(b) Residuals and repair checks**

- **SAME retirement class, repaired for a surviving owner:** `retire.rs:221`, `:231`, `:241`, `:249` sweep after stop, terminal `current()`, and watch closure. A zero remainder rereads `current()` and loops, publishing expiry without waiting for an unchanged epoch. The deterministic terminal barrier and separate file/image straddle-clock tests would fail if the final sweep or zero-remainder progress were removed. No further missed-wakeup schedule found while the owner remains alive.
- **SAME registration/locking contract, preserved:** `scope/commit.rs` and `result_commit.rs` are byte-identical to round 2. Snapshot → image cache → file ledger → lease order and the single stamp/register closure remain. `take_stale` releases ledger/lease locks before unlink. A first synchronous bind starts an `enable_time` current-thread runtime; thread/runtime startup failure sets `publish_blocked`, and both admission commits register nothing. Staged files/pending pixels then drop. That explicit failure path has no injection test; finding 1 concerns an owner disappearing after successful startup.
- **SAME test gaps from round 2, addressed:** `scope/data_tests.rs:86` leaves generation 1 admitted while the lease is generation 2; removing both `admit_data` generation comparisons makes its refusal assertion fail. It does not independently detect removal of either redundant comparison. `helper/scope_link_tests.rs:873` requires the observed uppercase cdhash in the ACK body; substituting the lowercase expected copy fails. Admission, live-code checks, post-flush checks and generation gating are unchanged. `auth.rs:476` accepts only 40/64 ASCII hex characters and lowercases them; different hash bytes cannot become equal through this normalization.
- **SAME portability/boundary checks, preserved:** JSON is decoded before path extraction; a Windows drive path is rejected as relative on Unix, while a literal Unix backslash survives unescaping. Test atomics and Unix-only imports retain matching cfgs; no new Linux/Windows unused-code defect found by inspection. No added `Command::new` anywhere in the repair diff; the thread builder is not a process launch. Stop constants remain **100 ms / 500 ms / 5 s / 4750 ms**.
- **SAME accepted limitations:** copied response bytes and ignored unlink errors remain; A1–A4 and accepted S1/S3 (b) residuals were not reopened. Real helper launch, TCC, FD inheritance, and real-model runs remain **NOT_RUN by design**. S12 reads live code identity by pid; real admission depends on that read and still fails closed when it returns `None`. Multi-process guest reads and a Developer ID team id remain not_run.

**Nice-to-have**

- **SAME test-helper coverage class:** `result_round6_tests.rs:572` still truncates paths at whitespace, `]`, quotes or commas, all legal in Unix filenames. A temporary parent containing those characters can make the existence check inspect an absent prefix. Compare extracted paths with the actual generated paths; the new backslash fixture fixes its targeted case, not these pre-existing delimiters.

Validation: repair diff whitespace check passed; relevant unchanged files compared byte-for-byte. No cargo, compilation/clippy, Rust tests/mutations, network/SSH/model calls, personal agent-home access, or source/test edits. Only this report was written, uncommitted.

## 第3轮处理

评审轮数已用尽（3/3），不再补丁观察者。r3 的两个必修都是同一类「所有者生命周期」缺陷，机制整段替换为租约回调。上面的发现保留，不改写成已通过。

退役由租约状态变化本身同步触发，不由会消失的所有者触发。`ScopeLease::on_invalidated` 接受 `Weak<dyn Fn() + Send + Sync>`。撤销、重绑，以及读者调用的 `expire()`，都在放下状态锁之后经 `publish` 同步调用仍存活的回调，并丢弃已死弱引用。文件账本和图像缓存在 `bind_lease` 各登记一个回调：取自己的锁，摘出本代次已失效的登记，放锁后 unlink 或释放像素。磁盘删除不在租约锁、也不在账本锁里。锁序仍是快照 → image cache → 文件账本 → 租约。登记仍只有 `commit_if_current` 里的那一个盖章闭包；撤销后该闭包不会成功。

已删除，不留第二套：观察者任务、`gcu-lease-retire` 线程及其自建运行时、`watching` / `publish_blocked`、`join_bounded`、超时后被分离的 joiner。回调里的 panic 由 `catch_unwind` 接住，不传播到撤销路径。unlink 错误仍忽略，不回滚。

回调先记清扫序号再 `try_lock`。序号小于等于本次在锁内读到的值，才在摘除之后标完成；读锁期间新发出的序号留到下一趟。锁被占用就返回，持锁方解锁后补扫。同一线程已持锁时不 `try_lock`。这是为了 `revoke_while_cache_admission_waits_does_not_deadlock_or_publish`：图像准入先拿缓存锁再停在测试闸门上，撤销若去等这把锁，闸门要等撤销返回，15 秒后断言失败。补扫最多八趟；序号若不再前进，剩下的票留到下一次解锁，不再空转。

r3 必修 1：运行时被丢弃后不再没人退役。回调挂在租约上，不挂在运行时或专用线程上。同一个仍有效的租约再次 `bind_lease` 会重新登记，不会因为 `watching` 仍为 true 而直接返回。换新租约 bind 只把回调登记到新租约上；旧租约撤销不会删掉新租约已经登记的文件或像素。store `Drop` / `close` 丢掉强引用，随后的撤销升不起来这条弱引用，也不会 panic。

r3 必修 2：不再有 5 秒 join 超时后分离、稍后 `remove_file` 别人目录的 worker。`close` 先清回调，目录名由 `Arc<TempDir>` 占住，进行中的 unlink 放下引用之后目录才消失。

性质被破坏时会失败的测试（先断言路径不存在或 `resident` 为 false，再 get/export）：

- `publish_runs_live_callbacks_outside_the_lock`：回调里调用 `current()`。若 `publish` 仍持有状态锁，同一线程死锁，watchdog abort。去掉 `fire` 后计数停在 0。
- `panicking_callback_does_not_skip_the_next_or_escape`、`dropped_callback_is_not_called_and_does_not_panic`、`rebind_publishes_and_renew_does_not`：续期不增加计数；时钟越过期限后要读租约才再计一次。
- `revoke_drops_files_and_pixels_before_export_or_get`、`rebind_drops_files_and_pixels_before_export_or_get`、`expiry_on_lease_read_drops_artifacts_before_export_or_get`：去掉登记或去掉 `fire` 后路径仍在。到期那条在读租约之前断言产物仍在。
- `renew_does_not_retire_delivered_artifacts`。
- `sync_bind_without_a_runtime_still_retires_before_get_or_export`：`Handle::try_current()` 为 Err。
- `dropped_runtime_then_revoke_still_retires_before_get_or_export`：运行时里 bind 并提交，丢掉运行时，再在没有运行时的情况下 bind 同一个租约并再提交。撤销后两条路径都不在。
- `second_bind_of_the_same_live_lease_still_retires_on_revoke`：再次 bind 时已交付的文件仍在；撤销后两条都删除。
- `replacement_lease_bind_retires_only_on_the_new_lease`：`run-a` 与 `run-b`。bind 新租约后旧路径已不在；旧租约撤销不删新文件；新租约撤销才删。
- `dropped_store_then_revoke_does_not_panic`：store 的 `Weak` 升不起来，撤销不 panic。
- `ledger_lock_holder_and_callback_do_not_deadlock`：另一线程持有账本锁时撤销。回调在 `try_lock` 之前发信号，持锁线程再 `current()`，解锁后补扫。握手 8 秒 `recv_timeout`，独立 watchdog 10 秒 abort。`try_lock` 之后，这条测试不再因为「回调仍在租约状态锁内」而 abort。那个破坏由 core 的 `publish_runs_live_callbacks_outside_the_lock` 抓住（回调里直接 `current()`，watchdog abort）。
- `delivered_image_is_unreadable_after_lease_expiry` 与 `delivered_file_is_unreadable_after_lease_expiry`：暂停钟拨过期限后、读租约前，像素和路径仍在；读过之后、get/export 之前才消失。

### 仍存在的限制

- 仅被动持有 store、没有任何人读租约时，到期退役延迟到下一次读租约。不为它再造定时器或线程。生产里 S3 引擎循环与 peer 复查会读租约，因此会发布到期。
- 撤销返回时，若另一线程仍持有账本或图像缓存锁，删除要等该守卫解锁后的补扫。图像准入会在持锁后停在测试闸门上，撤销不能去等这把锁。`resident` 只观察、不补扫，避免把「回调没跑」掩盖成已退役。
- 已经复制进响应的字节不回收。
- unlink 错误忽略，不回滚。
- 没有单独的测试把 unlink 停在 `remove_file` 之前再 `close`。目录名由 `Arc<TempDir>` 占住，不再交给分离的 worker。
- A1–A4、真实 helper 启动、TCC、FD 移交、真实模型仍是 not_run。活体 SecCode 读取已由 S12 实现；真实准入依赖该读取，读失败仍 fail-closed。真实 guest 内多进程与 Developer ID team id 仍 not_run。
- Stop 常量未改。无新 `Command::new`。无新依赖。S6 准入类型未动。


**MERGE-AFTER-FIXES**

Independent acceptance review of the replacement at `4d33ec94` (`b0758f0c`, `1dfe8bed`, `4d33ec94` after `e6771d90`). Read all three previous reports, including repair records, first. Paths below are relative to `computer-use/crates/`. Static review only; schedules and mutation sensitivity were not executed.

The replacement is **overall simpler**: runtime cancellation, worker startup, joins and detached joiners disappear. The lease-state critical section and original atomic registration remain intact. However, deferred cleanup is not yet a reliable retirement boundary, and synchronous cleanup introduces a new control-path dependency.

**Must-fix, category (a)**

1. **[P1; NEW read-admission regression] An export started after revoke can return stale bytes.** `gcu-mcp/src/files.rs:323`, `:425`, `:431`.

   Principle: deferred physical retirement must not permit a new read of an invalid epoch.

   Schedule: a file admission/stage holds the ledger with a delivered generation-1 file present. Revoke records a ticket, fails `try_lock`, and returns. A subsequent `export()` also fails its preliminary sweep and waits for the ledger. The holder releases its mutex, then is descheduled before `flush_retired()` (`:137` or `:91`). Export wins the mutex and copies every row without checking `visible`. Its eventual unlock may delete the file, but the returned bytes have already escaped. This is not the accepted read-admitted-before-revoke residual. Previously the preliminary sweep waited for the mutex.

   Revalidate/filter under the export lock before reading; unlink after unlocking. Add this exact holder → revoke → new-export handoff test.

2. **[P1; SAME retirement-liveness class as rounds 1–3] The eighth sweep can strand an invalidated row permanently.** `gcu-mcp/src/image_cache.rs:150`, `:164`, `:173`; analogous `gcu-mcp/src/files.rs:342`, `:358`, `:389`.

   Principle: a pending ticket needs a future executor, including when the last mutex holder is itself a sweeper.

   Concrete schedule: A sweeps a still-valid image. For each of passes 1–7, another same-live-lease bind finishes its unlocked callback-registration/request portion while A holds the cache mutex; its request advances the ticket and its `try_lock` fails. The bind's lease assignment occurs between A's passes. On pass 8, A reads the image as valid; then revoke changes the lease and its callback requests another ticket while A still holds the mutex. A marks only its earlier `seen`, unlocks, and exhausts the loop. `extract` has no unlock flush. All other callers have returned; requested > finished and the stale pixels remain forever without another operation. The ledger has the same capped-driver structure.

   Preserve bounded work while providing a guaranteed handoff for leftover tickets, or establish an equivalent progress invariant. Merely leaving them for an unspecified “next unlock” does not satisfy retirement without later get/export/close. Add a deterministic final-pass race.

3. **[P1; NEW synchronous-cleanup/locking class] Cleanup can run under outer output locks and delay input release.** `gcu-core/src/scope/model.rs:727`; `gcu-mcp/src/files.rs:351`; `gcu-cli/src/scope/mod.rs:290`, `:298`, `:478`.

   Principle: artifact cleanup must stay outside output critical sections and cannot obstruct emergency input release.

   Schedule: core observation holds the snapshot mutex (`gcu-core/src/action/observe.rs:352`) when `commit_if_current` calls `expire` (`scope/commit.rs:35`). Expiry invokes the file callback, which can unlink while that snapshot mutex remains held. Likewise, file-admission Drop flushes before the surrounding image admission and screenshot-delivery guards drop (`gcu-mcp/src/result_commit.rs:156`, `:183`). `Hold` tracks only the individual store, not those outer locks.

   Separately, control revoke calls the inline file callback before reaching `release_all_input`; cancellation Drop likewise revokes before scheduling release. A stalled filesystem unlink stalls that path, and a file callback also delays subsequent image callbacks. Eight passes bounds neither the number of unlinks nor syscall duration; there is no cleanup timeout. Defer disk work until all output locks are released and keep input release independent of it. Test parked cleanup against both lock release and input-release issuance.

4. **[P2; SAME ownership/lease-association class as round 3, new interleaving] Concurrent replacement binds can retain the wrong lease's callback.** `gcu-mcp/src/files.rs:298`, `:309`; `gcu-mcp/src/image_cache.rs:109`, `:120`; `gcu-mcp/src/retire.rs:36`.

   Principle: the bound lease and retained callback must change as one serialized association.

   Schedule: A assigns L1, then pauses before `arm`. B binds L2 completely and commits L2 artifacts. A resumes, replaces the strong L2 callback with its L1 callback, then sweeps using the still-bound L2, keeping those valid artifacts. Revoke L2: its weak callback is dead, so the artifacts remain indefinitely. Sequential replacement is correct; concurrent different-lease binds are neither serialized nor refused. This concerns the supported synchronous store surface; current Results use a fixed lease. Serialize/refuse conflicting binds and add a barrier test.

5. **[P2; SAME cross-owner namespace class as round-3 item 2] `Arc<TempDir>` does not preserve directory ownership against the orphan reaper.** `gcu-mcp/src/files.rs:45`, `:375`, `:452`; `gcu-host-protocol/src/owner_unix.rs:233`, `:236` (Windows equivalent: `owner_windows.rs:234`, `:237`).

   Principle: retain the pathname's ownership lock until every operation using that pathname finishes.

   Schedule: an unlink batch retains an aged directory D and pauses before `remove_file`. Concurrent `close()` drops D's `OwnerLock`, retaining only its `Arc<TempDir>`. A later store's normal orphan scan can now lock and rename D after the 60-second grace. If D is subsequently reused, the old unlink—and the old TempDir's eventual pathname-based deletion—can delete replacement content. Name reuse remains conditional, as in r3; an Arc is not an inode anchor. Retain the owner lock with the shared directory/batch. Add parked-unlink → close → reaper → replacement coverage.

**(b) Residuals and preserved guarantees**

- **Lock principle, SAME preserved lease exclusion:** every `publish` caller releases the lease state guard first: revoke, successful rebind, and deadline expiry. `current`/rebind/renew's second expiry checks also drop their guards before calling `expire`; `remaining` calls it before acquiring its read guard. The callback-list mutex is released before invocation. Read-only callback reentry into the lease therefore does not self-deadlock; nested expiry reaches a terminal state. This does not imply caller-owned output locks are absent (item 3).
- **Deferred-progress principle, NEW conditional contract:** same-thread lease reads under admissions really can publish. Each store's `Hold` branch records a ticket; FileAdmission/ImageAdmission Drop releases its mutex and Hold, then flushes. Ordinary AreaLock also flushes after unlocking. Request-before-`try_lock` plus SeqCst counters handles the ordinary missed-wakeup race: if the holder already completed its unlock/flush before the request, the callback can acquire the free mutex; otherwise an intervening holder owes the flush. Item 2 breaks that obligation for the final capped sweep. Constructed admission guards also flush on unwinding; panic containment alone does not guarantee completion of a panicked sweep. A guard deliberately retained forever cannot promise retirement. While pending, raw file paths remain readable and pixels remain allocated; cache `get` checks visibility, whereas export has item 1. The intended contract is retirement by completion of the holder's next unlock cleanup, not necessarily by revoke return; it is not established universally yet.
- **Arithmetic principle, NEW theoretical limitation:** `Sweep::mark` uses ordinary `<`, drivers use `>=`, and requests wrap (`retire.rs:114`, `:134`). After wrap, a high finished value suppresses new sweeps; store-id wrap can also alias Hold identities. There is no wrap/ABA proof. These require approximately 2^64 operations/allocations and are not practical blockers, but the guarantee assumes no wrap.
- **Expiry principle, NEW weaker replacement contract:** passive stores can retain files/pixels forever after the deadline if nobody reads the lease. Even `remaining()` can straddle the deadline: its initial `expire` sees pre-deadline time, then its second clock read returns zero without publishing. Production Machine's zero-remainder branches revoke; a standalone caller must read again/revoke. This deliberately loses the deleted owner's autonomous expiry progress. It is distinct from explicit revoke/rebind being stranded in items 2/4.
- **Weak ownership principle, SAME lifetime defects removed for ordinary/sequential use:** no runtime or thread owns retirement now. Stores own callback Arcs; callbacks capture weak stores; leases retain weak callbacks, pruning dead entries on push/fire. Store Drop releases resources even with pending tickets. A fire that already upgraded a callback may finish after clear/rebind; it reads the store's current bound lease. Sequential same-lease/new-lease rebinding works. Binding after revoke registers then requests an initial sweep, and valid commits are refused. Registration racing a publish is covered by that initial sweep. Two stores register independently. Callback panic is caught and later callbacks run; an indefinitely blocked callback is not contained by `catch_unwind`.
- **Atomicity/compatibility principle, SAME preserved boundaries:** `scope/commit.rs` and `result_commit.rs` are byte-identical to `e6771d90`; the single stamp/register closure and commit/revoke mutual exclusion remain. Watch publication and waker notification still precede callbacks, with no generation/watch ordering code changed. No new await, process launch, dependency, or `Command::new` was added. Stop constants remain **100 ms / 500 ms / 5 s / 4750 ms**; timing independence nevertheless has item 3. No new Windows/Linux cfg or unused/dead-code defect found by inspection. `files.rs` is **633 lines** (note only).
- **Accepted boundaries, unchanged:** copied response bytes are not recalled; unlink errors remain ignored. A1–A4 and accepted S1/S3 (b) residuals are not reopened. Real helper/TCC/FD/live SecCode/real-model work remains **NOT_RUN by design**.

**Tests and nice-to-have — SAME mutation-coverage class, with NEW replacement gaps**

The revoke/rebind/expiry/runtime-loss/replacement tests check paths or non-residency **before** get/export. Omitting callback registration or `publish`'s fire would fail them. Core's reentrant-current test has an 8-second abort watchdog; the busy-ledger test has 8-second handshakes and a 10-second watchdog. Other synchronous cases do not each have a watchdog. The cache-admission barrier covers another thread owning the cache, not same-thread deferred retirement of previously delivered pixels.

Existing tests do not force the final-pass, export-handoff, conflicting-bind, outer-lock/unlink, or close/reaper schedules above. Nor do they establish every claimed ticket/same-thread mutation: returning from the same-thread branch without recording a ticket needs a held-admission test with already-delivered artifacts; deleting the branch and falling through to `try_lock` may still pass on platforms returning WouldBlock. Add direct coverage. `resident()` correctly observes without sweeping in sequential assertions; it is a test-only raw mutex holder without an unlock flush, so concurrent use can itself strand a callback ticket. Keep that limitation explicit. The dead-weak test proves failed upgrade, not removal of the list entry itself.

Validation: source diff whitespace check passed; the full replacement diff flags only the existing added EOF blank line in the r3 report. No cargo, compilation/clippy, Rust tests/mutations, network/SSH/model calls, personal agent-home access, or source/test edits. Only this report was written, uncommitted.

## 第4轮处理

上面的 5 个必修都针对租约回调和延迟清扫。该机制已整段删除，这些调度不再有对应代码，不把原文改写成已通过。

`gcu-core` 回到第 1 轮基线：没有 `on_invalidated`，`publish` 不再调用回调。`gcu-mcp` 没有 `retire.rs`、清扫序号、`try_lock` 补扫或 `Arc<TempDir>`。统一原则：撤销之后不再提供；清理只发生在访问时和关闭时。没有后台所有者、回调、线程、任务或运行时。

1. 撤销后新的 `export` 读到旧字节：导出不再先做一次可能失败的预扫、再在锁里无过滤地复制。`export` / `get` / `cached_image` / `export_private_files` 在持有 store 锁时按 `visible` 过滤，只复制仍属当前代次的行；失效行在同一次临界区摘出，unlink 在放下 store 锁之后。持锁方释放后、旧行还在账本里时抢到锁的导出，仍然不能返回旧字节。去掉 `take_stale` / `take_invisible` 里的 `visible` 判断，对应测试会拿到旧字节。
2. 第八趟清扫把票据留到永远：没有清扫趟数，也没有留给“下一次解锁”的票据。无人再调用 get/export/close 时，行就留着。这是写进 ADR 的限制，不是漏掉的交接。
3. 清理跑在外层输出锁里、排在释放输入之前：撤销、重绑和到期不再调用文件或图像回调，因此不会在快照互斥量里 unlink，也不会挡在 `release_all_input` 前面。磁盘删除只在 store 自己的 get/export（放锁之后）或 `close`/`Drop` 里发生。core 的发布路径不再挂回调。
4. 并发 bind 留下错误租约的回调：没有回调可登记。`bind_lease` 只在 store 锁里保存租约。get/export 读的是当前保存的那一份。后一次 bind 覆盖前一次。撤销一个已经不再被 store 持有的租约，不会删掉新租约的行。
5. `Arc<TempDir>` 保不住目录所有权：目录不再放进 `Arc`。`close` 和 `Drop` 在所有者锁和目录对象都还在时 unlink 已登记文件，Unix 上接着在放锁之前删掉目录。没有在放锁之后继续 `remove_file` 的 worker。Windows 上打开的 `.owner-lock` 会挡住删目录，所以只在这些文件 unlink 完成之后才放锁并重试删目录；不会先放锁再删已登记文件。

仍写明的限制：撤销或重绑之后、下一次访问或关闭之前，原始文件路径在磁盘上仍可读，像素仍驻留内存。这与已经复制进响应的字节不回收同类。被动持有且无人访问的 store 不会自行清理。一次性虚拟机在每次评测结束时整台销毁。unlink 错误忽略，不回滚。A1–A4、活体代码身份读取、真实 helper/TCC/FD/SecCode、真实模型仍是 not_run。Stop 常量未改。无新 `Command::new`。无新依赖。S6 准入类型未动。

会失败的测试（整体有 abort watchdog；握手用通道，不用 sleep）：

- `export_after_a_lock_holder_releases_does_not_return_revoked_bytes`：持锁方不摘行就放锁，导出在此之前已宣布自己要抢锁。先断言路径不存在，再断言没有旧字节。去掉可见性过滤会返回 `old-bytes`。
- `get_after_a_lock_holder_releases_does_not_return_revoked_pixels`：同样的交错。去掉可见性过滤会返回像素。
- `revoke_then_export_and_get_reject_without_returning_old_bytes`、`binding_a_replacement_lease_hides_the_previous_epoch_on_access`：撤销或换租约 bind 之后、访问之前路径和像素都还在；访问时摘掉。先断言路径不存在，再断言被拒绝。旧租约撤销不删新租约的文件。
- `close_deletes_every_registered_file_while_the_owner_lock_is_held` 与 `drop_deletes_every_registered_file_while_the_owner_lock_is_held`：探针在放锁和删目录之前观察。去掉 Drop 的 purge 后 `ran` 为 false；只靠 `TempDir` 的字段析构删目录不算通过。先放锁再 unlink 时 `lock_held` 为 false。

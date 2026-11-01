**MERGE-AFTER-FIXES**

Round 1, independent static review of `cu2/scope-s4` at `96fcf873`, against `origin/feat/cu-v2` (`bc2a2810`). Reviewed the three S4 commits, merge result, ADR 0017 §2/§3/§5 including S1–S6, S3 r4, and S6 r4/r5 with repair records. Paths below are relative to `computer-use/crates/`. Schedules and test sensitivity are source assessments, not executed reproductions.

**Must-fix, category (a)**

1. **[P1; NEW S4 authentication class] Code identity is claimed, never observed.** `gcu-cli/src/helper/scope_link.rs:238`, `:266`, `:296`, `:327`; `gcu-driver-macos/src/scope_os/facts.rs:122`.

   Principle: authenticate the current process instance **and its current code** from trusted facts at admission, ACK, and every recheck. `verify_instance` re-reads boot/start correctly, but `verify_code` compares only two supplied `CodeWire`s. `ProcessFacts` cannot provide code identity. On recheck, the code comparison is literally expected-versus-expected; ACK encoding copies the expected helper code and checks that copy.

   Schedule: agree on instance P and code C; P changes executable while retaining its process tuple, or its live signature becomes unavailable. Keep the hello/ACK's claimed C unchanged. Boot/start checks pass, admission/ACK/rechecks still accept, and data remains available. Neither a different live cdhash nor a failed code query can influence this implementation. This is an offline authentication-interface defect, independent of the intentionally NOT_RUN launch/FD/TCC work and the accepted macOS window races.

   Require an injectable, fail-closed live code observation bound to the checked peer instance, and compare identifier, team identity, and cdhash against expectations on both sides. Add missing/changed-code tests with unchanged wire claims, including changes while ACK is pending. The current forged-wire-cdhash test (`scope_link_tests.rs:323`) does not exercise this property.

2. **[P1; S3-related ACK/liveness class, newly exposed in S4's helper contract] Rebind activates authority before its ACK, and an ACK write suspends peer monitoring.** `gcu-cli/src/scope/mod.rs:263`, `:360`, `:362`; `gcu-cli/src/scope/state.rs:142`; `gcu-cli/src/scope/data.rs:67`.

   Principle: usable data authority requires a flushed ACK for that generation and a matching peer; control backpressure must not suspend revocation checks. After the initial ACK, the latch stays open throughout rebind. `machine.apply` publishes generation 2 before awaiting its ACK write/flush.

   Schedule: establish generation 1; send a valid rebind admitting B; block the second ACK's write or flush. The watcher yields, so the outer select continues polling data. A **fresh** connection arms generation 2 and can use B before its ACK completes. This does not reuse an old connection's authority: S3's fixed handler-generation checks remain intact. The offline `ReadyLink` composition likewise exposes generation 2 to its serving future while `DataPlane::request()` succeeds. During that blocked ACK, removing the supervisor from injected facts also goes unnoticed until write completion/lease timeout: the 50 ms recheck exists only around reads.

   Keep the new generation unusable until ACK completion plus peer verification, and maintain peer/deadline monitoring during ACK writes. Gate actual data admission, not just the diagnostic latch. Add a barrier-controlled second-ACK test, with fresh data admission, peer loss, flush failure, and input-release assertions. Existing tests cover initial blocked writes and stale rebinds, not this schedule. Earlier S3 reviews accepted initial ACK ordering; they did not accept this rebind gap.

3. **[P1; S1-related retirement work explicitly deferred to S4] Revocation/rebind does not retire delivered files until someone exports or closes.** `gcu-mcp/src/files.rs:231`, `:239`, `:249`; `gcu-mcp/src/image_cache.rs:94`, `:119`; `gcu-mcp/src/result_commit.rs:54`, `:85`; `gcu-mcp/src/server.rs:395`.

   Principle: the epoch stamped at the existing atomic commit owns the artifact's subsequent lifetime, including its published file reference. Current cleanup is demand-driven; there is no lease-invalidation retirement owner.

   Schedule: legitimately commit an overflow/script/details file and deliver its path p under generation 1. Keep the scoped Connection/server alive, then rebind to generation 2 (or revoke/expire), without calling export/close. The sealed file remains at p and a filesystem read still retrieves its bytes. The stamp protects `export()`, not the already-published path. Cached pixels likewise stay allocated until get/admission/close. This concerns the retained server artifact, not erasing bytes already copied into a legitimate response. The direct scoped Connection surface cannot rely on the CLI socket watcher to close it.

   Arrange invalidation-driven retirement, with disk cleanup outside the lease critical section and the documented lock order; preserve the original `commit_if_current` registration/stamp closure. Do not add a later registration point. Test a held-open server after revoke/rebind/expiry and check p/deletion **before** invoking export. Current deletion assertions run export first (`lease_cache_tests.rs:137`; `result_round6_tests.rs:400`, `:548`), thereby triggering the cleanup they purport to verify.

4. **[P2; NEW portability regression] Windows test compilation has an unused trait import under `-D warnings`.** `gcu-cli/src/helper/scope_link_tests.rs:21`, `:237`, `:255`.

   Principle: platform-specific test dependencies must have the same cfg as their consumers. `AsyncReadExt` is imported for every test build; its only use is `.read()` in the Unix-only frame-reader test. The Windows all-targets clippy check therefore rejects the import. Gate that import with `#[cfg(unix)]` inside this test-only module; any separately defined Unix-only test helper needs `#[cfg(all(test, unix))]`. This is a source-derived build failure; no compiler was run.

**(b) Residuals and preserved boundaries**

- **Authority principle: only private control can change scope.** The supplied control stream is the sole hello/Install source; MCP/data receive no Machine. Strict decoding and S3's terminal transitions reject missing/unknown versions, bad digest, replay, old generation and sequence. Ordinary/scoped handshakes mutually refuse; the ordinary Frame parser independently rejects a scoped hello. No new model-reachable install/renew/widen route found. S3 history preserved.
- **Artifact commit principle: one atomic registration, with its producing epoch.** `result_commit.rs:165` stamps and registers images and every created file inside `commit_if_current`; snapshot → cache → ledger → lease order is retained. Image, batch JSON, overflow, discovery and script spills converge there; no production unstamped registration bypass or second commit point found. Sequential get/export after invalidation hides stale entries and export attempts deletion. A read already admitted before revoke can finish afterwards (`lease_view.rs:16`, `image_cache.rs:125`, `files.rs:255`); no stronger quiescence guarantee is established. Unlink errors are ignored after removing ledger rows. These are S4 lifetime limitations; finding 3 concerns a subsequent raw-path read with no retirement at all.
- **Cancellation principle: terminal failure owns revocation and input release.** Once `EngineOwner` exists, error exits converge on revoke/release and future drop revokes synchronously then schedules release (`scope/mod.rs:276`, `:404`). Scheduling still depends on a live runtime, as in S3. The new first-hello wait precedes that owner (`scope_link.rs:139`): dropping it skips release, but the current entry has not acquired scope or input there; no active-input bypass demonstrated. Extend cancellation coverage before generalizing this entry to native resources.
- **Admission principle: executable construction consumes `AdmittedLaunch`.** S4 adds no `Command::new` or process launcher; the added `runtime.spawn` schedules input cleanup only. No call into ordinary helper launch/attach was found. S6's default-build capability/provider refusal and test-support gate are unchanged; no A1 recurrence.
- **Compatibility principle: scoped additions preserve ordinary behavior and Stop.** Ordinary dispatch/Host/store paths are unchanged apart from the equivalent socket-path helper and stricter scoped-frame rejection. Constants remain 100 ms / 500 ms / 5 s / 4750 ms. No Linux production dead-code issue found by inspection; Windows issue is finding 4. Renamed S4 test names have no stale references in the searched docs. Runtime equivalence/builds remain unverified.

**Nice-to-have**

- **Store-isolation principle: enforce forbidden dependencies, not self-reported touches.** `identity.rs:124` counts only explicit calls; `scope_link.rs:132` fabricates three scoped touches before checking `< 3`, and production never increments Ordinary. Thus the ledger **asserts**, rather than enforces or measures, zero ordinary-store I/O. Adding an unrecorded `crate::config::load()` after the check would leave these totals/tests green. Current call-tree inspection finds no such I/O, so the missing structural regression is **nice-to-have, not a demonstrated category-(a) violation**. Add a source guard like S6's `command_new_is_only_in_the_admission_module`: prohibit scoped modules from referencing ordinary config/Host loaders, Stop/auth-store constructors, helper launch/attach/run entry points, and ordinary socket-opening functions; explicitly allow identity.rs's pure path comparisons and CodeSignature value comparisons. A scoped-store capability API would enforce this more strongly. NEW S4 test-enforcement gap, related in approach to S6's structural admission guard.

**Mutation-style coverage assessment** — inspection only:

| Claimed property | Would existing tests fail when broken? |
| --- | --- |
| Mutual refusal | Yes for accepting ordinary input at scoped admission, or admitting scoped input through the ordinary frame reader. Removing only the redundant explicit ordinary-reader guard stays green because strict Frame decoding still refuses it. |
| Mutual authentication | Process disappearance/start changes and a changed wire cdhash are covered. Live-code changes/fact failures with unchanged claims are unrepresentable and undetected (finding 1). Boot/team-field mutation coverage is incomplete. |
| ACK before data | Initial blocked-write ordering is covered by `data_factory_is_not_polled_until_ack_is_fully_flushed`. Duplex flush completes immediately, so deleting flush need not fail it. Rebind-before-ACK and peer loss during ACK are undetected (finding 2). |
| Revoke + release_all_input | Removing normal release fails EOF/expiry assertions; removing Drop release fails S3's cancellation test. S3 runtime tests also assert held input is zero. The S4 helper tests alone do not cover future drop; pre-hello cancellation and blocked-ACK peer loss are not covered. |
| Retirement at the same commit | Production finish tests cover precommit invisibility, revoke exclusion and valid-output controls; stamp omission breaks positive retrieval. Hand-stamped cache tests do not prove stamp placement. No test fails for lookup-only retirement (finding 3); no raw-path-before-export assertion exists. |
| Zero ordinary-store touches | No for an uninstrumented ordinary-store call. Tests prove counter arithmetic and the hardcoded three touches, not actual I/O isolation. |

Validation: `git diff --check bc2a2810...HEAD` passed. No cargo, compilation, test/candidate/mutation execution, network, SSH, model calls, desktop operations, personal agent-home access, or source/test edits. Real helper launch, TCC, FD inheritance and real-model runs remain **NOT_RUN by design**, not findings. Only this review was written, uncommitted.

## 修复记录

Round-1 repairs on `cu2/scope-s4` after this review. Findings above are unchanged. Each principle is its own commit: `d132bba0` (P1), `ca99c951` (P2), `187db283` plus `03d29e7a` and `ee5bf926` (P3), `f68dfdda` (P4), `ed42026f` (P5). `2c795806` keeps the test-only harness off the library clippy target. Stop constants, fail-closed refusal, S6 `Command::new` placement, and dependencies are unchanged. A1–A4 were not attempted.

### 原则一 — 当前代码身份来自事实，不是声称

`ProcessFacts::code_identity`（`gcu-driver-macos` 的 `scope_os/facts.rs`）在 admit、ACK 编码、ACK 校验和每次 recheck 读出该进程实例当前的 identifier、team、cdhash，并与期望比较。`None` 或读失败一律拒绝。真实访客 SecCode 读取由 S12 的 `LiveOsFacts` 按 pid 实现。真实准入依赖该读取；读失败仍返回 `None` 并拒绝，不用路径上的 `SecStaticCode` 伪造成功。真实 guest 内多进程与 Developer ID team id 仍 **not_run**。`ScriptedFacts` 可以在握手之后改代码或丢掉代码读取。

性质被破坏时会失败的测试：`unchanged_wire_live_code_is_refused_at_admit_ack_and_recheck`（线上声称不变，事实里的 cdhash、team、identifier 变化或读取失败，在 admit、ACK 待写、recheck 三处撤权并 `release_all_input`）；`unchanged_ack_wire_is_refused_when_live_code_changes`。原有只改 wire cdhash 的断言留在 `old_generation_replay_and_code_mismatch_revoke`。若把缺失观察当成通过，或只比较 wire，这些测试失败。

### 原则二 — 该代次 ACK 写完并 flush 且对端仍匹配之后，数据才可用

首次 install 与每次 rebind 都一样。`DataPlane` 只在 `open_generation` 之后对该代次 `allows`；`admit_data` 在不匹配时拒绝，且不 `arm_attempt`、不增加 effects。`write_acked` 在写和 flush 期间按 50ms 复查对端，并监视租约剩余时间。ACK 写失败、flush 失败或对端消失则撤权，外层 `release_all_input`。租约代次仍在 `apply` 时立即发布，不回退 S3「旧连接权威不能用于新代次」。库目标里只被测试调用的 `run_controlled` 和 `admit_data` 再导出是 `cfg(test)`；`confirm` 直接接收租约引用。

性质被破坏时会失败的测试：`rebind_data_stays_closed_until_the_ack_flushes`（栅栏卡住第二个 ACK 时，新连接 `admit_data` 被拒、effects 不变；不放开栅栏就摘掉 supervisor，会被发现并释放输入）；`rebind_ack_flush_failure_releases_held_input`（flush 失败后持有输入为 0）；`data_factory_is_not_polled_until_ack_is_fully_flushed`；`fresh_accept_uses_the_admitted_generation_gate`（源码必须调用 `admit_data(&plane, &lease)`，`arm_attempt` 恰好两处）。去掉门控、或在 flush 完成前 `open_generation`，这些断言失败。`rebind_new_generation_invalidates_old_attempt` 仍要求 `apply` 后 generation 立即为 2。

### 原则三 — 已交付产物由租约失效退役

`gcu-mcp/src/retire.rs` 在 `bind_lease` 时武装一个 tokio 任务。撤销、重绑或到期后删除已登记文件并释放缓存图像。任务在 `subscribe` 之后先退役一次，避免发布发生在任务被 poll 之前就丢掉。磁盘清理在缓存或文件锁与租约锁都放下之后做。锁序仍是快照 → image cache → 文件账本 → 租约。登记仍只有 `commit_if_current` 里的盖章闭包。没有运行时的同步调用不置位，稍后的运行时仍能启动任务。

性质被破坏时会失败的测试（连接保持打开，**先**断言路径或驻留，再 get/export）：`delivered_file_is_unreadable_after_lease_expiry`、`delivered_image_is_unreadable_after_lease_expiry`（`resident` 不回收）、`rebind_deletes_artifacts_while_the_session_stays_open`、`revoke_before_commit_hides_the_spill_and_revoke_after_commit_reclaims_it`、`revoke_after_image_commit_hides_the_published_batch`。失效后若文件还在或像素仍 `resident`，测试失败。先调用 export/get 会自己清掉，所以不能用来代替这些断言。到期前的那一次 export/get 只证明产物曾经存在。

### 原则四 — 平台专有导入与其使用者同一个 cfg

`scope_link_tests.rs` 里 `AsyncReadExt` 改为 `#[cfg(unix)]`。它只被 Unix 帧读取测试的 `.read()` 使用。`HeldFlush` 的 `AsyncRead` / `AsyncWrite` / `ReadBuf` 仍是全平台。本轮新增代码里没有另写只在 Unix 使用、却缺少 `cfg(all(test, unix))` 的辅助函数。

去掉该 cfg 后，Windows 的 `clippy -D warnings` 会把未使用导入当成错误。Linux 上该测试会编译，所以本机构建看不出这个问题。

### 原则五 — 隔离由源码守卫强制，不是计数

删掉 `run_helper` 开头对自己 touch 三次再检查总数的伪检查，生产路径不再接收 `StoreLedger`。`StoreRole` / `StoreLedger` 仅 `cfg(test)`，计数测试只证明计数器算术。`scoped_modules_do_not_reference_ordinary_helper_stores` 用与 `command_new_is_only_in_the_admission_module` 相同的源码包含扫描，检查 `helper/scope_link.rs`、`helper/identity.rs`、`scope/mod.rs`、`scope/plane.rs`。注释也算。禁止 `config::load`、`LocalHost::new`、`open_local_host`、`stop_reason::state_dir`、`stop_reason::write_marker`、`private::state_dir`、`policy_store::`、`HostPolicy`、`launch_helper`、`launch_for_onboarding`、`run_agent`、`run_stdio_proxy`、`client_attach`、`start_listener`、`UnixListener::bind`、`UnixListener::from_std`、`std::os::unix::net::UnixListener`、`launch_command`。`identity.rs` 的纯路径比较和 `CodeSignature` 值比较不在清单里。ADR 0017 S4 行已改为「隔离由源码守卫强制，不是计数」，并写明观察代码身份、ACK 后才放行数据、失效退役。

在上述四个文件里加入任一禁止符号（包括注释）会使该测试失败。`scope/data.rs` 不在扫描内，因为 S3 引擎宿主在那里 `open_local_host`；也不把 `TcpListener::bind` 当成普通 socket。

### 仍是已知窗口

- 真实准入依赖 S12 的活体 SecCode 读取；读失败仍 fail-closed。真实 guest 内多进程与已签名 Developer ID 应用的 team id 仍 not_run。
- Tart 内真实 helper 启动、TCC、真实 FD 移交与 ACK、真实模型评测仍 not_run。A1–A4 未做。
- 已经复制进响应的字节不回收。unlink 错误被忽略。撤销前已经放行的读仍可能做完（评审残余，未扩大）。
- 源码扫描不覆盖 `scope/data.rs`，也不覆盖 scoped MCP 的 loopback `TcpListener::bind`。
- macOS / Windows 由总控验证。老辛没有 node，`gcu-driver-linux` compositor 测试不作为本次结论。

### 验证（build-laoxin）

本地 `cargo fmt --all --manifest-path computer-use/Cargo.toml` 已跑。`rcargo clippy` 的 `-D warnings` 必须写在 `--` 后面。工作区 clippy 与特性集 clippy（`gcu-cli/script`、`gcu-cli/eval-autoapprove`、`gcu-mcp/script`、`gcu-eval/eval-autoapprove`、`gcu-eval/test-support`）退出码都是 0。`rcargo test -p gcu-cli -p gcu-mcp -p gcu-core -p gcu-driver-macos --locked --no-fail-fast` 以及同一命令加上面的特性集，退出码都是 0。特性集下 `owned_js_worker_helper` 仍是既有 ignore。macOS / Windows 未在这台构建机上跑。

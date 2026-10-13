# U01-R 核心授权返修 checkpoint

状态：core 合同已实现并完成本批验证，交 Codex 审查。不是 Computer Use 交付。未开始 U02。未 commit / push / PR / merge / tag / release。未改独立 snapshot crate，未重跑 P0 CLI，未跑 UI 全套。`src-tauri/src` 无 diff。

基线仍是 `8d8b71c262ab222748bca32412eea884d4b892e0`，分支 `feat/computer-use-implementation`。测试 Bound 只来自 `#[cfg(test)]` synthetic helper，decision class 为 `synthetic-host-not-a-human-or-production-grant`。没有生产 Bound 构造器。

实现和验证分开。先红日志原样保留。下面的绿命令是修完之后新跑的。

## 源码与运行时

领域代码在 `src-tauri/computer-use-core/src/task_consent/`（bounds、grant、mod、owner、parse、prepare、request、ticket、types）。旧的单文件 `task_consent.rs` 已删除，旁边没有同名文件。Broker 入口是 `broker/consent.rs`（16600 字节）。最大领域文件是 `types.rs`（16609 字节）和 `owner.rs`（15892 字节）。

完整 SHA-256 在 `tools/computer-use-probe/.run/task-consent-u01-20261003/hash-u01r-final.txt`。关键值：

| 对象 | SHA-256 | 字节 |
| --- | --- | --- |
| `task_consent/prepare.rs` | `87A20908608B808C3B58DB48420CCA5D4A9A60F597E5A56B701F718CF59F7D91` | 14011 |
| `task_consent/owner.rs` | `B6CD9784AAFA8C92B327D59FDEAF0C63B257BEA0521149DE27B2A983209A00EC` | 15892 |
| `task_consent/grant.rs` | `32FCF4E64D246465887F1701F54F20219B1A8BB44EB2FF020CD7652450A4BB7A` | 13426 |
| `task_consent/ticket.rs` | `73F5979DFC3A96EB933DC2CD9278B33EEEA995034D8F98C303DF19341AF15659` | 7325 |
| `broker/consent.rs` | `7AE0B5361309618B3247824BFB7DD5CCB9B83F9E5F174DB7F2A2112DEF7AC996` | 16600 |
| 通过套件的 lib test 二进制 `grok_computer_use_core-469532688ae03771.exe` | `83E1EDEFDCD0A0790170696A4EC4AD5BD3F62CCB45C1D516F74122D4D14E8F98` | 23584256 |
| 先红日志 `red-u01r-authority-2.txt` | `4FB8E7CE8C91D03DE3BFDE47C76CE1E7E70BB90BD56563129EA2945F11AAA51A` | 8224 |
| 修订后 ADR | `95A95F8EAE10B8461E2BDCD24EB6347B154928B4A608E3B6A0DD20D276FF61ED` | 37738 |

Cargo 在 `src-tauri` 下执行。日志里的 `EXIT=` 是 `$LASTEXITCODE`。wrapper 的 0 不代替这个值。

## 先红

行为红：`tools/computer-use-probe/.run/task-consent-u01-20261003/red-u01r-authority-2.txt`。

命令是当时的 `cargo test -p grok-computer-use-core --offline --lib task_consent_rework`。结果 `0 passed; 13 failed`，`EXIT=101`。失败是断言，不是编译失败。当时看到的事实包括：准备期 status 已是 `approved`；失败回滚后的 epoch 断言未成立；同一 intent 改能力后 live count 左 2 右 1；捕获的旧 owner context 仍被接受；未绑定 agent 被接受；增量请求打掉了 A；reservation 超过上界；有效 continuation 被 flood 丢掉；最后检查后的 Stop 仍留下可执行结果；Stop 回执被慢 abort 堵住；retire 后 owner 槽被复用；grant 查到后的 agent 替换仍然派发了 observe（`isError` 为 false）。

独立 snapshot 未改、未重跑。它的 `red-authority.txt` 仍是 1 passed / 7 failed、Cargo 101。那份旧 A 来自错误 prepare。本批回归里的 A 都先走完整 Host commit。

编译红（不是行为红）：`compile-u01r-check-1.txt`，`EXIT=101`。五处类型错误：`publish_feature` 同时借用 consent 和 feature 字段，`PrepareParts` 的 `Result` 推不出错误类型，`authorize_desktop_window` 返回 `Result<u64, _>`。

锁红（不是通过）：`green-u01r-consent-1.txt`。24 条已过，四条 barrier 超过 60 秒仍在跑，进程被终止。原因是 `if let Some(hook) = self.inner.lock()...` 把 broker 锁留到 hook 体内，hook 再取同一把锁。文件末尾记了 `EXIT=killed`。这不是绿。

## 验证命令

| 日志 | 命令 | 结果 |
| --- | --- | --- |
| `green-u01r-consent-2.txt` | `cargo test -p grok-computer-use-core --offline --lib task_consent -- --test-threads=8` | 28 passed，0 failed，`EXIT=0`。当时 hook 已先取出再调用。随后只做了 rustfmt，全套件覆盖同一批测试 |
| `core-suite-u01r-1.txt` | `cargo test -p grok-computer-use-core --offline` | lib `618 passed; 0 failed; 0 ignored`，31.34s。其余 target 的 `test result` 都是 ok，含 0 测试的 target。全文无 `FAILED`。`EXIT=0` |
| `clippy-u01r-5.txt` | `cargo clippy -p grok-computer-use-core --all-targets --offline -- -D warnings` | `Finished`，无 warning，`EXIT=0`。中间三次 `clippy-u01r-1` 到 `3` 是 `EXIT=101`，已保留 |
| `protocol-golden-u01r-1.txt` | `node --test tools/computer-use-mcp/protocol.golden.test.mjs` | 5 pass，0 fail，`EXIT=0`。`MODEL_TOOLS` 本批没有改合同 |
| rustfmt | `rustfmt --edition 2021 --check` 覆盖本批 Rust 文件 | `FMT_CHECK_EXIT=0`。`lifecycle.rs` 与 `lib.rs` 另一次 rustfmt `FMT_EXIT=0`，在全套件之前 |

未跑 UI 全套。未重跑 CLI / live negative / P0 oracle。

## R1 准备与 commit

实现：`start_preparation` 只把请求标成内部 Approving，不写 Approved、grant 或 continuation。`publish_feature` 在锁内按值返回新的 bool 和 epoch。绑定失败或复验失败走 `abort_preparation`：只有这张准备仍拥有的迁移才改 feature，并把 epoch 设成 `owned_epoch + 1`。0→开成 1→失败回关后 epoch 大于 1。用户已经把 epoch 推进走时，回滚不覆盖他们的 bool。本 commit 新建的 run 在授权前标成 `ConsentGate::Pending`。Stop 按准备上的 run id fence，不扫 `cu-task-` 前缀。hook 在放下 broker 锁之后才运行。

验证：`preparing_status_grant_and_ticket_stay_empty` 在 after-enable barrier 读到状态不是 approved，grant 0，continuation 0，claim 为空。`failed_bind_advances_epoch_past_one`：`missing-window` commit 失败，feature 关，epoch > 1，grant 0，continuation 0，`list_calls` 0，`observe_calls` 0。`stop_after_last_check_and_after_bind_publish_nothing` 的前半：最后检查前 Stop，commit 失败，`test_run_count` 0，grant 0。后半：绑定后 Stop，commit 失败，grant 0，continuation 0，`observe_calls` 0。`second_approval_during_prepare_is_not_success`：第二个 commit 不是成功。`stop_during_prepare_and_old_epoch_do_not_enable_or_roll_back`：before-enable 的 Stop 不开 feature；用户先开 feature 后，旧准备不把它关回去。`duplicate_request_and_commit_happen_once`：重复批准一份 grant、一次 continuation，第二次 consume 失败。

这些测试都在 `green-u01r-consent-2.txt` 和全套件里为 ok。后半 Stop 会 fence 本 commit 打开的 run 并交出 cleanup 票据；`consent_stop` 不在返回前 `finish_stop_cleanup`。物理 join 的证据在 R4 的慢 abort 测试，不在这条 Stop 断言里。

## R2 owner 与 agent

实现：owner nonce 单调，从 1 起，不复用。retire 留 tombstone，不把槽删掉再从 1/1 发。`require_current_owner` 比对 nonce、incarnation、connection generation。`None` agent 不能 submit、准备、finalize 或执行。第一次 `bind_agent` 原地写入 token 上的 agent，不增 incarnation，并丢掉空 agent 的 grant，返回的 run 列表为空。同一 agent 再登记不轮换 token。换 agent 或 `rotate_connection` 增加 nonce，退役 token、准备、执行和 continuation，按确切 run id fence。

验证：`unbound_agent_cannot_request_or_commit` 在 bind 前 submit 失败，grant 0，continuation 0，scope memory 空。`captured_owner_context_dies_after_reissue` 保留旧 `TransportContext` 对象，retire/reissue 和同 agent 重绑后 request、status、cancel、reserve、execution 都拒绝。`approved_memory_does_not_execute_for_the_next_agent`：记忆还在，grant 0，新 agent 的 `execution_run` 失败，executions 空。`retire_does_not_free_a_global_owner_slot`：满额后 retire 再签发仍然失败。IPC `feature_off_request_without_run_or_target_returns_pending`：同一 owner 两次 issue 的 token 字符串相同。未绑定 agent 时 HTTP `computer_request_access` 是 200 且 `isError` true，grant 0。绑定 `synthetic-agent` 后，同一 token 的 request 为 pending；synthetic commit 后 status 为 approved，HTTP observe 的 `isError` 为 false，`observe_calls` 为 1。这份批准只有 observe，随后的 act 为 `isError` true，executions 空。列表含 fixture，不含 decoy `OtherApp` 和 host chrome `Grok Dev`。响应正文不含 bearer。`consent_debug` 不含 bearer。`legacy_credential_is_not_replaced_by_transport`：未绑定的 transport observe 为 `isError` true，observe 计数不增加。rotate 后这条 transport 的 `computer_status` 是 401，原来的 legacy 凭据仍是 HTTP 200。

## R3 执行准入

实现：`begin_model_call` 在放下锁并处理过期 fence 之后返回带 owner、task、grant revision、epoch 的 permit。测试 barrier 在这之后、`dispatch_with_stop_handler` 之前。`recheck_model_call` 再对 `ConsentGate::Bound` 核对。`admit_consent` 是 broker 模块私有函数，`require_owner`、observe 和 act 在原生查询前做同一次短检查。Pending 拒绝。Legacy 保持原合同。活 bearer 不退回 legacy map。

验证：`agent_replacement_before_dispatch_reaches_no_adapter`：barrier 里换 agent 后 observe 的 `isError` 为 true，observe 计数不增加，executions 不增加。正向路径在 `pending_intent_is_not_scope_memory_and_incremental_keeps_a`：同一 token 完整 commit 后 observe 的 `isError` 为 false，observe 计数增加；对 B 的 input `isError` 为 true，executions 空。`pending_scope_cannot_commit_and_capabilities_are_split` 覆盖 observe-only 与 input-only。act / wait 仍走原来的 snapshot、ref、geometry 检查；`geometry_mismatch_zero_execution` 在全套件中通过。Managed、ExistingTab、WebView 的 `authorize` 经过 `require_owner`，因此经过 `admit_consent`。

## R4 fence 与 Stop 回执

实现：`consent_stop`、完成、到期、换 agent、connection rotation、回滚、cancel、feature-off 都按准备或 grant 上的确切 run id 调用现有 `fence_stop`。`fence_stop` 仍是调用 `revoke_run` 的唯一站点。`consent_stop` 只返回 `StopCleanupTicket`，不在返回前 `finish_stop_cleanup`，也不另开线程。到期扫描把空闲且超过原 deadline 的 run id 放进 outbox；`in_flight` 或 `stop_cleanup_pending` 的 run 跳过，也不在执行路径上刷新 idle deadline。cleanup 票据带 generation。

验证：`stop_ack_does_not_wait_for_slow_abort`。先打开无关 legacy `run-2` 并完成它自己的 observe。consent run 上的 act 进入 hang，且 abort 被挡住。`consent_stop` 在 400ms 内返回（`quick` 为 true，断言在测试末尾）。fence 之后对 `run-2` 的 act 是 `OutcomeKind::Rejected`，`executed` 为 false，executions 仍只有 `slow-action`。然后放开 abort，对每张票据 `finish_stop_cleanup`，join act 线程，`wait_stopped`。之后 `run-2` 的 act 为 `Verified` 且 `executed` 为 true。原 action 在新 lease 成功前已经 join。`request_state_is_bounded_and_completion_drops_the_grant` 在批准 grant 超过 idle TTL 后 `claim_continuation` 为空，且 `take_cleanup_tickets` 非空。`request_flood_does_not_consume_stop_budget` 通过：flood 之后 Stop 仍能 fence。

## R5 任务、范围和票据

实现：task id 只由 Host 的 `task_id_for` 分配，直到 `begin_new_task`、`complete_task` 或 Stop。解析器拒绝模型提交的 task/run/request/revision/epoch。相同 canonical 的 pending 复用。同一 intent 只改能力时，唯一 pending 卡 revision 增加。不同 scopeIntent 不合并，也不撤销已 commit 的 grant。B 的失败授权若没有新建 run，不 fence A 的 run。范围记忆只在 `finalize` 时从 Bound 身份写入。continuation 是 `AuthorityClaim`：app、agent、incarnation、connection、owner nonce、request、task、grant revision、feature epoch。`claim_continuation` 不消费。`consume_continuation` 先复验再标记。Approved 的 status 只带 `continuationIssued` 和 `decisionClass`，不再发票据。deny、cancel、expire 写 NoAuthority，不执行。

验证：`capability_change_revises_one_pending_request`：live count 1，request id 相同，revision 增加，grant 0。`pending_intent_is_not_scope_memory_and_incremental_keeps_a`：pending 时 memory 为空；A commit 后 memory 长度 1；B pending；B deny 后 A 的 observe 仍 `isError` false；B 绑到 `missing-window` 的 commit 失败，grant 仍为 1，A 再次 observe 仍成功。`host_new_task_retires_only_the_old_grant`：`begin_new_task` 后 grant 0，旧执行失败，memory 仍在，新请求是 pending 且 grant 仍为 0。`typed_ticket_cannot_be_consumed_after_stop_or_epoch_change`：Stop 后 claim 为空且旧 claim consume 失败；feature 关再开之后 validate 和 consume 都失败，executions 空。

U02 的忙时发送队列没有做。本批账本不提前丢票、不双消费。

## R6 上界

实现：terminal 的 request 才参与 trim。非终态达到 per-owner 8 或 global 32 时拒绝新请求。Binding 准备按 per-owner 4、global 32 计数。决策只丢 NoAuthority，或已消费、已作废的 continuation。有效未消费票据留着，塞不进去就拒绝新票据。scope 超过 per-owner 8 或 global 16 时丢最旧的记忆，不丢 grant。owner tombstone 计入 `MAX_OWNERS` 32。Stop 使用独立前缀和速率。退休 token 会从 rate map 里按后缀清掉，避免退役 bearer 占住预算。

验证：`terminal_reservations_and_live_continuations_stay_bounded`：40 轮不同 purpose，reservation 数不超过 32。`unconsumed_continuation_survives_decision_flood`：flood 之后 continuation count 仍是 1，claim 仍在。`request_flood_does_not_consume_stop_budget` 通过。全套件没有 ignored 测试，Clippy `-D warnings` 没有用 `allow(dead_code)` 放过产品代码。

## 仍留给后续批次

本批不派这些工作。

- U02：local interactive、feature-off、无 run 的 `session/new` 仍要出现申请目录项，并保留其它 MCP。`ensure_request_endpoint` 只听 loopback，不启动 browser/native helper。第 4.6 节六条 CU allow-once 匹配、`prompt_in_flight` 时的发送队列、边界上只发送一次 typed continuation，都还在 U02。core 的 claim/consume 已能复验，发送本身没有做。
- U03：申请卡、面板、15 个 locale、dialog。文案和 `window.confirm` 替代都不在本批。卡片必须调用 U02 的 Host 事务，不能只调 core commit。
- U04：生产多目标 resolver 和四类 membership。现在只有测试用 synthetic Desktop helper。范围内切换、增量扩大和缩小都未做。U05 要的不可执行批准范围记忆已能从 commit 的 Bound 身份留下，真实身份还没有。
- U05：用户一次点击的继续。复验 task、session、agent、revision、scope、idle 和 fresh observe。双击只一轮。模型不能调用 resume。
- U06：把撤权、feature-off、重连、账号、workspace root、fork、重启和 session 退出接到生产入口。core 已能按确切 owner fence，并在无 run 时取消准备。`fence_process_exit_session`、旧 `SessionGrants` tracked 表和 shutdown 枚举还没有覆盖这批 owner。不能用目录 absent 或旧 run 不存在推断准备和票据也不存在。

U07 的真人批准和真实模型闭环、U08 门禁、U09 开发包都不在本批。U00–U09 和 D00–D31 没有缩减。

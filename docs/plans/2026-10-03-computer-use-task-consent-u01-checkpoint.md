# U01 checkpoint：核心请求、任务授意与稳定 IPC

状态：停在审查边界。未 commit、未 push、未进入 U02。
HEAD：`8d8b71c262ab222748bca32412eea884d4b892e0`
分支：`feat/computer-use-implementation`
工作区：产品改动未提交。本批不把 core fixture 写成可用的完整 Computer Use。

产品默认关闭。测试里的 Host 决定是 synthetic，标签 `synthetic-host-not-a-human-or-production-grant`。它不是真人批准，也不是生产授权。本地 writer 的 isolated always-approve 不代表产品权限。

## 实现

权威状态只在 `ComputerUseBroker` 的 `Inner`：`feature_enabled`、`feature_epoch`、`consent: ConsentState`（`src-tauri/computer-use-core/src/task_consent.rs`，编排在 `broker/consent.rs`）。IPC 和 `tools::dispatch_with_transport` 只路由，不另存一份 grant 或 feature bool。

IPC 路径：

1. `IpcServer::issue_transport_credential` 向 Broker 取稳定 token。同一 owner、incarnation、connection generation、agent 再次构造得到同一个 token。token 不写入 legacy credential map，不进入模型 JSON，`Debug` 印出 `<redacted>`。
2. Live bearer 走 `dispatch_with_transport`。Retired 返回 401，不退回 legacy。Unknown 仍走 `credential_for_session` 的旧 map。
3. `computer_request_access` 只接受 `request` / `status` / `cancel`。request 字段是 purpose、surfaces、capabilities、scopeIntent。status 和 cancel 不带 purpose，也不接受模型的 task、run、request、revision、epoch。未知字段、别名 surface、超长、重复和非法 enum 拒绝。
4. 无 run、feature 关闭、本地 owner 有效时，request 返回 pending。不调用 owner run、authorized target，不启动 adapter、helper 或 browser worker，不读屏幕、DOM、UIA、AX、窗口标题、tab 或剪贴板。
5. 执行工具经 transport 查当前 grant。没有 grant 就拒绝，错误不附其他会话或范围外的元数据。legacy token 不被新 transport 覆盖，也不能从新路由 fallback 到 legacy。
6. `commit_access` 先核对 revision、当前 feature epoch、owner 和 Stop。合法的第一次开启与 reservation、grant 在同一把锁里发布。迟到旧卡不能先开功能再失败。更新的用户 epoch 不会被旧失败 rollback 关掉。
7. 生产代码没有 Bound 构造器。`#[cfg(test)] bind_synthetic_scope` 才能把范围标成 synthetic Bound。Pending 的 commit 失败，不产生 grant、execution 或 continuation。
8. 同一 token 在 synthetic Bound commit 之后，可以在该范围内做第一次 observe。读要 observe，写要 input。status、list、open 和第一次 observe 不要求尚不存在的 snapshot。list 只返回已授权目标。act / wait 仍走现有 run、target、snapshot、ref、geometry。
9. 暂停留下非执行 scope 记忆，执行身份失效。Stop、feature-off、owner retirement 取消 request、grant 和票据。清理仍先 fence 再 join。没有新 request 时不删除旧占用，也不重放 unknown。

字段合同：

- surface 只收 `desktop`、`managed-browser`、`existing-tabs`、`app-webview`。`browser`、`existing-tab`、`webview` 拒绝。
- capability 只收 `observe`、`input`。purpose 与 scopeIntent 在 trim 后 1..=200。surfaces 最多 4 个且不重复，capabilities 最多 2 个且不重复。
- scopeIntent 只用于展示和去重，不构成执行身份。同 canonical intent 复用；Chrome 与 Excel 不合并。同 family 换 scopeIntent 时旧卡退休。
- 请求 TTL 10 分钟，grant 空闲 TTL 30 分钟。时钟是 `Instant` 起点加测试可推进的 `Duration`，测试不真实 sleep。
- 模型回复含 state、requestId、revision、authority、purpose、scopeIntent、surfaces、capabilities。不含 taskId 和 bearer。approved 的 status 另加 `authority=grant`、`continuationIssued`、`decisionClass`，不再次执行，也不再发票据。
- `Continuation` 与 `NoAuthority` 分型。deny、cancel、expire、stop、retire 只写无权限原因。
- 请求有独立信号量和 `request:` 速率桶（4 / 800ms）。Stop 桶独立。live request 最多 8，存储请求最多 32，scope 记忆最多 16，决定最多 32，退休 token 最多 64，owner 最多 32。
- agent id 可晚到。空字符串拒绝。第一次 `bind_agent` 采纳预留 token，不增加 incarnation。之后换 agent 退休 transport 和执行 grant，留下 scope 记忆。
- `feature_epoch_at_issue` 不是执行授权。grant 比对当前 epoch。重复写入当前 feature 值不推进 epoch。

`MODEL_TOOLS` 在 `computer_stop` 后增加 `computer_request_access`。Node `protocol.mjs` 和 `catalog.json` 用同一顺序和同一输入 oneOf。旧 golden 安全断言还在：schema 文本仍不得出现 pageId、profile、selector、evaluate。

本批改过的文件：

- `src-tauri/computer-use-core/src/task_consent.rs`
- `src-tauri/computer-use-core/src/broker/consent.rs`
- `src-tauri/computer-use-core/src/broker/tests_task_consent.rs`
- `src-tauri/computer-use-core/src/ipc/tests_task_consent.rs`
- `src-tauri/computer-use-core/src/lib.rs`
- `src-tauri/computer-use-core/src/broker.rs`
- `src-tauri/computer-use-core/src/broker/lifecycle.rs`
- `src-tauri/computer-use-core/src/broker/tests_identity.rs`
- `src-tauri/computer-use-core/src/ipc.rs`
- `src-tauri/computer-use-core/src/tools.rs`
- `tools/computer-use-mcp/protocol.mjs`
- `tools/computer-use-protocol/golden/catalog.json`
- `docs/plans/2026-10-02-computer-use-task-consent-adr.md`
- `docs/plans/2026-10-03-computer-use-task-consent-u01-checkpoint.md`

未改 Codex 的 `oracle-counterexamples-v2.mjs`。未接 UI、未自动打开用户 Computer Use、未点击系统或 Share 许可。

## 验证

证据目录：`tools/computer-use-probe/.run/task-consent-u01-20261003/`。SHA-256 均为本机 `Get-FileHash`。

### P0：oracle

旧 oracle 快照没有被覆盖：

| 文件 | SHA-256 |
| --- | --- |
| `preserved-u00r/oracle.mjs` | `bfe73b4038fdf90c1bcf9be87a0f6163c6b9c7c7be2c9352049513a21e51c6a7` |
| `preserved-u00r/events.ndjson` | `89981790a60848abacb55a38f09224815945c659615182d4294b88a22d732c07` |
| `preserved-u00r/fixture-calls.ndjson` | `61ec1da02a4692db0f9d56a1f5ed1aabfc58a1e25a513521570fde95e4b92ba0` |
| `preserved-u00r/oracle-report.json` | `c381264a6a43fff0d33800b9031836ae574a4f5cdcf05693d1c21d7f0552a0be` |
| `preserved-u00r/negative.mjs` | `09144e9e6426e06a9a4b8f617748b7eed0b1516426cc2ccf59fd6b80e69529fe` |
| `preserved-u00r/negative-report.json` | `aabb77488e172a9069ca8bea2c423719f20cdbe80c9f95dd6e59316e39cdc663` |

`oracle-report.json` 的 9/9 是旧 oracle 的结果。它不覆盖后来加上的三条谓词。`negative.mjs` 本批没有重跑。

现行 oracle 是 `tools/computer-use-probe/.run/task-consent-u00r-20261002/oracle.mjs`，SHA-256 `23ef887aa6dd8efa9ef9a4e15b554b534b4bceff04293ad676dadff47ee846a2`。十条谓词：permission-raw-saved、qualified-permission-captured、structured-use-tool-identity、answers-are-allow-once-test-data、cross-turn-asked-again、update-mcp-during-prompt、overlap-ordered-end-turn、positive-fixture-once、cancel-then-late-no-extra-call、eof-no-extra-call。后三条要求真实正向 fixture 各一次、cancel 到进程终态之间没有新调用，以及原子进程的真实 exit/close。

独立反例脚本未改。审查命令：

```text
node tools/computer-use-probe/.run/cu-consent-orchestrated-20261002/batch01-u00-rework/codex-review/oracle-counterexamples-v2.mjs
```

脚本在任一 `actuallyPassed` 时退出 1，否则退出 0。

- 修复前 `counterexample-before.json`（`820a8f4137db6e5d56dc5fba04384109de686ffba80b06161019585fb69435c1`）：sourceHash 为旧 oracle，positiveControl true，三组 `actuallyPassed: true`。这是当时的退出 1。
- 修复后 `counterexample-after.json`（`d3dac7b2c3bd60c98f8c436a6f6ccdc0d6afc72aa285590e262a28920bd769b6`）：sourceHash 为现行 oracle，positiveControl true，`post-cancel-call-before-eof`、`no-positive-fixture-execution`、`eof-without-terminal-child` 都是 `actuallyPassed: false`。这是退出 0。

重放命令是 `node tools/computer-use-probe/.run/task-consent-u01-20261003/replay-and-negative.mjs`。`replay-report.json` SHA-256 `4e3348728fa5c7bc2cde4901e0f27bdf7d4bc9b0993507a7c34c66cf22edb1fb`：preserved 真实日志 10/10，`replayExit` 0。九个变异全部 `wentRed`，`mutatedExit` 1。其中六个是原 negative 的 omit-session-cancel、late-fixture-calls-plus-one、omit-permission-raw、extra-tools-call-in-log、overlap-stop-mismatch、baked-ok-does-not-pass；三个是上面的新反例。报表 `decisionClass` 是 `test-data-not-a-human-or-production-grant`，`productionAppVerified` false。采集逻辑没有改，没有新的 CLI probe。

### P2：先红后绿

原红灯保留在 `red-request-pending.txt`，SHA-256 `2149f26dad5e04311f77297eaefacf1371ee07e3f4819af70e489be70a816077`。它编译成功，然后行为失败：

```text
assertion left == right failed: feature-off request with a local owner and no run must return pending,
got {"code":"run_not_found","content":[{"text":"run not found","type":"text"}],"isError":true}
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 590 filtered out
```

这是当时 `dispatch` 没有这条入口。该文件没有被后来的运行覆盖。

随后两次失败不是那条行为红灯：

- `green-consent-compile-shadow.txt`：测试助手与局部变量同名，`E0618`，编译失败。不把它当成行为断言。
- `green-consent-double-commit-red.txt`：12 通过、1 失败。第二次 commit 复用了开启功能前的 reservation epoch，返回 `StopRequested`。

修复后同一过滤器：

```text
cargo test -p grok-computer-use-core --lib --offline -- feature_off_request_without_run_or_target_returns_pending legacy_credential_is_not_replaced_by_transport request_flood_does_not_consume_stop_budget unapproved_transport_calls_no_adapter_and_rejects_bad_owners forged_fields_have_no_authority_effect pending_scope_cannot_commit_and_capabilities_are_split duplicate_request_and_commit_happen_once late_approve_after_deny_cancel_ttl_or_stop_creates_nothing stop_during_prepare_and_old_epoch_do_not_enable_or_roll_back scope_intent_does_not_merge_and_old_card_cannot_approve request_state_is_bounded_and_completion_drops_the_grant failed_bind_restores_a_feature_this_commit_enabled resume_drops_authorization_snapshot_and_lease
```

工作目录 `src-tauri`。`green-consent-tests.txt` SHA-256 `c88d5afeb706565c6b3b66fe067abc6b1c46b2b555489d0a4c357cc61fff18fa`。结果 `ok. 13 passed; 0 failed`，进程 `EXIT=0`。这次聚焦运行在 clippy 清理之前。清理只加了 `Default`、把测试时钟方法标成 `cfg(test)`、把 `let else` 改成 `?`、把布尔 `assert_eq` 改成 `assert`。下面的包级 suite 在这些清理之后重新跑过，包含这 13 个测试。

这些测试打到的效果计数：

- 未批准 transport 上全部读、写、stop、handoff：`isError`，`list_calls` 0，`observe_calls` 0，executions 空。空 session、空 agent、未知 token、换 agent 后的旧 token 拒绝。
- 伪造 task、run、request、revision、epoch、未知字段、别名、重复、超长、非法 capability：全部 `Err`，feature 仍关，live 0，grant 0，adapter 0。
- Pending commit 失败，feature 仍关，continuation 0，`list_calls` 0。observe-only 的 act 失败且 executions 空；随后 observe `screenshot:false` 成功，`observe_calls` 1，结果不含 bearer。input-only 的 observe 和 list 失败，调用计数不增加。
- 同一 token：pending，然后 synthetic Bound commit，然后 fresh observe。status 为 approved，不再次 observe，也不再发票据。重复 request 同 requestId。第二次 commit 的 continuation id 相同，`list_calls` 不增加，`continuation_count` 1。
- deny、cancel、时钟推进 601 秒、Stop 之后的 commit：无 grant，feature 仍关，原因分别为 denied、cancelled、expired、stopped。`list_calls` 0，`observe_calls` 0。
- preparing 前 Stop：commit 失败，feature 关，grant 0，`list_calls` 0。preparing 前用户打开 feature：commit 失败，feature 保持开，epoch >= 1，grant 0。enable 之后用户先关再开：commit 失败，feature 保持开，epoch > 1，grant 0，adapter 0。
- 死目标 `missing-window`：commit 失败，feature 回到关，epoch 0，grant 0，`list_calls` 0，`observe_calls` 0。
- Chrome 与 Excel 不合并，旧卡 commit 失败，scope 记忆 2，`list_calls` 0。rotate 后旧 token 是 Retired，status 失败，scope 记忆仍在。
- 12 个不同 purpose：前 8 个成功，其后失败。live <= 8，stored <= 32。grant 空闲推进 1801 秒后不可执行，feature 仍开。`complete_task` 后 grant 0，scope 记忆还在，下一次 request 是 pending。
- IPC：两次 issue 得到同一 token；pending 的 HTTP 200 且 `isError` false；正文含 `state=pending`、`authority=none`，不含 token；`list_calls` 0，`observe_calls` 0，`test_run_count` 0。synthetic commit 后 open 与 observe 成功，`observe_calls` 1。list 含 fixture id，不含 `OtherApp` 和 `Grok Dev`。act 失败，executions 空。
- legacy credential 在 issue transport 前后不变。无 grant 的 transport observe 不增加 `observe_calls`。rotate 后旧 transport HTTP 401，legacy status 仍 200。
- request 桶打满返回 429。随后 stop 不是 429，且 `isError` true。
- `resume_drops_authorization_snapshot_and_lease`：暂停前记下一条 pending scope；恢复后 scope 记忆仍是 1，execution observe 失败，`Debug` 不含 token。原 observe `TargetUnauthorized`、act `Rejected`、executions 空仍在。

### P3：回归

```text
cargo clippy -p grok-computer-use-core --all-targets --offline -- -D warnings
```

`clippy.txt` SHA-256 `a044f27143378ee4e0efe5712ec53de2578e25fd4193e32412fd2666b710903c`。`Finished dev profile`，`EXIT=0`。先对改过的 Rust 文件跑了 `rustfmt --edition 2021`，没有格式化无关文件。

```text
cargo test -p grok-computer-use-core --offline
```

工作目录 `src-tauri`。`core-suite.txt` SHA-256 `a709b950ad720235ac681b7f6b8f02b661dc8ef7ceac884fa4055fddc0e06884`。lib `602 passed; 0 failed`。同一命令里的其余测试二进制也都是 `test result: ok`，合计进程 `EXIT=0`，耗时约 63.56 秒。其中 macOS AX / capture / pointer 合同在本机通过，这只说明合同测试可编译可跑，不是 macOS 真人验证。

```text
node --test tools/computer-use-mcp/protocol.golden.test.mjs
```

`protocol-golden.txt` SHA-256 `12c38779ff1d9962a549a1d7fcf36891cba8d2a52d74b9b7f276e3dc9c768000`。5 passed，0 failed，`EXIT=0`。node 打了 `NO_COLOR` 与 `FORCE_COLOR` 同时存在的警告，测试本身通过。

### not_run

- 全套 UI suite。本批没有改 UI。
- 新的 CLI probe，以及 live `negative.mjs`。采集没有变，所以没有重跑受影响 fixture。
- `mt.exe` post-link。本批 libtest 没有出现 `0xc0000139`。
- 依赖或 CLI 升级。
- 真实 UI 批准、真实软件 Computer Use、系统许可和浏览器 Share。
- U02 的 session 预连接、ACP CU permission handler、prompt 队列。
- U03 聊天卡、U04 多目标 resolver、U05 一键 resume。
- 跨平台 U07。

## 范围

U01 已完成：Host core 的请求类型、单一权威状态、稳定 transport、无权 request/status/cancel、执行工具的当前 grant 门、synthetic Bound 上的第一次 observe、暂停后的非执行记忆、Stop / feature-off / owner retirement，以及上面的 core、IPC、golden 回归。

U01 未完成，也不能从本批测试外推：

- 没有生产 Bound 构造器。没有测试 helper 时 commit 停留在 pending，不会成功。
- 没有聊天卡，没有真实用户点击，没有自动开启功能。
- continuation 只进一次性账本。没有把它送进 prompt 队列。
- App 的 `session/new` 还没有在 CLI 返回 agent id 之前装入这条目录。core 的预留和 `bind_agent` 可以接 U02，本批没有接。
- 旧 `tools::dispatch` 仍要求 owner。没有 transport 的 `computer_request_access` 不会返回 pending。

U02 入口：`ensure_request_endpoint`、session/new 时用预留 owner 发 transport、第 4.6 节六条件的 CU-only allow-once、以及按 `Continuation` / `NoAuthority` 分开的 prompt 队列。忙时仍不发第二条 prompt。

U03 入口：聊天卡只展示 purpose、scopeIntent、surfaces、capabilities、requestId、revision。批准必须走 Host `commit_access`，不能先开 feature 再校验。

## 收尾

`git status` 仍是未提交改动。HEAD 没有移动。本批 cargo、clippy、node golden 进程都已结束，退出码如上。结束后没有名为 `cargo`、`rustc` 或 `grok_computer_use_core` 的进程。IPC 测试服务器随测试进程退出。没有为了“没有端口”去结束别的进程。

停在这里，等 Codex 看 diff 后再派 U02。

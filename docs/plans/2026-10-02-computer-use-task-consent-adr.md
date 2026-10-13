# Computer Use 任务授意 ADR

2026-10-12 增量合同：[授权等待期间的状态查询](2026-10-12-computer-use-pending-status-checkpoint.md)。准确 live transport 的空参数 `computer_status` 可只读最新未批准/终态申请元数据，不创建 permit、不读取电脑；显式 run/target 和 Approved 后的执行状态仍受原 TaskGrant gate。此例外替代下文历史“所有 status 必须已有 grant”的表述，不放宽 observe/input/upload 或普通 CLI 允许缓存。

状态：U01 核心已由 Codex 接手修复并接收，详见 `2026-10-03-computer-use-task-consent-u01-codex-finish-checkpoint.md`。Host/ACP/UI/生产范围绑定仍待接线；整体未交付。下文 U01-R 与第 2 节保留当时的基线，不改写历史验证结论。
基线：`8d8b71c262ab222748bca32412eea884d4b892e0`，分支 `feat/computer-use-implementation`。
U00 证据：`tools/computer-use-probe/.run/task-consent-u00-20261002/`。
U00-R 证据：`tools/computer-use-probe/.run/task-consent-u00r-20261002/`。
CLI：`grok 1.0.46 (2765805b9442)`。探针是 normal `--permission-mode default`，没有 `--always-approve`，没有恢复外层 writer 会话。探针里的 allow-once 是测试数据，不是真人批准，也不是生产授权。

Computer Use 仍默认关闭。本文不把功能标成完成。

## 修订说明

Codex 对上一稿的结论是 needs_rework（`docs/plans/2026-10-02-computer-use-task-consent-u00-codex-review.md`）。上一稿写过“U00 已冻结”。那个状态撤销。下面四条在上一稿里没有可执行答案，本修订才补上：

- 第 4.6 / 5 节只排除了 `SessionAllowCache` 和 `always-allow`，并直接写成一张 Host 卡。CU 工具仍走通用 `use_tool` 权限。
- `RequestCredential` 被写成只有 `session_id` 和 token，且只能调用 `computer_request_access`。同一个固定 MCP 进程在批准后如何执行，没有路由。
- 第 4.2 节写成先 `set_feature_enabled(true)` 再 commit。关闭或取消的旧卡可以先把功能打开。
- 第 4.3 节的 surface 用了下划线名字，并把相同的 surface / capability 枚举当成相同范围。

U00 已经核实并且本修订不改写的事实仍在第 2、3 节：空 home 的认证失败、configured home 的固定字符串 fixture、`allow-once` 不覆盖下一次调用、忙时目录替换、重叠 prompt、cancel 与 EOF。第一次 configured 进程 pid 67024 的退出码 1 来自 writer 自己的 `taskkill`（工具调用 `call-08e8209c-e63c-43b3-bf00-b9aee69da3b7-65`），终止晚于 20 秒测试回执和 prompt id 4 的 `end_turn`。那不是已证的产品崩溃。

U01-R 修正（2026-10-03）。权威状态只在 Broker `Inner` 的 `ConsentState`，和 feature bool、`feature_epoch` 同一把锁。IPC 与 `dispatch_with_transport` 只路由。Codex 对上一批 U01 的结论是 partial / needs_rework。下面三句不再是合同，实现也不再这样做：第一次 `bind_agent` 收养 `agent_session_id` 为 `None` 的 grant；失败回滚把 epoch 写回 0；未批准的 intent 写成已批准范围记忆。

- 准备和 commit 分开。准备记录有精确 owner，可单独取消。preparing 时模型可见状态是 pending，可执行 grant 数是 0，continuation 数是 0。status、重复 commit 和领取票据都不把“准备已开始”当成成功。只有真实绑定成功后，在权威锁内复验 owner incarnation、agent、connection generation、request/revision、task、feature epoch、reservation、Stop 或准备取消，以及实际绑定结果，再写入 Approved、一份 grant 和一张 typed continuation。
- 失败只回滚本预约仍拥有、且没有被更新过的那一次 feature 迁移。回滚 bool 的同时 epoch 向前。0→1→失败回关之后 epoch 大于 1，不恢复 0。用户后来的开/关不被旧 rollback 覆盖。
- Owner 使用 Host 单调 nonce，retire 留下 tombstone，不删槽后从 1/1 重发。捕获的旧 ctx 在 retire、reissue 和同一 agent id 重绑之后，request、status、cancel、reserve 和 execution 都拒绝。正式模型调用和 Host commit 必须有当前 agent。`None` 不是执行身份，`bind_agent` 不收养先前的 None grant。第一次绑定保持预留 token 字符串稳定。新 agent 或重连退役旧 token、准备、执行和续行。同一 agent 的普通重复登记不轮换 token。
- 当前执行准入带着 owner、task、grant revision 和 feature epoch 进入 Broker 已有的 action admission，在最终派发前受同一 fence。新 transport 不退回 legacy 权限。grant 已经查到、adapter 尚未派发时，Stop、换 agent、connection rotation 或能力退休胜出，则读和输入的派发数是 0。
- `consent_stop` 只做有界 fence，交出 cleanup 票据和准确的 `stop_requested`。同步返回前不逐个 `finish_stop_cleanup`。原物理 owner 未确认退出时，新的 desktop lease 被拒绝。到期扫描交出被退休 run 的确切 cleanup。空闲 TTL 按真正空闲计时；长 action 或尚未终态的 cleanup 不因另一次模型工具调用而延长。
- taskId 由 Host 任务边界管理，模型不能指定。相同 canonical 的 pending 请求复用。同一 intent 的能力变化修订唯一活跃申请。不同 scopeIntent 不合并。已 commit 的 A 在 B 处于 pending、deny、cancel 或 bind 失败后仍可执行。B 未批准前，对 B 的读取和输入是 0。只有 Host 明确新任务、完成或 Stop 才退休原任务。
- 申请历史和批准范围记忆是两种类型。记忆只来自最终 commit 的真实 Bound 身份。新 agent 得不到可执行 grant。continuation 在真正 commit 之后生成，票据含 app、agent、incarnation、connection、owner nonce、request、task、grant revision、feature epoch 和一次性消费身份。查询 Approved 不再发票据。claim 不消费；发送前的 consume 按权威绑定复验。deny、cancel、expire 是限定 owner 和 request 的 NoAuthority，不能触发执行。
- owners、requests、preparations、decisions、scope memories 和 retired tokens 分别有 per-owner 与 global 上界。未消费的有效 continuation 不被其它请求 flood 丢掉。容量耗尽时拒绝新工作，不删掉仍有效的权限来假装有界。Stop 保留独立槽和速率。

测试 Bound 只来自 `#[cfg(test)]` 的 synthetic Host helper，标签 `synthetic-host-not-a-human-or-production-grant`。没有生产 Bound 构造器。prompt 发送队列是 U02。重复写入当前 feature 值不推进 epoch，也不取消仍在等待的申请。真实开/关把 epoch 加一。失败回滚造成的关也把 epoch 加一。用户转到关时 fence 当前 run，并取消 request、grant 和票据。

## 1. 三层身份

权威状态只放在 Host core（`SessionGrants` + `ComputerUseBroker`）。React、Tauri command 和 MCP 进程只保存 Host 发来的 id，不各存一份可执行授权。

| 层 | 类型 | 现有代码里最接近的东西 | 这一层允许做什么 |
| --- | --- | --- | --- |
| 传输 | `SessionTransportCredential` | `server.mjs` 启动时读一个 `GROK_APP_CU_TOKEN`，所有工具共用。`credential_for_session`（`ipc.rs`）今天还要求 owner 和 authorized target | 证明这条 MCP 连接属于当前 App session、agent incarnation 和 connection generation。不授予电脑权限 |
| 任务授意 | `TaskGrant` | `session_grants.rs` 的 `Slot` 只有 `run_id` / `attempt_id` / `generation`，没有可跨暂停保留的任务范围 | 记住用户已批准的 scope、能力、期限和绑定。不能单独产生观察或输入 |
| 执行绑定 | `ExecutionBinding`，执行数据仍在现有 `RunState` | `broker.rs` `drop_execution_identity` 清 target、snapshot、ref、geometry；`resume` 还会 `generation += 1` 并释放 desktop lease | 一次复验后的观察和动作。旧 snapshot / ref 不能再用 |

模型正文、连接 token、截图和 ACP session id 都不是授权。`requestId`、`taskId`、grant revision、continuation id 只由 Host 生成。请求 JSON 里出现这些 id、`authorize`、`alwaysApprove`、PID、路径或命令，整份请求拒绝。

`SessionTransportCredential` 的绑定字段是：

- `app_session_id`
- `agent_session_id`（正式执行前必须是当前 agent；预留 transport 时可以还没有，`None` 不是执行身份）
- `connection_generation`
- `feature_epoch_at_issue`（签发记录，不作为后来的执行世代）
- `retired: bool`

token 只放进 Host 拉起的 MCP 进程环境，由 `server.mjs` 作为 loopback `Authorization` 发给 Host。工具结果、模型可见正文和探针以外的日志不写这个 token。模型不能从返回值里拿到 bearer，也不能拿它换执行权。执行时比对的是 grant 上的 epoch 和 Host 当前 epoch。

批准之后 Host 写入当前 `TaskGrant` 和短期 `ExecutionBinding`。下一次执行调用仍带同一个 transport token。Host 用 token 找到绑定，再查自己的 grant / binding。token 自己不变成执行凭据。未批准、暂停、撤权时，同一条连接上的执行调用拒绝。

`credential_for_session` 可以留下，给已经有 run 的旧目录重建使用。它的替换逻辑不得删除或轮换 transport token。新的稳定入口调用 `issue_transport_credential`，不调用 `validate_session_binding`，也不调用 `authorized_target`。

退休：App session 结束、agent incarnation 变化、ACP 退出、重连、换模型、connection generation 增加，都退役旧 transport token、旧执行绑定和未消费的 continuation。同一 App session 上的非执行 scope 记忆可以留下。它不会自动成为新 agent 的 `TaskGrant`。新 agent 要重新申请。用户稍后的“继续此任务”是 U05 的 Host 点击，不是连接继承。

## 2. 现在的链路

本地 interactive 会话要等到已经有目标和 run，工具才会出现：

1. `mcp_acp_entry_with`（`inject.rs`）先 `require_owner`，再 `authorized_target`。没有目标时错误是 `authorize a target in the Computer panel first`。
2. `credential_for_session` 调用 `validate_session_binding`，后者同样要求 `require_owner` 和 `authorized_target`。空 `run_id` 不能当成传输凭据。
3. `build_catalog`（`computer_use.rs`）只有 `desired.run_id == Some` 才把 `grok-computer-use` 放进目录。`update_mcp_servers` 是整表替换。
4. `attach_computer_use` 在 turn 忙时直接返回 `wait for the current turn to finish`。`live_session_is_busy` 覆盖 `prompt_in_flight`、Connecting、未结束的 Streaming、打开的工具，以及 plan / ask / permission gate。这个检查保留。
5. `tools::dispatch` 对每个模型工具先 `require_owner`，再拒绝伪造权限字段，再要求 `feature_enabled`。`MODEL_TOOLS` 没有 `computer_request_access`。`computer_authorize` / `computer_resume` / `computer_reconnect` / `computer_pause` 在 `HOST_ONLY_TOOLS`。
6. feature 默认关：`FEATURE_DEFAULT = false`，`AppSettings.computer_use_enabled` 缺省也是 false。`set_feature_enabled`（`feature.rs`）只写一个 bool；关的时候清空 session 集合。今天没有 feature epoch。
7. MCP 子进程 `tools/computer-use-mcp/server.mjs` 在启动时读取一个 `GROK_APP_CU_TOKEN`，`listChanged` 是 false，Host HTTP 超时 15 秒。目录里没有无权申请工具。
8. `resume` 丢掉执行身份，没有另一份任务范围可以留下。测试 `resume_drops_authorization_snapshot_and_lease` 把“范围记忆也没了”写成了合同。

因此今天最小的失败是：没有 owner / authorized target 的申请在工具分发和目录注入两处都失败，用户看不到申请卡。

## 3. CLI 1.0.46 已经核实的行为

以下只来自 owned probe 的线协议。

- `initialize` 返回 `protocolVersion: 1`、`agentVersion: 1.0.46`、`agentCapabilities.loadSession: true`。
- 空的 `GROK_HOME` 上，`authenticate { methodId: "cached_token" }` 12 秒无响应；随后 `session/new` 返回 `-32000 Authentication required` / `no auth method id provided`。没有凭据就不能建立会话，也就不能注册工具。这条 firstFailure 留在 U00 目录，U00-R 没有重跑。
- 已配置的独立 `GROK_HOME` 上，`session/new` 可以在没有任何 Computer Use run 的时候装入只返回固定字符串的 fixture。`_x.ai/mcp/server_status` 里看到的名字只有探针自己的 `u00-fixed-fixture`。随后的热更新又出现 `u00-fixed-extra`。
- 同一个 agent session 上，后一条 `session/prompt` 会在前一条 `end_turn` 之后继续。这就是任务批准后的续行载体。
- CLI 权限不是挂起的 MCP HTTP。`session/request_permission` 是 agent 发给 client 的 JSON-RPC。第一条探针把回复故意推迟 20013ms，原来的 `session/prompt`（id 4）一直开着，回复后同一条 prompt 以 `stopReason: end_turn` 结束。这条证据只说明 CLI 权限框能跨过 20 秒。Computer Use 不能模仿它，因为 MCP 子进程把 Host 调用限制在 15 秒。
- 实际权限框的包装器是 `use_tool`。选项固定为三条：`always-allow`（kind `allow_always`）、`allow-once`（kind `allow_once`）、`reject-once`（kind `reject_once`）。线上没有 `allow-always-mcp`。
- `allow-once` 不覆盖下一次调用。同一 turn 里，不合格名称失败后，合格的 `server__tool` 会再要一次权限；下一个 turn 也会再要。
- 第一次探针对不合格调用选过一次测试用的 `always-allow`。其后两条合格 `use_tool` 仍然各自弹出权限。这次观察不能外推成“always-allow 无害”。Computer Use 的 Host 回执不选择它。
- U00 的 `_x.ai/session/update_mcp_servers` 在 `session/prompt` 尚未结束时返回成功（回执 tMs 21026，prompt B 结束 tMs 32936）。CLI 那一次允许忙时整表替换。产品仍不把忙时替换当作批准等待的办法。
- 重叠的第二条 `session/prompt` 没有返回 `task_already_running`，而是在第一条之后以 `end_turn` 结束。Host `turn.rs` 在 `prompt_in_flight` 时拒绝第二条发送，这个 Host 围栏保留。续行必须排队，不能靠 CLI 拒绝来防双发。
- `session/cancel` 之后，原 prompt 的 `stopReason` 是 `cancelled`。1500ms 后补上的测试 `allow-once` 没有再调用 fixture。
- 权限还悬着时关闭 stdin，CLI 记录 `unable to receive 'session/request_permission' response, channel closed`，进程退出码 0，fixture 没有新调用。

`session/load` 只核实了 capability 为 true，没有对一条持久化会话做 load。重连后续行走 Host 队列，不走未验证的 load 副作用。

### 3.1 U00-R 权限消息里实际有的身份

U00 的两份日志没有留下 permission `rawInput` / `_meta`。U00-R 用正常权限模式又跑了一条无害 fixture，服务器名 `u00r-fixed`，工具只返回 `U00R_FIXED_STRING` 或 `U00R_OTHER_FIXED`。没有注册 Computer Use、浏览器或桌面工具。样本是 `attempt-1/permission-raw/permission-0.json`，会话 `01a0fd5a-3299-7743-834f-50f742569ba9`。四条权限的形状相同。

权限对象里和身份有关的字段是：

- `toolCall.kind` 为 `other`。
- `toolCall.title` 为限定名，例如 `u00r-fixed__fixture_fixed_string`。
- `toolCall.rawInput.variant` 为 `UseTool`。
- `toolCall.rawInput.tool_name` 为同一个限定名字符串。
- `toolCall.rawInput.tool_input` 是模型参数，例如 `{ "note": "u00r-q" }`。
- `toolCall._meta["x.ai/tool"].name` 为 `use_tool`，`kind` 为 `use_tool`，`namespace` 为 `grok_build`。

权限上没有单独的 server 字段，也没有 Host 签发的 server id。限定名是一个字符串，服务器名和工具名靠第一个 `__` 拼在一起。

同一次调用的 `session/update` 更早一条 `tool_call` 上，`title` 还是 `use_tool`，`rawInput.tool_name` 已经是限定名。title 会变，不能当匹配键。

工具完成后的 `tool_call_update.rawOutput` 才出现分开的 `tool_name`（不合格短名）和 `server_name`。那是调用之后的输出。允许回执不能等它。

因此本 CLI 给了结构化的 `rawInput.variant` + `rawInput.tool_name`，同时 `_meta` 名字仍是包装器 `use_tool`。它没有给一份与 title 无关的 Host 注册 id。第 4.6 节的匹配只用这次看见的结构，不补未出现的字段。

## 4. 协议决定

### 4.1 申请工具预先挂上，执行工具仍关闭

本地 interactive 会话在 `session/new` 时就放入稳定目录项，名字仍是 `grok-computer-use`，`listChanged` 仍是 false。SSH、side-channel、`empty_mcp_servers` 不挂。`supports_local_computer_use` 的这三个条件继续生效。

这项目录使用第 1 节的 `SessionTransportCredential`。构造它的函数不得调用 `mcp_acp_entry_with`、`authorized_target`、`ensure_host_runtime` 或 `browser_supervisor::ensure_product_attached`。`ensure_host_runtime` 今天会注册 surface adapter 并附带 managed browser。feature 关闭时这些都不能启动。

U02 增加 `ensure_request_endpoint`：只保证 loopback IPC 在听。desktop / browser / WebView executor 留到 Host commit 成功之后。

### 4.2 feature epoch 与批准预约

`computer_request_access` 在 feature 关闭时也可以被调用。它不读屏幕、不列窗口、不启动 helper。返回：

```json
{ "state": "pending", "requestId": "<host>", "revision": 1, "authority": "none", "purpose": "...", "scopeIntent": "...", "surfaces": ["desktop"], "capabilities": ["observe"] }
```

模型正文没有 taskId，没有 bearer。Host 画一张卡，文案表达“启用并允许本次任务”。点击走下面的两段准备，不存在先把 Approved、grant 或 continuation 发布、再发现绑定失败的窗口。点击前没有 capture、输入、native helper 或 browser worker。

U01-R 在 Host `Inner` 上使用 `feature_epoch: u64`，与 feature bool 同一把锁。U00 时 `set_feature_enabled` 只有 bool。真实的开/关转换把 epoch 加一。重复写入当前值不推进 epoch，也不因此取消 pending 申请。失败回滚若把这次预约自己打开的功能关回去，epoch 再加一，不写回打开之前的值。

批准按顺序做：

1. 在权威锁内复验 owner nonce、incarnation、agent、connection generation、request、revision、task 和 Stop。没有当前 agent，或卡片已关闭、取消、过期、revision 不对：拒绝。此步不打开 feature，不写 grant。
2. 写下可取消的准备记录，内部阶段为 Approving。模型可见状态仍是 pending。grant 数和 continuation 数保持 0。
3. 放出锁。测试 barrier 可以在这里 Stop 或让用户改 feature。
4. 回到锁内。准备若已不是当前 owner 的当前世代，中止，不发布。功能当前为关且这张准备仍拥有这次迁移时，打开功能并把 epoch 加一，记下这张准备拥有的 epoch。功能已经为开时不改 epoch。
5. 再次放出锁，做最后一次前置检查，然后 `open_run` 和真实绑定。这一段之间的 Stop，以及绑定中或绑定后的 Stop / feature-off，都不能留下新的可执行 run、lease、grant 或票据，也不能返回 Approved。本 commit 新建的 run 在绑定完成前标记为 Pending，不能经 transport 或 legacy 身份执行。
6. 绑定失败，或最后一次复验失败：只回滚这张准备仍拥有的 feature 迁移，epoch 向前。用户或另一张卡已经把 epoch 推进到更新的值时，不覆盖他们的 bool。
7. 绑定成功后，仍在权威锁内做最后一次完整复验，先写入 typed continuation，再写 Approved、一份 grant，以及来自 Bound 身份的范围记忆。重复批准最终只有这份 grant 和这一次续行。

准备有精确 owner。Stop 在 run 尚不存在时也能取消它。迟到候选由原 owner fence、abort 和 join，不交给当前 owner 执行。归属不靠 runId 前缀扫描。

用户从设置关掉 feature 时，epoch 增加，并取消当前 request、grant 和未消费的 continuation，再 fence。此后到达的旧准备在步骤 4 或步骤 6 失败。

卡片属于领域组件和 15 个 locale 的 catalog，走现有 dialog 规则，不使用 `window.confirm` / `prompt` / `alert`。文案本身是 U03。

### 4.3 一个模型工具，三种 tagged 动作

模型入口只增加 `computer_request_access`。正文是 tagged union。未知字段拒绝。模型提交的 task、run、request id 不产生授权。

```json
{ "action": "request", "purpose": "1..=200 chars", "surfaces": ["desktop"], "capabilities": ["observe"], "scopeIntent": "1..=200 chars" }
{ "action": "status" }
{ "action": "cancel" }
```

`request` 必填 purpose、surfaces、capabilities、scopeIntent。`status` 和 `cancel` 没有这些字段，也不接受模型补上的 id。查询和取消只作用于当前有效 agent incarnation 上的那一条活跃申请。对不上当前 incarnation 时拒绝，并且不新建申请。

surface 只使用 `SurfaceKind::as_wire` 已经发出的四个 kebab-case 值：`desktop`、`managed-browser`、`existing-tabs`、`app-webview`。模型合同和卡片回显只用这四个。`from_wire` 今天还接受别名 `browser`、`existing-tab`、`webview`；别名可以解析，不作为合同输出，也不再发明 `managed_browser`、`existing_tab` 这类下划线名字。

capabilities 的合同值是 `observe` 和 `input`。空数组、重复值、未知值拒绝。

内部状态经过 Approving。模型可见字符串在 finalize 之前保持 `pending`。`approved` 只出现在 commit 和真实资源绑定都成功之后。点击和准备开始都不是 approved。deny、cancel、expire 与批准范围记忆分开保存。

去重键是：

- Host 生成的 `taskId`
- canonical intent：purpose 的 UTF-8 字节、按 `as_wire` 排序后的 surfaces、排序后的 capabilities、scopeIntent 的 UTF-8 字节
- 申请来源的 connection generation

pending 期间键相同，返回原来的 `requestId`。purpose 或 scopeIntent 不同就不合并。surfaces 或 capabilities 变宽或变窄都使旧卡 revision 失效：revision 增加，旧 revision 不能 commit。已经 commit 的 grant 不被更宽的调用扩大。范围外的新请求是一条增量 pending；旧 grant 继续有效，新 revision 在自己的 commit 之前不能执行。

相同的 surface 枚举和 capability 枚举不是相同范围。Chrome 与 Excel、或同一个应用的另一个实例，枚举可以一样。它们靠不同的 scopeIntent 区分，不能并成一条申请。scopeIntent 只参与去重和展示。

Host 批准的范围是下面的类型。`Pending` 不能执行。`Bound` 只由 Host 写入。模型的 scopeIntent 字符串没有到 `OpaqueIdentity` 的转换。U01 测试用 Host 侧测试构造器，不从模型 JSON 反序列化出身份。UI 和原生解析是后续批次。

```rust
enum ScopeResolution {
    Pending,
    Bound { member_id: HostId, identity: OpaqueIdentity },
}
```

申请 TTL 10 分钟，grant 空闲 TTL 30 分钟。时钟只用 Host 的 monotonic `Instant`。空闲计时在等待批准或 cleanup 未完成时暂停。正在执行的任务不因为几步操作就重授权。到期不批准、不执行。两个期限都不是永久权限。90 秒延迟批准的产品测试放在 U07，必须真实等待；U00 的 20 秒等待只证明了 CLI 权限框，不代替那项测试。

### 4.4 Host 决定和一次性续行

`computer_request_access` 在返回 pending 前不得持有 Broker 执行锁、connect lock、action admission 或 desktop lease，也不得把 15 秒 HTTP 拉长。用户思考期间模型 turn 可以结束。

commit / deny / cancel 是 Host 函数，不是模型工具。commit 使用第 4.2 节的预约。校验通过后才进入 `approving`，然后准备资源。

只有 commit 成功才发一张 `ContinuationTicket`：

- 绑定 app session、agent、incarnation、connection generation、owner nonce、request id、task id、grant revision、feature epoch，以及一次性消费身份。
- claim 按 app session 找到未消费票据并返回这份类型，不消费，也不只返回裸字符串。consume 在发送前按权威绑定复验。第二次 consume 失败。查询 Approved 不再另发票据。
- 账本只允许消费一次。未消费的有效票据不会因为其它请求 flood 被丢掉。
- `prompt_in_flight` 或通用 CLI permission / plan / ask gate 还开着时，票据排队。`turn.rs` 已经拒绝忙时的第二条 `session/prompt`，U02 继续用这个条件，不在忙时再发一条。
- 队列排到边界后，Host 发送一条新的 `session/prompt`。正文是固定句子，包含 request id 和 revision，告诉模型范围已批准、先列范围内目标，并且不能调用 authorize / resume / reconnect。拒绝、取消和过期使用另一句固定句子，要求停止，不能重试申请。
- 不新建聊天，不重放用户原话，不把消息送到别的 session。
- 发送前再核对 generation 和 epoch。不匹配就丢弃票据，不发送。

模型拿到 pending 后停止电脑操作。它可以用 `action: status` 回读，Host 不靠模型轮询来推进。

### 4.5 范围、暂停和一键继续

`TaskGrant` 保存用户批准的 `ScopeResolution`、能力和第 1 节的 owner 绑定。`RunState` 只保存当前执行目标。集合内切换目标或普通导航时，grant 留下，`drop_execution_identity` 清掉旧 snapshot / ref / geometry，然后重新观察。

暂停同样留下 grant，清掉执行身份和 lease。模型不能调用 `computer_resume`。以后的一键继续是 Host 点击：复验 grant 仍属于这个 App session 和当前 agent 策略、目标身份仍在、feature epoch 仍是授予时的世代或更新但仍为开、物理占用空闲，然后走现有 resume 的 generation 递增和 fresh observe。换过 agent incarnation 的 scope 记忆不能直接变成新的执行 grant。U01 先把“范围记录还在、执行身份不能用”写成新合同。测试 `resume_drops_authorization_snapshot_and_lease` 改名并保留前半句，不再要求范围记录消失。

compact、换模型、ACP 重连：作废 transport credential、执行身份和未发送的 continuation。同一 App session 可以留下非执行 scope 记忆。不能后台复活。用户要点一次“继续此任务”。

Stop、撤权、feature-off、任务完成、到期、换 agent、connection rotation 和失败回滚：按确切 task、run 或 preparation 先 fence，再把物理收尾交给原有 cleanup 票据。`consent_stop` 的回执不等待慢 abort。`stop_requested` 和物理 `stopped` 保持分开。grant 数变成 0 或 snapshot 被清掉，都不证明旧 action 已终止，也不证明 lease 可以交给下一个 owner。原 cleanup 不能退休同名的新 owner 或新 run。旧 ref 不重放，unknown 副作用不重放。App 重启、fork、账号、workspace root、remote 身份变化不继承活 grant。远程 IM、SSH 和定时任务不继承本地申请权。

### 4.6 通用 CLI 确认里的 CU 回执

Computer Use 的任务批准不进入 `SessionAllowCache`，不调用 `resolve_permission`，不修改 CLI `--permission-mode`，不传递 `--always-approve`。shell、文件和其他 MCP 的确认保持原样。

CU 工具在这条 CLI 上仍会发出 `session/request_permission`。Host 在回执前做 CU-only 判断。判断只认第 3.1 节看见的结构，并和 Host 自己登记的目录项做全等比较。

一条权限进入 CU 分支，当且仅当下面六条同时成立：

1. `toolCall._meta["x.ai/tool"].name` 是 `use_tool`。
2. `toolCall.rawInput` 是对象。
3. `toolCall.rawInput.variant` 是 `UseTool`。
4. `toolCall.rawInput.tool_name` 含且只按第一个 `__` 拆成非空 server 和非空 tool。
5. server 全等于 Host 写进当前 agent incarnation、当前 connection generation 的目录项名字。比较对象是 Host 登记簿，不是模型提示词里的字符串，也不是 `tool_input`。
6. tool 全等于该登记项上的一个具体工具名。

任一条件失败，这条权限对 CU 分支是 Unidentified。CU 分支不回复 allow-once。title 相等、title 里出现服务器名、`use_tool:*`、用户能猜到的公开名字、`--always-approve`、allow-all-MCP，都不满足这六条。

`computer_request_access` 命中这六条时，Host 可以回复 `allow-once`，即使还没有 `TaskGrant`。这个回执只让申请调用到达 Host。它不写缓存，不选择 `always-allow` 或 `allow-always-mcp`，不调用 `allow_tool_family`。

其他 CU 工具命中这六条时，只有当前 `TaskGrant` 已 commit，并且该工具实际要核的身份成立，才回复 `allow-once`。status、list、open 和第一次 observe 不要求尚不存在的 snapshot。act / wait 仍要求当前 run、target、snapshot、ref 和 geometry。内容读取还要 `observe`，输入还要 `input`。feature epoch 与 grant 一致、agent incarnation 与 connection generation 一致，且 Stop / revoke 未落下。否则回复 `reject-once`；卡片已经结束时回复 cancelled。不把这条权限交回通用允许路径。这条 CLI 回执本身是 U02；U01 先把同样的分型做进 core gate。

shell、文件、以及其他 server 的 MCP 在第 5 条失败，继续走现有权限框。

通用回执发出之后，`tools::dispatch` 仍执行 core gate。Stop 或 revoke 若在回执之后、工具体之前落下，调用拒绝。探针里 cancel 之后的迟到 allow-once 没有增加 fixture 调用，说明 CLI 通道会丢掉已取消 turn 的后续执行；产品 core gate 是同一竞争的 Host 合同。

若以后的 CLI 只剩 title、没有 `rawInput.variant` 和 `rawInput.tool_name`，判断结果是 Unidentified，申请工具也不自动 allow-once。本批不假设那样的字段已经存在，也不写一个依赖自由标题的匹配器。core 的 transport gate 在那种情况下仍拒绝执行调用。

## 5. `use_tool:*` 与 CU 回执的边界

代码事实：

- 这次线上权限的 `_meta["x.ai/tool"].name` 是 `use_tool`。`decode_permission_request` 会优先采用这个名字，于是 Host 工具名变成包装器，内部限定名留在 title 或 `rawInput`。
- `scope_key` 是 `tool_name + ":" + normalize(target)`。没有路径时用 title。title 为空时 `normalize_scope_target` 返回 `*`，于是 key 变成 `use_tool:*`。
- `SessionAllowCache::is_allowed` 命中精确 key，或者命中 `{tool}:*`。
- Ask 策略下，`allow_session` 只写入 UI 传来的那条 key。`should_widen_session_allow` 仅在 `AllowForSession` 或 `Auto` 时再写 `{tool}:*`。
- 一旦 `use_tool:*` 进了缓存，后来的每一个 `use_tool` 范围都命中。`may_auto_allow` 在 Ask 下也会因为缓存命中自动回复 `allow-once`。
- `fallback_always_allow_for_tool("use_tool")` 仍会猜 `allow-always-mcp`。CLI 1.0.46 这次实际发布的 id 是 `always-allow`。

CU 回执不写这些 key。任务批准的用户点击是第 4.2 节的 Host 卡。探针里每次 `use_tool` 一张 CLI 卡，是通用权限的行为；CU 分支用第 4.6 节把它收成 allow-once 或拒绝，不把它收成会话级放行。

## 6. U01 和 U02 要改的接口

### U01：core 类型和受限 IPC

新类型放在 `computer-use-core`，和 `protocol` golden 一起导出：

- `AccessAction`：tagged `Request { purpose, surfaces, capabilities, scope_intent }`、`Status`、`Cancel`。
- `AccessRequest`：`request_id`、`task_id`、`app_session_id`、`agent_session_id`、`connection_generation`、`feature_epoch`、`purpose`、`surfaces`、`capabilities`、`scope_intent`、`state`、`revision`、`needs_enable`、`deadline`。
- `TaskGrant`：`task_id`、`app_session_id`、`agent_session_id`、`connection_generation`、`feature_epoch`、`revision`、`surfaces`、`capabilities`、`scope: ScopeResolution`、`idle_deadline`、`execution_run_id: Option<String>`。
- `ScopeResolution` / `OpaqueIdentity`：按第 4.3 节。模型 JSON 不能构造 `OpaqueIdentity`。
- `ContinuationTicket`：第 4.4 节的绑定字段加 `consumed`。
- `SessionTransportCredential`：第 1 节的五个绑定字段加 token。token 不出现在 `Debug` 的工具结果路径上。
- `ApproveReservation`：第 4.2 节的字段。
- `FeatureEpoch`：`u64`，与现有 feature bool 一起更新。

`MODEL_TOOLS` 增加 `computer_request_access`。它在 `dispatch` 里走独立分支：核对 transport credential 的 session、agent incarnation 和 connection generation；不调用 `require_owner`；不调用 `authorized_target`；feature 关闭时仍返回 pending。执行工具保持 owner、feature、目标检查，并增加 grant、binding 和 epoch 检查。同一个 token 在没有当前 grant 时，执行工具失败。

`IpcServer::issue_transport_credential` 只向 Broker 取 token，不写入 legacy credential map。退休用 `rotate_connection` 和 `retire_owner`，不另加一个会删掉 scope 记忆的 revoke。`credential_for_session` 仍只服务已有 run 的旧目录；重建它不得替换 transport token。Retired token 返回 401，不退回 legacy map。Unknown token 继续走旧 map。执行路由是：token → 当前 owner → 当前 `TaskGrant` → 该工具实际要核的 run / target / snapshot / ref。

Host 函数：`commit_access_request`、`deny_access_request`、`cancel_access_request`。它们不是 `MODEL_TOOLS`。重复 commit 只成功一次。伪造 session、agent、run、task、request、revision 或 epoch 拒绝。关闭的旧卡在预约步骤 2 失败，功能保持原状。

第一条纵向路径：无 run、feature 关闭、本地有效 Host owner 的 `computer_request_access` 经 IPC 返回 pending，adapter 调用数为 0。Pending 不能 commit。只有测试 Host helper 先写成 synthetic `Bound`，commit 才写 `TaskGrant` 并允许范围内的第一次 observe。feature-off 之后的迟到 commit、伪造 revision、Stop 之后的 commit 都不建立可执行 grant，也不发 continuation，也不在失败路径上把 feature 从关变成开。模型回复含 state、requestId、revision、authority、purpose、scopeIntent、surfaces、capabilities，不含 taskId 和 bearer。

最小失败场景：现有 `dispatch` 在没有 owner 时直接失败；`mcp_acp_entry_with` 在没有 authorized target 时失败。U01 的第一个新测试应当先红在这条路径上，再加独立分支。测试数据要标明不是真人批准。

要改的测试合同，不能只删断言：

- `computer-use-core` 的 `protocol_golden.rs` `catalog_matches_rust_constants`：目录包含新工具，surface 合同值是四个 kebab-case 名字。
- `ipc/tests.rs` `catalog_rebuild_reuses_the_live_session_credential`：执行 token 的稳定性和 transport token 互不覆盖。
- `broker/tests_identity.rs` `resume_drops_authorization_snapshot_and_lease`：执行身份作废，任务范围记录留下且不能直接观察；新 agent incarnation 不继承执行 grant。
- `tools.rs` 里“模型工具都要 owner”的断言：申请工具是唯一例外，执行工具仍然要 owner。
- 新增：无 grant 时同一 transport token 的执行工具失败；工具结果 JSON 不含 bearer；agent 变化后旧 token 退休；旧卡预约不调用 enable；更新的用户 epoch 不被失败回滚关掉；Stop 在 approving 期间 fence；不同 scopeIntent 不去重；模型 scopeIntent 不能变成 `OpaqueIdentity`；`status` / `cancel` 拒绝 purpose 和模型 id。
- 第 4.6 节的匹配用夹具权限 JSON：`variant` + `tool_name` + `_meta` 名字命中登记簿才 allow-once；只有 title 时不回复 allow-once；shell / 其他 MCP 不进入该分支。

### U02：稳定目录和排队续行

`build_catalog` 对本地 interactive 会话始终保留 `grok-computer-use` 申请项，即使 `run_id` 是 `None`。执行项不再靠“有 run 才出现”来授权；授权在 dispatch 和当前 binding。整表替换必须保留无关 MCP 和 official-aux。

`attach_computer_use` 的 busy 拒绝保留，给仍然需要替换执行目录的路径使用。申请工具不依赖这次替换，因为会话打开时已经在目录里。

Host 在发出 CU allow-once 之前用第 4.6 节判断。判断读的是 ACP 权限里的 `rawInput` 和 `_meta`，对照 Host 登记簿。通用 `resolve_permission` 不接收 CU 的 allow-once 结果作为 session allow。

续行由 session manager 在 prompt 边界发送，规则见 4.4。MCP instructions 改为：需要电脑时先 `computer_request_access`；pending 期间停止派发；scope 内才观察和动作；authorize / resume / reconnect 仍不是模型工具。

U02 测试：

- `src-tauri/src/session_manager/computer_use/tests/catalog.rs`：`run_id == None` 时目录仍有申请项，无关 server 还在；现有“desired absent 最终去掉执行权限”的断言保留，改成执行调用失败而不是目录里完全没有申请工具。`stop_during_mcp_update_converges_to_absent` 与 `base_extension_revision_race_preserves_latest_base_and_cu_entry` 继续约束整表替换。
- `src-tauri/src/session_manager/computer_use/lifecycle.rs` 与 `computer_use/tests/lifecycle.rs`：Stop 仍 fence 执行和 continuation；session 退出撤销 transport credential。
- 新测试：continuation 在 `prompt_in_flight` 时不发送，边界到达后只发送一次；agent session 变化后票据作废。
- `ComputerPanel.test.tsx` 的 Stop / pending 合同先不改行为。U03 才接卡片。U02 只保证新的 pending 状态不会被面板误画成 running。

## 7. 本修订没有提前完成的事

U03 仍负责卡片文案和 15 个语言。U05 才把已存在的 grant 接到一键继续。原生窗口解析、Chrome 与 Excel 的真实身份采集，本批只冻结 `ScopeResolution`，不实现解析器。

U00-R 的探针证明了 CLI 权限字段和通用确认时序。它没有证明 App 已经发出 CU allow-once。U01-R 在 core 里实现了 feature epoch、两段 commit 和 typed continuation。第 4.6 节的六条匹配和实际 prompt 发送仍是 U02。多目标解析是 U04。卡片是 U03。一键继续是 U05。本批的 core 绿灯不是完整 Computer Use 交付。

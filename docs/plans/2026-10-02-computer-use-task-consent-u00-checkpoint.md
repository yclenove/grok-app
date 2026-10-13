# U00 checkpoint：任务授意事实与 ADR

状态：partial。只完成 U00 的事实采集。Computer Use 未完成。`productionAppVerified: false`。
Codex 审查结论是 needs_rework。本文件保留当时的时间表，并修正下面三处过宽的说法：退出码 1 的来源和时刻、continue 探针“10 条断言全真”、以及 writer pid 37260 仍在运行。返修结果在 `docs/plans/2026-10-02-computer-use-task-consent-u00-rework-checkpoint.md`。本批没有修改产品源码，没有 commit，没有启动或重启 Ubuntu VM，没有恢复外层 writer。

- 设计：`docs/plans/2026-10-02-computer-use-task-consent-adr.md`
- 证据根：`tools/computer-use-probe/.run/task-consent-u00-20261002/`
- 旧日志未覆盖：`grok-cli-20261002/work`、`gnome-pending-consent-20261001`、`ubuntu-gnome-vm-20261001` 都还在。

## 基线

| 项 | 值 |
| --- | --- |
| HEAD | `8d8b71c262ab222748bca32412eea884d4b892e0` |
| 分支 | `feat/computer-use-implementation` |
| 产品树 | `src`、`src-tauri`、`tools/computer-use-mcp` 无 diff |
| CLI | `C:\Users\Administrator\.grok\bin\grok.exe` |
| 版本 | `grok 1.0.46 (2765805b9442)` |
| 外层 writer | 写本表时 owner 记录 pid 37260 为 live，命令含 session resume 和 `--always-approve`。该进程后来退出码 0。探针没有使用它。本行不表示它在返修时仍在运行 |
| 已读约束 | `AGENTS.md`、`CONTRIBUTING.md`、计划全文，以及 wiki 的 computer-use、i18n、dialogs、settings、catalog、maintain 中和本批有关的条目 |

wiki 约束已经写进 ADR：15 locale、禁止 `window.confirm`、feature 默认关、模型 catalog 不负责授权。`docs/llm-wiki/computer-use.md` 仍是旧的 GNOME 证据，不是这条授权链的现状。

## 命令

1. `git rev-parse HEAD`、`git status --short`。退出码 0。HEAD 同上，产品路径无改动。
2. `& 'C:\Users\Administrator\.grok\bin\grok.exe' --version`。退出码 0。输出 `grok 1.0.46 (2765805b9442)`。
3. `node ...\task-consent-u00-20261002\probe.mjs`。Node `v24.15.0`。退出码 0。
4. 第一次 configured 子进程 pid 67024 退出码 1。来源是 writer 工具调用 `call-08e8209c-e63c-43b3-bf00-b9aee69da3b7-65` 的 `taskkill /PID 67024 /T /F`。权限测试回执在 tMs 32120，prompt id 4 的 `end_turn` 在 tMs 43958，prompt id 5 已在 tMs 43960 写出，进程退出在 tMs 44096。终止晚于那次 20 秒回执。这不是已证的产品崩溃。被结束的是这次探针子进程。
5. `node ...\task-consent-u00-20261002\probe-continue.mjs`。脚本固定 `process.exit(0)`，所以退出码 0 不代表断言机器会在失败时变红。

两次探针的 argv 都是：

```text
grok --no-auto-update --permission-mode default --no-subagents agent --no-leader --model grok-4.7 --reasoning-effort low stdio
```

没有 `--always-approve`，没有 `--resume`。工作目录是探针 fixture。环境关掉了 Claude / Cursor / managed MCP，并删除了 `GROK_SESSION_ID` 和 `MYSQL_*`。空 home 与已配置 home 都没有读取或复制 `auth.json`。

## 消息时序

### attempt-fresh-home

进程 pid 65384，退出码 0，用时 16701ms。断言 3 条：`initialize-ok` 真，`authenticate-cached-token` 假，`attempt-error` 假。

| tMs | 事件 |
| --- | --- |
| 47 | client `initialize` id 1 |
| 2178 | result。`protocolVersion` 1，`agentVersion` 1.0.46，`loadSession` true |
| 2181 | client `authenticate` `{ methodId: "cached_token" }` id 2 |
| 14199 | 12 秒超时，没有 result |
| 14201 | client `session/new`，目录只有固定字符串 fixture |
| 14567 | error `-32000`，`Authentication required`，data `no auth method id provided` |
| 16699 | 进程退出 0 |

这是本批保留的 firstFailure：没有凭据的 home 不能建立 ACP 会话，因此也不能在该 home 上注册申请工具。后面的成功会话没有覆盖这条记录。

### attempt-configured-home

进程 pid 67024，会话 `01a0fd3e-f796-7360-a69b-5903263756d1`，退出码 1，用时 44097ms。报告里四条布尔值为真：`initialize-ok`、`authenticate-cached-token`、`session-new`、`prompt1-completed`。其中 `initialize-ok` 和 `prompt1-completed` 在脚本里把 ok 写成常量 true；`session-new` 检查了 session id；认证失败分支会记 false。两个探针末尾都是 `process.exit(0)`。退出码 1 只来自上一条记录的 taskkill。

| tMs | 事件 |
| --- | --- |
| 241 | initialize 成功，版本和 loadSession 同上 |
| 248 | cached_token 成功 |
| 1056 | `session/new` 成功。没有 Computer Use run |
| 1057 | `session/prompt` id 4 开始 |
| 12107 | `session/request_permission` id 0。工具名 `use_tool`，标题 `fixture_fixed_string`。选项 `always-allow` / `allow-once` / `reject-once` |
| 32120 | 测试数据选择 `always-allow`。距离请求 20013ms。不是真人批准 |
| 39605 | 同一 prompt 再次请求权限。标题 `u00-fixed-fixture__fixture_fixed_string`。选项相同 |
| 39607 | 测试数据选择 `allow-once` |
| 41414 | 第三次权限。标题 `u00-fixed-fixture__fixture_other_tool` |
| 41416 | 测试数据选择 `allow-once` |
| 43956 | `_x.ai/session/prompt_complete` |
| 43958 | prompt id 4 result，`stopReason: end_turn` |
| 43960 | 下一条 `session/prompt` id 5 已写出 |
| 44096 | taskkill，退出码 1 |

fixture 只记录了两次调用，结果分别是 `U00_FIXED_STRING` 和 `U00_OTHER_FIXED`，参数被忽略。stderr 在 15:33:09Z 记录不合格名称 `fixture_fixed_string` 被 CLI 拒绝，并提示必须使用 `server__tool`。`always-allow` 之后，两条合格调用仍然各自请求权限。

prompt 2、忙时目录替换、cancel 和 EOF 在这条进程里没有跑完。下一条探针换了规则：不再选择 `always-allow`。它不是这条记录的重跑。

### attempt-continue

进程 pid 75644，会话 `01a0fd42-e9f1-7d73-bf71-9917da286213`，脚本退出码 0。报告里 10 条布尔值都是 true。`overlapping-prompt`、`update-mcp-during-prompt`、`cancel-then-late`、`eof-while-pending` 在 `probe-continue.mjs` 里把 ok 写成常量 true。下面的时间表来自 `attempt-continue.ndjson` 和报告 detail，和独立阅读一致；它不是 10 项自动化验收已经关闭。失败路径当时也会留下绿色退出码。决策只有测试用的 `allow-once` 或取消，文件里 `selectsAlwaysAllow: false`。U00-R 用真实谓词重跑了这四类检查。

| tMs | 事件 |
| --- | --- |
| 222 | initialize 成功，`loadSession` true，agent `1.0.46` |
| 231 | cached_token 成功 |
| 1145 | `session/new` 成功 |
| 1549 | `_x.ai/mcp/server_status` 名字只有 `u00-fixed-fixture` |
| 13278 | 权限 id 0，标题 `fixture_other_tool`，测试数据 `allow-once` |
| 18478 | 权限 id 1，标题 `u00-fixed-fixture__fixture_other_tool`，测试数据 `allow-once` |
| 20716 | prompt A `end_turn`。同一 turn 有 2 次权限 |
| 20730 | prompt B 已发出后立刻发送 `_x.ai/session/update_mcp_servers`，目录含原 fixture 和 `u00-fixed-extra` |
| 21026 | update RPC 成功，keys `["result"]`。此时 prompt B 还未结束 |
| 21055 附近 | server status 出现 `u00-fixed-extra` |
| 29220 | prompt B 仍请求权限，标题 `u00-fixed-fixture__fixture_fixed_string`，测试数据 `allow-once` |
| 32936 | prompt B `end_turn` |
| 35359 | 重叠的第二条 `session/prompt` 也 `end_turn`。没有 `task_already_running` |
| 41651 | cancel 阶段权限 id 3 |
| 41655 | 发出 `session/cancel`。此前 fixture 调用数 2 |
| 41664 | 原 prompt result，`stopReason: cancelled` |
| 43175 | 迟到的测试 `allow-once` |
| 43684 | fixture 调用数仍是 2 |
| 46522 | EOF 阶段权限 id 4，探针不回复并关闭 stdin |
| 49539 | 进程退出 0 |
| 73715 | EOF prompt 在客户端侧 30 秒超时。fixture 调用数仍是 2 |

EOF 的 stderr 原文是：`failed to request permission` / `unable to receive 'session/request_permission' response, channel closed`，data `xaiAcpChannelFailure=recv_failed`。

两次 configured 运行的 stderr 开头都有两条 `failed to parse MCP JSON` / `expected value at line 1 column 1`。fixture 随后完成了 initialize 和 `tools/list`。这更像子进程 stdout 上的非 JSON 首行，不改变上面的权限时序。

## implemented 与 verified

| 项 | implemented | verified |
| --- | --- | --- |
| 产品申请工具、TaskGrant、续行 | 否。本批禁止改产品 | 否 |
| CLI 1.0.46 initialize / loadSession capability | 不适用 | 是，两次会话 |
| 无凭据 home 的 session/new | 不适用 | 是，失败被保留 |
| 无 run 时用 `session/new.mcpServers` 注册无权 fixture | 不适用 | 是。这是 CLI 能力，不是 App 已经这么做 |
| 20 秒后回复同一条 `session/request_permission`，原 prompt 继续 | 不适用 | 是。测试数据 |
| `allow-once` 不抑制下一次 `use_tool` | 不适用 | 是。同 turn 和下一 turn 都再请求 |
| `always-allow` 对后续合格工具的影响 | 不适用 | 只观察到失败的不合格调用之后，后续调用仍要权限。不再外推 |
| 忙时 `update_mcp_servers` | 不适用 | 这一次返回成功 |
| 重叠 `session/prompt` | 不适用 | 这一次没有报错，后完成 |
| cancel 后迟到 allow-once | 不适用 | fixture 没有新调用 |
| 未回复即 EOF | 不适用 | 通道关闭，fixture 没有新调用 |
| Host `tool:*` 缓存的实际 App 会话 | 代码已读 | 没有拿真人会话做缓存实验。结论来自代码加线上的 `use_tool` 名字 |
| 真人点击、系统权限、桌面内容 | 否 | 否 |

上表里 CLI 时序标成 verified 的，依据是日志里的 tMs、stopReason 和 fixture 调用数。`probe.mjs` / `probe-continue.mjs` 的若干 `assert(..., true)` 和固定 `process.exit(0)` 没有把失败路径跑成非零退出。那一列不是 10 项自动化验收关闭，也不是真人批准、90 秒 App 等待、production scope 或跨平台完成。

## 未验证的真人和原生环节

- 没有真人批准 App Computer Use，也没有点击 UAC、portal、防火墙或浏览器 Share。
- 没有截屏，没有读用户窗口或标签页。
- 没有 `session/load` 一条旧会话。只确认 capability 为 true。
- 没有证明忙时目录替换会保留无关的用户 MCP。探针目录里只有自己的 fixture；Host 源码要求整表替换时自己把无关项带上。
- 没有证明重叠 prompt 在所有时序下都不会返回 `task_already_running`。Host 忙时拒绝仍然保留。
- GNOME pending-consent 的旧 VM 没有重启，那些结论也不在本批里更新。

## 推荐下一批

只做 U01。入口和测试名单在 ADR 第 6 节。第一条应该是一条先失败的 core 测试：没有 owner、没有 authorized target 时，`computer_request_access` 今天走 `dispatch` 的 `require_owner` 失败；修完后返回 pending，且 adapter 调用数为 0。

精确源码入口：

- `src-tauri/computer-use-core/src/tools.rs`：`MODEL_TOOLS`、`dispatch_with_stop_handler`
- `src-tauri/computer-use-core/src/ipc.rs`：`validate_session_binding`、`credential_for_session`
- `src-tauri/computer-use-core/src/session_grants.rs`：`Slot`、`SessionGrants`
- `src-tauri/computer-use-core/src/broker.rs`：`drop_execution_identity`
- `src-tauri/computer-use-core/src/broker/resume.rs`：`resume`
- `src-tauri/computer-use-core/src/broker/tests_identity.rs`：`resume_drops_authorization_snapshot_and_lease`
- `src-tauri/src/computer_use/inject.rs`：`mcp_acp_entry_with`
- `src-tauri/src/computer_use/mod.rs`：`ensure_host_runtime`
- `src-tauri/src/session_manager/computer_use.rs`：`build_catalog`、`attach_computer_use`
- `src-tauri/src/session_manager/process.rs`：`live_session_is_busy`
- `src-tauri/src/session_manager/turn.rs`：`prompt_in_flight` 时拒绝第二条 prompt
- `tools/computer-use-mcp/server.mjs`：固定目录和 15 秒超时
- `src-tauri/src/permission.rs`：`scope_key`、`SessionAllowCache`、`should_widen_session_allow`、`fallback_always_allow_for_tool`
- `src-tauri/src/session_manager/control.rs`：`resolve_permission`
- `src-tauri/src/session_manager/events.rs`：权限缓存和自动 `allow-once`

U02 依赖 U01 的类型，不在本批开始。

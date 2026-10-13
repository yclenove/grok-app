# U00-R checkpoint：架构通路与证据返修

状态：partial。只完成 R1–R5 的设计和 owned probe。Computer Use 未完成。`productionAppVerified: false`。没有进入 U01，没有修改产品源码，没有 commit。
ADR：`docs/plans/2026-10-02-computer-use-task-consent-adr.md`（修订说明在文首；先前 unresolved 没有写成早已解决）。
原 U00 checkpoint 的范围修正：`docs/plans/2026-10-02-computer-use-task-consent-u00-checkpoint.md`。

基线 HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`，分支 `feat/computer-use-implementation`。`git status --short` 对 `src`、`src-tauri`、`tools/computer-use-mcp` 无输出。

## 保留的首败

`tools/computer-use-probe/.run/task-consent-u00-20261002/` 的两轮日志没有覆盖，也没有重跑。

- 空 home：pid 65384，`authenticate` 12 秒无响应，`session/new` `-32000 Authentication required`。这仍是 firstFailure。
- configured pid 67024 退出码 1 来自 writer 工具调用 `call-08e8209c-e63c-43b3-bf00-b9aee69da3b7-65` 的 `taskkill`。回执 tMs 32120，prompt id 4 `end_turn` tMs 43958，prompt id 5 已写出 tMs 43960，进程退出 tMs 44096。晚于 20 秒测试回执。不是已证产品崩溃。
- continue pid 75644 的四条常量 true 断言没有被当成验收。时间表仍以原 ndjson 为准。

## 本批命令

CLI 仍是 `C:\Users\Administrator\.grok\bin\grok.exe` 1.0.46。没有升级 CLI，没有重启 VM，没有改用户代理。`GROK_HOME` 使用已配置的隔离目录，没有读取或修改 `auth.json` / `config`。argv：

```text
grok --no-auto-update --permission-mode default --no-subagents agent --no-leader --model grok-4.7 --reasoning-effort low stdio
```

没有 `--always-approve`，没有恢复 writer 会话。fixture 名 `u00r-fixed__fixture_fixed_string` 与 `u00r-fixed__fixture_other_tool`，只返回 `U00R_FIXED_STRING` / `U00R_OTHER_FIXED`。决策类是 `test-data-not-a-human-or-production-grant`。

| 命令 | 结果 |
| --- | --- |
| 在仓库根目录 `node --check probe.mjs` | 退出码 1，`MODULE_NOT_FOUND`。没有启动 grok |
| `node probe.mjs`（第一次，目录 `task-consent-u00r-20261002`） | 探针退出码 0。CLI pid 52168，会话 `01a0fd5a-3299-7743-834f-50f742569ba9`，进程退出码 0。oracle 8/8。update / overlap 的 tMs 是 `Promise.all` 之后的记笔记顺序，不作为时序验收。日志原样留在 `attempt-1/` |
| `node probe.mjs`（第二次，回执时间） | 探针退出码 0。CLI pid 76920，会话 `01a0fd60-1590-7ff2-8085-40d09473e420`，`initialize` agent `1.0.46`、`loadSession` true。进程退出码 0。oracle 9/9，退出码跟随 oracle |
| `node negative.mjs` | 退出码 0。含义是六处测试数据改动都让 oracle 变红，并且子进程对基线退出 0、对缺权限事件退出 1。不是另一条 CLI 授权 |

stderr 仍有 `failed to parse MCP JSON` / `expected value at line 1 column 1`，以及 EOF 时的 `unable to receive 'session/request_permission' response, channel closed`。fixture 完成了两次固定字符串调用。这些警告不改变下面的谓词。

## 第二次运行的真实谓词

oracle 不读取预写的 ok。缺事件为失败。文件 `oracle-report.json`，`ok: true`，`passed: 9`，`total: 9`，`cliExit.code: 0`，`probeError: null`。

| 谓词 | 断言依据 |
| --- | --- |
| `permission-raw-saved` | 4 条权限原文，字节 923 / 919 / 928 / 921 |
| `qualified-permission-captured` | 4 条都含 `u00r-fixed__`，`titleOnly` false |
| `structured-use-tool-identity` | `metaName` `use_tool`，`rawVariant` `UseTool`，`rawToolName` `u00r-fixed__fixture_fixed_string` |
| `answers-are-allow-once-test-data` | 3 次回答都是 `allow-once`，决策类为测试数据。EOF 那条没有回答 |
| `cross-turn-asked-again` | prompt A `end_turn`；cross-turn-b 仍有 1 条权限 |
| `update-mcp-during-prompt` | rpc 7 发送 tMs 12809，回执 tMs 12999 且 ok，prompt B rpc 5 回执 tMs 24822 |
| `overlap-ordered-end-turn` | prompt B tMs 24822 `end_turn`；重叠 rpc 6 tMs 27599 `end_turn`。没有 `task_already_running` |
| `cancel-then-late-no-extra-call` | cancel tMs 30322，rpc 回执 tMs 30323 `cancelled`，迟到 allow-once tMs 31839，fixture 调用数保持 2 |
| `eof-no-extra-call` | 未回答即关闭 stdin，fixture 日志调用数 2，前后调用数相等，CLI 退出码 0 |

fixture 两次调用的结果是 `U00R_FIXED_STRING` 和 `U00R_OTHER_FIXED`，`argumentsIgnored: true`。

权限原文的身份字段（四条同形）：`toolCall.title` 与 `toolCall.rawInput.tool_name` 都是限定名；`rawInput.variant` 是 `UseTool`；`_meta["x.ai/tool"].name` 是 `use_tool`。权限上没有单独的 server 字段。`rawOutput.server_name` 只出现在完成后的 `tool_call_update`，不作为允许回执的输入。

## 负向

`negative-report.json`：`baseSource` `cli-events.ndjson`，`provesRunnerGoesRed` true，`childExit.baseExit` 0，`childExit.mutatedExit` 1。

| 改动 | 变红的谓词 |
| --- | --- |
| 去掉 `session-cancel` | `cancel-then-late-no-extra-call` |
| 把迟到的 fixture 调用数加一 | `cancel-then-late-no-extra-call` |
| 去掉全部 `permission-raw` | `permission-raw-saved`、`qualified-permission-captured`、`structured-use-tool-identity`、`cross-turn-asked-again` |
| fixture 日志多一条 `tools/call` | `eof-no-extra-call` |
| 重叠 rpc 的 `stopReason` 改成 `cancelled` | `overlap-ordered-end-turn` |
| 去掉 cancel 并在事件上写 `ok: true` | `cancel-then-late-no-extra-call` |

没有为这张表重跑整套 suite，没有危险工具。

## SHA-256

| 文件 | sha256 |
| --- | --- |
| `events.ndjson` | `89981790a60848abacb55a38f09224815945c659615182d4294b88a22d732c07` |
| `oracle-report.json` | `c381264a6a43fff0d33800b9031836ae574a4f5cdcf05693d1c21d7f0552a0be` |
| `negative-report.json` | `aabb77488e172a9069ca8bea2c423719f20cdbe80c9f95dd6e59316e39cdc663` |
| `fixture-calls.ndjson` | `61ec1da02a4692db0f9d56a1f5ed1aabfc58a1e25a513521570fde95e4b92ba0` |
| `permission-raw/permission-0.json` | `dc7ecc3b68459fe86d845b1b018e5760f49780edf1d88fc55912c490d39e899c` |
| `permission-raw/permission-1.json` | `842c5200ebc0719bda6bcd3b8c635c697b41bd685ab3705e484ba21e7cd338f7` |
| `permission-raw/permission-2.json` | `21a3b13c781f863bfeb8dd5b744365fceb6f5aff7f6c6ff2d8b3f3ff3a5378e5` |
| `permission-raw/permission-3.json` | `1d9500c2c64e19c0bceb11f3ebcc3b99057a10b6a387f8efb40200d075f270fa` |
| `attempt-1/events.ndjson` | `28d6e295c7502e6e55e59513ecb5a9cb2a0d5d53450953891c6f01fd31cb0ed4` |
| `attempt-1/oracle-report.json` | `b6778b2053c628f3b58d0dd36df921b1706441abdb77ebe646a52802c8b45ba2` |
| `attempt-1/permission-raw/permission-0.json` | `47282d0d9733ab10557cd1916e87857312d8c95b8fc8dbdd445992f8451aa046` |

## R1–R5

| 项 | 结果 |
| --- | --- |
| R1 | ADR 4.6 写出 CU-only 判断：`_meta` 名 `use_tool`，`rawInput.variant` `UseTool`，`rawInput.tool_name` 按第一个 `__` 拆开后与 Host 登记簿全等。title、`use_tool:*`、公开服务器名、always-approve、allow-all-MCP 都不放行。申请工具可 allow-once；执行工具还要当前 grant 和 binding。回执之后 core gate 仍拒绝 Stop / revoke。若以后只剩 title，判断为 Unidentified |
| R2 | 采用 session transport credential，绑定 App session、agent incarnation、connection generation、feature epoch。token 只进 MCP 进程环境。执行路由查 Host 当前 TaskGrant / ExecutionBinding。不向模型导出 bearer。legacy `credential_for_session` 不得删掉 transport token |
| R3 | credential、request、grant、continuation 都带上述 owner 绑定。ACP 退出、重连、换模型退役旧凭据和执行 grant。同 App session 的非执行 scope 记忆不自动变成新 agent 的 grant。批准先做 revision + epoch + agent 预约；旧卡在打开 feature 之前失败。回滚不动更新的用户 epoch。Stop 在 approving 期间继续 fence |
| R4 | `request` / `status` / `cancel` 是 tagged union。surface 合同值是 `desktop`、`managed-browser`、`existing-tabs`、`app-webview`。去重键含 Host task、canonical intent 和来源 generation。相同枚举不是相同范围。批准范围是 `ScopeResolution`，模型 scopeIntent 不能变成 `OpaqueIdentity` |
| R5 | 受影响谓词已按回执时间重跑并通过。负向六处改动都变红，子进程退出码 1。原首败和原两轮日志保留。常量 true 与固定 exit 0 不再作为验收 |

## 仍未验证

- 没有真人批准 App Computer Use，没有点击 UAC、portal、防火墙或浏览器 Share。
- 没有 90 秒 App 等待，没有 production scope，没有跨平台完成。
- feature epoch、transport credential、CU allow-once 分支都还没有产品实现。探针里的 allow-once 是测试数据。
- `session/load` 仍只核实 capability。
- 重叠 prompt 这一次再次后完成；Host 忙时拒绝保留。没有证明所有时序都不会返回 `task_already_running`。
- 忙时目录替换这一次在 prompt B 结束前返回成功。没有证明无关用户 MCP 被保留。

## U01 仍要改的源码

- `src-tauri/computer-use-core/src/tools.rs`：`MODEL_TOOLS`、`dispatch_with_stop_handler`。申请分支不调用 `require_owner`。执行分支保持 owner，并检查 grant、binding、epoch。
- `src-tauri/computer-use-core/src/ipc.rs`：新增 `issue_transport_credential` / `revoke_transport_credential`。`credential_for_session` 不删除 transport token。
- `src-tauri/computer-use-core/src/session_grants.rs`：`Slot` 今天没有任务范围。
- `src-tauri/computer-use-core/src/broker.rs`：`drop_execution_identity`、`set_feature_enabled` / `fence_feature_enabled`。epoch 与预约是新状态。
- `src-tauri/computer-use-core/src/broker/resume.rs`：`resume`。
- `src-tauri/computer-use-core/src/broker/tests_identity.rs`：`resume_drops_authorization_snapshot_and_lease`。
- `src-tauri/computer-use-core/src/adapter.rs`：`SurfaceKind::as_wire` 的四个 kebab-case 值。
- `src-tauri/computer-use-core/src/protocol_golden.rs`：`catalog_matches_rust_constants`。
- `src-tauri/computer-use-core/src/ipc/tests.rs`：`catalog_rebuild_reuses_the_live_session_credential`。
- `src-tauri/src/computer_use/feature.rs`：今天的 bool 开关。
- `src-tauri/src/permission.rs`：`scope_key`、`SessionAllowCache`。CU 回执不写 `use_tool:*`。
- `src-tauri/src/acp_client.rs`：`decode_permission_request`。匹配要读原始 `rawInput`，不能只信它抽出的 `use_tool` 名。

U01 测试名见 ADR 第 6 节。第一条仍是先红的 core 测试：没有 owner、没有 authorized target 时，今天的 `dispatch` 失败；独立分支返回 pending，adapter 调用数为 0。测试数据标明不是真人批准。

## U02 仍要改的源码

- `src-tauri/src/computer_use/inject.rs`：`mcp_acp_entry_with`。申请目录项不得走 `authorized_target`。
- `src-tauri/src/computer_use/mod.rs`：`ensure_host_runtime` 不在 feature 关闭时为了列申请工具而启动。
- `src-tauri/src/session_manager/computer_use.rs`：`build_catalog`、`attach_computer_use`。
- `src-tauri/src/session_manager/computer_use/tests/catalog.rs`：`stop_during_mcp_update_converges_to_absent`，`base_extension_revision_race_preserves_latest_base_and_cu_entry`。
- `src-tauri/src/session_manager/computer_use/lifecycle.rs` 与 `computer_use/tests/lifecycle.rs`：session 退出撤销 transport credential。
- `src-tauri/src/session_manager/process.rs`：`live_session_is_busy`。
- `src-tauri/src/session_manager/turn.rs`：`prompt_in_flight` 时拒绝第二条 prompt。
- `src-tauri/src/session_manager/control.rs`：`resolve_permission`。CU allow-once 不进入这里的 session allow。
- `src-tauri/src/session_manager/events.rs`：`may_auto_allow`。
- `tools/computer-use-mcp/server.mjs`：继续用进程环境里的 bearer 调 loopback，不把 token 写进工具结果。
- `src/components/computer-use/ComputerPanel.test.tsx`：U02 只避免把 pending 画成 running。卡片是 U03。

本批停在这里，等 Codex 审查后再派 U01。

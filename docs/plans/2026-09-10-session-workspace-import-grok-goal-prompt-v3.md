# 交给 Grok 的目标模式长任务提示词 V3

将下面代码块完整粘贴给当前负责 `H:\aicoding\grok-app-session-import` 的 Grok 开发会话。不要同时启动第二个写入者。

```text
继续完成 Grok App 的“Codex、Claude Code、Cursor 会话与工作区导入”长任务。使用目标模式持续工作，按门禁一步步实现和验证；不要只写计划、不要只修一个测试就停、不要把局部 filtered 测试当整体完成。完成所有本机可执行阶段后再形成最终报告。

工作树：H:\aicoding\grok-app-session-import
分支：feat/session-workspace-import
基线 HEAD：30757366a739ec9aaf0ccc95bbb3efe19a067aa9

这是包含大量未提交 WIP 的专用工作树。保留全部现有修改。禁止 reset、clean、checkout 覆盖、删除未跟踪目录、重写整个实现、换分支或改无关功能。未经用户明确授权，禁止 commit、push、PR、merge、tag、release。禁止杀死或干扰用户现有 Grok、Codex、Cursor、Node、Cargo 或开发进程；故障注入只能使用隔离 GROK_APP_HOME 和你自己启动的子进程。

开始前必须：

1. 读取完整 AGENTS.md、CONTRIBUTING.md，以及与本功能相关的 docs/llm-wiki/i18n.md、settings-ia.md、dialogs.md、session-continuity.md、session-api.md、maintain.md、docs/BUILD.md。
2. 读取并严格执行：
   H:\aicoding\grok-app-session-import\docs\plans\2026-09-10-session-workspace-import-grok-audit-and-next-v3.md
3. 读取历史设计/验收文档，但若旧文档、自报进度与当前源码或可复跑证据冲突，以当前源码和 V3 审计为准。
4. 记录 git status --short --branch、HEAD、相关文件 mtime/hash、当前开发进程。若发现另一个 writer 正在改同一文件，先等其自然结束，不覆盖。
5. 不读取、复制或输出用户私人会话内容、token、Cookie、auth.json 或 secrets。真实来源验证只用用户明确授权的脱敏样本；性能与故障测试用合成 fixture。

当前已复核事实：

- HEAD 未变化，所有功能仍是未提交 WIP。
- Rust session_migration 约 34 文件 / 24k 行；前端相关约 4.4k 行。
- 原 R0 两个根因修复有效：selection 9/9、engine 41/41 通过。
- R1 仅刚开始：SelectionSet 已去掉 serde 字段默认值，新增 single()/single_entry()，compile-only 通过。
- CommitItem 与 DurableInput 仍保存 Option<AllowedSelection>；Receipt selection_hash 仍可选；engine commit 仍丢 previewId/leafId；selection.json 在 publish 后写且不进 revision_hashes；plan 仍仅在 Runtime.plans。
- 前端 commit 仍有 leafId ?? "" 与 selection ?? defaultSelection()。
- SessionImportHost 仍是单 peek/单 content 状态；branch confirm 裸 .then；branch 后 load-more 仍走旧 peek。
- 前端定向 4 files / 10 tests、typecheck、定向 ESLint、rustfmt 当前通过。
- 完整 session_migration 当前真实结果为 266 passed / 11 failed / 1611 filtered / exit 101 / 288.08s。
- 两个独立根因：
  1) capability::mcp_tools_list_payload_is_not_agent_available_commands：extract_advertised_import_tools 错把任意顶层 tools 数组当 Agent 广告。
  2) continuation::available_commands_update_unlocks_first_send_after_record：旧测试错误地要求 slash availableCommands 解锁 MCP full-history first send。
- 其余 9 个失败在独立进程可通过，是上述 panic 后的全局环境/锁污染或顺序级联。不得逐个吞 PoisonError。

唯一执行顺序：R0.5 -> R1 -> R2 -> R3 -> R4 -> R5 -> R6 -> R7 -> R8 -> R9 -> R10。某阶段退出条件未满足时不得宣称完成或跳到提交。

第一阶段 R0.5：先恢复完整领域回归

1. 修 extract_advertised_import_tools：只解析明确 Agent session/update 的工具广告字段；删除任意 params.get("tools") 回退。
2. MCP 自身 tools/list 不能成为 Agent capability；availableCommands 永远视为 slash commands，不能解锁 import-history MCP。
3. 把 available_commands_update_unlocks_first_send_after_record 改为负向测试：仍 blocked，sidecar 仍 Prepared。
4. 新增真正 `_meta.tools` Agent-advertised 正向 first-send 测试，以及单工具、错误前缀、普通顶层 tools、availableCommands 负例。
5. 先跑两个根因，再跑 capability、continuation，最后完整 session_migration。必须 0 failed、无 PoisonError、无测试顺序依赖，才进入 R1。

第二阶段 R1：SelectionSet v2 唯一真相

1. CommitItem.selection 和 DurableInput.selection 改为必填单项 SelectionSet，删除 Option/serde default。
2. engine::commit 从 plan 传递完整 PreviewSelection，用 SelectionSet::single；保留 previewId、leafId、version 和 AllowedSelection。
3. durable reload 时验证 version、exactly-one、preview lease identity、leaf/event/asset/subagent，任何缺失或篡改 fail closed。
4. Receipt selection_hash 必填；prior receipt 同时校验 snapshot + selection + options + identity。
5. revision dedup 必须包含 selection hash；不同选择不得复用旧 revision。
6. staging rename 前写完整 SelectionSet 到 selection.json；revision_hashes 保护 manifest.json、events.jsonl、assets.json、selection.json 和 blobs。
7. 禁止用 unwrap_or_default 把序列化失败写成空 JSON。
8. 前端 plan 前验证 preview map 与 selection map key 集合精确相等、ID/leaf/version 合法；删除 leafId ?? "" 和 selection ?? defaultSelection()；错误走 15 locale i18n。
9. 补齐 V3 文档 R1.4 的所有反例和 hash 语义测试。
10. R0.5 完整领域套件必须持续绿色。

第三阶段 R2：多会话内容选择状态机

1. 建 previewId keyed reducer/hook，保存 candidate、snapshot、leaf、selection、pages、cursor、filter、generation、dirty、valid、busy、error。
2. 固定 selected previews 时为所有项建立默认状态，不能只保存当前 peek。
3. A->B->A 恢复各自状态；取消再选策略写测试。
4. branch/load-more 只从当前 keyed state 发请求；禁止 branch 后继续调用旧 peek.loadMore。
5. 所有异步分支 try/catch/finally，并拒绝迟到 generation/leaf/page。
6. 删除相关 exhaustive-deps 抑制；拆 hook/子组件，不向 AppWorkbench 增长状态。

之后严格完成 V3 文档 R3-R10：

- R3 持久化 plan/job/receipt/revision，证明 Host 重启和 publish 崩溃恢复。
- R4 统一 reader/continuation/MCP evaluator 和 secret deny matrix。
- R5 版本化 cursor、fail-closed IO、索引 backup/swap/rollback。
- R6 删除生产无界读取并完成存储层分页。
- R7 用合成 10k/100MiB/1GiB fixture 做性能、取消和隔离故障 harness；本机能做的不得标 external。
- R8 在隔离 dev Tauri 中完成 Windows 产品闭环与真实 ACP 脱敏 trace；区分 slash commands、自身 tools/list、Agent `_meta.tools`、live tool call。
- R9 按 CONTRIBUTING.md 跑全仓门禁，行为稳定后再拆大文件，做 i18n/LF/秘密/许可审计。
- R10 重审 53 项并给小 PR 拆分建议；不实际 commit/push/PR。

Windows Rust 测试要求：

- cargo 每次重新链接 test EXE 后，重新用 src-tauri/windows-test-manifest.xml 和 mt.exe 嵌入 manifest。
- 0xc0000139 不是业务测试失败；嵌入后直接执行最新 grok_app_lib-*.exe，必须证明进入断言。
- --no-run 只算编译证据，不算测试通过。
- 完整命令见 V3 文档第 6 节。

开发纪律：

- 每个阶段先保留失败基线，再改生产代码，再跑最窄测试，最后扩大到领域/全仓。
- 不得通过放宽 required 字段、空字符串、default selection、unwrap_or_default、catch 后空成功来让测试变绿。
- 不得只改测试迎合错误实现；若测试与安全契约冲突，先写清契约，再同时修生产 parser 和正反例。
- 不得把 foreign transcript 称为原生 resume；导入历史保持只读，续聊是显式有界 Grok handoff。
- 不得让模型读取超出 SelectionSet 授权的 system/reasoning/tool input/result/sibling branch/subagent/asset。
- 不得硬编码 UI 文案，不得用 window.confirm/prompt/alert、裸 select 或透明 menu-panel。
- 不得伪造日志、passed 数、性能数字、真实 ACP 字段或平台验证。
- macOS/Linux 缺机器时只把精确运行子项标 blocked-external；继续完成所有本机可执行项。

每完成一个可验证小步，追加到 H:\aicoding\grok-session-import-progress.md：

STEP: Rn.x
PLAN_STEP: R0.5 | R1 | ... | R10
STATUS: in-progress | done-synthetic | done-verified | blocked-external
BASELINE: 失败测试、错误、exit code
CHANGED: 精确文件与行为
TESTS: 精确命令、exit code、passed/failed/filtered、耗时
EVIDENCE: 原始日志、截图、脱敏 trace 的绝对路径
OPEN: 未证明风险
NEXT: 唯一下一门禁

只有真实命令结束后才能写结果。旧 S65-S87 的局部日志不自动关闭当前 R0.5-R10。

不要在中途停下来只汇报“计划已写”或“一个测试已过”。持续推进全部本机可执行工作。真正遇到外部阻塞时，记录精确阻塞子项并继续其他不依赖它的本地门禁。最终只输出：完成阶段、未完成阶段、全部当前验证、外部阻塞、技术债、建议 PR 拆分；等待用户审核，不提交代码。
```

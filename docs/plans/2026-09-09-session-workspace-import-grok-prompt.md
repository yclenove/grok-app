# 交给 Grok 的目标模式提示词

下面代码块可直接作为新任务提示词。它要求按步骤持续开发，不允许跳步或用合成证据冒充实机完成。

```text
你正在继续 Grok App 的“Codex、Claude Code、Cursor 会话与工作区导入”长任务。

工作区：H:\aicoding\grok-app-session-import
分支：feat/session-workspace-import
当前基线 commit：30757366a739ec9aaf0ccc95bbb3efe19a067aa9

这是一个存在大量未提交 WIP 的专用工作树。现有改动都是应保留的工作成果。禁止 reset、clean、checkout 覆盖、删除未跟踪目录或重写整个实现。除非用户之后明确授权，禁止 commit、push、PR、merge、release。

你的最终目标不是补几条测试或写一份报告，而是把规划的完整首版做成可真实使用、可验证、可审核的产品功能：Windows、macOS、Linux 支持只读导入 Codex、Claude Code、Cursor 会话和工作区；用户可逐会话选择分支、事件、字段和附件；导入历史可阅读/搜索/导出；可显式创建新的 Grok 会话续聊；完整选择权限贯穿 handoff 与 MCP；大历史有界；真实 ACP、活数据库、故障恢复和三平台均有证据。

开始前必须完整阅读：

1. AGENTS.md
2. CONTRIBUTING.md
3. docs/BUILD.md
4. docs/llm-wiki/i18n.md
5. docs/llm-wiki/dialogs.md
6. docs/llm-wiki/session-continuity.md
7. docs/llm-wiki/session-api.md
8. docs/plans/2026-09-08-session-workspace-import.md
9. docs/plans/2026-09-08-session-workspace-import-contracts.md
10. docs/plans/2026-09-08-session-workspace-import-acceptance.md
11. docs/plans/2026-09-09-session-workspace-import-execution-runbook.md
12. docs/plans/2026-09-09-session-workspace-import-current-review.md
13. docs/plans/2026-09-09-session-workspace-import-next-phase.md
14. docs/plans/session-import-p0/acceptance-results.md
15. H:\aicoding\grok-session-import-progress.md

然后检查 git status、当前源码和验证日志。文档声明与源码冲突时，以源码和可复跑证据为准；先修正文档中的错误结论，不能顺着旧完成声明继续。

严格从 next-phase 的 S59 开始，一次只执行一个步骤。不要自行跳到容易做的后续项，也不要因为已有许多测试就跳过权限闭环。

每一步工作方式：

1. 从最终验收反推该步必须证明的行为。
2. 先补会失败的行为/协议测试，确认它确实覆盖 shipped path。
3. 只改完成该步所需的最小生产代码；复用现有领域模块，不向 App.tsx/AppWorkbench.tsx 塞状态。
4. 运行最窄测试，再运行该领域回归；失败必须定位根因，不得删测试、放宽断言或改成 mock 逃避。
5. 检查安全、分页、取消、重启、i18n 和 UI 竞态。
6. 把精确结果追加到 H:\aicoding\grok-session-import-progress.md。
7. 只有当前步门槛全部满足，才进入下一步。

进度记录必须使用：

STEP: Sxx
STATUS: in-progress | done-synthetic | done-verified | blocked-external
CHANGED: 精确文件与行为
TESTS: 精确命令、退出码、通过/失败数
EVIDENCE: 日志/截图/脱敏 trace 的绝对路径
OPEN: 尚未证明的事项
NEXT: 唯一下一步

强制证据纪律：

- done-synthetic 不能描述成“功能已完成”或“实机通过”。
- fixture、mock、编译成功、字符串搜索、cross compile 都不能代替真实 Tauri、真实 ACP、真实活库或目标平台。
- 测试必须确实进入断言。Windows 0xc0000139 时先给该次 test exe 嵌入 Common Controls v6 manifest 再运行。
- 不得伪造测试数字、截图、trace、平台结果或完成状态。
- 遇到没有机器/账号/CLI 的真实外部阻塞，记录 blocked-external、所需环境、精确命令和预期证据，然后继续做仍可在当前环境完成且顺序允许的工作；不能缩小最终目标。

首要技术要求：

- S59-S66 先完成 AllowedSelection v2。批量导入中每个 preview 都有独立 selection；leaf/event/field/asset 任一变化都使旧 plan/draft 失效。
- 本地阅读用的 searchable_text 与模型可见投影分离。system、metadata、unsupported payload 永不进入模型；reasoning、tool input、tool result、compaction、subagent、asset 都经同一 evaluator。
- sidecar/MCP 必须保存并验证完整 selection，不能用默认 false 或一个裸 hash 重建。
- search/read/asset read 每次调用重新核对 session、revision、leaf、selection hash 和撤销状态。
- S67-S70 删除无界 archive/job/index/catalog 读取，完成 100 MiB/1 GiB 可复跑基准。
- S71 必须按真实 Grok Build session update 修 ACP 能力解析：availableCommands 是 slash commands，实际 Agent 工具读取 _meta.tools。MCP Ready、自身 tools/list、Agent advertised 和 live call 是不同证据。
- full continuation 没有 Agent advertised/live verified 时必须拦截；不能静默降级。片段模式只能由用户明确选择。
- 创建 draft 不调用模型；首次发送交接只注入一次；发送后状态未知不自动重放。
- 来源 SQLite 只读私有快照，不 checkpoint/VACUUM/repair；源 hash/辅助文件前后不变。
- 所有正文、列表、搜索、作业项、诊断和 IPC 都有稳定分页或字节上限。
- UI 使用现有 Select/GlassModal/ContextMenu 和样式；无 native select、window confirm/prompt/alert、透明菜单、硬编码文案。15 locale 同步。
- 不读取或提交真实 token、cookie、账号数据、私人历史或未脱敏 trace。

不要把后续步骤合并成一个无法审查的大改动。每一步结束先自审 diff，记录当前事实，再继续下一步。只在 S82 的逐项验收全部满足且无未处理高风险问题后，报告“完整首版等待用户审核”；在此之前只能报告具体进度和剩余项。

现在从 S59 开始：先用测试冻结 SelectionSet v2、逐 preview 选择、canonical hash、字段/资产最小权限和旧 sidecar 兼容策略，然后实现最小生产代码并验证。不要先做 UI 美化或继续堆外围测试。
```

# 会话与工作区导入下一阶段开发计划

日期：2026-09-09  
前置复核：`2026-09-09-session-workspace-import-current-review.md`  
原始契约：`2026-09-08-session-workspace-import-contracts.md`  
验收清单：`2026-09-08-session-workspace-import-acceptance.md`

## 1. 最终目标

交付 Windows、macOS、Linux 首版统一的会话与工作区导入：

1. 只读发现和导入 Codex、Claude Code、Cursor，以及显式选择的通用文件。
2. 保留来源身份、revision、分支、工具、压缩、附件和完整性证据。
3. 用户可逐会话选择 branch/leaf、event、字段、subagent、reasoning 和附件。
4. 同一不可变选择贯穿 plan、commit、reader/export、handoff、MCP search/read/asset read。
5. 导入历史与原生会话严格分离；续聊显式创建新的 Grok 会话。
6. 真实本地 Grok Build 能验证并调用只读历史工具；不支持的运行环境只允许用户明确选择片段模式。
7. 大历史全链路有界，100 MiB/1 GiB 实测符合交互与资源目标。
8. 三平台实机、活数据库、故障恢复和全仓回归均有可复跑证据。

## 2. 不可破坏的约束

- 保留当前未提交 WIP，不 reset、clean、checkout 覆盖或重做整个领域。
- 来源永远只读；不得 checkpoint、VACUUM、repair 或迁移来源数据库。
- `imp:<uuid>` 不得进入原生 connect/send/rewind/automation/Remote IM。
- 创建 draft 不发模型请求；未知派发状态不自动重发。
- 被排除的内容不得经任何模型路径回流。
- 所有列表、正文、索引和 IPC 有条目或字节上限。
- 不在 `App.tsx` / `AppWorkbench.tsx` 新增领域状态。
- 15 locale 同步；不得硬编码用户可见字符串。
- 禁止原生 `<select>` 和 `window.confirm/prompt/alert`。
- 不读取、复制或提交真实 token、cookie、账号资料和私人历史。
- 没有用户明确授权时，不 commit、push、PR、merge 或 release。

## 3. 统一选择契约 v2

S59 前先冻结以下语义，后续不得在各模块各自解释：

```text
SelectionSet
  version: 2
  entries[]
    previewId or importId + revisionId
    leafId
    eventIds[]
    pinnedEventIds[]
    assetIds[]
    fields
      messageText
      toolCallName
      toolCallInput
      toolResultContent
      compactionSummary
      visibleReasoning
    includeSubagents
```

规则：

- UI 编辑的是每个 preview/import 自己的 selection，不允许批量会话共用一个裸 event ID 集合。
- event 必须位于固定 leaf 的 lineage；subagent 只有显式开启后才可选。
- system、metadata、unsupported payload 永不进入模型授权。
- 允许 event 不等于允许 event 的全部字段；所有文本投影必须经过字段判定器。
- `assetIds` 是显式 allowlist；不能用一个 `includeImages=true` 放开整个 revision。
- selection 使用 canonical serialization 计算 hash；version、leaf、event、field、asset 任一变化都改变 hash。
- plan 保存每个 preview 的完整 selection；commit receipt/revision manifest 保存其不可变副本。
- continuation sidecar 保存完整 selection 或受 hash 保护的受控引用，MCP 不得靠默认值重建。
- 本地阅读器默认可展示完整导入资料，但“交给 Grok 的范围”必须独立、清晰且可编辑。

## 4. 执行步骤

### S59：冻结 v2 schema 与兼容策略

目标：先用失败测试固定选择对象、canonical hash、旧 sidecar/plan 的读取策略和拒绝规则。

动作：

- 增加 Rust/TS v2 DTO，并为 `version`、字段默认值和资产 allowlist 写协议测试。
- 明确旧 v1：只允许读取已有本地草稿；不得静默扩大授权。旧字段无法证明时采用最小权限并要求重新准备 draft。
- 为每 preview selection 定义稳定排序；重复 ID、未知 ID、越 branch ID、pin 不在 eventIds 中均拒绝。
- 测试 hash 与数组输入顺序无关，但对语义变化敏感。

门槛：Rust/TS golden JSON 一致；无生产调用仍使用含义不明的 v1 默认值。

### S60：唯一权限判定器

目标：handoff、MCP search/read、export-to-model 和 asset read 共用一个 evaluator。

动作：

- 提供 event、block、tool field、asset 四层 allow/deny API。
- 将 `searchable_text()` 留给本地阅读；新增 selection-aware model/search projection。
- reasoning、tool input、tool result、compaction、subagent、asset 做笛卡尔测试。
- system/metadata/unsupported payload 无论 UI 输入什么都拒绝。

门槛：不存在第二套手写字段过滤；secret-like fixture 在所有模型出口均不可见。

### S61：preview 的 branch、关系和选择元数据

目标：前端能用分页 API 构造 selection，而不是猜 active leaf 或事件类型。

动作：

- preview 首屏返回 active leaf、可选 leaves、关系摘要、默认 selection、资产清单摘要。
- preview event 返回 selectable、defaultSelected、subagent、字段可用性、asset IDs 和来源 citation。
- branch 切换由 Host 校验并返回该 lineage 的分页 cursor；禁止前端加载全图再自行算 branch。
- cursor 绑定 preview snapshot + leaf + filter，旧 cursor 在条件变化后失效。

测试：分叉、rollback、compaction、subagent、迟到页、切 leaf、过期 lease、10k events。

### S62：内容选择 UI

目标：用户在导入前逐会话确认范围。

交互：

- 在内容步骤中打开独立的会话详情区域，不把完整选择器挤进候选行。
- 提供 branch/leaf `Select`、event checkbox、可见页全选/反选、reasoning/subagent 开关、工具字段开关和附件选择。
- 默认：主链 user/assistant + 工具摘要；system、reasoning、subagent、图片 bytes 关闭。
- 选择摘要显示“将归档”和“可交给 Grok”两个不同计数。
- 加载更多后保留已选状态；切 branch 前显示应用内确认对话并清理越界 selection。
- 键盘、IME、busy、empty、error、取消、窄窗口和 200% 缩放均有明确状态。

测试：hook reducer、迟到响应、分页合并、切 leaf 失效、最长中文路径、无鼠标流程。

### S63：逐 preview plan、commit 与持久化

目标：批量导入的每个会话拥有自己的 selection，重启后仍可审计。

动作：

- `PlanState` 改为保存 `SelectionSet`，删除单个全局 `selection_hash` 的歧义。
- plan hash 覆盖 preview snapshot、mapping 和每个 selection。
- commit item、receipt、revision manifest 保存对应 selection 与 hash。
- selection 变化使旧 plan 失效；不得继续提交旧 hash。
- inventory 的 selected/imported/excluded 按实际 selection 重算。

测试：两个 preview 不同 event ID、顺序重排、重启恢复、preview 过期、source refresh、重复 requestId。

### S64：reader、export 与 continuation 统一范围

目标：阅读完整归档与模型授权明确分离，并能从 reader 编辑续聊范围。

动作：

- reader 固定 revision/leaf；切 revision 后搜索、event cursor、export 和 draft 全部刷新。
- “从此处继续”以选中 event 为 leaf，加载已保存 selection 并允许二次收窄。
- Markdown 普通导出默认导出用户当前可见 branch；export-to-model 必须走 evaluator。
- selection 变化立即废弃旧 draft；编辑 handoff 文本不改变后续 MCP 可读范围。

测试：兄弟 branch、旧 revision、新 revision、搜索命中未加载页、draft 迟到、被删除 entry。

### S65：MCP 授权完整传输

目标：MCP 每次调用都使用 sidecar 中完全相同的 v2 selection。

动作：

- sidecar/grant 保存 selection v2 或受控 selection 文件引用、hash、revision、leaf、session 和协议版本。
- search 只索引/返回字段允许的文本；read 同样逐字段投影。
- 增加有界 `import_history_asset_read`，仅允许 selected asset ID、stored blob 和显式 media field；不接受路径。
- grant 文件校验 canonical hash、文件所有权/权限、session 绑定、撤销和 catalog 可见性。
- stdout 只允许逐行 JSON-RPC；诊断走 stderr 且脱敏。

测试：字段笛卡尔、猜 ID、路径穿越、A token 用于 B、撤销、跨 revision、并发、畸形/超长行、maxBytes。

### S66：选择权限端到端关门

目标：用 shipped path 证明 UI 选择真的控制 plan、archive、handoff 与 MCP。

场景：一个含主链、兄弟分支、system、reasoning、tool input/result、subagent 和图片的 fixture；批量选择两个会话，每个选择不同。

证据：

- 前端操作后的 IPC payload。
- revision manifest/receipt 的 selection hash。
- handoff 捕获文本。
- MCP search/read/asset 的 allow 和 deny。
- 修改 selection 后旧 plan/draft 必须 `PLAN_STALE`。

门槛：任何出口出现被排除 secret 都不得进入 S67。

### S67：事件与 continuation 流式读取

目标：100 MiB 单会话不再因 `Vec<ImportedEvent>` 产生无界 RSS。

动作：

- 为 archive 建 event offset/ordinal 索引；lineage 使用有界 relation/index 数据。
- handoff 只读取 selection 所需事件，并按预算从尾到头选取；钉选单独有界读取。
- 大 selection 使用临时受控 ID 集/SQLite 表，不在 IPC 或环境变量传十万 ID。
- 所有损坏/IO 错误显式返回，删除 `unwrap_or_default` 空 hash 降级。

### S68：搜索索引流式重建

目标：索引损坏或版本变化时可流式重建，内存与单页文本上限可测。

动作：逐条解析、批量事务、可取消、原子发布；CJK n-gram 与字段权限元数据一起索引。搜索 cursor 绑定 index revision + selection hash。

### S69：job、catalog 与诊断分页

目标：所有列表在存储层分页，而不是读全量后 `skip/take`。

动作：job items、entries、revisions、continuation refs、issues/diagnostics 使用稳定 cursor；前端合并去重并防迟到页。

### S70：可复跑性能基准

数据集：10k metadata、100 MiB 单会话、1 GiB 批次、长工具输出、缺附件、活 WAL。

记录：冷/热缓存、CPU、峰值 RSS、磁盘增量、首批/完整扫描、preview、index、commit、reader 首屏、MCP 初始化、取消确认。模型响应时间单列。

门槛：首批约 2 秒、已索引首屏约 500 ms、取消确认约 300 ms；超标必须定位和修复，不只记录数字。

### S71：修正 ACP session update 解析

目标：准确区分 slash commands、MCP Ready、Agent advertised tools 和 live call。

动作：

- 按固定 Grok Build 源码和真实 trace 解析 `_meta.tools`。
- `availableCommands` 仅记录 slash commands，不能满足 import-history 工具门闩。
- 对 camelCase/snake_case、未知 metadata 和增量更新写兼容测试。
- 记录 CLI/version/route/home mode 和脱敏 evidence。

### S72：能力状态机与失败体验

状态至少包括：`not-requested`、`starting`、`mcp-ready`、`agent-advertised`、`live-verified`、`unavailable`、`policy-blocked`。

完整模式只接受 Agent advertised 或 live verified；超时/重连不静默改成片段。UI 提供明确重试和“仅交接片段”选择。

### S73：官方/自定义 × shared/independent 实测

四组合逐一记录：CLI 版本、脱敏 session update、实际工具名、一次 search、一次 read、一次 deny。普通会话不得注入工具；official-aux、热更新、warm reuse、A→B→A、fork/load/background 均复测。

### S74：首次派发真实闭环

目标：100 轮前钉选约束实际进入一次且仅一次 `session/prompt`；其余允许记录可由工具读取。

动作：捕获 Host 发送边界；检查 prepared→dispatching→running→completed；编辑 draft、重复点击、连接前失败和发送后断线分别验证。

### S75：崩溃与恢复

实测杀 Agent、杀 Host、关窗口、重启、丢 completion、publish 中断、磁盘只读/满。能证明未发送才允许重试；不能证明则进入 UNKNOWN 并让用户决定。

### S76：Windows Tauri 实机

覆盖默认根、自定义根、活库、批量导入、reader、搜索、导出、选择、续聊、撤销。视口覆盖 1280×720、窄窗口、200% 缩放、深浅主题、中文路径、IME、纯键盘。保存截图和操作记录。

### S77：真实来源兼容矩阵

在不提交私人数据的前提下，用脱敏摘要记录当前稳定版 Codex、Claude Code、Cursor：路径、版本、schema fingerprint、完整/partial、附件和活 WAL 行为。未知版本必须显示 unsupported/partial，不得误报 complete。

### S78：macOS 实机

验证默认路径、权限、大小写卷、Intel/ARM 路径语义、UI、活库、本地 MCP 和大文件。无机器时标 `blocked-external` 并给出可复制命令及所需证据。

### S79：Linux 实机

验证 XDG 路径、权限、符号链接、大小写、Wayland/X11 UI、本地 MCP 和打包运行。无机器时处理同 S78。

### S80：全仓回归

```text
pnpm deps:check
pnpm typecheck
pnpm test
pnpm lint
pnpm build:ui
cd src-tauri
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Windows `0xc0000139` 时嵌入 Common Controls v6 manifest 后运行生成的测试 exe，并证明测试进入断言。

回归至少包括原生 session、CLI import/attach、搜索、automation、Remote IM、setup gate、普通 MCP、项目状态和 wallpaper/Imagine 入口。

### S81：技术债收口

- 删除或收窄 `session_migration/mod.rs` 的模块级 allow。
- 消除重复 DTO、生产 stub、静默 IO 降级和未使用命令。
- 检查 source-only 许可和第三方 NOTICE。
- `git diff --check`；新文本统一 LF；不机械改动无关文件换行。

### S82：最终验收与交付

重新逐行填写 53 项表。每项只能是：

- `实机通过`
- `合成测试通过`
- `部分`
- `失败`
- `未验证`
- `blocked-external`

交付源码支持矩阵、完整性损失、性能表、故障记录、UI 截图、脱敏 ACP/MCP trace、外部阻塞复现步骤和建议提交拆分。只有所有当前可执行门槛通过、无未处理高风险问题时才报告“等待审核”。

## 5. 每步记录格式

每完成一步，追加到 `H:\aicoding\grok-session-import-progress.md`：

```text
STEP: Sxx
STATUS: in-progress | done-synthetic | done-verified | blocked-external
CHANGED: 文件和行为，不写空泛总结
TESTS: 精确命令、退出码、通过/失败数
EVIDENCE: 日志、截图或脱敏 trace 的绝对路径
OPEN: 未证明的事项和风险
NEXT: 唯一下一步
```

`done-synthetic` 不能写成“功能完成”；fixture/mock/cross compile 不能代替真实 ACP、真实 UI 或目标平台。

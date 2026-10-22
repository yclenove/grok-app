# 会话与工作区导入当前实现复核

日期：2026-09-09  
工作树：`H:\aicoding\grok-app-session-import`  
分支：`feat/session-workspace-import`  
复核基线：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9` 加当前未提交 WIP

## 1. 结论

当前实现不是概念验证，也不是只做了几个入口。Grok 已经完成了较完整的导入领域骨架、三家适配器、持久作业、只读历史、搜索、工作区映射和续聊基础设施，合成测试覆盖面很大。

但它仍不能被称为可发布首版。当前准确状态是：

> 大规模、可编译、合成测试较完整的 WIP；真实产品闭环、权限选择闭环、内存边界和三平台实机验收尚未完成。

最关键的差距不是“再补几个测试”，而是以下四条运行链路还没有形成同一个可信闭环：

1. 用户在 UI 中选择的 branch、event、字段和附件没有形成逐会话 `AllowedSelection`。
2. plan、commit、reader、handoff 和 MCP 没有持续使用同一份不可变选择。
3. ACP 能力探测读取的字段与当前 Grok Build 实际广告工具的位置不一致。
4. 大历史的若干生产路径仍整份读入内存，尚未满足 100 MiB/1 GiB 目标。

## 2. 已完成的能力

| 领域 | 当前实现 | 质量判断 |
| --- | --- | --- |
| 来源发现 | Codex、Claude Code、Cursor、通用 JSON/Markdown；默认根、自定义根、来源登记与 relocate | 骨架完整，合成覆盖较好；真实安装数据仍需验证 |
| 来源读取 | JSONL 有界解析；Cursor transcript、Composer、Store；SQLite/WAL 私有快照 | 安全边界设计正确；活数据库和多版本兼容仍缺实机证据 |
| 归一化 | message、tool call/result、compaction、metadata、unsupported；parent/fork/subagent/rollback 关系 | 领域模型较完整，fixture 覆盖较强 |
| 快照与身份 | 独立 `imp:<uuid>`、source identity、revision、preview lease、source revision | 方向正确，避免把外部会话伪装成 Grok 原生会话 |
| 持久作业 | plan、commit、receipt、恢复、取消、retry、undo、tombstone | 合成故障注入较多；杀 Host/Agent 和真磁盘故障未验证 |
| 工作区映射 | 多 root、已有项目、新记录、unassigned、路径语义保留 | 模型与测试已建立；真实跨机路径映射未验证 |
| 阅读与检索 | 导入历史分组、分页阅读、字面/CJK 搜索、Markdown 导出、全局搜索跳转 | 可见产品骨架已接线；revision/leaf/selection 尚未统一绑定 |
| 续聊 | draft、sidecar、固定 revision、首次发送门闩、dispatch 状态、stdio MCP | 合成状态机较完整；真实 ACP 工具暴露与 prompt 传输未通过 |
| 前端 | 导入向导、候选分页、peek、映射、job、reader、搜索、导出、续聊草稿 | 可见但不是完整交互；缺内容权限选择和实机布局/键盘验收 |
| i18n | 15 个 locale 的导入文案 | parity 测试通过，后续新增文案仍须同步 |

## 3. 已有验证证据

以下数字来自当前工作区的验证日志，不应扩大解释范围：

- Windows Rust library：`1846 passed; 0 failed; 1 ignored`。
- `session_migration` 子集：最近记录 `239 passed; 0 failed`。
- Rust `clippy --all-targets -- -D warnings`：已有通过记录。
- 全仓 Vitest：`602 files / 7115 passed`。
- 导入相关 Vitest：`17 files / 101 passed`。
- TypeScript：通过。
- 10,000 条合成元数据扫描：首次可见约 `78 ms`。

这些证据能证明实现有较强的合成回归基础，不能证明：

- Windows Tauri 的实际点击、缩放、IME 和弹层表现。
- 官方/自定义 × shared/independent 四种真实 ACP 组合。
- 真实 Codex、Claude Code、Cursor 活数据库兼容性。
- macOS、Linux 行为。
- 100 MiB 单会话和 1 GiB 批次的峰值 RSS、磁盘和取消响应。
- 进程被杀后的真实恢复与“不重复发送”。

当前 53 项表把 50 项标为“合成测试通过”、3 项标为“未验证”，但其中部分“合成测试通过”只覆盖了底层判定，不代表对应 UI 或真实运行闭环已经完成。下一阶段必须按证据层级重新审计，不能把底层单测写成产品实机通过。

## 4. 阻塞发布的代码问题

### 4.1 `AllowedSelection` 没有贯穿 UI 与多会话 plan

- `src/hooks/useSessionImport.ts` 的 preview、plan 和 continue 调用没有构造或传递内容选择。
- `SessionImportHost.tsx` 只能选择候选会话，不能选择 branch/leaf、event、reasoning、subagent、工具字段或附件。
- Rust `PlanState` 只有一个全局 `selection_hash`，无法表达批量导入中每个 preview 的不同选择。
- commit item 没有保存该 preview 的完整选择，刷新或重启后不能重建相同授权。
- sidecar 只保留 event IDs/hash，字段和资产权限在 MCP 重建时丢失。

影响：用户界面看似允许“导入选中的会话”，但没有控制哪些历史会进入模型；被排除的 reasoning、工具参数或附件无法得到端到端保证。

### 4.2 MCP 的字段投影仍可能越权

- `ImportedEvent::searchable_text()` 包含 `VisibleReasoning`。
- MCP 侧重建 selection 时曾把 include flag 固定为 `false`，并未保存完整字段授权。
- event allow/deny 与 event 内部 block/工具 input/result/asset allow/deny 不是同一判定器。

影响：即使 UI 以后关闭 reasoning，search/read 仍可能通过通用文本投影返回它。

### 4.3 ACP 工具广告解析与真实协议不一致

当前 `capability.rs` 只读 `availableCommands`。已核对的 Grok Build 源码中：

- `availableCommands` 是 slash command 列表。
- Agent 工具名位于同一 session update 的 `_meta.tools`。

因此当前逻辑可能永久拦住真实 full continuation，也可能把非工具命令误当成工具能力。必须分别记录 slash commands、MCP Ready、Agent advertised tools 和 live tool call，不得互相替代。

### 4.4 生产路径仍有无界读取

已确认的重点位置：

- `continuation.rs::load_events`：整份 `events.jsonl` 收集为 `Vec`。
- `search.rs`：索引重建前加载完整 archive。
- `engine.rs::job_items_page`：先读完整 job 再分页。
- catalog/list 和部分 export/continuation 路径仍需逐一审计。
- `current_file_revision` 的 `read(...).unwrap_or_default()` 会把 IO 错误伪装为空文件 hash。

影响：小 fixture 全绿不能证明大历史可用；错误降级还可能生成错误 revision。

### 4.5 只读和质量债

- `open_catalog_readonly()` 的 schema 行为需要确保只验证、不建表、不迁移。
- `session_migration/mod.rs` 顶部的大范围 `allow(dead_code)`、`allow(type_complexity)` 等会掩盖未接线代码和临时结构。
- 多个 Rust/TypeScript DTO 手工重复，协议漂移风险高。

## 5. 发布判定

当前可称为：

- 三家来源的导入核心已基本实现。
- 大部分安全、恢复和格式规则已有合成测试。
- UI 和续聊已有可继续完善的产品骨架。

当前不可称为：

- 完整功能已开发完成。
- 用户选择范围已安全贯穿。
- Grok 续聊工具在真实 CLI 上可用。
- Windows/macOS/Linux 首版均已支持并验证。
- 大数据性能目标已通过。

下一阶段以 `2026-09-09-session-workspace-import-next-phase.md` 为唯一执行顺序。每一步都必须产生可复跑证据，并明确区分 `done-synthetic`、`done-verified` 与 `blocked-external`。

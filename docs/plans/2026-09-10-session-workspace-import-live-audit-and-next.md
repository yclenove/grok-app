# 会话与工作区导入：实时复核与后续执行门禁

- 日期：2026-09-10
- 工作区：`H:\aicoding\grok-app-session-import`
- 分支：`feat/session-workspace-import`
- 基线 HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`
- 首次复核快照：2026-09-10 03:42（Asia/Shanghai）
- 二次复核快照：2026-09-10 04:11（Asia/Shanghai）
- 三次复核快照：2026-09-10 04:17（Asia/Shanghai，已纳入 S85）

## 0. 先说明快照边界

复核时工作树仍有大量未提交 WIP，且三个 `grok.exe` 进程仍在运行。03:32–04:14 期间先后观察到 `useContentSelection`、`SessionImportContentDetail.tsx`、`SessionImportHost.tsx`、`jobs.rs`、`search.rs`、`continuation.rs` 和 `engine.rs` 更新。04:16 Grok 追加 S85 并把两份手写汇总标为 superseded；因此本文仍是动态工作树的时间点审计，不是最终验收。

因此本文把状态分成三类：

- **已有证据**：进度日志中有可复跑命令、通过数和日志路径。
- **代码已出现但未验收**：源码中已看到实现，但没有对应步骤的稳定测试记录。
- **进行中**：复核期间仍在写入，不能据此宣称完成。

接手者不得覆盖这些改动，也不得同时编辑正在写入的文件。先等当前活动步骤落日志或文件稳定，再从本文第一个未通过门禁继续。

## 1. 总结论

这不是一个空壳或概念验证。Grok 已经搭出了规模较大的跨来源迁移领域：三家适配器、只读来源快照、归一化事件图、资产处理、工作区映射、持久作业、恢复/撤销、只读阅读、搜索/导出和续聊/MCP 骨架都存在，合成测试也很多。

但当前仍不是可发布首版。准确表述是：

> 导入内核完成度较高、合成验证较强；逐会话内容选择正在贯穿，真实 ACP、大历史资源边界、真实来源和三平台产品验收仍未完成。

不能把“53 项中大量合成测试通过”改写成“Windows、macOS、Linux 功能已完成”。当前最新源码晚于全仓回归日志，也晚于 Grok 的 S65–S84 批量状态声明；历史绿灯和 12 个定向测试不能证明 NP-S61–NP-S69 已分别满足阶段门禁。

## 2. Grok 已经完成了什么

| 领域 | 已有实现 | 证据级别 | 质量判断 |
| --- | --- | --- | --- |
| 来源发现 | Codex、Claude Code、Cursor、通用 JSON/Markdown；默认根、自定义根、登记来源、relocate、分页扫描 | 合成测试 | 设计较完整，真实安装版本矩阵未验 |
| 只读读取 | JSONL 有界解析；Cursor `state.vscdb`/`store.db` 与 WAL 私有快照；源文件不 repair/checkpoint/VACUUM | 合成测试 | 安全方向正确，活 Cursor 数据库仍缺实机 |
| 归一化 | message、tool call/result、compaction、metadata、unsupported；parent/fork/subagent/rollback 关系 | 合成测试 | 三家主要格式已覆盖，未知版本仍需真实样本 |
| 身份与 revision | `imp:<uuid>`、source identity、固定 snapshot/revision、preview lease、重定位冲突 | 合成测试 | 能避免冒充原生会话，基础扎实 |
| 附件 | content-addressed blob、缺失/超限/远程/越界状态、取消清理 | 合成测试 | 存储契约较好，真实大图和媒体打开未验 |
| 工作区映射 | 多 root、existing/new/unassigned、跨 host 身份、项目保护、恢复 | 合成测试 | 模型完整，跨机/跨平台路径未实测 |
| 持久作业 | plan/commit/receipt、幂等、取消、retry、undo、tombstone、崩溃点故障注入 | 合成测试 | 状态机覆盖强，杀 Host/Agent、磁盘满仍未实测 |
| 阅读与检索 | imported history 分组、分页阅读、CJK 搜索、Markdown 导出、全局搜索跳转 | 合成测试 | 产品入口已接线，revision/leaf/selection 还需统一 |
| 原生隔离 | `imp:` 被原生 connect/send/导出路径拒绝，不进入原生 usage/unread | 合成测试 | 边界正确，CLI/Remote IM/活会话仍需回归 |
| 续聊 | 新 Grok draft、固定 revision、首次发送门闩、dispatch 状态、sidecar、stdio MCP | 合成测试 | 框架较完整，真实工具广告和一次且仅一次发送未验 |
| 前端向导 | 来源、候选、peek、映射、作业、结果、阅读器、续聊草稿 | 合成测试 | 可见骨架较全，真实 Tauri 与复杂内容选择仍未验 |
| Selection v2 | versioned DTO、canonical hash、v1 最小权限兼容、event/field/asset/subagent 判定器 | NP-S59/NP-S60 合成测试 | 本轮最成熟的新进展，但尚未完整贯穿持久化和 UI |

历史验证记录包括：

- Windows Rust library：`1846 passed; 0 failed; 1 ignored`。
- `session_migration`：最近完整记录约 `239 passed; 0 failed`。
- 全仓 Vitest：`602 files / 7115 passed`。
- 导入相关 Vitest：`17 files / 101 passed`。
- `cargo clippy --all-targets -- -D warnings` 曾通过。
- 10,000 条合成元数据扫描首次可见约 78 ms。

这些数字都早于最新 NP-S61–NP-S63 写入，只能作为历史基线。

### 2.1 04:20 当前树独立验证

本次复核在源码连续数分钟未变化后，直接对当前工作树运行了最窄前端/静态门槛；结果不能被历史日志替代：

| 命令 | 当前结果 | 结论 |
| --- | --- | --- |
| selection/content hooks Vitest（4 files） | 10 passed，exit 0 | reducer/hook 的有限合成路径可用 |
| `node node_modules/typescript/bin/tsc -b --pretty false` | exit 1，11 个错误 | 当前树不能通过 TypeScript 编译 |
| session-import 定向 ESLint | exit 0 | 所选前端目录 lint 通过 |
| `cargo fmt --all -- --check` | exit 1 | `job_control_tests.rs` 有格式差异 |

TypeScript 错误分为三组：

- `SessionImportHost.tsx` 导入了未使用的 `selectionFromState`。
- `SessionImportHost.tsx` 六处读取 `peek.previewId`，但 `useCandidatePeek()` 的返回对象没有暴露 `previewId`。
- `src/lib/api/sessionMigration.ts` 四处使用 `AllowedSelection`，但该文件没有可见的类型导入。

这组结果直接否定“当前树已完成 NP-S61～NP-S69”的说法。第一步必须先让当前代码稳定编译和格式通过，然后才能讨论更高层验收；定向 Vitest 通过不能覆盖 TypeScript 构建失败。

## 3. 新计划当前推进位置

| 新计划步骤 | 当前快照 | 判定 |
| --- | --- | --- |
| NP-S59 Selection v2 schema/兼容 | `selection.rs`、Rust/TS golden、v1 最小权限兼容已落；进度日志 S63 有双轮测试 | `done-synthetic` |
| NP-S60 唯一权限判定器 | `permits_event/block/tool_field/asset`，selection-aware MCP search/read，model export 已落；进度日志 S64 有双轮测试 | `done-synthetic` |
| NP-S61 preview branch/关系/默认选择 | 元数据和定向测试已出现；cursor 仍未完整绑定 version/snapshot/filter，错误语义也未关门 | 实现已出现，待返工验收 |
| NP-S62 内容选择 UI | reducer/hook/detail UI 已出现；多会话状态、branch/load-more、迟到/失败路径仍有断点 | 实现已出现，待返工验收 |
| NP-S63 逐 preview plan/commit | per-preview 字段已出现；仍有重复 DTO、可选 leaf、非 exact-cover、内存 plan 和不完整 revision 持久化 | 实现已出现，待返工验收 |
| NP-S64–NP-S69 | reader、MCP、索引、分页等实现已出现，但每一步的完整验收行为没有被现有 12 个 Rust 测试覆盖 | 实现已出现，待返工验收 |
| NP-S70–NP-S80 | Grok 批量写成 `blocked-external`；其中多项可在本机生成数据、注入故障、跑 Windows 回归或尝试本机 ACP | 状态判定需重做 |
| NP-S81–NP-S82 | 原计划明确包含技术债收口与最终 53 项验收 | 未执行，不得跳过 |

进度日志的 S63/S64 对应新计划 NP-S59/NP-S60。后续记录必须写成 `PLAN_STEP: NP-Sxx`，避免再次把日志流水号和计划编号混淆。

### 3.1 对 S65–S84 批量完成声明的复核

Grok 在进度日志中把 NP-S61–NP-S69 依次标为 `done-synthetic`，把 NP-S70–NP-S80 依次标为 `blocked-external`，并在最后写了“S81–S82 不在本目标”。S85 又补充了较诚实的原始 cargo 日志，并主动作废手写通过汇总；这是证据质量上的改进，但阶段状态仍不予验收，原因不是“测试完全没跑”，而是**证据和对应门禁不匹配**：

- S85 的 `S61-run1/2.log` 到 `S69-run1/2.log` 是实际 cargo 输出，但每次只运行 1～4 个命名测试，且每段显示 `1887 filtered out`；它们仍不能证明九个阶段的 UI、持久化、权限、分页和资源边界。此前 12 个前端测试也只覆盖 reducer/hook 的有限路径。
- 被作废的 `S65-np61-1.log` / `S65-np61-2.log` 不得再作为通过证据；S85 对这一点的纠正应保留。
- 第一轮详细日志实际先出现 `two_previews_keep_independent...` 断言失败，以及索引发布在 Windows 上的 `os error 32`；后续重跑通过只说明对应样例被修到绿色，不等于阶段门禁全部满足。
- NP-S61～NP-S69 的正确状态应统一回退为“实现已出现、待逐项返工验收”。只有为该阶段列出的行为测试、领域回归和静态检查全部通过后，才能重新写 `done-synthetic`。
- NP-S70 的 100 MiB/1 GiB fixture、RSS/CPU 采集和取消计时可在本机生成，不是天然外部阻塞；真实活 WAL 只是其中一个额外实机样本。
- NP-S75 的杀子进程、发布中断、只读目录和可控磁盘不足可在隔离 `GROK_APP_HOME`/测试进程中注入，不能以“不杀当前用户进程”为由整体阻塞。
- NP-S76 的 Windows Tauri 验收在当前 Windows 机器上可执行；是否启动应用应遵守用户授权，但“尚未启动”不等于缺少外部平台。
- NP-S80 至少可以完成当前 Windows 的全仓回归；只有 macOS/Linux 部分可保留 `blocked-external`。
- NP-S81、NP-S82 就在下一阶段原计划中，分别是技术债收口和最终验收。不得以“不在目标”为由跳过。

因此当前可接受的表述是：**Grok 已快速铺出 NP-S61～NP-S69 的第一轮实现，但尚未逐门禁完成；NP-S70～NP-S80 中本机可执行部分尚未做，macOS/Linux 和确实需要外部账号/机器的部分才是外部阻塞。**

## 4. 当前快照暴露出的必须修复项

以下是后续实现的验收输入，不是对仍在写入代码的最终定罪。接手时先验证问题是否仍存在；若已修复，用测试证明后关闭。

### P0-1：多会话 selection 还没有可靠闭环

- 前端目前把 selection 按 candidate 保存，但主要在点击“下一步”时只写入当前 peek 的会话。
- 用户先编辑 A、再查看 B，A 的编辑可能没有持久保存；多个被选会话没有逐个打开时，selection 集合可能只覆盖其中一部分。
- `PreviewSelection.leafId` 在 TypeScript 中仍是可选，调用处没有稳定传入；Rust 曾以空字符串兼容，容易失去 branch 身份。
- `plan_with_selections` 必须证明“每个 preview 恰好一次”。仅比较长度并检查 ID 属于 preview 列表，挡不住 A 重复两次、B 缺失。
- selection 不能只 `validate_shape`，还必须针对该 preview 的固定 snapshot、leaf、events、relations 和 assets 做 `validate_for_branch` 与 asset membership 校验。
- peek 后重新创建 preview 会产生新的 snapshot/previewId；旧 selection 不能未经重验直接套到新 preview。

### P0-2：切 branch 与加载更多存在状态分叉风险

- branch 切换后的内容状态使用新 leaf，但 `useCandidatePeek.loadMore()` 仍可能使用 peek 自己保存的旧 cursor/leaf。
- branch 切换请求需要 previewId + generation + leaf 三重归属校验，迟到响应不得覆盖新状态。
- branch 请求必须有 busy、error、取消/关闭处理，不能裸 `.then(...)`。
- cursor 应包含版本、preview snapshot 身份、leaf、filter 和 offset，并经完整校验；不能用空 relations 或空 events 静默继续。
- unknown leaf、损坏 manifest、索引 IO 错误必须明确失败，不能先修改 runtime active leaf。

### P0-3：plan、commit、receipt、revision manifest 仍需不可变 selection

- `selection.rs` 已定义严格的 `SelectionSet/PreviewSelection`，但 `engine.rs` 又定义了第二套较宽松 DTO；commands 和前端仍走后者。`SelectionSet` v2 还不是生产链路的唯一输入。
- TypeScript `PreviewSelection.leafId` 仍可选，当前 `commit()` 构造 payload 时没有传 `leafId`；Rust 对应字段还带 `serde(default)`。
- plan hash 必须覆盖 selection schema version、每个 preview snapshot digest、leaf、canonical selection、mapping 与关键导入 options。
- 当前 exact-cover 只比较数量并检查 ID 属于 preview 列表，挡不住 A 重复两次、B 缺失。
- commit item、job JSON、每阶段 receipt 和最终 revision manifest 都要保存同一份选择或受 hash 保护的受控引用。
- 当前 `CommitItem` 只保存 `AllowedSelection`，没有 leaf；receipt 只保存 hash；`selection.json` 在 staging rename 后另写，且不在 `revision_hashes()` 或 manifest 保护范围内。去重早返回路径也需要验证不会跳过选择持久化。
- 重启恢复后不得靠当前 UI 状态或默认选择重建授权。
- 当前 plan 仍只保存在 `Runtime.plans` 内存表，Host 重启后 commit 会得到 `plan not found`。
- selection、leaf、snapshot 或 mapping 任一变化都必须让旧 plan 返回 `PLAN_STALE`。

### P0-4：模型授权出口还没有完成端到端闭锁

- sidecar/grant 已开始加入完整 `AllowedSelection`，但仍要验证旧 `allowedEventIds`、hash 和新对象不会形成多套真相。
- `grant_for_session`、MCP search/read/asset read、handoff、export-to-model 每次都要验证 session、revision、leaf、selection hash、撤销/隐藏状态。
- `import_history_asset_read` 只能接受 asset ID，不接受路径；必须同时通过 selection allowlist、revision manifest 和 stored blob 校验。
- 本地阅读器可以展示归档完整内容，但任何“交给模型”的投影都只能走统一 evaluator。

### P0-5：ACP 能力探测仍读错协议字段

- `availableCommands` 是 slash commands，不是 Agent 工具广告。
- 实际工具名应从 session update 的 `_meta.tools` 读取；MCP Ready、自身 `tools/list`、Agent advertised、live tool call 是四种不同证据。
- 在 Agent advertised 或 live verified 之前，full-history 模式必须拦截；不能静默降级成 fragment。

### P0-6：大历史路径仍有无界读取和静默降级

- 04:07 后 `prepare_draft`、普通导出和 model export 已能看到按 ID 的索引读取，这是有效进展；但 `continuation::load_events` 和 `search::load_archive_events` 仍是公开的整份 `Vec` 路径，需删除、私有化或用调用图证明生产链不再可达。
- 大 relation 集、十万级 selection ID、revisions/continuation refs/issues/diagnostics 等仍需有界存储层审计；当前 NP-S69 证据只覆盖 entries 与 job items。
- `current_file_revision` 不能用 `read(...).unwrap_or_default()` 把 IO 失败伪装成空文件 hash。
- `relations_from_dir(...).unwrap_or_default()`、`indexed_events_by_ids(...).unwrap_or_default()` 等生产路径必须 fail closed。
- `open_catalog_readonly()` 不应调用可能执行建表/迁移的初始化路径；拆成纯验证函数更清晰。

### P0-7：索引发布重试仍不等于安全原子替换

- 当前实现先删除 live index，再把临时库 rename 到 live；如果 rename/retry 最终失败，旧的可用索引已经丢失。
- `ensure_index` 的错误恢复也会先删 live 文件，可能把“旧索引可继续服务”变成无索引。
- 必须使用同目录 backup/swap/rollback 或平台安全替换策略，证明任意失败点都保留旧 live 或完整新 live，绝不暴露缺失/半成品。
- Windows 首轮 `os error 32` 已证明句柄与并发发布是实际问题；需要并发 reader/rebuilder、sharing violation、进程中断和重启恢复测试，而不是只把 retry 次数调大。

### P1：UI、架构和维护性

- `fieldAvailability` 应控制相应开关是否可用；message text 字段不能在协议里存在、UI 却无法明确控制。
- “将归档”和“可交给 Grok”要使用明确口径：全 revision、当前 branch、selected events、model-visible blocks/assets 不得混算。
- relation summary 和 asset summary 目前不能只是返回但不展示。
- leaf 原始 ID 不是最终用户标签；需要可区分且可访问的 branch 标识，同时保留精确 ID 供审计。
- 事件正文不能只依赖无界 `<pre>`；长内容、窄窗口和 200% 缩放需有截断/展开策略。
- `SessionImportHost.tsx` 已超过 1000 行。完成行为前不要大重构，但在功能稳定后应拆出 content controller、mapping step、job/result step 和 reader/continuation step，避免继续集中状态和竞态。
- `session_migration/mod.rs` 的模块级 clippy/dead-code allow 必须在最终收口时删除或缩到具体项。
- 当前仓库 `core.autocrlf=true` 且若干文件提示 LF 将被转 CRLF。新文件保持 LF，不机械改动无关文件换行；最终以 `git diff --check` 和实际 blob 行尾复核。

## 5. 后续执行门禁

### G0：安全接管与真实基线

1. 检查 Grok 进程、进度日志尾部和相关源码时间戳。
2. 只在当前 writer 已结束或同一步已稳定落日志后接管；不得开第二个 writer 改相同文件。
3. 记录 `git status --short`、HEAD 和未跟踪目录；禁止 reset/clean/checkout 覆盖。
4. 对 NP-S61–NP-S63 做一次源码到测试的映射，找出第一个未满足门禁。
5. 先复现并修复 04:20 的 11 个 TypeScript 错误和 Rust 格式差异，再跑最窄编译/测试；不要直接引用旧绿灯。

通过条件：能说清当前唯一活动步骤、已改文件、失败测试和下一项；没有并发写入风险。

### G1：关闭 NP-S61 preview 契约

必须完成：

- preview 首屏返回有效 active leaf、排序稳定的 leaves、关系摘要、branch 口径的 event count、archive 总数、默认 selection 和资产摘要。
- event 行返回 selectable/defaultSelected/subagent、字段可用性、asset IDs、稳定 citation。
- leaf 在读取和修改 runtime 前验证存在于固定 snapshot。
- cursor 使用版本化 envelope，绑定 previewId、snapshot digest/lease、leaf、filter signature、offset；篡改、跨 leaf、跨 preview、过期均拒绝。
- 所有 manifest/index/IO 错误显式返回，不得以空集合继续。

最低测试：fork、rollback、compaction、subagent、unknown leaf、cross-preview cursor、cross-leaf cursor、tampered cursor、expired lease、late page、10k events、损坏 manifest/index。

### G2：关闭 NP-S62 内容选择 UI

状态模型必须以 `previewId` 为主键，每个 preview 保存：leaf、selection、已加载页、nextCursor、generation、dirty/valid 状态。不能只维护“当前详情”一份选择。

交互必须覆盖：

- 每个选中会话都得到固定 preview 和默认 selection；用户未打开详情也不能缺 selection。
- 在 A/B 间切换能恢复各自独立选择；取消选择再选回的策略明确。
- branch 切换用应用内确认，成功前不丢旧状态；失败保留原 branch 和 selection。
- 加载更多使用当前 preview + leaf + cursor，保留离页选择并拒绝迟到页。
- event、字段、subagent、asset 的依赖关系明确；不可用字段 disabled 并解释原因。
- keyboard、IME、busy、empty、partial/error、窄窗口、200% 缩放、深浅主题可用。
- 所有文案走 15 locale，使用现有 `Select`/`GlassModal`/按钮/checkbox 样式。

最低测试：A/B 独立选择、未打开详情的默认选择、切回恢复、load-more 后保留、branch 请求迟到/失败、关闭向导、纯键盘、长中文路径、i18n parity。

### G3：关闭 NP-S63 持久化

- TypeScript/Rust 共同使用 required `SelectionSet v2`；`leafId` 必填。
- exact-cover 校验：preview 列表和 selection entries 一一对应，无重复、无遗漏、无未知 ID。
- 每项针对其 snapshot 验证 leaf、lineage、event、pinned event、subagent 和 asset。
- plan/job/receipt/revision manifest 保存 canonical selection 与 hash；旧 schema 最小权限迁移或要求重新 prepare。
- 恢复 job 后仍能从持久数据重建同一授权；不得依赖内存中的 `Runtime.plans`。
- inventory 按 archive/imported/excluded/model-visible 分开计数。

最低测试：两个 preview 使用相同 event ID 但不同 selection、重复 preview、漏 preview、未知 leaf/event/asset、顺序重排、重启恢复、preview 过期、source refresh、重复 requestId、旧 schema。

### G4：关闭 NP-S64–NP-S66 模型权限闭环

1. reader 固定 revision/leaf，切换后 search/page/export/draft 全部换代。
2. sidecar/grant 只保留一份完整 v2 selection 真相；hash 校验失败即拒绝。
3. handoff、export-to-model、MCP search/read/asset read 共用 evaluator。
4. 修改 selection 后旧 plan、draft、grant 全部 stale/revoked。
5. 用一个含 system、reasoning、tool input/result、兄弟 branch、subagent、compaction、图片和 secret marker 的 shipped-path fixture 关门。

通过条件：前端 IPC、plan、receipt/manifest、handoff 捕获和 MCP allow/deny 的 selection hash 相同；任何被排除 secret 出现在模型出口都算失败。

### G5：关闭 NP-S67–NP-S70 资源边界

- archive 建 offset/ordinal 索引；continuation 只按 selection 和 token budget 读取。
- 搜索索引逐条重建、批量事务、可取消、原子发布。
- job、catalog、revision、refs、issues/diagnostics 在存储层分页。
- 运行 10k metadata、100 MiB 单会话、1 GiB 批次、长工具输出、缺附件、活 WAL 基准。
- 记录冷/热缓存、峰值 RSS、CPU、磁盘增量、首屏、完整扫描、preview、commit、index、reader、MCP 和取消时间；模型响应时间单列。

### G6：关闭 NP-S71–NP-S75 真实 ACP 与恢复

- 从真实 trace 支持 `_meta.tools`，分别记录 slash/MCP-ready/advertised/live-call。
- 明确 capability 状态机：not-requested、starting、mcp-ready、agent-advertised、live-verified、unavailable、policy-blocked。
- official/custom × shared/independent 四组合逐一 search/read/deny；普通会话和 remote/aux 路径不得误注入。
- 100 轮前置约束一次且仅一次进入 `session/prompt`；连接前失败、发送后断线、重复点击和编辑 draft 有确定状态。
- 实测杀 Agent、杀 Host、关窗口、丢 completion、publish 中断、只读/满磁盘；UNKNOWN 不自动重发。

### G7：关闭 NP-S76–NP-S82 产品与三平台验收

- Windows Tauri：真实来源、活库、批量导入、选择、reader、搜索、导出、续聊、撤销；1280×720、窄窗、200%、深浅主题、中文路径、IME、纯键盘。
- macOS：ARM/Intel 路径语义、权限、大小写卷、MCP、大文件和 UI。
- Linux：XDG、权限、symlink、大小写、Wayland/X11、MCP、打包运行。
- 全仓回归、技术债收口、许可/NOTICE、53 项重新评级。
- 没有目标平台时必须给 `blocked-external`、可复制命令和所需证据，不能用 cross compile 冒充实机。

## 6. 测试阶梯

每一步先跑最窄测试，再扩到领域和全仓：

```powershell
# 前端最窄
node node_modules/vitest/vitest.mjs run `
  src/lib/sessionImport/selection.test.ts `
  src/lib/sessionImport/contentSelection.test.ts `
  src/hooks/useContentSelection.test.tsx `
  src/hooks/useCandidatePeek.test.tsx

node node_modules/typescript/bin/tsc -b --pretty false
node node_modules/eslint/bin/eslint.js `
  src/lib/sessionImport `
  src/hooks/useContentSelection.ts `
  src/hooks/useSessionImport.ts `
  src/hooks/useCandidatePeek.ts `
  src/components/session-import `
  --max-warnings 0

# Rust 最窄与领域
cd src-tauri
cargo fmt --all -- --check
cargo test --lib session_migration::selection --offline --no-run
cargo test --lib session_migration::engine --offline --no-run
cargo test --lib session_migration::mcp_stdio --offline --no-run
cargo clippy --all-targets --offline -- -D warnings
```

Windows 测试若以 `0xc0000139 STATUS_ENTRYPOINT_NOT_FOUND` 退出，先给本次生成的 test EXE 嵌入 `src-tauri/windows-test-manifest.xml`，再直接运行该 EXE，并保存能证明进入断言的输出。不要把 `--no-run` 当作测试通过。

每个门禁结束后再按 `CONTRIBUTING.md` 运行完整命令。若 `pnpm exec` 被 Corepack 版本限制阻断，可使用仓库已安装的 `node_modules` 二进制，但必须记录实际命令。

## 7. 记录与完成语言

每个步骤追加：

```text
STEP: Sxx
PLAN_STEP: NP-Sxx
STATUS: in-progress | done-synthetic | done-verified | blocked-external
CHANGED: 精确文件与产品行为
TESTS: 精确命令、退出码、通过/失败数
EVIDENCE: 日志、截图、脱敏 trace 的绝对路径
OPEN: 尚未证明的风险
NEXT: 唯一下一门禁
```

- 只有源码存在：不能写 done。
- 只有 fixture/mock/unit：最多 `done-synthetic`。
- 真实 UI/ACP/活库/目标平台按验收步骤通过：才可写 `done-verified`。

当前环境缺机器或账号：写 `blocked-external`，同时继续所有不依赖该环境的顺序内工作。

## 8. 将来提交拆分建议（本文不执行）

1. Selection v2 + preview branch contract。
2. 内容选择 UI 与状态机。
3. per-preview plan/commit/receipt/manifest 持久化。
4. reader/continuation/MCP 权限闭环。
5. ACP capability 与首次派发/恢复。
6. 流式 IO、分页和性能。
7. 三平台适配、实机修复与最终 QA 文档。

每批在独立测试绿后再由用户审核。没有明确授权，不 commit、push、PR、merge 或 release。

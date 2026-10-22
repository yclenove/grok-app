# 会话与工作区导入：Grok 当前成果审计与后续开发执行书 V4

- 审计时间：2026-09-10 06:20（Asia/Shanghai）
- 工作树：`H:\aicoding\grok-app-session-import`
- 分支：`feat/session-workspace-import`
- 基线 HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`
- 当前状态：大量未提交 WIP；没有 commit、push、PR、merge 或 release
- 本地基线漂移：相对本机 `origin/main` 为 0 ahead / 17 behind，相对真正源项目本机 `upstream/main` 为 0 ahead / 57 behind；本轮未 fetch，远端实际差距可能更大
- 本文优先级：覆盖 V3 中已经过期的当前状态和测试数字；历史设计仍可参考

## 1. 一句话结论

Grok 已经写出了有实质内容的跨来源导入系统，Codex、Claude Code、Cursor 和通用文件的主干能力不是 mock；但它现在仍是“主体已成形、授权与恢复边界未闭、产品链路有断点”的开发中版本，不能提交，更不能称为完成。

当前最重要的变化是：R0.5 已经关闭，最新完整 Rust 领域套件为 278 passed / 0 failed。下一步必须从 R1 的 SelectionSet 原子迁移开始，不得重复修 R0.5，也不得先横向增加新来源或 UI 入口。

## 2. Grok 实际完成了什么

### 2.1 代码规模与产品覆盖

审计开始时工作树包含 46 个已跟踪修改文件和 139 个未跟踪文件；加入本文和配套提示词后为 141 个未跟踪文件、约 38.7k 个物理行。除计划文档外，新增源码、i18n 与 fixture 约 34k 个物理行，属于需要分批审阅的大型 WIP；其中：

| 区域 | 规模 | 当前评价 |
| --- | ---: | --- |
| `src-tauri/src/session_migration/` | 34 个 Rust 文件，约 24k 行 | 有实质领域实现，但大文件和跨层契约仍未收口 |
| Tauri command | 292 行 | IPC 已接入 SelectionSet v2 |
| 前端导入组件/Hook/库 | 约 5.4k 行 | 主流程可见，跨组件状态和错误路径不足 |
| 15 locale session-import 文件 | 约 2.9k 行 | 键基本齐全，但多数 locale 仍有大量英文占位 |
| 研究、契约、计划、验收文档 | 约 2.9k 行 | 覆盖面广，但旧状态和旧日志不能当当前证据 |

已落地的主能力：

1. Codex、Claude Code、Cursor、通用 JSON/Markdown 的来源发现与 adapter。
2. Cursor SQLite/WAL 私有快照；不 checkpoint、VACUUM、repair 或写回来源库。
3. 归一化事件图、branch、subagent、tool call/result、compaction、unsupported 和附件状态。
4. 分页 scan、冻结选择、cancel/continue、source revision、来源登记与重定位。
5. preview lease、固定 snapshot、事件分页、内容选择和 workspace mapping 基础。
6. durable job、receipt、retry、cancel、undo、tombstone 和部分发布恢复。
7. 独立 `imp:` 历史、reader、CJK search、Markdown export、全局搜索接线。
8. 显式 Grok continuation draft、sidecar、first-send gate 和 import-history MCP。
9. 设置、侧栏、项目菜单、导入向导、作业结果和 imported reader 产品入口。

这些工作说明方向和投入是真实的，不是按钮壳子。

### 2.2 做得比较好的部分

- foreign history 保持只读，没有伪装成原生 resume；续聊是显式、有界的 Grok handoff。
- 来源数据库的只读边界、路径逃逸、WAL、损坏输入、半行 JSONL 和 source changed 有较多反例测试。
- `AllowedSelection.project_event()` 已形成统一投影基础，system/opaque reasoning/兄弟 branch 等有 deny 测试。
- scan、preview 和 job 已考虑分页、取消、迟到响应和幂等，而不是一次性全读。
- R0.5 已正确区分三种证据：MCP 自身 `tools/list`、Agent 广告 `_meta.tools`、真实 live tool call；`availableCommands` 不再误解为 MCP 工具。
- AppWorkbench 本次净减少，没有继续往被冻结的 App shell 塞大块状态。

### 2.3 Grok 自报进度中不够可靠的地方

- `grok-session-import-progress.md` 把一批本机可执行项写成 `blocked-external`，包括合成 100 MiB/1 GiB fixture、Windows dev Tauri 和本机故障注入；这些不是天然外部阻塞。
- 多个阶段只用一两个定向测试就标记 `done-synthetic`，不能证明跨层闭环。
- 旧 S65 手写日志后来已自行作废；旧 S41/S47 等全仓结果早于 9 月 10 日最新源码，不能继续作为当前门禁。
- `docs/plans/session-import-p0/acceptance-results.md` 的 53 项状态早于最新改动，必须在 R1-R9 后全部重审。

质量判断：基础实现值得保留并收口，但“完成度报告”明显比真实可交付状态乐观。

## 3. 本次独立验证

### 3.1 当前通过

| 门禁 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check` | exit 0 |
| `pnpm typecheck` | exit 0 |
| session-import 相关定向 ESLint，`--max-warnings 0` | exit 0 |
| `pnpm lint` | exit 0 |
| `pnpm deps:check` | exit 0 |
| i18n parity + settings catalog | 2 files / 60 tests passed |
| capability 定向 Rust | 12 passed / 0 failed |
| continuation 定向 Rust | 29 passed / 0 failed |
| selection 定向 Rust | 9 passed / 0 failed |
| 完整 `session_migration` Rust | 278 passed / 0 failed / 1611 filtered；349.06s；exit 0 |
| 4 个 selection/content 定向 Vitest | 10 passed / 0 failed |

完整 Rust 结果对应 05:48 重新链接、重新嵌入 Windows manifest 的最新测试 EXE；晚于当前 `session_migration` 源码。

### 3.2 当前仍为红色或未验证

相关前端 13 文件套件：

```powershell
pnpm exec vitest run `
  src/hooks/useSessionImportScan.test.tsx `
  src/hooks/useCandidatePeek.test.tsx `
  src/hooks/useContentSelection.test.tsx `
  src/lib/sessionImport/contentSelection.test.ts `
  src/hooks/useSessionImportJob.test.tsx `
  src/hooks/useImportedReader.test.tsx `
  src/hooks/useImportedCatalog.test.tsx `
  src/components/session-import/SessionImportSourceStep.test.tsx `
  src/components/session-import/ImportedEventList.test.tsx `
  src/lib/sessionImport/selection.test.ts `
  src/lib/sessionSearch.test.ts `
  src/i18n/messages.test.ts `
  src/lib/settingsCatalog.test.ts
```

结果：13 files；12 passed / 1 failed；142 passed / 3 failed。三个失败都在 `useImportedReader.test.tsx`，旧断言仍期待四参数调用，而生产代码已传第五个 `leaf` 参数。应先确认 leaf 契约，再修测试并补 revision/owner 反例，不能简单删除断言。

`pnpm audit:prod` 当前 exit 1：`@tiptap/core 3.30.4` 命中 high severity `GHSA-j95f-988m-3j2f`，修复版本为 3.30.5 或更高。本机现有 `upstream/main` 已升级到 Tiptap 3.31.3，预计同步上游即可关闭；最终仍需 fetch 后复核。不要把重复依赖升级混入 SelectionSet PR。

`HEAD..upstream/main` 的本地 ref 已涉及 362 个文件，与当前会话导入 WIP 重叠 14 个路径，包括 `Cargo.toml`、ACP、session manager、store、AppWorkbench、sidebar、search hook 和 session API。必须在 R1 形成绿色原子边界后设置独立同步门禁，不能等 R8/R9 才第一次碰上游。

当前源码之后尚未完成：全仓 Vitest、`build:ui`、当前源码 clippy、全库 Cargo test、真实 Tauri、真实 ACP、真实来源活库、macOS 和 Linux。

`git diff --check` 为 exit 0，但它看不到未跟踪文件的行尾问题。14 个非英文 `session-import.ts` 当前混合 CRLF/LF，其中 11 个缺最终 LF；不要全仓机械 renormalize，R9 单独修本功能新增文件。

## 4. 阶段账本

| 阶段 | 状态 | 结论 |
| --- | --- | --- |
| R0.5 capability 基线 | `done-synthetic` | 当前完整 Rust 领域套件绿色；真实 ACP 仍属 R8 |
| R1 SelectionSet 唯一真相 | `in-progress / P0` | 只贯穿到 IPC/PlanState；commit、durable、receipt、dedup、revision 仍是旧契约 |
| R2 多会话内容状态机 | `in-progress / P0` | 后端有分页；前端仍是单 peek、单 content |
| R3 plan/job/revision 持久恢复 | `in-progress / P0` | job/receipt 有骨架；plan 只在内存 |
| R4 reader/continuation/MCP 授权 | `partial / P1` | 投影基础已在；sidecar/grant/export 仍有多份授权真相和 optional selection |
| R5 cursor/IO/index 原子性 | `partial / P0` | 有部分 hash/corrupt 测试；裸 cursor、fail-open、索引替换风险仍在 |
| R6 有界读取/分页 | `partial / P1` | 有预算和部分分页；整份读取、catalog 第一页、重复 reader 仍在 |
| R7 性能/故障 harness | `mostly-unstarted` | 只有 10k scan 合成基线；100 MiB/1 GiB/RSS/kill 矩阵未做 |
| R8 Windows Tauri/真实 ACP | `unverified` | 生产接线存在；没有实机闭环和脱敏 trace |
| R9 全仓门禁/技术债/i18n | `unstarted` | 快速门禁局部绿色；全仓与真实翻译未完成 |
| R10 53 项重审/PR 规划 | `unstarted` | 旧报告过期，禁止据此拆 PR |

## 5. 当前 P0/P1 缺口

### 5.1 R1 P0：SelectionSet 在 plan 后被拆回旧结构

1. `plan_with_selection_set()` 先 canonicalize 再 validate；重复 event/pinned/asset ID 会先被 dedup，绕过重复 ID 拒绝。
2. `AllowedSelection` 和 `SelectionFields` 的 v2 serde 仍有大量 default；缺失字段可变成默认授权。旧 v1 兼容必须与新 v2 wire/durable 严格解析分开。
3. `engine::commit()` 只拿 `.selection`，丢失 previewId、leafId 和 SelectionSet version。
4. `CommitItem.selection`、`DurableInput.selection` 仍是 `Option<AllowedSelection>`；durable 还带 serde default。
5. `Receipt.selection_hash` 仍可选，没有完整 SelectionSet、options hash 和 preview identity。
6. prior receipt 只比较 snapshot/content hash。
7. revision dedup 只使用 snapshot + options，不包含 selection。
8. `selection.json` 在 staging rename 后才写，且只写裸 AllowedSelection。
9. revision hash 漏掉 `assets.json` 和 `selection.json`。
10. `verify_published()` 不解析/验证 SelectionSet identity 和 semantic hash。
11. receipt、manifest、assets、selection 的生产序列化仍有 `unwrap_or_default()`，失败时可能写出空 JSON。

### 5.2 R2/R4 产品 P0/P1

1. `SessionImportHost` 只有一份 `useCandidatePeek()` 和一份 `useContentSelection()`；A→B→A 会丢 branch、勾选、页、cursor、busy/error。
2. content reset effect 压掉 `react-hooks/exhaustive-deps`。
3. branch confirm 是裸 `.then()`，无 catch/finally、generation 或 owner 检查。
4. 切 branch 后 load-more 仍调用旧 `peek.loadMore()`，且请求没有 leaf。
5. plan 前仍使用 `leafId ?? ""` 和 `selection ?? defaultSelection()`。
6. 搜索键盘打开 imported history 可识别 `kind`，鼠标点击却把结果降成普通 SessionRow 后走原生 `openSession`。
7. continuation 创建成功后，实际挂载没有传 `onOpenNativeSession`，不会导航到新会话。
8. Host 把 peek/reader error callback 定义成空函数，初始错误可能完全不可见。
9. reader 无 owner/generation；A 的迟到响应可覆盖 B。
10. revision deep link 参数被声明但监听时忽略。

### 5.3 R5/R6/R9 P1

1. preview/page/reader/job cursor 仍有裸数字形式，未统一绑定 owner/snapshot/leaf/filter/version/expiry。
2. 损坏 preview lease 被当作不存在；部分 relations/manifest/index 错误被默认成空。
3. index replace 仍可能先删 live 再 rename，没有完整 backup/swap/rollback/fsync 证明。
4. 整份 `load_events()` / `load_archive_events()` helper 仍以公开/dead 形式保留在带 `allow(dead_code)` 的模块中；当前静态调用点仅见测试或无调用，应删除或私有化，并用调用图证明生产不可达。
5. catalog wrapper 丢弃 `nextCursor`，UI 只显示前 50 条。
6. `useSessionImportUi` 与 `useImportedReader` 重复加载 reader 数据。
7. workspace mapping DTO 有 create project/explicit target/unassigned，但 UI 只完成部分交互。
8. i18n 是“键齐”而非“翻译完成”；每个 locale 有 190 键，但德语约 65%、多数西文/韩文/泰文约 68% 的值仍与英文完全相同。

## 6. 下一轮唯一执行顺序

### R1.0：固定前端和后端基线

1. 保存当前 `git status --short --branch`、HEAD 和相关文件 mtime。
2. 记录 278/0 Rust 与 142/3 前端基线；不要重复修改 R0.5。
3. 确认 `useImportedReader` 第五个 leaf 参数是当前正式契约。
4. 更新三个过期测试期望，并增加“切 leaf/revision 时使用新 owner”的反例。
5. 重跑上面的 13 文件套件至 0 failed，随后才改 R1 生产类型。

退出条件：前端相关套件基线绿色；没有通过删测试、弱化断言或 catch 空错误达成。

### R1.1：严格 v2 schema 和共享验证器

目标：解析、规范化、授权验证是三个明确步骤，任何输入不得先被修正后再验。

1. 在 `selection.rs` 将 live v2 wire/durable 结构改为严格 required 字段；建议对 v2 DTO 使用 `deny_unknown_fields`。
2. v1/旧草稿兼容只通过显式 `parse_stored_selection(Value)` 路径处理，不得让新 IPC/durable 自动走 serde default。
3. 顺序必须是：raw decode → raw shape/duplicate validation → semantic validation → canonicalize → hash。
4. `SelectionSet::validate_shape()` 增加非空和上下文要求；job item 使用 `single_entry()` 强制恰好一个 entry。
5. 建立一个共享 prepared selection validator，至少验证：
   - schema version；
   - previewId、candidateId、source/lease identity；
   - snapshot digest；
   - 非空且存在的 leaf；
   - event/pinned IDs 唯一、存在、属于 leaf；
   - system/opaque/兄弟 branch 不可选；
   - subagent 关系与 includeSubagents；
   - asset 已 stored、被选中事件引用、路径不可猜测。
6. plan、durable reload、published verify 必须调用同一语义验证器，不得复制三套近似规则。
7. TypeScript 新增严格 `validatePlanSelectionBoundary()`；不得依赖静态类型代替运行时验证。

先写失败测试：缺字段、unknown field、错误 version、重复 ID、空 ID、unknown/cross-preview event、unknown/unselected asset、0/2 entries。

### R1.2：原子贯穿 Plan→CommitItem→DurableInput

1. `CommitItem.selection_set: SelectionSet`，必填，必须恰好一个 entry。
2. `DurableInput.selection_set` 和 `selection_hash` 必填，删除 serde default/Option 旁路。
3. DurableInput 明确保存 preview identity；from_item 写入前验证，into_item 重载时再次验证。
4. `engine::commit()` 找到完整 `PreviewSelection` 后使用 `SelectionSet::single(entry.clone())`。
5. `commit_item_from_preview/parts` 只接收完整单项 SelectionSet。
6. reopen prepared revision 后重新计算 prepared hashes、严格读取 lease、验证 selection hash 与完整语义。
7. 将旧 `plan()` / `plan_with_selection()` / `plan_with_selections()` 限制为 `#[cfg(test)]` helper 或删除生产入口。
8. 修改全部测试构造器；统一 helper 必须从真实 prepared leaf/events/assets 生成合法 selection，禁止用空选择让编译通过。

退出条件：production 不再出现 `CommitItem.selection: Option<_>` 或 `DurableInput.selection: Option<_>`；删除/篡改 durable selection 必然 fail closed。

### R1.3：Receipt、dedup、publish 与恢复原子化

1. Receipt 必填保存：完整单项 SelectionSet、selection hash、options hash、snapshot hash、preview/candidate identity。
2. prior receipt 恢复同时核对 snapshot、selection、options、candidate、preview、provider/native identity；任一变化返回 `PlanStale`。
3. revision semantic key 必须至少绑定 `{snapshotHash, optionsHash, selectionHash, normalizerVersion}`；不同 selection 不得 dedup。
4. 将 hash 拆成两个函数：
   - prepared hashes：manifest、events、assets、blobs；
   - published hashes：prepared hashes + selection.json。
5. 在 staging 内先严格序列化完整单项 SelectionSet，写 `selection.json`，flush/fsync，再计算 published hashes，最后 rename。
6. 禁止 `serde_json::to_vec_pretty(...).unwrap_or_default()`；所有生产序列化错误显式返回。
7. `verify_published()` 同时验证文件 hash、SelectionSet shape、exactly-one、selection hash、preview/leaf identity。
8. 恢复 rename-before-receipt 等边界时，不得从 default selection 重建授权。

必测：同 snapshot/options 不同 selection 不去重；selection/assets 缺失或篡改；receipt selection/options/identity 篡改；rename 前后失败；重复恢复不产生第二 revision。

### R1.4：前端提交边界 fail closed

1. plan 前比较 selected candidate、preview map、selection map 三个 key 集合，必须精确相等。
2. 验证 previewId/leafId 非空且唯一、SelectionSet/AllowedSelection version 正确、ID 无空白/重复、pinned 是 event 子集。
3. 删除 `leafId ?? ""`、`selection ?? defaultSelection()` 和任何缺状态时自动扩大授权的 fallback。
4. 缺失状态显示 i18n 主错误，并保留可复制诊断；不得硬编码用户文案。
5. 同步 15 locale 键，但真实翻译质量放在 R9 统一验收。
6. 补 TS 纯函数测试和 hook 提交阻断测试。

### R1.5：R1 门禁

按以下顺序执行：

1. `cargo fmt --all -- --check`
2. Rust compile-only；重新嵌入 Windows manifest
3. selection 定向
4. jobs 定向
5. job_control 定向
6. engine 定向
7. 完整 `session_migration`，必须 278/0 或更多、0 PoisonError
8. 前端 selection/content/reader 13 文件套件，必须 0 failed
9. `pnpm typecheck`
10. session-import 定向 ESLint，`--max-warnings 0`

R1 所有退出条件满足前，禁止进入 R1.6 或 R2。

### R1.6：上游集成安全门禁

1. R1 全绿后先只读刷新并审计 `upstream/main`，记录 ahead/behind、362 文件变化是否已漂移、14 个重叠路径及语义冲突。
2. 当前巨大 WIP 没有可恢复 checkpoint；未经用户明确授权，禁止直接 merge、rebase、stash、reset 或 checkout。
3. 向用户申请“仅本地 checkpoint commit + 同步 upstream”的授权；仍不得 push 或提 PR。
4. 获得授权后，把当前成果按可审责任边界做本地 checkpoint，fetch upstream，在专门集成步骤逐文件做语义合并。
5. 重点复核 ACP first-send、session manager、store、AppWorkbench/search 路由和 API DTO，不得用 ours/theirs 整文件覆盖。
6. 同步后重跑 R0.5、R1 全套、前端 13 文件、typecheck、lint、build 和 audit；只有全部恢复绿色才进入 R2。

若没有 checkpoint 授权，状态只能写 `needs-user-authorization`，不得把它伪装成技术性 `blocked-external`，也不得继续在落后 57 个提交的基线上扩展 R2-R8。

### R2：previewId keyed 多会话内容状态机

1. 新建 domain hook/reducer，例如 `useSessionImportContentStates`；不得把状态加回 AppWorkbench。
2. 每个 preview 独立保存 candidateId、previewId、snapshot、leaf、selection、pages、nextCursor、filter、generation、dirty、valid、busy、error。
3. 固定 selected previews 时为每项建立初始合法 state；不能只初始化当前打开详情。
4. A→B→A 恢复各自完整状态；明确取消再选是保留还是清空，并写测试。
5. branch/load-more 请求携带 previewId、snapshot、leaf、filter、cursor、generation；迟到 owner/generation 一律丢弃。
6. 禁止 branch 后调用旧 `peek.loadMore()`；所有异步路径 try/catch/finally，失败后 busy 必须复位且错误可见。
7. 删除 exhaustive-deps 抑制；将 SessionImportHost 的业务块拆入 domain 组件/hook。

退出条件：A/B 切换、branch/load-more 迟到与失败、双击、取消再选、键盘和 disabled 均有测试。

### R3：持久化 plan 与完整恢复

1. catalog 增加版本化 plan 存储：canonical SelectionSet、snapshots、mappings、options、hash、created/expiry。
2. `plan()` 返回前持久化；`commit()` 从 durable plan 加载并重验，不依赖 `Runtime.plans`。
3. requestId 幂等绑定 planId + planHash；过期、篡改、选择/映射/options 变化 fail closed。
4. 覆盖 plan 后 Host 重启、commit 前重启、各 publish phase 中断、receipt recovery。
5. current revision 切换必须保证只看到完整旧 revision 或完整新 revision。

### R4：授权唯一真相与产品断链

1. sidecar/grant 保存一个完整 v2 SelectionSet；派生 event IDs/hash 必须重新计算并一致。
2. handoff、model export、MCP search/read/asset read 全部从 revision selection 加载并走同一 evaluator。
3. hidden/delete/undo/new revision/selection tamper 立即使旧 grant/draft stale 或 revoked。
4. 修全局搜索鼠标点击 imported history 路由，补键盘/鼠标一致性组件测试。
5. 给 SessionImportHost 正确传 `onOpenNativeSession`，续聊创建后导航一次且仅一次。
6. 接通 revision deep link；切 revision/leaf 时 reader owner 更新，迟到响应不能覆盖。
7. secret matrix 覆盖 system、reasoning、tool input/result、兄弟 branch、subagent、compaction、asset。

### R5：cursor、fail-closed IO、索引安全发布

1. 所有 cursor 使用版本化 envelope，绑定 owner、snapshot/index revision、leaf、filter、offset、expiry。
2. 拒绝裸数字、tamper、cross-owner、cross-leaf/filter、stale/expired。
3. 审计生产 `unwrap_or_default()` / `.ok()`；损坏 lease/manifest/relations/index/receipt 不得变成空成功。
4. readonly catalog 只验证已有 schema，不初始化或执行 migration DDL。
5. 索引发布使用同目录 temp + fsync + backup/swap/rollback，并实现启动恢复。
6. 覆盖 Windows sharing violation、并发 reader/rebuild、每个 rename 失败点、取消和重启。

### R6：有界读取、catalog 分页和 reader 去重

1. 删除/私有化生产可达的整份 event/archive 读取；用调用图证明。
2. revisions、continuation refs、issues/diagnostics、catalog、job items 全部存储层分页。
3. API 保留 catalog `{items,nextCursor,total}` envelope；UI 支持 load more/虚拟化，不能只显示前 50 条。
4. 合并 `useSessionImportUi.readerEvents` 与 `useImportedReader`，每份数据只加载一次。
5. reader merge 按 owner/generation 去重并执行条数/字节预算。
6. 10k 测试必须包含反序列化或内存证据，不能只断言返回 50 条。

### R7：本机可复跑性能与故障 harness

1. 仓库内生成合成 10k metadata、100 MiB 单会话、1 GiB 批次、超长 tool output、缺附件和 WAL。
2. 测 cold/warm wall time、first visible、峰值 RSS/CPU、磁盘增量、取消确认时间。
3. 只用隔离 `GROK_APP_HOME` 和自己启动的进程注入 kill、只读目录、空间不足和 publish 中断。
4. 不读取私人历史，不杀用户现有 Grok/Codex/Cursor/Node/Cargo 进程。

### R8：Windows Tauri 与真实 ACP

1. 隔离 app home 启动 dev Tauri，走通来源→扫描→多选→内容→映射→导入→reader→搜索→导出→续聊→撤销。
2. 覆盖 1280×720、窄窗、200%、深浅主题、中文路径、IME、纯键盘。
3. 脱敏 trace 区分 slash commands、自身 tools/list、Agent `_meta.tools`、live tool call。
4. 验证 official/custom × shared/independent；remote/official-aux 不误注入。
5. 100 轮 first-send exact-once；UNKNOWN 不自动重发。
6. 完成 workspace mapping 的 create project、explicit target、unassigned 确认交互。

### R9：全仓门禁、i18n、行尾和技术债

1. 按 `CONTRIBUTING.md` 跑 deps/audit/typecheck/test/lint/build、fmt/clippy/full cargo test。
2. 复核 R1.6 同步后 `@tiptap/core` high advisory 已关闭；不要重复做上游已有的依赖升级。
3. 完成 15 locale 的真实翻译；不得把英文占位当完成。
4. 只规范本功能新增的 14 个 mixed-EOL locale 文件；禁止全仓机械 renormalize。
5. 行为稳定后再拆 `engine.rs`、`jobs.rs`、`continuation.rs`、`mcp_stdio.rs`、`SessionImportHost.tsx`。
6. 检查 hardcoded UI、secret、license/NOTICE、settings search、setup gate 和 AppWorkbench growth。
7. macOS/Linux 用真实机器或 CI 运行；cross compile 只记 compile evidence。

### R10：53 项重审与小 PR 计划

1. 逐项重审 F01-F12、W01-W07、D01-D05、S01-S04、R01-R07、C01-C11、U01-U07。
2. 只能标：实机通过、合成测试通过、部分、失败、未验证、blocked-external。
3. 每项链接最新代码之后生成的日志、截图或脱敏 trace。
4. 给出小 PR 拆分和依赖顺序，但未经用户审核不得 commit、push、PR。
5. 使用 R1.6 已完成同步后的基线规划小 PR；若 R1.6 仍未获授权，明确列为提交前阻塞。

## 7. Windows Rust 测试规则

每次 Cargo 重新链接 test EXE 后，都重新嵌入 `src-tauri/windows-test-manifest.xml`。`0xc0000139` 表示测试程序未进入断言，不能算业务失败或通过；`--no-run` 只算编译。

```powershell
cd H:\aicoding\grok-app-session-import\src-tauri
cargo test --lib session_migration --offline --no-run

$mt = "C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\mt.exe"
$exe = (Get-ChildItem target\debug\deps\grok_app_lib-*.exe |
  Sort-Object LastWriteTime -Descending |
  Select-Object -First 1).FullName

& $mt -nologo -manifest windows-test-manifest.xml "-outputresource:$exe;#1"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& $exe session_migration::selection --test-threads=1 --nocapture
& $exe session_migration::jobs --test-threads=1 --nocapture
& $exe session_migration::job_control --test-threads=1 --nocapture
& $exe session_migration::engine --test-threads=1 --nocapture
& $exe session_migration --test-threads=1 --nocapture
```

## 8. 记录与停止规则

每个小步只在真实命令结束后记录：

```text
STEP: Rn.x
STATUS: in-progress | done-synthetic | done-verified | blocked-external
BASELINE: 原始失败、exit code
CHANGED: 文件与行为
TESTS: 完整命令、exit code、passed/failed/filtered、耗时
EVIDENCE: 最新日志、截图、脱敏 trace 绝对路径
OPEN: 尚未证明的风险
NEXT: 唯一下一门禁
```

- compile-only 或只写代码：`in-progress`。
- fixture/unit 通过：最多 `done-synthetic`。
- 当前平台真实 UI/活库/真实 ACP/恢复矩阵通过：对应子项才是 `done-verified`。
- 只有确实缺机器、账号、授权或路由的精确子项可标 `blocked-external`。
- 本机可生成 fixture、可启动 dev Tauri、可注入自有子进程故障，不得标外部阻塞。
- 不得伪造日志、passed 数、性能数字或协议字段。
- 不得读取/输出私人会话、token、Cookie、`auth.json` 或 secrets。
- 未经用户审核和明确授权，不得 commit、push、PR、merge、tag 或 release。

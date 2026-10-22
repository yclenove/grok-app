# 会话与工作区导入：Grok 当前成果审计与下一阶段开发计划 V2

- 审计时间：2026-09-10 04:54（Asia/Shanghai）
- 工作树：`H:\aicoding\grok-app-session-import`
- 分支：`feat/session-workspace-import`
- 基线 HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`
- 状态：大量未提交 WIP；没有 commit、push 或 PR

## 1. 结论先行

Grok 已经完成的不是一个空壳：跨 Codex、Claude Code、Cursor 和通用文件的导入领域、只读快照、归一化、预览、后台作业、阅读器、检索、撤销以及受控续聊骨架都已落到生产代码，测试资产也很可观。

但当前还不能称为“首版开发完成”。更准确的阶段判断是：

> 基础导入内核和多数合成场景已经成形；SelectionSet v2 正在改造中；多会话选择、不可变持久化、模型授权、资源边界、真实 ACP、Windows 产品闭环和三平台验收仍未关门。

当前最重要的问题不是继续横向加功能，而是先让新的 SelectionSet 契约贯穿现有链路，并恢复稳定的领域回归。旧进度日志中的 `done-synthetic` 只表示某些定向样例曾通过；不能直接外推为对应整个阶段完成。

## 2. Grok 已经做了哪些工作

### 2.1 已有产品与领域能力

| 领域 | 当前实现 | 审计判断 |
| --- | --- | --- |
| 来源发现 | Codex、Claude Code、Cursor、通用 JSON/Markdown；默认根、自定义根、登记/重定位、分页扫描 | 已实现，合成验证较强；真实版本矩阵未完成 |
| 只读来源 | JSONL 完整前缀；Cursor SQLite/WAL 私有快照；不 repair/checkpoint/VACUUM 来源库 | 方向正确；仍需活 Cursor 写库实测 |
| 归一化 | 消息、工具调用/结果、reasoning、compaction、unsupported、fork/subagent/rollback 关系 | 已有大量适配与 golden 测试；未知版本仍需真实样本 |
| 身份/revision | `imp:` 命名空间、source identity、固定 snapshot、preview lease、刷新/冲突 | 基础扎实；Selection v2 与 revision 完整性尚未统一 |
| 附件 | content-addressed blob、missing/too-large/remote/outside-root、取消清理 | 合成路径已实现；真实媒体和 UI 打开未验 |
| 工作区映射 | 多 root、existing/new/unassigned、host identity、项目保护与恢复 | 模型较完整；跨平台路径/权限未实测 |
| 作业恢复 | plan/commit/job/receipt、幂等、取消、retry、undo、tombstone、故障注入 | 骨架成熟；plan 仍内存-only，selection 未完整持久化 |
| 阅读/搜索 | imported history 分组、分页阅读、CJK 搜索、Markdown 导出、全局搜索跳转 | 已接入产品；授权和大历史边界仍需统一 |
| 原生隔离 | `imp:` 不进入原生 connect/send/usage/unread 等路径 | 合成验证较强；活会话、CLI、Remote IM 需回归 |
| 续聊 | 显式创建 Grok draft、固定 revision、首次发送闩锁、sidecar、import-history MCP | 有完整骨架；真实 ACP 工具广告/一次发送未验 |
| 前端入口 | 侧栏、项目菜单、设置搜索、向导、候选、peek、映射、结果、reader | 已有可见产品接线；复杂内容选择状态机未完成 |
| i18n | 15 locale 新增 session-import 域并挂到索引 | 已铺开；最终 parity 和语言质量仍需全仓门禁 |

### 2.2 实现规模与工程投入

本次审计看到的新增领域约为：

- `src-tauri/src/session_migration/`：34 个 Rust 文件，约 24k 行。
- Tauri 命令入口：约 292 行。
- session-import 前端组件、hooks、领域模块：约 4.5k 行。
- Rust migration 测试属性约 277 个；相关前端测试用例约 141 个。
- 当前工作树共有约 92 个 modified/untracked 状态项，其中 135 个未跟踪文件；未跟踪文本约 34k 行。

这说明 Grok 的工作量很大，但也意味着必须以小门禁收口，不能继续靠少量定向测试替代整体回归。

## 3. 本次独立验证结果

### 3.1 当前通过

| 检查 | 结果 |
| --- | --- |
| `node node_modules/typescript/bin/tsc -b --pretty false` | 通过 |
| selection/content 定向 Vitest | 4 files / 10 tests 通过 |
| session-import 定向 ESLint | 通过 |
| `cargo fmt --all -- --check` | 通过 |
| `cargo test --lib session_migration::selection --offline -- --nocapture` | 9 passed / 0 failed |
| `cargo test --lib session_migration::selection --offline --no-run` | 编译通过 |

最早一次 Rust compile-only 曾因 `PlanState.selections` 已改名为 `selection_set`、测试仍引用旧字段而失败；Grok 随后把这 5 处测试引用同步完成，compile-only 重新通过。这说明它仍在实际推进，但也证明当前是动态重构期。

### 3.2 当前失败

`cargo test --lib session_migration::engine --offline -- --nocapture` 的结果：

- 31 passed
- 10 failed
- 1847 filtered out

其中两个独立复跑可稳定失败，是首发根因：

1. `restart_keeps_unexpired_preview_without_ram`
   - 错误：`HISTORY_ACCESS_UNAVAILABLE: preview leaf is required`
   - 含义：Host 重启后从磁盘打开 preview 的路径与新 required leaf 契约没有完全对齐。不能简单把 leaf 改回可选或空字符串；应保证 prepared manifest/index 能恢复确定 leaf，或为合法无分支单线会话定义一致的默认 leaf。
2. `changing_selection_invalidates_plan_hash`
   - 错误：`selection references an unknown event or leaf`
   - 含义：旧测试手写 `e1` / `secret`，但这些 ID 不属于生成快照。严格校验正确地拒绝了测试数据。测试必须先从 preview 读取真实 leaf/event IDs，再修改一个仍然合法的 selection 维度来证明 hash 改变；未知 event 应成为单独的拒绝测试。

其余 8 个失败是共享 `APP_HOME_ENV_LOCK` 被上述 panic 污染后的 `PoisonError` 级联。修复顺序必须是：先分别关闭两个根因，再重跑整个 engine；不要给 8 个级联失败分别打无意义补丁。

## 4. 对当前完成度的评级

| 批次 | 当前真实状态 | 说明 |
| --- | --- | --- |
| NP-S59 Selection v2 schema/兼容 | `done-synthetic` | v2 DTO、canonical hash、v1 最小权限兼容和 golden 已存在 |
| NP-S60 唯一 evaluator | `done-synthetic` | block/tool/asset/event projection 和部分 MCP/export 已接入 |
| NP-S61 preview branch/default selection | 进行中 | 实现已出现，但 Host 重启恢复测试失败，cursor 契约也不完整 |
| NP-S62 内容选择 UI | 进行中 | 单详情 reducer 已有；多会话 A/B 状态、branch/load-more 和错误态未闭环 |
| NP-S63 plan/commit | 进行中 | SelectionSet 已进入 IPC/PlanState；plan 内存-only，job/revision 仍只存 `AllowedSelection` |
| NP-S64–NP-S69 reader/MCP/index/page | 实现已出现、待逐门禁验收 | 不能用每次 1–4 个测试、`1887 filtered out` 的日志批量关闭六个阶段 |
| NP-S70–NP-S80 | 大部分未执行 | 100 MiB/1 GiB、Windows Tauri、隔离 kill/IO 故障和本机全仓回归可在本机做，不应整体标 external |
| NP-S81 技术债 | 未执行 | 必须完成 |
| NP-S82 最终 53 项验收 | 未执行 | 必须完成 |

保守但实用的判断：基础内核约七成成形，产品可交付度明显低于此。现在适合继续收口，不适合提交为一个巨大 PR，也不适合宣称 Windows/macOS/Linux 全部可用。

## 5. 当前 P0 问题清单

### P0-A：SelectionSet v2 还没有贯穿唯一真相

- IPC 已只接收必填 `SelectionSet`，TS/Rust `leafId` 也已 required，这是有效进展。
- `PlanState` 已使用 `selection_set` 和 `selection_hash`，exact-cover 已检查重复、遗漏和未知 preview。
- 但是 `CommitItem`、`DurableInput` 仍只保存 `Option<AllowedSelection>`，丢失 preview/leaf/SelectionSet version。
- `Receipt` 仍只有 `Option<String> selection_hash`，没有不可变选择内容或受保护引用。
- `selection.json` 在目录 publish/rename 之后才写，且 `revision_hashes()` 只覆盖 `manifest.json`、`events.jsonl` 和 blobs；因此 selection 不受 revision/receipt 完整性保护。
- plan 仍只存在 `Runtime.plans: HashMap`；Host 重启后 commit 会 `plan not found`。
- 前端 `commit()` 仍对缺 selection 使用 `leafId ?? ""` 和 `defaultSelection()`，在边界处重新制造不合法默认值。这里应在 UI 状态层 fail closed，而不是等 Host 拒绝。

### P0-B：多会话内容选择状态仍会分叉

- `selectionByCandidate` 已开始按 candidate 保存 leaf + selection，未打开的 selected candidate 也会在 `previewSelected()` 中得到默认 selection，这是有效进展。
- 但详情 UI 仍是单例 `content` 状态，只在点击“下一步”前保存当前 peek；A→B→A 的页面、cursor、dirty/error 等不会按 preview 独立恢复。
- `previewSelected()` 会为所有选中 candidate 重新创建 preview，旧 selection 仅按 leaf 字符串复用，没有显式 snapshot digest 归属。
- branch confirm 使用裸 Promise `.then()`，没有 catch/busy/generation/snapshot 校验。
- branch 切换后“加载更多”仍调用 `peek.loadMore()`，其内部使用 peek 保存的旧 leaf/cursor，而不是 `content.state` 当前 leaf/cursor。
- Host 组件用 `eslint-disable react-hooks/exhaustive-deps` 抑制同步问题；这不是可接受的最终状态。

### P0-C：错误语义仍有 fail-open

已确认的高风险例子：

- `engine::current_file_revision()` 对普通文件使用 `read(...).unwrap_or_default()`，IO 失败会被伪装为空文件 hash。
- preview path 中仍有 `relations_from_dir(...).unwrap_or_default()`。
- Cursor source revision 对 DB 读取仍有 `read(&db).unwrap_or_default()`。
- `open_catalog_readonly()` 仍调用 `init_schema()`；只读打开不应执行 CREATE/ALTER/migration 路径。

不是所有 `unwrap_or_default()` 都有问题；仅修生产数据/权限/完整性路径。展示缺省和测试辅助可保留，但要逐个分类。

### P0-D：索引发布还不具备强原子性证明

当前日志证明 Grok 处理了 Windows sharing violation 并增加关闭连接/重试，但“重试成功”不等于“任意失败点都保留旧 live”。必须有同目录 temp + backup/swap/rollback 契约，并测 reader 持锁、rename 失败、进程中断和重启恢复。

### P0-E：真实 ACP 与模型授权仍未验证

- `availableCommands` 与 `_meta.tools` 的语义已开始区分，但没有活 ACP trace。
- MCP Ready、自身 `tools/list`、Agent advertised、live tool call 必须是四种独立证据。
- reader 可以看完整归档；handoff/export-to-model/MCP search/read/asset 必须只看唯一 evaluator 允许的投影。
- 首次 handoff 必须一次且仅一次进入真实 `session/prompt`；发送后 UNKNOWN 不得自动重发。

### P0-F：资源边界与真实产品闭环未完成

- 10k metadata 旧记录不能替代当前代码的 100 MiB 单会话、1 GiB 批次、长工具输出、RSS/CPU/磁盘/取消测量。
- 当前 Windows 机器能够生成合成数据和启动 dev Tauri；这些不是天然 external。
- macOS/Linux 实机、确实缺失的账号/路由才可单独 `blocked-external`。

## 6. 下一阶段固定执行顺序

任何阶段未过退出条件，不得进入下一阶段。每阶段要有失败基线、生产改动、最窄测试、领域回归和原始日志。

### R0：冻结并恢复当前红绿基线

目标：让动态 WIP 重新成为可稳定验证的代码树。

1. 记录 HEAD、`git status --short`、相关文件 hash/mtime、正在运行进程。
2. 修 `restart_keeps_unexpired_preview_without_ram`：定义并持久恢复合法 leaf；禁止放宽 required leaf。
3. 修 `changing_selection_invalidates_plan_hash`：用 preview 返回的真实 IDs；另加未知 event 拒绝测试。
4. 分别复跑两个失败测试；再跑完整 `session_migration::engine`，必须 0 failed。
5. 跑 Selection 9 tests、定向前端 10 tests、typecheck、lint、rustfmt。

退出条件：两个根因独立绿；engine 整模块 0 failed；没有用 `unwrap_or_default` 或空 leaf 绕过契约。

### R1：关闭 SelectionSet v2 唯一化

目标：从 UI 请求到 revision/MCP 的每一层都指向同一个不可变授权对象。

1. 删除生产入口中的 `plan()`、`plan_with_selection()`、`plan_with_selections()` 兼容路径，或限制为测试/显式旧格式迁移；新 IPC 只能接受 v2。
2. `CommitItem` / `DurableInput` 保存完整 `PreviewSelection` 或受 hash 保护、可解析的 selection reference，不能只存 `AllowedSelection`。
3. 增加 exact-cover 反例：preview `[A,B]` + entries `[A,A]`、缺 B、未知 C、空 leaf、未知 leaf、未知 event、跨 preview event、unknown/unstored/unselected asset。
4. 证明 selection entry 顺序不改 hash；schema/leaf/field/asset/subagent/snapshot/mapping/options 任一语义变化均改 plan hash。
5. 前端在调用 plan 前验证 selection map 与 preview map 精确等价；缺失时显示 i18n 错误，不填空 leaf/default selection。

退出条件：只有一个生产 DTO/validator；所有反例真实失败；SelectionSet 在 job input 中可恢复。

### R2：关闭多会话 UI 和 branch/page 状态机

目标：每个 preview 独立保存内容选择，迟到/失败响应永不覆盖别的会话或 branch。

1. 建 `previewId -> ContentState` reducer/store，保存 snapshot、candidate、leaf、selection、pages、nextCursor、generation、dirty/valid/busy/error。
2. 首次固定 preview 时创建默认 state；未点开详情的 selected 会话也必须有 selection。
3. A→B→A 恢复各自 leaf、勾选、页面和错误态；取消再选的保留/重置规则写成测试。
4. branch 请求与 load-more 统一使用当前 state 的 previewId + snapshot + leaf + filter + cursor + generation。
5. branch 切换不再调用旧 `peek.loadMore()`；每个 Promise 有 catch/finally，迟到页被拒绝，失败保留旧 state。
6. 删除相关 `eslint-disable`；拆出 `useSessionImportContentStates`，不要继续膨胀 Host。

退出条件：A/B 状态、branch 迟到/失败、load-more 迟到/失败都有组件/hook 测试；键盘和 disabled 状态明确。

### R3：持久化 plan/job/receipt/revision

目标：Host 在 plan 后、commit 前或 publish 任意边界退出，都能恢复同一不可变语义。

1. 给 catalog 增加版本化 plans 存储或受控 plan 文件；只保存 canonical SelectionSet、snapshot digests、mapping、options、hash、created/expiry。
2. commit 从持久 plan 加载并重验 lease/source/snapshot/hash，不依赖 `Runtime.plans`。
3. `selection.json` 在 staging rename 前写入；selection/manifest/asset metadata 均进入 revision hashes/manifest。
4. receipt 保存完整受控 reference + hash；每阶段恢复都验证而不是重建默认 selection。
5. 去重路径必须比较不可变 selection 语义；不同 selection 不能错误返回旧 revision。
6. 测 plan 后 Host 重启、重复 requestId、preview 过期、source/mapping/selection/options 变化、rename 后崩溃和 receipt 恢复。

退出条件：清空 Runtime 后仍能 commit 合法 plan；篡改 selection 文件必定 fail closed；publish 不暴露半成品。

### R4：统一 reader/continuation/MCP 授权

目标：完整归档只供本地阅读；任何模型出口只认一个 selection evaluator。

1. sidecar/grant 只存一个 v2 真相；旧 fields 只能由它派生，并在读取时做一致性校验。
2. handoff、model export、MCP search/read/asset read 全部调用同一 evaluator。
3. asset read 只收 assetId；核对 session/import/revision/leaf/selection hash/catalog visibility/stored blob。
4. 用 secret fixture 覆盖 system、reasoning、tool input/result、兄弟 branch、subagent、compaction、图片；对每个模型出口分别断言拒绝。
5. selection/revision 被撤销、隐藏、刷新或篡改后，旧 grant/draft 立即 stale/revoked。

退出条件：不存在第二授权真相；任何 secret 泄露均阻断后续。

### R5：cursor、fail-closed IO 与索引安全发布

目标：分页不可跨作用域复用，所有完整性/IO 失败明确失败，旧索引在重建失败时仍可服务。

1. 所有 preview/search cursor 使用 versioned envelope，绑定 snapshot/index revision、leaf、filter signature、offset、owner。
2. 拒绝裸数字、tamper、cross-preview、cross-leaf、cross-filter、expired cursor。
3. 分类并修复生产 `unwrap_or_default`：source/read/manifest/index/relations 失败不得变空成功。
4. `open_catalog_readonly()` 拆成真正只读 schema validation，不运行初始化 DDL。
5. event index 使用同目录 temp + backup/swap/rollback；启动时可识别并修复中断状态。
6. 测并发 reader+rebuild、Windows sharing violation、rename 任意失败点、截断/旧 schema、取消和重启。

退出条件：任意发布故障保留完整旧 live 或完整新 live；没有缺失窗口。

### R6：删除无界读取并完成存储层分页

目标：超大历史不进入单个 Rust `Vec`、JSON job、IPC payload 或 React 数组。

1. 删除/私有化 `load_events`、`load_archive_events` 等整份读取，或用调用图证明生产不可达。
2. entries、job items、revisions、continuation refs、issues/diagnostics 均存储层分页。
3. relation、selection ID、tool output、单事件正文设字节/数量预算，超限明确 partial/truncated。
4. 前端按 owner/generation 合并去重，不先全读再 `skip/take`。

退出条件：静态调用审计 + 10k/100 MiB 测试证明有界；IPC 页大小可观测。

### R7：本机性能与隔离故障 harness

目标：把可复跑的性能/恢复证据纳入仓库，不使用用户私人历史。

1. 临时生成 10k metadata、100 MiB 单会话、1 GiB 批次、超长 tool output、缺附件和合成 WAL。
2. 记录 cold/warm wall time、first visible、峰值 RSS/CPU、磁盘增量、取消确认；模型耗时单列。
3. 用隔离 `GROK_APP_HOME` 和子进程注入 Host kill、publish 中断、只读目录、空间不足。
4. 不杀用户当前 Grok/Codex/开发进程，不触碰真实来源数据。

退出条件：脚本/命令可复跑；F12/U03/U07/R01/R02/R07 给出当前代码数据与阈值判断。

### R8：Windows Tauri 与真实 ACP

目标：证明用户真正能完成导入和续聊，而不仅是单元测试能调用函数。

1. 在用户授权的 dev Tauri 中跑：来源→扫描→多选→内容选择→映射→导入→reader→搜索→导出→续聊→撤销。
2. 覆盖 1280×720、窄窗、200%、深浅主题、中文路径、IME 和纯键盘。
3. 用脱敏 trace 分开记录 slash commands、MCP ready、自身 tools/list、Agent `_meta.tools`、live tool call。
4. 验证 official/custom × shared/independent；普通/remote/official-aux 不误注入。
5. 100 轮前置上下文一次且仅一次进入真实 `session/prompt`；UNKNOWN 不自动重发。

退出条件：Windows 产品闭环和真实 ACP 子矩阵有截图/脱敏 trace；缺账号只阻塞精确子项。

### R9：全仓回归与技术债

目标：当前 Windows 全仓绿色，模块达到可审查状态。

1. 按 `CONTRIBUTING.md` 跑 deps/audit/typecheck/test/lint/build:ui/rustfmt/clippy/cargo test。
2. 修模块级 allow、重复 DTO、生产 stub、旧 helper、静默 IO、失效命令、硬编码 UI。
3. 拆 `SessionImportHost.tsx`（约 1068 行）、`engine.rs`（约 3434 行）、`continuation.rs`（约 1672 行）、`jobs.rs`（约 1449 行），但只在行为测试稳定后做。
4. `git diff --check`、LF 策略、15 locale parity、秘密扫描、依赖/许可/NOTICE 收口。
5. macOS/Linux 用 CI/目标机分别验；cross compile 只能算 compile evidence。

退出条件：Windows 当前树所有本机可执行门禁通过；P0/P1 清零；跨平台缺口有精确 handoff。

### R10：重审 53 项与拆分提交准备

目标：形成用户可审核的事实报告，不直接提交。

1. F01–F12、W01–W07、D01–D05、S01–S04、R01–R07、C01–C11、U01–U07 逐项重新评级。
2. 每项只能为：实机通过 / 合成测试通过 / 部分 / 失败 / 未验证 / blocked-external。
3. 每项链接最新日志/截图/trace，旧绿灯不得覆盖新改动。
4. 形成小 PR 拆分建议，但未经用户审核不得 commit/push/PR。

建议未来拆分：

1. Selection v2 + preview contract。
2. 多会话内容选择 UI/state machine。
3. durable plan/job/receipt/revision。
4. reader/continuation/MCP authorization。
5. cursor/IO/index/resource bounds。
6. Windows 产品闭环与跨平台修复。
7. 技术债、回归和验收文档。

## 7. 测试阶梯

每次只为当前阶段跑必要范围，阶段末再扩大：

```powershell
# Frontend narrow
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
  src/components/session-import --max-warnings 0

# Rust narrow and domain
cd src-tauri
cargo fmt --all -- --check
cargo test --lib session_migration::selection --offline -- --nocapture
cargo test --lib session_migration::engine --offline -- --nocapture
cargo test --lib session_migration --offline -- --nocapture
cargo clippy --all-targets --offline -- -D warnings
```

Windows 若出现 `0xc0000139 STATUS_ENTRYPOINT_NOT_FOUND`，按 `docs/plans/session-import-p0/START.md` 给本次 test EXE 嵌入 `windows-test-manifest.xml`，然后直接运行并证明进入断言。`--no-run` 只算编译证据。

## 8. Grok 每一步的强制记录格式

```text
STEP: Rn.x
PLAN_STEP: R0 | R1 | ... | R10
STATUS: in-progress | done-synthetic | done-verified | blocked-external
BASELINE: 失败测试、错误、exit code
CHANGED: 精确文件与产品行为
TESTS: 精确命令、exit code、passed/failed/filtered
EVIDENCE: 原始日志、截图、脱敏 trace 的绝对路径
OPEN: 尚未证明的风险
NEXT: 唯一下一门禁
```

判定规则：

- 只有源码或 compile-only：`in-progress`。
- fixture/mock/unit 全绿：最多 `done-synthetic`。
- 当前平台真实 UI/活库/真实 ACP/恢复按矩阵通过：对应子项才是 `done-verified`。
- 只有确实缺少外部机器、账号、授权或路由的精确子项可 `blocked-external`。
- 不得用一组定向测试关闭多个阶段；不得把 `filtered out` 当作通过；不得伪造日志或数字。
- 不得 commit、push、PR、merge 或 release，直到用户审核并明确授权。


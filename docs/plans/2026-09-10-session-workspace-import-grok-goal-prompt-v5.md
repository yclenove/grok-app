# 交给 Grok 的目标模式长任务提示词 V5

将下面代码块完整粘贴给负责 `H:\aicoding\grok-app-session-import` 的 Grok 开发会话。不要同时启动第二个写入者。

```text
继续完成 Grok App 的“Codex、Claude Code、Cursor 会话与工作区导入”长任务。使用目标模式持续执行。不要停在代码扫描、泛泛计划、单个测试或一份乐观报告；严格通过一个门禁后再进入下一个门禁。

工作树：H:\aicoding\grok-app-session-import
分支：feat/session-workspace-import
当前 HEAD：30757366a739ec9aaf0ccc95bbb3efe19a067aa9

这是专用但包含大量未提交 WIP 的工作树。必须保留所有现有修改。禁止 reset、clean、checkout 覆盖、删除未跟踪文件、换分支或从头重写。未经用户明确授权，禁止 commit、push、PR、merge、tag、release。禁止杀死或干扰用户已有 Grok、Codex、Cursor、Node、Cargo 和开发进程。

开始前完整读取：

1. H:\aicoding\grok-app-session-import\AGENTS.md
2. H:\aicoding\grok-app-session-import\CONTRIBUTING.md
3. docs/llm-wiki/i18n.md
4. docs/llm-wiki/settings-ia.md
5. docs/llm-wiki/dialogs.md
6. docs/llm-wiki/session-continuity.md
7. docs/llm-wiki/session-api.md
8. docs/llm-wiki/maintain.md
9. docs/BUILD.md
10. 当前执行书：
   H:\aicoding\grok-app-session-import\docs\plans\2026-09-10-session-workspace-import-r1-review-and-next-v5.md
11. 上游集成逐文件 runbook：
   H:\aicoding\grok-app-session-import\docs\plans\2026-09-10-session-workspace-import-upstream-integration-r1.6.md

V5 覆盖 V4 中已经过期的 R1 状态和测试数字。旧研究/契约仍可作背景，但旧进度、旧 acceptance、旧日志与当前源码/V5冲突时，以当前源码、V5 和当前代码之后生成的可复跑证据为准。

开始时记录且不修改：

- git status --short --branch
- HEAD、origin/main、upstream/main 与 merge base
- worktree 列表
- tracked 修改与 untracked 文件精确数量
- 相关源码和当前测试 EXE 的 mtime/SHA-256
- 当前运行的开发/测试进程

不得读取、复制、输出用户私人会话、token、Cookie、auth.json 或 secrets。真实来源验证只使用用户明确授权的脱敏样本；性能/故障测试使用合成 fixture 和隔离 GROK_APP_HOME。

以下是已复核的当前事实，不得重复做旧工作或伪造不同基线：

- 所有功能仍是未提交 WIP；复核时有 46 个 tracked 修改与 143 个 untracked 文件。
- 本机 upstream/main 为 f013b0b96d4536cb2db0859dee9e89d407b36488（v0.2.34）；功能 HEAD 是 merge base，相对 upstream/main 0 ahead / 57 behind。上游改动 362 个路径，与 WIP 精确同路径重叠 14 个 tracked 文件。远端 tip 可能变化，集成前必须重新 fetch/计算。
- R0.5 已关闭：capability/continuation 基线已进入完整领域回归。
- R1 已按合成证据关闭，不要重写：
  - 前端 15 文件 156 passed / 0 failed；pnpm typecheck、定向 ESLint、pnpm lint 均 exit 0。
  - Rust selection 10/10、jobs 9/9、engine 41/41、job_control 30/30。
  - 完整 session_migration 284 passed / 0 failed / 1611 filtered，412.02s，exit 0。
  - 日志：H:\aicoding\grok-session-import-r1-full.log 与 H:\aicoding\grok-session-import-r1-job-control.log。
  - 测试 EXE SHA-256：F578B44BBB8F975059ABDF2BCE2DDA1EDE8504350002F6561B0764DC2FBED8B2；mtime 2026-09-10T07:48:31.5669880+08:00。
  - cargo fmt、git diff --check、deps:check 均 exit 0。
- R1 现在严格贯穿完整 SelectionSet：live v2 required/deny-unknown/raw-first；CommitItem/DurableInput/Receipt 必填；receipt 全 identity；selection-aware dedup；assets/blobs/selection hashes；selection 在 rename 前写入并 sync；shared evaluator 验证 branch/event/asset/blob digest。
- 前端已删除空 leaf/default selection 提交旁路；reader owner 是 importId+revision+leaf；显式空或用户更改选择后分页不再扩权。
- pnpm audit:prod 当前仍因 @tiptap/core 3.30.4 的 GHSA-j95f-988m-3j2f（high）exit 1；本机 upstream/main 已升级 Tiptap 3.31.3。不要在功能代码里重复造依赖升级。
- 当前 10k job 用例约 374 秒主要来自为 10k 项构造真实 prepared revision/index/lease，只是分页正确性与有限反序列化证据，不是产品性能成绩。
- 尚无当前源码后的真实 Windows Tauri、真实来源活库、真实 ACP 四组合、macOS/Linux 实机证据。

先做一个不改代码的 R1 smoke audit：

1. 确认日志 exit 0、EXE hash/mtime 与源码时序匹配。
2. 静态确认 production 不存在 CommitItem/DurableInput 的 optional/default selection，不存在 leafId??""、selection??defaultSelection，不存在可生产调用的旧 plan helper。
3. 如果源码没有在上述验证之后变化，不要再跑 412 秒全套，也不要重新实现 R1；直接进入 R1.6。
4. 如果源码确实变化，先运行最窄受影响测试，再按影响扩大。每次 Cargo 重新链接测试 EXE 后重新嵌入 src-tauri/windows-test-manifest.xml。

第一阶段：R1.6 授权门

当前没有可恢复 checkpoint。你必须检查用户是否在本任务中明确说过：

“允许在 feat/session-workspace-import 专用工作树中建立仅本地 checkpoint commit，并语义集成最新 upstream/main；不 push、不提 PR、不合并到源项目主干。”

如果没有这句或等价的明确授权：

- 不得 commit、merge、rebase、stash、reset、checkout 或继续 R2。
- 状态写 needs-user-authorization，不写 blocked-external。
- 只向用户提出这一项精确授权请求，然后停止；不要夹带第二个问题。

如果已有明确授权，则持续执行以下全部步骤，不要在中间只交报告：

1. 重新审计 status、secrets、build output 和 untracked 清单。
2. 按可审责任边界建立仅本地 checkpoint commits；不得纳入 token/auth/private history/logs/target/node_modules。
3. fetch upstream，重新计算 merge base、ahead/behind、changed paths 和 overlap。
4. 使用 no-commit 方式做语义集成；禁止对重叠文件整文件选 ours/theirs。
5. 严格依照 R1.6 runbook 处理全部 14 个重叠路径和间接依赖。
6. acp_client、session_manager/events/process/turn/events_bg 必须保留上游“锁内只采集状态，锁外 I/O/持久化/kill”的结构。
7. continuation 顺序：Dispatching 在 prompt RPC 前 durable；Running 紧邻 prompt_for；success/failed/unknown 锁外落盘；UNKNOWN 绝不自动重发。
8. events_bg 的 Error 与 ProcessExited 必须补同一 continuation transition。
9. 保留上游 process tree kill、stream/journal/timeout/freeze 修复、ThemeShellContext、providerRetryStatusStore、memoized session tree 和 search cleanup invalidation。
10. 保留本功能的 imp: 只读隔离、migration commands、import-history MCP、WSL/TCP handling、imported search kind 和 UI 入口。

R1.6 合并后门禁：

- conflict marker search + git diff --check
- cargo fmt --all -- --check
- cargo check/clippy（按 CONTRIBUTING）
- capability、continuation、SelectionSet、jobs/job_control/engine 与完整 session_migration
- 上游 freeze、kill tree、stream、journal、timeout/stall 测试
- session-import 前端 15 文件
- search palette、sidebar、theme、navigation 测试
- pnpm typecheck、pnpm lint、pnpm build:ui
- pnpm deps:check、pnpm audit:prod；Tiptap high 必须关闭

任何门禁失败，先保留原始输出并修根因，不得删测试、弱化断言、catch 后返回空成功或把 failure 写成外部阻塞。R1.6 全绿后直接进入 R2。

第二阶段：R2 previewId keyed 多会话内容状态机

1. 新建 domain hook/reducer `useSessionImportContentStates` 或同等模块；不得向 AppWorkbench 添加 state。
2. 每个 preview 独立保存 candidateId、previewId、snapshotId、leafId、filterKey、leaves、events、nextCursor、Selection、generation、dirty、busy、error。
3. 固定 selected previews 时为每一项建立状态，不只初始化当前打开的详情。
4. A→B→A 恢复各自 branch、选择、页、cursor、dirty、busy/error；source revision 改变才使旧 state stale。
5. 所有 branch/load-more 请求捕获 owner + generation + request sequence；旧成功、失败、finally 一律丢弃。AbortController 不能代替 owner 校验。
6. branch confirm 用应用内 modal，禁止 window.confirm；所有异步路径 try/catch/finally，失败后 busy 必须复位且错误可复制。
7. branch 后 load-more 使用新 leaf/new cursor，禁止旧 peek.loadMore。
8. explicit-empty/dirty selection 跨页、跨会话切换保持，不得重新授权 defaultSelected。
9. 把 SessionImportHost 对应业务块拆到 domain hook/component，保持 App shell + AppWorkbench 总行数不增长。
10. 补 V5 列出的 A/B、迟到响应、branch、双击分页、取消、empty selection、键盘/disabled/error 矩阵。

R2 退出条件：所有多会话/branch/page 正反例绿色；没有裸 .then；没有 exhaustive-deps 抑制；真实 dev UI 至少完成隔离 smoke，但只能标 Windows 当前子项。

第三阶段：R3 durable plan 与完整恢复

1. catalog 存储版本化 canonical plan：SelectionSet、preview/snapshot/file identity、mappings、options、hash、created/expiry。
2. plan 返回前持久化；commit 从 durable plan 加载并重验，不依赖 Runtime.plans。
3. requestId 严格绑定 planId+planHash；plan/selection/mapping/options/lease 任一变化 fail closed。
4. 覆盖 plan 后 Host 重启、commit 前重启、每个 publish phase 中断、rename-before-receipt 与 receipt recovery。
5. current revision 切换只允许完整旧 revision 或完整新 revision。

第四阶段：R4 唯一授权真相与产品断链

1. revision SelectionSet 是唯一授权真相；sidecar/grant 不再维护可漂移的 optional selection/event IDs/hash。
2. handoff、model export、MCP search/read/asset read 全部从 revision selection 进入同一 evaluator。
3. hidden/delete/undo/new revision/tamper 使旧 grant/draft stale 或 revoked。
4. 修全局搜索 imported history 鼠标点击路由，使其与键盘一致。
5. 正确接入 onOpenNativeSession；续聊成功后导航一次且仅一次。
6. 接通 revision/leaf deep link；reader 错误不得显示成空历史。
7. 运行完整 secret matrix：system、visible/opaque reasoning、tool input/result、sibling、subagent、compaction、asset。

第五阶段：R5-R6 cursor、IO/index 与有界读取

1. 所有 cursor 版本化并绑定 owner/snapshot/leaf/filter/offset/expiry，拒绝裸数字/tamper/cross-owner/stale。
2. 清理生产 `.ok()`/default fail-open；损坏 lease/manifest/relations/index/receipt 必须报错。
3. readonly catalog 只读验证 schema，不初始化或迁移来源库。
4. 索引使用同目录 temp+fsync+backup/swap/rollback，覆盖 Windows sharing violation、并发 reader/rebuild 和启动恢复。
5. 删除/私有化生产整份 event/archive 读取；catalog/revisions/refs/issues/job items 真分页。
6. 前端保留 `{items,nextCursor,total}`，支持 load more/虚拟化；不能只显示前 50 条。
7. 合并重复 reader 数据链，并施加条数/字节预算。

第六阶段：R7 性能与故障 harness

1. 仓库内生成合成 10k metadata、100 MiB 单会话、1 GiB 批次、长 tool output、缺附件/WAL。
2. 分别记录 cold/warm wall time、first-visible、峰值 RSS/CPU、磁盘增量、取消确认时间。
3. 隔离 fixture 构造耗时与被测 API 耗时；当前 374 秒测试不得作为 UI 性能数字。
4. 故障注入只使用隔离 GROK_APP_HOME 和自己启动的进程；不得杀用户进程。

第七阶段：R8 Windows Tauri 与真实 ACP

1. 隔离 app home 启动 pnpm dev 正确配置，走来源→扫描→多选→内容→映射→导入→reader→搜索→导出→续聊→撤销。
2. 覆盖 1280×720、窄窗、200%、深浅主题、中文路径、IME、纯键盘。
3. 使用脱敏 trace 区分 slash commands、自身 tools/list、Agent _meta.tools 与 live tool call。
4. 验证 official/custom × shared/independent；remote/official-aux 不误注入。
5. 100 轮 first-send exact-once；UNKNOWN 不自动重发。
6. 完成 workspace mapping 的 create project、explicit target、unassigned 交互。

第八阶段：R9 全仓、i18n、行尾与技术债

1. 按 CONTRIBUTING 跑 deps/audit/typecheck/test/lint/build、fmt/clippy/full cargo test。
2. 完成 15 locale 真实翻译，不把英语占位当完成。
3. 只规范本功能新增的 mixed-EOL locale 文件，禁止全仓机械 renormalize。
4. 行为稳定后拆 engine.rs、jobs.rs、continuation.rs、mcp_stdio.rs、SessionImportHost.tsx；先保持测试保护再拆。
5. 检查 hardcoded UI、secret、license/NOTICE、settings search、setup gate 和 AppWorkbench growth。
6. macOS/Linux 通过真实机器或 CI；cross compile 只写 compile evidence。

第九阶段：R10 验收和小 PR 方案

1. 基于最终同步后的源码逐项重审 53 项 F/W/D/S/R/C/U。
2. 状态只能是：实机通过、合成测试通过、部分、失败、未验证、blocked-external。
3. 每项链接当前源码之后的日志、截图或脱敏 trace。
4. 按依赖顺序设计小 PR；不要实际 push/PR，等待用户审核。

开发纪律：

- 每个小步：失败基线→最小实现→窄测试→扩大回归→退出条件。
- 不得使用 optional、空字符串、default selection、unwrap_or_default、.ok()、catch 后空成功来掩盖状态。
- 不得把 imported transcript 称为原生 resume；它始终是只读历史，续聊是显式 Grok handoff。
- 不得让模型读取 SelectionSet 之外的 system/reasoning/tool input/result/sibling/subagent/asset。
- 不得使用 window.confirm/prompt/alert、原生 select、透明菜单或硬编码 UI 文案。
- 不得往 AppWorkbench 加新业务状态或大功能块。
- compile-only 不算测试；filtered 不算 passed；fixture/mock 不写成实机。
- 本机可完成的 fixture、Windows dev Tauri 和隔离故障测试不得标 blocked-external。
- macOS/Linux 缺机器时只标精确子项，然后继续其余本机工作。
- 每次 Rust 重新链接后重新嵌入 windows-test-manifest.xml；0xc0000139 表示没有进入断言。

继续把新结果追加到：
H:\aicoding\grok-session-import-progress-v4.md

每一步只在真实命令结束后记录：

STEP: Rn.x
STATUS: in-progress | done-synthetic | done-verified | needs-user-authorization | blocked-external
BASELINE: 原始失败和 exit code
CHANGED: 精确文件与行为
TESTS: 精确命令、exit code、passed/failed/filtered、耗时
EVIDENCE: 日志、截图或脱敏 trace 的绝对路径
OPEN: 尚未证明的风险
NEXT: 唯一下一门禁

不要预写测试数字。不要在中途只回复“完成了”“计划已写”“局部测试过了”。除 R1.6 明确授权门或确实缺外部机器/账号外，完成一个阶段后直接继续下一个阶段。最终只输出：阶段账本、最新完整验证、真实产品证据、精确外部阻塞、剩余技术债和建议小 PR 拆分；等待用户审核，不 push、不提 PR。
```

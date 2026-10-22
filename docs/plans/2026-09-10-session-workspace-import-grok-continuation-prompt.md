# 交给 Grok 的续开发提示词

使用方式：等当前正在写入同一工作树的 Grok 步骤稳定或结束后，把下面代码块发到**同一个开发会话**。04:17 复核时仍有三个 `grok.exe`；S85 已落日志，但不要据此并发启动第二个 writer。

```text
继续 Grok App 的“Codex、Claude Code、Cursor 会话与工作区导入”长任务，目标模式持续执行，不要只写计划或只补一两个测试。

工作区：H:\aicoding\grok-app-session-import
分支：feat/session-workspace-import
基线 HEAD：30757366a739ec9aaf0ccc95bbb3efe19a067aa9

这是包含大量未提交 WIP 的专用工作树。现有改动属于要保留的成果。禁止 reset、clean、checkout 覆盖、删除未跟踪目录、重写整个实现或改动无关功能。没有用户明确授权，禁止 commit、push、PR、merge、release。不要停止、杀死或干扰其他 Grok/Codex/开发服务进程。

第一件事不是立刻写代码，而是安全接管：

1. 检查 H:\aicoding\grok-session-import-progress.md 尾部、git status、相关源码时间戳和正在运行的 Grok/cargo/rustc/node 进程。
2. 对相关源码和进度日志做两次时间戳快照；如果另一个 writer 仍在修改同一批文件，等待它完成当前稳定步骤并落进度，不要并发编辑。
3. 完整阅读并遵守：
   - AGENTS.md
   - CONTRIBUTING.md
   - docs/BUILD.md
   - docs/llm-wiki/i18n.md
   - docs/llm-wiki/dialogs.md
   - docs/llm-wiki/session-continuity.md
   - docs/llm-wiki/session-api.md
   - docs/plans/2026-09-08-session-workspace-import.md
   - docs/plans/2026-09-08-session-workspace-import-contracts.md
   - docs/plans/2026-09-08-session-workspace-import-acceptance.md
   - docs/plans/2026-09-09-session-workspace-import-execution-runbook.md
   - docs/plans/2026-09-09-session-workspace-import-current-review.md
   - docs/plans/2026-09-09-session-workspace-import-next-phase.md
   - docs/plans/2026-09-10-session-workspace-import-live-audit-and-next.md
   - docs/plans/session-import-p0/acceptance-results.md
4. 文档和进度声明与当前源码/可复跑证据冲突时，以源码和新证据为准，修正错误结论后继续。

当前已确认：

- 新计划 NP-S59（Selection v2 schema/兼容）和 NP-S60（唯一权限判定器）已有 done-synthetic 证据。
- 进度日志 S65–S73 把 NP-S61–NP-S69 批量标成 done-synthetic，但主要证据只是同一组 12 个 Rust 定向测试、12 个前端测试和 rustfmt。该证据不足以覆盖九个阶段，状态全部回退为“实现已出现、待返工验收”。
- 详细首轮日志曾真实出现两个失败：`two_previews_keep_independent...` 断言失败，以及 Windows 索引发布 `os error 32`。后续样例重跑为绿，只能关闭这两个样例，不能自动关闭阶段。
- S85 已把手写 `S65-np61-1/2.log` 标为 superseded，并补了 `S61-run1/2.log` 到 `S69-run1/2.log` 的实际 cargo 输出。保留这项纠正；但这些日志每段只跑 1～4 个测试并显示 1887 filtered out，仍不能替代各阶段完整门禁。
- S74–S84 把 NP-S70–NP-S80 批量标为 blocked-external，并写“S81–S82 不在本目标”。这个判断不成立：本机可生成 100 MiB/1 GiB 数据、采集 RSS、注入隔离故障、跑 Windows Tauri/全仓回归；NP-S81/NP-S82 本来就在原计划中。
- 04:11 的当前源码仍明确存在：重复 PreviewSelection DTO、可选/默认 leaf、非 exact-cover 校验、内存-only plan、不完整 selection revision 持久化、多会话 UI selection 丢失风险、branch/load-more 状态分叉、多套 MCP 授权真相和先删 live index 再 rename 的发布窗口。
- 04:20 独立当前树验证：选择相关 Vitest 4 files / 10 tests 通过、定向 ESLint 通过；但 TypeScript 构建 exit 1（11 个错误），`cargo fmt --all -- --check` exit 1。先修这些真实门槛，不能引用旧 typecheck/rustfmt 绿灯。
- 历史 1846 Rust library、239 migration、7115 Vitest 等绿灯早于最新写入，必须对当前树重新验证。

从 docs/plans/2026-09-10-session-workspace-import-live-audit-and-next.md 的 G0 开始，找到第一个未满足门禁，按 G0→G7 顺序持续完成。不要因为源码里已有同名 struct/button/test 就跳步。

严格按下面的返工批次执行；一个批次没有证据就不得进入下一个，也不得把多个批次共用一份汇总日志：

R0 当前树冻结与失败基线
- 等现有 writer 停稳，记录 HEAD、status、相关文件 hash/mtime、运行进程和最新日志。
- 先复现 04:20 基线：`SessionImportHost.tsx` 的未用 `selectionFromState`；Host 六处使用但 `useCandidatePeek` 未返回的 `previewId`；`sessionMigration.ts` 缺失 `AllowedSelection` 类型导入；`job_control_tests.rs` 格式差异。用最小改动修复，不改行为来掩盖问题。
- 再跑 selection/content hooks、TypeScript、session_migration 最窄测试，保存原始 stdout/stderr 与 exit code。
- 建“需求 -> 生产入口 -> 测试名 -> 证据文件”矩阵。没有映射的测试不计入该需求。

R1 SelectionSet v2 唯一化
- 删除 engine/commands/frontend 的平行 DTO；生产 IPC 直接使用一份 versioned SelectionSet。
- `leafId` 在 TS/Rust/IPC/job 中全部 required，禁止 serde default/空字符串兼容新请求。
- exact-cover 使用集合等价：无重复、无遗漏、无未知 preview；测试 A,A 对 A,B、空 leaf、同 event ID 跨 preview。
- 每项针对固定 preview snapshot 验证 leaf、lineage、event、pin、subagent、field 和 asset membership，不能只 validate_shape。

R2 多会话 UI 与 branch/page 状态机
- 状态以 previewId 为键保存 leaf、selection、pages、nextCursor、generation、dirty/valid/busy/error。
- 创建每个 selected candidate 的固定 preview 时同时保存默认 selection；未打开详情也必须有 selection。
- A→B→A 恢复各自选择；取消再选策略明确；不能在“下一步”只保存当前详情。
- branch 请求和 load-more 都使用当前 previewId+snapshot+leaf+filter+cursor+generation；迟到/失败保留旧状态。
- 禁止裸 `.then()` 无 catch/busy；应用内确认、键盘、IME、窄窗、200% 和 15 locale 一起关门。

R3 plan/job/revision 持久化
- plan 不能只存在 Runtime.plans；用 catalog/受控文件持久化 canonical SelectionSet、snapshot、mapping、options 和 hash，重启后 commit 可恢复且重新校验。
- CommitItem、job input、每阶段 receipt、revision manifest/selection 文件必须表达同一个 leaf+selection，并纳入 revision hash/完整性校验。
- selection.json 必须在 staging 发布前写入；去重路径也必须校验/返回既有不可变 selection 语义。
- 测试 Host 重启、重复 requestId、selection/mapping/source 变化、preview 过期、rename 后崩溃和去重。

R4 reader/continuation/MCP 单一授权真相
- sidecar/grant 只保留一个完整 v2 selection；兼容字段只能由它派生并在读取时严格一致校验，不能独立授权。
- reader 可看完整归档；export-to-model、handoff、MCP search/read/asset read 必须全部走同一 evaluator。
- asset read 只接收 assetId，并绑定 session/import/revision/leaf/hash/catalog visibility/stored blob；路径和未选资产拒绝。
- shipped-path secret fixture 覆盖 system、reasoning、tool input/result、兄弟 branch、subagent、compaction、图片；每个模型出口分别断言 deny。

R5 cursor、IO 与安全发布
- preview cursor 使用 versioned envelope，绑定 previewId、snapshot digest/lease、leaf、filter signature、offset；拒绝裸数字、tamper、cross-preview、cross-leaf、expired。
- 消除生产路径 `relations_from_dir/indexed_events/read(...).unwrap_or_default()` 的空成功；损坏 manifest/index/IO 均 fail closed。
- event index 使用同目录安全 swap/backup/rollback。不得先删除 live 再 rename；任意失败点必须保留完整旧索引或完整新索引。
- 测并发 reader+rebuild、Windows sharing violation、取消、崩溃、旧 schema、截断库及重启恢复。

R6 有界读取与存储层分页
- 证明 `load_events`/`load_archive_events` 不可从生产链到达；删除或改为有界 iterator。relation 与巨大 ID 集也必须有资源上限。
- entries、job items、revisions、continuation refs、issues/diagnostics 全部存储层分页，cursor 稳定且绑定查询条件。
- 前端页合并去重并拒绝迟到页，不能先全读再 skip/take。

R7 本机可复跑性能和故障 harness
- 在临时目录生成 10k metadata、100 MiB 单会话、1 GiB 批次、超长 tool output、缺附件和合成 WAL；不得要求用户提供私人历史。
- 用可提交/可复跑脚本记录 cold/warm、wall time、峰值 RSS/CPU、磁盘增量和取消确认；模型耗时单列。
- 在隔离 GROK_APP_HOME/子进程注入 kill、publish 中断、只读、空间不足。绝不杀当前用户的 Grok/Codex/开发进程。
- 超标必须定位并修复；不能只把它写进 blocked 文件。

R8 Windows 真实闭环与 ACP
- 在用户授权启动开发版后完成 Windows Tauri 点击矩阵；没有尝试前不能写 external。
- 用已安装 Grok Build 做脱敏 trace，区分 slash commands、MCP ready、自身 tools/list、Agent advertised `_meta.tools` 和 live call。
- official/custom × shared/independent、普通/remote/official-aux、A→B→A、首次发送 once-only、发送后 UNKNOWN 分别验收。
- 若账号/路由确实缺失，记录已执行命令、原始错误和最小外部条件，只阻塞对应子项。

R9 当前平台回归与技术债
- 先完成 Windows 的 deps/typecheck/vitest/lint/build/rustfmt/clippy/cargo test；macOS/Linux 不得阻塞本机回归。
- 删除模块级 allow、重复 DTO、生产 stub、无界旧 helper、静默 IO、未用命令；拆分超大 Host 但不改变已稳定行为。
- `git diff --check`、LF、i18n parity、依赖/许可/NOTICE 和秘密扫描通过。

R10 最终 53 项重审
- NP-S81 技术债和 NP-S82 最终验收必须执行，不能写“不在本目标”。
- 每项按实机/合成/部分/失败/未验证/blocked-external 单独评级，并链接对应最新证据。
- 只有本机可执行项全部通过、P0/P1 无开放问题后，才报告“完整首版等待用户审核”；macOS/Linux 缺机器时保留精确的跨机 handoff。

每个门禁固定执行：

1. 从验收行为反推失败测试，先证明测试能抓住问题。
2. 只做该门禁所需生产改动，复用领域模块；不要继续把状态堆进 App.tsx/AppWorkbench.tsx，也不要让 SessionImportHost.tsx 无限膨胀。
3. 先跑最窄测试，再跑领域回归、typecheck、lint、rustfmt/clippy；Windows 0xc0000139 必须嵌入 Common Controls v6 manifest 后直接运行 test EXE，确认真正进入断言。
4. 审核安全、分页、字节上限、取消、重启、迟到响应、i18n、键盘和 UI 错误态。
5. 把精确事实追加到 H:\aicoding\grok-session-import-progress.md；日志写入 H:\aicoding\session-import-verification，不伪造数字。
6. 当前门禁满足后立即继续下一门禁，不要停下来问“是否继续”；只有需要新增权限、真实外部账号/机器或会破坏用户数据时才停。

现在最优先逐项证明并修复这些风险；如果当前代码已修，必须用相应红绿测试关闭：

A. 多会话选择
- 每个 selected preview 必须恰好一份 required leafId + SelectionSet v2；无重复、无遗漏、无未知 preview。
- A/B 会话来回查看后各自 selection 不丢；未打开详情的会话也拥有固定 snapshot 的默认 selection。
- 不得把旧 peek selection 未验证地套到新 preview/snapshot。
- plan 必须针对该 snapshot 校验 leaf、lineage、events、pinned events、subagent 和 assets，不能只 validate_shape。

B. branch 与分页
- 切 branch 后 load-more 必须使用当前 previewId + leaf + cursor + generation，不能继续走旧 peek cursor。
- branch 迟到/失败不覆盖新状态、不清空旧 selection；请求有 busy/error/cancel 路径。
- cursor 版本化并绑定 preview snapshot、leaf、filter、offset；cross-preview/cross-leaf/tamper/expired 全拒绝。
- manifest/index/IO 失败不能 unwrap_or_default 成空集合，unknown leaf 不能先写 runtime。

C. 不可变持久化
- SelectionSet 是 plan 唯一真相；leaf/field/asset/subagent/selection version/snapshot/mapping 都进入 canonical plan hash。
- job input、receipt、revision manifest 持久化完整 selection 和 hash；重启后不依赖 Runtime.plans 或 UI 默认值。
- selection 改变使旧 plan/draft/grant PLAN_STALE 或 revoked。

D. 模型权限闭环
- sidecar/grant 只保留一份完整 v2 selection 真相；旧 allowedEventIds 只能作为受校验兼容字段，不能另行授权。
- handoff、export-to-model、MCP search/read/asset read 全部走唯一 evaluator。
- asset read 只收 asset ID，核对 selected allowlist、revision manifest、stored blob、session/revision/leaf/hash/撤销状态。
- 用带 secret marker 的 system/reasoning/tool input/result/兄弟分支/subagent/compaction/image fixture 做端到端 deny 证明；任何泄露都阻断后续。

E. ACP 真实协议
- availableCommands 只当 slash commands；Agent 工具从 session update 的 _meta.tools 解析。
- MCP ready、自身 tools/list、Agent advertised、live call 分开记录；full mode 只接受 advertised/live verified。
- official/custom × shared/independent 做真实 search/read/deny；普通、remote、official-aux 路径不得误注入。
- 首次 handoff 一次且仅一次进入 session/prompt；发送后 UNKNOWN 不自动重发。

F. 大历史与错误语义
- 删除 continuation::load_events 等生产链路的整份 Vec 读取；archive offset/ordinal index、search rebuild、job/catalog/revision/ref/diagnostic 都流式或存储层分页。
- 修掉 current_file_revision 的 read(...).unwrap_or_default() 以及其他把 IO/损坏伪装为空成功的生产路径。
- open_catalog_readonly 只验证 schema，不执行 CREATE/ALTER/migration。
- 运行 10k、100 MiB、1 GiB、长工具输出、缺附件、活 WAL 基准并记录峰值 RSS/磁盘/取消响应；模型响应时间单列。

G. UI 与可维护性
- fieldAvailability 控制开关；message text、tool fields、reasoning、subagent、assets 的依赖和 disabled 原因清楚。
- “将归档”“当前分支”“可交给 Grok”计数不混淆；relation/asset summary 真正可见。
- 使用现有 Select/GlassModal/控件样式；无 native select、window confirm/prompt/alert、透明菜单或硬编码文案；15 locale parity。
- 验证 1280×720、窄窗、200%、深浅主题、中文路径、IME、纯键盘。
- 行为稳定后拆分超大的 SessionImportHost，而不是边开发边做无关大重构。

证据状态必须诚实：

STEP: Sxx
PLAN_STEP: NP-Sxx
STATUS: in-progress | done-synthetic | done-verified | blocked-external
CHANGED: 精确文件与产品行为
TESTS: 精确命令、退出码、通过/失败数
EVIDENCE: 日志/截图/脱敏 trace 的绝对路径
OPEN: 尚未证明的事项
NEXT: 唯一下一门禁

fixture/mock/unit/compile-only 最多是 done-synthetic。真实 Tauri、真实 ACP、活数据库、故障恢复和目标平台必须分别实测。没有 macOS/Linux 或真实账号时写 blocked-external 和可复制验收命令，但继续当前 Windows 环境内所有顺序允许的工作，不能缩小最终目标。

禁止再用“同一组 12 个测试”批量关闭多个计划步骤，也禁止用一个 `S70-S80-blocked-external.txt` 代替逐项尝试。每个 R 批次至少包含：该批次失败基线、生产改动、定向测试、领域回归、原始日志和仍开放风险。

最终只有在 G0–G7、全仓回归、技术债收口和 53 项重审完成后，才可报告“完整首版等待用户审核”。在此之前持续执行并只报告事实，不创建提交或 PR。
```

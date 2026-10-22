# 交给 Grok 的目标模式续开发提示词 V2

把下面代码块完整粘贴到当前负责 `H:\aicoding\grok-app-session-import` 的 Grok 开发会话。若该会话仍在跑测试，先等它自然结束当前命令；不要再开第二个 writer。

```text
继续完成 Grok App 的“Codex、Claude Code、Cursor 会话与工作区导入”长任务。使用目标模式持续执行，严格按门禁逐步开发和验证，不要只写计划、不要只修一个测试就停，也不要批量自报完成。

工作树：H:\aicoding\grok-app-session-import
分支：feat/session-workspace-import
基线 HEAD：30757366a739ec9aaf0ccc95bbb3efe19a067aa9

这是包含大量未提交 WIP 的专用工作树。所有现有改动都要保留。禁止 reset、clean、checkout 覆盖、删除未跟踪目录、重写整个实现、移动到别的分支或改动无关功能。未经用户明确授权，禁止 commit、push、PR、merge、release。禁止杀死或干扰用户现有 Grok/Codex/开发进程；需要故障注入时只能使用隔离 GROK_APP_HOME 和你自己启动的子进程。

第一步先安全接管：

1. 检查 git status、HEAD、H:\aicoding\grok-session-import-progress.md 尾部、相关源码 mtime/hash，以及 grok/cargo/rustc/node 进程。
2. 若另一 writer 仍修改同一工作树，等待其完成当前稳定步骤并落日志，不并发编辑。
3. 完整阅读并遵守：
   - AGENTS.md
   - CONTRIBUTING.md
   - docs/BUILD.md
   - docs/llm-wiki/i18n.md
   - docs/llm-wiki/dialogs.md
   - docs/llm-wiki/settings-ia.md
   - docs/llm-wiki/session-continuity.md
   - docs/llm-wiki/session-api.md
   - docs/plans/2026-09-08-session-workspace-import.md
   - docs/plans/2026-09-08-session-workspace-import-contracts.md
   - docs/plans/2026-09-08-session-workspace-import-acceptance.md
   - docs/plans/2026-09-09-session-workspace-import-execution-runbook.md
   - docs/plans/2026-09-09-session-workspace-import-next-phase.md
   - docs/plans/2026-09-10-session-workspace-import-grok-audit-v2.md
4. 文档、自报进度与当前源码/新测试冲突时，以当前源码和可复跑证据为准，并在进度日志纠正旧结论。

当前事实基线（开始时必须重新验证，不能盲信）：

- TypeScript build 通过。
- selection/content 定向 Vitest：4 files / 10 tests 通过。
- session-import 定向 ESLint 通过。
- cargo fmt --check 通过。
- session_migration::selection：9 passed / 0 failed。
- session_migration::engine：31 passed / 10 failed；真正独立根因是两个，其他 8 个为 APP_HOME_ENV_LOCK 被 panic 污染后的 PoisonError 级联。
- 根因 1：restart_keeps_unexpired_preview_without_ram -> HISTORY_ACCESS_UNAVAILABLE: preview leaf is required。
- 根因 2：changing_selection_invalidates_plan_hash -> selection references an unknown event or leaf。严格校验方向正确；旧测试使用不存在于 snapshot 的手写 event id。
- 当前 SelectionSet v2 已进入 TS/Rust IPC 和 PlanState，leafId required，exact-cover 初步存在；但 CommitItem/DurableInput 仍只保存 Option<AllowedSelection>，Receipt 只存 selection_hash，selection.json 在 publish 后写且不进入 revision_hashes，plan 仍只在 Runtime.plans 内存。
- 多会话前端仍是单 content state；branch confirm 是裸 .then()；branch 后 load-more 仍走 peek 的旧 leaf/cursor；commit 对缺 state 仍填空 leaf/defaultSelection。
- 当前工作树没有提交；旧 1846 Rust/7115 Vitest 等全仓绿灯早于最新改动，只能当历史证据。

唯一执行顺序：R0 -> R1 -> R2 -> R3 -> R4 -> R5 -> R6 -> R7 -> R8 -> R9 -> R10。一个门禁未满足不得进入下一个。详细行为和退出条件以 docs/plans/2026-09-10-session-workspace-import-grok-audit-v2.md 为准，下面是执行指令。

R0：恢复稳定领域基线

1. 先独立复跑两个根因测试，保存 stdout/stderr/exit code。
2. 修 restart preview：保证 prepared snapshot/manifest/index 在 Runtime 清空后能恢复确定、合法、非空 leaf。禁止把 leafId 改回 optional、serde default 或空字符串；禁止用 unwrap_or_default 掩盖损坏。
3. 修 plan-hash 测试：从 preview summary/index 读取真实 leaf/event IDs，构造两个都合法但语义不同的 selection；未知 event 另写必须拒绝的测试。
4. 分别重跑两个测试；再跑 cargo test --lib session_migration::engine --offline -- --nocapture。必须 0 failed，不能只跑命名样例。
5. 再跑 selection 9 tests、前端 10 tests、typecheck、定向 ESLint、rustfmt。
6. 若共享锁因 panic poison，先修首发 panic并隔离环境清理；不要给级联用例逐个吞 PoisonError。

R1：SelectionSet v2 唯一真相

1. 新生产 IPC 只接受 SelectionSet v2；旧 plan/plan_with_selection(s) 只允许作为显式兼容迁移或测试 helper，不得成为新请求默认路径。
2. CommitItem、DurableInput、job input 保存完整 PreviewSelection（previewId、leafId、AllowedSelection）或受 hash 保护且可解析的不可变 reference；不能只存 AllowedSelection。
3. exact-cover 必测：[A,B]+[A,A]、缺 B、未知 C、空 leaf、未知 leaf、未知/跨 preview event、未知/unstored/未选事件所属 asset。
4. entry 顺序不改 hash；version/leaf/event/field/asset/subagent/snapshot/mapping/options 任一语义变化改 plan hash。
5. 前端 plan 前做 preview map 与 selection map 集合等价验证；缺项显示 i18n error，禁止 `?? ""` 和 `?? defaultSelection()`。

R2：多会话 UI 状态机

1. 新建独立 hook/reducer，以 previewId 为键保存 candidateId、snapshot、leaf、selection、pages、nextCursor、generation、dirty、valid、busy、error。
2. 固定 selected previews 时同时建立所有默认 state；未打开详情的会话也有 selection。
3. A->B->A 恢复各自 leaf、勾选、页和错误态；取消再选的策略写成测试。
4. branch 和 load-more 统一从当前 keyed state 发 previewId+snapshot+leaf+filter+cursor+generation；branch 后不得调用旧 peek.loadMore。
5. 每条异步路径有 catch/finally、busy/error；迟到/失败保持旧 state，不覆盖别 preview/leaf。
6. 删除相关 exhaustive-deps 抑制，拆 hook/子组件，不把状态继续塞进 AppWorkbench 或无限膨胀 SessionImportHost。

R3：不可变持久化

1. 在 catalog 或受控文件中持久化 canonical plan：SelectionSet、snapshot digests、mapping、options、hash、expiry。
2. commit 从磁盘恢复并重新校验，Runtime.plans 清空后仍可继续合法 commit。
3. selection.json 必须在 staging publish 前写入，并和 manifest/asset metadata 一起纳入 revision hashes/完整性保护。
4. receipt/job/revision 保存同一个 selection reference/hash；恢复不能从 UI/default 重建。
5. 去重必须比较 selection 语义；不同 selection 不能错误复用旧 revision。
6. 测 Host restart、重复 requestId、preview/source/mapping/selection/options 变化、rename 后崩溃、receipt recovery、篡改 selection。

R4：模型授权单一闭环

1. sidecar/grant 只保留一个完整 v2 selection；旧字段只能派生并严格一致校验。
2. handoff、export-to-model、MCP search/read/asset read 全走同一 evaluator。
3. asset read 只接受 assetId，并绑定 session/import/revision/leaf/hash/catalog visibility/stored blob。
4. secret fixture 覆盖 system、reasoning、tool input/result、兄弟 branch、subagent、compaction、图片；每个模型出口都做 deny 测试。
5. selection/revision 撤销、隐藏、刷新或篡改后，旧 draft/grant 必须 stale/revoked。

R5：cursor、IO、索引安全发布

1. cursor 版本化，绑定 owner preview/import、snapshot/index revision、leaf、filter、offset；拒绝裸数字/tamper/cross-owner/cross-leaf/cross-filter/expired。
2. 审计生产 unwrap_or_default。current_file_revision、Cursor DB source hash、manifest/index/relations 读取错误必须 fail closed，不得伪装空成功。
3. open_catalog_readonly 只能验证现有 schema，禁止 CREATE/ALTER/migration。
4. 索引用同目录 temp+backup/swap/rollback；任一失败点保留完整旧 live 或完整新 live，不得先删 live。
5. 测 concurrent reader+rebuild、Windows sharing violation、rename 失败、进程中断、旧/截断 schema、取消和重启恢复。

R6：有界读取和存储层分页

1. 删除/私有化生产可达的 load_events/load_archive_events 等整份 Vec；用 iterator/indexed IDs/page。
2. entries/job items/revisions/continuation refs/issues/diagnostics 全部在存储层分页；前端不得全取再切页。
3. relation、selection IDs、event body、tool output 设数量/字节预算；超限明确 partial/truncated。
4. 页合并按 owner/generation 去重并拒绝迟到页。

R7：本机性能/故障 harness

1. 在临时目录生成 10k metadata、100 MiB 单会话、1 GiB 批次、长 tool output、缺附件、合成 WAL；不使用私人历史。
2. 记录 cold/warm、wall time、first visible、峰值 RSS/CPU、磁盘增量、取消确认；模型耗时单列。
3. 用隔离 GROK_APP_HOME/你自己启动的子进程注入 Host kill、publish 中断、只读和空间不足。
4. 超标要定位并修，不得写成 external 后跳过。

R8：Windows Tauri + 真实 ACP

1. 用户已要求最终真实产品闭环，但启动/操作开发版前仍按当前会话授权边界执行；不要冒充已点击。
2. 验证来源->扫描->多选->内容选择->映射->导入->reader->搜索->导出->续聊->撤销。
3. 覆盖 1280x720、窄窗、200%、深浅主题、中文路径、IME、纯键盘。
4. 脱敏记录 slash commands、MCP ready、自身 tools/list、Agent _meta.tools、live tool call，四者不可混称。
5. 验证 official/custom x shared/independent；普通/remote/official-aux 不误注入。
6. 100轮前置约束一次且仅一次进入真实 session/prompt；发送后 UNKNOWN 不自动重发。

R9：当前平台全仓回归和技术债

1. 按 CONTRIBUTING.md 跑 pnpm deps:check、audit:prod、typecheck、test、lint、build:ui，以及 cargo fmt/clippy/test。
2. 修重复 DTO、生产 stub、模块 allow、静默 IO、旧 helper、失效命令、硬编码 UI；15 locale parity。
3. 行为稳定后拆大文件：SessionImportHost.tsx、engine.rs、continuation.rs、jobs.rs；不做无关重写。
4. git diff --check、LF、秘密扫描、许可/NOTICE 收口。
5. macOS/Linux 单独实机/CI；cross compile 不得写 done-verified。

R10：53 项重审和用户审核包

1. 逐项重审 F01-F12、W01-W07、D01-D05、S01-S04、R01-R07、C01-C11、U01-U07。
2. 每项只允许：实机通过、合成测试通过、部分、失败、未验证、blocked-external，并附最新证据。
3. 本机可执行项全部完成，P0/P1 无开放问题后，才报告“完整首版等待用户审核”。
4. 给出小 PR 拆分草案，但不要 commit/push/PR；等待用户审核和明确授权。

每个微步骤必须先写/确认能失败的测试，再改生产代码；先最窄测试，再领域测试，再静态门禁。Windows 测试若 0xc0000139，按 docs/plans/session-import-p0/START.md 嵌入 windows-test-manifest.xml 后直接运行 test EXE，确认进入断言。--no-run 只算编译。

每一步把事实追加到 H:\aicoding\grok-session-import-progress.md，原始日志写 H:\aicoding\session-import-verification。固定格式：

STEP: Rn.x
PLAN_STEP: R0 | ... | R10
STATUS: in-progress | done-synthetic | done-verified | blocked-external
BASELINE: 失败测试/错误/exit code
CHANGED: 精确文件和产品行为
TESTS: 精确命令、exit code、passed/failed/filtered
EVIDENCE: 原始日志/截图/脱敏 trace 绝对路径
OPEN: 尚未证明的风险
NEXT: 唯一下一门禁

只有源码/compile-only 是 in-progress；fixture/mock/unit 最多 done-synthetic；真实 Tauri/活库/真实 ACP/恢复才可对相应子项 done-verified。只有确实缺少外部机器、账号、路由或用户授权的精确子项可 blocked-external，并继续所有不依赖它的工作。

禁止再用同一组定向测试批量关闭多个阶段；禁止把 filtered out 当通过；禁止伪造日志；禁止因为完成时间长而缩小 Codex、Claude Code、Cursor 或 Windows/macOS/Linux 的最终目标。除非需要新增权限、不可逆数据操作或真实外部条件，否则不要停下来问是否继续，按 R0 到 R10 持续推进。
```


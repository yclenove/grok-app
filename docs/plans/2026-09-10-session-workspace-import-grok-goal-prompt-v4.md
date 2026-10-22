# 交给 Grok 的目标模式长任务提示词 V4

将下面代码块完整粘贴给当前负责 `H:\aicoding\grok-app-session-import` 的 Grok 开发会话。不要同时启动第二个写入者。

```text
继续完成 Grok App 的“Codex、Claude Code、Cursor 会话与工作区导入”长任务。使用目标模式持续执行，严格按 R1.0→R1.5→R1.6→R2→…→R10 的门禁逐步开发和验证。不要停在计划、代码扫描、局部测试或一份报告；完成一个门禁后直接进入下一门禁。R1.6 是唯一需要用户授权的源码集成边界，规则见下文。

工作树：H:\aicoding\grok-app-session-import
分支：feat/session-workspace-import
基线 HEAD：30757366a739ec9aaf0ccc95bbb3efe19a067aa9

这是包含大量未提交 WIP 的专用工作树。必须保留全部现有修改。禁止 reset、clean、checkout 覆盖、删除未跟踪目录、换分支、重写整个功能或改无关模块。未经用户明确授权，禁止 commit、push、PR、merge、tag、release。禁止杀死或干扰用户现有 Grok、Codex、Cursor、Node、Cargo 或开发进程；故障注入只能使用隔离 GROK_APP_HOME 和你自己启动的子进程。

开始前完整读取：

1. H:\aicoding\grok-app-session-import\AGENTS.md
2. H:\aicoding\grok-app-session-import\CONTRIBUTING.md
3. 与本功能相关的 docs/llm-wiki/i18n.md、settings-ia.md、dialogs.md、session-continuity.md、session-api.md、maintain.md、docs/BUILD.md
4. 当前唯一执行书：
   H:\aicoding\grok-app-session-import\docs\plans\2026-09-10-session-workspace-import-grok-audit-and-next-v4.md

V4 覆盖 V3 里已经过期的状态和测试数字。旧设计可作背景，但旧自报进度、旧 acceptance、旧日志与当前源码/V4冲突时，以当前源码、V4 和可复跑证据为准。

开始时先记录 git status --short --branch、HEAD、相关源码 mtime/hash和当前开发进程。若另一个 writer 正在修改同一文件，等待其自然结束，不得覆盖。不要读取、复制、输出用户私人会话、token、Cookie、auth.json 或 secrets；真实来源验证只使用用户明确授权的脱敏样本，性能和故障测试使用合成 fixture。

当前已复核事实，禁止重复做旧工作或伪造不同基线：

- HEAD 未变化；所有会话导入代码仍是未提交 WIP。
- 相对本机 origin/main 为 0 ahead / 17 behind，相对真正源项目本机 upstream/main 为 0 ahead / 57 behind；本轮未 fetch。upstream 已改 362 个文件，其中 14 个与当前 WIP 重叠。禁止在巨大未提交 WIP 上直接 rebase/merge/stash。
- 审计开始时约 46 个 tracked 修改文件、139 个 untracked 文件；加入 V4 两份文档后约 141 个 untracked 文件。Rust migration 34 文件/约24k行，前端相关约5.4k行。
- R0.5 已关闭。最新完整 session_migration：278 passed / 0 failed / 1611 filtered / 349.06s / exit 0。
- capability 12/12、continuation 29/29、selection 9/9；fmt、typecheck、定向 ESLint、全仓 lint、deps:check 均 exit 0。
- 前端相关 13 文件套件当前 142 passed / 3 failed；三个失败均在 useImportedReader.test.tsx，旧断言未包含第五个 leaf 参数。先核对正式 leaf/revision/owner 契约，再修测试并补反例。
- pnpm audit:prod 当前因 @tiptap/core 3.30.4 的 GHSA-j95f-988m-3j2f（high）exit 1；本机 upstream/main 已升级到 Tiptap 3.31.3，最终 fetch 后复核，不得重复做上游已有的依赖升级。
- R1 只贯穿到 Tauri IPC 和 PlanState。commit 仍丢 PreviewSelection 外壳，CommitItem/DurableInput 仍是 Option<AllowedSelection>，Receipt hash 可选，dedup 不含 selection，selection.json 在 rename 后写且不进完整 hash。
- plan_with_selection_set 当前先 canonicalize 再 validate，重复 event/pinned/asset ID 会被静默 dedup。
- live v2 AllowedSelection/SelectionFields serde 仍会为缺字段填默认值；新 v2 wire/durable 必须严格，v1 兼容只能走显式旧数据迁移。
- 前端 commit 仍有 leafId ?? "" 和 selection ?? defaultSelection()。
- SessionImportHost 仍是单 peek/单 content；branch confirm 裸 .then；branch 后 load-more 仍走旧 peek；reader/revision/leaf 未在 UI 贯穿。
- 搜索鼠标点击 imported history、续聊后导航、错误可见性、reader owner、catalog 分页和重复 reader 状态均有明确缺口，详见 V4。
- 14 个非英文 session-import locale 文件是 mixed EOL；不得全仓机械 renormalize。

唯一执行顺序如下。任一阶段退出条件未满足，不得宣称该阶段完成、跳过失败或开始拆 PR。

第一阶段：R1.0 先恢复前端相关绿色基线

1. 复跑 V4 中的 13 文件 Vitest 命令，保存原始 142/3 基线。
2. 确认 events API 的 leaf 是正式 owner/scope 参数；不要为了旧断言删除生产参数。
3. 更新三个过期断言，并补切 leaf、切 revision、迟到旧 owner 不覆盖当前 reader 的反例。
4. 套件必须 0 failed。不得删测试、弱化断言或 catch 后返回空成功。

第二阶段：R1.1 严格 SelectionSet v2

1. 先写失败测试，再改生产代码。
2. 新 v2 IPC/durable 的 version、entries、previewId、leafId、AllowedSelection 全字段必须 required；同版本未知字段应拒绝。
3. v1/旧草稿只允许显式 parse_stored_selection(Value) 迁移；不得让 live v2 serde default。
4. 严格顺序：raw decode → raw shape/duplicate validation → semantic validation → canonicalize → hash。禁止先 canonicalize 再验证。
5. 将 engine 当前 selection 语义检查下沉为共享 validator，供 plan、durable reload、published verify 共用。
6. validator 必须验证 preview/candidate/lease/snapshot identity、leaf、event、pinned、asset、subagent、不可模型可见事件和跨 branch 越权。
7. 前端新增纯函数 validatePlanSelectionBoundary，运行时检查 selected/preview/selection 三个 key 集合、版本、唯一性和非空 ID。

第三阶段：R1.2 原子贯穿 commit 和 durable job

1. CommitItem.selection_set: SelectionSet 必填且 exactly one。
2. DurableInput.selection_set 与 selection_hash 必填；删除 serde default 和 production Option 旁路。
3. engine::commit 传完整 PreviewSelection，使用 SelectionSet::single(entry.clone())；不得只传 entry.selection。
4. commit_item_from_preview/parts 只接完整单项 SelectionSet。
5. durable 写入前和重载后都重新验证 selection hash、strict lease identity、prepared hashes 和完整语义。
6. 删除/限制 production 旧 plan helpers。
7. 修改测试构造时必须用真实 prepared leaf/events/assets 生成合法 selection；禁止空 selection/default 只为编译。

第四阶段：R1.3 原子 publish、receipt 和 dedup

1. Receipt 必填保存完整单项 SelectionSet、selectionHash、optionsHash、snapshotHash 和 preview/candidate identity。
2. prior receipt 同时核对 snapshot、selection、options、candidate、preview、provider/native identity。
3. revision semantic key 至少包含 snapshotHash + optionsHash + selectionHash + normalizerVersion；不同 selection 不得 dedup。
4. 拆 prepared_revision_hashes 与 published_revision_hashes：前者覆盖 manifest/events/assets/blobs，后者再覆盖 selection.json。
5. 在 staging 内严格序列化完整 SelectionSet、写入、flush/fsync、计算 published hashes 后才能 rename。
6. 清除相关生产 to_vec_pretty(...).unwrap_or_default；序列化失败显式返回。
7. verify_published 必须解析 SelectionSet 并检查 shape、exactly one、hash 和 identity。
8. 补 snapshot 相同但 selection 不同、durable/receipt/selection/assets 篡改和每个 publish crash boundary 反例。

第五阶段：R1.4 前端 fail closed 与 R1.5 门禁

1. 删除 leafId ?? "" 和 selection ?? defaultSelection()。
2. 缺失/重复/未知 selection state 在本地阻止 plan，并显示 15 locale i18n 错误。
3. 运行 fmt → compile-only → 重新嵌入 Windows manifest → selection/jobs/job_control/engine → 完整 session_migration → 前端 13 文件 → typecheck → 定向 ESLint。
4. 完整 Rust 必须保持 278/0 或更多；前端相关套件必须 0 failed。
5. R1 全部条件满足后先进入 R1.6，不得直接进入 R2。

第六阶段：R1.6 上游集成授权门禁

1. R1 全绿后先只读 fetch 状态审计，列出 upstream ahead/behind、重叠文件和逐文件语义风险；未经授权不要执行 fetch 后的 merge/rebase。
2. 当前没有可恢复 checkpoint。未经用户明确授权，禁止 commit、merge、rebase、stash、reset 或 checkout。
3. 如果用户尚未明确授权“仅本地 checkpoint commit + 同步 upstream”，记录 needs-user-authorization，输出精确请求并停在 R1.6；这不是 blocked-external。
4. 只有用户授权后，才按可审责任边界建立本地 checkpoint（仍不 push/PR），fetch upstream，逐文件语义集成 14 个重叠路径。
5. 同步后重跑 R0.5、R1、前端 13 文件、typecheck、lint、build、audit；全部绿色后才继续 R2。

R1.6 获得授权并通过后，严格完成 V4 的 R2-R10：

- R2：previewId keyed 多会话状态机，A→B→A、branch/load-more owner/generation、错误和取消测试。
- R3：持久化 canonical plan，Host 在 plan 后重启仍可 commit；publish 每个 phase 可恢复。
- R4：revision SelectionSet 成为 handoff/export/MCP 的唯一授权真相，并修搜索鼠标路由、续聊导航、revision deep link 和 reader owner。
- R5：所有 cursor 版本化；损坏 IO fail closed；index temp+fsync+backup/swap/rollback。
- R6：移除生产无界读取，catalog/revisions/refs/issues 真分页，删除重复 reader 数据链。
- R7：本机生成 10k/100MiB/1GiB fixture，测时间/RSS/CPU/disk/cancel，并只杀自己启动的隔离子进程。
- R8：隔离 Windows dev Tauri 全闭环和真实 ACP 脱敏 trace；official/custom × shared/independent；first-send 100 轮 exact-once。
- R9：全仓门禁、真实15语言翻译、仅本功能 mixed-EOL 修复、行为稳定后拆大文件；核对上游 tiptap advisory。
- R10：基于最新代码和证据重审 53 项，形成小 PR 依赖方案；不实际 commit/push/PR。

开发纪律：

- 每个阶段必须有失败基线、最小生产改动、窄测试、扩大回归和退出条件。
- 不得用 optional、空字符串、default selection、unwrap_or_default、.ok()、catch 后空成功让测试变绿。
- 不得只改测试迎合错误实现；先写清契约，再同步生产和正反例。
- 不得把 imported transcript 称为原生 resume；历史只读，续聊是显式有界 Grok handoff。
- 不得让模型读取 SelectionSet 之外的 system/reasoning/tool input/result/sibling branch/subagent/asset。
- 不得使用 window.confirm/prompt/alert、原生 select、透明菜单或硬编码 UI 文案。
- 不得往 AppWorkbench 增加新业务 state/大功能块。
- 不得把 filtered out 算 passed，不得把 compile-only 算测试通过，不得把 mock/fixture 写成实机通过。
- 本机能生成 fixture、启动 dev Tauri、运行 Windows 和注入自有子进程故障，不得标 blocked-external。
- macOS/Linux 缺机器时，只把精确运行子项标 blocked-external；继续完成其他本机门禁。

Windows Rust 每次重新链接 grok_app_lib-*.exe 后，必须重新嵌入 src-tauri/windows-test-manifest.xml。0xc0000139 表示没有进入断言；--no-run 只算编译。命令见 V4 第 7 节。

从现在开始只把新结果写到：
H:\aicoding\grok-session-import-progress-v4.md

每个真实完成的小步记录：

STEP: Rn.x
STATUS: in-progress | done-synthetic | done-verified | blocked-external
BASELINE: 原始失败和 exit code
CHANGED: 精确文件和行为
TESTS: 精确命令、exit code、passed/failed/filtered、耗时
EVIDENCE: 最新日志、截图或脱敏 trace 的绝对路径
OPEN: 尚未证明的风险
NEXT: 唯一下一门禁

不要预写测试数字。不要中途只回复“计划已写”“R1 已过”或“外部阻塞”。完成一个阶段后继续下一个；真正缺 macOS/Linux 机器或真实账号时记录精确子项并继续所有不依赖它的任务。

最终只输出：各阶段真实状态、当前完整验证、真实产品证据、仍有的外部阻塞、技术债、建议小 PR 拆分。等待用户审核，不提交代码。
```

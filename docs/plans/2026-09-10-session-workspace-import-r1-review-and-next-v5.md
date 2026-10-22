# 会话与工作区导入：R1 收口复核与下一阶段开发执行书 V5

- 复核时间：2026-09-10（Asia/Shanghai）
- 工作树：`H:\aicoding\grok-app-session-import`
- 分支：`feat/session-workspace-import`
- HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`
- 当前性质：专用工作树中的大量未提交 WIP；没有 commit、push、PR、merge、tag 或 release
- 上一份总计划：`docs/plans/2026-09-10-session-workspace-import-grok-audit-and-next-v4.md`
- 上游集成细则：`docs/plans/2026-09-10-session-workspace-import-upstream-integration-r1.6.md`
- 本文用途：确认 Grok 到底完成了什么、R1 当前质量，以及从 R1.6 开始的唯一执行顺序

## 1. 结论

Grok 已经完成的不是演示壳，而是一套覆盖 Codex、Claude Code、Cursor 与通用文件的真实跨来源导入骨架。来源发现、只读快照、归一化事件图、预览、内容选择、工作区映射、持久作业、独立只读历史、搜索/导出、显式续聊和 MCP 授权路径都已有实质代码。

但这仍不是可提交、可发布的完成品。当前最准确的判断是：

- **领域能力广度：主体已成形。** 四类来源和主要数据链都有代码与大量合成测试。
- **R1 授权/发布原子性：已收口到合成测试绿色。** SelectionSet 已成为 plan→job→receipt→revision 的必填值，并补齐篡改、不同选择不去重和附件 digest 反例。
- **产品闭环：仍有明显断链。** 多会话内容状态、持久 plan、搜索鼠标路由、续聊后导航、revision deep link、catalog 分页和错误可见性没有全部完成。
- **交付可信度：尚不足。** 没有当前代码后的真实 Windows Tauri 全链、真实 ACP 四组合、真实来源活库、macOS/Linux 实机证据，也尚未同步落后 57 个提交的 upstream。

因此下一步不能继续横向堆来源或入口。必须先做 R1.6 上游语义集成，再按 R2→R10 收口产品、恢复、性能和交付证据。

## 2. Grok 已完成的工作

### 2.1 来源与只读边界

1. Codex、Claude Code、Cursor、通用 JSON/Markdown adapter 与默认根发现。
2. Cursor `state.vscdb` / `store.db` 的只读 SQLite/WAL 私有快照。
3. 来源登记、重定位、source revision、损坏/忙源/半行 JSONL 等诊断。
4. 来源数据库不做 checkpoint、VACUUM、repair 或写回。

### 2.2 归一化与内容模型

1. message、tool call/result、compaction、reasoning、unsupported、asset 等归一化事件。
2. parent/branch/subagent 关系、active leaf 与来源证据。
3. preview 分页、branch leaf、默认选择与附件状态。
4. imported history 使用 `imp:` 身份并与原生会话写命令隔离。

### 2.3 导入与恢复骨架

1. scan 分页、冻结“选择当前”、refresh/pump、cancel/continue。
2. preview lease、固定 prepared revision、workspace mapping 基础。
3. durable job、receipt、retry、cancel、undo、tombstone、部分 crash recovery。
4. 独立 revision、reader、CJK search、Markdown export 与全局搜索接线。

### 2.4 续聊与模型边界

1. foreign transcript 保持只读，不冒充原生 runtime resume。
2. 续聊是显式、可编辑、有界的 Grok handoff draft。
3. continuation sidecar、first-send gate、import-history MCP 与 capability 检测已有实现。
4. system/opaque、兄弟 branch、未授权 subagent/asset 等已有拒绝投影基础。

### 2.5 产品入口

1. 设置入口、侧栏/项目菜单入口和导入向导。
2. 来源、内容、映射、作业、结果与 imported reader 界面。
3. 15 locale 的 session-import 键已建立；本轮新增 Selection 边界错误均已本地化。

## 3. 本轮 R1 收口完成情况

### 3.1 前端提交边界

已完成：

1. `useImportedReader` 以 `importId + revision + leaf` 作为 owner；旧请求的成功、失败和 finally 均不能污染新 owner。
2. live SelectionSet v2 先做严格 raw 校验，再 canonicalize。
3. selected candidate、preview map、selection map 三套 key 必须精确一致。
4. entries 非空；preview 唯一；preview/leaf/event/pinned/asset ID 非空且各自不重复；pinned 必须属于 selected event。
5. v2 缺字段、未知字段、错误类型和版本全部拒绝；v1 只走显式旧草稿迁移。
6. 删除 `leafId ?? ""` 和 `selection ?? defaultSelection()` 提交旁路。
7. 显式空 selection 或用户手动更改选择后，加载下一页不会把 defaultSelected 事件重新扩入授权。
8. 本地 Selection 边界失败不调用 plan IPC，busy 正常复位，并保留稳定错误码。

验证：

- 原始相关 13 文件：142 passed / 3 failed。
- 修复后相关 13 文件：154 passed / 0 failed。
- 加新增 hook/error 测试共 15 文件：156 passed / 0 failed。
- `pnpm typecheck`：exit 0。
- session-import 定向 ESLint：exit 0。
- `pnpm lint`：exit 0。

### 3.2 后端 SelectionSet 原子链

已完成：

1. live v2 DTO 使用 required 字段与 `deny_unknown_fields`；0 entry、重复/空白 ID 和错误版本 fail closed。
2. raw decode/shape/duplicate/semantic validation 在 canonicalize/hash 之前执行。
3. plan、durable reload、published verify 共用 prepared/published selection evaluator。
4. evaluator 验证 preview/candidate/source/lease/snapshot/provider/native identity、leaf、event、branch、subagent 与 asset 授权。
5. selected asset 必须被所选事件引用、状态为 stored、blob 是普通非 symlink 文件，且真实 SHA-256 等于 digest。
6. `CommitItem.selection_set`、`DurableInput.selection_set/selection_hash`、`Receipt.selection_set/selection_hash/options_hash` 均为必填。
7. prior receipt 核对 installation、candidate、provider、native session、source、root、legacy、snapshot、options 与 selection 全身份。
8. revision dedup 绑定 normalizer + options + selection；同 snapshot/options 不同 selection 生成不同 revision。
9. prepared hashes 覆盖 manifest/events/assets/blobs；published hashes再覆盖 `selection.json`。
10. `selection.json` 在 staging 中严格序列化、写入并 `sync_all()`，计算 hash 后才 rename。
11. receipt/selection 相关生产序列化错误显式返回，不再写空 JSON。
12. 旧 default-plan helper 仅在 `#[cfg(test)]` 下存在；生产入口只保留 `plan_with_selection_set()`。
13. explicit reimport 会正确解除 catalog hidden 状态。

新增关键反例：

- durable JSON 缺 selectionSet、未知字段、坏 selection hash、重复 ID、0/2 entries、candidate identity 篡改。
- 同 snapshot/options 但 selection 不同不得 dedup。
- receipt selection/options 以及 installation/native/source/root/legacy/snapshot/provider/candidate identity 篡改。
- receipt file hashes、published `selection.json`、`assets.json` 篡改。
- selected asset blob 字节与 digest 不符。
- copy cancellation fixture 在严格 lease 下仍验证原取消语义。

验证：

- selection：10/10。
- jobs：9/9。
- engine：41/41。
- job control：30/30，373.66 秒。
- 完整 `session_migration`：284 passed / 0 failed / 1611 filtered，412.02 秒，exit 0。
- 测试 EXE SHA-256：`F578B44BBB8F975059ABDF2BCE2DDA1EDE8504350002F6561B0764DC2FBED8B2`。
- 原始日志：`H:\aicoding\grok-session-import-r1-full.log`、`H:\aicoding\grok-session-import-r1-job-control.log`。
- `cargo fmt --all -- --check`：exit 0。
- `git diff --check`：exit 0；仅有存量 autocrlf 提示。

### 3.3 当前代码卫生

- `pnpm deps:check`：exit 0。
- `pnpm audit:prod`：exit 1；唯一结果为 `@tiptap/core 3.30.4` 的 high `GHSA-j95f-988m-3j2f`。
- 本机 `upstream/main` 已是 Tiptap 3.31.3，应通过上游同步吸收修复，禁止在本功能阶段重复造依赖升级。
- 复核前工作树为 46 个 tracked 修改、143 个 untracked 文件；规模大且完全未提交。

R1 的准确状态是 **done-synthetic**，不是 done-verified：它证明了当前 Windows 测试进程中的领域契约，不证明真实 Tauri、真实来源或三平台产品行为。

## 4. 仍未完成或未证明

| 阶段 | 状态 | 未闭合点 |
| --- | --- | --- |
| R1.6 upstream 集成 | `needs-user-authorization` | 分支相对本机 upstream/main 落后 57；14 个同路径重叠；尚无可恢复 checkpoint |
| R2 多会话内容状态 | `partial / P0` | Host 仍是单 peek/单 content；A→B→A 状态丢失；branch/load-more ownership 未统一 |
| R3 durable plan | `partial / P0` | plan 仍主要在内存；Host 重启后不能可靠 commit |
| R4 授权与产品断链 | `partial / P0-P1` | sidecar/grant/export/MCP 尚未只认 revision SelectionSet；搜索鼠标路由、续聊导航、deep link、错误可见性仍缺 |
| R5 cursor/IO/index | `partial / P0` | 多种裸 cursor；部分 `.ok()`/default fail-open；索引 swap/rollback/fsync 证明不足 |
| R6 有界读取/分页 | `partial / P1` | catalog UI 仅首 50；reader 双数据链；部分整份读取 helper 尚存 |
| R7 性能/故障 | `mostly-unstarted` | 10k 当前只是正确性 fixture；没有 cold/warm、first-visible、RSS/CPU/disk/cancel 数据 |
| R8 真实产品验证 | `unverified` | Windows dev Tauri、真实来源活库、真实 ACP 四组合和 100 轮 exact-once 未跑 |
| R9 全仓/i18n/技术债 | `unstarted` | full test/build/clippy、15 语言真实质量、mixed EOL、大文件拆分未完成 |
| R10 验收/PR 规划 | `unstarted` | 53 项旧结果需按最终源码重审；现在不能拆 PR |

明确不能声称：

- 导入的 Codex/Claude/Cursor 历史可在原 runtime 中原生 resume。
- macOS/Linux 已验证。
- 真实用户活库、真实 ACP、Tauri UI 已通过。
- 10k 用例的 374 秒是产品分页耗时；主要耗时来自为 10k 项构造真实 prepared revision/index/lease。
- 当前代码已适合 commit、push 或 PR。

## 5. 下一阶段唯一顺序

### R1.6：建立可恢复点并语义同步 upstream

这是当前唯一授权门。没有用户明确授权时，只能报告 `needs-user-authorization`，不能 merge/rebase/stash，也不能在旧基线上继续扩大 R2-R8。

授权文本应明确为：

> 允许在 `feat/session-workspace-import` 专用工作树中建立仅本地 checkpoint commit，并语义集成最新 `upstream/main`；不 push、不提 PR、不合并到源项目主干。

获得授权后：

1. 重新记录 status、HEAD、remote refs、worktree、未跟踪文件与秘密扫描结果。
2. 按责任边界建立可恢复 checkpoint，不把 token、auth、日志、target、node_modules 或私人会话纳入提交。
3. fetch upstream，重新计算 merge base、ahead/behind、changed paths 和 overlap；旧的 57/362/14 只能作为基线。
4. 使用可审查的 no-commit 语义合并；禁止对重叠文件整文件选 ours/theirs。
5. 严格按 `2026-09-10-session-workspace-import-upstream-integration-r1.6.md` 逐文件融合。
6. ACP/session manager 必须保留上游“锁内采集状态、锁外 I/O/持久化/kill”的结构。
7. continuation 顺序必须是：Dispatching 先 durable；Running 紧邻 prompt RPC；success/failure/unknown 锁外落盘；UNKNOWN 不自动重发。
8. 补 `events_bg.rs` Error/ProcessExited 的 continuation transition。
9. 同步后重跑 R0.5、R1、上游 freeze/kill/stream/journal、前端 15 文件、typecheck、lint、build、deps、audit。

退出条件：上游能力不回退；当前 `pnpm audit:prod` high 关闭；R1 契约继续绿色；没有锁内 sidecar/disk/IPC；工作树有明确可恢复点。

### R2：previewId keyed 多会话内容状态机

#### 目标数据模型

在 domain hook/reducer 中保存，而不是 AppWorkbench：

```ts
type PreviewContentOwner = {
  candidateId: string;
  previewId: string;
  snapshotId: string;
  leafId: string;
  filterKey: string;
};

type PreviewContentState = {
  owner: PreviewContentOwner;
  leaves: string[];
  events: PreviewEventRow[];
  nextCursor: string | null;
  selection: AllowedSelection;
  generation: number;
  dirty: boolean;
  busy: "idle" | "branch" | "page" | "preview";
  error: string | null;
};
```

实现要求：

1. 新建 `useSessionImportContentStates` 或同等 domain 模块，以 previewId 为 key。
2. selected previews 固定后为每项建立独立状态；不是只初始化当前详情。
3. A→B→A 必须恢复各自 leaf、selection、pages、cursor、dirty 和 error。
4. 明确“取消选择再恢复”的策略；建议向导本次生命周期内保留，重新 scan/source revision 改变时清空。
5. 每个 branch/load-more 请求捕获完整 owner + generation + request sequence；AbortController 只能优化，不能代替迟到结果校验。
6. branch confirm 使用应用内 modal，异步路径统一 try/catch/finally；错误后 busy 必须复位。
7. branch 切换后 load-more 必须走新 leaf/new cursor，禁止回到旧 `peek.loadMore()`。
8. selection 的 explicit-empty/dirty 语义跨页保持，不能因新页或切回会话扩大授权。
9. 将 `SessionImportHost.tsx` 中对应业务块拆入 domain hook/component；不得向 AppWorkbench 增加 state。

必测矩阵：

- A→B→A 保留所有状态。
- A 请求迟到成功/失败/finally 不污染 B。
- 同 preview 切 leaf 后旧页迟到不合并。
- load-more 双击只接受一份结果且不重复事件。
- branch 失败、page 失败、取消 branch、取消再选。
- explicit empty、用户清空、select page、invert page 后分页行为。
- 键盘、disabled、busy、错误复制和窄窗口基本交互。

### R3：持久 plan 与 crash recovery

1. catalog 增加版本化 durable plan：canonical SelectionSet、preview/snapshot/file identity、mappings、options、plan hash、created/expiry。
2. `plan()` 返回前写入；`commit()` 只从 durable plan 加载并重验，不能依赖 Runtime.plans。
3. requestId 绑定 planId + planHash；同 request 不得接收另一 plan。
4. plan/selection/mapping/options/preview lease 任一篡改 fail closed。
5. 覆盖 plan 后 Host 重启、commit 前重启、每个 publish phase 中断、rename-before-receipt、receipt recovery。
6. current revision 切换只能看到完整旧 revision 或完整新 revision。

### R4：SelectionSet 成为唯一授权真相并修产品断链

1. revision 中完整 SelectionSet 是唯一授权真相；sidecar/grant 只保存完整 v2 或可验证引用，不再维护可漂移的 event-id/hash 副本。
2. handoff、model export、MCP search/read/asset read 全部从 revision selection 走同一 evaluator。
3. hidden/delete/undo/new revision/selection tamper 使旧 grant/draft 立即 stale/revoked。
4. 修全局搜索 imported history 的鼠标点击路由，与键盘行为一致。
5. 正确传入 `onOpenNativeSession`；续聊创建成功后导航一次且仅一次。
6. reader 贯穿 revision/leaf deep link；错误不能显示成空历史。
7. secret matrix 覆盖 system、visible/opaque reasoning、tool input/result、兄弟 branch、subagent、compaction、asset。

### R5-R6：fail-closed IO、cursor、索引与有界读取

1. 所有 cursor 使用版本化 envelope，绑定 owner/snapshot/leaf/filter/offset/expiry；拒绝裸数字、tamper、cross-owner 和 stale。
2. 损坏 lease/manifest/relations/index/receipt 不得通过 `.ok()`/default 变成空成功。
3. readonly catalog 不做 schema 初始化或 migration DDL。
4. 索引发布使用同目录 temp + fsync + backup/swap/rollback，并覆盖 Windows sharing violation 与启动恢复。
5. 删除或私有化生产可达的整份 event/archive 读取。
6. catalog/revisions/refs/issues/job items 真分页；前端保留 `{items,nextCursor,total}`，支持 load more/虚拟化。
7. 合并重复 reader 数据链，每份数据只读一次，并执行条数/字节预算。

### R7-R10：性能、实机、全仓与验收

1. R7：仓库内合成 10k metadata、100 MiB 单会话、1 GiB 批次、长 tool output、缺附件/WAL；记录 cold/warm、first-visible、RSS/CPU/disk/cancel。
2. R8：隔离 `GROK_APP_HOME` 跑 Windows dev Tauri 全链；1280×720、窄窗、200%、主题、中文路径、IME、键盘；真实 ACP official/custom × shared/independent；100 轮 first-send exact-once。
3. R9：执行 CONTRIBUTING 全门禁；完成 15 locale 真实翻译；只修本功能 mixed EOL；行为稳定后拆大文件。
4. R10：按最终代码重审 53 项，只使用实机通过/合成通过/部分/失败/未验证/blocked-external；形成小 PR 依赖方案，但等待用户审核，不实际提交远端。

## 6. 全局纪律与停止条件

1. 每阶段都要有失败基线、最小改动、定向测试、扩大回归、退出条件。
2. compile-only 不是 runtime test；filtered 不算 passed；mock/fixture 不写成实机。
3. imported history 永远只读；foreign transcript 续聊永远是显式 Grok handoff，不写“原生 resume”。
4. SelectionSet 外的 system、reasoning、tool input/result、兄弟 branch、subagent、asset 不得进入模型路径。
5. 禁止 `window.confirm/prompt/alert`、原生 select、透明菜单、硬编码 UI 文案。
6. 禁止向 AppWorkbench 增加新业务 state/大块逻辑。
7. 不读取/输出私人会话、token、Cookie、auth.json 或 secrets。
8. 不杀用户现有 Grok/Codex/Cursor/Node/Cargo；故障注入只碰隔离 home 和自己启动的进程。
9. macOS/Linux 缺机器时，只标精确实机子项 blocked-external，其他本机工作继续。
10. 未经明确授权，不 commit、push、PR、merge、tag 或 release。

Windows Rust 每次重新链接测试 EXE 后必须重新嵌入 `src-tauri/windows-test-manifest.xml`。`0xc0000139` 表示测试程序没有进入断言；`--no-run` 只算编译。

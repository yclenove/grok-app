# 会话与工作区导入：Grok 成果审计与后续开发执行书 V3

- 审计时间：2026-09-10 05:33（Asia/Shanghai）
- 工作树：`H:\aicoding\grok-app-session-import`
- 分支：`feat/session-workspace-import`
- 基线 HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`
- 当前状态：大量未提交 WIP；没有 commit、push、PR、merge 或 release
- 本文性质：当前源码审计 + 后续唯一执行顺序；覆盖旧文档中与当前证据冲突的状态结论

## 1. 结论

Grok 已经搭出一套实质性的跨来源会话导入系统，不是按钮、mock 或空壳。Codex、Claude Code、Cursor、通用文件四类来源的发现、只读采集、归一化、预览、后台导入、阅读、搜索、导出和显式续聊都已有生产代码与不少合成测试。

但当前不能称为“开发完成”，也不适合开始提交。准确状态是：

1. 导入内核覆盖面较完整，架构方向总体正确。
2. 原 R0 的两个根因已被正确修复并独立通过。
3. R1 只完成了 `SelectionSet` 严格反序列化和单项 helper，尚未贯穿 job、receipt、revision 与前端提交边界。
4. 完整 `session_migration` 回归仍为红色：266 passed / 11 failed。拆分复跑后确认有 2 个独立契约失败，另 9 个是失败后共享状态污染或顺序级联；因此稳定基线仍未关闭。
5. 多会话内容选择、Host 重启后 plan commit、授权唯一真相、强原子索引发布、超大历史资源边界、真实 ACP、Windows Tauri 产品闭环和 macOS/Linux 验收仍未完成。

质量判断：基础实现值得继续收口；当前属于“功能广、门禁未闭、不可交付”的阶段。下一轮必须先恢复完整领域回归，再完成 SelectionSet 唯一化，不能继续横向加入口或适配器。

## 2. Grok 已完成的工作

### 2.1 后端领域

- 新增 `src-tauri/src/session_migration/`，当前 34 个 Rust 文件、约 24k 行。
- 来源覆盖 Codex、Claude Code、Cursor 和通用 JSON/Markdown。
- 支持默认来源、自定义文件/目录、登记来源、来源重定位、分页扫描、冻结扫描选择和 source revision 校验。
- Cursor 覆盖 JSONL 与 SQLite/WAL 私有快照；设计上不 checkpoint、repair、VACUUM 或写回来源库。
- 归一化事件包含消息、工具调用/结果、可见 reasoning、compaction、unsupported、fork、subagent 和 rollback 关系。
- 使用 `imp:` 命名空间隔离导入历史；原生 connect/send/usage/unread 等路径已有拒绝或隔离接线。
- 已有 preview lease、固定 snapshot、source changed、refresh、content-addressed blobs、missing/too-large/outside-root 等处理。
- 已有 workspace mapping、existing/new/unassigned、项目保护与恢复逻辑。
- 已有 plan、commit、durable job、receipt、retry、cancel、undo、tombstone 和部分崩溃恢复骨架。
- 已有 imported reader、CJK 搜索、Markdown 导出、全局搜索跳转。
- 已有显式 Grok continuation draft、sidecar、首次发送闩锁和 import-history MCP 骨架；没有把外国会话伪装成原生 resume。

### 2.2 前端产品面

- 新增会话导入向导、来源选择、候选分页、多选、peek、内容选择、workspace mapping、作业结果和 imported reader。
- 侧栏、项目菜单、设置入口与设置搜索已有接线。
- 新增 session-import i18n 域并挂入 15 个 locale。
- 已有导入作业轮询、scan generation 防迟到、reader 分页、搜索命中定位和错误映射的局部测试。

### 2.3 已确认有效的最新修复

原 R0 两个根因已经关闭：

1. Runtime 丢失后，preview 可以从受控磁盘 manifest/index 恢复确定 leaf；没有把 required leaf 放宽成 optional 或空字符串。
2. plan hash 测试改为从真实 preview 读取合法 leaf/event ID；两个合法但语义不同的 selection 会改变 hash，未知 event 被单独拒绝。

R1 已开始但只完成以下内容：

- `SelectionSet.version` 和 `entries` 不再使用 serde 默认值，新请求缺字段会直接反序列化失败。
- 新增 `SelectionSet::single(PreviewSelection)`。
- 新增 `SelectionSet::single_entry()`，要求 job item 恰好包含一个 preview selection。

这些改动已通过编译，但还没有被 job/publish 链路使用。

## 3. 当前独立验证证据

### 3.1 通过项

| 门禁 | 当前结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo test --lib session_migration::selection --offline --no-run` | 编译通过 |
| 直接运行 manifest 修复后的 selection test EXE | 9 passed / 0 failed |
| 直接运行 manifest 修复后的 engine test EXE | 41 passed / 0 failed |
| selection/content 定向 Vitest | 4 files / 10 tests 通过 |
| `pnpm typecheck` | 通过 |
| session-import 定向 ESLint | 通过 |
| `git diff --check` | 通过；但 Windows `core.autocrlf=true` 对 6 个既有 Rust 文件给出 LF→CRLF 警告 |

### 3.2 完整领域回归失败

命令等价于：

```powershell
$exe = Get-ChildItem target\debug\deps\grok_app_lib-*.exe |
  Sort-Object LastWriteTime -Descending |
  Select-Object -First 1
& $exe.FullName session_migration --test-threads=1 --nocapture
```

结果：

- 266 passed
- 11 failed
- 1611 filtered out
- 288.08s
- exit code 101

两个独立根因：

#### P0-0A：MCP 自身 tools/list 被误判为 Agent 广告

失败测试：

`session_migration::capability::tests::mcp_tools_list_payload_is_not_agent_available_commands`

生产函数 `extract_advertised_import_tools()` 会回退到 `params.get("tools")`。因此把 import-history MCP 自己返回的：

```json
{"tools":[{"name":"import_history_search"},{"name":"import_history_read"}]}
```

误认为 Agent/CLI 已向当前会话广告了两个工具。这违反文件顶部的安全契约：自身 `tools/list` 只能证明 MCP server 存在，不能证明模型可调用。

必须修复为“按明确 session/update 协议字段提取”，禁止从任意顶层 `tools` 数组推断 Agent capability。

#### P0-0B：旧 continuation 测试与新安全契约冲突

失败测试：

`session_migration::continuation::tests::available_commands_update_unlocks_first_send_after_record`

当前契约已明确：`availableCommands` 是 slash commands，不是 MCP 工具广告，不能解锁完整历史续聊。该测试仍期待它解锁 first send，因此独立失败为 `HISTORY_TOOLS_NOT_EXPOSED`。

必须把它改成负向测试，并新增真正的 `_meta.tools` Agent-advertised 正向 first-send 测试。不能为了让旧测试变绿而恢复从 `availableCommands` 提取工具。

其余 9 个失败在独立进程中均可通过：`unknown_dispatch_is_not_replayed` 单独通过，`session_migration::engine` 整模块 41/41 通过。它们是前述 panic 后 `APP_HOME_ENV_LOCK`/全局环境污染或测试顺序级联。修完两个根因后必须重新跑完整领域套件确认 0 failed，不能仅删除失败名字。

## 4. 当前主要技术缺口

### 4.1 SelectionSet 尚未成为唯一真相

当前仍存在：

- `CommitItem.selection: Option<AllowedSelection>`。
- `DurableInput.selection: Option<AllowedSelection>`，且带 serde default。
- `engine::commit()` 从 `PreviewSelection` 中只取内部 `selection`，丢失 `previewId`、`leafId` 和 SelectionSet schema version。
- `Receipt.selection_hash: Option<String>`。
- prior receipt 只校验 snapshot/content hash，没有同时校验 selection hash。
- revision dedup 的 `opt_hash` 只来自 `item.options`，不同 selection 可能错误复用旧 revision。
- `selection.json` 在 publish/rename 后才写，而且写的是裸 `AllowedSelection`。
- `revision_hashes()` 只保护 `manifest.json`、`events.jsonl` 和 blobs；`selection.json`、`assets.json` 未受 receipt 完整性保护。
- plan 只存在 `Runtime.plans: HashMap`；Host 重启后 commit 找不到 plan。
- 前端 `commit()` 仍用 `leafId ?? ""` 和 `selection ?? defaultSelection()` 在边界制造非法默认值。

### 4.2 多会话内容选择仍是单例状态

- `SessionImportHost` 只有一个 `useCandidatePeek()` 和一个 `useContentSelection()`。
- effect 只按 `peek.previewId` reset，并抑制 exhaustive-deps。
- A→B→A 不会完整恢复各自 pages、cursor、leaf、dirty、busy 和 error。
- branch confirm 使用裸 `.then()`，没有 catch/finally、generation 或迟到保护。
- branch 后 load-more 仍调用 `peek.loadMore()`；它持有旧 preview/leaf/cursor，可能把旧 branch 页追加到新 branch。
- 只有点击“下一步”时才把当前详情 selection 写回 `selectionByCandidate`。

### 4.3 fail-closed、资源边界和持久化仍不完整

- 生产路径仍有多处 `unwrap_or_default()`、`.ok()` 和忽略错误，需要逐项分类；不能一刀切，也不能让损坏来源/manifest/index/relations 伪装为空成功。
- `read_preview_lease()` 对损坏 JSON 返回 `Ok(None)`，可能把损坏与不存在混为一类。
- preview/current-file revision、relations、last indexed event 等仍有静默降级点。
- plan 未持久化；receipt 与 revision 未完整绑定 selection。
- 索引虽已有临时构建和 Windows rename 重试，但尚无 backup/swap/rollback 与中断恢复的完整证明。
- 超大 history 路径仍需调用图和 10k/100 MiB/1 GiB harness 证明有界。

### 4.4 真实产品验证仍缺失

- 未完成真实 Codex/Claude Code/Cursor 活库版本矩阵。
- 未完成真实 Grok ACP Agent `_meta.tools` 广告与 live tool call trace。
- 未完成 official/custom × shared/independent 四组合。
- 未完成 Windows Tauri 从扫描到续聊/撤销的完整点击闭环。
- 未完成 macOS/Linux 运行验证；cross compile 不能替代运行验证。

### 4.5 可维护性风险

当前大文件：

| 文件 | 行数 |
| --- | ---: |
| `engine.rs` | 3549 |
| `adapters/cursor.rs` | 2218 |
| `continuation.rs` | 1749 |
| `jobs.rs` | 1509 |
| `mcp_stdio.rs` | 1312 |
| `SessionImportHost.tsx` | 1083 |
| `useSessionImport.ts` | 488 |

这些文件暂时不能为了“好看”先大拆。必须先补行为门禁，再按责任边界机械拆分，否则会让未稳定的状态机更难审查。

## 5. 后续唯一执行顺序

任何阶段未达到退出条件，不得把它写成完成，也不得跳到提交、push 或 PR。每阶段先制造/保留失败基线，再改生产代码，再跑最窄测试，最后扩大回归。

### R0.5：恢复完整领域绿色基线

目标：关闭本次新发现的两个契约失败，让 `session_migration` 领域套件可以作为后续重构门禁。

实施：

1. 修改 `capability::extract_advertised_import_tools()`：
   - 只接受明确的 Agent session/update 工具广告字段。
   - `_meta.tools` 的 camel/snake 兼容必须由真实协议证据或固定 fixture 支持。
   - 删除任意 `params.get("tools")` 回退。
   - `availableCommands` 永远不进入 MCP capability。
   - MCP server 自身 `tools/list` 响应永远返回空广告列表。
2. 修改 continuation 测试：
   - 将 `available_commands_update_unlocks_first_send_after_record` 改为负向语义，断言 first send 仍被阻止、sidecar 仍为 Prepared。
   - 新增 `_meta.tools` 正向路径，两个必需工具都出现时才允许 full-history first send。
   - 新增只有一个工具、错误 server prefix、普通顶层 tools、slash availableCommands 四类负例。
3. 测试不得用 `availableCommands` 或自身 `tools/list` 伪造 Agent 广告。
4. 先独立跑 capability/continuation 根因，再跑完整领域套件。

退出条件：

- 两个原失败测试按正确语义通过。
- 正向 `_meta.tools` first-send 测试通过。
- `session_migration` 完整套件 0 failed。
- suite 内没有 PoisonError 或依赖测试顺序的失败。

### R1：SelectionSet v2 贯穿 UI→plan→job→receipt→revision

目标：每个导入 item 只存在一个不可变、版本化、可验证的授权选择对象。

#### R1.1 数据结构

1. `CommitItem` 改为必填 `selection_set: SelectionSet`，禁止 `Option<AllowedSelection>`。
2. `DurableInput` 同样持久化必填 `SelectionSet`，删除 serde default。
3. `DurableInput::into_item()` 必须：
   - 验证 SelectionSet version。
   - 验证恰好一个 entry。
   - 验证 entry.previewId 与 durable preview lease/当前 item 对应。
   - 重新对 prepared manifest/index 验证 leaf、event、asset 和 subagent 关系。
4. `engine::commit()` 必须从 plan 复制完整 `PreviewSelection`，用 `SelectionSet::single()` 交给每个 item，不能只取内部 `selection`。
5. 生产 IPC 只接受 `SelectionSet` v2。旧 `plan()`、`plan_with_selection()`、`plan_with_selections()` 只能变为 `#[cfg(test)]` helper 或显式旧草稿迁移，不得被新请求调用。

#### R1.2 publish、去重与完整性

1. `Receipt.selection_hash` 改为必填；如保存 reference，也必须受 receipt/file hash 保护。
2. prior receipt 恢复必须同时核对：snapshot digest、selection hash、options hash、candidate/preview identity。
3. revision dedup key 必须包含 canonical selection hash；不同 selection 不能复用同一 revision。
4. 在 staging 目录、rename 之前写入完整单项 `SelectionSet` 到 `selection.json`。
5. `revision_hashes()` 至少保护：
   - `manifest.json`
   - `events.jsonl`
   - `assets.json`
   - `selection.json`
   - 所有 stored blobs
6. 序列化失败必须返回显式错误，禁止 `to_vec_pretty(...).unwrap_or_default()` 写空 JSON。
7. recovery 从 receipt/selection 文件恢复时必须重新验证 hash 和 shape，不得重建默认 selection。

#### R1.3 前端 fail closed

1. 在 `useSessionImport.commit()` 调 plan 前比较：
   - preview candidate key 集合；
   - selection candidate key 集合；
   - 每项非空 previewId；
   - 每项非空 leafId；
   - selection schema version 和 shape。
2. 两个集合必须精确相等；重复、缺失、未知都在 UI 边界停止。
3. 删除 `leafId ?? ""` 和 `selection ?? defaultSelection()`。
4. 增加一个 i18n error key，并同步全部 15 locale；错误可显示详情，但用户主文案不可硬编码。
5. 不把新状态继续塞进 `AppWorkbench`。

#### R1.4 必测反例

- previews `[A,B]` + entries `[A,A]`。
- 缺 B。
- 未知 C。
- 缺 version、缺 entries、错误 version。
- 空 previewId、空 leafId。
- 未知 leaf。
- 未知 event、跨 preview event。
- unknown asset、未 stored asset、属于未选择 event 的 asset。
- job item 中 0 或 2 个 PreviewSelection。
- durable input 被删字段或篡改。
- prior receipt snapshot 相同但 selection 不同。
- selection 文件在 rename 前后被篡改。
- entry 顺序变化不改 hash。
- version、leaf、event、field、asset、subagent、snapshot、mapping、options 任一变化都改 plan/revision 语义 hash。

退出条件：

- UI、Tauri IPC、PlanState、CommitItem、DurableInput、Receipt、revision file 都使用同一 v2 SelectionSet 语义。
- job 重载后可恢复 selection，任何缺失/篡改均 fail closed。
- R0.5 完整领域套件仍为 0 failed。

### R2：多会话内容选择状态机

目标：每个 preview 独立保存内容状态，迟到或失败响应永远不能覆盖另一个 preview/leaf。

1. 新建 `useSessionImportContentStates` 或等价 reducer，以 previewId 为 key 保存 candidateId、snapshot、leaf、selection、pages、nextCursor、filter、generation、dirty、valid、busy、error。
2. previewSelected 固定集合时一次建立所有默认 state；未打开详情的 selected candidate 也有合法 selection。
3. A→B→A 恢复各自 branch、勾选、页、cursor、错误与 dirty 状态。
4. branch 与 load-more 都从当前 keyed state 发送 previewId + snapshot + leaf + filter + cursor + generation。
5. branch 切换后禁止调用旧 `peek.loadMore()`；所有 Promise 使用 try/catch/finally。
6. 对取消再选规定明确策略并写测试。
7. 删除相关 exhaustive-deps 抑制；把 SessionImportHost 的业务状态拆到 hook/子组件。

退出条件：A/B 切换、branch 迟到/失败、load-more 迟到/失败、取消再选、键盘/disabled 均有测试。

### R3：持久化 plan/job/receipt/revision

目标：Host 在 plan 后、commit 前或 publish 任意边界退出，都能恢复同一不可变语义。

1. 在 catalog 或受控文件中保存版本化 canonical plan：SelectionSet、snapshot digests、mappings、options、plan hash、created、expiry。
2. commit 从磁盘加载并重新校验，不依赖 `Runtime.plans`。
3. plan/preview 过期必须明确失败；requestId 幂等必须同时绑定 plan hash。
4. 测 plan 后 Host 重启、重复 requestId、preview 过期、source/mapping/selection/options 变化、rename 后崩溃、receipt recovery。
5. publish 任意失败点只能看见完整旧 revision 或完整新 revision，不能出现半成品 current revision。

退出条件：清空 Runtime 后仍可 commit 合法 plan；篡改或过期必然 fail closed。

### R4：reader/continuation/MCP 授权唯一闭环

目标：完整归档只供本地用户阅读；所有模型出口只认同一个 evaluator。

1. sidecar/grant 只保存一个完整 v2 selection；派生字段必须在读取时严格一致。
2. handoff、model export、MCP search/read/asset read 全部走同一 evaluator。
3. asset read 只接受 assetId，并绑定 session/import/revision/leaf/selection hash/catalog visibility/stored blob。
4. secret fixture 覆盖 system、reasoning、tool input/result、兄弟 branch、subagent、compaction、图片；每个模型出口分别做 deny 测试。
5. selection/revision 被撤销、隐藏、刷新或篡改后，旧 grant/draft 立即 stale/revoked。
6. 保留 R0.5 的 capability 证据等级：SelfToolsList < AgentAdvertised < LiveAgentCall；不得静默降级 fragment。

退出条件：不存在第二授权真相，任何 secret 泄露均阻断后续。

### R5：版本化 cursor、fail-closed IO、索引安全发布

1. 所有 preview/search cursor 使用版本化 envelope，绑定 owner、snapshot/index revision、leaf、filter signature、offset 和 expiry。
2. 拒绝裸数字、tamper、cross-preview/import、cross-leaf、cross-filter、stale/expired。
3. 审计生产 `unwrap_or_default`/`.ok()`：来源、revision、manifest、lease、index、relations 错误不得变成空成功。
4. `open_catalog_readonly()` 只能验证已有 schema，禁止初始化或 migration DDL。
5. 索引发布实现同目录 temp + fsync + backup/swap/rollback，并能在启动时恢复中断状态。
6. 测并发 reader/rebuild、Windows sharing violation、每个 rename 失败点、旧/截断 schema、取消、重启。

退出条件：任意发布故障后始终保留完整旧 live 或完整新 live。

### R6：有界读取与存储层分页

1. 删除或私有化生产可达的整份 `load_events`/`load_archive_events`；用调用图证明。
2. entries、job items、revisions、continuation refs、issues/diagnostics 全部存储层分页。
3. relation、selection IDs、tool output、单事件正文设置字节/数量预算，超限显式 partial/truncated。
4. 前端按 owner/generation 合并去重，禁止先全读再 skip/take。
5. 10k item 测试加入反序列化计数或内存证据，不能只断言 UI 返回条数。

退出条件：10k/100 MiB 路径证明 Rust Vec、SQLite JSON、IPC 和 React 数组均有界。

### R7：本机性能与故障注入 harness

1. 仓库内生成合成 10k metadata、100 MiB 单会话、1 GiB 批次、超长 tool output、缺附件、合成 WAL；不得复制用户私人历史。
2. 记录 cold/warm wall time、first visible、峰值 RSS/CPU、磁盘增量、取消确认时间；模型耗时单列。
3. 只使用隔离 `GROK_APP_HOME` 和自己启动的子进程注入 Host kill、publish 中断、只读目录、空间不足。
4. 禁止杀死用户现有 Grok/Codex/开发进程。

退出条件：harness 和阈值可在同类机器复跑；本机可做项不得标 blocked-external。

### R8：Windows Tauri 与真实 ACP

1. 使用隔离 app home 的 dev Tauri 走通来源→扫描→多选→内容选择→映射→导入→reader→搜索→导出→续聊→撤销。
2. 覆盖 1280×720、窄窗、200%、深浅主题、中文路径、IME、纯键盘。
3. 用脱敏 trace 区分 slash availableCommands、MCP ready、自身 tools/list、Agent `_meta.tools`、live tool call。
4. 验证 official/custom × shared/independent；普通/remote/official-aux 不误注入。
5. 100 轮验证 handoff 一次且仅一次进入真实 `session/prompt`；UNKNOWN 不自动重发。

退出条件：Windows 产品闭环和真实 ACP 子矩阵有截图、脱敏 trace 和明确结果。

### R9：全仓门禁与技术债

按 `CONTRIBUTING.md` 运行：

```powershell
pnpm deps:check
pnpm audit:prod
pnpm typecheck
pnpm test
pnpm lint
pnpm build:ui
cd src-tauri
cargo fmt --all -- --check
cargo clippy --all-targets --offline -- -D warnings
cargo test --offline
```

然后：

1. 修模块级 allow、重复 DTO、生产 stub、旧 helper、静默 IO、失效命令、硬编码 UI。
2. 在行为稳定后按职责拆大文件；拆分前后同一测试集必须保持绿色。
3. 跑 i18n 15 locale parity、设置搜索、`git diff --check`、秘密扫描、依赖/许可/NOTICE。
4. 不进行全仓行尾机械转换；当前 `core.autocrlf=true` 警告先记录，若需 `.gitattributes` 或 renormalize 必须另做小批次并审阅 diff。
5. macOS/Linux 使用 CI 或目标机运行验证；cross compile 只记 compile evidence。

退出条件：Windows 当前树所有本机可执行门禁绿色；P0/P1 清零；平台外部缺口精确到测试项。

### R10：53 项重审与小 PR 方案

1. 对 F01–F12、W01–W07、D01–D05、S01–S04、R01–R07、C01–C11、U01–U07 逐项重评。
2. 状态只能是：实机通过 / 合成测试通过 / 部分 / 失败 / 未验证 / blocked-external。
3. 每项链接当前代码之后的最新日志、截图或脱敏 trace；旧日志不能覆盖新改动。
4. 输出小 PR 拆分建议，但未经用户审核不得 commit、push 或 PR。

建议拆分：

1. capability baseline + Selection v2/preview contract。
2. 多会话内容选择 UI/state machine。
3. durable plan/job/receipt/revision。
4. reader/continuation/MCP authorization。
5. cursor/IO/index/resource bounds。
6. Windows 产品闭环与跨平台修复。
7. 技术债、全仓门禁与验收文档。

## 6. Windows Rust 测试方法

本机测试二进制若直接报 `0xc0000139 STATUS_ENTRYPOINT_NOT_FOUND`，每次重新链接后都要重新嵌入 manifest。`--no-run` 只算编译证据。

```powershell
cd H:\aicoding\grok-app-session-import\src-tauri
cargo test --lib session_migration --offline --no-run

$mt = "C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\mt.exe"
$exe = (Get-ChildItem target\debug\deps\grok_app_lib-*.exe |
  Sort-Object LastWriteTime -Descending |
  Select-Object -First 1).FullName

& $mt -nologo -manifest windows-test-manifest.xml "-outputresource:$exe;#1"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& $exe session_migration::capability --test-threads=1 --nocapture
& $exe session_migration::continuation --test-threads=1 --nocapture
& $exe session_migration::selection --test-threads=1 --nocapture
& $exe session_migration::engine --test-threads=1 --nocapture
& $exe session_migration --test-threads=1 --nocapture
```

每次 `cargo test --no-run` 重新链接 test EXE 后，旧的嵌入结果失效，必须再次运行 `mt.exe`。

## 7. 每一步的记录格式

只在命令真实运行后追加记录，不得预写通过数字：

```text
STEP: Rn.x
PLAN_STEP: R0.5 | R1 | ... | R10
STATUS: in-progress | done-synthetic | done-verified | blocked-external
BASELINE: 失败测试、原始错误、exit code
CHANGED: 精确文件与行为
TESTS: 精确命令、exit code、passed/failed/filtered、耗时
EVIDENCE: 日志、截图、脱敏 trace 的绝对路径
OPEN: 未证明风险
NEXT: 唯一下一门禁
```

判定规则：

- 只有源码或 compile-only：`in-progress`。
- fixture/mock/unit 全绿：最多 `done-synthetic`。
- 当前平台真实 UI/活库/真实 ACP/恢复矩阵通过：对应子项才是 `done-verified`。
- 只有确实缺机器、账号、授权或路由的精确子项可 `blocked-external`。
- 不得用一组定向测试关闭多个阶段，不得把 `filtered out` 算 passed。
- 不得伪造日志、测试数、性能数字或真实协议字段。
- 未经用户审核和明确授权，不得 commit、push、PR、merge、tag 或 release。

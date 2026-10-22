# 会话与工作区导入：逐步开发执行手册

日期：2026-09-09。状态：执行中。适用工作区：`H:\aicoding\grok-app-session-import`。

本文只规定实施顺序和每一步的硬门槛。产品范围以 `2026-09-08-session-workspace-import.md` 为准，数据与安全契约以 `2026-09-08-session-workspace-import-contracts.md` 为准，最终是否完成以 `2026-09-08-session-workspace-import-acceptance.md` 的 53 项实际证据为准。本文与前三份文档冲突时，先停下修正文档，不选择更容易完成的一种解释。

## 0. 当前检查点

- 分支：`feat/session-workspace-import`；基线 HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。
- 当前全部功能代码、方案和测试仍是未提交 WIP。不得 reset、checkout、clean、覆盖或搬到 `H:\aicoding\grok-app`。
- 最近一次已记录的 Rust 导入测试：102 passed、0 failed；日志：`H:\aicoding\session-import-verification\scan-tests.log`。这是合成测试，不是全量仓库、UI、ACP 或三平台验收。
- 当前扫描改造涉及 `scan.rs`、`discovery.rs`、`sources.rs`、`composer_discovery.rs`、`engine.rs`、Tauri commands、TS API、`useSessionImportScan.ts` 和向导。它必须先完成复核，不能绕过去开发别处。
- 已有报告中的“P0–P4 合成通过”是历史说法。只有当前源码重新运行所得证据才有效。
- 未授权任何 commit、push、PR、merge、release；最终只给用户审核工作树和建议提交分组。

## 1. Grok 的执行纪律

### 1.1 每次只能有一个活动步骤

严格按本文 `S00` 到 `S35` 执行。不得同时“顺手”做后续 UI、adapter、MCP 或重构。发现后续问题时只登记到进度文件；除非它直接阻塞当前步骤，否则不切换任务。

每完成一步，立即在 `H:\aicoding\grok-session-import-progress.md` 追加以下记录，然后自动进入下一步，不询问用户是否继续：

```text
STEP: Sxx
STATUS: done-verified | done-synthetic | blocked-external | failed
CHANGED: 文件列表
TESTS: 精确命令、退出码、passed/failed 数
EVIDENCE: 日志/截图/fixture/trace 的绝对路径
OPEN: 尚未覆盖的真实限制
NEXT: 唯一下一步 Sxx
```

`done-synthetic` 只代表合成测试。只有实机行为才能写 `done-verified`。没有执行的命令不能写“通过”。测试失败时留在当前步骤修复；外部设备或账号确实不可得时标 `blocked-external`，记录所需条件，再继续下一个不依赖它的步骤。

### 1.2 每一步固定六段流程

1. 先读当前实现和测试，不凭旧报告猜代码状态。
2. 写清该步不变量、输入、输出、错误码和资源边界。
3. 先补能证明行为的 fixture/测试，测试不能只搜索源码字符串。
4. 只实现本步骤需要的最小完整改动，不留假按钮、mock 生产路径或“先返回空数组”。
5. 运行定向测试、格式检查和相关回归；检查 diff 中的临时实现、静默错误和无界读取。
6. 更新进度与验收结果，过门后进入下一步。

### 1.3 全程不可破坏的红线

- 来源只读。不得对 Codex、Claude、Cursor 的原文件执行写 SQL、checkpoint、VACUUM、repair、格式迁移或账号配置改写。
- `imp:<uuid>` 永远不是原生活会话，不得进入 connect/send/rewind/automation/Remote IM；续聊必须显式创建新的 Grok 会话。
- 创建交接草稿不发模型请求。首次真正发送的状态不明时进入 `DISPATCH_UNKNOWN`，不得自动重发。
- 被排除的 event、system、reasoning、sidechain、工具字段或资产，不得经 handoff、MCP search/read 或其他模型路径回流。
- 相同文本不等于相同会话。来源身份是持久随机 source ID + provider + native ID/规范化相对 locator；不能用磁盘盘符或正文 hash 冒充设备身份。
- 所有列表、正文、搜索、作业项、诊断和 IPC 都必须分页或有字节上限；不能在 Host 或 React state 中缓存一万条完整正文。
- 不在 `App.tsx` 或 `AppWorkbench.tsx` 增加领域状态/大块逻辑；15 个 locale 与 English key 保持严格一致。
- 禁止原生 `<select>`、`window.confirm/prompt/alert`、透明菜单、UI 字符串硬编码和共享 `~/.grok` 配置改写。
- 不提交真实用户历史、数据库、token、cookie、账号 ID、绝对私人路径或未脱敏 ACP trace。

## 2. 第一关：稳定当前 WIP

### S00：冻结现状并建立可信基线

目标：确认分支、HEAD、脏文件和现有后台进程；保存 diff/测试清单，绝不清理 WIP。

动作：

- 完整阅读 `AGENTS.md`、README 开发段、四份方案、两份 REVIEW、本文和最新 progress。
- 记录 `git status --short`、`git diff --stat`、未跟踪目录、当前 dev/test 进程的命令行；不停止不属于本任务的进程。
- 建立 `H:\aicoding\session-import-verification\S00-baseline-*` 日志。
- 运行当前定向 Rust、前端 hook/component 测试、typecheck、相关 ESLint 和 `git diff --check`。

退出：当前源码的 passed/failed 数有新日志；任何失败归入 S01，不沿用 102/106 的旧数字。

### S01：修到当前分支重新全绿

范围：只修扫描改造造成的编译、类型、测试和命令注册问题。

重点文件：`scan.rs`、`scan_tests.rs`、`discovery.rs`、`sources.rs`、`composer_discovery.rs`、`engine.rs`、`commands/session_migration.rs`、`lib.rs`、`sessionMigration.ts`、`useSessionImportScan.ts`、`useSessionImport.ts`、`SessionImportHost.tsx`。

必须核对：

- `scan_start` 迅速返回 queued/scanning，而不是遍历完整根后返回。
- 每个目录项都消耗扫描预算；大量不匹配文件也能及时取消。
- 临时扫描 DB 只保存分页候选和有界诊断；Host/React 不保留全量 candidate 数组。
- refresh 创建新 scan ID/epoch；旧 page/cursor/preview 响应不能覆盖新扫描。
- `select current` 固定当时序号上界；之后发现的候选不自动加入。用户取消勾选后，实际预览/导入集合必须随之改变。
- Cursor Composer 枚举只读 key，不在扫描阶段读完整 value/blob；正文只在 preview 固定。

退出：定向 Rust/TS 测试、typecheck、相关 ESLint、rustfmt 和 diff-check 全绿；S01 不能顺手改 adapter 保真或续聊。

### S02：持久来源身份与迁移

目标：同一登记来源重扫保持 source ID；另一台设备/另一份来源即便 profile 同名也不合并；用户显式关联移动/备份路径时才复用 ID。

动作：审查 `sources` schema 和兼容迁移；处理旧 WIP 的 per-candidate installation ID；两边同时存在时报告冲突而不是合并；保存旧 path alias；两个来源中同 native ID/同文本仍为两个 entry。

测试：同路径重扫、同 profile 不同根、显式 relocate、Windows canonical/verbatim path、两个源同文本、旧 identity 迁移、冲突回滚。验收关联 D01、D02、D05、W03。

退出：任何路径变化都不会静默制造重复或串来源；catalog migration 可重复运行且失败不改坏现有记录。

### S03：扫描持久性、继续、取消和清理

目标：完成扫描状态机 `queued → scanning → paused/completed/cancelled/failed`。

动作：

- 到达 50,000 候选后进入 paused，并提供显式 continue，不把 limit 当“扫描完成”。
- cancel 在普通本地 IO 下快速停止新工作并释放 walker/SQLite snapshot；已完成候选仍可查看。
- 应用启动清理“本应用确认拥有且已过期”的 scan DB/WAL/SHM；不能删除活扫描或其他 app-data root 文件。
- 重启后若不恢复 walker，状态必须明确 expired/restart-required，不能展示永远 scanning。
- 诊断只存前 100 条详情，同时保留准确 issueCount。

测试：并发 state/page、cancel race、cancel completed、continue paused、进程重启遗留文件、错误路径、symlink/reparse 越界、cursor 属主校验。验收 U01、U03、R04。

### S04：扫描 API 与 UI 完整接线

目标：扫描不阻塞窗口；分页只替换当前页；跨页选择只保存 ID；关闭向导取消扫描但不取消已经独立开始的导入 job。

动作：使用独立 hook 管理 generation、polling/event 和 cleanup；页面显示准确 found/selected/issues/state；提供上一页、下一页、取消、继续和刷新；不重复显示 frozen/extra 两套候选；失败按 error code 显示。

测试：先订阅/拉状态竞态、迟到响应、快速切来源、翻页时 polling、全选再反选、unmount、IME/键盘。实机检查聊天在扫描时仍可操作。

第一关门槛：S00–S04 全部完成后，再进入归档层。不得因首屏已显示就跳到 UI 美化。

## 3. 第二关：归档、作业与恢复

### S05：固定 preview 与租约

目标：scan 只给元数据 hint；preview 重新核对 hint，然后生成不可变完整快照。JSONL 固定完整前缀，SQLite 使用私有一致快照。

动作：preview state 持久化 source ID/root/legacy ID、snapshot digest、过期时间和租约；plan/job 引用时续租；取消/失败/过期后只清理本应用拥有且未引用的临时资料。

测试：mtime 不变的改写、追加半行、预览中改写、预览过期、job 持 lease、重启清理、源删除。验收 F07、S01、S02、S04。

### S06：归档 schema 与派生索引可重建

目标：manifest/events/relations/assets 是不可变真相；event page/search index 是可重建派生资料。

动作：所有 schema/version 先检查再写；索引缺失/损坏自动从归档重建；归档真相损坏明确报错；不能静默跳过坏 event_json。cursor 绑定 revision/filter，并拒绝跨版本复用。

测试：删索引、截断索引、损坏 events、旧/新 schema、跨 revision cursor、重建中取消。验收 U06、C11。

### S07：阅读分页的双重上限

目标：每页最多 100 event 且文本合计最多 256 KiB；单个大工具输出/Unicode 边界仍能继续分页。

动作：Host 在序列化 IPC 前执行字节上限；返回截断和 nextCursor；UI LRU 最多 8 页/2 MiB 文本；不得读取整份 archive 后再 slice。

测试：多字节字符、单条超大正文、100+ events、past-end cursor、损坏中间记录、前后页无重复。验收 F08、F12、U03。

### S08：持久作业输入、分页结果与空间预检

目标：commit 立即返回 job ID；作业输入不依赖内存 preview；`job_get` 不携带无限 job_items。

动作：job_items 结果分页；磁盘估算包含 SQLite snapshot、normalized/index/assets 和新旧 revision 共存；至少保留 `max(512 MiB, 10%)`；写入过程中继续检查空间。

测试：重复 requestId、不同 planHash、预览过期后执行、重启后执行、10k job items、磁盘不足、结果分页。验收 D01、R02、R07。

### S09：receipt 故障矩阵

目标：prepared、staged、published、linked、committed 每个边界均可恢复且不重复。

动作：预分配 import/revision/project ID；发布文件逐个 hash；同卷原子 rename；catalog/project 用短事务；不在 SQLite transaction 中持 store lock；发布后补关联只能按 receipt 的既定决策。

测试：每个边界的故障注入、rename 后 receipt 前退出、catalog 后通知丢失、两个进程并发 commit。验收 R01、R02、D01。

### S10：取消、重试、撤销、删除和 tombstone

目标：取消不启动后续项，保留 committed；重试只重试失败项；撤销只撤销该 job 拥有的变化。

动作：已有 entry 刷新时撤销恢复 previous revision；项目被用户改名/置顶/信任或已有后续会话时保留并报冲突；被 continuation 引用的 snapshot 不回收；删除写 tombstone，刷新不得复活，显式重导入才清除。

测试：R03、R05、R06 全组合以及杀进程边界；不得删除工程目录。

第二关门槛：归档可在源消失后独立阅读，作业可重启恢复，任何取消/撤销都不损坏用户项目或来源。

## 4. 第三关：来源保真与附件

### S11：通用 JSON/Markdown 回归

目标：保留旧导入能力，同时拒绝歧义和无界文件。

测试 BOM/CRLF、Markdown fence 内角色字样、合法 schema、非法 JSON、单文件根、相对 locator 冲突、资源上限。验收 F10、D05。

### S12：Codex 完整适配

目标：按固定源码版本处理 legacy rollout、metadata/paginated 能力差异、归档、工具、fork、rollback、compaction。

动作：完成 `replacement_history`；验证跨文件 fork/lineage；只合并能证明是同一次输出的投影并保留 contributing spans；真实同文本回合保留；未知正文为 metadata-only/partial，不伪造空会话。

测试精确 golden graph、事件数、关系、源 span、选 branch 的文件变更事实。验收 F01、F02、F04、F06、F07、F09、F11、D03。

### S13：Claude Code 完整适配

目标：UUID/parentUuid、块顺序、tool_use/result、summary/custom title、sidechain 和附件一致。

动作：同一 record 多块按原顺序投影；工具结果只与祖先分支调用配对；sidechain 可发现但默认不授权；不可逆编码目录名不猜 cwd；redacted/encrypted reasoning 标 unsupported。

测试纯工具 record、文本+结果混合、同 callId 兄弟分支、缺父、summary、sidechain 选择。验收 F03、F05、F08、F11。

### S14：真实附件存储

目标：只有 bytes 确实复制且 hash/size/mime 可核对时才标 `stored`。

动作：仅允许来源根内且用户选择的资产；路径越界、URL、SSH、缺失和超限分别标 unresolved/missing/oversize/excluded；复制进 content-addressed blob；使用既有 loopback media resolver 展示；失效旧绝对路径不自动匹配新根同名文件。

测试正常图片、缺失、symlink 越界、同名错配、40/200 MiB 边界、复制中取消、源删除后预览。验收 F08、W07。

### S15：Cursor transcript

目标：支持已登记 transcript shape、空工作区/未知 cwd、subagents 和未知 block；正文 ID 与相对 locator 稳定。

测试字符串/块 content、工具、空目录、同名 transcript、与 DB 重复候选。验收 F01、D04。

### S16：Cursor Composer 与 legacy Chat

目标：支持明确版本的 workspace/global DB、Composer key/value 与旧 Chat；读取已提交 WAL，跨 DB 引用有界核对一次。

动作：先取得各 DB 私有 snapshot；保存各自 revision/evidence；bubble 悬空时重试一次，仍不一致则 partial/source-changed；不能按时间拼不同 snapshot。

测试活 WAL、exclusive lock、无 SHM、workspace/global 变化、悬空 bubble、未知 table/key。验收 S01–S03、D03。

### S17：Cursor Store 与跨来源聚合

目标：解析已登记 Store meta/root blob/children/leaf/ACP 根；缺 blob 为 partial；Composer/Store/transcript 只在原生身份和 lineage 可证实时聚合。

动作：不要把文本相似/时间接近当自动合并条件；正文冲突保留版本选择和证据；另一来源只补有明确 ID 的缺失字段/附件。

测试完整 Store、缺 blob、环/多父、同 native 冲突、三个来源一致/不一致、未知 schema。验收 D03、D04、F04。

第三关门槛：每个“支持”来源都有版本、schema fingerprint、fixture hash、精确 graph/inventory/issues 和来源证据；未知版本不能显示 complete。

## 5. 第四关：工作区与项目映射

### S18：SourceWorkspace 模型

目标：从来源保存 workspace ID、名称、roots、default root、host/path syntax/evidence；工作区即便没有会话也能出现。

动作：解析 `.code-workspace` 相对 roots；保留缺失 root；Windows/Posix/UNC/WSL/SSH 语义不因当前 OS 被改写。

测试空工作区、双 root、缺 root、中文空格路径、同路径不同 host。验收 W01–W03、W05。

### S19：映射决策与项目保护

目标：每个 root 独立选择已有项目、新记录、显式新路径或 unassigned，并明确 primary root。

动作：用户选择优先于 cwd 建议；不移动已有项目 Space，不覆盖名称/排序/置顶/信任；缺代码仍可阅读，首次执行工具前要求可用目录；不 checkout/建分支/恢复补丁。

测试冲突建议、已有项目状态、缺 worktree、关联失败策略、重复 plan。验收 W04–W06。

### S20：项目关联恢复

目标：项目 link 与 catalog 之间靠 receipt 恢复；不能把外部用户改动覆盖掉。

测试项目创建后崩溃、项目被改名、重复恢复、unassigned 后补映射、撤销。对应 R01、R06。

## 6. 第五关：完整产品交互

### S21：来源和会话选择页

目标：默认根、自定义目录、单文件、已登记来源和显式 relocate 清楚区分；扫描状态与错误准确。

交互：筛选、分页、可见项选择、冻结全选、反选、预览、键盘、IME、忙/空/错误/取消/继续齐全。不要显示多余说明文字，不把整页塞成按钮堆。

### S22：预览与 AllowedSelection

目标：按需预览分页，可选择 branch/leaf、event、字段和附件；默认排除 system、reasoning、sidechain 和图片 bytes。

动作：UI 展示 inventory/outcome/issues/来源 span；同一选择对象贯穿 plan、handoff、MCP；修改选择使旧 plan/draft hash 失效。

测试预览迟到、加载更多、超大块、切 leaf、排除 secret-like event。验收 F04、F05、F08、C04。

### S23：映射、导入进度和结果页

目标：映射页按 workspace/root 组织；导入页显示 job 的持久事实；关闭面板后 job 继续；结果页按成功/partial/failed 展示并可重试/撤销/打开。

动作：总数未知不用假百分比；取消中显示 cancelling；job items 分页；错误详情脱敏并按 15 语言映射。

### S24：只读历史阅读器

目标：按 branch/revision 分页阅读消息、工具、附件、unsupported/compaction/rollback；搜索、上一/下一命中、来源 citation、Markdown 导出均绑定固定 revision/leaf。

动作：不得把 imported event 送进 live ACP reducer；不得把 `[[chat:...]]`、历史命令或 system 文本变成可执行 UI；搜索命中未加载页时按 cursor 加载。

测试 U06、C05 以及大页/损坏索引/导出兄弟分支排除。

### S25：侧栏、设置、旧入口和 15 语言

目标：导入历史与原生会话视觉/行为分开；侧栏、项目菜单、设置搜索深链和旧“导入对话”入口都进入同一领域 UI。

动作：英文 key 为权威；15 locale parity；来源徽标、搜索、排序、空态、日期、只读提示；不增长 AppWorkbench 领域逻辑。

### S26：UI 实机质量门槛

目标：Windows Tauri 开发版在 1280×720、窄窗口、200% 缩放、深/浅主题、长中文路径和纯键盘下可用。

检查：无溢出/重叠/白边/屏外弹层/透明菜单/点击穿透/焦点丢失；扫描和后台导入时普通聊天可继续；收集截图和操作记录。macOS/Linux UI 留到 S34。

第五关门槛：用户无需理解内部 schema 就能完成“发现→选择→映射→导入→阅读”，所有失败都有下一步，界面没有假功能。

## 7. 第六关：MCP 与真正续聊

### S27：统一范围策略

目标：实现唯一 `AllowedSelection` 判定器，供 handoff、search、read、export-to-model 和 asset read 共用。

测试 event/branch/field/asset 的 allow/deny 笛卡尔组合、删除/撤销后的 grant、猜 ID 和引用跨 revision。验收 C04–C06。

### S28：stdio MCP 协议和授权传输

目标：发行二进制 `--import-history-mcp` 只运行 MCP，不启动 UI/插件/来源扫描；标准逐行 JSON-RPC，stdout 无日志污染。

动作：grant 绑定 appSessionId/revision/leaf/selectionHash/protocol；每次 tool call 重新验证；避免把大授权全集塞进易超限环境变量，使用受控文件/管道并校验所有权与 hash；search/read 有 limit/cursor/maxBytes。

测试 initialize/tools/list/call、畸形 JSON、超长行、并发、A token 用于 B、过期/revoked、stdout 纯净。验收 C06、C11。

### S29：逐次 open 的 SessionToolContext

目标：`AcpClient::open_session_at` 每次接收当前会话上下文；new/load/fork/warm reuse/background resume 都重新绑定，释放会话清空 grant。

动作：import-history 工具与普通 user MCP、official-aux 分开合成；普通会话绝不注入；shared home 零改写；SSH/WSL 不发送本机路径/二进制。

测试进程复用 A→B→A、普通会话、session load/fork、并发 background、SSH/WSL 策略。验收 C06–C08、U05。

### S30：真实能力探测矩阵

目标：验证 CLI 实际向 Agent 暴露两个工具，不以本服务自身 `tools/list` 冒充成功。

矩阵：官方/自定义 × shared/independent；记录 CLI 版本、模型、配置模式、脱敏 trace。完整模式探测失败时停止发送，允许用户明确改用片段模式；不能静默降级。

### S31：可编辑交接草稿

目标：创建 continuation 只建立普通 App session、固定 revision/leaf/selection 的旁车和可编辑 composer 草稿，不调用模型。

动作：显示首条将发送片段、后续工具可读范围和 token 估算；用户钉选超软预算时提示而非丢弃；重复 requestId 恢复同一 appSessionId/draft。

测试 C02、C04、C10；监控 mock/真实请求计数为零。

### S32：首次派发与恢复状态机

目标：用户在普通 composer 首次发送时，真实 ACP prompt 只注入一次固定交接；状态 `prepared→dispatching→running→completed/failed-before-dispatch/unknown`。

动作：写 dispatchId/promptHash/agentSessionId 后再调用 ACP；失败前后分类；断线先核对 journal/agent turn，不能证明未执行则 UNKNOWN；恢复新 Agent 时使用固定历史加本地后续回合，不双加 bootstrap。

实测：100 轮前钉选约束出现在捕获的真实 prompt；工具实际可调用；断线/杀 Agent/杀 Host；重复点击；模型结果不计入导入历史。验收 C03、C09、C10。

第六关门槛：真实本地组合有脱敏证据；没有账号/CLI 时只能标外部阻塞，不能把 mock 或片段模式写成完整续聊通过。

## 8. 第七关：压力、平台与最终审核

### S33：性能和故障实测

数据集：10,000 会话元数据、100 MiB 单会话、1 GiB 批次、长工具输出、缺附件、活 WAL。记录冷/热缓存、CPU、峰值 RSS、磁盘、首批/完整扫描/preview/index/commit/首屏/MCP 初始化，模型响应时间单独排除。

目标：首批 50 条约 2 秒、已索引首屏约 500 ms、取消确认约 300 ms、普通本地 IO 句柄约 5 秒释放；未达标就分析瓶颈并修复，不能只写数字。验收 F12、U03、U07、R07。

### S34：三平台与全量回归

Windows：真实发现路径、活库、UI、CLI 四组合和崩溃测试。macOS/Linux：各自默认路径、权限、卷大小写、活库、UI、本地 MCP。缺测试机时明确列 `blocked-external`、所需命令和预期证据，不用 cross compile 代替。

全仓命令：

```text
pnpm deps:check
pnpm typecheck
pnpm test
pnpm lint
pnpm build:ui
cd src-tauri
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Windows 测试若出现 `0xc0000139 STATUS_ENTRYPOINT_NOT_FOUND`，先把 `windows-test-manifest.xml` 嵌入该次生成的测试 exe，再运行；必须记录测试确实进入断言，不能把“编译成功”算测试成功。

回归至少覆盖原生 session、CLI 导入/attach、搜索、导出、automation、Remote IM、setup gate、普通 MCP 和现有项目状态。验收 U02、U05。

### S35：53 项审计和交付

动作：逐行复核 F01–F12、W01–W07、D01–D05、S01–S04、R01–R07、C01–C11、U01–U07；每行只能填“实机通过 / 合成测试通过 / 部分 / 失败 / 未验证”，附精确证据。删除或修复生产 stub、静默 `unwrap_or_default`、无界读取、重复 DTO、失效命令和临时测试分支。审查安全、恢复、并发、性能、i18n、UI、跨平台和原功能回归。

最终交付：

- 可运行代码与开发版启动方式。
- 来源/schema 支持矩阵和完整性损失说明。
- 53 项验收表、性能数据、故障记录、UI 截图、脱敏 ACP/MCP 证据。
- 未验证项和外部阻塞的最小复现步骤。
- 按可审阅边界给出建议提交拆分；仍然不 commit、push、PR、merge 或 release。

只有首版实现完成、当前可执行的验收已真实通过、外部阻塞精确列明且整体 review 无未处理高风险问题时，才可向用户报告“等待审核”。不得自行缩小范围后宣布全部完成。

## 9. 建议提交拆分（仅最终建议，不执行）

1. 规范、fixtures 与 schema。
2. catalog/source/scan/preview 基础设施。
3. 持久 job、receipt、恢复和撤销。
4. Codex/Claude adapters 与附件。
5. Cursor transcript/Composer/Store 与工作区。
6. 向导、阅读器、侧栏、设置和 i18n。
7. MCP 授权与逐会话注入。
8. continuation 派发/恢复。
9. 压力、平台、回归和交付文档。

实际拆分以最终 diff 的依赖为准；每个提交必须能编译并有对应测试，不用一个巨大 PR，也不把不可独立编译的半步硬拆开。

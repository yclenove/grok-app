# 旧 normalizer 会话身份关联：有界设计与分步实施

S197普通接受恢复前置：N384真实COMMIT前process exit为5passed/1failed；N385
6passed/3genuine failed/0ignored（4新实际tests，非协议子检查），719inputs/Git稳定。
未接受有效preview在crash、同candidate旧completed job、rollback I/O失败后都被
cleanup误当completed删除；COMMIT后lost ACK真实accepted owner保留/worker完成。
未修复、不开放关联lane；不能以无retryable row或boolean推导完整接受owner。
cleanup的exclusive lifecycle guard持有ACTIVE，不能在内调用会再取operation的
update_checked；恢复lease需同owner边界和精确不可变身份，不放SQLite进lease回调。
外部v386计划及N385 closeout记录后续真实durable reconciliation要求。只有测试
模块变化；quality40/40、App13294/千行80、fresh719/2529/26 drift0，owners0。
没有当前full/Clippy/seed或桌面/provider验收，唯一V325与所有私人数据不变。

S196（2026-10-13）：stored_job的SQL CAST/json_remove已替换为metadata
优先、只读BLOB长度、同snapshot流式strict header，保留原16MiB/普通input2/v1。
stable字段及SQL createdAt匹配，duplicate/missing/unknown/wrong-type拒绝；
mutable items/progress/cursor不作为authority。N380真实5红保留；N381718inputs
71专项passed（含9新tests），N382同inputs旧seed实际1test通过，28/22/54/91子检查
非195tests。N383原31064 exit0，完整六门通过：71专项/1058领域/3987全库passed、
0failed；1/14/16ignored不计通过、集合重叠，严格Clippy通过。fresh718native/
2529frontend/26seed drift0，seed/full相同、Git稳定、owners0、quality40/40、
App13294/千行80不增。清自身765temp files/10483322logicalB及N380空temp0B，
observed H deltas+11440128B/+4096B另列；日志/seed/WIP/deps/current EXE/V325保留。
CheckOnly通过/no launch；N383 closeout记录实际终态，不代表桌面/厂商/provider验收。
point reader不证明整个input/job/proof accepted CAS或正文
全部item语义；receipt等SQL未全面限界，关联通道仍拒绝。继续v375/v380真实
producer+旧history reader、crash/orphan/rollback-I/O、publisher/recovery、Undo/ref/
grant和UI及原53范围。两main重新fetch、refs未变，未获checkpoint授权仍不集成；
无App/新包/provider/安全修改/commit/push/PR。

S195（2026-10-13）：普通enqueue增加跨进程acceptance owner，先detached preflight，
再owner→publication→SQLite→lease；失败后仅回滚本次新retain位，核对不可变
lease身份并保留expiry/batch；已有接受任务不清，内存retain随成功COMMIT。
N376真实COMMIT失败红→N377五专项绿；N378同716inputs旧seed实际1test通过。
N379原95857完整六门passed：5专项/1049领域/3978全库、0failed；ignored不计通过，
集合重叠，fmt/compile/manifest/Clippy/fresh quality40/40绿。716native/2529frontend/
26seed drift0，Git稳定、owners0、App13294/千行80不增。guarded own-temp清理和
失败日志保留见外部N379 closeout。本前置不是crash/orphan/rollback-I/O恢复证据，
未开放任何association接受/publisher/recovery通道；proof2草稿仍非authority。
真实producer必须复用此owner，不能新增第二套rollback协议。B3 reader在启用前
还要修AssociationInput::stored_job的SQL CAST/json_remove整列分配边界：metadata
优先、有界流式投影、duplicate-aware精确stable job字段，不以mutable progress
作为不可变authority，不缩普通input2容量。两main只fetch未合入，需授权安全WIP
集成检查点；后续B4 CAS+reader、publisher/recovery、B5/B6及原53范围保持。

S194（2026-10-13）：registration_input共享读取前限界且保留原16MiB cap；普通
worker input2容量/字节不变。独立proof1/2严格分发，v1正常接受记录字节/digest
不变；新版草稿绑定真实job/plan/input/confirmation，不能当accepted authority。
InspectionWitness复用原inspect认证与review payload，保留operation guard，返回时
释放只读catalog事务；不序列化、不缓存grant。authorize在matching-current捷径前
严格读取proof，所有未接受proof2仍拒绝。schema1/input3/receipt2不混用版本。
N368/N370真实红修复；N369测试漏跑worker、N371编译接线错误日志保留。N373固定
旧seed实际1test通过，28B1/22plan/54input及91draft子检查不算195独立tests。
N374同714inputs完整六门通过，62专项/1044领域/3973全库passed、0failed，
1/14/16ignored不算通过；Clippy严格通过，fresh714/2529frontend/26seed drift0、
Git稳定、owners0、quality40/40、App13294/千行80不增长。按owner/hash清可重建
自身temp，完整日志、seed/WIP/当前EXE/唯一V325保留；没有桌面或provider验收。
两边main已fetch但未合并：upstream9c5eb3b5领先28提交，79变化路径与WIP重叠。
下一步详见外部plans/v375-accepted-association-next-slice.md：真实accepted CAS与
旧history reader必须一起闭合，再逐通道串publisher/recovery，B5/B6仍未完成。
特别要补UUID旧producer：现verify_publication_owner仅拦local:，不能用原v1 guard
冒称覆盖UUID/bare-stem。另需证明retention失败回滚的精确owner，不能盲清boolean
破坏其他已接受任务。全局SQL CAST限界/原53/完整平台provider范围均未完成。

2026-10-13。S193 状态：B1 inspect/B2a确认-plan/B2b input3保持；本批补上
worker大尺寸v3读取前限界、retention流式路径投影和present坏proof明确拒绝。
rowid/typeof+只读BLOB元数据不先SQL CAST整列；普通input2容量/字节、receipt2、
store schema1/v1保持。N359/N360真实红已修且日志保留；N359日志中断缺原terminal
不伪造完成，N363诊断中断不计full pass；N364是EXPLAIN P2测试索引前提错误。
两个检查修正并有整列读取对照，N36559专项通过。N366实际1旧seed test通过，
28B1/22plan/54input子检查不算104独立tests。N367原3862 exit0，同711inputs
完整六门通过：59专项/1041领域/3970全库passed、0failed；1/14/16ignored不计
通过，集合重叠。fresh711native/2529F344frontend-owned/26seed drift0、seed/full
输入相同、Git稳定、owners0、quality40/40、千行80/80、App13294不增。
退出owners后按hash清自身temp/旧8a测试副本及空temp，逻辑量与磁盘观测分列；
N363非空205files/3175537B和所有失败日志保留。详见外部N367 closeout/cleanup。
关联意图仍不是grant，未开放接受/发布/UI；Proofv2、原子accepted CAS、Undo/ref/
grant和UI待做。下一详细步骤：外部plans/v368-proof-protocol-next-slice.md，
接续v359计划。registration_input原16MiB cap读取还需复用限界，其他receipt等
SQL CAST未全面审计，不能泛称所有读取已限界。原53和完整平台/provider范围不缩。
唯一 V325/data 未替换，新协议切片不是关联交付或整体验收。
不改变原 V5、实施契约或 53 项验收。这里补兼容性设计，不把额外接口按钮
当作已完成，也不以合成存档替代真实厂商、ACP 或三平台验收。

S191完成B2b前置guard，不是input v3：N351真实复现旧v2输入忽略options关联
字段，修复共享拒绝单/复数behavior字段（包括null）。plan加载、入队、durable
shape和direct publish共用guard，worker重启不能独立发布。N35250专项通过，
N353旧seed1test及额外Clippy通过，706inputs稳定、quality40/40；没有重跑新
全领域/全库。B2a commit拒绝仍保留，receipt/input仍2。下一完整B2b冻结协议/
raw绑定/显式版本解析/重启认证步骤见外部plans/v354-durable-association-input.md。

## 本轮实际观察（V332）

既有 V243 hash-pinned 合成存档通过当前严格 reader，随后实际原登记目录
scan → preview → durable plan → 清空 resident runtime → enqueue → worker publish
→ reload 全链通过。目录仅一个 pinned fixture；没有重新登记/迁移原来源。

旧 import `imp:ba6616b6-b9e8-440a-b01e-ab6db40c27a7` / revision
`028cbb7c-61ff-4d05-bd60-601124babd21` 保留；新 import
`imp:600c9716-9a74-4608-9c7c-4f2eea76b146` / revision
`faa28a72-ac4f-4a6d-aa1a-40d62f6cd888` 独立，normalizer 1.0.6。
同一 installation UUID 的当前 fallback 与 V325 metadata 观察完全一致。
两边各 2 消息；旧 entry/job/5 receipt 和不可变文件保持一致；同 requestId
在发布前、发布后、清空 runtime 后都返回同 job。

外部证据：`runs/v332-title-595654cbb0524561852ae4b72f641499/` 的
`historical-seed-tests.stdout.log`、`terminal.json`、`source-before/after.json`。
1 实际执行测试 passed/0 failed/0 ignored；fmt/compile/manifest 通过，697
native inputs/Git 稳定。这是 focused discovery，不是完整六门（其 terminal
的 `passed:false` 只表示未运行六门），也不代表身份关联功能已实现。

V329 首轮新测试误用 DTO 不存在字段，编译未到断言；V330 错把旧登记目录
当作文件；V331 显式传文件不符合 Cursor directory-only 扫描契约。都保留
原失败证据，修正测试前提后 V332 才是完整链路事实；没有修改产品代码过关。

## 1. 已知输入与边界

既有 V243 合成存档由已记录的历史构建/桌面周期产生：normalizer 1.0.0，
Cursor，已登记随机安装 UUID，nativeSessionId 为 `transcript`。
当前 1.0.6 对没有声明原生 ID 的同一 fixture 使用作用域化 transcript-locator。
V325 仅验证旧记录可读和新扫描身份，未对该旧来源执行 prepare/commit。

现有 `sources::resolve_identity` 只处理旧 `local:SHA(locator)` 登记形状；
`registration_provenance::Proof` v1 也明确要求该旧安装身份，不能把它放宽成
“路径/内容相同即可”，借此冒称支持随机 UUID / bare-stem 升级。

旧存档必须真实存在并有完整 receipt/hash 链。不得修改当前 manifest 的
normalizerVersion 制造旧版测试；不得直接插入 canonical entry 模拟完整导入。
旧/新独立发布本身不证明 bug：目前没有用户明确确认的旧条目目标绑定。

## 2. 产品行为

默认仍按真实来源身份导入。相同标题、文本、时间或路径仅是提示，不能触发
合并、拼接消息、自动选择旧目标或覆盖另一条历史。

在导入复核页增加可选“作为已有历史的新版本”：

1. 用户主动开启并选择一个现有 imported entry；默认没有目标。
2. 展示旧来源/安装/原生身份、旧固定 revision、旧解析器版本，以及当前固定
   preview/来源身份；摘要不能代替可打开的两边只读内容。
3. Host 执行只读 dry-run，返回允许/拒绝原因、旧条目预期版本及绑定摘要。
4. 用户明确确认。取消/关闭只丢弃这个关联意图，不动条目、作业或选择。
5. 最终提交创建新 revision，保持原 importId；旧 revision、receipt、目录、
   续聊固定引用和已发送上下文不可改写。也不复用旧解析器投影。

首个实现仅允许经过证据验证的 Cursor registered-UUID / old-normalizer
bare-stem → 当前 scoped transcript-locator。其他 provider、未知旧版本、
typed Composer、跨安装/host/family、已有关联链均明确拒绝或保留独立导入。
这些拒绝是首个兼容迁移分步边界，不删减原导入功能/验收要求。

## 3. 一份授权，不增加平行导入系统

复用现有 preview、SelectionSet、durable plan、enqueue、worker、publisher、
reader 和 provenance store。关联不是另一个 commit API，也不是改写来源文件。

建议内部 Intent v1 绑定以下字段（命名在实现前做 Rust/TS 协议测试冻结）：

| 旧目标 | 新固定输入 | 确认与时效 |
|---|---|---|
| importId / expectedRevision / 原 installation、provider、native、normalizer | previewId / candidateId / sourceId / host / 当前 native family / snapshotDigest / SourceMember binding / SelectionSet hash | dry-run 版本与摘要 / receipt anchor + cutoff / planId + planHash / requestId / expiry |

前端只能传 opaque IDs、期望版本和确认摘要，不能传路径、新造 native ID、
任意 receipt JSON 或“相信我”的布尔值绕过 Host。Host 从已有授权对象读取事实。
每份 plan 对一个旧 import 至多一个关联意图，重复目标/重复 candidate 拒绝。

当前 durable plan record 是 v1（不是 SelectionSet v2）。增加行为字段时应
引入可严格识别的 plan v2；保留无关联 v1 的既有解释/重试，新版字段不能被
旧路径忽略。关联摘要必须参与 canonical hash，持久加载后重新校验。
已接受 job 的同 requestId 重试依旧只返回同一事实；换关联/plan 必须换 request。
DurableInput 同样需要显式版本兼容，不能靠 serde 默认值吞掉迁移意图。

## 4. Provenance，不降低旧授权强度

保留现有 Proof v1 的形状、digest 和校验。新增独立、严格版本化的 Proof v2
分支，专用于 registered old-normalizer identity association；不要修改 v1 的
`local:SHA(locator)` 条件，更不要用宽泛 OR 接受任意 installation。

继续使用既有有界 `registration_provenance` store；其 SQL 形状和单 import
唯一性不必改。版本分发须先检查 proof version；未知/缺字段/类型不符 fail
closed。v1 已存 JSON 与摘要完全不改；v2 记录原始身份、确认、新输入、锚定
receipt、cutoff 和接受 job 的精确绑定。首次实现不覆盖旧 proof、不支持链式迁移。

Reader 在相同 catalog snapshot 内验证：

- 原 receipt 仍用原身份/原文件 hash 认证，不改 manifest/selection/receipt。
- canonical entry 仅能通过这个已接受的精确 proof 与原 receipt 关联。
- 原 source 必须是同一 registered UUID / Cursor / local host；旧 bare-stem
  与旧 receipt 的 adapter locator 关系需要专门的历史格式验证函数。
- 当前身份必须来自通过 pin/preview/selection 认证的实际 adapter 输出。
- anchor、cutoff、accepted job、request、plan、durable input digest 全部吻合。
- receipt cutoff 前允许旧身份，之后必须符合新身份；伪造/重放不被宽泛别名接受。

先完整认证旧 archive，再取得既有 publication owner。继续执行 N327 的
进程 + OS publish.lock 非阻塞协议，在 SQLite writer/store 锁外取得；忙则
SourceBusy，无 job/identity/proof/lease/project 副作用，同请求可显式重试。
短 accepted-job 事务重新 CAS 旧目标、source、anchor/cutoff、冲突与 tombstone，
原子写 proof/identity/job/input。publisher 在外部项目副作用前与最终事务内都
重新验证归属，旧 queued producer 不得创建第二条 entry 或覆盖已存项目映射。

## 5. 隐藏、删除、撤销与恢复

- 已隐藏/曾删除来源不能被普通关联悄悄复活；原 explicit reimport 契约保留。
- 旧/新任一身份的 tombstone 必须检查；仅迁移 tombstone 到新键不能当授权。
- 新 canonical 身份已由另一 import 占有，或旧目标有另一关联 proof：冲突，
  不自动合并两条已发布历史，也不删除任何一边。
- 关联只是新版本的身份桥，不改变旧 continuation_refs、leaf/selection/handoff。
  不扩大旧模型 grant；变更 current revision 后沿用既有 stale/revoked 规则。
- Undo 新作业按既有 revision/关联撤销契约处理，不能删掉整个旧 entry；已有
  用户修改/外部引用按冲突处理。是否保留身份桥须先用真实恢复链验证，不能
  简单删 proof 导致保留下来的旧版本突然不可读。
- accepted 前 rollback 所有新元数据；accepted 后取消/失败/重开以 durable
  job/proof 事实恢复。lost ACK 仍 exact-once，不能重新分配 importId。

## 6. 分批落地与退出条件

### A. 只做真实链路发现（本轮）

用既有、hash-pinned V243 合成 archive 验证当前 reader、scan、prepare、plan、
durable enqueue、publish、reload 和同 request retry。记录是否独立，以及原
entry/job/receipt/archive 是否不变；不实现关联，不启动新 App 包。

### B. Domain + 协议

先建立显式 Intent/dry-run/plan/proof v2 测试，再实现授权、accepted transaction
与 publisher/reader 桥。普通 UUID 导入与 v1 local-registration 现有测试必须
保持绿；不能临时通过改写 receipt/降低校验来过测试。

必须覆盖：允许关联保留 importId/旧 archive；取消 dry-run 无写入；SourceBusy
后同请求 retry；当前/旧 identity 碰撞；foreign UUID/provider/host/family；
normalizer 不支持；旧目标/receipt/source/preview/selection/plan 篡改；目标版本
与 cutoff 变化；tombstone/hidden/explicit reimport；accepted 事务 rollback；
旧 queued publish 两种先后顺序与项目副作用；worker restart/lost ACK；固定
revision/ref/grant；Undo 和失败恢复后的旧/新读取。

### C. UI

放进复核流程，不新增 App/AppWorkbench 状态。使用现有 modal/Select/只读
reader/错误组件与设计 token，15 locale 同键并检查自然译文。键盘焦点、busy、
stale、取消、empty、窄窗、深浅色、中文路径和长字段分别验收。不默认选择目标。

### D. 验证与合并准备

一轮 focused gate 收敛后执行贡献指南全门；只有行为变化需要新桌面包，验证
通过再替换唯一预览入口。退出精确进程家族并清理 task-owned 可重建产物。
重新审 53 项，保留真实 vendor/WAL、ACP official/custom × shared/independent、
100 round、Windows IME/DPI、macOS/Linux、容量、15语言及小 PR 的证据缺口。
不提交/推送/发 PR，待用户审核。

## 7. 明确未解决的设计点

1. 历史版本/adapter 格式允许表，要用真实 producer/不可变样本逐项证明。
2. Undo 后身份桥和早于关联的旧作业恢复语义，要先验证再冻结，不能靠猜测。
3. 跨机器搬迁需要已有 SourceInstallation 显式重选证据，不能使用盘符/路径
   相等替代；本机 V243 样本不等于 macOS/Linux 迁移已通过。
4. 已存在两条独立发布历史的合并不在这个接口中实现；需要另一份明确的
   冲突解决方案，不能在本次关联功能里静默吞掉第二条记录。

## 8. S189 实施次序与下一切片

### B1. 当前只读切片

`identity_association::inspect` 只接受严格版本化的 opaque target/revision 与
raw PreviewSelection。复用 preview lifecycle、lease/integrity、SelectionSet
验证、SourceMember pin、来源登记、published auth、原 receipt anchor/cutoff。
目标和来源登记在同一 detached catalog snapshot 内验证；摘要额外绑定登记
路径别名，不能只绑定 SourceRegistration 的显示 profile。未知旧/新格式拒绝。

N339 实际既有 V243 合成 archive 正常路径、28 个故障/摘要绑定检查、普通
独立发布与同请求重试通过；每次 inspect 都核全部 catalog schema/row 摘要
及 preview lease 字节 hash 不变。包含错误/超限已存 proof 行的存在性拒绝，
不是用这个故障行制造“合法迁移”。正常 derived index、verification cache
和锁文件仍可能写入，不宣称全文件系统零写。

N342 注册异步 `session_import_identity_association_inspect` 检查命令，使用
spawn_blocking，不开放关联 grant、plan/job 写入或身份变更，也没有 UI 入口。
28 个子检查不计为 28 个独立 Rust test，也不计为已支持关联发布。N340 新增
快照测试误假定默认 catalog 为 WAL，37 passed/1 failed/1 ignored，完整门
中止；只改隔离测试前提，分别验证 rollback 与 WAL，不修改产品日志模式。
N341 修正后的专项38/领域1020 passed，但 Clippy 因未被产品调用的只读接口
dead-code 失败，未运行全库；失败保留，不以 allow(dead_code) 隐藏问题。
N342 接入实际异步命令后同一旧 seed 正常调用与28项检查执行通过，并核
command error 与 domain error 相同。N343 是相同701输入的完整 native gate，
fmt/compile/manifest/Clippy通过；39专项、1021领域、3950全库passed/0failed，
专项/领域/全库各1/14/16ignored不计通过；terminal.passed=true。
fresh701native与26seed drift0、HEAD/index不变，原task App/build/test owner0。
这仍不是关联提交/UI或原53全部交付；唯一预览V325未替换。

### B2. 确认与计划协议（下一实施切片）

1. 冻结 `AssociationConfirmation` v1：`version / importId / expectedRevision /
   previewId / reviewDigest`，`deny_unknown_fields`；不传 path、native、receipt
   或 `authorized:true`。selection 取本次 plan 的固定 raw 选择，不能另造。
2. 按 preview 重新构造 inspect 请求，确认 digest 必须精确相等；对每个目标
   和每个 preview 至多一个意图，缺失/重复/未覆盖/混入别的 preview 拒绝。
3. 普通计划继续 v1；有关联意图使用明确 plan record v2，必须将非空意图
   纳入 canonical plan hash、持久加载和 expiry 校验。v1 出现关联行为字段
   必须拒绝，不能忽略后按普通导入执行。不要把它藏在任意 options JSON。
4. `PlanState`、CommitItem、DurableInput 用完整绑定传递这个意图。现有
   DurableInput 是 version2；新 input record 引入独立明确版本，不能笼统
   调高共用 receipt 的 `DURABLE_SCHEMA_VERSION`，或默认 None 吞掉新行为。
5. 测试重启后持久计划/输入不丢关联意图；selection、target、expectedRevision、
   digest、source/别名、plan JSON 任一改动拒绝。同 requestId 不得换关联事实。

实施先分为 B2a（严格确认与 durable plan）和 B2b（CommitItem/input 传递），
保持完整 B2 范围不变。B2a 不算关联提交交付：加载含关联的计划后，在普通
enqueue 之前明确拒绝；已存在 request 的相同 plan/job 重试仍优先返回既有事实。
首个红测试检查 v1 record 夹带关联字段（包括 null/空数组）不能被当成普通计划。

只读 review digest 绑定 raw PreviewSelection，因此关联 v2 plan 保留 raw 选择
的顺序；不能先 canonicalize 再重新 inspect。v1 普通计划的 canonical payload
和哈希不变。v2 还将 planId/created/expiry 纳入关联 payload，持久加载检查这些
值与本次记录一致。确认失效必须在任何 lease renewal 或 durable plan 写入前拒绝。
关联确认只是一份未来 accepted transaction 必须重新认证的意图，不是 grant。

退出条件：协议/持久路径测试通过，但在 B3/B4 可工作前不暴露产品关联按钮，
不允许关联计划走普通 enqueue 静默创建另一 entry，不计为功能交付。

### B3. 严格 Proof v2 与 reader

独立版本分发；v1 字节、digest、local:SHA(locator) 条件和原授权完全保留。
v2 必须绑定原 UUID/旧 native/旧 normalizer、正常 inspect 的固定确认、旧
receipt anchor/cutoff，以及最终 accepted job/request/plan/input identity。
reuse 既有 store 的唯一 import 行和有界 SQL；不可覆盖原 proof 或支持链式
迁移。未知/超限/类型错/缺字段是拒绝，不是“没有 proof”。先 test-first：
旧 receipt 按原文件认证，新 canonical identity 仅由精确已接受 v2 解释。

### B4. accepted CAS、publisher 与同请求恢复

先完整 detached 验证，再在 SQLite writer/store 锁外取得已有非阻塞 publication
owner；Busy 无 identity/proof/job/lease/project 副作用。短事务 CAS target revision、
source/别名、anchor/cutoff、hidden/tombstones/native 冲突，原子接受 proof/identity/
job/input。publisher 在项目副作用前与最终事务内复查归属，保持原 importId。

特别注意：accepted 后 preview 已 retained，不能把同 requestId 重试重新送进
普通 inspect 后得到 SourceBusy。先核既有 request→同一 plan/job 事实并复用
accepted input；新请求才走完整 preflight。覆盖 lost ACK、清 runtime、worker
重启、源离线、queued 旧 producer 两种赢锁顺序和项目映射保留。

### B5. Undo/取消/失败与固定引用

已有 `jobs_publish::undo_job` 在 refresh 的 `previous_revision` 非空时只恢复
current_revision，保留两份 archive 和原 entry。这提供可复用的基础，但不
证明关联场景可直接工作。先验证关联 receipt 必须准确记录旧 revision，
不能误走 created_entry 的隐藏/删除分支；Undo 不删旧 entry/旧归档。

明确测试 Undo 后旧/新固定 revision 都可读，refs/selection/handoff 和已发
上下文不变，旧 grant 不扩权；不得简单删 proof 导致仍保留的旧历史不可读。
accepted 后取消/失败/Undo/后续同 native refresh 的身份桥保留语义须由真实
durable pipeline 证明后冻结。用户别名、后续 revision、外部引用冲突保持原行为。

### B6. UI 与原范围收口

仅在 B2–B5 完整串通后接现有复核页、只读内容和确认弹层，默认无目标。
完整 keyboard/busy/stale/cancel/empty 和深浅/窄窗/中文路径；15 locale 同键、
自然译文和视觉验收。App/AppWorkbench 不增长。最后一次相关逻辑收敛后跑
全门，才构建/实机验证/替换唯一预览；逐轮退出精确测试 owner 并清可重建产物。
原 53、真实厂商/live-WAL、ACP 四组合、100 round/UNKNOWN、Windows IME/DPI、
macOS/Linux、容量和小 PR 审阅不被该历史兼容切片替代。继续不提交/推送/PR。

## 9. B2a 证据与 B2b 执行边界

N345 39旧专项passed/2新回归failed/1ignored，证明 v1 loader 会忽略关联字段，
包括 null 和隐藏在 hash-valid options 的行为。保留失败日志。B2a 修复严格
版本/字段分发；关联 v2 必须非空，重复 target/preview、foreign preview、
非 canonical ID/digest、缺字段/null/未知字段拒绝。确认前重新 inspect，本次
raw选择、来源 metadata/alias 和 reviewDigest 必须相同，失败无 plan/lease 写入。

N346 实际46专项passed；N347原hash-pinned旧seed实际1test通过，保留原28个B1
子检查，新增22个plan子检查（不是22个独立test）。它验证 async plan endpoint、
raw顺序、runtime reset/recovery、错误确认/alias/profile/持久payload修改、拒绝
提交且catalog/lease不变；普通独立发布/同request重试/旧entry-job-5receipt仍保留。
输出中的旧 `associationInspectionOnly` 标志只描述B1 inspect，没有身份变更；
新增 `associationPlanChecks:22` 才是B2a范围，成功建plan确实写plan和续期lease，
不得将整个N347说成全文件系统只读。原immutable seed/来源不改。

F344补上B1遗漏的全前端验证，10176passed/0failed/0pending（含9wrapper），
types/全仓lint/UI build通过，3232 inputs/Git稳定。该native修改没有前端变更；
只能重用F344中fresh一致的前端-owned子集，不冒称F344绑定后来的native。

N348原50252退出0，terminal.passed=true：704native inputs/Git稳定，fmt/
compile/manifest/Clippy通过，46专项/1028领域/3957全库passed、0failed；各
1/14/16ignored不计通过。其后仅拆4个完整测试，正文逐字一致，父1158→941、
子222；N348→N349仅父测试changed/子测试added，产品实现inputs hash完全相同。
N349当前705输入fmt/compile/manifest和46专项通过，额外Clippy49.30s exit0；
quality40/40、千行文件80/80，上限不变。N350相同705输入旧seed实际1test通过，
保留28B1+22plan子检查。不冒称705输入全库重跑。失败日志、原seed、依赖和
当前EXE/bundle保留，退出owner后清自身temp，逻辑量与observed free delta分列。

B2b下一轮按以下次序，不能提前移除B2a的commit拒绝：

0. 千行预算收口已完成，所有assertions/fixtures/53范围保留，父helpers由
   子模块复用，外部历史seed的qualified入口不动。后续修改不得再次超限，
   B2b收敛后运行下一完整门，不用拆分前全库证据覆盖新产品实现。
1. 先写 CommitItem/input 丢意图、legacy2夹带新字段、v3缺失/null/类型错误、
   selection/target/revision/digest/plan事实不匹配的红测试。正例来自正常inspect
   和v2计划，不手造positive canonical entry或编辑旧manifest版本。
2. 引入独立input v3；原receipt `jobs::DURABLE_SCHEMA_VERSION=2`不动。v2即使
   association字段为null也拒绝；v3必须有nonnull确认。按显式version分发，
   不使用可回退到旧解释的untagged enum或Option默认值吞掉新行为。
3. PlanState→CommitItem→DurableInput传完整绑定，按preview一一对应，保留
   review raw选择。新字段纳入实际accepted input digest，并与job的planId/hash/
   request绑定。旧普通输入原字节/hash语义不改，混合plan普通项仍用v2。
4. 重启后的持久输入恢复/shape/receipt验证均保留意图；非空关联不能在任何
   enqueue/worker/直接publisher路径中被忽略。B3/B4未完成前继续明确拒绝。
5. B2b收敛后再跑focused/同seed/full门，然后依B3严格Proof reader→B4accepted
   CAS/recovery→B5Undo/ref/grant→B6UI。已有request查同plan/job的顺序不改，
   不重新inspect已retained preview。复用现有store/锁/worker，不另造导入系统。

当前唯一预览V325不含B1/B2a的最新后端，不替换包、不启动桌面、更不计整体验收。

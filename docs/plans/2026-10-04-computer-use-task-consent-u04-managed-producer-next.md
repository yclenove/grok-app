# U04 下一批：原 reservation 下准备 ManagedBrowser task scope

2026-10-04 最新：[ExistingTab多成员切换与原Host恢复（E2）](2026-10-04-computer-use-task-consent-u04-existing-switch-checkpoint.md)。
原Bound proof精确借用/切换/回滚，Pause保留active、fresh capture后原Host恢复；Stop提前终态与失败切换后Pause/Continue不可用已修。
23项新Core回归；最终Core1054/1070、默认App CU267/1手动ignored、ACP167/routing38、Node173/协议38及四种strict/fmt/quality通过。
3162源含extension/最终App冻结；原owners退出后清15组旧缓存逻辑6.28GiB，H空闲观察增加4.51GiB，source/current/App/Core/seed/fix.patch不变。
App parser/picker仍Desktop-only；下一步E3 off首次Pair+Share事务→E4共享UI/15locale→E5同候选联合链。
仅backend/synthetic peer，不是真人MV3/App/Grok/installed整链；全U D/四surface/三平台/签名/12h/UX仍开，active / partial — not releasable，单writer、未提交发布。

以下为E1 picker历史，不是当前最新结论。

2026-10-04 历史：[ExistingTab原分享 Host picker（E1）](2026-10-04-computer-use-task-consent-u04-existing-picker-checkpoint.md)。
Core picker接通、网页不走Desktop标题过滤，10项新回归；最终Core1031/1047、App CU267/1手动ignored、ACP167/routing38、Node173/协议38及strict通过。
3152源/最终App冻结，清15组旧缓存逻辑6.32GiB、H空闲端到端观察增约3.66GiB；首中断/新plan完成及source/current/seed不变证据保留。
App parser/picker仍Desktop-only；按ExistingTab E2切换/恢复→E3首次Pair+Share→E4共享UI/15locale→E5联合链继续，不重做managed constructor/publication/UI。
全U D/四surface/三平台/真人App/Grok/签名/12h/UX仍开，active / partial — not releasable，单writer、未提交发布。

2026-10-04 历史：[ExistingTab后端scope基础](2026-10-04-computer-use-task-consent-u04-existing-scope-checkpoint.md)。
source Share真实document proof、原reservation借用/回滚/Stop归还及17新Core回归已验；最终Core1021/1037、App CU267/1手动ignored、ACP167/routing38、Node173与strict通过。
3151源含extension冻结，原owners退出后清13组旧缓存、H空闲观察增加5.97GiB；仅backend，App/Core picker仍Desktop-only，off首用Pair+Share及inactive member switch未接。
下一步使用[ExistingTab E1–E5执行顺序](2026-10-04-computer-use-task-consent-u04-existing-next.md)，不重做已经完成的managed constructor/原ACP publication/UI；全U D/四surface/三平台/真人App/Grok/签名/12h/UX仍开。

2026-10-04 历史：[dispatcher与owned生命周期checkpoint](2026-10-04-computer-use-dispatch-owned-lifecycle-checkpoint.md)。
dispatcher取消join/失败丢失两反例已修；owned native feature-off、原stdin queued-stop及真实10分钟TTL1项通过。
普通native7通过/1手动ignored、默认App CU267通过/1手动ignored、ACP167/routing38及strict/Node28/fmt/quality通过；3084源冻结。
原owners退出后清旧缓存逻辑4.25GiB、H空闲观察增加4.17GiB，最新程序/seed/依赖/源码和首败保留。
下一步推进ExistingTab/WebView真实Host scope proof/原reservation及其它surface；原自然cleanup长尾、完整U D/三平台/真人App/Grok/签名/12h/UX仍开。
不是自动dispatcher/主窗真人/安装态E2E，不将synthetic证据拼成整链。

2026-10-04 历史：原owned approval完整native/真实隔离存储/原App ACP publication/实际ACP续行
五联合场景已通过，见[owned approval checkpoint](2026-10-04-computer-use-owned-approval-checkpoint.md)。
Stop/Drop在实际launch gate、session/transport replacement在最终publication gate，各原资源确认Stopped并join。
不是主窗真人/自动turn dispatcher/真实Grok或安装态；下一步补其它owned阶段/feature-off/deadline/
queued continuation、真实主WebView命令联合验收和自然cleanup长尾诊断，其它surface/全U D范围保留。

2026-10-04 历史：生产cleanup consumer的原join所有权已修、测试StopRequested假成功已补，见
[cleanup checkpoint](2026-10-04-computer-use-owned-cleanup-checkpoint.md)。
真实source Chromium gate跨pending仍占用、原close一次/Stopped证据已补；原自然长尾关闭根因仍开放。
下一步仍owned approval worker完整native批准/存储/原续行/Stop与late launch联合门槛及其它surface。

2026-10-04 历史：共享UI/compiled descriptor已接，见
[managed UI checkpoint](2026-10-04-computer-use-task-consent-u04-managed-ui-checkpoint.md)。
一次明确卡/面板gesture直接constructor，固定本任务当前tab、即时Cancel/Stop/失败retry/15locale；
Desktop parser/picker begin仍只支持桌面，不用managed假候选。挂载/换范围零准备副作用。
下一步补owned worker完整native批准/原cleanup_pending压力失败与其它surface；全U/D范围不变。

以下是接线前的历史设计与门槛，当前状态以最新checkpoint为准。

2026-10-04 更新：Core producer和默认App/ACP真实native publication已实现并验证，见
[本批checkpoint](2026-10-04-computer-use-task-consent-u04-managed-producer-checkpoint.md)。
生产TaskScopeTarget/picker/UI仍Desktop-only；下一步为第5节App owned runtime/命令/共享UI接线。
App/ACP/SessionManager/主 WebView managed 命令接线增量现已通过，见
[入口 checkpoint](2026-10-04-computer-use-task-consent-u04-managed-entry-checkpoint.md)；生产 UI descriptor 仍未开放，接下来完成共享controller/明确确认/15locale及UI联合验收。
以下保留原设计与完整门槛，不将Core constructor视为已开放产品功能。

原稿状态：**managed resource producer design**；第 3 节共用 App/ACP publication 已完成，见
[批准发布 checkpoint](2026-10-04-computer-use-task-consent-u04-approval-publication-checkpoint.md)。身份基础已完成，见
[checkpoint](2026-10-04-computer-use-task-consent-u04-managed-identity-checkpoint.md)。
本稿不开放surface gate，也不把legacy managed run或synthetic Bound当作任务模式。
单Codex writer，原dirty保留，不启动Grok/子代理、不自行提交/发布。

## 已核实的依赖循环

- `TaskScopeTarget::parse`、App picker capability和Core `begin_task_scope_picker`仍仅支持Desktop。
- `ConsentState::start_preparation`先要求request.bound有真实proof，然后才分配唯一run_id。
  每次重试有不同run；失败的原run及cleanup owner不能被新attempt借用。
- `/scope-identity`只能证明已存在的owned tab。`open_managed_profile`需原managed run admission。
- `authorize_managed_target`不准另一run claim现有tab。临时用legacy run建tab，再批准到新task
  run不是解法；捕获到合法UUID也不能补上ownership。
- App approval 原先最终commit只在返回后检查endpoint；现已拆锁外 native preparation 与
  borrowed authority-only commit，并精确 pin 原session/ACP/RequestTransport。managed producer
  复用该边界，不得恢复为grant先发布、再通知UI owner已失效。

## 必须满足的时序

明确用户managed范围选择 → 精确Host reservation → 非执行owned资源准备 → 真实proof →
该reservation的Bound/执行准备 → 原session/ACP/preference + Broker原子commit → 唯一续行。

卡片加载、metadata订阅或model request都不启动worker/profile/tab，不读取网页。
首版明确批准对象为本任务独立profile/context和一个实际tab；new-tab/popup策略若未显示
给用户并实现完整membership，不默认允许。日常browser/profile不在范围内。

## 1. Core reservation先于owned资源

新增Host-only、不可从tool JSON构造的managed preparation入口，沿原request/context/
selection_revision/feature epoch/deadline/Stop generation校验，不新造grant系统。
同一次user gesture冻结唯一preparation/run/profile policy；重复gesture在准备中不再建profile。

把“合法申请reservation”和“已捕获真实Bound”分开表达。未Bound的reservation不满足
commit/continuation/execution permit，不能借pending run绕admit_consent。保留Desktop
旧入口行为；不要将start_preparation原native检查改成宽泛optional而使现有调用绕过Bound。
可用私有typed phase或独立Host prepare handle，必须由原owner留存及Drop/Stop收束。

所有resources使用reservation实际run_id，不把model提供的profile/run_id作Host policy。
proof里的owner必须等于该run；task/session/selection/policy也须精确匹配。Bound在锁内
一次发布到原reservation，捕获/HTTP/native工作全在锁外，前后复验原事务。

## 2. 资源owner与终态

原opaque handle拥有resource准备、请求cancel handle及cleanup/join；UI waiter取消只fence，
不丢原资源。Stop/feature-off/reconnect/deadline不长持Broker锁等待spawn/HTTP。
必须记录尚无Broker run但已有worker/context的准备owner；不能只靠run_exists决定无需清理。
迟到profile/tab结果交原owner收尾，不能发grant、移到新task或仅按PID/name判已关闭。
重试必须等原context/profile确切idle或拒绝busy，不能同名replacement继承proof。

身份查询从本批typed client/Host链读取，不直接组装UUID/Target字符串冒充proof。
打包旧worker缺scope端点时准确unavailable，既有runtime只读，不自动refresh共享seed。

## 3. App最终发布必须原子pin

**共用部分已实现**：`AccessApproval::prepare_publication`锁外 native 复验，borrowed
`commit_for_transport`只提交authority；`task_access::approve`必需携带publication，
`session_manager/computer_use/task_publication`与ACP `TaskApprovalOwner`同时pin原
slot/process/agent_session/connection/RequestTransport。Desktop直接selection/picker和U05
恢复均复用该边界，Core及默认App replacement/foreign handle回归已通过。
managed producer继续复用，不另造重复pin；不在SessionManager锁下spawn/native/HTTP/join。
preference持有者、explicit enable revision及Broker feature epoch沿原首用事务，取消时不
回滚后来的用户选择或其它任务已启用的设置。拒绝/失败留原request可重试而无新增执行效果。
原Desktop批准终态缺口已补；managed资源准备仍待第1、2节。不新增另一个prompt或按tool逐次请求授权。

## 4. 第一条有意义红回归及门槛

先以实际source Node/Chromium + Core/Host的隔离owned fixture写生产constructor链：

1. pending request读回前后worker/profile/query/input计数均0；model字段不能调用producer。
2. synthetic明确用户managed选择后只有一份reservation、一份独立context及实际proof；
   在最终commit前无execution grant/continuation/网页内容/输入。
3. 最终同一run可fresh observe；legacy/另一task/run/profile/tab证明均在内容请求前拒绝。
4. 暂停/Stop、spawn与proof/prepare/commit阶段交错、waiter Drop、feature-off、transport
   replacement、deadline及同名context重建；旧结果不能publish，所有原owner确切终态。
5. 最终commit与ACP/session replacement确定性交错：replacement胜出时不出现grant；
   old committed callback不能发到新session。不能只验证UI收到error。
6. 同gesture双击不重复spawn；失败retry用全新run/context；所有依赖旧proof/ref/snapshot
   失效；new tab/popup策略未实现时不可扩大scope。

保留原red日志，再修实现转绿。fixture的Host决定必须标synthetic，不冒称真人/installed。
source worker与严格默认App native client链分别验证；不能拿纯recording worker绿替代。

## 5. UI接线及后续验收顺序

后端constructor/取消/原子publication全绿后才扩App TaskScopeTarget和picker capability。
UI复用共享task controller/chrome，明确“独立任务浏览器”范围，短busy/error/不可用文案及
15locale；准备中Stop可用，不要求用户先去设置授权，挂载不自动准备或执行。
任何新policy控件完成逻辑/视觉/能力一致性，不长App/AppWorkbench，不用native select。
隔离实际UI用production组件；真人App/OS授权由用户完成，不能自动点击。

最终Core两种feature、默认App CU/ACP/routing、probe strict、受影响frontend与真实
worker合同、fmt/type/lint/quality/build、源码及artifact冻结；集合重叠不相加。
原build/test/browser退出后清精确一次性产物/旧incremental，保留最新代、包、seed及证据。

随后继续完整U04其它surface/关联dialog/跨surface/增量与缩小，U05其它surface恢复，
U06–U09及D00–D31三平台真实App/native/normalGrok/全CI/签名安装/12h/完整UI UX。
本批managed身份基础和本稿不构成这些未完成项的替代验收。

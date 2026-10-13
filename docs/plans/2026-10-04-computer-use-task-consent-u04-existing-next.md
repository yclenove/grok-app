# U04 ExistingTab：后端身份基础之后的执行顺序

状态：**next implementation plan / full Computer Use active / partial — not releasable**。
工作区 `H:/aicoding/grok-app-computer-use`，分支 `feat/computer-use-implementation`。
单 Codex writer；原 dirty/untracked/`fix.patch` 保留，不启动 Grok/子代理，不自行提交、
push、PR 或发布。不读凭据、不改代理/共享 `~/.grok`，不操作用户日常浏览器或自动批准
真实 App/系统/Share 权限。每批按 AGENTS 第9条清理已退出 owner 的精确临时产物。

## 最新联合进度

见[新TaskGrant实际首链checkpoint](2026-10-04-computer-use-task-consent-u04-existing-joint-checkpoint.md)。
真实MV3/认证HTTP/原App ACP/new grant/MCP/独立DOM9检查两次通过；request/Host/ACP peer仍synthetic。
浏览器最低140以满足trusted local，未改managed130 seed；下一步原首次MCP request、完整多成员/
撤销/replacement/恢复矩阵与安装/descriptor产品入口门槛。完整CU仍active / partial — not releasable。

## 当前基础与明确缺口

真实 MV3 `SharedTabs.share` 将 main-frame 的 Chrome documentId 经 `PairingClient`、
认证 HTTP Share DTO 存入原 Host registry。Host-only opaque proof 绑定 full connection、
tab、原 share nonce、documentId/generation、browser/profile；不含 Cookie/token/URL/title，
不可从模型 JSON 构造或 serde，Debug 隐藏身份。缺 documentId 的旧 Share 仍能走 legacy，
但不能证明新 TaskGrant。原 reservation 的 preparation 已可原子借用该 proof，复用原
App/ACP publication 边界；失效/失败只归还本次借用，Stop 不关闭或取消用户 Share。

E1 已在[Host picker checkpoint](2026-10-04-computer-use-task-consent-u04-existing-picker-checkpoint.md)
完成：Core `begin_task_scope_picker` 接真实分享列表，网页不走Desktop标题过滤；10项新
回归、最终Core1031/1047及默认App/strict/Node通过，3152源冻结/旧缓存清理已收尾。
E2后端已在[切换与恢复checkpoint](2026-10-04-computer-use-task-consent-u04-existing-switch-checkpoint.md)
完成：23项新Core回归，最终Core1054/1070、默认App/四strict/Node/fmt/quality通过，3162源与App冻结；
原owners退出后清15组旧缓存、H空闲观察增4.51GiB，当前源码/缓存/程序/seed/fix.patch不变。
E3后端已在[首用事务checkpoint](2026-10-04-computer-use-task-consent-u04-existing-first-use-checkpoint.md)
完成：原full key/guard下off Pair/Share及reservation保活桥接，扩展readiness等待；10项新Core与4项新Node。
最终Core1064/1080、默认App CU267/1手动ignored、ACP167/routing38、Node177/协议38、四strict及fmt/quality通过。
3166源/最终App冻结；原owners退出后清15组旧缓存，H空闲观察增5.96GiB，current/source/seed不变。
E4a原App命令现已接通，见[原App准备checkpoint](2026-10-04-computer-use-task-consent-u04-existing-app-checkpoint.md)。
3169源冻结，Core1065/1081、ACP174/routing38、Node177/协议38及四strict/fmt/quality通过；
完整App CU为276/1 failed/1手动ignored，原managed native close21530ms超20秒等待，根因未证明。
新15组cache清理首次被活跃Rust owner安全挡下；owners退出、精确pairs/source/current hashes
重验后第二次执行已完成，H空闲观察增5.77GiB，当前产物/源码/失败现场不变；已执行清单不能复用。
E4a.1最新见[原空结果重试与关闭时序](2026-10-04-computer-use-task-consent-u04-existing-empty-retry-checkpoint.md)：
已补原process/目录/settle/consumer时序，但自然长尾根因仍开放；readiness→discover空结果
已可原permit/key/guard/TTL重试，新增3Core/3App回归。最终Core1068/1084、App CU280/0 failed/
1手动ignored、ACP177/routing38、四strict/Node28/fmt/quality通过，3171源冻结；H/C新清5.64/1.35GiB。
E4b.1历史见[共享任务准备UI](2026-10-04-computer-use-task-consent-u04-existing-ui-checkpoint.md)：
API/共享controller/可见observer/准备及原picker重试/15locale已接，最终前端8062项通过；
E4b.2最新见[内置扩展导出与慢交互](2026-10-04-computer-use-task-consent-u04-existing-extension-checkpoint.md)：
44生产文件/15locale内置、Host-owned显式导出/no-replace/复制焦点/慢准备与原owner生命周期已补。
前端8107、浏览器153、Node183、fresh App native12/CU292（1手动ignored）/ACP177/routing38及两种strict通过。
Share/Host信号仍synthetic；继续[新TaskGrant联合路径](2026-10-04-computer-use-task-consent-u04-existing-joint-next.md)，
不能借legacy mcp-open。descriptor仍关闭，不重做E1–E3，自然首败与全部交付范围保留。

**仍不是 ExistingTab 完整任务体验**：App `TaskScopeTarget::parse`仍 Desktop-only（有意fail-closed），
原picker命令已支持existing-tabs，共享准备UI/15locale已接但descriptor仍未开放；配对/Share 的生产 Broker
endpoints已可在原Host ExistingTab preparation下提供off metadata；通用配对及执行仍拒绝。
Core多成员inactive切换与原Host恢复已有producer，
App task-bound准备查询/确认及E4b.1 UI/locale、E4b.2内置导出/慢准备UX已接；
真人安装/原任务真实Share与新TaskGrant联合路径仍待完成，descriptor保持关闭。

既有五种 typed actions（click/set_value/type_text/scroll/wait）已经实现，不重做。
key/drag/coordinate/clipboard/eval 仍按真实能力 unavailable，不能顺手扩大权限。

## E1：真实分享列表的 Host picker（不开放 UI）

状态：**backend verified**，实际范围/最终源码和证据见上述E1 checkpoint。
`select` 只复制原opaque receipt；权威proof复验在原owned preparation/publication，
不在App registry锁内新增阻塞native查询。下面为已执行门槛，后续不能重做或扩大本批结论。

1. 在原 `HostScopePickerCall`/selection permit 上接 ExistingTab discovery，候选仅来自
   authenticated shared registry，不枚举日常全部 tabs，不读取网页正文/截图、不先借用。
   不让模型 UUID、当前 focus/title 或 legacy picker 字符串构造 NativeScopeIdentity。
2. 保留原 full request key、feature epoch、selection revision、deadline/cancel、executor
   generation 和候选 bounded limit。候选 UUID 只在原 picker 内可用，receipt 留在 Host。
3. `skip_desktop_target` 只作用于 Desktop；网页标题不能成为窗口安全过滤规则的替代品。
   ExistingTab title 为空时仍能显示 browser fallback，metadata redaction/长度上限不变。
4. Picker discover 与 choose 前后复验原 proof；reload/同 document 重新 Share/连接替换/
   过期/他人借用都拒绝旧 receipt。取消/Stop/Drop 在 discover、choose、reserve 各切点
   不丢原 owner；仅 metadata 的 picker 不创建执行 run 或 grant。

验收：在规范 request schema 下先证明原 Desktop-only gate 的实际断言失败，再实现；
原 Core picker API + authenticated HTTP Share 联合证明 discover→choose→原 reservation→
commit→Stop。fixture gesture/agent 明示 synthetic，不将 unit 或 legacy grant 冒称真人链。

## E2：多成员、切换与恢复（先完成才能放开多选）

状态：**backend verified**，实际范围/最终源码和证据见上述E2 checkpoint。
以下为已执行合同，不能冒称App/真人MV3完整体验已验收。

1. 原 Bound 保留每个被明确选择的 share proof；无选择的页面不可读/不可借用。不得借
   legacy run 创建资源再迁移到新任务；请求 JSON 中的 tab/run ID 不能获得生产准入。
2. inactive member 切入时，从原 Bound 找其 native proof，通过同 registry lock 的
   `acquire_scope_tab` 借用。沿原 task/executor/revision 的 scope switch publication，
   不是直接把 raw `tab:` 送到只认已借用资源的旧路径。
3. 最终切换提交前 Stop/replacement/reshare 胜出时只归还本次新借用；成功提交后才归还
   原 active tab。不能释放 foreign owner、关闭 tab、取消用户 Share 或复活旧观察。
4. 每次切换需要 fresh model observe，旧 ref/snapshot/actionId 不能跨成员复用；已经
   批准且 proof 未变的成员不按工具重新询问，新增范围或 document 换代必须显式决定。
5. U05 恢复复用原 Host/App/ACP publication，不把 Pause 后的旧 grant 直接恢复。
   proof 不符准确 unavailable；失败保留安全可重试状态，不暗选同名页面。

验收：至少两份真实 shares，多成员 bound、A→B→A、未选 C 拒绝、B 被 foreign 借用、
reshare/reload/connection replacement、切换 preparation/commit 与 Stop 交错、恢复失败。
每条同时核对实际 grant、原 TabInfo、内容/注入请求计数和清理 owner，不只检查 UI error。

## E3：默认关闭时的首次 Pair + Share（先设计事务，再接线）

状态：**backend verified**，上述checkpoint及[小ADR](2026-10-04-computer-use-task-consent-u04-existing-first-use-adr.md)
说明最终合同。以下保留设计门槛；App入口/真人整链仍待E4/E5，不重做本批。

不能简单删除 pairing/share feature gate、默认启用产品、自动配对或要求先去设置。
原 off 单元夹具直接调用 Host，不证明生产首用能走通。

先写一个明确小 ADR，回答以下问题并落到原 task/full-key 生命周期：

- 用户提出任务后，明确选择 ExistingTab 才进入一次非执行配对/分享准备；挂载、status
  或模型 request 不启动浏览器、不读页面、不发布 feature 或 execution grant。
- 若需在 feature-off 提供准备通道，该通道如何绑定原请求、原 challenge/connection，
  有界失效/取消并只允许 pairing/share metadata；不能令通用 HTTP 或模型工具绕 gate。
- 配对是设备/扩展连接级步骤，Share 是浏览器要求的 activeTab gesture；界面说明只在
  必要时出现，不把它们伪装成每次模型动作授权，也不能自动点击真实权限或 Share。
- 原批准的最终 feature preference + task grant + 原 App/ACP commit 继续共用事务。
  取消/拒绝/Stop 只退休本次拥有的准备资源，不撤销另一任务已有的连接/Share/设置。
- 安装或版本不兼容准确给出短原因、安装/重试入口；缺 document proof 不暗降级。

先有正式 Broker/HTTP first-use 负例和取消矩阵，再放宽受原事务约束的准备路径；
不能以直接 Host fixture 成功替代生产入口证据。

## E4：App 命令、共享 UI 与15语言

后端 E1–E3 与 publication 全绿后才扩 App parser、descriptor 和共享 task controller。
复用 `ComputerTaskConsent`/card/panel、项目 modal/Select/chrome，不长 App/AppWorkbench。
选择窗名称应跟 surface 一致，不用“选择窗口”泛指 tabs。只列真实已分享且可证明的
候选；显示短空态/版本不符/忙/error/retry，不堆说明文字，不输出 host credentials。

准备中 Cancel 与高优先 Stop 可用；重复手势不重复 reservation；切卡片/隐藏面板不丢
owner，旧请求结果不闪回。模块共享同一次用户批准，卡片和面板不能各发一个批准。
全部新增 copy 在15locale同步，以 en 为 authority；不造无后端的开关/按钮。

## E5：同最终候选的联合验收和收尾

1. Core default/test-support **all-targets**、两种 strict Clippy；默认 App 重建并按 CI 嵌
   Common Controls manifest，CU/ACP/routing 与 probe strict；受影响 Node/前端、fmt/
   type/lint/quality/build/协议与bundle检查。按原日志统计，各重叠过滤集合不相加。
2. Owned source Chromium + production sw.js + authenticated HTTP + 原 task reservation
   + App/ACP pin 做完整路径；既有 `existing-mcp-live` 的 `mcp-open` 是 legacy grant，
   不能把它的绿写成新 TaskGrant 验证，不另造测试批准系统。
3. User App/toolbar/系统权限的真人验收由用户操作；未实际运行标 not_run。源码 Chrome
   不等于 Edge、installed、macOS/Linux 或真实 Grok 验收。
4. 冻结包含 extension 的最终 source manifest 和执行 artifact；所有原 owners 确切退出
   后才审查 old/current exact cache pairs，先 inspection-only，再删除，保留当前包、
   immutable seed、依赖、失败现场和报告。清理后源码/current artifacts hashes不变。
5. 具名 checkpoint + latest 入口说明真实边界，再继续 WebView/其它 U04、U05/U06完整
   矩阵、U07–U09、D00–D31、Windows/macOS/Linux、真实 App/Grok、签名安装、同最终
   候选12h与整体 UX。完整范围不缩减，局部绿不标全目标完成。

# Grok U04：真实范围绑定、多目标切换与增量授权

状态：**Codex implementation in progress**。先落地必要核心回归与范围合同，不代表完整U04已交付。
2026-10-04 最新：[新TaskGrant真实联合路径首链](2026-10-04-computer-use-task-consent-u04-existing-joint-checkpoint.md)。
真实MV3/认证HTTP/原App ACP/new grant/MCP/DOM9检查两次通过；Host决定/request/ACP peer仍synthetic。
实测兼容性修正为Chrome/Edge140+，trusted local不降级；完整E5/多成员/撤销/恢复及交付仍开放。
source13/build15、本批回归及精准清理见checkpoint；descriptor仍关闭，单writer、未提交发布。

以下为E4b.2历史，不是当前最新结论。

2026-10-04 历史：[内置扩展导出与慢准备交互（E4b.2）](2026-10-04-computer-use-task-consent-u04-existing-extension-checkpoint.md)。
44个生产文件/15locale内置，Host-owned显式导出、no-replace、慢metadata/queued confirm、复制焦点及原owner生命周期已补。
最终前端707文件/8107项、浏览器153场景、Node183项、fresh默认App native12/CU292（1手动ignored）/ACP177/routing38及两种strict通过；最终source/artifact/首败/清理以checkpoint为准。
Share/Host/visibility仍synthetic，production descriptor保持关闭；按[联合路径下一批](2026-10-04-computer-use-task-consent-u04-existing-joint-next.md)接真实MV3/认证HTTP/原App ACP/新TaskGrant，不借legacy mcp-open。
原自然close长尾、本轮Node unknown/App首败根因及全U D/四surface/三平台/签名/12h/UX仍开放；Core本批未改/未全量重跑。
active / partial — not releasable，单writer、未提交发布；不用已执行清单重复清理。

以下为E4a.1历史，不是当前最新结论。

2026-10-04 历史：[ExistingTab空结果原任务重试（E4a.1）](2026-10-04-computer-use-task-consent-u04-existing-empty-retry-checkpoint.md)。
原readiness/discover空receipt固定先红后修；只转移空结果原permit/key/guard/TTL，不替换非空ID或新建授权。
3171源冻结，Core1068/1084、App CU280/0 failed/1手动ignored、ACP177/routing38、Node28及四strict/fmt/quality通过。
关闭原process/目录清理/consumer时序已补，自然21530ms长尾根因仍开放；首败未擦除。新15代cache和4份runtime已清，H/C观察增5.64/1.35GiB。
下一步E4b共享UI/15locale/真实重试路径，再E5联合链；全部U D/四surface/三平台/签名/12h/UX继续，active / partial — not releasable，未提交发布。

以下为E4a历史，不是当前最新结论。

2026-10-04 历史：[ExistingTab原App任务准备入口（E4a）](2026-10-04-computer-use-task-consent-u04-existing-app-checkpoint.md)。
原App picker接existing-tabs、task-bound准备查询/显式confirm与原process pin；raw parser仍Desktop-only，UI/descriptor未开放。
3169源/默认App冻结，Core1065/1081、ACP174/routing38、Node177/协议38、四strict/fmt/quality通过。
完整App CU为276 passed/1 failed/1手动ignored，managed native close21530ms超20秒等待；root cause未证明，不写全绿。
先核原物理收束→consumer→Stopped，再E4b共享UI/竞争/15locale及E5联合链；15组新cache清理首次被活跃Rust owner安全挡下，退出后重验已清，H空闲观察增5.77GiB，当前产物/源码/失败现场不变。
全部U D/四surface/三平台/签名/12h/UX继续，active / partial — not releasable，未提交发布。

以下为E3历史，不是当前最新结论。

2026-10-04 历史：[ExistingTab默认关闭首次Pair/Share（E3）](2026-10-04-computer-use-task-consent-u04-existing-first-use-checkpoint.md)。
原picker full key/guard下off metadata、显式Pair/Share及原reservation保活桥接；扩展等readiness再启动执行loop。
10项新Core/4项新Node，最终Core1064/1080、默认App CU267/1手动ignored、ACP167/routing38、Node177/协议38及四strict/fmt/quality通过。
3166源/最终App冻结；原owners退出后清15组旧缓存，H空闲观察增5.96GiB，current/source/App/Core/seed/fix.patch不变。
App parser/picker仍Desktop-only；下一步E4原App命令/共享UI/15locale→E5同候选联合链，不重做E1–E3。
仅backend/synthetic用户与peer，不是真人MV3/App/Grok/installed整链；全U D/四surface/三平台/签名/12h/UX仍开，active / partial — not releasable，未提交发布。

以下为E2历史，不是当前最新结论。

2026-10-04 历史：[ExistingTab多成员切换与原Host恢复（E2）](2026-10-04-computer-use-task-consent-u04-existing-switch-checkpoint.md)。
原Bound proof精确借用/切换/回滚，Pause保留active、fresh capture后原Host恢复；Stop提前终态与失败切换后Pause/Continue不可用已修。
23项新Core回归；最终Core1054/1070、默认App CU267/1手动ignored、ACP167/routing38、Node173/协议38及四种strict/fmt/quality通过。
3162源含extension/最终App冻结；原owners退出后清15组旧缓存逻辑6.28GiB，H空闲观察增加4.51GiB，source/current/App/Core/seed/fix.patch不变。
App parser/picker仍Desktop-only；下一步E3 off首次Pair+Share事务→E4共享UI/15locale→E5同候选联合链。
仅backend/synthetic peer，不是真人MV3/App/Grok/installed整链；全U D/四surface/三平台/签名/12h/UX仍开，active / partial — not releasable，单writer、未提交发布。

以下为E1 picker历史，不是当前最新结论。

2026-10-04历史：[ExistingTab原分享Host picker（E1）](2026-10-04-computer-use-task-consent-u04-existing-picker-checkpoint.md)。
Core picker已支持ExistingTab、网页不走Desktop标题过滤，10项新回归；最终Core1031/1047、App CU267/1手动ignored、ACP167/routing38、Node173/协议38及strict通过。
3152源含extension与最终App冻结；清15组旧缓存逻辑6.32GiB、H空闲端到端观察增约3.66GiB；首清理中断/新plan完成均保留，source/current/seed不变。
App parser/picker仍Desktop-only，按E2→E3→E4→E5继续；不是真人MV3/App/Grok整链，全U D/四surface/三平台/签名/12h/UX仍开，未提交发布。
2026-10-04历史：[ExistingTab后端scope基础](2026-10-04-computer-use-task-consent-u04-existing-scope-checkpoint.md)。
source Share真实document proof、原reservation acquire/rollback/Stop归还及17新Core回归已验；最终Core1021/1037、App CU267/1手动ignored、ACP167/routing38、Node173与strict通过。
3151源含extension冻结，原owners退出后清旧缓存H观察空闲增5.97GiB。仅backend，App/Core picker仍Desktop-only，off首用Pair+Share及inactive switch未接。
后续按[ExistingTab E1–E5顺序](2026-10-04-computer-use-task-consent-u04-existing-next.md)；下方managed待实现等描述为历史基线，已有constructor/publication/UI应复用，不能重做或借legacy。
2026-10-04最新：[共用原子批准发布](2026-10-04-computer-use-task-consent-u04-approval-publication-checkpoint.md)
已完成原App slot/process/agent/ACP/RequestTransport pin及锁外native准备，直接selection/picker
和U05恢复复用；Core两feature、默认App过滤集合、全前端及strict通过。3063源码冻结，
原owners退出后清五代旧缓存，H实际空闲增加3.01GiB。managed producer/UI仍未实现。
2026-10-04增量：[ManagedBrowser身份基础](2026-10-04-computer-use-task-consent-u04-managed-identity-checkpoint.md)
已完成真实worker/context/page证明及默认App native链；生产task producer/批准/UI仍未开放。
下一步按[原reservation资源准备方案](2026-10-04-computer-use-task-consent-u04-managed-producer-next.md)，
解决Bound→run→owned tab依赖循环，复用已完成原ACP/session原子publication；不得借legacy run或synthetic proof。
最新U03卡/面板与Host retained display name已接线，见[UI checkpoint](2026-10-03-computer-use-task-consent-u03-ui-checkpoint.md)。
每个显式picker选中成员的可读名称已保留；title不参与authority equality，读回不查询原生。
下方“卡片尚未接线”是历史基线；四surface/三平台、真实整组执行、增量及缩小仍需完成。
已实现批准清单、按 surface 准备、明确 selector、prepared generation、同请求重试、Windows 单根选择回执、整组 Host 回执/commit、Desktop 范围内切换核心与按精确实例读取目标名称；当前自动化
证据见 `2026-10-03-computer-use-task-consent-u04-checkpoint.md`。后续 App 明确选中目标→原 ACP transport→生产 HostScopeSelection→已有首用批准事务已接线，修复生产 HostOwnedAdapter 精确身份/metadata 转发；取消/重连/双击回归和最终证据见 `2026-10-03-computer-use-task-consent-u04-app-scope-checkpoint.md`。私有非交互 Windows station 的原生组失败/身份退休已验；聊天请求卡、可见窗口的真实 App/native 整组执行链、跨 surface 切换及增量/缩小仍在继续。
依赖：U01核心及U02会话接线/normal CLI无害闭环已完成，见对应Codex checkpoint；生产App联合流程仍待后续。
U04提供U03批准按钮必需的生产Bound/范围事务，实际请求卡在U04之后接线与联合验收。
用户已要求Codex接手，单Codex writer；不启动Grok、子代理或第二writer，不自行提交/发布。

## 用户结果与本批边界

同一任务可以一次批准一组具体应用实例或浏览器范围，在范围内切换不用重复批准。
需要新增范围时只申请新增部分；拒绝、取消或准备失败保留原范围。缩小范围即时撤销
被移除目标的执行能力，旧回包不能加回。暂停恢复留U05，但本批提供真实且不可执行的
批准范围记忆，不能以pending intent或标题字符串替代它。

U01的synthetic Bound helper只供明确标注的测试；本批接生产Host构造器。
构造器只允许用户请求卡/范围管理的真实Host事务调用，模型JSON不能构造批准身份。
先与U02的可信Host决定入口、现有范围picker接线；U03随后增加聊天卡手势。
没有卡片还不构成完整按需授权体验，不能提前写UI验收通过或自动授予模型权限。
Windows/macOS/Linux和四surface的合同一起设计/实现，实际平台验收逐行标记。
无法安全证明某平台细范围时准确unavailable，不授予整个桌面来假装应用隔离已实现。

先全文读U04总体计划、已接收checkpoint/ADR、AGENTS/CONTRIBUTING及相关wiki。
此稿的源码入口以2026-10-03查到的符号为依据；先rg核当前路径，禁止猜已存在API。

## 0. 当前入口与第一条红回归

- `computer-use-core/src/tools.rs`中list只返回当前authorized target，open只接受同target。
- `broker.rs::authorize_target/authorize_on_surface`与`broker/managed_authorization.rs`
  是现有单目标绑定；`reject_cross_surface`不允许用同run偷偷跨surface。
- `broker/target_readback.rs::authorized_target_info`从retained executor读取，已有迟到复验。
- `src/commands/computer_use.rs::computer_use_picker_targets_with_mode`与
  `computer_use_authorize_surface`、`computer_use/sessions.rs`是已有Host授权准备/收尾。
- `windows_adapter/native_target.rs`严格绑定HWND/PID/TID/stamp与原root；
  `macos_adapter/identity.rs`绑定process birth；两者不能退化成名称或PID匹配。
- `computer-use-core/src/browser/{managed_adapter,existing_adapter}.rs`、
  `computer_use/webview.rs`及`webview/lifecycle.rs`分别保留浏览器与view owner。

先写实际严格transport→Broker→owned adapter的两目标回归，用户合成决定明确synthetic。
批准A和B，实际切换A→B→A并fresh observe/动作，原fixture分别计数；旧单目标路径应红。
拒绝范围外C的读取/输入另立反例，不能仅测试UI显示两个名字。保留实际先红输出。

## 1. 生产Host范围准备与最终commit

1. UI决定核当前app/agent/incarnation/connection/request/task/revision/epoch与所选范围。
   复用U01已接收的两阶段事务；不得新建另一套grant或在点击时先写approved。
2. 默认已明确的owned范围可直接准备；歧义仅出现一次必要picker并保持同请求事务。
   用户未操作卡时不枚举标题/URL/AX/UIA/截图。显式展开选择只读取本地Host所需候选
   名称，不把全部候选、正文或截图发送模型，也不建立execution grant。
3. 每个候选包含真正平台身份、executor/runtime owner与generation；选择回执有revision。
   候选旧行、同名replacement、picker取消或权限拒绝不可commit。临时资源有确切原owner。
4. 外部权限/Share请求不长持Broker锁；Stop/feature-off可取消准备，晚回包由原owner收尾。
   最终commit后才生成批准范围记录和U02 typed continuation，重复提交只成功一次。
5. 旧面板手动授权、preview、旧browser工具也必须经过同一范围合同，不能成为旁路。
   不把legacy授权run自动加入当前任务；兼容路径保留自身原边界。

## 2. 四类membership，分清实例与文档

| Surface | Host批准对象 | 每次使用的复验 |
| --- | --- | --- |
| Desktop | 一组具体应用实例及明确授权根 | process birth/native object/root身份；关联dialog的真实owner关系；平台权限 |
| ManagedBrowser | 本任务owned profile/context及明确new-tab/popup策略 | context/profile owner、tab实例、document generation；不能读用户日常profile |
| ExistingTab | 已配对并真实分享的browser/tab实例，加App task范围 | pairing/Share/activeTab/document；新tab与失效权限不能假装继续有效 |
| WebView | 明确view label与当前instance | view owner/instance与document；相同label replacement不继承 |

Desktop的同名应用、同路径程序、父PID、标题相似不能独立证明membership。
关联dialog必须验证属于批准实例/授权根；无法证明时按需增量申请，拒绝parent spoof。
macOS birth并非window instance witness，Windows stamp并非process birth witness；
Linux X11/native Wayland按真实SDK/portal合同处理，不用XWayland或monitor冒充细应用范围。

Managed普通导航不重批，但旧snapshot/ref/pageGeneration立即失效。new tab/popup需满足
已显示给用户的策略；profile/worker/context replacement不继承。不得隐式跨其它session。
ExistingTab失去activeTab或Share时显示必要真实工具栏手势；不能自动点击或扩大权限。
WebView同实例导航fresh observe后继续；different view/instance需要新增授权。

## 3. 范围内切换是执行事务

模型list仅返回已批准且仍有效的成员；不存在grant时没有ambient target发现。
open_target只选择范围内身份，不承担authorize/resume；范围外返回结构化needs_access，
不先读取该目标来判断是否适合。错误不泄露其它session/范围外内容。

切换先退休旧snapshot/ref/geometry并取消原目标工作，等待原admission/cleanup终态，
再取得必要lease和新目标绑定；不得并存两个desktop input owner或抢另一session的lease。
Host锁只做权威修改，不持锁等待原生/worker查询。切换中Stop有界fence，迟到新绑定不发布。
保留TaskGrant，ExecutionBinding重新生成；第一次fresh observe完成前没有新input。
跨surface使用明确子binding/owned run设计，不简单移除reject_cross_surface绕现有资源合同。

动作/Wait/navigation/download/upload最终dispatch仍携带当前任务grant revision/epoch。
preview不能替换模型snapshot；旧ref在切换前后均拒绝，unknown动作先观察真实结果，不重放。
范围内focus/activation若平台需要，也必须经membership/fence；物理接管后模型不能抢焦点。

## 4. 增量与缩小，不破坏原授权

提交B时原commit A保持可执行；卡片只展示新增范围/能力，唯一pending request修订可回读。
B的pending/deny/cancel/expire/bind失败都没有B效果，也不取消A或吞掉A的有效续行票据。
批准B时原子更新任务scope/grant revision与对应execution admission；旧revision不可执行。
同请求重复事件与双击只一份增量commit，不为每个模型tool创建新task。

用户缩小范围先fence被移除目标、撤销其queued continuation/prepare/action admission；
物理收尾归原owner。剩余成员保留，新成员不能继承被删除目标的snapshot或scope memory。
明确区分申请历史与批准范围记忆；scope memory只记最终commit的真实Bound身份，
无bearer/正文/Cookie，不从denied/pending intent生成“已批准”记录。

## 5. 验证、交审与下一批

先有意义红→绿，再受影响core/IPC/Host/adapter/协议与UI接线检查；旧授权/Stop负向保留。
S10–S18/S22/S25/S27至少覆盖：两个owned应用五次切换、真实关联dialog及伪造关系、
PID/HWND/tab/view replacement、第三范围批准与拒绝、执行中缩小、Stop交错、lease冲突、
managed导航/tab/popup、ExistingTab失权与WebView导航。实际效果/未授权读取/输入独立计数。

Windows可用的owned原生fixture与真实managed worker做本机纵向链；生产Host决定测试为
synthetic，不能冒称真人或installed App。其它OS编译/fixture/native分别记录证据等级。
无系统批准不得自动点击；缺机器/TCC/portal/toolbar手势只阻断对应实际验收，继续ready项。
所有原child/profile/tab/listener/lease按原精确owner收尾，borrowed tab保持打开。

交`2026-10-03-computer-use-task-consent-u04-checkpoint.md`：逐合同实现/验证分开，
实际命令/退出码/source-runtime hash、先红与业务效果、原资源terminal/join、能力矩阵、
U05恢复所需已冻结的批准范围记忆/复验API。未验项不得写通过。
Codex接收后派U03完整卡片，再派U05；完整U00–U09/D00–D31不缩减，
不据scope fixture绿宣布全CU交付。

不commit/push/PR/merge/tag/release/reset/restore/clean/stash/rebase；不触凭据、共享~/.grok、
代理/VPN/用户profile/其它worktree或日常进程，不改变设备权限来迎合测试。

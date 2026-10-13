# ExistingTab：原 screenshot / 五动作的迟到结果合同（R5）

2026-10-05最新进度见[source80检查点](2026-10-05-computer-use-task-consent-u04-existing-capture-checkpoint.md)：
五动作20行和独立原App截图拒绝均已取得定点通过；独立CFT154原action后的真实PNG前置条件也已验证。
原session22543完整回归运行中，冻结源码；其终态与owners收束后按
[真实成功截图迟到合同](2026-10-05-computer-use-task-consent-u04-existing-late-capture-next.md)继续。
以下保留原R5合同与首次只读研究记录；API前置探针不是App/TaskGrant/真人成功截图或完整R5验收。

这是R4原owner替换之后的独立小批。先核对
[R4检查点](2026-10-05-computer-use-task-consent-u04-existing-recovery-owner-checkpoint.md)
的实际终态与清理；原build/test/browser owners退出后才修改冻结源码。
单Codex writer，不启动Grok/子代理，不提交/push/PR/发布。
dirty/untracked、fix.patch、全部首次失败与现场保留；active / partial — not releasable。
ExistingTab descriptor仍关闭，raw parser仍Desktop-only，完整交付范围保留。

## 准确的能力与原调用链

本surface的五项为`click`、`set_value`、`type_text`、`scroll`、`wait`。
`wait`是语义条件读取；其零DOM输入不等于没有原native执行。
`key`、`drag`、坐标与任意evaluate仍不支持，不扩大能力凑矩阵。

复用首次真实session MCP request/source MV3 Pair/三Share/App picker选A/B未选C、
新TaskGrant/原ACP首次delivery的联合fixture；不借legacy toolbar grant。
每项先取原fresh正文/refs，再固定原run/key、transport、tab/document、snapshot、
actionId、真实原extension action request/operationId和completion scope。
每个动作/切点/中断使用独立owned fixture，不能在已被Stop的同一run补新批准伪造旧结果。

现有生产链是Broker permit/action ledger → 原authenticated HTTP action transport →
`ActionCompletions.#execute`原claim/原scope/watcher → `SharedTabs.act`原preflight →
document-bound `actDocument` → 原completion/settle/receipt。
恢复竞争用的`observeDocument`返回门闩不能冒充输入门闩。

## 先实现一个click，再扩五项

### A：真实preflight返回先被持有，invoke之前退休

在原action已经由原scope claim、原`SharedTabs.act`进行当前document preflight时，
持有这一项原`inspectDocument`的真实返回。验证原tab/document、唯一matched调用、
原action仍pending，而且该action的`actDocument`从未invoke、DOM无效果。
其它inspect/status/取消脚本不可进入门闩。若现有instrumentation不能把preflight
精确关联到这一项原action，先保存反例；使用绑定原claim/operation的test-only切点，
不能用不相关tab query或任意延迟代替这个证据。

在held状态调用真实Host Pause，证明原同步权限fence已生效。交回原preflight结果，
自然join原action/中断/cleanup owner；原live复验应在`actDocument`invoke前拒绝。
精确固定实际拒绝/ledger状态，零click/input/scroll变化、零该action的native invoke。
另一个fixture再做真实Chat Stop的同步fence，最终原run实际Stopped。

若Host中断的异步收尾需等原action，private RPC只启动并保留其原waiter，
查询真实fence状态后释放门闩，再自然await原waiter。不能等待一个依赖尚未released
Promise的中断完成而制造fixture死锁；不能abort/移走worker或改预算解决它。

### B：真实actDocument已返回，效果已经发生，原结果迟到

只对原tab/document、精确operationId/snapshot/动作的这一项`actDocument`，
先执行同一原`chrome.scripting.executeScript`并取得真实rows，再持有返回。
不换函数、参数、rows或DOM，不让其它act/cancelDocumentOperation进入门闩。
独立原page先证明已经发生的真实效果：click恰好1次、文本为实际目标值、
scroll到实际位置；wait则证明真实匹配/等待结果和零输入，不能算成click效果。

再分别进入真实Pause/Chat Stop。保持原native Promise、原scope/watcher、原
cancellation/settle ownership；释放同一rows，自然join原调用及全部cleanup。
生产返回按实际合同是退役/Unknown，不能把已发生效果称为`NotStarted`或零执行，
也不能回滚DOM来刷零。拒绝旧权限发布、新fresh refs或新available continuation；
原actionId/operation和旧refs的重放必须不再产生第二次效果。
存在独立cancelDocumentOperation时须join它和原act Promise，不能只凭cancel ACK
或`physicallySettled`布尔声称原执行结束。

### C：五项的固定后置条件

click两切点定点完成后依次扩展set_value、type_text、scroll、wait，各自独立fixture。
文本使用既有中文fixture，分别固定目标value与实际input/change计数，不把DOM语义输入称为
OS trusted key或中文IME。wait是条件读取；pre-invoke退休不得读新的目标，post-invoke
不发布旧verified成功为新authority，也不重启旧wait。

扩展时按现有`act-document.mjs`的实际合同区分动作，不把click的`applied`断言直接复制到其它行：

| 动作 | 原fixture引用 / 参数 | post持有的真实原rows | 已发生效果与事件 |
| --- | --- | --- | --- |
| click | Fixture action / `{}` | applied / semantic_click_dispatched | click=1；输入值为空、文本事件与scroll=0 |
| set_value | Fixture input / 中文text | verified / field_value_changed | 目标值匹配；beforeinput=1、input=1、change=1，inputType=insertReplacementText |
| type_text | Fixture input / 中文text | verified / field_value_changed | 目标值匹配；beforeinput=1、input=1、change=0，inputType=insertText；不伪造提交change |
| scroll | Fixture scroll / delta=60 | verified / scroll_position_changed | 原scrollTop=60；click与文本事件=0 |
| wait | Fixture action / nameEquals=Fixture action | verified / condition_matched | 原actDocument确实invoke/返回一次；DOM输入、click、scroll均0 |

pre各行原actDocument invoke=0，原页面值、事件与滚动均不变；post各行invoke=1。
两个切点都核对原scope、action/completion/cancel Promise与Host waiter自然join，
但`nativeActions`计数和DOM输入计数必须分列，尤其不能将wait的invoke计成一次输入。
在post门闩进入后、开始中断前，独立原page验证上述真实后置条件；中断后不还原页面刷零。
原rows的status/detail只由原executeScript读取，不能由期望值合成。

实现顺序为set_value四行→type_text四行→scroll四行→wait四行；每行独立owned fixture，
新Rust测试放在`existing_joint/late_actions.rs`或其子模块，不让993行的
`existing_joint.rs`突破1000。场景解析只接受五动作×两切点×两中断的固定集合，
未识别动作或其它后缀必须拒绝。复用原门闩与原finish ledger，不新增生产权限或manifest。
fixture只增加被动事件计数；A/B/C未选隔离检查在held、join、replay、Continue后分别保留。
新候选包含Rust测试变化时必须生成新的默认App artifact，不能沿用仅probe变化时的75映射。

每种Pause/Chat Stop在两个切点的完整矩阵为5×2×2个独立fixture。
每行同时报告：实际native invoke数量、已发生效果、原call是否join、原cancel/settle
是否join、旧claim/refs/actionId拒绝、available authority与两种历史记录数量。
不能把历史ticket/ledger数量当可执行权限，也不能清历史记录得到零重放计数。

Pair/三Share、A/B/C原document/page保留；C零读取/输入，B不因A退役取得旧结果。
原feature/store保持true；原TCP peer零新的ACP续行wire。新的显式Continue仅可在
原所有owner实际收束后开始；Stop后的旧delivery/chat_started不能复活原task。
原owner替换、真实Share撤销/lease/导航等其它故障仍按完整E5/D07/D11逐行完成，
本批通过不得把这些尚未跑的组合自动标通过。

## screenshot单列，权限缺口如实标记

先准确复测当前owned headless/source manifest无真实activeTab grant时的显式
screenshot权限拒绝：原capture实际调用/permission denial、零图像发布，不能降级为
正文成功或伪造PNG/data URL。生产manifest与浏览器权限不得为测试被放宽。

成功截图迟到必须在真实activeTab用户授权的owned浏览器中：同一原capture Promise
确实返回真实data URL后held，再退休原authority，释放后图像在解码/发布前被丢弃；
不同页面/几何/焦点也不能承接旧图。真实授予条件缺失时该成功路径为not_run。
synthetic screenshot/error或fake capture回归不能替代这条native成功证据。

### 成功截图的授权条件研究（2026-10-05，只读结论）

当前固定CFT140（branch-heads/7339）的Extensions协议没有`triggerAction`，
不能用普通popup页面打开代替浏览器action授权。官方CDP master已提供
`Extensions.triggerAction({id,targetId})`；Chromium原实现通过所给tab target找到
BrowserWindow/ExtensionsContainer，再调用`ExecuteUserAction(kCdp)`。
这是可研究的原浏览器action入口，不是已证明截图成功，也不是真人手势证据。

公共来源：[CDP协议](https://raw.githubusercontent.com/ChromeDevTools/devtools-protocol/master/json/browser_protocol.json)、
[Chromium实现](https://raw.githubusercontent.com/chromium/chromium/main/chrome/browser/devtools/protocol/extensions_handler.cc)、
[CFT发布元数据](https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json)。
本次实际读取元数据Stable=154.0.8037.92/revision1689415；这是可选探针版本，
并未下载、运行或替换固定CFT140，不将master源码自动等同该release能力。

原56119全部owners结束后，用独立owned浏览器/profile验证实际Browser.getVersion及
协议能力，保留生产manifest/extension key和原Share链。先证明action前原capture
确实权限拒绝，再调用真实action并取得同一tab的真实capture返回；报告调用来源为
CDP test-driver而非人工授权。`targetId`须是实现接受的真实tab target，不能将page
target或未知ID假设为兼容；不支持/无窗口/action/授权仍拒绝则保存原证据并not_run。
不对正在换fixture的56119浏览器调用action，不重启原测试或改变其焦点。

原截图返回后held的场景必须同时见证解码计数、原capture Promise自然join、
原Host observe/cancel/idle、原extension transport自然终态；Host拒绝最终结果并不
单独证明扩展未解码或原浏览器操作已退出。当前v1观察transport没有每request
signal，SharedTabs.observe的live以Share/document/activity/deadline为主，Host Pause
保留Pair/Share时的及时退休仍是待实测疑点。先取得真实反例，不能仅据代码阅读
宣布bug已证实、修复完成或提前放开descriptor。

## 门禁、记录、清理

原子候选fmt→新freeze→默认App artifact。先click两切点定点，再五项与两种中断；
保留每个第一次失败的原log、primary/phase、root/profile/hash，primary先于辅助诊断。
不加原HTTP/control/lease/native deadline预算，不自动重试、不删失败现场刷绿。
最终同候选完整native、宽CU/ACP/routing、App/probe strict、Node/MCP/诊断unit、
新helper syntax/fmt/quality/diff/Verify；Core/前端若改则补其对应全量门禁。
App+AppWorkbench不得增长，≥1000行文件budget仍80。

全部原owners终态后，用新cleanup身份Plan→回读精确target/current/hash→Execute。
仅清确认过期的可再生产物，保留当前build/依赖/seed、源码/fix.patch及全部首败证据。
清前后复验37旧hash/5absences、64–66、67/68、71/72及本批新增首败；
分别报告逻辑bytes和实际空闲观察变化。未知owner/working/其它worktree deferred，
不杀无关进程、不删除整个target/node_modules/workspace，每批按AGENTS第9条收尾。

随后仍是完整E5及U04–U09/D00–D31、四surface、Windows/macOS/Linux/两Mac架构、
原生输入/恢复、签名安装升级回滚、真人Grok、同候选12h及UI/UX；范围不缩减。

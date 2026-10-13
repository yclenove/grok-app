# ExistingTab：原 App/ACP owner 替换的下一小批

publication 四项已在 source46/build46 执行，实际 focused4/完整native29通过与
宽回归、启动未复现及清理见[publication owner检查点](2026-10-04-computer-use-task-consent-u04-existing-publication-owner-checkpoint.md)。
choose/reservation八项已在source48执行，focused8通过；完整native36通过/1失败，
详见[owner boundaries检查点](2026-10-04-computer-use-task-consent-u04-existing-owner-boundaries-checkpoint.md)。
source50补十二项直接input拒绝，focused12通过；完整native36通过/1项startup首败（0checks），
见[原owner输入检查点](2026-10-04-computer-use-task-consent-u04-existing-owner-input-checkpoint.md)。
最新source52及五项原矩阵首败见[启动检查点](2026-10-04-computer-use-task-consent-u04-existing-bootstrap-checkpoint.md)：
dormant1/browser focused3通过，完整native33通过/5失败、exit101；原owners已退出，精准清理完成。
下一项[原control诊断与Cancel/Stop/Drop](2026-10-05-computer-use-task-consent-u04-existing-control-terminals-next.md)；
source48观察及历史startup根因仍OPEN，恢复竞争仍待执行。以下保留source46前的设计合同，不是重复开工入口。
接[批准前自然租约竞争](2026-10-04-computer-use-task-consent-u04-existing-preapproval-lease-checkpoint.md)，
保留原首次MCP/new TaskGrant链、所有失败根、单writer和完整交付范围。

## 先做 publication 四项

各项从全新隔离场景开始：off、首次实际session MCP request、原完整key/picker、
真实source Pair/Share与认证HTTP、固定A/B且未选C。沿当前PublicationProbe进入
原App pin之前的15秒门闩，见证原owned worker与reserved run，页面读取/input为零。
门闩在App/ACP锁外，不能持锁等待替换；原pin必须保持不变。

| 固定场景 | 真正更换的对象 | 必须仍活着的原对象 |
| --- | --- | --- |
| publication-app-process-replaced | 原SessionManager slot的process_id | 原ACP/RequestTransport及browser connection |
| publication-agent-replaced | 原slot的agent_session_id | 原ACP/RequestTransport及browser connection |
| publication-acp-replaced | 原slot的ACP Arc换为另一实际ACP fixture连接 | 原ACP/RequestTransport仍alive，不用None冒充替换 |
| publication-transport-replaced | 同一ACP slot的RequestTransport Arc | 原bearer仍合法，原ACP/App endpoint不变 |

前三项App/session/agent替换仍为synthetic slot mutation；不能写成OS进程死亡或真实
Grok重连验收。ACP新fixture有实际隔离TCP peer，但peer不是Grok。第四项可把现
`TaskViewFixture::replace_approval_transport_same_identity`同实现移到transport-only
fixture复用：保留原bearer，真正插入新Arc；不用新模型request或新grant构造器。
新peer/旧peer与transport的生命周期都要保留到实际join/finish，不靠drop丢句柄。

## 读回不能跟随新 owner

开始前固定原McpEndpoint、TaskApprovalOwner、full key、picker和reserved run。
替换后SessionManager的普通task-view可能拒绝或显示替换者；它不能被当作原owner
验证，也不能为了方便把slot还原再完成旧pin。保留原RequestTransport的Host-only
只读见证；若原token被设计性退休，返回固定retired分类，不能借新token补读旧权限。
浏览器connection/document/Share proof这四项不变，不能靠re-Pair造成同一个拒绝。

具体检查：

1. 在entered前记录原full key、run、store=true、feature=true、owned workers=1，
   executable grant/continuation/model content/capture/input全部为0。
2. 只替换本场景指定对象，见证新旧identity确实不同、其他原对象保持；不输出token/ID。
3. 释放原门闩并join；原生产pin准确返回 `Computer Use request owner retired`。
   如果实际失败族不同，保留首败查具体切点，不以泛化is_err或放宽断言刷绿。
4. 原TaskGrant/continuation为0，feature/store回滚初始false；原reserved run真的Stopped，
   原owned workers=0。业务reply与物理退出必须分别查证。
5. 原请求的模型正文/截图/list与旧观察/input不得获得权限；A/B/C页面和用户Share保留，
   未选C的正文/capture/input独立为0。若替换者有自有资源，旧rollback不得清它的资源。
6. 对保留bearer的transport替换项，明确证明拒绝来自Arc ownership而非token失效。
   原App process/agent/ACP endpoint仍相同，不把注册immutable executor失败当替换。

固定private child pipe只能用于测试Host手势与有限见证，不能新增模型可调用的approve/
reconnect/resume接口或通用任意对象替换API。不改原15/18/20秒与生产请求期限。

## 验证与后续

source45 choose-unshare在原startup-trusted-local-api首次evaluate失败，0checks，
原Error被归为other，后次诊断可用不能定根因。下批仅增加固定脱敏分类（例如明确
execution-context-destroyed、worker-closed、protocol-error族）与原worker close/context
事件见证，不记录原异常text/stack/URL或token。未知仍other，不猜成某一族。
带新见证对原失败场景做有限定位；成功只记non-reproduction，不销首败或自动重试。
这项诊断与owner四项可在同一小批，不以无限刷旧矩阵延迟其余ready工作。

下一source/artifact从46起，开工先核尚未被其它修改占用；不能覆盖source45证据。
先四项focused，再同候选完整native矩阵、受影响ACP/CU/routing、strict/Node/MCP/
fmt/quality及source/artifact核对。Core/前端未改则明确未全量重跑，不借旧总数。
历史锁/IO/B/startup/rename首败保持开放；再现保留原handle/Job/即时DELETE witness，
无根因时不得加通用sleep、宽限、固定重试或忽略失败。

之后继续choose/reservation各切点owner退休、Stop/取消/Drop与原join、queued续行
到stdin前撤销；再恢复并发与迟到正文/capture/input回包。已验原Host正常恢复和自然
lease不用重新开发；并发恢复需要原resume worker实际切点与fresh观察publication证明。
安装/原任务UX门槛后才开放ExistingTab descriptor，raw parser不顺手放宽。
完整U/D、四surface、三平台/双Mac、输入/恢复、签名安装升级回滚、真人Grok与同候选
12h不缩减；所有缺设备/真人授权行继续not_run，不标整体完成。

按AGENTS第9条在每批原owners退出后生成新精确清单，回读全部target/retained paths/
preserved hashes再Execute。保留当前可用产物、依赖、seed、源码和失败证据；
旧compiled sites/runtime/可证明废弃cache常规清理，不复用已执行身份或杀其它进程。

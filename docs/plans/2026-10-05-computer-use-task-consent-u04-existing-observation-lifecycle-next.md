# ExistingTab：修复原观察取消与物理完成（R5）

依据[source82真实反例](2026-10-05-computer-use-task-consent-u04-existing-late-capture-checkpoint.md)。
Pause/Chat Stop两条原App/MCP/CFT154 PNG链实际0/2失败，Host提前idle且扩展旧图仍解码/提交；
模型最终拒绝有效，不能因此跳过修复。原log、10个profile/PNG hash、source/artifact已保留。
单Codex writer，不启动Grok/子代理，不提交/push/PR/发布；descriptor关闭，raw parser仍Desktop-only。
完整U/D、四surface、三平台/两Mac架构、安装/真人Grok/12h/UX范围不缩减。

## 当前已推进到source86

后续已完成[source89 Host / IPC 基础](2026-10-05-computer-use-task-consent-u04-existing-observation-host-checkpoint.md)：
本文件第1节Host工作已补及Core1097/1113、两Core strict通过；第2节Host lifecycle routes已接。
继续第2节扩展v2协商/validator、第3节每request scope/watch/journal/recovery和第4–5节fresh整链验收。
当前扩展仍v1、默认App仍86，本树不是完整候选；82真实反例仍OPEN，原断言/全交付范围不减。

[扩展Promise所属占用检查点](2026-10-05-computer-use-task-consent-u04-existing-observation-ownership-checkpoint.md)：
原clock/query/script/capture/normalize已等待原Promise，deadline只退休结果，连接signal已贯穿；
reset/unshare保留原tab观察占用，阻止同tab新观察/输入/Share。新的ObservationOperation可接后续每request scope。
这并未修Host提前idle或传递Pause/Chat Stop取消；82真实反例仍OPEN，下面所有完成条件和跨平台交付要求不变。
下一步先核对86原owners/cleanup Plan及source，再修改fresh87+；默认App artifact已更新，不能再按82身份运行。

## 开工与身份（82原始起点）

原source82 build/focused/denial/strict/Node owners已终态；先完成fresh82-1精确清理与证据复验，
再改source83+。不能覆盖81编译首败、82两条原native首败，不能把82 EXE映射成新Rust/Core。
主fixture994行、joint983行、80个≥1000行文件预算；新逻辑拆入领域模块，不放宽预算。

## 1. 先修Host请求所属的占用与取消

从browser/host/transport.rs的PendingExtension、sweep_requests、capture_existing_document、
existing_operations_idle、quiesce_existing_operations及cancel_existing_operations入手。
区分“撤销执行/退休结果”与“原执行器已实际结束”：cancel/timeout可立即拒绝模型和新派发，
不得删除已交给原扩展的观察占用，或将队列空当成原scope退出。未派发请求可以退休为NotStarted。

为已派发观察保留独立的request/run/tab/document/generation/connection/browser身份、
只派发一次的状态、request cancellation及原执行器claim/watch/settle证据。
正文和截图、model与preview都经过同一完成规则；失败也必须归还原占用。
原未结束浏览器Promise保留时，Host Pause/Stop/switch/Continue不能越过它开始新观察或新输入。
deadline只请求退休，不释放仍活跃的工作；晚到result不得发布image/text/refs或占用新grant。

现有CompletionRegistry/CompletionProof有可复用的原claim/status/settle、取消与browser cleanup语义，
但不能直接复用offer_existing_completion：host/completion.rs的live要求已发布current_observation，
而首次观察应在尚无旧snapshot时工作。Share offer已有document_id，新的观察身份从原获准Share取，
并在原worker复验，不能调用额外model观察、采用别的tab或借旧动作proof补造权限。
若扩展现有完成合同，明确区分observation与action的用途、proof域及验证；不能使观察回执领取输入。
不要把截图套成click或伪造DOM操作完成nonce。原新snapshot由原请求生成，不从Host暗补模型身份。

先写能证明取消后占用仍在、直到原执行器settle才空闲的Host反例；保留自然结束、未派发拒绝、
claim前后竞争、duplicate/stale/result-before-settle、deadline、Pair/Share撤销与browser退出分支。
测试应核对原state转换和原sender/receiver，不用手动清pending冒充完成。

## 2. 明确观察协议与能力协商

原v1 ExtensionRequest与/extension-poll/result没有每request取消及物理settle合同。
新增明确版本/能力，与原Host及PairingClient/ExtensionTransport validators同步；缺该能力时诚实拒绝，
不继续执行缺收尾证明的旧通道。保留稳定session MCP transport，不新增模型授权/恢复工具。
观察request/proof/result/status须绑定同一实际原owner，不能跨operation、snapshot、document、run或connection。
取消watch/settle是原cleanup权限，不应因executionReady=false或新Pair而失去收尾途径。
proof/token仅在trusted worker/Host；不进模型、产品文案、stderr或普通诊断。

检查相应Core protocol、Host IPC routes、extension拷贝/签名/journal/recovery、catalog和导出固定文件清单。
若新增文件，更新实际生产bundle与校验，不只让探针import成功。保持权限/key/最小浏览器版本，
不得加all_urls、换managed seed或默认打开CU来绕过正确协商。

## 3. 原ExtensionTransport与SharedTabs的Promise收尾

每次原观察先取得request scope，再把它的signal贯穿原SharedTabs.observe和normalizeCapture的live。
原watch持有自己的实际Promise；scope取消时即使Pair/Share/document仍保留，仍须拒绝该请求。
已真实返回的旧data URL在原createImageBitmap前丢弃；已开始的decode/normalize则等待其真实收尾再settle。

当前SharedTabs.bounded使用Promise.race，5秒超时可能先返回而原native Promise还未结束。
不能只加signal或再套一层race便宣布完成；为原query/executeScript/capture/decode都保留所属Promise，
超时仅退休结果，不归还仍在用的观察slot。保持有界数量；不会无限堆积旧worker或poller。
原finally确实join所属capture/读取/decoder和cleanup waiter后，才提交一次物理settle回执、归还占用。
reset/断线/feature off/导航/worker退出不得孤立这些owner；原worker死亡的未知完成不能伪造settled，
沿原journal/browser/document可信退出证据处理。图片与正文失败不得自动成功降级或偷偷新observe。

## 4. 同一原App / TaskGrant重新验收

保留82两条首败，使用fresh候选重编译，先跑原CFT154 Pause/Chat Stop独立场景。
如果新完成协议需要调整test gate，应抓新原poll/observe/scope的精确切点，保留同一个实际原请求/PNG；
不要放松HostIdleBeforeRelease=false、decoder=0、零旧图/正文/refs发布的期望来刷绿。
现gate对v1 completeExtension的匹配可能需随真实生产合同更新，必须证明新原settle/watcher也自然join，
不能以不再调用旧completeExtension来伪装完成。

截图native调用1/settled1，实际几何/PNG与A/window/document不变；held时Host仍busy/cleanupPending。
原Pause/Stop立即撤权，原late PNG释放后解码0，旧请求/原consumed ACP delivery不能复活。
Pair/三Share/原A/B/C保留，B/C零model访问/输入，原feature/store不变；模型不收到旧图/正文成功。
Pause全部旧owners收束后显式一次Continue，新观察、新generation、恰好一次ACP续行；Chat Stop真正Stopped。
原截图权限拒绝仍独立测试，不自动正文降级；首次无旧观察、无截图的正文路径也要实际成功。
补自然截图成功（非中断）的原App/MCP链，不能只测拒绝或中断；真人Chrome/Edge授权仍独立开放。

## 5. 同候选门禁、清理与后续

Core改动补default/test-support全量和两strict；App/probe strict、原宽CU/ACP/routing、完整native，
Node/MCP/诊断unit、对应golden/validator/导出检查及helper syntax/fmt/quality/diff/Verify。
前端若改补全量typecheck/test/build、15locale与交互/渲染验证。门禁失败保留first failure/phase/profile/hash，
不加预算、重启原handle或删断言刷绿；编译实际终态前不得映射artifact。
原owners终态后fresh cleanup Plan→读精确targets/current/hash→Execute；保留当前构建、依赖、
seed/CFT154/CFT140/source/fix.patch及所有日志/PNG/首败，working/unknown/其他owner仅defer。
清前后复验全部历史hash/absences和82新增10hash；记录逻辑bytes与实际磁盘空闲变化及恢复方式。
此项完成后继续完整E5/安装态/真人Grok/授权体验/四surface/三平台/12h/UI UX，不能用局部fixture替代交付。

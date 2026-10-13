# ExistingTab：启动顺序诊断与 Cancel/Stop/Drop 接续

本合同的dormant/native附件部分已在source51/52执行；当前终态见[启动检查点](2026-10-04-computer-use-task-consent-u04-existing-bootstrap-checkpoint.md)：
focused dormant1/browser3通过，完整native33通过/5失败、exit101，宽回归和精准清理完成。
继续按[原control诊断与五项终态](2026-10-05-computer-use-task-consent-u04-existing-control-terminals-next.md)，
不重新开发启动部分；以下source50前提及详细后续合同为历史保留，不替代当前实测。

这是下一小批的实施合同，不是通过证明。source50 输入拒绝 focused12 实际通过，
完整 native 首个 browser-reopen 在 source-mv3-start / startup-worker-ready 失败，
0 checks；完整native36通过/1失败，宽CU293/38manual ignored、ACP178/routing38及
App两strict/Node177/MCP38/质量门禁通过，见[原owner输入检查点](2026-10-04-computer-use-task-consent-u04-existing-owner-input-checkpoint.md)。
source48原观察断连在source49诊断/source50矩阵中未复现，不能称根因已修。
保留所有首次失败、单 Codex writer，
不启动 Grok/子代理，不提交/push/PR/发布，不碰用户浏览器、凭据或共享 Grok home。

## 先处理 source worker 启动顺序

原 source50 日志证据：startup initialWorkerCount=0、workerEvents=0、原 context
未关闭，worker-ready 等待最终30秒 timeout。原 native 控制先在20413ms超时；
随后原 Node 报告并实际 teardown，不能把控制超时与 browser worker 超时混成
一次不同阶段的产品权限失败。原 App/store root 为 Temp 下
`grok-cu-managed-task-877e468c-b791-40a8-977d-a590c08b107d`；原 Browser root 为
`grok-cu-pairing-owned-8f750dee-3bdf-4301-9256-4d33164ca3e4`。只读 curated
joint-failure.json，其 SHA256：
`17b33d31c11eb1569f25374b3f6213fd12dd3b4aa4f8821e522a4b77189d8b35`。

只读代码已确认：`existing-task-joint-live.mjs` 等待 serviceworker 后才打开
source popup；`existing-task-browser-live.mjs` 初始路径先用 CDP loadUnpacked
安装一次，再接 Playwright context。需要验证“已安装但 worker 尚未启动/已
idle，而 popup 的真实消息尚未发出”的启动顺序假设。当前日志没有证明确切
原因，不能直接称为扩展或生产连接故障。

1. 为新冻结候选51+增加固定诊断：原 source 扩展安装回执是否匹配、该 source
   的 service_worker 数量及已安装 source popup 是否可打开。只输出数量/布尔/
   固定 phase；不枚举或记录任意用户页面、URL、ID、浏览器存储或原异常内容。
2. 在同一隔离 native Chrome/原 source extension 上构造确切 dormant worker。
   若用 CDP，必须先锁定该扩展的原 registration/version，再停止它并见证物理
   状态；不对用户浏览器操作、不伪造 onStartup/配对，不把 Debugger pause
   冒称 worker 停止。Chrome API 不支持或无法确证时如实记录，不能用 sleep
   制造“应该已经停止”的前提。
3. 有可重现证据后，沿用户打开扩展 popup 的真实路径激活 worker，再选择
   唯一匹配 source sw.js 的 worker。保留原 installed ID、API/version 检查与
   first-use off/no-authority；不接到其它 worker，不造配对或旧 grant，不增加
   worker重启、sleep/retry或任何原20/30秒预算。
4. 同时检查 browser/profile replacement helper 的新 context 启动顺序，复用
   小型 probe helper，保持初次安装一次与真正浏览器 onStartup 的证据区别。
   不能在每次重开重复 loadUnpacked，不能用暖启动成功替代 onStartup 见证。
5. 先跑原失败场景及 dormant-worker 反例，确认原等待与新真实 popup 路径的
   区别，再受影响完整矩阵和门禁。仅未复现就保持原根因 OPEN；未取得依据前
   不改 PairingClient 的错误退休、5秒请求、30秒lease或生产权限规则。

## 下一组联合切点：先选择与 discovery，再 reservation/publication

复用首次实际 session MCP request/source MV3/认证 HTTP/原 App ACP/新 TaskGrant
路径。固定原完整 key/picker/receipt，A/B选中、C未选，feature/store初始false；
每行新建隔离场景，不沿前一行恢复原 owner。不是再开发已通过的正常恢复。

第一小批先做以下五项；remaining 两个切点仍 required：

| 切点 | 实际操作 | 必须保留的生产入口/物理见证 |
| --- | --- | --- |
| choose，原 ready receipt 已返回 | picker Cancel | SessionManager::cancel_computer_use_task_picker；原选择不得再批准，零 reserve/approval/run |
| choose，同一原 receipt | task Stop | SessionManager::decide_computer_use_task(..., Stop)；原 pending request 不得复活，零 grant/continuation |
| discovery，原 OwnedTasks worker 已进入 | picker Cancel | 原 App Cancel；释放后原 load 真正返回/join，不发布旧 receipt |
| discovery，同一进入门闩 | task Stop | 原 App Task Stop；原返回后不发布旧 receipt/authority |
| discovery，同一进入门闩 | Drop 原 load future | 真正丢弃外层 future/guard；原 registry 保留 handle 到物理退出，不以 abort 返回冒称 join |

`DiscoveryProbe` 当前是真正原 App load/OwnedTasks，已有原3个Share snapshot进入
见证与生产5秒 discovery deadline。新操作/释放必须在原证据窗口内完成；fixture
太慢就保留首败，不提高预算。Drop 后需要从原 owned registry 取得物理完成
见证；当前 DiscoveryProbe::finish 只处理仍在 probe slot 的 waiter，不能把其
清空作为 registry 已收束的证据，不调用会永久停止全局registry的 join 去污染
后续同进程场景。必要的新 test-only wait/reap 入口只能读/等待原具体 owner，
不能替换其 handle、抢其它调用的结果或清除 join_failed。

choose 没有运行中的原 load future，因此不能把 Drop PickerSlot 冒称 Drop 异步
discovery。PickerSlot::drop、PickerCallGuard::drop 和 WorkerCall::drop 都有 fence，
仍需联合真实证明，不能仅从代码存在推论已验。

之后 reservation/publication 各自补 Cancel/Stop/Drop：前者保留原 owned reserve
到真实返回，后者保留准备好的原 run、store 回滚和实际 Stopped。零 run 行不能
制造 Stopped。publication 必须用原 App pin；若取消没有被原 pin/Core 接住，
先保留反例再修生产路径，不统一断言为 owner-replacement 的准确错误。

批准完成后的“关闭 picker”与任务 Stop 不相同：当前已有 ACP 单元证据表明
cancel 已提交 picker 不撤销已提交 grant；不能误改这一规则。新增联合切点都
明确位于 commit 之前；已 commit 的取消任务另用 Task Stop。

每项核原合法 bearer 的正文/截图/list/input拒绝、A/B/C独立零副作用，原Pair/
document/3个Share与页面保留；原grant/continuation、feature/store和workers闭合。
准确错误在实际原返回路径确认，不用宽泛is_err/允许集合或重试刷绿。

## ACP queued continuation 与恢复竞争仍在后面

`DecisionDelivery` 保留原 RequestTransport；真正 stdin admission 内检查
ledger.chat_stopped、generation、dispatch_id 与当前 transport，再消耗原
continuation。下一批要停在原 stdin 前，通过实际 App chat stop fence撤销，
原 peer 独立计数必须零 prompt。已进入 stdin 的 Unknown 单独记，不重放。
`fence_computer_use_chat_stop` 与需要真实 AppHandle 的 App stop/UI 点击证据
分开标明，私有 fixture不能冒称安装版用户点击。

恢复 worker 的 Pause/Stop/owner替换竞争、并发Continue和迟到正文/截图/input
继续按完整原身份执行；不重写已通过的正常 Host 恢复，不用新 endpoint接旧请求。

## 验证、清理及完整范围

每次源改动新freeze/artifact，先真实反例和focused，再同候选native矩阵、受影响
App CU/ACP/routing、两strict、Node/MCP及fmt/quality/diff。Core/前端未改明确
未全量重跑，不借历史数字。原 owners 实际退出后按 AGENTS第9条新精确清单、
回读target/retained/hash再Execute；保留当前构建/依赖/seed/源码/所有失败证据，
未知owner或文件deferred，不杀其它进程，分别报告逻辑与实际空间变化。

descriptor仍关闭、raw parser仍Desktop-only；安装/原任务UX门槛后才开放。
完整E5、U04–U09、D00–D31、四surface、Windows/macOS/Linux、双Mac架构、完整
输入/恢复、签名安装升级回滚、真人Grok与最终同候选12h不缩减；缺设备/真人
授权仍not_run。整体active / partial — not releasable。

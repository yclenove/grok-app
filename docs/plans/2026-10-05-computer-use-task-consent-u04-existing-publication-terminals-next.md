# ExistingTab：发布前 Cancel / Stop / Drop 下一批合同

这是下一小批的实施合同，不是通过证明。当前source62见[reservation检查点](2026-10-05-computer-use-task-consent-u04-existing-reservation-terminals-checkpoint.md)：
新切点均有定点通过证据，完整native40/6未绿；先补两项配对及原控制失败的精确见证，不冒称全绿。
单 Codex writer；不启动 Grok/子代理，不提交/push/PR/发布。不碰凭据、代理、
日常浏览器或共享 Grok home；保留 dirty/untracked/fix.patch 和全部首败。
整体仍 active / partial — not releasable，完整 U/D 与三平台交付范围不缩减。

## 进入条件

先收束 reservation 当前冻结候选的完整 native / App CU / ACP / routing、App 默认及
probe strict、Node/MCP/诊断、格式质量及 source Verify。失败逐项绑定原 App/profile/phase/hash；
初始化失败与实际授权切点失败分开。未复现不销项，不增加预算或无限复跑。
原 owners 退出后完成新身份的精确清理，保留当前产物、seed、源码和所有失败证据。

## 三个真实切点

沿原首次 session MCP request、source MV3、认证 HTTP、原 App/ACP picker 及
`task_access::approve_with_publication`。A/B 选中，C 未选，初始 feature/store false。
原 `PublicationProbe` 门闩位于最终原 App pin 之前；不得改成另外一套提交接口。

| 场景 | 原操作 | 切点前见证 | 切点后必须证明 |
| --- | --- | --- | --- |
| publication-picker-cancel | 原 SessionManager Cancel 同一 picker | 唯一准备 run；feature/store true；grant/continuation 零 | 释放原 pin 后准确拒绝；回滚；原 run 实际 Stopped |
| publication-task-stop | 原 SessionManager Task Stop 同一 key | 同上 | request Cancelled；原 pin 拒绝；回滚；原 run 实际 Stopped |
| publication-waiter-drop | abort 并 await 原异步批准 waiter | 原具体 approval registry UUID；唯一准备 run | outer 真正取消；原 inner handle 留到真实返回/join；零授权；回滚及原 run 实际 Stopped |

Cancel / Drop 后仍沿原 request 的准确 Pending/Preparing 合同核对，不强行统一成 Cancelled。
三个场景均保留原 App pin、request key、picker、transport Arc、准备 run 与 document。
不得拿 owner replacement 的错误代替本场景错误。最终 pin 的现有 Core 拒绝映射为
`Computer Use approval retired before publication`；如实际先被其它固定入口拒绝，先保存反例、
核对原调用路径，再确定精确断言，不放宽为错误集合或 `is_err()`。

已经 commit 后的“关闭 picker”不等于 Task Stop；已有已提交 grant 保留反例必须继续通过。
本批不修改该语义，不造新的 run 充当原准备 run 的 Stopped，也不把 registry 零计数当物理退出。

## 原句柄与清理归属

`FixtureApprovals` 使用原 owned approval worker 事务及隔离 preferences。
必要的新接口限 test-only：从该 fixture 的原 registry 捕获唯一 UUID，targeted join 已 abandoned
的原 handle。复用 `OwnedTasks::singleton_entry_for_test/join_abandoned_for_test`，不清 stopping/
join_failed、不调用全局永久 stop/join 取得中途证据、不抢其它 live waiter 的结果。

Drop 前先固定 UUID/run。abort + await 原 outer，见证 registry 中原 inner 尚存；释放原门闩，
等待原 rollback/pin 返回后 targeted join。失败 finally 必须沿同一 handle 收束原 inner。
最终 teardown 可以执行原 fixture 的全量 join，但不能用它代替切点内的具体 owner 证明。

原门闩保持 15 秒、控制 20 秒、RPC 18 秒、请求 5 秒、Pair lease 30 秒等现有预算。
不通过调大预算、sleep、自动重试、强断 Pair 或重建 receipt 达成通过。

## 联合后置条件

原合法 bearer 的正文/截图/list 与 A/B/C 各 set_value/type_text 共六次 schema-valid 输入
必须具体拒绝；页面独立 DOM/input/scroll/click、脚本及 capture 计数证明零副作用。
保留三个原 document、Share、Pair 和页面；仅任务临时权限回滚。

唯一原 run 最终 Stopped，feature/store false，grant/continuation 零；approval 启动恰好一次，
原 approval/discovery/reservation owners 零，原 App/ACP transport 未换。不能靠枚举用户标签页、
读浏览器存储或导出 Cookie/Token 做见证。Host手势/ACP peer仍 synthetic，不能写成安装态/真人结果。

优先复用 `verifyTerminalIsolation`；publication 专有准备/准确错误/run Stopped 单独核对。
固定新 phase/stage 若需要扩充，只加真实步骤和脱敏计数，不输出原异常正文、stack、任意 URL/ID。

## 验证及后续

源码改动必须新 freeze/artifact，不借旧可执行文件或旧门禁数字。先三个 focused 与已提交
Cancel 保留反例，再同候选完整 native、宽 CU/ACP/routing、App/probe strict、Node/MCP、
诊断持久化、fmt/quality/diff 和最终 Verify。Core/前端未改时明确未全量重跑。
全部原 owners 终态后新清单 Plan → 回读 → Execute；实际空闲变化与逻辑大小分开记录。

下一批仍 required：queued ACP continuation 在原 stdin admission 前由真实 App chat stop fence
撤销，peer 独立零 prompt；恢复 worker 的 Pause/Stop/owner替换、并发 Continue、迟到正文/截图/
input 竞争。不要同时展开本批和后续源码。descriptor 仍关闭、raw parser 仍 Desktop-only。
完整 E5、U04–U09、D00–D31、四 surface、Windows/macOS/Linux、双 Mac 架构、完整输入/恢复、
签名安装升级回滚、真人 Grok 及最终同候选 12h 保留；缺设备/真人授权标 not_run。

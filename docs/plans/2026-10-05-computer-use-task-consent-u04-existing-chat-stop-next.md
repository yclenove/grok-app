# ExistingTab：原 ACP stdin 前 Chat Stop 下一批合同

这是publication三终态收束后的下一小批，不是已实施或已通过证明。
当前候选source66完整native49/0、宽CU/ACP/routing及门禁已通过，原owners已退出；
首次focused的owned-fixture-open首败仍OPEN，详情见[执行暂停恢复检查点](2026-10-05-computer-use-task-consent-u04-existing-execution-suspension-checkpoint.md)。
本合同仍是下一小批，尚未实施或验证；不得在原owners活着时修改其冻结源码。
单Codex writer，不启动Grok/子代理，不提交/push/PR/发布。保留dirty/untracked/fix.patch和全部首败。
整体active / partial — not releasable；descriptor仍关闭，完整U/D与三平台交付范围不缩减。

## 已有路径及本批差异

`SessionManager::stop`在改变turn状态前调用`fence_computer_use_chat_stop`，该方法从原App endpoint
进入`AcpClient::computer_use_chat_stopped`，递增delivery generation、清notice并fence task consent。
`prompt_computer_use`在原stdin锁取得后复验Host turn、原transport Arc、delivery generation与claim，
只有实际admission才consume continuation；admission前拒绝返回`DeliveryAttempt::NotStarted`。

现有`TransportFixture::queued_managed_continuation_stops_before_wire`持有真实stdin锁，等待原pending，
触发Stop后释放锁、join原prompt，再从独立ACP peer证明零wire。
现有managed queued-stop走`decide_computer_use_task(..., HostTaskDecision::Stop)`，
delivery单元中的queued notice走直接`client.computer_use_chat_stopped`。
它们不能代替本批ExistingTab/new TaskGrant + 原App Chat Stop fence的联合验收。

## 固定原身份与进入条件

沿首次真实session MCP request/source MV3/认证HTTP/原App picker、新TaskGrant publication。
A/B选中、C未选，原feature从false到显式批准后的true；捕获原key、transport Arc、run、
prepared grant、唯一continuation claim及三个document。原ACP peer保持synthetic，不能写成真人Grok结果。

批准后原`manager.reserve_computer_use_delivery("view-app")`必须reserve一次，原agent不变。
握住原client的真实stdin锁，启动原`prompt_computer_use`；只等该prompt的准确pending见证。
已有claims可用于read-only见证，不consume、不替换receipt、不提前发送`delivery`成功命令。
准备点要求：尚未admitted、原pending恰好一个、原continuation恰好一个、peer尚无prompt。

## 真实切点与后置条件

1. 在原stdin锁未释放时调用原`SessionManager::fence_computer_use_chat_stop`，即产品Chat Stop
   的第一个同步边界。Host手势仍synthetic；不冒称调用了安装态UI或完整含AppHandle的`stop`。
2. 释放原stdin锁，await原prompt直到真实返回；精确`NotStarted`、未admitted、原pending空。
3. 独立peer读取证明零`session/prompt`，不能只看registry零计数。保留同一client/transport/run；
   cleanup必须join原prompt，不能abort后冒称自然完成。失败清理若abort须明确保留反例。
4. 原run实际`Stopped`后读取原authority：零可执行grant/continuation，无新增execution。
   feature/store是否保持true按原已批准任务的合同核对，不能复制first-use rollback为false断言。
5. 原合法bearer下正文/截图/list以及A/B/C六次schema-valid输入准确拒绝；原independent DOM、
   script/capture计数无副作用。保留原Pair、三个Share/document/page；不re-Pair/重建receipt/续租。
6. 重用旧delivery再次prompt仍精确`NotStarted`、零peer wire；模拟显式chat started也不能复活旧claim。
   仅新的用户task/request可以形成新批准，旧notice不得自动复活停止的chat。

优先提取现有queued helper的公共部分并保留managed回归，不复制另一套stdin admission实现。
test-only见证若新增，绑定原entry/UUID/claim，不清全局stopping/join_failed、不抢live waiter结果。
原3秒pending/join、请求/控制/Pair lease等预算不调大，不自动重试、强断Pair或sleep刷绿。
脱敏phase与被动连接快照在辅助await前保存primary，不打印token、header/body或任意URL/ID。

## 验证与收尾

owners退出后才写新候选67+，新freeze/artifact；先ExistingTab focused与managed queued-stop回归。
再同候选完整native、宽CU/ACP/routing、App/probe strict、Node/MCP/诊断、fmt/quality/diff及Verify。
不借旧产物或旧门禁数字；所有首次失败精确绑定App/profile/phase/hash，未复现不销项。
原owners终态后新清单Plan→回读→Execute；保留当前构建、源码、seed和失败证据，实际空闲与逻辑量分开。

同批补source66首败的精确probe见证：owned fixture的create/navigate/focus/tab-query固定切点，
原native evaluation只记录允许的错误类别。先持久化primary再做辅助await，不能输出任意URL、
脚本、header/body或凭据；不自动重试、增加预算或将旧unknown改写为已定位。

随后仍required：恢复worker中的Pause/Stop/原owner替换、并发Continue与迟到正文/截图/input竞争。
完整E5、U04–U09、D00–D31、四surface、Windows/macOS/Linux、双Mac架构、完整输入/恢复、
签名安装升级回滚、真人Grok、最终同候选12h及UI/UX全部保留；缺设备或真人授权记not_run。

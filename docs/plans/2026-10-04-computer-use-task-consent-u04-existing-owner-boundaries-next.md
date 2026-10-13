# ExistingTab：选择与预留期间的原 owner 退休

本合同已在source48/build48执行：新增focused8通过，完整native36通过/1失败，
实际结果见[owner boundaries检查点](2026-10-04-computer-use-task-consent-u04-existing-owner-boundaries-checkpoint.md)。
source50已补十二项直接input拒绝，focused12通过；完整native36通过/1项startup首败（0checks），
见[原owner输入检查点](2026-10-04-computer-use-task-consent-u04-existing-owner-input-checkpoint.md)。
最新source52及五项原矩阵首败见[启动检查点](2026-10-04-computer-use-task-consent-u04-existing-bootstrap-checkpoint.md)：
dormant1/browser focused3通过，完整native33通过/5失败、exit101；原owners已退出，精准清理完成。
下一项[原control诊断与Cancel/Stop/Drop](2026-10-05-computer-use-task-consent-u04-existing-control-terminals-next.md)，
再恢复竞争；source48观察及历史startup根因仍OPEN。以下保留原设计，不是重复开工或全绿证明。
单Codex writer，保留全部未提交内容、fix.patch和首次证据，不启动Grok/子代理、不提交发布。

## 下一批先做两个切点

使用同一个首次实际 session MCP request / source MV3 / 认证 HTTP / 原 App ACP
联合路径。各行新建隔离场景，初始 feature/store=false，实际 Pair 与 Share A/B/C，
固定原完整 key、picker、receipt 和 A/B 选择；C 不参与授权。不是改造已过的正常恢复。

| 切点 | 替换对象 | 必须证明的原路径 |
| --- | --- | --- |
| choose，原 receipt 已返回，尚未提交选择 | App process / agent / ACP Arc / transport Arc，各一项 | 旧 picker 不得被新 endpoint 接受；不启动 reservation 或 approval |
| reservation，原 owned reserve 已进入门闩 | App process / agent / ACP Arc / transport Arc，各一项 | 原 reserve 实际返回后，原 transport 或 publication pin 拒绝；不能借替代者完成 |

process/agent 是实际原 SessionManager slot 的合成变更，不是 OS 进程死亡；
ACP 替换保持旧 ACP 活着，并持有另一真实隔离 TCP peer 到实际 finish；peer 不是 Grok。
transport 替换保留原合法 bearer，仅更换实际 Arc，不能用 token 过期代替 ownership 证明。
Pair、BrowserSession、document 与 3 个 Share 保持不变，disconnect/re-Pair 数不增加。

原四项 replacement helper 当前只接受 publication 门闩；不能把它的 ready=true 或
run 见证照搬到新切点。抽取固定身份替换操作时，保持每切点自己验证进入条件，
private pipe 只允许固定场景；不给模型新增替换、批准或任意对象修改工具。

## 生产切点与断言差异

choose 必须调用 `SessionManager::approve_computer_use_task_picker`，保留
`with_original_task_picker` 的 process/key/picker 验证，不直接调用 ACP 绕过 App 检查。
原 ready receipt 仍由生产 load 创建，不能手工植入候选或结果。

reservation 继续使用 `ReservationProbe`，它进入的是原 Core reserve；保留
`start_picker_approval`、`PickerApprovalGuard`、原 owned reservation 和原 App pin。
释放后真正 join，不能取消外层 future 后把未知结果说成拒绝。原 15 秒门闩、
18 秒 RPC、20 秒 Host 控制、5 秒 discovery 与 10 分钟 request 期限不变。

不能把两切点统一断言为 publication 的错误：

- choose 的 App picker/current transport 检查可能在 native reserve 前拒绝。
- reservation 的 transport Arc 替换在 `finish_picker_approval` 的 reserve 返回后
  `scope_owner_is_current` 检查拒绝，正常应没有 constructor/run。
- reservation 的 App process/agent/ACP slot 替换，可能经过原准备后由原 App pin
  拒绝；若确实建立 reserved run，必须见证 feature/store 原值回滚和该 run 实际 Stopped。

每行先从原返回路径确定准确拒绝分类；遇到意外分类保留首败查切点，不用泛化
`is_err`、扩大允许集合、增加 sleep/retry 或调高预算。只有实际建立的 run 才能
断言 Stopped，零 run 行不能伪造一个 Stopped 见证。

替换后读回固定原 `TransportFixture::original_host_task_view`，不能跟随替代
SessionManager endpoint，也不能把 slot 还原后继续旧批准。只输出固定分类、
identity 相同与否、数量和阶段；不记录 token、原异常 text/stack 或 URL。

## 每行关闭条件

1. 固定原 key/picker/receipt，模型正文、截图、input 与 executable grant 为零。
2. 只指定身份不同，其他原身份相同；旧 ACP 与原 browser connection 仍活着。
3. choose 没有启动 owned reserve/approval；reservation 证明原 worker entered，
   释放后原句柄实际 join，最后 reservation/approval owners 全为零。
4. 原任务零 grant/continuation；store/feature 最终 false。若有原 run 则确切 Stopped。
5. 原合法 bearer 的 MCP 读/截图/list/input 仍无权限；A/B/C DOM 独立零效果，
   用户 3 个 Share 和页面保留。新 ACP 不获得 request/grant/execution。
6. 新旧 ACP/native 所有 owner 实际 finish；失败场景保留精确根、报告和首败日志。

八项 focused 后，同一新冻结候选跑完整 native 矩阵和受影响 CU/ACP/routing、
App 两种 strict Clippy、Node/MCP、fmt/quality/diff。Core/前端未改则明确未全量重跑，
不搬用旧总数。新 source/artifact 身份从 47 起，先核没有别的 writer 或同名证据。

## 再下一批：取消和恢复，仍未实现

选择、discovery、reservation、publication 的 Cancel/Stop/Drop 分开验收。
picker Cancel 使用原 `SessionManager::cancel_computer_use_task_picker`；任务 Stop
使用原 `decide_computer_use_task(..., HostTaskDecision::Stop)`；聊天 Stop 的
`fence_computer_use_chat_stop` 与实际 App `stop` 链分别标明证据，不能互相冒称。
当前 App `stop` 需要真实 AppHandle，不能凭私有 fixture 说已验安装版用户点击。

Drop 要实际丢弃原 future/guard，同时保留原 owned worker registry/句柄到物理 join，
不能只替换 slot 冒称 Drop，也不能把 abort 结果当成资源已释放。
queued continuation 在原 ACP stdin 之前撤销，验证原 peer 实际零 prompt；
已进入 stdin 的状态单独记录，unknown 不重放。

之后补原恢复 worker 的 Pause/Stop/owner 替换竞争、并发 Continue 和迟到正文/
截图/input。原 Host 正常正文恢复已验，不再重复开发；fresh publication 必须绑定
原请求/执行身份，旧回复不得复活权限或覆盖新代次。

## 构建清理与完整交付

每批原 build/test/browser owners 实际退出后，按 AGENTS 第 9 条生成新清单，
回读所有 target/retained paths 与 preserved hashes 再执行。只删精确过期产物，
保留当前可用程序、依赖、不可变 seed、源码和所有失败证据；不复用已执行清单。
分别记录逻辑删除量与磁盘空闲实际变化，未知 owner/文件 deferred，不杀其它进程。

ExistingTab descriptor 仍关闭，raw parser 仍 Desktop-only；安装/原任务 UX 门槛
通过后才考虑开放。全 E5、U04–U09、D00–D31、四 surface、Windows/macOS/Linux、
双 Mac 架构、完整输入/恢复、签名安装升级回滚、真人 Grok 和最终同候选 12h 不缩减。
缺设备或真人授权的验收仍记 not_run，overall active / partial — not releasable。

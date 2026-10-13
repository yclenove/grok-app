# ExistingTab：原恢复 worker 与迟到结果竞争下一批合同

本合同是[Chat Stop stdin 联测](2026-10-05-computer-use-task-consent-u04-existing-chat-stop-checkpoint.md)
之后的下一小批。source69 完整 native50/0、宽 CU295/51ignored、ACP178/routing38及
门禁通过，清理结果以检查点为准。原 owners 退出以前不修改冻结源码、不清理产物。
单 Codex writer；不启动 Grok/子代理，不提交、
push、PR 或发布。全部 dirty/untracked、fix.patch、首败及原始现场保留。

整体仍 active / partial — not releasable。ExistingTab descriptor 仍关闭；本批仍是
source MV3/认证 HTTP/原 SessionManager 的联合 fixture，Host 手势及 ACP peer 是
synthetic，不能冒称安装态 UI、真人 Grok 或三平台交付证据。

## 原实现与真实缺口

- `SessionManager::resume_computer_use_task_execution` 固定原 endpoint，进入
  `TaskExecutionCall::reserve_resume`，只对首次 reservation 启动原
  `task_controls::run_fenced` worker；相同原 address 的并发 Continue 返回 `InProgress`。
- `HostTaskResume::prepare_native` 在 authority lock 外采集新观察；ExistingTab 使用
  原分享页面的语义正文/refs，不要求额外 screenshot 权限，也不开放坐标输入。
- 最后 `commit_task_resume` 在原 App/process/agent/ACP pin 内发布观察与一次续行。
  迟到返回必须重新核对 suspended scope、原 generation、owner 和原 transport Arc。
- 掉落 UI waiter 不取消已经由 registry 持有的用户 Continue；显式 Stop/owner retirement
  先 fence，原 worker 仍负责结束与 cleanup。不能 abort worker 后说它自然完成。
- 现有 fake capture hook 的并发 Continue、Drop/shutdown 及 publication replacement 回归
  保留。`active-host-resume` 已验证顺序恢复与原地址重复拒绝，但尚未验证真实 MV3
  正文返回在恢复期间迟到时的并发、Pause、Stop 和 owner replacement。

`continuation_count` 是保留记录数，包含 invalid/consumed ticket；可执行续行须用
`claim_continuation` 和原 claim 的 `validate_continuation` 核对。不能把记录数写成权限数，
不能为得到零计数删除用于拒绝重放的历史 ticket。新证据分别命名历史与可执行数量。

## 固定原身份与一个真实门闩

沿现有首次 session MCP request → 原 Pair/Share → App picker → A/B 选中、C 未选 →
新 TaskGrant publication；保持原 client/transport Arc、request key、run、三 document、
原 lease 与原服务 worker。先取得一次原 MCP observation 并保存旧 refs，再由原
SessionManager 显式 Pause，等待原资源 cleanup，而不是设置一个假的 paused 标志。

在 owned fixture 的原 worker 上，对**这一项 recovery 的** `observeDocument` 调用放置
一次门闩：原 `chrome.scripting.executeScript` 实际完成原 document-bound 正文采集后，
暂缓交回原结果。其它 scripting/status/action/poll 不进入门闩；保留原函数、参数和结果，
不替换 DOM、document、grant、receipt 或 adapter。不能通过伪造 observe 回复造成功。

见证固定 original tab/document、这一项调用、门闩 entry 一次、原 Promise 未 settle；
只记录固定 bool/count/phase。实现优先复用原 probe instrumentation，产品不加任意脚本
入口。若这个 native cut point 无法被精确隔离，保留反例，采用绑定原 reservation 的
test-only gate；不得将不相关的延迟或另一次 capture 冒称原恢复。

原 pending/readiness/HTTP/control/lease/15 秒 prepare 预算不增大，不自动重试。
每个用例释放自己的门闩并 join 原任务，失败也必须交回原 ownership 的清理结果。

## 执行顺序与必须成立的后置条件

### R1 并发 Continue：先正向证明门闩与一项 owned worker

首次 Continue 进入原 worker 并卡在上述真实正文结果；第二次用同一旧 address 调用
原 manager，必须返回 `InProgress`，不创建第二个 native capture、worker 或续行。
卡住期间原 grant suspended、无可执行 continuation，正文/截图/输入不能借用恢复权限。

释放原门闩，原 worker 自然完成；同一 run/key/members、更新 generation 与 fresh refs，
恰好一项恢复正文 capture，没有额外截图或输入。只产生一项可执行 continuation，并由
真实 synthetic ACP stdin admission 发送一次、join 一次；重复 delivery 精确拒绝。
旧 refs/input 拒绝，fresh A 操作一次并去重；C 没有读取或输入，Pair/三 Share/document 保留。

### R2/R3 显式 Pause / 原 Chat Stop 先于迟到正文

分别新建独立原 fixture，只在原 recovery capture 被门闩持有后发起实际原 Host Pause
或 `fence_computer_use_chat_stop`。分别核对生产合同规定的状态：Pause 保持 suspended
且需要新的用户 Continue；Chat Stop 清 task authority，原 run 最终实际 `Stopped`。
Pause 的 Host address 来自原 reader 的同一 run/key 行，使用恢复 reservation 已更新的
generation；不拿旧 address 的拒绝冒称实际 Pause，也不从替换后的 slot 借用 address。

释放同一原正文结果并自然 join。旧 capture 不发布为 fresh observation、不产生可执行
续行，旧 claim/delivery 不可复活；零 input/ACP wire，原 Pair、三 Share、document/page
仍在。原 settings/feature 仍 true；不能套用 first-use 失败回滚 false 的合同。

Pause 的新显式 Continue 只在原 worker/cleanup 确证结束后开始；新结果可成功，旧 result
和旧 refs 仍不可执行。Chat Stop 只能通过新的用户 task/request 得到新批准，旧 notice
或显式 chat_started 不能复活停止的任务。

### R4 原 owner replacement：最后 publication 不能跟随新 slot

在同一真实 recovery capture 门闩切点，分别替换原 App process、agent、ACP Arc 和
移除 session slot。固定 original reader/pin，不从替换后的 current slot 重取 identity。
原 worker join 后精确 owner/publication retirement，不产生新执行权限或续行。
原 grant 可按实际合同保留为 suspended metadata；保留行数不能被写成可执行权限。

只验证原 fixture ownership；不得改日常用户 session、Grok home、凭据或浏览器。
替换方由独立只读 witness 证明没有被原 late result 改写。若 replacement 必须另建 ACP
peer，保留原 peer 检查零旧 wire，并各自结束，不共用 receipt。

### R5 迟到 screenshot / input：独立下一小批

在 R1–R4 同候选验证及收尾完成后，再对显式 screenshot 与五动作的原 promise/HTTP
结果设置切点。不得以正文零 capture 推断 screenshot happy path，通过不支持截图
的 headless/无 activeTab 环境只能验证准确拒绝，真人成功路径记 `not_run`。

输入在 invoke 前退休须零 DOM 效果；invoke 后已发生的效果不能删除或回滚伪造零效果，
须保留真实结果/Unknown 及去重，旧 result 不发布新权限、不重放输入。以独立 DOM、
script/capture 计数和原 action 身份验证，不能只靠 registry 数量。

## 证据、门禁与收尾

先 R1 定点与原顺序恢复/managed queued-stop 回归，再 R2/R3，再 R4；每个原子源码
候选重新 fmt、freeze、默认 App artifact。失败的原日志、root/profile、primary/phase/hash
先保存，不覆盖首败，不借前一 candidate 数字。primary 在辅助诊断 await 前持久化。

最终同候选跑完整 native、宽 CU/ACP/routing、App/probe strict、Node/MCP/诊断 unit、
fmt/quality/diff/source Verify；App + AppWorkbench 不增长，≥1000 行文件 budget 不放宽。
Core/前端若改变则补对应全量门禁，不改变时准确标明 `not_run`，不挪用历史结果。

全部原 build/test/browser owners 终态后，新 cleanup 身份 Plan → 回读 → Execute；
保留 current build、seed/依赖/源码及失败现场；复验所有 curated hash。分别报告逻辑
清理量与实际空闲变化；未知 owner、working generation 或其它 worktree 产物 deferred，
不杀进程、不删除整个 target/node_modules/共享缓存。按 AGENTS 第9条每批常规收尾。

之后仍 required：完整 E5、U04–U09、D00–D31、四 surface、Windows/macOS/Linux、
两 Mac 架构、完整原生输入/恢复、签名安装升级回滚、真人 Grok、最终同候选 12h 和
UI/UX。未具备设备或真人操作条件的行准确标 `not_run`，不以 fixture 宣布全项目可交付。

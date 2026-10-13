# Grok U05：一次点击继续任务，后台复验和新观察

状态：**Desktop Core + App backend + shared UI implemented / other surfaces and access recovery pending / partial**，见
[UI checkpoint](2026-10-04-computer-use-task-consent-u05-ui-checkpoint.md)、
[App checkpoint](2026-10-04-computer-use-task-consent-u05-app-checkpoint.md)及
[Core checkpoint](2026-10-03-computer-use-task-consent-u05-core-checkpoint.md)。
原会话恢复/typed续行后端、共享 UI Continue已接；其它surface与必要增量恢复尚未完成。
Codex已接手；U03相应UI/控制接口已接线，见
[UI checkpoint](2026-10-03-computer-use-task-consent-u03-ui-checkpoint.md)。复核当前U04范围
及具体surface readiness后分步实现本批；单Codex writer，不启动Grok/子代理或launcher。
完整U00–U09/D00–D31及Windows/macOS/Linux范围保持，不自行提交发布。

## 本批产品结果

用户主动暂停或物理接管后，点一次“继续”即可恢复原任务。Host后台检查原批准范围、
原资源退出和平台权限，取得新执行身份，fresh observe后把任务继续到原模型会话。
不要求重选同一目标、再点授权、再发“继续”。范围已失效或系统缺权限时准确显示
必要手势/增量请求，不自动选同名窗口、不后台复活旧grant、不重放unknown动作。

Stop/feature-off/completed为终态，不能通过Continue重新启用；新任务走新申请。
本批复用既有TaskGrant/批准范围记忆与执行恢复，不发明第二份UI权限真相。

## 0. 已查到的旧路径与先红回归

- `computer-use-core/src/broker/resume.rs::resume`已有独占admission和锁外surface查询，
  但末尾`drop_execution_identity`清掉target与snapshot，随后需要再次手动授权。
- `broker/lifecycle.rs::pause_due_to`取消执行并保留cleanupPending；`stop_state`依赖
  原adapter idle/in_flight/cleanup而释放lease，不能只看暂停bool。
- `broker/tests_identity.rs::resume_drops_authorization_snapshot_and_lease`表达旧合同。
- `src/commands/computer_use.rs::computer_use_resume`直接调用旧resume；
  `src/components/computer-use/useComputerController.ts::runCommand`只刷新status。
- U03后的卡片/面板共用状态源与控制器，U02已有typed续行调度，U04已有真实批准范围。

先写owned adapter+严格transport联测：正确commit→fresh observe→真实动作→pause→
一次Host用户continue→新观察→合法新动作，两个业务效果各一次，旧ref持续被拒绝。
原旧resume路径必须真实红；保存原断言/日志，不直接删旧测试来报绿。

## 1. 三类身份与显式用户恢复

批准范围记忆是非执行记录；短期ExecutionBinding和模型Observation分别退休。
pause即时fence全部原输入、queued execution continuation与旧snapshot/geometry/ref，
批准范围可保留供用户同任务恢复，不保留可执行lease/grant来绕过pause。
pending/denied intent不是批准范围；不能用它恢复。

Continue来自真实Host用户事件，核app/agent/incarnation/connection/task/grant revision/
feature epoch与当前暂停状态；模型authorize/resume/reconnect入口继续不可用。
同session/agent用户恢复可复验原范围。compact/换模型/ACP重连后的用户继续需新agent
绑定和明确Host规则，不采用旧agent票据。App重启/fork/account/root/remote切换不能继承。

双击共用一次恢复reservation，重复事件只回相同准确状态；React remount不重新派恢复。
请求准备失败或模型暂busy不要求用户二次点击；仍有效的内部阶段可以继续排队。
真正派发后的unknown不能盲重试，不把not_started和unknown混为一个recoverable error。

## 2. 恢复事务，不阻塞Stop

1. 权威锁内校验并预留单次resume generation，状态为resuming，执行仍禁止。
2. 原pause/Stop/action/worker/clipboard资源按精确原owner收尾并确认terminal/idle。
   慢cleanup显示等待状态；不能adapter bool为空或缺PID就释放admission/lease。
3. 锁外核U04 Bound scope当前membership/platform permission、executor/context/profile/
   view/window实例及原身份；目标已死或replacement应失败/needs_access，不能重新按名字匹配。
4. 原owner退出后取得必要排他lease、重新绑定execution identity。lease被另一会话持有时
   给准确等待/占用，不踢对方、不借它授权。managed profile可保留owned范围，权限仍新生成。
5. 在当前完整恢复身份下fresh model observe，核真实图像/语义/geometry与目标身份。
   UI preview不是这份观察；旧preview回包丢弃，fresh observe失败不画running。
6. 最终锁内复验resume reservation/owner/task/revision/epoch/Stop及观察发布身份；
   原子发布新execution grant与给U02的typed续行票据。新输入只在这一阶段以后准入。
7. 原会话busy/gate时由U02队列等待真正prompt结束，自动续行一次；不用重发用户原话。
   发送前Stop/retirement撤票，发送后unknown保留账本，不生成另一条prompt碰运气。

平台查询、abort、observe、lease I/O不长持Broker权威mutex；Stop只fence并交owned cleanup。
在每阶段设置可控barrier验证后到Pause/Stop/feature-off胜出，迟到资源由原owner收束。
不能仅在API开头多做一次check来替代最终action admission的权威fence。

## 3. 失败恢复与UI

卡片/面板都调用相同Continue命令，状态如实区分paused、resuming/等待清理、必要系统权限、
needs_access、failed/unknown、running；用户随时能Stop，busy不能把停止统一禁用。
目标失效/OS撤权需要必要picker/系统手势时，保留本次恢复上下文，一次准确流程完成后
继续；不为其余仍有效范围再弹授权。用户拒绝不自动重新请求。
锁屏/解锁、系统权限恢复和focus变化本身不会自动继续，仍等用户显式Continue。
DPI/拓扑改变退休旧geometry/ref，再按真实平台合同观察；不能继续使用之前坐标。
unknown动作先独立观察真实效果并让原任务据实继续，原actionId绝不再次产生副作用。

使用U03已有UI层次/订阅，不增加App/AppWorkbench state；15locale同步自然短文案。
恢复错误先给可执行的原因/动作，技术trace折叠；无实现的Continue不能用mock成功启用。

## 4. 必需验收与交审

每类用合法请求与实际owned效果验证，不靠schema错误充当权限拒绝：

| 场景 | 必须验证 |
| --- | --- |
| pause→一次continue | 新观察与新身份；新动作一次；旧ref前后都拒绝；额外授权/手发消息0 |
| slow action/abort/cleanup | original cancellation；原lease未退前新执行0；Stop回执有界；终态后恢复 |
| 两个并发Continue | 一个prepare/observe/commit/ticket/prompt；重复结果不双消费 |
| Stop/Pause/feature-off竞争 | 每个阶段barrier；0 post-fence派发；late资源精确收尾 |
| target/context/view replacement | old scope拒绝；不选同名replacement；必要增量流程准确 |
| 另一chat持desktop lease | 不抢占、不串grant；对方效果与旧资源不受影响 |
| observe失败/权限拒绝/断连 | 准确failed/unknown；无running假状态；原失败保留 |
| model busy/reconnect/late event | 路由原正确会话；旧ticket失效；恢复后自动续行一次 |

保持旧identity/managed resume/quiescence/actionId/Stop测试，改旧合同必须解释原安全谓词
现在如何验证。受影响core/IPC/App/controller/protocol定向→全相关suite、fmt/strict Clippy、
pnpm typecheck/lint/build与locale键检查；不重跑未变CLI oracle或无关全套来填时长。
Windows owned原生与managed browser纵向链分别记；其它OS实现/编译/native证据分行。
真人系统权限/toolbar/App批准不自动操作；synthetic联测不冒称U07正常用户点击验收。

交`2026-10-03-computer-use-task-consent-u05-checkpoint.md`，记录先红、源/runtime hash、
命令真实退出码、业务计数/身份/时序、原owned cleanup/terminal/join，逐实现与验证状态。
准备U06 lifecycle矩阵所需API/负向证据，Codex接收后再派U06。完整交付仍需U07–U09/D00–D31。

不commit/push/PR/merge/tag/release/reset/restore/clean/stash/rebase；不动凭据、共享~/.grok、
代理/VPN/用户profile/日常进程/其它worktree；不自动点击真实系统或CU批准。

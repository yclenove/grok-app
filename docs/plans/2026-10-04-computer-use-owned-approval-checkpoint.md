# U04/U06 增量：原 owned approval 的联合 native 验收

状态：**owned managed approval verified / full Computer Use partial — not releasable**。
工作区 `H:/aicoding/grok-app-computer-use`，分支 `feat/computer-use-implementation`，
HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`。单 Codex writer；原 dirty/untracked/
`fix.patch` 保留，未启动 Grok/子代理、未 commit/push/PR/发布。

## 本批实现与证据范围

生产 `approve_computer_use_managed_task` 与 test-only 隔离入口共用同一个
`approve_managed_with` constructor：原 full-key reservation、ACP owner 前后复验和原 App/ACP
publication pin不被替换。只注入任务临时目录的真实 preference writer、独立 owned worker registry
和原 `BrowserSupervisor`；实际执行原 `task_access::approve_with_publication` 全事务，
不再手动分拆 publish_feature/prepare_execution/prepare_publication 来替代 owned approval。
生产默认-off、权限、预算、deadline、epoch和Stop行为未放宽；未改Core/生产前端。

source Node/Chromium 创建实际独立 profile/context/tab与native proof。fixture仅在原 launch
返回前和原最终 publication callback 前添加明确synthetic gate，不替换浏览器结果或重放关闭。
记录固定 route 名及原 launch/close phase，不记录header/body/token/页面URL/内容。
fixture与fixture-only依赖均不进入产品bundle。

**仍是 synthetic Host gesture、App session、ACP peer及gate，不是用户主窗口点击、真实Grok或安装态E2E。**
正向续行是主动调用生产 `computer_use_decision` / `prompt_computer_use`，对原保留的本地ACP peer
实读一条 `session/prompt` 并实写/等待其回执；不是自动SessionManager turn dispatcher或模型后续工具循环。
没有将先前production UI synthetic IPC与本批native结果拼接成完整真人链。

## 五条联合场景

1. 正常批准：request/readback零worker/profile/执行；一次gesture才启动runtime。
   原非执行run→实际Bound→原pin commit；preference/broker/Host开关沿原owned事务。
   native准备/最终commit前零grant/continuation/observe/input；一次fresh observe只读原tab。
   原agent实际收到一条ACP续行并回执，重复delivery在stdin前拒绝且peer无第二条prompt。
2. native launch已实际完成但原返回被hold时Stop：Stop不等待物理创建，旧run仍StopRequested/
   context occupied；release后迟到结果不能publish，原owned workerrollback，原context最终Stopped。
3. 同launch边界取消waiter：调用方Drop只fence；registry仍保留原worker，release后原prepare/
   rollback/join完成，没有discard后另造cleanup owner。
4. 实际Bound/prepare_publication完成后替换App process：原slot pin拒绝，真实临时settings回滚，
   没有grant/continuation/observe/input；原worker和context确切终态。
5. 同publication边界替换ACP transport Arc但故意保留原bearer有效：仍按原transport身份拒绝，
   不能依赖token已失效碰巧过关；真实preference回滚并join原资源。

五场景都在准备中用同一full-key constructor验证第二gesture拒绝且第二runtime closure不执行。
原 `/open`一次，原native launch/close各一次；负向无observe/input，正向仅一份fresh observe。
成功临时profile/storage/runtime只在原approval/consumer/ACP/Child/kernel Job完成后精准删除。
超时等待保留原JoinHandle到真正join，失败保留场景，不把unknown/StopRequested当Stopped。

## 本批验证

证据 `tools/computer-use-probe/.run/codex-u04-owned-approval-20261004/`，不覆盖上批日志。

- build1因新夹具的Arc receiver和Debug类型要求编译失败，未进入断言；原日志保留。
- build2初次联合五场景全绿；随后完善新测试timeout/error收尾，build3为最终源码候选。
- 最终单项五场景 **5 passed /0 failed**，在最终App CU整组中同五项再次通过；
  没有新发现的生产反例，不冒称本批生产bug先红后绿。
- 默认App与probe all-targets strict Clippy、fmt、quality、diff均exit0。
- Node context-close/bundle audit **28通过**，fixture syntax exit0。
- 默认App `computer_use`宽filter实际 **263 passed /0 failed**，ACP **167**、routing **38**。
  相同于上批`computer_use::`的子集为239（原234+本批5），宽filter另带24个既有command/
  control/store测试；不是本批新增29条，也不是另跑239条。上述集合有重叠，不相加。
  `live_handshake`未运行，synthetic panic负向由原join捕获/保留error，不是漏报后台失败。
- Core及前端未改，未重跑其全量；最近结果为Core1004/1020和前端703文件8031测试，
  是上批历史验证，不算本批重跑。

冻结3083源，`source-files-1.sha256` SHA256
`63921a0f2b5130c1aaef86c8c41f54d0f2e347113f913afa126242aae38c9b01`。
最终默认App features=[]测试harness `grok_app_lib-dc611c8f8bc4f678.exe` SHA256
`a513561a5898cff20a6b3aa43b96924f013cf483e7391f5b112b2ca358d714ba`；原Common Controls
manifest按现有后链嵌入，已实际进入断言。**测试harness不是新发行包。**
immutable seed manifest仍为
`001cf9617ce64b6cb6448c38a848533577a6116f33138bbc242e8e03e873b5ec`。

## 构建卫生

所有原build/test/browser owners终态且CIM回读无匹配owner后，精准删除五组已核对的旧App
incremental，保留各组最新代：**4,559,720,681 logical bytes（4.25GiB，含hardlink）**；
H空闲观察增加 **4,264,976,384 bytes（3.97GiB）**。最新App/Core/immutable seed及缓存hash
均不变。独立`cache-cleanup-plan/before/cache-cleanup.json`保留，旧批清理未重复执行。
3083源清理后逐项再复验不变；成功fixture的临时目录由测试在原Child/Job/approval/consumer
join后删除。源码、当前程序、依赖、包及报告、日志、hash和全部旧失败现场都保留。
删除缓存不在回收站，可由cargo重建；这不是删用户程序、项目或共享cache。

## 下一步和未关闭范围

1. 补feature-off、deadline、runtime spawn/身份查询阶段和post-commit/queued continuation的
   owned联合交错；增加真实主WebView命令→SessionManager自动续行的隔离联合验收。
   当前ACP方法/原App pin/真实存储及native并不替代真实UI/系统用户授权或Grok模型循环。
2. 对原自然cleanup_pending长尾补无hold-close的压力/时序诊断；本批native关闭虽通过，
   **原首败根因仍未关闭**，不能说关闭已经加速或修好了。上批三个失败根及原日志保留。
3. U04其它surface/关联dialog/跨surface/切换/增量与缩小、U05其它surface恢复、U06完整矩阵、
   U07–U09及D00–D31，四surface/Windows macOS Linux/真实App和Grok/签名安装/
   同最终候选12h/完整UI UX均仍开放，不关闭或缩减持续目标。

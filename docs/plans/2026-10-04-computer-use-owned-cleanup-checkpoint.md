# U04/U06 增量：原清理消费者的退出所有权

状态：**cleanup ownership repair verified / full Computer Use partial — not releasable**。
工作区 `H:/aicoding/grok-app-computer-use`，分支 `feat/computer-use-implementation`，
HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`。单 Codex writer，原 dirty/untracked/
`fix.patch` 保留；未启动 Grok/子代理，未 commit/push/PR/发布。

## 本批边界

- 接续 managed UI 批的真实 native `run_cleanup_pending` 首败。原日志与
  `grok-cu-managed-task-8d6dc69f-c9c4-44dd-a8a1-84a8773defcd` 现场不修改、不删除。
- 5 秒是 worker 清理 HTTP **观察者**的回包期限，不是 browser/context 的物理终态；
  期限后原关闭仍持资源。未调大任何输入/HTTP产品期限，未重放动作或 native close。
- 本批既修测试验收漏洞，也修生产 watcher 的退出所有权；不冒称原长尾关闭已加速。

## 已复现与修复

1. `TaskViewFixture::finish` 原先只 unwrap 一次清理回包，且忽略 `StopRequested`。
   已以阻塞的原 abort + 第二收尾消费者真实复现：**0 passed/1 failed**。
   新 test-only 收尾观察器留原 ticket/generation/incarnation，只接受 `Stopped`；
   仅对原 cleanup 的 pending 观察继续等待，保留 pending 次数，期限未确认仍报错/留现场。
   原错误和正在工作的 consumer 不会被一个成功回执覆盖。两条定向回归已绿。
2. 生产 `consent_watch::join` 原先 await 前从 registry `take()` 原句柄。
   调用方取消会丢掉最后的 join owner；下次 join 在物理清理仍忙时误报成功。
   另一个缺口是原 join panic 后，后续 join 将它改写为成功。
   两条真实反例 **0 passed/2 failed**；不是 native 权限失败或编译失败。
   已改为 registry 留存原 Arc/异步 slot，等待方只借原句柄；真实 await 后才退休，
   原 join failure 由进程内状态保留。生产单 consumer、Stop fence、权限/epoch不变。
   最终反例用 join-entry Notify 定位取消边界，不靠睡眠猜首次 await 是否进入。

## 实际 source Chromium 延迟清理证据

新增 test-only `fixtures/task-approval-worker.mjs`，给原 native close 加隔离文件 gate，
不替换关闭结果、不调 deadline、不截获/输出 token、网页或输入；fixtures 被打包审计排除。
实际默认 App/ACP/Core publication 与原清理 consumer 使用 source Node/Chromium、独立 profile。
Host/session/ACP、人工延迟和批准是 synthetic，**不是主窗真人点击、真实 Grok 或 installed E2E**。

- 第一轮未启动：Windows `\\?\` 原生规范路径与 Node temp 普通路径比较不等；
  是新夹具护栏问题，统一 namespaced 比较，仍拒链接和非明确隔离根。
- 第二轮在 Stop 断言失败：错误地把 ledger 中 retained/invalid continuation 数量当可执行权。
  原账本语义已回读；改核 executable grant=0 与 `claim_continuation=None`，未放宽生产权限。
  两份首次失败日志和隔离根保留。
- 第三轮实际通过：清理观察者到 5 秒返回 pending，原 context 仍 occupied；
  release 后原 close event 和 physical promise 各一次，原 run 最终 `Stopped`。
  原生关闭自身测得 **633ms**，另 **5秒为明确synthetic gate**；不能把7.76秒测试时间说成原生性能。
  正文/native effect 为0；原 supervisor/kernel Job 与 worker join 后才删除一次性根。

证据根 `tools/computer-use-probe/.run/codex-u04-owned-cleanup-20261004/`。
`cleanup-red-1`、`watcher-red-1`、`native-cleanup-1/2/3` 均保留不改写。
build1是PowerShell5对cargo stderr的wrapper错误，未进入断言；确认原owner退出后修wrapper。
build2/3/4分别为真实红测/初次收尾绿/原生与watcher红测候选；build5为本批最终默认App候选。

## 最终门禁与构建卫生

| 本批实际验证 | 结果 / 边界 |
| --- | --- |
| 最终默认App features=[] | build5；原Common Controls manifest按现有方案后链嵌入，实际进入断言 |
| Watcher | 3项通过，含两条先红反例；期望的synthetic panic由join捕获为error，不是漏报后台失败 |
| 精确fixture收尾 | 2项通过，包括原consumer忙时等待Stopped和观察期限失败后原owner仍fenced |
| 原生清理 | 最终单项1、默认CU中的同项1均通过；synthetic gate，各保留1次pending回包和原close一次 |
| 默认App CU | 234通过，0失败；包含上述重叠集合，排除live_handshake，不与定向计数相加 |
| ACP / routing | 167 /38通过，集合与CU重叠，不相加 |
| 质量 | 默认App与probe all-targets strict Clippy、fmt、quality、diff均exit0 |
| Node | context-close与bundle audit共28项通过；最终fixture语法检查exit0 |

最终两次native关闭自身分别测得455ms/382ms；5秒gate仍明确synthetic，
非人为延迟的原长尾首败未复现/未关闭根因。Core及生产前端未改，未虚报全量重跑。
未变前端的最近完整门禁仍是 managed UI checkpoint 的703文件8031测试，不算本批重跑。

- 冻结3080源，`source-files-1.sha256`：
  `510135053a13e7d9425d78081dfe435643c8cd23d53bc78ab09a33c7ef1dd37b`。
- 默认App测试harness `grok_app_lib-dc611c8f8bc4f678.exe`：
  `0cbdf91fe8c621dec9697ec235a7bb0932736affe2e9de7014f464d46dd73ffb`。
  这是测试产物，不是新发行包；历史候选hash按各批报告保留，不借用最终候选的验证结果。
- immutable seed manifest保持
  `001cf9617ce64b6cb6448c38a848533577a6116f33138bbc242e8e03e873b5ec`。
- 原build/test/browser owners退出并回读后，精确删除5组旧App incremental：
  **4,487,082,720 logical bytes（4.18GiB，含hardlink）**；H空闲观察增加
  **4,252,872,704 bytes（3.96GiB）**。最新缓存/App/Core/seed hash不变，
  清理报告及before/plan保留。删除的缓存不在回收站，cargo可重建。
  成功原生fixture已按原supervisor/kernel Job join证明清理一次性profile/runtime；
  两个新夹具失败根与上一批自然首败场景不删，源码、依赖、当前程序及报告/日志/hash保留。

## 后续完整范围

1. 补原 owned approval worker 的 managed UI/API→App/ACP→runtime→实际存储→native Bound→
   publication→原续行整链，尤其 Stop/Drop/replacement/late launch 的真实资源终态。
   当前手动 approval/native cleanup 与 production UI synthetic IPC 不能拼成真人整链证明。
2. 对原长尾首败补非人为延迟的关闭/系统 I/O 压力对照；保留首败，不能一次快关闭就关根因。
3. U04其它surface/关联dialog/切换/增量缩小、U05其它surface恢复、U06全部生命周期、
   U07–U09，以及D00–D31/四surface/Windows macOS Linux/真实App与Grok/签名安装/
   同最终候选12h/完整UI UX均保留。此修复不等于整体可交付，不关闭持续目标。

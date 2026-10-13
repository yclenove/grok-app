# ExistingTab：原 publication Cancel / Stop / Drop 检查点

日期2026-10-05；单Codex writer，工作区`H:/aicoding/grok-app-computer-use`，
分支`feat/computer-use-implementation`，HEAD`8d8b71c262ab222748bca32412eea884d4b892e0`。
未启动Grok/子代理，未提交/push/PR/发布；dirty/untracked/fix.patch和全部首败保留。
整体active / partial — not releasable；完整native为48 passed / 1 failed，整套未绿。

## 本批改动

按[publication三终态合同](2026-10-05-computer-use-task-consent-u04-existing-publication-terminals-next.md)
增加原准备run的实际App Cancel/Task Stop，以及真正abort+await原批准outer waiter。
只改test/probe，未修改生产权限策略；与source63比较准确六个源文件变化。

- 原App/agent/ACP/transport pin不变，门闩仍位于原最终publication pin之前。
- `FixtureApprovals`只提供test-only原唯一UUID与targeted abandoned join，不永久stop全局registry。
- 准备点：原run恰好一个，feature/store true，grant/continuation零，原approval owner一个。
- Drop后原inner仍在registry；原outer实际取消，释放同一门闩后targeted join原inner。
- 准确拒绝为`Computer Use approval retired before publication`；Cancel/Drop回Pending，Stop为Cancelled。
- 临时feature/store回滚false，零可执行grant/continuation，原approval启动与pin进入各恰好一次。
- 不把stop_requested当完成：await原run实际Stopped，再核最终原authority。
- 原正文/截图/list及A/B/C六次schema-valid输入准确`feature_disabled`拒绝；独立DOM/script/capture零副作用。
- 原三个document/Share/page、Pair与transport保留；已commit后关闭picker保留grant的反例回归。
- 成功报告验证并输出source63被动连接缓存，真实Pair确认计数1、固定HTTP route/status已实际采到。

Host手势及ACP peer仍synthetic，不是installed App/真人Grok验收。
三个定点通过不代表完整矩阵通过。完整矩阵取得了新的准确反例：publication Cancel
回滚后`/cu/extension-poll`403，紧接着`/cu/extension-disconnect`403；原worker仍live、未detach，
Pair确认1但disconnect请求1。执行通道把临时feature禁用误判为整条连接失效，导致本地Pair退休。
历史source62的两项配对失败尚未取得同样的原HTTP见证，不直接销项或移用新证据。

## 冻结候选与已知实际结果

证据目录：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。
source64：3284 files，manifest SHA256
`978f38a17cbf5b6af08712e5d6c2a4bf5092cc9d3173cd538ce6068f0666328d`。
默认App test EXE SHA256
`259f44a8b9fd478516e74ac8aaff1c7335554285a5f93c2e815bb58ee7b85919`。
新artifact覆盖旧默认EXE，不能再拿artifact62/63运行；所有本批验证指定artifact64。

| 门禁 | 实际结果 |
| --- | --- |
| 默认App no-run / Common Controls manifest | exit0，1m29s，artifact64已记录 |
| 三个publication focused | 3 passed / 0 failed / exit0，47.71s |
| 已commit后Cancel保留grant反例 | 1 passed / exit0，0.01s |
| App / probe strict | exit0，28.25s / 26.84s |
| Node / MCP | 177 / 38 passed，各exit0 |
| connection/phase诊断unit | 5 passed / exit0 |
| 两个改动MJS syntax / fmt / quality / diff | 各exit0；App+Workbench仍13573行 |
| focused/strict/node后源码Verify | 3284 files不变 |
| 完整native | 48 passed / 1 failed / exit101，1080.68s |
| 宽CU | 295 passed / 0 failed / 50 ignored / exit0，125.28s |
| ACP / routing | 178 / 38 passed，各exit0；wrapper耗时3.22s / 2.92s |

原native22237、regression69354、Node67240、build73822、focused40315、strict17116均已终态。
Core/前端本批未改，未全量重跑。上一候选source62的40/6与六项首败只作为历史，不是本批数字。
所有source55–57/62原首败保留；`existing-connection-evidence-cache-before-1.json`已复验37个hash和5个原缺失。

## 清理与下一批

本批新清单`existing-publication-terminals-cache-64-1`已完成Plan→回读三组精确obsolete/current/hash→Execute。
逻辑3897958035 bytes；H空闲实际观察增3662909440 bytes，约3.41GiB；累计实际观察增18428874752 bytes，约17.16GiB。
源码/current/App/Core/cache/seed/fix.patch复验不变；37个旧curated hash、5个原缺失和3个source64新首败hash均未变。
原cleanup38764/97113已终态，清单已执行不得复用。删掉的缓存未入回收站，可由Cargo重建。
不删除整个target/node_modules/worktree，不杀其它进程；unknown/working/failed sites仍保留。source63零删除。

先新候选65+修原执行poll在临时禁用时误断Pair：仅实际HTTP403、原epoch与原认证status
确认`executionReady:false`才转入等待；坏凭据/网络/身份变化仍拒绝，不改Host执行403或续租规则。
需覆盖observation/action poll及negotiate的竞争与原promise/sequence保留；不刷绿关闭原首败。
修复联测及同候选门禁完成后，按[原ACP stdin前Chat Stop合同](2026-10-05-computer-use-task-consent-u04-existing-chat-stop-next.md)，
之后补恢复Pause/Stop/owner替换、并发Continue与迟到正文/截图/input竞争。
descriptor仍关闭，raw parser仍Desktop-only；完整E5、U04–U09、D00–D31、四surface、
Windows/macOS/Linux、双Mac架构、完整输入/恢复、签名安装升级回滚、真人Grok、最终同候选12h及UI/UX保留。
缺设备/真人授权记not_run，整体尚不可交付。

# ExistingTab source54终态与清理

日期2026-10-05。HEAD8d8b71c262ab222748bca32412eea884d4b892e0，单Codex writer；
不启动Grok/子代理，不提交/push/PR/发布，不碰凭据/代理/日常浏览器/共享Grok home。
source54产品consent、permission、lease、input与恢复规则未改。
整体active / partial — not releasable；缺设备/人类授权仍not_run。

## 原候选的实际终态

3276 files，source manifest SHA256
`f16dcf17f1c37a230abc6d7ee063cdbe42aa9affc94cd1bafdc5e01989b28388`；
默认App binary SHA256
`090774006b8e5a8c5c23b919fae6693be19fbc349fa9ffdbd81df80d17076614`。
证据仍位于`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。

| 验证 | 实际结果 |
| --- | --- |
| 默认no-run / Common Controls manifest | exit0，40.17s |
| 主诊断持久化/首败保留 | 2 passed / exit0；首次newline测试错误保留 |
| 完整native joint | 20 passed / 18 failed / exit101，1511.45s；`existing-control-native-matrix-1.txt` |
| 宽App CU | 290 passed / 3 failed / 39 ignored / exit101，230.03s；`existing-control-wide-computer-use-1.txt` |
| ACP / routing | 178 / 38 passed，各exit0，2.70s / 2.32s |
| App / probe strict | 各exit0，30.43s / 28.96s |
| Node / MCP / fmt / quality / diff | 177 / 38 passed与三个门禁exit0 |

原session90856已退出，整体exit101，native失败后继续完成三个宽回归。
73410/86828/86450均已退出。Core/前端本批未改，未全量重跑；不借旧293或33/5总数。

## 全部首败保留与诊断边界

`existing-control54-failure-inventory.json`记录全部18组test、原App根、原peer直接报告的
profile、control/command计数、阶段和三个curated文件的SHA256；不按Temp时间猜映射。
其中8项primary为native-worker-evaluation-timeout/checks0/owned-fixture-open；
3项postcondition-timeout/checks3/share-host-ready；1项timeout/checks2/pair-click；
1项discovery-inactive-unshare为private-rpc-timeout/checks4。
5项原profile无primary记录，缺失保持缺失，不借邻近case推断阶段。
初始化前失败不能当作对应授权/恢复切点失败。所有18组原失败根及旧首败均保留。

宽CU首败：session-replacement-publication在原根
`grok-cu-managed-task-f924628e-9ac3-4afe-8472-a6ff22a2de45`删除时AccessDenied，
原IO诊断icudtl.dat/deleteAccessError32。transport-replacement-publication原根
`grok-cu-managed-task-50d5dd92-b9d6-4db6-9a34-6fa9f368bccc`与waiter-drop-launch原根
`grok-cu-managed-task-cec43dec-9336-4f5e-9778-fdda2336ae24`分别4/3次pending回复，
原consumer未确认Stopped。后续supervisor原Child/Job退出见证不等于consumer此前已收束。
不扩大预算、不把unknown改为成功，三项仍OPEN。

只读假设：Playwright attach使用waitForDebuggerOnStart，并在Runtime启用/worker reload
发送Runtime.runIfWaitingForDebugger；私有native附件没有对应事件见证/resume。
没有观测到原失败worker实际paused/reloaded，不能据此宣称根因或修改生产策略。
后续验证须用原附件/原command具体send/reply证据与受控反例，不用心跳/重试刷绿。

## 本批精确清理

新身份`existing-control-cache-1`，SourceAttempt54/ArtifactAttempt54；Plan回读后首次
Execute被其它session-import隔离target的Rust owner安全defer，没有删除或写before。
该owner退出后重新核精确路径、owners、source及current/retained hashes，Execute成功。
未执行source53旧Plan，未停止或清理其它任务。

清除三组完整旧App incremental generation，逻辑3830000264 bytes（3.57GiB）；
H空闲实际观察增加1764708352 bytes（1.64GiB）。分别记录逻辑和空闲变化，硬链接不按
逻辑大小冒称释放量。`existing-control-cache-1-cleanup.json`保存清单、before/after及复验。
current App/Core、最新缓存、immutable seed、CFT140、依赖、dirty/untracked/fix.patch、
所有首败根/日志/截图/hash保留；incomplete working及未知owner文件仍deferred。
缓存未入回收站，可由Cargo重建。AGENTS第9条为以后每批的常规收尾要求。

下一批执行[choose/discovery五项终态合同](2026-10-05-computer-use-task-consent-u04-existing-control-terminals-next.md)。
E5、U04–U09、D00–D31、四surface、Windows/macOS/Linux、双Mac架构、完整输入/恢复、
签名安装升级回滚、真人Grok与最终同候选12h仍开放；descriptor关闭，raw parser Desktop-only。

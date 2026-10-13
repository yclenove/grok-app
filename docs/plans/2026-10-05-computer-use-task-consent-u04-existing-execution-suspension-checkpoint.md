# ExistingTab：执行暂停恢复与共用 readiness 等待检查点

日期2026-10-05；单Codex writer，工作区`H:/aicoding/grok-app-computer-use`，
分支`feat/computer-use-implementation`，HEAD`8d8b71c262ab222748bca32412eea884d4b892e0`。
未启动Grok/子代理，未提交/push/PR/发布；原dirty/untracked/fix.patch及全部首败现场保留。
整体active / partial — not releasable；source66完整native49 passed / 0 failed。
首次focused仍为2 passed / 1 failed，不能用随后单项通过替换该首败记录。

## 已修复的生产路径

source64完整矩阵取得原HTTP反例：临时feature回滚false后，原执行poll403被扩展误判为
连接失效，随后disconnect403；原worker仍live且Pair仅确认一次。
source65加入精确执行暂停后，强化focused1/2失败；原Stop/Drop现场证明多个恢复等待及popup
状态查询触发status429，然后退休Pair。两批原证据均保留，不放宽Host门禁来规避问题。

source66相对source64准确九个源文件变化，生产改动只有三个扩展模块：

- `pairing-client.mjs`保留真实HTTP状态；仅execution poll/action poll/negotiate的403、
  原session/epoch及有效signal/controller、原认证status确认`executionReady:false`时暂停执行。
  坏凭据、网络、429、协议错误或身份变化仍失败；业务claim/result不转成可重放的暂停。
- `extension-transport.mjs`和`action-transport.mjs`在暂停时等原连接恢复，保留原promise及
  原observation/action sequence；取消、替换和重复sequence仍拒绝，不补做旧副作用。
- 同client、原epoch、同signal的恢复等待共用一条promise/readiness循环；原owner finally
  只清自己的WeakMap entry。没有时间缓存、放宽身份、延长请求期限或提高限流预算。
  原250ms等待及Host800ms/8次限流合同不变。

其余改动是unit及原publication三终态probe：在原pin仍被持有时等待实际执行poll进入，
再触发Cancel/Task Stop/outer waiter Drop，等待原403到达并核对零disconnect。
三项完整矩阵证据均为Pair确认1、disconnect0、实际observation poll403各1，
保留的被动HTTP片段中status429各0；原run实际Stopped、inner joined、零grant/continuation，
原Pair和三个Share/document/page保留，九次读/输入请求均准确拒绝且独立DOM零副作用。

Host手势、App session及ACP peer仍synthetic，浏览器为owned source MV3/CFT140。
这不是安装App、真人工具栏授权或真实Grok业务验收。已commit后关闭picker不撤销grant的回归保留。

## 冻结候选与实际验证

证据目录`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。
source66为3284 files，manifest SHA256
`826d80ad3fec8459b708e495732f952573f274ead6903172271bec0a4fa0a0e9`。
默认App test EXE SHA256
`c46dbdd1d30a6a97b01eb0676de2fa2fb6b1d6369a0343e0d8f28f541af42c79`。
旧默认EXE已被覆盖；本批所有App验证指定artifact66，不能复用64/65 artifact。

| 门禁 | 原实际结果 |
| --- | --- |
| 默认App no-run / manifest | exit0，artifact66已记录 |
| 首次publication focused | 2 passed / 1 failed / exit101，35.06s |
| 首败Drop的单项隔离定位 | 1 passed / exit0，16.85s；不改首次结果 |
| 已commit后Cancel保留grant | 1 passed / exit0，0.01s |
| 完整native联合矩阵 | 49 passed / 0 failed / exit0，1075.52s |
| 宽CU | 295 passed / 0 failed / 50 manual ignored / exit0，127.67s |
| ACP / routing | 178 / 38 passed，各exit0 |
| App / probe strict | 各exit0，wrapper20.62s / 20.91s |
| Node / MCP / connection-phase unit | 184 / 38 / 5 passed，各exit0 |
| syntax / fmt / quality / diff / source Verify | 各exit0；3284源不变，App+Workbench仍13573行 |

Core和前端本批未改，未全量重跑，不借历史数字。原build17034/Node9700、focused53812、
strict66343、Drop定位97546、native25105和regression61377均已终态，未杀无关进程。

## 仍开放的首次失败

`existing-publication-terminals-focused-failure-66-1.json`绑定唯一原App/profile/三个文件hash。
原Drop首败停在`owned-fixture-open`，0 checks、Pair确认0、HTTP记录空；source worker已匹配、
context未关闭，errorFamily仍other。不能归因于本次暂停/限流修复，也不能因为定位和矩阵通过就销项。
后续在同一original fixture补固定open/navigate/focus/tab-query切点及脱敏原evaluation错误分类，
保存primary再做辅助诊断；不扩大期限、不自动重试、不制造新的Pair掩盖原首败。
source62未知配对失败及更早startup/IO/rename/观察首败继续保留原状态，不移用64/65的HTTP证据。

## 清理与继续入口

原owners退出后，新清单`existing-publication-terminals-cache-66-1`完成Plan→回读精确路径/
current/hash→Execute，清五组obsolete App incremental generations。
逻辑4664810467 bytes；H空闲实际观察增2578841600 bytes，约2.40GiB。
累计实际观察增21007716352 bytes，约19.56GiB；逻辑量不冒充真实释放量。
源码/current/App/Core/cache/CFT140/seed/fix.patch复验不变；37个旧hash、5个原缺失、
source64/65/66的三个首败日志及12个curated hash在清理前后均不变。
新增私有`verify-publication-terminal-evidence.ps1`仅复验这些精确旧证据，不读凭据或修改源码。
已执行清单不复用；删除缓存未入回收站，可由Cargo重建。
不删除整个target/node_modules/worktree，working/跨variant/未知owner及失败现场继续保留。
按AGENTS第9条每批常规检查磁盘和收尾清理，无需再等用户提醒。

下一小批按[原ACP stdin前Chat Stop合同](2026-10-05-computer-use-task-consent-u04-existing-chat-stop-next.md)，
冻结新候选67+；优先复用真实stdin helper、原manager reservation和原同步Chat Stop fence，
证明原prompt实际NotStarted/join、独立ACP peer零wire及旧claim不能复活。补上述fixture见证。
随后仍required：恢复中的Pause/Stop/owner替换、并发Continue及迟到正文/截图/input竞争。
descriptor仍关闭，raw parser仍Desktop-only；完整E5、U04–U09、D00–D31、四surface、
Windows/macOS/Linux、双Mac架构、完整输入/恢复、签名安装升级回滚、真人Grok、最终同候选12h及UI/UX保留。
缺设备或真人授权记not_run；局部矩阵通过不等于整体可交付。

# ExistingTab：真实恢复期间的原App / agent / ACP owner替换检查点

日期2026-10-05；单Codex writer，工作区`H:/aicoding/grok-app-computer-use`，
分支`feat/computer-use-implementation`，HEAD`8d8b71c262ab222748bca32412eea884d4b892e0`。
未启动Grok/子代理，未提交/push/PR/发布；dirty/untracked/fix.patch及全部首败保留。
整体active / partial — not releasable，ExistingTab descriptor仍关闭。

本批接在[R1–R3检查点](2026-10-05-computer-use-task-consent-u04-existing-recovery-races-checkpoint.md)
与[R4合同](2026-10-05-computer-use-task-consent-u04-existing-recovery-owner-next.md)之后。
source73全部owners已终态、15组obsolete cache精确清理与全部原hash复验完成后才改源码。

## 原owner与独立替换

仍从首次真实session MCP request/source MV3/认证HTTP/原App picker选A/B未选C、
新TaskGrant与真实synthetic ACP stdin首次delivery开始；旧claim/delivery在首次admission前固定。
原fresh观察之后调用真实manager Pause；原恢复门闩只持有原A document的真实
`observeDocument` executeScript返回。matched=1/documentMatched、原registry UUID/
waiter pending以及零executable/available claim都有实证。

process、agent、实际独立ACP Arc、移除原slot各使用新的独立owned fixture，
按这个顺序逐项定点执行。没有在一个原fixture串联四次替换，也没有用`acp=None`
冒充真实ACP Arc replacement。

替换前固定原endpoint process/agent/ACP Arc、原transport reader、request key/run、
原claim/consumed delivery、原registry entry和原native Promise。替换后只读这些原pin；
新slot的address、grant、claim或receipt不能完成原恢复。

- process只改原synthetic slot的process字段，独立只读见证核对其固定替换值。
- agent只改同slot的agent字段；原reader仍来自原transport。
- ACP创建实际独立`TaskTransportFixture`/TCP peer，原与新broker分开核验。
  新peer request/grant/execution/available claim均零；原和新TCP peer分别检查零wire，
  各自按原ownership结束，不共用receipt或把两边数量加总。
- removed移走原synthetic slot，保留完整原值作为只读身份见证，不重新插回原slot。

交回同一真实原rows后，原worker/waiter自然join，精确
`Computer Use request owner retired`。原registry entry实际退休；没有abort、reset或
抢另一个waiter的结果。原执行回到不可执行Paused metadata，不发布fresh观察/refs，
旧claim验证失败、consumed delivery两次精确NotStarted且零新ACP wire。
historical continuation records与available authority单独报告，没有清ticket刷零。

原Pair/三Share、A/B/C原document/page保留，A/B无输入、C无读取/输入；
正文script只增加原恢复这一项、截图0、动作0。原feature/store仍true。
替换slot字段与新peer的空authority在held/自然join/replay后分别复核，不借current getter重建原身份。
所有成功fixture的原native/browser/control/peer owner都结束后才清成功临时根。

source73→74只涉及9个App test/probe文件，新增其中2个文件；生产Core/前端/权限行为未改。
Host手势、App session、ACP peer仍synthetic，不是安装态UI/真人Grok/三平台验收证据。
source72 B切换首败、source66 owned-fixture-open及更早startup/IO/rename/观察首败仍OPEN。
本批定点通过不销未知旧失败，也未自动重试或增加budget。

## source74冻结与实际门禁

证据目录`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。
source74为3295 files，manifest SHA256
`5560692d4f1efcb98f285369eb22bc950785dc94bac81491c45a61d8d5530b5a`。
默认App test EXE SHA256
`f004722175ec5138c54db6979994f8e7281c5aff33fea854500537c1047a3698`，
来自实际artifact74；默认EXE路径已被覆盖，不能再用artifact73运行。

| 门禁 | 同候选实际结果 |
| --- | --- |
| 默认App no-run / manifest | exit0，artifact74已记录 |
| process / agent定点 | 各1 passed / exit0，16.43s / 17.33s，各12 checks |
| 实际独立ACP Arc / removed定点 | 各1 passed / exit0，18.48s / 18.76s，各12 checks |
| 完整native联合矩阵 | 56 passed / 1 failed / exit101，1194.94s；四项owner替换均通过，整套未绿 |
| 宽CU | 295 passed / 0 failed / 58 manual ignored / exit0，138.17s |
| ACP / routing | 178 / 38 passed，各exit0，2.70s / 2.33s |
| App / probe strict | 两项实际exit0，66.35s / 61.72s |
| Node / MCP / 诊断unit | 184 / 38 / 6 passed，各exit0 |
| 新helper与既有syntax / fmt / quality / diff / Verify | 均exit0；3295源不变 |

Core/前端本批未改，未全量重跑，不移用source73的Core1086/1102数字作为本候选验收。
主existing_joint.rs为962行，新owner module为366行；≥1000行文件budget仍80，
App+AppWorkbench仍13573行。冻结期间未改产品源码，原完整native owner已实际终态。

## source74首次失败：配对请求未收到HTTP响应

完整native的唯一失败为
`existing_task_joint_host_resume_chat_stop_rejects_late_native_return_and_old_delivery`。
primary在`original-app-confirm-required` / `source-popup-unconfirmed-pair-rejected`，
只有2 checks，恢复与Chat Stop主体尚未进入；不能称为Chat Stop权限断言失败。
原source worker启动完成（528ms），仍在原context；没有detach/worker-stop/clear/disconnect。
原`/cu/pairing-challenge`请求记录为failed、status=null，pairingConfirmed=0，
popup显示connectionFailed。现有证据不能区分socket、fetch权限或其它传输原因，根因OPEN。
没有自动重跑、增加deadline、改Pair/权限或用相邻通过项消除首败。

原App root为`C:/Users/Administrator/AppData/Local/Temp/grok-cu-managed-task-e8cc3b17-453b-4ee7-b15b-bd183f16b22d`；
原profile为`C:/Users/Administrator/AppData/Local/Temp/grok-cu-pairing-owned-efa97fd4-1150-4b5c-8ad1-010450469f8c/profile`。
原log SHA256为`81d2a24e9f21808db0ec4ec990b40022b93c7ab45b1055f7fef5ff93e68f2969`。
`existing-recovery-first-failure-74-1.json`保存精确test/root/profile、primary/phase/failure三hash。
记录脚本只增加focused/native路由与实际失败test解析，旧71/72记录不改；
复验脚本可单独复验74，清前后原log及三hash均不变。

## 清理与下一批

source74原build/test/browser owners已退出，无Cargo/rustc owner；未停止日常浏览器。
新身份`existing-owner-cache-74-1`已按Plan→回读精确target/current/hash→Execute收束，不能复用。
仅删3组原App obsolete incremental generations，精确targets在同名plan/before/cleanup JSON。
逻辑删除3867464970 bytes（约3.60 GiB）；H盘freeBefore=245046120448、
freeAfter=248779460608，实际空闲观察增3733340160 bytes（约3.48 GiB）。
空闲观察可能受其它磁盘活动影响，不把逻辑bytes等同实际回收。
保留当前build/Core/cache/依赖/seed/CFT140、source/fix.patch与全部失败现场；
3295源、当前可执行文件及保留的最新cache hashes复验不变。
清前后复验37旧hash/5absences、64–66三日志/12hash、67/68两日志/6hash、71/72两日志/6hash，
以及本批74原日志/三hash均通过。
未知owner、working generations、其它worktree/失败现场deferred，不杀无关进程，
不删除整个target/node_modules/workspace，按AGENTS第9条每批常规收尾。
本系列累计实际空闲观察增加37022396416 bytes（约34.48 GiB）。
删除缓存未进回收站，可通过Cargo构建再生成。

下一批保留配对首败OPEN，先补原失败fetch的精确脱敏见证，不盲改传输；独立按
[R5原截图/五动作迟到结果合同](2026-10-05-computer-use-task-consent-u04-existing-late-actions-next.md)
先click真实preflight/invoke后两切点，再set_value/type_text/scroll/wait与Pause/Chat Stop。
已发生DOM效果不能回滚刷零，Unknown/旧actionId不能重放；真实截图成功路径缺activeTab
授权条件时准确not_run，不伪造图像、不放宽生产manifest权限。

完整E5、U04–U09、D00–D31、四surface、Windows/macOS/Linux/两Mac架构、完整原生
输入/恢复、签名安装升级回滚、真人Grok、最终同候选12h与UI/UX全部仍required。
不缩范围、不宣称完整可交付；raw parser仍Desktop-only。

# ExistingTab：真实 Share 撤销与最终授权提交边界

状态：active / partial — not releasable。单 Codex writer，原 dirty/untracked/fix.patch
全部保留；本批无 commit/push/PR/发布。接首次 MCP request / A/B/C 检查点，不称完整 E5。

## 真实反例与修复

沿原 session MCP 首次 request、Host full key、source MV3/认证 HTTP、原 App picker /
owned approval / ACP publication，不借 legacy mcp-open 或另建 grant。

source16/build19 已实际跑原成功链与三个 choose 前撤销场景：4 passed，78.05s，exit0。
实际 popup Unshare、同 document re-share、同 URL reload/re-share均使原 candidate 拒绝。
重新分享不是旧 receipt 复活，重新加载的 Chrome documentId 也确实不同。

source17/build20 在原 TaskApprovalPin 之前加入 test-only barrier，锁外等待；真实 popup
Unshare 的认证 HTTP 已成功、Host 已回读撤销，随后仍调用不变的原 App/ACP pin。
active A 和 inactive B 两项实际均发布了过期权限：rejected=false / grants=1 /
continuations=1 / owned_workers=0；native 红测 0 passed / 2 failed，35.11s，actual exit101。
仅 fixture 页面及 synthetic Host/ACP peer，无模型 input 副作用；独立 DOM 仍零效果。

根因：prepare_publication 的范围复验发生在锁外，commit_authority 的最后提交只检查
Broker request/run，不再检查原 Host 分享证明。prepare→commit 存在可实际撤销的窗口。

修复：最终提交按 Broker gate → Host registry 的既有锁序，对所有已选 ExistingPage
复核原 connection/shareId/documentId/generation/browser/profile。Host 锁保持到原 consent
finalize 和 run binding 完成，不能“检查后解锁再提交”。同时只读复核 retained executor
generation / target；注册执行器不可替换。不查询 native adapter/DOM/文件，不在 App pin
里丢 owned handle。失败沿原 abort_locked、fence、owned rollback / cleanup / join 收尾。
未选 C 的 Share 变化仍允许原 A/B 提交，不使用全局 Share epoch 无差别撤销其它任务。

## 单测首败与夹具纠正（不隐藏失败）

Core red-1：1 passed / 5 failed，actual exit101。其中 executor 场景并未发生替换，
而是 immutable registry 挡下注册后夹具 unwrap 失败；不能把它算真实过期授权反例。
该项改查 replacement registration 被拒绝、原 adapter Arc/generation 不变且原批准仍成功。
修复前 red-2 实际 2 passed / 4 failed，exit101；四项均到达 stale-authority 断言：
active/inactive Unshare、同文档 inactive re-share、connection expiry。

source19 第一轮修复后权限断言已通过，但四项停在 inactive B 的 is_closed 断言。
B 未借用，Host 没有 TabRecord；is_closed 对 absent record 返回 true，并不是物理关闭。
夹具现明确断言准备前后 B 均未借用、清理前后剩余用户 Share 原样保留。真实 browser
isClosed 的独立断言不变。green-1 的实际失败原日志保留，不改生产返回值或放宽权限断言。
source20 green-2 定向实际 6 passed / 0 failed / exit0；新增 Host callback 持锁合同在全量跑。

## 当前候选与已完成检查

证据根：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。
source20 共3252文件；source-files-20.sha256 SHA256：
`0bcf6386e6ceb6ddebe296f604183b4f0ddc627320dd0f2e620ab447ee0aa0e0`。
默认 App build21 no-run 实际 exit0，嵌入原 CI manifest；executable：
`src-tauri/target/debug/deps/grok_app_lib-63e3432e557be5b6.exe`；SHA256：
`b79ceddb8cc12db4f788df1a20b1a012723e8ef79def4cd111668aa1b1a1509c`。
旧 build18/19/20 同路径已覆盖，仅保留原记录，不按旧 hash 称可回放。

source20 实际 exit0：Core default all-targets1075 / test-support all-targets1091（含 Windows
上的平台协议合同，不是 macOS/Linux 实机）；两种 Core strict clippy3；Node177、独立
embedded audit7、MCP/golden38、fmt6/quality6/diff8、Node syntax。
App/Workbench 合计13573行，无新 UI 字符串。集合重叠不相加。

默认 App 最终 native 六场景两轮各6 passed / 0 failed / exit0，113.87s / 115.86s。
每轮为原成功13 checks及五个负向各6 checks；两类publication均实际 rejected=true /
grants=0 / continuations=0 / owned_workers=0，隔离preference回滚false，原reserved run
到Stopped。未选C与其他用户Share/tab保留；批准前及负向的内容/截图/input零效果。
新profile/session/run独立执行，不是unknown重放。default/probe strict clippy-app8/probe7
实际exit0。宽CU293 passed / 0 failed / 7 manual ignored，121.35s；六个joint manual gate
已如上另跑两轮，另一10分钟deadline manual gate本批未重跑。ACP178、routing38，actual
exits均0，集合重叠不相加。3252源验证后复核不变。前端生产本批未改，未全量重跑。

## 证据与安装边界

真实 source MV3、authenticated HTTP、原 App/ACP publication、session MCP / 新 TaskGrant
路径；Host/Share 手势、App session、ACP peer仍 synthetic，不是真人 toolbar activeTab /
installed App / 真实 Grok / screenshot happy path / 三平台 E2E。
显式 CFT140+ / IncludeIgnored gate 不降级 trusted local 保护，不改 managed Chromium130 seed。
原native红测的两个managed-task根：
`C:/Users/Administrator/AppData/Local/Temp/grok-cu-managed-task-680d1abb-390b-47a7-b079-d2a0d828b0d9`
和`C:/Users/Administrator/AppData/Local/Temp/grok-cu-managed-task-cdf650b2-f380-4801-8b6d-689350ce7107`，
及对应owner-marked pairing profile/joint-failure.json全部保留；失败证据不能当废弃runtime
删除。没有新增生产批准API或模型权限。
ExistingTab descriptor仍关闭，raw parser仍 Desktop-only。

## 构建清理与下一批

本批原build/test/browser owners全部实际退出，新的existing-revocation-cache-1绑定
source20/build21，inspection→Execute两步实际exit0。删除15组完整旧App/Core增量缓存，
逻辑6876117736 bytes（约6.40GiB）；H空闲334082859008→339453853696 bytes，实际观察
增5370994688 bytes（约5.00GiB）。逻辑量含hardlinks，不与实际空间混算。
全部3252源、最新App/Core/CFT/seed/fix.patch与15组current缓存逐项hash复核不变；
plan/before/cleanup.json全部保留，已执行清单不复用。缓存不在回收站，cargo可再生成。
没有删除target/node_modules整体或共享缓存，没有杀其它工作区进程。失败build的working
generation、跨variant不确定缓存/未知runtime与所有失败现场继续保留；两个native红测根
仍在。没有为了空间抹掉失败证据或降低以后验证的依赖要求。

AGENTS第9条继续作为日常每批收尾规则；下一批用新的候选绑定Name和精确inspection，
拒绝reparse/working/live owner/路径逃逸，保留当前产物与证据，记录实际空间变化。

接着做 discovery/reservation 切点、connection/profile/lease 失效、App/agent/ACP
replacement、Stop/取消/Drop、真实 Pause→原 Host恢复；原owner保留至物理settle。
满足安装/原任务 UX 门槛后才开放 descriptor并从真正产品入口复跑。
完整 E5、其它 U04、完整 U05/U06、U07–U09 / D00–D31、四surface、三平台/双Mac架构、
所有输入/恢复、签名安装升级回滚、真人 Grok与同最终候选12h仍开放。
自然21530ms managed close、旧 Node TypeText unknown、App worker_timeout/PermissionDenied
根因未证明，不能因本批复测绿销项。目标保持 active / partial — not releasable。

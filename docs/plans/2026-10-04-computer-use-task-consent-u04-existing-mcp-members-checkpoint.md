# ExistingTab：首次真实 MCP request 与 A/B/C 新 TaskGrant 联测

状态：active / partial — not releasable。单 Codex writer；保留所有原 dirty/untracked/
fix.patch，无 commit/push/PR/发布。本批接 joint 首链，不将局部联测称完整 E5。

## 本批实际完成

首次 request 不再由 fixture 的 broker.submit_access 合成。原 TaskViewFixture 保留旧
synthetic 合同，拆出只创建原 bound ACP/RequestTransport/独立认证 HTTP 的 TransportFixture。
新增普通测试证明构造结束没有 request/grant/execution/continuation，feature 仍 off。
真实 session MCP 先 initialize/tools/list，再发一次 computer_request_access(action=request)，
收到 pending / authority:none 即结束模型 dispatch；原 App readback 取得完整 HostRequestKey
并固定一次，后续 picker/confirm/approve 不跟随新 key，也不接收 Node 提供的 request key。

同一原 App constructor/owned approval/publication、新 TaskGrant 路径现验证三页：显式 Share
A/B/C，但 Host picker 只选择 A/B。模型列出的范围只有 A/B；C 的 open、正文观察、截图观察和
typed click 被拒绝，C 实际计数/输入值/scrollTop 不变，扩展没有向 C 派发 observeDocument/
actDocument，captureVisibleTab 计数始终为零。测试自身的 DOM 读数是独立 oracle，不是模型访问。

A→B→A 经实际 computer_open_target 切换，targetGeneration 前进。原 A 引用切到 B 后拒绝，
原 B 引用及往返前的 A 引用回到 A 后仍拒绝。A/B 均以自身 fresh observation 执行 click、
set_value、type_text、wait、scroll；CJK/emoji 值实际为“你好，世界🙂”，点击去重不重复副作用，
A 点击 0→1→2/B 0→1/C 始终 0，scrollTop A 0→60→120/B 0→60/C 始终 0。
原 App/agent ACP continuation 实际写入一次并 join，duplicate 在 wire 前拒绝。Stop 到原
Stopped 后旧观察拒绝，三个用户 Share 和 tab 均保留；独立 Job/profile owner 收尾。

没有调用 legacy mcp-open、另一套 grant 构造器或模型可用的 approve/resume/reconnect API。
没有放宽默认权限、预算、unknown 重放或原 owner fence。现只加强测试接线；ExistingTab
production descriptor 仍关闭，raw target parser 仍 Desktop-only。

## 原生证据与边界

同 production source MV3、认证 HTTP、原 App/ACP publication、新 TaskGrant、session MCP 和
独立 DOM 效果真实执行。Host/Share 手势、App session 与 ACP peer 仍 synthetic；不是真人
toolbar activeTab、installed App、真实 Grok、截图 happy path 或三平台 E2E。
隔离官方 CFT140+ 前提不变，不修改 managed Chromium130 immutable seed；manual ignored
gate 均显式 IncludeIgnored。每轮独立 profile/session/run，不是重放 unknown action。

source14/build17 的首次请求链 10 checks：native-1/native-2 各 1 passed，13.34s/19.73s，exit0。
source15/build18 的最终多成员链 13 checks：members-native-1/members-native-2 各 1 passed，
24.93s/28.82s，exit0。source14 的成功不能外推到后续源码；最终数字以 source15 为准。
build16 的 E0502 首败保留：Fixture 的 DerefMut 借用了整个 f，ACP peer reader 存活期间
不能再经 Deref 访问 f.client。仅在借 peer 前 clone 原 client，仍检验实际原 wire 与 duplicate
拒绝，未改变生产行为；新 empty-transport 和原 recovery-wire 单测各 1 passed。

## 最终候选与检查

证据根：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。
source15 共 3250 文件；source-files-15.sha256 的 SHA256：
`16da769d75302cd3be6353697a1fc5aa26b074d3eb5f51ec3404a8b098f4b43b`。
默认 App build18 no-run 实际 exit0，嵌入原 CI manifest。executable：
`src-tauri/target/debug/deps/grok_app_lib-63e3432e557be5b6.exe`；SHA256：
`a3eae5960efbf80a8a1bb75f56f5797c1a8e58e7ad9c621b7458f2f21f32c60e`。
build15/build17 的同路径已被后续构建覆盖，不再按旧 hash 运行或称可回放。

source15 actual exit0：宽 App CU293（2 manual ignored；本 joint 另实际跑两轮）、ACP178、
routing38；default strict clippy-app7/probe strict clippy-probe6；Node extension/bundle177 +
独立 embedded audit7，MCP/golden38；fmt5/quality5/diff6/Node syntax。集合重叠不相加。
App/Workbench 仍 13573 行，无新 UI 字符串。Core 和前端生产源码本批未改，没有冒用上批
全量 Core/前端/browser 数字。source15 在验证后全部 3250 项复验不变。

## 构建清理

AGENTS 第9条继续作为每批收尾规则。已清的旧 joint-cache/runtime 清单不能复用。
新增本批候选绑定的 cleanup-batch-cache.ps1：必须新 Name，source/artifact hash 匹配，
inspection 后才可 Execute；只允许明确旧/当前完整增量缓存对，拒绝 reparse、working、
路径逃逸、活跃 owner 和变更的候选，保留当前程序/cache、CFT/seed/依赖及所有失败现场。
检查逻辑大小与实际空闲变化分别记账，删除后复验 source/current/artifact hashes。

本批第一次 inspection 在删除/建清单前被其他 Rust owner 安全挡下，未删文件。只读回读
显示该 rustc 的 out-dir 位于另一个 session-import 的 C 盘临时 target，不是本 CU 工作区。
不为清缓存终止其他任务。其退出后以新 mcp-members-cache-2 identity 进行 inspection→Execute，
两步实际 exit0；`mcp-members-cache-2-plan/before/cleanup.json` 全部保留，完成清单不可复用。
清5组完整旧App增量缓存，逻辑4633814006 bytes；H空闲从336241156096变为339440803840
bytes，实际观察增3199647744 bytes（约2.98 GiB）。逻辑量含hardlinks，不与实际量混算。
3250 source、最新App/Core/CFT/seed/fix.patch及五组current缓存逐项hash复验不变。
删除缓存不在回收站，cargo可再生成；没有杀任何其他任务。失败build的working/不确定
cross-variant/runtime/失败现场仍保留，不能当垃圾。此前约6.28 GiB与本批分开记账。

## 下一批（不缩减完整范围）

1. 真实 Share 在 discovery/choose/reserve/publication 各切点撤销，原请求不晚到发布
   grant/continuation；同 document re-share、reload/同 URL replacement 的旧 proof/candidate 拒绝。
2. connection/profile/lease 失效，以及 App process/agent/ACP replacement、Stop/取消/Drop；
   原 owner 留到物理 settle，不借新连接，不影响未选 C 或另一 owner 的 Share。
3. ExistingTab 实际 Pause→原 Host resume，原 scope 复验和 fresh capture 后原会话续行一次，
   旧 refs 不复活；然后达到安装/原任务 UX 门槛再开放 descriptor，并从真正产品入口复跑。
4. 完整 E5、WebView/其它 U04、完整 U05/U06、U07–U09/D00–D31；四 surface、三平台/
   双 Mac 架构、全部输入/恢复、签名安装升级回滚、真人 Grok 与同最终候选12h仍开放。

旧自然21530ms managed close、旧 Node TypeText unknown、App worker_timeout/PermissionDenied
首败根因仍未证明。本批绿不替这些销项；未标目标完成或可交付。

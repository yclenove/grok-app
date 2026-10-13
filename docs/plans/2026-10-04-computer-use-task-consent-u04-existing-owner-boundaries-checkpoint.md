# ExistingTab：choose/reservation 原 owner 退休检查点

当前 source48/build48；实现[选择/预留 owner 计划](2026-10-04-computer-use-task-consent-u04-existing-owner-boundaries-next.md)
的八项切点。focused **8 passed**，完整 native **36 passed / 1 failed / exit101**，
整套未绿。生产 consent/lease/input/恢复行为未改；整体仍 **active / partial — not releasable**。

工作区 `H:/aicoding/grok-app-computer-use`，分支 `feat/computer-use-implementation`，
HEAD `8d8b71c262ab222748bca32412eea884d4b892e0` 未变。单 Codex writer；全部
原 dirty/untracked/fix.patch 保留，未启动 Grok/子代理，未提交/push/PR/发布。

## 八个实际切点

同一生产首次 session MCP request → 原 App key/picker → source MV3 Pair/Share
与认证 HTTP → A/B 原选择（C 未选）。原 ACP、browser connection、document 和
三个 Share 保持存活；transport 替换保留原合法 bearer，仅换实际 Arc。

| 切点 | 原身份替换 | 准确拒绝 | reservation / approval / run |
| --- | --- | --- | --- |
| choose | App process | original App picker owner changed | 0 / 0 / 无 |
| choose | agent | request owner unavailable | 0 / 0 / 无 |
| choose | ACP Arc | original App picker owner changed | 0 / 0 / 无 |
| choose | transport Arc | original App picker owner changed | 0 / 0 / 无 |
| reservation | App process | request owner retired | 原 reserve join / 1 / 原 run Stopped |
| reservation | agent | request owner retired | 原 reserve join / 1 / 原 run Stopped |
| reservation | ACP Arc | request owner retired | 原 reserve join / 1 / 原 run Stopped |
| reservation | transport Arc | request owner retired | 原 reserve join / 0 / 无 |

表中错误均带 `Computer Use ` 前缀。choose 通过真正的
`SessionManager::approve_computer_use_task_picker`，原 ready receipt 来自生产
load；没有直接绕过 App 检查。reservation 保留原 owned worker 到实际返回/join，
process/agent/ACP 经原 App pin 拒绝并回滚 store/feature=false；transport 在
constructor 前拒绝。零 run 行不制造 Stopped 见证。

每项7checks，均固定读回原 RequestTransport，不跟随替换 slot、不恢复 slot 后
补批准。原 grant/continuation 为零，owner 收束、页面/Share 保留，新 ACP 无
request/grant/execution。替代 ACP 是实际隔离 TCP fixture并 finish，不是 Grok；
process/agent slot 与 Host/Share 手势仍合成，不是 OS 进程死亡、安装版主窗、
真人 toolbar 或真实 Grok E2E。

本批八个源文件为 transport fixture、approval fixture、joint fixture、owner
replacement helper、其新增 boundaries、reservation helper、新 Node owner helper
与 joint Node runner。15/18/20秒门闩/RPC/控制、5秒discovery、30秒lease 和10分钟
request 期限未改。原四项 publication helper 复用固定身份替换，仍有独立切点断言。

直接 MCP negative input 尚有缺口：本批 owner 行调用 read/screenshot/list，并
独立检查 script/capture 与 A/B/C DOM 零效果，**没有直接发 schema-valid
computer_act input**。不能把零效果冒称实际 input-denial 调用证明；下批补齐。

## source47 编译首败与 source48 原观察首败

source47 build 实际 exit101，无 artifact47。原因是测试 helper 的 private
reject_choose 可见性及 deserialize never-type fallback；已用窄可见性和显式
Vec<String> 修复，重新冻结 build48 通过。原47 manifest、jsonl/stderr 保留；
这是编译失败，不是运行时失败。

source48 完整矩阵唯一失败：
`existing_task_joint_active_connection_repair_cannot_revive_original_grant`。
原场景六项 checks 已完成，在 `original-grant-fresh-observation-no-input` 断言
失败；尚未发 repair-challenge，replacement not-started。startup complete，原
worker 未关闭；失败时 popup unpaired，curated HTTP 中有 extension-disconnect
200。`detail=share-host-ready` 是上一阶段残留，不代表 Share timeout。
当前诊断不能区分原 MCP isError 与预期正文断言，也不能确定哪个路径触发退休。
未重跑刷绿、未改生产规则；根因保持 OPEN。

精确原失败根：App/store 为 Temp 下
`grok-cu-managed-task-0f988835-c63b-43e1-8cc2-e6507f8250e4`；浏览器为
`grok-cu-pairing-owned-025e2dcd-e4c0-4890-ad01-769372d85c01`。
仅读取 curated `profile/joint-failure.json`，其清理前后 SHA256 均为
`90c7450804af574f9296782639be36615b72ad9cb340e376eefdd67bb81e3844`。
原错误 text/stack、Cookie/Token、配对记录与任意 browser storage 未读取。

## 同候选实际验证

证据目录 `tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。
source48 **3272 files**，manifest SHA256：
`556797de153b108a7ce59b57082d3704b0c846e8870b849b3485e8061089b724`。
默认 App no-run build48 exit0，2m28s，CI Common Controls manifest 已嵌入。
当前 `src-tauri/target/debug/deps/grok_app_lib-63e3432e557be5b6.exe` SHA256：
`46cd72aca72cd639a4385d2dd526d08834899fa5d9488711d2a73e6e40d83bc7`。
source46 同路径 binary 已覆盖，artifact46 仅历史，不再作为可重放候选。

| 实际运行 | 结果 | 时间 |
| --- | --- | ---: |
| existing-boundary-owner-focused-1 | 8 passed / 0 failed / exit0 | 117.45s |
| existing-boundary-owner-native-matrix-1 | 36 passed / 1 failed / exit101，37项 include-ignored | 856.78s |
| existing-boundary-owner-wide-computer-use-1 | 293 passed / 0 failed / 38 manual ignored / exit0 | 125.11s |
| existing-boundary-owner-wide-acp-client-1 | 178 passed / 0 failed / exit0 | 2.66s |
| existing-boundary-owner-wide-routing-tests-1 | 38 passed / 0 failed / exit0 | 2.34s |

原顺序 pipeline session66866 实际 exit101，保留 native 红结果；后续三个宽
回归均真正执行，没有提前退出或重复启动。八个新增场景与原 publication 四项
在矩阵中均通过；集合重叠不相加。宽 CU 的38 manual ignored 是37项 joint 加
原10分钟deadline；后者本批未重跑。

App default/probe strict Clippy48 实际 exit0，分别49.72s/52.09s；Node48 **177**、
MCP48 **38**、fmt48/quality48/diff48 exit0。Core/前端未改未全量重跑，不借
source46/45 数字。App21+Workbench13552 = **13573** 行未增长。Verify48 在
构建后、清理前后均通过，源与当前 artifact 不变。

## 常规构建清理已执行

所有本批原 build/test/browser owners 实际退出后，新
`existing-boundary-owner-cache-1` 完成 plan inspection、全部3项 target/retained
paths 与6项 preserved hashes 回读，再 Execute；实际 exit0。清3组旧 App
incremental generation，逻辑 **3844882806 bytes（3.58 GiB）**；H 空闲从
**331122765824** 增至 **334851895296** bytes，区间实际观察增加
**3729129472 bytes（3.47 GiB）**。两种数字受硬链接/其它写入影响，不冒称整批
构建净变化。缓存不在回收站，原 Cargo 构建可重新生成；已执行身份不复用。

当前 App/Core binaries、最新缓存、CFT140、不可变 seed、依赖、源码与fix.patch
复验不变；source48 两个失败根、source45 两个失败根及source42 两个删除失败根
清理后均存在。source47 compile jsonl/manifest 和source45 curated JSON 哈希
清理前后不变。没有删除整个 target/node_modules 或另一个工作区。
不完整 `grok_app_lib-28mpg1j4ghpco/s-hmvys28aps-0y4sbl2-working`、跨variant
缓存及未知runtime/失败文件继续 deferred；未杀任何无关进程。
AGENTS第9条已规定以后每批收尾清理并在大构建前检查空间，不等用户提醒。

## 接续仍 required

先按[原观察断连诊断与 input 拒绝计划](2026-10-04-computer-use-task-consent-u04-existing-observation-diagnostic-next.md)
补固定诊断、有限原场景定位，取得证据后才修生产路径；再补十二项 owner 的
直接 MCP input-denial。之后继续 Cancel/Stop/Drop、原 worker 物理 join、ACP
stdin 前 queued continuation 撤销、恢复并发与迟到输出。正常恢复/lease/re-Pair
已验部分不重复开发。

source45 startup、source42 Windows目录删除及历史B/IO/seed rename首败仍保留，
未复现不销根因。source43测试前提已在44修复，首次日志仍保留。descriptor仍
关闭，raw parser仍Desktop-only。完整E5、U04–U09、D00–D31、四surface、三平台/
双Mac架构、全部输入/恢复、签名安装升级回滚、真人Grok与同候选12h仍未闭合；
缺设备或真人权限的门槛仍not_run。

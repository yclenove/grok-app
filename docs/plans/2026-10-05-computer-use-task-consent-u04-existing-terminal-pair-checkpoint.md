# ExistingTab：真实终态与取消后的配对状态

日期2026-10-05，单Codex writer。工作区`H:/aicoding/grok-app-computer-use`，分支
`feat/computer-use-implementation`，HEAD仍`8d8b71c262ab222748bca32412eea884d4b892e0`。
不启动Grok/子代理，不提交/push/PR/发布，不碰凭据、代理、日常浏览器或共享Grok home。
保存全部dirty/untracked/fix.patch和首败。完整目标仍active / partial — not releasable。

## 已证明的反例与修复

本批承接[source54结果](2026-10-05-computer-use-task-consent-u04-existing-control-results.md)
和[choose/discovery终态合同](2026-10-05-computer-use-task-consent-u04-existing-control-terminals-next.md)。

source55新增原App choose/discovery五种终态与exact abandoned registry handle的join见证。
OwnedTasks六项通过；真实native五项均越过原Cancel/Stop/Drop及inner join，随后在
配对保留断言失败，0 passed / 5 failed / exit101，93.38s。
source56定位证明读、截图、list和六次合法输入均正确拒绝，失败detail为`terminal-pair-retained`。
source57再加入初始/最终计数，取得确切反例：confirmed始终1，disconnects从0变成1，
popup变成unpaired；当时原document和三Share仍保留，未获得权限或产生输入。

原链是：任务准备退休 → `/cu/extension-status`被准备门禁拒绝 → PairingClient把状态
失败当作配对失效 → 退休本地连接并发disconnect。`retire_scope_pairing`原合同明确
completed Pair属于用户设备状态；准备取消只应退休未完成的Challenge。

source60仅为status增加窄的被动认证：

- 有效准备或feature enabled时，沿原身份校验与续租路径；兼容生产SW每10秒的合并status续租。
- 准备结束、feature仍off时，验证原origin/token/instance/nonce/generation和原30秒租约，
  返回`ok:true, executionReady:false`，不续租、不新建配对。
- heartbeat、Share更新、browser registration、poll/action准入不开放；扩展客户端不吞掉
  真正鉴权、网络或协议失败。明确关闭功能仍撤销Pair；原租约过期照常退休。
- 原Cancelled/Pending差异、零grant/continuation和原worker退出断言不放宽。没有run的
  场景不制造Stopped，不增加5秒请求/20秒控制/30秒lease等任何预算。

HTTP反例先红：`existing-passive-status-red-1.txt`，2 failed / exit101。
source58/59两次集中fixture碰到原HTTP限流；原失败日志保留。最终把身份负例与
过期/替换/关闭案例分开，并只完成第二个challenge一次；生产限流未修改。

## 当前冻结候选及实际验证

source60共3278 files；manifest SHA256
`335f855bf93644aa6af0d911ee9d81efb78127eb8ec358f126809c96abf6626a`。
默认App binary SHA256
`500b092df8fe9a2ae53bdd09a257b00a5d99bb8bcb40445b5c6333a99bf5d16c`。
证据统一在`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。

| 门禁 | 已观察结果 |
| --- | --- |
| HTTP first-use focused | 13 passed / exit0；`existing-passive-status-core-focused-3.txt` |
| 原native choose/discovery五终态 | 5 passed / exit0，79.73s；每项6checks；`existing-passive-status-terminals-2.txt` |
| 默认App no-run / Common Controls manifest | exit0，51.92s，默认features为空 |
| 默认 / test-support Core全量 | 1085 / 1101 passed，各exit0；不是移用旧数字 |
| Core默认/support、App默认/probe四strict | 全部exit0 |
| 宽App CU | 295 passed / 0 failed / 44 ignored / exit0，133.07s |
| ACP / routing | 178 / 38 passed，各exit0，2.67s / 2.32s |
| 完整native joint | 43 passed / 0 failed / 0 ignored / exit0，978.81s；`existing-passive-status-native-matrix-1.txt` |
| Node / MCP /诊断持久化 | 177 / 38 / 2 passed，各exit0 |
| fmt / quality / diff | 各exit0，App+Workbench仍13573行 |

原session24665已exit0，源码3278项最终复验不变；全流程退出码/耗时记录在
`existing-passive-status-verification-1.json`。owners已终态，没有遗留等待句柄。
`existing-passive-status60-native-inventory.json`记录完整43/0和原日志SHA256
`e0708f40a5f62d22a76fc44efdb22e28adf30b3c4675e6748418b76b42ce79a2`。
前端本批未改，未全量重跑；本候选门禁绿不能替代真人/安装态/三平台验收。

一次native入口未带显式CFT环境，5项均在原启动前拒绝，0/5失败、0.03s；
`existing-passive-status-terminals-1.txt`保留。设置已验证的隔离CFT140路径后，上述
真实五项通过；不是扩大预算、替换原失败或使用日常profile。

真实五项有source MV3/认证HTTP/session MCP/原App入口、A/B/C独立DOM/input/script/capture
零副作用及具体inner handle的退出见证；Host手势、session/ACP peer仍synthetic。
不是installed App、真人Grok、toolbar截图成功或三平台验收。

## 首败与清理

`existing-terminal55-failure-inventory.json`、`existing-terminal56-failure-inventory.json`、
`existing-terminal57-failure-inventory.json`已逐一绑定原peer profile、App root、curated
primary/phase/legacy SHA256。source57原App为
`grok-cu-managed-task-34eaa894-e015-4535-a86e-7d41341e3dea`，原profile为
`grok-cu-pairing-owned-f63c35d8-0838-4d61-9c29-d52e26ab3635/profile`，全部保留。
其log SHA256为`850fe48f6356cd4d418cd12bfe8097611c7b2c35be54ac33185f32a50fe05f5f`。

此前source54清三代、source56清五代，逻辑3830000264/4632660889 bytes，H空闲
分别实际观察增加1764708352/4540919808 bytes，合计约5.87GiB。
source55未执行的旧Plan不得复用；source60须原owners实际退出后再建新Plan、回读、Execute，
本批已按此完成`existing-passive-status-cache-1`：清15组精确旧App/Core generation，
逻辑6977884279 bytes（6.50GiB），H空闲实际观察增加3989278720 bytes（3.72GiB）。
source/current/retained/seed/fix.patch哈希复验不变；失败profile和原日志保留。
清理后source55–57原profile的21个curated证据哈希逐一复验不变。
连同此前两次，实际空闲观察增加10294906880 bytes，约9.59GiB，不能按硬链接逻辑大小冒称释放。
`existing-passive-status-cache-1-cleanup.json`保存before/after、targets与复验。
已执行的清单不得复用；incomplete working/cross-variant/未知owner文件仍deferred，不杀其它任务。
删掉的是可由Cargo重建的缓存，未入回收站；AGENTS第9条为以后每批常规要求。
当前构建、依赖、seed、源码和失败证据保留，不删除整个target/node_modules/worktree。

## 继续顺序与未完成范围

本候选原验证与清理已收束，当前门禁全绿；known失败不扩大预算、不无限刷绿。
source54的worker-evaluation/control初始化失败及宽CU清理AccessDenied/pending consumer
本轮未复现，根因未证明，仍OPEN。新配对修复不能自动关闭这些问题，也不能把
初始化前失败称为授权切点失败。

下一组仍按原合同补reservation/publication的Cancel/Stop/Drop、原准备run回滚与Stopped、
queued ACP续行撤销及恢复竞争。已经commit后的关闭picker不能被误改成Stop。
新源必须新freeze/artifact身份；focused后同候选完整门禁，owners退出后精确清理。

完整E5、U04–U09、D00–D31、四surface、Windows/macOS/Linux、双Mac架构、完整输入/恢复、
签名安装升级回滚、真人Grok与最终同候选12h继续保留。缺设备/真人授权保持not_run，
descriptor仍关闭、raw parser仍Desktop-only；本批局部通过不代表可交付。

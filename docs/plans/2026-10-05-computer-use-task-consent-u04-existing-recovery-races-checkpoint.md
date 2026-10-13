# ExistingTab：真实恢复 worker 与 Continue / Pause / Chat Stop 竞争检查点

日期 2026-10-05；单 Codex writer，工作区 `H:/aicoding/grok-app-computer-use`，
分支 `feat/computer-use-implementation`，HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`。
未启动 Grok/子代理，未提交/push/PR/发布；dirty/untracked、fix.patch 与首次失败现场保留。
整体 active / partial — not releasable；ExistingTab descriptor 仍关闭。

## R1–R3 的实际路径

沿原首次 session MCP request → source MV3 Pair/三 Share → 原 App picker 选 A/B、
C 未选 → 新 TaskGrant/原 ACP stdin 首次 delivery。保留原 claim、consumed delivery、
request key/run、client/transport、document 和 task-controls registry owner。

先在原 run 取正文观察与 old refs，再调用真实 manager Host Pause。恢复门闩只持有
原 A document 的这一项 `observeDocument` 对真实 `chrome.scripting.executeScript`
返回的结果，不伪造 rows、DOM 或 grant；其它 script/status/action 不进入门闩。
原 wrapper、matched=1、documentMatched=true、原 registry UUID 和 waiter pending 均有实证。

- R1：并发第二次 Continue 精确返回 `InProgress`，不启动第二项 capture/worker。
  交回原结果并自然 join，原 run/key/members 保留，fresh generation/refs 与一次续行；
  真实 synthetic ACP stdin 发一次、join 一次，旧 delivery/ref 拒绝，fresh click 去重。
- R2：恢复中真实 Host Pause 使用原 reader 中当前 generation；原迟到 rows 交回并
  自然 join 后精确拒绝，不发布 fresh observation/available claim，不产生 ACP wire。
  原 worker 结束后，新的显式用户 Continue 能恢复；旧结果/refs 不能复活。
- R3：恢复中真实 `fence_computer_use_chat_stop` 先退休权限，迟到结果自然 join 后
  精确拒绝，原 run 最终实际 `Stopped`。旧 consumed delivery 两次精确 `NotStarted`，
  包括显式 `chat_started` 之后，均零 wire，不冒称完整安装态 `SessionManager::stop`。

Pair、三 Share 与 A/B/C 原 document/page 保留；拒绝路径输入/截图为零，C 无读取。
历史 invalid/consumed continuation records 与可执行/available claim 分开报告，不删 ticket 刷零。
Host 手势、App session 与 ACP peer 仍 synthetic，不能作为真人 Grok/安装态 UI 验收。

## 原始失败与生产修正

source70 只完成 R1 的 limited verification：focused 1 passed / 18.82s / 14 checks、
顺序恢复 1 passed / 19.76s、managed queued-stop 1 passed / 3.45s；App/probe 两 strict、
Node184/MCP38/诊断6、syntax/fmt/quality/diff/source Verify 通过。
本候选未跑完整 native、宽回归和 Core 全量，不能移用下一候选的数字。

source71 新三项 focused 为 **2 passed / 1 failed**。原 Host Pause 在恢复中的 suspended
grant 上错误调用要求 executable 的 `ConsentState::recheck_binding`，正确当前 generation
因此被拒绝。Node 后续 private RPC timeout 是 Rust 提前退出控制后的症状，不能冒称
生产 Pause 永久挂死。原日志、profile、primary/phase/hash 已保存在
`existing-recovery-first-failure-71-1.json`。

source72 新增 crate-private `recheck_pause_binding`，只用于 Host Pause 的 ExistingTab 分支。
它允许完全匹配的 retained suspended grant，仍校验原 owner/scope/feature identity；
原 `recheck_binding` 的 executable 要求保留，不开放模型读取/输入、不 mint grant/ticket。
Core 新回归先证明旧 address 拒绝、当前 Pause fence 真实 pending resume，再证明原迟到
capture 拒绝、零权限、新用户 Continue 成功，旧 refs/claim 不复活；prepared/pending 竞争
矩阵也加入真实 Host Pause。Core 定点不是空 filter。

source72 focused 仍 **2 passed / 1 failed，80.53s**：R2 已完成前14 checks，包括真实 Pause、
迟到结果拒绝、新 Continue/fresh click，随后 B 切换失败。primary 为
`resumed-original-group-switch-retains-unselected-isolation` / `original-group-member-b`。
旧 `/cu/extension-result` 403 已记录，但没有证据证明它导致 Pair 断开；根因仍 **OPEN**。
不得据猜测改 rate limit、transport、预算、pacing 或自动重试。原现场/hash 保存在
`existing-recovery-first-failure-72-1.json`，source73 未复现不等于销项。

source73 仅增加 B open/observe 固定 phase 与允许的 error code 诊断，未修未知 B 首败；
source66 owned-fixture-open 和更早 startup/IO/rename/观察首败同样保留 OPEN。

## source73 冻结与实际门禁

证据目录 `tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。
source73 为 3293 files，manifest SHA256
`b4c7d450c29555acce084ec55befb2c50013d910a8a539ebca68404ded5c029a`。
默认 App test EXE SHA256
`e4e964f6f2d2c35cf35e3138746a9a46c35328cfdd4595b7e31a400098aeebc6`，
来自实际 `app-artifact-73.json`；默认路径已覆盖旧 artifact，不再用69/70/71记录运行。

| 门禁 | 同候选实际结果 |
| --- | --- |
| 默认 App no-run / manifest | exit0，artifact73 已记录 |
| R1–R3 focused | 3 passed / 0 failed / exit0，52.45s；14/16/12 checks |
| 顺序恢复 / managed queued-stop | 各1 passed / exit0，17.91s / 3.41s |
| 完整 native 联合矩阵 | 53 passed / 0 failed / exit0，1131.27s |
| Core default / test-support 全量 | 1086 / 1102 passed，各 exit0；14 / 17个 test binaries |
| 宽 CU | 295 passed / 0 failed / 54 manual ignored / exit0，145.79s |
| ACP / routing | 178 / 38 passed，各 exit0，2.75s / 2.34s |
| Core default/support、App/probe strict | 四项实际 exit0 |
| Node / MCP / 诊断 unit | 184 / 38 / 6 passed，各 exit0 |
| 六 syntax / fmt / quality / diff / source Verify | 各 exit0；3293源不变 |

App + AppWorkbench 仍13573行，≥1000行文件 budget仍80；未增加豁免。
前端本批未改，未全量重跑，不移用历史数字。冻结期间不改产品源码、不清活跃产物。

## 清理与下一批

source70 原 owners 终态后，新身份 `existing-recovery-cache-70-1` 经 Plan → 回读 → Execute，
已删除5组明确 obsolete App incremental generations。逻辑4731515206 bytes；
H实际空闲观察增加4612907008 bytes（约4.30 GiB），累计29204668416 bytes（约27.20 GiB）。
缓存未入回收站，Cargo 重建可再生成；current build/Core/cache、CFT140/seed/依赖、
source/fix.patch、原失败现场保留。37旧hash/5原缺失、64–66三日志/12hash、
67/68两日志/6hash在清理前后不变。

source73 的全部原 build/test/browser owners 已实际终态。新身份
`existing-recovery-cache-73-1` 已完成 Plan → 回读精确15个target/current/hash → Execute，
删除5组 App 和10组 Core 的 obsolete incremental generations。逻辑删除7004126533 bytes
（约6.52 GiB），H实际空闲观察增加4084387840 bytes（约3.80 GiB）；
累计实际空闲观察增加33289056256 bytes（约31.00 GiB）。逻辑大小不冒充真实释放量。
source73/current App/Core/cache、seed/CFT140/fix.patch 清前后复验不变。
37旧hash/5原缺失、64–66三日志/12hash、67/68两日志/6hash、71/72两日志/6hash
清前与清后均不变。缓存未入回收站，Cargo重建可再生成；没有删除有用失败现场。
未知 owner、working generation、其它 worktree/失败现场 deferred；
不杀无关进程，不删除整个 target/node_modules/workspace。每批按 AGENTS第9条常规收尾。

收束后按[恢复中的原 owner 替换合同](2026-10-05-computer-use-task-consent-u04-existing-recovery-owner-next.md)
进入新候选74+：先独立 process，再 agent、实际独立 ACP Arc、移除原 slot，均固定原
reader/endpoint/transport/registry/native Promise。R5迟到 screenshot/input 是随后独立小批。

完整 E5、U04–U09、D00–D31、四 surface、Windows/macOS/Linux、两 Mac架构、完整原生
输入/恢复、签名安装升级回滚、真人 Grok、最终同候选12h及UI/UX全部仍required。
外部条件缺失的验收行写not_run，不缩范围、不宣称完整可交付；raw parser仍Desktop-only。

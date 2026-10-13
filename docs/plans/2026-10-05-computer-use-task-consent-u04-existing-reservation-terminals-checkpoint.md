# ExistingTab：原 reservation Cancel / Stop / Drop 检查点

日期2026-10-05。单 Codex writer，工作区 `H:/aicoding/grok-app-computer-use`，分支
`feat/computer-use-implementation`，HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`。
未启动 Grok/子代理，未提交/push/PR/发布；dirty/untracked/fix.patch 和首败均保留。
完整目标仍 active / partial — not releasable。本批完整 native **40 passed / 6 failed**，门禁未全绿。

## 本批改动与真实切点

承接[source60配对状态修复](2026-10-05-computer-use-task-consent-u04-existing-terminal-pair-checkpoint.md)
及[原终态合同](2026-10-04-computer-use-task-consent-u04-existing-bootstrap-cancel-next.md)。
本批只增加 test/probe 范围的原 reservation 三终态，没有新增生产授权行为修改。

- 原 App Cancel、原 App Task Stop、真正 abort + await 原异步批准 waiter。
- 固定原 registry UUID；Drop 后保留原 inner handle 到 targeted join，不清全局 stopping/join_failed。
- 实际 hook 进入次数恰好1；准确原拒绝为 `Computer Use selected window instance retired or changed`。
- 原 request Pending/Cancelled 区别，零 run/approval/grant/continuation/store，以及原 workers 真正退出。
- source MV3 / 认证 HTTP / session MCP；A/B选中、C未选，六次 schema-valid 输入及正文/截图/list准确拒绝。
- 独立 DOM/script/capture 零副作用；原 Pair、三个 document/Share 与页面保留。

原 choose/discovery 五项的隔离后置条件提取为 `verifyTerminalIsolation`，原预算与断言保留。
没有 run 的场景不制造 Stopped。Host手势与 ACP peer 仍 synthetic，不是 installed App / 真人 Grok 验收。

source61 编译遇 E0596：旧 owner replacement 只有共享 `ReservationProbe` 引用，而 ready 被误改为
可变引用。已保持原 `ready(&self)`，新场景单独 `capture_original_entry(&mut self)`；首败编译日志保留。

## 冻结候选与实际结果

全部证据在 `tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。
source62 共3280 files，manifest SHA256
`e4f6da2b42ed41b66f664409ede2f4517e9e29a9fe7e1e59788c198c3cd6b3dc`。
默认 App test EXE SHA256
`d2a9bcbea9e183c0b843cff94710fbe02dddbd75f462860fdb94465baca381cb`；默认 features 为空。
它已覆盖原默认 test EXE，不能再拿 artifact60 运行或冒称本批重跑过 source60 的 Core/前端全量。

| 门禁 | 实际结果 |
| --- | --- |
| 默认 App no-run / Windows Common Controls manifest | exit0，约2m05s |
| 新 reservation focused 首轮 | 2 passed / 1 failed / exit101，33.24s；Stop/Drop通过，Cancel在owned-fixture-open首次失败，0checks |
| 同候选 Cancel 一次定位 | 1 passed / exit0，23.32s；真正 Cancel/inner join/Pair保留通过；不关闭原初始化首败 |
| 完整 native joint | 40 passed / 6 failed / 0 ignored / 实际exit101，1243.41s；整套不绿 |
| 完整矩阵里的原 choose/discovery 五终态 | 五项均通过，各6checks；包含共用 helper 回归 |
| 完整矩阵里的新 reservation 三终态 | Drop通过；Cancel/Stop未到新切点，在原初始化/分享准备失败 |
| 宽 App CU | 295 passed / 0 failed / 47 ignored / exit0，127.51s；manual native另按上行实测 |
| ACP / routing | 178 / 38 passed，各exit0，2.69s / 2.34s |
| 默认 App / probe 两strict | exit0，39.27s / 35.88s |
| Node / MCP /诊断持久化 | 177 / 38 / 2 passed，各exit0 |
| 三个改动 mjs 的 syntax | 各exit0 |
| fmt / quality / diff | 各exit0；App+Workbench仍13573行 |
| 源码最终与清理后 Verify | 3280 files 不变 |

Core与前端本批未改、未全量重跑；未移用旧全量数字。strict/regression原退出码与耗时分别在
`existing-reservation-terminals-verification-strict-1.json` 和
`existing-reservation-terminals-verification-regression-1.json`。
原build73043、focused10679、定位17088、native76241、strict94600、regression94955及
cleanup78410/85160均已终态；无本批遗留运行 owner。native wrapper显示exit1，原run-app记录的实际test exit为101。

## 原失败：不把未到切点的失败混成授权失败

首次 focused 日志 `existing-reservation-terminals-focused-1.txt` SHA256
`f587f2a267bd5d33ad7bf6048b0ddb4f1df669eafaa5d88970daec4fbd9c07e5`；原 App为
`grok-cu-managed-task-ba355907-9668-40e3-8e71-22f4694a0476`，原 profile为
`grok-cu-pairing-owned-6b68b042-384e-4e67-bc9b-1da7132978d7/profile`。
startup complete788ms、source匹配，随后owned-fixture-open失败；未进入 Cancel，根因仍 OPEN。

完整 native 日志 `existing-reservation-terminals-native-matrix-1.txt` SHA256
`a1be00b43783fe72a152792eb95f08614a599fa8564e4d4303fbccccab7cad9b`。
`existing-reservation-terminals-first-failure-1.json` 与
`existing-reservation-terminals-matrix-failure-1.json` 逐一绑定7个原失败 App/profile及 curated hash。
原primary缺失明确记录null，不补造终态；使用原control/phase见证，不按Temp时间猜关联。

| 完整矩阵失败 | 原证据 | 状态 |
| --- | --- | --- |
| profile-tab-id-collision | 11checks；最后为stop-original-run-through-original-mcp；断言失败，popup最终unpaired | OPEN；Stop后的配对保留失败，尚无准确触发渠道证据 |
| connection-repair-before-publication | 4checks；replacement-real-pair-and-shares-no-authority / share-host-ready超时，popup unpaired/shareUnavailable | OPEN；尚未完成原publication拒绝 |
| lease-expiry-before-publication | 2checks；原未确认Pair UI等待超时；control_timeout=true，Node终态尚未收到 | OPEN；未进入lease/publication切点 |
| lease-expiry-during-reservation | control_timeout=true，consumedCommands=1；primary缺失，phase停在popup-navigate | OPEN；未进入reservation切点 |
| reservation-picker-cancel | control_timeout=true，consumedCommands=1；primary缺失；后续原phase到owned-fixture-open约25.59s | OPEN；未进入Cancel切点 |
| reservation-task-stop | 3checks；原分享准备share-host-ready，private-rpc-timeout；未进入start-reservation | OPEN；不能标Task Stop授权失败 |

两项配对失败的辅助worker诊断为空；目前无法区分status、执行轮询或原worker退出触发。
只读代码已核对各条拒绝/退休路径，尚未证明根因，不提前修改生产策略或吞掉失败。
本机一次只读内存观察为总63.77GiB/空闲25.12GiB，不足以证明这些超时由内存或节点导致。
source54等旧worker/control、AccessDenied/pending-consumer及所有历史首败继续 OPEN，不因本批未复现销项。

## 本批清理已完成

原 owners 退出后，新身份 `existing-reservation-terminals-cache-1` 完成 Plan → 回读精确路径/retained/hash → Execute。
清5组旧 App incremental generations，逻辑4671976146 bytes；H空闲实际观察增4471058432 bytes，约4.16GiB。
当前源码、App/Core产物、保留的最新cache、CFT140、immutable seed与fix.patch均复验不变。
清理前后37个原curated证据哈希及5个原本缺失的文件均未变，包括source55–57的原首败。
记录见 `existing-reservation-terminals-cache-1-cleanup.json` 及 before/after evidence verification。
连同此前source54/56/60，累计实际空闲观察增14765965312 bytes，约13.75GiB；不把硬链接逻辑量当实际释放量。

未删除整个target/node_modules/worktree，不杀其它进程。删掉的缓存未入回收站，可由Cargo重建。
incomplete working、跨variant/未知owner及失败现场仍保留；本清单已执行，不得复用。
按 AGENTS 第9条，每批构建前看空间、原owners退出后常规精确清理，无须用户再次提醒。
H空闲长期下降的其它来源尚未定位，本批只报告本清理窗口的观察变化，不归因或删除其它任务产物。

## 下一步

先为两项配对失败补固定、脱敏的原步骤/计数见证：原Stop调用的具体后置条件、原Pair计数/
HTTP固定route+status、原worker物理状态与Share回执。保留原primary优先写入；辅助失败不得覆盖
主失败。若改源码新freeze/artifact63+，分别一次定点定位，不调大预算、不无限刷绿。
控制超时的原phase已有记录，继续核对原startup/worker/control owner，不能统一称权限问题。

随后按[publication三终态下一批合同](2026-10-05-computer-use-task-consent-u04-existing-publication-terminals-next.md)
补原准备run回滚/实际Stopped、queued ACP stdin前chat Stop撤销、恢复竞争；不重新开发已通过的正常链。
descriptor仍关闭、raw parser仍Desktop-only。完整E5、U04–U09、D00–D31、四surface、
Windows/macOS/Linux、双Mac架构、完整输入/恢复、签名安装升级回滚、真人Grok及最终同候选12h保留。
缺设备/真人授权标not_run；本批仍 active / partial — not releasable。

# ExistingTab：原 Chat Stop / ACP stdin 联测检查点

日期 2026-10-05；单 Codex writer，工作区 `H:/aicoding/grok-app-computer-use`，
分支 `feat/computer-use-implementation`，HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`。
未启动 Grok/子代理，未提交/push/PR/发布；全部 dirty/untracked/fix.patch、首败及原现场保留。
整体 active / partial — not releasable。source69 完整 native 为 50 passed / 0 failed，
本批宽回归与门禁也已通过；局部验收不替代仍未完成的完整交付范围。

## 联测与原始失败

沿首次真实 session MCP request/source MV3/认证 HTTP/原 App picker、新 TaskGrant，
批准 A/B、C 未选；原 `manager.reserve_computer_use_delivery` reserve 一次。
握住原 client 的真实 stdin lock，等待原 pending=1、continuation=1、匹配原 turn，
再进入产品 Chat Stop 的第一个同步边界 `fence_computer_use_chat_stop`。
不冒称调用了含 AppHandle 的完整安装态 `SessionManager::stop`。

原 prompt 自然 join，精确 `NotStarted`、未 admitted、pending 空。独立 TCP peer 在
Stop 前后读取一字节证明零 wire，连未完成的 NDJSON 片段也不能漏掉。
原 run 实际 `Stopped`、零 executable grant/continuation；feature/store 仍 true。
正文/截图/list 与 A/B/C 六项 schema-valid input 准确 `target_unauthorized` 拒绝，
独立 DOM/script/capture 无效果；Pair、三 Share/document/page 保留。
重用旧 delivery，在 Stop 后及显式 chat_started 后均精确 `NotStarted`、零 wire。

source67 首次 Existing focused 为 **0 passed / 1 failed**，31.82s。原 queued helper
已通过，错误在 snapshot/reply 前；Node 随后 private RPC timeout 是后续症状。
source68 仅增加固定 bool/count 见证，并拆分只读 fixture 查询以修正行数门禁。
其 Existing focused 仍 **0 passed / 1 failed**，31.85s；原 snapshot 精确证明只有
`continuations=1` 不符合断言，其它原 feature/settings/owner/grant 条件均成立。

根因是联测 oracle 把 `continuation_count` 的**保留记录数**当作可执行权限数。
`ConsentState::invalidate_tickets` 保留旧 ticket 并标 invalid，用来拒绝重放；
`claim_continuation` 不会返回该 ticket。source69 验证原 claim 已失效、无新可执行 claim，
分别报告历史记录与可执行数量，不删除历史 ticket，也不改变生产权限逻辑。
managed queued-stop 复用的 helper 同样增强原 claim 退休断言。

67/68 的原失败、profile、primary/phase/hash 不覆盖；其诊断通过不替代原失败结果。
source66 的 owned-fixture-open 及更早 startup/IO/rename/观察首败仍 OPEN，不能归因于
本次 oracle。现 probe 的 A/B/C 创建流程增加 create/navigate/focus/tab-query 固定切点，
原 CDP failure 只记录类别/允许的 exceptionClass；无重试、预算增大或任意 payload 输出。
primary 在辅助诊断 await 前持久化，原始首败不被 teardown 覆盖。

## 冻结与实际门禁

证据目录 `tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。
source69 为 3287 files，manifest SHA256
`47be1055e6d90b0bce89db1128b392017e9bdda176b28a436212d1a0059d97e7`。
默认 App test EXE SHA256
`b4d754110e708b5da72e911a31ef2738b87e6730dbaf0e9a6ba95b5f5cdcbadc`。
同一默认 EXE 路径已被 artifact69 覆盖；不能再拿 artifact66/67/68 运行。

| 门禁 | 同候选原实际结果 |
| --- | --- |
| 默认 App no-run / manifest | exit0，artifact69 已记录 |
| Existing Chat Stop focused | 1 passed / exit0，17.22s，8 checks |
| managed queued-stop 回归 | 1 passed / exit0，3.41s |
| publication 三终态回归 | 3 passed / exit0，54.43s |
| 完整 native 联合矩阵 | 50 passed / 0 failed / exit0，1141.69s |
| 宽 CU | 295 passed / 0 failed / 51 manual ignored / exit0，126.38s |
| ACP / routing | 178 / 38 passed，各 exit0，2.68s / 2.34s |
| App / probe strict | 各 exit0，37.29s / 37.82s |
| Node / MCP / 诊断 unit | 184 / 38 / 6 passed，各 exit0 |
| syntax / fmt / quality / diff / source Verify | 各 exit0；3287 源不变 |

`existing_joint.rs` 只读 stopped/preparation-after-stop/shared-count 查询移到 `readback.rs`，
保留原输入处理及语义；主文件 996 行，≥1000 行文件 budget 恢复 80，
App + AppWorkbench 仍 13573 行。没有增加豁免。
Core/前端本批未改，未全量重跑，不借历史结果。
本批 build68/69、Node68/69、focused68/69、strict69、native69、regression69 均已终态；
冻结测试期间未修改产品源码或清理其产物，未杀无关进程。

## 产物清理与下批

原 build/test/browser owners 终态后，精确清单 `existing-chat-stop-cache-69-1` 完成
Plan → 回读三个 target/current/hash → Execute，删除三组 obsolete App incremental cache。
逻辑删除 3867595325 bytes，约 3.60 GiB；H 空闲实际观察增加 3584045056 bytes，
约 3.34 GiB。累计实际观察增加 24591761408 bytes，约 22.90 GiB；
逻辑文件大小不冒充真实释放量，不把其它工作区同期磁盘变化计入本批。

source69/current App/Core/cache/seed/CFT140/fix.patch 清理前后复验不变；
37 个旧 curated hash/5 个原缺失、source64–66 三原日志/12 hash、
source67/68 两原日志/6 hash 也在清理前后全部不变。
所有源码/依赖及失败现场保留，执行身份不得复用；未知 owner、working generation、
跨 variant 或其它 worktree 产物 deferred，未杀无关进程。
删除缓存未入回收站，Cargo 重建可再生成；未删除整个 target/node_modules/worktree。

用户要求的 routine cleanup 已写入项目 AGENTS 第9条及通用
`C:/Users/Administrator/.codex/AGENTS.md`，大构建前查空间，每批结束主动收尾。

收束后按[原恢复 worker 与迟到结果竞争合同](2026-10-05-computer-use-task-consent-u04-existing-recovery-races-next.md)
进入并发 Continue、恢复中 Pause/Chat Stop、原 App/process/agent/ACP replacement，
随后单独推进迟到 screenshot/input。不得在本批原 owners 活着时修改 frozen source。
descriptor 仍关闭，raw parser 仍 Desktop-only；完整 E5、U04–U09、D00–D31、四 surface、
Windows/macOS/Linux、双 Mac 架构、完整输入/恢复、签名安装升级回滚、真人 Grok、
最终同候选 12h 与 UI/UX 全部保留。缺设备/真人操作条件记 not_run，局部通过不等于可交付。

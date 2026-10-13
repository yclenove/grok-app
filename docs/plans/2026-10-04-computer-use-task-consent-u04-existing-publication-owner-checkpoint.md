# ExistingTab：原 App/ACP publication owner 替换检查点

当前候选 source46/build46；接
[批准前自然租约竞争](2026-10-04-computer-use-task-consent-u04-existing-preapproval-lease-checkpoint.md)。
本批实现[原 owner 计划](2026-10-04-computer-use-task-consent-u04-existing-owner-next.md)
中的 publication 四项，并增加启动失败的固定脱敏分类与原 worker close 见证。
生产 consent/lease/input/恢复行为未改。整体仍 **active / partial — not releasable**。

工作区 `H:/aicoding/grok-app-computer-use`，分支 `feat/computer-use-implementation`，
HEAD `8d8b71c262ab222748bca32412eea884d4b892e0` 未变。单 Codex writer，
保留全部原 dirty/untracked/`fix.patch`；未启动 Grok/子代理、未提交/push/PR/发布。

## 原路径与四个场景

首次实际 session MCP request → 原完整 key/picker → source MV3 实际 Pair/Share、
认证 HTTP → A/B 原选择（C 未选）→ 原 owned reservation/approval → 原 App pin。
沿原 publication 门闩停在 pin 之前，原 executable grant/continuation 为零，
原 reserved run、feature/store=true 和 owned worker=1 都实际存在。

| 场景 | 实际替换 | process/agent/ACP/transport 原身份保持 |
| --- | --- | --- |
| publication-app-process-replaced | 原 SessionManager slot 的 process_id | false / true / true / true |
| publication-agent-replaced | 原 slot 的 agent_session_id | true / false / true / true |
| publication-acp-replaced | 另一实际隔离 ACP/TCP fixture 的 Arc | true / true / false / true |
| publication-transport-replaced | 同 ACP slot 的另一 RequestTransport Arc，原合法 bearer 不变 | true / true / true / false |

前三项 App/agent slot 变更仍是合成手势；ACP peer 是真实隔离 TCP fixture，不是 Grok。
原 ACP 与原 browser connection 保持存活，3 个 Share/document/Pair 不变，不靠
re-Pair/disconnect 或 token 过期造成拒绝。替代 ACP 不创建 request/grant/execution。

替换后固定原 RequestTransport 的 Host-only 读回，不跟随新 SessionManager slot；
不恢复 slot 后补做旧批准。保留原 production pin，具体拒绝均为
`Computer Use request owner retired`。原 worker release/join、零 grant/continuation、
store/feature 回滚初始 false、原 reserved run 真正 Stopped、用户 Share/页面保留
均已检查。替代 ACP 在 manager Drop 后实际 kill/finish，原 ACP 也实际 finish。

每项 7 checks：off 无模型读取、首次实际 MCP 无权限、原 App 确认要求、原 picker
Unshare/Share、指定 owner 替换且零权限、原 pin 准确拒绝、原任务零权限且 Share 保留。
DOM 独立零效果；模型正文/capture/input 为零，未选 C 单独检查。

修改限于四个测试/验收源文件：transport fixture、joint fixture、新增
`existing_joint/owner_replacement.rs` 与 `existing-task-joint-live.mjs`。
原 15/18/20 秒门闩/RPC/控制、5 秒 discovery、30 秒 lease 和 10 分钟 request
期限未改。不是 OS 进程死亡、安装版主窗、真人 toolbar 或真实 Grok E2E 证明。

## 启动首败：增加诊断，未修根因

source45 原 choose-unshare 在 startup-trusted-local-api 失败，0 checks，首次 MCP
尚未发出。原错误 text/stack 未记录，无法事后分类，仍保持 OPEN。
本批补固定 execution-context-destroyed / worker-closed / protocol-error 等分类，
以及原 worker close/发生阶段布尔见证，不记录任意异常内容、URL、ID 或凭据。
未增加启动 sleep/retry、未放宽 API 要求，不能称为启动修复。

`existing-owner-startup-diagnostic-replay-1` 原场景实际 **1 passed / exit0，17.75s**；
完整本批矩阵也没有复现。两者均只记 non-reproduction，不销原首败。
原失败 App/store 与 browser 根仍为 Temp 下：
`grok-cu-managed-task-9428132c-6ed2-4f28-834c-cc6ce809778e`、
`grok-cu-pairing-owned-664c1858-8c41-44a1-a05a-4c166351ddac`。
只读过 curated `profile/joint-failure.json`，未读取任意 browser storage。

## 同候选实际验证

证据目录：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。
source46 共 **3270** 文件；manifest SHA256：
`3c362cbec0ce302c0fd26fbadec267ca36366be6292a15f751d14faddb6deacb`。
默认 App build46 no-run 实际 exit0，CI Windows Common Controls manifest 已嵌入。
`src-tauri/target/debug/deps/grok_app_lib-63e3432e557be5b6.exe` SHA256：
`d1a71552816847080078cbfe43ecfc5c27158551a0079774752badf44f5cc00d`。
source45 同路径 binary 已被覆盖，旧 hash 仅为历史，不作为可重放候选。

| 实际运行 | 结果 | 时间 |
| --- | --- | ---: |
| existing-publication-owner-focused-1 | 4 passed / 0 failed / exit0，各 7 checks | 71.70s |
| existing-publication-owner-native-matrix-1 | 29 passed / 0 failed / exit0，include-ignored | 767.73s |
| existing-publication-owner-wide-computer-use-1 | 293 passed / 0 failed / 30 manual ignored / exit0 | 124.94s |
| existing-publication-owner-wide-acp-client-1 | 178 passed / 0 failed / exit0 | 2.69s |
| existing-publication-owner-wide-routing-tests-1 | 38 passed / 0 failed / exit0 | 2.34s |

native 29 项包含本批新四项；宽 CU 的 30 manual ignored 包含已另跑的 29 项
joint 与原 10 分钟 request deadline，该 deadline 本批未重跑。集合重叠不相加。
App 默认/probe 两种 strict Clippy、Node **177**、MCP **38**、fmt46/quality46/diff46
实际 exit0。App+Workbench 合计 **13573** 行未增长。
Core/前端本批未改，未全量重跑，不移用 source45 Core/前端数字作为本批通过证据。
Verify46 构建后及清理前后均通过，3270 源不变。

## 本批构建清理

原测试 session 81030 实际 exit0，原 build/test/browser owners 退出。
首个 `existing-publication-owner-cache-1` 在生成 plan 前被短暂 Rust owner 保守阻止，
exit1，零删除；后续只读 inventory 已无 Rust owner，但不能反推原 owner 身份。
该拒绝保存为同名 deferred JSON，没有杀任何进程，也没有复用其身份。

新 `existing-publication-owner-cache-2` 完成 inspection、全部 3 项 target/retained
paths 和 preserved hashes 回读，再 Execute。已清 3 组旧 App incremental
generation，逻辑 **3817897470 bytes（3.56 GiB）**；H 空闲从 **331237875712**
增至 **334913998848** bytes，清理区间实际观察增加 **3676123136 bytes（3.42 GiB）**。
这是清理区间空闲变化，不是整批构建净变化；硬链接/其它写入会影响两种数字。
缓存不在回收站，原 Cargo 构建可以重新生成；已执行清单身份不能复用。

当前 App/Core binary、当前缓存、CFT140、不可变 seed、依赖、源码、dirty/untracked
与 fix.patch 哈希复验不变。source45 两个失败根及 source42 两个删除失败根仍在；
source45 curated 诊断清理前后 SHA256 均为
`06f986764ce96fda29fffd78fee9ceac6ea73542aded404c225b259f3da6292d`。
所有历史失败证据与报告/日志/哈希保留，没有删除整个 target/node_modules 或其它工作区。

未确认废弃的跨 variant 缓存/未知 runtime 与失败文件继续保留。明确 deferred 的
不完整 working generation 为
`src-tauri/target/debug/incremental/grok_app_lib-28mpg1j4ghpco/s-hmvys28aps-0y4sbl2-working`，
已复核仍在。以后按 AGENTS 第 9 条每批收尾，不等用户再次提醒。

## 后续仍 required

下一项按[选择/预留 owner 退休](2026-10-04-computer-use-task-consent-u04-existing-owner-boundaries-next.md)
执行两个切点各四身份；不能把 publication 的准确错误或 Stopped 见证照搬到零 run 行。
再 Cancel/Stop/Drop 与原 worker 实际 join、ACP stdin 前的 queued continuation 撤销，
恢复并发/迟到正文、截图和 input。已验正常恢复、lease、re-Pair 不重复开发。

source45 startup、source42 Windows 目录删除、source43 测试前提及历史 B/IO/seed
rename/初始化首败保持开放；绿回归是未复现，不是根因修复。
ExistingTab descriptor 仍关闭、raw parser 仍 Desktop-only。安装/原任务 UX 门槛后
才考虑开放；完整 E5、U04–U09、D00–D31、四 surface、Windows/macOS/Linux、
双 Mac 架构、全部输入/恢复、签名安装升级回滚、真人 Grok 与最终同候选 12h 仍未闭合。

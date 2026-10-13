# ExistingTab：原 owner 退休后的直接输入拒绝检查点

当前 source50/build50。choose/reservation/publication 的十二个原 owner 场景均已
补上实际、schema-valid MCP 输入调用：focused **12 passed / exit0**；完整 native
**36 passed / 1 failed / exit101**。唯一失败位于 source MV3 启动，尚未进入权限
检查；整套未绿。生产 consent/lease/input/恢复行为未改，整体仍
**active / partial — not releasable**。

工作区 `H:/aicoding/grok-app-computer-use`，分支 `feat/computer-use-implementation`，
HEAD `8d8b71c262ab222748bca32412eea884d4b892e0` 未变。单 Codex writer，全部
dirty/untracked/fix.patch 和历史首次失败现场保留；未启动 Grok/子代理，未提交、
push、PR 或发布。App/AppWorkbench 合计 **13573** 行，未增长。

## 直接 MCP input 拒绝的实际范围

十二项覆盖 choose/reservation/publication，各自四种原 owner 变化：App process、
agent、ACP Arc、相同合法 bearer 下的 RequestTransport Arc。每项实际向 A/B/C
分别调用 `set_value` 与 `type_text`，一次 focused 集共 **72 次请求**。每次完整
synthetic action 都经过真实 MCP `validateActionRequest`；实际调用准确返回
`feature_disabled`，不能把 schema/ref 缺失或解析失败当作权限拒绝。

每次请求后独立检查 A/B/C 的 DOM、原 worker script/input/capture，均零副作用。
最终通过 test-only 固定 `original-owner-authority` 命令读原
`f.original_host_task_view`，原 grant/continuation/worker/store authority 均零；
不跟随替换后的 SessionManager slot。十二项在完整矩阵中也通过，仍保留各原
切点的准确拒绝、原 reservation/publication 收束及无 run 行不制造 Stopped 的规则。

实际 MV3、认证 HTTP、首次 session MCP、原 App ACP/pin/owned worker 都在联合
路径中；Host/Share/进程与 agent 身份手势、ACP peer 仍 synthetic。不是安装版 App、
真人 toolbar、真实 Grok 或 OS 进程死亡证明。

本批三个源文件：

- `tools/computer-use-probe/existing-task-joint-live.mjs`：原首次观察的准确 phase、
  sanitized response 分类和 fixed connection retirement 诊断，并调用直接输入 helper。
- `tools/computer-use-probe/existing-task-owner-replacement.mjs`：完整 action 校验、
  六次真实输入拒绝、逐次独立零效果及最终原 authority 见证。
- `src-tauri/src/session_manager/computer_use/task_publication/tests/existing_joint.rs`：
  仅固定 owner 场景拒绝后可用的原 authority 只读入口，不新增批准或模型能力。

## source48 原观察首败与 source49 诊断

source49 对原 `active-connection-repair` 做一次带新诊断的定位复测，实际
**1 passed / exit0，27.58s**；原 observation 成功，预期 fixture 正文和 document
identity 均存在。这是 **non-reproduction**，不是根因修复。source50 完整矩阵中
该场景也通过，source48 原退休原因仍 OPEN。

source49 manifest SHA256：
`a424130ee65c372ded66bbb3442e6298e68a157104afcb288c56925affcd981d`。
原 evidence 为 `existing-observation-diagnostic-replay-1.txt`。source49 门禁属于
历史候选，不借给 source50；同名 App binary 已被 build50 覆盖，不能声称旧外部
probe 脚本仍能用当前 binary 重放旧候选。

source48 原 App/store root：
`C:/Users/Administrator/AppData/Local/Temp/grok-cu-managed-task-0f988835-c63b-43e1-8cc2-e6507f8250e4`。
原 browser root：
`C:/Users/Administrator/AppData/Local/Temp/grok-cu-pairing-owned-025e2dcd-e4c0-4890-ad01-769372d85c01`。
curated `profile/joint-failure.json` SHA256 保持：
`90c7450804af574f9296782639be36615b72ad9cb340e376eefdd67bb81e3844`。

## source50 新启动首败：0 checks

完整 native 唯一失败为
`existing_task_joint_browser_reopen_cannot_revive_original_grant`：
`stage=source-mv3-start`、`detail=startup-worker-ready`。
`initialWorkerCount=0`、`workerEvents=0`、原 context 未关闭，replacement 尚未开始；
尚未进入 Pair/Share、首次 MCP、grant、原观察或输入，不能归类为权限/输入失败。

原 native 控制先在 **20413ms** 超时，`node_reported=false`；之后原 Node 报告
serviceworker-ready 超时，startup elapsed **30684ms**，原 teardown 已执行。
控制超时与 worker-ready 超时分别记录，不扩大原20/30秒预算或吞掉首败。

精确原 App/store root：
`C:/Users/Administrator/AppData/Local/Temp/grok-cu-managed-task-877e468c-b791-40a8-977d-a590c08b107d`。
原 browser root：
`C:/Users/Administrator/AppData/Local/Temp/grok-cu-pairing-owned-8f750dee-3bdf-4301-9256-4d33164ca3e4`。
curated `profile/joint-failure.json` SHA256：
`17b33d31c11eb1569f25374b3f6213fd12dd3b4aa4f8821e522a4b77189d8b35`。
仅读 curated JSON/目录名，没有读取任意 browser storage、Cookie、Token 或原异常内容。

只读代码确认原 native 初次 install-once 后接 Playwright，再等待 serviceworker，
之后才打开 source popup。已安装但 dormant worker 是否需要 popup 真实消息激活，
仍是待证假设；不能由本次 timeout 推论生产扩展的确切根因。source45 启动首败
和 source42 目录清理首败均保留，不能因其它行通过销项。

## 同候选实际验证

证据目录：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。
source50 **3272** 个源文件，manifest SHA256：
`3e47d33af02cbb3f49b95b7a2d3b821f9137e6b280ac42a996ea551827b7f143`。
默认 no-run build 实际 exit0，56.30s，Common Controls manifest 已嵌入；
`app-artifact-50.json` 的 executable 为
`src-tauri/target/debug/deps/grok_app_lib-63e3432e557be5b6.exe`，SHA256：
`a402605ca875321f3b25cff65be28b61bd68db1b404b2fd4fcf22718ea31b9fb`。

| 实际运行 | 结果 | 时间 / 证据 |
| --- | --- | --- |
| focused owner input | 12 passed / exit0 | 198.84s；`existing-owner-input-focused-1.txt` |
| 完整 native joint | 36 passed / 1 failed / exit101 | 883.16s；`existing-owner-input-native-matrix-1.txt` |
| 宽 App CU | 293 passed / 38 manual ignored / exit0 | 137.52s；`existing-owner-input-wide-computer-use-1.txt` |
| ACP | 178 passed / exit0 | 2.72s；`existing-owner-input-wide-acp-client-1.txt` |
| routing | 38 passed / exit0 | 2.35s；`existing-owner-input-wide-routing-tests-1.txt` |
| App strict Clippy | exit0 | 22.68s；`clippy-app-50.txt` |
| probe strict Clippy | exit0 | 20.13s；`clippy-probe-50.txt` |
| Node | 177 passed / exit0 | `node-50.txt` |
| MCP/protocol | 38 passed / exit0 | `protocol-50.txt` |
| fmt / quality / diff | 实际 exit0 | `fmt-50.txt` / `quality-50.txt` / `diff-50.txt` |

原 sequential session6493 已 terminal，最终 exit101 保留 native 失败；后续宽回归
实际运行并通过。没有因为 wait 到期重启。Core/前端未改，未全量重跑，不移用
历史总数。source50 冻结后未改源文件；计划/交接文档不在本 manifest 内。

## 本批清理

本批原 build/test/browser owners 已退出。新身份 `existing-owner-input-cache-1`
的第一次 inspection 被全局活跃 Rust owner guard 挡下，未生成 plan、未删除。
只读定位显示另一任务的 rustc 写入 Temp 下
`grok-session-import-v54-ui/evidence/v127-native-build-target-r2`；没有停止它，也未
改变清理 guard。其自然退出后重新生成 plan，回读全部精确 target/retained paths、
当前 App/Core/CFT140/seed/fix.patch hashes，再执行；实际 exit0。

仅删除三个旧 App incremental generation（最新各代保留）：

| cache leaf | 删除 generation | 保留 generation |
| --- | --- | --- |
| `grok_app_lib-0n3mcki2m3oer` | `s-hmwf65grcm-1n5shb1-54mt9y8aa8v7ximsqambr64ah` | `s-hmwga8dghv-1dkq59n-18apwgktupnput06qj7bs49xs` |
| `grok_app_lib-2owadcr28y5pk` | `s-hmwf5bo2in-1xz90w6-baepd8fxc6be3qpksrbtm0tr8` | `s-hmwg9ud6k0-12vz58b-1ghomntprealsdh9m7ldsipeu` |
| `grok_app_lib-3h8n2al1obuno` | `s-hmwf2et1qi-1wu3n6j-esq2o2anw25uosdvfs0ne2wya` | `s-hmwg7hbx5e-1dufijh-7a9a378a2gr12s1w5wnlg3ghf` |

逻辑删除 **3861592488 bytes / 3.60 GiB**；H 空闲实际观察增加
**2482601984 bytes / 2.31 GiB**。两种数字分开记录，不把硬链接/其它磁盘活动
混入逻辑删除大小。精确清单、before/report 为
`existing-owner-input-cache-1-{plan,before,cleanup}.json`；已执行身份不得复用。
删除未入回收站，`cargo` 重构可生成这些缓存。本批 CFT140 直接运行，未创建重复
Chromium runtime copy。3272 源、当前 artifact/全部保留 hash 与各最新 cache files
清理前后均不变；全部历史失败根仍在，source48/45/50 curated hash 均复验不变。

保留当前 App/Core/最新 incremental、immutable seed、CFT140、依赖、源码、fix.patch
与全部日志/hash/失败现场。原未完成的
`src-tauri/target/debug/incremental/grok_app_lib-28mpg1j4ghpco/s-hmvys28aps-0y4sbl2-working`
及 cross-variant/未知 owner 文件 deferred。未删除整个 target、node_modules、共享
cache 或其它 worktree；实际清理仅上述三个废弃 generation。后续每批继续先检查
disk-free，原 owners 退出后新清单/回读/hash/执行，不等待用户再次提醒。

## 接续入口与完整范围

下一小批按[启动顺序诊断与 Cancel/Stop/Drop](2026-10-04-computer-use-task-consent-u04-existing-bootstrap-cancel-next.md)：
先构造原 source dormant worker 的确切反例，验证 popup 真实路径/唯一 worker，
保持 install-once 与 browser onStartup 的证据区别；不加 sleep/retry/预算。然后
choose Cancel/Stop、discovery Cancel/Stop/真实 Drop/物理 join，再 reservation/
publication 终态、ACP stdin 前 queued continuation 撤销和恢复并发/迟到输出。

descriptor 仍关闭、raw parser 仍 Desktop-only。完整 E5、U04–U09、D00–D31、
四 surface、Windows/macOS/Linux、双 Mac 架构、完整输入/恢复、签名安装升级回滚、
真人 Grok 和最终同候选12h均未缩减；缺设备/真人授权的项保持 not_run。

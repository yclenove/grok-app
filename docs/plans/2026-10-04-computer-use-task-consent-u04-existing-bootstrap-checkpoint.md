# ExistingTab：真实 popup 唤醒与原生 worker 启动检查点

本批先取得 source51 的确切 dormant worker 反例，再以 source52 修改私有探针
启动顺序。生产 PairingClient、consent、lease、input 和恢复规则未改。
原session66939及本批所有owners已退出：完整native **33 passed / 5 failed / exit101**，
宽回归通过，精准清理已完成。不能由局部通过推论整套通过。
整体仍 **active / partial — not releasable**。

工作区 `H:/aicoding/grok-app-computer-use`，分支 `feat/computer-use-implementation`，
HEAD `8d8b71c262ab222748bca32412eea884d4b892e0` 未变。单 Codex writer，
未启动 Grok/子代理，未提交/push/PR/发布。原 dirty/untracked/fix.patch 和全部
首次失败证据保留；App/AppWorkbench 合计13573行未增长。

## source51 的真实反例

新增固定 `startup-dormant` 原生场景，实际4 checks、**1 passed / exit0，2.58s**。
通过原 App test harness 所属 Job/profile 启动同一隔离 CFT140，源码扩展仅
install-once；按确切 source scope/script URL 锁定唯一 activated registration/version。
原 native worker 从 running 经一次 `ServiceWorker.stopWorker` 真正到 stopped。

停止后的实际见证：

- Chrome 原生 worker runningStatus 为 stopped；原生 source target 数为 **0**。
- Playwright `context.serviceWorkers()` 却仍返回 **1** 个相同 source URL 的旧 facade。
- 原 native worker 保持 stopped，尚未打开 source popup，App 无 request/grant/run。
- 打开真实 source `popup.html` 后，由生产 `popup.js` 自身的 `cu-status` 消息唤醒。
  同一个原 registration/version 回到 running，源码 API/installed ID 验证通过。
- popup 为 unpaired，browser marker 为 started=false/previousId=null；无 Pair/Share、
  首次 MCP、任务权限、伪造 onStartup 或浏览器重启。

这证明旧 Playwright facade 不能充当 native worker 存活见证，也证明真实 popup
能唤醒本次确切 dormant source worker。不是直接重现 source50 的30秒等待超时，
所以 source50 的历史确切触发原因仍 OPEN；source48 原观察断连也仍 OPEN。
未增加任何 sleep/retry/worker restart loop 或原生产时间预算。

source51 manifest：3274 files，SHA256
`03f6707427c4e85ab061888d77e8002d92171c27df9d603345ddc459af0c6da5`。
证据 `existing-bootstrap-dormant-diagnostic-1.txt`。本历史候选仅运行此定位场景，
不借 source50/52 的完整矩阵或门禁总数；后续源变化不可用当前脚本冒称重放51。

## source52 的私有探针修复

`existing-task-bootstrap-live.mjs` 增加共享 `openSourceJointWorker`：先打开真实
source popup，再查询 Chrome 的确切 registration/version running 状态；固定
source URL 的原生 target 必须唯一且与该 version 的 targetId 一致。通过现有
`attachNativeWorker` 建立新原生调试附件，复验 runtime.id、Chrome140+ 与
真实 scripting/capture/trusted storage API。不会接到其它 worker，也不使用
`serviceWorkers()[0]` 或旧 facade 作为执行上下文。

初次 joint 启动与 browser/profile replacement 共用此路径。原扩展首次安装一次，
同 profile 真正重开不重复安装；实际 browser onStartup/新 BrowserSession 与
新 profile 的 started=false 仍分别验证，不以 popup 唤醒代替浏览器重启证明。
原附件归属当前 context，替换/结束时实际 dispose，原 browser/Job 的物理退出
仍独立等待。没有修改生产退休、5秒请求、30秒lease、20秒native控制等规则。

新 dormant 场景也改走同一个 helper，实际 **1 passed / exit0，1.61s**，仍见证
native stopped/targets0/旧 facade1 → 同原 registration/version running；无权限。
Chrome原生ready条件采用原已有15秒 `until`，没有增加原30秒worker等待预算；
condition polling 不会重新安装/启动 worker，也不会重发 Pair/Share 或模型动作。

源文件：新增 bootstrap probe 与固定 Rust bootstrap test；joint runner、原 browser
replacement helper 和 joint test contract接线。固定诊断只输出布尔/数量/phase；
registration/version/target ID 和任意页面 URL、Cookie/Token/配对记录不输出。
本验证不包含安装版 App、真人 toolbar/Grok 或 macOS/Linux。

## source52 同候选验证

证据目录 `tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。
source52 **3274** files，manifest SHA256
`fc1cd90bbeae8655bf04e65129d2a18495dbb689ad9bb4c3206604ed2ab17316`。
默认 no-run build 实际exit0，Common Controls manifest已嵌入，记录
`app-artifact-52.json`。当前 executable
`src-tauri/target/debug/deps/grok_app_lib-63e3432e557be5b6.exe` SHA256：
`a516c8e2eb5162ea47c6bbff4f36d092205101cc96d075415f85c8f72deeb39d`。
source52只改JS探针，故其binary与51相同；源manifest不同，不能混作同一候选。

| 运行 | 当前实际结果 / 状态 | 证据 |
| --- | --- | --- |
| fixed dormant bootstrap | 1 passed / exit0，1.61s | `existing-bootstrap-dormant-fixed-1.txt` |
| browser/profile replacement focused | 3 passed / exit0，172.05s | `existing-bootstrap-browser-focused-1.txt` |
| 完整 native joint | 33 passed / 5 failed / exit101，1112.03s | `existing-bootstrap-native-matrix-1.txt` |
| 宽 App CU | 293 passed / 39 ignored / exit0，200.84s | `existing-bootstrap-wide-computer-use-1.txt` |
| 宽 ACP / routing | 178 / 38 passed，各exit0，2.68s / 2.33s | `existing-bootstrap-wide-acp-client-1.txt` / `existing-bootstrap-wide-routing-tests-1.txt` |
| App strict Clippy | exit0，22.45s | `clippy-app-52.txt` |
| probe strict Clippy | exit0，18.96s | `clippy-probe-52.txt` |
| Node / MCP | 177 / 38 passed，各exit0 | `node-52.txt` / `protocol-52.txt` |
| fmt / quality / diff | 各实际exit0 | `fmt-52.txt` / `quality-52.txt` / `diff-52.txt` |

Core/前端未改，未全量重跑，不搬历史数字。原pipeline保留native exit101，同时
依次完成三组宽回归。source52最后Verify仍3274 files/相同manifest；清理前后也
分别Verify通过。没有因control首败重开原session、覆盖日志或扩大任何预算。

## source52 的五项首次失败

以下App/store root均位于 `C:/Users/Administrator/AppData/Local/Temp`，全部保留。
control timeout与Node实际失败阶段分开记录；缺主阶段JSON就保持未知。

| 原矩阵行 | 原App/store root | 原实际结果 |
| --- | --- | --- |
| profile-tab-id-collision | `grok-cu-managed-task-35f41266-3212-4b52-85cf-9a8905ee685e` | control_timeout=true，node_reported=false，elapsed22617ms；无主阶段JSON |
| active-connection-repair | `grok-cu-managed-task-46bbb50c-6ead-488e-b681-234531ec70d3` | control_timeout=true，node_reported=false，elapsed28691ms；原live Node命令行直接绑定browser root `grok-cu-pairing-owned-90aa47fb-8149-40ba-bffb-7fb468570f97`，无主阶段JSON |
| active-lease-expiry | `grok-cu-managed-task-d528540a-9e70-40be-b3ea-99aac0f5c421` | control_timeout=true，elapsed50485ms；随后Node报告share-host-ready，startup complete617ms，3checks、paired/shareUnavailable，尚未到lease故障切点 |
| boundary_owner_choose_agent | `grok-cu-managed-task-a492f6a2-6947-4e27-91b4-85b68f0930ae` | control_timeout=false，node_reported=false，elapsed14844ms；随后Node报告unshare-host-ready，startup complete729ms，3checks、unpaired，尚未到agent replacement切点 |
| boundary_owner_reservation_process | `grok-cu-managed-task-83c0035a-1a35-42d6-93ba-08aad20ad31b` | control_timeout=true，node_reported=false，elapsed21450ms；无主阶段JSON |

只读了curated failure文件、owner marker和原任务进程信息，未读取任意profile存储。
两个curated文件hash复验如下；除上述repair外，当前旧探针没有直接输出原peer→
profile路径绑定，不能仅凭创建时间称以下browser根与App根一一确切映射：

- `grok-cu-pairing-owned-7f2a742c-01ed-439c-904f-ea55d8dae0e8/profile/joint-failure.json`：
  `14c2e0d5097dafa363080c2df0e06868c2d48966d2e6b43b4ce6f88c8adb6ecb`，share-host-ready。
- `grok-cu-pairing-owned-a8ee50f0-eb69-4d2c-860f-8a84e0031762/profile/joint-failure.json`：
  `b34b4d65eb85ad2049e4196affee9e4d6391a7e2fc3e2e91c8434fbeea27148f`，unshare-host-ready。
- 创建时间相合但绑定未证的碰撞/预留browser根
  `grok-cu-pairing-owned-f0c76a23-307b-405e-a960-9978b50a6c28`、
  `grok-cu-pairing-owned-6252b299-2dc2-44e7-b0a5-c8d15ff6d718`也保留，无主阶段JSON。

五项保持OPEN。系统单次CPU/RAM/IO采样不能证明资源竞争是原因。主阶段证据
当前写在辅助popup/worker诊断之后，后者可能卡住；下一批先补跨Node退出保留的
有界phase证据及原peer直接绑定，不用RPC心跳、测试重试或放宽20秒control窗口。

## 清理和下一项

原owners退出后已执行新身份 `existing-bootstrap-cache-1`：先Plan，回读三组精确
target/retained paths及当前App/Core/CFT140/seed/fix.patch hashes，再Execute。
清理三组旧App增量缓存，逻辑 **3803855328 bytes / 3.54 GiB**，H盘空闲观察增加
**2508877824 bytes / 2.34 GiB**。最新cache每文件hash、当前产物与source52均未变；
本批五个App失败根和全部browser失败根保留。
证据 `existing-bootstrap-cache-1-plan.json`、`existing-bootstrap-cache-1-before.json`、
`existing-bootstrap-cache-1-cleanup.json`。已执行身份不复用。
保留当前构建/依赖、immutable seed、CFT140、源码/fix.patch与全部失败现场；
`grok_app_lib-28mpg1j4ghpco/s-hmvys28aps-0y4sbl2-working`和跨variant/未知owner
文件仍deferred，不停止其它任务。删除未进回收站，但可由cargo重建缓存。

接着按[原control诊断与终态合同](2026-10-05-computer-use-task-consent-u04-existing-control-terminals-next.md)
先补窄phase/主失败先落盘/原peer绑定及旧native附件重复dispose的确切所有权缺口，
再补 choose Cancel/Stop 与 discovery Cancel/Stop/真实Drop/物理join，再 reservation/
publication终态、ACP stdin前queued续行撤销、恢复并发及迟到输出。原观察/startup/
目录清理/seed rename 等首次证据未因本批局部通过销项。

descriptor仍关闭、raw parser仍Desktop-only。完整 E5、U04–U09、D00–D31、四surface、
Windows/macOS/Linux、双Mac架构、完整输入/恢复、签名安装升级回滚、真人Grok、
最终同候选12h均不缩减；缺设备/真人授权的项保持not_run。

# U03 Host 请求回读与原任务决定 checkpoint

状态：**U03 implementation in progress；整体 active / partial — not releasable**。
分支 `feat/computer-use-implementation`，HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`。
接续 [U04 App 范围批准接线](2026-10-03-computer-use-task-consent-u04-app-scope-checkpoint.md)。
Codex 单 writer，原 dirty/untracked 内容保留；没有 commit/push/PR/发布。

## 已实现的合同

`computer_use_task_readback({ sessionId })` 沿 SessionManager 的原本地 endpoint → ACP 的原
RequestTransport → Core authority lock 读取 typed HostTaskView。没有 endpoint 就准确报不可用，
不启动 request/runtime、driver、browser、profile，也不走 legacy `computer_use_status`。
回读不调用 adapter discovery、scope witness、liveness、observe/input，不消费决定或续行票。
Host 期限扫描只交给既有原资源 cleanup consumer；不在回读线程做原生清理。

HostTaskView 的请求独立于 run：

- request：HostRequestKey、typed phase、纯文本 purpose/scopeIntent、canonical surfaces、
  capabilities、monotonic remainingMs；无 run 时也可读取。
- request phase：pending/preparing/approved/denied/cancelled/expired/retired/completed。
  选择 worker 正在工作时有真实 selecting 标记；旧选择 Drop 不能清掉新选择标记。
  preparing 与 model `computer_request_access` 的 pending 表示分别服务 Host 与 model 合同，
  不修改 model 工具 JSON 的既有含义。
- grants：已提交成员的 retained targetId/surface、capabilities、suspended/executable、runId；
  不包含 native proof、transport token、continuation/dispatch credential。
- executions：原 run/generation、typed phase、retained 目标名、cleanupPending 和
  hasFreshObservation。generation 是十进制字符串，避免 JS Number 丢失代次。
  preparing/awaiting_observation/idle/busy/paused/resuming/switching/unknown/
  stop_requested/stopped 分开；批准后未 observe 不能显示成正在执行。
  名称只是原批准元数据，**不是当前窗口存活证明**；回读不查询用户桌面来猜 liveness。

执行行只取当前 owner/task 的 bounded authority records，排除 legacy 和跨会话 run。
准备阶段的 original Pending run 也按原 Preparation 标识回读，不能忽略 commit 前的
Stop/cleanup。清理原 run 的责任仍由原 consumer 保留，不依赖 UI 展示。
同任务大量 terminal run 留在 Broker history 时，不能全部复制到卡片状态。

`computer_use_task_decide({ sessionId, key, decision })` 接受 deny/cancel/stop：

- key = connectionId/taskId/requestId/revision，仅作关联，不是批准凭据。
  Core 在同一锁下核原 transport owner、agent、完整 key 和请求状态。
  DTO 拒绝未知授权字段；前端不能靠 token/runId/targetId 猜一个批准。
- deny/cancel 无 run 时可用，准备中会使原预约失效；拒绝新增请求保留同任务原 grant。
- stop 核 taskId 后 fence 原 owner 的任务资源，再把原 cleanup 交给既有 consumer。
  duplicate Stop 可以幂等，但旧 task 或旧 connection 不能停止新任务，即使 agent 名相同。
- 结果是原 key/decision 的 HostTaskDecisionReceipt。它只确认决定已处理，不声称物理清理完成，
  也不附带随后可能已经切换到新 task 的 readback；实际 cleanup/status 独立回读。
- ACP mutex 只覆盖同步元数据/authority fence，没有 await/native query；SessionManager 前后核
  原 endpoint。错误不回传 token 或底层 transport 内容。

新增 `src/lib/api/computerTask.ts` 接上述命令及 typed DTO，使用 isDesktopHost 门禁，mirror/
浏览器客户端不能通过这个 facade 获得本地决定入口。没有新增用户可见 UI 文案或设置。

## 实际验证

证据根：`tools/computer-use-probe/.run/codex-u03-host-readback-20261003/`。
所有 fixture 决定均 synthetic；未点击真实 App/CU/OS 授权，也未读取凭据。

初次 `core-host-1.txt` exit101：11 pass / 1 fail，失败在 legacy fixture 的 open_run，原因是
fixture 没开启 feature，尚未进入该测试的 readback 断言。只修 fixture 的开关前置，未放宽
生产 feature gate。这个失败不是旧 UI 缺陷的 red test；真正无 run 卡片 UI 的先红验收待下一批。

`app-build-1/2`、`core-full-2`、`app-computer-use-1/2`、`app-acp-client-1` 与早期 Clippy
属于中间源码。最终新增准备 run/有界历史回归后重建候选，不借早期输出宣称最终候选通过。

| 最终检查 | 实际结果/日志 |
| --- | --- |
| Core 全量 test-support/locked/offline | `core-full-3.txt`：714 unit + 220 integration = **934 pass**，0 failed/ignored，exit0 |
| 默认 App 构建 | `app-build-3.jsonl/.stderr.txt`：exit0，实际完成后取 invocation 唯一 features=[] test artifact |
| App CU | `app-computer-use-3.txt`：194 pass，0 failed/ignored，exit0，四线程；含新增 5 项 ACP readback/decision fixtures |
| App ACP | `app-acp-client-2.txt`：140 pass，0 failed/ignored，exit0，四线程；明确 skip live_handshake |
| App routing | `app-routing-2.txt`：38 pass，0 failed/ignored，exit0，四线程 |
| API | `api-3.txt`：三个文件 11 pass，含新增6项 task API 测试，exit0 |
| TypeScript/ESLint | `typecheck-2.txt` exit0；全 src `lint-1.txt` exit0；最终新 API 文件 `lint-2.txt` exit0 |
| rustfmt/diff/quality | `fmt-2.txt`、`diff-2.txt` exit0；`quality-2.txt` final PASS；App 壳/Workbench 未增 state/行数 |
| Strict Clippy | `core-clippy-2.txt`（test-support）、`app-clippy-2.txt`（默认）与 `probe-clippy-2.txt` 均 locked/offline/all-targets/-D warnings，exit0 |
| 最终前端构建 | `ui-build-2.txt` exit0；既有 large-chunk 提示保留，未隐藏 |

App 过滤集合重叠，不能把 194/140/38 相加当独立总测试数。
Core 新增14项覆盖 request-only、零 adapter 副作用/无 credential、选择与预约 preparing、
无 run 的 deny/cancel/Stop、旧 owner/revision/task、增量拒绝、首用取消回滚、deadline/退休、
legacy/foreign 隔离、准备 run 的原清理身份及同 task 40次历史保留时有界回读。
ACP 新增5项用 localhost 自建原连接核真实 RequestTransport 路由、replacement/same-agent、
kill/missing owner 与无 run 决定，不是 native App 手势验收。

最终候选默认 App harness 嵌入 `src-tauri/windows-test-manifest.xml`，避免本机 Common Controls
启动错误；等待编译完全结束后再使用 mt.exe，不把 manifest 注入产品 linker。
`src-tauri/target/debug/deps/grok_app_lib-dc611c8f8bc4f678.exe` SHA256：
`1c91772038d4e19812dc1ae1a221715f5e57615721faf31ffeb42f690373b4f4`。

源子集 `source-files-2.sha256`：原 U04 的 Core/src、App/src、cu-probe/MCP/consent-fixture
算法加本批两个 task API TS 文件；git cached + nonignored untracked、Sort-Object -Unique、
lowercase SHA256 + 两空格 + repo path、LF 终行。**664文件**，manifest SHA256：
`0e161b1d972a738e50d5b3b0c1a3e70a2c311e9d9d3f2fb1c4696080e71b4c2a`。
这是受检子集，不是全部产品的发布清单；未以旧 native/normal CLI 证据冒领本批实机验收。
文档完成后复验664源文件及上述 harness hash 一致；所有本批构建/检查/测试的原工具会话
均已终态并收集，无未回收工作句柄。

## 下一批按顺序继续

1. 补显式 Host picker：只在用户选择范围手势后枚举 bounded 候选；保留原 request/selection/
   executor/native instance，选择 token 留 Host，原 worker 在取消/Stop/重连/晚回包后真正 join。
   开卡/展开目的不提前 discovery/capture，不能复用会启动运行环境的 legacy list_targets。
   当前 Desktop selectedTargets 已有批准合同，其他没有 witness 的 surface 如实 unavailable。
2. 补真实 Host subscription/events + bounded readback 的每 session 领域源；先冻结事件合同再
   实现，不能杜撰 U02 已存在事件。卡片/面板共用 source，保留 stale watchdog/原确切停止。
3. 保留并先红 `ComputerTaskCardLive` 无 run 卡片回归，接 pending/preparing/terminal/grant/原
   execution；没有自动 new request/retry/enable，Stop 不随 approve busy 禁用。
   不把批准卡显示成运行、不把 unknown 画绿、不通过 current legacy run 猜新任务。
4. 用项目 shell/tokens/GlassModal 做简洁范围选择与卡片/面板布局，完成15语言、键盘/返回
   焦点、窄栏/长语言/DPI，隔离 fixture 渲染逐张截图核验并记录源/hash。
5. 用户真 App 选择/首用、App/ACP/native 可见组批准/切换等联合验收另记录。当前没有这些
   手势的证据，不能宣布 U03 或整体 Computer Use 已完成。

U04 的其他平台 witness、关联 dialog、browser context/tab/document、跨 surface/原子增量与
缩小、U05–U09/D00–D31、installed/signing/真实 Grok/长稳仍保留。
遗留 App shutdown 同步 native cleanup 与 feature_lifecycle orphan worker join 未在本批解决。

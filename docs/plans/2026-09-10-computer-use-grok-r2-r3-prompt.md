# 交给 Grok 的 Computer Use R2.2–R3 长任务提示词

> [!CAUTION]
> 本提示词已完成并过期，禁止再次粘贴给 Grok。当前使用
> [B1 发行 Browser Runtime 长任务提示词](2026-09-10-computer-use-grok-runtime-pack-prompt.md)。

日期：2026-09-10  
用途：在 Grok App 的目标任务模式粘贴下面正文；不要额外加 `/goal`。只有在 Grok Build 交互终端使用时，才在正文最前加 `/goal`。

```text
在 H:\aicoding\grok-app-computer-use 的 feat/computer-use-implementation 分支继续 Computer Use 开发。

你接手的是一个约 56 个 tracked modified、197 个 untracked 文件、约 4 万行未跟踪内容的共享未提交工作树，不是新项目。禁止初始化、推倒重来、清空工作树、reset、clean、checkout --、stash、强切分支或覆盖现有改动。

开始前必须完整读取：
1. AGENTS.md
2. docs/llm-wiki/computer-use.md
3. docs/plans/2026-09-10-computer-use-grok-progress-review-r2.md
4. docs/plans/2026-09-10-computer-use-grok-r2-r3-execution.md
5. docs/plans/2026-09-10-computer-use-grok-repair-plan.md
6. docs/plans/2026-09-09-computer-use-execution-state.md
7. docs/plans/2026-09-09-computer-use-grok-step-plan.md

旧的 computer-use-grok-next-batch.md 和 computer-use-grok-repair-prompt.md 只作历史，不是入口。状态冲突时，以 execution-state 最新追加日志、progress-review-r2 和 r2-r3-execution 为准。先检查 git status 和账本最新项，不得照静态提示重复做已经完成的工作。

一、当前可信 checkpoint（2026-09-10 R2.2-c 审查后）

- 工作区：H:\aicoding\grok-app-computer-use。
- 分支：feat/computer-use-implementation。
- HEAD：30757366a739ec9aaf0ccc95bbb3efe19a067aa9。
- 全部 Computer Use 修改仍未提交，必须原样保留。
- R0.1、R1.1、R1.2、R1.3、R2.1 已完成，禁止返工。
- Codex 已完成 R2.1H 审查回补，禁止重复记录 Red 或重写：
  - Rust typed error envelope focused tests 5/5；
  - Node error/脱敏 tests 8/8；
  - 真实 BrowserSupervisor → LoopbackPlaywrightWorker → production Node worker HTTP 400 往返为 invalid_request/not_started，PASS；
  - core 182/182、driver 12/12、MCP 4/4、前端定向 129/129，Rust fmt/core Clippy/App check、ESLint、typecheck 均通过；App check 仅 3 个既有 dead-code warning。
- R2.2-a 已完成：observation state 7/7，opaque snapshot/ref、私有 target map、严格失效与泄漏负向测试均 Green。禁止返工。
- R2.2-b 已完成 E1：PageState 8/8，navigation、同 URL reload、popup、close、rebuild、iframe invalidation 已接线；R2.1H error probe 回归通过。真实 Chromium lifecycle 留 R3。
- R2.2-c 只完成核心模块：observation-extract focused 5/5，Browser Node 全套 30/30；production server `/observe` 仍走旧 ARIA/roles 分支，所以本项仍是 in_progress。
- Codex 审查已复现 P1：`safeDisplayUrl("data:text/plain,SECRET_PAYLOAD")` 原样返回 secret；裸 `[role]` 还会让非交互节点占满 64 cap；action-time handle/signature 复核也未闭环。修复前禁止接入 production。
- 当前唯一代码入口是 R2.2-c 安全加固与 production `/observe` 接线。不要重新制造 R2.2-a/b 的 Red，不要从 Rust R2.2-d 或 R2.3 开始。
- R3 完成后停止并交报告。不要进入 R4–R9、平台补全、runtime pack、App E4、commit、push 或 PR。

二、单 writer 与固定循环

这个共享工作树只能有一个代码 writer。不要启动多个子代理并发改代码或账本；只读审查和测试可并行。execution-state 只能由当前协调者追加。每个原子项前后都检查 git status 和目标 diff；发现非本原子项变化，停止协调，不能覆盖。

一次只做一个原子项，严格按：

Read → State → behavior Red → Implement → Targeted Green → Diff review → Regression gate → Record

开始时向 execution-state 追加 in_progress、预计文件、具体 Red、明确不做项。结束时追加实现状态与验证状态、命令/exit code/测试数/耗时、真实后置条件、E0–E3、身份/授权/取消/重放/staging/泄漏复核、未验证项和下一项。

当前项不绿不得进入下一项。普通编译、类型、lint 和测试错误自行修到绿。已有测试覆盖的契约记 already-covered，不要故意破坏 production 制造 Red；也不能拿旧 Red 或 MODULE_NOT_FOUND 冒充新契约 Red。

三、本批执行顺序

严格执行 r2-r3-execution.md，只按以下顺序。标为完成的项目只跑回归，不重写：

1. R2.2-a pure observation state：已完成，只回归。
2. R2.2-b PageState/current observation 与 navigation/reload/popup/close/rebuild/iframe/node invalidation：已完成 E1，只回归。
3. R2.2-c：先修 URL/候选节点/可见性安全契约，再接 production `/observe` 和 live HTTP tests；当前从这里开始。
4. R2.2-d Rust 2xx `ok:true`、page/observation strict parser 与 bounded/no-proxy loopback client。
5. R2.2-e Host current observation/ref allowlist、零 worker dispatch 与返回后 identity CAS。
6. R2.2-f private DTO/catalog 负向锁定和 production test/selector route 清理。
7. R2.3 action schema、全链 actionId、keyed fingerprint、完整 ledger、run-wide in-flight、unknown quarantine。
8. R3.1 Bearer/真实 identity/actionId/最小 env/finally cleanup fixture。
9. R3.2 PNG/ARIA/nodes/opaque refs。
10. R3.3 三组 typed actions。
11. R3.4 完整矩阵、browser-managed-contract 成功链和连续三次 fixture。

三点一、当前第一个原子项的精确做法

先在 execution-state 追加“R2.2-c 安全加固与 HTTP 接线 in_progress”，只列这些预计文件：

- tools/computer-use-browser/observation-extract.test.mjs
- tools/computer-use-browser/observation-extract.mjs
- 一个独立的 observe HTTP contract test；不要把大量测试逻辑塞回 server.mjs
- tools/computer-use-browser/server.mjs 仅做 import 和旧 `/observe` 分支替换
- execution-state 只在开始和全部 Green 后各追加一次

Red 1 只改 pure tests：

- data/file/javascript/invalid URL 返回中绝不能出现 SECRET_PAYLOAD、Windows 用户路径、userinfo、query value 或 fragment；about:blank 保留。
- 前 64 个 heading/document/presentation 等 role 后面有 button 时，button 必须仍可进入候选；测试不得靠把 cap 调大通过。
- hidden、inert、aria-hidden 节点不得得到可操作 elementRef；disabled 节点可展示但后续动作必须拒绝。

看到具体 assertion fail 后修 pure 模块。不要通过删除测试、返回原 URL 的 hash、把 secret 写日志或一律抛 500 来通过。

Red 2 只加 production HTTP contract：启动 production server 的随机 loopback/Bearer/隔离 profile，使用自建页面证明 `/observe` 仍缺 snapshot/nodes，然后最窄接线。测试至少断言：

- HTTP 2xx 顶层 `ok:true`；pageId/pageGeneration/snapshotId/nodes 必填且有界。
- 连续两次 observe snapshotId 不同，第一次 ref 在第二次后返回 typed stale/not_started。
- navigation、同 URL reload、iframe attach/navigate/detach 期间不发布混合 snapshot。
- worker HTTP body 允许并必须携带供 Rust 核对的 `pageId`，但不得包含 selector、handle、profile、query/data/file secret；后续模型 projection 必须移除 `pageId`。
- 所有失败路径 finally 关闭 worker/browser，PID 和 temp root 无残留。

本项命令至少包括：

```powershell
node --check tools\computer-use-browser\observation-extract.mjs
node --test tools\computer-use-browser\observation-extract.test.mjs
$browserTests = Get-ChildItem -LiteralPath tools\computer-use-browser -Filter *.test.mjs | ForEach-Object FullName
node --test $browserTests
node --check tools\computer-use-browser\server.mjs
```

随后回归 `browser-error-contract`，记录 worker PID 退出与 temp roots 清理。只有 pure、production HTTP、Browser 全套和 error probe 全绿，才将 R2.2-c 标为 implemented/passed 并进入 Rust R2.2-d。

四、R2.2 不可变契约

- snapshotId/elementRef 必须由 CSPRNG 生成，elementRef 只在 runId+tabId+pageGeneration+snapshotId 内解析。
- selector、DOM path、nth、Locator、ElementHandle、profile、pageId 不得进入 public node、模型 schema/projection、trace 或日志。
- 新 observe 替换旧 snapshot；任意主 frame committed navigation、同 URL reload、close、worker rebuild 使旧 identity 失效；popup 独立 identity。
- iframe navigate/detach、目标 detach/replaced 必须使相关 ref stale。observe 提取前后 CAS generation；ARIA、nodes、PNG、URL、geometry 不得来自不同时刻。
- public node 至少只有 elementRef/role/name/disabled 和批准的可选字段。节点上限沿用 OBSERVATION_NODE_CAP=64，PNG base64 上限沿用 400000；其余 text/url/title/ARIA/JSON/timeout/delta cap 由 Rust 权威常量并用 Node/Rust golden 锁步。
- generation 只能是 1..Number.MAX_SAFE_INTEGER。缺 pageId/generation/snapshot/nodes 不得 fallback 为空串、1 或请求值。
- observation-level truncated 与 node-level truncated 分开：未返回节点不分配 ref；字段被截断的节点不可 act；完整节点在全局截断时的行为必须固定并跨层测试。
- display URL 只允许 `http:`、`https:` 与精确 `about:blank`。`data:`、`file:`、`javascript:`、其他/无效 scheme 必须返回不含 payload、路径、用户名、query value 或 secret 的固定结果；先加具体 assertion Red。
- 不得使用裸 `[role]` 收集所有 ARIA role。候选只能是 native interactive controls、allowlisted interactive roles、真实 focusable/contenteditable；hidden、inert、`aria-hidden` 策略写入契约。disabled 可展示但不得执行。
- capture 后 `isConnected` 只证明采集时刻；动作前还必须复核 handle connected、私有 signature 与 page/frame identity。失败不得猜 selector、nth 或 fallback。
- 坐标绑定 snapshotId+imageContentId+width/height+scale+crop+scroll+viewport；NaN/Infinity/负数/越界/旧图片都在 Host 和 worker 零派发。PNG 未完成前禁止坐标 act。
- Host stale/unauthorized/schema 拒绝用正确 BrokerError，不伪装成 worker completion；RecordingBrowserWorker 必须证明调用次数为零。
- worker 返回后 Host 必须 CAS run/profile/page/generation/snapshot/connection；旧响应不能覆盖新 identity。
- Rust HTTP client 在 JSON 前限制 bytes，验证 Content-Type，只接受显式 loopback base，禁系统代理和 redirects；2xx 只接受 ok:true。

五、R2.2-f 必须清掉的后门/兼容债

R3.3 前，production worker entrypoint 中这些能力必须删除，或移动到不会随产品启动的独立 test harness：

- /marker、/state、/crash 与无 actionId evaluate helper；
- selector-based /popup、/upload、/download 默认 a#dl；
- Rust ManagedLocator.selector/role/name locator、read_state、selector upload/popup wrapper；
- Host wrapper 内部自动生成 actionId 的兼容入口。

/pids 仅可作为 Host-authenticated、固定 shape、无页面数据的生命周期诊断保留，模型不可见。fixture 的页面后置条件由自建 fixture 独立 oracle/test port 读取，不得为测试在 production worker 留任意 selector/evaluate 后门。最终报告给出静态搜索结果和每个保留项理由。

六、R2.3 actionId 与并发

- actionId 在最外层可重试边界生成，Host→Rust client→worker 原样传递；禁止内部重生 UUID。
- ledger key=(owner/run, actionId)。同 run 即使换 profile/page，同 ID 不同语义也冲突；不同 run 不泄漏。
- new-tab、popup、navigate/reload、act、upload、download 全走同一 ledger。
- fingerprint 使用进程随机 key 的 HMAC-SHA-256/等强 keyed hash，只覆盖 canonical semantics；密钥、明文、hash 不写日志，不存表单明文、URL query、token、selector。
- same ID/same fingerprint：pending 返回 in-flight；done/rejected/unknown 回放原 immutable terminal envelope；不二次执行。
- same ID/different fingerprint：conflict，零执行。
- 除同 ID 外，不同 actionId 并发写也必须受 run-wide in_flight 互斥，第二路零派发。
- unknown 清 observation；若底层 promise 可能晚到，fresh observe 不能自行解锁，page/run quarantine 到 promise settle 或关闭 context/rebuild identity。late completion 不能改 unknown。
- ledger 满后拒绝 unseen ID，但已知 ID 可查询；不淘汰后重放。跨 worker 重启 exactly-once 留给 R4，必须写未完成。
- 并发测试用 deferred/barrier，不用 sleep 碰运气。

七、R3 fixture、动作与下载

- production supervisor 与 fixture Bearer 都由 OS CSPRNG 生成至少 32-byte。随机 loopback port/profile/staging。
- 子进程先 env_clear，再只恢复启动必需的最小 OS 变量和明确 GROK_CU_*；禁止 fixture `{...process.env}`。用 OPENAI_API_KEY/GROK_API_KEY=SHOULD_NOT_REACH_CHILD sentinel 证明 worker/Chromium 看不到。
- 所有断言失败都必须 RAII/finally shutdown，等待 worker 和 Chromium descendants 退出，聚合主错误与 cleanup 错误，再清 temp。
- canonical actions：set_value 替换；type_text 追加/模拟键入；select 只操作 select/option，禁止 selectOption 失败后退化 fill；key/scroll/drag/wait/navigate/reload 严格按执行书身份矩阵，不造 dummy ref。
- 下载边读边计数，每跳重验 URL；写唯一 .part，校验/flush 后 atomic rename。取消、失败、超限、redirect 拒绝都清 .part。禁止先 arrayBuffer 全量读入；禁止静默从带浏览器 cookie 的 download 退化成无 cookie Node fetch。
- production raw selector/evaluate/CDP/shell/file/data/javascript URL 全部禁止；上传下载只走 Host 授权 run staging。
- R3 新增 `browser-managed-contract` gate：真实 BrowserSupervisor → LoopbackPlaywrightWorker → production server → 自建页面，验证 open→observe→opaque ref action→observe verify→PNG/identity→shutdown。只有 direct Node fixture + Rust mocks 不算 Host 成功闭环。
- R3 required fixture/probe 在当前 Windows 机不得 skip/not_run；缺 browser/runtime 就 fail 并报告。macOS/Linux/安装版 E4/E5 继续 not_run。
- run-fixture 最终连续三次 Green；每次记录 exit、wall time、browser/Node version、页面/文件后置条件、worker/Chromium PID 全退出。flake 后先修，再从第 1 次重新计数。

八、工程和安全红线

- 不碰 H:\aicoding\grok-app 或其他 worktree、stash、正式安装、用户账号/Token/Cookie、系统代理、共享 ~/.grok、真实 Chrome/Edge profile、ChatGPT/微信窗口。
- 不硬编码 10808/10809，不改代理。
- 测试仅用隔离 GROK_APP_HOME/profile/staging、自建 fixture、随机 loopback、target-cu-review、--offline。
- 根目录只用 pnpm，不运行 npm install/yarn。
- 不加 lint allow、skip/ignore、宽松断言、production 测试后门。
- 不向 src/App.tsx 或 src/app/AppWorkbench.tsx 增加 Computer Use state/分支。现有 AppWorkbench +6 行增长债留 R7 修，不能继续扩大。
- 当前三大文件基线 browser.rs=2975、browser_supervisor.rs=1678、server.mjs=990，合计 5643。结束合计净行数不得增加；新增逻辑拆 observation/action-schema/action-ledger/download 小模块。
- 不 commit、push、PR、merge、tag、release。

九、真实性边界

- mock/jsdom 是 E1；loopback/真实子进程是 E2；自建真实页面后置条件才是 E3；已安装 App+真实模型才是 E4。
- HTTP 200、tool ok、截图存在、DOM tree 变化都不等于任务成功，必须检查页面/文件/PID 后置条件。
- 26-byte js-runtime 和 53-byte worker seed 仍是 placeholder，不能报 healthy/可发行。
- desktop private worker 当前只 prepare_dispatch，真实 OS 动作仍在 App 进程。
- macOS element click 仍是固定坐标占位，key/scroll/drag 未实现；Linux type/key/scroll/drag 未实现且 capability 有误报；Windows 当前结果仍 verifiable:false。
- browser model tools、R4 Host ledger、Existing Tabs/WebView/Windows installed E4、macOS/Linux、四 target E5 全部未完成。
- Windows/Browser 成功不能冒充 Windows+macOS+Linux 首版完成。

十、最终交付

严格运行 r2-r3-execution.md 第 13 节所有门禁和静态搜索。R3 全绿后立即停止改代码，输出：

1. R2.1H regression、R2.2、R2.3、R3 每项实现/验证状态和 E0–E3。
2. 每个 behavior Red 与最终 Green 的命令、exit code、测试数、耗时、真实后置条件。
3. 所有 flake 和修复。
4. identity、授权、取消、幂等、run-wide in-flight、unknown quarantine、staging、下载、环境/日志泄漏复核。
5. git status --short --branch、git diff --check、三大文件总行数、legacy selector/test route 搜索。
6. 修改文件及职责。
7. 明确 R4–R9、平台能力债、runtime placeholder 与所有 E4/E5 未完成项。
8. R4–R7 精确接力点，但不要提前实现。

只允许说“R2.1H regression 和 R2.2–R3 在明确证据等级通过”，禁止说“Computer Use 已完成”。

现在先读取权威文件与 git 状态，确认账本最新项为 R2.2-c in_progress，且 Browser Node 全套基线为 30/30。第一步只能改 `observation-extract.test.mjs`：为 `data:`/`file:`/`javascript:` secret、本地路径泄漏、裸 `[role]` 饿死真实控件和 hidden/inert/aria-hidden 策略建立具体 assertion Red。随后修 `observation-extract.mjs` 到 focused Green，再把 `captureManagedObservation` 最窄接入 production `/observe`，添加真实 loopback HTTP contract。R2.2-c focused + live HTTP + Browser 全套 + R2.1H probe 全绿并记账后，才进入 R2.2-d。不要从 Rust、UI、R2.3 或 R3 提前开工。
```

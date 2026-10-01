# Computer Use 下一轮 48 小时执行书

> 日期：2026-09-13
> 工作区：`H:\aicoding\grok-app-computer-use`
> 分支：`feat/computer-use-implementation`
> 基线 HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`
> 审查前源码/工作树指纹：`70e0cced20d5f7162f11894160a31a07e65ffd8995bc7503ae3d0edfda502b71`
> 当前判断：`partial — not releasable`
> 配套审查：`docs/plans/2026-09-13-computer-use-codex-post-finalization-audit.md`

本文件定义下一轮实现顺序、证据合同和完成谓词。它不授权提交、推送、PR、合并或发版，也不把旧 evidence 中的 `passed` 继承到新代码。48 小时是持续执行窗口，不是必须耗满的假计时；但最终有一段至少 12 小时的真实 active soak，因此 40 分钟内只能产出 checkpoint，绝不可能得出“全部完成”。

## 1. 当前真值

应保留的成果：

- Computer Use 默认关闭，独立于 YOLO/普通编辑权限；
- session/run/target generation、桌面 lease、停止与 unknown 语义已有骨架；
- Windows Desktop fixture、Managed Browser worker、typed WebView adapter 和 session MCP 都有可复用实现；
- Rust core 当前 `294/294`、driver `12/12`、前端相关 `131/131`，格式、lint、typecheck 和 core Clippy 通过；
- Windows native probe 当前通过，Electron fixture 为 `not_run`；
- Windows runtime seed 自检通过。

尚未解决的发布阻塞：

1. Broker 通用 `observe/act/alive/preview/stop` 仍只走一个 Desktop adapter；
2. WebView 的产品测试直接调用 adapter，绕过 Broker/session MCP；
3. Existing Tabs extension 只保存 pairing key，没有 tab share/observe/act/heartbeat/reconnect/cancel/return transport；
4. pairing 的 `response` 被忽略，`complete_dual` 没有持有证明，本地进程可抢跑；
5. Managed/Existing Tabs 授权由两个 Host 命令拼接，缺少单事务和 generation compensation；
6. stop/revoke/App exit 不统一解绑 WebView；
7. bundle 只有 Windows runtime seed，未包含 extension，macOS/Linux runtime 和安装生命周期未验证；
8. macOS 缺 key/scroll/drag，Linux X11 除 click 外动作不完整，GNOME Wayland native 仍不可用；
9. App 编译仍有 36 个 warning，7 个 Computer Use 文件超过 1000 行，pairing 前端测试仍是源码字符串断言；
10. 旧报告手工把缺记录或存在失败子项的阶段标为 passed，F13/F14 均未运行。

## 2. 最终完成谓词

### 2.1 产品完成

只有以下条件同时成立，才允许写 `Computer Use passed/releasable`：

```text
四个 surface 全部经同一个 Broker-owned execution/lifecycle route
AND Desktop/Managed/WebView/Existing Tabs 都从 session MCP 完成 observe -> act -> verify
AND 所有授权都是一个 Host 原子事务，旧 attempt/generation 无法发布
AND stop/revoke/context change/feature off/App exit 对四个 backend 都完成幂等清理
AND extension transport 与 pairing 通过真实 Chrome 和 Edge 生命周期测试
AND 安装产物不依赖仓库目录、PATH Node、用户浏览器 profile 或 online latest
AND Windows、macOS arm64/x64、Linux X11、GNOME Wayland 各有真实 E5 证据
AND 当前 fingerprint 完成 F13 freeze 后未发生源码、测试、runner、配置或 runtime 漂移
AND F14 至少 12 小时真实 active soak，各 scenario 独立达标
AND unauthorized write / wrong target / post-stop dispatch / secret leak / orphan 为 0
```

真实模型 E4 没有用户新批准时必须保持 `not_run`，整体最多为 `partial`。缺少 macOS/Linux 实机时，对应行保持 `not_run` 或 `blocked_external`，不能用 Windows、cross-compile、XWayland 或 Chromium 结果替代。

### 2.2 本机阶段完成

Windows 本机可以独立完成并报告：

```text
Windows local implementation ready
Windows source E3 ready
Windows installed E5 ready
overall product partial
```

“本机 ready”不等于“三平台完成”，也不等于可以默认打开功能。

## 3. 工作边界

### 3.1 Git 与工作树

- 保留当前所有 tracked、untracked、ignored 修改，不 reset、clean、restore、checkout、stash 或重建 worktree；
- 不 `git add`、commit、push、建 PR、merge、tag 或 release；
- 不修改 `H:\aicoding\grok-app` 和其他 worktree；
- 不删除或覆盖此前 evidence；本轮只追加到自己带 owner marker 的新 run；
- 只可清理本轮 `owner.json` 明确登记且绝对路径位于本轮隔离根内的子进程和临时文件；
- 不终止任何不是本轮创建并记录的 Grok、Codex、Grok App、Chrome、Edge 或系统进程。

### 3.2 用户数据和外部系统

- 不读取、打印、导出或复用 Token、Cookie、浏览器 storage、密码、系统凭据或共享 `~/.grok`；
- 不改变代理、VPN、路由、账号、正式 Grok App 或用户日常浏览器；
- 不控制用户的 ChatGPT、微信、工作页面或任何未在本轮 fixture 中明确授权的窗口；
- App、浏览器和模型测试使用独立 `GROK_APP_HOME`、独立 identity、隔离 `--user-data-dir` 和 run-owned staging；
- 未获明确批准不得运行真实模型 E4，也不得向外部服务发送测试数据；
- runtime 只使用精确版本和 digest，不在线解析 `latest`。

### 3.3 产品规则

- Computer Use 默认关闭，不能被 YOLO、acceptEdits 或普通权限隐式打开；
- model MCP 不能 authorize/resume/reconnect/pair/grant/system-permission；
- dead/stale/unknown target 必须 fail closed，不回退 Desktop；
- timeout 返回 `unknown`，需要重新 observe，不能重放副作用；
- 禁止任意 shell、任意 JavaScript eval、Cookie/storage 导出和模型指定任意下载路径；
- 不向 `App.tsx`/`AppWorkbench.tsx` 增加状态；新状态放 domain module；
- UI 文案保持 15 locale 键一致，使用项目 `Select`/modal/menu，不使用系统默认 select 或 `window.confirm/prompt/alert`。

## 4. 新 evidence 合同

### 4.1 新 run

创建唯一、已被 `.gitignore` 覆盖的目录：

```text
tools/computer-use-probe/.run/next-48h/<run-id>/
  baseline/
  checkpoints/
  failures/
  logs/
  metrics/
  reports/
  owner.json
  state.json
  manifest.jsonl
```

开始写日志前必须验证：

```text
git check-ignore -q tools/computer-use-probe/.run/next-48h/<run-id>/owner.json
```

`owner.json` 至少记录 run id、repo canonical path、branch、HEAD、开始时间、OS/arch、writer PID、隔离 App/browser/model home、允许清理的绝对路径白名单和 schema version。

### 4.2 原子状态

原子 batch/scenario 只能使用：

```text
not_started / in_progress / passed / failed /
blocked_external / invalidated / not_run
```

`partial` 只能用于总览。总览状态必须由 required 子项的最差状态计算，不能手工覆盖。一个子项 `failed/not_run/blocked_external/invalidated` 时，父项不能是 `passed`。

### 4.3 Manifest

每次尝试追加一条，不覆盖旧日志。记录至少包含：

```json
{
  "seq": 1,
  "phase": "R1",
  "batch": "R1.2",
  "scenario": "webview-mcp-observe",
  "attempt": 1,
  "argvRedacted": [],
  "startedAt": "",
  "endedAt": "",
  "activeWallMs": 0,
  "exitCode": 0,
  "status": "passed",
  "evidenceLevel": "E3",
  "log": "logs/R1.2-0001.log",
  "bytes": 0,
  "sha256": "",
  "codeFingerprint": "",
  "postconditions": []
}
```

规则：

- 日志关闭后立即记录 size/hash，之后不可复用该路径；
- 首败、每次修复和最终结果都保留；
- `state.json` 从 manifest 聚合生成，以同卷临时文件 + rename 原子替换；
- 每 90 分钟和每个 batch 结束写 checkpoint；
- fingerprint 覆盖 HEAD、tracked binary diff、nonignored untracked source/config/docs、lock/manifest digest；
- 任何生产代码、测试、probe、runner、extension、配置或 runtime 改动都会使后续 freeze/soak `invalidated`；
- 最终逐条校验 log 存在、size、hash、fingerprint 和 postcondition；
- fixture/mock/static grep 最高只能是 E1/E2，不能记为产品 E3。

### 4.4 证据等级

| 等级 | 含义 |
| --- | --- |
| E0 | 设计、清单、静态审查 |
| E1 | unit/property/contract test |
| E2 | 隔离进程或组件集成测试 |
| E3 | 当前分支构建的真实 App/Host/MCP + 本地 fixture 后置条件 |
| E4 | 用户批准的真实模型任务 |
| E5 | 真实安装产物和目标 OS 生命周期 |

## 5. 目标架构

Broker 必须成为唯一控制平面。建议先写 ADR，再实现等价结构：

```text
ComputerUseBroker
  -> SurfaceExecutorRegistry
       desktop         -> DesktopExecutor(OS adapter / private driver)
       managed-browser -> ManagedBrowserExecutor(App-owned worker)
       app-webview     -> WebViewExecutor(App-owned side browser)
       existing-tabs   -> ExistingTabExecutor(authenticated extension transport)
```

每个 executor 至少支持：

```text
kind/capabilities
list or resolve candidate
provision + claim
target_alive
observe
act
preview policy
pause/resume policy
cancel/stop
release/cleanup
diagnostics
```

Broker 保存的 `TargetBinding` 至少包含：

```text
session_id
run_id
surface
backend_target_id
ownership (app-owned / borrowed)
target_generation
document_or_geometry_generation
executor_generation
authorization_attempt_id
```

Host command、UI、session MCP、trace 和 tool result 都必须携带 surface/run/target/generation。任何产品调用不得从 Tauri command 或 App-shell 直接绕过 Broker 调 adapter。专用 `browser_*` 工具可以保留兼容性，但必须复用同一个 executor 和同一份授权/ledger/cancellation 状态。

## 6. 48 小时排程

| 时间窗 | 阶段 | 目标 |
| --- | --- | --- |
| 0-1h | R0 | 现场真值、新 evidence、Red 基线 |
| 1-7h | R1 | 唯一 Broker-owned SurfaceExecutor |
| 7-11h | R2 | 原子授权、generation fencing、统一 cleanup |
| 11-15h | R3-R4 | Managed Browser 与 WebView 通过 Broker/MCP |
| 15-25h | R5-R7 | 安全 pairing、完整 MV3 transport、Existing Tabs 产品闭环 |
| 25-30h | R8 | runtime/extension/bundle/installer |
| 30-34h | R9-R10 | macOS/Linux ready work、质量和隐私债 |
| 34-36h | R11-R12 | 当前 fingerprint 产品 E3、F13 freeze |
| 36-48h | R13 | 至少 12 小时 scenario-keyed active soak |

这是依赖顺序，不是允许跳过验收的 deadline。若前置阶段未通过，不能假装进入 freeze/soak。到 48 小时时仍有缺项，应输出诚实的 `partial — not releasable` 和恢复条件，不得补写 passed。

## 7. 详细批次

### R0：现场真值与 Red 基线（0-1h）

任务：

1. 读取 `AGENTS.md`、`docs/llm-wiki/computer-use.md`、本执行书和配套审查全文；
2. 记录 branch、HEAD、tracked/untracked/ignored 数量、磁盘、OS/arch、现有 writer，不修改现场；
3. 新建 evidence run 并证明 ignore/ownership；
4. 计算新 fingerprint，旧 `4bd5...`、`70e0...` 只作为历史；
5. 写四个 Red：非 Desktop generic observe、WebView MCP、Existing Tabs transport、本地 pairing 抢跑；
6. 记录当前 App warnings、超千行文件、bundle inventory 和 OS capability matrix。

退出条件：

- 四个 Red 都能独立失败并指出生产调用链，不是源码字符串存在性；
- 没有修改任何旧 evidence；
- 新 manifest 可逐条复核。

### R1：唯一 SurfaceExecutor（1-7h，最高优先级）

任务：

1. 写 ADR：同步/异步边界、executor 生命周期、错误语义和 ownership；
2. Broker 从单一 `adapter` 改为按 `SurfaceKind` 路由的 registry；
3. `observe`、`act`、`target_alive`、capabilities、preview、pause、resume、stop、release 全部基于当前 `TargetBinding.surface`；
4. Desktop 只是 registry 中的一个 executor，不是 fallback；
5. Managed/WebView/Existing Tabs backend 未注册时返回 typed unavailable；
6. unknown wire surface、surface/target mismatch、executor generation mismatch 在副作用前拒绝；
7. 移除或封闭能绕过 Broker 的生产入口；测试 helper 必须显式标 fixture-only。

必须先红后绿的测试：

- WebView target 调 generic `computer_observe` 时不得进入 Desktop adapter；
- Existing/Managed target 调 generic `computer_act` 时不得拿 Desktop lease；
- 四个 executor 的 alive/capabilities/stop 互不串路；
- backend 缺失、target dead、旧 generation 均为零副作用；
- 并发两个 surface 时，授权和 action ledger 不互相覆盖。

退出条件：

- `ComputerUseBroker` 的通用执行路径不再直接引用唯一 Desktop adapter；
- 四种 surface 有生产注册点；
- MCP/Host trace 能证明 surface -> executor；
- core tests 和 core Clippy `-D warnings` 通过。

### R2：原子授权与统一生命周期（7-11h）

任务：

1. 设计一个 typed Host transaction，例如 `authorize_surface(request)`；
2. 单个 attempt 内完成 `begin -> provision/bind/share -> claim -> authorize -> activate -> attach MCP -> complete`；
3. 任一步失败按 ownership 补偿：关闭 App-owned profile/tab、归还 borrowed tab、unbind WebView、撤销 token/lease、fail ticket；
4. attempt id、ticket revision、run generation 和 selector revision 全链携带；
5. 用户在 A 未完成时选 B，A 的晚回调不能发布；
6. UI 在 busy/stopping 时禁用 surface、target、refresh 和 bind selector；允许明确 Cancel；
7. stop/revoke/feature off/context change/session switch/model switch/App exit 都进入同一 idempotent cleanup；
8. `shutdown_product` 同时处理 Managed、WebView、Existing Tabs 和 Desktop lease，而不只关 managed worker。

必须验证：

- A -> B 快速切换、双击、失败后重试、attach MCP 失败、worker 启动失败、WebView bind 失败；
- stop 发生在每个 transaction step；
- 重复 stop/revoke/exit 不 panic、不重复副作用；
- cleanup 后无 active grant、lease、preview、profile、worker、binding、session key。

退出条件：

- `ComputerPanel` 不再拼接两个授权命令；
- stale Host 请求无法授权错误目标；
- 四个 surface 都有相同的 lifecycle trace 和补偿测试。

### R3：Managed Browser 产品闭环（11-13h）

真实产品链：

```text
panel profile selection
-> atomic Host authorize
-> App-owned profile/tab
-> session MCP generic observe/act/verify
-> pause/takeover/resume
-> stop/revoke/close
```

要求：

- profile id 永远不进入 Desktop adapter；
- `browser_*` 与 generic `computer_*` 共享 executor、grant、generation、ledger 和 cancellation；
- navigation/download/popup/upload 使用 typed API，下载只到 run-owned staging；
- fork/compact/model switch/session restore 后必须重新授权和 observe；
- 双 session/双 profile 不串 tab、token、截图或下载；
- App-shell/fixture 不直接调用 worker 作为产品通过依据。

E3：branch-built App + isolated home，至少 20 个完整循环，独立页面 oracle；包含 worker kill、navigate during stop、malformed response 和 timeout unknown。

### R4：App WebView 产品闭环（13-15h）

要求：

- `product_webview()` 作为 WebView executor 注册到 Broker，不再是平行全局控制面；
- `computer_observe/computer_act` 经 session MCP -> IPC -> Broker -> WebView executor；
- App-shell 验收不能出现直接 `wv.observe/wv.act`；
- target list、alive、preview、pause、stop、unbind 都由 binding lifecycle 控制；
- cross-origin iframe、权限 UI、复杂下载保持 typed unsupported；
- 禁止任意 eval、Cookie/storage 迁移；
- 隐藏面板停止 preview，旧 run/generation 帧不能闪回。

E3：真实 Tauri side browser，至少 20 个 `bind -> authorize -> MCP observe -> MCP act -> verify -> unbind` 循环；另做 bind 失败、窗口关闭、导航换文档、stop during action 和下一轮恢复。

### R5：Pairing 安全协议（15-18h）

先写安全 ADR，威胁模型至少包含恶意本地进程、恶意网页、其他扩展、重放、日志/URL 泄漏、service-worker 重启和浏览器/App 重启。

最低协议要求：

- public GET 只返回 nonce、expiry 和非敏感元数据；
- pairing secret/code 不进入 URL、query、地址栏、history、referer、普通日志、截图或 crash report；
- Origin/Host/CORS 只是附加防护，不作为身份；无 Origin 的浏览器 pairing 请求不能直接成功；
- extension 必须证明持有一次性用户转移 secret，例如对 nonce/instance/extension id/connection nonce 做 constant-time MAC；
- App 与 extension 是两个独立显式确认步骤；
- session key 只在认证后的响应/通道交付，并绑定 extension id、App instance 和 connection generation；
- stable extension identity 不能硬编码不存在的 `pw-ext-installed`；开发/打包/Chrome/Edge identity 有明确来源；
- expiry、rate limit、wrong host、forwarded、wrong origin、wrong id、wrong MAC、replay、并发抢跑、App restart、extension restart 全部 fail closed；
- revoke/rotate/stop/logout/App exit 使旧 key 立即失效。

推荐方案：App 显示短时一次性 pairing code，用户在 extension popup/side panel 中输入或确认；extension 用该 code 生成 MAC，认证后再建立 loopback transport。若选择 native messaging，ADR 必须同时覆盖三 OS 安装和卸载。不能继续使用“只验证公开 nonce + 两个布尔值”。

### R6：完整 MV3 Existing Tabs transport（18-23h）

extension 至少需要：

- service worker 的认证连接、心跳、重连、command queue、deadline、cancel 和 key rotation；
- 用户点击 Share 当前 tab 的显式入口，模型不能枚举所有 tabs；
- 最小权限说明和 optional permission 流程；
- tab record：browser/profile/tab id、origin、focus、home index、document generation、connection generation；
- observation：固定 schema、截图/DOM/可操作节点、observation id、generation、bounds；
- typed action：click/set value/type/key/scroll/wait/navigation，禁止远程任意脚本；
- action 必须引用最新 observation，navigation/reload/close/focus/disconnect 使旧 observation 失效；
- stop/revoke 取消 command、detach content script、删除 run grant、归还 tab，不关闭 borrowed tab；
- service-worker suspend/restart 后安全恢复或明确要求重新配对，不悄悄沿用过期授权；
- session key 不暴露给页面 world，不使用 `localStorage`，日志完全脱敏。

E2/E3：隔离 Chrome for Testing profile，覆盖至少 5 个 tab 生命周期、双 session、跨导航、关闭、断线/重连、旧 generation 拒绝、stop during action。真实 Edge 在可用时独立运行；没有 Edge 才标 `blocked_external`。

### R7：Existing Tabs 经 Broker/MCP（23-25h）

要求：

- extension 的真实 share event 是 `offer_shared_tab` 的生产调用者；
- picker 只显示用户明确 share 的候选；
- atomic authorize 生成 run-scoped grant；
- generic `computer_observe/computer_act` 和兼容 `browser_*` 都进入 ExistingTabExecutor；
- managed worker 明确拒绝 borrowed tab 的旧路径不再成为产品死路；
- trace 能从 extension connection -> Host offer -> picker -> grant -> MCP -> result 全链关联；
- disconnect/reconnect、navigation、tab close、stop/revoke 和 App exit 都能归还/撤销。

E3：branch-built App + packaged extension source，至少 20 个完整循环，DOM/截图独立 oracle，零错误 tab、零 post-stop dispatch。

### R8：runtime、extension、bundle、installer（25-30h）

任务：

1. extension manifest、service worker、content scripts、popup/side panel、LICENSE、说明和固定 identity 元数据进入 Tauri resources；
2. Windows/macOS arm64/macOS x64/Linux x64 分别有独立 runtime lock、tree digest 和 resource map；
3. build hook 按目标平台准备/检查，不把 Windows blob 放进其他包；
4. 安装后不依赖仓库源码、PATH Node、系统 Playwright、用户 profile 或 online latest；
5. runtime repair/upgrade 原子化，tamper 后同版本可恢复，失败保留旧健康版本；
6. extension 安装/配对入口在 installed App 中可找到，不要求用户选择仓库目录；
7. bundle 审计拒绝 probe、fixture、测试后门、secret 和多余 runtime。

Windows E5 使用隔离目标或 throwaway VM，串行验证：

```text
clean install
-> first start
-> extension install/pair
-> Desktop/Managed/WebView/Existing Tabs smoke
-> same-version repair after tamper
-> upgrade
-> rollback
-> uninstall
-> verify user data policy and no orphan process
```

macOS/Linux 分别验证 `.app/.dmg`、AppImage/deb/rpm 的同等生命周期。缺签名/商店发布可标外部 blocker，但 source checkout 可用绝不能算 installed passed。

### R9：四 OS backend ready work（30-32h）

Windows：保留现有能力，补 installed App、DPI、多屏、中文输入、焦点和取消后置条件。

macOS arm64/x64：完成 type/set value/key/scroll/drag/cancel、TCC 诊断、Retina/多屏坐标、窗口存活与 App bundle helper。

Linux X11：完成 X11 screenshot、XTEST click/type/key/scroll/drag、焦点、窗口 identity 和取消。

GNOME Wayland：实现经 ADR 选择的 portal ScreenCast/RemoteDesktop + EIS/libei（或等价 native helper），覆盖 permission、PipeWire、坐标和断线。`native_wayland` 在真实 helper + 实机证据前保持 false，XWayland 不算 native。

Windows 机器不能制造其他 OS 的 E5。可完成 protocol、build config、compile/check 和远端执行脚本，但状态必须诚实。为每个缺失设备写：环境要求、命令、fixture、oracle、预期 artifact 和恢复步骤。

### R10：质量、隐私和可维护性（32-34h）

任务：

- 清掉 Computer Use 相关 App warning，不用 module-level `allow` 隐藏；
- 将 `app_shell.rs`、`runtime.rs`、`runtime_prepare.rs`、`webview.rs`、`broker.rs`、`broker/gates.rs`、`windows_adapter.rs` 按职责拆分；Computer Use 新文件不再超过 1000 行；
- App-shell 只做真实产品 orchestration，fixture runner 移到明确的 test/probe support；
- 把 `computerUse.pairing.test.ts` 从字符串 grep 改成 Tauri invoke/状态机行为测试；
- locale parity、selector busy/error/empty/keyboard、preview hidden/stale frame 都有行为测试；
- 保持 App/AppWorkbench 行数不增长；
- 记录 LF 策略，但不要在这个巨大 dirty 分支中做无关全仓 renormalize；
- diagnostics/trace/staging/profile/pairing 分目录，retention 同时有 record/bytes/age 上限；
- support bundle 压缩后二次 secret scan；cleanup 防 symlink/junction/path traversal；
- `pnpm audit:prod` 的既有依赖漏洞诚实记录，不把 failed 父项写 passed。

必跑门禁：

```text
git diff --check
pnpm typecheck
pnpm lint
Computer Use/frontend integration Vitest
cargo fmt --all -- --check
cargo clippy -p grok-computer-use-core --all-features --locked --offline -- -D warnings
cargo test -p grok-computer-use-core --all-features --locked --offline
cargo check -p grok-app --lib --locked --offline
runtime check per available target
bundle audit
locale parity
dependency check and production audit
```

### R11：当前 fingerprint 的产品 E3（34-35h）

必须从 branch-built App、isolated home、session MCP 起点验证：

1. 默认关闭，YOLO 不能开启；
2. Desktop：authorize -> MCP observe/act/verify -> pause/takeover/resume -> stop；
3. Managed：profile -> atomic authorize -> MCP -> action/navigation/download -> stop；
4. WebView：bind -> atomic authorize -> MCP -> typed action -> unbind；
5. Existing Tabs：pair -> explicit share -> picker grant -> MCP -> navigation/reconnect -> return；
6. 四个 surface 各至少 20 次；
7. 双 session、stale observation、wrong target、timeout、worker kill、window/tab close、App exit；
8. 每次用独立 oracle 验证真实后置条件，而不是 tool `ok`；
9. 最终无孤儿 worker/browser/profile/lease/token/binding/staging。

任何产品路径仍直接调用 adapter、任何一个 surface 缺 MCP 闭环，都不能进入 R12。

### R12：F13 freeze（35-36h）

冻结前：

- 从 manifest 重新生成状态矩阵；
- 校验每条 evidence 的 path/size/hash/fingerprint；
- required 本机项必须 passed；外部 OS/E4 保留独立状态；
- 计算 freeze fingerprint 并复制 source/runtime/bundle inventory；
- 终止的只能是本 run owner 登记的子进程；
- freeze 后禁止修改生产代码、测试、runner、extension、配置和 runtime。

若必须修代码，立即把 freeze 和其后 evidence 标 `invalidated`，回到对应 Red，重新跑 R11/R12。

### R13：至少 12 小时主动长稳（36-48h）

不能用 sleep、空轮询、重复静态命令、build 时间或多个并行 lane 的耗时相加冒充。总墙钟至少 12 小时，并按 scenario 独立计数：

| scenario | 最低主动墙钟 | 最低轮数 |
| --- | ---: | ---: |
| desktop-appshell | 2h | 60 |
| managed-product | 3h | 100 |
| webview-product | 2h | 100 |
| existing-tabs-product | 2h | 100 |
| fault-stop-restart | 2h | 80 |
| privacy-runtime-package | 1h | 60 |

每个 scenario 独立保存：

```text
active_wall_s
rounds
ok
fail
consecutive_fail
first_failure_log
last_success_log
resource_trend
```

通过条件：

- 每个 scenario 达到自己的墙钟和轮数，成功率不低于 90%；
- unauthorized write、wrong target、post-stop dispatch、secret leak、unrecovered worker/browser/profile/lease/token/binding 均为 0；
- 一个 scenario 的成功不能清零另一个 scenario 的失败；
- 同一根因连续失败、runner 被杀、机器重启、证据损坏或 fingerprint 漂移立即停止并保留首败；
- 若修复代码，则旧 soak `invalidated`，从新 freeze 重新累计 12 小时。

## 8. 阻塞与继续规则

遇到缺少 macOS/Linux/Edge/签名/真人授权：

1. 记录具体缺失环境、为何本机不能解决、已完成的 ready work；
2. 写可复制的恢复命令、fixture、oracle 和预期 evidence；
3. 对该原子格标 `blocked_external` 或 `not_run`；
4. 继续同阶段其他 ready batch，不能因为一个外部格提前结束整个目标；
5. 到 48 小时仍未恢复时输出 `partial — not releasable`。

遇到 panic、死锁、越权、错误目标、secret leak、post-stop dispatch、manifest mismatch 或用户环境被触碰：

- 立即停止当前 workload；
- 保留首败、进程 ownership、fingerprint 和现场；
- 只清理本 run 登记的资源；
- 不 reset/restore/clean，不用重试掩盖；
- 回到最近的 Red 和最小修复。

## 9. Checkpoint 格式

每 90 分钟或每个 batch 结束，报告：

```text
time/run-id/current fingerprint
completed batch + exact evidence level
changed production files
tests with pass/fail counts
first unresolved failure
security/lifecycle invariants
external blockers + recovery
next ready batch
git boundary: uncommitted / unpushed / no PR
```

禁止写“基本完成”“应该没问题”“功能都通了”而不给生产 trace 和后置条件。

## 10. 最终交付

最终报告必须包含：

- run id、repo、branch、HEAD、baseline/freeze/current fingerprint；
- R0-R13 每个原子状态、最高 evidence level 和 evidence 路径；
- Desktop/Managed/WebView/Existing Tabs 四列独立状态；
- Windows/macOS arm64/macOS x64/Linux X11/GNOME Wayland 独立状态；
- Chrome/Edge、source/installed、App/MCP/real-model 独立状态；
- 12 小时每个 scenario 的 active wall、rounds、ok/fail、首败和资源趋势；
- pairing threat model 和负向测试；
- bundle/install/repair/upgrade/rollback/uninstall inventory；
- warnings、大文件、dependency audit 和未偿技术债；
- manifest path/size/hash/fingerprint 完整性；
- 未完成项、external blocker、恢复命令；
- 明确写 `未 commit / 未 push / 未提 PR`。

只要任一 required 项未通过，标题和结论必须是：

```text
partial — not releasable
```

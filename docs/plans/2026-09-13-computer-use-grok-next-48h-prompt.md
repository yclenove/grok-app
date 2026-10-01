# 给 Grok 目标模式的下一轮 48 小时提示词

以下正文可以直接交给 Grok。执行细节以仓库内 `docs/plans/2026-09-13-computer-use-grok-next-48h-execution.md` 为准；本提示词同时写出硬边界，不能只读标题后自行缩减任务。

---

你现在接手 `H:\aicoding\grok-app-computer-use` 的 Computer Use 长任务。以高级桌面系统工程师、安全工程师、浏览器扩展工程师和发布工程师的标准工作。不要把已有测试数量当成完成，也不要在几十分钟后写“任务完成”。本轮必须按目标模式持续推进最多 48 小时；只有真实完成谓词成立才能提前结束，而完成谓词包含 freeze 后至少 12 小时的主动长稳，所以 40 分钟内最多只能给 checkpoint。

## 一、当前现场

- 工作区：`H:\aicoding\grok-app-computer-use`
- 分支：`feat/computer-use-implementation`
- HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`
- 工作树有大量用户必须保留的 tracked/untracked/ignored 修改。
- 上一次审查前参考 fingerprint 是 `70e0cced20d5f7162f11894160a31a07e65ffd8995bc7503ae3d0edfda502b71`，但新审查/计划文档已再次改变工作树；启动后必须重新计算，不能沿用这个值。
- 当前真实结论：`partial — not releasable`。
- 已有成果可以复用：Computer Use 默认关闭、Broker/session/MCP 基础、Windows adapter、managed Playwright worker、typed WebView、runtime seed、前端面板和大量安全测试。
- 当前重新验证过的基础结果：Rust core 294/294、driver 12/12、前端相关 131/131、typecheck/lint/fmt/core Clippy 通过、App 编译通过但有 36 warnings、Windows native probe 通过且 Electron `not_run`、Windows runtime seed check 通过。
- 这些结果只证明对应层，不证明四 surface 产品闭环、安装版、真实模型或跨平台完成。

## 二、开始前必须全文阅读

按顺序完整读取，不可只 grep 摘要：

1. `AGENTS.md`
2. `docs/llm-wiki/computer-use.md`
3. `docs/BUILD.md`
4. `docs/plans/2026-09-13-computer-use-codex-post-finalization-audit.md`
5. `docs/plans/2026-09-13-computer-use-grok-next-48h-execution.md`
6. `docs/plans/2026-09-13-computer-use-existing-tabs-pairing-adr.md`
7. `docs/plans/2026-09-13-computer-use-blocking-http-adr.md`
8. 当前 `tools/computer-use-probe/.run/finalization/20260912T172044Z-seven/` 的 state、manifest 和 final report，只作为历史证据，不继承其中 passed。

若旧文档与当前审查或代码冲突，采用更严格的安全和验收条件，并在新 evidence 中记录冲突。不要修改旧 evidence 来让它看起来一致。

## 三、绝对边界

### Git/发布

- 禁止 `git reset`、`git clean`、`git restore`、`git checkout --`、stash、强制切分支、重建 worktree。
- 禁止 `git add`、commit、push、建 PR、merge、tag、release。
- 不得删除、覆盖或回滚用户已有 tracked/untracked/ignored 文件。
- 不修改 `H:\aicoding\grok-app` 或其他 worktree。
- 本轮所有修改保留在当前 dirty worktree，等待用户和 Codex 审查。

### 进程/文件并发

- 不终止任何不是本轮创建并在 `owner.json` 登记的 Grok、Codex、Grok App、Chrome、Edge 或系统进程。
- 每次编辑前检查目标文件最新 mtime 和 git diff；发现其他 writer 在改同一文件时，不覆盖，转到不冲突的 ready batch，并写 checkpoint。
- 只允许清理本轮隔离根内且 owner 白名单明确登记的临时目录和子进程。

### 用户数据/外部系统

- 不读取、打印、导出、复用或写入账号 Token、Cookie、浏览器 storage、密码、系统凭据或共享 `~/.grok`。
- 不改变代理、VPN、路由、账号、正式 Grok App 或用户日常浏览器配置。
- 不控制用户的 ChatGPT、微信、工作页面和任何未明确属于本轮 fixture 的窗口/tab。
- 所有测试使用独立 `GROK_APP_HOME`、独立 identity、隔离 browser `--user-data-dir`、run-owned staging。
- 未经用户新批准，不启动真实模型 E4，不向外部服务发送测试内容。
- runtime 不在线解析 `latest`，只允许精确版本、digest 和 target-specific lock。

### 禁止假完成

- Fake/mock/stub/jsdom/source grep/空 target/cross-compile/单 Windows fixture 不能提升为产品 E3/E4/E5。
- API 200、按钮可点、tool `ok`、模块存在、循环次数多都不等于真实后置条件。
- 不添加生产测试后门，不吞错，不用 skip/ignore/弱断言/宽泛 `allow`/自动重试副作用换绿。
- 不把 `partial`、`failed`、`blocked_external`、`invalidated`、`not_run` 汇总为 passed。
- 不把 Windows/Chromium/XWayland 结果复制给 macOS、Linux X11 或 GNOME Wayland。
- 不在代码改动后沿用旧 fingerprint/freeze/soak。
- 不用 sleep、空循环、重复静态检查、build 时间或并行 lane 耗时相加冒充主动长稳。

## 四、最先承认并修复的 P0

不要从增加 fixture 数量开始。当前最关键事实如下：

1. `ComputerUseBroker` 仍持有一个 Desktop adapter。通用 `capture/observe/act/authorized_target_alive/capabilities/preview/pause/resume/stop` 没有按 `TargetBinding.surface` 选择 backend。
2. `SurfaceRouter` 目前主要做 ID 分类和 authorize，不是执行路由器。
3. App-shell 的 WebView 循环仍直接调用 `product_webview().observe/act`，绕过 session MCP 和 Broker，不能算产品 WebView E3。
4. Existing Tabs extension 的 service worker 只在内存保存 session key；没有 share、transport、heartbeat、reconnect、observe、act、navigation generation、cancel 或 return。
5. `browser_observe` 仍进入 `observe_managed_tab`；该 managed path明确拒绝 user-owned tab，因此 Existing Tabs 没有模型执行路径。
6. pairing HTTP 接受无 Origin，请求中的 `response` 被丢弃，`complete_dual` 只依赖公开 nonce/instance 和两个布尔值；恶意本地进程可抢 session key。硬编码 `pw-ext-installed` 也不是稳定扩展身份。
7. Managed/Existing Tabs UI 先 provision/grant，再 authorize，是两个 Host 命令；busy 时 surface/target selector 仍可变更，旧请求可能授权错误目标。
8. stop/revoke/App exit 没有统一解绑 WebView，`shutdown_product` 只关 managed browser。
9. bundle 只有 Windows seed，extension 不在 Tauri resources，macOS/Linux runtime 是 `not_run`。
10. macOS 缺 key/scroll/drag；Linux X11 除 click 外输入不完整；GNOME Wayland `native_wayland=false`。
11. evidence report 手工把 F2/F4/F6/F7/F9/F12 标 passed，和生产路径、缺失 manifest phase、Edge/macOS/Linux `not_run`、`audit_prod=failed` 冲突。
12. App 仍有 36 warnings；Computer Use 有 7 个超千行文件；pairing 前端测试只是源字符串断言。

如果你认为任何一条已经不成立，必须给出当前 production call trace、行为测试和独立后置条件，而不是口头反驳。

## 五、新 evidence 协议

创建全新的：

`tools/computer-use-probe/.run/next-48h/<run-id>/`

目录包含 `baseline/ checkpoints/ failures/ logs/ metrics/ reports/ owner.json state.json manifest.jsonl`。写日志前用 `git check-ignore -q` 证明 owner 文件被忽略。

规则：

- 原子状态只允许 `not_started/in_progress/passed/failed/blocked_external/invalidated/not_run`；`partial` 只做总览。
- 每个尝试用新日志文件，关闭后立即记录 bytes 和 SHA-256；不覆盖历史失败。
- manifest append-only，至少记录 seq、phase、batch、scenario、attempt、脱敏 argv、开始结束、active wall、exit code、状态、evidence level、日志、hash、bytes、code fingerprint、postconditions。
- `state.json` 必须从 manifest required records 聚合，父项取最差 required 子项，不能手填 passed。
- 每 90 分钟以及每个 batch 完成时写 checkpoint。
- fingerprint 覆盖 HEAD、tracked binary diff、nonignored untracked source/config/docs 和 lock/manifest digest。
- 任何 production/test/probe/runner/extension/config/runtime 变化都使 freeze 后 evidence invalidated。
- 最终逐条验证日志存在、size、hash、fingerprint；缺一条就不能 passed。

证据等级：E0 设计/静态，E1 unit，E2 隔离组件/进程，E3 branch-built App + Host/MCP + fixture 后置条件，E4 用户批准真实模型，E5 目标 OS 安装产物。

## 六、严格执行顺序

### R0，0-1 小时：现场真值和 Red

- 记录 status、HEAD、当前 fingerprint、文件/进程/磁盘/OS，不清理现场。
- 建立新 evidence owner/manifest/state。
- 写能真实失败的四个 Red：非 Desktop generic observe、WebView through MCP、Existing Tabs transport、本地 pairing 抢跑。
- 记录 warnings、超千行文件、bundle inventory、OS capability matrix。
- R0 未有原始 Red 和生产调用链，不进入 R1。

### R1，1-7 小时：唯一 Broker-owned SurfaceExecutor

先写 ADR，明确同步/异步边界、ownership、错误和 cleanup。建立 `SurfaceExecutorRegistry` 或等价结构，至少注册 Desktop、Managed Browser、App WebView、Existing Tabs 四个 executor。

Broker 根据授权 binding 的 surface 路由 list/claim/alive/observe/act/capabilities/preview/pause/resume/cancel/stop/release/diagnostics。Desktop 只是一个 executor，绝不 fallback。binding 全链带 session/run/surface/backend target/ownership/target generation/document-or-geometry generation/executor generation/attempt id。

MCP、Tauri command、UI、trace 都不能绕 Broker 直接调用 adapter。兼容 `browser_*` 可以保留，但必须复用同一 executor、grant、ledger 和 cancellation。

必须证明：WebView/Existing/Managed 通用 observe/act 不进 Desktop；dead/stale/mismatch/unavailable 零副作用；四 backend lifecycle 不串；并发 surface 不覆盖授权。

R1 gate：Broker 通用执行代码不再直接依赖唯一 Desktop adapter；四 surface 有生产注册点；core test/Clippy 通过；production trace 可见 surface -> executor。

### R2，7-11 小时：原子授权和统一 cleanup

把 UI 的两命令拼接改成一个 typed Host transaction：

`begin -> provision/bind/share -> claim -> authorize -> activate -> attach session MCP -> complete`

同一 attempt/ticket/generation 内执行。任一步失败必须按 ownership 补偿：App-owned profile/tab 关闭、borrowed tab 归还、WebView unbind、token/lease/grant 撤销、ticket fail。A 未完成时选 B，A 的晚回调绝不能发布。busy/stopping 时禁用 surface/target/refresh/WebView selector，允许明确 cancel。

stop/revoke/feature-off/context-change/session switch/model switch/App exit 必须走同一幂等 cleanup，覆盖四 executor；`shutdown_product` 不能只关 managed worker。

至少覆盖每个 transaction step 的 stop、attach MCP 失败、worker/bind/share 失败、双击、A->B 快切、重复 revoke/exit。最终 active grant/lease/preview/profile/worker/binding/session key 全为空。

### R3，11-13 小时：Managed Browser 真产品闭环

从 panel profile selection 开始，经原子 Host authorize、session MCP generic observe/act/verify 到 pause/resume/stop。profile id 不进 Desktop。navigation/download/upload/popup 都是 typed API；下载只到 run staging。双 session、fork、compact、model switch、restore、worker kill、timeout unknown 都独立验证。

使用 branch-built App + isolated home + 独立页面 oracle 至少 20 轮。直接调 worker 或 `mcp_scripted_agent_run.rs` 只能辅助，不算该产品 gate。

### R4，13-15 小时：WebView 真产品闭环

把 product WebView 注册为 Broker executor。产品验收路径必须是 session MCP -> IPC -> Broker -> WebView executor。App-shell 不得直接 `wv.observe/wv.act`。target list/alive/preview/pause/stop/unbind 都由 binding lifecycle 管理。继续拒绝任意 eval、Cookie/storage 迁移、cross-origin iframe、权限 UI 和复杂下载。

真实 Tauri side browser 至少 20 轮 `bind -> authorize -> MCP observe -> MCP act -> verify -> unbind`，另测 bind 失败、navigation generation、窗口关闭、stop during action、隐藏 preview、旧帧和下一轮恢复。

### R5，15-18 小时：安全 pairing

先写威胁模型/ADR，覆盖恶意本地进程、网页、其他扩展、重放、service worker suspend、App/browser restart、URL/日志泄漏。

公开 GET 只能有 nonce/expiry/公共元数据。一次性 secret/code 不得进入 URL、history、referer、普通日志、截图。Origin/Host/CORS 不是身份。extension 必须对 nonce + instance + stable extension id + connection nonce 提交 constant-time 验证的持有证明；App 和 extension 分别显式确认；认证成功后才在保护通道交付绑定 generation 的 session key。wrong host/origin/id/MAC、无 Origin、forwarded、expiry、replay、并发抢跑、rotate/revoke/restart 全部负测。

推荐用户把 App 显示的短时 pairing code 输入/确认到 extension popup/side panel，再由 extension 做 MAC。也可选 native messaging，但必须同时解决三 OS 安装。不得继续忽略 `response` 或用公开 nonce + 两布尔值完成配对。

### R6，18-23 小时：完整 MV3 transport

service worker 实现认证连接、heartbeat、reconnect、command queue、deadline、cancel、key rotation。用户必须点击 Share 当前 tab，模型不能枚举全部 tabs。权限最小化并有 optional permission UX。

transport 必须有 tab/browser/profile/origin/home/focus/document generation/connection generation；observation id、截图/DOM/可操作节点/bounds；typed click/set-value/type/key/scroll/wait/navigation；最新 observation 约束；navigation/reload/close/focus/disconnect 使旧 generation 失效；stop/revoke detach、删 grant、归还 tab但不关闭 borrowed tab。禁止远程任意 JS，session key 不进入 page world/localStorage/log。

隔离 Chrome for Testing 覆盖至少 5 类 tab 生命周期、双 session、导航、关闭、断线重连、service worker restart、stale generation、stop during action。Edge 可用则独立运行，不可用才 `blocked_external`。

### R7，23-25 小时：Existing Tabs 经 Broker/MCP

extension share event 必须成为 `offer_shared_tab` 的 production caller。picker 只显示用户 share 候选；atomic authorize 创建 run-scoped grant；generic `computer_observe/computer_act` 和兼容 browser tools 都进 ExistingTabExecutor，不能再落到明确拒绝 user-owned tab 的 managed path。

用 branch-built App + packaged extension source 至少 20 轮 pair -> share -> picker -> grant -> MCP observe/act/verify -> navigation/reconnect -> stop/return。每轮独立 DOM/截图 oracle，错误 tab 和 post-stop dispatch 必须为零。

### R8，25-30 小时：runtime、bundle、installer

- extension 完整资源、LICENSE、说明、identity 元数据进入 Tauri bundle，不依赖仓库目录。
- Windows/macOS arm64/macOS x64/Linux x64 各自有 target-specific runtime lock、tree digest、resource map 和 build check。
- 安装后不使用 PATH Node、系统 Playwright、用户 profile、online latest 或 fixture。
- repair/upgrade 原子化，tamper 可恢复，失败保留旧健康版本。
- bundle audit 排除 probe/test backdoor/secret/多余 binary。

Windows 在 throwaway VM/隔离目标验证 clean install -> first start -> extension install/pair -> 四 surface smoke -> same-version repair -> upgrade -> rollback -> uninstall -> no orphan。不要碰用户当前正式安装或未登记 PID。macOS/Linux 安装包分别留独立 E5；缺环境就写恢复命令和 `blocked_external/not_run`。

### R9-R10，30-34 小时：跨平台 ready work、质量、隐私

- macOS 完成 type/set-value/key/scroll/drag/cancel、TCC、Retina/多屏/坐标。
- Linux X11 完成 screenshot、XTEST click/type/key/scroll/drag、焦点/identity/cancel。
- GNOME Wayland 按 ADR 实现 portal ScreenCast/RemoteDesktop + EIS/libei 或等价 native helper；实机通过前 `native_wayland=false`。
- Windows 不得制造其他 OS passed；为缺失 OS 写可直接执行的环境、命令、fixture、oracle、artifact 和恢复步骤。
- 清掉 Computer Use 相关 App warnings，不用宽泛 allow。
- 拆分 `app_shell.rs/runtime.rs/runtime_prepare.rs/webview.rs/broker.rs/broker/gates.rs/windows_adapter.rs`，Computer Use 文件不再超过 1000 行；fixture runner 与产品 orchestration 分离。
- pairing 前端测试改为行为测试，不再做源字符串存在性。
- 保持 App/AppWorkbench 行数不增长；15 locale parity；UI busy/error/empty/keyboard/hidden-preview/stale-frame 全路径。
- diagnostics/trace/staging/profile/pairing 分目录并限制 record/bytes/age；support bundle 压缩后二次 secret scan；cleanup 防 symlink/junction/path traversal。
- 诚实处理 `pnpm audit:prod`；有 failed 子项时 F12 不能 passed。

### R11-R12，34-36 小时：产品 E3 与 freeze

在一个尚未冻结的当前 fingerprint 上，用 branch-built App + isolated home + session MCP 串行验证四 surface，每个至少 20 轮。覆盖默认关闭、YOLO 不授权、双 session、stale observation、wrong target、timeout、worker kill、window/tab close、App exit、资源清理。每轮必须由独立 oracle 验证实际后置条件。

必跑：`git diff --check`、前端相关 Vitest、`pnpm typecheck`、`pnpm lint`、Rust fmt、core Clippy `-D warnings`、core/driver tests、App check、每 target runtime check、bundle audit、locale parity、dependency check/audit。

从 manifest 聚合状态并验证每条 log 的存在/size/hash/fingerprint。required 本机项都 passed 才写 F13 freeze fingerprint。freeze 后禁止改代码、测试、runner、extension、配置和 runtime。必须修代码时先标 invalidated，修后重新跑 R11/R12。

### R13，36-48 小时：至少 12 小时主动长稳

按 scenario 独立累计：

- desktop-appshell：至少 2h/60 轮
- managed-product：至少 3h/100 轮
- webview-product：至少 2h/100 轮
- existing-tabs-product：至少 2h/100 轮
- fault-stop-restart：至少 2h/80 轮
- privacy-runtime-package：至少 1h/60 轮

每个保存 active_wall_s、rounds、ok、fail、consecutive_fail、first_failure、last_success、resource trend。每个 scenario 成功率至少 90%，且 unauthorized write、wrong target、post-stop dispatch、secret leak、orphan worker/browser/profile/lease/token/binding 全部为零。不同 scenario 不能互相清失败。runner 被杀、机器重启、证据损坏、fingerprint drift 或需要代码修复时，保留首败，旧 soak invalidated，新 freeze 后重新累计完整 12 小时。

## 七、持续执行规则

- 每 90 分钟或一个 batch 结束写 checkpoint，但写完 checkpoint 后继续下一个 ready batch，不等待用户回复。
- checkpoint 必须列 run id/fingerprint、完成 batch/evidence level、production files、测试精确计数、首个未解失败、安全 invariant、external blocker/recovery、下一项和 Git 边界。
- 遇到一个 external blocker，不要结束目标；标记对应原子格后继续其他 ready work。
- 一个问题最多做三次有证据、不同假设的聚焦修复。仍失败就保留首败，标 failed，继续不依赖它的工作；不要无限重复同一命令。
- 重型 Cargo、runtime、browser、App、installer 串行，避免争用 seed/target/profile。
- 不要问“是否继续”；除真实模型、外部设备或会改变用户环境的动作需要用户明确授权外，一直推进。

## 八、最终报告合同

最终报告必须列出：

- run id、repo、branch、HEAD、baseline/freeze/current fingerprint；
- R0-R13 每个原子状态、最高 evidence level、日志路径；
- Desktop/Managed/WebView/Existing Tabs 独立状态；
- Windows/macOS arm64/macOS x64/Linux X11/GNOME Wayland 独立状态；
- Chrome/Edge、source/installed、App/MCP/real-model 独立状态；
- 每个 soak scenario 的 active wall、rounds、ok/fail、首败和资源趋势；
- pairing threat model/负向测试；
- bundle/install/repair/upgrade/rollback/uninstall inventory；
- warnings、大文件、dependency audit 和技术债；
- manifest path/size/hash/fingerprint 完整性；
- external blocker 的具体恢复命令；
- 明确写 `未 commit / 未 push / 未提 PR`。

只要任何 required 项未通过，标题和结论必须原样写：

`partial — not releasable`

现在从 R0 开始。不要复用旧 passed，不要先写完成报告，不要停止在计划复述；持续做真实实现、验证、证据和下一 ready batch。


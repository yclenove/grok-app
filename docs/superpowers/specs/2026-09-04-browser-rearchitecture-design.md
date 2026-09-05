# 浏览器重构总体设计

**状态：** 总体设计与 M0–M6 独立计划已整理；产品实现尚未开始

**日期：** 2026-09-04

**修订：** 2026-09-05，中文规格一致性复核

**分支：** `codex/browser-rearchitecture`

## 摘要

Grok App 的浏览器将采用渐进式双后端核心，并在后续增加可选连接器：

```text
系统 WebView
  -> Preview：应用内快速预览，长期保留

固定版本 Managed Chromium
  -> Agent Browser：macOS 与 Windows 首发

Chrome Connector
  -> 后续由用户显式认领已有 Chrome 标签页或标签组
```

现有系统 WebView 不会在 M1 被替换，也不承诺最终删除。它继续承担启动快、成本低的 Preview。独立打包并签名的 Managed Chromium 为 Agent Browser 提供可复现的浏览器版本与 Playwright 交互语义。当任务确实需要用户当前 Chrome 登录态时，后续的 Chrome Connector 才允许用户显式认领 Chrome 标签页或标签组。

用户选择的是任务和意图，不需要每次先选择由哪个浏览器打开。普通 URL、localhost 和只读查看默认进入 Preview；点击、输入、稳定截图、Console、Network 或 Trace 等能力会触发可见的 Managed 升级；用户明确要求使用当前 Chrome 登录态时才选择 Chrome Connector。用户始终可以手动覆盖路由；一旦固定后端，系统不得静默改回其他后端。

所有控制统一经过 Rust Host Browser Gateway。Rust Host 是唯一权限源，也是 Grok Build ACP、第一方浏览器 MCP、Preview、Managed Chromium 与未来 Chrome Connector 的统一接入点。BrowserSession、BrowserProfile、BrowserRuntime、ControlLease、BrowserTab、Artifact 和 AgentBinding 都是彼此分离的领域对象。用户接管会原子撤销 Agent 控制、封住已派发的复合动作、暂停观察，并要求用户显式交还后才能恢复。

本文是 umbrella design，只定义产品合同、架构、安全模型、发布硬门和里程碑边界。M0 至 M6 已拆为十个独立规格与开发计划，见[实施总索引](../plans/2026-09-05-browser-rearchitecture/README.md)。运行验收记录须在后续实际开发时产生，不能用文档完成代替；每个工作包按自身依赖和出口实施。

## 目标

- 把浏览器变成一等任务工作区，提供 Codex 风格的浏览器任务分组、清晰状态与可预测路由。
- 为普通 URL、localhost 和只读检查保留快速 Preview 路径。
- 提供可复现的 Agent Browser，具备成熟的 locator、actionability、frame、下载、截图、Console、Network 与 Trace 能力。
- 摄像头、麦克风、位置等系统能力遵循操作系统权限。
- 让权限、用户接管、高风险动作、Artifact、恢复和进程清理均可审计、可测试。
- Managed Runtime 只通过经过验证、签名且匹配平台的发行渠道交付。
- 遵守现有 i18n、设置 IA、对话框、媒体交付、SSH 和 AppWorkbench 约束。

## 非目标

- 不在 M1 删除或替换系统 WebView，也不把最终替换 WebView 作为路线承诺。
- 不在本路线中直接把 CEF 嵌入 Tauri；CEF 只保留为 M6 的独立数据决策门。
- 不自动导入 Chrome、Cursor 或系统浏览器的 Cookie、密码、历史、扩展等隐私数据。
- 不把 screencast、截图流或窗口 reparent 伪装成完整嵌入式浏览器。
- 不开放本机 remote-debugging TCP，不接受任意 CDP 客户端，也不依赖 PATH、外部 Node、外部 Chrome 或 `npx`。
- 不在首次使用 Agent Browser 时在线下载 Chromium。
- 崩溃后不自动重放结果不确定的写操作、表单提交、上传、下载、支付、发布、发送、删除或账户/安全设置变更。
- 不承诺支持任意 Shadow DOM 或跨域 iframe，只支持各里程碑确定性 fixture 明确声明的子集。
- 在没有用户同意的度量机制前，不声称拥有全量生产 Browser 遥测或所有机器通用的性能 SLA。

## 产品原则

1. **意图优先于引擎。** 用户表达任务，路由器选择能真实完成任务的最低权限后端。
2. **显式升级。** 能力升级必须可见、可解释、可撤销；不支持就是不支持，不能静默降级后伪造成功。
3. **唯一权限源。** 只有 Rust Host 能决定哪个 connector、Profile、origin、Artifact 与 lease 可以执行操作。
4. **用户输入就是 fence。** Host 收到归属明确的原生输入后立即撤销 Agent lease，并阻止旧命令继续执行；无法证明输入归属时不授予 writer lease。
5. **URL 不是身份。** Host UUID 稳定；WebView label、CDP targetId、Chrome groupId 和 URL 都只是可替换绑定。
6. **保守恢复。** 只恢复安全上下文并说明不确定性，不恢复 Agent 自动驾驶。
7. **证据先于开放。** 权限、打包、恢复、E2E 和发布门必须一起通过，能力才可打开。

## 当前实现与缺口

当前内置浏览器是有价值的 Preview 基础，但还不是 Agent Browser 的安全边界。以下事实已在分支基线 `a43e3071` 核对；实现前应重新核对变化：

| 区域 | 当前证据 | 设计后果 |
| --- | --- | --- |
| 前端浏览器模型 | `src/lib/sideWorkbench.ts` 的浏览器标签主要只有 `id`、`url`、`title`、`name`。 | 引入 Host 持有的 BrowserTab 与 BackendBinding；React 只消费投影。 |
| 浏览器组件 | `src/components/side-workbench/BrowserTab.tsx` 持有大量 loading、Design Mode 和 WebView 状态。 | 持久领域状态迁入 `src/components/browser/`、provider、hook 和 Host command。 |
| 自动化 | 当前 Design Mode 依赖 `side_browser_eval` 注入与轮询。 | Design Mode 可继续作为 UI 功能，但任意 eval 不能充当 Agent Gateway。 |
| Tauri Host | `src-tauri/src/side_browser_host.rs` 创建子 WebView，并按 URL 索引待处理下载。 | 下载改用唯一 transfer ID；URL 不能作为下载身份。 |
| 媒体交付 | `src-tauri/src/media_server.rs` 使用进程级 token，并接入范围较宽的 `path_scope`。 | Browser Artifact 使用会话级 capability token 与 opaque handle；MCP 不暴露绝对路径。 |
| Runtime 生命周期 | `host_runtime` heartbeat 能识别 Host 状态，但不能证明 Browser descendants 已清理。 | 增加 BrowserRuntimeSupervisor、ProfileGuard 和 descendant ledger。 |
| 打包 | Windows portable 当前只复制 `Grok.exe`，没有 Browser Runtime 树。 | portable 必须携带完整不可变 Runtime tuple，并验证每个可执行文件。 |
| 验证 | 当前 CI 没有真实浏览器跨平台 E2E，也没有最终安装包真机验证。 | 增加确定性 fixture、真机 package matrix、签名检查和 kill-switch 演练。 |

可复用基础包括 `store_lock::write_bytes_atomic`、`host_runtime` 的 unclean detection 和 `audit_ledger` 的 append-only 记录。Browser Profile 的长期锁必须使用独立类型；现有 `ExclusiveLock` 可能持有全局 mutex，不适合承担长期 Runtime 所有权。

## 外部调研与证据边界

以下结论来自固定 revision 的公开源码和官方公开文档。公开事实与设计推断必须严格区分。

### OpenAI Codex

[OpenAI Codex 公开仓库](https://github.com/openai/codex) revision `b3f5e45cc1de8bcb09d320f3211378db285aa201` 可以直接核对 Browser Use 配置、origin policy、上传/下载、Full CDP 门控、turn/thread 审批和身份传递。这些公开模式支持本文的 Host 权限模型与审批生命周期。

ChatGPT 桌面 Browser、其 Chromium Runtime 和 Chrome 扩展不在该公开仓库中，因此本文不把它们的内部实现当作已知事实。

### Cursor

[Cursor Browser 官方文档](https://cursor.com/docs/agent/tools/browser) 描述了 secure web view 与扩展内 MCP，公开能力包括导航、点击、输入、截图、Console 和 Network，并描述 workspace/tab/token 隔离。此次调研未找到 Cursor IDE 内置 Browser 的公开源码，也未找到 `cursor.browserView.*` 的稳定公共 API 合同；这些证据不能证明其内部实现。闭源发行物 `cursor-browser-automation` 不会被复制或再分发。

### Grok Build

[xAI Grok Build 公开仓库](https://github.com/xai-org/grok-build) revision `72a61251fcffb464bcc687aeb5a998e5a98ec0c9` 提供了可借鉴的 ACP/MCP、Computer Hub、session registry、cleanup 和流式结果模式，但没有完整的 Playwright/CDP Browser Runtime。本文复用的是协议与会话思想，不假设 Grok Build 已经解决浏览器隔离。

### 辅助项目

[Anysphere Tabshare](https://github.com/anysphere/tabshare) revision `49537abffcbe18f1476eed5630b68127193eda5b` 的 `background/src/background.ts` 会枚举承载 `tabs.day` 的窗口标签，并同步创建、更新和排序事件。它证明扩展同步窗口标签的路线可行，但没有提供本文要求的逐标签显式认领、权限隔离或正式任务组协议；这些合同需要 Grok App 自行设计。它不是 Managed Chromium 的替代品，也不是 Cursor Browser 源码。

已评估 [`chromiumoxide`](https://github.com/mattsse/chromiumoxide) 0.9.1。所核对版本的标准启动路径在 `src/browser/config.rs` 添加 `remote-debugging-port`，且没有提供本文所需的完整 Playwright actionability 合同，因此不作为 M1 的首选；这不代表它无法通过二次开发支持其他 transport。Managed worker 选择 `playwright-core`，因为它已经提供成熟的 locator、auto-wait、frame、声明范围内的 Shadow DOM、popup、文件、Console、Network、截图和 Trace 原语。

本文没有任何结论依赖 Codex、Cursor 或 ChatGPT 的私有源码。对闭源产品，只把公开行为当作参考，最终合同由 Grok App 自身的安全要求决定。

## 已考虑方案

### A. 继续扩展系统 WebView

该方案打包成本最低，也能保留现有应用内体验。但它无法提供固定 Chromium revision、一致自动化语义、可靠 Trace 和 Agent 凭据隔离，平台 WebView 差异会直接变成产品合同。

结论：保留为长期 Preview，但不能作为唯一后端。

### B. 立即在 Tauri 中嵌入 CEF

CEF 可能提供更深的一体化 Chromium 视图，但会同时引入大型原生发行物、平台窗口集成、升级、崩溃隔离、许可证和安全维护成本。第一版若同时解决嵌入和 Agent 控制，风险过于集中。

结论：不作为既定终点，只保留为 M6 的数据决策门。

### C. 渐进式双后端核心，加后续 Chrome Connector（推荐）

Preview 保留快速启动与本地查看；签名的 Managed Chromium 提供固定浏览器和 Playwright 语义；确实需要用户现有 Chrome 状态时，再由用户显式认领。统一 Workbench 与 Gateway 提供一致产品合同，但各后端的安全与生命周期仍相互隔离。

结论：该方案比 A 增加 Runtime 打包成本，但无需先承诺 CEF，就能交付真实可用的 Agent Browser。

### D. 始终驱动用户外部浏览器

这可以减少 Runtime 打包，却会绑定任意浏览器版本，扩大隐私面，并让 tab 所有权、扩展可用性和进程清理都变得含糊。

结论：仅作为 M3 中由用户显式认领的 Chrome Connector 路径。

## Workbench、任务分组与智能路由

### 任务分组

Workbench 在聊天和文件工作旁显示浏览器任务组。每个任务组包含稳定 `BrowserSession` ID、标题、紧凑标签栏、当前标签、后端/Profile 标识、连接状态、Activity，以及暂停、接管、交还、显示窗口、诊断和关闭控制。

Codex 风格的任务分组在 Preview 2.0 即落地，不等待 Chrome Connector。关闭或隐藏 React pane 只会 detach UI 投影，不会结束仍在运行的任务；显式关闭 BrowserSession 才会请求优雅停止并记录最终状态。

一个 AppSession 可以拥有多个 BrowserSession；一个 BrowserSession 只能属于一个 AppSession，并且任一时刻只有一个当前 `ProfileRef`。归档会挂起任务组并撤销 binding/lease。删除 AppSession 会 tombstone 其 BrowserSession，但不会删除持久 project/named Profile。Fork 创建全新的 BrowserSession，不继承 grant、lease、binding、tab 或 command queue；用户可以再显式选择已有 named Profile。Rewind 永远不会撤销外部网页副作用，也不会复活旧浏览器状态。

### 自动路由

| 任务意图或能力 | 默认后端 | 原因 |
| --- | --- | --- |
| 打开普通 URL、localhost 或只读资料 | Preview | 启动快、开销低。 |
| 点击、输入、安全表单、稳定截图、Console/Network 检查或 Trace | Managed | 需要 Agent 级 actionability 和确定性检查。 |
| 使用用户当前 Chrome 登录态、标签或标签组 | Chrome Connector | 必须由用户显式认领并完成扩展握手。 |
| Managed/Chrome 能力不可用 | Preview 或明确失败 | 不得把 Preview 说成已完成 Agent-only 操作。 |

路由器依据任务意图、所需 capability、实时 policy、Profile 可用性、平台和用户 override 作出确定性决策，并在升级前发出 reason code。用户始终拥有手动覆盖入口；用户固定后端后，路由器不得静默改变。固定后端无法满足能力时，只能建议兼容后端，并等待用户确认。

### 后端切换合同

Preview、Managed 与 Chrome 之间只允许传递：

- canonical URL 与 navigation intent；
- viewport 尺寸与 device-scale intent；
- BrowserSession 和当前 BrowserTab 身份；
- 用户明确的任务与 capability 请求。

禁止传递 Cookie、storage、密码、历史、表单值、file grant、download 和 opaque backend handle。切换会生成新的 BackendBinding 和新 snapshot。若 URL 无法安全复现，UI 必须说明上下文未迁移，并提供人工继续入口。

### 可见状态

Workbench 根据正交领域状态派生 `starting`、`ready`、`navigating`、`agent-running`、`user-control`、`paused`、`blocked`、`disconnected`、`crashed` 和 `closed`。所有状态都必须有本地化标签和可执行下一步；loading、empty、busy、error、unsupported 路径均使用同一 Workbench chrome。

菜单与对话框复用现有 `Select`、`ContextMenu`、`GlassModal`、`setAppDialog` 和实体 panel 样式。不得引入原生 `<select>`、浏览器业务右键菜单、透明菜单、点击穿透或错误 z-index。

## Browser Gateway 与第一方 MCP

### 调用链

```text
Grok Build ACP
  -> 第一方 grok-browser MCP
  -> Rust Host Browser Gateway
  -> Preview / Managed / Chrome capability adapter
  -> 目标 tab 与 Artifact Store
```

第一方 Browser MCP 独立于 `official-aux` 和用户 MCP。它必须同时覆盖 official main 与 custom main 路由，不能被现有“`official-aux` 只对 custom main 注入”的规则误伤。只有 Gateway 可以签发 Browser、origin、Profile、文件与控制 capability。

本机 transport 不能因为“仅当前用户可访问”就被视为可信。每条 MCP connection 还必须在握手时提交 Host 签发的 launch-scoped connector credential。该 credential 在 Host 重启时轮换，绑定连接且不写日志、不持久化。

注入复用本地 ACP `session/new` / `session/load` 的 `mcpServers`，同时覆盖冷启动、预热复用与恢复路径。Shared 和 Independent 都采用会话级注入，不改写共享 `~/.grok/config.toml`；关闭用户第三方 MCP 不应移除第一方 `grok-browser`，但 Host 的 Browser capability/权限禁用必须生效。连接断开后清理注册，重启去重，同名用户 MCP 不得冒充第一方身份。M1 的 SSH ACP 继续省略本地 Browser MCP。

### 请求身份链

每个 MCP 请求绑定 connector 身份、AppSession、BrowserSession、BrowserTab、ProfileRef、turn/thread 和当前 connection。命令携带 `hostBootId`、`backendBindingGeneration`、`leaseId`、`fenceEpoch`、`requestId` 和 `expectedTabRevision`。所有事件携带 `backendBindingGeneration`、`tabRevision`、`navRevision`；Managed 事件额外携带 `runtimeGeneration`。旧身份或 revision 必须结构化拒绝，不能落到恰好复用了旧 targetId 的标签上。

Host 每次请求都完整校验：

```text
authenticated connector
  -> active AgentBinding
  -> AppSession
  -> BrowserSession
  -> BrowserTab
  -> current BackendBinding
  -> ProfileRef / ControlDomain
  -> current ControlLease
  -> live frame / origin
  -> requested capability
```

每条关系都必须匹配已存 owner 和 generation；持有任意单个 UUID 都不足以授权。AgentBinding 记录 connector/server 身份、ACP connection、App/agent session、binding generation 与 bound/revoked 时间，并在该绑定内登记当前有效 turn。绑定持续到连接断开或明确撤销；turn 结束只清除当前 turn、撤销 turn-scoped grant/lease、取消队列和观察，不销毁仍连接的 thread binding。下一 turn 必须由 ACP 身份重新登记，不能由 MCP 自报。默认只允许一个 writer；另行批准的 observer 只能读。

结果使用闭集状态，例如 `ok`、`denied`、`stale`、`unsupported`、`needs_confirmation`、`needs_takeover`、`retryable_failure`、`interrupted`、`unknown_outcome` 和 `runtime_capacity`，避免客户端把能力不可用折叠成成功。

### Origin policy

`OriginPolicyRule` 是持久 allow/block 配置；`OriginGrant` 是短期运行时授权。OriginGrant 绑定 AppSession、BrowserSession、AgentBinding、ProfileRef、canonical origin、capability set、`turn | thread` scope、policy revision、签发/过期时间和撤销状态。

优先级固定为 managed deny > user block > exact allow > default。导航离开 origin 只会让 grant 不再匹配；expiry、policy revision 或 binding revocation 才会使其失效。任何持久规则都不能跳过即时高风险确认。

turn grant 随当前 turn 结束而失效；thread grant 可以跨同一有效 AgentBinding 内的 turn 存活，但不保存 ControlLease、队列或观察权限。连接断开、Host 重启、BrowserSession 关闭/归档/删除、Profile 或 backend binding 改变均撤销相关 grant；reconnect 必须重新授权。每次新 turn 仍检查当前 policy、输入 Provider、接管锁存状态与 kill switch，再决定是否签发新 lease。thread grant 不能代替用户显式交还。

Host 从当前 BackendBinding 计算 origin，不相信 MCP 声明的 origin。origin 按 scheme、IDNA host 和 effective port canonicalize。DOM 动作同时验证顶层 origin 与执行 frame origin；跨域 frame 必须同时获得两者授权。popup/opener 在任何观察或动作前先分配所有权。Console 和 Network 还要验证内容产生方或请求目标 origin，而不只看顶层页面。

`about:blank` 和 `blob:` 只能继承经过验证的 creator/opener origin；`data:`、`javascript:`、`file:`、opaque 或无效 origin 默认拒绝，或进入独立于 OriginGrant 的专用 capability。

### Capability surface

M1 提供导航、受限 DOM/locator 动作、安全输入、截图、Console/Network metadata、受限下载、已批准上传和 Artifact handle。

Full CDP 是独立 Developer Mode capability：默认关闭、短租约、单独批准，并且只能运行在恰好包含一个 BrowserSession 的独占 Runtime。默认必须使用 disposable ephemeral Profile；如需 persistent Managed Profile，必须再次进行高风险确认，且该 Profile 不得附着其他 BrowserSession。Full CDP 不向 Preview 或 Chrome Connector 开放，也不得与共享 Runtime 模式共存。需要单项 CDP 能力时，应优先提供 capability-scoped CDP proxy，而不是开放 Full CDP。

## 权限、安全与用户接管

### 现有会话权限映射

Browser 沿用现有会话权限作为主入口，不新增另一套 Browser YOLO 模式。以下均是 Host Gateway 的行为映射，不改变 Grok Build CLI 的 enum 或 spawn 语义；CLI 对 MCP 的一次许可不能替代 Browser 动作审批。

| App permission policy | Browser 行为 |
| --- | --- |
| `ask` | 未授权 origin/capability 先询问；已批准范围内允许只读观察，写操作逐次询问。 |
| `accept_edits` | Browser 按 `ask` 处理；代码编辑许可不等于网页写入许可。 |
| `allow_for_session` | 按明确批准的 origin、capability 与 turn/thread scope 缓存普通操作许可。 |
| `auto` | 在站点策略允许范围自动执行低风险读取、导航和滚动；不确定写操作与后果性动作仍询问。 |
| `dont_ask` | 仅执行已有授权覆盖的动作；需要新确认时立即返回 `denied`，不创建等待弹窗。秘密步骤返回 `needs_takeover`。 |
| `always_approve` / YOLO | 普通 Browser 操作可自动执行；仍遵守站点 deny、后果性动作即时确认、秘密接管与 OS 权限。 |

用户切换权限后，Host 递增 policy revision，撤销不再匹配的 grant/lease，并 fence 受影响动作。`dont_ask` 遇到高风险动作的结果是拒绝，不能用“不可绕过确认”把它变成等待输入的挂起任务。

### 高风险确认

支付、最终下单、发送、发布、删除、账户变更、安全设置变更和其他不可逆外部副作用，都必须在执行前进行可见、本地化的即时确认。创建或扩大持久权限规则也必须确认。有效的既有非高风险规则只能在声明生命周期内授权，永远不能绕过高风险闸门。

文件已经 attached 不等于表单已经 submitted；最终提交仍是独立高风险动作。

密码、OTP、Passkey、CAPTCHA 等秘密步骤必须由用户接管。Agent 可以说明需要什么并等待信号，但不能读取、保存或重放秘密。

摄像头、麦克风、位置和其他 OS 能力继续走操作系统权限提示。Browser policy 只记录结果与原因，不伪造系统授权。OS 拒绝、缺少 entitlement 或权限被撤销均是最终结论，UI 显示明确 blocked 状态。

### 接管 fencing

接管信号是 Host 的 `NativeInputFenceProvider` 归属到当前 ControlDomain 的 OS 原生输入，不是对物理设备来源的密码学证明。远程桌面和辅助输入也保守地视为外部控制。DOM `event.isTrusted`、内容脚本、CDP 事件和 Playwright 回调都不能证明输入来自用户，因为自动化输入也可能表现为 trusted。M1/M3 的 Gateway 只通过 Playwright/CDP 自动化，不使用 `CGEventPost` / `SendInput` 等 OS 输入注入。

| 后端 | 必须验证的原生来源 | 接管范围 |
| --- | --- | --- |
| Preview | App 拥有的窗口/WebView 分发前原生输入路径 | 当前 `previewWebView(bindingId)`；不提供 Agent DOM/input capability。 |
| Managed | 平台原生输入 Provider，绑定已登记 Runtime PID/窗口集合 | 整个 `managedRuntime(runtimeId)`，包括其中全部 BrowserSession。 |
| Chrome Connector | native-messaging companion 绑定实际浏览器进程身份，再使用平台原生输入 Provider | 整个 `chromeConnection(connectionId)`；任一相关窗口输入撤销该连接下全部 claim 的控制。 |

macOS 的候选路径是 Host 内 listen-only `CGEventTap`，需要探测 Input Monitoring 权限；Preview 可使用 App 内原生事件。Windows 的候选路径是专用消息线程上的 Raw Input、必要的低级 hook 与 HWND/PID 归属，不要求管理员或 `uiAccess`。具体可行性、监听失效检测和签名安装包权限行为都必须由 M0 真机 PoC 证明，不能只依据开发机表现。Chrome 扩展没有稳定的 tab 到原生窗口安全映射，不能承诺逐 tab 的输入归因。

pointer down、touch、wheel、key down 与 IME 的可覆盖范围写入平台 fixture；hover 不触发接管。没有对应原生输入事件的 AX/辅助技术操作不在 M1 自动接管承诺中，需要显式接管入口。任一受控 ControlDomain 收到第一个归属明确的原生输入时，Host 必须原子执行：

1. 递增 `fenceEpoch`；
2. 设置 Host 权威 `TakeoverLatch=user`，并撤销当前 ControlLease；
3. 关闭新命令 admission；
4. 取消 in-flight task；
5. 清空排队命令；
6. 暂停 DOM、截图、Console 和 Network 观察；
7. 写入不含敏感 payload 的 audit event。

每条 worker command 携带 `hostBootId`、`controlDomainId`、`leaseId`、`fenceEpoch` 和 `requestId`。Gateway 在派发前校验；worker 在每个可观察或有副作用的 primitive 之前，以及每次 await/actionability retry 之后重新校验。

如果某个 primitive 与撤销发生竞态，该 primitive 记为 `unknown_outcome`，同一复合命令的后续 primitive 一律不得执行。`TakeoverLatch` 独立于 lease，不能因为 lease 已撤销、过期、turn 结束或连接重建而自动清空。UI 保持 `user-control`，直到用户在可信 App UI 显式交还。交还重新验证 binding、Provider 和 policy，清除 latch，生成新 lease、新 command queue 和新 snapshot，绝不续跑旧队列；Agent 请求不能触发交还。

授予/续期 writer lease 与派发动作前，Host 必须证明 Provider healthy、平台权限有效、进程/窗口归属和 backend generation 匹配。权限拒绝/撤销、监听失效、目标归属含糊或 Provider 健康不可证明时，立即 fence、暂停观察并进入 `blocked_input_attribution`；恢复探测也不能自动交还。UI 提供系统设置和重新检测，人工 Preview 继续可用，生产环境没有绕过开关。

参考机上，从 Host 收到权威原生输入事件到 Host 拒绝命令并收到 worker fence acknowledgement 的目标为 p95 <= 250 ms；不把不可观测的物理输入时刻当测量起点。每个后端使用自己的原生来源验证等价 fencing 结果。Managed 是 M1 硬门，Chrome 是 M3 硬门，不能把 M1 验证算作 Chrome 已通过。

### 审计与脱敏

持久 Audit 只记录 action class、canonical origin、opaque actor/session/profile/tab ID、capability、policy revision、requestId、decision 与 outcome，包括 `interrupted` 和 `unknown_outcome`。

完整 URL/query、DOM 文本、selector、输入内容、header/body、Cookie、token、文件路径、密码/OTP 和未脱敏 worker stderr 均不得进入持久 audit model 或 support bundle。MCP 只接收 opaque Artifact handle，永远不接收绝对路径。

## 领域模型与状态机

### 核心对象

1. **BrowserSession**：用户可见的浏览器任务组，只属于一个 Grok App AppSession；关闭 React pane 只 detach 投影。
2. **BrowserProfile**：App 拥有的 Managed 站点状态目录，类型为 `project | named | ephemeral`。ID 是随机 UUID，项目移动不改变 ID；项目删除只把它变成 orphan。
3. **ProfileRef**：判别联合 `managed(profileId) | preview(partitionId) | chrome(connectionId, claimId)`。Preview 与 Chrome 不会被伪装成 App-owned BrowserProfile，Grok App 不删除外部 Chrome 数据。
4. **BrowserRuntime**：Node、Playwright worker 与 Chromium 进程树。同一活跃 Managed Profile 只有一个 Runtime；多个 BrowserSession 可以共享。
5. **ControlDomain**：`managedRuntime(runtimeId) | previewWebView(bindingId) | chromeConnection(connectionId)`。ControlLease 在该 domain 授予唯一 writer，并包含 lease ID、expiry、`fenceEpoch`、actor 和 policy revision。独立的 TakeoverLatch 保存用户接管状态；claim 仍约束可访问 tab，但不是 Chrome 的输入隔离单位。
6. **AgentBinding**：把已认证 ACP/MCP connection 绑定到 BrowserSession，在绑定内登记有效 turn，避免 reconnect 或 MCP 自报 turn 冒充原连接。
7. **BrowserTab**：使用 Host UUID。CDP targetId、WebView label、Chrome tab/group ID 只是易失 BackendBinding；URL 不是 identity。
8. **Artifact**：拥有长期 ID 的不可变 Browser 输出；长期 ID、短期 access token 和 live capture handle 是三种不同概念。
9. **UploadGrant / Download**：显式、独立授权的 transfer，不从聊天附件或 URL 隐式推导权限。

### 正交状态机

```text
BrowserSession: provisioning -> ready -> suspended | recovering | blocked
                -> closing -> closed
Projection:     attached <-> detached
ControlMode:    none | agent | user              (由 TakeoverLatch + ControlLease 派生)
TakeoverLatch:  clear -> user                    (仅可信 App UI 交还可清除)
BrowserTab:     opening -> open -> detached | crashed -> closing -> closed
Navigation:     idle -> requested -> provisional -> committed | failed
BrowserProfile: creating -> ready -> migrating -> ready | needs_repair
                -> deleting -> deleted
BrowserRuntime: absent -> installing -> starting -> healthy -> draining
                -> stopped | failed | quarantined | cleanup_failed
ControlLease:   none -> granted -> renewed -> revoked | expired
AgentBinding:   binding -> bound -> revoked
Artifact:       writing -> ready -> promoted | deleting -> deleted
                | failed | interrupted | expired
UploadGrant:    proposed -> awaiting_approval -> authorized -> attached -> consumed
                | denied | expired | cancelled | failed
UploadStaging:  copying -> ready -> purging -> purged | purge_failed
Download:       requested -> awaiting_policy -> receiving -> staged -> exporting
                -> exported | needs_review | denied | cancelled | failed | interrupted | expired
```

Projection、Navigation、Runtime health 和 ControlMode 与 BrowserSession lifecycle 正交，UI visible state 只能由这些权威状态派生，不能持久化第二份真相。committed navigation、tab 创建/关闭/顺序与 Session 状态转换需要 checkpoint；高频 loading、progress 和 lease renewal 只留内存。

Runtime target registry 维护完整映射：

```text
(runtimeGeneration, targetId)
  -> (BrowserSessionId, BrowserTabId, tabRevision)
```

tool 只能枚举所属 BrowserSession 的 target。popup 在 Host 根据 opener 分配 Session 前不可观察。共享 Runtime 同时只允许一个 Agent writer；任一窗口的可信用户输入会撤销整个 ControlDomain 上所有 BrowserSession 的自动控制，所有受影响任务组都显示接管状态。关闭一个 Session 只减少 Runtime ref；最后一个 ref 结束才 drain Runtime。

每次 Host 启动都会改变 `hostBootId`；每次 Runtime 或 backend 重启都会递增单调 generation。旧 binding、target、live capture stream、command 和 generation-scoped access token 全部失效。重启恢复的 BrowserSession 保守地设为 `TakeoverLatch=user`，等待人工恢复/交还，不从旧 lease 推导控制权。不可变 ready/promoted Artifact ID 继续存在，但必须在重新验证调用方身份后签发新的访问 capability；partial Artifact 标记 interrupted 并删除。

## Managed Runtime 架构

### 进程拓扑

```text
Grok Build
  -> grok-browser MCP
  -> macOS UDS 0600 / Windows user-only Named Pipe ACL
  -> Rust Browser Gateway + BrowserRuntimeSupervisor
  -> 每个活跃 Managed Profile 一个 ProfileGuard + Node/playwright-core worker
  -> launchPersistentContext
  -> 与 Playwright 精确匹配的完整 Chromium
```

Host 到 worker 使用私有、有长度帧且带背压的 stdio RPC。Playwright 到 Chromium 使用私有 pipe。worker stdout 只能传协议；stderr 进入有界诊断 ring buffer。截图、Trace 和 Network body 等大结果写入 Artifact Store，只返回 opaque handle。

worker 必须先收到 Host `initialize` 才能启动 Chromium。独立签名的 ProfileGuard 是 worker 的直接父进程，但不属于 Host 或受控 descendant container。它通过继承的私有 pipe 观察 Host liveness，并在无法建立完整监管时让 Managed fail closed。

Windows 上只有 ProfileGuard 持有 non-inheritable Job handle。它以 suspended 状态创建 worker，将其加入 Job Object，验证 `KILL_ON_JOB_CLOSE` 与禁止 breakaway 后，才 resume 并允许 `initialize`；Job 绑定窗口内不得执行任何 worker 代码。真实安装包测试必须证明 Chromium sandbox/utility process 无法 break away。

macOS 上 ProfileGuard 位于受控 process group 之外。Host pipe EOF 后先发 TERM，再在有界时间后发 KILL，reap 自己拥有的 child，并按 descendant ledger 逐个确认身份已退出，最后才释放 Profile lock。startup orphan audit 只是二级补救，不是主要保证。

新 Host 必须先获得独立 reconciliation lock，核对上一次 run nonce/ledger 并证明旧 descendants 已退出，随后才能获得或启动同一 Profile Runtime。Host 崩溃不得让仍被旧 worker/Chromium 使用的 Profile 重新变为可写。PID、heartbeat、`owner.json` 和 TTL 只能辅助诊断，不能授权偷锁；禁止按进程名 broad kill。

ProfileGuard 使用已验证 hash 的绝对 executable path、无 shell 启动 worker；使用私有 cwd 与 env allowlist，不继承 API key、auth/proxy secret、agent home 或无关 handle。需要代理凭据时，通过 scoped secret channel 交付。worker 不监听 TCP，只接收 framed stdio/private pipe，并且只看见 capability-scoped Profile、staging 与 Artifact 路径。Chromium sandbox 不得关闭。任一进程、handle、文件权限或 sandbox 约束建立失败，Managed 必须 fail closed，Preview 继续可用。

### Runtime tuple 与安装

Node patch、`playwright-core`、Chromium revision、helper binary、protocol version、target triple、capability、license、SBOM、source commit 与每个文件 SHA-256 固定为一个签名 Runtime tuple。manifest 至少包含 schema/runtime ID、release version、平台、架构、字节数、App 兼容范围、Profile epoch、key ID 和 capability。验证完成后 Runtime 目录不可变。

M1 把完整 Runtime tuple 放进签名 App installer/package，并与兼容 App release 原子升级。它拥有独立 embedded manifest 与验证合同，但没有独立在线 Runtime downloader。M6 才可在独立门控后启用使用不同签名 manifest、protocol 与 key 的 component updater。因此第一次打开 Agent Browser 不依赖联网下载。

提取前验证外层签名、archive size/hash、签名 manifest、平台/架构和单调 release sequence。只能提取到隔离的 no-follow staging root；绝对路径、path traversal、hardlink、device file、超量文件数或展开体积都必须拒绝。macOS Framework 必需的 symlink 只允许 manifest 精确声明的相对目标，逐跳验证始终留在该 tuple 内、无循环、无链接父目录逃逸；其他 symlink 一律拒绝。提取后，在运行任何 helper 前逐文件验证声明 hash、链接布局与适用的平台签名。

Host 持久化最高已接受 sequence 与签名 revocation floor，防止重放旧但签名正确的漏洞 tuple。只有在验证与 health check 全部成功后才切换 current pointer；中断、低磁盘或架构不匹配不得改变当前 tuple。活跃 Session 固定使用启动时 tuple，直到 drain。

防重放 sequence 与 Runtime 版本分离。接受新的发行/回滚指令要求该指令 sequence 不低于已接受值；显式回滚到旧 tuple 必须由更高 sequence 的签名回滚指令精确授权，且不得低于 revocation floor。本地一次自动 LKG 恢复只可选择已验证、已记录且被当前有效签名策略允许的 tuple，不重新接受旧发行指令，也不降低最高 sequence。没有授权的旧 tuple 即使签名正确也不可启用。

last-known-good 只有在 App/Profile epoch 兼容且没有被撤销时才可使用。startup handshake、disposable Profile 空白页截图或 close probe 失败，会 quarantine 当前 tuple，并且最多自动回滚一次；没有合格 LKG 时禁用 Managed，继续提供 Preview。

新 tuple 必须先在 disposable Profile 上 canary，才能打开 persistent Profile。Profile epoch 更高的 tuple 不得原地打开旧 Profile；显式、可验证的 migration 创建新 Profile generation，并保留原始版本。Runtime rollback 或 App downgrade 只能选择兼容 Profile generation，否则 UI 保留数据并提供兼容 Runtime、修复或新 Profile。任何回滚都不得 wipe、merge 或覆盖 Profile。

### 窗口、容量与平台

M1 中 Managed Chromium 是独立可见窗口。Workbench 同步任务组、tab、状态、缩略图、Activity、显示窗口、暂停、接管和交还。M1 为 Agent 操作与诊断采集有界 Console/Network 数据和 Trace Artifact；完整的人类 Console、Network、Trace 面板在 M2 交付。该合同不依赖 CEF。

M1 平台仅为 macOS arm64/x64 与 Windows x64。参考实现默认最多同时运行两个 ProfileRuntime，最终 ceiling 由实测决定。第三个请求优先复用相同 ProfileRuntime，否则返回 `runtime_capacity`；UI 再提供停止空闲 Runtime 或继续 Preview 的明确选择，不静默驱逐活跃 Profile。

Linux 属于 M4，SSH Browser Bridge 属于 M5，CEF 评估属于 M6。交叉编译不算运行证据；macOS x64 必须在真实 Intel 设备或 x64 VM 验收。

## Artifact、上传、下载与隐私

### UploadGrant

UploadGrant 绑定 BrowserSession、BrowserTab、顶层/执行 frame origin、input binding、源 Artifact 或已批准本地文件、预期 hash/size、ControlLease fence、single-use capability 和 expiry。

用户批准后重新执行 `lstat`、拒绝 symlink、进行有界复制到 App 私有 spool，并对副本再次校验 hash/size。只有授权 Runtime 链能读取该不可变副本；Agent 只看到 basename 和 opaque handle。下列任一事件都会撤销未使用 grant：navigation/origin 变化、用户接管、复制前源文件变化、turn/session 结束、Host 重启、用户取消或签发已满一小时。

M1 禁止目录上传。`attached` 不代表已经通过网络 `submitted`；最终提交仍单独经过高风险闸门。

消费、撤销、到期或失败后立即关闭上传 admission，但调用返回不等于 Chromium 已读完文件。M1 保留单文件 200 MiB 的路径上传，交付路径前持久记录绑定 runtimeId/generation 的 `UploadSpoolLease`；一旦路径交给 worker，副本保留至该 Runtime 的 Chromium/worker descendants 全部确证退出，再记录 `purging -> purged`，失败留 tombstone 由 reconciliation 重试。未派发的副本可立即清理。不能因 CDP ACK、worker 句柄关闭或 tab/Session 结束就提前删除，必须验证延迟 FileReader、延迟提交和大于 50 MiB 的真实用例。

这是 Runtime 持有的私有上传副本，不是普通 24 小时 Artifact；所有尚未物理删除的副本计入总容量。Runtime Doctor 显示 cleanupPending/占用，并提供用户显式停止 Runtime 后清理的入口。接管只撤销自动控制，不为删文件强杀用户正在使用的浏览器。已附着文件可能被网页脚本立即读取或发送，因此 attach 本身按对该 origin 的文件披露授权，撤销不能声称收回已到达网页的字节；竞态只记录 `unknown_outcome`，不自动重传。Chrome Connector 不拥有用户 Chrome 的退出权，M3 必须另证字节接管和副本清理策略，不能机械复用 Managed 退出条件。

### Download

每次下载使用唯一 `downloadId`，禁止按 URL 做 key。持久记录绑定发起 Session/Tab/origin chain、policy decision、staged Artifact ID、hash/bytes 和私有 destination reference。

`receiving` 阶段崩溃会标为 `interrupted` 并删除 `.partial`。已 `staged` 的项目可在重启后恢复为“等待保存”，但不得自动 export。`exporting` 阶段崩溃必须检查目标，Download 进入 `needs_review`，相关命令 outcome 为 `unknown_outcome`；不能把二者当作同一状态枚举，也不能盲目覆盖或重试。

staging 已携带 quarantine/MOTW 时予以保留；缺少时在 export 阶段应用并验证。应用不得自动打开或执行导出文件。

### Artifact 与保留

Artifact 使用短期、单主体的 session/artifact capability token，不复用当前进程级 media token，也不只依赖宽泛 `path_scope`。MCP 不返回绝对路径。

Artifact 创建顺序必须可抗崩溃：写入 `.partial` -> 校验 hash/size -> 原子 rename -> 原子发布 `ready` metadata。Promotion 先写 durable owner reference，再写 chat journal reference。chat 写入失败只留下可 reconcile 的 orphan promotion，绝不能先让聊天引用一个可能被 cleaner 删除的 blob。

refCount 从可重建 owner record 派生，并由 reconciliation 校正，不能成为唯一真相。临时 Artifact 与 staged download 默认保留 24 小时；用户路径中的 exported 文件永远不由 Browser cleaner 删除。

删除流程为 `tombstone -> async delete -> deleted`。cleaner 跳过 active lock、in-flight transfer 与有 durable owner 的 Artifact。project/named Profile 变为 orphan 后继续在 Settings 可见，只有用户通过应用内确认才能删除。ephemeral Profile 只在 Session、Runtime、lock 和 reference 全部结束后删除。

关闭/删除 AppSession 或 BrowserSession 时，Host 先关闭命令 admission、fence 并撤销 grant/binding/lease、取消 transfer/观察，然后持久化 Session 的 closing/tombstone，最后释放 tab 和 Runtime reference。共享 Runtime 只在最后一个 reference 结束后 drain；Session 只有在自己的资源清理得到确认后才报告 closed/deleted。已派发的上传副本先由 durable UploadSpoolLease 接管为 Runtime-owned cleanupPending，Session 结束不伪报副本已物理删除，也不阻塞共享 Runtime 的其他 Session。元数据写入失败必须显式报 degraded，继续阻止动作。删除 project/named Profile 是另一个用户确认操作，不能随会话删除执行。

Profile cleanup 只能操作已知随机 UUID 根，不跟随 symlink，并可从 tombstone 续删。浏览器崩溃不等于 Profile 损坏，绝不自动 wipe 或 merge。

### 持久化与存储边界

M1 不引入 SQLite。领域 aggregate 使用 `store_lock::write_bytes_atomic` 原子写 JSON。Runtime 只持久化 descriptor、单调 generation counter、`hostBootId`/run nonce 与 descendant reconciliation ledger。PID、pipe endpoint、connector/worker token、ControlLease、active AgentBinding 和 live progress 均不得重新水合为权限真相。

每个对象的 UUID manifest 是 source of truth，index 可重建。所有可变文件使用原子替换和 user-only permission/ACL，且不得进入 support bundle。持久化失败会让 Browser domain 进入可见 degraded 状态，阻断新副作用和 durable grant，同时保留 read-only Preview。

由平台 app-data resolver 提供私有 `browser/v1` 根，代码不得硬编码 home path：

```text
browser/v1/
  sessions/<browserSessionId>/manifest.json
  profiles/<profileId>/manifest.json + user-data/
  runtimes/<runtimeId>/descriptor.json + descendant-ledger.json
  artifacts/<artifactId>/manifest.json + immutable bytes
  transfers/uploads/<uploadGrantId>/...
  transfers/downloads/<downloadId>/...
  audit/<date>.jsonl
```

Runtime program tuple 位于独立 immutable component store；exported 文件只存在于用户选择的路径。M0 必须逐平台验证 user-only permission、symlink 防护、OS backup 行为和 Chromium credential-at-rest protection。若无法证明持久 Profile 的静态凭据保护，则 persistent Managed Profile 保持关闭，只开放 ephemeral Profile。

## 崩溃恢复与重连

App 启动时改变 `hostBootId`，并撤销全部 ControlLease、AgentBinding、connector credential 和未消费 UploadGrant。恢复原则是恢复上下文，而不是恢复自动驾驶：

- 首先只恢复 tab metadata 和脱敏 display URL。
- 只有在崩溃前 `restorePolicy` 已明确允许该 approved origin 的 HTTP(S) GET，且该导航不是下载、表单结果、auth callback 或 sensitive URL class 时，才可自动导航。
- 其他情况打开 `about:blank` 并提供用户触发的“恢复页面”；仅仅 Method=GET 不能证明安全。
- 完整敏感 recovery URL 不进入 audit 或 support bundle。
- 不恢复 POST、表单、JavaScript、upload、download 接收/export 动作、lease、binding、token 或旧 command queue。已完成的 staged download metadata 可以恢复为“等待保存”，不发起新的网络请求或文件写入。
- 有副作用的命令必须在 dispatch 前持久化不含 payload 的 inflight record。
- 崩溃后未确定的副作用标为 `unknown_outcome`，要求用户检查，绝不自动 replay。
- `requestId` 只能防 transport 重试，不能证明网页业务幂等。
- Profile 所有权由 ProfileGuard lock 与 descendant reconciliation 共同决定；PID、heartbeat、`owner.json` 只用于诊断。
- Chrome Connector 重启后显示 `disconnected`，要求用户/扩展重新声明，不按旧 groupId 静默重连。

Runtime 失败时，UI 说明原因、使旧 generation 失效，并提供 Preview、修复和诊断。Preview fallback 只恢复查看上下文，不代表 Agent 动作已经完成。从 Runtime 失败到可见可用 Preview 的初始目标为 p95 <= 2 秒。

## M0–M6 全量路线

每个里程碑都必须独立编写规格和实现计划。权限、Profile、接管、打包、恢复和 E2E 未一起完成前，相关 feature flag 保持关闭。

### M0：Preview 2.0 与 Runtime 可行性

- 交付任务分组、共用 Browser Workbench、完整导航/状态模型、后端无关 tab identity，以及接入现有 Preview 的 Design Mode。
- 完成 Runtime tuple 构建/签名 PoC、ProfileGuard/process ledger 和 fixture harness，但不默认暴露 Managed Browser。
- 在全部 M1 平台验证 NativeInputFenceProvider：Gateway 的 Playwright/CDP 动作不自触发接管，外部原生输入撤销正确 domain，权限拒绝/撤销、监听失效、窗口变化和 generation 变化均 fail closed。未通过的平台不能授予 Managed writer lease，也不能计入 M1 同批完成。
- 硬门：AppWorkbench 行数不增长、i18n catalog lockstep、确定性任务组 E2E、package verification PoC、开发支持平台 descendants 清理为零。

### M1：完整 Managed Agent Browser

- 在 macOS arm64/x64 与 Windows x64 交付签名 Managed Chromium、Playwright worker、Profile lock、Gateway、路由、权限、接管、Artifact、恢复和可见窗口同步。M1 默认项目独立 Profile，并提供 ephemeral Profile；数据模型从 M0 保留 named Profile，命名/跨项目复用管理在 M2 完成。
- 硬门：跨平台 golden path、越权指标为零、旧 lease 无法通过、无 orphan descendant、最终包签名完整、opt-in beta 批准。

### M2：检查面板与响应式工作流

- 增加人类可用的 Console、Network、Trace 面板、Design Mode v2、响应式 viewport 和验证工作流。
- 完成 named Profile 创建、重命名、显式项目绑定、复用标识和删除确认；复用不会授予其他项目的 tab、Artifact 或 ControlLease。
- 硬门：脱敏、Artifact 保留、Trace export，以及 frame/Shadow DOM/跨域 fixture 声明范围全部通过。

### M3：Chrome Connector

- 交付扩展与 native-messaging companion 配对、显式 tab/group 认领、capability negotiation、撤销和 disconnected 恢复。
- 硬门：每个任务组均由用户显式认领；撤销立即生效；不自动导入 Cookie/历史；每条命令绑定 extension、实际浏览器进程与 Host 身份；原生输入 Provider 故障时不授予 writer lease。

### M4：Linux 功能对等

- 交付 Linux x64 AppImage/deb/rpm、X11/Wayland 覆盖，并保持 sandbox。
- 硬门：声明发行版达到等价安全与恢复证据；禁止用 `--no-sandbox` 过门。

### M5：SSH Browser Bridge

- 增加本地 Browser Bridge，经受管 tunnel 验证远端 localhost；Browser Runtime 仍在本机。
- 硬门：tunnel identity、origin 重新计算、断线清理，以及对 remote agent control 的明确 unsupported 状态。
- 该阶段不等于远端 Agent Browser，也不代表向远端 ACP 注入本地 Browser MCP。

### M6：独立更新、优化与 CEF 评估

- 交付独立 Runtime security component updater、容量/性能优化，并执行数据驱动的 CEF 独立评估门。
- 只有当实测启动、崩溃隔离、权限、可访问性、打包、升级与维护成本整体优于 Managed，且不削弱安全模型时，CEF 才进入后续计划；否则继续以 Managed 为正式 Runtime。

依赖关系：

```text
M0 -> M1
M1 -> M2 / M3 / M4
M1 + SSH supervisor work -> M5
M6 使用前序里程碑的生产近似证据
```

## 测试、性能与发布控制

### Golden path 与真机矩阵

M1 在 macOS arm64、macOS x64 和 Windows x64 上使用确定性本地 fixture server，覆盖：

- Preview 自动升级 Managed，正确显示 task group/Profile；
- 表单动作、截图与稳定 tab 状态；
- Host 收到原生输入后的 fencing、显式交还、高风险确认和秘密挑战接管；
- turn/thread grant 寿命、接管后下一 turn 不自动拿回控制、Provider 权限撤销与监听失效；
- renderer、Chromium、worker、Host 与 App 分别崩溃，旧 generation 失效且不自动 replay；
- Runtime/Profile 不可用时显示原因与 Preview/修复/诊断；
- upload、download、Artifact promotion、retention、redaction 与 export metadata；
- 同时运行用户自己的 Chrome，证明 teardown 不会误杀；
- popup、iframe、Shadow DOM、跨域、Console、Network 和 Trace 的声明支持子集。

package lifecycle matrix 必须使用实际 macOS arm64/x64 DMG 与 Windows x64 NSIS/portable，在最低支持 OS 和当前稳定 OS 上覆盖：

- clean、offline first run；
- N-1 -> N、active Runtime update 与兼容 rollback；
- 低磁盘、中断写入、坏 archive、坏 pointer；
- uninstall/reinstall；
- 含空格与非 ASCII 的安装/数据路径；
- Gatekeeper/Defender；
- sleep/wake；
- Profile 登录态在兼容重启/升级后可用，且不同 Profile 不串。

组合矩阵覆盖 App N-1/N x Runtime N-1/N、手工 App downgrade、Profile epoch 前后、active Session 固定 generation、pointer 切换前后 crash、LKG 缺失/撤销。任何 update 都不得原地修改 active directory；旧 generation 只有在 refCount 归零并 drain 后才能回收。

Profile lock 还要覆盖 dev/latest/stable 或两份 App 同时解析到同一 `browser/v1` 的场景。第二个同 Profile claimant 必须返回 `profile_busy` 或 `reconciliation_required`，不得杀第一个 Runtime，也不得根据 PID 猜锁。升级接管前必须证明旧 run nonce descendants=0。

15 个 locale 执行 catalog key parity 与非空检查；视觉压力集使用 `en`、`zh`、`de`、`ru`、`ta`，不构造不可执行的 15 locale x 平台 x 状态截图笛卡尔积。不支持的 fixture 行为必须准确显示 unsupported 并提供人工接管。

确定性 CI/package 硬门要求每个支持 package/OS 对声明 fixture 连续三轮 100% 通过。自动 replay、orphan descendant、未授权访问、Profile 串用、路由误判和安全越权必须为零。下面的百分比只用于 beta 运营信号，不能拿来容忍确定性测试不稳定。

### 初始工程上限与 beta 信号

每项指标必须记录参考硬件、fixture、网络边界、精确起止点、分子/分母、排除项与样本数。至少 30 个样本才报告 P50/P95/max；P99 至少需要 100 个样本。

| 指标 | 初始门槛 |
| --- | --- |
| 点击 Agent Browser 到稳定截图（冷启动） | p95 <= 8 秒 |
| 健康 Runtime 新建 tab | p95 <= 2 秒 |
| Host 收到权威原生输入到拒绝命令和 worker 确认 | p95 <= 250 ms |
| crash 到新 generation 或明确失败 | p95 <= 10 秒 |
| Runtime 失败到可见可用 Preview | p95 <= 2 秒 |
| beta 非故障注入动作成功率 | >= 98% |
| beta `unknown_outcome` | <= 0.5%；自动 replay = 0 |
| beta crash-free session | >= 99.5% |
| 测试结束 30 秒后本 run descendants | 0 |
| 路由正确率 | 100% |
| 安全边界越权 | 0 |

CPU、RSS、磁盘和包体 ceiling 先在 M0 按参考机建立基线再版本化，不能表述为所有机器统一 SLA。当前没有全量生产 Browser 遥测，只能使用本地诊断包或经用户明确同意的匿名计数。

### 发布环与 kill switch

发布顺序为：

```text
CI / fixture
  -> internal dogfood
  -> Managed opt-in beta
  -> platform cohort
  -> Managed default
```

每次升环要求连续两个 24–48 小时观察窗口通过，并且每个平台/Runtime build 至少积累 100 个 BrowserSession、1,000 个非故障注入动作、30 次冷启动、10 次 crash/cleanup drill 和 5 次 upgrade/rollback。分母不足就延长观察，不能只按时间自动升环。没有批准的遥测时，只计算明确记录的 dogfood 验收。

Chrome Connector、Design Mode v2、Linux 与 SSH 各自使用独立 flag 和发布环。一例安全越权、secret 泄漏、Profile 串用、数据丢失、orphan process 或接管后仍能执行命令，立即停止升环。

Rust Host 在创建 Managed binding/Runtime、签发/续租 ControlLease、接收每条 command 时检查签名、单调、会过期的 policy；worker 在每个 primitive 前验证 Host 下发的当前 policy epoch、lease 和 fence。Host 原子缓存最高 sequence 与 key/revocation metadata；低 sequence 和无效 key rotation 必须拒绝。fetch 失败时可以继续使用有效未过期 cache。

官方包携带仅允许该精确 Runtime tuple/capability 的签名 bootstrap policy，包含 `issuedAt`、`expiresAt`、sequence 与 key ID。无 cache 的离线首启可在它仍有效且系统时钟可信时使用，并记录最高 sequence；bootstrap 不得覆盖更新 cache、撤销记录或 `stop_all`。过期的离线安装包只提供 Preview，取得新鲜有效 policy 后才能启用 Managed。“离线首启”验收分别覆盖有效和过期 bootstrap，不承诺无限期离线自动化。

无有效 policy、cache 过期或系统时钟回拨异常时，只对 Managed 执行 `block_new`。它禁止新 binding/lease、续租和新 command admission；已派发命令仅在原 lease 未到期且未被撤销时完成有界 drain，不得借轮询、自动重试或复合命令追加动作无限续跑。只有新鲜且明确签名的 `stop_all` 才能因远程 kill switch 强制终止现有 Runtime；本地崩溃、安全故障或用户关闭仍可按 Supervisor 合同终止。

BrowserSession、Gateway 与 Preview 不受 Managed kill switch 影响。`block_new` 阻止新 Managed binding/lease，让既有工作 drain；仅用于安全事件的 `stop_all` 原子关闭 admission、递增 fence、撤销 lease/UploadGrant、取消工作，再 drain/kill 命中的 Runtime。与 `stop_all` 竞态的副作用报告 `unknown_outcome`，绝不重放。

UI 必须说明 Agent Browser 暂时不可用及原因，同时保留 Preview。生产版没有本地绕过开关。

### 打包与签名硬门

生产 Release 必须先保持 Draft/staging。payload 使用不可变、版本化或 content-addressed 文件名。平台、架构、版本、文件名和签名必须精确配对；缺包、verifier 定义的 blocking warning、上传失败或下载失败都会让 Release 保持 Draft 并使 job 失败。

所有 Draft asset 上传完成后，由独立 clean runner 重新下载并验证，再执行一次 publish。签名、版本化 index/pointer 只在 Release publish 后切换。客户端只有在 index 引用的全部 payload 可下载且 digest/signature 匹配时才接受新版本；404 或半发布继续使用当前版本。GitHub Release 多资产本身不被假设为原子事务。

任何包含 Managed capability 的官方生产包，都必须具有适用的 Apple/Authenticode、embedded Runtime manifest、package signature 和 Tauri updater signature；缺一项即发布失败。Host 根据 compile-time distribution attestation 判断 Managed 资格。UI setting 或 remote flag 都不能把 unsigned/community build 升级为 Managed，后者硬退 Preview/manual。

macOS 验证必须枚举最终 `.app`、Chromium Framework、helper app、Mach-O 和 dylib，逐一检查 Team ID、hardened runtime、designated requirement 与最小 entitlement。`codesign --verify --deep --strict` 只作为补充；最终包还要通过 notarization、staple validation 与 Gatekeeper `spctl`。

Windows 的 NSIS 和解包后 portable 必须枚举每个 PE、与 Runtime manifest 匹配，并通过 `signtool verify /pa /all`、timestamp 与 certificate chain 验证。Tauri updater `.sig` 不能代替 Apple 或 Authenticode。

Windows portable 必须包含 `Grok.exe`、完整 immutable Runtime tuple、manifest/license 和可搬动的相对 bootstrap layout。M1 portable 标记为 `manual-update-only`，绝不能静默变成 NSIS 安装。未来原地 portable updater 必须先通过独立真实包 E2E。

### Supervisor teardown 硬门

ProfileGuard 的 descendant ledger 包含 Host boot ID、run nonce、Profile identity、PID、start time 和 executable path。

```text
close admission
  -> revoke / fence
  -> drain
  -> bounded stop
  -> Job / PGID kill
  -> wait/reap 或基于身份确认退出
  -> 30 秒后 descendants = 0
  -> release Profile lock
```

测试分别杀死 App 与 Host，同时保持用户自己的 Chrome 运行，以证明 orphan cleanup 与 ledger scope。禁止按进程名执行 `pkill`/`taskkill`。

30 秒后仍有经过身份确认的 descendant 时，Runtime 进入 `cleanup_failed/quarantined`，不得报告 closed，ProfileGuard 继续持有 ledger 与 Profile lock。该 Profile 在 retry/reconciliation 证明旧 run 已消失前不能重启。UI 提供诊断、作用域明确的 retry 和“重启操作系统”路径。只有 run nonce、PID start time 与 executable identity 全部匹配的进程才允许处置。

## 迁移与仓库边界

### 迁移映射

| 现有表面 | 目标边界 |
| --- | --- |
| `side_browser_host.rs` 的 WebView 创建/下载回调 | Preview adapter + Transfer service；唯一 `downloadId`，不按 URL 索引。 |
| `BrowserTab.tsx` 的 loading/Design Mode 状态 | Workbench projection + browser hook；持久状态进入 browser domain/provider。 |
| `side_browser_eval` | 只用于受限 Design Mode inspector；Agent 操作走 Gateway capability。 |
| `media_server.rs` 的进程 token/path scope | 普通 App media 继续使用；Browser Artifact Store 使用 session-scoped token 和 opaque handle。 |
| `host_runtime` heartbeat | 继续负责 Host health；Browser Supervisor 证明 descendant 所有权与清理。 |
| 现有原子 JSON helper | 复用于 Profile、Session、Runtime descriptor、Transfer 与 Artifact checkpoint。 |

### 设置 IA 与用户配置

不新增浏览器一级设置菜单，沿用现有 Settings IA：

| 配置或管理面 | 现有位置 | 说明 |
| --- | --- | --- |
| Managed Runtime 状态、版本、修复、诊断与 distribution eligibility | 运行时 · 诊断（`runtime/tools`） | 只展示真实健康状态和可执行修复，不允许 UI 绕过签名与 policy。 |
| BrowserProfile、orphan、磁盘占用、Artifact/Transfer 保留与删除 | 运行时 · 隐私（`runtime/privacy`） | project/named Profile 删除必须应用内确认；展示 retention 与数据所有权。 |
| OriginPolicyRule、持久权限、Developer Mode 与 Full CDP | 通用 · 权限（`general/permissions`） | 扩大权限必须确认；高风险即时确认不可持久关闭。 |
| Chrome Connector 配对、claim 状态与撤销 | 运行时 · 连接（`runtime/connection`，M3） | 每个 tab/group 仍需在 Workbench 显式认领。 |
| 单任务后端选择、暂停、接管与交还 | Browser Workbench | 属于当前任务上下文，不做容易误伤所有项目的全局默认。 |

每个新增设置必须有稳定 `anchorId`，登记到 `SETTINGS_ENTRIES`，支持搜索与 deep link，并通过 `settingsCatalog.test.ts`。仅增加展示或导航时，不得顺带改变 `settings_get/set` 或扩展 API 语义。

新代码放入 `src/components/browser/`、领域 provider/hook/component/lib 和 `src-tauri/src/browser/`，按 gateway、adapter、runtime supervisor、profile、lease、artifact、transfer、persistence 拆分。

涉及原生 WebView 遮挡的菜单、modal 和 Workbench 布局继续复用现有 native-cover 路径；本设计不替代独立的 browser-modal-cover 修复。实现开始前重新核对相关修复是否已进入基线，避免回退已修复的叠层行为。

`src/App.tsx` 与 `src/app/AppWorkbench.tsx` 继续遵守 growth freeze：不得增加新 feature state 或大型功能块，且二者总行数只能下降。Workbench 只消费 provider projection。

所有 UI 文案使用 `createT(locale)`/`t()`。以 `en` 为 key authority，同时补齐其他 14 个 locale。任何新设置必须登记 `settingsCatalog`，支持 deep link 与 search，并使用 `intlLocale()`/`isTightScript()` 处理日期、数字和紧凑文字。

对话框与菜单只使用现有应用内 portal、`Select`、`ContextMenu`、`GlassModal`、`setAppDialog` 和实体 panel。禁止 `window.confirm`/`prompt`/`alert`、原生 `<select>`、透明 menu、点击穿透和错误 stacking。

Browser Artifact 遵循 `docs/llm-wiki/media-delivery.md`：稳态 UI 在适合时使用 loopback HTTP；嵌入 Browser 内容永远拿不到 App media token；MCP 不返回绝对路径。SSH 遵循 `docs/llm-wiki/ssh-remote.md`，M1 不得伪装已支持 remote Browser MCP。

## 风险与明确延后项

| 风险 | 缓解措施或边界 |
| --- | --- |
| Runtime 包体与更新成本 | M1 随 App 打包 immutable tuple，保留兼容且未撤销的 LKG；M0 测基线；独立 updater 只在 M6 后开放。 |
| Profile 损坏或并发写 | 持续 OS lock、Profile epoch、generation recovery；不静默 merge/wipe。 |
| 用户与 Agent 竞态 | Host + worker 双层 fencing、取消复合动作、暂停观察、显式交还。 |
| 网页兼容性 | Playwright 语义 + 声明 fixture；不支持时准确提示并交由人工。 |
| secret/文件泄漏 | capability token、最少审计、不可变 staging、MCP 不见绝对路径、遵循 OS 权限。 |
| 原生窗口集成 | M1 使用独立可见 Managed window；CEF 是 M6 独立门。 |
| 平台进程清理 | ProfileGuard、Job Object/PGID、descendant ledger、与用户 Chrome 并行测试。 |
| 外部产品实现未知 | Codex/Cursor 闭源部分只作为公开合同参考，不写成源码事实。 |
| 遥测覆盖低 | 仅本地诊断或用户同意的匿名计数，不宣称全量 fleet 达标。 |

本路线不承诺自动导入 Chrome Profile、兼容任意浏览器扩展、M5 前支持 SSH remote Agent Browser、M4 前提供 Linux 对等能力，也不承诺最终替换 WebView。

## 独立规格与计划

[实施总索引](../plans/2026-09-05-browser-rearchitecture/README.md)汇总六十项任务的开发顺序、负责角色与工程量；[共用合同](2026-09-05-browser-rearchitecture/00-contracts.md)统一接口；[需求与验收矩阵](../plans/2026-09-05-browser-rearchitecture/acceptance-matrix.md)映射 BR-01 至 BR-24。十个工作包可以分别审查：

| 工作包 | 独立规格 | 开发计划 |
| --- | --- | --- |
| M0 Preview 2.0 与任务分组 | [规格](2026-09-05-browser-rearchitecture/m0-preview.md) | [M0-W01–06](../plans/2026-09-05-browser-rearchitecture/m0-preview.md) |
| M0 Runtime 可行性 | [规格](2026-09-05-browser-rearchitecture/m0-runtime.md) | [M0-R01–06](../plans/2026-09-05-browser-rearchitecture/m0-runtime.md) |
| M1 Browser Gateway 与权限 | [规格](2026-09-05-browser-rearchitecture/m1-gateway.md) | [M1-G01–06](../plans/2026-09-05-browser-rearchitecture/m1-gateway.md) |
| M1 Managed Runtime、Profile 与 Artifact | [规格](2026-09-05-browser-rearchitecture/m1-runtime.md) | [M1-R01–06](../plans/2026-09-05-browser-rearchitecture/m1-runtime.md) |
| M1 双平台分发与发布 | [规格](2026-09-05-browser-rearchitecture/m1-delivery.md) | [M1-D01–06](../plans/2026-09-05-browser-rearchitecture/m1-delivery.md) |
| M2 检查、响应式与 named Profile | [规格](2026-09-05-browser-rearchitecture/m2-inspection.md) | [M2-01–06](../plans/2026-09-05-browser-rearchitecture/m2-inspection.md) |
| M3 Chrome Connector | [规格](2026-09-05-browser-rearchitecture/m3-chrome.md) | [M3-01–06](../plans/2026-09-05-browser-rearchitecture/m3-chrome.md) |
| M4 Linux | [规格](2026-09-05-browser-rearchitecture/m4-linux.md) | [M4-01–06](../plans/2026-09-05-browser-rearchitecture/m4-linux.md) |
| M5 SSH Browser Bridge | [规格](2026-09-05-browser-rearchitecture/m5-ssh.md) | [M5-01–06](../plans/2026-09-05-browser-rearchitecture/m5-ssh.md) |
| M6 独立 Runtime 更新与优化 | [规格](2026-09-05-browser-rearchitecture/m6-updates.md) | [M6-01–06](../plans/2026-09-05-browser-rearchitecture/m6-updates.md) |

每份规格与计划包含接口、迁移、测试、指标、rollout flag、回滚及验收证据要求。原生输入来源、动作取消、Chrome 逐 claim 下载和 Wayland 支持仍是必须取得证据的实验门；研究 No-go 不等于对应功能通过。整套设计文档交付不启动产品实现，开发按用户指定的工作包与已满足依赖推进。

## 本总体设计验收清单

- [x] Preview 作为长期支持后端，WebView 没有“最终必替换”的矛盾。
- [x] Managed M1 平台仅为 macOS arm64/x64 与 Windows x64。
- [x] Chrome Connector、Linux、SSH 和 CEF 均有明确的后续边界。
- [x] Codex、Cursor、Grok Build、Tabshare 和 Playwright 的证据边界已说明。
- [x] Workbench 路由、任务分组、手动覆盖与后端切换合同已定义。
- [x] Gateway 身份链、origin policy、权限、高风险审批与接管 fencing 已定义。
- [x] Session/Profile/Runtime/Lease/Tab/Artifact/Transfer 身份和正交状态机已定义。
- [x] 打包、签名、升级、回滚、Supervisor teardown 与 kill switch 均有可量化硬门。
- [x] 上传/下载隐私、保留、崩溃恢复与 unknown outcome 已定义。
- [x] 仓库、i18n、设置、对话框、媒体、SSH 与 AppWorkbench 约束已映射。
- [x] M0–M6 依赖和后续独立规格已明确。

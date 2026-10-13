# 05 · 路线 B：集成进 Grok App

> 目标：Grok App 用户在设置里打开 Computer Use 后，直接在聊天里说"帮我在 X 里做 Y"，首次使用某 App 时在聊天流里点一次授权，
> 之后像 Codex 一样流畅执行；全程可见、可停、可撤销。驱动与工具语义与路线 A **完全相同**——实际运行的就是**同一个 `gcu` 二进制**。

**集成方式：松耦合。** App 不链接 `gcu-core` 与任何驱动；App 随包携带 `gcu`，以子进程（引擎）方式托管，双方只通过版本化的
**Host 协议**通信（[03 §15](03-architecture.md#15-host-协议路线-b-松耦合集成)）。App 负责"人"的部分（审批、可视化、设置、生命周期），
引擎负责"机器"的部分（观察、动作、策略执行、MCP）。

必须遵守仓库规则（`AGENTS.md`）：所有文案走 `src/i18n/`（15 语言，`en` 为键权威）；不用 `window.confirm/alert/prompt`；不用原生 `<select>`；
菜单/弹层用已有组件与实底样式；新设置必须登记 `settingsCatalog`；**不得**给 `src/App.tsx` / `src/app/AppWorkbench.tsx` 增加状态或大块功能；
功能变更三件套（交互逻辑、视觉一致、功能对等）全部完成才算完成。

---

## 0. 集成带来什么（相对路线 A 单独安装）

松耦合不等于"只是帮用户装了个 MCP"。以下是只有集成才能做到、且必须交付的优势：

| 优势 | 路线 A 独立使用时 | 路线 B 集成后 |
|---|---|---|
| 安装 | 用户自己装 `gcu`、跑 `gcu install grok` | 零安装：设置里打开开关即可；随 App 自动更新 |
| 授权体验 | MCP elicitation 或弹出的小审批窗 | **聊天流内联审批卡**，和对话在一起；可看到是哪条消息触发的 |
| 可视化 | helper 绘制的光标与胶囊 | App 覆盖窗 + **侧栏时间线**（每步截图缩略图、目标、结果、耗时），可回看 |
| 轮次边界 | 推定（空闲 45s）或依赖客户端钩子 | **精确**：App 解析 ACP 流，轮次结束立即清理 |
| 会话感知 | 不知道会话类型 | 只给本地会话注入；SSH / 远程 IM / 定时任务会话**不**注入；fork/切模型自动重置 |
| 设置与撤销 | `gcu approvals` 命令行 | 设置页统一管理：始终允许列表、敏感确认级别、前台策略、系统权限状态 |
| macOS 权限 | 独立 helper `Grok Computer Use.app` | 归属 Grok App 本身，用户只授权一次 |
| 内置浏览窗 | 无 | App 自带隔离浏览窗（系统 WebView） |
| 崩溃处理 | 客户端看到 MCP 断开 | App 自动重启引擎、面板提示、熔断保护 |
| 结果呈现 | 文本 + 图片 | 聊天流专用工具卡（动作图标、徽标、缩略图可放大） |

---

## 1. 进程与数据流

```
Grok App (Tauri 主进程, Rust)
 ├─ ComputerUseHost（新，src-tauri/src/computer_use_v2/）—— 只依赖 gcu-protocol + gcu-host-protocol
 │    ├─ EngineSupervisor：启动/停止/重启 `gcu engine --host-socket <path>`（Tauri externalBin），健康检查、熔断
 │    ├─ HostServer：Host 协议服务端（UDS / 命名管道，仅当前用户可访问）
 │    │     ├─ approval.requestApp / confirmAction / handoff → 前端事件 → 等待用户决定 → 应答
 │    │     ├─ policy.load / grantAlways → App 设置存储
 │    │     ├─ overlay.* → App 覆盖窗
 │    │     └─ event → 前端面板时间线
 │    └─ 生命周期转发：ACP 轮次结束 → engine.session.turnEnded；会话关闭/fork/切模型 → engine.session.close
 │
 ├─ gcu 引擎（子进程，与路线 A 同一二进制）
 │    ├─ core + 平台驱动 + MCP（loopback streamable-HTTP，127.0.0.1:随机端口）
 │    └─ 每个会话的 MCP token 由 App 通过 engine.session.open 申请
 │
 ├─ 会话注入（session_manager）：对启用 CU 的本地会话，在 ACP newSession 的 mcpServers 中加入：
 │    方案 1（首选，若 Grok Build 支持 http MCP）：{ name:"computer-use", transport:"http", url:<mcp_url>, headers:{Authorization:"Bearer <token>"} }
 │    方案 2（回退）：{ name:"computer-use", command:"<App 资源目录>/gcu", args:["mcp","--connect",<mcp_url>], env:{GCU_TOKEN:"<token>"} }
 │    —— 两种方案都不需要 Node；token 不进 argv；不写入 ~/.grok。
 │
 └─ 前端（React）
      ├─ 聊天流内联卡片：App 授权卡、敏感动作确认卡、交接卡（handoff）
      ├─ Computer 侧栏面板：当前 App/窗口、步骤时间线（含缩略图）、停止
      └─ 设置 › 扩展 › Computer Use：开关、系统权限状态、始终允许的 App、敏感确认级别、前台策略、热键
```

**turn 边界：** App 已在解析 ACP 流（`session/update`、`stopReason`），在每个 prompt 结束时发送 `engine.session.turnEnded`。
**会话结束/删除/切换模型/fork：** 发送 `engine.session.close`，引擎清除 session 级授权与快照。

**macOS TCC：** 引擎由 App 启动，TCC 责任归父 App，系统设置里显示"Grok"（S-02 实测确认；不成立时按 [03 §5.1](03-architecture.md#51-macos) 的备选：
把 `gcu` 放进 App bundle 作为 helper，仍属 App 签名身份）。
**Windows 前台：** 前台动作期间 App 主窗口不得抢焦点（覆盖窗 `WS_EX_NOACTIVATE`）；若 App 自身是前台窗口且用户正在输入 → 守卫拒绝。

**耦合边界（`xtask check-deps` 强制）：** `src-tauri` 只能依赖 `gcu-protocol`、`gcu-host-protocol`。前端只通过 Tauri 命令/事件与 `ComputerUseHost` 交互，
不直接接触引擎。引擎替换为外部路径（开发者设置）时，App 代码零改动。

---

## 2. Rust 侧模块（新）

`src-tauri/src/computer_use_v2/`（v1 目录 `computer_use/` 不迁入 main；v2 名称在 P5 末尾改回 `computer_use/`）

| 文件 | 职责 | 行数上限 |
|---|---|---|
| `mod.rs` | `ComputerUseHost` 生命周期（feature 开关变化时启停）、Tauri state 注册 | 250 |
| `supervisor.rs` | 引擎进程：定位 externalBin（或开发者设置的外部路径）、启动、握手版本检查、心跳、空闲 60s 退出、崩溃重启、10 分钟 3 次熔断 | 300 |
| `host_server.rs` | Host 协议服务端：监听 UDS/命名管道（权限仅当前用户）、一次性连接 token、分发请求到下列模块 | 300 |
| `inject.rs` | `engine.session.open` 取 MCP 端点 → 构造 ACP mcpServers 条目；`should_inject(session)`：功能开启 + 本地会话 | 200 |
| `approval.rs` | 处理 `host.approval.*`：emit `cu://approval` → 等待 `cu_approval_decide` 命令 → 应答；超时；会话关闭自动拒绝挂起请求 | 300 |
| `overlay.rs` | 处理 `host.overlay.*`：覆盖窗创建/定位/隐藏；排除截图；光标动画参数下发给覆盖窗前端 | 300 |
| `events.rs` | 处理 `host.event`：节流（每会话 ≤10 事件/秒），缩略图转发；大图按需 `engine.screenshot.get` | 200 |
| `policy_store.rs` | 处理 `host.policy.*`：读写"始终允许"列表、敏感确认级别、前台策略 → App 设置存储（`app_settings`）；变更时发 `engine.policy.changed` | 200 |
| `commands.rs` | Tauri 命令：`cu_status`、`cu_doctor`（转发 `engine.doctor`）、`cu_approval_decide`、`cu_stop`、`cu_approvals_list`、`cu_approvals_revoke`、`cu_open_system_settings(kind)` | 300 |

依赖：**仅** `gcu-protocol`、`gcu-host-protocol`。`gcu` 可执行文件作为 Tauri `externalBin` 打包（各平台各架构）。

---

## 3. 前端模块（新）

全部放在 `src/components/computer-use/`、`src/hooks/`、`src/lib/computer-use/`、`src/lib/api/computerUse.ts`；**不碰** App.tsx / AppWorkbench.tsx 的状态。

| 组件 | 说明 |
|---|---|
| `ComputerUseApprovalCard.tsx` | 聊天流内联卡：App 图标+名称+理由；按钮 `仅本次` / `本会话` / `始终允许` / `拒绝`；倒计时（复用 `PermissionCountdown`）；与现有 ACP 权限卡同一视觉（复用其样式类） |
| `ComputerUseActionConfirmCard.tsx` | 敏感动作确认卡：动作摘要（"在「微信」中点击「发送」"）+ 目标区域裁剪图 + `允许` / `拒绝` |
| `ComputerUseHandoffCard.tsx` | 遇到登录/验证码/系统安全提示时：说明 + `我已处理，继续`（触发一条用户消息） |
| `ComputerUsePanel.tsx` | 侧栏面板（SideWorkbench 的 Computer tab，替换 v1 面板）：当前状态、时间线（工具、目标、结果、耗时、缩略图）、`停止` |
| `ComputerUseOverlay.tsx` | 覆盖窗前端（独立 Tauri 窗口 `cu-overlay` 的入口）：光标、状态胶囊、停止按钮 |
| `settings/ComputerUseSection.tsx` | 设置页：总开关、系统权限状态与"去授权"按钮、始终允许列表（可撤销）、敏感确认 `Select`（关/标准/严格）、前台策略 `Select`（从不/询问/总是）、停止热键展示 |
| `useComputerUseEvents.ts` | 订阅 `cu://*` 事件，维护每会话的时间线 store（`src/lib/computer-use/store.ts`，zustand 或项目现有 store 模式） |

**卡片嵌入点：** 聊天流已有"工具调用/权限请求"渲染路径；在 `src/components/lobe-chat/` 的工具调用渲染中，按 `toolName` 前缀/MCP server 名 `computer-use`
渲染专用卡（截图缩略、目标、结果 effect）。审批卡作为一种 pending 交互插入当前 assistant 消息尾部。

**设置登记：** `src/lib/settingsCatalog/entries/extensions.ts` 新增/替换 `ext.computer`（锚点 `settings-anchor-ext-computer`，关键词含 computer use / 电脑操作 / 桌面控制 / 15 语言）。
深链 `#/settings/extensions/computer`。

**i18n：** `src/i18n/messages/<locale>/computer-use.ts` 重写：删除与 v1 task/scope/pairing/runtime 相关键，新增本节键；`en` 先行，其余 14 语言同 PR 补齐（不留英文占位）。

---

## 4. 交互规格（完整路径）

### 4.1 首次开启

1. 设置 › 扩展 › Computer Use：开关默认关。打开时：
   - macOS 检查 TCC：缺权限 → 区块内显示两张权限卡（辅助功能/屏幕录制）+"去授权"（打开系统设置对应面板），授权后轮询刷新；屏幕录制授予后显示"需要重启 Grok"按钮（App 自重启）。
   - Linux Wayland：显示"首次操作时系统会请求屏幕共享/远程控制权限"说明。
   - Windows：无额外步骤。
2. 开关打开后，**新建或下一次发送**的本地会话注入 MCP；已存在且正在运行的会话在下一轮生效（ACP 会话需重建时，复用 v1 已有的"重建会话 MCP"机制）。
3. 关闭开关：立即 Stop 所有进行中的动作、拒绝挂起审批、下一轮不再注入。

### 4.2 一次任务

```
用户：帮我把微信里张三发的会议时间记到日历
模型：get_app_state{app:"微信", reason:"读取张三的消息"}
  → 聊天流出现授权卡「允许 Grok 使用 微信？」[仅本次][本会话][始终允许][拒绝]
  → 用户点「本会话」→ 工具返回状态+截图（面板时间线出现第一步，缩略图）
模型：click{app:"微信", element_index:4} → 返回 diff
模型：get_app_state{app:"日历"} → 授权卡（日历）→ 用户点「始终允许」
模型：computer_batch{app:"日历", steps:[...新建事件..., {expect:{text_present:"会议"}}]}
模型：（回复）已添加。
```

- 覆盖窗光标在目标窗口上移动；胶囊显示"Grok 正在使用 日历"；用户按停止 → 当前步骤 `stopped_by_user`，模型收到后应停止。
- 若某步触发敏感词（"发送"）→ 动作确认卡；拒绝则工具返回 `refused: user_declined`。

### 4.3 状态与错误呈现

| 情况 | 聊天流 | 面板 | 覆盖层 |
|---|---|---|---|
| 等待审批 | 授权卡（倒计时） | "等待授权：微信" | 隐藏 |
| 执行中 | 工具卡：动作 + 目标 + 进行中 | 时间线追加 | 光标移动 + 胶囊 |
| `unknown` | 工具卡标黄"结果未知，已返回最新界面" | 黄点 | — |
| `user_active` | 工具卡灰"你正在使用电脑，已暂停前台操作" | 灰点 | 胶囊提示 |
| `handoff_required` | 交接卡 | "需要你处理" | 胶囊"需要你处理" |
| 权限缺失 | 工具卡红 + "去授权"按钮（打开设置区块） | 红点 | — |

---

## 5. 内置浏览窗（L1，P5 可选交付）

- 新 Tauri 窗口 `cu-browser`（系统 WebView），带地址栏/后退/前进/刷新（复用现有 WebView 组件或新建，遵循既有视觉）。
- 它就是一个普通窗口：驱动通过 OS 无障碍树操作它（WKWebView/WebView2/WebKitGTK 均暴露网页无障碍树）。
- 额外工具 `browser_navigate{url}`（仅当该窗口打开时注册），拦截 javascript/file/data/元数据地址。
- 不与用户浏览器共享 cookie；数据目录在 App 数据目录下 `cu-browser-profile/`，设置页可"清除浏览数据"。
- Grok App 主窗口仍在拒绝名单中；`cu-browser` 窗口单独放行（以窗口 label 识别，不以进程识别）。

---

## 6. 从 v1 迁移

| v1 产物 | 处理 |
|---|---|
| `computer_use_enabled` 设置项 | 保留键名与含义（默认 false）；v2 读取同一键 |
| v1 面板/卡片组件 | 删除（不在 main 上存在，无需迁移）；v2 新组件替换 SideWorkbench Computer tab |
| v1 i18n 键 | 复用其中通用键（`computerUse.title` 等），其余删除 |
| v1 slash 命令 `computer-use` / `computer-use-browser` | `computer-use` 保留为"打开 Computer 面板"；`computer-use-browser` 改为"打开内置浏览窗"（L1 交付后），之前隐藏 |
| v1 用户数据（`%APPDATA%/grok-app/computer-use/`） | main 上不存在（v1 未发布），无需迁移；如本机测试残留，v2 首次启动不读取、不删除 |

`docs/llm-wiki/computer-use.md` 在 P5-09 重写为 ≤200 行产品规则页（入口、架构一图、授权规则、禁止事项、验证命令），并从 `AGENTS.md` 的 Read first 列表链接。

---

## 7. 路线 B 特有风险

| 风险 | 缓解 |
|---|---|
| Grok Build 不支持 http MCP | 方案 2（`gcu mcp --connect` sidecar）；S-01 spike 先确认 |
| ACP 会话 MCP 列表变更需要重建会话 | 复用 v1 已验证的"会话 MCP 重建"路径；只在下一轮生效并在 UI 提示 |
| 引擎崩溃/卡死 | 引擎是独立进程，App 不受影响；supervisor 重启，进行中调用返回 `unknown`；10 分钟 3 次崩溃熔断并提示 |
| App 与引擎版本不匹配 | 握手检查 Host 协议 major；App 只随包携带经测试的引擎版本；开发者外部引擎有显式标记 |
| 引擎子进程的 TCC 归属不是 App | S-02 实测；备选为 bundle 内 helper |
| 覆盖窗与主窗焦点争抢 | 覆盖窗不可激活、不可点击穿透以外区域（仅胶囊可点） |
| 审批卡阻塞期间用户切换会话 | 审批与会话绑定；切走不取消，回来仍在；会话关闭则拒绝 |
| App 体积 | 驱动是纯 Rust + 系统框架，预计 +3–6 MB；无 Chromium |

# 04 · 路线 A：独立 MCP（`grok-computer-use` / `gcu`）

> 目标：一个可以 `npm i -g`/`brew install`/`winget install` 后，接入 Grok CLI、Codex、Claude Code、Antigravity（`agy`）、Gemini CLI、opencode 等任意 MCP 客户端的
> 跨平台 Computer Use 服务，体验对齐 Codex，能力在三平台一致。

---

## 1. 产品定义

| 项 | 决定 |
|---|---|
| 名称 | 产品名 **Grok Computer Use**；CLI `gcu`（别名 `grok-computer-use`）；MCP server 名 `computer-use` |
| 许可 | MIT（与 Grok App 根仓库一致） |
| 形态 | 单一静态链接 Rust 可执行文件（每平台每架构一个）；macOS 另包成 `Grok Computer Use.app` |
| 平台 | macOS 14+（arm64/x64）、Windows 10 1903+（x64/arm64）、Linux glibc 2.31+（x64/arm64；X11 完整，Wayland 分合成器声明） |
| 依赖 | 零运行时依赖（无 Node/Python/.NET）；Linux 需要系统 AT-SPI2（GNOME/KDE 默认具备） |
| 分发 | GitHub Releases（主）、npm 包装器（仅下载对应二进制的 launcher，`optionalDependencies` 分平台包）、Homebrew tap、winget、`.deb/.rpm`、`cargo install`（源码） |

---

## 2. CLI 规格

```
gcu                          # 无参数：macOS 打开 onboarding（缺权限时）；其他平台打印 doctor 摘要
gcu mcp [--transport stdio|http] [--port N] [--profile default|minimal|full]
gcu mcp --connect <url> --token-fd <fd>     # 代理模式（路线 B 用，见 05）
gcu doctor [--json]          # 权限、能力、私有 API、版本、日志路径
gcu call <tool> [--args JSON] [--json]          # 单次调用，调试与脚本
gcu batch --file steps.json                     # 同 computer_batch
gcu script --file a.js | -                      # 同 computer_script（feature）
gcu approvals list|revoke <app>|revoke-all       # 管理"始终允许"
gcu install <client> [--scope user|project]     # grok | codex | claude | antigravity | gemini | opencode | cursor | vscode
gcu uninstall <client>
gcu turn-ended [payload]                        # 客户端钩子入口（Grok: Stop/end_turn；Codex: notify）
gcu resume                                      # 仅用户显式解除 Stop 锁存，不向模型暴露
gcu worker                                      # 内部：Windows 隔离执行子进程（隐藏）
gcu version
```

**退出码：** 0 成功；2 参数错误；3 权限缺失；4 工具返回 isError；10 内部错误。
| 协议/进程结果 | CLI 退出码 |
|---|---|
| 成功（无 isError） | 0 |
| 参数解析错误；ambiguous_app、invalid_key、out_of_bounds | 2 |
| permission_missing | 3 |
| 其余业务 isError（含 approval_denied、key_blocked、stale_*） | 4 |
| internal / panic / 未知异常 | 10 |

**`gcu call` 输出：** 默认人类可读（文本 + 截图保存到临时文件路径）；`--json` 输出 MCP `CallToolResult` 原样 JSON。

### 2.1 `gcu install <client>` 写入的配置

| 客户端 | 位置 | 内容 |
|---|---|---|
| Grok Build CLI | `~/.grok/config.toml`（共享模式）的 `[mcp_servers.computer-use]`；若用户使用独立 GROK_HOME，用 `--grok-home` 指定 | `command = "<abs path>/gcu"`, `args = ["mcp"]`；同时安装 Grok skill（若 Grok Build 支持插件/skill 目录）与 notify 钩子（若支持） |
| Codex | `~/.codex/config.toml` `[mcp_servers.computer-use]` | 同上；`notify` 若已存在则链式包装（保存原值，`gcu turn-ended --previous-notify`） |
| Claude Code | `~/.claude.json` `mcpServers.computer-use` | stdio |
| Antigravity CLI（`agy`） | 不直接改文件：调用 `agy mcp add computer-use <abs path>/gcu mcp`（官方子命令，幂等"add or update"）；卸载用 `agy mcp remove computer-use` | stdio；`agy` 未登录时仍可写配置，`doctor` 提示需登录后才能实测 |
| Gemini CLI | `./.gemini/settings.json` 或 `~/.gemini/settings.json` | stdio |
| opencode | `~/.config/opencode/opencode.json` | stdio |
| Cursor / VS Code | 各自 `mcp.json` | stdio |

TOML（Grok、Codex）用 toml_edit 0.25，只改自己的表项并保注释。JSON 用 serde_json + preserve_order 按键增删，测试断言键值不断言空白；若 JSONC 注释必须保留，P4-05 另选库并写 ADR、更新 deny.toml。

规则：先备份原文件为 `*.gcu-backup-<ts>`；只增删自己的条目；`--dry-run` 打印 diff；`uninstall` 精确移除。
**改写 `~/.grok/config.toml` 前必须征得用户确认**（交互式 y/N；非 TTY 需 `--yes`）——与 Grok App "共享模式拒绝改写 ~/.grok"的规则一致，独立 CLI 的写入是用户显式命令触发的。

---

## 3. macOS helper 与 TCC

**问题：** TCC 授权归属于"负责进程"。若由终端/IDE 拉起 `gcu mcp`，权限会落在 Terminal/iTerm/Cursor 上（mac-computer-use 的做法），
用户在不同客户端里要重复授权，且等于给了整个终端屏幕录制权。

**方案（参考 iFurySt/opensymph 的 app agent，clean-room 实现）：**

```
客户端 ──stdio──▶ gcu mcp（CLI 进程，无任何 TCC 敏感调用）
                     │ 1. 检查 helper 是否运行（Unix socket $TMPDIR/grok-computer-use/<uid>.sock）
                     │ 2. 未运行则 `open -g -j -b ai.grok.computer-use.helper --args agent`
                     │ 3. 将每个 MCP 请求转发到 helper（长度前缀 JSON 帧；同一 MCP 会话复用一条连接）
                     ▼
            Grok Computer Use.app（LSUIElement=1，后台 agent，唯一执行 AX/SCK/CGEvent 的进程）
              - 持有 TCC 身份（辅助功能、屏幕录制、可选输入监控）
              - 绘制覆盖层（光标/胶囊）与审批窗
              - 空闲 10 分钟且无连接时退出
```

- UID/文件权限只隔离其他用户，不证明调用者可信。socket 目录 0700、端点限当前用户之外，helper 必须通过连接的 audit token 取对端代码签名：可执行文件与 helper 同一 cdhash/同一签名身份（两项均核验），其他客户端不得连接；getpeereid/token 文件不能替代代码身份认证。新连接独立 MCP 会话，不继承其他连接的 turn/session 授权。
- 对端认证在握手前执行，端点抢占/替换拒绝；允许同二进制 CLI 调用的边界不包含同 UID 对受信进程内存的读写或修改受信二进制。路线 B 优先 App 创建并继承的专用匿名管道/socketpair，见 03 §15。
- helper 与 CLI 是**同一个二进制**（`gcu agent` 子命令），`.app` 只是包装；版本必须完全一致，不一致时 CLI 让旧 helper 退出并重启新版本。
- 开发构建命名 `Grok Computer Use (Dev).app`、bundle id `ai.grok.computer-use.helper.dev`，避免与正式版在系统设置里重名。
- **Onboarding：** 缺权限时 helper 显示一个小窗口（wry WebView + 共享 HTML/CSS，15 语言），两张卡片（辅助功能、屏幕录制），
  "去设置"按钮用 `x-apple.systempreferences:` URL 打开对应面板；轮询 `AXIsProcessTrusted()` / `CGPreflightScreenCaptureAccess()` 收敛；
  屏幕录制授予后提示"需要重启 helper"，按钮一键重启。**绝不自动点击系统对话框。**

Windows / Linux 无 TCC 问题，`gcu mcp` 进程内直接执行（Windows 可选 worker 子进程隔离）。

---

## 4. 审批（ApprovalProvider 实现）

优先级：

1. **MCP elicitation**（客户端在 initialize 中声明 `elicitation` 能力时）：
   在 `tools/call` 处理函数内调用 `peer.create_elicitation`（FormElicitationParams、OriginatingRequestId），message `允许 Grok Computer Use 使用「微信」吗？`；requestedSchema 为正规 JSON Schema：
   `{"type":"object","properties":{"decision":{"type":"string","enum":["turn","session","always","deny"]}},"required":["decision"]}`，
   附 `_meta`：`{"gcu/app": {...}, "gcu/risk": "low|high", "gcu/reason": "..."}`。客户端渲染能力差异大，单选是最保守的通用形态。
2. **helper 审批窗**（客户端不支持 elicitation 或返回错误时）：
   - macOS：helper 弹出小窗（同 onboarding 的 WebView 栈），置顶、显示 OS 核验签名状态/发布者/路径（图标与名称辅助）、模型理由纯文本且处理双向控制字符、四个按钮（本轮/本会话/始终允许/拒绝）、默认焦点拒绝且不捕获聊天 Enter、键盘可达；
   - Windows / Linux：`gcu mcp` 进程用 `tao + wry` 弹同样的 HTML 窗口（Windows 用系统 WebView2，Linux 用 GTK3 的 webkit2gtk-4.1——Linux 若缺 WebKitGTK 则回退到 `zenity/kdialog` 并在 doctor 提示）。
3. **非交互环境**（无显示/CI）：默认拒绝。`GCU_APPROVAL=auto-*` 仅存在于 cargo feature `eval-autoapprove` 构建（一次性评测环境），发布构建遇到该变量必须报错退出，不能通过环境开自动审批。

rmcp 3.5：`default-features=false, features=["server","macros","transport-io","transport-streamable-http-server","elicitation"]`。stdio 仅 NDJSON；若 S-01 证明需 Content-Length，自写 codec 并独立测试，不宣称 rmcp 自动识别。审批决定为 ApprovalDecision { Turn, Session, Always, Deny }，超时返回 approval_timeout；批准后重新验证绑定状态，变化重审。

**持久化：** `approvals.json`（0600），键为 §7.2 的 App 身份，值 `{decision:"always", granted_at, identity:{team_id|publisher|path_hash}}`。
`gcu approvals` 子命令管理；不提供"全部始终允许"。

敏感动作确认走同一 provider 的 `confirm_action`，UI 显示动作摘要与已脱敏目标区域裁剪图；拒绝为 approval_denied。网页区默认 strict，未知语义点击/Enter/Space/含换行输入/自定义快捷键按高风险，IDE 与嵌入执行环境限制见 03 §7.4。
系统动态库 WebKitGTK、GTK、libpipewire 的许可不在 cargo-deny 图中，deny.toml 注释边界、发行说明单列依赖。

---

## 5. 覆盖层（gcu-overlay）

| 元素 | 行为 |
|---|---|
| 软件光标 | 动作前从上次位置以弹簧曲线（response≈1.4s 可调到 0.35s 默认，避免拖慢）移动到目标，点击时脉冲；不移动真实指针；30s 无动作或 turn-ended 隐藏 |
| 状态胶囊 | 屏幕顶部居中："Grok 正在使用 微信 · 点击「发送」"＋"停止"按钮；前台模式时变色提示"正在使用键盘鼠标，移动鼠标可接管" |
| 前台模式边框 | Windows/Linux 前台输入期间目标窗口外框发光（参考 Windows-MCP），提示用户不要同时操作 |
| 不进截图 | macOS `sharingType=.none`；Windows `WDA_EXCLUDEFROMCAPTURE`；X11 采集前隐藏、采集后恢复 |

实现：macOS 在 helper 中用 AppKit 透明 `NSPanel`（objc2）；Windows 用分层窗口 + Direct2D；Linux X11 用 override-redirect ARGB 窗口；Wayland 无全局覆盖能力 → 仅胶囊用 layer-shell（wlroots/KDE）或不显示（GNOME，声明于 capabilities）。
Wayland layer-shell 的 crate 在 P4-04 选型后补 03 §2 与 deny.toml，GNOME 不显示并声明能力。
Stop/恢复走独立控制通道与 watchdog，停止锁存不因下一轮/重连/重启恢复；热键注册失败则禁用依赖该能力的路线。`GCU_OVERLAY=0` 可关闭视觉覆盖层，不能取消停止保障。

---

## 6. 签名、公证与分发

| 平台 | 要求 | 负责人 |
|---|---|---|
| macOS | Developer ID Application 签名 + hardened runtime + 公证（notarytool）+ staple；entitlements 仅 `com.apple.security.automation.apple-events`（P10 若用 AppleScript）；**不**申请 `get-task-allow` | 维护者提供证书（CI secret）；无证书时只发布 ad-hoc 构建并在 README 说明"每次更新需重新授权" |
| Windows | Authenticode（EV 或 OV）签名 exe；SmartScreen 信誉 | 维护者提供证书；暂缺时发布未签名版并说明 |
| Linux | 发布 SHA256SUMS + minisign/cosign 签名 | CI |
| npm | `gcu` 主包（launcher）+ `@grok-computer-use/darwin-arm64` 等平台包；launcher 只做"找到二进制并 exec"，不下载 | CI，`npm publish --provenance` |

**更新：** `gcu` 不自更新（包管理器负责）；`doctor` 提示新版本（仅当用户启用 `check_updates = true`，默认关闭，不联网）。

供应链：`cargo deny check`（许可 + 漏洞 + 来源）、`cargo auditable` 构建、GitHub Actions 全部 pin 到 commit SHA、SBOM（`cargo cyclonedx`）随 release 发布。

---

## 7. 与 Grok Build CLI 的配合

需要在 P0 用 spike 卡（[06 S-01](06-roadmap.md#s-01-grok-build-cli-mcp-能力探测spike)）确认下列事实，结论写入 ADR：

| 问题 | 若支持 | 若不支持 |
|---|---|---|
| MCP 结果中的 image content 是否真正传给模型（不是被转成文本） | 默认附图 | 将截图写入临时文件并在文本中给路径（模型用 Grok 的看图工具）——体验下降，需在 README 标注 |
| 是否支持 `elicitation/create` | 用 elicitation | 用 helper 审批窗 |
| 是否有 turn 结束钩子 / notify | `gcu install grok` 安装钩子 | 用空闲 45s 推定 |
| 是否支持 MCP server `instructions` | 用 | 安装 skill 文件代替 |
| `structuredContent` 是否透传给模型 | 精简文本 | 文本为主（默认即如此） |
| 工具结果大小上限 | 按上限调 `max_image_edge` 与 `text_limit` | — |

---

### S-01 结论（[ADR 0001](../../../computer-use/docs/adr/0001-grok-build-mcp-capabilities.md)，grok 1.0.52）

| 问题 | 结论 | v2 处置 |
|---|---|---|
| image 是否抵达模型 | 有条件：像素量约 ≥6 万才作为视觉输入，小图变文本 | 短边下限 256 + 放大小裁剪 + 文件路径后备（03 §4.4） |
| elicitation | 能力位有；headless 直接 `cancel` | 先试，失败/取消一律走 helper 审批窗；无界面会话不依赖它 |
| http MCP / ACP | 支持；`session/new` 的条目必须写 `type:"http"` 与 `headers:[{name,value}]`，写 `transport` 会被拒 | 05 §1 已按此；`server/discover` 失败可忽略 |
| instructions | 注入为 synthetic user 消息，模型可见 | 照常使用（≤1,200 字符） |
| turn 结束 | Hook `Stop`（`reason == end_turn`），没有 Codex 式 `notify` | `gcu install grok` 安装 `Stop` 钩子，仅处理 `end_turn`；钩子缺失时空闲 45s 推定 |
| 结果大小 | 文本约 20,000 字节前缀，超出写溢出文件；structuredContent 拼进文本末尾 | 渲染总量控制在 20 KB 内；长内容放文件 |
| 并发 | 同一条助手消息内的多个 `tools/call` 并发发出 | `gcu-mcp` 每会话内部串行执行，但必须能同时接收并排队 |
| annotations | headless 下不影响权限 | 策略完全自管，不依赖 `readOnlyHint` |

## 8. 路线 A 里程碑（与 06 路线图对齐）

| 里程碑 | 内容 | 对应阶段 |
|---|---|---|
| A-α | macOS 单平台：`gcu mcp` minimal 9 工具（含 list_apps/list_windows）；default 另含 capabilities，helper/onboarding，elicitation + helper 审批，覆盖层 | P0–P1、P4 部分 |
| A-β | + Windows、Linux X11；`gcu install` 全客户端；npm/brew/winget 分发 | P2–P4 |
| A-1.0 | + batch/wait_for/script、性能预算达标、评测基线通过、签名公证 | P6、P9 |
| A-1.x | + Wayland、DOM 增强扩展、OCR（可选） | P7、P8、P10 |

---

## 9. 独立仓库拆分时机

在仓库内 `computer-use/` 开发，满足以下全部条件后用 `git subtree split --prefix computer-use` 拆为独立仓库（例如 `grok-computer-use`），
Grok App 改为依赖其 git tag（Cargo `git = ..., tag = ...`）：
1. A-1.0 已发布；2. 路线 B 已上线 ≥2 个 App 版本；3. 公共 API（`gcu-protocol`、`gcu-host-protocol`）一个月内无破坏性变更。
拆分前任何对 `computer-use/` 的修改都走 Grok App 仓库的 PR 流程与 CI。

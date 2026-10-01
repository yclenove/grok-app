# Computer Use 调研：Grok App 三平台集成

日期：2026-09-08。状态：调研完成，技术选型待原型验证；尚未实现、安装驱动或运行 GUI 实测。

用户要求：类似 Codex 的完整 Computer Use 体验，Windows、macOS、Linux 从首版一起支持。产品方案和分批计划见 [执行方案](../plans/2026-09-08-computer-use.md)。

2026-09-08 补充：[Codex / Pi / DeepSeek Harness / Claude Code 框架对照](2026-09-08-computer-use-harness-comparison.md)。上一版已参考 Codex；Pi、DSH、Claude Code 是用户追问后补查，已将工具运行时、取消、权限与恢复结论写回执行方案。

同日新增 [DSH 第三方插件调研](2026-09-08-dsh-third-party-plugins.md)：定向核对 12 个项目的源码、版本和许可。BrowserSkill 加入已有 tab 连接的 P0 候选，与 Playwright 扩展比较；桌面 Cua 和受管浏览器 Playwright 主线保持。插件的三平台声明、后台操作和恢复能力仍须实测，不能用社区 README 替代验收。

## 1. 结论

这条路线可行，但不是给模型加一个 `computer_use` 参数就能完成。需要同时交付模型工具循环、本地执行器、浏览器控制、目标授权、暂停接管、状态记录和三平台安装诊断。

推荐 **Grok Build + App 自有 Computer Use Broker + Cua Driver 桌面适配器 + Playwright 浏览器适配器**。保留 OS 原生适配器接口作为替换路径；不从零重写全部平台驱动，也不把上游 MCP 原封不动暴露给所有会话。Cua Driver 是优先验证对象，是否进入发布依赖由 P0 决定。

三个关键判断：

1. Grok Build 源码已有 MCP 图片结果转换和向模型补入图片的路径，优先沿用现有登录与 ACP，不要求用户先申请另一家的 API key。但本机发布版 CLI、具体模型和中转链仍须端到端验证。
2. Cua Driver 有三平台 Rust 实现和可核对的行为测试记录，能降低工作量；它的后台操作能力有明确缺口，尤其 Linux 不同 Wayland 合成器不能等同。
3. 浏览器优先 DOM/可访问性操作，桌面优先 UIA/AX/AT-SPI，截图坐标补足视觉场景。所有输入操作都必须经过同一个目标、权限和取消边界。

## 2. 调研基线与证据强度

| 对象 | 本次核对范围 | 能证明什么 |
| --- | --- | --- |
| Grok App | `upstream/main`：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9` | 当前架构、接入点和规则 |
| 原壁纸集成分支 | `1916898630459bf5af7d10ee04e494d2face8ac5`，保留原工作区 | 是另一路开发，不能当成本设计分支已有代码 |
| Grok Build 本地源码 | `9684fa3cdbf2995e30ea8b9b637f1db008f144fc` | 曾下载的源码结构 |
| Grok Build 远端 | `72a61251fcffb464bcc687aeb5a998e5a98ec0c9`，定向读取 MCP、tool result 和 hub 源码 | 远端仍有图片转换路径；不代表本机二进制相同 |
| 官方文档和开源 README | 2026-09-07 至 09-08 在线读取；核心开源资料固定 SHA | 已公开的协议、能力和上游声明 |
| 运行验证 | 本次未进行 | 尚不能报告任务成功率、延迟、后台不抢焦点或安装兼容性 |

原工作区未提交的壁纸修改保持原样。此分支从上述上游基线独立建立，不依赖壁纸 PR 合并。

## 3. Codex 与公开 API 的边界

[官方 Computer Use 产品说明][S1]描述了应用选择、截图、菜单和键鼠操作、应用授权、暂停接管等体验。Windows 使用当前活动桌面，会占用前台；需要用户同时工作时，可用隔离虚拟机。macOS 需要 Screen Recording 和 Accessibility 权限。

[公开 Computer Use API 指南][S2]支持自有工具/代码执行接口，也支持结构化 `computer` 动作。**运行环境和动作执行由接入应用提供**；Responses API 不会直接获得用户电脑的控制权。

本机已读 Computer Use 插件接口作为体验参考：窗口对象、窗口截图和可访问性树、元素索引/坐标操作。未找到足以确认 `@oai/sky` 可独立再分发的授权依据，因此不将 Codex 捆绑运行时作为产品依赖，不复制它的私有协议。公开指南足够支持独立设计。[S1][S2]

Codex 的锁屏使用、远程运行和产品账户权限不自动转移给 Grok App。首版不承诺锁屏/UAC 桌面操作或任意应用后台输入。

## 4. Grok 现有能力能否接上

### 4.1 Grok Build 是首选模型入口

官方 [MCP 文档][S3]支持本地 stdio 和远程 HTTP 服务器。App 已通过 ACP `mcpServers` 注入会话工具，没必要为了 Computer Use 重写聊天主循环。

远端源码核对结果：

- [`xai-grok-mcp/src/servers.rs`][S4]：处理 `ContentBlock::Image`，`format_mcp_image` 将图片转为 data URI。
- [`acp_session_impl/tool_calls.rs`][S5]：提取工具结果中的 base64 图片、归一化，并补入带图的模型上下文。
- `expose_image_base64` 是额外暴露原始数据的选项，默认关闭。我们的观察结果应使用标准 MCP image block，不能把 base64 当大段文字塞给模型。
- 同时返回简短 text JSON 与 image；关键 `snapshotId`、元素引用不能只放 `structuredContent`。不同 CLI 版本处理结构化结果的路径不完全相同。
- [`image_normalize.rs`][S26]还会限制图片字节数、像素数和最长边并重新缩放。当前源码普通路径上限为 1,500,000 bytes、2,408,448 pixels、2000 px，另有 1024 px 严格路径；这是该源码版本的实现细节，不能当稳定公开 API。P0 必须验证模型实际看到的尺寸和坐标映射，不能只测“图片传过去了”。

这比“应该支持图片”更有依据，但还不是发布版实测。P0 用只含随机视觉标记的合成截图证明模型确实看到图片，不能靠文件名、替代文字或 AX 文本猜中。

### 4.2 Computer Hub 不是现成桌面执行器

Grok Build 的 [`xai-computer-hub-core`][S6]是 transport、registry、resolver；SDK 是工具服务器/调用方的连接、会话和生命周期设施。名字里的 Computer 不等于已经具备用户电脑上的 UIA、AX、截图和输入实现。

[Grok Bot 官方说明][S7]介绍的是持久云电脑，和本机电脑分开。不能据此推断 Grok App 的 Build OAuth 已获得 Grok Bot 云电脑服务或本地系统权限。本方案不接未公开的云电脑私有接口。

### 4.3 直连 API 可做可选模型适配器

xAI 公开提供 [图像理解][S8]、[函数调用][S9]和 [Remote MCP][S10]。这些能力足以作为自建循环的组成部分，不代表有一个与 Codex 完全同构的内建桌面工具。

Remote MCP 由 xAI 服务器访问 `server_url`，不能直接连接用户电脑上的 `127.0.0.1`。本机桌面不应为了这个功能暴露公网 MCP；可选直连模型应使用 **客户端函数调用循环**，本地执行、返回观察。

API key 路线按对应 API 账户计费；不将 SuperGrok/ChatGPT 订阅视为公开 API 额度，也不把 Build OAuth 搬到任意 API endpoint。首版优先现有 Build 官方登录路径，额度及可用模型遵循原服务规则。

## 5. 开源候选比较

以下是本次读取的仓库与文档，不是本地测评结果；活动情况只表示仓库仍在维护，不代表发布稳定。

| 方案 | 许可证 / 核对 SHA | 适用部分 | 主要边界 | 决策 |
| --- | --- | --- | --- | --- |
| [Cua Driver][S11] | MIT；`c5a15f3df3b29ffbe774de9f33d632fe75afec75` | Rust 三平台桌面、MCP/SDK、观察/输入/诊断 | macOS 嵌入身份、平台动作差异、Linux helper 和部分未验证路线 | 首选桌面原型，固定版本，经 App Broker 适配 |
| [Playwright MCP][S14] | Apache-2.0；`8a13ef8e9f7385a0f89477922127f31cbfde9761` | 浏览器 AX/DOM、截图、标签页、表单、已有 Chrome/Edge 扩展连接 | 不是权限沙箱；完整工具面含任意脚本/存储能力；默认 profile 并发冲突 | 复用 Playwright 与受控工具实现，不整体放开 MCP |
| [UI-TARS Desktop][S15] | Apache-2.0；`c2ad42e3eb9b27830db41a3e6f51ca7179d9b168` | GUI 操作器、视觉模型、任务轨迹和产品体验参考 | 完整独立应用/Agent 栈；README 本地平台表述为 Windows/MacOS/Browser，不能据此承诺 Linux 本地完整支持 | 借鉴交互与评测，不嵌入第二套桌面 App |
| [Windows-MCP][S16] | MIT；`08ddee78c26182b103d62c1c84c1fbec82a280b2` | Windows UIA/截图/输入快速验证 | Windows 专用，Python 3.13+；文档有英语系统与 App-Tool 限制 | Windows 对照原型，不作三平台主依赖 |
| [Enigo][S17] + [XCap][S18] | MIT + Apache-2.0；分别 `a88d9b7e2cec7043ab5f03e754500a091ea928d1` / `5c205f20cde2d5bdfcbc058db073cfd274a8eeda` | Rust 输入和截图积木 | 不提供完整 AX/目标授权/任务控制；Wayland 实验或有特殊场景限制 | 原生适配器备选，不宣称两库组合就是完整产品 |
| 直接实现 OS SDK | 各 SDK 与绑定按其许可证 | 最大可控性；修补具体平台缺口 | 生命周期、输入法、多屏、权限、测试和发行成本最高 | 保留统一 trait，按实测缺口逐项实现 |

Cua Driver 当前仓库含 `platform-windows`、`platform-macos`、`platform-linux`、独立 CLI/MCP 与 SDK；[根 LICENSE.md][S12]已核实 MIT。MIT 源码可用不代表可以直接复制第三方发布包的签名/商标/额外资产。依赖锁定、NOTICE、构建来源和目标包仍需 P0/P9 审核。

仓库旧 [ego-lite 调研](ego-lite-browser-automation-plugin.md)仍可作为可选插件的历史方案。它当时区分了 MIT harness 和闭源浏览器，且 macOS 优先；本次未重新验证其最新平台情况，不作为三平台首版的默认依赖。

## 6. Cua Driver 真正能省什么、还缺什么

其 [action-support 行为记录][S13]区分 Delivered、Refused、Gap，比只有“支持三平台”的 README 更有参考价值：

- Windows、macOS、X11、Sway 已有针对具体 GUI fixture 的成功/拒绝记录；其中“全部通过”包含按预期拒绝，不能转写为所有动作均支持。
- Windows Electron 的后台键盘、滚动等存在明确拒绝；后台左键可通过 UIA 等语义路径完成，不能推出任意游戏/Canvas 都能后台操控。
- GNOME/Mutter 的 GTK3 行为有记录，但需要 WinRects helper 和 portal 授权，共享 Electron/Tauri 与部分视频验证仍未闭环。
- KDE/KWin 有目标身份适配，原始键鼠输入因无法把最终输入绑定到目标窗口而禁用；不能默认退回全局输入然后宣称仍然精确绑定。
- 上游 Linux 文档之间粒度不同，以具体行为记录和固定版本代码为准，不用总览覆盖未验项。

因此建议复用已成熟部分，把 **GNOME Wayland 完整验证、KDE 能力展示、中文输入、多屏和更新后权限保持**列成明确开发工作。首版三平台支持不能用“Linux 能启动程序”冒充完成。

## 7. OS 层约束

| 平台 | 观察 | 输入 | 设计影响 |
| --- | --- | --- | --- |
| Windows | UI Automation + Windows.Graphics.Capture | UIA patterns；必要时前台 SendInput | WGC 先检测支持；SendInput 受 UIPI 完整性级别限制；普通进程不能承诺控制提权窗口。[S19][S20][S21] |
| macOS | Accessibility + ScreenCaptureKit | AX actions；必要时 Quartz 输入 | Screen Recording / Accessibility 归属于负责的 App 身份；嵌入 helper 必须由 Grok App 正确托管并签名。[S22][S23] |
| Linux X11 | AT-SPI + X11 capture | 语义动作 + 前台 XTest | X11 自身不提供按应用权限隔离；需要 App 目标验证和桌面输入互斥。[S13] |
| Linux Wayland | AT-SPI + ScreenCast/PipeWire；目标身份可能需合成器 helper | RemoteDesktop portal / libei；按后端能力 | portal 由用户选择共享来源；capture 和 input 都要探测，不可拿 X11 全局坐标直接复用。[S24][S25] |

Wayland 规范允许通过 portal 创建会话、选择设备/来源并建立 PipeWire、EIS 输入通道。**有 portal 接口不等于有可靠的窗口身份绑定或后台操作能力**。GNOME、KDE、Sway 的差异须保留在 capability report 中。

## 8. Grok App 接入点与现有缺口

| 当前代码 | 可复用 | 要补的部分 |
| --- | --- | --- |
| `src-tauri/src/extensions.rs` | `build_session_mcp_servers_for_connect`、本地 MCP 注入 | 当前 custom + official-aux 默认省略其他 MCP；Computer Use 必须作为显式启用的 App 会话能力独立合并，不能依赖“加载全部扩展”开关 |
| `src-tauri/src/acp_client.rs` | `session/new/load`、工具轨迹、权限 RPC、取消 | 会话工具注入预算超时会返回空列表；Computer Use 需显式显示不可用，不能让模型假装有工具 |
| `src-tauri/src/session_manager/` | 会话状态、stop、重连、后台任务 | 当前切聊天不会停止原任务；桌面输入租约必须全局可见，接管取消不能只通知模型 |
| `src-tauri/src/permission.rs` | 权限 UI 和请求基础设施 | 现有 scope 以路径/命令为主，不能代表 app/window/tab；`is_edit_tool` 包含字符串匹配，桌面动作不能误套文件自动批准 |
| `src-tauri/src/side_browser_host.rs` | 已有 navigate、eval、title/url/body text snapshot；命令已避免在 UI 线程阻塞 | 它已有基础自动化，但没有完整、版本化的元素引用和 Computer Use 权限面；也不等于三平台完整 Playwright |
| `src/lib/pluginRecommended.ts`、插件适配流程 | 安装入口与诊断模式 | 驱动下载/更新、版本能力、失败恢复、三平台打包 |
| `src/lib/settingsCatalog.ts`、资源侧栏、会话工具 UI | 设置搜索、入口和结果展示 | Computer 面板、目标选择器、持续的暂停/接管入口和本地轨迹 |

现有 Tauri 浏览器分别使用系统 WebView。不能把全部内嵌浏览器当 Chromium/CDP；首版外部受管 Chromium 为完整网页自动化路径，内嵌 WebView 只提供逐项验证的 typed DOM 能力，遇到缺口明确提示在受管浏览器继续。

## 9. 尚需实证的事项

1. 发布版 Grok Build 经 App ACP 调用 MCP 的图片是否完整到达官方模型；custom 图像模型是否也通，纯文本模型能否安全限制为语义操作。
2. Cua 固定版本在四个现有发行 target 上的可构建性、依赖体积和签名授权归属。
3. Cua 的取消、目标复用防护和 bounded 授权与 App policy 组合后，是否仍有绕行入口。
4. GNOME Wayland 的 helper 安装体验、中文输入和共享 WebView fixture；KDE raw input 仍是缺口，不计入完整支持。
5. Playwright 扩展连接的目标 tab 限制、断线、OAuth 弹窗和 profile 互斥。
6. 模型质量/速度：没有本项目测量数据前，不宣称与 Codex 同等成功率或提高几倍。

## 来源

以下来源均于本次调研在线打开或下载后读取。上游测试数字仅归属上游；本报告没有把它们算作 Grok App 实测。

[S1]: https://learn.chatgpt.com/docs/computer-use
[S2]: https://developers.openai.com/api/docs/guides/tools-computer-use
[S3]: https://docs.x.ai/build/features/mcp-servers
[S4]: https://github.com/xai-org/grok-build/blob/72a61251fcffb464bcc687aeb5a998e5a98ec0c9/crates/codegen/xai-grok-mcp/src/servers.rs#L1687
[S5]: https://github.com/xai-org/grok-build/blob/72a61251fcffb464bcc687aeb5a998e5a98ec0c9/crates/codegen/xai-grok-shell/src/session/acp_session_impl/tool_calls.rs#L2925
[S6]: https://github.com/xai-org/grok-build/blob/72a61251fcffb464bcc687aeb5a998e5a98ec0c9/crates/common/xai-computer-hub-core/src/lib.rs
[S7]: https://docs.x.ai/grok-bot/computer-and-apps
[S8]: https://docs.x.ai/developers/model-capabilities/images/understanding
[S9]: https://docs.x.ai/developers/tools/function-calling
[S10]: https://docs.x.ai/developers/tools/remote-mcp
[S11]: https://github.com/trycua/cua/blob/c5a15f3df3b29ffbe774de9f33d632fe75afec75/libs/cua-driver/README.md
[S12]: https://github.com/trycua/cua/blob/c5a15f3df3b29ffbe774de9f33d632fe75afec75/LICENSE.md
[S13]: https://github.com/trycua/cua/blob/c5a15f3df3b29ffbe774de9f33d632fe75afec75/libs/cua-driver/docs/action-support.md
[S14]: https://github.com/microsoft/playwright-mcp/blob/8a13ef8e9f7385a0f89477922127f31cbfde9761/README.md
[S15]: https://github.com/bytedance/UI-TARS-desktop/blob/c2ad42e3eb9b27830db41a3e6f51ca7179d9b168/README.md
[S16]: https://github.com/CursorTouch/Windows-MCP/blob/08ddee78c26182b103d62c1c84c1fbec82a280b2/README.md
[S17]: https://github.com/enigo-rs/enigo/blob/a88d9b7e2cec7043ab5f03e754500a091ea928d1/README.md
[S18]: https://github.com/nashaofu/xcap/blob/5c205f20cde2d5bdfcbc058db073cfd274a8eeda/README.md
[S19]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput
[S20]: https://learn.microsoft.com/en-us/windows/uwp/audio-video-camera/screen-capture
[S21]: https://learn.microsoft.com/en-us/windows/win32/winauto/entry-uiauto-win32
[S22]: https://developer.apple.com/documentation/screencapturekit
[S23]: https://github.com/trycua/cua/blob/c5a15f3df3b29ffbe774de9f33d632fe75afec75/libs/cua-driver/rust/Skills/cua-driver/EMBEDDING.md
[S24]: https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html
[S25]: https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html
[S26]: https://github.com/xai-org/grok-build/blob/72a61251fcffb464bcc687aeb5a998e5a98ec0c9/crates/codegen/xai-grok-shell/src/session/image_normalize.rs

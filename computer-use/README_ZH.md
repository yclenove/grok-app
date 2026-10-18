# Grok Computer Use

[English](README.md) · [中文](README_ZH.md)

`gcu` 是 Grok Computer Use 的命令行 MCP 服务。MCP 服务名是 `grok-computer-use`。许可是 MIT。每个平台的发行物是一个原生二进制，不附带浏览器、Node、Python 或 .NET。npm 包只用本机已有的 Node 18 或更新版本去启动旁边已经装好的二进制，不会再下载一份。

本文与这棵树里的 `gcu` 命令和发行打包脚本一致。macOS、Windows、Linux 的干净虚拟机安装没有跑过（见 [STATUS 的 P4-07](../docs/plans/computer-use-v2/STATUS.md)）。**未验证**表示该行为没有在对应机器上演示过。GitHub 压缩包以及 `.deb` / `.rpm` 里附带的是 [packages/README.md](packages/README.md)，不是本文件。

## 这个构建实际能跑什么

> **升级提示：**省略 `--driver` 现在表示 `auto`。既有客户端登记的 `gcu mcp`
> 在升级到包含对应 native 后端的构建后，可能首次接触真实桌面。
> 重连前请核对平台、系统权限和应用授权。安装器既不加 `fake`，也不加自动审批。
> Grok App 的 Computer Use 设置仍默认关闭。

`--driver auto|native|fake` 通过 [`assemble.rs`](crates/gcu-cli/src/assemble.rs)
选型。默认 `auto` 按 OS、入口、已编入 feature 和显示会话选择；`native`
要求同一条真实路线并遵守相同检查。两者绝不回退 fake、不提权、不借终端权限、
不静默扩大输入路线。Linux 的非空 `WAYLAND_DISPLAY` 优先，即使同时有 `DISPLAY`
也选 Wayland；仅有 `DISPLAY` 时选 X11；两者都没有则无安全 native 路线。

| 命令 / 平台 | 实际装配与边界 |
|---|---|
| `gcu version`、安装/卸载、approvals、resume、turn-ended、`mcp --connect` | 不创建桌面驱动；最后一项代理已存在的认证端点。 |
| macOS 路线 A：auto/native 的 `gcu mcp` stdio | 代理 Launch Services 启动的 `Grok Computer Use.app`。AX/SCK/输入共用 LocalHost Stop 锁存与活动源。仅 helper 的 TCC、端到端动作及恢复仍为 **not_run**。 |
| macOS：无子命令 / `gcu agent` | 无子命令启动 helper/onboarding；`agent` 是 helper 入口，不能从 shell 借用其 TCC。 |
| macOS：auto/native 的 `doctor`、`call`、自建 HTTP MCP | 退出 2：`This entry does not assemble a native driver; use the helper or the App`。不代理 helper；仍可显式用 fake。 |
| Windows：`mcp`、`call`、`doctor`、无子命令 | `native-windows` 装配带 Host Stop/活动源的 monitored Worker 驱动；无子命令运行 doctor。未编入该 feature 时 auto/native 退出 2，只能显式离线 fake。 |
| Linux X11：同上入口 | `native-x11` 装配带 Host Stop/活动源的 AT-SPI + X11；无子命令运行 doctor。未编入该 feature 时 auto/native 退出 2，只能显式离线 fake。 |
| Linux Wayland：同上入口 | `native-wayland` 装配探测与 Host 锁存/活动适配。驱动已有经证明的单窗口采集/输入挂接，但 CLI 不发起 portal 同意，也不挂接这些会话；不声称原生 Wayland 截图/输入可用。未编入时只能显式离线 fake。 |
| 路线 B：`gcu engine` | 由 App 托管的已认证 Host/Stop 通道。Windows/Linux 需对应 feature；macOS 只准入 macOS 27 arm64 的 App 子进程链，其余版本/Intel 拒绝。完整产品 TCC 验证为 **not_run**。不是客户端安装命令。 |

三个 CLI feature `native-windows`、`native-x11`、`native-wayland` **默认关闭**。
当前[发行工作流](../.github/workflows/release-computer-use.yml)未开启它们：
Windows/Linux 产物的**桌面执行仍为 fake-only**，auto/native 拒绝而不是运行 fake。
Linux 独立的 `gcu-driver-linux/pipewire` feature 不会开启 native 装配。
A0/A0-M/A1/A2/A3 平台装配卡已合并，不代表满足[发行门槛](../docs/plans/computer-use-v2/07-quality-gates.md#8-发布门槛)。

不支持的入口或缺少 feature 时退出 2 并说明原因。装配后，`gcu doctor --json`
报告 `driver`、`route`、`native_disabled`、`ready`、`issues` 及驱动能力；
`ready=false` 退出 3。`native_disabled` 字段也会承载 `selected native-windows`
这样的选择说明。实际限制以 `issues` 和能力为准：通用的 `permissions.status` /
`private_apis.status` 仍为 `not_applicable`，不能证明权限已授予。
MCP initialize instructions 包含路线说明，`capabilities` 描述所选驱动能力。
Windows 装配失败可能先于 doctor 报告退出；错误不等于成功诊断。
macOS native 使用 helper/App 诊断；`doctor --driver fake` 只诊断离线驱动。
运行时缺权限/能力可返回 `permission_missing`、`capability_unavailable`、
`elevation_required` 或 `handoff_required`，都不触发 fake 回退。

`--scene` 只对 `--driver fake` 有效，auto/native 带场景退出 2。
省略场景时 fake 用 `form`。fake 只运行内存桌面，不发送 OS 输入；
发行二进制还须 `GCU_ALLOW_FAKE=1`，该变量不授予真实权限或审批。
场景：`form`、`chat`、`list`、`modal`、`webarea`、`canvas`、`ide`、`login`、
`terminal`、`web_terminal`、`submit_unknown`。
fake 夹具身份带测试映像摘要，不授权真实应用。两条路线共用 CLI `approvals.json`，
并非分开的授权文件；fake 记录不迁移为真实身份。快照/图片属于当前连接。
切换驱动或清理 fake 数据不清除真实 Stop。没有 `GCU_DRIVER` 覆盖开关。

没有 `gcu batch`、`gcu script`，也没有 `gcu approvals revoke-all`。

## 安装

发行工作流里的构建目标（不代表桌面验收矩阵）：macOS 14 及以上的 arm64 与 x64，Windows 10 1903 及以上的 x64，Linux glibc 的 x64 与 arm64。Linux 二进制在 Ubuntu 22.04 上构建。musl 和 Alpine 不是目标。Windows arm64 不是发行目标。

按机器选择压缩包：

| 机器 | 压缩包 |
|---|---|
| macOS Apple 芯片 | `gcu-<version>-aarch64-apple-darwin.tar.gz` |
| macOS Intel | `gcu-<version>-x86_64-apple-darwin.tar.gz` |
| Windows x64 | `gcu-<version>-x86_64-pc-windows-msvc.zip` |
| Linux x64 | `gcu-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| Linux arm64 | `gcu-<version>-aarch64-unknown-linux-gnu.tar.gz` |

标签形式是 `cu-v<version>`。手动触发工作流只产出构建物，不发布 Release。Homebrew 和 winget 清单是给人提交的。这个工作流不会替你开那些拉取请求。

### 核对下载

校验和只能发现损坏，不能证明发布者是谁。标签发布还可以带 `SHA256SUMS.sigstore.json`。手动运行没有这个签名包。

```sh
sha256sum --check --ignore-missing SHA256SUMS
```

macOS 用 `shasum -a 256 -c SHA256SUMS`。Windows 用 `Get-FileHash <archive> -Algorithm SHA256` 与 `SHA256SUMS` 对照。

有 sigstore 包时，用 `cosign verify-blob` 核对。证书身份是本仓库标签上的 `.github/workflows/release-computer-use.yml`，OIDC 签发者是 `https://token.actions.githubusercontent.com`。完整命令在 [packages/README.md](packages/README.md)。

### GitHub Release

把压缩包解压到新目录，并把该目录加入 `PATH`。macOS 上让 `Grok Computer Use.app` 和 `gcu` 留在一起。第一次使用前用绝对路径登记 helper：

```sh
open -Ra '/absolute/path/Grok Computer Use.app'
gcu version
```

发行版应用名是 `Grok Computer Use`，bundle id 是 `ai.grok.computer-use.helper`。调试构建是 `Grok Computer Use (Dev).app`，bundle id 是 `ai.grok.computer-use.helper.dev`。应用包里的可执行文件必须和旁边的 `gcu` 是同一份字节。没有任何连接时，helper 在 10 分钟后退出。

### npm

需要已发布的版本，以及 Node 18 或更新版本。启动器不会下载二进制，它只运行 npm 已经装上的可选平台包。

```sh
npm install -g gcu@<version>
```

如果安装时去掉了可选依赖，启动器打印 `gcu: missing @grok-computer-use/<platform>; reinstall with optional dependencies enabled` 并以退出码 1 结束。`<platform>` 是 `darwin-arm64`、`darwin-x64`、`win32-x64`、`linux-x64` 或 `linux-arm64`。其他组合会打印 `gcu: unsupported platform <platform>` 并以退出码 1 结束。装好的文件无法启动时，打印 `gcu: cannot execute the installed native binary` 并以退出码 1 结束。

macOS 上登记与二进制放在一起的 helper：

```sh
open -Ra "$(npm root -g)/@grok-computer-use/darwin-arm64/bin/Grok Computer Use.app"
```

Intel 把 `darwin-arm64` 换成 `darwin-x64`。

### Homebrew

工作流会写出 formula，不会发布 tap。对生成的 formula 文件：

```sh
brew install --formula ./gcu.rb
```

formula 的 caveats 会打印 `open -Ra` 的路径，即 `$(brew --prefix gcu)/libexec/Grok Computer Use.app`。卸载用 `brew uninstall gcu`。

### winget

工作流为 `RongleCat.GrokComputerUse` 写出三份清单。提交前用 `winget validate --manifest <directory>` 检查。winget 列出该包之后：

```sh
winget install RongleCat.GrokComputerUse
winget uninstall RongleCat.GrokComputerUse
```

### Debian 与 RPM

包名 `grok-computer-use`。二进制是 `/usr/bin/gcu`。共享库由包管理器提供。桌面的无障碍服务仍然需要。这些包不含 macOS helper。

```sh
sudo apt install ./gcu-<version>-<target>.deb
sudo apt remove grok-computer-use
sudo dnf install ./gcu-<version>-<target>.rpm
sudo dnf remove grok-computer-use
```

### 未签名的包

没有配置 Apple 签名密钥时，helper 保留只绑定标识符的 ad-hoc 签名。它不是 Developer ID，也没有公证。Gatekeeper 可能拦截。不要关闭 Gatekeeper。每次更新这种 ad-hoc 构建后，都要在“隐私与安全性”里重新检查 helper。ad-hoc 签名不是发布者身份。

只放行这一个应用：

1. 在 Finder 里按住 Control 点 `Grok Computer Use.app`，选“打开”，再选“打开”。
2. 或者打开“系统设置 → 隐私与安全性”，对该应用选“仍要打开”。

如果拦的是隔离属性，只对 helper 清掉它，然后再打开：

```sh
xattr -cr '/absolute/path/Grok Computer Use.app'
open '/absolute/path/Grok Computer Use.app'
```

没有配置 Windows 证书时，`gcu.exe` 不签名。SmartScreen 可能显示发布者未知。选“更多信息 → 仍要运行”。先核对 `SHA256SUMS`。未签名文件也可能被启发式杀毒软件报出。校验和说明字节没有被改过，不会让发布者变成已知。配置了证书而签名失败时，构建会中止，不会退回未签名文件。

## 权限

把权限授给 helper 或桌面会话。不要把屏幕录制授给终端。

### macOS

辅助功能和屏幕录制属于 helper 应用，不属于启动 `gcu mcp` 的“终端”、iTerm 或编辑器。

先用 `open -Ra` 登记应用，再经 Launch Services 启动。macOS 上不带子命令的 `gcu` 会这样做。helper 还没运行时，`gcu mcp` 也会这样做。不要在终端里输入 `gcu agent` 来获取授权。

发行版 macOS 构建带 onboarding 窗口（`macos-webview`）。没有这个特性的构建不会打开窗口。只有缺少辅助功能或屏幕录制时窗口才会出现。它不会替你授权，也不会去点系统对话框。

窗口标题是“Allow Computer Use”。两张卡片：

| 卡片 | 窗口上的说明 | 何时生效 |
|---|---|---|
| Accessibility（辅助功能） | 让 helper 读取当前应用。 | 你允许之后立即生效。 |
| Screen Recording（屏幕录制） | 让 helper 看到屏幕。 | 只有 helper 重启之后才生效。 |

焦点在“Not now”（现在不要）。Escape 关闭窗口。Enter 不会打开设置，也不会重启 helper。“Open Settings”打开对应的“隐私与安全性”面板。允许屏幕录制之后，卡片会显示“Allowed — restart the helper”。使用“Restart helper”。重启失败时窗口保持打开，并说明 helper 没有重启。

onboarding 修复曾在 macOS 27 Mini 上验证。完整打包 helper 独占 TCC 路径、撤销/错误 PID/安全提示矩阵、签名/Dev 隔离，以及系统退出 helper 后的自动重拉仍为 **not_run**。路线 B 使用 App 子进程责任链，只准入 macOS 27 arm64；完整 App 产品路径为 **not_run**，不声称 macOS 14/15/Intel 可用。物理活动守卫需要输入监控权限；缺失或监听失效会禁用依赖输入。驱动的 `macos-private-spi` 默认开启，但仍有运行时探测，不能使 Chromium 后台点击可用。

`list_apps` 不需要这两项权限。列出窗口、启动应用和执行动作需要。缺授权返回 `permission_missing`；终端 native `gcu call` 仍不支持。SecurityAgent、CoreAuthenticationUI 或 UserNotificationCenter 拥有的可见安全提示会被拒绝。

### Windows

没有 TCC 提示。驱动读取 UI Automation，并且拒绝抬高自己的完整性级别。

目标的完整性更高时返回 `elevation_required`，说明是 “The target has a higher integrity level.”。这包括提权后的安装程序，以及 `gcu` 为 Medium、设置窗口为 High 的情况。驱动不会提权，也不会申请 UIAccess。UIPI 不会被绕过。安全桌面（UAC、登录、锁屏）会被拒绝。

`native-windows` 使用 monitored Worker 内的 UIA 与 WGC → PrintWindow，
父进程持有钩子、热键、输入释放和 Job 清理，绝不切到 InProcess。
引擎自身 High/System、UIAccess，以及非交互/安全桌面在 Worker 启动前拒绝。
UIA Invoke 可能激活窗口，后台能力只是 partial，不承诺无打扰。

前台输入遵循 [ADR 0013](docs/adr/0013-windows-stop-guard-liveness-lease.md)
的 Stop 守卫租约。空闲时不持续续鼠标证明，仅注入租约过期不会锁存 Stop。
策略/活动检查通过后，动作必须在注入前证明双钩子；注入期间续 100 ms 租约，
证明丢失即锁存并释放输入。消息泵心跳不是钩子证明。
物理鼠标机器上，零移动/零滚轮/零水平滚轮之后的第四探针是同位置绝对移动，
**可能触发悬停**；光标变了即作废，不补偿、不重试。
doctor 空闲时可能没有新鲜注入租约，其输出不授予前台权限。

托管 `windows-2022` runner 未证明鼠标钩子投递，相关前台测试跳过。
派工记录仅报告 **1 次**物理 `hook_probe` 通过，不代表所有产品路径验收。
STATUS 中 A1 的物理键鼠集成、动作期间续租、UAC/钩子丢失、热键冲突、
按住输入时的 Stop 时限、卡死 Worker、DPI、系统 App 冒烟仍为 **not_run**。
旧探针失败结果属于历史记录，不能作为新租约行为的证据。

### Linux

纯 X11 用 AT-SPI 读树和做动作，用 X 服务器管窗口和截图。被挡住的窗口只有在复合管理器能提供该窗口像素时才截。否则驱动拒绝截图，不会交回别的窗口的内容。

X11 装配需要 `native-x11`、正常工作的 AT-SPI 无障碍总线和 X server；
XI2 观察物理活动，XGrabKey 登记 Stop，XTEST 是受策略/活动/Stop 门禁约束的前台输入。
缺 AT-SPI 时 native 未就绪，不能改成截图加坐标绕过。
AccessKit 夹具缺 EditableText；真实 GTK/Qt 兼容性为 **not_run**。

[Wayland 指南](docs/wayland.md)（[中文](docs/wayland.md#zh)）区分装配和真实 portal 授予。
`native-wayland` 选择路线；真实 PipeWire 帧还需 `gcu-driver-linux/pipewire`
和兼容系统库。A3 增加经证明的单窗口采集/几何及限定范围的 reis 挂接 API，
没有增加 CLI 自动同意流程，当前 CLI 只探测。
GNOME/KDE/wlroots 的同意、帧、输入映射、撤销、到期和 Stop 均为 **not_run**；
other/unknown 合成器不能启用这条经证明的路线。
AT-SPI 文本/语义操作仍受身份和审批约束。
XWayland 只能用精确解析的 X 窗口，不能借 XTEST 控制原生 Wayland。
不提供整桌面采集/输入回退，也没有 Wayland 胶囊。
由获授权调用方创建会话时，restore token 仅所有者可读，绑定来源/窗口/进程/授权，
有效期不超过 24 小时且不长于授权。输入需要范围证明及健康的已注册 Stop；
已挂接 Stop 监听丢失会撤销绑定采集并保留 Stop 锁存。
portal 接口存在不等于同意或 Stop 注册成功。

## 接入客户端

`gcu install` 和 `gcu uninstall` 只改客户端配置，不启动驱动。写入的服务名是 `grok-computer-use`（`computer-use` 被 Claude Code 保留）。命令是这个 `gcu` 的绝对路径，参数是 `mcp`。

```sh
gcu install grok --dry-run
gcu install --client codex --scope user --json
gcu uninstall --client claude --yes
```

客户端：`grok`、`codex`、`claude`、`antigravity`、`gemini`、`opencode`、`cursor`、`vscode`。`--scope` 是 `user`（默认）或 `project`。客户端可以写在位置参数上，也可以写 `--client`。`--dry-run` 只打印差异，不写文件。`--json` 打印一个对象。重复安装不会复制条目。卸载只去掉 `grok-computer-use` 和本安装器拥有的钩子。

安装的 `gcu mcp` 选择 `auto`：包含对应后端的构建可接触真实桌面；当前 Windows/Linux 发行工作流产物缺 native 装配 feature，因而退出 2。没有 fake 回退。安装器不加 `--driver fake`，也不加自动审批。macOS 使用上面的 helper 代理。重连旧安装前请重读升级提示。

对 Grok 目录的真实修改会在终端上询问：

```text
gcu will modify files under ~/.grok (MCP config and the Stop hook). Continue? [y/N]
```

回答 `y` 或 `yes`。其他回答都取消。不是终端时要加 `--yes`。管道和 `GCU_YES` 都不是确认。项目范围的 Grok 安装如果把 Stop 钩子写进 Grok 主目录，同样会问。`--grok-home` 用来替换 `~/.grok`。

| 客户端 | 用户范围 | 项目范围 |
|---|---|---|
| `grok` | `<grok-home>/config.toml`，默认 `~/.grok/config.toml`。Stop 钩子是 `~/.grok/hooks/computer-use-turn-ended.sh`（Windows 为 `.cmd`）。只有 `reason` 为 `end_turn` 时钩子才调用 `gcu turn-ended`。 | `<project>/.grok/config.toml`。Stop 钩子仍在 Grok 主目录。 |
| `codex` | `~/.codex/config.toml`，含 `notify = ["<gcu>", "turn-ended"]`。 | MCP 在 `<project>/.codex/config.toml`。`notify` 仍在用户文件。 |
| `claude` | `~/.claude.json` | `<project>/.mcp.json` |
| `antigravity` | `agy mcp add` / `agy mcp remove`。`--agy` 指定二进制。 | `<project>/.agents/mcp_config.json` |
| `gemini` | `~/.gemini/settings.json` | `<project>/.gemini/settings.json` |
| `opencode` | `~/.config/opencode/opencode.json` | `<project>/opencode.json` |
| `cursor` | `~/.cursor/mcp.json` | `<project>/.cursor/mcp.json` |
| `vscode` | 用户的 `Code/User/mcp.json`：macOS 在 Application Support，Windows 在 `%APPDATA%`，Linux 在 `~/.config`。 | `<project>/.vscode/mcp.json` |

`--home` 覆盖 `GCU_HOME`，后者覆盖 `HOME`（Windows 为 `USERPROFILE`）。`--project` 覆盖 `GCU_PROJECT`，后者覆盖当前目录。`--gcu` 是写进配置的绝对可执行文件。

真正写入之前，原文件会复制为旁边的 `<name>.gcu-backup-<unix-time>`。备份仅所有者可读写（Unix 为 `0600`，Windows 为仅所有者的 ACL）。本命令无法解析的文件会原样留下，包括读不懂的 OpenCode JSONC（退出码 2）。`agy` 命令失败是退出码 10。找不到 `agy` 二进制是退出码 2。

短边至少 256 的图像才会附给模型。更短的图会先在已经脱敏的画面里放大一次。仍然无法附加时，结果保留截图文件路径，并附上说明 `[image attachment omitted: short edge below 256; use screenshot file]`。见 [ADR 0001](docs/adr/0001-grok-build-mcp-capabilities.md)。

## 授权

一个应用按四档之一授权。超时不是第五档。等待 120 秒，然后拒绝。

| 档 | 持续 | 是否保存 |
|---|---|---|
| 本轮（`turn`） | 到本轮结束。Grok 的 Stop 钩子发送 `end_turn`。Codex 使用 `notify`。没有钩子时，空闲 45 秒结束本轮。 | 否。 |
| 本会话（`session`） | 到 MCP 会话关闭。 | 否。 |
| 始终允许（`always`） | 到你撤销。路径、内容哈希、代码哈希、证书、产品或 build 有变化时，要重新批准。 | 是，写在 `approvals.json`，模式 `0600`。 |
| 拒绝（`deny`） | 仅这一次请求。 | 否。 |

客户端如果提供表单 elicitation，会先在工具调用里询问。选项是 `turn`、`session`、`always`、`deny`。elicitation 里的 `always` 不会被保存。取消不是批准，无头客户端立刻取消也一样。然后本地对话框再问。你在那个对话框里选择始终允许之后，才会写入 `approvals.json`。

本构建没有 WebKit 审批窗。有桌面时，Linux 先用 zenity，再试 kdialog；macOS 用 `osascript`；Windows 用 PowerShell。第一项是拒绝。聊天框里的 Enter 不会批准。没有显示器、在 CI 里、或这些工具都不存在时，请求被拒绝。发行构建拒绝任何 `GCU_APPROVAL` 值并以退出码 2 结束。该变量只存在于带 `eval-autoapprove` 的调试构建。

对话框展示操作系统核对过的签名、发布者和路径。应用名和图标只是标签，不是身份。模型理由是标成 “Model-provided” 的纯文本。签名不覆盖加载进该应用的插件。

敏感动作是另一次确认（允许或拒绝），不是四档授权里的一档。拒绝、超时和取消都不会执行。对话框开着时屏幕变了，这次确认作废。敏感区域无法定位时，不会显示未脱敏的画面。

没有一条命令可以允许全部应用。

```sh
gcu approvals list
gcu approvals list --json
gcu approvals revoke KEY
```

`KEY` 是 `list` 打印的一行。没有这个键时打印说明并以退出码 2 结束。授权文件读不了也是退出码 2。

文件位置：

| 系统 | 路径 |
|---|---|
| macOS | `~/Library/Application Support/grok-computer-use/state/approvals.json` |
| Windows | `%LOCALAPPDATA%\grok-computer-use\state\approvals.json` |
| Linux | `${XDG_STATE_HOME:-~/.local/state}/grok-computer-use/approvals.json` |

`gcu` 只读取 `config.toml` 里的 `[observe]`、`[policy]`、`[guard]`。它不会为了保存授权去改这个文件。

| 系统 | `config.toml` |
|---|---|
| macOS | `~/Library/Application Support/grok-computer-use/config.toml`（与 `state` 目录同级） |
| Windows | `%APPDATA%\grok-computer-use\config.toml` |
| Linux | `${XDG_CONFIG_HOME:-~/.config}/grok-computer-use/config.toml` |

文件存在时，空的 `stop_hotkey`、超出接受范围的预算或图像格式、或这三张表里的未知键，都会以退出码 2 结束。其他顶层表会被忽略。文件不存在时使用内置默认值。驱动实际登记的热键是下面的内置组合。这个 helper 不会把配置里的字符串传给 macOS 驱动。

## Stop 与恢复

Stop 会锁存。模型的下一轮、重新连接和新进程都不会清掉它。标记文件是上面那个状态目录里的 `stopped`。恢复不是 MCP 工具。

路线 B 将引擎控制连接的非正常断开（包括崩溃）视为**全局持久 Stop**。
本地停止断线前，引擎尝试在 CLI 状态目录写入 `stop-reason`（`hotkey`、`panel`
或 `internal_error`）。App 只用于展示原因：文件缺失、伪造或删除都不能恢复或放宽 Stop。
正常显式 shutdown/空闲退出，以及 Windows 内部 Worker 重启不触发这类断线 Stop，
也不清除旧锁存。恢复必须是新的可信用户动作；模型续轮、重连或重启都不恢复，
也不重放未完成动作。

安全上限仍为 ≤100 ms 锁存 Stop、≤500 ms 发输入释放、≤5 s 结束阻塞原生调用或杀 Worker。
驱动期限为 4750 ms，留出清理时间。这些是要求的预算，不是本次新增的物理时限实测；
缺桌面测量的项仍为 **not_run**。审批独立等待 120 秒，不能阻塞 Stop。

轮次结束清本轮授权并释放输入；会话关闭撤 token、会话授权、订阅、worker 及已挂接的
portal 资源/图片。Always 授权和 Stop 保留；数据删除限制见下文。

驱动真的在跑时，内置 Stop 组合键是：

| 系统 | 组合键 | 演示情况 |
|---|---|---|
| macOS | Control+Option+Command+. | 物理按键 **未验证**。 |
| Windows | Ctrl+Alt+Shift+Backspace | 托管运行器跳过了热键。物理按键 **未验证**。 |
| Linux X11 | Ctrl+Alt+Shift+Backspace | Xvfb 上的抓键测试通过。物理按键 **未验证**。 |
| 原生 Wayland | 有 GlobalShortcuts 注册 API；没有真机证明的注册 | 注册与范围证明前依赖输入保持关闭。真实会话 **not_run**。 |

进程内服务器（含 `--driver fake`）收到 SIGINT 和 SIGTERM 时会锁存 Stop 并松开按住的输入。macOS 上不带 `--driver` 的 `gcu mcp` 只转发 stdio。本文不声称那个代理进程里的 Ctrl+C 会锁存 helper。

macOS helper 与 Host 共用 Stop 锁存，不会再自建一把启动即锁上的私有锁。经 helper 的真实动作、仅 helper 的 TCC、App 子进程责任链、物理热键，以及真实的 `gcu resume` 都是 **未验证**。不声称 macOS 14、macOS 15 和 Intel。配置文件里的热键字符串仍然没有传进驱动。

`gcu-overlay` 能画 Stop 控件，并在 macOS 和 Windows 上把自己排除出截图。它没有链进这个 `gcu` 二进制。它的真实界面 **未验证**。`GCU_OVERLAY=0` 只会藏起那层界面。这个二进制里没有这个开关。

恢复进程内会话时，终端的输入和输出都必须是 TTY：

```sh
gcu resume
gcu resume --session /path/to/gcu-control-directory
```

进程会打印 `You are resuming stopped model operations. Type yes <challenge> to confirm:`。在 120 秒内输入 `yes` 和那八位十六进制字符。有多个控制目录时加上 `--session`。目录名是 `gcu-control-*`，在 `$XDG_RUNTIME_DIR` 或系统临时目录下。确认不对则会话保持停止。

这项检查只是尽力而为。同一用户下的程序可以伪造 TTY。见 [ADR 0003](docs/adr/0003-resume-trust.md)。`gcu turn-ended` 结束本轮授权，不会清除 Stop。钩子失败时打印一行脱敏说明并以退出码 0 结束。

## 平台差异

驱动库和 `gcu` 命令不是一回事。表里是测试和 [STATUS](../docs/plans/computer-use-v2/STATUS.md) 已经表明的事。“在 `gcu` 里”指这个二进制接上了什么。

| 能力 | macOS | Windows | Linux X11 | Linux Wayland |
|---|---|---|---|---|
| `gcu mcp` 里的真实驱动 | 路线 A helper 共用 Host Stop/活动；路线 B engine 只准入 macOS 27 arm64。完整 TCC/产品路径 **not_run**。 | 默认关闭的 `native-windows` 后已装配，仅 monitored Worker；当前发行工作流未开启。 | 默认关闭的 `native-x11` 后已装配 AT-SPI + X11、Host 锁存/活动；当前发行工作流未开启。 | 默认关闭的 `native-wayland` 探测/Host 适配；驱动有来源证明挂接 API，CLI 不启动同意；真实 portal **not_run**。 |
| 无障碍树 | AX。Mini、macOS 27、arm64：真机契约 26 通过、4 跳过、0 失败。第一次读取可能是很短的不完整树。macOS 14/15 和 Intel **未验证**。 | UIA，在 `windows-2022` 运行器上：契约 15 通过、15 跳过、0 失败。第一次读取可能不完整。物理桌面 **未验证**。 | AT-SPI，Xvfb CI：23 通过、6 跳过、0 失败。物理桌面 **未验证**。 | 有环境探测和 portal 代码。真实合成器 **未验证**。 |
| 窗口截图 | 那台 Mini 的驱动测试里，ScreenCaptureKit 能截被挡住的窗口。打包后的 helper **未验证**。 | WGC，然后 PrintWindow。被挡住的窗口和 150% DPI **未验证**。 | Xvfb 上的 XComposite 通过。没有复合且被挡住：拒绝。 | portal ScreenCast。真实 portal **未验证**。 |
| 后台动作 | 部分可用。macOS 27 上 AppKit 能收到后台点击。Chromium 不能（[ADR 0006](docs/adr/0006-macos-tcc-helper.md)）。 | 部分可用。UIA Invoke 可能激活窗口。原生按钮用不激活前台的点击。 | 有对应接口时用 AT-SPI。抓不到 Stop 时不用 XTEST。 | 只有可靠 Stop 时才用 portal 输入。没有全桌面退路。真实会话 **未验证**。 |
| 前台键入和指向 | 受策略约束。物理空闲检测 **未验证**。 | ADR 0013 动作门控 Stop 租约；第四探针可能悬停。仅报告 1 次物理 hook_probe 通过，完整 A1 检查 **not_run**；托管 runner 前台不可用。 | 热键抓取成功时，Xvfb 上的 XTEST 可用。物理设备 **未验证**。 | **未验证**。 |
| Stop 热键 | 见 Stop 一节。 | 见 Stop 一节。 | Xvfb 测试通过。 | 不声称可用。 |
| 覆盖层不进截图 | 实现在 `gcu-overlay`，不在这个二进制里。macOS 真机契约跳过了这项。真实界面 **未验证**。 | 已实现，不在这个二进制里。真实界面 **未验证**。 | X11 驱动在截图租约期间藏起覆盖层窗口。真实界面 **未验证**。 | 声明为不可用。 |

更多说明：[macOS 驱动笔记](crates/gcu-driver-macos/README.md)、[Windows 驱动笔记](crates/gcu-driver-windows/README.md)、[Windows 运行器诊断](crates/gcu-driver-windows/diagnostics.md)、[覆盖层笔记](crates/gcu-overlay/README.md)。

## 浏览器扩展

所选 native 驱动在平台能力允许时提供 L0 浏览器无障碍路线，不需要扩展。
可选扩展的固定 ID 为 `jgpkhdjlddaoopklipcnpcjcpiancmff`，但**所有平台 L2 DOM
融合/动作均关闭**。A4a 无法证明共享顶层文档与 OS 窗口的映射；A4b 被阻塞，
没有挂接生产 DomChannel。ADR 0014 仅为 Proposed，不代表已接受的机制或交付承诺，
本检出没有其正文。见 [A4a 证明与既有 Chrome/Edge 安装步骤](docs/browser-extension.md#proof)。
共享徽章仅表示浏览器同意，不代表 DOM 控制可用、应用已授权或 Stop 已恢复。
L0 不能阻止浏览器内部重定向。

## 隐私

`get_app_state.include_screenshot` 接受 `true`、`false` 或 `"auto"`
（[ADR 0011](docs/adr/0011-auto-screenshot-policy.md)）。默认仍为 `true`；
既有布尔调用行为与序列化形状不变。auto 评估可用交互节点、弹窗内控件及原始 canvas
角色占比。`gcu_core::observe::QualityThresholds.min_interactive_nodes` 和
`canvas_role_ratio` 均**待评测校准**，默认 `None`，没有编造生产阈值。
评测可通过 `ActionRouter::with_auto_capture_policy` 注入边界，不是新配置键。
没有交互节点边界时 auto 保守保留图片；校准后充分的树可能省图，canvas/稀疏树、
文本 diff 为空而安全像素变化仍请求图片。动作结果也会为坐标动作或显式请求附图。

auto 仍先采集并脱敏再决定交付，以发现文本树没有表达的像素变化。
省略的采集不替换上次已交付模型的图片。不完整原生树、未知敏感区域、树/采集或
几何不一致仍按既有路线拒图。不声称采集延迟改善；只有同任务评测证明成功率不降后
才能更改默认值。

截图在交给模型、对话框或 `gcu call` 之前先脱敏。敏感区域涂成不透明灰色，并向外扩 4 像素。敏感区域无法定位，或树不完整时，不返回图像。文本树仍可以返回。未脱敏的画面不会为模型写出去。

不带 `--json` 的 `gcu call` 把这些安全图像复制到 `$XDG_RUNTIME_DIR` 或临时目录下的私有 `gcu-call-*` 目录。目录模式 `0700`，文件模式 `0600`（Windows 为仅所有者的 ACL）。后续走到产物清理的 CLI 调用会尝试删除超过一小时的目录；没有到期定时器。你也可以更早删掉。`--json` 打印 MCP 结果。正常 MCP 连接关闭/Drop 会删除其临时溢出文本和截图。强杀和删除失败可能留文件；Windows MCP 图片 DACL 验证仍未闭环。

日志只到标准错误。没有日志文件。`doctor` 最多保留上一个已结束进程的 20 条元数据：工具名、结果、耗时、时间戳。没有键入的文本、目标标签或图像字节。读取时过滤超过七天的记录，不会自动删除旧文件。路径是状态目录里的 `recent-calls.json`。

这些 CLI 规则不会擦除 App 聊天历史、审批卡副本或客户端/模型提供方已存结果。
撤销授权阻止后续访问，不等于物理擦除既有观察。
App journal/图片清理、Windows 文件 ACL、签名授权匹配和正式引擎验签仍有
[security.md](docs/security.md) 与[数据清单](docs/data-inventory.md)列出的缺口。
两份文档是注明基线的审计，其装配描述早于平台卡，须结合当前代码与 STATUS 阅读。
`GCU_LOG_PAYLOADS` 当前没有载荷日志实现，不能据此承诺全秘密清洗或 App 日志
7 天 / 5×5 MB 保留保证。剪贴板历史/同步可在条件恢复后仍保留粘贴文本，gcu 无法删除外部副本。

会话密钥不放进环境变量，也不放进 `argv`。不要去刮子进程的环境。恐慌时打印 `Internal panic; operation was not replayed`，不含载荷。

## 故障排除

| 你看到的 | 含义 |
|---|---|
| `native-x11 is not compiled into this build`（或 `native-windows` / `native-wayland`） | 省略 `--driver` 即选择 `auto`。该路线未编入时，`auto` 和 `native` 以退出码 2 结束，并且不会退回 fake。`--driver fake` 是显式的离线驱动。发行二进制还要设置 `GCU_ALLOW_FAKE=1`。`--scene` 只对 `--driver fake` 有效。 |
| `Fake driver requires a debug build or GCU_ALLOW_FAKE=1` | 这是发行二进制。 |
| `GCU_APPROVAL is forbidden in release or non eval-autoapprove builds` | 去掉 `GCU_APPROVAL`。发行构建没有自动批准开关。 |
| Gatekeeper 说应用已损坏或来自身份不明的开发者 | ad-hoc 构建。按未签名包的步骤做。不要关闭 Gatekeeper。 |
| SmartScreen 说发布者未知 | 未签名的 Windows 构建。校验和一致后再选“更多信息 → 仍要运行”。 |
| `permission_missing` | helper 缺少辅助功能或屏幕录制，或者 Linux 会话缺少 AT-SPI / portal 授予。 |
| 允许屏幕录制之后仍然失败 | 重启 helper。macOS 只把这项授予应用于之后启动的进程。辅助功能立即生效。 |
| 第一次看 egui / AccessKit 窗口时树很短，而且没有截图 | 树是懒发布的，并被标成不完整。在 macOS 27 Mini 上，夹具大约 588 毫秒后从 7 个节点长到 54 个。不完整的树不出图。再读一次可能就完整了。Windows UIA 上也见过同样的懒首次读取。 |
| `SetValue` 返回不可设置 | Linux 上的 AccessKit 文本框没有实现可编辑文本；macOS 上的 egui 文本框可能报告成功但文字没变。驱动会把值读回来核对。改用 `type_text`。在 Linux 上这需要前台策略。真实的 GTK 和 Qt 应用 **未验证**。 |
| `elevation_required` | Windows 目标完整性高于 `gcu`，或引擎自身具有被禁止的 High/System 完整性或 UIAccess。`gcu` 不会提权。把目标以非提权方式运行，或自己完成这一步。 |
| Windows 运行器上前台动作不可用 | 托管运行器的受限令牌从未把合成鼠标送到低级钩子。这是那个环境的结果，不是对有物理鼠标的电脑的承诺。 |
| macOS 上 Chromium 忽略后台点击 | 在 macOS 27 上测得。驱动不会改走前台点击来掩盖这一点。 |
| `Computer-use helper rejected the connection` | CLI 和 helper 不是同一份已签名字节，或者套接字不属于这个用户。请登记包含这份 `gcu` 的应用。 |
| `Launch Services did not start the computer-use helper` | 先对 `.app` 的绝对路径执行 `open -Ra`。Launch Services 不认识的应用，`open -b` 启动不了。 |
| `The computer-use helper runs only on macOS` | 在其他系统上启动了 `gcu agent`。 |
| 授权文件无法读取 | 文件不是仅所有者可读写、是符号链接，或已损坏。修好或删掉这一个文件。不要改主目录的权限。 |
| 模型一直收到 `stopped_by_user` | Stop 仍锁着。`gcu turn-ended` 不会清它。在 TTY 里用 `gcu resume` 恢复。真实的 macOS helper 恢复 **未验证**。 |
| `gcu install` 说要改 `~/.grok` | 它确实会改。项目范围的 Grok 安装仍会把 Stop 钩子写到那里。只有你确实要这样时才加 `--yes`。 |

## 卸载

1. `gcu` 还在 `PATH` 上时去掉客户端条目。客户端和范围要和安装时一致。如果会改 Grok 目录而标准输入不是终端，加上 `--yes`。

   ```sh
   gcu uninstall grok
   gcu uninstall codex --scope project
   ```

   这会删除 `grok-computer-use` 条目和本安装器拥有的钩子。`*.gcu-backup-*` 文件会留下。

2. 撤销持久授权；二进制已经删掉之后，也可以直接删除 `approvals.json`：

   ```sh
   gcu approvals list
   gcu approvals revoke KEY
   ```

3. 卸掉包，或删掉解压目录：

   ```sh
   npm uninstall -g gcu
   brew uninstall gcu
   winget uninstall RongleCat.GrokComputerUse
   sudo apt remove grok-computer-use
   sudo dnf remove grok-computer-use
   ```

4. 操作系统授予要自己撤。`gcu` 不会撤它们。

   - macOS：“系统设置 → 隐私与安全性 → 辅助功能”和“屏幕录制”。去掉 `Grok Computer Use` 或 `Grok Computer Use (Dev)`。
   - Linux portal：在桌面设置里去掉 ScreenCast / RemoteDesktop 权限。Wayland 驱动若保存恢复令牌，目录是 `${XDG_DATA_HOME:-~/.local/share}/grok-computer-use/wayland-restore`（目录模式 `0700`，文件模式 `0600`）。默认发行路线不打开 portal；自定义 native 构建仅在获授权调用方挂接经证明的会话时创建 token。删除本地 token 不等于撤销桌面 portal 授予。

5. 如果还想清掉残留：`config.toml`、`approvals.json`、`stop-reason`、`recent-calls.json`、上面的 Wayland `wayland-restore` 目录，以及 `gcu-call-*` / `gcu-control-*` 目录。路径见“授权”和“Stop 与恢复”。删除保留数据前停止全部进程；删除 `stopped` 不是受支持的恢复方式。App 的策略/历史/Stop 另存，见数据清单。

## 命令

由 clap 解析的子命令都接受全局参数：`--driver auto|native|fake`（默认 `auto`），`--scene <name>`（只对 fake 有效；省略时 fake 使用 `form`）。

```sh
gcu version
gcu doctor --driver fake --json
gcu call list_apps --driver fake --scene chat
gcu call list_apps --driver fake --args '{}' --json
gcu mcp --driver fake --scene form --profile minimal --transport stdio
gcu mcp --transport http --token-fd <fd> --port <port> --profile default
gcu mcp --connect <url> --token-fd <fd>
gcu install grok --scope user --dry-run --json
gcu uninstall vscode --scope project
gcu approvals list --json
gcu approvals revoke KEY
gcu resume --session <gcu-control-directory>
gcu turn-ended --help
```

`gcu mcp` 的默认值是 `--transport stdio`、`--profile default`（`default`、`minimal` 或 `full`），以及 `--turn-idle-secs 45`（至少为 1）。HTTP 必须带 `--token-fd`。`--port` 默认是临时端口。`--connect` 是 stdio 代理，也需要 `--token-fd`。自建 native HTTP 遵守上文入口限制（macOS 不支持）。

`gcu turn-ended` 在普通解析之前处理。它从标准输入读 Grok 的 Stop JSON，或接受一个 Codex notify JSON 参数。`--session` 选择一个控制目录。`--previous-notify` 先运行原来的 notify 命令并保留它的退出状态。其他错误打印一行脱敏说明并以退出码 0 结束。

`gcu engine` 出现在 `--help` 里。它需要 `--host-fd`，或者 `--host-socket` 加 `--host-token-fd`，以及 `--stop-fd`。这些通道由 App 宿主提供。

退出码：0 成功；2 参数错误、拒绝确认、`ambiguous_app`、`invalid_key` 或 `out_of_bounds`；3 `permission_missing`；4 其他工具错误，包括 `approval_denied`、`elevation_required` 和 `stopped_by_user`；10 内部失败、恐慌，或安装时的 IO / `agy` 失败。

## 给开发者

这个目录是独立的 Cargo 工作区，固定 Rust 1.99.0。这些 crate 里，桌面 App 只能依赖 `gcu-protocol` 和 `gcu-host-protocol`。在本目录：

```sh
cargo fmt --all --check
bash scripts/verify.sh
CROSS=1 bash scripts/verify.sh
cargo xtask check-deps
cargo xtask golden --check
cargo xtask budget
cargo deny check
```

`verify.sh` 依次运行 fmt、clippy、test、check-deps、golden、budget、deny，遇到第一个失败就停。机器很小的话用 `rcargo` 把编译放到共享服务器上。`cargo xtask golden --check` 把协议 JSON 与 `crates/gcu-protocol/golden/` 逐字节比较。`cargo xtask budget` 执行 14,000 字节的工具 schema 上限。许可允许列表拒绝 GPL、AGPL 和 LGPL。WebKitGTK、GTK、libpipewire 和 libxkbcommon 是系统库，不在这张图里。

布局：`crates/`（协议、host API、host 协议、core、各驱动、假驱动、MCP、覆盖层、CLI）、`apps/fixture`、`xtask`、`packages`、`docs/adr`、`ui/locales`。计划进度在 [STATUS.md](../docs/plans/computer-use-v2/STATUS.md)。一次性安装检查写在 [scripts/install-smoke/README.md](scripts/install-smoke/README.md)。

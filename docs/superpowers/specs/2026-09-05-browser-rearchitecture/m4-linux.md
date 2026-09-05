# M4：Linux 功能对等设计

**状态：** 设计完成，开发与平台实验未执行；全部发布证据均待产生。
**上游：** [总体设计](../2026-09-04-browser-rearchitecture-design.md)、[共用合同](00-contracts.md)。
**实施：** [M4 开发计划](../../plans/2026-09-05-browser-rearchitecture/m4-linux.md)。
**准入：** M1-G06、M1-R06、M1-D06；M2 面板只在 M2-06 通过后作为增量能力验收。

## 目标与非目标

- 在明确列名的 Linux x64 桌面环境交付 Managed Runtime、Profile、接管、恢复和实际安装包。
- 保持 Chromium sandbox；系统限制导致不能建立沙箱时关闭 Managed，提供可用 Preview。
- 对 X11、原生 Wayland 和 XWayland 分别报告能力，不以环境变量或能打开窗口推断安全能力。
- 证明 persistent Profile 的凭据静态加密路径，不允许 Chromium 悄悄使用明文/basic 密钥存储。
- 保留 M1 的请求身份链、Host 权限源、ProfileGuard 和 Artifact 隔离。
- 不交付 Linux arm64、无桌面服务器、root 运行、Flatpak/Snap 或任意发行版承诺。
- 不将远程桌面、屏幕录制 portal、GlobalShortcuts portal 或 DOM trusted event 当作全局输入监听证明。
- 不引入 `--no-sandbox`、关闭 seccomp、放宽系统 AppArmor 或要求 root 的产品修复。
- 不以本包声明 M3 Chrome Connector 已支持 Linux；该连接器需要自己的进程归属和认领证据。
- 不改动 App shell/AppWorkbench 的状态所有权，也不把 CEF 纳入 Linux 交付前提。

## 当前代码证据

| 现有位置 | 核对事实 | 本包增量 |
| --- | --- | --- |
| `.github/workflows/ci.yml` | Rust 矩阵已有 macOS、Windows、Ubuntu；Linux 安装 WebKit/GTK 依赖 | 增加真实桌面与安装包安全验收 |
| `.github/workflows/release.yml` | Linux 构建使用 Ubuntu 22.04 | 保持构建基线，核对 Runtime 自身 ABI |
| `scripts/build-local.sh`、`package.json` | `pnpm build:linux` 已构建 AppImage/deb/rpm | 把完整签名 Runtime tuple 纳入三个包 |
| `docs/BUILD.md` | 已记录 AMD/Hyprland 下 bundled WebKit EGL 黑屏 | Preview 与 Managed 分开归因和留证 |
| `src-tauri/src/secrets.rs` | Secret Service 不可用时 App secrets 可以回退 0600 文件 | 浏览器 Profile 凭据禁止套用这一回退 |
| `src-tauri/src/updater.rs` | Linux App updater 仅 AppImage 自动；deb/rpm 手动 | 保持包类型真实能力，不混同 M6 Runtime updater |

上述代码未证明 Linux Managed 已工作；新模块均是未来产物。

## 模块与接口

| 模块 | 职责 |
| --- | --- |
| `src-tauri/src/browser/platform/linux/capabilities.rs` | 桌面、ABI、sandbox、密钥存储与输入 Provider 探测 |
| `src-tauri/src/browser/platform/linux/input_fence.rs` | X11 候选 Provider、Wayland 能力声明、监听健康与归因 |
| `src-tauri/src/browser/platform/linux/sandbox.rs` | 启动约束、进程状态证明、失败原因 |
| `src-tauri/src/browser/platform/linux/credentials.rs` | Browser 专用 Secret Service probe 与 persistent 准入 |
| `src-tauri/src/browser/runtime/profile_guard.rs` | 复用 M1 guard，增加 Linux descendant ledger 与退出证明 |
| `src/components/browser/platform/LinuxCapabilityNotice.tsx` | Host 状态投影、重新检测、人工 Preview 与修复入口 |
| `scripts/browser/verify-linux-package.mjs` | 三种最终包提取、manifest、签名、ELF 与权限核对 |
| `tests/browser/native/linux/driver.py` | 通过 AT-SPI2 控制实际安装的 Tauri/WebKitGTK App，并采集真实窗口证据 |
| `scripts/browser/run-linux-native.mjs` | 复用 M1 native-driver 合同，编排 Linux 原生 driver 与证据收集 |

```ts
type LinuxCapabilityReport = {
  display: "x11" | "wayland" | "xwayland" | "unknown";
  inputAttribution: "proven" | "blocked";
  sandbox: "proven" | "blocked";
  credentialStorage: "encrypted" | "locked" | "unavailable";
  evidenceId: string;
  reasonCodes: string[];
};
```

报告是 Host 对当前启动环境的判断，不能由 UI 或 worker 自报提升。
`evidenceId` 指向签名发布矩阵与本次健康探测组合，不含用户名或完整机器路径。
所有浏览器命令继续使用 `RequestEnvelope`；wire 为 camelCase，`protocolVersion=1`。
`hostBootId`、binding/runtime generation、revision、lease、fence、policy 全部按共用合同校验。
generation/revision/fence 使用 u32；溢出关闭 admission 并更换相应身份，不回绕；安全 sequence 仍为十进制字符串。

## 平台可行性门

执行依赖为 M4-01 -> M4-03 -> M4-04 -> M4-02 -> M4-05 -> M4-06。
M4-01 收集环境、ABI 与 Linux 原生 driver；M4-03/04 先证明监管与 Profile，再由 M4-02 验证实际输入归因。
早期纯状态机 fixture 可并行编写，但不能在不受监管或凭据存储未准入的 Runtime 上完成输入实验。
X11 候选实现使用 XInput2 原生事件与受监管 Chromium 窗口集合关联。
`_NET_WM_PID`、窗口标题、焦点或 URL 单独都不足以证明归属；须联合启动进程身份与窗口映射证据。
X11 本身没有隔离同显示服务器恶意客户端的安全保证；本功能不声称提供该隔离。
Gateway 的 Playwright/CDP 动作不得触发接管；其他原生输入保守视为外部控制。
窗口映射冲突、PID/start-time 不一致、Provider 重连或显示服务器重启立即 fail closed。
原生 Wayland 没有可假定存在的通用全局输入观察与跨进程窗口身份 API。
portal 可用不代表能覆盖 pointer/key/wheel/IME；每个 compositor/backend 组合必须单独举证。
没有合格 Provider 时，原生 Wayland 和归属不完整的 XWayland 只提供人工浏览，writer lease 为零。
需要 compositor 特有扩展的实验须明确最低版本、权限、事件集合和故障探测；不默认启用。
部分 X11 通过而 Wayland 未通过时，可以交付明确受限的 Linux beta，但不能标记“Linux 完整功能对等”。
M4-06 的完整出口要求声明支持矩阵中的 writer 行全部通过；No-go 行必须显式移出支持声明并保留限制。

## 输入与接管流程

1. 创建 Managed binding 前验证发行 attestation、有效 policy、sandbox、Provider 与 Profile 模式。
2. 注册 `runtimeId + runtimeGeneration + PID/startTime + window set`，绑定完整 ControlDomain。
3. 收到原生 pointer down、wheel、key down、touch 或已声明 IME 输入后按 M1 原子 fence。
4. 撤销该 Runtime 下全部 writer lease、暂停观察、取消队列并等待 worker 确认。
5. 无法判断归属或监听失效时进入 `blocked_input_attribution`，不把超时当作无输入。
6. 重新检测通过仅恢复可用状态；用户仍须在可信 App UI 显式交还。

hover 不触发接管；缺少原生事件的辅助技术操作遵循显式接管限制。
Wayland 下鼠标已经操作过窗口却未被观察到，必须判实验失败，不能靠用户培训过门。
Host 收到权威事件到拒绝命令且 worker ACK 的目标为 p95 <= 250 ms，至少 30 个样本。

## Sandbox 与进程监管

M1 的 ProfileGuard 继续持有 Profile lock，并通过私有管道观察 Host liveness。
Linux guard 使用独立进程组、启动身份 ledger 与 pidfd（内核支持时）核对受监管子进程。
PID namespace、user namespace、seccomp 与 Chromium 当前构建策略共同构成验证结果。
检查 renderer 的 `NoNewPrivs`、`Seccomp` 与 namespace 信息，不能仅检查启动参数中没有禁用开关。
Ubuntu 的 userns/AppArmor 限制必须在默认系统配置下测试；失败提供结构化原因，不修改系统策略。
宿主进程、guard 和不同 Chromium 子进程的 sandbox 状态分别记录，不能要求它们具有相同状态。
清理顺序为 fence、drain、TERM、限时 KILL、wait/reap、核对 ledger，再释放 Profile lock。
30 秒后仍存在匹配 run nonce 的 descendant 时保持 `cleanup_failed` 和 lock；禁止按进程名清理。
guard 自身异常退出须由外层 liveness/启动 reconciliation 发现；旧 descendants 未清零不得再开同 Profile。

## 凭据与 Profile

Linux persistent Profile 只在实测 Secret Service 支持且 Chromium OS crypt 使用该存储时开放。
App API key 的现有 0600 文件回退不等价于浏览器 Cookie/密码加密。
凭据 probe 使用 disposable Profile 和合成测试 Cookie，验证磁盘无明文并能在正常重启后解密。
仅扫描不到明文不足以通过；还须证明密钥被 Secret Service 管理，不能是可推导的 basic 常量密钥。
Secret Service 锁定、D-Bus 断开或用户拒绝解锁时返回 `credential_store_locked/unavailable`。
UI 可选择人工 Preview 或已验证的 ephemeral Profile；不得静默把 persistent 降为临时数据。
ephemeral Profile 只有经验证的私有 tmpfs 路径可用时开放，退出后销毁；不承诺 swap/休眠镜像保密。
没有该条件则关闭 Linux Managed，保留数据供修复，不写入磁盘明文替代位置。
持久目录 0700、敏感文件 0600，拒绝 symlink 父目录和另一 UID 所有的目录。
Profile epoch 和 generation 遵循 M1；回退包不能原地打开更高 epoch 的 Profile。

## 实际包与系统矩阵

M1 的 `tests/browser/harness/native-driver.ts` 仅已有 macOS/Windows adapter；M4 必须新增 Linux 实现。
外置 driver 使用 AT-SPI2/pyatspi 提供 startInstalledApp、clickControl、readStatus、stopApp，操作对象必须是已安装 App 的进程/窗口。
Linux runner 依赖发行版 `python3-pyatspi`、`python3-gi` 与 AT-SPI2 session bus；依赖和辅助技术权限单独记录，不改产品权限。
X11 截图由实际窗口采集；Wayland 使用经用户授权且在该 compositor 验证的截图 portal，缺权限/不可访问树时记录 blocked。
Playwright 只做测试编排、fixture oracle 和 Managed 页面对照；它打开同一网页成功不等于 Tauri/WebKitGTK App UI 已通过。
允许以已实测的 WebDriver adapter替代 AT-SPI2，但须记录精确驱动/版本与 App window identity，不接受普通 Chromium 页面代替。

| 包 | 必测系统/桌面 | 真实限制 |
| --- | --- | --- |
| AppImage x64 | Ubuntu 22.04 GNOME X11、Ubuntu 24.04 GNOME Wayland | 测 FUSE 可用/缺失及提取运行；WebKit EGL 单独记录 |
| deb x64 | Ubuntu 22.04/24.04、Debian 12，X11 与可用 Wayland | 使用系统 WebKit；包管理升级/卸载保留 Profile |
| rpm x64 | Fedora 44 GNOME Wayland、Xfce X11 | Fedora 为明确验收目标；不据此承诺 RHEL/openSUSE |
| 诊断扩展 | Arch 固定快照 + AMD/Hyprland | 只收集 EGL/Wayland 证据，不计正式支持行 |

rpm 的 X11 writer 测试使用版本锁定的 Xfce/X11 环境；镜像必须记录桌面组件版本。
矩阵记录实际 OS、kernel、compositor、GPU、驱动、OpenSSH、glibc、Secret Service、包 digest。
Ubuntu 22.04 构建不自动意味着全部 Runtime ELF 满足 glibc 2.35；逐 ELF 检查版本需求后才声明最低线。
不改变当前构建基线来掩盖 ABI 失败；提高最低系统版本必须更新用户支持声明和独立验收。
三个包均含完整 Runtime、SBOM、license、manifest、package signature 和 M1 distribution attestation。
AppImage 重打包或改 AppRun 后重新验证和签名；Tauri updater `.sig` 不能验证内部所有文件。
deb/rpm App 更新保持包管理/手动路径；M6 后才可按独立合同更新用户目录下 Runtime。

## 故障、回滚与量化验收

| 故障 | 恢复 | 必须保存的证据 |
| --- | --- | --- |
| sandbox 或输入 Provider 不可用 | fence，明确能力限制，Preview | 原因码、脱敏 probe、未获 writer lease |
| 密钥存储锁定 | 保留 Profile，显式解锁后重测 | 合成 Cookie 往返与无明文验证 |
| Host/worker/Chromium crash | M1 reconciliation，新 generation，显式交还 | descendants=0、旧命令 stale |
| 升级失败或 epoch 不兼容 | 兼容且未撤销 LKG，最多一次；否则 Preview | pointer、Profile generation 与无损校验 |
| AppImage 黑屏 | 记录 Preview EGL；可选择通过验证的系统包 | UI 非空截图与独立 Runtime probe |

每个声明 package/OS/display/credential 组合连续三轮 golden path 100% 通过。
每行必须包含 native-driver 类型、安装包 digest、实际 App PID/窗口身份、原生截图和控件状态；缺 native 证据即 not_run。
安全越权、自动重放、Profile 串用、接管后旧 primitive、测试结束 30 秒后本 run descendants 全为零。
至少 30 次冷启动/建 tab/恢复样本满足 M1 的 8 秒、2 秒、10 秒 p95 初始目标。
实际安装、N-1 升级、回退、离线 bootstrap 有效/过期、低磁盘、空间/非 ASCII 路径均留证。
输入实验至少覆盖 200 次外部输入、200 次 Gateway 自动化动作、30 次监听断开/权限或窗口变化。
15 locale key parity 通过，`en/zh/de/ru/ta` 检查错误/限制/接管视图无重叠。
总证据目录为 `docs/qa/browser-rearchitecture/m4/`；M4-06 签署 Go/No-go，所有记录目前未执行。

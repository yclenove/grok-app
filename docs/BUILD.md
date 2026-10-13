# Grok App 桌面端构建与发布

CU当前状态见[CI修复检查点](plans/2026-10-13-computer-use-ci-fixes-checkpoint.md)第八批：旧ATK GetActions崩溃已独立/原产品复现，改用有界机器名协议后Jammy X11库52项、原生输入19/可访问性41/剪贴板66项通过，待新远端复验。`9309a6bb`远端Windows仍plugin auth超时（2406通过/1失败），ARM64产品target仍失败。两边main已合入、最新fetch无新增；维护运行时、完整NOTICE、安装及完整R1–R4继续，App macOS 11+ / CU受管浏览器14+与用户App144/home保持。

支持平台：

| 平台 | Triple | 本地构建 | CI Release 产物 |
|------|--------|----------|-----------------|
| macOS Apple Silicon | `aarch64-apple-darwin` | ✅ | `.dmg` |
| macOS Intel | `x86_64-apple-darwin` | ✅（在 Apple Silicon 上交叉） | `.dmg` |
| Windows x64 | `x86_64-pc-windows-msvc` | ✅ 本机 Windows，或 **macOS/Linux 经 cargo-xwin** | NSIS `*-setup.exe` + **绿色版** `*-portable.zip` |
| Linux x64 | `x86_64-unknown-linux-gnu` | ✅ 本机 Linux | **AppImage** + **.deb** + **.rpm**（Release 仍用 `ubuntu-22.04`，glibc 2.35） |
| Linux ARM64 | `aarch64-unknown-linux-gnu` | ✅ 本机 aarch64 Linux | **AppImage** + **.deb** + **.rpm**（Release 用原生 `ubuntu-24.04-arm`，glibc 2.39，面向 Ubuntu 24.04+） |

> macOS / Linux 交叉打 Windows 安装包使用 Tauri 官方 runner：`cargo-xwin` + `makensis`（NSIS）。  
> 见 [Build Windows apps on Linux and macOS](https://v2.tauri.app/distribute/windows-installer/#build-windows-apps-on-linux-and-macos)。

## 窗口 chrome

| 平台 | 配置 | UI |
|------|------|-----|
| macOS | `tauri.macos.conf.json`：`decorations` + `titleBarStyle: Overlay` + 透明侧栏 | 原生 traffic lights |
| Windows | `tauri.windows.conf.json`：`decorations: false`、非透明 | 自绘 min / max / close |

关闭窗口 → 隐藏到托盘；退出请用托盘 **Quit Grok**。

## 1. 本地环境

```bash
# 依赖：Node 22+、pnpm 9、Rust stable、Xcode CLT (macOS)
pnpm install
pnpm setup:cross   # rust targets + (macOS) cargo-xwin / nsis / llvm 检查
```

### `pnpm dev` 与已安装版并排

`pnpm dev` 会 merge [`src-tauri/tauri.dev.conf.json`](../src-tauri/tauri.dev.conf.json)：

- `identifier=com.grokapp.desktop.dev`（single-instance mutex 与正式版 / **grok-app-latest** 隔离）
- 开发页固定使用 `http://127.0.0.1:1421`；Vite、Tauri `devUrl` 与本地媒体 CORS 必须保持同一来源，避免 Windows 将 `localhost` 解析到未监听的 IPv6 后出现 `ERR_CONNECTION_REFUSED`
- `productName=Grok Dev`，Dock / 窗口图标用 `src-tauri/icons/dev`（白色 AppIcon.appiconset，不是从黑标反相）
- Windows AUMID / WinRT toast 跟 bundled identifier，不和正式版抢任务栏分组
- 会话/设置仍默认同一套 App data（`%APPDATA%\grokapp\grok-app`）；可用 `GROK_APP_HOME` 覆盖（`scripts/dev-white-icon.sh` 会指到临时目录）
- **不要**裸跑 `tauri dev` / `pnpm tauri dev`（没有 `--config` 会撞正式版单实例锁）

### macOS

App 的发行最低版本仍为 **macOS 11.0**。Computer Use 的**受管浏览器**单独要求 **macOS 14+**；较旧或无法核验版本时，不启动该浏览器并给出对应说明，不将此限制应用到普通聊天或其它 CU surface。两个架构的原生系统边界与维护 browser pack 验收尚未完成，见[最新检查点](plans/2026-10-13-computer-use-managed-browser-boundary-checkpoint.md)。不要为了新版 Playwright 全局抬高 App 的 `minimumSystemVersion`。

- Xcode Command Line Tools：`xcode-select --install`
- Apple Silicon 上构建 Intel：`rustup target add x86_64-apple-darwin`（脚本已处理）
- **Windows 交叉编译额外依赖**（`setup:cross` 会检查）：
  ```bash
  brew install llvm makensis
  cargo install --locked cargo-xwin
  # 建议把 clang-cl 放进 PATH（Apple Silicon）：
  export PATH="/opt/homebrew/opt/llvm/bin:$PATH"
  ```
- 首次 Windows 构建会下载 MSVC CRT/SDK 到 `~/.cache/cargo-xwin`（与 GrokGo 可共用缓存）

### Windows（原生）

- [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)（C++ 工作负载）
- [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/)（多数 Win10/11 已带）
- Rust MSVC toolchain：`rustup default stable-x86_64-pc-windows-msvc`

装好上述工具后，可双击仓库根目录 [`install-latest.cmd`](../install-latest.cmd)，把 **已合入的 `origin/main`** 打成并排安装的 **grok-app-latest**。这不是 GitHub Release，也不覆盖正式版 **Grok**。详见下文「Windows：并排安装已合入的 main」。

### Linux（含 Arch / Ubuntu / Debian）

```bash
# Debian/Ubuntu
# Prefer Ayatana only (libappindicator3-dev conflicts with libayatana-appindicator3-dev).
sudo apt install libwebkit2gtk-4.1-dev librsvg2-dev \
  patchelf libgtk-3-dev libayatana-appindicator3-dev libssl-dev \
  libpipewire-0.3-dev libspa-0.2-dev libclang-dev \
  build-essential pkg-config curl meson ninja-build python3-jinja2 xdg-utils

# Arch
sudo pacman -S webkit2gtk-4.1 base-devel curl wget file openssl appmenu-gtk-module \
  libappindicator-gtk3 librsvg libpipewire clang meson ninja python-jinja pkgconf
```

Linux App 直接依赖 Wayland crate（即使入口仍默认关闭），因此构建需要
**PipeWire/SPA ≥ 0.3.65、GTK ≥ 3.24，以及静态 libei ≥ 1.5**。
libei 旧版本的连接退出缺陷不能通过动态链接系统同 SONAME 库规避。

CU 受管浏览器还需要系统 NSS/NSPR 和 ALSA。deb 配置声明 `libnspr4`、`libnss3`、
`libasound2 | libasound2t64`，RPM 用相应五个64-bit SONAME capabilities；
构建机已有这些库不等于最终用户安装会有，不能依赖 browser resource 自动推导。
[维护版内部原生检查点](plans/2026-10-13-computer-use-maintained-native-checkpoint.md)
记录了 Jammy 缺库首败、私有环境补库及新02的28项通过；真实包安装/完整依赖审核仍待完成。

先在**全新私有目录**构建校验过 SHA-256 的 libei SDK，不覆盖系统库：

```bash
bash scripts/build-computer-use-libei.sh "$HOME/.cache/grok-cu-libei-1.5-sdk"
export PKG_CONFIG_PATH="$HOME/.cache/grok-cu-libei-1.5-sdk/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export LD_LIBRARY_PATH="$HOME/.cache/grok-cu-libei-1.5-sdk/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
bash scripts/check-computer-use-linux-sdk.sh
```

已有完整 SDK 可直接复用环境变量；builder 拒绝覆盖已有目录。发行时须保留
SDK 的 `share/licenses/libei/COPYING`。`LD_LIBRARY_PATH` 供原生夹具的 libeis 使用；
产品的 libei 必须静态链接，不能用这个变量假装修复发行物。

**Ubuntu 22.04 发布门槛仍未全部解决：** Jammy 官方包为 PipeWire 0.3.48，低于上述要求。
新增 `scripts/build-computer-use-pipewire.sh /absolute/new/sdk-directory` 可构建固定来源
及 SHA-256 的私有 PipeWire 1.6.9 + Debian1.6.9-2 安全补丁，不安装系统服务、不替换系统库。将该SDK和
libei SDK的 `lib/pkgconfig` 都加入 `PKG_CONFIG_PATH` 后再运行SDK检查。

维护版本已经在隔离Jammy/glibc2.35实际编译，并验证原/迁移目录实际私有模块装载、
contexts创建/退休，runtime清单的最高需求GLIBC2.34。新增48项guard/audit/runner回归
和8个实际装载负例通过；范围、来源补丁及失败记录见
[维护版SDK检查点](plans/2026-10-12-computer-use-maintained-sdk-checkpoint.md)。
旧1.0.5兼容性试验保留为历史证据，不再作为产品接线候选。

```bash
# 需patchelf；仅处理SDK副本，不替换系统库。
bash scripts/check-computer-use-pipewire-runtime.sh /absolute/PW-SDK /absolute/EI-SDK /absolute/NEW-OUTPUT
export GROK_CU_PIPEWIRE_RUNTIME=/absolute/NEW-OUTPUT/runtime
# Tauri before-build audits/copies this pack into generated native-pipewire resources.
```

该入口只输出客户端runtime的许可/SHA/ELF/ABI清单，并验证实际私有装载。
它不是完整App的安装或桌面验收。SDK支持显式目标：x64 使用 Ubuntu22.04 /
glibc2.35，ARM64 使用 fork 已声明的 Ubuntu24.04 / glibc2.39；两者的 ELF、
动态 loader、manifest 和产品加载策略分别核验，不能互相替换。
ARM64 checker 在第四个参数传 `aarch64-unknown-linux-gnu`，stager 传
`--target aarch64-unknown-linux-gnu`；checker 要求匹配的原生机器。
CI 同时保留 Jammy-x64 并加入 Noble-ARM64 原生 SDK / 产品 loader 矩阵；
CU 开发分支 push 也执行该门禁，远端结果按实际 run 记录。
**ARM64 受管浏览器资源仍未补齐，因此整套 ARM64 Release 尚不可交付。**

最新[产品私有loader检查点](plans/2026-10-12-computer-use-product-loader-checkpoint.md)：
DT_RPATH、安装/迁移/Cargo布局、实际库身份、16件资源和18个ELF正/负场景已验。
`release.yml` 已接入SDK准备及资源门禁，本地YAML解析通过，远端CI/release尚未运行。
[完整Jammy App续行](plans/2026-10-12-computer-use-jammy-app-build-checkpoint.md)记录原失败、平台修正及同一03正式候选：
完整构建/strict/导航9项与Cargo布局下的实际窗口、私有库身份、224ms正常关闭已验；
仍须正常聊天/所有CU lifecycle、AppImage/deb/rpm资源与依赖、真实安装和portal验证才可宣称可发布。
当前browser/Node seed仍旧，维护版晋升还须完成[编译合同检查点](plans/2026-10-12-computer-use-runtime-pins-checkpoint.md)
中的新布局/文件配额/系统底线/许可及完整四目标回归，不能借兼容性候选作为最终维护候选。
通用[归档/树额度合同](plans/2026-10-13-computer-use-runtime-contract-checkpoint.md)已统一为编译/审计同一JSON，
Chromium默认1024文件/2048条目/600MiB单件/1GiB总树；prepare不另覆盖。锁/manifest不能扩大额度，
失败须保留旧seed/恢复证据。容量适配及当前旧seed回归不等于最终来源/通知/维护版prepare通过。
独立loader/SDK的通过不替代这些门禁。
[公共运行源/许可分类检查点](plans/2026-10-13-computer-use-runtime-license-classification-checkpoint.md)记录36固定来源与剩余缺口；
94项审查工具测试不代替完整NOTICE/实际binary覆盖。当前四官方157快照仍只有sample Credits，不晋升维护runtime。
后续[LLVM/Windows支持源](plans/2026-10-13-computer-use-support-license-checkpoint.md)已补到47固定来源/99工具测试；
25个yes错误、165目录错误与4异常Shipped仍保留，仍不替代完整NOTICE/最终四目标prepare与原生安装验收。
最新[公共浏览器源/ANGLE递归目录](plans/2026-10-13-computer-use-browser-recursion-checkpoint.md)已补到57固定来源/113工具测试；
19个yes错误、187目录错误与5异常Shipped仍保留，准确递归pins不能替代实际builder继承/binary覆盖。
完整NOTICE与维护runtime晋升仍未通过，macOS仍App11+/CU受管14+，原四目标prepare/原生安装门禁不变。
后续[gclient机制/公开build入口](plans/2026-10-13-computer-use-gclient-build-evidence-checkpoint.md)已核验固定helper源码优先级、
OpenScreen25 Git/7 non-Git并封存361条CI，140工具tests通过；四archive IDs已定位，
真实builder参数/发布对象generation/hash/完整通知仍需验证，不从sample分支或SUCCESS推导晋升。
最新[实际checkout/固定GCS对象](plans/2026-10-13-computer-use-checkout-object-checkpoint.md)已采四完整manifest/bootstrap，
原四ZIP的SHA256/MD5/CRC32C与固定generation一致；Windows24.062ms时间差保留，非publisher认证。
192工具tests/0skip通过不替代完整NOTICE/实际CIPD helper或生产pack；产品pins/四锁/seed仍未晋升。
后续[六个实际递归仓库](plans/2026-10-13-computer-use-checked-recursive-checkpoint.md)按四manifest补齐六固定源，
63源7273路径/439 raw gaps、19 yes错/190目录错/5异常值；206工具tests/0skip与独立重算通过。
这是来源输入增量，不排除原缺项或晋升维护pack；完整NOTICE、执行CIPD helper及原R1–R4仍需完成。
最新[实际recipe/helper/config](plans/2026-10-13-computer-use-recipe-provenance-checkpoint.md)核验原recipe ZIP/SHA、
包内helper/Git bytes、四log布局/DEPS fallback AST及四config/Git blobs/32 bootstrap keys；236工具tests通过。
这不替代平台license coverage/完整NOTICE、远端运行时测量或最终production pack；原维护版晋升门禁保持。
后续[四平台来源/生成输入](plans/2026-10-13-computer-use-platform-coverage-checkpoint.md)联合重验主树+63源与原manifest，
准确匹配59/62/56/56、冲突0；439 raw gaps/19 yes错/190目录错/5异常值未排除。
八准确Crashpad/node源已采，固定GCS生成输入与Lit锁/README版本差异实读；258工具tests/0skip通过不等于完整NOTICE。
下一批校验固定生成对象/实际分支与完整通知后，再晋升最终维护pack；App11+/受管Mac14+及原R1–R4全部门禁不变。
最新[Khronos 完整许可正文](plans/2026-10-13-computer-use-khronos-notice-checkpoint.md)已绑定七SPDX声明/一原内嵌permission，
两固定标准全文11358 bytes与八原版权声明实核，380工具tests/0skip及offline复验通过。
这只是八文件通知片段；完整四平台NOTICE/最终维护pack与原R1–R4尚未完成，App11+/受管Mac14+保持。
最新[复制API通知与省略记录](plans/2026-10-13-computer-use-notice-supplements-checkpoint.md)收齐38API头文件，
原版权/完整短声明/permission与完整许可绑定；17省略记录复核为10配置许可、3复制API/registry来源未证、4义务未解决。
411工具tests/0skip通过；39880-byte补充仍partial，不用于发行，不替代完整NOTICE/SBOM与原R1–R4。
App11+/CU受管Mac14+与产品旧runtime/pins/四锁/seed保持，下一批真实copied-generated输入及四义务仍需完成。
前批[默认通知候选与完整 ANGLE GN 来源](plans/2026-10-13-computer-use-notice-candidates-checkpoint.md)补齐实际WebGL全文，
495有效候选全文缺口归零，独立partial preview为5453606 bytes；17省略记录/196目录问题/5异常值仍开放。
全部162 ANGLE GN原文与399工具tests通过，未证明完整通知/binary排除；预览不得用于发行。
最终维护pins/四锁/production pack及原R2–R4全部门禁保持，App11+/CU受管Mac14+与当前旧产品不变。
后续[ANGLE CPU候选源](plans/2026-10-13-computer-use-cpu-consumer-source-checkpoint.md)新增两准确GN/C++输入，累计381工具tests通过；
候选引用/独立Android条件只作来源证据，全部CPU消费图/二进制排除未证明，完整维护pack门禁保持。
前批[真实 C++ 来源与 Khronos 通知](plans/2026-10-13-computer-use-consumer-checkpoint.md)已验三主C++库/四manifest/原license字节、
11源码规则和8copied headers逐文件Apache/MIT/完整permission及platform引用；369工具tests/独立01/02同SHA通过。
7554paths/422 raw gaps/17 yes错、196目录错/5异常值保留；七SPDX声明完整文本/其它生成源/完整NOTICE与原R1–R4仍需完成。
App11+/受管Mac14+与产品/pins/四锁/seed未晋升，source审查通过不替代最终pack/native验收。
前批[ANGLE / OpenScreen 集成](plans/2026-10-13-computer-use-integration-checkpoint.md)已验generated flag导出/helper AST、
16准确source rules/四原GN参数、四条实际路径许可映射与Perfetto四manifest pin；357工具tests/独立04/05同SHA通过。
7541paths/422 raw gaps/17 yes错、196目录错/5异常值保留；完整NOTICE/R1仍红，继续其它消费路径/生成输入与原R2–R4全部门禁。
App11+/受管Mac14+与产品/pins/四锁/seed未晋升；审查工具通过不替代production pack/native验收。
前批[Crashpad 配置与 lss 来源](plans/2026-10-13-computer-use-crashpad-checkpoint.md)已验根default/四原GN参数、
external lss/zlib分支和真实路径许可，319审查工具tests/独立重算通过；7518paths/422 raw gaps/17 yes错、
190目录错/5异常值保持，不排除standalone缺项。完整NOTICE/R1仍红，不晋升产品/四锁/seed；
App11+/受管Mac14+与原R2–R4所有门禁保持active/partial。
前批[固定生成依赖闭包](plans/2026-10-13-computer-use-node-generated-checkpoint.md)已验原GCS对象/generation/hash、
完整6544 entries/234source paths与原recipe布局/四actual checkout同-object段；7507paths/422 raw gaps/17 yes错，
190目录错/5异常值保留。296工具tests/0skip通过不等于完整NOTICE/实际二进制验收；继续准确lss/Crashpad分支、
ANGLE/OpenScreen/其它生成输入与完整R1，再晋升维护pack，原R2–R4与App11+/受管Mac14+全部保持。
不能改为 24.04、伪造 pkg-config 版本、只复制新头文件，或禁用 Wayland 依赖来制造绿灯。
后续[四平台实际archive/GN参数](plans/2026-10-13-computer-use-archive-build-checkpoint.md)已封存4条完整GetBuild和16原日志，
同source/pins/上传路径实核，actual GN args与同源默认确认sample机制；169审查工具tests通过不替代完整NOTICE。
GCS generation/下载ZIP bytes关联、实际checkout/递归与完整通知仍需补证，不从publisher路径或custom vars推导晋升。
产品/pins/四锁/seed/App144保持，App11+/CU受管Mac14+与原R2–R4全部门禁不变。
私有较新 SDK 的 `cargo check` 通过，不等于这一发布门槛已通过。

通过 SDK 检查后：

```bash
pnpm build:linux
# 或
./scripts/build-local.sh linux
```

产物：`src-tauri/target/x86_64-unknown-linux-gnu/release/bundle/`  
（`appimage/`、`deb/`、`rpm/` —— 对应 Ubuntu/Debian 系与 Fedora/RHEL 系）。

构建 RPM 需要 `rpm` 工具：`sudo apt install rpm`（Debian/Ubuntu CI 已装）。

#### 已知问题：AppImage 黑屏（AMD + Hyprland / Wayland）

AppImage 内置 Ubuntu 22.04 CI 容器的 WebKitGTK。部分 AMD + Hyprland 环境会报  
`Could not create default EGL display: EGL_BAD_PARAMETER`，窗口全黑，但宿主进程（媒体、ACP、登录）仍正常。

Issue [#539](https://github.com/RongleCat/grok-app/issues/539) 对照实验：相同 env 下仅换用**系统 WebKit** 即可恢复 UI——根因是**内置 WebKit 的 EGL 栈**，不是应用业务代码。`.deb` / `.rpm` 链接系统 WebKit，不受影响。

用户侧缓解：各 README 的「Linux blank/black window」段，或仓库脚本  
`scripts/run-linux-appimage-system-webkit.sh`。

仓库保留 `src-tauri/src/linux_webkit.rs` 的 host-WebKit/退出辅助实现，但当前未在产品入口注册，不能依赖它自动解决黑屏或FUSE/helper退出问题。外部wrapper也须独立核对私有PipeWire优先级、实际helper归属和挂载生命周期；SDK/loader探针通过不证明此项已完成。不要为了此问题单独把 Linux CI 升到 Ubuntu 24.04——会抬高 glibc 底线。

## 2. 本地构建命令

```bash
pnpm build:help         # 打印全部平台命令说明

# —— 按你的电脑（最常用）——
pnpm build              # 当前主机自动选 target（Apple Silicon → aarch64）
pnpm build:mac          # 同上，仅 macOS：为「这台 Mac」打 .app + .dmg

# —— 指定平台 ——
pnpm build:mac-arm      # macOS Apple Silicon（aarch64-apple-darwin）
pnpm build:mac-intel    # macOS Intel（x86_64-apple-darwin）
pnpm build:mac-all      # ARM + Intel 两套 Mac 包（仅 macOS 主机）
pnpm build:win          # Windows x64 NSIS（本机 Windows，或 Mac/Linux 上 cargo-xwin）
pnpm build:linux        # Linux x64 AppImage/deb/rpm（需 Linux 主机）
pnpm build:all          # mac-arm + mac-intel + win（仅 macOS 主机）

# 或直接脚本：
./scripts/build-local.sh mac          # 当前 Apple 芯片 / Intel Mac
./scripts/build-local.sh mac-arm
./scripts/build-local.sh win
./scripts/build-local.sh linux
./scripts/build-local.sh all
```

### Windows：并排安装已合入的 main

给**已经有编译环境**、等不及下一版 GitHub Release 的人：把当前仓库 fast-forward 到 `origin/main`，打一份**未签名** NSIS，静默装到 `%LOCALAPPDATA%\grok-app-latest`。

```bat
install-latest.cmd
```

或：

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/install-latest.ps1
```

行为：

- 要求 **tracked 工作区干净**；`git fetch origin --tags` 后把 `main` **fast-forward** 到 `origin/main`（当前在别的分支也会切走）
- Settings → About：HEAD 恰好是 `vX.Y.Z` tag 时显示该版本，否则显示 short commit hash（不要把 main 误当成上一 GitHub Release）
- 临时 Tauri overlay：`productName=grok-app-latest`，`identifier=com.grokapp.desktop.latest`（single-instance mutex 与正式版隔离）
- 只打 NSIS（`--bundles nsis --no-sign --ci`），`/S` 静默安装
- **不覆盖** 正式 **Grok** 安装目录；开始菜单多一项 `grok-app-latest`
- 会话/设置仍走同一套 App data（`%APPDATA%\grokapp\grok-app`，可用 `GROK_APP_HOME` 覆盖）——不要和正式版同时当写入端开着
- 本机 NSIS 仍会注册 `grok://` / `.grokskin`（后装的覆盖系统关联）
- 不是 nightly CI；没有 macOS / Linux 对等脚本

`origin` 应指向你想跟上的 GitHub 仓库（贡献者通常是 `RongleCat/grok-app`）。

### Apple Silicon 本机安装包（示例）

在 M 系列 Mac 上：

```bash
pnpm install
pnpm build:mac
# 或
pnpm build:mac-arm
```

成功后脚本会列出产物，典型路径：

```
src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/Grok_*.dmg
src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Grok.app
```

双击 `.dmg` 安装，或直接运行 `.app`。  
通知 / Dock 等依赖正式 `.app` 的能力在 **安装包或 `.app` 产物** 上验证，不要用 `tauri dev` 裸二进制当生产行为。

`build:win` 在 macOS 上等价于：

```bash
pnpm exec tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc
```

产物目录：

```
src-tauri/target/<triple>/release/bundle/
  macos/     # .app / .dmg  （产品名 Grok）
  nsis/      # Windows 安装版（*-setup.exe）
  deb/       # Debian/Ubuntu .deb
  rpm/       # Fedora/RHEL .rpm
  appimage/  # 通用 Linux AppImage
src-tauri/target/release/grok-app.exe       # Windows 本机主 EXE；绿色版还必须包含 resources
```

拷贝测试建议：

```bash
mkdir -p dist-installers
cp src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/*.dmg dist-installers/
cp src-tauri/target/x86_64-apple-darwin/release/bundle/dmg/*.dmg dist-installers/
cp src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/*-setup.exe dist-installers/
# Windows 本机绿色版：完整资源校验、打包并解包回读（本地默认不上传）
# bash scripts/package-windows-portable.sh vX.Y.Z
cp src-tauri/target/x86_64-unknown-linux-gnu/release/bundle/appimage/* dist-installers/ 2>/dev/null || true
cp src-tauri/target/x86_64-unknown-linux-gnu/release/bundle/deb/* dist-installers/ 2>/dev/null || true
cp src-tauri/target/x86_64-unknown-linux-gnu/release/bundle/rpm/* dist-installers/ 2>/dev/null || true
```

Windows 绿色版不能仅压缩主 EXE。先以当前源码执行 `node scripts/prepare-computer-use-runtime.mjs --prepare --target x86_64-pc-windows-msvc`，再在 Windows 构建机运行 `bash scripts/package-windows-portable.sh vX.Y.Z`。脚本要求构建机具备 Node、Rust、7-Zip；用户解压后的 Computer Use 运行时不依赖系统 Node/Rust。它按 `tauri.windows.conf.json` 原样携带 Windows seed，保留入口名 `Grok.exe`，校验源和已复制 seed，并对 ZIP 解包后逐文件验证 `portable-files.json`。该文件仅是包内完整性清单，**不是发布者签名**。

CI 的 Windows build 不传 `--target`，默认主文件为 `src-tauri/target/release/grok-app.exe`。若使用 `pnpm build:win` 的显式 target 输出，在 Windows 上打本地包时指定 `GROK_PORTABLE_EXE=src-tauri/target/x86_64-pc-windows-msvc/release/grok-app.exe`；不会扫描目录并猜测其他 EXE。运行时校验在独立的构建暂存目录执行，再按资源映射复制进交付目录，校验产生的 mutation lock / owner sidecar 不入包。

默认产物是根目录 `Grok_X.Y.Z_x64-portable.zip` 和 `dist-portable/Grok_X.Y.Z_x64-portable/`；已有目标拒绝覆盖，不清空整个 dist-portable。`GROK_PORTABLE_EXE` / `GROK_PORTABLE_OUTPUT` 可用于明确的本机构建或隔离探针，设置这些覆盖项时拒绝 `--upload`。只有发布 CI 显式传入 `--upload` 才会上传。用户须保留整个目录和 resources；便携版本的人工替换/回退以完整版本目录为单位。安装器更新仍使用其安装布局，不能把 ZIP 往返验证当作便携版自动更新或安装完成的验收。

## 3. GitHub Actions 发布（推荐）

> **AI / 维护者完整 checklist：** [docs/llm-wiki/release.md](./llm-wiki/release.md)（发版步骤、CHANGELOG 写法、损坏处理、禁止事项）。

工作流：`.github/workflows/release.yml`  
发版说明：从 `CHANGELOG.md` 对应版本章节生成（`scripts/changelog-for-release.py`），**仅本版变更**；安装 / Gatekeeper 见 README。

### 触发方式

1. **推送版本 tag**（推荐、稳定）  
   ```bash
   # 1) 先在 CHANGELOG.md 写好 ## [X.Y.Z] - YYYY-MM-DD
   # 2) 提交干净 main 后：
   ./scripts/release-tag.sh 0.1.1
   # 或直接推送：
   ./scripts/release-tag.sh 0.1.1 --push
   ```
2. **Actions → release → Run workflow**（手动）

`release-tag.sh` 会：

- 校验 `CHANGELOG.md` 存在该版本章节（否则失败）
- 同步 `package.json` / `tauri.conf.json` / `Cargo.toml`
- 提交 `chore: release vX.Y.Z` 并打 annotated tag
- 可选 `--push` 触发 CI

没有对应 CHANGELOG 章节时，**tag 与 CI release 都会失败**（有意为之）。

### 仓库设置

- **Settings → Actions → General → Workflow permissions**  
  勾选 **Read and write permissions**（用于创建 Release 并上传资产）

### 可选：签名 Secrets

未配置签名时仍会出包；macOS 可能提示「已损坏」，Windows 可能 SmartScreen 拦截。

| Secret | 用途 |
|--------|------|
| `APPLE_CERTIFICATE` | Developer ID Application `.p12` 的 base64（`openssl base64 -A -in cert.p12`） |
| `APPLE_CERTIFICATE_PASSWORD` | 上述 `.p12` 的导出密码 |
| `APPLE_SIGNING_IDENTITY` | `security find-identity -v -p codesigning` 引号内整串，如 `Developer ID Application: Name (TEAMID)` |
| `APPLE_TEAM_ID` | 10 位 Team ID |
| `APPLE_API_ISSUER` | App Store Connect Issuer ID（UUID） |
| `APPLE_API_KEY` | App Store Connect Key ID |
| `APPLE_API_KEY_P8` | `AuthKey_<id>.p8` 全文（CI 写成临时文件再设 `APPLE_API_KEY_PATH`） |
| `WINDOWS_CERTIFICATE` | **Windows Authenticode** `.pfx` 的 base64（`certutil -encode` 或 `[Convert]::ToBase64String`） |
| `WINDOWS_CERTIFICATE_PASSWORD` | 上述 `.pfx` 的导出密码 |
| `GROK_UPDATER_PUBLIC_KEY` | 应用内自动更新公钥（与 endpoint 一起嵌入 release 构建） |
| `TAURI_SIGNING_PRIVATE_KEY` | Tauri updater 签名私钥（启用自动更新时必需） |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 私钥密码（可为空） |

#### Windows Authenticode（正式代码签名）

- **与 Tauri updater minisign 不是一回事**：`TAURI_SIGNING_PRIVATE_KEY` 只签更新包，**不能**消除 SmartScreen「未知发布者」。
- 需要 **代码签名证书**（OV / EV；SSL 证书无效）。EV 通常立刻建立信誉；OV 可能仍短暂显示 SmartScreen，直到证书信誉积累。
- Release CI（`windows-latest`）在两个 secret **都非空** 时会：
  1. 把 base64 解成 `.pfx` 并 `Import-PfxCertificate` 到 `Cert:\CurrentUser\My`
  2. 写出 `src-tauri/tauri.windows.sign.conf.json`（`certificateThumbprint` + sha256 + DigiCert 时间戳）
  3. `tauri build --config …/tauri.windows.sign.conf.json` 让 bundler 调用 `signtool`
- **不要**在 GitHub 填入空的 `WINDOWS_*` secrets；缺省即跳过签名，构建仍成功。
- 本地 Windows 签名：导入 PFX 后按 [Tauri Windows signing](https://v2.tauri.app/distribute/sign/windows/) 配置 thumbprint，或复用同一 merge config。

应用内自动更新详情见 [desktop-auto-update.md](./desktop-auto-update.md)。  
Release CI 在 secrets 齐全时会生成 `tauri.release.conf.json` 并注入 `GROK_UPDATER_*`。

**注意：** 不要在 workflow 里传入**空**的 `APPLE_*` secrets，否则 codesign 导入会失败。

### Release 内容

矩阵会为以下平台上传安装包到同一 GitHub Release：

- macOS ARM64  
- macOS x64  
- Windows x64  

Release body = 下载表 + 该版本 CHANGELOG + 安装说明（含 `xattr`）。

## 4. 版本号约定

保持一致（`release-tag.sh` 会自动改）：

- `package.json` → `version`
- `src-tauri/tauri.conf.json` → `version`
- `src-tauri/Cargo.toml` → `[package].version`

About 页版本（`app.versionFooter` 的 `{version}`）在 Vite 构建时注入：恰好是 `vX.Y.Z` release tag 则显示该 tag，否则显示 short commit hash。

Tag 格式：`v0.1.1`（前缀 `v` + semver）。

## 5. 故障排查

| 现象 | 处理 |
|------|------|
| CI “Resource not accessible by integration” | 打开 workflow 写权限 |
| release job：no CHANGELOG section | 补 `## [X.Y.Z]` 后再 tag |
| macOS Intel build 缺 target | 确认 rustup 安装了 `x86_64-apple-darwin` |
| Windows 交叉缺 makensis / clang-cl | `brew install makensis llvm`；`export PATH="$(brew --prefix llvm)/bin:$PATH"` |
| cargo-xwin 首次很慢 | 正常：在拉 CRT/SDK；缓存目录 `~/.cache/cargo-xwin` |
| Windows 本机无法交叉 | 用 `pnpm build:win`（cargo-xwin）或 CI |
| macOS 下载后打不开 | 未签名：系统设置 → 隐私与安全性 → 仍要打开；或 `xattr -cr /path/to/Grok.app` |
| Windows 找不到 grok CLI | 安装 Grok Build 并确保 `%USERPROFILE%\.grok\bin` 或 PATH 上有 `grok.exe` |

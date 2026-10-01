# Computer Use：原生 parent 与 PortalRegistry 全程所有权

日期：2026-09-30。状态：**active / partial — not releasable**。
本阶段是实际代码/构建/原生回归进展，不是最终版完成声明。

## 本次实现

- core 增加 `NativeParent` 与保留式 UI dispatch 接收边界；
  `AuthorizationTicket::same_preparation` 同时验证真实共享 preparation 身份、nonce 和公开字段，
  不把字段相等的另一票据当成原操作。
- GTK `ParentLease` 实现该边界。PortalRegistry **先登记 Pending 占用，再调度原 UI 导出**。
  调用者取消或 deadline 到达只撤销权限、进入 Closing，不抛弃已排队的原任务，
  不重新创建 parent、不提前返回 idle。
- 晚到 lease 先交给原 entry 保留，再验证票据/handle。拒绝、Portal 错误、Stop、Drop 均请求原
  GTK owner 释放；native owner、UI release 与原 worker join 未证实时不清空占用。
  UI reply 丢失不是「没有发生导出」的证据，仍保留 unknown/unproven，而不是伪造 Closed。
- 输入 policy 在底层回调前后同步检查 parent 撤销；窗口 unmap 后已有 adapter 也立即拒绝输入，
  不等待轮询才失效。原生回调不在 registry mutex 内执行。
- App Tauri 主线程桥改为通过上述 registry API 排队；移除会丢弃未知 UI 工作的旧等待边界。
  **consent command/picker 尚未调用这一完整桥，Linux factory 仍为 X11，native_wayland=false。**

`ParentLease::is_closed` 只证明本地 GDK export 引用已经释放；不冒充远端 compositor acknowledgement，
更不等同于 Portal、PipeWire 或 EI 已完成回收。各层仍有各自的真实 join/retirement 证据。

## 实际发现和修复的构建问题

第一次 parent-registry 候选的实际 Linux App 编译失败：
`linux_parent.rs` 导入 Wayland crate，但 App `Cargo.toml` 没有直接 Linux 依赖。
已补齐该依赖；对应本次 lock 增量只有 App dependency list 的一项。
没有以关闭模块、仅编独立 crate 或复用修复前的通过日志掩盖问题。

随后完整 App 的 `cargo clippy --all-targets -- -D warnings` 发现三项此前独立库检查未覆盖的问题：
浏览器 supervisor 的测试专用 `pid`/`shutdown_with` 在 Linux 测试构建中无使用者，以及命令测试的
singleton slice 冗余 clone。前两项改为精确的 Windows test 或跨平台 probe feature 条件，
没有禁用 Linux probe；后一项改为 `std::slice::from_ref`。保留失败日志，再次冻结当前源码，
重跑完整 Linux App 严格检查，并补跑 Windows 原进程所有权/失败回收的 7 项测试。

直接依赖暴露了 CI 的准备顺序错误：原来在 App Clippy/Test **之后** 才构建 libei。
现将唯一一次私有 libei SDK 构建提前，通过 `GITHUB_ENV` 传递环境，运行真实 SDK preflight。
Wayland build.rs 显式要求 PipeWire ≥ 0.3.65（上游 sys crate 仅检查 ≥ 0.3），
继续要求静态 libei ≥ 1.5；没有动态降级。

新增公共原生测试运行器，取代只在 `.run` 目录存在的临时启动方法：

- 强制独立 PID/network/mount namespaces、私有 `/tmp` 与 loopback；拒绝绕过该入口直接运行。
- 原始环境中的 display、Wayland FD、session bus、Xauthority 均不作为桌面来源；使用自有 headless labwc。
- 全量运行 `--include-ignored`；拒绝零用例、filtered/ignored「绿灯」，必须包含真正的 GTK→Registry 联测。
- 保留原始 Wayland wire、C EI events、PNG 所在报告、独立 daemon/source 日志与 child cleanup receipt。
- 持有并 join 原始 compositor 和 test child；超时先终止、再对同一 owner 升级 kill/wait；
  不把 deadline 当成功，不重新启动未知状态任务。PID 1 回收原 namespace 内的 orphan。
- CI 仅在必要时提权运行 namespace launcher，不关闭全局 AppArmor/sysctl；失败证据亦上传。
  CI YAML 与顺序已本地核对，**尚未声称 GitHub hosted job 已运行通过**。

## 同一冻结候选的证据

根目录：`tools/computer-use-probe/.run/wayland-parent-registry-final2-20260930/`。
`source-freeze.json` 覆盖 **344 文件**；相对上一已封存的 334 项，16 项变化、10 项新增审计覆盖。
后者包含原已存在但本次纳入冻结的 `docs/BUILD.md`、`scripts/build-local.sh`，不等于新建十个功能文件。

| 检查 | 本候选结果 | 边界 |
| --- | --- | --- |
| Wayland 全量库/原生套件，同一 binary | **105 × 3，0 failed / 0 ignored / 0 filtered** | 线程数 1/4/4；私有 Portal + PW/EI + labwc，不是 GNOME |
| GTK parent→registry 原生联测 | 每轮 3 场景；2 exports / 2 destroys | 排队前取消、晚到 lease、真实输入后 unmap 同步拒绝 |
| 独立 GTK native probe，同一 binary | 每轮 **12 Wayland + 4 X11 负例**，三轮 | 每轮 9 exports / 9 destroys；原 labwc/Xvfb 精确 join |
| PW/EI 原始夹具归档 | 每轮 55，共 **165**，缺失/owned residual 0 | 独立解析 PNG、原 C events、peer/run/generation 与 actual parent |
| Linux / Windows core | **586 / 587** | 平台条件用例数不同 |
| Linux / Windows driver | **16 / 16** | 实际原私有 worker contract |
| Linux / Windows macOS wait FFI contract | **21 / 21** | 是 FFI double，不是 macOS 原机验收 |
| App 命令 / WebView / session-MCP / feature / browser supervisor | **4 / 69 / 21 / 1 / 7，共 102** | Windows 原 App test artifact 的独立 manifest 副本；未修改原 EXE；7 项覆盖同一原进程所有者回收 |
| X11 unit | **49** | 本阶段未重跑 X11 native 19 / AT-SPI 41；旧记录仍归旧阶段 |
| Linux / Windows 实际 App cargo check | **通过 / 通过** | Linux 使用正确独立 seed 的资源 overlay，不覆盖 Windows seed |
| 四原生 crate 严格联合 Clippy | **通过，-D warnings** | core / Wayland / X11 / GTK parent，all-targets |
| Linux 完整 App 严格 Clippy | **通过，-D warnings** | all-targets；前候选三项失败保留，修复后重新冻结、完整回归 |
| 运行器 / SDK guard contracts | **16 通过** | 其中 mock SDK 测试不是原生库验收 |
| 真实运行器超时负例 | **拒绝成功，原 test child SIGKILL 后 wait，残留 0** | 自有 inert hung test，不是业务输入效果证明 |
| fmt / include-command fmt / diff check | **通过** | 冻结后再次核对源码哈希 |

本地实际 SDK：PipeWire 1.4.2 / GTK 3.24.49 / libei 1.5.0。
native test executable 的 ELF `NEEDED` 无动态 libei，执行前后复制产物哈希一致。
`verify-native.mjs`、`verify-parent.mjs` 核对原始协议；`seal.mjs` 与独立 Python verifier
覆盖当前源、文档、产物以及失败尝试的文件哈希；结果以相应日志和 `receipt.json` 为准。

## 保留失败，不改写历史

- 初始 100 pass / 1 fail 的 `Network is unreachable`：私有 network namespace 的 loopback 未启用。
  只在自有 namespace 中 `ip link set lo up`；原日志保留，重新跑全套，不改断言。
- 初始 locked dependency 拒绝、Clippy collapsible-match 报错、Linux App unresolved import 均保留。
  本次重新冻结并跑当前候选，不将修复前结果计作修复后证据。
- 初期 Rust 可见性/orphan-rule 编译错误曾由工具直接输出，但首个编译日志在封存前已被后续检查覆盖；
  不声称这些初期编译错误有完整原文件归档。
- `wayland-parent-registry-20260930/`、`wayland-parent-registry-build-20260930/` 与
  `wayland-parent-registry-final-20260930/` 是历史尝试；第三个包含完整 App 严格 Clippy 的三项失败，
  不是当前候选。当前收据另列它们的文件哈希，不修改旧 freeze。
- 更早 Windows staging rename access denied、单次即时 PipeWire node snapshot、首个 parent residual PID
  身份未证等问题仍按各自原始记录保留；后续未复现不能冒充根因修复。
- 本次未提交、推送、tag、发布、签名或安装产品；未触及用户真实桌面输入。

## 新发现的发布门槛与完整剩余范围

**Ubuntu 22.04 release 仍未打通。** 已保存官方 Jammy 包页面，实际版本为 PipeWire 0.3.48，
低于库需要的 0.3.65。必须在原 glibc 底线上构建/验证私有 PipeWire SDK、模块和
AppImage/deb/rpm 运行时依赖/许可证，不能提升到 Ubuntu 24.04、伪造 pkg-config、
只复制新头文件或禁用 Wayland 依赖换取通过。现代 SDK 下 App check 通过不是发行包验收。
实际 ELF 依赖审计发现当前私有 SDK 要求 **GLIBC_2.38**；已保存逐库 `readelf` 原输出，
进一步证明不能将本机 SDK 产物冒充 Ubuntu 22.04 的兼容运行时。发行基线没有上调。

以下全部保持原目标范围，**没有缩成仅协议兼容或仅私有夹具可用**：

1. App 原生 consent command/picker、factory routing、真实 OS 权限/session lock/用户接管/恢复，及安装态 GNOME 验收。
2. Windows x64、macOS arm64 + Intel、Linux X11 + native GNOME Wayland 全平台实机矩阵。
3. Desktop、managed browser、existing Chrome + Edge、App WebView，经 App / ACP / MCP 的完整能力。
4. 全输入、中文 IME、clipboard、取消与恢复；所有 native UI 的窄窗口/DPI/权限状态。
5. 签名 clean install/update/repair/rollback/uninstall 与 Linux baseline 包装。
6. 真实 Grok E4，以及**同一最终冻结候选**的 12h active soak。

因此不能调用 `update_goal(complete)`。本检查点没有将原目标暂停、缩小或标为 blocked。

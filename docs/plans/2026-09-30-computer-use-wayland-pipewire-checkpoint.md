# Computer Use — Wayland PipeWire 实际帧采集检查点

日期：2026-09-30。完整目标仍为「接手 grok 的工作，完成 computer use 所有功能开发，推进到最终版」。
当前状态 **active / partial — not releasable**；本检查点不是把最终版重新定义为 PipeWire 子模块。
上轮和本轮均为 progress；本轮直接修改生产 Rust，执行真实本地协议及像素验收，不是状态重述或等待。

## 本轮落地

- `computer-use-wayland/src/capture.rs`：生产 `PortalSession::start()` 接入真实 PipeWire consumer。
  只使用 portal 发下来的 Unix fd；不打开默认 socket、默认 camera 或 XWayland。
  初始 registry roundtrip 核验 node / portal serial，随后以观察到的唯一 serial 固定 target.object。
  `node.dont-reconnect=true`；目标移除、协议错误和原 portal/EIS 生命周期失效时关闭原消费者。
- `frame.rs`：CPU 映射 buffer → 独立持有的 RGBA；RGB/BGR/RGBA/BGRA/RGBx/BGRx、chunk offset、
  行 padding、正负 stride、VideoCrop、大小与边界检查；每帧保留 run/node/serial、序列与 format generation。
  协商 CPU MemFd/MemPtr，拒绝把 DMA-only/modifier 当作 CPU 指针；8192/边、256 MiB/plane 上限。
- 格式改变先清除旧图；真实 resize 后 generation 更新。空/损坏/暂停/超过两秒的图不作为可用观察，
  不填黑图或默认图。portal Granted 不等于图像已经可用，更不等于 libei input ready。
- 取消立即撤销 frame store；native owner 的真实 join 完成后才发布 Closed。
  取消一次 Stop 的调用者等待不能把 native handle 丢掉；不同 run 不共用 frame authority/retirement。
- Linux 条件依赖固定 `pipewire = 0.10.1`。相对本轮开始的 Cargo.lock，新增 19 包，
  原 674 包的版本和 checksum 都未改变。Windows App 不引入这个 Linux 动态库。
- CI 增加 PipeWire/SPA/libclang 编译依赖、严格 Clippy 和显式私有 PipeWire C source 验收步骤。
  **没有运行远端 CI、提交、推送、打 tag 或发布。**

## 原生证据与边界

本机 WSL Debian 13，隔离解包 Debian PipeWire **1.4.2-1** SDK/daemon；没有全局 apt install、
启动桌面服务或改系统 PipeWire 配置。每个夹具有独立 0700 runtime、私有 daemon/socket/C video source，
portal 仍由私有真实 D-Bus 测试服务提供，fd 通过 SCM_RIGHTS 传入生产 consumer。

实际证明：
1. 真正映射到生产 consumer 的 MemFd 图像每个像素和连续帧发生预期变化，而非伪造图片。
2. native VideoCrop `(2,2,28,20)` 与 padding/offset 正确；32×24 → 48×32 后新 crop/generation 正确。
3. Stop、portal 撤销、source 消亡、daemon 消亡均撤销图像并回收消费者；Closed 后 registry 不留 capture node。
4. 实际 registry 中错误 serial 被拒绝，不创建 capture node。
5. 两个真实消费者的 run-tagged 图独立；Stop 第一条不影响第二条；Drop 第二条回收 native owner。
6. 取消 Stop 等待后再次等待仍跟踪原 owner，不能提前 idle。

这些是 **E2 / 实际 PipeWire native-IPC 与像素证据**，不是 GNOME 原生桌面 E3/E4。
夹具手动链接私有 source/sink，不代表 GNOME session-manager/portal 自动连接已验收。
EIS 端仍是 socket 生命周期夹具，**没有实际 libei 输入**。

## 本轮发现并修复的验证缺陷

- 初始 SDK 缺少 clang 内置头，补隔离 `libclang-common-19-dev` 并设 resource-dir 后编译通过。
- resize 第一次失败：C fixture 在 PipeWire 建线程后才处理 SIGUSR1，未屏蔽的线程可被信号终止。
  修复为 pw_init 前屏蔽并由 loop signalfd 接收，实际 format renegotiation 随后通过。
- 发现前阶段 SlowCleanup 测试在 zbus async-io executor 内用 Tokio sleep，后台 panic 仍可能显示 test passed。
  旧的绿色计数不能证明该等待路径。改成 executor 无关的真实 pending handler，并对清理增加至少 900ms 的断言。
  最终三轮全部 **无后台 panic / 无 FAILED**。旧证据不覆盖、初期失败/伪绿色日志保留。

## 最终冻结回归

冻结 **302** 个选定源文件；相对上阶段 298 个源，8 个既有文件变化、4 个新增，其他选定源未变。
完整列表在 `source-change-audit.json`，冻结后重验没有漂移。

- 同一个 joint-feature 编译得到的确切测试可执行文件，归档后直接运行三次（线程 1、4、4）：
  **每次 24 passed / 0 failed / 0 ignored**，包含全部 3 个显式 native tests；不是只跑过滤后的正例。
- Wayland + X11 / native-probe 联合严格 Clippy `-D warnings`：通过。
- X11 单测 **49 passed / 0 ignored**；owned Xvfb **19 PASS 组**；GTK/AT-SPI **41 PASS 组**。
- Windows `cargo check --offline --locked -p grok-app --lib --jobs 4`：通过。
- workspace fmt、diff check：通过。没有本轮前端/完整 Core/真实安装验收的宣称。
- 归档 58 个本阶段私有 fixture 的日志目录；最终进程检查没有本阶段的 source/daemon 残留。

精确 Wayland 测试二进制 SHA256：
`a19eb2ccd8c1287b657ae94ebb67760dedc9bc1d1d19e85fad39f36587b7fadd`

证据目录：`tools/computer-use-probe/.run/wayland-pipewire-20260930/`。
`receipt.json` 关联源、文档、实际二进制、日志、SDK/package/runtime hashes 及上一阶段回执；
Node 与独立 Python 分块核验。上一阶段回执本身仍为
`829122ed36dc0da367a355180503a5d05abb840d39f51ae2991d99a1e20ded5f`。

## 下一步与完整未完成范围

下一条可实施路径：隔离接入 **libei/EIS 的实际设备、region、按键/指针 owner**，绑定 run/portal/frame
几何和撤销生命周期；不得用 Notify* 或 XWayland 代替。继续处理静态画面 freshness、rotation/DPI、
锁屏/拓扑撤销、AT-SPI 授权范围、IME/clipboard、parent/restore UX，并在原生 GNOME 上验收。

App 当前仍正确拒绝 Wayland；没有设置 `native_wayland=true`。本轮没有把未完成 consumer 包装成 UI 功能。
Windows x64、macOS arm64/x64、Linux X11/原生 GNOME Wayland；Desktop/managed/existing tabs/WebView；
App/ACP/MCP；完整输入/中文 IME/clipboard；签名干净安装/更新/修复/回滚/卸载；窄窗/DPI/权限 UI；
真实 Grok E4；**同一最终冻结候选 12h active soak** 的原始范围全部保持。
没有满足最终完成审计，不更新 goal 为 complete/blocked/paused。

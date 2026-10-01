# Computer Use — Wayland 实际 EI 输入检查点

日期：2026-09-30。完整目标仍是「接手 grok 的工作，完成 computer use 所有功能开发，推进到最终版」。
状态 **active / partial — not releasable**。上轮为 progress；本轮修改生产 Rust、构建入口和真实协议夹具，
并修复原生测试实际暴露的崩溃，不是状态重述、等待或把最终版缩成 EI 子模块。

## 本轮实现

- `PortalSession::start()` 同时监督 PipeWire capture 与单线程 libei input owner；只消费 portal 授予的 fd，
  不读 `LIBEI_SOCKET`，不回退 Notify*、XWayland 或其他桌面。所有 FFI 对象留在原生 owner 线程。
- 独立 `input_state()` 发布已协商/恢复的能力和 generation；portal Granted 不代表 input/frame ready。
  有界 32 命令队列、执行期限、旧 generation 拒绝、取消后撤权；未知结果不重放。
- 实际 evdev key / button、相对移动、精确 mapping_id 的区域绝对移动、平滑/离散滚动、取消滚动、
  ReleaseAll。拒绝重复按下、未持有键的释放、非有限/越界输入，停止时仅释放本 owner 的合成状态。
- 绝对坐标只接受唯一匹配的 EI virtual region；应用一次区域 offset，不把 physical scale 当截图比例。
  物理 EI 设备的毫米与虚拟设备逻辑像素不能混用。窗口采集也不等于窗口隔离键盘权限。
- device pause/resume/移除、seat/连接失效会更新或撤销能力；正在模拟时新增拓扑直接退休。
  Stop/Drop/portal 撤销、截图源失效与 EI 失效共享原 run 清理。Closed 在两个 native owner join 后发布；
  取消 Stop 等待不丢原 JoinHandle。每轮事件处理有上限，不能无限饿死取消。
- `InputSubmission` 只证明提交到 libei，**不证明 compositor/application 已执行**；不是截图动作授权、
  Unicode/中文输入、clipboard 或 App/ACP/MCP 端到端实现。

## 真实失败与修复

1. 原 libeis **1.3.901** 的 `eis_device_pause()` 只处理 RESUMED，不处理 EMULATING。
   最初持键后调用 pause 的测试没有收到任何 pause，确实失败；1.5.0 上游源码仍有该条件。
   改为实际 neutral-device pause/resume 与 active-device remove/disconnect 分开验收，未把无效调用算通过。
   生产收到持键期间 PAUSED 时会撤权，但该事件路径及 compositor 的实际按键状态复原仍须原生 GNOME 验证。
2. 审查发现 libei fd backend 不保证非阻塞。显式设置 O_NONBLOCK，加入静默/半个握手的 Stop 测试。
   此负例在旧 **1.3.901** 上触发了真实 **SIGSEGV**（`test-7.log`），不是可忽略的测试断言。
   上游 `ei_disconnect` 在 BACKEND 阶段操作尚未建立的 connection；1.5.0 已增加 BACKEND 排除条件。
3. 构建现要求 **static libei >= 1.5**；不让同 SONAME 的旧系统 `.so` 在运行时重新引入崩溃。
   `scripts/build-computer-use-libei.sh` 固定 1.5.0 完整源包 SHA256：
   `da1fba92daccd0667bc46c3ee952d4ae8cfc6bdb4c0bb4d34df26528fb240618`。
   私有新 prefix / digest-before-extraction / retained COPYING；已有目录和坏摘要均真实拒绝。
   最终 ELF 无 libei 动态依赖；放回旧动态库搜索路径，原静默/部分握手测试仍通过；旧 SDK 构建被拒绝。
4. 崩溃遗留的单个私有 bus 使用日志中确切 PID、当前 argv、专属环境身份与 pidfd 验证后退休，
   没有按名称 kill 用户服务。测试 bus/PipeWire/EIS 子进程增加 parent-death guard，日常仍保留 Child 句柄清理。
5. 新物理设备负例首次因 fixture 没有声明 physical size 被协议拒绝；补齐合法 size 后才验证单位拒绝。
   所有初期失败、诊断与 SIGSEGV 日志保留，未用后续绿色覆盖历史。

## 冻结源上的实际验证

**309 个选定源文件冻结**；相对前阶段 302 项，9 个既有文件改变、7 个新增，其他选定源未变。
Rust Cargo.lock 原 **693 包版本/checksum 不变，无新增包**；新增 crate build dependency 复用已有 pkg-config。
native libei 从旧隔离测试 SDK 改成由正式 SDK builder 产出的静态 1.5.0，不把此变化说成“全部依赖未变”。

- 同一归档 joint-feature 测试二进制三轮（线程 1、4、4）：每次 **34 passed / 0 failed / 0 ignored**。
  包含全部 9 个显式 native tests、静默/部分协议测试；最终日志无后台 panic / SIGSEGV / FAILED。
- 真实私有 D-Bus + SCM_RIGHTS + libei/libeis：事件坐标/scan code、owned key/button up、scroll cancel、
  新旧 generation、缺失/重复 mapping、物理单位、多 run 隔离、取消提交、取消 Stop、移除/断连/拓扑退休。
- 实际 PipeWire 像素和 EI 输入共享同一个 production supervisor：capture source 死亡后发送 held-key release，
  关闭两个 native consumers，图像/输入同时失效。不是假图片或绕开生产 owner 的模拟调用。
- Wayland+X11/native-probe 联合严格 Clippy `-D warnings`：通过。
- X11 单测 **49 passed / 0 ignored**；owned Xvfb **19 PASS 组**；GTK/AT-SPI **41 PASS 组**。
- Windows App library `cargo check --offline --locked -p grok-app --lib --jobs 4`、workspace fmt、diff check：通过。
- 私有 SDK builder 本身实际构建，负例验证；运行时检查没有本阶段拥有的 fixture 进程残留。
  归档 **214** 个阶段私有 PipeWire/EIS 日志目录（含早期失败），不是 214 项独立完成需求。

确切 Wayland 测试二进制 SHA256：
`51e3216424f01ad39066dca0adab37decec9f4887cb355df0cbc28bb5630fe39`

证据：`tools/computer-use-probe/.run/wayland-eis-20260930/`；receipt 绑定源、文档、二进制、日志、
SDK/上游源码、进程检查和上阶段原 receipt。上阶段 receipt 字节不改，SHA256：
`f0bde2f76f832b1b629cd3655aec2d28ebd754186db3f39da6db2a0feacae957`。

## 仍未达成的最终版要求

这仅为 **E2 / 真实 native IPC + 像素/输入事件证据**。环境为 WSL Debian、私有 portal/EIS/PipeWire peers；
不等于原生 GNOME session-manager、用户桌面、真实 Grok 或安装态 E3/E4。
removed/disconnected 设备无法接收 key-up；测试只证明不能再提交、上下文退休，未证明远端物理状态复原。
资源耗尽/底层 setup failure、静态画面 freshness、rotation/DPI/多屏拓扑、锁屏恢复仍未完整验收。

下一条实施路径：把 frame/run/format generation 与 EI region/device generation 绑定成可撤销观察授权，
再接入 Host desktop adapter；补完整输入/IME/clipboard、AT-SPI scope、portal parent/restore UX，
在原生 GNOME 验证实际界面效果。当前 **App 仍拒绝 Wayland，未设 native_wayland=true**，不能把低层可用冒充产品完成。

以下完整要求没有被本检查点缩小或宣布完成：Windows x64；macOS arm64/x64；Linux X11 和原生 GNOME Wayland；
Desktop/managed browser/existing Chrome+Edge/App WebView；App/ACP/MCP；完整输入/中文 IME/clipboard/取消与恢复；
签名干净安装/更新/修复/回滚/卸载；原生窄窗/DPI/权限 UI；真实 Grok E4；**同一最终冻结候选 12h active soak**。
本轮没有重跑完整前端/Core、真实安装/发布、远端 CI；没有提交、推送或打 tag。
最终完成证据不齐，goal 保持 active，不调用 complete/blocked/paused。

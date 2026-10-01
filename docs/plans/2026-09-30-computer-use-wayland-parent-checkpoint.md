# Computer Use：GTK 原生 Wayland parent 租约与 UI 线程释放

日期：2026-09-30（Asia/Shanghai）。目标仍为 **active / partial — not releasable**，不是最终版。

## 本轮交付

- 新增 `src-tauri/computer-use-gtk-parent/`，使用真实 GTK3/GDK Wayland `xdg-foreign` 导出，而不是拼接模型提供的 parent 字符串。仅接受 GTK 主线程、未取消的实际 AuthorizationTicket、已映射的真实 toplevel；通过 GType 判断 Wayland，拒绝 popup、X11，不依赖环境变量推断后端。
- `ParentLease` 可跨工作线程传递，但 GTK Window/GdkWindow 及原始导出引用由 GTK 线程独占。Ready 前取消、Ready 后撤销、截止时间、Drop、hide/remap、destroy 均关闭旧租约；迟到回调不得恢复授权。
- 同一窗口的两个成功 export 对应两个 GDK 引用、一个 compositor handle；释放第一个引用不能销毁第二个，最后一个才销毁原生导出。已完成的授权票据不能再次 export，但不因 preparation 完成而误撤销已有租约。
- App 新增 `src-tauri/src/computer_use/linux_parent.rs` 主线程准备桥：真实 Tauri Window → 原 GTK 窗口 → parent lease；队列超时/调用方退出后不再创建晚到导出，发送竞态中的租约由原 UI owner 收尾。
- **桥接尚未接入实际 consent command/picker 与 PortalRegistry 全生命周期。** Linux factory 仍仅 X11，`native_wayland=false`；本轮不提前打开尚未完成的原生 Wayland 功能。

`ParentState::Closed` 只证明本租约的 GDK 导出引用在原 GTK 线程释放；不等于 compositor 已确认、portal session 已清理或应用操作效果已验证。GTK 主循环停止时不会靠计时器到期伪造 Closed。

## 原生发现与修复

1. 在真实私有 labwc 下，`native-attempt-3` 的窗口销毁测试触发 fatal GLib critical：GtkWidget 已断开 handler，owner 又断开同一 ID。改为在同一 UI 线程用 `g_signal_handler_is_connected` 检查精确 ID。
2. 源码核对发现 GdkWindow 的 destroy/unmap 不能当作 export 引用已释放；原 owner 的强引用尚在，GDK finalization 尚未发生。改为每个成功 export 无条件平衡一次 unexport。新增“双租约共享窗口后 destroy”的原生协议测试，避免多引用遗漏。
3. GDK callback 的 destroy-notify 在成功交付 handle 后就会运行，并非 unexport 通知；userdata 生命周期与 export 引用分开管理。原始协议证明两次共享 export 只创建一次原生 handle，首个 close 不产生 destroy，最后 close 恰好产生一次。
4. popup 虽也可能是 GdkWaylandWindow，但不是 xdg-foreign toplevel；在调用原生 ABI 前拒绝，避免协议错误终止连接。

原始失败日志和修复前源码保留，未降低 `G_DEBUG=fatal-criticals`，未取消共享引用或销毁断言。

## 验证边界

- **334 个主源文件冻结**：在前阶段 329 个基础上新增 5 个；已有主源仅 Cargo.toml、Cargo.lock、computer_use/mod.rs 改变。
- 同一冻结 parent executable，在私有 user/network/mount namespace、私有 headless labwc 中三轮，每轮 **12 组 Wayland + 4 组 X11 负例**。每轮真实协议有 **9 个导出 / 9 个销毁 / 9 个不同 compositor handle**；11 次成功 API export 中两组共享窗口正确引用计数。Python 与 Node 分别检查原始协议。
- 每轮直接保留并 wait 真正的 labwc/Xvfb Popen 子进程，6 个原生子进程返回码均为 0；各私有 runtime 的 owned 进程扫描为 0。不是等待包装 shell 即视作底层服务已退出。
- **Linux 实际 App `cargo check --lib --offline --locked` 通过**，包含 Linux-only Tauri/GTK 桥接；不是仅 Windows cfg 检查或替代源码 harness。使用单次检查的 TAURI_CONFIG overlay 指向真实已准备 Linux seed，未改共享 Windows seed。此检查不证明 Linux 安装包可交付。
- Windows App check、core **586**、driver **16**、FFI wait **21**、App command/WebView/session-MCP/feature **4/69/21/1（95）**通过。
- Linux core **585**、driver **16**、FFI wait **21**；同一 Wayland binary 三轮各 **93 / 0 ignored**（25 native），X11 unit/Xvfb/AT-SPI **49/19/41**；联合严格 Clippy（含新 GTK crate）、fmt、diff check 均通过。159 个原生目录与 PNG/原始 EI/peer 身份独立核对，owned 残留 0。

证据目录：`tools/computer-use-probe/.run/wayland-parent-20260930/`。
重点文件：`source-freeze.json`、`parent-owned-{1,2,3}/wayland-wire.log`、`parent-wire-verification.json`、`independent-parent-review.json`、`linux-app-check-third.log`、`native-runtime-hashes.json`、`boundary-review.json`。

GTK/GDK 实际版本 3.24.49；labwc 为 0.8.3。SDK 仅通过下载 deb 并解包到私有目录补齐，不执行系统 apt install。使用真实原生协议；**labwc 不是 GNOME，测试不是已安装 App 的原生 UI 验收**。

## 保留失败，不改写历史

- 初次 labwc 启动因缺少 Xwayland 可执行文件退出；私有 SDK 补齐后又遇到 WSL 共享 `/tmp/.X11-unix` sticky-bit 条件。最终使用独立 mount/network namespace 中的私有 `/tmp`，未修改用户桌面目录或全局权限。
- 首批 `parent-final-3` 测试主体通过，但即时清理审计看到一个仍存在的 PID，故整轮不算验收通过；随后同一 PID 已不存在，不能据此倒改首次失败。检查系统 xvfb-run 脚本证明 EXIT trap kill 后不 wait；替换为直接持有真实 Xvfb 进程并 wait。首次 PID 的确切身份没有及时记录，**不宣称已经证明该 PID 就是 Xvfb**。
- Linux App 首次检查缺 libsoup SDK，第二次检查发现共享 seed 是 Windows 资源而非 chrome-linux；均保留原失败。最终只对本次 Linux 检查使用真实 Linux seed overlay，不删除资源校验或生成假文件。
- 初次 SDK 查询使用 WSL 中不存在的 rg，以及外部源码 curl 超时，均不算产品验证通过。
- 历史 Windows tiny_prepare publication access-denied 与旧阶段单次 PipeWire 即时节点快照失败，后续通过不能当作根因已修复；这两个历史状态保持未确证。

## 下一阶段与完整目标

下一阶段接线：**真实 parent lease ↔ PortalRegistry owner ↔ App consent command/picker**，保持取消/窗口丢失/授权失败/Stop 的精确所有权、阻止迟到授权与旧票据复活；随后接入真实 OS permission、session lock、用户接管、restore，并在实际 GNOME 中验收。

原目标范围不缩小：Windows x64、macOS arm64/Intel、Linux X11/原生 GNOME Wayland；Desktop、managed browser、已有 Chrome/Edge、App WebView；App/ACP/MCP；完整输入、中文 IME、clipboard、取消/恢复；签名 clean install/update/repair/rollback/uninstall；原生窄窗/DPI/权限 UI；真实 Grok E4；**同一最终冻结候选 12h active soak**。这些尚未全部完成或证明，不调用 goal complete/blocked/paused。

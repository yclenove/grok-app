# Computer Use：同步原生事件观察与 client 漏报修复

目标仍为**接手 grok 的工作，完成 Computer Use 所有功能开发，推进到最终版**。
状态 **active / partial — not releasable**；只修复并验证本节明确列出的普通 client 路径，
不把它替代完整输入、安全接管或最终版验收。文档使用开发指令日期 2026-09-30；
`20261001` 是既有本地运行目录标签。

## 已落地的生产修改

- `tools/computer-use-gnome-shell/policy.js` 新增 `NativeEventWatch`；extension 在
  export endpoint 前订阅 `global.backend.get_core_idle_monitor()` 的 one-shot
  user-active callback。在同步 callback 内读取 `Clutter.get_current_event()` 的
  真实 event/source，沿用原生 device-node 身份判定；不读取 idle 毫秒数，不把
  idle reset、device name、synthetic bit 或 last-device-changed 当物理输入证据。
- 每次原生 callback 先重新订阅，再观察；Mutter 删除的是旧 one-shot ID。
  无 event、订阅失败/ID=0、未知实现、发布失败均保守失效；stop/disable 取消原订阅，
  stale callback 不可复活。异常不跨入 Mutter，也不 suppress/steal 用户输入。
  不保留按键、文本、坐标；D-Bus 仍只有版本/epoch/代次/blocked。
- 原 late Clutter filter 仅作为补充，Shell 路径允许重复 loss 代次，不授予权限。
  新增 8 项生命周期/故障 contract，Node 总计 **18/18**。CI 已有此测试入口，
  此处是本地通过，不声称远程 CI 已运行或证明真实输入覆盖。
- 新增受 owned-VM 标记约束的 original-Host upgrade 测试：只通过原 `act(Repair)`
  发布新 embedded helper；当前 Shell 未重启前 Enable 必须拒绝。没有直接改写来宾
  helper 文件、伪造安装状态、reload 已缓存模块或修改真实用户桌面。

## 同一严格 gate：红到绿

上一阶段 `gnome-input-source-20261001/client-delivery-{001,002}.json` 保留原字节：
两轮均 **0/7**，client 每项收到一次但 helper 代次不变。

本阶段 `gnome-input-observer-20261001/client-delivery-{001,002}.json` 均 **7/7**：
两个连续 pointer move、button down/up、scroll、Shift down/up，每项 client 计数 +1，
helper 代次 +1（wheel action +2）。原 fixture SHA
`e6761c3739de4028b453c80d134da996975f79925f11767edab91f7ebff8576c`，oracle、动作集合及
owner/epoch/前台/native-Wayland 约束都未降低。两个 helper epoch 不同，重复启停后结果仍成立。

来宾是原有真实 GDM/Ubuntu/GNOME Wayland VM，attempt8、QEMU PID11668；原 Host Repair
之后实际 GDM 重启，Shell PID **1339 → 2604**，native Wayland/session active 重新验证。
Host upgrade 和 repaired-enable-disable 两项各自显式 **1 passed / 0 ignored**。
仍为 featureEnabled=false，无 App grant、portal 自动同意或 App/ACP/MCP 输入。

构建前 **418** 条选定源码和 tar 冻结，测试 ELF SHA256：
`17c48035fc02fa7a6ae431992d8c846cda1266a0ed30027c5e5ddd6839e16616`。
原 Host 实际发布并回读的 helper：

- `extension.js`: `bbb5cac3b2886b3bc2d1396c4cc0add93af0a99e680d2fd2bdfdadbf24b12454`
- `policy.js`: `daaae416091de6025ec69ebb062e7be52269bcb665cfb9f63806521d8ff3d89b`

两轮 fixture/SSH 均正常退出并 join、helper 明确停用且 endpoint absent；原 QEMU 正常
关机 exit=0 并 join。最终目录 0700、文件 0600，未修改 host 输入权限或已有用户安装。
本轮不再有 live VM/fixture 等待任务。

## 残余 P0：不能由普通七项外推

官方 Mutter46.2 源码已按 URL/SHA 留档，实际 GI callback 也已在上述 VM 运行。
`events.c` 的 core-idle callback 位于普通 native-client 消费之前，但仍位于以下早退之后：

1. `meta_display_process_captured_input`；
2. `meta_wayland_text_input_update` → `clutter_input_focus_filter_event` → IM vfunc；
3. mapped tablet-pad action handler。

因此 **IME/input-capture/mapped-pad 的完整观察仍未修复/验证**。IM 未消费的按键可能
重新入队，不能据此假定被消费的中文组合输入也安全；`notify_key_event` 是函数而不是
可随意连接的逐键观察信号。没有 monkeypatch Shell/IM、重排 Mutter filter、访问
`/dev/input`、提权守护进程或抢占 input-capture 来掩盖这些缺口。

还缺实际 EI 排除、touch/gesture/tablet/grab、真人接管、真实 App grant 撤销/停止时序。
两份 green JSON 仍明确 `humanPhysicalInputVerified=false`、
`appGrantRevocationVerified=false`、`fullInputCoverageVerified=false`。
helper/preview 仍默认关闭；不能用于真实自动化或宣称 native Wayland 完整安全。

## 校验范围与历史边界

- 本轮 preview helper **17 passed / 8 manual ignored**（其中新 upgrade 与既有 repaired
  两项另在 VM 显式执行），commands/portal/feature **15/6/1**，Node **18**、Python oracle **8**。
  preview 全 targets `Clippy -D warnings` 通过。单个改动 Rust 文件在 Windows rustfmt
  检查通过；最初 WSL `cargo fmt` 因隔离 toolchain 没有 fmt 子命令而失败，原日志保留，
  不是静默跳过或声称该命令通过。
- 上一阶段 full Wayland171、default-build 和更早 platform 结果仅为历史证据，
  本轮没有重跑，不冒领新候选全套通过。构建冻结后只有 README/验收说明更新，
  embedded JS 与 Rust 测试输入保持冻结 SHA；新旧 receipt/archive 分开保存。
- 仍为 GA Mesa24.0.5 对比环境。更新 Mesa25.2.8/virtio 导致 Shell 崩溃仍未解决；
  不把每日 Ubuntu image 或降版本对比当正式发行/兼容性完成。

## 完整目标保持不变

Windows x64、macOS arm64+Intel、Linux X11/Ubuntu24.04 GNOME Wayland；Desktop、managed
browser、existing Chrome+Edge、App WebView 经 App/ACP/MCP；完整输入/中文 IME/clipboard、
OS 授权、接管/锁屏/焦点/拓扑、取消/恢复；Ubuntu22.04、AppImage/deb/rpm、签名安装/升级/
修复/回滚/卸载、原生 UX/DPI、真实 Grok E4、**同一最终冻结候选的 12h active soak**
全部保留。当前仅证明普通 client 漏报修复，未通过最终版逐项完成审计。

本轮归类 **progress**：生产观察路径已改变，原本全红的严格真实 client gate 两轮全绿。
下一步是 IME/capture/pad 等早退路径与实际 grant 撤销链路，不缩小产品支持范围来满足测试。
不调用 complete/blocked/paused，不提交、推送、签名或发布。

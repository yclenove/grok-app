# Computer Use：GNOME 授权选择与接管监控接线

日期：2026-09-30。目标仍为 **active / partial — not releasable**；不是最终版。
本轮证据目录沿用本地运行标签 `tools/computer-use-probe/.run/wayland-gnome-consent-20261001/`。
上一目标回合属于 **progress**；本轮修改生产代码、注册表和 Linux GTK 桥接，而非只更新计划。

## 生产增量

- 新增 `connect_for_consent` / `GnomePolicyActivation`，原 login1、ScreenShield、
  helper unique owner 和订阅在选择器、激活、收尾之间保持不变，不重新连接或替换 watch。
- 选择阶段只能吸收同一 epoch 的物理输入代次；实际 input policy 始终为 false，
  没有可用 adapter/target。锁屏 pulse、owner/epoch/version 变化、错误消息、掉线、
  heartbeat 失败和事件循环超时仍关闭授权，不能借“正在选择”吞掉安全事件。
- 原生 consent ready 后才消费一次性激活令牌。先处理已排队的选择器事件，再用
  250 ms 内的新 GetState 建立检查屏障；屏障期间及激活后的任何代次变化均撤销。
  激活被取消/丢弃、旧快照晚到、过期运行循环均不能恢复旧 grant。
- Registry 先保留占用，再初始化并持续运行原监控；未真正 ready 的策略不提前包装为
  sticky lease。原 Host 权限、AuthorizationTicket、GTK parent 检查均保留，不被新策略替换。
- `select_parented_gnome_for_authorization` 已接到 App 的 `linux_parent.rs`。
  原生会话失败/拒绝/取消先收尾 PW/EIS，再停止并 join 原 OS monitor，最后等待 GTK owner。
  初始化期间取消也保留原 future；不能通过超时/丢 receiver 假报 Closed、idle 或允许替换。
- 原立即严格监控 API 保持行为；GNOME 专用路径缺 helper 时失败关闭，不自动安装，
  不退回 XWayland。App factory 和 native_wayland 能力标志仍未启用。

## 验证记录

367 源冻结，相对上一候选 **351 未变、11 修改、5 新增**。
新增 **9** 个 staged watcher、**5** 个 registry、**1** 个原生联测用例。
定向非原生子集 **131 passed / 0 failed / 0 ignored / 32 filtered**。

| 当前冻结候选检查 | 结果及范围 |
| --- | --- |
| 同一 Wayland executable | **163 × 3**，线程 1/4/4，0 failed / ignored / filtered |
| 原生联测 | 授权选择期间没有 target；选择后捕获真实 PNG；再次物理代次信号先由独立 C EIS peer 观察断连，不靠后续模型调用/Stop |
| 证据独立核对 | **195** 个原生 fixture 目录；PNG 解码/像素、精确 C peer、无重放输入、parent 协议、native/registry/子进程收尾 |
| Linux App / Wayland | 两者完整 all-targets strict Clippy 通过 |
| helper / runner | Node contracts **10**、runner contracts **17** 通过 |
| 格式 / 质量 | workspace fmt、git diff、quality final 通过 |

本轮没有重新跑 Windows/macOS/core/App selected/独立 GTK probe 或 GJS GI14/16；
这些旧证据仅作历史引用，不能据此宣布当前候选全平台通过。完整原生回归含现有 GTK
parent-registry fixture，但不等于重新验证整套 GTK 独立 probe。

初始化编译曾暴露测试可见性与 Debug bound 两个错误，Clippy 曾报告未显式处理内层
revocation Result；均已修正并保留诊断。另一次 cargo 参数位置错误保留为执行诊断。
这些不是生产行为的 red→green 证明；不将编译错误包装为已复现产品故障。

## 证据边界与完整目标

私有 D-Bus helper 信号不是实体输入；真实 PW/EIS/PNG/GTK 协议 fixture 也不是
installed Ubuntu GNOME 或实际 App UI 验收。异步监控不是 compositor 内的原子输入栅栏，
不能声称零锁屏后输入。本轮不安装/启用用户扩展，不改变输入权限，不发布、签名或安装 App。

以下完整范围继续保留，不因本轮用例通过而删除：

1. installed Ubuntu 24.04 GNOME Wayland：真实 helper 加载、设备分类、锁屏/接管/恢复、
   session PID 绑定、focus/topology、实际 parented consent 和 App factory/命令/选择器流程。
2. 可见、15 locale 的 helper 安装/启用/状态/停用/恢复和 Stop UX；原生窄窗/DPI/权限 UX。
3. Windows x64、macOS arm64/Intel、Linux X11 与 native Wayland 全矩阵；Desktop、
   managed browser、existing Chrome/Edge、App WebView 经 App/ACP/MCP 的完整功能。
4. 全输入、中文 IME、剪贴板、取消/异常恢复和 OS 许可真实闭环。
5. 签名后的 clean install/update/repair/rollback/uninstall；Linux 打包、模块/许可及
   Ubuntu 22.04 私有 SDK/AppImage/deb/rpm 兼容性，不能以当前私有高版本 SDK 替代。
6. real Grok E4、同一最终冻结候选 **12h active soak**，以及完整需求逐项完成审计。

下一阶段应推进实际 App consent/factory 与显式 helper 安装/恢复交互，同时继续取得
installed GNOME 的真实平台证据；不能只靠扩大 fixture 测试数量宣布最终版完成。

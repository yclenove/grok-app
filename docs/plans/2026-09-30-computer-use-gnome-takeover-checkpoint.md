# Computer Use：GNOME 原生策略 helper 与物理接管代次

日期：2026-09-30。目标继续 **active / partial — not releasable**，不是最终版。
运行证据：`tools/computer-use-probe/.run/wayland-gnome-takeover-20261001/`。
上一目标回合分类为 **progress**；本轮进一步修改生产实现并取得新验证证据。

## 生产增量

`tools/computer-use-gnome-shell/` 增加只读 GNOME Shell helper。它在 Shell 原有 D-Bus
connection 导出 `NativePolicy1.GetState/Changed`；只传协议版本、epoch、代次、blocked。
不传按键/文本/坐标/设备名/设备路径；不打开 `/dev/input`，不抢事件，不注入输入，
不截图，不修改 Shell 方法，不改变权限，不自动安装或启用扩展。

官方 Mutter **46.0** 源码明确显示 EIS virtual device 也使用 `InputMode.PHYSICAL`，
不能用该枚举判断真人输入。现在从原生事件 source device 的 compositor-owned
`device-node` 区分 libinput 设备与原生虚拟设备；未知来源撤权，事件始终传播给用户。
这不代表能区分本 App EI client 与其他虚拟输入 client。

helper 观察 ScreenShield actor visibility、locked/active 和 resolved session-mode
状态，比公开 ActiveChanged 的动画延迟更早产生 loss signal。Ubuntu 继承 user 的
session 不再被名称判断误拒绝。lock→unlock、实体输入、disable/error/reload 都无法
让已绑定旧 epoch/代次的授权复活；广播失败也不会把异常抛出到物理事件处理链。

Rust 新增 `GnomeNativePolicyWatch`：复用已核对 UID/PID/login1 的原 Shell unique owner，
订阅先于快照；组合原 login1/ScreenShield future，不 detach。helper 状态变化、错误
消息、取消、掉线、丢 owner、unexport、超时均永久关闭原 grant。100 ms heartbeat、
250 ms reply deadline、500 ms freshness；reactor 卡住时 policy 自身也能拒绝输入，
晚到的 heartbeat 不恢复旧 grant。仍必须有 Host 权限策略和 portal consent。

## 当前冻结验证

362 源冻结：与上一候选相比 **349 未变、4 修改、9 新增**。

| 当前候选检查 | 结果及证据边界 |
| --- | --- |
| 新 Rust policy 用例 | **10**，真实私有 D-Bus；定向 login1/GNOME 总计 **32** |
| 新 native PW/EIS 联测 | **1**；helper 代次变化先由独立 C peer 观察 disconnect，不靠下次 act/capture/Stop |
| 同一 Wayland executable | **148 × 3**，线程 1/4/4，0 failed / ignored / filtered |
| 原生证据 | **189** 个 fixture 目录，真实 PNG/C EI 事件/精确 peer/parent 及收尾独立核对 |
| helper Node contracts | **10** 通过；包括原生/虚拟来源、早期锁屏、Ubuntu mode、广播失败 |
| 私有实际 GJS/D-Bus | 两轮：Ubuntu 46.0 的 Mutter GI 14、Debian 48.7 的 GI 16；每轮 7 次方法验证、5 个内容为空的状态信号 |
| App / Wayland | 完整 Linux App 与 Wayland all-targets strict Clippy 通过 |
| runner / 格式 / 质量 | **17** contracts、workspace fmt、git diff、quality final 通过 |

GJS 使用私有提取的 1.82.3 SDK，不是 installed GNOME 46/48；只验证实际 GVariant/
exported-object 传输以及 native GI API 可调用入口。输入 source 仍明确是 fixture，
没有在用户 GNOME Shell 中启用扩展，也没有真实硬件输入验收。所有操作在 owned
namespace/private bus；没有更改用户安装、登录桌面、input group 或系统权限。
同一 frozen binary SHA 在三轮前后相同，原 Linux session **32162** 已确认 exit 0。

保留 `fixture-executor-panic.log`：首轮虽然 libtest 返回通过，测试服务内部使用 Tokio
timer 却运行在 zbus executor，产生后台 panic；该结果**不作为通过依据**。已改为
无 reactor 依赖的 Notify，并完成无 panic 的定向及三轮全量验证。这是 fixture 修正，
不是伪称生产缺陷的 red→green。SDK 初次依赖/命令问题也不作产品故障结论。
Windows/macOS/core/App selected/X11、单独 GTK probe 本轮未重跑，不借用历史结果。

## 未完成边界与下一步

**native_wayland 仍为 false。** helper 的可见安装/启用/状态/禁用/恢复 UI 及本地化、
App factory/consent 接线、真实 Ubuntu GNOME helper 装载、physical libinput vs EIS、
锁屏时序、具体 target/focus/topology 与原生 PID/login1 绑定仍须验收。消息与 heartbeat
是异步 loss 通道，不是 compositor 原子输入栅栏，不能声称“锁屏后零输入”。

consent 自身会产生真人鼠标输入。App 必须在 picker 阶段使用会话/锁屏策略，显式选择
完成后、激活 grant 之前建立新的物理输入基线；不得在有效 grant 期间忽略真人输入。
这项事务仍未接线，不能只装扩展后就开启 native Wayland。

完整目标仍为 Windows x64、macOS arm64/Intel、Linux X11/native GNOME Wayland；
Desktop/managed browser/existing Chrome+Edge/App WebView 经 App/ACP/MCP；完整输入、
中文 IME、clipboard、取消恢复；真实权限/锁屏/接管/恢复；签名 clean install/update/
repair/rollback/uninstall；窄窗口/DPI/权限 UX；真实 Grok E4；同一最终冻结候选
**12h active soak**。Ubuntu 22.04 包装、历史 Windows publication/即时 PW 节点快照/
残留 PID 身份等未证问题不因本轮通过而视为已修复。没有 commit/push/tag/publish。

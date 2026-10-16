# 0008 Pure Rust EI sender behind portal input controls

状态：接受（03 §2 / P7-03 已指定的依赖与输入路线）
日期：2026-10-15

## 背景
[03 §2](../../../docs/plans/computer-use-v2/03-architecture.md#2-crate-划分与依赖规则)
指定 `reis 0.7`（tokio）实现 libei 协议，不使用 libei-sys。
新增锁定依赖 `reis 0.7.1` 后，`cargo deny check` 的 advisories/bans/licenses/sources 全部通过。

## 决定
仅 Linux 依赖 reis，使用会话 `ConnectToEIS` 返回的 FD；不探测环境 socket，不回退 portal Notify*、XTEST 或全桌面。
沿用 ADR 0007 的 ashpd/pipewire 方案，不修改 deny.toml。
portal 同意后仍要求独立的可信来源证明：精确窗口/映像/进程实例、节点/serial、mapping ID 与键盘输入范围。
仅预检当前焦点不构成输入隔离；无法证明范围的普通全局键盘会话保持禁用。
成功注册且持续健康的全局 Stop 控制由可信 Host/桌面适配器提供；检测到 GlobalShortcuts 接口不算注册成功。

## 备选与为什么不选
libei-sys 增加原生绑定且违背指定方案；全局输入或猜测来源不能满足授权范围。

## 影响
不修改 Driver/Router 宪法接口。所有工具仍进入 ActionRouter → DriverAction → LinuxDriver。
Fake portal/EI/来源/时钟覆盖跨平台生命周期；真实 Wayland 运行记为 not_run。
reis 本身无需额外系统库；键盘映射复用既有 libxkbcommon，PipeWire 依赖保持 ADR 0007。

## 退出条件
若 reis 无法支持必需 EI 版本或 compositor 无法提供可信输入范围，保持路线关闭并另提 ADR；不得放宽门禁。

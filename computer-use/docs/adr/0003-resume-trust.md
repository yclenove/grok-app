# 0003 CLI 恢复的尽力而为信任门槛
状态：接受
日期：2026-10-13

## 背景（事实 + 证据链接）
[安全评审 #1/#9/#21](../../../docs/plans/computer-use-v2/reviews/2026-10-13-security-threat-review.md)指出同 UID 不等于可信用户、Stop 不得自动恢复、停止不能排在动作/审批后。[P0-10](../../../docs/plans/computer-use-v2/06-roadmap.md#p0-10-gcu-climcp--call--doctor--versionfake)仅组装 fake 驱动，可信覆盖层与 App 恢复入口在 P4-04/P5。

## 决定
`gcu mcp` 为每个进程在 `$XDG_RUNTIME_DIR`（缺省为平台用户临时目录）内创建随机私有控制目录。Unix 目录 0700，socket/token/challenge 为 0600；Windows 使用 owner-only 受保护 DACL 和拒绝远程客户端的随机命名管道，抢先占用的端点直接失败。控制任务独立于 MCP 动作和审批队列，Stop 先锁存，再调用 `Session::handle_stop`。停止状态持久保存到用户状态目录，重连、下一轮、重启均不清除。

`gcu resume [--session <目录>]` 在读取凭据前检查 stdin/stdout 都是交互 TTY；显示 challenge，让用户手输 `yes <challenge>`。客户端发送 256-bit 随机 token 与 challenge，服务端验证后消费旧凭据、轮换新凭据，再调用 Host 的 `clear_stop_by_user`。未认证、重放、非 TTY、取消或超时均不能恢复。token/challenge 不进入环境、argv、日志或 MCP 输出；MCP 工具表没有恢复工具。凭据和控制目录随 MCP 会话退出删除。

**同 UID 带 shell 工具的模型仍可能读取 token 并伪造 TTY 执行 resume。TTY、文件权限与 token 只是尽力而为的门槛，不证明人类身份。真正的可信恢复在覆盖层按钮、App 面板或热键（P4-04/P5）。** `gcu resume --help` 明确给出这个边界。

## 备选与为什么不选
仅 token/UID：无法隔离同用户模型；仅输入 yes：没有会话认证与重放保护；本卡实现完整可信 UI：超过 P0-10 的 fake CLI 范围。

## 影响（对接口、测试、用户）
不改 Driver/Host 协议和工具语义。`Connection` 提供进程内调用及可信 Host Session 接入，停止控制不会暴露到模型面。测试覆盖双 TTY 检查、未认证与旧 token 拒绝、恢复后的锁存、MCP 工具表、独立输入释放及重启持久化；Linux 用现有 libc 的 PTY 做进程级确认/取消测试，其他平台用可注入 `IsTty` 单测与控制通道测试，无新增 PTY 库。自动审批只在 debug + `eval-autoapprove` 构建有效，release 遇到 `GCU_APPROVAL` 拒绝启动。

## 退出条件（什么情况下推翻本决定）
P4-04/P5 提供可验证用户事件的可信恢复入口后，将 CLI 恢复请求交给该入口；不再以 TTY 作为允许恢复的身份门槛。

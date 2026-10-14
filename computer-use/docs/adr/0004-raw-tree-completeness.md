# 0004 原生树遍历完整性

状态：接受
日期：2026-10-13

## 背景（事实 + 证据链接）
[`RawTree`](../../crates/gcu-core/src/model.rs) 仅有 `root`；[AX](../../crates/gcu-driver-macos/src/ax/tree.rs) 与 [AT-SPI](../../crates/gcu-driver-linux/src/atspi/tree.rs) 的节点/深度上限会静默裁树，期限会丢弃部分读取结果。未读区域可能包含密码节点，core 因此无法证明截图脱敏完整。总控已批准方案 1：显式上报事实，由 core 统一拒绝不安全出图。

## 决定
- `RawTree` 增加 `incomplete: Option<Incomplete>`，缺省 `None` 表示原生枚举完整。`Incomplete { reason, read_nodes: u32, unread_nodes: Option<u32> }` 只含事实；原因是 `Deadline` / `NodeCap` / `PartialError`，并以 `DepthCap` 区分现有深度限制。已读计数为保留的原生节点数；未读总数未知则 `None`，不把直接子节点数冒充整个未读子树大小。首个原因保留，停止整次遍历的期限优先。
- Linux/macOS 保持 2s / 5000 上限与取消/致命错误语义；已有节点的期限中断返回部分树，首个节点未读完仍返回 `Timeout`。被容忍的原生字段/子节点读取错误标记 `PartialError`。
- core 在裁剪前保留此事实并将敏感区域定位标为不可靠；`redact::apply` 独立检查完整性，返回现有 `NeedsHandoff`。观察请求截图时返回 `handoff_required`，不交付模型图、缩略图、审批裁剪或临时图片；截图前后的树均须完整。
- 文本观察正常交付，并附 `[truncated: native tree incomplete …]`。结构化 `truncated` 增加可选 `native_incomplete`（同一纯数据类型由 protocol 定义、core re-export）；`omitted_nodes` 仍只数 core 已知的可见性/渲染预算遗漏，不混入未知原生数量。完整树 wire 输出保持不变，原生不完整且无预算遗漏时允许 `omitted_nodes: 0`。

## 备选与为什么不选
- 各驱动自行禁图：重复安全策略，遗漏一个平台即泄露。
- 仅布尔标志或把原生遗漏混入预算计数：无法解释原因，未知数量也会被误报成零或猜测值。

## 影响（对接口、测试、用户）
所有 `RawTree` 构造点与 protocol 错误状态 schema 同步更新；Fake 源覆盖边界、期限、部分错误，core 覆盖登录/密码/未知区域拒图、完整树与纯文本回归。现有完整树快照不变；仅错误 schema golden 新增完整性定义。本卡不实现 Windows P2-02。

## 退出条件（什么情况下推翻本决定）
仅在独立可信机制可证明所有截图敏感区域均完整定位时，另写 ADR 评估解除此拒图条件；渲染预算不能作为证明。

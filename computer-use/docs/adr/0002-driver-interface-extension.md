# 0002 Driver 接口的最小扩展（P2-01 触发）

状态：接受
日期：2026-10-13

## 背景
Windows 驱动的发现卡（P2-01）按 03 §5.2、§7.4 需要：引擎提升/管理员目标/安全桌面/缺权限的类型化拒绝；最小化窗口；首次发现时后台索引尚未完成的诚实标注；窗口是否在当前虚拟桌面。
现有 `DriverError` 只有 `CapabilityUnavailable` 等通用项，`list_apps` 返回 `Vec<AppInfo>` 无处表达“仍在索引”，`WindowInfo` 无虚拟桌面信息。执行规程 §5 规定改 03 接口须先决定，因此由总控批准并记录。

## 决定（全部向后兼容）
1. `DriverError` 新增 `ElevationRequired`、`PermissionMissing { permission }`、`SecureDesktop`、`WindowMinimized`，并提供 `error_code()` 到协议错误码的唯一映射（`SecureDesktop` → `handoff_required`）。四者均证明“零副作用”（`maybe_executed() == false`）。`Unsupported` 不新增：沿用 `CapabilityUnavailable`。
2. `Driver::discovery_status() -> DiscoveryStatus { indexing }`，带默认实现（完成），仅 Windows 等有后台索引的驱动覆盖。
3. `WindowInfo.on_current_desktop: Option<bool>`（缺省不序列化）；前台动作对 `Some(false)` 的窗口拒绝，`None` 表示平台无法判断。

## 备选与为什么不选
- 保持接口、让驱动私有元数据绕行：Router 与 batch 无法统一处理，且每个平台各写一套。
- 把 `indexing` 放进 `AppInfo`：它描述的是整次发现而非单个 App。

## 影响
- 03 §5 的 Driver 方法清单不变，新增的是默认方法与错误变体；fake 驱动无需改行为。
- 契约测试框架新增“门禁类错误必须零副作用”的约定（见 tests/driver_errors.rs）。

## 退出条件
若 v2 之后出现需要“部分成功”语义的发现结果，再把 `DiscoveryStatus` 扩展为每项状态。

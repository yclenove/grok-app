# Computer Use：App 授权准备票据与原生选择撤销

日期：2026-09-30。目标 **active / partial — not releasable**。本轮延续完整 Computer Use 最终版目标，没有把成功标准缩减为通过以下测试。上一检查点已形成实现和可核验证据，属于 progress；本轮重新检查实际代码和执行状态后继续实现，不是等待一个未确认存活的任务。

## 实际代码变更

- `SessionGrants::AuthorizationTicket` 新增不可由模型构造的进程内 preparation signal。取消尝试、撤销、失败、forget、grants Drop 都撤销尚未完成的准备；所有已等待及晚到的等待者收到同一持久信号。拒绝旧/外来票据，取消目录代际耗尽也不能丢失撤销信号。
- `publish(finish=true)` 在同一状态锁内完成 preparation。完成票据不能再次开始准备，但不因后续替换尝试失败而自动撤销原 native grant；已完成授权仍由现有 Broker Stop/清理路径管理。没有把准备票据当成输入权限或 UI 所有权。
- 实际 App `computer_use_authorize_surface` 在排队的目标绑定前、返回后检查准备票据。已取消工作不再进入绑定；绑定期间被取消则按精确 session/run 释放晚到的 WebView 绑定，再返回失败。沿用后续 Broker 授权、activate、异步 attach、complete 及失败 fence，不新增桌面回退。
- `PortalRegistry::select_for_authorization` 可绑定真实 SessionGrants 票据；核对 run，在登记前及 owner 启动时重验。协商和已移交原生 owner 均监听准备撤销；撤销后停止公开目标/claim/Ready，并保留原 granting reactor、adapter/native owner，直到精确 join。
- 明确区分“登记表验证不存在此 run”和查找失败：前者 idle=true，使取消发生在原生选择登记之前的 Broker cleanup 可以完成；poisoned lookup、存在但尚未 join 的 owner 仍 busy。晚到的已取消准备票据不能再创建原生选择。
- 本轮没有原生 parent 导出、OS 权限监控或真实 GNOME App 启用。`select_for_authorization` 是已验证的接入边界，不等于整条 App native consent UI 事务已经接通。

## 本轮验证与范围

证据目录：`tools/computer-use-probe/.run/wayland-authorization-20260930/`。主源 **329** 个；相对 registry 检查点改动 7 个已有文件、增加 2 个测试文件。源清单在最终原生回归前冻结并复核；不是最终发行包或最终候选的冻结。

| 范围 | 实际证据 |
|---|---|
| SessionGrants 定向 | **32 passed**，包含新增 6 个 preparation 信号、完成/撤销、容量耗尽、过期票据与隔离用例 |
| Wayland 初始协议回归 | **68 passed / 25 ignored**；此轮明确不把 ignored 当通过，随后对全部原生用例显式执行 |
| Wayland 最终全量 | 同一 executable 三轮各 **93 passed / 0 failed / 0 ignored**，线程 1/4/4，其中 25 个显式 native 用例 |
| 新增跨层回归 | 取消在登记前、协商中、handoff 发布前、完成后；跨 run 拒绝；真实 SessionGrants → Broker/HostOwnedAdapter → PortalRegistry，而非只直接调用 registry.cancel |
| 新增 native 回归 | 私有 D-Bus、实际 PipeWire/libeis：App 等价的 activate 后、commit 前取消；不派发 Broker cleanup，先证明票据信号已 join 原 native owner，节点消失、原 EI peer 断开、输入事件 0、晚 commit/重启选择均拒绝，再完成 Broker Stop |
| 独立原生核对 | **159** 个最终夹具目录，逐轮 53；原始 C EI peer 身份、事件、PNG 28×20 解码像素与 run/target/generation 核对；owned 残留 0；两 run 隔离/去重的既有检查保留 |
| Core | Windows **586**、Linux **585** passed；driver 两平台各 **16**、FFI wait contract 各 **21**（不是 macOS 原生验收） |
| 实际 App Rust harness | 命令 **4**、WebView **69**、session/MCP **21**、feature lifecycle **1**，合计 **95** passed；使用本轮 Cargo 精确产物的副本和已存在的 Common Controls manifest，保留 raw/test 哈希与资源读回；未改原始 build artifact |
| X11 | unit **49**；owned Xvfb **19** 组、GTK/AT-SPI **41** 组通过 |
| 静态/依赖 | Linux core/Wayland/X11 联合严格 Clippy、Windows App check、workspace/命令 fmt、diff 检查；同一原生测试二进制前后哈希一致，动态依赖不包含 libei |

本轮边界审查脚本初版错误地寻找 `portal_capabilities` 中显式的 `native_wayland: false`，实际值来自 `Capabilities::for_surface(WebView)`。保留初版和失败日志，修改审查去核对真实派生值及没有后续覆盖；**没有修改产品能力声明来迎合检查**。源码字符串审查仅作为辅助，不冒充运行时证明。

历史 Windows staging→seed access denied、wrapper 单次 capture-node 快照失败的直接原因仍未确证，本轮通过不能证明已修复。早期失败保留在前序封存证据中；前端上次测试数不冒充本轮重跑，本文不声称进行了新 UI 视觉验收。

## 未完成项与下一步

**App Linux factory 仍仅 X11，`native_wayland=false`；本轮不是安装态 GNOME 或 Grok E4。** 下一个实现环节是将真实 native parent 的导出/释放与票据生命周期结合，再接 OS permission/session lock/user takeover/restore 和完整 App consent 事务；不得以格式正确的假 parent、fixture policy、XWayland 或私有原生回路代替真实平台行为。

完整原范围继续保留：Windows x64、macOS arm64/x64、Linux X11/native GNOME Wayland；Desktop/Managed Browser/Existing Chrome+Edge/App WebView；App/ACP/MCP；完整输入、中文 IME、clipboard、取消与恢复；签名干净安装、更新、repair、rollback、卸载；窄窗口/DPI/权限 UI；真实 Grok E4；**同一最终冻结候选 12 小时 active soak**。这些要求仍未逐项验证完成，不能调用 complete；本轮没有提交、推送、tag、发行、安装更新或向用户桌面发输入，也没有暂停目标。

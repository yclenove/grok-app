# Computer Use：Host-owned Wayland 多 run 注册与授权生命周期

日期：2026-09-30。目标 **active / partial — not releasable**。本轮是实际代码和原生回归推进，不是完整目标验收或发行候选。

## 实际实现

- 新增 `PortalRegistry`，实现生产 `ComputerUseAdapter` 路由和 Host-only `select/state/cancel/forget`。没有全局 current run；每个 run 的原生 consent、target、adapter、停止状态和所有者分别保留。枚举不会启动授权，也不会隐式 claim 或输入。
- `PortalSelection` 是不可从模型 JSON 反序列化的进程内票据，绑定 run 与随机 selection identity。旧票据和其他 registry 的票据不能取消同名替代 run。只有精确原生 owner、adapter worker 和 selection thread 已 join，才允许显式 forget；不自动覆盖保留条目。
- Pending consent 从登记时就计入占用。外来 run、相等/过期代际不能撤销授权；Ready 后使用 adapter 实际推进的 generation，而不是初始 generation。取消或策略失效与原生授权移交竞争时，晚到的 grant 不得发布为 Ready。
- 每个选择保留独立、长寿命的 granting reactor，覆盖 D-Bus negotiation、原生 transport、adapter 和退出。Registry Drop 只请求撤销，由持有 entry 的线程继续完成原 owner 收尾，不以 UI 请求返回、超时或 Closed 信号代替 join。
- 移交改为保留式 slot：Host 校验失败，或 adapter 线程/runtime 创建与交接失败，原 PortalSession/HostSession 仍由调用方持有，可以等待精确 owner，而不是只 Drop 掉 join handle。
- Release 先关闭 registry 入口再 fence adapter；idle 回调返回后重验相同 active adapter 和目标存活，避免将已退休 adapter 的 idle 当成仍在收尾的选择线程已完成。Failed 明确表示收尾未证明，不能 forget 或用新授权覆盖。
- 要求 monitor scope、正 safe-integer generations 和非空 `wayland:` parent 参数。**这里只验证 parent 格式，不证明它由真实窗口导出。** Host policy 仍为必须注入的接口，没有新增“全部允许”的生产实现。

## 本轮验证

证据目录：`tools/computer-use-probe/.run/wayland-registry-20260930/`。

| 范围 | 证据 |
|---|---|
| 源文件 | 327 个主源；相对上一检查点改动 6 个已有文件、增加 3 个 registry/测试文件；不等于最终发行包冻结 |
| 注册表定向测试 | 10 passed：pending、慢关闭、双 run、旧票据/外来 registry、取消与策略失效竞争、Drop、denial、参数拒绝、失败移交保留原 owner |
| Wayland 全量 | 同一二进制三轮 **87 passed / 0 failed / 0 ignored**，线程数 1/4/4；其中 24 个显式原生用例 |
| 新增原生端到端 | 实际 Broker → HostOwnedAdapter → PortalRegistry → 两组独立 private PipeWire/libeis；左右 run 分别 capture/click/去重/Stop，左侧退休不关闭右侧 native peer/capture |
| 原生独立核对 | PNG 解码、精确 run/target/generation、原始 C EI 事件、独立 peer 绑定；153 个最终夹具目录完整归档，归档时 owned 进程残留 0 |
| Linux core / driver / FFI contract | **579 / 16 / 21** passed；FFI contract 不等于 macOS 原生验收 |
| X11 | unit **49**；owned Xvfb **19** 组、GTK/AT-SPI **41** 组通过 |
| 静态检查 | Linux core/Wayland/X11 联合严格 Clippy、Windows App check、workspace fmt、diff 检查通过；动态依赖不包含 libei |

首个测试编译用了不存在的 `has_capture` helper，已改用现有严格 `no_capture_node()` 和实际独立节点快照；编译失败日志保留。早期脚本因 CRLF 重定向失败，改为 LF；没有更改产品行为、放宽测试断言或绕过 native 用例。

上一阶段 Windows staging→seed access denied，以及 wrapper 阶段单次 capture-node 快照失败的直接原因，**仍未确证**。本轮通过不代表修复这些历史失败。Windows core/App Rust/前端测试的上一轮数值不冒充本轮重跑结果。

## 未完成项与下一步

**Linux App factory 仍只接 X11，`native_wayland=false`。** 本轮 registry 走真实 portal session/native adapter，不是 map-only 假 grant，但其接入 App/ACP/MCP 尚未完成：仍需 SessionGrants ticket 与原生 consent 的完整事务、真实窗口 parent 导出/释放、OS permission/session lock/user takeover 监控、恢复策略，以及真实 GNOME 的安装态验收。不能用测试专用 policy、伪造 parent、XWayland 或私有 native fixture 代替这些要求。

异常 owner panic/cleanup failure 保持 Failed/busy 而非伪造成功；操作系统失效后的产品诊断与恢复交互仍属于后续工作。

完整原目标不缩减：Windows x64、macOS arm64/x64、Linux X11 与 native GNOME Wayland；Desktop/Managed Browser/Existing Chrome+Edge/App WebView；App/ACP/MCP；完整输入/中文 IME/clipboard/取消与恢复；签名干净安装、更新、repair、rollback、卸载；窄窗口/DPI/权限 UI；真实 Grok E4；**同一最终冻结候选 12 小时 active soak**。尚未逐项证明这些要求，不能标记 complete。本轮没有提交、推送、tag、发行、安装更新或向用户桌面发送输入。

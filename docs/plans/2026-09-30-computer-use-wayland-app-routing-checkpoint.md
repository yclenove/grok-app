# Computer Use：App 目标选择保留 run 所有权

日期：2026-09-30。目标 **active / partial — not releasable**，不是完整目标完成或发行候选。

## 实际修复

此前 `computer_use_list_targets` 对请求中的 run 做聊天归属校验后，仍调用无 run 的 `list_targets_for_surface`。这会丢失已授权 portal / retained WebView 适配器需要的身份；产品 wrapper 已透传 run 并不能修复这层 App 入口。

- Broker 增加 `list_targets_for_surface_in_run`。Desktop 和 WebView 使用精确 run 枚举；先检查 feature/run，失败不调用 native adapter。
- App 命令保持 `cu_blocking` 非阻塞调度。共享实现先经 SessionGrants 和 Broker 双重聊天所有权校验，再将已校验 run 传给 picker，而非仅检查后丢弃。
- 首次授权前仍可无 run 发现目标。Managed Browser profile 和用户共享 Existing Tab 仍是 Host 授权前候选，不替换成当前模型已经拥有的 tabs，也不回退到 Desktop。
- 空白聊天、外来聊天/请求 run、未知 surface，以及“全局记录正确但传入 Broker 的 owner 不同”均拒绝。枚举不隐式授权、claim 或输入。
- 实际 PortalAdapter 直连和经过 HostOwnedAdapter 的原生回路现在同时执行 App 使用的 Host picker API：无 run 拒绝，外来 run 无目标，本 run 返回唯一保留目标，之后原有 PNG/EI/去重/代际/Stop 回路不变。

Core 新增三个测试；浏览器测试使用真实非空 profile 目录和显式共享的未授权 tab，避免用两个空列表相等作为兼容证明。App 测试执行与命令相同的实现，不是另写一份路由。`test_support` 仅在 `cfg(test)` 下扩大到 crate 内可见，生产依赖和权限没有放宽。

## 验证及边界

证据目录：`tools/computer-use-probe/.run/wayland-app-routing-20260930/`。

| 范围 | 当前证据 |
|---|---|
| 主冻结源 | 324 个；相对 wrapper 检查点已有冻结中 4 个变化、3 个纳入冻结；不是整个发行包的冻结 |
| 原生 Wayland | 同一二进制三轮 **76 passed / 0 ignored**，含 23 个显式 native 用例；串行一次、四线程两次 |
| Linux core / driver / FFI contract | **579 / 16 / 21** passed；FFI contract 不等于 macOS 原生验收 |
| Windows core | 首次 **579 passed / 1 failed**；同一 binary 的单项诊断 1 passed，四线程全量诊断 **580 passed**；首次失败未被覆盖或判定已修复 |
| Windows driver / FFI contract | **16 / 21** passed |
| App Rust 测试 | 命令 **3**、WebView **69**、聊天/MCP 生命周期 **21**、feature-off 生命周期 **1**，共 **94** passed |
| 前端相关回归 | **20 files / 184 tests** passed；不是 rendered UI 或实际 Grok E4 |
| X11 | unit **49**，owned Xvfb native **19** 组，GTK/AT-SPI **41** 组 |
| 静态检查 | Linux core/Wayland/X11 联合严格 Clippy、Windows core 严格 Clippy、Windows App check、workspace 和 include 文件 fmt、diff 检查通过 |
| 独立原生核对 | 解码 PNG，按 nativePeerRoot 关联原始 C EIS 事件和 run/picker target；141 个最终夹具目录已归档，归档时 owned 残留 0 |

App 测试最初存在 `test_support` 私有模块编译错误，修正 test-only 可见性后又遇到已知 Common Controls v6 test harness 启动错误 `0xc0000139`。按仓库 CI 路径，对本轮 Cargo JSON 明确返回的测试 executable **副本**用现有 `windows-test-manifest.xml` 后链并读回 manifest，再运行测试。没有修改产品 build.rs、用户安装或缓存原 executable；原始失败保留。

原生测试报告最初直接序列化没有 Serialize 的 TargetInfo，编译失败保留为 `report-serialize-build-failure.log`。只改为写出经过断言的 targetId/kind，再重新冻结；没有给生产类型添加序列化能力。

## 仍未确定的失败

`final-windows-core.log` 的 `tiny_prepare_is_deterministic_check_is_readonly_and_chromium_tamper_fails` 在第一次 prepare 的 staging→seed rename 返回 Windows access denied (5)。单独重跑和同一 executable 四线程全量均通过，但**不能据此声称根因已修复**。保留原日志、harness hash 和候选临时目录只读取证；失败测试未打印 UUID，候选目录与失败调用的关联也明确未证明。没有更改 runtime publication 的断言、权限、等待、清理或回滚算法来使测试变绿。

上一 wrapper 阶段的单次 `capture node leaked after Closed` 直接原因仍未确证；其独立确定性 teardown 顺序修复和原始失败证据继续保留，不被本轮三次通过抹去。

## 下一步与完整目标

**这不是已接线的 native GNOME App。** Linux App factory 仍只接 X11，`native_wayland` 没有改成 true。下一步是 Host-owned 多 run portal registry、授权前原生 consent 与真实 parent handle、长寿命 runtime/撤销/Stop 生命周期，以及实际 OS permission/用户接管/恢复策略；不能用允许全部的 policy、XWayland 或仅探针通过替代。

原目标仍包括 Windows x64、macOS arm64/x64、Linux X11 + native GNOME Wayland；Desktop/Managed Browser/Existing Chrome+Edge/App WebView；App/ACP/MCP；完整输入/中文 IME/clipboard/取消与恢复；签名的干净安装、更新、repair、rollback、卸载；窄窗口/DPI/权限 UI；真实 Grok E4；**同一最终冻结候选 12 小时 active soak**。这些未完成项没有删减。本轮未执行提交、推送、发行、用户安装更新或用户桌面输入。

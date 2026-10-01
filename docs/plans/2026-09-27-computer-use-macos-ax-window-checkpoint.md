# macOS AX 窗口实例绑定检查点

本机时间：2026-09-27，Asia/Shanghai（终端时钟已核对）。
接续 [进程出生身份检查点](2026-09-27-computer-use-macos-identity-checkpoint.md)。
完整目标“接手 grok 的工作，完成 computer use 所有功能开发，推进到最终版”仍为 **active**。
未 commit、push、PR；没有替换、关闭用户 App 或授予系统权限。R17 UI/UX 源码保留，
本批没有把后台改动冒充新的界面验收。

## 本轮实际进展

上一轮分类为 **progress**：前一检查点的生产源码和日志存在，实际读到 Core 531/531、
FFI 18/18。不是等待某个旧任务，也没有重启旧任务。本轮在当前工作树上继续实现：

- 新 `macos_adapter/ax_window.rs` 保留公开 AX 原生窗口代理。先验证进程出生身份及
  CG 窗口在只读测试点的前后遮挡归属，再取得 AX 命中元素的 `AXWindow`，核对 PID、
  类型、角色和边界。测试点只用于身份查询，不用于猜测语义点击。
- 为每个已保留的 AX 窗口实例生成独立 nonce，目标 ID 为
  `mac:<pid>:<wid>:<birth-seconds>:<birth-microseconds>:<32-lowercase-uuid>`。
  重复发现同一代理保留 nonce；同 PID、出生时间、WID、标题和边界但 AX 对象不同的替身
  不继承原授权。未知 nonce、另一适配器的 nonce 及旧三/四/五段 ID 一律拒绝。
- liveness、截图前后、坐标映射及输入边界都检查保留代理与当前代理；比较 `CFEqual`
  之后仍查询保留对象本身。观察到 CG 窗口消失/失去资格时删除其 nonce，不因后续数值
  ID 再出现而复活。新窗口必须重新发现、授权、取得模型截图。
- `FrameRegistry` 同样要求完整实例 ID，移除仅为旧纯测试保留的五段 key 发布兼容分支。
  截图元数据保留完整进程出生身份；发布/查询权限按 run 和完整实例 ID 隔离。
- 事件配置完成后重新核对截图坐标映射、几何、显示拓扑和遮挡，而非只比较 AX 身份。
  按下之后窗口被替换时不向替代窗口发送 release；不确定占用在 Stop 后仍保持，
  没有把返回或取消伪造为原生物理完成。
- AX 对象逐个设定 0.2 秒消息 timeout，解析预算 2 秒并检查取消。AX 访问通过互斥锁
  串行化；发现/校验争用时不排队。删除已死身份可能等待当前查询；Stop 不取得此锁。
  Create/Copy 对象 RAII 释放，最多保留 128 个窗口代理；测试分别统计瞬时 CF 对象和
  保留 AX 对象，并显式销毁适配器确认零残留。
- 权限/能力如实收紧：截图和输入都需要 Screen Recording 与 Accessibility，以及
  可以证明归属的暴露窗口。AX 树、语义操作、文本、按键、滚动和拖拽能力仍未开放。

## 已复现的失败与最终验证

证据目录：`tools/computer-use-probe/.run/macos-ax-window-20260927/`。

1. 首次窗口实例调用链 **28/28**：`ax-first-contract.log`。
2. 新增两项边界反例，实际 **28 passed / 2 failed**：`ax-boundary-red.log`。
   事件配置中同一窗口移动仍发送旧坐标；观察到窗口消失却没有删除保留身份。
   修复后 **30/30**：`ax-boundary-green.log`，断言未放宽。
3. 旧五段 key 发布仍被允许的反例实际 **0 passed / 1 failed**：`ax-legacy-red.log`。
   移除兼容分支、迁移原有纯测试到完整实例 ID 后，纯合同 **16/16**：`ax-legacy-green.log`。
   最初 `--exact` 配了短前缀，运行了零项；保留 `ax-legacy-filter-miss.log`，不计为验收。
   纯合同命令同时指定的 integration target 也被名称过滤为零项，不冒充 30 项回归。
4. 最终无名称过滤的串行全量 **Core 535/535（111.01 秒）+ FFI 30/30**，终态 exit 0：
   `ax-acceptance-final.log`。先前中间版本 **534/534 + 30/30** 另存于
   `ax-core-and-contract-full.log`，没有覆盖旧结果。
5. 严格 all-targets Clippy `-D warnings` exit 0：`ax-clippy-acceptance.log`。
   scoped rustfmt exit 0：`ax-format-final.log`。相邻七个源码文件尾随空白零命中。
6. 全量测试启动前记录 **153 个 Rust 源码/测试/依赖清单文件** SHA-256，结束后核对无漂移。
   `source-before-final.json` 和 `source-receipt.json` 保存源码及日志/API 文档指纹。
   使用原来的独立 Windows current seed，没有修改用户 App 或用户运行时目录。

**这些是 Windows 执行生产 Rust 适配器并链接系统 FFI 替身的测试，不是 macOS 真机验收。**
替身 PNG 仍为 transport sentinel；不证明真实 ImageIO 输出、macOS 链接/SDK、权限 UI、
AX server 对象唯一性、并发线程行为或真实点击效果。CI 已有非 macOS contract 步骤会
运行这些用例；本轮未推送、未运行远端 CI。

## 公开接口核对

读取并保存了 Apple 官方 DocC 数据中的 AXUIElement / AXUIElement.h，以及
`AXUIElementSetMessagingTimeout`、`AXUIElementCopyElementAtPosition`、
`AXUIElementCopyAttributeValue`；另外保存 Rust accessibility-sys 维护者的 FFI 声明。
API 文档确认可对 AX 代理使用 CF polymorphic 操作；timeout 属于单个代理，不自动传播
到与之相等的新代理，因此每个新 Create/Copy 元素都必须设置。命中坐标为 Float/f32。
这些核对不是 AX 生命周期不复用或跨线程安全的原生证明。

- `https://developer.apple.com/tutorials/data/documentation/applicationservices/axuielement.json`
- `https://developer.apple.com/tutorials/data/documentation/applicationservices/axuielement_h.json`
- `https://developer.apple.com/tutorials/data/documentation/applicationservices/1459345-axuielementsetmessagingtimeout.json`
- `https://developer.apple.com/tutorials/data/documentation/applicationservices/1462077-axuielementcopyelementatposition.json`
- `https://developer.apple.com/tutorials/data/documentation/applicationservices/1462085-axuielementcopyattributevalue.json`
- `https://raw.githubusercontent.com/tmandry/accessibility/master/accessibility-sys/src/ui_element.rs`

## 仍须推进的交付范围

1. macOS arm64/x64 真机编译、签名和权限测试；真实 AX 对象的销毁、重用、exec 转换、
   线程/生命周期行为必须验证。当前代理保留与 nonce 不是原子绑定证明，也没有 AXObserver
   销毁通知。不能声称所有同进程窗口重用已经闭合。
2. 当前只能为存在可核实暴露测试点的窗口建立/验证身份，完全遮挡或 AX 不支持的窗口
   拒绝继续。这是未完成的产品限制，不把它改写成最终验收范围。逐窗口 AX 预算也不是
   整个列表的期限；CG 同步调用耗时、全列表上界和慢应用行为须继续处理。
3. 查询与 CGEventPostToPid 之间仍非原子；原生队列完成、即时停止与不确定 release 的
   实际恢复未完成。AX 树/语义引用、完整输入、ScreenCaptureKit 仍属于最终交付要求。
4. Windows/浏览器退出长尾、WebView 运行中取消、X11 完整文本/恢复、GNOME 原生 Wayland、
   正式扩展、App/ACP/MCP 全链、签名安装/升级/回滚、重设计 UI 的原生窄窗/缩放/权限矩阵、
   真实模型 E4 与冻结 12 小时 active soak 继续。以上 565 项通过不能代替完整最终版验收。

下一步从保留 AX 窗口实例继续实现窗口内有界 AX 树和精确元素引用/动作，并补齐真正的
生命周期与原生验收；不要回退到标题、几何哈希、伪 AX root 或全桌面输入兜底。

# macOS 单窗口截图、坐标与回归检查点

本机日期：2026-09-27，Asia/Shanghai；最初的纯坐标测试于 9 月 26 日执行。
分支仍为 `feat/computer-use-implementation`。完整 Computer Use 最终版目标保持 active；
本批没有 commit、push、PR，没有替换、关闭或授权用户 App。

## 根因与改动

旧 `macos_adapter.rs` 把 `OnScreenOnly`（1）与一个 window ID 传给截图接口，
使用手写无限矩形；它不是“仅此窗口”的捕获选择。旧截图转换假设 32 位 BGRA，
未验证实际像素格式和完整缓冲区，转换失败还会遗漏图像释放。旧点击把截图像素直接
加到 Quartz 屏幕点坐标，Retina 映射错误；语义点击竟回落到固定 `(24,24)`。

当前源码：

- 单窗口选择使用 `CGRectNull`、`IncludingWindow`、`BoundsIgnoreFraming | BestResolution`。
  没有整屏失败回退。捕获前后重查同一 PID/window ID 的边界与显示器配置；变动不发布截图。
- 由 ImageIO 编码实际 CGImage，删除未经检查的 BGRA 指针遍历。检查图像大小、长度及
  上限；Create/Copy 对象使用 RAII，分配/编码失败也释放成功创建的对象。
- 新 `quartz_frame` 以每个 run/target 的模型截图绑定输入，将真实图像像素映射到窗口点坐标。
  负屏幕原点、Retina、窗口移动/缩放、显示器变动、越界、旧截图和跨 run 均有检查。
- 预览不创建也不替换模型权限；新模型捕获开始即退休旧截图。失败、Stop、释放或被替代的
  捕获不能在迟到返回时恢复旧权限。几何 revision 限制在 JavaScript 精确整数范围内。
- 点击只接受当前截图坐标；增加目标点遮挡检查并定向到目标 PID。取消后只尝试原输入的
  release；无法确认目标仍适用时不向替代窗口补发事件，并保留不确定输入占用。
- 删除假 AX root、固定点语义点击和无绑定的全局 Unicode/SetValue 路径。能力表如实标记
  尚未实现的 AX/text/key/scroll/drag，不再仅因 Accessibility permission 存在就声称支持。
  **这不是缩减最终范围：这些完整能力必须继续实现并通过真机门禁。**

## 已执行证据与保留失败

日志根：`tools/computer-use-probe/.run/macos-frame-20260927/`。

- 初始纯逻辑：9/9，日志在相邻 `macos-frame-20260926/core-frame-tests.log`。
- 新边界反例首先 **9 passed / 2 failed**：公开边界结构中 NaN 被接受、u64 revision 超出
  JavaScript 精确整数范围。修复后 **11/11**：`frame-boundary-red.log` / `frame-boundary-green.log`。
- 直接编译并调用生产 `macos_adapter.rs` 的 **9/9 FFI contract**：选择参数、编码所有权、
  2× 坐标、预览隔离、失败退休、权限/错误目标/遮挡拒绝、取消、双击中断及不确定 release。
  `macos-ffi-contract-green.log`。首次引入的子模块路径编译错误保留在 `macos-ffi-contract-first.log`；
  修复明确的 `#[path]` 后通过，不隐藏首次失败。
- `cargo clippy -p grok-computer-use-core --all-targets --offline --target-dir target-cu -- -D warnings`
  已通过，包含生产适配器的非 macOS FFI contract 编译：`core-clippy-first.log`。
- Windows 全量 Core 使用既有独立 current seed 首轮 **530/530，114.77 秒**：
  `windows-core-full.log`。修正仅有的模块声明排序格式问题后，最终在一条命令中重新执行
  **Core 530/530，109.16 秒 + FFI contract 9/9**，终态 exit 0：`core-and-ffi-final.log`。
- scoped rustfmt 最初只报告模块声明排序，原日志 `format-check.log` 保留；修正后
  `format-check-final.log` exit 0。CI 的非 macOS 两个 runner 增加同一 FFI contract 命令，
  名称明确注明 doubles/non-native；**尚未推送或实际执行远端 CI**。

**证据边界：**上述 FFI 测试执行生产 Rust 调用链，但系统函数是测试替身，编码器输出也是
明确标记的 transport sentinel，不是原生 PNG/颜色正确性的证明。没有运行 macOS，
没有证明 Apple framework ABI/链接、权限 UI、窗口实际内容、真实输入效果或签名安装包。
CGEvent 队列返回也不证明应用已处理输入；结果仍是 unverified，不能冒充原生物理完成。

源码与日志 SHA-256 另存 `source-receipt.json`，用于区分本批源码与此前 R17 原生 UI exe。

## 仍需完成的 macOS 工作

1. 实际 Apple Silicon / Intel 编译与独立原生 fixture：窗口图像只含授权窗口、阴影与透明度、
   混合 DPI/多屏/旋转/负原点、窗口变动和权限拒绝；实际像素/独立效果计数验收。
2. 用精确进程出生身份与窗口/AX 实例绑定解决 PID/window ID 重用；当前 ID 不证明出生身份。
3. 完整 AX 树和 opaque element refs；真实 semantic click/SetValue/Unicode 编辑/选择与 IME。
   键盘、滚动、拖拽、前台/用户接管与受保护窗口策略不能用坐标猜测或全局输入代替。
4. 原生事件确认、即时 Stop、实际队列/物理完成与不确定 release 的恢复。检查与派发之间
   尚非原子；不要把本批的 slot guard 当作所有 macOS 竞态均已关闭。
5. 评估并落地当前 SDK 的 ScreenCaptureKit 路径；当前仍使用 Quartz capture，
   未验证所有目标 macOS 版本/SDK 的可用性。不得用旧 API 的 mock 结果宣称最终兼容性。
6. 签名资源 archive 交付、私有 runtime 解包、arm64/x64 安装/更新/回滚和 UI 权限流程。

## 全局范围不变

R17 UI 改版及原生有限验收保留，窄窗/完整控制/完整安装版仍未闭合。Windows/浏览器退出长尾、
WebView 物理取消和关闭性能、X11 真实 WM/完整文本选择与恢复、GNOME 原生 Wayland、
正式 Chrome/Edge 扩展、全部平台安装/更新/回滚、真实 Grok E4 和冻结 12 小时 active soak
仍需要各自证据；不能据本批测试宣称整个 Computer Use 已完成。

## API 核对来源

2026-09-26/27 直接读取 Apple 开发者文档的 JSON 及维护者 bindings，未读取登录凭据：

- `https://developer.apple.com/tutorials/data/documentation/coregraphics/cgwindowlistcreateimage.json`
- `https://developer.apple.com/tutorials/data/documentation/coregraphics/cgwindowlistoption/optionincludingwindow.json`
- `https://developer.apple.com/tutorials/data/documentation/coregraphics/cgwindowimageoption/boundsignoreframing.json`
- `https://developer.apple.com/tutorials/data/documentation/imageio/cgimagedestinationcreatewithdata(_:_:_:_:).json`
- `https://raw.githubusercontent.com/servo/core-foundation-rs/master/core-graphics/src/window.rs`

部分旧 Apple archive / 错误 URL 返回 404，未把这些请求当作核对成功。

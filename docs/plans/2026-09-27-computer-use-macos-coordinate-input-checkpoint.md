# macOS 截图坐标绑定文本与命名键检查点

本机终端时间：2026-09-27 05:06，Asia/Shanghai（+08:00；UTC 2026-09-26）。
HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`；未 commit/push/PR。
完整目标“接手 grok 的工作，完成 computer use 所有功能开发，推进到最终版”保持 **active**。
接续 [精确控件文本追加](2026-09-27-computer-use-macos-text-checkpoint.md)。
本轮首先核验上轮 source-receipt 的 256 项，零漂移，确认上轮为 progress；没有把旧会话当作仍在后台运行。
保留用户 App、运行时、权限与 R17 UI，没有授予权限或替换安装。

## 1. 本轮实际生产修改

- 新 `src-tauri/src/computer_use/macos_adapter/coordinate.rs` 为显式请求的 `Key` / `TypeText` 坐标绑定目标。
  必须是当前 run 已发布、带截图的模型观察；预览、无图观察、旧 snapshot、旧 geometry、跨 run、已退休引用均不能取得权限。
- 坐标使用既有 CapturedFrame 的 image-pixels 映射，保留负原点和实际截图缩放；再次核验原窗口实例、显示修订及请求点未被其他窗口遮挡。
- 原生 `AXUIElementCopyElementAtPosition` 命中对象必须与该模型观察中保留的 AX 对象精确相同，AXWindow 必须是授权原窗口。
  不选最近控件，不向上猜父控件，不把窗口背景当作当前焦点输入目标。
- 请求点和传给 AX 的 Float 舍入后点都必须在目标窗口和观察控件的半开边界内；拒绝 NaN/无穷、越界及舍入跨边界。
  这修复了 `(39.999999, 80)` 原本在控件外，却因 Float 舍入进入其左边界而获得输入权限的真实反例。
- 坐标命中后复用既有精确引用/祖先/角色/启用/可见/焦点链和 native action 占用，不复制另一份较弱派发逻辑。
  只对观察时已经发布相应动作、派发时仍聚焦的目标输入；不点击抢焦点，不发全桌面键盘事件。
- 每阶段派发检查重新验证 live hit。命中变化、遮挡、控件几何变化、权限撤销、安全角色变化或 Stop 中止后续文本/按下。
  同时为原精确引用路径补上最终控件 bounds 检查，不能只验证窗口 bounds。
- 语义引用查找失败不转为坐标；坐标 SetValue 不伪装成追加；显式 clipboard 请求仍拒绝而不替换机制。
- 命名键继续预分配按下/释放，取消后只清理原窗口内自身的键对；Tab 可自然移动同一窗口内焦点。
  文本继续保留原文、按末尾零长度选区追加，空字符串 no-op；未知 native 完成或 key-up 保留占用并禁止重放。
- 能力和 CI 合同入口同步更新。结果仍为 applied/unverified，不把 FFI 成功当作已验证的目标后置条件。

## 2. 红测、修复与可复现范围

证据目录：`tools/computer-use-probe/.run/macos-coordinate-input-20260927/`。

- `coordinate-red.log`：2/2 失败，明确旧代码没有坐标 Key/TypeText 路径；接入后 `coordinate-first.log` 为 2/2。
- `coordinate-boundaries-red.log`：14 passed / 2 failed。
  一个是真实 Float 跨控件边界错误，已通过双重半开边界校验修复。
  另一个是新增测试错误假定 600×400 窗口可接受 900×1000 截图；既有捕获层正确拒绝长宽比不匹配。
  未放宽生产捕获校验：缩放测试改用真实等比例 1×/1.5×/2×，另增明确拒绝畸变截图的用例。
- 新 coordinate 文件最终 **21** 项测试，多项逐一遍历错误类型、原生命中/空返回/错误码、保护状态、窗口/控件身份、缩放、背景与边界、模型授权、输入期间 Stop/取消、未知完成等变体。
- FFI double 新增请求点专用命中与回调；窗口绑定的其他只读探测点不被该注入伪装成同一测试阶段。
  原有测试中“所有坐标均不支持”的能力断言同步为已实现路径；拒绝测试仍明确使用未命中控件的背景点，不删除隔离约束。
- 没有读取 AXValue/安全文本、没有焦点点击/鼠标兜底；每条路径检查保留 CF/AX 对象释放。

## 3. 冻结源码验证

- Windows Core **547/547**，用时 99.92 秒；七组 Mac FFI 合同 **150/150**（AX 27、capture 33、coordinate 21、keyboard 17、pointer 18、text 20、value 14）。总计 **697/697**，零失败/忽略，exit 0。
  使用独立 `windows-seed-current`，实际日志/退出码为 `core-final.log` / `.exit`。
- Debian/WSL 七组 Mac FFI **150/150**，exit 0；不是 Linux 原生 Mac API 验收。
- Windows 与 Linux Core all-targets Clippy `-D warnings` 均通过；8 个本轮直接修改 Rust 文件格式检查通过。
- 227 项选定生产/测试/依赖/CI 文件在回归前冻结，全部测试和 Clippy 完成后 before/after **零漂移**。
- 本轮所有 test/clippy 句柄均已正常退出，无未确认后台任务；source-receipt 另纳入本检查点、索引与实际日志。
- 未改 browser production、Windows/X11 后端或 App UI，不借用历史原生结果证明本轮 Mac 修改。

## 4. 仍未完成的真实范围

- 此次新增的是 **截图点绑定到已观察、已聚焦 AX 控件** 的文本/键路径，不是任意像素表面输入的全部实现。
  AX 缺失/截断、自绘控件或命中对象与可编辑后代/父控件不同的场景不能宣称已经支持；需要继续按真实控件行为开发和验证，不能把拒绝这些场景当作全功能完成。
- Mac arm64/x64 SDK/ABI、真实 Cocoa/WebKit 控件、AX 命中粒度、权限切换、真实队列处理与异常释放/恢复仍 not_run。
  下一项原生工作仍需 owned Cocoa fixture，验证真实截图→命中→聚焦→文本/键效果，不能只运行 capability/list-target 的 selftest。
- Hit-test、焦点读取、选区 setter 与事件队列并非原子事务；检查后外部输入/覆盖层/焦点变化仍可能竞态。没有通过增加确定性 double 测试声称原子隔离已成立。
- 显式 clipboard、真实 IME、完整输入/持键恢复仍开放；原浏览器导航 504 和 native/worker 退出长尾没有新关闭证据。
- 原目标全范围保留：Windows、macOS arm64/x64、Linux X11/GNOME native Wayland；Desktop/managed browser/existing tabs/App WebView；App/ACP/MCP；完整输入/生命周期/取消/恢复；签名安装/更新/回滚；重新设计的原生 UI 窄窗/缩放/权限矩阵；真实 Grok E4 与冻结源码 12h active soak。
  每项需要相应范围的当前证据；本轮 697/150 不等价于最终版验收。目标继续 active。

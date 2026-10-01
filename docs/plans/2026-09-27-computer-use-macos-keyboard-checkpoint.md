# macOS 命名键输入与浏览器导航诊断检查点

本机终端：2026-09-27 04:39，Asia/Shanghai（+08:00；UTC 2026-09-26）。
HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`；未 commit/push/PR。
完整目标“接手 grok 的工作，完成 computer use 所有功能开发，推进到最终版”仍为 **active**。
接续 [指针输入与滚动](2026-09-27-computer-use-pointer-scroll-checkpoint.md)；不改用户 App、运行时和权限，不回退 R17 UI。

## 1. 本轮真实实现

- 新 `src-tauri/src/computer_use/macos_adapter/keyboard.rs` 接入 CGEventCreateKeyboardEvent / CGEventPostToPid；只支持协议已有的 14 种命名键及其别名，不新增组合键或把不支持键替换成 Enter。
- 观察仅在可见、启用、非安全控件自身 AXFocused、应用前台、AXFocusedWindow 和 AXFocusedUIElement 均与保留对象吻合时发布 `key`。
  动作沿现有精确引用、父子关系、窗口实例、截图几何/显示修订和模型观察链验证，不用点击获取焦点，不做语义到坐标或桌面回退。
- 私有事件源，显式清除事件修饰键；按下与匹配释放全部预分配。任何源/事件分配失败发生于输入之前。
  派发前检查 HID 与 combined-session 的持键/修饰键，事件配置和最终 AX 读取后重查权限、焦点、身份和当前观察，取消不得产生新按下。
- Tab/Enter 可以自然改变同一原窗口内的焦点；自己的 key-up 不要求原控件仍聚焦，但要求原窗口/进程/几何、前台、权限及物理持键边界仍可验证。
  Stop/观察退休后允许这个匹配释放；窗口替换、焦点跨窗口、失去权限等导致释放不明时保留 NativeActionSlot 占用，Stop 不伪造 idle，其他 run 不抢占。
- `key.semantic` 仅在 Screen Recording 与 AX 权限可用时声明；`key.coordinate`、TypeText/追加、IME 仍未实现，不冒称能力齐全。
  返回只表明事件已排队，`verifiable=false`、`postcondition_ok=false`；不把 void FFI 调用当作目标已处理的证明。

## 2. 红测、修正及测试模型边界

证据目录：`tools/computer-use-probe/.run/browser-navigation-input-20260927/`。

- `keyboard-red.log`：新增 5 项调用链全部失败，明确此前缺少 Key 实现；初次接入后 5/5。
- `keyboard-boundaries-red.log` 保留测试错误使用不存在枚举名导致的编译错误，不算产品缺陷。
  更正枚举名后，`keyboard-boundaries-red-compiled.log` 为 11 passed / 4 failed。
- 修复真实反例：派发前未检测其他合成持键；最后焦点读取可以退休模型观察而仍按下；按下后观察退休仍返回成功。
  最终测试还覆盖焦点读取中变成安全控件、窗口/控件实例或几何改变，事件分配失败、错误参数、旧/预览/跨 run 引用、取消及 Stop 竞争。
- 同时纠正一项过强测试假设：原测试要求 key-down 后仅凭 combined-session 状态区分“自己的按下”和“另一发送者的同键按下”。
  本轮保存的 Apple DocC 明确该表合并所有事件源，不能作为所有权 oracle。若直接用于拒绝匹配 key-up，会把自己的按下误当外来输入。
  改为派发前检查合成持键，清理时仍保护独立 HID 物理持键；新增自己的合成按下不得阻止匹配释放的明确用例。
  这不是声称外部合成发送者竞态已解决；不同发送者的原子隔离、真实队列完成和恢复仍未证明，属于最终原生矩阵的开放项。
- 共 **17** 项 keyboard 测试；多项内部逐一遍历全部键、权限/焦点/身份变化以及前后派发边界。
  FFI 回调不在 extern C 内断言，不强行释放用户持有键，不读取 AXValue/安全字段、不碰剪贴板。

## 3. 浏览器先前 504 未被虚假关闭

- 保留上轮 `macos-pointer-actions-20260927/browser-scroll-contract-final.log` 的 29/31 失败；初始 `/goto` 返回 504 的根因仍未确认。
- 修改仅在 `typed-act-http.test.mjs`：有界、受控路由的 request/finish/close 时间序列，失败当时捕获 worker 日志尾部和类型化回复，避免后续场景覆盖关键现场；不把请求体或认证头写入诊断。
- 原来忽略的 wait 场景 `/close` 响应增加状态和原 profile 断言，不把未验证关闭当作成功。
- 不改 `navigation.mjs` 的 8000ms 原生截止，不增加自动重试，不放宽 unknown/no-replay 或快照断言。
- `browser-debug-baseline.log`：同一原始两文件套件 **31/31**、exit 0、48.64 秒。
- `browser-debug-concurrent.log`：原两文件加 navigation 单元合同，和完整 Windows Core 回归并行，**37/37**、exit 0、56.47 秒。
  此负载不等于上轮所有并行构建/原生流程，不足以证明压力根因消失，也不是 12 小时 active soak。

## 4. 冻结源码的终态回归

- `core-final.log`：Windows Core **547/547**（114.80 秒），AX **27**、capture **33**、keyboard **17**、pointer **18**、value **14**，共 **656/656**，exit 0，无测试名称过滤。
- `linux-macos-ffi-final.log`：Debian WSL 同一生产 Mac 调用链与 FFI 替身 **109/109**，exit 0。
  Windows/Linux 上的 Mac FFI 替身不是 Mac SDK、arm64/x64 ABI、真实焦点/键盘布局、系统权限或效果验收。
- `core-clippy-final.log`、`linux-core-clippy-final.log`：两个宿主 Core all-targets `-D warnings` 均 exit 0。
- `format-final.log`：五个直接修改 Rust 文件格式检查通过；browser 测试文件 `node --check` 通过。
- CI 非 Mac 宿主合同入口增加 `macos_keyboard_contract`；远端 CI 未在本轮执行。
- `source-before-final.json` / `source-after-final.json`：选定 Core 源码/测试、Mac 适配器、browser 顶层模块及直接夹具、依赖/CI 共 **223** 个文件，**0 漂移**。
  这是选定范围的指纹，不是全部产品/发行包的可复现构建或签名证明。完整本轮日志/文档指纹见 `source-receipt.json`。
- 已取得终态的句柄：baseline 41048、Core 97692、concurrent browser 44860、Linux 32522；均 exit 0。
  没有用文件或对话意图代替进程存活/完成证明。

## 5. 完整目标保持开放，不缩小为已做部分

1. Mac arm64/x64 真机编译、签名、权限、焦点/键盘/布局和控件生命周期；完整 TypeText/追加/IME/坐标 key、ScreenCaptureKit、排队完成与物理恢复。
2. Windows 与浏览器完成/退出长尾（含原 504）、X11 全输入和恢复、GNOME 原生 Wayland。
3. Desktop / managed browser / existing tabs / App WebView，以及 App / ACP / MCP 全链、WebView 运行中取消和正式扩展。
4. 所有目标平台的签名安装、更新与回滚；保留并完成重设计 UI 的原生窄窗、缩放、权限和交互矩阵。
5. 真实 Grok E4、冻结后的 12 小时 active soak 和逐项最终交付审计。

本轮判定 **progress**：实际修改生产调用链和验收测试，复现后修复边界，取得终态与指纹证据。
下一步继续 Mac 完整文本输入/恢复与真实宿主证据；浏览器原负载超时保留为待定位，不因两次绿灯关单。
没有调用 complete、blocked 或 paused，完整目标 active。

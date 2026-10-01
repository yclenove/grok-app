# macOS 精确控件文本追加检查点

本机终端：2026-09-27 04:55，Asia/Shanghai（+08:00；UTC 2026-09-26）。
HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`；未 commit/push/PR。
完整目标“接手 grok 的工作，完成 computer use 所有功能开发，推进到最终版”继续 **active**。
接续 [命名键与浏览器导航诊断](2026-09-27-computer-use-macos-keyboard-checkpoint.md)。
本轮开始核验上轮 256 项 source-receipt，无漂移；上轮属于真实 progress，不是等待或状态复述。
不改用户 App、运行时或权限，不回退 R17 UI。

## 1. 实际实现

- 新 `src-tauri/src/computer_use/macos_adapter/text.rs` 接入生产 `TypeText` 语义追加路径。
  仅向已聚焦、可见、启用、非安全文本控件的精确保留 AX 引用派发；不通过点击抢焦点，不做坐标/桌面兜底。
- 逐控件检查 `AXSelectedTextRange` 与 `AXSelectedText` 可写性，并校验 `AXNumberOfCharacters`、CFRange 类型与范围。
  追加端点来自控件原生字符计数，不以 Rust Unicode scalar 长度计算原有文本位置；不读取或替换整个 AXValue。
- 文本、属性名与插入范围均在首次选择区写入前分配。先将选择区设为原文末尾的零长度，再读取确认选择区/字符数，最后写一次 AXSelectedText。
  原有非空选择区不得被直接替换；控件忽略选择区设置、用户编辑改变端点时拒绝追加，不猜位置、不重放。
- 最终焦点/窗口/身份验证可能再次改变选择区，因此其后重新读取端点，随后只做权限、取消及模型观察授权检查，再写文本。
  CFIndex 必须是无损可转换的非负有界整数；CFRange 类型必须是 4，拒绝负数、溢出、错误类型和超范围。
- 空字符串是仍需精确授权的 no-op，不移动选择区。中文、emoji、组合字符、换行及协议字符上限走相同路径。
  非法/NUL/超限文本和显式 `via: clipboard` 请求拒绝，不偷偷改用其他路径。
- 控件变为安全字段后，先检查角色/子角色，再继续文本可写性、字符数或选择区查询。
  取消/Stop/目标释放或模型观察退休阻止后续文字写入，保留对象不等于仍有输入授权。
- 选择区或文本 native setter 非零返回均视为完成未知，保留 NativeActionSlot 占用并禁止重放；不因 Stop 冒称 idle。
  成功仅返回 applied/unverified，`verifiable=false`、`postcondition_ok=false`，不把 AX 返回值当作目标后置条件已证明。
- 接入节点 action 列表、能力声明、现有 exact-reference 分发及非 Mac CI FFI 测试入口；原 SetValue 仍保持替换语义。

## 2. 反例与测试模型

证据目录：`tools/computer-use-probe/.run/macos-text-append-20260927/`。

- `text-red.log`：最初 2/2 失败，明确此前无 TypeText 实现。
- `text-first.log`：新子模块缺少显式 path 的编译错误已保留；修正为与现有模块一致的路径后 2/2 通过。
- `text-boundaries-first.log`：14 passed / 2 failed；分配回调或选择区写入中变成安全字段后，仍先查询了文本属性。
  修复为每阶段先验证保护状态；测试不通过放宽安全读取计数来掩盖问题。
- `text-ordering-red.log`：17 passed / 1 failed；最终焦点读取期间用户改变选择区仍会替换原文。修复为最后重新检查零长度末尾选择区。
- `text-protection-red.log`：2/2 失败；可写性/字符数查询回调中变为安全字段后，后续元数据查询未阻断。
  修复为可写属性之间、字符数/选择区读取前都重查文本角色和安全子角色。
- 最终新文件 20 项测试，包含内部多场景循环：Unicode/原文保留/连续追加/空控件、错误参数和 native 数据、未聚焦/只读/隐藏、安全字段、旧/预览/跨 run/坐标/桌面请求、分配失败、端点变化、取消、Stop 重入、未知结果禁止重放及 CF 对象释放。
- FFI 控件模型独立实现“替换当前选择区”，而不是见到 TypeText 就模拟追加，因此非空选择区/错位的生产错误能真实导致红测。
  模型的 UTF-16 字符域只是确定性测试，不冒充真实 Cocoa 控件索引行为或 SDK/ABI 验收。
- Apple 官方 selected-text/selected-range setter、原生字符计数、AX CFRange 枚举，以及 Apple CFNumber.h 的 CFIndex=14/Boolean 转换声明已保存。
  `api-sources.json` 记录原始 URL；失败的 AXValueCreate DocC URL 不计作 API 或平台验收证据。

## 3. 冻结验证

- Windows 完整 Core 547 + 六组 Mac FFI（AX 27、capture 33、keyboard 17、pointer 18、text 20、value 14）：**676/676**，零失败/忽略，exit 0；Core 用时 101.27 秒。
  使用独立 `windows-seed-current`，不改用户安装 seed；`core-final.log` / `core-final.exit` 保留实际结果。
- Linux 六组 Mac FFI：129/129；Linux Core all-targets Clippy `-D warnings` 通过。
- Windows Core all-targets Clippy `-D warnings`、7 个直接修改 Rust 文件格式检查通过。
- 225 项选定源文件/测试/依赖/CI 指纹冻结；完整回归及严格检查结束后再核验 before/after **零漂移**。
  全部本轮 test/clippy 会话已正常退出，无未确认后台任务；最终 source-receipt 另纳入文档与日志。
- 本轮未改 browser production、未重跑 browser/navigation/真实 Windows 或 X11 场景，不搬用旧 native 结果证明本次 Mac 修改。

## 4. 未完成，不能称最终版

- AX 的读取、选区设置和文本设置不是原子 compare-and-append；最终外部输入竞态没有可证明的原子隔离。
  当前实现收窄窗口并拒绝已观察到的变化，但不能保证任意并发 native 编辑都不会穿过最后一次检查。
- Mac arm64/x64 的真实 SDK 链接、CF/AX ABI、NSTextField/NSTextView/ComboBox 索引及编辑事件、权限切换、真实异常完成/恢复仍 **not_run**。
  Windows/Linux FFI double 绿色不能冒充这些证据。下一项原生工作应使用 owned Cocoa fixture 验证真实 setter 效果和上述竞态边界。
- Mac 坐标文本/命名键、显式剪贴板路径、真实 IME 及完整持键/输入恢复仍未完成；不把此次语义追加当作完整输入功能。
- 原浏览器导航 504、worker/native 退出长尾继续开放；本轮无关闭这些问题的新证据。
- 全目标仍包括 Windows、macOS arm64/x64、Linux X11/GNOME native Wayland；Desktop/managed browser/existing tabs/App WebView；App/ACP/MCP；完整输入、生命周期、取消与恢复；签名安装/更新/回滚；重新设计的原生 UI 窄窗/缩放/权限矩阵；真实 Grok E4 与冻结源码 12h active soak。
  每项必须按其范围取得当前证据；不以本轮 20 项定向测试替代最终验收。

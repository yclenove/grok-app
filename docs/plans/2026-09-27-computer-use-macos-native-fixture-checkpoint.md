# macOS owned Cocoa 原生输入验收夹具检查点

本机终端时间：2026-09-27 05:30，Asia/Shanghai（+08:00；UTC 2026-09-26）。
HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`；未 commit/push/PR。
完整目标“接手 grok 的工作，完成 computer use 所有功能开发，推进到最终版”继续 **active**。
接续 [截图坐标文本/命名键](2026-09-27-computer-use-macos-coordinate-input-checkpoint.md)。
本轮首先实查工作树，并核对上轮 source-receipt 248 项零差异，上一轮归类为 progress；没有凭旧输出推断存在后台任务。

## 1. 实际新增的可执行验收链

- `tools/computer-use-fixtures/macos/InputFixture.swift`：独立 owned AppKit 窗口，真实 `NSTextView`、按钮和带固定测试值的安全字段。
  文本、UTF-16 选区、原生 keyDown/keyUp 及按钮计数由应用自身读取，不能通过管道设置预期值或伪造输入效果。
  管道只接受 `state / focus / move / quit`；焦点/移动仅作用于刚启动的夹具窗口，不操纵用户 App。
- `cu-macos-native`：Core 的 `test-support` 专用二进制，直接包含生产 `macos_adapter.rs`。
  macOS 构建不链接 Quartz/AX doubles；非 macOS 构建没有该原生后端，实际执行必须返回 `not_run` / exit 2 / 零个通过项。
  此探针不加入 Grok App bundle，不替换用户安装或 runtime。
- `macos_native_gates.rs` 的 18 个有序门禁：owned PID/窗口号/nonce 标题/窗口实例绑定；实际 PNG 解码及安全 AX 排除；语义和坐标 Unicode 追加；坐标 Left 与语义 Right 的真实按下/释放和选区后置条件；预览/旧帧/无图/未知引用/取消/移动后旧几何拒绝；新观察恢复；语义和坐标按钮各一次；Stop 退休；新 generation 观察在窗口关闭后失效；owned 子进程/输入占用清理。
  成功标志只在整个矩阵、独立应用后置条件与退出检查都通过后生成。拒绝测试逐次采样 200ms，不能冒充原子隔离证明。
- 每条 JSONL 回执严格校验版本、nonce、请求序号、真实 child PID、原窗口号和标题；限制行长度、文本、事件数组与 UTF-16 选区，拒绝回执错位和溢出。
  同一个 child handle 管理生命周期，EOF/90 秒夹具 watchdog 退出，错误路径只清理自己启动的进程；不按名称终止进程，不授予 TCC 权限。
- `run-owned.sh --owned-cocoa`：显式 opt-in，构建 Swift/Rust 夹具，保留 OS/架构/工具链、工作树、二进制哈希、源码前后哈希、PNG、观察、逐门禁状态与最终结果。
  拒绝非 Mac 和已知 Rosetta 运行；探针与 Cocoa 夹具还分别在自身进程检查 `sysctl.proc_translated`，每条回执必须与探针架构一致且非翻译运行；源码漂移时整个脚本失败，不能拿部分绿色结果冒充冻结验收。
  `README.md` 区分编译、原生架构运行、截图人工核验与完整产品验收。
- CI 新增验收器合同入口，以及 Apple SDK 下 arm64/x86_64 Swift typecheck 和 Mac 原生 Rust 探针编译步骤。
  **本轮只是写入这些 CI 步骤，没有执行 Mac CI，不能记录为 Mac 编译通过。** 不在 CI 自动弹窗/申请/授予权限。

## 2. 对验收器自身的验证

新增 `macos_native_fixture_contract.rs` **14** 项、多变体合同；`pipe-contract-child.mjs` 仅作为管道错误/退出替身，不是 Cocoa，也绝不能计入原生结果。

- 错版本/nonce/PID/窗口/序号/标题、未知字段、非 JSON、截断和超长回复均拒绝。
- Unicode 选区按 UTF-16 校验，溢出/越界或事件证据截断不能通过。
- 缺 key-up、重复或错误键对、错误历史前缀、拒绝后文本/选区/事件/点击发生变化均不能通过。
- 同标题不同 PID、同 PID 不同窗口、重复匹配、错误窗口实例/坐标域不能自动选择其他目标。
- 缺少原生平台/adapter、门禁缺项或顺序错、清理失败不能生成 PASS；证据文件禁止覆盖。
- 实际启动 owned Node 管道子进程验证握手、往返、EOF、坏 JSON、超长/部分回复、错身份/序号、超时与非零退出，不只测试 parser。

首轮 13/13 即通过。复核发现仅检查 shell 无法证明探针/夹具自身未翻译执行；补上两者进程内检查、严格架构回读及第 14 项合同，拒绝错误状态/长度/未知结果。修正前 710/163 与对应源码冻结保留在 `pre-architecture/`，修正后重新冻结并完整复跑，未复用修正前结果。首次 strict Clippy 报 `unnecessary_unwrap`，已改为 `if let Err(error)`，没有降低 lint 或修改旧测试。
原始日志 `clippy-first.log` 保留。终端跨 shell 引号的一个调用在解析阶段失败，改为独立 `.sh` 后执行；没有重启运行中的回归任务。

## 3. 当前实际结果与证据边界

证据目录：`tools/computer-use-probe/.run/macos-native-fixture-20260927/`。

- Windows Core **547/547**（99.29 秒）、Mac FFI doubles **150/150**、验收器合同 **14/14**，总计 **711/711**，零失败/忽略，exit 0。
  `core-final.log` / `.exit` 使用独立 `windows-seed-current`，不修改用户 runtime。
- Debian/WSL：Mac FFI + 验收器合同 **164/164**，exit 0；不是 Mac 原生测试。
- Windows/Linux Core `--all-targets --features test-support` Clippy `-D warnings` 通过；两宿主探针实际构建成功；五个新增 Rust 文件格式检查通过。
- Windows、Linux **实际运行编译后的 cu-macos-native**：均返回 `not_run` / exit **2** / 空 passed；测试传入不存在的 fixture，不启动/伪装 Cocoa。
  `windows-not-run-final/`、`linux-not-run-final/` 及对应日志/退出码保留。
- shell 语法和 Node 语法检查通过。原生 Swift / AppKit SDK 编译、Mac arm64/x64 GUI 执行、真实 PNG/控件视觉核验本轮均 **not_run**。
- 236 项选定源码/测试/依赖/CI/夹具说明在完整回归前冻结，全部测试与 Clippy 结束后前后 **零漂移**。
  最终 source-receipt 另纳入本检查点、索引与实际日志。所有测试/编译句柄均已终止；Windows/Linux 无仍存活的本轮 Node 管道夹具子进程。
- 本轮未修改生产 Mac 输入实现、其他后端、UI、用户安装、权限或运行时；没有用前轮原生 Windows/X11 结果证明新 Cocoa 夹具通过。
- 查阅并保存 Apple 原始 DocC：NSTextView、NSApplication.activate、NSWindow.makeFirstResponder、NSAccessibilityProtocol.setAccessibilityLabel，以及 Rosetta translation environment 的进程内 sysctl/ENOENT 说明（HTTP 200）。
  最初 NSView 下的 label URL 返回 404，明确保留 unavailable，未冒充有效来源；来源索引为 `apple-sources.json`。

## 4. 未完成项与下一步真实工作

此轮完成的是 **原生验收通道和验收器合同**，不是 Mac 原生验收，更不是“所有 computer use 功能已完成”。
下一步在两种原生 Mac 架构上运行同一生产调用链，检查 PNG/AX/真实 Cocoa 效果，按失败证据修正实现；不得只跑 capability/list-target selftest 或把管道替身换入原生结果。

原始范围完整保留：Windows、macOS arm64/x64、Linux X11/GNOME native Wayland；Desktop/managed browser/existing tabs/App WebView；App/ACP/MCP；完整输入、生命周期、取消和恢复；签名安装/更新/回滚；重设计原生 UI 的窄窗/缩放/权限矩阵；真实 Grok E4 与冻结源码 12h active soak。
其中 NSTextField/ComboBox/WebKit/自绘或 AX 缺失、SetValue/拖拽/滚动原生矩阵、剪贴板/IME、权限切换/并发队列/完整恢复仍需继续，不因这 18 项而缩减。
原浏览器导航 504 和 worker/native 退出长尾没有新关闭证据。目标保持 active，不申请 complete/blocked/paused。

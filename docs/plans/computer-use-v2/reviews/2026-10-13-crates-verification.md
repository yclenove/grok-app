# 第三方依赖核实（Grok grok-4.7 xhigh，只读，数据取自 2026-10-13 crates.io/docs.rs）

## 核实表

| 库 | 版本 | 许可 | 最近更新 | 结论 | 备注 |
|---|---|---|---|---|---|
| rmcp | 3.5.1 | Apache-2.0 | 2026-10-05 | 有风险 | 近 90 天下载约 1930 万。服务端 elicitation、stdio、streamable-http 都有，但是 feature，且 stdio 只按换行分帧。三平台纯 Rust |
| schemars | 1.2.3 | MIT | 2026-10-12 | 可用 | 近 90 天约 1.98 亿。rmcp 依赖 `schemars ^1.0`，`JsonSchema` derive 对齐。三平台 |
| insta | 1.49.0 | Apache-2.0 | 2026-10-03 | 可用 | 近 90 天约 3140 万。三平台 |
| proptest | 1.12.0 | MIT OR Apache-2.0 | 2026-10-10 | 可用 | 近 90 天约 5630 万。三平台 |
| criterion | 0.8.2 | Apache-2.0 OR MIT | 2026-02-04 | 可用 | 近 90 天约 6500 万。三平台 |
| cargo-deny | 0.20.2 | MIT OR Apache-2.0 | 2026-07-09 | 可用 | 近 90 天约 190 万。三平台 CLI |
| cargo-llvm-cov | 0.9.1 | Apache-2.0 OR MIT | 2026-09-06 | 可用 | 近 90 天约 179 万。三平台都要 `llvm-tools-preview`；aarch64-linux 可用 |
| cargo-fuzz | 0.13.2 | MIT OR Apache-2.0 | 2026-06-09 | 有风险 | 近 90 天约 168 万。官方说明只支持 x86-64/AArch64 的 Unix，不含 Windows；要 nightly |
| similar | 3.2.0 | Apache-2.0 | 2026-08-17 | 可用 | 近 90 天约 5790 万。序列/文本 diff。三平台 |
| smol_str | 0.3.6 | MIT OR Apache-2.0 | 2026-10-08 | 可用 | 近 90 天约 3750 万。三平台。§2 允许表未列入 |
| smallvec | 1.16.3 | MIT OR Apache-2.0 | 2026-10-10 | 可用 | 近 90 天约 3.18 亿。三平台。§2 允许表未列入 |
| unicode-segmentation | 1.13.3 | MIT OR Apache-2.0 | 2026-06-01 | 可用 | 近 90 天约 1.46 亿。UAX #29 字素簇。三平台 |
| interprocess | 2.4.5 | 0BSD OR Apache-2.0 | 2026-10-10 | 可用 | 近 90 天约 441 万。UDS 与 Windows 命名管道都有。OR 表达式可过 Apache-2.0 白名单 |
| axum | 0.8.9 | MIT | 2026-04-14 | 可用 | 近 90 天约 1.31 亿。rmcp 的 HTTP 传输是 tower service，官方示例用 axum 挂载。三平台 |
| tokio | 1.53.2 | MIT | 2026-10-03 | 可用 | 近 90 天约 2.56 亿。`sync`/`time`/`rt` 都在。三平台 |
| tracing | 0.1.44 | MIT | 2025-12-18 | 可用 | 近 90 天约 2.19 亿。三平台 |
| clap | 4.6.7 | MIT OR Apache-2.0 | 2026-09-14 | 可用 | 近 90 天约 2.54 亿。三平台 |
| toml_edit | 0.25.17 | MIT OR Apache-2.0 | 2026-10-09 | 有风险 | 近 90 天约 2.31 亿。只保证 TOML 保格式；不管 JSON |
| wry | 0.57.0 | Apache-2.0 OR MIT | 2026-09-08 | 有风险 | 近 90 天约 1490 万。macOS WKWebView、Windows WebView2、Linux 精确依赖 `webkit2gtk =2.0.2`（GTK3）。aarch64-linux 需要系统包 |
| tao | 0.37.1 | Apache-2.0 | 2026-09-26 | 可用 | 近 90 天约 1490 万。三平台窗口。Linux 默认带 x11 |
| objc2 | 0.6.5 | MIT | 2026-10-05 | 可用 | 近 90 天约 5100 万。仅 Apple。绑定 crate 要求 `>=0.6.2, <0.8` |
| objc2-app-kit | 0.3.2 | Zlib OR Apache-2.0 OR MIT | 2025-10-04 | 可用 | 近 90 天约 2960 万。默认含 `NSWorkspace`、`NSRunningApplication`、`NSPanel`。仅 macOS |
| objc2-foundation | 0.3.2 | MIT | 2025-10-04 | 可用 | 近 90 天约 4330 万。仅 Apple |
| objc2-screen-capture-kit | 0.3.2 | Zlib OR Apache-2.0 OR MIT | 2025-10-04 | 可用 | 近 90 天约 85 万。默认含 `SCScreenshotManager`。仅 macOS 14+ |
| objc2-core-graphics | 0.3.2 | Zlib OR Apache-2.0 OR MIT | 2025-10-04 | 可用 | 已有 `CGEvent::post`、`post_to_pid`、`keyboard_set_unicode_string`。仅 macOS |
| objc2-core-services | 0.3.2 | Zlib OR Apache-2.0 OR MIT | 2025-10-04 | 可用 | 默认含 `MDQuery`。计划未点名。仅 macOS |
| accessibility-sys | 0.2.0 | MIT / Apache-2.0 | 2025-03-22 | 可用 | 近 90 天约 17 万。含 `AXUIElementCopyMultipleAttributeValues`。只依赖 `core-foundation-sys`。仅 macOS |
| accessibility | 0.2.0 | MIT / Apache-2.0 | 2025-03-22 | 有风险 | 近 90 天约 6.3 万。同仓库高层封装，依赖 `cocoa 0.26` + `objc 0.2`，与 objc2 栈冲突 |
| core-graphics | 0.25.0 | MIT OR Apache-2.0 | 2025-05-27 | 有风险 | 近 90 天约 2670 万。0.25.0 源码有 `post_to_pid`。servo/core-foundation 栈，与 objc2 重复。仅 macOS |
| security-framework | 3.7.0 | MIT OR Apache-2.0 | 2026-02-20 | 有风险 | 近 90 天约 9580 万。有 `SecStaticCode::from_path`。`SecCodeCopySigningInformation` 在 crate 与 sys 的 main 中都不存在。仅 macOS |
| windows | 0.62.2 | MIT OR Apache-2.0 | 2025-10-06 | 有风险 | 近 90 天约 9140 万。feature 名已核实。Win10+ / ARM64 Windows 可调用；aarch64-linux 只能编译 |
| atspi | 0.30.0 | Apache-2.0 OR MIT | 2026-05-06 | 有风险 | 近 90 天约 496 万。纯 Rust + zbus，不链 libatspi。默认 `async-io`。Linux 运行时有用，三平台可编译 |
| zbus | 5.19.0 | MIT | 2026-08-09 | 可用 | 近 90 天约 3090 万。atspi 要 `^5.5`，ashpd 要 `^5.13`，主版本一致 |
| x11rb | 0.14.0 | MIT OR Apache-2.0 | 2026-07-16 | 可用 | 近 90 天约 2350 万。纯 Rust 协议。`composite`/`xtest`/`xinput` 非默认。aarch64-linux 可用 |
| ashpd | 0.13.13 | MIT | 2026-07-17 | 有风险 | 近 90 天约 634 万。`screencast` 有 `set_restore_token` / `set_persist_mode`；`remote_desktop`、`pipewire` 是独立 feature。默认 tokio |
| pipewire | 0.10.1 | MIT | 2026-08-19 | 有风险 | 近 90 天约 154 万。FFI，要系统 libpipewire。aarch64-linux 可以；macOS/Windows 不行。与 ashpd 的 0.9 不兼容 |
| reis | 0.7.1 | MIT | 2026-07-30 | 可用 | 近 90 天约 24 万。纯 Rust 的 libei/libeis 协议，不是 C 绑定。`libei-sys` 不存在。有 `tokio` feature。协议只在 Linux portal 的 fd 上有意义 |
| rquickjs | 0.14.0 | MIT | 2026-09-18 | 有风险 | 近 90 天约 248 万。静态编进 QuickJS，三平台可编。内存/时间限制要自己接，且受 feature 影响 |
| cargo-deb | 3.8.0 | MIT | 2026-09-01 | 可用 | 近 90 天约 24 万。在 Linux CI 上打对应架构的包 |
| cargo-generate-rpm | 0.21.0 | MIT | 2026-05-04 | 可用 | 近 90 天约 7.7 万。同上 |
| cargo-cyclonedx | 0.5.9 | Apache-2.0 | 2026-03-19 | 可用 | 近 90 天约 83 万。子命令就是 `cargo cyclonedx` |
| cargo-auditable | 0.7.7 | MIT OR Apache-2.0 | 2026-10-02 | 可用 | 近 90 天约 38 万。子命令 `cargo auditable`。三平台 |
| egui | 0.36.2 | MIT OR Apache-2.0 | 2026-09-08 | 有风险 | 近 90 天约 630 万。依赖 `accesskit ^0.24.1`（非可选）。三平台可编 |
| accesskit | 0.25.1 | MIT OR Apache-2.0 | 2026-09-25 | 有风险 | 近 90 天约 749 万。0.25 与 egui 0.36 的 `^0.24` 不在同一区间 |
| eframe | 0.36.2 | MIT OR Apache-2.0 | 2026-09-08 | 有风险 | 默认 feature 含 `accesskit`。拉 `accesskit_winit ^0.32`（最新 0.32.2 用 accesskit 0.24，并默认启用 Linux 的 `accesskit_unix`） |
| xkbcommon | 0.9.0 | MIT | 2025-08-09 | 有风险 | P3-03 点了名。FFI，要系统 libxkbcommon。aarch64-linux 可以；macOS/Windows 不用 |
| gtk4 | 0.11.5 | MIT | 2026-09-20 | 可用 | 近 90 天约 160 万。crate 许可是 MIT，系统 GTK 是 LGPL、动态链接。仅 Linux 夹具 |

许可白名单（MIT / Apache-2.0 / BSD / ISC / Zlib，以及 OR 表达式里至少有一个被允许）可以盖住上表全部 crate 许可。没有整库不可用或被撤回的项。

## 修改建议

[严重度 高] 03-architecture.md §5.2、06-roadmap.md P2-03 — `windows` 0.62.2 的 `GraphicsCaptureItem` 上没有 `CreateForWindow`。该类型现有的是 `TryCreateFromWindowId`（Windows 10 版本 2104 / 10.0.20348 起，且打包应用还要 `graphicsCaptureProgrammatic`）。桌面未打包进程从 Win10 1903 起应走互操作接口。`IsBorderRequired` 与 `IsCursorCaptureEnabled` 在 `GraphicsCaptureSession` 上，这一点成立。— 把 P2-03 和 §5.2 的截图行改成：`windows = "0.62"`，features 至少 `Win32_System_WinRT_Graphics_Capture`、`Graphics_Capture`、`Win32_Foundation`。用 `IGraphicsCaptureItemInterop::CreateForWindow(hwnd)` 得到 `GraphicsCaptureItem`；Win11 再对 `GraphicsCaptureSession` 设 `IsBorderRequired = false`、`IsCursorCaptureEnabled = false`，Win10 探测失败则忽略。不要改用 `TryCreateFromWindowId`。UIA 另加 feature `Win32_UI_Accessibility`。

[严重度 中] 03-architecture.md §11.5、06-roadmap.md P6-05 — `rquickjs` 0.14.0 能做嵌入式引擎，但不是带墙钟和内存上限的现成沙箱。`Runtime::set_memory_limit` 在文档中写明：开启 `rust-alloc` 或 `allocator` 后是空操作。`Runtime::set_interrupt_handler` 只在引擎执行 JS 时周期性调用，不管 Rust 宿主回调。默认 feature `std` 只是 Rust std，不是 QuickJS 的 `os` 模块。— 依赖写成 `rquickjs = { version = "0.14", default-features = false, features = ["std"] }`，明确不要开 `rust-alloc`、`allocator`、`loader`、`dyn-load`、`full`。内存用 `runtime.set_memory_limit(32 * 1024 * 1024)`。30 秒墙钟在 interrupt handler 里看 `Instant`，返回 `true` 中断；驱动调用次数和取消在 `cu.*` 宿主绑定里计数。宿主回调里的 Rust 分配不计入 32MB。P6-05 的内存炸弹用例要在未开 `rust-alloc` 的构建上跑。

[严重度 中] 03-architecture.md §2、§11，04-route-a-standalone-mcp.md §4，06-roadmap.md P0-09 — `rmcp` 3.5.1 的能力与计划写法有三处对不上。`elicitation` 不是默认 feature；`server` 默认不含 `transport-io` 和 `transport-streamable-http-server`。`async_rw.rs` 只用 `read_until(b'\n')` 和写出时追加 `\n`，没有 Content-Length。`elicitation/create` 的参数是 `CreateElicitationRequestParams::FormElicitationParams { message, requested_schema, meta }`，且协议 2026-07-28 起必须在处理客户端请求期间发出（`OriginatingRequestId`）。计划里的 `{"decision": ["once","session","always","deny"]}` 不是 JSON Schema。— `gcu-mcp` 写成 `rmcp = { version = "3.5", default-features = false, features = ["server", "macros", "transport-io", "transport-streamable-http-server", "elicitation"] }`。stdio 只承诺 NDJSON；若 S-01 证明某个客户端要 Content-Length，在 `gcu-mcp` 自写 codec，不要写成 rmcp 自动识别。审批在 `tools/call` 处理函数内调用 `peer.create_elicitation`。schema 使用：

```json
{"type":"object","properties":{"decision":{"type":"string","enum":["once","session","always","deny"]}},"required":["decision"]}
```

[严重度 中] 03-architecture.md §5.3、06-roadmap.md P7-02 — 最新 `ashpd` 0.13.13 的可选依赖是 `pipewire ^0.9`，crates.io 上的 `pipewire` 最新是 0.10.1，两个主版本的流类型不能混用。`SelectSourcesOptions::set_restore_token` 和 `set_persist_mode` 在 0.13.13 里存在。— 不要单独 `cargo add pipewire`。依赖写成 `ashpd = { version = "0.13", default-features = false, features = ["tokio", "screencast", "remote_desktop", "pipewire"] }`，让它带上 pipewire 0.9。aarch64-linux 的 CI 安装对应的 `libpipewire-0.3-dev`。macOS/Windows 不编译这个依赖。

[严重度 中] 03-architecture.md §5.1 — `security-framework` 3.7.0 有 `SecStaticCode::from_path`（即 `SecStaticCodeCreateWithPath`）。`SecCodeCopySigningInformation` 以及 team id 键不在 `security-framework` 3.7.0 的包装里，也不在当前 main 的 `security-framework-sys/src/code_signing.rs` 里。授权主键需要的 team id 还没有现成 API。— §5.1 改为：`SecStaticCode::from_path` 用 `security-framework` 3.7；team id 与 bundle id 在 `gcu-driver-macos` 用自有 `extern "C"` 调用 `SecCodeCopySigningInformation`，读取 `kSecCodeInfoTeamIdentifier` 和 `kSecCodeInfoIdentifier`，并加一个对已签名与未签名 app 的单测。

[严重度 中] 03-architecture.md §5.1 — 批量 AX API 在 `accessibility-sys` 0.2.0 的 `AXUIElementCopyMultipleAttributeValues` 里，这个选择成立。同仓库的 `accessibility` 0.2.0 会拉入 `cocoa` 0.26 和 `objc` 0.2。— 写成只依赖 `accessibility-sys = "0.2"`，不要依赖 `accessibility`。属性名常量用该 sys crate 已导出的集合。

[严重度 中] 03-architecture.md §5.1 — 定向事件不需要再引入 servo 的 `core-graphics` 0.25。`objc2-screen-capture-kit` 0.3.2 已经带入 `objc2-core-graphics`，后者有 `post_to_pid` 和 `keyboard_set_unicode_string`。两套 CoreGraphics/CoreFoundation 绑定同时进 `gcu` 会遇到类型不能混传。`CGEventSetWindowLocation` 两边公开绑定都没有，继续 `dlsym` 是对的。— §5.1 的 crate 列改为 `objc2-core-graphics`（feature `CGEvent`），删掉 servo `core-graphics`。私有符号仍用 `dlsym`，解析失败则该能力记为 `missing`。

[严重度 中] 03-architecture.md §5.3、06-roadmap.md P3-01 — `atspi` 0.30.0 是上游纯 Rust 实现，默认 features 是 `connection`、`proxies`、`p2p`、`wrappers`，zbus 走 `async-io`。引擎是 tokio。— 依赖写成 `atspi = { version = "0.30", features = ["tokio"] }`。不要 fork zbus。树操作走 `atspi-proxies` 的子接口，超时放在 tokio 调用外层。

[严重度 中] 06-roadmap.md P2-02 — `windows` 0.62.2 里 `IUIAutomationCacheRequest` 存在（`AddProperty`、`AddPattern`、`SetTreeScope`），feature `Win32_UI_Accessibility`。结构变化、属性变化、焦点不是同一个 `IUIAutomationEventHandler`。三个类型分别是 `IUIAutomationStructureChangedEventHandler`、`IUIAutomationPropertyChangedEventHandler`、`IUIAutomationFocusChangedEventHandler`；`IUIAutomationEventHandler` 只对应 `AddAutomationEventHandler`。— P2-02 改成订阅这三个接口，pattern 是否可用用 `UIA_IsInvokePatternAvailablePropertyId` 这类缓存属性，而不是把 pattern 对象本身放进 CacheRequest。

[严重度 中] 04-route-a-standalone-mcp.md §2.1、06-roadmap.md P4-05 — `toml_edit` 0.25 只做 TOML 保注释编辑。Claude（`~/.claude.json`）、Gemini、opencode、Cursor/VS Code 的 `mcp.json` 是 JSON，`toml_edit` 不会读也不会保空白。— 卡片改成：TOML（Grok、Codex）用 `toml_edit` 0.25，只改自己的表项。JSON 用 `serde_json` 且开启 `preserve_order`，按键增删；测试断言键和值，不断言空白。若必须保留 JSONC 注释，在 P4-05 另选一个库并写进 `deny.toml`，不要把 `toml_edit` 写成 JSON 工具。

[严重度 中] 06-roadmap.md P0-09 — `cargo-fuzz` 0.13.2 的 README 写明 libFuzzer 只支持 x86-64 和 AArch64 的 Unix，不支持 Windows，且要 nightly。P0 CI 矩阵含 `windows-2022`。— P0-09 改为：fuzz 目标入库，只在 `ubuntu-22.04` 和 `macos-14` 的 nightly job 跑 1 分钟；Windows job 不跑 `cargo fuzz`。aarch64-linux 在支持范围内，有自托管 ARM 时再加。

[严重度 中] 03-architecture.md §2、06-roadmap.md P1-01 — 夹具应使用 `eframe` 0.36.2 的默认 `accesskit` feature，不要另加 `accesskit` 0.25.1。`egui` 0.36.2 固定 `accesskit ^0.24.1`；`egui-winit` 0.36.2 要 `accesskit_winit ^0.32`（0.32.2 对应 accesskit 0.24，默认含 `accesskit_unix`）。egui 文档列出的无障碍控件是按钮、复选框、单选、滑块、下拉、文本框、链接、窗口。2000 行虚拟列表、密码框的 password 状态、拖拽和模态是否都出现在 AX/UIA/AT-SPI 里，文档没有保证。— P1-01 写成依赖 `eframe = "0.36"`（不要关 `accesskit`，不要加 accesskit 0.25）。DoD 增加三条：密码框在三平台的无障碍里是密码/受保护文本；虚拟列表只要求可见行加滚动范围，不要求 2000 行全部进树；拖拽源和放置目标有可操作角色。不满足就改用 `fixtures-native`，不要让契约测试去适配一个不完整的树。

[严重度 低] 03-architecture.md §5.1 — Spotlight 的 `kMDItemLastUsedDate` / `kMDItemUseCount` 不在 AppKit。`objc2-core-services` 0.3.2 默认 feature 含 `MDQuery`。— 发现行的 crate 改为 `objc2-app-kit`（`NSWorkspace.runningApplications`）和 `objc2-core-services`（`MDQuery`）。

[严重度 低] 03-architecture.md §2 与 §3 — `RawNode` 使用了 `SmolStr` 和 `SmallVec`，允许依赖表没有这两项。二者分别是 0.3.6 和 1.16.3，许可都是 MIT OR Apache-2.0。— `gcu-core` 允许列加上 `smol_str`、`smallvec`。`similar` 只用于渲染行的序列 diff；树对齐仍按 §4.5 的 fingerprint，不要用 `similar` 做树 diff。

[严重度 低] 03-architecture.md §5.1 — `SCScreenshotManager` 在 `objc2-screen-capture-kit` 0.3.2 里的方法是 `captureImageWithFilter_configuration_completionHandler`，参数是 `SCContentFilter`、`SCStreamConfiguration` 和 completion。计划里的 `captureImage(contentFilter:)` 少了后两个参数，也不是同步返回。遮挡窗口能否采到仍由 S-02 实测。— 写成调用该方法（feature `SCScreenshotManager` 已在 crate 默认里），完成回调里取 `CGImage`；失败映射为驱动错误。不要按同步函数写。

[严重度 低] 04-route-a-standalone-mcp.md §4、06-roadmap.md P4-02 — `wry` 0.57.0 在 Linux 上精确依赖 `webkit2gtk =2.0.2` 和 `gtk` 0.18，也就是 GTK3 的 WebKitGTK 4.1，不是 GTK4。crate 许可是 MIT，系统库是 LGPL，动态链接，不进 cargo-deny 图。— 文档写明运行依赖包名 `webkit2gtk-4.1`；缺库时走已写的 zenity/kdialog。`deny.toml` 加一句：系统动态库 WebKitGTK、GTK、libpipewire 不由 cargo-deny 判定，发行说明单独列出。`gtk4` 0.11 只给 `fixtures-native`。

[严重度 低] 06-roadmap.md P3-03 — `xkbcommon` 0.9.0（MIT，2025-08-09）是链系统 `libxkbcommon` 的 FFI，不是纯 Rust。— P3-03 写成 `xkbcommon = "0.9"`，只在 Linux 驱动启用；CI 安装 `libxkbcommon-dev`。不要放进 macOS/Windows 依赖。

[严重度 低] 04-route-a-standalone-mcp.md §5 — Wayland 胶囊写了 layer-shell，没有点名 Rust crate。实现库、许可和 GNOME 不支持都还没核实（待确认）。— P4-04 在选定 crate 后补进 §2 依赖表和 `deny.toml`；选定前保持「GNOME 不显示、能力表声明」这句，不要假设已有依赖。

总体结论：计划点名的库都还在，许可也能进白名单，没有被撤回的；要改的是 Windows 截图入口、rmcp 的 feature 和帧格式、pipewire 0.9/0.10 分裂，以及 rquickjs 的限制必须自己接，而不是换掉整份依赖清单。

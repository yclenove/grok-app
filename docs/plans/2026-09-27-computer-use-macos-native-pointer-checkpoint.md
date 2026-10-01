# Computer Use — macOS owned pointer fixture checkpoint

日期：2026-09-27（Asia/Shanghai）；完整目标 **active**。

本批接续 macOS owned Cocoa 夹具与指针归属实现。它补充的是可执行的原生验收路径、独立后置条件和验收器正确性，**不是 Mac 真机通过、产品全功能完成或最终版发布**。当前分支 `feat/computer-use-implementation`，基线 HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`；保留已有 dirty/untracked 工作，不提交、推送、替换用户 App/运行时或修改权限。

## 实际改动

- `tools/computer-use-fixtures/macos/InputFixture.swift` 增加真实 `NSView`：滚轮移动绘制标尺的视口，左键拖拽移动蓝色方块。事件、按钮、按下/释放、位置、滚轮量和修饰键来自 Cocoa 回调，而不是探针填写期望值。
- 原生有序门禁从 **18 项扩展为 26 项**：增加左双击、右单击、中单击、正/负滚动、零滚动及往返拖拽。坐标取自当轮截图/AX 中 nonce 命名的精确 owned 控件，不使用桌面兜底或猜测坐标。
- `macos_native_pointer.rs` 校验追加式事件历史、原生按钮/点击计数、中性修饰键、成对释放、滚动实际效果、拖拽方块位置/尺寸、路径与最终移动。允许系统合并中间拖拽事件，但不能省略最终实际拖动与释放。
- AppKit 的 `clickCount` 仅用于 down/up；motion 记录 0，慢拖拽的 up 可为 0。点击门禁仍要求准确计数，不用放宽点击断言掩盖问题。Apple 主文档 JSON 保存在证据目录，未以跨宿主 Rust 测试代替 Swift SDK 检查。
- 私有 JSONL 协议升为 **v2**，要求有界 pointer 状态。旧版本、未知字段/事件、非有限坐标、溢出、历史重写均失败；只能 `state / focus / move / quit`，没有注入输入、写入预期事件、设置标尺/方块状态的命令。
- 每次滚动/拖拽后保留新窗口 PNG/观察与独立应用状态；拒绝路径同样要求 pointer 状态未变。200ms quiet 检查是有界采样，不冒充原子隔离证明。
- 修复新增错误路径：保存 `failure-state.json` 失败也必须继续显式 owned shutdown；原门禁错误、证据写入错误和清理错误独立保留。新增测试覆盖四种证据/清理成功失败组合。
- `.github/workflows/ci.yml` 的非 Mac FFI 合同列表补入 `macos_pointer_ownership_contract`。本批没有运行远程 CI，不声称 Swift 双架构编译已通过。

## 先失败的证据与修正范围

1. 新 pointer 验收器第一次 **19/21、exit101**：零滚动提前返回而没验证坐标；方块尺寸变化错误复用了位置容差。两者已修正，原日志保留。后来增加慢拖拽计数与清理错误测试；最终夹具合同是 **23/23**。
2. 首次 Windows Core **558/561、exit101**：两项超时测试在 worker 尚未调度时就断言已执行一次。产品仍返回 Unknown 并保持占用；修复测试为条件变量等待适配器真实 dispatch 边界，仍断言只执行指定 action 一次。有限 300ms sleep 改为明确 hold 至 stop，防止调度延迟使目标路径变成成功。**测试配置的 30/40ms 超时值、产品 Broker 逻辑及未知结果禁止重放不变**，并补足测试 owned worker 的停止回读。
3. 同轮另有 `tiny_prepare_is_deterministic_check_is_readonly_and_chromium_tamper_fails` 的 `publish seed: move prepared staging to seed: os error 5`。独立复跑 **1/1**，最终全量也通过；**没有解释或关闭该 Windows 发布拒绝访问根因**，没有加盲重试、降级断言或改权限。
4. Windows r2 外层误用 Windows PowerShell，将 Cargo 进度 stderr 转为终止性 `NativeCommandError`，未采集到测试退出结果。检查 Cargo/rustc/Core 测试进程确实均不在后，才用当前 PowerShell 7.6.5 启动独立 r3；旧文件保留，不把启动器错误算成测试通过或产品失败。

## 当前已验证

证据根：`tools/computer-use-probe/.run/macos-native-pointer-20260927/`。

| 项目 | 当前证据 | 边界 |
| --- | --- | --- |
| Windows Core + 9 个 macOS 合同二进制 | `windows-final-r3.log/.exit`：**743/743、exit0** | 561 Core + 182 合同；没有 ignored/filtered |
| Linux Core + 同 9 个合同二进制 | `linux-final-r2.log/.exit`：**742/742、exit0** | 560 Core + 182 合同；没有 ignored/filtered |
| 夹具协议/验收器合同 | 两宿主各 **23/23** | 含恶意管道 double；不是 Cocoa 真机输入 |
| Core strict Clippy | Windows r3 / Linux r2：**exit0** | all-targets、test-support、`-D warnings` |
| Rust 格式 | `format-final-r3.exit`：**0** | workspace `cargo fmt --all -- --check` |
| 原生验收探针构建 | 两宿主 **exit0** | 非 macOS 构建，不是 Swift/AppKit SDK 编译 |
| 实际运行非 Mac 探针 | Windows r3 / Linux r2：**not_run、exit2、0 passed / 26 required** | 未启动假 Cocoa，不把 not_run 算通过 |
| Shell / Node 语法 | r2 两项 **exit0** | runner 与管道 adversary |
| 冻结范围 | `source-before-r2.json` / `source-after-r2.json`：**244 文件零漂移** | 选定源码/依赖/CI/夹具，不是全部工作树 |

`final-audit.json` 从日志、退出码、门禁数组、源码字节和探针散列重新核对，而非仅记录意图。相对第一轮 244 文件快照，仅改了四个预期文件：`fake.rs`、`broker/tests.rs`、`macos_native_gates.rs`、`macos_native_fixture_contract.rs`。

Windows 当前非 Mac 探针 SHA-256：`7afc2bebd897e361d5c0e2c997a4d10b9ce3e7a79817ecd3048b985bd8a23b42`。

Linux 当前非 Mac 探针 SHA-256：`94abf87590e749d29de5c57c8731cd64578013fd7f7a0d9925fbe2d2139010c8`。

## 未完成项与继续方向

- **Mac arm64/x64 的 Swift SDK 编译、真实授权交互桌面上的 26 项执行及 PNG 人工核对：NOT_RUN。** `run-owned.sh --owned-cocoa` 是下一次实际原生执行入口；新编译 AppKit 夹具不能用 Node 管道 double 替代，不能自动操作权限提示。
- Windows 偶发种子发布拒绝访问仍需捕获真实文件/进程占用与阶段证据；单次/最终通过不是根因修复。既有浏览器导航/PID/退出长尾问题同样不因本批变绿而关闭。
- 完整输入、剪贴板、中文 IME、选择替换、自绘/无 AX 控件、多应用/并发输入、取消与原生占用恢复仍需实现及验收，不能只停留在这一个 Cocoa 夹具。
- 保持完整范围：Windows、macOS arm64/x64、Linux X11 与 GNOME 原生 Wayland；Desktop、托管浏览器、既有标签页、App WebView；App/ACP/MCP 全链路；安装/升级/回滚及签名；重设计原生 UI 的窄窗/系统缩放/权限矩阵；真实 Grok E4；冻结源码 **12 小时 active soak**。
- R18 原生 UI 已有构建/观察证据，但未完成的权限遮挡后交互、窄窗和缩放矩阵不可用 mock 191/191 代替。本批未对用户 App 或系统权限作任何输入。

本批及上一批的真实修改属于 progress；中间仅工具转发失败的轮次不算开发进度或经核实等待。完整目标保持 **active**，未调用完成/暂停/阻塞状态更新。

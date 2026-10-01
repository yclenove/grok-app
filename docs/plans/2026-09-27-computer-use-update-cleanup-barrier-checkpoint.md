# Computer Use：更新后清理门禁与重试检查点

日期：2026-09-27。完整目标“接手 grok，完成 Computer Use 所有功能并推进最终版”仍为 **active**，不是最终版完成声明。

上一阶段：[X11 keeper 迟到原回执恢复](2026-09-27-computer-use-x11-keeper-late-recovery-checkpoint.md)。本阶段检查已有 App 更新消费者，修复清理错误被吞掉后仍然调用重启，以及重复清理命令用全局布尔标记提前返回成功的问题。**仅覆盖 install 返回后的路径；Windows 安装器内部退出仍是明确发布阻断项。**

## 已实现

- `useUpdater.ts`：安装成功后必须等待 `prepare_for_app_update` 成功才调用 `relaunch`。安装本身失败不进入清理。清理或重启失败保留本次已安装版本；显式重试只执行清理和重启，不重新查询、下载或安装。
- 新增 `preparing-restart` 状态，等待和重试过程中 About/Sidebar 都显示真实的准备状态并禁用重复执行。失败后保留可点击的“重试清理并重启”，About 展示版本和错误；检查更新和开发者模拟选项不能抹掉这次恢复状态。
- 安装占用中的 Update 资源不被 UI 卸载提前关闭；原操作完成后释放资源。安装或清理期间 UI 所有者卸载后，不再触发重启。此处只保证当前 hook 生命周期，**没有新增跨 Provider 重挂载/页面重载的持久化更新日志**。
- `updater/shutdown_gate.rs`：同一在途清理只有一个拥有任务；并发调用等待同一真实结果。调用者取消不会取消清理或让后来调用提前成功；任务 panic 返回失败。终态后的显式重试创建新一轮清理，不永久缓存一次成功，因为 App 可能仍在运行并产生新工作。
- `shutdown_product_checked` 汇总 run 清理和 managed browser 清理失败。App 的 catalog barrier 同时保留本地清理失败与 blocking task panic，而不是仅依据远端 catalog 结果给出成功。Remote IM stop 的错误也传回更新调用方。
- 三条新文案在全部 15 个 locale 中完整提供；沿用已有 About、Sidebar、按钮、提示与错误样式，没有扩充 App/AppWorkbench 状态块。

## 当前证据

目录：`tools/computer-use-probe/.run/update-cleanup-barrier-20260927/`。

| 检查 | 实测结果 | 覆盖边界 |
|---|---:|---|
| 旧行为红测 | 2 失败、2 通过 | jsdom 中真实 hook 调用链；旧实现清理失败仍调用被 mock 的 relaunch |
| 前端最终回归 | **101/101**，冻结后再通过一次 | hook 10、状态帮助函数 44、About/Sidebar DOM 5、完整 locale 测试 42 |
| 更新门禁原生测试 | **7/7** | 4 个并发/取消/重试/panic 测试与 3 个现有 updater 状态测试 |
| 本地清理汇总 | **3/3** | 实际 Tauri blocking runtime；本地失败、panic 和先前 catalog 错误都不能变成成功 |
| Windows 子进程所有权 | **7/7** | owned Node 子进程与真实 Kernel Job；失败保留原 worker、并发等待、仅回收所属进程 |
| Session/ACP/MCP 回归 | **21/21** | 私有 TCP/adapter 夹具；延迟回应、失败重试、换代与跨会话隔离，不是实际 Grok E4 |
| Windows App 构建 | 通过 | 完整 App lib test harness 编译；执行上述 **38 个**测试，未冒充运行全部 1733 个测试 |
| 严格静态检查 | 通过 | App `cargo clippy --lib --tests -- -D warnings`、完整 TS typecheck、7 个相关 TS/TSX 文件 ESLint、workspace fmt、git diff 检查 |
| 选定源码冻结 | **38/38 零漂移** | 26 个本阶段编辑文件 + 12 个依赖见证，不是全产品最终候选冻结 |

原生 panic 测试有刻意注入的 panic 输出，终态均通过；没有将该日志描述为“没有 panic”。最早一次 `red-hook.log` 是缺 jsdom 导致的 `document is not defined`，**不作为缺陷复现证据**；真正行为红测是 `red-hook-dom.log`。前端安装、Host prepare 和 relaunch 是受控 mocks；未执行用户 App 的实际安装、退出或重启。

本阶段没有修改 Cargo/锁文件/CI。与上一阶段 315 个选定源摘要比较，只有 `computer_use/mod.rs`、`computer_use/shutdown.rs` 两项变化；本阶段新增/修改的 updater 和前端文件在独立 38 项清单中。上一阶段 Linux 629/629 与 X11 66+41+19 三轮成绩**没有在本阶段重跑**，不能累加冒充当前 App/安装验收。

主要复验命令（Windows App 测试 exe 运行前按项目 CI 方式嵌入 `windows-test-manifest.xml`）：

```text
npx vitest run src/hooks/useUpdater.test.ts src/lib/appUpdateHonesty.test.ts src/components/settings/AboutUpdateRow.test.tsx src/i18n/messages.test.ts
npm run typecheck
npx eslint src/hooks/useUpdater.ts src/hooks/useUpdater.test.ts src/lib/appUpdateHonesty.ts src/lib/appUpdateHonesty.test.ts src/components/settings/AboutUpdateRow.tsx src/components/settings/AboutUpdateRow.test.tsx src/components/SidebarUpdateButton.tsx --max-warnings 0
cargo clippy --manifest-path src-tauri/Cargo.toml -p grok-app --lib --tests --offline --locked --jobs 4 --target-dir src-tauri/target -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml -p grok-app --lib --no-run --offline --locked --jobs 4 --target-dir src-tauri/target --message-format=json
<recorded-test-exe> updater:: --test-threads=1 --nocapture
<recorded-test-exe> computer_use::shutdown::tests:: --test-threads=1 --nocapture
<recorded-test-exe> computer_use::browser_supervisor::process::tests:: --test-threads=1 --nocapture
<recorded-test-exe> session_manager::computer_use::tests:: --test-threads=1 --nocapture
node tools/computer-use-probe/.run/update-cleanup-barrier-20260927/audit.mjs
```

## 真实发布缺口：Windows install 内退出

已检查本机 Cargo.lock 对应的 `tauri-plugin-updater 2.10.1` 原始依赖，路径、SHA-256 和行号见 `windows-installer-gap.json`：

1. Windows `updater.rs:787` 的 `install_inner` 先 extract。
2. `updater.rs:837–839` 调用 `on_before_exit`；接口签名（289 行）为 `Fn()`，不是可以拒绝退出的 `Result`。
3. plugin `lib.rs:108–109` 将其设为 `app_handle.cleanup_before_exit()`。
4. `updater.rs:855` 调用 `ShellExecuteW`，865 行直接 `std::process::exit(0)`。因此不会返回前端执行本阶段的 post-install prepare。

**没有**修改 registry 源、fork updater、绕过签名校验、执行真实安装器，或把 Windows 改成手动更新来缩小目标。修正了有关“所有平台都在 install 后清理”的代码注释。Windows 需要在已验证/已准备的安装包与安装器启动之间接入**可失败、可等待、保留原包和所有权、可重试的原生门禁**；清理未完成不得启动安装器或进程退出。安装器启动失败也必须返回可恢复错误，不能靠无返回值退出 hook 或普通 ExitRequested 冒充覆盖。应以私有子进程夹具先验证，再进行签名安装/升级/回滚验收。

## 仍未完成

- 普通 cooperative Quit 目前仍忽略清理报告并最终退出；本阶段没有改变该策略，也没有把它描述为已安全。
- App 更新/退出尚未查询原生 clipboard retention/keeper handoff；X11 实际 clipboard TypeText、异步 paste 精确完成/恢复等先前缺口保留。
- 门禁任务完成不是 voice/IM/mirror 每个底层子进程均物理终止的证明；已有服务返回契约和失败恢复仍需完整审计。
- 已安装版本只在本次 hook 内保留，Provider 重挂载/页面重载后的恢复需要 Host 持久化状态与真实候选身份绑定。
- 新文案 DOM/交互已验证，但本阶段未做安装版原生 UI 截图、窄窗/缩放/权限全覆盖视觉验收。
- 本阶段未运行 macOS arm64/x64 或 Linux 完整 App/原生 GNOME Wayland、签名安装更新回滚、真实 Grok E4，或同一最终冻结候选的 12 小时主动 soak。

完整范围仍是 Windows、macOS arm64/x64、Linux X11 与 native GNOME Wayland；Desktop、managed browser、existing tabs、App WebView；App/ACP/MCP；全部输入/IME/剪贴板/取消/恢复；签名安装/更新/回滚；重设计原生 UI；真实 E4 和最终 12h。不能用本阶段门禁测试代替以上任一完整要求。

本阶段没有修改用户桌面/全局剪贴板/权限提示、替换用户 App/runtime、commit、push 或 PR。`final-audit.json` 与 artifact receipt 记录范围和摘要；完整目标继续 **active**。

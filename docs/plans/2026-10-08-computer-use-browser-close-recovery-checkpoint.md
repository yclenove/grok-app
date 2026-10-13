# 浏览器关闭失败与显式恢复检查点

2026-10-08，`feat/computer-use-implementation`，HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`。
状态仍是 active / partial — not releasable。本批未 commit、push、PR 或发布。
前一目标回合实际检查了用户窗口、刷新和菜单响应，得到启动证据；本批继续产品实现与验收。

## 产品行为

`embeddedBrowserLifecycle.ts` 持有每个 label 的前端租约、原 create Promise 与原 close Promise。
StrictMode/短暂重挂不会提前关闭页面；开始关闭后必须等原生回执。关闭失败保留原 owner，
不自动重试，也不让同 label 重建冒充关闭成功。用户显式重试共用原 Promise，真正成功才退休。
Host 的 `side-browser-close-pending:` 让同 label 在 renderer 重载后重新挂载时恢复失败提示；
这不证明已移除标签页在 renderer 重载后能够自动枚举所有保留 owner。

页面已移除时，工作台显示正式 GlassModal，包含页面名、简短错误、“稍后”与“重试关闭”。
“稍后”仅收起当前提示，保留原 owner 和正在进行的关闭；等待期间按钮去重。
新一次重试失败重新显示提示。同 label 重新挂载时，内嵌页显示恢复入口，成功后才重新创建。
技术详情默认折叠，沿用共享 modal、焦点管理和 native cover；六个新文案键已覆盖全部十五语言。
状态/新 UI 在领域模块中，未增加 App/AppWorkbench 状态。

Windows Host 关闭会先隐藏原 view，再等待真实下载联合终态。失败仍保留同一个 native owner，
移除标签页不会在等待期间继续覆盖其他页面。前端 boot/layout 在每个异步边界核对卸载或代际，
迟到的 position/size 回执不能再触发显示、导航、下载 hook 或初始化。
已派发的原生请求仍由原生层处理；这些前端检查不声称能够撤回已派发请求。

## 渲染验收与修复

入口：`node tools/computer-use-probe/ui-browser-close-acceptance.mjs`。
使用正式 Vite 构建、实际产品组件/store/样式、固定 Chromium 的独立可写副本；IPC 为 synthetic。

四次运行均保留 source hash、截图、原错误和 cleanup 记录，目录位于 `tools/computer-use-probe/.run/`：

| 运行 | 结果与对应修复 |
| --- | --- |
| `browser-close-ui-2026-10-07T17-59-58-915Z` | 61 项完成后，320×480 / 德语 / 200% 字号按钮文字被裁切；只对本提示框允许按钮换行、自适应高度 |
| `browser-close-ui-2026-10-07T18-01-54-637Z` | 61 项完成后，长标题挤掉正文，“详情”无法实际点击；标题栏用短的“浏览器”，完整错误放入可滚动正文 |
| `browser-close-ui-2026-10-07T18-05-10-805Z` | 62 项完成后，泰米尔语 / 200% 字号长正文产生横向溢出；正文允许长词换行 |
| `browser-close-ui-2026-10-07T18-07-28-446Z` | **82 项通过**：79 布局 + 3 键盘/原 Promise 生命周期场景；源码 hash 复验一致 |

覆盖十五语言、light/dark、320/900 宽，800 高和 480 高/200% 字号，长页面名/错误详情；
另有 DPR 1.25/1.5/2。验证普通与展开详情的布局、操作按钮可读、无横向溢出、Tab/Shift-Tab
保持在 modal、Enter 不重复发 close、原回执成功/失败、等待中 Escape 后仍保留原请求且恢复焦点。
已人工查看德语/泰米尔语大字号与 busy 截图。未增加 timeout、force click、sleep 或放宽断言。
Chromium DPR/字体 token 是辅助渲染证据，**不是 Windows OS DPI 或 native/ACP/MCP/真人批准验收**。

渲染通过后补了前端迟到布局竞争检查；它不改变上述 modal/store 样式与行为。
最后 75 项前端回归覆盖 lifecycle 10、React 恢复/迟到 boot 22、本地化 43，全通过，无 ignored。
`tsc -p tools/computer-use-probe/tsconfig.ui.json --noEmit` 与定向 ESLint exit0。
新 held-layout 测试最初有 Promise<void>/Promise<undefined> 类型错误，已显式声明 mock 的 void 回执；
最初那次 typecheck 不计成功，修复后的真实 typecheck exit0。

## 原生证据与回归

实际 App IPC/Wry 隐藏/保留 owner 门禁在
`src-tauri/target/codex-test-runtime/app-download-close-recovery-20261008/`。
`native-result.ok=true`；App SHA256：
`03D152455B658990246C9B1B40A71AC00A44FEA42CAF0853EA99B09B99DD6AC3`。
`owners.json` 记录原进程 joined、10 个观察到的 owners 均退出、remaining=[]。

两个原场景均 receivedBytes=16384，等待中 retained view 确实 hidden；UI/keeper 响应。
真实 nativeStarted/nativeTerminal/Wry requested/finished/processed 各 1，browserExited=0。
并发两 Close 在原真实回调释放后收尾 17ms；五秒失败实际 5010ms，显式重试释放后收尾 21ms。
same-label 新 instance 在同 browser 完成 37-byte 内容验证，零重放/迟到复活，staging 清空。
17/21ms 是回调释放后的收尾时间，不是完整下载或完整关闭耗时。
Host approval/publication pin synthetic，realAcp/realMcp/realModel=false，freshFrontend=false；
不能把该 debug App fixture 和独立 UI fixture 拼成 fresh 用户包或真实模型全链证据。

当前原生源又重新编译并运行了回归，不借用旧 harness：

    cargo test --locked -p grok-app --lib --features computer-use-probe,tauri/custom-protocol --no-run --message-format=json

唯一 Cargo 返回的 App library harness 复制到
`src-tauri/target/codex-test-runtime/app-download-close-recovery-regression-20261008/webview-tests.exe`，
用 Windows SDK mt.exe 嵌原 `windows-test-manifest.xml` 并读回后直接执行。
copy SHA256：`DB6C7893024169EEFFC75180A43C84A45A7D960DF64BF0FE47A332270DB52813`。
WebView 127 + Host 9 + side_browser_blob 4 = **140 passed，0 failed / ignored**。
同目录保留 Cargo JSON、manifest、三个原 test logs 和三份 Clippy logs。

三组 strict 实际 exit0：

    cargo clippy --locked -p grok-app --lib --tests --bin computer-use-app-probe --features computer-use-probe,tauri/custom-protocol -- -D warnings
    cargo clippy --locked -p grok-app --lib --tests -- -D warnings
    cargo clippy --locked -p grok-computer-use-probe --all-targets -- -D warnings

`cargo fmt --all -- --check`、质量 final 和 diff 检查通过。
App+AppWorkbench 13573，≥1000 行文件 80/80；质量 PASS 只表示代码质量，不表示 CU 全部交付。
本批触及的 WorkbenchMain 按 `.gitattributes` 机械规范化为 LF，UTF-8 无 BOM；
回读确认仅换行变化，实际 Git 内容 diff 仍为恢复提示的两行接入，未覆盖已有源修改。
当前关键源 SHA256：

| 文件 | SHA256 |
| --- | --- |
| `side_browser_host/close_windows.rs` | `33AF2E6EAB5E848F3C68F91C256B3A870CE3C926F4ACD560B21FBA55EC7D920E` |
| `embeddedBrowserLifecycle.ts` | `B4A92CEE5E4391F80D7A2CD27EDEC8CEB010FFEF0412830B60E3E0C6BDF56456` |
| `EmbeddedBrowser.tsx` | `8EB26A5246192099BE8C939AF8DDCC36280B8F1C92D33DB3A8AC8F3A7CE86C13` |
| `EmbeddedBrowserCloseNotice.tsx` | `E236E9483FA8BD72B19A97560F52BE3A79AC2C0F2EA7C0EFA2E406B3D47AC33A` |

## 用户窗口与清理

唯一用户预览仍是 PID24536，2026-10-07 20:32:07 创建的 `target/debug/grok-app.exe`。
本轮实际前置、刷新，等待加载结束后看见正常首页/侧栏/输入框，点开模型菜单并用 Escape 收起。
刷新会短暂黑屏；显式 bypass proxy 的 loopback curl HTTP200，PowerShell 默认请求超时不计成功。
该后端仍是 2026-10-07 20:23:41 的构建，没有把本批 native 修改更新成 fresh 用户 App。
Vite HMR 可能加载当前前端，不作为 packaged App 证明。

四次渲染的原 browser/server owners 退出后，各自只清 `site` 和 `browser-runtime`，
逐文件 hash/普通文件/实际路径检查后删除；事后四组目录均不存在，exact runtime 无 live owner。
逻辑量共 **1,490,693,053 bytes**；四个实际 H 空闲前后观测增量共 **1,491,226,624 bytes**。
空闲变化可能含并发文件活动，与逻辑量分记。各 `cleanup.json` 给出精确目标与复建命令。
报告、源 hash、首败截图/错误、日志和当前 App/native harness/seed/依赖/home 均保留。
上一批两代 incremental 删除曾被自动审批拒绝；本批未重新尝试或换工具绕过，继续暂缓。

## 下一项与完整交付范围

1. DownloadStarting 前取消、unknown/cancel grace、最终 staging publication 的真实竞争边界。
2. 原 App upload：原 input/ref/document/owner 身份与实际服务器内容 oracle，补取消/导航/替换。
3. fresh 用户 App/包：保留当前 home/login，实看启动，原 ACP/MCP/真实 Grok 与控制恢复验收。
4. Windows、macOS arm64/Intel、Linux X11/native Wayland、四 surfaces、安装升级回滚、模型矩阵、
   原生 UI/UX、同一冻结候选 12h active soak 和分批 commit/push 仍开放。

完整基线仍是 `2026-10-02-computer-use-remaining-plan.md` 的所有 U/D 任务。
本批关闭的是产品恢复 UX 的 scoped 工作包，不缩减目标、不把辅助截图当完整 native 交付。

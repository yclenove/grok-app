# Computer Use App WebView 导航检查点

2026-10-07，分支 `feat/computer-use-implementation`，基线 HEAD `8d8b71c2`。
本批未 commit、push、PR 或发布；整体仍是 active / partial — not releasable。

## 本批实现

App WebView 现在通过原 Broker、TaskGrant 和 `computer_navigate` 支持以下 typed 导航：

- URL 导航、reload、back 与 forward；
- URL 仅允许既有浏览器导航 allowlist，reload/history 不接受多余 URL；
- 派发前复验原 run、target、TaskGrant member、native document 与空闲状态；
- back/forward 先在原生线程查询历史，确认无历史时零派发、零 document generation 变化；
- 派发后等待原生 NavigationStarting/NavigationCompleted，而不是用 URL 轮询冒充完成；
- 每次成功导航产生新的 native document、执行 targetId 和 TaskGrant member；Broker 的
  `targetGeneration` 独立递增，不要求与 native document generation 数值相等；
- successor 只在原 native document authority 下原子提交；旧 snapshot/ref、旧 target 和旧
  member 同时退休，不会在 commit 前泄漏半成品身份；
- 成功结果按原 actionId 精确重放且不二次派发；已派发但完成不确定时保留 unknown，立即退休
  旧 native document，之后同 actionId 只读 ledger，不重新触发副作用；
- Windows Host WebView2 已接 typed URL/reload/back/forward、原生历史预检、原生导航计数和
  一次性 unknown 注入。Linux/macOS 仅完成对应平台代码，尚无实机通过证据。

## 权威验证

- `cargo check -p grok-computer-use-probe`：通过。
- `cargo clippy -p grok-computer-use-core --all-targets -- -D warnings`：通过。
- `cargo clippy -p grok-app --lib --no-default-features --tests -- -D warnings`：通过。
- `cargo clippy -p grok-computer-use-probe --all-targets -- -D warnings`：通过。
- `cargo test -p grok-app --lib --no-default-features --no-run`：通过，当前 harness 为
  `grok_app_lib-63e3432e557be5b6.exe`。
- 向该 harness 嵌入 `src-tauri/windows-test-manifest.xml` 后直接运行
  `computer_use::webview --test-threads=1`：**107 passed / 0 failed / 2259 filtered out**。
- `node --test tools/computer-use-mcp/protocol.golden.test.mjs`：**8 passed / 0 failed**。
- `cargo fmt --all -- --check` 与 `git diff --check`：通过。
- 重新从当前源码构建并嵌入同一 manifest 的 `cu_probe.exe webview`：完整通过，终态
  `computer_use webview: PASS`。导航门禁为：

```text
gate: webview_navigation url=1 replay=0 reload=1 back=1 forward=1
no_history=0 unknown=1 unknown_replay=0
native_starts=6 native_completions=6
```

本机直接启动未嵌 manifest 的 Windows harness/probe 仍可能在进入断言前报
`0xc0000139 STATUS_ENTRYPOINT_NOT_FOUND`；这属于缺 Common Controls v6 的启动环境错误，
不计为测试通过。产品 linker 未增加第二份 `MANIFESTINPUT`。

## 复审结论

- native document generation 与 Broker `targetGeneration` 在断言中分别递增，没有把两个
  独立时钟错误比较为相等。
- unknown 路径在 adapter 返回前取消原 binding；Broker 保存原请求 fingerprint 与 owner，
  重放不访问已经退休的 native document。
- 无 history、非法 URL、旧 generation、旧 owner、第二次导航、commit 失败以及导航与最终
  document guard 的竞争均有定向回归。

## 测试产物收尾

三次真实 probe 的 PID `61120`、`56728`、`28552` 及其 Cargo/rustc/WebView2 owners 均已退出。
本轮按完整绝对路径核对 33 个 `%TEMP%\\grok-cu-*` fixture 目录：全部位于当前用户 Temp、
不是 reparse point，且没有进程命令行持有；逻辑大小合计 **100,806,937 bytes**。

原生 PowerShell 精确 `Remove-Item -LiteralPath` 仍被当前执行策略拒绝，没有改用其它 shell
绕过，因此实际回收 **0 bytes**，这 33 个目录暂列 deferred。它们可由
`cargo build -p grok-computer-use-probe --bin cu_probe` 后运行 `cu_probe.exe webview` 重建。
当前用户预览、依赖、源码、当前 App/probe、失败证据和其它工作树均未触碰。

## 后续

下一独立批次继续 App WebView upload/download staging：下载只能进入 run-owned staging，上传
必须绑定原文件选择/任务身份，Pause/Stop/导航/Close/unknown 均不得复用旧 document 或路径。
随后仍需 fresh 用户包和真人原 ACP/MCP/Grok 整链、Windows 安装/升级/回滚、macOS arm64/Intel、
Linux X11/native Wayland、四 surface、模型矩阵、UI/UX 与同一冻结候选 12 小时 active soak。


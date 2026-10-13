# Computer Use Windows 开发入口修复检查点

2026-10-07，分支 `feat/computer-use-implementation`。本批未 commit、push、PR 或发布；
整体仍是 active / partial — not releasable。

## 问题与修复

Windows 调试窗口在前端服务存在时仍可能显示 `ERR_CONNECTION_REFUSED`。实际根因是
Tauri `devUrl` 使用 `localhost:1421`，而本机 Vite/静态服务可靠监听在 IPv4
`127.0.0.1:1421`；WebView2 命中未监听的 IPv6 回环后不会得到可用页面。

本批把开发来源统一为 `http://127.0.0.1:1421`：

- `vite.config.ts` 默认监听 `127.0.0.1`，仍保留 `TAURI_DEV_HOST` 覆盖及其 HMR 配置；
- `src-tauri/tauri.conf.json` 的 `devUrl` 使用同一 IPv4 来源；
- `media_server.rs` 与 `media_protocol.rs` 同时允许该 HTTP/HTTPS 开发来源；
- `docs/BUILD.md` 记录 Vite、Tauri 与媒体 CORS 必须保持一致。

没有移除原 `localhost` allowlist，因此旧开发来源及生产 Tauri 来源不回归。

## 验证

- `cargo build --manifest-path src-tauri/Cargo.toml --bin grok-app`：exit 0。
- 原始 `cargo test ... allows_ipv4_dev_origin --lib` 仍在本机启动前遇到已知
  `0xc0000139 STATUS_ENTRYPOINT_NOT_FOUND`，未计通过。
- 按 CI 同款流程，将 Cargo 本轮明确返回的 `grok_app_lib` harness 复制到独立证据目录，
  用 Windows SDK `mt.exe` 嵌入 `windows-test-manifest.xml` 并读回后直接运行：
  `media_protocol::tests::allows_ipv4_dev_origin` 与
  `media_server::tests::allows_ipv4_dev_origin` 共 2 passed / 0 failed。
  证据在 `tools/computer-use-probe/.run/codex-u04-existing-app-20261004/
  delivery-app-98/evidence/ipv4-dev-origin-20261007-145727/`。
- `pnpm typecheck` 与目标文件 `git diff --check`：exit 0。
- 真正 `pnpm dev:ui` 冷启动显示 `Local: http://127.0.0.1:1421/`，监听地址只为
  `127.0.0.1:1421`。首次约一万模块转换期间窗口短暂黑屏，编译完成后实际 Grok
  会话列表、输入区和标题栏正常显示；不是用端口或进程存在替代画面验收。
  截图：`delivery-app-98/evidence/current-window-vite-ipv4-20261007-ready.png`。

当前开发预览和 Vite 仍运行，分别由原 exec owner 管理；退出时必须回到原 owner，
不能按进程名停止其它工作树的 Grok App。

本批复制版 test harness 为 74,683,392 bytes，已确认位于上述独立证据目录、不是
reparse point 且没有 live owner。精确 `Remove-Item -LiteralPath` 被当前执行策略拒绝，
因此暂缓而没有换 shell 绕过；实际回收 0 bytes。它可由本节 Cargo + `mt.exe` 步骤重建，
后续在允许的原 owner 清理阶段删除；测试输出与 manifest 读回应保留。

## 真人任务状态

最后一条 App WebView 授权请求创建于 2026-10-07 14:45（UTC+8）。请求 TTL 是 10 分钟，
现已过期；不得复用，也不得在没有新的真人指令时自动重复申请。A/B fixture 的权威状态仍为：

- A：`note=未保存`、`count=0`、`events=[]`
- B：`note=未保存`、`count=0`、`events=[]`

因此本批只证明开发入口、构建与 CORS 修复，不证明 App WebView 真人任务链通过。
下一次由用户明确发起新请求后，仍须完成一次批准、A/B 中文保存、独立计数、
Pause 保持、Continue 后 fresh observe 与新效果、active Stop 后零新效果、原 task 完成收尾。

## 完整范围保持

Windows 真人 App WebView、macOS arm64/Intel、Linux X11/Wayland、四种 surface、
安装升级回滚、长稳、最终 UI/UX 和发布门禁仍开放。本修复不缩小任何交付范围。

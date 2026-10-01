# Computer Use：Windows 更新预备失败与恢复边界

日期：2026-09-30（本机 +08:00）；工作区 `H:\aicoding\grok-app-computer-use`。
分支 `feat/computer-use-implementation`；HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。

**完整 goal 仍为 active / partial — not releasable。** 上轮与本轮均为 progress；没有声明最终完成，也没有把目标改成只完成 Windows updater。

## 实际缺陷与修复

- 起初怀疑“不可信 inventory”会造成误阻塞。加强原回归后它在修改生产代码前已通过：该路径原本返回 `WindowsInstallPreflight`。此证据保留为 `preexisting-untrusted-inventory-pass.log`，不当作失败复现。
- 随后用测试签名器签署不兼容的 completion contract，通过真正的 `install_windows_checked` 复现缺陷：完成协议校验返回 `WindowsInstallPending / journal`，但此时没有 prepared installer、没有 journal candidate、更没有 cleanup 或 OS dispatch。catalog 却停在 `Blocked`，后续查询又报 original journal missing。`red-contract-test.log` 记录 **0 passed / 1 failed，Blocked != Failed**。
- `run_windows_attempt` 现在记录**本次持有执行权的初次 staging closure 确实失败**。仅当没有 prepared transaction，且权威 journal snapshot 成功返回 None 时，才把该 staging pending error 转成已有的 preflight failure，由 catalog 发布明确的 `Failed / staging` 结果。没有新增 IPC、权限、UI 状态或安装完成捷径。
- 回归覆盖四种已签名但不支持的 completion contract：protocol、install_scope、空 bundle_id、非法 bundle_id。失败不清理 App 服务；pending 返回同一明确失败；旧 nonce 不可恢复；修正签名 contract 后允许新 nonce；一旦 Prepared 已发布，cleanup 拒绝仍保留原 nonce，重试不重新准备安装器。
- 反例使用真实 Windows FILE_SHARE_READ handle 阻止**测试私有** active.dpapi 替换：不确定 publication 使 journal 进入 fault，即使释放阻挡 handle，当前 owner 仍 Blocked，不能再次派发。另一个反例证明“恢复时原 prepared installer 缺失”不会因 journal 为 None 被当作初次校验失败。
- 原来的 LaunchIntent / Accepted 保护分支保持不变；已有崩溃、原进程身份、显式拒绝和退出证据回归一起重跑。任何已派发或无法确定结果的状态，均没有被本次修复释放。

## 本轮验证

证据根：`tools/computer-use-probe/.run/windows-update-staging-recovery-20260930/`。

| 检查 | 本轮实测 | 边界 |
| --- | --- | --- |
| 默认 updater library | **89 passed / 0 failed / 8 ignored** | features = default,rustls-tls,zip；完整该 library suite |
| 独立 no-zip updater library | **88 passed / 0 failed / 8 ignored** | 独立 consumer，features 仅 rustls-tls，固定 lock |
| 原生 callback parser | **两套配置各 1 passed** | 显式运行 ignored 测试；复用上一封存阶段的实际 NSIS callback 字节，并重新核对 SHA；本轮没有重跑 NSIS 安装进程 |
| App 直接依赖 | **38 passed** | updater 7 / shutdown 3 / browser process 7 / MCP session 21，不是全部 App library |
| 前端恢复链 | **4 files / 99 passed** | useUpdater、updateRecovery、AboutUpdateRow、appUpdateHonesty，包含只接受明确 preparation-failed 回执才释放 UI owner |
| 严格 Clippy | **两套通过，0 warning** | App + 默认 updater、独立 no-zip；offline / locked / -D warnings |
| TypeScript / 格式 / diff | **通过** | tsc -b、workspace/vendor fmt、diff --check；未把历史 ESLint/Node/远程 CI 结果称为本轮运行 |
| 权限与上游资产 | **通过** | recovery 限本地 main/session-* 两条命令，不进入默认权限；upstream IIFE hash 未变 |
| 选定源码 / 依赖 | **110 项核对无漂移** | 仅三个 Rust 文件及 GROK-PATCH.md 相对上一封存源变化；不是全产品最终冻结候选；App Cargo.lock 未变 |

每套 8 个 ignored 中，原生 callback parser 本轮显式单独执行；其余 7 个是父测试运行的 owned-child 入口，不重复计成额外顶层通过数。所有新增 staging fixture 的 cleanup guard 都拒绝派发，inert MZ payload 不是可执行安装器。本轮未启动安装态 Grok App、未执行真正的 Grok 安装/卸载程序、未签名、未发布、未修改用户安装注册信息。

构建来源保存在 `app-updater-build.jsonl`、`nozip-build.jsonl`，按 Cargo compiler-artifact 精确选择 harness，再嵌入 Windows 测试 manifest。`executables/` 留存本次测试二进制，`final-review.json` 核对配置、结果和范围；`receipt.json` 与独立 Python 分块 SHA 校验记录封存完整性，而非产品完成证明。

上一封存回执保持不变：`windows-completion-writer-20260930/receipt.json`，SHA-256 `d0d913d3c98ffc14f32c13212c6458221c2f488ccfa1775cea1c0fb02b4d8d40`。原生 callback 输入复用必须明确，不将旧 fixture 运行时间伪称本轮。

## 剩余完整范围

仍须完成并逐项验证 Windows x64、macOS arm64/x64、Linux X11 与原生 GNOME Wayland；Desktop / managed browser / 已有 Chrome/Edge tabs / App WebView；App / ACP / MCP；完整 actions/input、中文 IME、clipboard、取消及恢复；签名 clean install/update/repair/rollback/uninstall；原生窄窗口/DPI/权限 UI；真实 Grok E4；**同一个最终冻结候选的 12h active soak**。

本轮解决的是安装前可证明未执行的失败恢复，不是安装器执行后的失败/回滚完成证明。后者和跨平台原生验收仍在下一阶段范围内；不得用本轮 library/fixture 通过替代最终版验收。

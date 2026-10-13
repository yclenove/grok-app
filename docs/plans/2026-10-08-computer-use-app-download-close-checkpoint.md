# Computer Use 原 App 下载关闭检查点

2026-10-08，`feat/computer-use-implementation`，HEAD `8d8b71c2`。
单 Codex writer，保留原 WIP；本批未 commit、push、PR 或发布。
整体 active / partial — not releasable。

## 实际修复

上一批 Host-owned WebView2 fixture 没有注入资源浏览器的手动下载 polyfill。
改用实际 App library、main WebView IPC、两个真实 SideBrowser child 和原 Wry 下载回调后，
发现 polyfill 拦截 `a[download]`，转入 fetch / reqwest / 普通“保存文件”框，CU 原生计数全部为零。
这解释了为什么底层 fixture 通过不能代表 App 内实际下载可用。

Windows CU 脚本在原生隔离世界运行，不能读取普通页面世界的函数。
曾尝试 page global 的候选仍弹保存框，已移除该方案，未增加 page-world fallback。
最终仅在已保留的 exact anchor 同步 click 期间设置 DOM 标记，finally 恢复原属性；
手动 polyfill 跳过这个点击，其它普通和嵌套链接仍走原桥。
标记不含路径或凭证、不授予权限；原 native instance / URL expectation / Broker staging 继续决定准入。
395 行 polyfill 提取到 `side_browser_blob/polyfill.js`，Rust 用 include_str 与 JSON label 注入，
保持手动 blob/data/http 下载逻辑；JS 回归直接使用这个正式文件。

保留失败 close owner 期间，`side_browser_create` 明确拒绝重建/导航同 label 的原实例，
只能在真实回调交付后显式重试关闭。异步 IPC 与原五秒门禁没有改大或绕开。

## 实际证据

候选目录均在 `src-tauri/target/codex-test-runtime/`，首败、日志和 profiles 保留。

| 目录 | 结果 |
| --- | --- |
| `app-download-close-first-20261008` | fixture settings 缺必要字段，App 回退默认设置；尚未进入下载，不计 IPC/Wry 验证 |
| `app-download-close-native-20261008` | 实际 App 弹普通保存框，CU 回调未发生；原清理未完成，随后 exact PID/path/creation 验证后强制终止 |
| `app-download-close-polyfill-20261008` | 页面函数方案在隔离世界不可用，六项回调计数零；原清理未完成，随后精确验证并强制终止 |
| `app-download-close-dom-bridge-20261008` | 原生取消/联合回调已走通；fresh 下载因 canonical Windows 路径前缀检查失败，正常退出；launcher exact-home 单次检查遇到迟退 owner |
| `app-download-close-canonical-20261008` | canonical 文件身份核对、原 IPC/Wry 两场景实际通过；原 App 与 10 个观察到的 browser owners 全部正常退出 |

两个强制收尾的失败 owners report 保留原失败结果，不能改写成正常退出。
失败候选存在 OS exit0，所以 launcher 强制读取 native-result.ok；OS exit0 单独不证明通过。
launcher 现在把 exact-home WebView2 纳入同一个 owner join barrier，覆盖 App 短命时 CIM 未及时看到的子进程。

最终 App probe SHA256：
`D187236DEA272D6DB04C54513955D8BF66B6C887A83E8889365DCFACA9CB8831`。
Cargo 来源：`app-download-close-canonical-build-20261008.jsonl` 唯一 App probe executable。

| 原 App 场景 | 结果 |
| --- | --- |
| 两个同时 Close IPC | 原下载已收 16384 bytes；held Wry 完成期间 UI 和 keeper 响应；原回调释放后收尾 105ms |
| Close 超时和显式重试 | 原五秒门禁实际 5017ms 失败，原 view 保留，同 label create 拒绝；释放原回调后显式重试收尾 28ms |
| 每场原下载 | nativeStarted / requested / nativeTerminal / finished / processed 各 1，browserExited=0；原 adapter idle，原 observer/install/COM 退休 |
| 每场 same-label 新 owner | 新 instance、同物理 browser、37 bytes 内容校验和原真实回调各 1 |
| 零重放与清理 | 旧 caller 重放零网络、迟到 body 不复活，staging 文件清空 |
| 原进程 | Start-Process 原 handle joined，11 个记录 owners 全部退出，exact-home 复查无遗留 |

105 / 28ms 是**释放真实回调后的收尾时间**，不是整次下载/完整关闭性能。
人为保留的是原 Wry 回调的交付；没有捏造 DownloadStarting / StateChanged / Finished，
没有让 browser exit 替代单 view 的联合终态，没有延长门禁、刷绿或放宽断言。

该入口是 feature-gated debug App probe，调用原 grok_app_lib::run 与正式 IPC/Wry，
并非安装包或 fresh frontend 验收。Broker request/picker/TaskGrant 走真实代码，
Host approval/publication pin 为 synthetic；realAcp / realMcp / realModel 均 false。

## 窗口、回归与清理

本批亲自前置并截图检查唯一用户预览 PID24536（2026-10-07 20:32:07）：
真实窗口正常显示 `127.0.0.1:1421` 聊天页、侧栏、输入框与 Grok4.7 极高，没有连接失败页面。
该预览未重建，尚未含本批下载修复；不是新的安装包或 CU 真人全链证明。
最终 native fixture 窗口被枚举到，但在下一次截图调用前正常退出；不声称截图看过它的两页。

正式 DOM/polyfill 回归 24/24、App fixture 回归 3/3 实际通过。
Cargo 本轮唯一 libtest 复制到 `app-download-close-regression-20261008/webview-tests.exe`，
用 Windows SDK mt.exe 嵌入原 `windows-test-manifest.xml` 后实际运行：
WebView 127 + Host 9 + side_browser_blob 4 = **140 passed，0 failed / ignored**。
后链 libtest SHA256：`A507BFA841D4AF4727E31E6EEC71A0BFDD398B627BC20F4867BD2116FC3282F1`。
三组 strict 实际 exit0，包含新 optional bin，不使用上一批结果代替：

    cargo clippy -p grok-app --lib --tests --bin computer-use-app-probe --features computer-use-probe,tauri/custom-protocol -- -D warnings
    cargo clippy -p grok-app --lib --tests -- -D warnings
    cargo clippy -p grok-computer-use-probe --all-targets -- -D warnings

质量脚本已实际 PASS：≥1000 行文件 80/80，App + AppWorkbench 13573 没有增长。
真实用户预览前置后再次发送刷新快捷键，画面正常；同一 Vite PID35932 监听 IPv4 1421，
显式不走代理的 loopback curl 返回HTTP200。PowerShell 默认网络栈那次请求五秒超时，
不能用它证明 WebView 加载失败，也没有把那次请求算成功。

原 build/test owners 已结束。精确校验两代旧 cache 位于本工作树 incremental，
无 reparse，后续同 crate 的已完成新一代存在，逻辑文件量 **2,841,777,753 bytes**。
删除命令被自动审批拒绝，工具仅报告 `blocked by policy`；未改 shell/工具绕过。
因此本批未删除任何 artifact，实际回收 **0 bytes**，暂缓目标为：

- `src-tauri/target/debug/incremental/grok_app_lib-33kef7nod9ufp/s-hmzunw5qns-0svy5mc-7aa0z3ictlaq8zptu89s0nt13`：2,839,076,200 bytes。
- `src-tauri/target/debug/incremental/computer_use_app_probe-2a0hblyazupb9/s-hmzuot00d9-0gihzl5-4s8s3vm8voz99lltdt035vj7x`：2,701,553 bytes。

它们仅是可重建加速缓存；相同 Cargo App build 可生成新的 cache，不能保证目录名或 binary hash 相同。
当前 native 候选、当前 manifested libtest、所有首败 profiles/日志、依赖/seed、用户 home/预览全部保留。

## 剩余交付

1. EmbeddedBrowser 的 close 超时目前主要是 console.warn；补可见的失败、原 owner 保留和显式重试流程。
2. DownloadStarting 前取消、unknown / cancel grace、最终 staging publication 的竞争。
3. App 原生 upload 与实际服务器 oracle。
4. 更新用户 App/包，亲自查看并接原 ACP/MCP/真人 Grok 验收。
5. 原 Windows、macOS arm64/Intel、Linux X11/Wayland、四 surfaces、安装升级回滚、模型矩阵、
   UI/UX、同一冻结候选 12h active soak 与分批交付均保持开放；本局部通过不能缩小交付标准。

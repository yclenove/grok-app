# Computer Use 共享浏览器下载关闭检查点

2026-10-08，`feat/computer-use-implementation`，HEAD `8d8b71c2`。
单 Codex writer，保留原 WIP；本批未 commit、push、PR 或发布。
整体 active / partial — not releasable。

## 实际发现

之前的 disposable Close 只证明最后一个 controller 关闭后，原 browser exit 能退休丢失回调。
新增 `cu_probe webview-download-shared-close` 使用一个全新 probe profile 和两个真实 controller；
先核对两者 BrowserProcessId 相同，再让下载实际收到首段并保持剩余 body 未释放。
关闭下载页时 keeper 保持运行，不允许用 browser exit、逻辑 closed 或 timeout 冒充下载完成。

两个首败均保留原 EXE、日志和失败 profile：

| 候选目录（相对 `src-tauri/target/codex-test-runtime/`） | EXE SHA256 | 结果 |
| --- | --- | --- |
| `webview-download-shared-baseline-20261007-235900` | `784ED90C208E1E6CF8D71351A6D41E214E38D6D486207B5C09D4DF47E0D68814` | 真实 `(requested,claimed,finished)=(1,1,0)`，单 view close 无终态 |
| `webview-download-shared-cancel-20261008-000100` | `E511670BF0EF1D361B486E108F00063DAD4DD56EAA5CF53E0A43B6423F696595` | 无条件发 Cancel 后立即 Close 仍丢失回调 |

Host shutdown 先设置 alive=false，消息循环可能不消费队列中的 Shutdown；取消必须落在 STA 必经收尾。
更关键的是共享环境中 Cancel 是异步的，controller Close 会撤销下载回调，必须在真实联合终态后关闭。
没有延长原五秒门禁、放宽断言或刷绿。中间 drain 候选真实通过 42ms，但不拼到最终候选。

## 源码修复

- Host 新 profile 模块区分共享 controller 与最后 browser owner，仍拒绝 caller 已存在的 profile。
  共享 fixture 在取消后用原 STA 事件泵等待真实 native/Wry 终态，再 Close；没有固定等待或假计数。
- Windows 下载 admission 与关闭使用同一 installs gate；Closed 状态不被迟到安装回复恢复。
  已通过早期 document check 的旧 caller 也不能在实例退休后注册新 expectation。
- App 的 `side_browser_close` 改为异步命令，阻塞等待在 worker 中，UI 可继续交付原回调。
  Windows close owner 捕获原 Webview/instance；并发调用加入同一 owner，失败保留 owner 供显式重试。
  只在原下载物理终态后才派发 native close，遇到 replacement 拒绝，不重新按 label 关闭新 view。
- BrowserProcessExited observer 保留 token，在原 STA 注销；native DownloadOperation 带原 instance，
  仅在联合终态或确切 browser exit 后清理该 instance 的 COM 引用。共享 fixture 检查 installs、
  observer、native COM registry 均无本实例残留，取消/导航本身不构成释放证明。
- App-shell 验收调用也移出 UI 线程。没有新增 App/AppWorkbench 状态或 UI 文案。

源码 App 入口已接上述逻辑，但**原 App / IPC / Wry 实际关闭入口的慢下载验收尚未跑**。
本批 native 主证据仍为 Host-owned WebView2，Wry bridge 是 fixture 的真实 native StateChanged 转接；
Host approval/pin synthetic，ACP/UI/真人 Grok、安装包和长稳均不能据此标记完成。

## 最终候选与验证

证据目录：`src-tauri/target/codex-test-runtime/webview-download-shared-final-20261008-001700/`。

- `cu_probe.exe` SHA256 `9ED9D163AB38D978BEEF2F4670EF8CF4C6353D00C587E11251687B11FCB386CD`。
- `webview-tests.exe` SHA256 `61DDD37D5CACC3B3F500BB8A8AF0D3B30FEA6C5B71B188711CEA0C7208644FD7`。
- Cargo artifact 来自 `webview-download-shared-final-build.jsonl` 的唯一 libtest 结果；复制后按 CI 流程用
  Windows SDK mt.exe 嵌入 `windows-test-manifest.xml`，未直接运行未经后链的 Windows test binary。

同一最终 probe 候选：

| 门禁 | 实际结果 |
| --- | --- |
| shared single-view Close | 44ms；真实终态=1、owner exit=0、keeper 可操作；新 owner 同 browser 下载成功 |
| held Pause / Stop / Navigate | 68 / 69 / 81ms；真实终态=1、清理及零重放/零迟到 body 复活 |
| disposable Close | 1259ms；真实下载终态=0、确切 browser exit=true，unknown；新 native owner 成功 |
| ordinary download | 两次成功；37 bytes 内容/路径验证；重放和错误身份零派发；8MiB+1 拒绝，network=claimed=finished=3 |

`webview-download` 与 lifecycle 最后一项 Close 的退出窗口有短暂重叠；两者独立 profile/进程，
结果保留，但这些数字不是串行性能基线、冻结候选或长稳结果，不拼接成性能/12h 证明。

实际 libtest：WebView 127 + Host 9 = **136 passed，0 failed / ignored**。
新回归验证旧实例退休拒绝迟到 admission、不影响新实例、两终态前不确认物理退休，
以及联合终态后 caller 尚未收尾时仍保留其占用；不是用 compile/no-run 计通过。

下列命令实际 exit0：

    cargo clippy -p grok-app --lib --tests --features computer-use-probe -- -D warnings
    cargo clippy -p grok-app --lib --tests -- -D warnings
    cargo clippy -p grok-computer-use-probe --all-targets -- -D warnings
    cargo fmt --all -- --check
    python scripts/check-code-quality-gates.py --mode final
    git diff --check

质量预算仍 80/80，App+Workbench 13573。源码编译出现 MSVC 创建导入库的 linker stdout warning，
构建 exit0，未通过调整 lint 或删除断言掩盖。三组 strict 不含该 build-only linker 消息。

## 进程与清理

本批原 probe / harness exec owners 均已返回终态，进程复查无 cu_probe/webview-tests 遗留。
唯一用户预览仍 PID24536（2026-10-07 20:32:07），`target/debug/grok-app.exe`；
本批未重建/替换该 App，保留其 home、登录和用户会话。没有启动另一份用户预览。

逐一校验绝对路径、SHA256、owner=0、父目录及文件无 reparse 后，非递归删除三个废弃 EXE 副本：

- `webview-download-shared-drain-20261008-000500/cu_probe.exe`：34,442,752 bytes。
- `webview-download-framing-20261007-224000/webview-tests.exe`：75,697,664 bytes。
- `webview-download-joint-20261007-230200/webview-tests.exe`：75,748,864 bytes。

逻辑文件量合计 **185,889,280 bytes**；两次删除前后 H 盘即时空闲增量合计
**185,892,864 bytes**。分别为 34,443,264 与 151,449,600 bytes；不把整个开发期间的
空闲空间变化归因于清理。清理自检最初误匹配执行删除检查的 PowerShell 自身，复查后仅排除
当前检查 PID，仍拒绝其它 live/uncertain owners，没有停止任何进程。

这些是旧通过候选的可重建副本，日志、hash、原首败、当前候选/当前 harness 与源码仍保留。
生成同类副本：Cargo 返回的确切 executable → Copy-Item 到独立 runtime → mt.exe 后链。
复现旧行为需对应历史源码差异，当前源码构建不是旧 binary 的同 hash 重建。
所有失败 profiles、当前成功 profiles、依赖、incremental、immutable seeds、用户包和 home 暂保留；
没有删除工作区、target、node_modules 或 shared cache。

## 下一批必须做

1. 加原 AppHandle + side_browser_close + Wry 的 held 下载门禁，证明异步 IPC 期间 UI/keeper 继续响应、
   原 requested/finished 回调、staging 清理、same-label replacement、并发 close 与失败后显式重试。
   实际 App 的超时恢复提示仍需验收，console 捕获错误不能作为 UX 完成证明。
2. 覆盖 DownloadStarting 前取消、dispatch outcome unknown、cancel grace、最终 staging publication 竞争；
   未派发和已派发但终态不明必须分清，禁止未知结果自动重放。
3. 接 App WebView upload 的原生文件准入、准确 input/ref/document/owner 与实际上传服务器 oracle。
4. 重建用户 App/包，先亲自查看窗口和操作，再交给用户。继续原 ACP/MCP/真人链。
5. 全部原三平台、两 Mac 架构、X11/Wayland、四 surfaces、安装升级回滚、模型矩阵、UI/UX、
   同候选 12h 和分批交付保持开放；不得把本检查点缩成最终交付标准。

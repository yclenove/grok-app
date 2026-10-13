# App WebView 下载诊断检查点

后续进展见[慢下载生命周期检查点](2026-10-07-computer-use-download-lifecycle-checkpoint.md)：
Pause/Stop/navigation/disposable Close已验并修browser exit丢回调退休；133回归、质量80/80通过。
下方40/40及83>80属于历史候选；shared单view关闭/timeout/原ACP/UI/fresh包/跨平台/长稳仍开放。

2026-10-07，`feat/computer-use-implementation`；未 commit、push、PR 或发布。
整体保持 active / partial — not releasable。最新候选是下方联合退休修正，历史失败仍保留。

## 已实现与边界

下载已接原 TaskGrant / Broker / App WebView adapter / Windows WebView2，文件落到
run-owned staging。探针覆盖两次连续下载、actionId 成功及失败重放零二次派发、
错误 generation / snapshot / ref 零副作用，以及超过 8 MiB 拒绝发布并清理。
这些通过场景不能代替长稳、慢下载生命周期或真人 Grok 全链验收。

## 未解决的真实失败

固定候选 `E7FAD72918736A9A66D9EB0448D7AE7619AD3C88D36CAAA4A790AD83BD2FE085`
位于 `src-tauri/target/codex-test-runtime/webview-download-stable-20261007-221000/`。
完整 40 轮：38 passed / 2 failed。

- 第 35 轮超限下载：network=4、native=(4,3,2)，服务器收到两次超限请求；
  68.097 s 后原生结束仍未确认，总耗时 71.34 s。
- 第 36 轮初始 NavigationCompleted 的 IsSuccess=false。
- 成功轮 3.30–5.55 s，平均 3.63 s；原 `stability-40.log` 保留。
- 更早候选观察到 WebView2 interrupt reason 12（NETWORK_FAILED）；不能称用户取消。

此前增加连接 worker 和 socket 超时并没有解决根因，不能标记稳定。

## 本批改动与验证

`probe_download/http.rs` 将一次 TCP read 改为完整读取有上限的 HTTP 请求头，
支持请求行和 CRLF 任意分段，拒绝截断及超过 8192 bytes 的请求。
服务器记录读取/写入失败类型、页面请求数，并保留超限请求数与写入字节统计；
不输出 URL、Cookie 或 Token。初始导航失败补 WebErrorStatus 数值。

固定候选 `DD311F7B4712C8255E1FC9258F8F72158490FD522C66A98EB6A3DC0A91F9A956`
位于 `src-tauri/target/codex-test-runtime/webview-download-framing-20261007-224000/`。
复测再次出现 network=4 / native=(4,3,2) / native-terminal-timeout，
超限写入 ConnectionAborted，因此完整请求头修正不足以修复此问题。
本批在完成 30 轮（18 passed / 12 failed）后停止已拒绝候选的原 runner PID47576，
身份含创建时间和 exact command 校验；第31轮原 native child 允许自然结束，未计通过。
它随后已退出，原日志保留。不能将此批写为完整40轮或只选成功轮作为结论。

上述固定候选的 manifest 后链 harness：WebView 121 / Host 9，共 130 passed，
包括 3 项新请求头分段、截断及上限回归；probe strict Clippy 通过。

下一诊断候选新增 fixture-only `GROK_CU_DOWNLOAD_PROFILE`，使用已有
spawn_disposable / BrowserProcessExited 见证对比共享 profile；不修改产品 profile。
取消前仅记录原生 State、InterruptReason、BytesReceived 与 TotalBytesToReceive，
并先 clone COM operation、释放 RefCell borrow 后再 Cancel，避免同步 StateChanged 重入。
此后候选必须重新构建并验证，不得借 DD311F7B 的结果。

诊断候选 `C65D2296A9970B2CB22ED73C7BCF9CB95DB2A3FE4813C8F7FE798F927B234119`
的6个独立新 profile：1 passed / 5 failed；5个失败均读到 native state=IN_PROGRESS、
received=0、InterruptReason=24（SERVER_CONTENT_LENGTH_MISMATCH），BrowserProcessExited
均实际确认。共享部分6次因测试 runner 留下空 profile 环境变量在准备前拒绝，
不是网络测试，不能计入共享 profile 比较；随后修正空值等同未指定。

## 非阻塞 socket 与联合退休修正

Windows Winsock 新接受的 socket 继承非阻塞 listener 模式。服务器 listener 用
nonblocking accept，但 worker 原来直接 read / write_all；改为明确
`stream.set_nonblocking(false)` 后才设置原5 s / 15 s IO timeout。
新增真实 TcpStream 接收完整 8 MiB+1 body 的长度与内容回归。

固定候选 `1BA2AB108278958351E91D864C3A375A3E00C4F94461277788450BEC6FC918CB`
完整40轮：38 passed / 2 failed，成功3.16–4.49 s，平均3.52 s；没有重复网络请求、
socket 写入失败或68 s超时。2个失败转为正常 size-rejected / 第二次成功下载时
Wry终态计数仍差1，证明前一版在另一结束回调到达前返回，不能标为稳定。

最新候选 `87160F27BF4214A571D870923C66D3FDA7E2A1559B0D01B0CAA4EE1F19379764`
位于 `src-tauri/target/codex-test-runtime/webview-download-joint-20261007-230200/`。
现在等待 native terminal 与 Wry完成后返回，caller/Wry/terminal三方齐全才释放
ScriptTracker；另一回调也通知等待者。fixture 在通知 caller 前记录自己实际收到的
claimed operation 终态，保持原 exactly-once / staging / 原生计数断言不变。
单元回归新增/加强 native-first 与 Wry-first 中仍不能提前 idle 的断言。
本候选实际完成40/40，3.23–4.91 s，平均3.62 s；另6个独立profile全部通过，
每个均收到真实 BrowserProcessExited（pump 310–381 ms）。原 runner exit0，
本批cu_probe owners均已退出；没有增大业务超时、固定sleep或放宽原断言。

fresh manifest 后链 harness `C826514FA6297481CDA0BBC5A281C9653C10B547973C91FF2A3E8A4C64A63D72`：
WebView122 / Host9，共131 passed；包括4项HTTP回归和加强的联合退休占用断言。
以下本候选检查均实际exit0：App probe-feature lib/tests strict Clippy、
App default lib/tests strict Clippy、probe all-targets strict Clippy、cargo fmt及git diff --check。
最终大型文件预算仍是下方已记录的FAIL，不借这些定向检查称全部门禁通过。

最终代码质量脚本本轮实际失败：FILES_OVER_1K_BUDGET=83，要求 ≤80；其它项通过。
不能写 FINAL=PASS，后续仍需处理大型文件预算。

## 下一步

1. 以服务器首字节/channel 见证真实在途，补 Pause、Stop、导航、Close，
   核验原生终态、staging 清理、旧请求零复活与新 owner 不受旧取消影响。
2. 覆盖 timeout/cancel grace与原native实例真实退出后的占用退休。
3. 再推进 upload、原 ACP/MCP/Grok/统一 UI 与 fresh 用户包验收。
4. 修大型文件预算83>80；后续新增下载生命周期应拆领域模块，windows.rs已982行。

macOS arm64/Intel、Linux X11/Wayland、四 surfaces、安装升级回滚、模型矩阵和
同候选 12 h soak 等完整范围仍开放。

## 用户预览与产物

本批通过 computer-use 实际激活并截图 `target/debug/grok-app.exe` 的 Grok 窗口，
确认显示新会话主页、侧栏和输入框，来源为 `http://127.0.0.1:1421/`，
而非用户先前的 localhost 拒绝连接错误页。开发入口修复见同日 dev-origin 检查点。
仅证明预览界面启动，不能视为 CU 整链通过；保留该用户预览及其 Vite。
测试结束后已重新激活并截图该窗口，仍为正常新会话主页。该预览exe为20:23:41构建；
本批只重建了cu_probe及harness，不能称新下载修复已打入fresh用户App包。

失败 probe/日志保留为诊断证据。成功 harness 副本和成功独立 profile 须在原
test/browser owners 退出且身份、目录边界、reparse points 校验后清理，保留日志和 hash；
未退出或归属不明的目录延期，不停止其它工作树进程。

清理实际被自动执行策略拒绝，工具仅返回 `blocked by policy`。
批量目录清理未执行；缩小为两条literal、非递归的测试副本删除也被拒绝。
因此本批删除目标为0、实际回收0 bytes，不换shell/工具绕过。
两个已核验owners=0、无reparse point的可丢弃harness副本暂留：

- framing目录 `webview-tests.exe`，75,697,664 bytes，
  SHA256 `2EEA886469F93D88B59397A30D3CE32931B7DF23EA48A4EC1CB186ABEEA90E6B`；
- joint目录 `webview-tests.exe`，75,748,864 bytes，上述C826514F hash。

二者逻辑总量151,446,528 bytes，不能称已回收。可由Cargo明确返回的harness
Copy-Item到独立目录，再用Windows SDK mt.exe嵌入windows-test-manifest.xml重建。
C65成功profile round-4和joint成功profiles round-1至6同样暂留；后者均有exit_event=true。
失败profiles、所有probe候选和日志保留为证据，源码/原home/当前预览/依赖不删。

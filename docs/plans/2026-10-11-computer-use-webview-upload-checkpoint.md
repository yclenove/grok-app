# App WebView 上传基础与测试退出码检查点

2026-10-11，工作区 `grok-app-computer-use`，分支 `feat/computer-use-implementation`。
本批未 commit、push、PR 或发布。整体仍为 **active / partial — not releasable**。

## 本批完成

- `HostSelectedUploadFile` 保留 Host 选中的不可变内存内容，不重新打开路径。
  当前上限 8 MiB，支持空文件；文件名和 MIME 校验，Debug 不输出内容或文件名。
  构造该值不是用户文件同意，也不是 TaskGrant。
- App WebView 通过原生隔离世界的固定内核，将 File/DataTransfer 赋给原观察中的
  file input；不接受模型给的路径、URL、selector 或任意可执行脚本。
  file input 不再宣称可 click/set_value/type_text 打开系统文件框。
- 原 run/target/native document/snapshot/ref/执行 epoch 均保留。
  本轮补齐非零但错误的 `targetGeneration` 拒绝，拒绝前不派发文件内容。
- 一次 assignment 消耗原 snapshot；取消/超时后保留原 native operation 占用到实际结束。
  staged publication 在原绑定、页面与取消 guard 下提交。已经完成的赋值不能被 Pause 撤销，
  但不得发布旧成功或重放赋值。Verified 只证明字段赋值，不证明网站最终提交成功。

主要代码：`computer-use-core/src/adapter/upload.rs`、
`src/computer_use/webview/upload.rs`、`dom.rs` / `dom.js`、
`webview/tests/upload.rs` 与 `webview/probe_upload.rs` / `probe_upload/server.rs`。
这些路径相对 `src-tauri/`。

## 实际 Windows App 证据

本批统一证据根为 `src-tauri/target/codex-test-runtime/`。
Cargo 新构建日志：`app-upload-build-20261011-01.jsonl` / `.stderr.log`，exit 0。
实际执行 EXE SHA256：
`02E890A55511CD0AC38043D6BDEB3CF45ADFD5E38BC9E416A66F79F2D1614D45`。

`app-upload-native-20261011-01/` 的原 App library + SideBrowserLiveView + 固定内核
与独立 HTTP receiver 已实际通过 **8 场景**：

1. 非零但错误的页面 generation：NotDispatched，receiver 零文件。
2. 中文文件名、34-byte 二进制内容与 MIME 精确一致；input/change 各一次。
3. 原 snapshot 重放拒绝，receiver 请求数不增加。
4. file input 的 accept 身份改变后，原 ref 拒绝。
5. 中文空文件赋值与独立接收成功。
6. assignment 前 abort 拒绝。
7. assignment 已发生后 abort：原 bytes 被独立见证，但 publication=false、零重放。
8. 原生导航拒绝旧页面请求，新 owner 的完整 8,388,608-byte 文件精确一致；
   原生赋值和接收约 1382 ms，最后 idle。

最终 receiver 仅 4 次实际 POST。整轮原 App exit 0，观测到的 7 个 owners 均退出。
本批 `emptyConnectionsWithoutHttp=0`；这不能反推第一次失败一定是 speculative socket。

该证据明确 `hostApprovalSynthetic=true`、`taskGrantPath=false`、
`realMainIpc=false`、`realAcp=false`、`realMcp=false`、`realModel=false`，
没有导入登录数据，前端不是 fresh 构建。它证明 adapter，不证明产品上传全链。

## 退出码根因与修复

安装中的 Tauri 2.11.5 `App::run` 文档和实现表明它直接退出进程，不返回。
tauri-runtime-wry 2.11.4 的 Windows RequestExit 分支使用 ControlFlow::Exit，
因此以前 `app.exit(2)` 的失败报告可能仍伴随进程 exit 0；bin 在 `run()` 后读报告的逻辑不可达。

现在仅 Windows debug + `computer-use-probe` 的专用入口，在隔离 instance ID、
绝对 App home、harness=1 和明确模式校验后使用 `run_return`。
普通产品入口仍使用 `run`；两入口共享同一原始 ExitRequested/Exit cleanup callback，
未绕过 CU 正常收尾或先强制杀进程。
专用 bin 在 event loop 正常收尾后读取报告，检查 ok 与原 mode，再退出 0/2。
release 和非 Windows 的专用 bin 明确拒绝。

`app-upload-negative-20261011-01/` 与正常轮使用完全相同 SHA256；仅在自己的
independent home 将 closeToTray=true，实际 App gate 拒绝执行：ok=false、result=null、
error=`upload gate requires an isolated independent App`，原 App **exit 2**，唯一观测 owner 退出。
没有使用 shared `~/.grok` 或复制用户凭据。runner 自身 exit 0 表示它正确验证了该失败，
不是把 native 上传判为成功。

runner 新参数为 `-RejectCloseToTray`，只用于 upload 的负例；它对原 exit code、报告、
设置及 owner barrier 分别校验。既不吞失败，也不允许更换候选 EXE。

## 回归与质量

`app-upload-regression-20261011-01/` 包含本轮 Cargo JSON 唯一返回的 App libtest、
Common Controls v6 manifest 嵌入与读回、测试与三组 strict 日志。
原 harness SHA256：`4367265F186B6A59DFAC3D46BEE5B11EEE0DFA536E95A4FC25EC3488E7B3536E`；
执行副本 SHA256：`009DD1E0584802E89011819B6F0F69260F30CC5DDF413803A1ECA68A7F49B372`。

- App WebView 139、Host 9、side_browser_blob 4、Core upload 2：去重 **154 passed / 0 failed / ignored**。
- DOM/polyfill 24、App fixture 3：**27 passed / 0 failed**。
- `cargo clippy -p grok-app --locked --lib --tests --bin computer-use-app-probe
  --features computer-use-probe,tauri/custom-protocol -- -D warnings`：exit 0。
- `cargo clippy -p grok-app --locked --lib --tests -- -D warnings`：exit 0。
- `cargo clippy -p grok-computer-use-probe --locked --all-targets -- -D warnings`：exit 0。
- fmt、JS syntax、PowerShell parser、diff 与本批源码 LF 检查通过。
- final 质量脚本 PASS：App + AppWorkbench 13573，≥1000 行文件仍 80/80。
  这只是代码质量，不等于完整 CU 可交付。

## 首败与清理

首次实际上传 `app-upload-native-20261008-01/` 的 HTTP receiver 报
`failed to fill whole buffer`，report=false 而原进程 exit 0；全部现场、报告、原 EXE/home 继续保留。
后续 parser 只接受尚未读到任何 header 字节时的 EOF 为 unused connection；
截断 header/body 仍失败，8192-byte header bound 保持，不吞 body。
两个 parser 单测通过；本批成功不能宣称已正向测定首败 socket 的来源。

build/test/native 原 owners 均已退出。精确校验三份已替代副本的路径、hash、类型、
父目录和 live owners 后，清理调用仍被自动执行策略拒绝；**零删除，实际回收 0 bytes**，
未换 shell/工具绕过。暂缓逻辑大小 **246,649,856 bytes**：

- `app-upload-regression-20261008-01/webview-tests.exe`：76,146,176 bytes。
- `app-upload-regression-20261008-02/grok-app-lib-tests.exe`：76,164,608 bytes。
- `app-upload-negative-20261011-01/computer-use-app-probe.exe`：94,339,072 bytes。

路径均相对证据根。两个旧 libtest 可由 Cargo no-run + 本项目 manifest 后链重建，
不保证新编译 hash 与旧版本一致。负例 EXE 可由保留的当前 native 候选同 SHA256 复制再生。
当前可复测 native 候选、当前 libtest、所有报告/manifest/log/hash、首败 home 与 source/seed 保留。
以前被拒绝的 download EXE / incremental 清理仍暂缓，本批未重试它们。

2026-10-11 检查时未观察到用户 Grok 测试 App 或 1421 listener，未自动重启旧预览。
2026-10-08 的主页截图只代表当时的旧后端，不能作为本批用户包证据。

## 追加的原下载关闭首败与 fresh02 复验

追加 `app-download-close-regression-20261011-01/` 使用同一个上传候选检查原 main IPC/Wry 流程，
实际 **失败**：App close IPC observation deadline expired，requested=1/completed=0/failed=0，
nativeTerminal=1/wryFinished=1/wryProcessed=0，原 native view 被保留。
runner 的原始 owners.json 记录 originalProcessJoined=false，不能改写为正常收尾或绿灯。

源码显示 timeout 场景用同样五秒观察 timer 等待产品的五秒 close cutoff，存在回执观察竞争。
错误 cleanup 又试图按 label 重新打开已经 fenced 的 view，因身份已退休而跳过原 callback release；
stderr 明确留下 `native download witness dropped with an undelivered Wry callback`。

当前补了仅 fixture 的八秒 IPC 观察预算（产品五秒 deadline 未变），以及持有原 controller 的
RAII completion guard，在 PendingDownload 析构/join 前先 fence 原 run、释放原真实 callback。
guard 不重查 label、不找新 view、不制造 terminal、不重播下载。
fresh02 已实际重新构建，正常原下载关闭两场景通过；但主动在 held 时引发断言失败的
error unwind 定点仍待单独验证，不能把正常 success path 当作该负例证据。

原 App 后续的 kernel Process readback 显示 HasExited=true，但 CIM 仍保留 zombie record；
不能只用 CIM 记录认定 alive/exit。其专属 side-browser 原 root15168 仍活着，已核验原 parent30348、
exe、home/profile 与 creation identity（CIM 微秒和 kernel 100ns 精度对齐），仅强制结束该原失败
fixture browser 并 join。最后所有原 kernel owners 均无 live；这只是失败现场回收，绝非 native success。
未停止其它 App/浏览器，未删除任何现场，原始 owners/report/log 不改写。

fresh02 的三轮 native 统一 EXE SHA256：
`9506583655B03E0DB147BE9703E8A41A30C42B24C1C9C966F20B78D84E58CC0C`。

- `app-download-close-regression-20261011-02/`：原 main IPC/Wry 两场景通过。
  两并发 Close 与五秒超时后显式 retry，各自原 native/Wry requested/finished/processed 均恰好一次。
  真实失败 cutoff 5015 ms，释放后 close 32/26 ms；keeper/UI 可用，原 native resources 退休、
  staging 空，同 label 新 owner 的 37-byte/hash 正确，无重复下载。
  App exit 0，9 个观测 owners 均退出；Host批准仍 synthetic，ACP/MCP/model=false。
- `app-upload-native-20261011-02/`：8 场景再次通过，App exit 0，7 个 owners 均退出。
  8 MiB 的 assignment/接收约 1626 ms；本轮实际出现 1 条 header 前 EOF 的空连接，
  因而证明空连接确会在真实浏览器发生，但仍不能把第一轮未知首败来源写成已测定。
- `app-upload-negative-20261011-02/`：原隔离条件负例正确 exit 2，1 个 owner 退出。
  runner 三轮均 exit 0 表示各自预期结果与 owner barrier 验证通过。

`app-upload-regression-20261011-02/` 是包含 guard 修复的 fresh libtest。
原 harness SHA256：`38EF8732ABD4ED9F3713B99917774281D180A10000B8C8F147D4DE7EC820C532`；
后链副本 SHA256：`19CDABA5AA799066610E3A5C3F56B72D39C770125E79FA1223A45D3E59B9E1ED`。
App WebView139/Host9/blob4 共152 passed，0 failed/ignored；fresh02 三组 strict 均实际 exit0，
Core upload2 passed，因此当前源去重154 passed。日志在同一 fresh02 目录，不借 fresh01 代替。
本轮不再尝试被策略拒绝的清理；新当前 native/test 候选与这次关闭失败现场也保留。

## 下一步与完整范围

1. 加真实 held callback 断言失败的 unwind 定点；fresh02 strict/Core2 与正常关闭两场景已验。
2. 接 Host 文件选择与明确上传同意，ticket 绑定原任务/run/target/generation/snapshot/ref/action。
   只在用户选择后读入精确文件内容；模型参数不能代替同意、提供路径或跨任务复用 ticket。
3. 接 Broker 原 TaskGrant / ExecutionPermit / admission / action fingerprint / 结果账本。
   失败、超时、取消占用与所有 replay 边界保持；same actionId 不能再次发文件。
4. 接 UI、ACP/MCP catalog 与模型上传工具，复用任务内授权体验；到动作时请求所需同意。
   真实选文件、取消、Pause/Continue/Stop、导航/重绑以及网页最终提交均要独立效果见证。
   当前 `computer_upload` 仍拒绝，不能标为已可用。
5. D22 Windows private native download buffer 的 crash/异常退出 journal 与维护恢复仍开放。
6. fresh 用户 App/包、原 ACP/MCP/真实 Grok、Windows/macOS arm64/Intel/Linux X11/Wayland、
   四 surfaces、安装升级回滚、完整模型矩阵、UI/UX、同一冻结候选 12h active soak 与分批交付均保持。

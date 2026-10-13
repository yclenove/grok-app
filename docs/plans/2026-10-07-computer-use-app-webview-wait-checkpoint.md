# Computer Use App WebView Wait 检查点

2026-10-07，分支 `feat/computer-use-implementation`，基线 HEAD `8d8b71c2`。
本批未 commit、push、PR 或发布；整体仍是 active / partial — not releasable。

## 本批实现

App WebView 的 typed subset 现在支持 semantic `Wait`：

- observation 只为当前可等待节点发布 `wait` action；
- `nameEquals` 必填、去空白后非空、UTF-16 不超过 256；
- `timeoutMs` 默认 2000 ms，显式范围 1–10000 ms；
- 每约 50 ms 检查一次原 snapshot 保存的 DOM `WeakRef`，不按名称重新查找；
- 同一 DOM 对象的可访问名称延迟变化可命中；同名替换对象、不可见、文档变化、
  binding/epoch 变化、Pause/Stop 均拒绝；
- reply 严格限定为 `{ok, matched}`，不把页面字段或错误文本回显给模型；
- 成功和超时均不消费原 snapshot，成功后可继续使用同一 exact ref 做后续动作；
- Broker 整链把命中记录为 `Verified`，一次 wait 只消费一次模型观察预算。

匹配回执到最终验证之间的窄竞争也已关闭。命中后先保持原 binding admission，
再取得原 native document authority；该回调只检查 epoch、取消、binding 和原 snapshot，
不在 document guard 内再次查询 `is_current()`，因此不递归锁 native document。
导航、关闭和 renderer failure 使用同一 authority 顺序，若它们先发生则旧匹配不能提交；
若 Wait 先提交，则该瞬间成为线性化点，随后导航不改写已经成立的历史结果。

Windows probe Host 把 generation、alive 和 document authority 收拢为一个生命周期结构，
避免为通过 Clippy 添加豁免。产品 `SideBrowserLiveView` 继续使用原生
`NativeView::with_document_authority`，没有降低为元数据或 URL/标题比较。

## 权威验证

- CI 同款 manifest 后直接运行本轮 Cargo 返回的 `grok_app_lib` harness：
  **92 passed / 0 failed / 2259 filtered out**。新增确定性竞争测试先让 DOM 回执命中，
  再让导航在最终 native commit 前获胜，旧 Wait 被拒绝。
- 真实 `cu_probe.exe webview` 首次直接启动遇到已知
  `0xc0000139 STATUS_ENTRYPOINT_NOT_FOUND`；嵌入 `windows-test-manifest.xml` 后，
  同一构建完整通过并终态输出 `computer_use webview: PASS`。
- 真实 WebView2 关键门禁：
  `wait_same_object=true`、`wait_snapshot_retained=true`、
  `wait_replacement_stale=true`；原 timeout/deadline/setup cancel/renderer exit/
  normal close 门禁同时通过。
- `cargo clippy --lib --features computer-use-probe -- -D warnings`：通过。
- `cargo fmt --all -- --check`、`git diff --check`、
  `node --check src-tauri/src/computer_use/webview/dom.js`：通过。

## 实际窗口检查

按用户要求，启动后不是只查进程或端口。通过 Windows 窗口自动化按完整可执行文件路径
唯一选中 `H:\aicoding\grok-app-computer-use\src-tauri\target\debug\grok-app.exe`，
实际打开并连续两次查看。当前窗口正常显示 Grok 主界面、会话侧栏、输入区和
`http://127.0.0.1:1421/` WebView 内容，没有 `ERR_CONNECTION_REFUSED`。

用户截图中的页面是 Microsoft Edge 的 `localhost` 拒绝连接页，不是当前 Grok 主窗口；
本轮检查时窗口清单中也已没有该 Edge 页。另一个
`Grok Session Import Preview` 来自独立验证目录，未触碰。

## 构建产物清理

构建、测试和 probe owners 均已退出；当前用户试用 Grok 预览保留。清理前 H 盘空闲
约 291.17 GB。已精确验证下列可再生产物均位于本工作树、目标本身不是 reparse point：

- 昨日前 32 个 `target/debug/incremental` 代，逻辑大小 17,275,111,921 bytes：
  `cu_probe-{15b2dhsrphdq4,1c4ogl0rlw2tj,1gk5gw2pdf5b1}`、
  `grok_app_lib-{0l30gxr2li4mk,0n3mcki2m3oer,0x86lpm4ymz0q,19rkwzfh4mf5a,
  1t0ofz3nlklwx,28mpg1j4ghpco,28s4frpzb5aaa,2icup5f8khgov,2owadcr28y5pk,
  33kef7nod9ufp,3ge7u524pbbwu,3h14f346xx91m}`、
  `grok_app-{07z6e1jhq8nlk,0si54zzeoiwl5,0zb2w8boanpou,11u1ct6kgt8uj,
  17oohfirt685y,1bx1dffws9ljh,1ehrquch08182,1u77c9r814gz9,28jhc9wm32otw,
  2czj6l7r91tdz,2ev888z2e6r2j,2iax6w1o6n5er,2p7u3hu9w6xso,
  2xuutkgng2zyi,30f77974l8uy7,3f1jlw7sph2hv,3iowx85xuw6qa}`；
- 三套旧 harness 的 `.d/.exe/.pdb` 共 9 个文件，逻辑大小 2,025,529,146 bytes：
  hashes `ceb3eb1764add76a`、`dc611c8f8bc4f678`、`941fe17c09eef67d`。

原生 PowerShell 精确删除被当前执行策略拒绝，未换 shell 绕过；因此本批实际回收
**0 bytes**，以上目标全部 deferred。它们可由 `cargo test --lib --no-run` 或
`cargo build -p grok-computer-use-probe --bin cu_probe` 重建。今天的增量代、当前
`63e3432e557be5b6` harness、当前 App/probe、依赖、源码和证据均保留。

## 后续

下一批按既定顺序继续 App WebView `type_text`，然后 navigation/reload/history，
最后 upload/download staging。完整范围仍包括 Windows、macOS arm64/Intel、Linux
X11/native Wayland，Desktop/Managed Browser/Existing Tabs/App WebView，App/ACP/MCP，
安装升级回滚、模型矩阵、UI/UX、真人整链与同一冻结候选 12 小时 active soak。


# Computer Use App WebView `type_text` 检查点

2026-10-07，分支 `feat/computer-use-implementation`，基线 HEAD `8d8b71c2`。
本批未 commit、push、PR 或发布；整体仍是 active / partial — not releasable。

## 本批实现

App WebView typed subset 现在支持可验证的 semantic `type_text`：

- 只为 `textarea` 以及 `input` 的 text/search/url/tel/email 类型发布能力；
  `contenteditable` 继续只提供 `set_value`，不虚假承诺可靠的光标插入；
- 使用模型刚观察到的 exact DOM ref 和该控件当前 selection，不按名称、selector
  或相同 id 重新查找替代对象；
- 使用原生 `value` setter，在原选择区插入中文、emoji 或普通文本，并按 DOM 的
  UTF-16 光标单位设置新 selection；
- 先发送可取消的 `beforeinput(insertText)`，通过后写值并发送
  `input(insertText)`；不发送会暗示提交/失焦的 `change`；
- 页面在 `beforeinput` 期间改值、改 selection、改焦点、替换对象或阻止事件时，
  原动作失败且 snapshot 已消费，业务层不得以同一 actionId 重放；
- 最终同时复验 exact 对象、焦点、完整新值和 selection，只有全部成立才返回严格的
  `{ok:true, verified:true}`；页面字段值和页面控制的错误文本不回显；
- 最大 4000 个 Unicode 字符，拒绝 NUL、额外 selector 和错误参数；
- 明确拒绝 `via=clipboard`，不读取、覆盖或恢复用户全局剪贴板；
- Adapter 将通过后的动作标记为 verifiable/postcondition_ok，Broker 整链结果为
  `Verified`，而 click/set_value/scroll 仍不会被错误升级为 Verified。

## 权威验证

- `node --test tools/computer-use-probe/webview-dom.test.mjs`：
  **21 passed / 0 failed**。覆盖中文、emoji、选区插入、事件顺序、事件阻止、
  字段隐私、对象替换和一次性 snapshot。
- CI 同款 manifest 后直接运行本轮 Cargo 返回的
  `grok_app_lib-63e3432e557be5b6.exe computer_use::webview --test-threads=1`：
  **95 passed / 0 failed / 2259 filtered out**。新增
  `broker_reports_webview_type_text_as_verified` 已实际证明 Broker 结果为 Verified。
- 重新构建并嵌入 manifest 的真实 `cu_probe.exe webview` 完整通过，关键门禁为
  `type_text_verified=true`、`type_text_private=true`，终态输出
  `computer_use webview: PASS`。原 Wait、timeout、deadline、setup cancel、renderer
  exit 和 normal close 门禁同时通过。
- `cargo clippy --lib --features computer-use-probe -- -D warnings`：通过。
- `node --check src-tauri/src/computer_use/webview/dom.js`、
  `cargo fmt --all -- --check`、`git diff --check`：通过。

## 构建产物清理

本批 Cargo、rustc、Clippy 和 `cu_probe` owners 均已退出；用户正在试用的唯一当前
预览 `src-tauri/target/debug/grok-app.exe` 保留并未停止。H 盘清理前空闲
292,220,600,320 bytes。

已逐项解析绝对路径并验证位于本工作树 `src-tauri/target/debug` 内、不是 reparse
point；本批确认可再生但删除被执行策略拒绝，未改用另一 shell 绕过，因此实际回收
**0 bytes**。deferred 共 14,542,658,506 logical bytes：

- 27 个 2026-10-07 前的旧 incremental 目录，13,244,755,244 bytes：
  `cu_probe-{15b2dhsrphdq4,1c4ogl0rlw2tj,1gk5gw2pdf5b1}`、
  `grok_app_lib-{0l30gxr2li4mk,0n3mcki2m3oer,0x86lpm4ymz0q,19rkwzfh4mf5a,
  28mpg1j4ghpco,28s4frpzb5aaa,2owadcr28y5pk,3ge7u524pbbwu,3h14f346xx91m}`、
  `grok_app-{07z6e1jhq8nlk,0si54zzeoiwl5,0zb2w8boanpou,11u1ct6kgt8uj,
  17oohfirt685y,1ehrquch08182,1u77c9r814gz9,28jhc9wm32otw,
  2czj6l7r91tdz,2ev888z2e6r2j,2iax6w1o6n5er,2p7u3hu9w6xso,
  2xuutkgng2zyi,3f1jlw7sph2hv,3iowx85xuw6qa}`；
- 两套旧 harness hash `dc611c8f8bc4f678`、`941fe17c09eef67d` 的
  `.d/.exe/.pdb` 共 6 文件，1,297,903,262 bytes。

这些目标可由 `cargo test --lib --no-run` 或
`cargo build -p grok-computer-use-probe --bin cu_probe` 重建。今天仍被构建复用的
incremental 代、今天更新的 `ceb3eb1764add76a`、当前 `63e3432e557be5b6` harness、
当前 App/probe、依赖、源码和验证证据全部保留。

## 后续

下一独立批次继续 App WebView navigation / reload / history，再做 upload / download
staging。完整范围仍包括 Windows、macOS arm64/Intel、Linux X11/native Wayland，
Desktop/Managed Browser/Existing Tabs/App WebView，App/ACP/MCP，安装升级回滚、模型
矩阵、UI/UX、真人整链与同一冻结候选 12 小时 active soak。

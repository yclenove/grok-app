# ExistingTab：Host typed action 队列与版本协商检查点

日期：2026-09-20。工作区 `H:\aicoding\grok-app-computer-use`，
分支 `feat/computer-use-implementation`，HEAD `30757366`。
总状态 **partial — not releasable**，默认关闭，未 commit/push/PR。

## 已实现

设计依据：[typed action 派发 ADR](2026-09-20-computer-use-existing-action-dispatch-adr.md)。

- `extension_action` 定义严格 Rust v2 request/command/negotiation/result 类型。
  语义单次左键、set_value/type_text、垂直 scroll、Wait 与已有 JS 内核相同；
  未增加 eval、任意参数、坐标点击、key 或 navigation 能力。
- `host/actions` 在同一授权锁内核验 session/run/tab、grant/document generation、
  Chrome documentId、原 snapshot 及原可用 ref；每条动作有单调 10 秒期限。
  原文档 generation 不一致、无效 ref 或能力未协商均无法入队。
- 显式 v2 协商绑定当前 connection；动作列表无重复、同连接不可改，重新配对清空。
  v1 poll 永远只返回 Observe；v2 单独 poll 返回动作，一次派发，不重送丢失响应。
- `CompletionRegistry` 支持 queued reservation。入队时没有 key；首次派发才
  生成 proof，Host 仍只保留 HMAC，不把明文 key 存在待发队列里。
- v2 claim 携带完整 request + proof，并和已派发的不可变 request 逐字段比较。
  命令、文本、ref、sequence、deadline 均不可替换。registry 的 bound 标记使
  旧 generic completion claim 即使持有正确 proof 也无法领取该动作。
- applied/verified 必须先 claim 并已有效 settle；rejected/unknown 可以结束
  业务结果等待，claimed 物理占用保留至独立回执完成。结果单次提交、不重放。
- 混合容量仍为 8，以观察、动作业务等待和物理回执的并集计数，同一动作不重复
  计数；浏览器已结束但业务结果未提交时仍保留该 tab 的准入占用。
- Broker feature gate 和真实 HTTP 路由已接通：
  `/cu/extension-actions/negotiate|poll|claim|result`。无公开 enqueue/授权接口。
  正文上限 64 KiB，精确 Origin/Host、重复头拒绝、无 forwarded、Bearer 检查；
  单独有界限流桶。解析/协议失败不回显原始正文、proof 或用户输入。

请求/命令 Debug 不打印用户输入，completion key 继续脱敏。普通清理 status/settle
仍可在关闭功能/撤销后完成原回执，不授予新动作权限。

## 首败与修复

证据根目录：`tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/`。

新增行为测试先在 `existing-action-dispatch-occupancy-red.log` 实际失败：settle
已经删除 registry 项，但动作结果还在排队，旧 reborrow gate 只检查 registry，
允许在业务完成之前重新授权。现 grant/reconnect 共用动作及回执占用检查。

修复后同一测试在 `existing-action-dispatch-pause-red.log` 暴露第二个实际失败：
`cancel_actions` 仅退休观察及 registry，没有同步唤醒已 settle 的业务等待者。
现 Pause/Stop/return/release/连接失效把动作队列也纳入同步清理；不会等下一次
轮询才撤销业务等待。两条原失败证据保留，断言没有放宽。

`existing-action-dispatch-host-first.log` 是从仓库根目录运行 Cargo 导致的
“找不到 Cargo.toml”调用错误，不是测试断言。随后从 src-tauri 正确执行，
`existing-action-dispatch-host-first-build.log` 中最初 10 项通过。

## 已验证

| 检查 | 结果 / 日志 |
| --- | --- |
| 新 Host/registry/wire/HTTP | 19 个新增测试；完整套件包含在下行 |
| Core 全量 | 442 passed，0 ignored；`existing-action-dispatch-core-full.log` |
| Driver 全量 | 12 passed，0 ignored；同上 |
| 真实 loopback 新接口 | 4 项；`existing-action-dispatch-http-first.log` |
| Core 严格 Clippy | all-targets/all-features、`-D warnings`；`existing-action-dispatch-core-clippy.log` |
| App 严格 Clippy | lib + computer-use-probe、`-D warnings`；`existing-action-dispatch-app-clippy.log` |
| 质量门禁 | final PASS，千行文件 80/80；`existing-action-dispatch-quality-first.log` |
| 格式与语言目录 | cargo fmt、diff whitespace、15 extension locale check |

HTTP 测试实际通过新路由排队/单次领取/settle/业务提交；包含改写文本、未先
settle 的成功结果、重复领取/结果、功能关闭后只允许清理、错 Origin、未知字段、
错误版本和 64 KiB 正文上限。4000 emoji 的请求大于旧 8 KiB，能够经 Host 新
poll 完整传输；这不证明目前的 JS PairingClient 已能读取该大小。

## 下一项：JS 客户端、生产 SW 与重启隔离

本批没有修改 JS。`ActionCompletions` 仍调用旧 generic claim，当前探针仍由
私有 pipe offer。正式 `ExtensionTransport/sw.js` 仍 Observe-only，模型能力
仍 NONE。上述 loopback 测试不是浏览器动作验收，也不是 MCP `computer_act`。

下一批按顺序完成：

1. 为 PairingClient 增加严格 v2 negotiate/poll/claim/result 方法。claim 提交
   完整 `{request, proof}` 到新路径，不能降级旧 generic claim。只对 v2 poll
   放宽到 64 KiB；其他配对回复仍 8 KiB。参数 null/多余字段、UTF-8/Unicode
   边界、旧 epoch、未协商、错完整 binding、响应丢失必须有测试。
2. 将 ActionCompletions 接到真实 transport/SW，reset 保留旧原脚本、watcher
   和 cleanup owner。新模型结果与旧清理权限严格分开，结果和动作不重试。
   协商失败不能被推断为具备动作能力；保留旧 v1 的纯观察边界。
3. 新真链探针由私有 fixture 仅进行授权/入队，proof 和动作全部从生产 HTTP
   poll 派发、claim/result 回传；实际 Chrome DOM 计数器/CJK 字段验证副作用。
   现有 generic receipt 探针是上一阶段证据，不覆盖此新队列。
4. 单独实现 App/SW 重启后的旧占用与原文档退休证明，处理 64 条/5 分钟
   tombstone 淘汰后的恢复。不得把丢失的内存对象当物理完成，也不能关闭用户 tab。
5. 这些门禁通过后再接 ExistingBrowserAdapter.act 与实际 MCP 闭环、开放确有
   证据的 capabilities，并继续 key/navigation/contenteditable/页面 scroll parity。

完整后续范围不变：toolbar activeTab 和成功截图、原生启动内部取消、App/ACP
生命周期/UI、安装修复升级回滚卸载、Chrome/Edge、macOS arm64/x64、Linux
X11/GNOME native Wayland、真实模型、同一最终候选至少 12h 主动长稳。
历史 Windows IPC 10053 根因仍未关闭，不能用通过的复测代替解释。

最终重建、原链回归与指纹见下续节；尚未发生的检查不能记成已通过。

## 重建与原完成回执真链回归

`cargo build -p grok-computer-use-probe --bin cu_probe` 通过，日志
`existing-action-dispatch-build.log`。仍有既有 Windows linker 的“正在创建库”
stdout warning；严格 Core/App Clippy 单独通过，没有增加 lint 豁免。

本批重建的 `cu_probe existing-tab-completion` 全部 8 项通过，日志
`existing-action-dispatch-completion-live.log`。它在 owner 标记的隔离 App home
及新 Chromium profile 下验证原 generic receipt 通道与真实浏览器点击、CJK、
丢失 claim/settle 回包、Wait 取消、unpair 后原脚本/取消脚本共同收束；用户 tab
保留。此为原链回归，不把它冒充新 v2 队列派发或生产 MCP 动作验证。

源码局部指纹（非发布冻结）：307 文件，
`530ACBEF2C0870F2F6751C95CDD1B648B179883E81716AA6A14B17E8ECE9E4E6`。
cu_probe SHA256：`5D503F88EFD47BD1F9BF9EB181D4F4E9C6FED433215C325155B808242172B80E`。
日志 `existing-action-dispatch-fingerprint.log`。范围/算法沿用 Host 回执检查点：
Core src、App computer_use、browser/extension/MCP/probe tools、Computer Use
components/lib/API 及其 authorize/pairing 测试；rg 遵循 ignore，rs/mjs/json/html/
css/ts/tsx，路径转 `/` 后 PowerShell Sort-Object -Unique；各行“路径 空格 大写
文件 SHA256”，以 LF 拼接（无末尾 LF），UTF-8 后再 SHA256，不含文档和构建产物。

`cu_probe existing-tab-extension` 在同一重建候选下全部 34 项通过，日志
`existing-action-dispatch-observation-app.log`。真实 process-wide App Broker、
私有 Node/MCP、MV3、Chromium 的原观察/预览/Stop/撤销链没有回归；fixture
Stop 为 5 ms、10000 元素截断观察往返为 282 ms，仅为单次测量。它仍不包含
toolbar 成功截图、新 v2 动作队列或真实模型操作。

最后只读检查匹配本批 cu_probe、隔离 private Node、owned Chromium profile
的存活进程数为 0。探针依 owner marker 清理自己的临时 profile，证据与隔离
App home 保留。HEAD 未变、index 空，未操作日常 profile、账号或代理。

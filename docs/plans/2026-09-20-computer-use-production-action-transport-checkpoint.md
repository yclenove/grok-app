# ExistingTab：生产 SW 与 v2 动作通道检查点

日期：2026-09-20。工作区 `H:\aicoding\grok-app-computer-use`，
分支 `feat/computer-use-implementation`，HEAD `30757366`。
总状态 **partial — not releasable**。默认关闭，adapter 动作 capability 仍 NONE。
未 commit/push/PR；不是最终发布冻结。

## 本批实现

- PairingClient 的 v2 negotiate/poll/claim/result 已接通，握手回复严格校验。
  领取提交完整不可变 request + proof；协议版本、当前 connection、命令和
  Unicode 均校验，不降级到旧 generic claim。只有动作 poll 回复上限为 64 KiB，
  旧配对等回复仍为 8 KiB；4000 码点输入可完整传输。
- 独立动作轮询与 v1 观察轮询共存。动作 sequence 不能倒退或重复；协商失败
  保留只读观察。领取、动作、业务结果各尝试一次；未知结果不能重放副作用。
- 生产 `sw.js` 通过 SharedTabs 执行动作；reset 后仍由原 ActionCompletions
  持有原始脚本、取消调用、watcher 和独立清理对象。仅清理回执有有界退避重试。
  浏览器 timer 包装保持正确 receiver，不能替换为脱离 globalThis 的原生方法。
- 新 `cu_probe existing-tab-dispatch` 使用真正的 source `sw.js`，私有管道只
  提供隔离 fixture 的授权/入队。命令、proof、领取、结果与清理都经过生产 HTTP，
  不从管道把 proof 交给页面，也不在可信扩展页另建 transport 冒充 SW。
- 故障注入只在该隔离浏览器实际 SW 内包装 fetch/executeScript，保存的 packet
  只在该 worker 内存中，输出仅阶段、计数、状态。独立 DOM 计数/字段值验证副作用。

原 `existing-tab-completion` 探针显式注入 generic claim，以保留旧的 unbound
receipt 固定用例。它是历史协议 fixture，不是产品动作降级路径。

## 真实失败：配对限流阻塞导航撤销

第一轮新增 9 项真链通过。再补“丢失动作 poll”与“领取时导航”后，后者在
等待候选退休时超时：扩展收到真实导航事件，发出 unoffer，但未成功撤销。
失败保存在 `existing-action-transport-live-final.log`，细分阶段与计数分别在
`existing-action-transport-navigation-diagnostic.log`、`...navigation-counters.log`。
这些文件名中的 final 不代表通过；以实际退出码和断言为准。

根因：challenge/confirm/offer、status/heartbeat、unoffer/disconnect 共用
8 次/800 ms 的 pairing 限流，OPTIONS 也计数。连续真实配对和分享可能用尽额度，
阻塞导航 unoffer 的预检；随后的断连回退也可能被同一额度拦下，只剩租约过期清理。
没有证据表明发生误点，但不能把这个延迟当作正常的即时撤销。

新增真实 loopback 回归先失败（`...retirement-red.log`，3 failed/6 passed），
分别固定握手饱和后的撤销、续租/断连，以及独立撤销额度的有界性。修复为固定
pairing、extension-lease、extension-retirement 三个桶，OPTIONS/POST 使用
各自同一桶；每桶上限、Origin/Host/Bearer/连接身份与正文规则不变。
续租不能消耗撤销额度；错误 key 不能撤销。`...retirement-fixed.log` 的 9 项通过。

## DOM 测试清理

旧测试在 jsdom window.close 后仍可能触发 MutationObserver；此前即使出现
异步异常也显示通过。现 VirtualConsole 把 jsdomError 变为测试失败，测试
fixture 销毁前先 retire snapshot，补足 jsdom.close 不发送 pagehide 的差异。
产品 DOM 代码未因此改变。首败 `...teardown-red.log` 与修复日志保留。
`...js-clean.log` 是中间结果，仍有 observe teardown 异常，不作为干净证据。

## 验证与证据边界

日志根：`tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/`。
下表文件均带 `existing-action-transport-` 前缀。

| 检查 | 当前证据 |
| --- | --- |
| 扩展/MCP Node 测试 | `js-final.log`：101 passed，0 skipped；已额外搜索无异步异常 |
| 初始实际 SW + v2 动作 | `live-first.log`：9 项通过，尚未含两个追加边界 |
| 原回执真链 | `completion-regression.log`：8 项通过，发生在限流修复前 |
| 原 App/MCP/MV3 观察真链 | `observation-regression.log`：34 项通过，发生在限流修复前 |
| Core 全量（含限流修复） | `core-final.log`：445 passed，0 ignored |
| Driver | 本批未修改/未重跑；上批为 12 passed，不能写成本批新结果 |
| 严格 Core Clippy | `core-clippy-final.log` 通过 |
| 严格 App Clippy | `app-clippy-final.log`：lib + computer-use-probe、`-D warnings` 通过 |
| 重建后的生产 SW/v2 动作 | `live-retirement-fixed.log`：11 项全部通过 |
| 限流修复后的实际观察回归 | `observation-retirement-fixed.log`：34 项全部通过 |
| 质量门禁 | `quality-retirement-fixed.log`：final PASS，千行文件 80/80 |
| 格式/目录 | cargo fmt --check、15 locale check、JS syntax、git diff --check 通过 |

`build-retirement.log` 为修复后的 probe 重建。既有 Windows linker “正在创建库”
stdout warning 仍在；独立严格 Clippy 通过，没有加入 lint 豁免。
git diff --check 使用仓库既有换行设置通过。禁用 autocrlf 的一次诊断会把既有
CRLF 当作整行新增并报空白；没有为此批量改写用户工作树或变更 Git 配置。

11 项真链分别为：正式 SW 握手、排队单次点击、4000 码点 Unicode、丢失 claim
零动作、丢失 settle 仅重试清理、丢失业务结果不重放、Host 取消实际 Wait、
unpair 等待原脚本与取消脚本、重复 poll 撤销且不再点击、丢失 poll 撤销且零动作、
领取期间导航使候选即时退休并且新旧页面零误触发。最后一项先确认 Host 仍
保留领取占用，再释放滞留 claim 回复，确认正确归还且用户 tab 仍打开。

修复后 34 项回归包含实际 process-wide App Broker、私有 Node/MCP、MV3 和
Chromium 的配对/观察/预览/Stop/撤销。单次测量：Stop 5 ms；10000 元素截断
观察往返 285 ms。成功截图依然未覆盖，截图行仍是 API 拒绝与归一化证据。

## 本批局部指纹与收尾

源码 312 文件 SHA256：
`BE7695287BFA4662A5DC546AA4CC8F8C598B621F5A6058A24277AE4C044C817A`。
cu_probe SHA256：
`9018EE8C96845A1F102EE557C7DF22384B7D8527F1F4DE421458E40B55A4FE20`。
日志 `existing-action-transport-fingerprint-final.log`。这是局部指纹，不是发布冻结。

范围/算法沿用上一批：Core src、App computer_use、browser/extension/MCP/probe
tools、Computer Use components/lib/API 及 authorize/pairing 测试；rg 遵循 ignore，
rs/mjs/json/html/css/ts/tsx，路径转 `/` 后 PowerShell Sort-Object -Unique；各行
“路径 空格 大写文件 SHA256”，以 LF 拼接（无末尾 LF），UTF-8 后再 SHA256。
文档和构建产物不混入源码 hash。此前 fingerprint.log 是注释修订前的中间值，
最终以上述 final 文件为准。

19 个本批源码/新文档已额外检查行尾空白，覆盖 git diff 看不到的未跟踪文件。
收尾时本批 cu_probe、隔离 private Node、owned Chromium profile 存活进程为 0；
临时 profile 由探针按 owner marker 清理，证据与隔离 App home 保留。
HEAD 未变、index 为空；未接触日常 profile、真实账号或代理。

## 还不能宣告完成的项目

真实动作证据使用 private fixture admission，尚不是 session MCP → adapter.act。
App/SW 重启丢失原 Promise/占用的处理未完成；64 条/5 分钟完成 tombstone 淘汰后，
旧客户端清理还可能永久重试。这些关闭前，不开放 ExistingTab 动作能力。

下一批见 [重启隔离与 MCP 接入执行计划](2026-09-20-computer-use-restart-and-mcp-execution.md)。
此外，原生 toolbar activeTab 与成功截图、managed 启动内部取消、App/ACP 生命周期、
界面响应/重开体验、Chrome/Edge 安装交付、macOS arm64/x64、Linux X11/GNOME
native Wayland、真实模型循环和同一最终候选至少 12h 主动长稳仍未齐备。
历史 Windows IPC 10053 根因仍未关闭；本轮成功不能替代该项解释。

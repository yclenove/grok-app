# ExistingTab：文档执行身份与旧观察拒绝

日期：2026-09-20；工作区 `H:\aicoding\grok-app-computer-use`。
分支 `feat/computer-use-implementation`，HEAD `30757366`；未提交/推送/PR。
总状态 **partial — not releasable**，默认关闭，ExistingTab action capability 仍 NONE。

后续最新进度见 [2026-09-21 文档生命周期检查点](2026-09-21-computer-use-document-lifetime-checkpoint.md)。
本文指纹/测试数字属于此前批次；不要将 getFrame null 解释成原文档销毁。

前置：[原文档 SW 恢复检查点](2026-09-20-computer-use-worker-recovery-checkpoint.md)。
决策：[重启 ADR](2026-09-20-computer-use-action-restart-adr.md) 的“文档执行身份”。
本批修复一个真实缺口，不重复建立 journal，也不宣称 App/完整浏览器恢复完成。

## 问题与实现

旧 observeDocument 只检查当前 snapshot，退休后可被迟到的同名观察重新创建。
`existing-document-fence-red.log` 的断言证实旧观察仍被接受；首败发生在重建
检查处，不把尚未执行的后续点击断言写成已经测到的副作用。

新增生产 `execution-clock.mjs`：SW 启动在扩展私有 IndexedDB 中，通过 strict
readwrite transaction 原子领取递增 epoch；事务完成才允许使用。每个 worker
再分配递增 sequence。只保存一条 version/epoch 元数据，不保存 Bearer、动作
输入、页面内容、cleanup proof。并发分配靠事务串行；失败/缺记录/损坏/溢出
拒绝新 Share。计数不因换配对或 App 重启重置，也不使用随机数判断先后。

新增 `document-execution.mjs`：显式 Share 才能向精确 documentId 安装身份，
必须前进且不能越过活动 operation。观察必须有该 epoch、严格前进的 sequence；
无身份、旧 epoch、重复或逆序调用在创建 refs 前拒绝。snapshot 保留所属执行
身份；新 Share 或该身份关闭后旧 action 不能使用它。preview 更新调用顺序，
不替换模型 refs。各个异步注入在发起前分配身份，迟到顺序不会被重新编号。

CompletionRecovery 在原 snapshot 上执行固定取消函数时，额外关闭旧文档
执行身份，再等待原 operation.finished。关闭后同 epoch 的更大 sequence
也不能通过迟到 Share/observe 重开；新 SW 用更大 epoch，用户重新配对/Share
后可继续。旧 cleanup 只能碰匹配 snapshot，不能关闭新 owner。普通动作的
snapshot 退休不关闭整个 epoch，故同 worker 后续新观察仍正常。

SharedTabs 和正式 sw.js 已接线，clock 不是可省略的生产依赖。旧历史 fixture
也显式取得 clock/fence，没有给旧无身份调用保留降级入口。Host proof/claim/
业务结果协议没有放宽。

## 当前验证

日志根：`tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/`。
以下文件名均使用 `existing-document-fence-` 前缀。

| 项目 | 结果与覆盖 |
| --- | --- |
| 扩展/MCP 全量 | `js-final.log`：120 passed，0 skipped，`--test-concurrency=1` |
| 实际生产 SW/Host 动作 | `live-final.log`：18 passed；真实事务分配及五个实际重启切点 |
| 实际观察/MCP 回归 | `observation.log`：34 passed；预览、Stop、导航/切换/滚动/期限竞争与大页面 |
| 历史回执 fixture | `completion.log`：8 passed；仍是可信扩展页/私有 grant，非生产 MCP actions |
| 原生 DOM 内核/Sharing | `kernel.log`：13 passed；值、事件、计数、滚动、Wait 和同 URL reload |
| 静态/构建 | `clippy.log` 严格 App Clippy、`build.log` probe、cargo fmt、扩展语法通过 |
| 文案/质量 | 15 locale 检查通过；`quality.log` final PASS，千行文件 80/80 |
| Core/Driver | 本批未改 Core/Driver 源码，未重跑；445/12 仍是历史证据 |

真实数据库用 owned profile 内独立 fixture 数据库，12 次并发领取得到连续且
不重复的 epoch；原生事务 abort 不提交值；缺记录、错版本、额外字段、负数
和溢出被拒绝。测试不修改生产计数器，也没有切换系统时钟。

五个实际 SW 重启场景，在恢复后重新注入旧 observe 的原参数，均被文档拒绝，
snapshot 保持退休。后续新配对/Share 时再注入旧观察，仍拒绝；当前 epoch
确实大于上次。正常新观察和动作继续由正式 SW/Host 派发。独立点击计数没有
增加，已经发生的 click 保持恰好一次。此项是原参数的真实文档重注入验证，
**不是已经证明 Chrome 自己排队中的原注入跨重启后的执行时序**。

保留的中间日志：`js-first.log` 因旧测试仍期待两参数取消调用，在 barrier 前
断言失败后等待未结束；已终止该自有测试进程并修正为三参数合同。`js-second.log`
为并行 jsdom 运行时一次观察缺少节点的失败；耗时超过观察预算，未放宽产品
200 ms 限制，最终串行全量 120 项通过。`js-serial.log` 是加两项 SharedTabs
合同前的 118 项结果，当前以 final 文件为准。真实浏览器三组/内核全部通过。

## 局部指纹

源码 321 文件 SHA256：
`45F13DECE224118576630D0E00D6756AB627FEED60E7F5455A7AB091E8A566B7`。
cu_probe SHA256：
`34110BE7A97840EC0E50C5D1C3F6360711B67279F82F301BCEBCF43AFF48FA16`。

范围/算法沿用前置检查点：Core/App computer_use、browser/extension/MCP/probe、
Computer Use components/lib/API 及两份 API 测试；rg 遵循 ignore，rs/mjs/json/
html/css/ts/tsx，路径转 `/` 排序去重；“路径 大写文件 SHA256”按 LF 无末尾换行
拼接后 UTF-8 再 hash。文档/二进制不混入源码 hash，不是发布冻结。

收尾：git diff --check 和 26 个明确修改文件的行尾空白检查通过，覆盖未跟踪
源码/文档；最终 Node 日志无未处理异步错误诊断。按 probe/private Node/owned
profile 标识检查，相关进程残留为 0（含已中断的首次 Node 测试）。index 为空，
HEAD 未变；没有操作日常浏览器、真实账号、Cookie 或代理。

## 接下来从这里继续

不再重新实施 epoch、journal 或同原文档恢复。按完整
[重启与 MCP 计划](2026-09-20-computer-use-restart-and-mcp-execution.md) 推进：

1. 原文档 reload/navigation/close 时，恢复不能永远占用，但必须取得可信的
   原文档/操作销毁证据；missing snapshot、API 异常、超时仍不是证明。
2. Chrome 已接收但未执行的原注入、扩展 reload/update 后不同 isolated world
   和旧版本序列化函数；新检查不能约束旧版本未带检查的函数，必须有迁移边界。
3. 64 条/5 分钟完成记录淘汰后的 cleanup-only 恢复；有界、可终止且不放宽
   活动 proof 验证。再补真实 App 终止/新 endpoint 与 App/SW 两种重启顺序。
4. 完整浏览器退出清除 session journal、旧 Host claimed 占用的收束仍未实现。
5. 上述门禁通过后，接 adapter.act/真正 session MCP，再补 parity、App/ACP、
   UI、安装、三 OS、真实模型和同一最终候选至少 12h 主动长稳。

IndexedDB 完整删除/扩展重装不被解释为旧文档已销毁，也不能自动恢复旧授权。
历史 Windows IPC 10053 根因依然未关闭。默认关闭和动作 NONE 继续保留。

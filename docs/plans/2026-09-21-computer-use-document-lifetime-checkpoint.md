# ExistingTab：文档离开、缓存保留与丢回执恢复

日期：2026-09-21（America/Los_Angeles）。
工作区 `H:\aicoding\grok-app-computer-use`；分支 `feat/computer-use-implementation`；
HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。未提交/推送/PR。
总状态 **partial — not releasable**；默认关闭，ExistingTab action capability 仍 NONE。

前置：[文档执行身份检查点](2026-09-20-computer-use-document-execution-checkpoint.md)。
决策：[重启 ADR](2026-09-20-computer-use-action-restart-adr.md) 的 2026-09-21 更正。

## 已修复与实测中推翻的假设

原实现遇到 SW 重启后 reload，找不到旧 snapshot，Host claimed 占用无法收束。
本批最初把 webNavigation.getFrame(documentId) 的 null 当作销毁证明；正常
reload/navigation/close 的 22 行曾通过，但后续真实缓存测试推翻了这个判定。
Chrome 148.0.7778.96 返回 null 后，后退仍能恢复同一 documentId 及原点击计数。
对应版本 Chromium 的 RenderFrameHostChanged 会删除 FrameNavigationState，
即使 DocumentUserData 仍在 BFCache。最初的 22 行不能单独作为正确性结论。

当前 null 一律 unavailable。cached/prerender/pending_deletion 保持占用，
异常、缺 snapshot、权限撤回和超时都不当作销毁证据；不调用 getAllFrames
扫描其他标签。webNavigation 的新增权限只查询已分享/准入的原 documentId。
incognito 权限查询前后均核对，目前这部分只有单元证据。

claim 前先把 journal 写成功，再在精确原 documentId 的 isolated world 安装
固定 guardian；安装回执也必须精确匹配，才能 claim。它在可信且非缓存的
pagehide 上关闭原执行身份、退休 snapshot，等待原 operation.finished，再
出具终态。普通 controller 仍须等待自己持有的原 act/cancel/watcher/native
Promise；新回执不能跳过它们。恢复者只清理原 request，不重放 act/claim/业务结果。

实际测试又发现，SW 已停止时 reload 会丢失 runtime.sendMessage，即便原操作
已经完成且页面已调用发送。已改成先写一次性终态暂存，再发 runtime nudge：

- session journal v3 保存最多 8 个完整原 proof、阶段、incognito 元数据以及独立
  的 256-bit terminal nonce。session 区域仍是 TRUSTED_CONTEXTS。
- guardian 只拿 snapshot/request 身份和独立 nonce，不拿 Host proof/completionKey。
  extension local 暂存仅含随机 requestId 键与 nonce；没有 endpoint、配对凭据、
  页面内容、URL 或动作输入。该 local 区域供扩展自己的 content script 写入。
- storage.onChanged 与启动/重试补读都核对当前 owner 的 nonce。先确认 session
  终态写成功，再清 local 暂存；删除失败保留 owner。旧 nonce、错结构、旧 request
  无法完成新 owner；不恢复授权，也不能据此把 unknown 改成业务成功。
- 已完成 guardian 可能迟到写孤立 key；onChanged 和启动只清理本协议前缀的
  孤立键，保留其他 local 数据。准入 owner 上限为 8，并非宣称磁盘瞬时永远 ≤8 键。
- v1/v2 严格迁移，缺失的 documentContext/terminalToken 保持 null，不伪造证明。

普通 SW 中 unknown 动作现在也拥有逐条清理重试，不必重启才能恢复。仍未结束
的原 native Promise 只占自己的记录，不阻断另一 tab 的重试。释放 SharedTabs
旧槽位时再次核对 request/tab/document/snapshot，且原 act 已返回。

## 当前验证

日志根：`tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/`。
下表文件统一使用 `existing-document-lifetime-` 前缀。

| 检查 | 结果 / 文件 |
| --- | --- |
| 扩展/MCP 单元 | `js-verified.log`：139 passed，0 skipped；串行执行 |
| 实际生产 SW/Host 动作 | `live-verified.log`：22 passed；已移除临时诊断注入 |
| 实际 BFCache | `cache-verified.log`：4 passed；真实离开、后台重启、同 documentId 恢复、后续点击 |
| 实际观察/MCP | `observation.log`：34 passed；preview/Stop/导航/滚动/期限/大页面 |
| 历史回执 fixture | `completion.log`：结果见日志；不代表生产 session MCP actions |
| 严格 App Clippy | `clippy.log`：`--features computer-use-probe -- -D warnings` 通过 |
| 格式/文案/质量 | cargo fmt 通过；15 locale 通过；`quality.log` final PASS，千行文件 80/80 |
| Core/Driver/DOM kernel | 本批未改对应实现，未重跑；先前数字仅为历史证据 |

22 行包含原 controller 回复丢失后 reload、实际 SW 重启后 reload/navigation/close，
以及原来的 claim 前、claim 回复滞留、native 注入前、真实 Wait、click 回复滞留。
缓存 4 行不要求 getFrame 一定返回 cached；必须有 Chrome 的真实查询、原 occupancy
保持、后退后相同 documentId/原计数，以及清理后新 click 恰好一次的独立证据。
这不是用模拟 lifecycle 或换文档代替缓存验收。

保留的首败/中间证据：`red.log` 是原 reload 无法清理；`cache-first.log` 与
`cache-diagnostic.log` 还受 Playwright 等待 load 影响；`cache-query.log` 明确记录
null → 同原文档 active 且点击计数保留；`cache-red.log` 是已否决 null 方案的恢复
失败。`live-guard.log`/`live-receipt-diagnostic2.log` 显示仅 native message 不够；
`live-receipt-diagnostic4.log` 只记录自有 fixture 布尔值和消息类型，确认 pagehide
可信、非缓存、操作已结束、发送已调用，但未交付。首次 diagnostic 有局部变量
作用域错误，记录在 `live-receipt-diagnostic.log`，不算产品断言通过。所有临时
诊断 wrapper 已移除，最终以 verified 日志为准。

测试期间 Clippy 曾与观察探针短暂重叠；两者均完成通过，未将该运行描述为全程串行。
后续 Cargo/浏览器/App 继续按计划串行，避免引入无关的计时噪声。

## 局部指纹

源码 327 文件 SHA256：
`966F9B84F9D7E5966549B01FF064153638EB0917EF06B2CCE66AF4259EEF7BD4`。
cu_probe SHA256：
`B545FF981C440589D522890BC64AF810549F09439228C18BF4D66DD09BC41285`。

范围/算法沿用前置检查点：Core/App computer_use、browser/extension/MCP/probe、
Computer Use components/lib/API 及两份 API 测试；rg 遵循 ignore，rs/mjs/json/
html/css/ts/tsx；路径转 `/`、PowerShell 排序去重；“路径 大写文件 SHA256”用
LF 拼接、无末尾换行，以 UTF-8 再 hash。文档/二进制不混入源码 hash，非发布冻结。

## 下一项与边界

不重做 journal、epoch 或本文已通过的切片。继续
[重启与 MCP 计划](2026-09-20-computer-use-restart-and-mcp-execution.md)：

1. 补非 pagehide 的 renderer 崩溃、终态写入失败及旧版本没有 guardian 的记录。
   当前这些情况保守保持占用。缓存文档不会假装销毁；未恢复前可能仍 busy。
2. Chrome 已接受但尚未执行的原注入、扩展 reload/update 后旧 isolated world、
   旧版序列化函数仍需真实迁移/隔离证据，不能据本次 SW 重启就关闭该项。
3. 实施 R3：64 条/5 分钟完成 tombstone 淘汰后，cleanup-only 的终态恢复，
   不放宽当前 pending proof/status/claim/result，不越过本地物理完成。
4. 实际 App crash/新 endpoint、App/SW 两种重启顺序，以及整个浏览器退出后
   session journal 丢失、Host claimed 仍在的情况未完成。local nonce 不等于
   完整磁盘恢复协议，不能凭它重建 proof/授权。
5. R1–R4 关闭后接 adapter.act/真正 session MCP actions，再做 parity、App/ACP、
   UI、真实工具栏与成功截图、安装 Chrome/Edge、三 OS、真实模型及冻结候选
   ≥12h 主动长稳。历史 Windows IPC 10053 根因仍未关闭。

没有操作日常浏览器、真实账号、Cookie、代理或 VPN；没有新增子代理。

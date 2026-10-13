# ExistingTab：原扩展观察 Promise 所属占用（source86）

2026-10-05，单 Codex writer；分支 `feat/computer-use-implementation`，未提交/push/PR/发布。
整体 active / partial — not releasable。ExistingTab descriptor 仍关闭，raw parser 仍 Desktop-only。
完整 E5、U04–U09/D00–D31、四 surface、三平台/两 Mac 架构、安装、真人 Grok、12h 和 UI/UX 均保留。

## 本批实际修改

新增生产 `observation-operation.mjs`，观察所属的 clock/query/executeScript/capture/normalize
调用都等待原始 Promise。5 秒阶段超时或请求 deadline 只退休结果，不用 Promise.race 提前释放占用。
原解码已经开始时等待其返回，并由原 normalizeCapture 的 finally 关闭 bitmap；尚未开始时直接丢弃旧图。

SharedTabs 保留每 tab 的原观察 owner，最多与 action 合计 8 个。reset、活动变化、unshare
只取消原 owner，直到其原 Promise 结束才删除。原 owner 尚在时，新观察、该 tab 的输入和替代 Share 被拒绝。
失败的新模型观察或取消的 preview 不能恢复旧模型 snapshot。自然成功的 preview 保留原模型 snapshot。
Transport 和 sw 将原连接 signal 传给 SharedTabs；这只是已有 reset 信号，尚不是 Host 的每 request 取消 scope。

新增模块已进入固定生产 allowlist，45 件文件/15 locales/权限/key/最低 Chrome 版本不变。
新模块使 allowlist 顺序和旧固定文件数检查暴露错误：排序已修正，source84 的 45≠44 首败日志保留，
source85 更新预期为 45。source86 同步原 capture/action probe 的 exact module cut；没有放松 native 断言。
未修改生产 Core、manifest、managed seed、前端或用户环境。

## 身份和验证

冻结 source86：3305 files，manifest SHA256
`ced658a931b0f51b32a83c311a90dd4e3bf7b7dbaa435d4a0b8cd564e8bd3528`。
默认 App artifact86 SHA256
`8ff6a52d61593d8a2b99a525792a11ce7790b532dd90107613d4a4bcbf739be9`。
Windows test manifest 已嵌入。默认 EXE 已覆盖82，不能沿旧 artifact82 身份运行新源码。

证据目录仍为 `tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。

| 本候选检查 | 实际结果 |
| --- | --- |
| 新观察占用测试 | 最终 9 项，包含在 Node193；原扩展上的首轮 0/8 失败日志保留 |
| Node / MCP / 诊断 unit | 193 / 38 / 8 passed，exit0 |
| 固定扩展校验 | 7 passed，exit0；45 件生产文件、15 locales |
| App 默认 no-run / App、probe strict | 均实际 exit0；两 strict 为24.38s /24.87s |
| 原 App 内置与导出 | 12 passed，20.89s；嵌入 bytes 与源码、导出内容完全相同 |
| 原截图权限拒绝链 | 1 passed，17.54s；capture1/denial1/settled1/decode0，零自动正文降级 |
| 原 click 派发后 Pause | 1 passed，18.82s；原输入1，旧结果 unknown，原 action/cancel/waiter/scope 均 joined，显式 Continue |
| syntax / fmt / quality / diff / source Verify | 通过；App+AppWorkbench13573、≥1000行文件80，两个主fixture994/983不变 |
| 完整 native、宽 CU/ACP/routing、Core、前端全量 | 本批未跑；R5完整协议接线后按原计划做，不借 source80 数字 |

新单元测试使用 synthetic decoder/Promise barriers，不能替代真实 PNG 或真人 installed App 证据。
两条 native 回归仍是 owned CFT140、原 session MCP/TaskGrant/App/ACP 链；Host/session/ACP peer synthetic。

## 未解决的真实反例与下一步

[source82 原 Pause/Chat Stop 迟到 PNG](2026-10-05-computer-use-task-consent-u04-existing-late-capture-checkpoint.md)
仍 OPEN：Host 删除已派发 pending 后提前 idle，扩展在 Pair/Share 保留时收不到该 request 的 Host 取消。
本批没有更改 Host sweep，也未重跑这两条已知失败刷绿；仅接连接 signal 不会修好它们。
观察 proof/claim/status/watch/settle、用途隔离、首次无旧 snapshot、持久 journal/worker 或 browser 退出收束
以及真实自然成功截图链，继续按[完整 R5 合同](2026-10-05-computer-use-task-consent-u04-existing-observation-lifecycle-next.md)实现。

清理前已核验：原37hash/5absences、64–66的3log/12hash、67/68的2log/6hash、
71/72/74的3log/9hash、75–77的3log/9hash、82的1log/10hash均不变。
fresh `existing-observation-owner-cache-86-1` 已生成并回读精确 Plan：5个旧 App incremental，
逻辑4728314790 bytes（约4.40 GiB）；保留每个同 variant 当前完整 generation 和 current86/依赖/seed/全部首败。
本批原 build/test owners 已终态；另一工作区 Rust 自然退出后 Execute 实际成功，不停止任何无关进程。
清理记录 `existing-observation-owner-cache-86-1-cleanup.json`：5个精确 target 已删除，逻辑4.40 GiB；
H freeBefore244572479488、freeAfter247179354112，空闲观察增2606874624 bytes（约2.43 GiB）。
这是磁盘空闲变化，不等于逻辑文件量；每个 current generation/current App/Core/seed/fix.patch/source
及所有上述首败证据清后复验不变。删除未进回收站，Cargo重新构建可生成这些缓存。
不完整working、跨variant和无明确归属的runtime/失败现场仍保留。

本批新首败日志也保留：`existing-observation-owner-first-failure-83-1.txt` SHA256
`853bd09a604fb32e698501b519cc3d64a9010df3a6a73873edd01f3d2b5b2159`；
`extension-84.txt` SHA256 `f562522f5fcdd3932184567f78d66e89d9b33063fc6e02b302793506bf3794ed`。
83是原扩展上新测试的日志身份，不声称存在冻结source83/App artifact83。

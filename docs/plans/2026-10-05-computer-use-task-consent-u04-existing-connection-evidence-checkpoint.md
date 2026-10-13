# ExistingTab：原 worker 被动连接诊断检查点

日期2026-10-05；单 Codex writer，分支 `feat/computer-use-implementation`，HEAD
`8d8b71c262ab222748bca32412eea884d4b892e0`。未启动 Grok/子代理，未提交/push/PR/发布。
整体仍 active / partial — not releasable，descriptor关闭，完整交付范围与所有首败保留。

## source63 的实际边界

只改test/probe：原CDP session被动保存最多32条loopback CU固定route/status/outcome/elapsedMs，
Pair/disconnect/context-clear计数及原worker stopped/detached状态。不保存URL、header/body、
Bearer、requestId或异常正文，不发新的HTTP、不续租、不查询死亡worker。
primary在辅助await前同步保存脱敏快照；Stop拆出真实后置步骤。
辅助诊断import使用绝对extension URL，但未证明历史空诊断由相对import引起。

证据目录：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。
source63：3282 files，manifest SHA256
`a234bb6f18bfd0e95a42bde12ae3821f72444cd648c4ca5c6346909034e4598c`。
默认App no-run exit0，约2.36s；artifact63 SHA256
`d2a9bcbea9e183c0b843cff94710fbe02dddbd75f462860fdb94465baca381cb`。
Rust未改，二进制与62相同，但本批重新记录artifact，运行加载新的冻结MJS。

| 本批门禁 | 实际结果 |
| --- | --- |
| connection/phase unit与四项syntax | 5 passed；各exit0 |
| publication connection-repair一次定点 | 1 passed，24.62s |
| profile-tab-id-collision一次定点 | 1 passed，57.13s |
| Node / MCP | 177 / 38 passed，各exit0 |
| quality / diff | exit0 |
| 最终源码Verify | 3282 files不变 |

未跑source63完整native、宽CU、ACP/routing或Rust strict；Core/前端未改，未全量重跑。
两个历史失败未复现，不销项；成功场景尚未输出实际HTTP快照，不能冒称取得失败触发证据。
[source62完整矩阵](2026-10-05-computer-use-task-consent-u04-existing-reservation-terminals-checkpoint.md)
仍为40 passed / 6 failed，六个原失败及首次Cancel初始化失败均OPEN。

## 清理核对

AGENTS第9条已固定为每批常规步骤。新身份`existing-connection-evidence-cache-1`的只读规划
返回`No complete obsolete pairs; nothing deleted`，没有执行删除或复用已执行清单。
`existing-connection-evidence-cache-before-1.json`已复验37个原curated哈希和5个原缺失文件未变。
本批未释放空间；截至source62累计实际空闲观察增14765965312 bytes，约13.75GiB。
源码、当前可用构建、seed、依赖、dirty/untracked/fix.patch、失败现场与证据保留。
未知owner/incomplete缓存继续deferred，不杀其它任务进程。

## 下一批

按[publication三终态合同](2026-10-05-computer-use-task-consent-u04-existing-publication-terminals-next.md)
实施新候选64+：原准备run、精确approval UUID、Cancel/Stop/Drop、实际inner join和Stopped。
同候选补完整native与相关门禁；成功报告加入安全的被动连接快照以验证真实采集。
仍保留已commit后关闭picker不撤销grant的反例、queued ACP chat Stop与恢复竞争。

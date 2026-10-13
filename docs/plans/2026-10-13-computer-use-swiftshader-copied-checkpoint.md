# CU 维护浏览器：MD5 与 libbacktrace 特殊通知

2026-10-13；`feat/computer-use-implementation`；active / partial。
上一轮为实质 progress：[SwiftShader 通知](2026-10-13-computer-use-swiftshader-notice-checkpoint.md)
已改变工作区并完成可复验的来源/正文绑定；本轮继续其未解决 copied-source 义务。
用户已确认 App macOS 11+ / CU 受管浏览器 macOS 14+，边界保持。

## 实质结果

从同 sealed SwiftShader tree 采集 **13 paths / 12 独立 Git blobs / 88774 bytes**：
五份 LLVM10/16/llvm-subzero MD5 源文件、`.gitmodules`、根/两库 CMake 和四份
libbacktrace copied config。新 profile 独立输出，不重启或覆盖前批成功 collectors。
同四实际 checkout manifests、regular source paths、Git raw batches、blob/正文/hash
全部复验；Gitlink 不冒充普通源码。
source receipt SHA：`b9b72145db43d7b139f327321f578c70b456d3eee8ee3c62acf47856dc549e58`。

五份 MD5 的完整原 block 均保留 Alexander Peslyak 2001、public-domain 声明失效
时的替代许可、redistribution 条件及无担保条款，不用 LLVM 根 Apache/NCSA 替代。
Linux/Darwin 两份 backtrace-supported.h 的 FSF 2012–2016 / Ian Lance Taylor
原署名、三条 source/binary/endorsement 条件及完整 disclaimer 保持。
其余两 config.h 是无版权声明的生成配置；不伪造 attribution，也不因此认定
整个 libbacktrace 没有版权义务。四份 copied config 的完整 regular 路径集合已对齐。

源码证据明确：Boost 1.78.0 由 CMake 按需下载，原 ZIP/tar.gz SHA 均绑定；
libbacktrace 的真实 Gitlink 为 `5a99ff7fed66b8ea8f09c9805c138524a7035ece`，原
`.gitmodules` 仓库 URL 一致。根 CMake 在 REACTOR_EMIT_DEBUG_INFO 分支初始化
并加入两库，原默认为 FALSE，另有设置 TRUE 的分支；没有运行/求值 CMake。
完整28 GN中相关引用另行记录，源码缺引用不是 binary 排除证据。
未采 Boost 原 archive 或 libbacktrace Gitlink checkout，相关义务保持开放。

## 验证与边界

七个完整 inline notices 生成 **10197-byte partial supplement**，不用于发行。
独立 `swiftshader-copied-notices-01` / `02` 整个 report 与片段逐字节一致：

- inventory：223613 bytes，SHA `ed9429e906d5236dd1282bf79886d5835fd78e5ff644cf53bc1d6d41abe61f36`
- fragment SHA：`c980b90f5f3d764e4954d83ece2e2438f372c8b8c00b22b2d95eae3eef989b1d`

前批完整11通知 byte/path bindings、91194-byte fragment 和原 receipt 保持；
八 SwiftShader 原目录错误、17 yes 错误/196目录问题/5异常值/421 raw gaps 原样保留。
原错误不作清零目标，不为统计采无关测试/Android 源；仍需准确来源/消费关系。

显式完整工具 suite **437 passed / 0 failed / 0 skipped**：integration 29 项与
新增 copied inspector 8 项通过；完整许可、失效 public-domain fallback、binary
条件、Gitlink身份、下载pin、copied inventory、额度、输出不覆盖均有反例。
preledger质量 final 通过；103 LF/EOF/hash、82 Python AST、96严格 whitespace
与原 R2 22源码SHA通过。文档最终回读见下述 final 文件；未复记旧产品/native结果。
App+Workbench13573 / ≥1000源码80 不增长。

证据根：`tools/computer-use-probe/.run/maintained-notice-source-20261013-01/`。
本批 suite：`swiftshader-copied-tool-suite-01.log`；preledger：
`swiftshader-copied-preledger-verification-01.json` / `quality-swiftshader-copied-preledger-01.json`。
最终：`swiftshader-copied-final-verification-01.json` / `quality-swiftshader-copied-final-01.json`；
清理：`swiftshader-copied-cleanup-01.json`。
原 collector session26195 已 exit0，两 inspector 同步 exit0。没有新 compiled site、
runtime 执行副本或重复包，删除0 / 逻辑0 / 实际回收0；source/bare/raw/所有证据、
current App144/依赖/seed/WIP/用户 home 保留，没有停止其它进程或提交发布。

## 后续必须继续

准确处理其余 copied/generated notices、Boost/libbacktrace 与原 ANGLE 义务；
完成四平台完整NOTICE/SBOM/源码提供/LGPL-media与稳定安全更新策略后，接最终
维护pins/四锁/production prepare/check/repair/rollback/bundle。
原[R1–R4](2026-10-13-computer-use-maintained-runtime-next.md)全部范围保持：四架构
原生、所有surface/UI UX、安装签名更新卸载、真人原ACP/MCP/Grok/model/picker、
唯一最终candidate12h/D00–D31。产品仍 Node20/PW1.48/Chromium130，整体未可发行。

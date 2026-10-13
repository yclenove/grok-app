# CU 维护浏览器：Skia 实际消费者与 Dawn 通知

2026-10-13；`feat/computer-use-implementation`；整体 active / partial。
延续 [SwiftShader 特殊通知](2026-10-13-computer-use-swiftshader-copied-checkpoint.md)。
用户已允许 App macOS 11+ / CU 受管浏览器 macOS 14+，边界保持。

## 本批结果

Skia 原固定 tree 的全部 **109 GN/GNI / 636068 bytes**、Chromium `skia/` 的
全部 **4 GN/GNI / 53422 bytes** 已采集，合计 **113 paths / 689490 bytes**。
独立源码树、Git blob、raw batches / Gitiles bytes、四 actual checkout manifests
和每条 source receipt 全部复验。未执行上游 GN、DEPS、hooks 或源码函数。
Skia pin：`c88b49d1d6c7b27b1b611f78e1abf924383e801b`，原 source receipt
使用 `external-license-02/skia`，不借 cache generation 的 receipt。
GN receipt：`dea4f951fb3a70ead4e8227eb8c503341dc337802f2d16e8ead40445f4aba7d2`；
embedding receipt：`1c370fc040c44588ad8d9a390f56ff499c46d188dedf7e641bf808d5fc7256f6`。

Chromium Skia 明确引用 Dawn，默认表达式涉及 Mac / Windows / Linux。
原495默认通知候选没有 Dawn；补入准确来源后得到 **502 候选**，均有完整
许可正文。字面规则与manifest不证明实际有效GN / binary使用或排除。
Dawn 仓库 `https://dawn.googlesource.com/dawn.git`，四实际manifest全部匹配
`088a8d00efd1c70b58c549d355401d115904b648`；root tree
`527b73bb36eeb7ac8e4cfa420d0740b32633ea01`。独立gpu group保留旧groups与receipts，
`external-license-08` 采集 **55 paths / 52 blobs / 178896 bytes**。
根 receipt：`83f2059270865a39f3d0e33e23908baac658612b03a186c88a11bd5cbf098119`；
组件 receipt：`87828aa51164c6d1d791e2ed110d757b3a6fc64e62b8df6bc11029a727f83c72`。
原README写的 `a117f96e09e88e76c0a9e3b553b7316cda50633d` 与actual pin不同，
准确保留差异，不改原字段或用README版本替代实际版本。

九项真实consumer绑定：Dawn、Wuffs、Expat、Fontconfig、ICU、Harfbuzz、
Zlib、WebP（含PATENTS）与Vulkan headers。36份Skia自有GN build glue
保留原版权并绑定Skia BSD；不覆盖其引用的外部库。34个原Skia目录错误
全部保留，也不将standalone `../externals/` 路径改成Chromium alias。
首遍误将Vulkan父目录许可写为 `src/LICENSE.txt`，真实校验拒绝，原失败日志
保留；已按原metadata修正为 `third_party/vulkan-headers/LICENSE.txt`。

加入Dawn后：7629 selected paths / 1277 raw refs / 851 resolved / **426 raw gaps**；
**18 yes带错 / 212目录问题 / 5异常值**。原17 yes错 / 196目录问题 / 421 raw
gaps保留；新增五unresolved引用与16目录问题明确展示，不借主树同名库的
许可代替不同Dawn standalone pins。Rust、copied API、Dawn递归及完整有效图
仍需继续，不将partial片段作为完整浏览器NOTICE或shipping SBOM。

独立 `skia-coverage-01/02` 的整个inventory与 **87418-byte** partial片段已确认
逐字节一致。inventory：1435609 bytes，SHA
`c0d9f760c3f990b7e920e0719a96724dfa90c460f4399c52ef54dae2607deab5`；fragment SHA
`4e478b6c299f1d3bfcf0d5da711a75bbae3d09f20f0e42ab20cd5198e6c8688e`。

## 验证与清理

工具 suite **448 passed / 0 failed / 0 skipped**；不是产品或原生验收。
版本/manifest/GN完整集合/许可字节/原版权/真实consumer缺失/输出不覆盖
均有反例。最终108 LF/EOF/hash、85 Python AST、101严格whitespace、原R2
22源码SHA与质量final通过，格式首败（文档EOF）已修正且原日志保留；
App+Workbench **13573** / ≥1000行源码 **80** 不增长。
证据根：`tools/computer-use-probe/.run/maintained-notice-source-20261013-01/`。
日志：`skia-first-pass-01.log`、`skia-corrected-pass-01.log`、
`skia-independent-pass-02.log`、`skia-tool-suite-final-01.log`。
最终：`skia-final-verification-02.json`、`quality-skia-final-01.json`；
清理：`skia-cleanup-01.json`。collectors、两成功inspectors和final suite的
原owners均已exit0，首败inspector已exit1且保留原失败。

无新compiled site、writable runtime copy或重复包；删除0、逻辑0、实际回收0。
source/bare/raw/全部失败证据、current App144、依赖、immutable seeds、WIP与
用户home保留；其它批live/不确定owner不动。未提交、push、PR或发行。

## 后续仍须执行

按 [R1–R4](2026-10-13-computer-use-maintained-runtime-next.md) 推进：优先Dawn
实际Chromium overrides、新来源缺口及剩余Skia copied notices / ANGLE义务；
完成四平台NOTICE/SBOM/源提供/LGPL-media/稳定安全策略，再最终维护pins、
四锁与production prepare/check/repair/rollback/bundle。随后四架构原生、
四surface/UI UX、安装签名更新卸载、真人原ACP/MCP/Grok/model/picker、
唯一最终candidate12h与D00–D31全部须完成。产品仍Node20/PW1.48/Chromium130。

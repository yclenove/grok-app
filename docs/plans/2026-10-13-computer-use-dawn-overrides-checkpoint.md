# CU 维护浏览器：Dawn 的实际 Chromium 配置与许可来源

2026-10-13；`feat/computer-use-implementation`；整体 active / partial。
延续 [Skia/Dawn 检查点](2026-10-13-computer-use-skia-dawn-checkpoint.md)。
App macOS 11+ / CU 受管浏览器 macOS 14+ 按用户确认保持；两 Mac 原生仍 not_run。

## 本批来源与结果

Dawn 原固定 tree 的全部 **206 GN/GNI / 962226 bytes**，以及 Chromium 的
`build_overrides/dawn.gni` **1323 bytes** 已采集并核验。原 tree、Git blob、
raw batches / Gitiles bytes、四实际 checkout manifests 和 source receipts
均重新验证；没有执行上游 GN、DEPS、hooks 或源码函数。
GN receipt：`6ca7b20b102486d7c29e4395bfc2d4b857405ca857f4cb1dc3d2be562339a0b8`；
embedding receipt：`def386981759e4a4f5bc554677fe7d015b1d80dddfa4f37fe6b8f319b64833fd`。
Dawn pin 继续是 `088a8d00efd1c70b58c549d355401d115904b648`。

实际 Chromium 声明 **17 个目录配置**。其中15项绑定准确原许可文件，包括
ANGLE、GLFW、Jinja2、JsonCpp、Perfetto、SPIR-V Tools、SwiftShader、Vulkan
headers/loader/utility/validation、DXC两个目录、DX headers和Agility SDK。
`dawn_googletest_dir` 与 `dawn_partition_alloc_dir` 仍明确未完成。
配置字面值和引用不证明完整组件通知、有效 GN 图或二进制使用/排除。
Perfetto 已在原加载链完整复验；本批复用封存记录并复验真实许可字节，
不重复合并其源码树。

`external-license-09` 按四实际 manifests 收齐两组件的4个原文件，合计
**21784 bytes**。Utility pin 为 `90738f6edfa19561a978fc5f97543ba37075ba9d`；
其原 `LICENSE.md` 只有304 bytes，保留 Khronos/Valve/LunarG 的2023版权和
SPDX声明，并绑定已封存的完整 Apache-2.0 正文。短声明不替代正文。
Validation实际 pin 为 `c93a48d963e7f311d8ca0511b5f41dc3abe63bff`，不同于
Dawn standalone 声明，不能借用 standalone 版本的许可。
根 receipt：`ea6361295cb448fde18ac19f8cc6eef48e296e6dc9d8be411b00db76673cff16`；
Utility：`41a5c4365b058b9efea7b1cf0529fb360f7995b9cde9cbdaa4133aa9a972176f`；
Validation：`9f626936053c79aed0572e83efc12b678ec3eb2e9db22b1faed6e6ff377ff004`。

Dawn 原 DEPS 静态目录为 **68 Git / 39 non-Git / 0 unresolved / 0 mismatch**。
四实际 manifests 共272次比较：27准确匹配、245缺项、0身份冲突；缺项不是
NOT_SHIPPED。CIPD/GCS合法目录尾斜杠按原声明保留，不转换为 Git 身份或
写入文件系统；内部空段/遍历/绝对路径/Git尾斜杠仍拒绝。
recursive receipt：`1171021ade48eaed6c7488dc5e3c03c78f9ab7941db6999791433ae4daba5520`。

来源汇总由 **7629→7633 selected paths / 1277 raw refs / 851→853 resolved**，
raw gaps **426→424**。只关闭 Utility 和 Validation 两条原许可引用，原引用
和关闭依据均保留；**18 yes带错 / 212目录问题 / 5异常值** 完整保持。
GN文件单独核验，不把它们算作新增许可文件来抬高覆盖计数。
独立 `dawn-coverage-01/02` 的整个 inventory 和片段逐字节一致。
inventory：799300 bytes，SHA
`710e626fab93c08965efd7f0f9d83309530396a97c34bd534f5e5e9bb0d7c020`；
partial fragment：149919 bytes，SHA
`3222f8c455b3dd26391c2906bb58b1d6f250dfe58145a8b51701ba824c34f689`。
片段只作来源证据，不用于发行，不当作完整 Chromium NOTICE/SBOM。

## 验证与收尾

来源工具 suite **462 passed / 0 failed / 0 skipped**，不是产品或原生验收。
第一遍因 Python3.8 不支持 dict union 失败，改为兼容的字典展开；原失败保留。
112项 LF/单一EOF/hash、88 Python AST、105严格whitespace、原 R2 22源码 SHA
和前批 Skia/Dawn 两份完整原证据复验通过。FINAL质量通过，App+Workbench
13573 / ≥1000行源码80未增长；不是完整产品验收。
最终回读：`dawn-final-verification-01.json`、`quality-dawn-final-01.json`；
清理：`dawn-cleanup-01.json`。本批 collectors/inspectors/工具tests已终态，
两个成功 inspectors exit0，首败 exit1且原日志保留。
证据根：`tools/computer-use-probe/.run/maintained-notice-source-20261013-01/`。
日志：`dawn-first-pass-01.log`、`dawn-corrected-pass-01.log`、
`dawn-independent-pass-02.log`、`dawn-tool-suite-01.log`。

本批没有新 compiled site、writable runtime copy 或重复包；删除0、逻辑0、
实际回收0。其它批 live/不确定owner产物不动。原源码、raw/bare
缓存、成功/失败证据、当前 App144、依赖、immutable seeds 和用户 home 保留。
未提交、push、PR 或发行，产品仍 Node20/PW1.48/Chromium130。

## 继续执行

按 [R1–R4](2026-10-13-computer-use-maintained-runtime-next.md) 完成剩余来源和
通知闭包，包括 Dawn 未绑定目录、Skia/copied notices 与 ANGLE 源提供义务；
然后选定完整四平台 NOTICE/SBOM/源码提供/媒体许可和安全更新策略，推进
最终维护 pins、四锁与 production prepare/check/repair/rollback/bundle。
四架构原生、四 surface/UI UX、安装签名更新卸载、真人原 ACP/MCP/Grok/model/
picker、唯一最终候选12h和 D00–D31仍全部开放，不因工具通过缩小交付范围。

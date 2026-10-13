# CU 维护浏览器：SwiftShader 复制源码通知

2026-10-13；`feat/computer-use-implementation`；active / partial。
承接[复制 API 通知与省略记录](2026-10-13-computer-use-notice-supplements-checkpoint.md)。
用户确认的 App macOS 11+ / CU 受管浏览器 macOS 14+ 边界保持。

## 本批结果

固定 SwiftShader `0f87bb2d3742c2992fd1a7dcb2f051eebf79299c` 的全部 regular
GN/GNI 已采集并独立与 sealed tree 对齐：**28 paths / 262996 bytes**。
另五份真实 C++/LLVM 头文件 **374218 bytes**；两个 profile 均与四原 checkout
manifest 匹配，复验原 Git raw batches、blob、完整正文与 receipt。
原三个成功 source collectors 未重启、覆盖或重新下载。

实际 Reactor GN 指向 `third_party/llvm-10.0`；源码默认在支持的架构使用
Subzero，Mac ARM64 需要 LLVM。ASTC 默认开启，Device 有真实条件依赖，
Marl 在 Device/Pipeline/System/Vulkan/WSI 有引用。上述是原源码字面规则，
不是有效 GN 图或二进制测量；没有根据源码缺项排除 Boost/libbacktrace。

绑定 **11 份原源码通知**：ASTC、Marl、llvm-subzero、Subzero/bloat、两 LLVM
Support 声明、两 COPYRIGHT.regex 与 LLVM16 BLAKE3；完整原字节保留。
LLVM10 Support 的 `Copyright (C) 2004 eXtensible Systems, Inc.` 与旧 Illinois
声明绑定固定 SPDX 的完整 NCSA 正文，未以根 Apache 覆盖。
两份抽样 `StringRef.h` 均声明 `Apache-2.0 WITH LLVM-exception`，绑定完整
Apache 和 LLVM exception。LLVM10 不能整体归为 NCSA，LLVM16 不能据树存在
声称是活跃 Reactor。SPDX 标准正文不是浏览器组件。

新 NCSA/LLVM exception 正文共 **2619 bytes**，固定 v3.28.0 tag/commit/tree
原证据及 raw JSON 保持；新 collector 离线复验通过。
三个 source receipt 分别为：

- GN：`2596577a1b9de3fd996fd4cf4657ad5829979a0f9011434a577e29119d427db8`
- C++ samples：`182e3ed534dc20cb3ce76a100450b913988f55fd8cadac29e002473d874c8717`
- LLVM texts：`5f47933a24b7f439c6967851a954cb01d1ca611579bcf6e5b2c1d89785fb5f15`

## 剩余边界

八个 SwiftShader 原目录错误全部保留：六个有 partial source notices 绑定；
Boost/libbacktrace 的 checkout/licensing/shipping 义务仍开放。libbacktrace
真实树包含 `160000` Gitlink，它不是普通源文件，也不是“不随包”的证明。
完整 copied source attribution（含 LLVM MD5/其他 alternate notices）未齐。
原 **17 yes 错误 / 196 目录问题 / 5 异常值 / 421 raw gaps** 没有删除或重写。

片段 **91194 bytes**，标为 `PARTIAL ... NOT FOR DISTRIBUTION`。
两次独立完整重算 `swiftshader-notices-01` / `02` 的整个 inventory 和片段
逐字节一致，不只比较计数：

- inventory：226462 bytes，SHA `2bf2c81272f462813e9740a28f616c6cecd57f939ea1620a45a3eee9cd3634a8`
- fragment SHA：`61f278a5d3271b1495a78989ff1426dffb1a8a961ed4583b8080465caa44c614`

## 验证与收尾

显式完整工具 suite：**427 passed / 0 failed / 0 skipped**；新 collector 7 项、
新 supplement 8 项及原 integration profile 27 项通过。
上轮旧错误文案断言首败保留，修为 `all selected exact` 后真实复验；没有
跳过或改松源身份、字节、完整例外、失败 raw 保留和 output 不覆盖门禁。
代码质量 final、原 R2 22 源码 SHA、100 文件 LF/EOF/hash、80 Python AST、
93 严格 whitespace 检查通过；文档更新后的最终回读见同批 final 文件。
App+Workbench13573 / ≥1000 源码80 的冻结门禁保持；工具测试不替代产品实机。

证据根：`tools/computer-use-probe/.run/maintained-notice-source-20261013-01/`。
完整测试日志为 `swiftshader-tool-suite-01.log`，格式初验为
`swiftshader-format-hashes-01.json`；最终为 `swiftshader-final-verification-01.json`
和 `quality-swiftshader-final-01.json`，清理为 `swiftshader-cleanup-01.json`。
本批没有 compiled site、runtime 执行副本或重复包：删除0，逻辑删除0，实际回收0。
保留 source/bare/raw/全部失败与成功证据、当前 App144/依赖/seed/WIP/用户 home。
没有停止其它进程，没有 commit/push/PR/release。

## 下一批

优先复验 LLVM MD5/alternate notices、Boost/libbacktrace 的准确源义务，以及
原 ANGLE 四未解决项；随后完成四平台完整 NOTICE/SBOM、源码提供/LGPL-media
和稳定安全更新策略。R1 通过后迁移最终维护 pins/四锁/生产 prepare/check/
repair/rollback/bundle。原[R1–R4](2026-10-13-computer-use-maintained-runtime-next.md)
全部门禁继续：四架构原生、所有 surface/UI UX、安装签名更新卸载、真人原
ACP/MCP/Grok/model/picker、唯一最终候选12h/D00–D31。
产品仍 Node20/PW1.48/Chromium130；未晋升用户包，整体尚未可发行。

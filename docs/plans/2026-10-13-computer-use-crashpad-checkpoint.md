# CU 维护浏览器：Crashpad 的 Chromium 配置与实际 lss 来源

2026-10-13；`feat/computer-use-implementation`；active / partial。
承接[固定生成依赖闭包](2026-10-13-computer-use-node-generated-checkpoint.md)。
App 继续支持 macOS 11+；CU 受管浏览器单独要求 macOS 14+。

## 本批结果

固定 Chromium 根 `.gn` 明确设置 `crashpad_dependencies = "chromium"`。
四个原 archive build 的完整 GN 写入日志都没有覆盖这个参数；
同版本 Crashpad buildconfig、lss/zlib BUILD 和两个 wrapper header 的分支对应关系已核验。
这解释了 Chromium 使用外部依赖，而独立构建的 metadata 指向嵌套目录的区别。
仅为准确源码和原构建参数的对应证据；没有测量远端 effective GN、编译图或二进制组件。

| 依赖 | Chromium 配置的实际引用 | 已核验许可证 | 原始扫描错误 |
| --- | --- | --- | --- |
| lss | `third_party/lss/linux_syscall_support.h` | `third_party/lss/LICENSE`，BSD-3-Clause | 保留嵌套 `lss/linux_syscall_support.h` 缺项 |
| zlib | `//third_party/zlib`、`<zlib.h>` | `third_party/zlib/LICENSE`，Zlib | 保留嵌套 `zlib/LICENSE` 缺项 |

lss 来源准确 pin 为 `29164a80da4d41134950d76d55199ea33fbb9613`，
repository 为 `https://chromium.googlesource.com/linux-syscall-support.git`。
主 DEPS、Gitlink 和原 Linux checkout manifest 三者匹配。
Windows / Mac 两架构的 manifest 没有该 exact entry；不据此宣称未随包或免除通知义务。

## 可复验的来源与对应关系

`collect-chromium-crashpad-lss-sources.py` 只读固定 public Gitiles URL，
拒绝跳转，单次20秒、raw2MiB/source1MiB/选定总量8MiB；源码保持 inert data。
主树四件 config/header 加 lss 三件 license/README/header，共 **267958 bytes**。
另一个 `defaults` profile 采根 `.gn` **25525 bytes**。
Git blob 校验和 lss 根 Git tree 完整重建 SHA1 校验均通过。
根树八项的身份完整，`tests` 子树未采集且明确记录，不能称完整递归 checkout。
不执行 GN、gclient、upstream code，不创建 source aliases 或跟随目录链接。

两份来源 receipt seals：

- `crashpad-lss-source-01/crashpad-lss-inventory.json`：`39f335d0b005a88498a6127f4045a58843c2a8de10062a19db638939239fe830`。
- `crashpad-default-source-01/crashpad-default-inventory.json`：`9f347a29e64ebe208cf6b17c59062df1fdaf33959a18e259de67e8f9ea0e7252`。

`inspect-chromium-crashpad-coverage.py` 重新验证 seals/raw/source/tree/catalog/manifests，
再验证上一批准确 Crashpad GNI/BUILD 输入和四份原 GN 日志。
真实 source path、Git mode/blob ID 保留；不把主树 zlib 或 Linux lss 伪装成 standalone 嵌套文件。
root default、分支与实际 licensed paths 在独立 mapping 报告中列出，原 metadata 错误不排除。

两次独立 coverage `01` / `02` 内容和 SHA256 完全一致：
`4f0687a93a5f4ce3eb12bf8c078659c49e735709be105f7876ce0e3f52d5d187`。
结果为 **7518 selected paths / 1250 raw references / 828 resolved / 422 raw gaps**；
**17 yes metadata errors / 190 directory errors / 5 nonstandard Shipped values** 全部保留。
本批增加11件可核验 source paths 和两条配置映射，原始 raw gaps 没有减少。
这不是完整 NOTICE/SBOM 或产品验收通过，`licenseCoverage:not_proven` / `promoted:false`。

## 验证与清理

新增 collector13 / inspector10 项，加既有296项，显式 runner 实际
**319 passed / 0 failed / 0 skipped**。覆盖 tree删项/篡改/路径/重复、固定pins与四平台对应、
raw/source额度和非UTF8、失败保留、真实路径不覆盖、不创建嵌套alias、GN override和缺平台拒绝。
语法、严格格式、LF/EOF/hash、原 R2 22 hashes 与四份历史许可 seals、质量门禁收尾记录见
`crashpad-final-input-hashes-01.json` / `quality-crashpad-final-01.json`。
实际59份Python AST、66份严格格式、73份LF/EOF/hash通过，App+Workbench13573/≥1000源码80不增长。
辅助verifier首败将combined指标再次相加；原错误日志保留，按机器指标定义和shell+workbench双验修正，
没有调整产品门禁或删掉断言。
审查工具测试不替代产品、四架构原生、真实模型或发行包验收。

本批没有生成 compiled site、runtime执行副本或重复大包；来源数据、全部证据和工具依赖保留。
清理记录 `crashpad-cleanup-01.json`；live preview/不确定owner cache暂缓，空间变化不作回收量。
产品 pins、四锁、seed、App144/home 没有晋升；没有提交、推送、PR或发布。

## 下一步

1. 核验 ANGLE / OpenScreen 在 Chromium 内的准确配置、引用路径与实际 checkout，
   分开报告 standalone metadata 和真实公共依赖；其余生成输入及字段异常继续核对。
2. 完成四平台 NOTICE/SBOM、源提供、LGPL/媒体义务与稳定安全更新策略后，才能选择最终 browser。
3. 继续原[R1–R4](2026-10-13-computer-use-maintained-runtime-next.md)所有门禁：
   compiled pins/四锁/production prepare/check/repair/rollback/bundle、四架构原生与系统版本边界、
   安装/签名/更新/卸载、四surface/UI UX、真人原ACP/MCP/Grok/model/picker、最终candidate12h与D00–D31。

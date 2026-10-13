# CU 维护浏览器：固定 registry XML 与原许可

2026-10-13；`feat/computer-use-implementation`；整体 active / partial。
延续[ANGLE 生成记录与四平台回归](2026-10-13-computer-use-angle-generation-checkpoint.md)。
用户已允许 App macOS 11+ / CU 受管浏览器14+，两种 Mac 架构原生仍未验。

## 本批结果

复用原有有界 Git collector，按已封存的 ANGLE DEPS/Gitlink 固定 revision，
补齐三个 registry 的五份 XML、完整根许可及声明。新增显式 extra paths 在
创建目录或执行 Git 前验证，默认采集行为保持；上游源码和 generator 未执行。

| 来源 | 固定 revision | 完整树条目 | 选定文件 | 唯一原文字节 |
| --- | --- | ---: | ---: | ---: |
| EGL-Registry | 7dea2ed79187cd13f76183c4b9100159b9e3e071 | 385 | 3 | 193256 |
| OpenCL-Docs | 5b4ca15f0e5a5be87b56b99f652f728e05cab587 | 417 | 5 | 504533 |
| OpenGL-Registry | 9cb90ca4902d588bef3c830fbb1da484893bd5fb | 1354 | 3 | 2992371 |

总2156树条目、11份选定原文、3690160 bytes。每份原文核对 Git blob 与
SHA256；五份 XML 全部匹配原生成记录中的 MD5，MD5仅作生成记账。

五个原版权块原样保留，并绑定已验证的 SPDX v3.28.0 完整 Apache 正文。
OpenCL XML 的生成代码署名例外及 GPL/LGPL 特定冲突豁免完整保留，
不据此省略署名。仓库根 LICENSE 的 mixed licenses 不覆盖 XML 自身声明。

原38个复制 API 头文件中，6个与固定 OpenGL registry 树的 blob 完全相同：
`gl2.h`、`gl2platform.h`、`gl3.h`、`gl31.h`、`gl32.h`、`gl3platform.h`。
其余32个保持未匹配；同名文件不作 provenance，相同 blob 也不证明历史
复制操作或 binary 使用。三 registry 的原 metadata、12个本地false和原
manifest 缺项继续保留，不推导 binary 排除，不改写原错误。

## 统一候选与验证

新增第七份 `registry-xml` 补充。原五份和六份补充入口分别冻结，旧统一
候选01–04不变；新05/06是独立复算的同一七份补充候选。
当前正文5931433 bytes，SHA256
`6da0c28a06618460347ab568b9f63b4873cc05580d969936720fa13065c23a2d`。
502个默认候选与原18项 metadata 分类11/3/4保持；05原进程exit2，
`releaseReady:false`，不能作为发行 NOTICE 或 binary SBOM。
06产生同一完整输出，PowerShell重定向包装进程返回1；未另行记录其内部
native退出码，不把包装码当作原Python的exit2证据。

完整显式来源工具 suite：**554 passed / 0 failed / 0 skipped**。
覆盖 extra-path 边界、准确 pin/完整树/原文身份、五 XML 版权及例外、
生成记录匹配、复制头比较、七份补充接线及旧候选兼容。

证据根：`tools/computer-use-probe/.run/maintained-notice-source-20261013-01/`。
原文位于 `angle-registry-license-source-01/`；根 receipt SHA256
`eba7ef976e55ff25faa19e415a00c55889925ab19cc56468dd2bcbc3a5a9bacd`。
独立 `angle-registry-coverage-01/02/` 的整个 inventory 为250123 bytes，SHA256
`ee5b2586a193a8ba794677b3e44b3d896dc30f0e250cf97f1e2d8e9632855207`；
13701-byte 通知片段 SHA256
`66d71c7871b2637aa94a9b8f1a4d9cccb1c08305c7cee4466c622ee6e46e13b9`。

首遍检查曾将 registry 的四项 correspondence 错用于原 headers 的十二项，
在输出目录创建前拒绝。现独立读取 headers 原完整 context 并核对相同主树/
checkout 身份；首败 `registry-xml-first-pass-01.log` 保留，重试和独立02成功。
完整测试日志 `registry-xml-tool-suite-01.log`；最终回读与清理记录为
`registry-xml-final-verification-01/02.json`、`registry-xml-cleanup-01.json`。
129 LF/EOF/hash、100 Python AST、122严格格式及原R2 22源码身份核对；
App+Workbench13573 / ≥1000源码80不增长，旧平台/条件/生成报告及补充保持。

本批无新增 compiled site/runtime 副本/包，删除0、实际回收0。原来源 cache、
依赖和全部失败/验证证据保留，live或不确定owner的其它产物不动。
产品仍Node20/PW1.48/Chromium130；App144/home/登录/history/未发送草稿保持。
无新模型任务、代批授权、commit/push/PR/发行。

## 下一步

不再扩大旧平台/条件检查器。继续 SwiftShader 剩余源义务、复制/生成源的
未完成部分、媒体/source-offer及安全更新策略，收齐完整四平台NOTICE/SBOM，
再推进最终维护pack/pins/四锁与production prepare/check/repair/rollback/bundle。
原R3–R4四架构原生、四surface/UI UX、安装签名更新卸载、真人模型/picker/
文件/Pause/Continue/Stop/Quit、同一最终候选12h及D00–D31仍开放。

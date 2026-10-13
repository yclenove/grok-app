# CU 维护浏览器：Khronos 完整许可正文与通知片段

2026-10-13；`feat/computer-use-implementation`；active / partial。
承接[真实 C++ 来源与 Khronos 头文件通知](2026-10-13-computer-use-consumer-checkpoint.md)。
用户确认的兼容边界保持：App macOS 11+，CU 受管浏览器 macOS 14+。

## 实际结果

七份实际 ANGLE 头文件的 SPDX 简短声明已绑定到对应完整许可正文，
另一份 `KHR/khrplatform.h` 原完整 permission/disclaimer 保持原样。
八份源文件的 copyright owner、实际年份和 attribution block 均来自前批已封存的原字节；
没有用标准正文里的版权占位符替换实际版权声明。

全文来自 SPDX `license-list-data` 的准确 `v3.28.0`：
annotated tag `779ef2e5dff6d4af389c53de5e97116ab0bb52e8`，
commit `c4a7237ec8f4654e867546f9f409749300f1bf4c`，
tree `f7502cc75f02f09e94bd67930022c74d408cfbbe`。
SPDX 数据集是规范许可文本来源，不是浏览器依赖；
公开 HTTPS metadata 关联不冒充完整 Git tree 重建或发布者签名认证。

| 正文 | bytes | Git blob SHA-1 | SHA-256 |
| --- | ---: | --- | --- |
| `text/Apache-2.0.txt` | 10280 | `137069b823873b8bcf42979bcf8e9371052d26a2` | `074e6e32c86a4c0ef8b3ed25b721ca23aca83df277cd88106ef7177c354615ff` |
| `text/MIT.txt` | 1078 | `d817195dad53ec992418c28ffca5fbd1cd86502a` | `b05785f9f18e6716bab63424b11454513b9943a222595b70411009202fc592b5` |

Apache 完整 terms 与 appendix、MIT 的标准版权 placeholder 全部保留，共 **11358 bytes**。
五份 Apache、两份 MIT 声明逐文件绑定，八份实际 attribution 原文均写入
`khronos-header-notice.txt`；片段 SHA-256：
`a47b53e73633b3d5781f2a5ccac1fd4b69106e3341b4466f3fae8c8951adc783`。
这里只完成这八文件的片段；完整 Chromium NOTICE/SBOM、全部 copied/generated 源覆盖和发行判断仍未完成。
前批 **7554 paths / 422 raw gaps / 17 yes metadata errors / 196 directory errors / 5 异常字段** 保持。

## 复验与质量

`collect-khronos-notice-texts.py` 限定五个编译时固定请求，20秒超时，
raw256KiB、单正文64KiB、合计64KiB，拒绝重定向及压缩解码响应。
raw 先落盘；失败保留原 raw 和 incomplete receipt，不覆盖已有目录。
offline verify 重验实际 consumer/header 来源、tag/commit/tree 元数据、每份 raw hash、
base64、Git blob、完整文本 size/hash、七条绑定和逐字节片段，而非只信 collector exit0。
封存 inventory SHA-256：
`179b480aff39a2f024ec8e0b0fc1b0abefc77d21f59a8ddec29329b51b6db240`。

新增11项 meaningful tests，显式 runner **380 passed / 0 failed / 0 skipped**。
覆盖错误tag/commit/tree、文件身份/字节/编码/预算、缺正文、错误SPDX、
版权与placeholder保留、raw/text/fragment篡改、伪造完整NOTICE/晋升和失败证据保留。
测试用小型 synthetic 正文验证拒绝边界，另以实际采集正文运行 offline CLI；两者分开记录。
最终 owned 69 Python AST / 79严格格式 / 86 LF-EOF-hash、diff及质量门禁已验；
App+Workbench13573、≥1000源码80不增长，原R2 22源码hash和十份历史许可报告seals不变。
实际收尾记录为 `khronos-final-input-hashes-01.json` / `quality-khronos-final-01.json`。

## 清理与下一步

本批只有inert source/工具/日志，没有compiled site、runtime执行副本或重复包；
删除0 / 逻辑0 / 实际回收0，空间观察单独记录于 `khronos-cleanup-01.json`。
保留当前App144、依赖/immutable seeds、源WIP、用户home及全部成功/失败证据；
未操作用户App、代理、登录或模型任务，没有提交、推送、PR或发布。

下一步是其余copied/generated源、ANGLE CPU真实消费路径与字段问题，
再完成四平台NOTICE/SBOM、源提供/LGPL-media及稳定安全更新策略。
原[R1–R4](2026-10-13-computer-use-maintained-runtime-next.md)完整范围保持active/partial：
维护版最终pins/四锁/production prepare/check/repair/rollback/bundle、四架构原生/系统边界、
安装/签名/更新/卸载、四surface/UI UX、真人ACP/MCP/Grok/model/picker、唯一最终candidate12h/D00–D31。
产品仍为旧Node20/PW1.48/Chromium130；此来源审查不晋升产品运行时或用户包。

# CU 维护浏览器：六个实际递归仓库的许可证输入

2026-10-13；`feat/computer-use-implementation`；active / partial。
承接[实际 checkout / 固定 GCS 对象](2026-10-13-computer-use-checkout-object-checkpoint.md)。
用户确认的边界不变：App macOS 11+，CU 受管浏览器 macOS 14+。
本批只补来源审查工具、公开源数据和证据；没有晋升产品 pins、四锁、seed 或 App144。
不提交、推送、提 PR、发布、操作 GUI、发送模型任务、代批授权或选择用户文件；不改代理。
完整四架构、四 surface、安装 / UI UX / 真人模型 / 最终同候选 12h / D00–D31 范围保留。

## 实际新增

`collect-chromium-checked-recursive-sources.py` 在网络前重新校验原四平台 build / log /
checkout / catalog receipts，要求六个准确路径、URL、revision 在四份 manifest 中均匹配。
不根据 standalone 默认值猜版本，不将未出现的目录排除为 NOT_SHIPPED。
使用既有有界 Git 采集器，只保存 inert source-data，不执行 DEPS、hooks 或上游函数。
输出必须是任务 evidence root 下的新目录；迟败保留已完成项和 partial root receipt。

原 `checked-recursive-license-01` 收齐 6 个固定源、36 个选定文件、86751 source bytes。
root receipt SHA256：`ad3f368823ba0f4432cfe3e0f0311bc47bc876ef92463501e295ba7b3420836a`。
每个组件另有独立 seal，收集完整仅指这六项，不等于全部递归来源或二进制覆盖完整。

| 来源 | 固定 revision | 选定文件 |
| --- | --- | ---: |
| ANGLE glmark2 | 6edcf02205fd1e8979dc3f3964257a81959b80c8 | 5 |
| ANGLE rapidjson | 24b5e7a8b27f42fa16b96fc70aade9106cf7102f | 3 |
| ANGLE VK-GL-CTS | 9335a4e4e455ed600cb7fa486999e3ed6c2a0dbf | 4 |
| OpenScreen buildtools | 4277578aa9c45906e51ad33cac1a5a7ad5288010 | 19 |
| OpenScreen tinycbor | 213155c8729b07dea836c5ac6e807a089486bf7b | 1 |
| OpenScreen simple-dns | 5c7ec98798d7c9f7d76df7aa3bbe754913ff5159 | 4 |

`inspect-chromium-checked-recursive-licenses.py` 重新核验六份 component receipt、完整树和
选定原文 blob，按原 checkout 路径合并；不覆盖原 57-source receipts。
新 `checked-recursive-license-audit-02.json` 实际 exit0，独立重算与原 01 bytes 完全一致。
两份 SHA256 均为 `2a8297315e1e1d81bb1caa524389f0e40751588052b7018358c2a943d1fab77f`。

| 原始 / 默认 policy 指标（不是 shipping 数量） | 原 57 源 | 新 63 源 |
| --- | ---: | ---: |
| 选定路径 | 7237 | 7273 |
| 选定路径 bytes | 14425388 | 14526827 |
| raw license references | 1249 | 1250 |
| 未解析 raw references | 440 | 439 |
| yes metadata 带错 | 19 | 19 |
| 目录 metadata 错误 | 187 | 190 |
| 非标准 Shipped 值 | 5 | 5 |

关闭的是 ANGLE rapidjson 的准确 `src/license.txt` 引用。
新增三个目录错误来自 OpenScreen 自己 buildtools 下的 libc++、libc++abi、libunwind。
这些错误如实保留，不把主树同名 LLVM 来源移植为它们的身份或许可证证明。
`allRecursiveSourcesCollected:false`、`binaryComponentCoverage:not_proven` 和
`licenseCoverage:not_proven` 保持；完整 NOTICE / R1 仍红。

## 验证与清理

显式 runner 原日志 `license-tests-recursive-final-01.log` 实际 exit0：
**206 passed / 0 failed / 0 skipped**，原 192 + 采集 8 + 重验 6。
覆盖四平台齐全、精确路径 / URL / revision、重复 / 额外匹配、receipt seal /
顺序 / 篡改、输出逃逸 / 已存在、迟败保留，以及不排除原缺项 / 不晋升的合同。
这是工具回归，不重复计作产品、原生平台或完整 App 验收。

本批 owned Python 语法、LF / EOF / 严格尾空白、hash / diff 与实际 quality final gate
收尾记录在 `recursive-final-input-hashes-01.json` 和 `quality-recursive-final-01.json`。
原 R2 的 22 个 source hashes、combined05 / policy05 / classification03 继续独立核验。
App + Workbench 13573 行、≥1000 行源码 80 件不增长；FINAL: PASS 仅表示本批质量。

没有新编译 site、运行时副本或重复包可退休；删除 0、logical removed 0、实际回收 0。
保留源 / bare / SDK / 原失败与成功证据 / 当前 build / seed / 用户 home。
live preview 和未知 owner cache 暂缓，不停止它们来清理；空间观察单列，不冒充回收。
`recursive-cleanup-01.json` 记录边界；重新生成使用新 evidence 后缀，不能覆盖原记录。

## 接下来

1. 补准确 config / recipe 与执行 CIPD depot_tools 身份；`.DEPS.git` 到 `DEPS` 的读取机制
   必须按实际 helper 证明，不能拿 checkout 内 depot_tools 或行号相近代替。
2. 按真实平台补剩余运行源和生成输入，形成可审阅的完整 NOTICE / SBOM；处理 source 提供 /
   LGPL-media 与稳定安全更新策略。原有 19 yes 错误、190 目录错误和 5 异常值不得静默忽略。
3. 按[维护版 R1–R4](2026-10-13-computer-use-maintained-runtime-next.md)完成最终来源选择、
   四锁 / production prepare-check / repair-rollback-bundle，再做四原生架构、安装 / 签名 /
   更新 / 卸载、四 surface / UI UX / 真人原 ACP-MCP-model-picker 与最终同候选长稳门禁。

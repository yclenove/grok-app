# CU 维护浏览器：四平台来源对齐与生成输入机制

2026-10-13；`feat/computer-use-implementation`；active / partial。
承接[实际 recipe/helper/config](2026-10-13-computer-use-recipe-provenance-checkpoint.md)。
用户确认的 App macOS 11+ / CU 受管浏览器 macOS 14+ 不变。
本批只增加来源审查工具、测试与文档；没有改产品 pins、四锁、seed、App144 或用户 home。
没有提交、推送、提 PR、发布、操作 GUI、发模型任务、代批授权、选择用户文件或修改代理。

## 1. 四平台来源和 policy 的联合核对

新 `inspect-chromium-platform-coverage.py` 从原封存字节重新核验主树、57 个固定公共来源、
6 个实际递归来源、四份实际 bot_update manifest、原 build/bootstrap receipt 和两个递归 DEPS catalog。
不信任上一份分类报告，不执行 upstream Python/GN/DEPS，也不借同名目录替换仓库或版本。
全部 64 个来源包含 Chromium 主树，以下统计是来源数量，不是随包组件数量。

| 实际 archive target | manifest Git 仓库 | 已采来源准确匹配 | 已采来源 exact entry 未出现 | 实际 manifest 中尚未采来源 |
| --- | ---: | ---: | ---: | ---: |
| windows-x64 | 157 | 59 | 5 | 98 |
| linux-x64 | 169 | 62 | 2 | 107 |
| mac-intel | 152 | 56 | 8 | 96 |
| mac-arm64 | 152 | 56 | 8 | 96 |

四平台 repository/revision 冲突均为 0；`.git` 后缀不做别名化。
`platform-source-coverage-02.json` 与独立重读的 `03` 逐字节相同；原 `01` 是增加分类字段前的历史输出，保留而不覆盖。
02/03 SHA256：`abd9e3c0cb332e2126da16cc27871efc216c39b30c4c0261997d573a08081115`。

每项同时展示三种不同证据：已采树的最长路径归属、实际 manifest 的最近根、固定 DEPS 的最近声明；
再列原始 path resolution、Gitlink、准确身份匹配和未求值条件。最近父根出现不证明缺失的子路径存在，
更不证明有效依赖映射，保留 `effectiveSourceMappingProven:false`。
所有原始缺口和 policy 错误在每个平台都保留，没有按 manifest 缺项删除条目。

63 个外部/递归来源仍是 7273 个选定路径 / 1250 个 raw 引用 / 439 个 unresolved raw 引用；
19 个 yes metadata 错误、190 个目录错误、5 个非标准 Shipped 值均不减少、不改大小写。
四平台本批 raw gap 分类恰好相同：18 实际 manifest Git 源未采、29 固定 Git 声明 exact entry 未出现、
20 non-Git 输入未采、3 原文 `Not Included`、1 非本地或不支持的引用、368 索引缺路径/实际映射未证。
`Not Included` 不是 `NOT_SHIPPED`；其余 absent 也不构成许可证豁免。
这是分诊，不是完整 NOTICE/SBOM、完整 checkout、二进制来源认证或发行许可通过。

## 2. Crashpad / node 的准确源机制

新 `collect-chromium-generated-input-sources.py` 只读八个白名单 Gitiles URL，固定在
`f52796fc20c6e7d618374bb8bf096e7b2155e14c`。每份 decoded bytes 同时匹配手工独立列出的 Git blob ID
与原完整主树；20 秒单请求 / 禁跳转 / 4 MiB raw / 2 MiB decoded / 8 MiB 总 decoded 上限。
原 raw 在验证前保留；后失败写 partial receipt，不安装、解压生成依赖、运行脚本或物化 upstream source paths。

`generated-input-source-01` 实际收齐 8 件 / 271849 decoded bytes：
Crashpad buildconfig、lss/zlib BUILD.gn、导入 update.py，以及 node BUILD.gn、node_modules.py、package.json 和 package-lock.json。
inventory SHA256：`9295d0afbe743857cdcc892dc4937b8d86b0937d30f1cc7567c4ceabc880cc4d`。

实读结论及边界：

- Crashpad buildconfig 默认 `standalone`，由 `crashpad_dependencies` 区分环境；不能只见目录位于 Chromium 就断言已进入 chromium 分支。
- lss BUILD.gn 的 chromium 分支设置 `CRASHPAD_LSS_SOURCE_EXTERNAL`；zlib 对应 external 分支依赖 `//third_party/zlib`。
  这是源码中的明确分支，不是符号链接、GN 有效值测量或实际二进制依赖证明。
  没有伪造 Crashpad 的 `lss/lss`、`zlib/zlib` 路径，也没有因此关闭其原 metadata 错误。
- node_modules.py 只提供工具路径，不负责拉取依赖。固定主 DEPS catalog 对 `third_party/node/node_modules`
  声明的是 GCS 输入，不是 Git 或 CIPD：bucket `chromium-nodejs`，object
  `605b17899d6503df7f2ce96d5458976b36c5e456`，generation `1791258586019361`，11492257 bytes，
  SHA256 `f7d15a9ba38b018602496cde9b094c34c5876c75723d3a5497682fd2e71fcdf3`。
  条件仍是未求值的 `non_git_source`；没有声称远端 builder 的该对象下载已测量。
- 固定 package-lock 的 Lit 为 **3.3.0**，但已采 README.chromium 写 **3.2.1**；MSAL-browser 两者为 **4.2.1**。
  下一批应按准确 GCS object/hash 校验包中实际版本和许可证，不能抓 README 版本的 npm 文件当完整闭包。
  npm resolved URL/integrity 仅作原锁字段，不运行 npm 或用它们替代 GCS 输入。

一次只读 PowerShell 展示 package-lock 因 JSON 含空字符串 key 失败；改用 `ConvertFrom-Json -AsHashtable`
后准确读取两条记录。没有改原 JSON、丢弃空 key 或把首次展示失败写成来源验证通过。

## 3. 测试、质量和清理

显式 runner 增加 platform 15 / generated-input 7 项，累计 **258 passed / 0 failed / 0 skipped**。
覆盖深层归属、实际未采来源、absent/different、仓库后缀、近似前缀、生成输入/Gitlink/模式、remote、
大小写与原错误保留、路径逃逸/覆盖拒绝、Git blob 校验、非 UTF-8/过额和 raw/partial receipt 迟败保留。
它们只验证本地审查工具，不是产品/native/model/安装验收。

收尾结果见 `platform-final-input-hashes-01.json` / `quality-platform-final-01.json`：
47 Python AST / 52 严格格式 / 59 LF-EOF-hash 及实际质量门禁通过；
App + Workbench 13573 行 / ≥1000 行源码 80 件保持，原 R2 22 hashes 和四份历史许可回执保持。
原 collector/inspector owners 均终态，无新 compiled site、writable runtime 或重复运行包可退休；
删除 0 / logical 0 / 实际回收 0。八件 source-data、来源包/bare/SDK、全部成功/失败证据、seed、当前 App/home 保留。
live preview 和不确定 owner cache 暂缓，磁盘空闲观察不是物理回收量。

## 4. 后续，不缩减交付范围

1. 有界采集并校验上述固定 GCS node_modules 对象及 tar/gzip 格式、路径、条目/总量、准确许可证和版本；
   保留 README/lock/archive 差异，补实际构建使用证据，不执行包管理器或把 source closure 当 binary coverage。
2. 补 Crashpad GN 的准确默认覆盖/有效分支与 lss header 来源；ANGLE/OpenScreen 的 standalone 路径不借主树同名版本。
3. 继续完整四平台 NOTICE/SBOM、源提供/LGPL-media 与稳定安全更新策略；R1 仍红，不晋升产品。
4. 继续[维护版 R1–R4](2026-10-13-computer-use-maintained-runtime-next.md)的最终 pins/四锁、production prepare/check/
   repair/rollback/bundle、四架构原生、签名安装更新卸载、四 surface/UI UX、真人原 ACP/MCP/model/picker、同最终候选 12h 与 D00–D31。
   原全部范围保持 active / partial；当前不是完整可交付状态。

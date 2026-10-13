# CU 维护浏览器：默认通知候选与完整 ANGLE GN 来源

2026-10-13；`feat/computer-use-implementation`；active / partial。
承接[Khronos 完整正文](2026-10-13-computer-use-khronos-notice-checkpoint.md)和
[CPU 候选消费源](2026-10-13-computer-use-cpu-consumer-source-checkpoint.md)。
App macOS 11+ / CU 受管浏览器 macOS 14+ 的已确认边界保持。

## 本批实质结果

准确 Chromium `licenses.py` 的 AST 已核验：默认 shipped-only 只排除精确 `Shipped == NO`，
不是只接受 `yes`。原 `Yes` / `No` 不静默归一化；READMEs 中的错误记录被省略，
SPECIAL_CASES 的错误记录仍返回，错误本身必须另行审阅。
新增 `inspect-chromium-notice-candidates.py` 按这些原规则生成标为
`PARTIAL SOURCE NOTICE PREVIEW - NOT FOR DISTRIBUTION` 的源码通知预览。
`load_consumer()` 提取后，完整原 consumer report 与 sealed 原件逐字段重现；不只比较统计。
这不是执行上游 generator、完整 checkout、实际 GN 图或二进制 SBOM。

实际 **495 候选 / 17 无效 README 记录 / 5 非标准 Shipped 值**。
原 WebGL `Shipped: No` 仍进入默认生成器，并缺完整正文；没有把它当精确 `no` 排除。
补采其实际 `714857a28445e8f5d8d6ae1c78498578009534d8`：
16006 tree entries / 20 selected files / 13 unique blobs / 61176 source bytes，
准确 DEPS/Gitlink 和四 actual checkout manifests 全匹配。
正文来自实际 `third_party/webgl/src/LICENSE.txt`，不是同名邻接来源或 SPDX 短声明。

| 证据 | SHA-256 |
| --- | --- |
| WebGL root receipt | `25bef4416458ae27a69f4a9b694c70f29b7f3f5180359df4de7dbe4fbb3d5382` |
| WebGL component receipt | `a46f35e697bc3941d2b36833295659a2276cd9c9c72da00c6e5da59b3f5643bc` |
| 独立生成 notice inventory `02` / `03` | `1eecc988cd5ab4f9cfc983433b37fce55bb2ad8607e3e41d250f4e6c35c6d682` |
| 独立生成 partial text `02` / `03` | `8a8cc20622ac69588c0b59c5e7f1d7a85fdb00c8315e25f44e0d4836656949a3` |

候选完整正文缺口 **1 → 0**；原第一份预览和原字段/缺口保留。
最终预览 **5453606 bytes**，每条正文都绑定真实路径/size/hash，原版权与正文逐字节保留。
新合并 **7574 paths / 1251 raw refs / 830 resolved / 421 raw gaps**；
**17 yes metadata errors / 196 directory errors / 5 异常值** 保持。
有效候选正文齐全不代表省略记录、copied/generated 源或全部随包组件齐全。

## 完整 ANGLE GN 来源与 CPU 观察

从 sealed ANGLE tree 编译出全部 **162 regular `.gn` / `.gni` paths**，包含根 `.gn`。
同 revision `9649fea17b77e672ae19687bf0222109b6270e0d` / root tree
`c584938e2146426bdfb86c736797517c81fecc26`、四原 manifests与每个 Git object 已验。
合计 **1018314 source bytes / 161 unique blobs**，一个 blob 被两个路径共用。

首轮 Gitiles collector 对共享 blob 重名错误；修复为只复用逐字节相同的原数据，
不同字节/超预算不能覆盖，raw与partial receipt保留。
第二轮 HTTP429已确认原session终态，不循环重试或改代理。
改用原 task-owned、同 commit 的 bare Git cache；三批显式固定对象 fetch、
三批原 `cat-file` stdout和162份路径关联封存。
Git transport明确区分原Git blob与Gitiles base64，没有把未请求的Gitiles URL冒充网络响应。
只补 cache objects，不 checkout、执行 hook/GN/source、改产品 runtime或创建授权。
Git receipt SHA：`c2dc8248fa6ac39f83a342ebd14f77fb2ecdc2bf1c5e31f46d47af2e9591d785`。
独立offline verifier重验原batch headers/objects/bytes/order、每份blob/size/hash与所有source路径。

扫描全部162份GN文本，七处CPU字面引用都在其自己的 `third_party/cpu_features/BUILD.gn`，
其它GN文本没有直接引用；独立采集的 `src/tests/BUILD.gn` 与全量版本逐字节一致。
原DEPS此项的独立Android条件保持。这不是所有C++消费者、外部GN引用或有效二进制图的证明，
没有据此作shipping排除或删除原错误。
首验将旧仅header模式的六处数量用于更宽模式，漏计 `.c`；准确七行重验通过，原失败已单独保存。

## 质量、收尾与下一步

本批最终 **399 passed / 0 failed / 0 skipped**；覆盖原规则/大小写/错误省略、
完整正文/真实路径/篡改/边界、共享blob、四路读请求收束、显式Git transport及原batch回读。
72 Python AST / 84严格格式 / 91 LF-EOF-hash、diff和最终质量门禁通过；
原R2 22源码hash、十份历史许可seals和前批SPDX正文/fragment保持。
App+Workbench13573 / ≥1000源码80不增长。
收尾为 `notice-candidate-final-input-hashes-01.json` / `quality-notice-candidate-final-01.json`；
源/GN复验为 `notice-candidate-source-coverage-01.json`。

本批没有compiled site/runtime执行副本/重复包，删除0 / 逻辑0 / 实际回收0；
`notice-candidate-cleanup-01.json` 单独保留空间观察和再生说明。
source/bare/全部失败和成功证据、current App144/依赖/immutable seeds/WIP/home保持；
未启动或停用户App、改代理、发模型任务、自动批准、提交/推送/PR/发布。

下一步审阅17省略记录和196目录问题的真实来源/消费关系，以及其它copied/generated文件；
结合完整四平台NOTICE/SBOM、源提供/LGPL-media和稳定安全更新策略完成R1。
原[R1–R4](2026-10-13-computer-use-maintained-runtime-next.md)全部active/partial：
最终维护pins/四锁/production prepare/check/repair/rollback/bundle、四架构原生/系统边界、
安装/签名/更新/卸载、四surface/UI UX、真人ACP/MCP/Grok/model/picker、最终唯一candidate12h/D00–D31。
产品仍为旧Node20/PW1.48/Chromium130；通知预览不用于发行，未晋升用户包。

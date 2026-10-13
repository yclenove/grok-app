# CU 维护浏览器：公共浏览器来源与 ANGLE 递归目录

2026-10-13；`feat/computer-use-implementation`；active / partial。
承接[LLVM/Windows支持源](2026-10-13-computer-use-support-license-checkpoint.md)，
本批仅继续 R1 的固定源码输入与审查工具，不复记旧 Core/native/UI 验收。
App macOS 11+ / CU 受管浏览器 macOS 14+ 按用户确认保留，Mac 原生验收仍开放。
产品版本、四锁、seed、App144 和用户 home 未晋升；无提交/推送/PR/发布/GUI/
模型任务/自动授权/用户文件选择，也不使用此前取消任务中的凭据。

## 10 个公共浏览器来源

新增编译内置 `browser` 组：anonymous-tokens、DevTools frontend、federated-compute、
ink、libprotobuf-mutator、oak、OpenH264、OpenScreen、OpenXR、sframe。
准确 URL/revision/checkout path 在 `BROWSER_PINS`；全部对齐原固定 parent catalog
及 Gitlink，不以 latest、另一版本或另一平台来源代替。
OpenXR 的准确 `checkout_openxr` 只保留为数据，不执行条件或证明实际 binary 使用。

原 collector session7706 终态 **exit0**，新 `external-license-06`：
10 components / **876 paths / 474 仓库内去重 blobs / 965513 bytes**。
root receipt SHA256：
`332ff906798fe4d190d46815c4a2bbe0ea87d168817214c967ce3b36657e42c7`。
10 件完整 component receipt 分别重新实读 SHA256，再编译为独立 seals；不覆盖旧证据。
源采集留下 **13 个未解析引用**（DevTools 2 / OpenScreen 11）。
`collectionComplete` 仅指所选数据采集完毕，不表示完整 NOTICE 或许可义务已完成。

loader 增加显式 boolean `include_browser`，closure/policy CLI 的 `--include-browser`
与分类 helper 接线；输出含 browser receipt seal。
同时固定每代 generation/root SHA/pins 对象/seals 对象之间的一一映射：
跨组、复制 dict 或任意 root seal 在读取 receipt 前拒绝。
原条件校验扩展到 browser，丢失、放宽、改 OS 或已 evaluate 均拒绝。
所有上游 Python、DEPS、hooks 仅作数据，不 import、checkout 或执行。

## 联合复验：新增完整树也暴露真实缺口

全新 `combined-license-inputs-05.json` 实际 **exit0**：
57 个 parent Git 来源 / **7237 paths / 14425388 bytes by path**，
**1249 raw refs / 809 resolved / 440 unresolved**；原 main missing 已解析 **40 → 52**。
仍有216个 parent Git /157个 non-Git 声明及递归/生成/平台/build/binary覆盖开放。
来源声明或 raw ref 不是 shipping binary 组件数量。

`license-policy-05.json` 实际 **exit0**：950 candidates /808 entries /
**187 directory errors /505 yes /298 no /5 非标准值 /19 yes 带错**。
关闭前批12个 yes metadata 许可错误，OpenScreen 完整树又暴露6个递归 yes 错误，
净变化 **25 → 19**，不是仅关闭6个原错误。
directory errors **165 → 187** 全部保留，不剪树、忽略错误或修改上游元数据刷绿。
新增 OpenScreen pybind11 的 `Yes`，其它4个异常值及 ANGLE 三个 `Not Included`
继续原样保留；它们不是 NOT_SHIPPED，也不构成通知已满足。

`license-gap-classification-03.json` 实际 **exit0**；440 raw gaps 全部保留：

| 类别 | raw refs |
| --- | ---: |
| 当前已采树未索引该路径 | 367 |
| 未采 parent Git 来源 | 21 |
| 未采递归 Gitlink | 29 |
| 声明的 non-Git 输入 | 19 |
| 非标准 `Not Included` 原值 | 3 |
| 远程引用，未跟随 | 1 |

复验报告 SHA256：

- combined05：`ab63ad9a26027b349b36bdd72256035c9fb1a5acc0b7f79aad32d1702a8d59cf`
- policy05：`9d1425d5349732f017608fc3863f2658d7bb75a20fbac3660aa7ef806a46437b`
- classification03：`d9fc1249a233ebe8d81ae3a2138f41205d6bd888f027a1ee12e27f30f661452d`

## ANGLE 递归声明：准确版本，不猜继承

新静态 `inspect-chromium-recursive-deps.py` 在复验原 main/core sealed bytes 后，
解析准确 ANGLE DEPS Git blob `58285b6d243a5c15ee0a1a690962c38f43d4b3e8`。
纯 DEPS parser 支持显式 repository-relative 布局与独立 source commit；父树默认报告不变。
原始 Gitlink 与路径边界、条件、non-Git 全部保留，不执行任何上游语句。

全新 `recursive-deps-angle-01.json` 实际 **exit0**：
**80 Git /441 non-Git /0 unresolved /0 mismatch /0 uncatalogued Gitlinks**。
SHA256 `142b5a840a4a52b96bfa4a8c8513ec0bb243edad0c589ae63070d3aad1cde414`。
parent `recursedeps` 明确含 ANGLE；parent literal `build_with_chromium=True`，
ANGLE standalone literal 为 False。`inheritanceVerified`、`builderCustomVarsKnown`、
`conditionsEvaluated`、`recursiveSourcesFetched`、`shippingDecisionsMade` 全部为 False。
不能据 parent 默认值直接排除所有 standalone 输入，也不能将不同的递归 LLVM/Vulkan
版本覆盖 Chromium 根 pin；实际 gclient 继承、builder custom vars 与 build inputs 仍须证明。

## 本批验证、清理与下一步

显式本地 runner **113 passed /0 failed /0 skipped**：
tree13 /DEPS11 /external31 /closure15 /policy20 /classifier15 /recursive8。
原日志 `license-tests-browser-01.log` 保留；默认 unittest 的0tests绝不作验收。
覆盖 browser/OpenXR 准确 pin、独立 seal、generation/组身份、显式 boolean、
递归布局/defaults/Gitlink/越界/动态语句不执行与原所有工具边界。
本批最终15份工具语法/零 trailing whitespace、22份 owned LF/EOF/hash、diff 和质量证据保留；
首轮统一 trailing-whitespace 检查在 BUILD 的既有 Markdown 两空格换行处拒绝，
并非新改动引入；不全文件重写已有文档。工具/新检查点仍要求零尾空白，
已有文档保留 Markdown 换行语义并以 diff 检查新改动，原失败如实记录。
FINAL 仅指本批质量，不是 CU 全面交付。
R2 原22份源码 SHA256 全部复核不变，App+Workbench13573/≥1000源码80未增长。
本批最终 quality JSON 与22项 source/doc hashes 独立保存，不覆盖首轮记录。
仅只读核对用户 preview 原 PID59356/准确 app144 路径与 creation UTC
`2026-10-12T15:43:07.9761177Z` 仍匹配；不称作本批 GUI 或最新源码验收。

无新可退休 build、执行 browser 副本或重复包，删除0/实际回收0。
固定 source/bare/全部报告是后续必要输入，current候选/依赖/seed/home/旧失败证据保留，
不确定 cache 暂缓，不停止无关 process。

下一步优先实际 gclient 递归变量继承与固定 OpenScreen/ANGLE 公共运行输入，
再核验19个 yes metadata errors、异常字段、生成路径、平台 builder/binary 覆盖。
不为 raw count 清零采441个无关 non-Git 或作默认 shipping 决策。
完整同源 NOTICE/源提供/LGPL-media/维护与安全策略仍未齐，四官方157快照仍只有
401-byte sample Credits，R1保持红，不晋升产品。
R2最终格式/布局/版本/五Mac链接/四锁/完整prepare/check/repair/rollback/bundle、
R3四架构原生（两Mac含原生Intel）、R4四surface/UI UX/安装签名更新修复回滚卸载/
真人原模型ACP-MCP/picker/同最终候选12h/D00–D31全部保持，
见[维护版计划](2026-10-13-computer-use-maintained-runtime-next.md)。

# CU 维护浏览器：公共运行源与许可缺口分类

2026-10-13；`feat/computer-use-implementation`；active / partial。
本批推进 R1 审查工具和固定输入，不是 runtime 晋升或完整交付。
App 保持 macOS 11+，仅 CU 受管浏览器要求 macOS 14+；没有提高安装底线。
未 commit/push/PR/release，未改产品 pins、四 source locks、共享 seed 或用户 App144/home。

## 上一批收尾

[R2 有界合同](2026-10-13-computer-use-runtime-contract-checkpoint.md)最终文档/清理脚本已复核
LF/末尾换行；diff 与 FINAL 质量再次实际 exit0，另存 `quality-final.json`，不覆盖 first。
BUILD/质量 ledger 的既有 Markdown hard-break 空白保留，新增行无 trailing whitespace。
22 件上一批源码 SHA256 逐件复核相同；没有重新运行或重复记账旧 Core/CLI/native 结果。
两件旧 incremental 已在原 owners 终态后清理，准确路径及逻辑/可用空间观察仍见原检查点。

## 17 个准确公共来源

collector 新增独立 `runtime` 组，固定 parent DEPS/gitlink 的 repo/revision/条件：
QUICHE、cpu_features、cpuinfo、CRC32C、Expat、Fontconfig、LevelDB、libgav1、LLVM libunwind、
Snappy、Vulkan Loader、Wayland、wayland-protocols、KDE plasma-wayland-protocols、GTK、Wuffs、Zstd。
完整 URL/pins 在 `inspect-chromium-external-licenses.py` 的 `RUNTIME_PINS`，不选 latest/任意远端。
Fontconfig 和四项 Wayland 输入的准确 `checkout_linux` 只作数据保存，不执行/推导条件；
丢条件、换 Windows、放宽表达式或标记已 evaluate 都拒绝。

原 session74566 **exit0**；新 `external-license-04`：17 components、28 selected paths、
27 仓库内去重 blobs 合计、212136 bytes。GTK 两条路径共享一个 blob；path/unique/bytes
统计不可混用。父引用/追加路径和已采引用缺失均为 0；不是 full checkout、递归或 binary 覆盖。
每份 component receipt 加独立编译 SHA256 seal，先核验完整清单，再读 full tree 和不可变原文字节。
无 checkout materialization、上游源码/DEPS/hooks 执行、凭据导出、浏览器启动或产品变更。
root receipt SHA256 `21f92338bd3d6c3622ce1963c002f3af25fd45377d648e44bb6ea398d17ddeea`。
旧 core/media receipts/seals 保留；loader 需显式 boolean group，错误组/未封存/部分输入拒绝。

## 新联合复验与分类

全新 `combined-license-inputs-03.json`：36 个 parent Git 来源、6345 paths、12633685 bytes by path，
1201 raw refs / 742 resolved / **459 unresolved**。原主树缺失已有 28 项解析到核验原文；
仍有 237 parent Git 声明、157 non-Git 声明及递归/生成/真实平台构建输入未覆盖。
这些不是实际发行组件数量，不要求为归零采全部 Android/private/测试源。

`license-policy-03.json`：870 candidates / 761 entries / 154 directory errors，
470 精确 `yes` / 287 `no` / 4 其它原值；yes entries 带错 **56 → 39**，17 许可缺口关闭。
新 GTK 树真实暴露一个目录错误（153 → 154），没有剪树或跳错刷绿。
Kotlin/rapidhash/Windows App SDK 的 `Yes` 与 WebGL 的 `No` 保持原值，未擅自归一化/做 shipping 决策。

新 `inspect-chromium-license-gap-classification.py` 重验原输入/重扫 policy，不信任旧报告。
按最长准确路径边界/Gitlink OID 分类，更具体声明不被祖先 stub 误归类；远程 URL 只留数据，
越界 license path 不伪装成远程引用。459 raw gaps 如下，完整 39 yes errors/154目录错误/4异常值保留：

| 来源类别 | raw refs |
| --- | ---: |
| 当前已采树未索引该路径（含待调查生成输入） | 370 |
| 未采 parent Git 源 | 53 |
| 未采递归 Gitlink | 16 |
| 声明的 non-Git 输入 | 19 |
| 远程引用，未跟随 | 1 |

combined SHA256 `a5d8bd89d047469b5aee49ab854c7d39569eda4d3d3e55478a8dc7c05caeb071`；
policy SHA256 `e9405727bfd15a933d2b44683af500cc07c795640dec71b9522a802196d03fc7`；
classification SHA256 `a0a986ceb4ae33898984e16a5d9be58de2bd9877393ed57c21562002edd70592`。
所有条件、实际 binary 使用、完整 NOTICE 和源提供义务仍未证明。

## 验证与清理

`run-chromium-license-tests.py` 显式执行 tree13/DEPS9/external26/closure12/policy20/classifier14：
**94 passed / 0 failed / 0 skipped**，完整日志 `license-tests-runtime-01.log` 保留。
覆盖准确条件、seals、重复/越界/symlink、祖先/子 ownership、原值不归一化、递归 OID、
non-Git、原文 hash/不可变 payload 和 evidence 不覆盖。
通配 `unittest discover` 因连字符模块名实际 **0 tests**，exit0 也不是验收；
保存 `license-discovery-not-acceptance.log`，新 runner 有逐模块最低计数与禁止 skip。

本批无 disposable build/browser runtime/重复包；source bare caches 是后续输入，不当垃圾删除。
删除 0、实际回收 0；不确定旧 cache 暂缓，不停止无关进程。原 App144/login/home 未触碰，
只做 CIM 核验，不声称 GUI/native 或新用户包验收。最终 LF/diff/语法/质量门禁保留；
FINAL 仅代码质量，不等于整套 CU 交付通过。

## 下一步与完整范围

1. 针对公共 LLVM runtime、ANGLE 递归和 Windows 接口补准确来源，不为 raw 计数归零采无关源。
2. 对 370 未索引路径查生成规则/实际平台 checkout；不能只凭 Android/tests 名称忽略可能发行义务。
3. 继续实际 binary 输入、同源完整 NOTICE/源提供/LGPL-media 与安全更新策略。四官方157快照仍只有
   401-byte sample Credits，正式 generator 未执行，R1 红；collector/classifier exit0 不允许晋升。
4. R2 最终布局/五 links/四锁/prepare/check、R3 两 Mac 原生与四架构、R4 四 surface/UI UX/
   安装/签名/更新/修复/回滚/卸载、真人原模型/ACP/MCP/picker、同最终候选12h/D00–D31
   按[维护版计划](2026-10-13-computer-use-maintained-runtime-next.md)继续，原范围完全保留。

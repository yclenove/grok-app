# CU 维护浏览器：Dawn 剩余配置许可

2026-10-13；`feat/computer-use-implementation`；整体 active / partial。
延续[四项依赖条件](2026-10-13-computer-use-condition-context-checkpoint.md)。
用户确认 App macOS 11+ / CU 受管浏览器14+；两 Mac 原生仍未验。

## 本批来源与实现

为既有有界 collector 增加准确 Googletest 来源和两个 PartitionAlloc 源码
请求。按主 DEPS/Gitlink 验证 Googletest 实际 pin
`4fe3307fb2d9f86d19777c7eb0e4809e9694dde7`，完整251条树、原LICENSE
1475 bytes已采。完整 notice 保留原版权、permission和disclaimer。

Googletest 的原 README 却写 revision
`b2b9072ecbe874f5937054653ef8f2731eb0f010`，同时 `Shipped: no`。
分析器保留四个原字段/行号与实际 DEPS 差异，不改README或用no自动证明
二进制排除；源码身份以实际DEPS与四份原manifest准确匹配为依据。

PartitionAlloc 的固定 `BUILD.gn` 566 bytes 与 `raw_ptr.h` 56704 bytes按
原主树Git对象/原始响应验证，两处原BSD引用绑定同commit的Chromium
根LICENSE。2023/2020原版权头原样保留到补充通知；不会只写BSD标签。
原源请求receipt、四原manifest、完整通知字节与原Dawn输入都重新复验。

新增 `inspect-chromium-dawn-support.py`，将
`dawn_googletest_dir=//third_party/googletest/src` 和
`dawn_partition_alloc_dir=//base/allocator/partition_allocator` 两处原配置
绑定准确原许可。与先前15项合计17个目录有source-level许可绑定。
这不是两个组件所有传递依赖或完整浏览器通知/SBOM的证明，原所有metadata/
目录/字段/raw错误保持；不创建路径alias、不执行GN/DEPS/hooks、不晋升产品。

## 验证与证据

新增4个collector/profile反例、8个补充通知测试与4个合并测试：实际pin与旧README不能
互换、原响应/版权/许可/元数据必须完整、输出不覆盖/越界、预算包含全部
原许可和头部。完整显式来源工具suite **511 passed / 0 failed / 0 skipped**。

证据根：`tools/computer-use-probe/.run/maintained-notice-source-20261013-01/`。
Googletest：`external-license-10/`；PartitionAlloc：`dawn-support-source-01/`。
补充：`dawn-support-coverage-01/02/dawn-support-notices.txt` 与
`dawn-support-inventory.json`，独立两次整个文件逐字节一致。原通知3747 bytes，
SHA256 `dd97497118dd286aabd9ee78ace2255bbdce8230dfe6e93ce22c0a3e94dbcb52`；
report218879 bytes，SHA256
`1ef5ef60896fabc329cc53c85059d97a088e4ff45372ef65ddfde0c93361cfa3`。
当前 assembler 加入第六份补充，保留前五份原字节；新统一产物使用
`notice-assembly-03/04/`，完整通知与inventory逐字节比较，原01/02不覆盖。
合并会清除两项已绑定的source-level目录缺口，但保留Dawn传递依赖开放项、
全部原metadata错误与完整NOTICE/SBOM/发行阻断；CLI仍必须exit2。
实际03/04均exit2，整个统一正文5917690 bytes、SHA256
`8807681f4123f57469a03b989a9b6fdbcb4bca156f05466702ec05c2ff8ff315`；
inventory1044530 bytes、SHA256
`a2f6dca809cb846c045d6882883d2b3f771101fabefaa2cf1494a85fff5a4271`，
独立文件逐字节一致。默认502候选与原18错误分类11/3/4均保持，发行未通过。
最终结果写入 `dawn-support-final-verification-01/02.json`；质量
`quality-dawn-support-final-01/02.json`；收尾 `dawn-support-cleanup-01.json`。
原R2 22源码指纹、原两条件报告/两Dawn/两统一候选及五份封存补充保持。
121 LF/EOF/hash、94 Python AST、114严格格式与质量final通过，
App+Workbench13573 / ≥1000源码80不增长；02为最终文档回读。

日志：`dawn-tests-collect-01.log`、`dawn-support-collect-01.log`、
`dawn-support-tool-suite-01/02.log`、`dawn-support-first-pass-01.log`、
`dawn-support-independent-pass-02.log`、`assembly-support-first-pass-03.log`、
`assembly-support-independent-pass-04.log`。本批未新建运行时/compiled site/包；
来源bare cache和原证据供下批使用，删除0/实际回收0，不停止无关owner。
App144及其home/登录/history/未发送草稿保留，旧Node20/PW1.48/Chromium130
未替换，无提交/push/PR/发行。

## 下一个交付步骤

完成其余copied/generated/registry、媒体/
source-offer与安全更新策略及四平台NOTICE/SBOM。之后直接推进最终维护
pins/四锁、production prepare/check/repair/rollback/bundle。
原R3–R4四架构原生、四surface/UI UX、安装签名更新卸载、真人模型/picker/
文件/Pause/Continue/Stop/Quit、唯一最终候选12h和D00–D31仍保持开放。

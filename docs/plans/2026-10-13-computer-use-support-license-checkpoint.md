# CU 维护浏览器：LLVM 与 Windows 运行接口许可输入

2026-10-13；`feat/computer-use-implementation`；active / partial。
前一批是[17公共运行源/分类](2026-10-13-computer-use-runtime-license-classification-checkpoint.md)，
本批继续 R1，不把已完成94测试或旧Core/native结果重复记作新验收。
App11+/CU受管Mac14+和全部原范围保持，产品 pins/四 locks/seed/用户App144/home未晋升，
无commit/push/PR/release/GUI/模型任务/自动授权/用户文件选择。

## 新输入与封存

新增编译内置 `support` 组11件，均逐件对齐同一固定 parent catalog 和 gitlink：
LLVM compiler-rt、libc++、libc++abi、llvm-libc、Dragonbox、fast_float、FXdiv、
Microsoft Multimedia Redirection、Microsoft WebAuthn、pthreadpool、XNNPACK。
准确 URL/revision 在 `SUPPORT_PINS`，不替换成latest/另一平台版本。
两Microsoft源的准确 `checkout_win` 保留为数据；不执行条件或推导实际binary用了它们。
错误组/未知组件、丢条件/改Linux/放宽表达式/已evaluate，在源或tree读取前拒绝。

原 session36483 **exit0**，`external-license-05`：11 components / 16 paths /
16仓库内去重blob合计 / **116181 bytes**，已采引用及父引用/extra路径0缺失。
每件完整component receipt新增独立SHA256 seal，原core/media/runtime seals保留。
共享`GROUPS`只接受内置dict身份，不接受拷贝dict或任意远端。
root SHA256 `de8ba2d653bfe0aec9e938a35102719949fab3e1cc99bc6386f85de988b4a28f`。
所有上游Python/DEPS/hooks都只作data，不checkout/materialize源码路径，不执行上游程序。

## 联合复验终态

`combined-license-inputs-04.json` 实际exit0：47个parent Git来源、6361 paths、
12749866 bytes by path、1201 raw refs / **762 resolved / 439 unresolved**；
原主树missing已40项解析到verified bytes。剩226 parent Git/157 non-Git声明及递归/
生成步骤/条件/真实binary输入开放；source声明不是shipping组件数量。

`license-policy-04.json`：881 candidates / 761 entries / **165 directory errors**，
470 yes / 287 no / 4非标准值；yes entries带错 **39 → 25**。
11个新源还补齐PDFium的三项跨仓库引用，共关闭14个metadata许可缺口。
完整索引真实暴露11个目录错误（154 → 165），不是许可证收集失败，不剪树/跳错刷绿。
只证明对应许可原文存在且hash核验，不证明binary启用了这些组件或全部发行义务已履行。

## 真实缺口调查，不下载假路径

已实读准确ANGLE三个README原文，EGL-Registry、OpenCL-Docs、OpenGL-Registry均写
`Shipped: yes` 与 `License File: Not Included`。它不是实际文件名，也不等于NOT_SHIPPED。
classifier只单独标注该原值，仍保留原missing reference和yes error，不把它归为不发行/许可已满足。
其它4个`Yes`/`No`值也不归一化。

`license-gap-classification-02.json` 实际exit0，全部439 raw gaps保留：

| 来源类别 | raw refs |
| --- | ---: |
| 当前已采树未索引该路径 | 367 |
| 未采parent Git源 | 33 |
| 未采递归Gitlink | 16 |
| 声明的non-Git输入 | 19 |
| 非标准`Not Included`原值 | 3 |
| 远程引用，未跟随 | 1 |

同时保留25个yes error entries/165目录错误/4异常Shipped。
combined SHA256 `a865a9741154b2d7731dffe16812d3ffecc49549afe137c2426729057d365a23`；
policy SHA256 `3b9ac3d1bf67ebef1201013be215db6b76a090ab57fb633c8f4940d7c0b7351e`；
classification SHA256 `9e0eb21f44e1b641dfa05b95867dd8bd00538a431d5ecb4d38611cea1f2521fb`。

另实查parent DEPS literal `build_with_chromium: True` 和`recursedeps`，ANGLE standalone
DEPS多个输入带`not build_with_chromium`，PDFium三项README直接指向Chromium根。
尚未解析真实builder custom_vars/继承语义/平台checkout；不能直接以此排除所有ANGLE递归项，
也不能用ANGLE standalone pins覆盖Chromium根不同版本。下一批需准确来源/条件证据。

## 本批验证、清理与下一步

显式runner最终 **99 passed / 0 failed / 0 skipped**：tree13/DEPS9/external29/closure13/
policy20/classifier15，完整原日志`license-tests-support-01.log`保留。覆盖准确Windows条件、
组身份和唯一pins、独立seals、原boolean标记、Not Included不改变resolution/义务、旧全部边界。
之前98项试跑exit0在工具流，但输出截断；不冒充完整raw log，最终99原日志另存。
最终syntax/LF/EOF/diff和质量门禁记录在本批final evidence；FINAL仅质量，不是CU交付。

无新可退休build/运行副本/重复包，删除0/实际回收0。新source/bare/所有reports是后续输入，
旧首败和current binary/依赖/用户home保留；不确定cache暂缓，不停止无关owner。

继续公共浏览器运行源/非标准元数据/生成步骤/真实平台与递归覆盖，同源完整NOTICE/
源提供/LGPL-media/稳定安全更新策略。四157快照仍401-byte sample Credits，generator未运行，R1红。
不能因25错误减少就改产品pins或晋升seed。
R2最终格式/布局/五links/四锁/完整prepare/check、R3四架构原生（两Mac含原生Intel），
R4所有四surface/UI UX/安装签名更新修复回滚卸载/真人原模型ACP-MCP/picker/
同最终候选12h/D00–D31全部保持，见[维护版计划](2026-10-13-computer-use-maintained-runtime-next.md)。

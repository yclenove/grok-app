# CU 维护浏览器：复制 API 通知与省略记录交接

2026-10-13；`feat/computer-use-implementation`；active / partial。
承接[默认通知候选与完整 ANGLE GN 来源](2026-10-13-computer-use-notice-candidates-checkpoint.md)。
App macOS 11+ / CU 受管浏览器 macOS 14+ 的已确认边界保持。

## 本批实质结果

按同一 sealed ANGLE tree 收齐八个 API 目录的所有 regular `.h`，以及
`src/third_party/khronos/` 的全部 regular `.h`：**38 paths / 1430862 source bytes**。
不是只检查八份历史头文件；完整路径集合与固定树逐项一致，四实际 checkout manifests 匹配。
使用显式 Git object transport、原同 revision bare cache，raw batch/stdout/source bytes 全部封存。
root receipt SHA：`9615a5d31c9e25e10959b5b51a6a434a0db7f275a955321eb4845e57f7e9f93e`。

38 份原版权块逐字节保留：**11 SPDX / 20 Apache 短声明 / 5 完整内嵌 permission / 2 ANGLE BSD 引用**。
SPDX 与 Apache 短声明均绑定前批准确标准全文，两个 ANGLE 扩展绑定真实根 LICENSE。
原年度、OpenCL 商标声明、完整 disclaimer 和标准占位符保持，不以 SPDX 名称替代完整正文。
补充片段 **39880 bytes**，SHA：
`5f960834cf56af323de6eb9a80603bb73ac2658d789698b7ed081e40483b88dd`。
仍标为 `PARTIAL COPIED HEADER NOTICE SUPPLEMENT - NOT FOR DISTRIBUTION`。
这些目录的完整覆盖不代表所有 copied/generated 文件或完整 Chromium NOTICE/SBOM。

## 17 条原省略记录

| 来源复核结果 | 条数 | 仍须保留的边界 |
| --- | ---: | --- |
| 配置对应的真实许可路径和完整字节已绑定 | 10 | 配置来源证据，不是有效 GN / binary 测量 |
| 对应 API 的实际复制头文件通知已绑定 | 3 | EGL/OpenGL/OpenCL 原始 registry checkout/provenance 仍未证明 |
| source / shipping obligation 未解决 | 4 | ANGLE OpenCL ICD Loader、ASTC、CPU 与 Fuchsia SDK；manifest 缺项不是未随包证明 |

十条对应包括 Crashpad lss/zlib、ANGLE Vulkan/LLVM、OpenScreen URL/QUICHE/Perfetto/三 C++ 库。
记录各自原 metadata、真实 LICENSE 路径/对象/hash/字节以及原配置证据，不建立伪 alias。
其余四条的原 DEPS 条件、实际四 manifest 查询结果独立保留，不执行条件或把缺项归为 NOT_SHIPPED。
原 **17 yes 错误 / 196 目录问题 / 5 非标准值 / 421 raw gaps** 全部保持。
495 默认有效候选的原完整 report 和 5453606-byte 预览经重新构建逐字段/字节重现。

## 验证与收尾

显式完整工具 suite：**411 passed / 0 failed / 0 skipped**。
新增回归覆盖完整路径筛选、版权块/完整短声明/内嵌 disclaimer、实际路径与 Git/digest 边界、
manifest 缺项不排除、原省略记录不丢失、通知字节及额度、已有输出不覆盖。
首轮在本机 Python 3.8 的 `str.removeprefix` 失败；原日志保留，改用等价切片后11项真复验通过。
原原生/App/R2 结果没有替代本批 source 实测；本批工具测试也不代替产品原生验收。

独立 `notice-supplements-02` / `03` 的 inventory 与完整 fragment 实际SHA一致。
inventory SHA：`10e3b61045f3aa88a6ebd90a30f2947fb19d872d4ab0250898a72bb6db2b5574`。
preledger实际质量/格式/hash通过：75 Python AST / 88严格格式 / 95 LF-EOF-hash，
原R2 22源码hash/十历史许可seals与SPDX再次匹配；最终回读记录为
`notice-supplement-final-input-hashes-01.json` / `quality-notice-supplement-final-01.json`。
App+Workbench13573 / ≥1000源码80 的冻结门禁保持。

本批只增加源码和证据，无 compiled site、runtime 执行副本或重复包；不删除 source/bare cache。
保留 current App144、依赖、immutable seeds、所有失败/成功证据、WIP 和用户 home。
本批删除0 / 逻辑删除0 / 实际回收0，最终空间观察与原owner终态记录为
`notice-supplement-cleanup-01.json`；不把并发磁盘变化计为回收。

## 下一批必须推进的内容

先处理196目录问题中的实际 copied/generated 版权输入与四条未解决义务，
再完成四平台完整NOTICE/SBOM、源提供/LGPL-media与稳定安全更新策略。
R1通过后迁移最终维护pins/四source locks/worker/layout/prepare/check/repair/rollback/bundle；
随后四架构原生/安装/签名/更新/卸载、四surface/UI UX、真人原ACP/MCP/Grok/model/picker，
最后冻结唯一candidate做12h与D00–D31。原[R1–R4](2026-10-13-computer-use-maintained-runtime-next.md)完整范围保持。
产品仍为旧Node20/PW1.48/Chromium130，本批不晋升用户包或宣称可发行。

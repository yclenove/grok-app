# CU 维护浏览器：统一通知候选与发行阻断项

2026-10-13；`feat/computer-use-implementation`；整体 active / partial。
延续 [Dawn 实际配置](2026-10-13-computer-use-dawn-overrides-checkpoint.md)。
App macOS 11+ / CU 受管浏览器 macOS 14+ 边界保持；两 Mac 原生仍 not_run。

## 实际产物

新增 `assemble-chromium-notice-candidate.py`，复用 Dawn 的源码复验入口，
重新得到完整相同的 Dawn inventory 与片段，再按当前已验证输入生成默认
通知正文。合并五份封存补充：复制 API、SwiftShader、MD5/libbacktrace inline、
Skia/Dawn 与 Dawn 实际配置。原声明、版权、permission、disclaimer、partial
标记均按原字节保留，不用短 SPDX 声明替代完整许可，不修改原元数据路径。

当前 **502 默认候选 / 正文缺口0**；默认正文5534914 bytes，SHA
`c753f3fc7053f39e47fed03eaad5cfa97b71a7f5b764bdc3f61c16b3dbdb12d9`。
合并文件 **5913901 bytes**，SHA
`c3d18afd4943e504ba711a3a718d344cebbd4677217b67c9bede3c45ff253afb`。
独立 `notice-assembly-01/02` 的整个候选和 inventory 逐字节一致。
inventory1043064 bytes，SHA
`b55239a307fcc32c5e6d1e5e3ad801fffc5cfea18b56932633be67aa86c5a2cd`。
这是真实可审阅的统一候选；**不是完整发行 NOTICE 或二进制 SBOM**。

18条原有 yes metadata 错误全部保留并对应：11条已有真实配置许可绑定，
3条复制 API 已补通知但 registry provenance 未证，4条来源/使用义务待处理：
ANGLE OpenCL ICD Loader、ASTC encoder、cpu_features 与 Fuchsia SDK。
原 **424 raw gaps / 212目录问题 / 5异常值** 完整保留；不按配置绑定把原错误
改为0，也不凭 manifest 缺项证明 binary 排除。

候选 inventory 单列 Dawn Googletest/PartitionAlloc、SwiftShader/copied-generated、
三处 registry、四平台 binary/SBOM、源码提供/媒体许可/安全策略的开放项。
`releaseReady:false`、`completeChromiumNotice:false`、`promoted:false`。
CLI 完成导出后实际 **exit2**，防止产物生成成功被当作发行门禁通过。
缺原片段、修改字节、不同 commit、越界预算、重定向或重写元数据均拒绝；
不执行上游代码/条件/GN，不创建 browser/runtime/manifest 或授权。

## 验证与收尾

显式来源工具 suite **475 passed / 0 failed / 0 skipped**，包括13项新的
完整片段/预算/错误保留/真实目录绑定/输出不覆盖反例；不是产品实机验收。
测试首次因多行表达式括号遗漏出现 SyntaxError，已修正，原工具输出保留。
两次原 assembly owners 均终态，实际 exit2/releaseReady:false；这个预期阻断
保留，不能改报成发行通过。115 LF/单一EOF/hash、90 Python AST、108严格
whitespace、原R2 22源码SHA和原Dawn两份完整报告/片段复验通过。
FINAL质量通过，App+Workbench13573 / ≥1000行源码80未增长。
最终文档回读：`assembly-final-verification-01.json`、`quality-assembly-final-01.json`；
清理：`assembly-cleanup-01.json`。

证据根：`tools/computer-use-probe/.run/maintained-notice-source-20261013-01/`。
产物：`notice-assembly-01/maintained-browser-notice-candidate.txt` 与
`notice-assembly-01/notice-assembly-inventory.json`；独立副本使用 `02`。
日志：`assembly-first-pass-01.log`、`assembly-independent-pass-02.log`、
`assembly-tool-suite-01.log`。本批无新 compiled site、writable runtime 或包，
删除0、逻辑0、实际回收0；其余批 live/不确定owner产物暂缓，不停止其它进程。
当前 App144、home/登录/草稿、依赖、immutable seeds 与所有 WIP 保留。
产品仍 Node20/PW1.48/Chromium130，未提交、push、PR 或发行。

## 下一个交付步骤

1. 对四条待处理来源，结合原 DEPS 条件、parent 变量继承、四实际构建记录
   与对应消费者核对。先区分 standalone 声明和实际 Chromium 输入，不为
   raw 计数清零下载无关 Android/Fuchsia SDK；原记录保留，排除结论要有依据。
2. 补复制/生成来源、Dawn 剩余目录、媒体及 source-offer 义务；统一候选作为
   后续审阅入口，不再将分散的片段当作最终通知。完成四平台 NOTICE/SBOM/
   安全更新策略后才允许选定维护 browser 并晋升。
3. 继续 [R1–R4](2026-10-13-computer-use-maintained-runtime-next.md)：最终 pins/
   四锁/production prepare/check/repair/rollback/bundle，四架构原生、四 surface/
   UI UX、安装签名更新卸载、真人 ACP/MCP/Grok/model/picker、唯一最终候选12h
   与 D00–D31全部保持开放，不能借本批来源工具绿灯缩小范围。

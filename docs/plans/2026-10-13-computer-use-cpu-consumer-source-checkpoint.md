# CU 维护浏览器：ANGLE CPU 候选消费源核验

2026-10-13；active / partial；承接[Khronos 完整许可正文](2026-10-13-computer-use-khronos-notice-checkpoint.md)。

两份准确 ANGLE 源 `src/libGLESv2.gni` / `src/gpu_info_util/SystemInfo_android.cpp`
共 **25913 bytes** 已采，独立 sealed tree/Git blob/SHA 与四 actual checkout manifests 匹配。
inventory SHA-256：`b0b138d508c231e9b02de81cf8c71937e88b069704884e7d508fcf94827f821d`。
源列表把该 implementation 列在 Android system-info sources 内；
这两份文件本身没有 `cpu_features` / `cpu-features.h` 引用。
原 ANGLE DEPS 此项的 literal condition 是 `checkout_android and not build_with_chromium`。

这里只核验这个候选消费位置，没有遍历所有 GN/import/C++、求值完整图或证明二进制不使用 CPU 库。
主 Chromium 虽引用同版本 CPU 库，仍未把其路径伪装为 ANGLE 本地 `src` 的消费位置。
全部原错误和目录保持，原 consumer audit 的7554 selected paths不是本批新重算的合并计数。
其它 CPU 消费位置和完整四平台 NOTICE/SBOM/源提供/安全策略继续开放。

collector 增加一个 fixed profile，仍使用原20秒/字节预算/无redirect/source-as-data合同。
首轮遗漏 output-prefix 注册导致 `KeyError`，尚未建source目录；失败日志01保留。
补齐注册并增加 existing/foreign-output反例后，原目标目录成功采集，成功日志02单独保留。
显式 runner实际 **381 passed / 0 failed / 0 skipped**；
实际offline profile/raw/source/四manifest重验、前批SPDX完整正文和fragment重验、
69 Python AST / 80严格格式 / 87 LF-EOF-hash与最终质量门禁通过。
原R2 22源码hash不变，App+Workbench13573/≥1000源码80不增长。
记录：`cpu-consumer-source-coverage-01.json`、`quality-cpu-consumer-final-01.json`。

没有compiled site/runtime副本/重复包，删除0/逻辑0/实际回收0，
`cpu-consumer-cleanup-01.json` 保留空间观察与再生说明；
current App144/依赖/immutable seeds/源WIP/用户home与全部证据保持。
没有操作用户App、代理、登录或模型任务，也没有提交/推送/PR/发布。

下一批先完善 CPU/其它copied-generated消费来源和字段问题，完成R1后进入最终维护runtime迁移。
原[R1–R4](2026-10-13-computer-use-maintained-runtime-next.md)完整目标仍active/partial，
App11+/受管Mac14+、产品旧Node20/PW1.48/Chromium130、pins/四锁/seed/App144/home均未晋升。

# CU 维护浏览器：真实 C++ 来源与 Khronos 头文件通知

2026-10-13；`feat/computer-use-implementation`；active / partial。
承接[ANGLE / OpenScreen 集成](2026-10-13-computer-use-integration-checkpoint.md)。
App 保持 macOS 11+，CU 受管浏览器要求 macOS 14+。

## 本批结果

准确 `build/config/c++/c++.gni` 默认使用 in-tree libc++，source prefixes 明确为
`//third_party/libc++/src` / `//third_party/libc++abi/src`。
主 buildtools 两 BUILD 的实际 source 引用和 libc++ 对 LLVM-libc 的依赖已核验；
四原 GN 写入参数没有覆盖这四个 C++ 参数。
三库准确 DEPS/Gitlink 与四 actual manifests 一致，真实 `LICENSE.TXT` 字节、Git blob/SHA256 已验。
没有把 standalone OpenScreen/ANGLE 的同名目录改写为主树路径；原缺项仍保留。
这些是源码规则和原参数证据，未测量 effective GN 或二进制链接组件。

| 来源 | 准确 revision | 已核验许可证原路径 |
| --- | --- | --- |
| libc++ | `97b436da4c33663581d394f4ee0a5977fc38c2f4` | `third_party/libc++/src/LICENSE.TXT` |
| libc++abi | `0e691bb69ac90450d8d53bc8ee5968c62f9c15a7` | `third_party/libc++abi/src/LICENSE.TXT` |
| LLVM-libc | `97e60a1e7f7180965cc84e22ddde9f1d4ffd640f` | `third_party/llvm-libc/src/LICENSE.TXT` |

ANGLE 的总 BSD license 不能代替其复制的 Khronos 文件的逐文件版权/许可。
实际八份头文件及所引用的三份 platform headers、KHR platform header已收齐；
每份 attribution block 按原字节保留，不能把 SPDX 简短声明当完整许可文本。

| ANGLE 内的头文件 | 原声明 | 版权年份 |
| --- | --- | --- |
| `EGL/egl.h`、`EGL/eglext.h` | Apache-2.0 | 2013–2020 |
| `EGL/eglplatform.h` | Apache-2.0 | 2007–2020 |
| `GLES2/gl2.h`、`GLES3/gl3.h` | MIT | 2013–2026 |
| `GLES2/gl2platform.h`、`GLES3/gl3platform.h` | Apache-2.0 | 2017–2020 |
| `KHR/khrplatform.h` | 完整内嵌 permission/disclaimer，无 SPDX 字段 | 2008–2018 |

copyright owner 原文均为 The Khronos Group Inc.，原许可块/hash/行号保留。
同一 GLES include 链同时有 MIT 和 Apache-2.0 文件，不按目录统一归类。
七个简短 SPDX 头文件仍需要完整对应许可文本；这不是完整 registry checkout、
registry generator 复现或所有 Khronos 文件/最终 NOTICE 验收，不创建 registry 嵌套 alias。

## 来源与复验

复用有界 fixed Gitiles collector 的 consumers / platform profiles，
采 **13份 / 277232 bytes**；包括5份 build rules 与8份实际 headers。
固定 source/blob 与独立 sealed tree匹配，Chromium/ANGLE/OpenScreen三来源均对应四 actual manifests。
仍为20秒、raw2MiB/source1MiB/总8MiB，拒绝 redirect/Content-Encoding，源码只作 inert data。

receipts：

- consumers10份：`861b91404cabff20baac863fae621dbe52a03c571b1440eb222facb5d41a9ee1`。
- platform3份：`6a6b1b7df2dd85bafa6dee5fe4187f66039c0de51f1eb5405c17efa3168f3f76`。

`inspect-chromium-consumer-coverage.py` 从实际来源重新生成前批 integration audit，
与原 sealed `04` 做内容及原序列化字节匹配；没有只比较计数或复用旧绿色结论。
随后核验两个新 profiles、11条 C++ source rules、三库四 manifests和8份 attribution/include引用。
`consumer-coverage-01.json` / `02` 独立同 SHA256：
`1c4a82836579fde17a64a00a3d7ae62eca9054b56b4fa047f721522a085fed8d`。

结果 **7554 selected paths / 1251 raw references / 829 resolved / 422 raw gaps**；
**17 yes metadata errors / 196 directory errors / 5 nonstandard Shipped values** 全保留。
比前批多13份准确 source paths，未排除 raw gaps 或把 header归因当 binary组件数量。
`licenseCoverage:not_proven` / `promoted:false`。

## 验证与清理

collector增加2项，consumer inspector新增10项，显式 runner实际
**369 passed / 0 failed / 0 skipped**。
覆盖foreign/conditional DEPS、Gitlink/manifest缺失或冲突、license缺失/篡改/symlink/digest错、
C++覆盖、MIT/Apache/版权年份变更、截断disclaimer、缺platform header和虚假registry别名。
前批报告重算首败是比较器假定LF，但原Windows writer产物为CRLF；修正为仅接受两种准确序列化，
仍校验原raw seal且不重写旧证据，LF/CRLF及篡改反例均通过。
测试首败误期望缺header的后一个诊断，实际先被include检查拒绝；按真实路径拒绝原因修正，原失败日志保留。
收尾67份Python AST、76份严格格式、83份LF/EOF/hash与质量门禁通过；
App+Workbench13573 / ≥1000源码80不增长，原R2 22源码hash、四历史许可seals、
两Crashpad和两integration报告保持。记录为 `consumer-final-input-hashes-01.json` /
`quality-consumer-final-01.json`。
本批只生成inert source/报告，没有compiled site/runtime执行副本/重复包，
删除0/逻辑0/实际回收0，`consumer-cleanup-01.json` 保留空间观察与保留项。
保留current App144、dependencies/seeds/source WIP、用户home和全部成功/失败证据；
产品pins、四锁、seed、App144未晋升，没有提交、推送、PR或发布。

## 下一步

1. 完整 SPDX license文本、其余 copied/generated文件与 ANGLE cpu_features消费路径；
   后者 BUILD 仍引用本地 `src`，没有根路径重定向证据，不用主 cpu_features同名来源填它。
2. 完成实际四平台 NOTICE/SBOM、源提供/LGPL-media、稳定安全更新策略，继续其它生成输入和字段问题。
3. 原[R1–R4](2026-10-13-computer-use-maintained-runtime-next.md)全部保持：最终pins/四锁/production prepare/check/repair/rollback/bundle、
   四架构原生/系统边界、安装/签名/更新/卸载、四surface/UI UX、真人ACP/MCP/Grok/model/picker、最终candidate12h/D00–D31。

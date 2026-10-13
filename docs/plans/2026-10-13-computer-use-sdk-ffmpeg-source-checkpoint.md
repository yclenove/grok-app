# CU：SDK / FFmpeg 来源收尾与远端同步预检

2026-10-13；`feat/computer-use-implementation`；整体 active / partial。
承接[三项 ANGLE 库许可接入](2026-10-13-computer-use-angle-library-checkpoint.md)。
App 保留 macOS 11+；CU 受管浏览器要求 macOS 14+，两种 Mac 原生验收仍未完成。

## 本批已完成

上一轮两个采集进程已分别加入并取得真实 exit 0：Fuchsia SDK 为 session 96879，
FFmpeg 为 session 71156；没有在等待期间重启或替换原采集。
证据根为 `tools/computer-use-probe/.run/maintained-notice-source-20261013-01/`。

准确 SDK 声明为 `fuchsia/sdk/core/${{platform}}`、`version:33.20261003.1.1`，
本次取得其中的 `linux-amd64` 包。完整归档 3163546039 bytes、GCS generation
`1790999028701501`，SHA256 为
`e33ce110978587e1771928171ca0efffb4e609a5505353c16f873cda2b5d4114`。
原采集与独立回读均按完整归档计算 SHA；没有安装、解压可执行文件或执行上游代码。

归档 10327 个条目；SDK manifest 实际 ID 为 `33.20261003.1.1`，
host 为 `x86_64-unknown-linux-gnu`，有 519 个 parts。
原来缺失的根 `LICENSE` 是 1275-byte BSD-2-Clause，版权为 Fuchsia Authors。
同时发现并完整保留三份附加文本：`LICENSE.vulkan` 11661 bytes、
`tools/x64/aemu_internal/LICENSE` 524 bytes、
`tools/x64/crosvm_internal/LICENSE` 1539 bytes。
Vulkan 文本含 Apache 2.0 及例外说明；aemu 文本引用 Apache 2.0；crosvm 是 BSD-3-Clause。
不能用根 BSD 文本概括整个 SDK，也不能把这些文本视为所有内嵌组件许可已齐。

完整五份源文本含 manifest 共 75844 bytes。独立 `sdk-ffmpeg-sync-review-03/04/`
整个报告、五份源文本及 15327-byte partial 通知片段逐字节一致；
报告 SHA256 `70582c746a2759c19288b2c3553144d24258c20c5574363b6facf25d3bf79cfa`，
片段 SHA256 `6dc3d18325bf82d2671dfeb717f342856790f2b1832465f1bbda46a01e763d2a`。
这些只是准确 Linux SDK 来源，不证明其他宿主 SDK 可取得、浏览器实际使用、
binary 排除或完整包通知覆盖。本批尚未把 SDK 片段接入统一候选。
原候选07/08、全部18原始 metadata 错误和剩余 SDK 分类均保持。

FFmpeg 准确 revision 为 `5c85ff666b3434d9323b83437f910ad8431ab7c9`，
来自 Chromium 的 `third_party/ffmpeg.git`。11351 条完整树、11 份选定原文
162504 bytes 及原许可选择全部复验；新增 `BUILD.gn` / `ffmpeg_options.gni`
的 Git blob、SHA256 和内容与原采集记录一致。组件 receipt SHA256 为
`bea963238be1b7d798323a458e222fc6e603c13ab7f405f2412fd443c07f939e`。

准确声明是 `is_component_ffmpeg = is_component_build`。四平台原 GN 写入参数
都记录 `is_component_build=false`，没有显式 FFmpeg override；按这些记录和
源码默认值会走 `static_library` 分支。尚未执行有效 GN 或测量最终 binary，
因此这是构建来源结论，不能冒充实际链接测量。四个原 ZIP 无独立命名 FFmpeg
文件也不能证明排除媒体。后续必须完成对应源码、Chromium 配置与修改、
重链接/替换途径及分发材料的核验，不能直接假定独立动态库替换路线已满足。
`binaryLinkageMeasured`、`sourceOfferComplete`、`releaseReady` 仍为 false。

## 远端同步

已成功执行 `git fetch --prune origin` 和 `git fetch --prune upstream`。
本次固定引用及合并顺序见[同步实施计划](2026-10-13-computer-use-upstream-sync-plan.md)。
对当前 committed HEAD 的两次非 checkout 合并预检均得到一致结果：
fork 有2个冲突文件，源仓库有6个冲突文件。235个 tracked WIP 文件中，
与 fork 更新重叠7个、与源仓库更新重叠36个。
原 HEAD、真实 index、235份 tracked WIP 内容和 status 在预检前后均保持，
但此预检不包括将 WIP/未跟踪源文件真正参与合并，不能把冲突数当作最终上限。
没有合入当前工作分支、提交、push、PR 或发行。

## 验证和清理

原565个来源工具测试属于上一批结果；本批未改产品代码或来源工具，
通过原源码哈希与既有验证证据回读保持该结论，不宣称重新执行565个测试。
本批独立来源回读及 Git 预检实际成功；首次回读01错误复用了20MiB小文件
hash helper，被大小门禁拒绝，失败证据保留。随后仅为3GB源归档使用按准确
大小限制的流式hash；03/04为最终脚本的独立成功结果。

最终格式、源码/旧候选身份和质量回读记录在 `sdk-ffmpeg-final-verification-01.json`；
清理记录为 `sdk-ffmpeg-cleanup-01.json`。本批没有新编译网站、runtime副本或包，
删除0、实际回收0。3GB SDK是准确来源证据，和当前候选、依赖、旧失败证据保留；
不删除别的工作区或归属不明目录。当前用户 App144、隔离home、登录、会话、
未发送草稿继续保留；没有新开测试App、代发模型任务、授权或选文件。

## 下一步

在继续产品改动和新包验收前，先按同步计划保存可恢复CU源码快照、分别合入
fork与源仓库并验证会话生命周期。随后接入准确SDK来源并保留包内义务边界，
继续SwiftShader Boost/libbacktrace、复制/生成源、完整四平台NOTICE/SBOM、
媒体/源码提供与安全更新策略，再维护pack/pins/四锁和production
prepare/check/repair/rollback/bundle。原R3–R4四架构原生、四surface/UI UX、
安装签名更新卸载、真实模型/picker/文件/Pause/Continue/Stop/Quit、
同一最终候选12h及D00–D31完整范围仍开放。产品runtime尚未晋升。

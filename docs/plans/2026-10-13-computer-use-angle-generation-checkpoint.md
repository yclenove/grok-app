# CU 维护浏览器：ANGLE 生成记录与四平台来源回归

2026-10-13；`feat/computer-use-implementation`；整体 active / partial。
延续[Dawn 剩余配置许可](2026-10-13-computer-use-dawn-support-checkpoint.md)。
用户已允许 App macOS 11+ / CU 受管浏览器14+；两种 Mac 架构原生仍未验。

## 本批修复

真实重跑发现旧 `inspect-chromium-platform-coverage.py` 将新增 WebGL/Dawn 等
补充组计入原五组，却仍要求原57个来源，直接抛出 `ValueError`。
检查器现显式固定 `core/media/runtime/support/browser` 五组；后续补充继续由
独立 receipt 验证，不改变原报告范围。缺来源、重复来源及错误revision仍拒绝。
新增三个反例，原回归失败日志保留。

真实重跑 `platform-source-coverage-04/05.json` 与原03逐字节一致，SHA256
`abd9e3c0cb332e2126da16cc27871efc216c39b30c4c0261997d573a08081115`。
主树加63源、四平台实际manifest匹配59/62/56/56，以及439 raw gaps、19 yes错误、
190目录问题、5异常字段全部保持。这是旧范围重现，不是当前完整通知覆盖。

## 固定来源与生成记录

ANGLE pin仍为 `9649fea17b77e672ae19687bf0222109b6270e0d`。
新增三个独立采集profile，均以原完整Git树、原Git对象、SHA256、四个原manifest
和完整cat-file响应核验：

| profile | 文件 | 原文字节 | 内容 |
| --- | ---: | ---: | --- |
| angle-registry | 4 | 158479 | registry helper、generation runner、两ANGLE扩展XML |
| angle-generation | 9 | 206451 | 五份生成hash记录、四个generator源码 |
| angle-generated | 7 | 841109 | EGL loader、GL enum、proc table等选定生成输出 |

总20份原文、1206039 bytes。可执行位100755是普通Git源码，作为数据采集和
审查；collector与复验器均接受100644/100755，继续拒绝symlink/gitlink和错对象。
上游源码、generator、hooks及条件求值器均未执行。

新增 `inspect-chromium-angle-generation.py`：静态读取字面XML输入和五项
generator映射；核对原MD5算法的AST，包括Windows/Cygwin的CRLF归一化。
五份生成记录拒绝重复JSON键、非规范路径、非MD5值和超预算。
22条记录与已采固定源码匹配，其中涵盖全部七份选定生成输出。
每个源仍以Git对象和SHA256验证；MD5只作上游生成记账，不当安全身份。
108条未测记录逐条保留，未宣称重跑generator或完整生成覆盖。

三处registry准确DEPS/Gitlink：

| registry | revision |
| --- | --- |
| EGL-Registry | 7dea2ed79187cd13f76183c4b9100159b9e3e071 |
| OpenCL-Docs | 5b4ca15f0e5a5be87b56b99f652f728e05cab587 |
| OpenGL-Registry | 9cb90ca4902d588bef3c830fbb1da484893bd5fb |

三声明都是 `not build_with_chromium`。复用原已封存context，在四份实际配置
对应的本地分析中得到12个false；原manifest缺项也保留。二者都不证明二进制
排除，也不证明copied API headers来自这些registry的相同revision。
原 `Revision: DEPS` / `License: Apache-2.0` / `License File: Not Included` /
`Shipped: yes` 全部保留，不建立假许可路径、不消除原metadata错误。
五份registry XML的实际字节、完整原许可和复制头文件来源仍待验证。

## 验证与收尾

完整显式来源工具suite：**533 passed / 0 failed / 0 skipped**。
覆盖旧报告scope回归、可执行源码仍惰性数据、跨profile/覆盖拒绝、生成记录
格式/重复键/路径/预算、Git身份不可被MD5替代、生成hash不符、错误registry
pin/condition/metadata、manifest缺项不作binary排除。

证据根：`tools/computer-use-probe/.run/maintained-notice-source-20261013-01/`。
三个profile对应 `angle-registry-git-source-01/`、
`angle-generation-git-source-01/`、`angle-generated-git-source-01/`。
独立 `angle-generation-coverage-01/02.json` 整个报告逐字节一致：
261768 bytes，SHA256
`58001809c56623a479952230a705cdec2dc0377364f74830e3341f62804b4155`。
日志为 `registry-generation-first-pass-01.log`、
`registry-generation-independent-pass-02.log`、`registry-tool-suite-02.log`。
旧平台失败留在 `platform-baseline-regression-01.log`，未覆盖或隐藏。

最终格式/原输入/独立产物指纹与质量回读见
`registry-generation-final-verification-01/02.json`；清理记录
`registry-generation-cleanup-01.json`。App+Workbench13573 / ≥1000源码80
保持；原R2 22源码、条件报告、Dawn补充与原统一通知候选保持。
最终回读脚本首败是原20.47MB平台报告超过其16MiB读取额度，未进入后续
断言；按真实固定大小改为32MiB有界读取并保留原SHA，重跑通过。原失败日志
`registry-generation-final-verification-01.log` 和通过的 `01-retry.log` 均保留。
本批未构建App/runtime副本或包，删除0、实际回收0；原来源cache及全部证据
供后续使用，其他live/不确定owner的产物不动。

当前产品仍Node20/PW1.48/Chromium130，维护候选未晋升。
App144及home/登录/history/未发送草稿保留，未发送新模型任务或代批授权。
无新commit/push/PR/发行。

## 后续顺序

下一步仅补上述五份准确XML及原许可、核对复制头文件来源；生成记录不能
替代这些证据。随后处理SwiftShader剩余源义务、媒体/source-offer和安全更新
策略，完成四平台NOTICE/SBOM后推进最终维护pack/pins/四锁及
production prepare/check/repair/rollback/bundle。
不扩大旧四平台/四条件检查器的范围。
原R3–R4四架构原生、四surface/UI UX、安装签名更新卸载、真人模型/picker/
文件/Pause/Continue/Stop/Quit、唯一最终候选12h和D00–D31仍开放。

# CU 维护浏览器：三项 ANGLE 库许可接入

2026-10-13；`feat/computer-use-implementation`；整体 active / partial。
延续[三库来源采集](2026-10-13-computer-use-angle-remaining-source-followup.md)和
[registry XML 检查点](2026-10-13-computer-use-angle-registry-checkpoint.md)。
App macOS 11+ / CU 受管浏览器14+边界保持，两种 Mac 架构原生未验。

## 本批结果

OpenCL-ICD-Loader、astc-encoder、cpu_features 的三个准确revision/Gitlink、
764条完整树、5份原文42962 bytes、原metadata路径和原四平台上下文已复验。
三份根许可34372 bytes；astc另两份测试图片CC0/MIT许可单独标记，不当作
binary依赖证明。cpu_features完整LICENSE中的Android BSD附录保留。
不执行上游代码、generator或条件，不修改原 metadata 和旧报告。

独立 `angle-library-notices-01/02/` 整个inventory及43704-byte通知片段一致。
inventory91121 bytes、SHA256
`7330869ea14057472603f93f8502d646ebe4186a0c9c07f4eecdb41117155505`；
片段SHA256 `e35f603ea3d6133c9d95cc95dffcc9b9b9a344b8b71086ffa32524f0f59ffee8`。

第八份 `angle-libraries` 补充已接入统一候选，原五/六/七份入口分别冻结，
旧候选01–06不变。全部18条原始metadata错误继续保留，并同时记录原分类：
11配置绑定 / 3复制来源待证 / 4未解决来源义务。新分类是11 / 3 / 3准确库
许可已绑定 / 1未解决；剩余项明确为Fuchsia SDK，不以本地条件false或
manifest缺项推导binary排除。新绑定也不证明完整组件通知或binary使用。

独立新候选07/08整个正文及inventory一致；正文5975182 bytes，SHA256
`b9c3babfbea8c48866f6c40e82b1cb1df33f724aa871e61086f71e4224722147`。
两个原进程均实际exit2、`releaseReady:false`，不是发行NOTICE或binary SBOM。

## 验证与收尾

完整显式来源工具suite：**565 passed / 0 failed / 0 skipped**。
新覆盖原文/CRLF/Android BSD附录保留、完整五路径、缩短/改hash拒绝、字节
上限包含标签/分隔符、错Gitlink/声明路径/去原错误拒绝、输出越界/覆盖拒绝、
三库绑定不消除原错误或剩余SDK、错来源/重复绑定/错误角色及假binary批准拒绝。
原七份补充入口及五/六份旧入口可显式重现。

证据根：`tools/computer-use-probe/.run/maintained-notice-source-20261013-01/`。
日志为 `angle-library-notices-first-pass-01.log`、`angle-library-notices-independent-02.log`、
`angle-library-tool-suite-01/02.log`、`angle-library-assembly-07.log`及`angle-library-assembly-independent-08.log`。
最终审查补强了按准确路径验证库/测试图片角色，错误角色新增反例通过；02为最终源码完整suite。
最终格式/hash/完整独立产物与质量回读见 `angle-library-final-verification-01/02.json`；
133 LF/EOF/hash、102 Python AST、126严格格式及原R2 22源码身份保持。
App+Workbench13573 / ≥1000源码80不增长，原条件/平台/生成报告及全部旧补充保持。
清理见 `angle-library-cleanup-01.json`：无新构建/runtime副本/包，删除0、实际回收0；
source cache、依赖、全部失败/验证证据与当前可用包保留，其它归属不明产物不动。
产品仍Node20/PW1.48/Chromium130，App144/home/登录/history/未发送草稿保持。
无commit/push/PR/发行，也未代发模型任务、授权或选文件。

## 后续门禁

继续Fuchsia SDK准确来源/许可、SwiftShader Boost/libbacktrace及复制/生成源剩余
义务；媒体/完整源提供与安全更新策略、完整四平台NOTICE/SBOM仍未完成。
本批只读查看四个原snapshot ZIP中央目录，均无名称包含ffmpeg的条目；这只
说明没发现独立命名的FFmpeg库，不证明媒体被排除、静态链接或具体codec启用。
后续须核对准确GN配置/最终binary及LGPL对应源码和重链接路径，不能直接采用
独立动态库替换例外。原两次猜错源码路径的404是读取失败，不当作源码缺失。
不扩大旧平台/条件检查器范围。

随后最终维护pack/pins/四锁、production prepare/check/repair/rollback/bundle，
以及原R3–R4四架构原生、四surface/UI UX、安装签名更新卸载、真人模型/picker/
文件/Pause/Continue/Stop/Quit、同一最终候选12h和D00–D31全部仍开放。

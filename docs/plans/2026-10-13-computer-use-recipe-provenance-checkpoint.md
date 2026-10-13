# CU 维护浏览器：实际 recipe 包、helper 与构建配置

2026-10-13；`feat/computer-use-implementation`；active / partial。
承接[六个实际递归仓库](2026-10-13-computer-use-checked-recursive-checkpoint.md)。
App macOS 11+ / CU 受管浏览器 macOS 14+ 的用户确认边界及完整原范围保持。
本批仅审查工具、公开源码和证据；产品 pins、四锁、seed、App144 / home 不晋升。
不提交、推送、提 PR、发布、操作 GUI、发模型任务、代批权限或选择用户文件，不改代理。

## 1. 四个平台实际使用的不可变 recipe 包

`collect-chromium-recipe-instance.py` 网络前重新生成原四 archive / bootstrap 检查结果，
要求四份完整 receipt 中 package、server、actual_version、`luciexe` 命令一致。
只向固定公共 CIPD ResolveVersion / DescribeInstance 发送只读请求；
Resolve 的 version 必须是原不可变 ID，绝不解析 `refs/heads/main` 或其它浮动标签。
将 ID 解为 SHA256 ObjectRef，两个响应都必须准确匹配原 package / digest。
有界 256 KiB、20 秒、NoRedirect，raw 先保存再检查，迟败保留 partial receipt。

原 `recipe-instance-01` 实际 exit0，两个响应分别 343 / 2747 bytes。
root SHA256：`fd0a6a2ccb216b5017e05c965cf36d36912bfcb21d9a628de288308da5b006c6`。
实际 package 为 `infra/recipe_bundles/chromium.googlesource.com/chromium/tools/build`；
instance 为 `2QjLPDj2oNMKlnOBPW4LhuQHkh6Mc52_OS_yRNlo_RwC`，对应整包 SHA256：
`d908cb3c38f6a0d30a9673813d6e0b86e407921e8c739dbf392ff244d968fd1c`。
公开 git_revision tag 是 `b2d836bb9a700e372df6eb3ed40a2c8dcb40eeae`；
tag 本身不当作整包源码认证，附带 SLSA metadata 也没有声称完成签名验证。

`collect-chromium-recipe-package.py` 复核上述 seal / raw，再调用固定 GetInstanceURL。
只接受返回的 HTTPS `storage.googleapis.com` URL，不跟跳转；不使用用户凭据或 Cookie。
64 MiB 下载上限，streaming SHA256 与原不可变包 ID 一致才读取 ZIP。
20000 条目、4 MiB 单文件、128 MiB 声明树和 16 MiB 选定源码上限；
拒绝逃逸 / 重复 / NUL 名称差异 / 加密 / 超额，links 和特殊文件不选读。
不解压、不安装、不执行，只将选定 UTF-8 源码保存为 hash 命名的 inert source-data。
临时签名 URL 只存在于忽略的本地原证据中，不写进公开文档或工具输出。

原 `recipe-package-01` 实际 exit0：6214369 ZIP bytes / 3278 entries /
19458259 声明 unpacked bytes；13 个选定原文 / 417282 bytes。
root SHA256：`8c0bf4a8cb14e5bd49a24745f88e101cd9b1c4360d586321648db950338ecf52`。
这份 archive 是后续审查所需的来源证据，不是可删除的重复运行包。

## 2. helper 身份与 `.DEPS.git` fallback

新 `inspect-chromium-recipe-provenance.py` 重新校验完整 ZIP / digest / ZIP index /
选定 blob、原 Git helper receipts、原 16 件配置日志和四 checkout 日志。
`recipe-provenance-01.json` 实际 exit0，SHA256：
`1c52c04129bc3411152932526873b94ee8b642e837728bbbc37aa61bc572cd58`。

包内 `build/infra/config/recipes.cfg` 明确固定 depot_tools revision
`21ae2d24f8ee0ca3459bda416129d7791c4313f1`；两个真实打包 helper 与此前独立
Git blob 逐字节一致，而不是仅凭 checkout 内同名目录猜测：

| 包内文件 | bytes | SHA256 |
| --- | ---: | --- |
| depot_tools/gclient.py | 205384 | ac100091d3d3c02993cbcad11d13cd1e25ae8adb4346746bf85886dd96094913 |
| depot_tools/gclient_eval.py | 45233 | 091353f94971d21fd8cfb40cb193e47b64c0e87ff573f4cff10674f3ef99f19d |

精确 AST 核验 gclient lines 993–1011：先尝试配置文件，缺失时追加 `DEPS` 并选第一个实际文件。
bot_update lines 44 / 88 / 406–408 则固定同一 package-relative root 下的 gclient 路径和调用。
四份原 stdout 的 DEPOT_TOOLS_DIR、THIS_DIR 与 gclient sync 命令都匹配该 bundle 布局；
Windows 仍只保留原 4 MiB 前缀，不改称完整日志。
四配置原请求 `.DEPS.git`，同固定主 Git 树只有 `DEPS`；fallback 的准确实现已验证。
`recipeBundledHelperSourceVerified:true` / `depsFallbackImplementationVerified:true`。
没有测量远端运行时文件 SHA、重新执行 gclient、求值 DEPS 条件或宣称 binary 认证；
`builderFilesystemMeasured:false` / `actualEffectiveConditionsEvaluated:false` 保留。

## 3. 原 bootstrap config 的准确源文件

`collect-chromium-builder-config.py` 按四个原 bootstrap 的 config path 和
last_changed_commit `82064764761481b897d400961ee71601b5d796e0` 读取固定 Gitiles TEXT。
只有四个编译 properties.json URL，256 KiB raw / 128 KiB decoded / 20 秒 / NoRedirect。
每份 base64 解码原文都必须匹配独立封存主树中的准确 Git blob，再比较最终 bootstrap 值。
比较保留 JSON 类型差异（bool 不当数字），不同或缺项显式记录，不求值 override 语义。

原 `builder-config-source-01` 实际 exit0：4 files / 8465 decoded bytes。
每份 8 keys / 合计 32 keys 全部准确匹配原 bootstrap，缺项 / value differences 为 0。
root SHA256：`a77775679360f96b1fe935536a952bc4ef791c24144a4c7246d75c73dd920226`。
这不替代完整 recipe / GN 执行、平台许可覆盖或二进制通知。

一次 `git cat-file -e <config revision>` 存在性探针意外触发 partial-clone 自动抓取。
原 session39331 长时间无输出后，只按原 PID26124 / 创建身份 / 精确命令匹配取消其进程树；
没有停止其它工作区、用户 App 或代理。原 wrapper 随后 exit0 并打印 false，
**该 false 不用于判断 commit 是否存在**，被取消的探针也不算来源验证成功或网络不可用。
后续准确 config 采集走上述有界 Gitiles，不重启自动抓取；bare / 原证据保持。

## 4. 验证、质量、清理与后续

显式 runner `license-tests-recipe-final-01.log` 实际 exit0：
**236 passed / 0 failed / 0 skipped**，原 206 + instance7 + package9 + provenance8 + config6。
首轮 package test 的反斜杠 ZIP fixture 被 Windows ZipInfo 自动规范化，导致 1 个断言失败；
修正为对已读 record 明确注入非法反斜杠，保留原拒绝断言，duplicate warning 也改为明确断言。
没有删测试、放宽路径规则或执行收集到的 Python；原失败工具输出保留。

收尾记录为 `recipe-final-input-hashes-01.json` / `quality-recipe-final-01.json`：
owned Python AST、LF / EOF / 严格尾空白、hash / diff 和实际 quality final gate。
App + Workbench 13573 行、≥1000 行源码 80 件不增长；FINAL: PASS 仅本批质量。
R2 原 22 source hashes、原三许可回执及 63 源递归审计独立重验不变：
439 raw gaps、19 yes 错误、190 目录错误、5 异常值；完整 NOTICE / R1 仍红。

无新编译 site / runtime copy / duplicate package 可退休；删除 0 / logical 0 / 实际回收 0。
来源 ZIP / SDK / bare / 当前 build / seed / 用户 home / 所有成功与失败证据保留；
live preview 和不确定 owner cache 暂缓，空间观察不冒充回收，见 `recipe-cleanup-01.json`。
重新运行使用新 evidence 后缀；本批原 collector owners 均终态，不重复下载原 recipe 包。

下一步按实际 manifest、配置和已核验 helper 机制构造平台级来源 / policy coverage，
补齐生成输入并形成可审阅的完整 NOTICE / SBOM、source 提供 / LGPL-media 与稳定安全更新策略。
不通过“目录没出现”静默清错误。之后继续[维护版 R1–R4](2026-10-13-computer-use-maintained-runtime-next.md)
的四锁 / production prepare-check / repair-rollback-bundle、四原生架构、安装 / 签名 /
更新 / 卸载、四 surface / UI UX / 真人原 ACP-MCP-model-picker 与最终同候选 12h / D00–D31。

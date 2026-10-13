# CU 维护浏览器：四项依赖条件核对

2026-10-13；`feat/computer-use-implementation`；整体 active / partial。
App macOS 11+ / CU 受管浏览器 macOS 14+ 已获用户确认；两 Mac 原生尚未验。
当前用户包仍 App144 / Node20 / Playwright1.48 / Chromium130，未晋升维护候选。

## 结果与边界

新增 `inspect-chromium-condition-context.py`。重新复验完整 Dawn 输入与原汇总，
核对准确主 DEPS、gclient helper 变量优先级、四份实际 config 和原 sync stdout。
分析器只接受有界 names、布尔常量与 and/or/not；未知变量保持三值逻辑，
先拒绝全部不支持的语法，再分析，短路不能隐藏函数调用或其它动态语法。
不执行上游 DEPS/GN/hooks，不修改元数据、运行时、用户数据或授权。

| 原目录 | 原条件 | 四份记录配置下的本地推导 |
| --- | --- | --- |
| ANGLE OpenCL-ICD-Loader | `checkout_angle_cl_deps and not build_with_chromium` | false |
| ANGLE astc-encoder | `not build_with_chromium` | false |
| ANGLE cpu_features | `checkout_android and not build_with_chromium` | false |
| Fuchsia SDK 直接获取项 | `checkout_fuchsia_no_hooks` | false |

主 DEPS 精确字面默认 `build_with_chromium=True`、`checkout_fuchsia_no_hooks=False`；
gclient builtin 不覆盖这两项。四实际 config 只含两个已记录的 custom vars，
无 custom deps/target OS/CPU 覆盖。helper precedence 已由固定源码复验。
四原 sync argv 不含条件覆盖，并禁用 hooks。Windows stdout 只有原 4MiB 前缀，
本批没有宣称它记录了成功终态；其余三份实际有原命令的成功行。

这是 **4项 × 4配置 = 16个本地 false**，不是远端 `.DEPS.git` 字节测量、
有效 GN 图或二进制排除证明。Fuchsia 另有 `checkout_fuchsia and not
checkout_fuchsia_no_hooks` 的 post-sync SDK hook；是否实际执行/使用仍未测。
因此仅记录无需为清除 raw 计数收集独立构建项/SDK，四项原 metadata/条件/
空 manifest 及所有错误原样保留，不将它们自动改成 not shipped。

## 验证与证据

新增20项测试，覆盖三值逻辑、先验语法拒绝、预算、字面默认值、重复字段、
builtin/custom/config 覆盖、四目标完整性、原 sync 顺序与完成证据、原错误与
manifest 保留，以及输出越界/覆盖拒绝。显式来源工具 suite **495 passed /
0 failed / 0 skipped**。这不是产品或原生验收。

证据根：`tools/computer-use-probe/.run/maintained-notice-source-20261013-01/`。
实际输出 `condition-context-01.json`；独立重算 `condition-context-02.json` 与其
整个报告逐字节一致，222887 bytes，SHA256
`6a3d410ffe5e56af86b6333c8a4af433f20735332032e589ac93539506db4af2`。
完整报告逐字节比较、源格式/hash、原 R2 22源码与前批所有封存片段/两份
Dawn/两份统一候选复验结果写入 `condition-final-verification-01.json`；文档
最终回读使用 `condition-final-verification-02.json`。118 LF/EOF/hash、92 Python
AST与111严格格式通过，App+Workbench13573 / ≥1000源码80保持。
原统一通知仍 5913901 bytes、502默认候选，releaseReady/完整NOTICE/SBOM/
promoted 均 false；本地分析器正常 exit0 不代表发行阻断已解除。

日志：`condition-tests-01.log`、`condition-tool-suite-01.log`、
`condition-first-pass-01.log`、`condition-independent-pass-02.log`。
质量结果 `quality-condition-final-01/02.json`；收尾 `condition-cleanup-01.json`。
本批没有 compiled site、runtime 副本或包，删除0/实际回收0；保留当前用户
预览、登录/history/未发送草稿、依赖、源码及全部有用证据。

当前 App144 实际窗口已查看，普通界面正常；真人内嵌网页/上传任务仍是
未发送草稿，不能报告模型工具调用或后置条件通过。未发起新的权限请求。

## 后续工作

直接补 Dawn Googletest/PartitionAlloc 的准确配置许可与 copied/generated/
registry 来源，处理媒体/source-offer/安全策略并完成四平台 NOTICE/SBOM。
然后进入维护候选最终 pins、四锁、production prepare/check/repair/rollback/
bundle。原四架构原生、四 surface/UI UX、安装签名更新卸载、真人模型与
picker/文件/Pause/Continue/Stop/Quit、唯一最终候选12h及 D00–D31均保持开放。
不继续扩大这四个已分析条件的工具范围，也不借局部测试缩小完整交付目标。

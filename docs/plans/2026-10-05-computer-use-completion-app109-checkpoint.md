# 完成工具进入真实 Windows 应用 app109

日期：2026-10-05。单 Codex writer，分支 `feat/computer-use-implementation`；未提交、推送或发布。
证据根：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。
前序：[明确任务完成修复](2026-10-05-computer-use-explicit-task-completion-checkpoint.md)。

## 构建与来源

实际应用、前端和运行时新构建通过，原执行 session31134 已返回 exit0；没有使用 libtest 代替应用。
命令为 `pnpm exec tauri build --debug --no-bundle --config src-tauri/tauri.dev.conf.json --config src-tauri/tauri.windows.conf.json`。
前端54.77秒，Rust应用2m39s。原 chunk-size 警告和 linker 的创建库提示保留，不算断言失败。

构建前、构建后和便携包装后，source108 的3323文件复验一致：
`588bfd39aef87bec04d755aa28180d8a44b9ad031ca0481fe80c60b7740d0248`。
app109 是 source108 的包编号，不代表 source109。

| 产物 | 来源与校验 |
| --- | --- |
| `dist-portable/Grok_CU_20261005_app109/Grok.exe` | 92956672 bytes；SHA-256 `3ab2b4cf60539228a33dae1177291f8c410baf0ab3904426d1fbb825bf8c1e4d` |
| `dist-portable/Grok_CU_20261005_app109.zip` | 208405681 bytes；SHA-256 `af6310bd4bb0cf09e91854a091299daee83bb898725b99750a19d6d0388f76d0` |
| 包清单 | 450文件，共533937078 bytes；SHA-256 `b07da90d695aba3a104a46010c376ef7291e8672dea01e68757ea1482268a32d` |

stage和ZIP逐文件解包校验均exit0；清单文件本身另计，总解包451文件。
版本标识 `0.2.38-cu-dev.20261005.app109`，未签名、未发行。
具体记录 `delivery-app-109/artifact.json`、build/stage/package日志。

## 当前实机状态

唯一预览由 `delivery-app-109/launch-preview.ps1` 启动；启动入口验证包hash、独立模式、
实际退出设置、无其它任务预览，并恢复调用进程的环境变量。
精确App owner PID16980，创建UTC `2026-10-05T13:24:23.2203800Z`。
复用 `delivery-app-98/startup-home` 和原instance；原登录与历史会话保留。
界面确认 Grok4.7、xhigh、请求批准。没有调整代理、共享home或日常Chrome。

固定本机fixture由原session45470持有，PID60752，URL `http://127.0.0.1:1474/`。
实际App发送的新任务在会话 `09d11384-001b-4879-b2a9-8efbb5fc9cc7`，原消息位于
`delivery-app-98/startup-home/sessions/09d11384-001b-4879-b2a9-8efbb5fc9cc7/messages.json`。
模型检索当前CU目录并通过实际MCP提交managed-browser的observe/input申请：

- taskId `f00c24acdd0f40ad8ee9ddd135572b23`。
- requestId `c870a2d52b114e499ada382450810c56`；revision1，authority none，state pending。
- 任务卡已显示“允许本次任务”；批准前独立fixture回读为未保存、计数0、events空。

本次真人交接后，真实UI依次由等待批准进入准备、已授权，Grok自动在原会话续行。
Codex没有注入任何权限按钮动作，未把用户此前app102旧任务的确认当作本次批准。
本次未另收到新任务的聊天确认；证据为手工交接后的真实Host批准与业务链。
Computer Use guidance要求不能自动操作安全/隐私权限申请，业务与完成工具由真实Grok执行。

独立fixture核验保存“中文收尾验证 你好”，随后恰好3次increment，计数依次0、1、2、3；
仅4条effects，无重复保存或第四次点击。实际App消息18条工具、0错误，CLI ledger确认所有操作
都来自本任务CU工具，只有一次request与一次`computer_complete_task`，使用原taskId。
完成回执`authority=none/state=completion_requested`不作为业务成功证明，业务由上述fixture独立核验。

真实截图及随后刷新的native accessibility tree显示任务已完成、App就绪，任务Pause/Stop均移除；
原browser PID17416已退出。初次AX回读仍滞后，刷新后与截图一致，没有据旧AX错误判断产品失败。
原App拥有的runtime worker在完成后保持复用，随后随精确App退出消失，不作为任务orphan。
报告 `delivery-app-109/real-completion-result.json`；消息SHA-256
`f4bc3e1b0c35c51b32b5cf8aed94f92d2f623ba953cee0fdef5c0e78a86e1b70`，原ledger SHA-256
`d56b0775404347a0388eaef784c00587a22abdf30dbe40745884215f024fbe54`。
第一次报告脚本用Windows PowerShell5.1读不到原long-path ledger；错误保留，未产生pass报告。
用当前PowerShell7执行同一保存的verifier后exit0；不移动/替换原会话文件。

CLI启动记录还有既有第三方MCP未登录、chatcut字段和网络请求警告；没有为验收删除或改写这些配置。
它们未阻止本次CU目录发现及申请；其它MCP功能未在本任务内验证。

## 收尾与后续

已通过原生窗口Alt+F4退出精确app109；App16980、CLI61592、MCP44396、runtime worker30688和
browser17416均已不存在，host_runtime.json的shutdown=true。fixture经原session45470发送quit，exit0。
原home、登录、聊天和业务证据保留；没有按进程名停止其它应用。退出时WebView2有class unregister
1412日志，原owners均消失，保留该警告、不冒称所有日志零错误；见`shutdown-result.json`。
原包装脚本已清理自身临时解包副本；
没有单独测量该副本实际回收空间，不把同期磁盘变化当作回收值。

本批Windows受管浏览器的真实任务完成链为verified_scoped；随后继续暂停/一次Continue/Stop和
installed Chrome/Edge等原计划。缓存与旧包仅在原owners退出且新包验证成功后按精确清单清理。

已审查并删除2组精确过期App/Core增量代：逻辑2965722506 bytes，H空闲实测增加2884521984 bytes；
保留最新代和原Core/App验证产物，各件hash与source108复验一致。
另删除旧app102便携目录及ZIP：逻辑742418797 bytes，H空闲实测增加743448576 bytes，当前app109三件hash保持。
合计逻辑3708141303 bytes（3.45GiB），空闲实测增加3627970560 bytes（3.38GiB）；不是同一个指标，
硬链接和同期磁盘活动可能影响空闲观察。报告 `completion-app109-cache-cleanup.json`、
`delivery-app-109/previous-package-cleanup.json` 记录精确目标及重建方法。
旧包/缓存不在回收站，可用原source/config与cargo/stage/package工具重建；home、登录、会话、
证据、依赖、seed和fix.patch保留。旧working/未知跨变体缓存与失败runtime/profile继续延期。

完整四surface、三平台、两Mac架构、安装升级回滚、UX、真实模型矩阵与12h长稳未完成。
overallComplete=false，状态active / partial。

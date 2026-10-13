# 真实任务完成后的授权收尾

后续实际应用验收已通过：[app109真实Grok完成链与产物收尾](2026-10-05-computer-use-completion-app109-checkpoint.md)。
下文“尚未打包/仍为app102”是本源码批次结束时的历史状态；当前包为app109/source108，整体仍partial。

日期：2026-10-05；单 Codex writer，`feat/computer-use-implementation`。没有提交、推送或发布。
证据目录：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。
合同：[本批计划](2026-10-05-computer-use-explicit-task-completion-plan.md)。

## 原实机结果

app102 / Grok 4.7 / xhigh / ask，真人点击实际批准按钮后，自动保存“中文验证 你好”，
计数恰好从 0 增加至 3；20 条工具消息、0 工具错误。业务 effects 与模型回复一致。
App、原 CLI、MCP 和受管浏览器已退出，登录与会话保留。
原报告 `real-app-105-result.json`，详见[实际登录与首次聊天检查点](2026-10-05-computer-use-real-login-cold-chat-checkpoint.md)。

实机发现任务卡在模型完成后仍显示等待续行。原因有三层：Core 的明确 completion
没有模型入口；`awaiting_observation` 被写成等待续行；全 stopped 又遮住了 completed。
ACP 的 `end_turn` 同时用于等待批准与接管，不能据此无条件撤权。

## 本批实现

- 新增 `computer_complete_task({ taskId })`；ID 复制自 Host request/status，不接受其他字段。
- 原 transport / agent / task 身份在权威锁内复验；仅 pending 的申请不能完成。
  旧任务、跨会话、旧连接和伪造 ID 被拒绝，重复完成最后一个相同任务幂等。
- 同一任务的增量 scope 全部撤权，并使用原 fence / cleanup owner。回执
  `completion_requested` 只确认撤权，不伪造物理结束或业务成功；不关闭全局 feature。
- 模型说明要求核验结果后明确结束任务，再回复用户；等待批准、澄清和暂停仍可继续。
- App 的精确 MCP 权限处理允许这个减少权限的入口，真实 IPC 负责严格检查。
  不能走需要执行 grant 的普通 preflight，也不能落入 replay / YOLO / 通用允许策略。
- 卡片在清理阶段仍显示停止中；Host 确认全 stopped、无 cleanup pending、无 executable
  grant 后显示任务完成并移除 Pause/Stop。等待模型首次观察显示已授权。
  复用已有翻译键，15 locale 键集合没有变化，App / AppWorkbench 没有新增状态。

## 验证身份与结果

source106：3323 件，SHA-256
`f30d8d975fec0b7c564f858c025ae675613c0ccb701fcfd598041479cf003ee8`。
原 UI 两项失败、一项通过；原 Core 两项失败，分别证明等待文案/完成优先级和缺少模型入口。
首败 `completion-ui-106-red.json`、`completion-core-106-red.txt` 保留。

Core 默认 all-targets 1104 passed /0 failed /0 ignored；test-support 1120/0/0。
默认与 support 严格 Clippy all-targets 通过。
包含实际 loopback IPC、独立完成 admission、跨会话拒绝、全部 scope 撤权、原 cleanup 和旧任务拒绝。
日志 `completion-core-*-106.txt`；这些是 FakeAdapter / synthetic scope，不是真人页面验收。

source107 只同步遗漏的 TypeScript 工具目录；source106 的 Rust 输入逐件一致。
前端领域回归 34 文件 /381 项全部通过；MCP 39 项、typecheck、定向 ESLint、质量、fmt、diff 通过。
原目录不一致首败 `completion-ui-106-green.json` 保留，最终 `completion-ui-107-green.json` 为 exit0。
App 编排 13573 行、大文件 80 不增长。日志 `completion-*-107.txt`。

source108：3323 件，SHA-256
`588bfd39aef87bec04d755aa28180d8a44b9ad031ca0481fe80c60b7740d0248`。
相对107只改 ACP permission 的明确完成路由和定向 fixture；Core/MCP/前端源保持原验证输入。
最终 App 默认/probe 严格检查、默认 lib 编译均 exit0；精确 permission 3项和 delivery 4项通过。
原 delivery 首次过滤器拼写没有匹配测试，脚本明确 exit1；该空结果不算验证。
更正为实际列出的 `acp_client::computer_use::delivery::tests` 后，fresh
`completion-app-delivery-108-2.txt` 实际4/0/0、exit0，原空过滤器日志保留。
默认 App lib EXE SHA-256 `5224298f9c1eea34bc3c9bc83fe33bebca8afb3cf5097bdb1bf64fc431756714`。
最终source108复验3323件/hash不变；未重跑不受两处源差异影响的Core/前端全套。
lib 测试 EXE 不作为可运行的应用包。

## 保留的验收边界

当前用户包仍为 app102 / source101，EXE SHA-256
`5f44ffed1adf66923707deaa8c769085731ce23ca08cc25d0268d4b36561d5c8`。
本批收尾修复尚未进入该包，不能声称已经由真实 Grok 在产品中调用完成工具。
下一批构建 fresh 包、复用当前独立登录/home，以原 App/真人批准验证完成工具、原 browser
退出和卡片终态，再接暂停/恢复/Stop。保留原完整 E5/U04–U09/D00–D31、四 surface、
Windows/macOS/Linux、两 Mac 架构、安装升级回滚、UI/UX 与最终候选 12h；整体仍为 partial。

## 本批产物收尾

首次清理检查在定向测试仍运行时被 owner guard 延期，没有删除或停止进程。
原 owners 结束后使用 fresh `completion-cache-108-2-plan.json`，逐项审查15组精确旧
App/Core增量代；拒绝reparse/工作区外路径，并保护各变体最新代和原产物。
最终删除逻辑7115333177 bytes（6.63GiB），H空闲实测增加5519761408 bytes（5.14GiB）；
两者分列，硬链接和同时磁盘活动可能影响空闲观察值。
最新代、source108和保留产物hash复验不变；当前app102、seed、登录/会话、依赖、
fix.patch、全部dirty/untracked和失败证据保留。
旧working不完整缓存、跨变体未知文件和失败runtime/profile继续延期；没有删除源目录、
完整target、node_modules或其他工作区。已删缓存不在回收站，可由cargo重建。
收尾报告 `completion-cache-108-2-cleanup.json`。

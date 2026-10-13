# 真实 App 任务收尾修复

日期：2026-10-05。接续 source105 的真实 App / Grok / 真人批准结果；固定中文备注和三次计数已经通过。
本批只修执行状态与明确任务收尾，当前可用预览仍为 app102。其余四 surface、三平台、安装升级、UI/UX、真实任务矩阵和最终 12h 验收继续按完整计划推进。

## 已确认的问题

Core 的 `complete_task` 能撤销任务的全部增量权限并交给原清理 owner，但产品模型工具没有接入。
`end_turn` 也用于等待批准和暂停接管，不能把它无条件当作任务完成。
UI 将 `awaiting_observation` 错写成等待续行，并在全部执行已停止时遮住 request 的 `completed`。

## 本批实现合同

1. 模型新增 `computer_complete_task({ taskId })`。taskId 复制自 Host request/status；严格拒绝缺失、未知字段、伪造、其他会话、旧任务和旧 transport。
2. 完成入口只减少原任务权限，不能启用功能、批准申请、恢复执行或完成另一任务。仅 pending 的申请不能被标为完成。
3. 同任务所有已批准/准备中的增量 scope 一起撤权；同步 fence 后由现有 owner join，不在模型工具线程伪造物理完成。
4. 重复完成当前最后已完成任务幂等；新任务开始后旧 taskId 不得结束新任务。
5. 模型先观察核验业务结果，再明确完成，最后回复用户。等待批准、澄清和暂停保留原合同；完成回执是撤权确认，不是业务正确性证明。
6. UI 在清理中保持停止状态；仅 Host 确认全 stopped、无 cleanup pending、无可执行 grant 后显示任务完成并移除 Pause/Stop。首次等待模型观察显示已授权。

## 验证与边界

先记录原 UI 与工具路径的失败，再跑定向 Core/IPC/MCP/UI 回归、相关严格检查、格式和质量门禁。
保留首败、source/hash、当前包和登录会话；原 owners 结束后清理本批废弃产物。
不将 synthetic Core/UI 成绩写成真实 Grok/App 完成验收；新包的真人任务收尾仍需要单独实测。
不提交、推送或发布，本批结果留在原开发分支。

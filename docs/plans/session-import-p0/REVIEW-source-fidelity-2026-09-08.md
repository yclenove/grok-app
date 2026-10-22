# 会话导入：作业与来源记录保真复核

日期：2026-09-08。实现分支 `feat/session-workspace-import`，基线 `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。这是进行中的工程记录；原首版范围和 53 项验收仍保留，不能据此标为全部完成。

## 已修复的具体问题

- 导入先返回持久作业 ID，后台发布；输入来自固定预览，重启无需再读来源。相同 requestId 携带不同计划会报错。
- 发布 receipt 预分配 ID，重试恢复同一 revision。发布目录、catalog 和作业状态之间的中断可补完；没有逐文件向最终目录复制的降级。
- 复制按块检查取消。取消保留已提交项；撤销更新已有导入时恢复旧 revision，不隐藏整个来源条目，也不删除仍被引用的快照。
- UI 先订阅作业通知再拉持久状态；事件只是提示，丢通知可轮询恢复。旧作业请求返回不能覆盖新作业，关闭 UI 不取消后台任务。
- Codex 不再按相邻相同文本合并真实发送；仅将相邻、相反投影的 `event_msg` 与 `response_item` 配对一次，保留两个 source span。控制/回合记录结束配对候选。
- Claude 按内容块顺序保留文本、调用和结果；下一条记录的 `parentUuid` 连接前条记录最后一个块。修复纯工具记录缺父节点、混合工具结果与文本时结果消失的问题。
- 图校验实际进入预览和相关 adapter，检查重复身份、父节点缺失、冲突父链、环及分支内工具配对。遍历为迭代式，60,001 个节点没有 10,000/50,000 深度截断；子代理不能用主代理的同名 callId 冒充配对。
- 已提交归档读取不再静默略过损坏的 JSONL/事件/关系。来源的部分成功与应用归档自身损坏分开处理。

## 固定源码核对

核对版本：OpenAI Codex `6750f5bd1356fe1553c0fcc9f2632704f3055946`，只研究协议，未复制第三方实现。

- [rollout 序列化](https://github.com/openai/codex/blob/6750f5bd1356fe1553c0fcc9f2632704f3055946/codex-rs/history/src/rollout_payload.rs)：压缩记录名是 `compacted`，摘要在 `message`，还可能携带 `replacement_history`。
- [协议事件](https://github.com/openai/codex/blob/6750f5bd1356fe1553c0fcc9f2632704f3055946/codex-rs/protocol/src/protocol.rs)：`ThreadRolledBackEvent.num_turns` 表示回退用户回合数。
- [历史重建](https://github.com/openai/codex/blob/6750f5bd1356fe1553c0fcc9f2632704f3055946/codex-rs/core/src/session/rollout_reconstruction.rs)：回退移除最近用户回合，压缩替换历史与回退交互需要保留其语义。

新增解析 `compacted` 与 `event_msg/thread_rolled_back`：保留旧分支，将后续事件连接到保留的主链。不能仅因两种事件名称已支持，就宣称所有 compaction replacement、fork 跨文件来源和元数据版本都支持。

## 验证记录

- 基础修复后导入测试曾为 64 passed；本轮开始复核当前源码时为 89 passed，4.31 秒。
- Codex 重复发送、Claude 混合工具记录和长图校验修复后：93 passed、0 failed，4.72 秒。日志：`H:\aicoding\session-import-verification\migration-fidelity-tests.log`。
- UI 作业状态 hook：4 passed；覆盖先订阅、切换作业的旧请求、读状态途中通知、卸载途中完成订阅。
- 前端类型检查、修改涉及的 ESLint：通过。
- `compacted` / `thread_rolled_back` 和损坏归档新增回归后：95 passed、0 failed，4.85 秒。日志：`H:\aicoding\session-import-verification\migration-fidelity-final-tests.log`，编译日志为同目录 `migration-fidelity-final-build.log`。
- receipt 故障测试采用合成库 trigger，不等于已做断电或 OS 杀进程实验。没有三平台实机或真实模型验证结论。

## 仍需完成

持久来源实例、Cursor 多库/Store 兼容与跨源身份、完整附件存储、Codex replacement/fork 保真、预览与范围选择 UI、多根映射与空工作区、作业分页和租约清理、100MB/1GB 内存与性能、真实逐 open MCP 授权/能力探测、完整续聊发送与恢复、小窗口/键盘/15 语言以及原回归门槛。

其中性能压测和 Windows UI 属于当前可继续推进的工作，不应写成必须等待外部环境。macOS/Linux 实机和未可用的实际账号/运行时组合需要分别列出缺失证据，不能用合成测试替代。

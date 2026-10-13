# Codex 对 U00-R 的审查与 U01 准入条件

审查日期：2026-10-03。结论：**R1–R4 的设计通路基本接收；R5 仍需修复。**
这不是产品验收通过。当前只有 ADR、owned fixture 和文档，没有产品源码 diff。
允许下一批先完成下述证据修复，再进入 U01 的小范围核心实现；修复失败不得开始产品代码。

batch01-u00-rework 的原 exec5802 已返回 exit0，owner 精确记录 PID71624 的
UTC start `2026-10-02T15:52:53.3015361Z`、join `2026-10-02T16:14:56.6202391Z`；
实际原 PID 已缺失。不是仅凭 checkpoint 或 CLI exit0 判断开发通过。

## 接收的事实

已全文读修订 ADR、U00-R checkpoint、oracle、negative runner，并核对真实 permission 和 fixture 日志。
normal/default CLI 1.0.46 的权限消息有 `rawInput.variant=UseTool`、`rawInput.tool_name=server__tool`
以及 `_meta["x.ai/tool"].name=use_tool`。title 会改变，不是匹配依据。
Host 注册目录的精确比较是可实现的；真实同名 server 冲突仍要在 U02 检查，不能只认公开名字。

第二轮同 agent session `01a0fd60-1590-7ff2-8085-40d09473e420` 的两个固定工具各执行一次。
真实 update RPC 回执早于 prompt B 完成，overlap 后完成；cancel 后迟到 synthetic allow-once
以及未回答的 EOF 没有新增调用。CLI 76920 退出0，fixture 71156/57780 的原 PID 已独立读回缺失。
这些是 scoped CLI wire 事实，仍非真人 App grant、90秒产品审批或平台验收。

修订 ADR 的稳定 session transport → Host 当前 TaskGrant/ExecutionBinding 路由、owner generation、
feature epoch/reservation、tagged request/status/cancel 和 intent 去重可以作为 U01 起点。
旧日志和首败保留；原 pid67024 的强制终止不再被归因成产品崩溃。

## R5：九项谓词仍有三个可复现的误通过

审查时 oracle SHA-256：`bfe73b4038fdf90c1bcf9be87a0f6163c6b9c7c7be2c9352049513a21e51c6a7`。
Codex 只写了独立 synthetic 审查脚本，没有改 Grok 的 oracle，也没有任何真实批准或电脑操作：

```text
node tools/computer-use-probe/.run/cu-consent-orchestrated-20261002/batch01-u00-rework/codex-review/oracle-counterexamples-v2.mjs
```

正向 control 通过；以下三组不合格测试数据却都 `actuallyPassed:true`，审查脚本退出1：

| 数据变化 | 漏掉的约束 |
| --- | --- |
| late reply 后、EOF 前额外执行一次；EOF前后计数都改为3，日志也3次 | cancel谓词只比较 late reply 当下；没有覆盖直到真实关闭的完整区间 |
| 没有任何正向 fixture 执行；所有计数0、日志为空 | 没有要求两条正向调用真的发生，两个工具各一次 |
| 删除 process-exit；eof-closed 写 null exitCode / timeout | EOF谓词不要求原CLI真实终态，timeout也通过 |

原 negative 的六个变异会红，不说明这些额外反例也会红。保留原九项绿色报表，补充准确范围，
不能覆盖旧报表后宣称旧 runner 一直有效。
修复需用原真实记录重放的新 oracle 及全部负向；不需要为了纯谓词修复再刷真人或整套suite。
如改采集时序或fixture，则新唯一 attempt 保留原记录，实际运行受影响的 scoped fixture。

## U01 必须收紧的合同

1. **Pending scope 不能 commit 成 approved。** ADR第6节的“scope为Pending或Bound”不成立。
   Pending最多awaiting/approving；Host实际资源绑定和Bound身份成功后才生成可执行grant和续行。
2. **按工具验证身份。** status/list/open/首个observe没有现成snapshot/ref。
   generic回执不得要求所有工具已有snapshot而堵死第一次观察；也不能为此免除act/wait的身份合同。
   请求无需grant，内容读取需observe能力，输入需input能力；各工具仍核它实际使用的身份。
3. **transport issue epoch不是永久执行授权。** 首次批准会推进feature epoch，同一个transport必须仍能
   走Host当前grant；不得因为token签发时epoch旧就让稳定连接失效。grant/reservation用当前epoch严格检查。
4. **完整owner建立流程。** U02的session/new目录建立早于CLI返回agent session id。
   使用Host预留incarnation/generation及随后原子绑定，或严格等效流程；不以空id永久跳过agent核验。
5. **决定通知分型。** approve后的grant continuation与deny/cancel/expire的无权限通知不能互相冒充；
   拒绝不生成执行票据。U01提供类型/一次性账本，实际prompt队列属于U02。

这些是对本批计划的细化，没有改变默认关闭、Host-only用户决定、无范围外读取、Stop先fence后join、
unknown不重放等要求。跨平台U07/旧D00–D31仍开放，不因为U00结束就关闭完整目标。

下一任务：[U01 核心实现派发](2026-10-03-computer-use-task-consent-dispatch-u01.md)。

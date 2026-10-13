# Codex U01 终态审查

结论：**partial / needs_rework；不派U02**。P0 oracle接收，其余已有基础可保留，
但任务授权核心仍违反已冻结合同。不是因为缺UI或原生机器而拒收。

## 已核实际终态与验证

batch02-u01-core 原exec32341退出0；owner精确PID46440、UTC start
`2026-10-02T16:18:20.4284739Z`，join `2026-10-02T17:11:23.9348266Z`。
回读实际PID缺失、同workspace/session无其它Grok writer；HEAD仍8d8b71c2。
已读最终ADR/checkpoint、完整相关实现与测试，以及tracked diff和untracked产品文件。
`git diff --stat`不统计新增文件，不能据其211行误报本批总代码量。

Grok原日志经Codex读取核对：core lib **602/602，0 ignored/filtered**，同一包级命令
其余十个测试binary均通过（27/33/21/17/24/18/9/20/14/21）；最终EXIT0。
strict core Clippy EXIT0、golden Node5/5。原首红/编译失败/双commit红均保留。
core-suite hash `a709b950ad720235ac681b7f6b8f02b661dc8ef7ceac884fa4055fddc0e06884`，
clippy hash `a044f27143378ee4e0efe5712ec53de2578e25fd4193e32412fd2666b710903c`。
这是Grok执行、Codex核原日志，不声称Codex另跑602项。

Codex实际独立执行：MCP目录全部测试 **37/37，exit0**，`cargo fmt --all -- --check`
exit0、`git diff --check` exit0。没有改产品文件，也未跑真实App批准/系统许可。

## 独立反例：7个实际断言失败

使用ignored `batch02-u01-core/codex-authority-review-20261003-attempt01/`，
原文snapshot SHA256与终态live `task_consent.rs`完全相同：
`7548ee2de344e9a43c0ed9a8a411c9bcb173be2cbdc9a1e7aec33485a0da01be`。
`broker/consent.rs`终态hash
`e5ac1f8395f3fe48fea8dae257abafb855b60dd5297e7c83475c261aa49b24d6`。
独立crate只运行原文ConsentState，SurfaceKind/BrokerError来自实际core；没有adapter效果。
正向control通过；**1 passed/7 failed/0 ignored/0 filtered；真实Cargo101**。
命令、范围与解释见[preliminary增量](2026-10-03-computer-use-task-consent-u01-preliminary-review.md)。
`red-authority.txt` hash `4d63ff627004b16ae1e3f68fd40ec7e978350c78c886f4439b15e2404e344cc9`。

| 失败 | 终态源码入口 | 要守住的行为 |
| --- | --- | --- |
| prepare已经报approved | task_consent.rs:709；broker/consent.rs:69 | 最终绑定及复验成功才发布grant/continuation |
| retire/reissue同agent后接受旧ctx | task_consent.rs:398、1245 | 新owner身份不重用；旧捕获context也拒绝 |
| None agent可prepare执行grant | task_consent.rs:709、1245 | 预留transport不能获得未绑定agent的执行权限 |
| 申请B即撤销A | task_consent.rs:1101、1262 | 新增范围待决定/拒绝/失败保留原commit范围 |
| rollback返回epoch0 | task_consent.rs:864；broker/consent.rs:264 | bool回滚也推进epoch，不能退回旧世代 |
| 能力变化留两张active卡 | task_consent.rs:1101 | 同Host任务唯一pending请求及修订，旧卡失效 |
| pending意图写入scope memory | task_consent.rs:1286 | 申请历史与真正批准范围记忆分型 |

增量反例先确认当前A execution authority存在，再申请B看到它消失；A由错误prepare发布，
没有声称成功native执行。修复后的broker回归必须用真正完整commit得到A。
状态断言先失败后，其后count/ticket断言可能未到，不将其声称为额外独立运行失败。

## 还需修复的静态结论与缺失回归

以下是最终源码推导，不冒充已运行native/cleanup反例；U01-R要以barrier/owned fake验证。

1. 准备完成后没有final commit gate。`plan_still_valid`只看epoch/grant；检查之后到open_run
   的Stop以及bind之后的退休仍可错过。`already_committed`也可能在第一批准尚未绑定时
   向第二批准返回成功。需真实两阶段commit和原准备资源owner。
2. `bind_agent_session`只操作ConsentState；complete/detached只drop target/snapshot，
   不取消run.cancellation、不fence或join。旧输入/lease/资源可能仍活，rollback留下候选run。
   `consent_stop`同步finish全部cleanup，违背有界Stop确认。复用现有fence→cleanup handoff。
3. transport在`execution_run`后再classify，随后变成只含session/run的legacy binding；
   实际Broker准入没有完整task/agent/grant revision gate。需撤权发生在查grant与派发之间的回归。
4. `Decision::Continuation`只有app/task/revision/id/consumed；take只收app并返字符串，
   没有request/agent/incarnation/connection/epoch和领取复验。U02不能再建第二权威账本补救。
5. reservation上界实际无效：reserve→deny/cancel/expiry的consumed=false记录永不清除。
   MAX_LIVE per-owner、MAX_REQUESTS全局与全局活请求不匹配；scope记忆会越过owner退休累积；
   IPC retired transport的rate桶清理也缺闭环。有效未消费continuation可能被全局pop_front挤掉。
6. idle TTL只是查execution时重置，不区分实际长action/物理cleanup；到期删除grant未交出
   原run收束工作。不能用下一次tool调用清target代替物理收尾。

ADR的U01新增说明允许第一次bind_agent收养None grant、checkpoint写bind失败epoch0、
未批准intent算scope记忆，均不是可接收的新合同。修订ADR必须与上述正确边界一致。
已有resume测试加的是pending意图，未验证“已批准scope跨暂停保留”；需修成有权限的场景，
原旧snapshot/ref拒绝断言继续保留。

## 接收与下一批

接收：P0 oracle修复及其10/10重放/九负向；严格tagged请求、无run且feature-off的HTTP
pending路径与零adapter效果；transport/legacy隔离的基础、schema/golden目录一致、
现有core/Clippy/fmt/MCP回归证据。未接收：上述执行权限与生命周期核心合同。
synthetic Bound的限制在U01范围内合理，不因没有生产resolver本身判bug。

下一批：[U01-R dispatch](2026-10-03-computer-use-task-consent-dispatch-u01-rework.md)。
先R1事务/epoch、R2身份，再R3准入、R4资源收尾、R5任务/typed票据、R6内存。
每项先红后绿，不改独立原snapshot。修完再验收U01，之后派U02会话预连接/CU-only
CLI回执/续行队列、U03聊天卡；U04–U09与D00–D31仍保留。未提交、未发布。

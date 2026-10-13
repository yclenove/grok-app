# U01-R2 实施中审查

终态补记：batch04 已 max180 / exit1 / join，原 writer 缺失。此文件下方 running/draft
描述是历史现场；五个独立 draft 后来已保存165文件副本并尝试执行，因产品E0277未进断言。
以 [U01-R2终态审查](2026-10-03-computer-use-task-consent-u01-rework2-codex-review.md)
为当前结论，不把旧阶段绿或下文尚未执行说明当当前状态。

状态：**running / preliminary**，不是终态验收。当前批次为
batch04-u01-core-rework2，原PID38388/start `2026-10-02T18:44:00.9471359Z`，
原exec11754。每次恢复先核实时进程，观察超时不认定退出，不启动第二writer。
Codex只读产品源码/原日志、写计划与审查文档，不接管实现。

## 实际先红已读取

Grok把四条独立反例及正向控制迁入当前产品core测试：
`broker/tests_task_consent_rework2.rs`四条，`broker/consent.rs::rework2_gap`一条。
未改旧batch03独立review snapshot、tests或red结果。

原日志 `tools/computer-use-probe/.run/task-consent-u01-20261003/red-u01r2-authority-1.txt`：
**1 passed / 4 failed / 0 ignored / 618 filtered，EXIT=101**，实际进入断言。
SHA256 `fec41177eebc90632c0226e7b488104a5f23957561dd86ba01ef5cc8104c78b5`。
这是Grok执行/Codex独立读取，不是Codex重复跑产品测试。

- 正向：批准observe范围，真实FakeAdapter读取成功；合法完整action请求被能力拒绝。
- F1：B只允许input，借A的observe实际读取B一次，返回B targetId/isError:false。
- F2：feature原本on，最后check后Stop，迟到bind再abort，实际run仍Running。
- F3：96轮顺序issue/bind/request/retire在index32失败，live transports/grants/preparations0。
- F4：29分钟后真实模型读取成功，再2分钟grant被清零，仅空闲2分钟。

以上是修复前证据，不当作最终版本失败；等终态新源码/hash/绿回归再验收。

## 本轮已读取的阶段性绿灯

`compile-u01r2-codex-1.txt`为中途编译失败（Rust借用冲突），未进断言，原日志保留。
随后`compile-u01r2-codex-2.txt`实际5 passed/0 failed/0 ignored/618 filtered；
writer原wire明确`EXIT=0`、timed_out=false。SHA256
`a20091e89fc92fae29e09bc319093be4c4eac4eb8f460c0cd3f944bbe7a7c83d`。
`compile-u01r2-consent-1.txt`实际32 passed/0 failed/0 ignored/591 filtered，wire同样
真实`EXIT=0`。SHA256 `98e40545d3783db155e86f7d5826cc5566acdde9157fdd53e70c4c7205a7f11f`。
Codex独立读取，未在live工作区重跑。五项原断言初步绿，不代表最终源码已冻结、
补充竞争/完整suite/strict Clippy/独立副本或U01-R2接收；writer仍在实施。

## 最终审查范围

迁入的五项是起点，不能只修这五条表面结果。终态还核：

1. F1完整permit/真实成员/能力进入最终Broker准入；两目标同时活、A退休、两个方向的
   能力隔离。新增B pending/deny/cancel/bind后失败保留A；用合法payload和真实effect。
   不以禁用增量、只留单grant、每路回退legacy run来让反例绿。
2. F2 feature-on/新enable两条，真正最后check→open、open→note、bind后、finalize前的
   竞争。原预约/candidate资源责任保留到terminal；finalize容量失败要清理/回滚。
   grant0或Stop回执不是cleanup/lease/join证明，测试实际消费票据。
3. F3 retired slot安全回收，同时保留未终态准备/action/cleanup的容量；旧捕获ctx及
   token不能复活。实际start_preparation正反控制，不能继续用恒Err的reserve。
   owners之外retired_outbox/cleanup_tickets等真实容量和消费者也核，不静默丢责任。
4. F4实际工作结束后的空闲边界，长action/cleanup/unknown、preview/status轮询和
   退休后的迟到completion；busy独立快照不能与新准入竞争导致错误退休。
5. U02需要的typed NoAuthority通知与Continuation调度接口：精确owner绑定、一次消费，
   否定通知不授予执行；当前reason字符串数组不证明可安全用于生产自动续行。
6. 2026-10-03本轮实施中补查两个返修关联分支，终态按最终源码复核，尚非实际失败：
   当前prepare开始改为每request独立run，而complete_task仍返回一个run并删除整task的
   grants；若最终仍如此，需证明全部原owned runs都被fence、cleanup和收束，不能只清
   权限记录。submit_access/begin_model_call中sweep后使用`?`的错误路径也要消费到期
   run交接；仅一次到期后被拒的模型请求、再无后续成功请求时，原run不能仍Running或
   继续占lease。这两条属于F1/F4关联终态检查，不提前宣布当前writer遗漏或接管修改。
7. TTL测试要分清“推进时间”与“执行sweep”：现有advance_consent_clock同时sweep，
   不能据此证明无人轮询时的真实批准/续行准入。终态检查start_preparation、finalize和
   validate/consume_continuation对monotonic deadline的直接复验；跨申请期限的慢prepare
   不得commit，仅空闲过期后的旧claim也不得续行。Host定时扫描/通知消费者归U02/U06，
   但core准入不能依赖它恰好先执行。以上是当前源码关联审查，尚未执行反例。

Codex已为第6/7项在当前batch04的ignored `codex-review/`准备五个独立draft断言、
raw-clock test-only seam、capture/run脚本与隔离Cargo配置。三个为authority-only
synthetic检查，两个为实际FakeAdapter读取和精确cleanup；**尚未snapshot或运行**。
两脚本解析0 errors，Rust draft已rustfmt，仅证明工具准备，不是产品回归通过。
writer终态后按最终API复核，仅在新副本加载两测试module；原产品算法与batch03证据不改。

本批接收后按最终API复核U02，再实际派发。后续依赖顺序已更正为
U02→U04→U03→U05→U06→U07→U08→U09：生产范围先于卡片，编号和总范围仍保留。
完整D00–D31未缩减，局部核心/mock绿不等于产品或三平台交付。

# U01-R 实施中审查记录

状态：**running / preliminary**。不是终态验收，不据本文件启动第二writer或派U02。
历史记录已终态：见[U01-R终态审查](2026-10-03-computer-use-task-consent-u01-rework-codex-review.md)。
下述live、编译和锁死描述为实施中时间点，不是当前进程/代码结论。
Grok batch03-u01-core-rework，原PID7892/start `2026-10-02T17:15:44.1545314Z`，
原exec15322；现场live需重新核实际身份，不能用本文件代替进程检查。
本轮Codex只读产品源码/原测试输出，并编写后续dispatch与审查文档。

## 已读真实先红输出

Grok实际运行，Codex独立读取原日志；不是Codex重跑，不是完整suite或真人授权。

| 日志，相对repo根 | 实际结果 | SHA256 |
| --- | --- | --- |
| tools/computer-use-probe/.run/task-consent-u01-20261003/red-u01r-authority.txt | 1 passed / 12 failed / 0 ignored / 602 filtered，Cargo101 | 114587e84d681c2b1021117c8be58982fe7cca6d6ec92fee0af4d5bbc6aee76c |
| tools/computer-use-probe/.run/task-consent-u01-20261003/red-u01r-authority-2.txt | 0 passed / 13 failed / 0 ignored / 602 filtered，Cargo101 | 4fb8e7ce8c91d03de3bfde47c76ce1e7e70bb90bd56563129ea2945f11aaa51a |

第二轮增加准入与adapter间agent replacement竞争，观察tool实际返回isError:false，
表明旧transport经run路由仍到达观察；不是仅编译失败。旧独立snapshot/七反例未改。
同批其它真实红包括提前Approved、重复批准提前成功、epoch回退、Stop后迟到run、
旧ctx复用、None agent、pending intent记忆、reservation积累、有效continuation丢失和慢Stop。
这是pre-fix证据；最终必须复查新源码和绿回归，不能重复拿旧红拒绝已修复版本。

## 最终审查要注意的测试范围

当前`broker/tests_task_consent_rework.rs`在本次读取时SHA256为
`e40771b04f31346e0ef24c7d92558be7f185e5c88c69b5c25041845b9ad4c50a`；
live writer会继续修改，以下是实施中提示而非最终缺陷结论。

1. `retire_does_not_free_a_global_owner_slot`把安全上界表达成“退休后永远不能补一个owner”。
   合同要求实际live/retained数据和资源全局有界、incarnation不可复用，并非进程一生只能
   打开32个聊天。正确实现可在原资源终态、旧token/ctx可靠失效后安全回收slot。
   最终需同时证明flood有界和正常退休后继续新会话；不能靠永不回收owner让测试绿。
2. `pending_intent_is_not_scope_memory_and_incremental_keeps_a`目前只测B pending时A能读。
   B deny/cancel/failed bind后的A保持、正确task边界、完整合法input对B的拒绝还需覆盖。
   空JSON的computer_act可以因schema无效失败，不独立证明B的capability gate。
3. `stop_ack_does_not_wait_for_slow_abort`目前只等待回执、释放fake abort并join两个线程。
   最终需实际消费StopCleanupTicket，核旧action cancellation/admission/lease，慢原owner
   未terminal时新lease被拒绝，收尾后才可新执行。丢ticket后的join不能当cleanup闭环。
4. 首个prepare检查按顺序assert；旧state断言失败后，后续count/ticket断言未执行。
   最终绿色或分立反例才证明所有条件。Stop-before-open和after-bind也需各自独立证据。
5. typed票据绑定/claim/send准入、complete/expiry/rotation的物理收尾、真正idle计时及
   全局memory/rate上界仍按R1–R6逐项查源码/测试，不因这13条全部绿就自动接收。

## 2026-10-03 02:20 追加观察（仍非终态结论）

原writer已执行第一轮新模块编译，`compile-u01r-check-1.txt`为实际Cargo101：
类型推断、返回类型和MutexGuard字段借用未通过；尚未进入新版本绿断言。
不把这份实施中编译输出当最终交付失败，等writer修完并终态后重新审查。

- 当前`broker/consent.rs`四个prepare hook仍直接置于Rust2021 `if let` scrutinee的
  mutex临时guard下；回归hook会重入Broker，最终必须确认取出hook后已释放锁，
  并在有界测试中证明竞争断言真实完成，不能用被杀的harness或等待超时宣称通过。
- `owner.rs::issue`目前对包括retired记录在内的`owners.len()`限制32，尚未看到
  安全回收路径；最终同时验证超过32个顺序聊天可继续用、旧ctx不可复活、实际存储有界。
- `prepare.rs`把增量scope映射至同一task run，最终要核新增B成功及失败时的实际target/
  consent绑定与A的关系。不同scope能力不得借同run或相同revision跨用；仅测缺失target
  的失败不足以证明真实目标已绑定后的失败可回滚。
- `commit_access`最后在`finalize`后条件写run.consent；最终需在发布前复验真实run/target
  及stop状态，finalize容量失败也必须收束原预约/候选和本事务feature迁移。
- 当前idle_deadline只看到在finalize初始化，busy集合是在单独锁段采集；最终要验证
  真实活动完成后重新开始空闲计时，不能把运行30分钟等同于空闲30分钟。
- 测试专用reserve目前在身份匹配后也总返回Err；旧ctx拒绝不能借这一恒拒绝路径
  作正反控制。正式start_preparation须有当前ctx成功、旧ctx失败的同条件证据。

以上是最终审查清单，live源码会改变。Codex没有修改产品源码、旧独立快照或原失败日志。

02:24追加独立语言运行证据：当前core edition为2021，独立小探针
`batch03-u01-core-rework/codex-review/guard-lifetime.rs`由`rustc --edition=2021`编译并
实际运行exit0。if-let scrutinee control中`try_lock`失败，独立let control中成功；
它只验证语言锁生命周期，不导入产品代码、不操作真实桌面。
探针SHA256 `d5085a395c9fb831185f0e1025b72f0ff2b1094d73e34beb9590b0574257662b`。
当时`broker/consent.rs`SHA256
`23aa6e51077c7d0f9f5746b97da5bb59bfc97bd393778c812f85bea0029f0c83`。
Grok原`green-u01r-consent-1.txt`已进入28项断言，其中4项报告超过60秒仍运行；
尚无最终result/EXIT。不把24条ok或文件名green当整个suite通过，不终止或重启live writer。

继续等原writer终态，读取checkpoint/全diff/内层退出和原resource收尾后，写终态review。
U02/U03/U04为未派发draft，上批接收后按真正API复核再用原launcher启动。

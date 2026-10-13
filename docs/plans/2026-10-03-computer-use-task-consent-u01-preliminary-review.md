# U01 实施中的 Codex 只读观察

状态：**preliminary / not final acceptance**。Grok batch02-u01-core 仍在写代码。
本文件不判定最终交付失败，不修改产品代码，也不启动第二 writer。
等该批真实退出后，复查最终源码、测试与这些场景；已自行修好的条目不再重复派修。

观察入口：`computer-use-core/src/task_consent.rs`、`broker/consent.rs`。
观察时 hash 分别为 `aa75a232d79b09936e803494dc04797cbf675a9effc218db21321e60ed69bbab`
和 `1dda24d2cf75a6724604404a662510da71af3b318f796b8b0048e081e1fb5774`。
后续继续变化；以下是源码推导的场景，还没有宣称独立执行通过或失败。

## P1：批准和续行在目标绑定之前发布

`ConsentState::prepare_commit` 把 request 写为 Approved，建立可执行 GrantRecord，
并 push Continuation。Broker 随后释放锁、调用 after-enable hook、检查 plan，才
`open_run` / `authorize_desktop_window`。该窗口内 status 可报 approved，票据可被领取，
目标实际还未成功绑定。bind 后也没有最后一次原子提交复验。

验收用 after-enable 或 binding barrier 暂停准备，另一线程查 status、执行 grant count、
take_continuation；应仍为 preparing/pending、无可执行grant、无续行票据。
再分别令绑定失败、Stop/feature-off/owner retirement胜出，不能返回批准成功。
还要覆盖第一次 plan_still_valid 检查之后、open_run之前的Stop，防止旧准备新建未被fence的run。

## P2：丢弃执行身份代替了撤权和物理收尾

`release_detached_task_runs` 对失去 grant 的 `cu-task-` run 只调用
`drop_execution_identity`，没有取消原 action cancellation、fence 或原生 abort/join。
complete_task也是直接drop；rollback移除grant但没有收束可能已经open的候选run。
consent_stop则在同步入口直接调用finish_stop_cleanup，可能把Stop确认和阻塞清理合在一起。

验收用可控慢action/慢cleanup的owned fake adapter：在撤权、expiry、owner变化、complete和
绑定中途失败时，旧动作要收到取消，旧lease/占用在原资源确认结束前不能释放；
先fence、单独cleanup handoff，Stop确认不能被慢abort阻断。新owner不被旧cleanup误清理。
不能靠清snapshot或读到grant_count=0就判定物理收尾完成。

## P3：owner退休后的generation ABA与未绑定agent

issue新Owner每次都初始化incarnation=1 / connection_generation=1；retire_owner删除Owner。
require_current_owner只比较这些字段和agent Option，并接受(None,None)。
因此捕获旧TransportContext后retire→同App session重新issue，旧context可与重建的1/1/None匹配。
submit_access/execution_run在classify bearer和获得状态锁之间有真实竞争窗口。
prepare/commit目前同样能在agent未绑定时进行，grant_agent_matches接受(None,None)。

验收保留旧ctx，在retire/reissue并绑定同一个持久化agent session id后调用旧ctx的
request/status/cancel/execution，均应拒绝。新owner身份必须不可复用；完整owner的最终agent绑定
不能靠空id永久跳过。预留transport如何供session/new使用与真正模型调用/commit资格分开。

## P4：同任务增量申请会撤掉原grant

open_request每次创建新task_id和revision=1；family_key不含scopeIntent。
retire_family对相同purpose/surface/capability且Approved的旧申请也执行退休，并删除旧grant。
这意味着同任务申请新增Excel可先撤掉原Chrome范围，与“新增范围拒绝/准备失败保留原范围”冲突。
MAX_LIVE=8还允许同一个owner出现多条活跃pending，status/cancel却只操作最新一条。

验收同任务已有Bound grant A，发范围B申请后批准前及deny/failed后，A仍能按原能力执行，
B始终不可执行；旧卡revision失效，活跃申请去重且唯一。Host显式新任务才退休原任务。
当前remember_scope发生在request阶段，记录的是未批准intent；批准范围的非执行记忆
必须和请求历史分开，不能把从未批准的scopeIntent当作用户曾允许的范围。

## P5：continuation缺完整owner/revision合同

Decision::Continuation只有id/app_session/task/revision/consumed，没有agent incarnation、
connection generation、feature epoch和requestId。take_continuation只收App session并返回字符串，
没有领取时完整绑定复验。不能让U02再另存一份无法核对的权威票据。

验收票据在实际commit后生成，typed payload包含原绑定；领取和发送前复验，
旧agent/epoch/revision/Stop票据零发送，busy只排队，不丢失也不双消费。
deny/cancel/expiry保持无执行权限的独立通知类型。

## P6：epoch和有界reservation

rollback返回previous_epoch并令Broker把epoch恢复旧值，违反monotonic epoch合同。
补行为测试：预约→enable→绑定失败/rollback后，旧epoch预约不得因回到旧数字而重新有效；
回滚自己的bool迁移也推进epoch，且不能关闭更新用户epoch开启的功能。

trim_reservations在len超界时只retain(!consumed)。反复reserve→deny/cancel/expire的reservation
保持consumed=false，旧request即使被trim，reservation仍可累计。验收真实循环超过上限，
证明存储有界且不丢失当前取消事实；不能只在注释或常量声明中说“有界”。

以上按最终源码和效果核实后决定返修项，不将源码推导冒充真人、App、native或跨平台证据。
完整U00–U09和D00–D31仍开放。

## 独立 E1 反例增量：7 条进入真实断言的失败

Grok 仍 live，当前不启动第二 writer。Codex 将 `task_consent.rs` 原文复制到 ignored
review crate，复制前/快照/复制后三个 SHA256 一致：
`7548ee2de344e9a43c0ed9a8a411c9bcb173be2cbdc9a1e7aec33485a0da01be`。
没有编辑产品源文件。review crate 只调用这份原文的 ConsentState；SurfaceKind/BrokerError
从实际 core crate 导入，所有 scope/decision 均为明确 synthetic，无 adapter/真人批准/内容读取。

位置：`tools/computer-use-probe/.run/cu-consent-orchestrated-20261002/batch02-u01-core/`
`codex-authority-review-20261003-attempt01/`。`src/lib.rs` 是独立反例，
`src/task_consent.snapshot.rs` 是保留原文，`red-authority.txt` 保留实际失败。

实际命令（repo 根）：

```text
cargo test --manifest-path tools/computer-use-probe/.run/cu-consent-orchestrated-20261002/batch02-u01-core/codex-authority-review-20261003-attempt01/Cargo.toml --target-dir src-tauri/target --offline --lib -- --nocapture
```

进入断言，**1 passed / 7 failed / 0 ignored / 0 filtered；Cargo exit101**。
正向 control 核实严格 request 去重、未批准 grant/continuation 为0、deny后 reserve拒绝。
不是编译失败、libtest环境错误、真人或 native 验收。

| 反例 | 实际违约结果 |
| --- | --- |
| 旧ctx → retire → reissue → 绑定同持久化agent id | 旧ctx request被接受 |
| agent未绑定 → synthetic Bound → reserve/prepare | None agent可建立执行grant |
| prepare返回候选，尚未调用任何目标/run绑定 | status已经是approved |
| 已有A authority → 申请同purpose/capability的B | 未决定B就撤销A execution |
| 自己enable到epoch1 → bind失败rollback | rollback返回epoch0 |
| 同scope改能力observe→observe+input | 两条live request，不是唯一修订卡 |
| 从未批准的pending intent | scope_memories已记录它 |

其中增量反例的A通过当前prepare已经发布的authority建立，未声称已执行adapter；
修复后必须用完整commit后的A作真实broker回归，不能照抄该旧准备步骤作为新正向。
其它 grant count/continuation 后置检查可能被前一个失败中断，不能外推成已单独执行。

随后读取的live source仍为同hash，broker/consent.rs hash
`e5ac1f8395f3fe48fea8dae257abafb855b60dd5297e7c83475c261aa49b24d6`。
Grok自身定向13项/core全suite/Clippy/golden报告绿不覆盖这些反例。
最终终态仍需重核hash；若自行修复，基于最终源重新构造对应回归后再判，不重复派旧问题。

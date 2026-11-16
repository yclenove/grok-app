# S11-M4b：machine 集成设计

修订记录：2026-11-15 round-3 controller 修订：每步至多一次发布的成本契约与 S0/S1/S2 顺序，联合界改为 A=2346、D=2446、4792<5000（余量 208ms）；round-1 补齐 D1/D3～D5 并移除 ACK 工作门；round-2 controller 决定以既有 `RENEW_MS=800ms` 为 inventory 最小发布间隔、每个 fresh pin 仅触发一份下一服务 step 内优先发布的 lease，关闭 D2-r2 请求数开放问题（[review-r2](reviews/2026-11-15-s11-m4b-design-review-r2.md)）。
日期：2026-11-15；状态：待独立设计审阅，本文不授权真实接线。
基线：`3e3c390b`（M3、M4a、C11、`g+5s` 已合并）。下文源码路径相对 `computer-use/`，`tart/` 指 `crates/gcu-eval/src/disposable/tart/`。
依据：[ADR 0019 §3/§6](../../../computer-use/docs/adr/0019-tart-mailbox-channel.md)、[M4 一审](reviews/2026-11-13-s11-m4-review.md)、[二审](reviews/2026-11-13-s11-m4-review-r2.md)、[M4a 三审](reviews/2026-11-13-s11-m4a-review-r3.md)。
废弃分支 `cu2/s11-m4` 只供反例定位，不整段移植；本卡负责 R2-2～R2-8、R2-11、R2-12，以及机器侧验收后消费。

## 1. 最小执行机制与截止

保留单一同步调用方；在 `Attempt` 内保存枚举游标、固定输入、已发记录和不透明句柄。没有线程、回调、observer、自动续租或 Drop 清理。
现有命名入口（reserve/clone/start 等）初始化对应游标并推进一片；与后续统一 `poll(caller)` 都返回 `Result<StepProgress<CompletedStep>, StepError>`，明确区分 `Pending` 与 `Done(CompletedStep)`。
`CompletedStep` 是闭集：`Reserved, Cloned, Started, Isolated, Bootstrapped, LoggedIn, Ready, Ran, Revoked, Destroyed, Cancelled, ProviderCrashNoted, RuntimeLossNoted, OwnerLeaseObserved, Sealed(AttemptReceipt), Recovered(Vec<AttemptReceipt>)`；各入口只交回对应变体。
完成值以 move 随 `Done` 交付一次并清空游标/结果槽；recovery 的回执向量跨 yield 保存到全部完成。无活动游标的 poll 返回 `StepError::NoActiveStep`，无 IO、无重复写入/回执；Pending 不表示阶段完成。
新命令不能覆盖未完成游标；重复入口只允许相同输入，不能重发。取消/Stop 是优先状态转换，保留被取消命令的句柄。

每个公开推进入口只创建一次 `StepCtx { start, end = start.checked_add(100ms) }`；内部推进不能再开 step。
`machine/io.rs` 是唯一调度入口，owner 服务、锁、账本、runner、guest、codec/parse、消费记账都接收同一上下文，实际调用截止只能更早：

```text
op_end   = min(首次 caller, begin_time + cap, phase_end, aggregate_end, attempt_end)
call_end = min(step.end, 本次 caller, saved op_end, 当前适用的其它旧截止)
```

`op_end` 在命令首次 begin 时保存一次，与 step.end 分开；新的 poll/caller、accepted、inventory 均不能延长它。
prep 起于 reserve 入口（含锁/读回）；start 起于卷准备入口；Task aggregate 起于首次 run；cleanup 起于首次撤权/取消。query 的短 cap 仍交所在 bootstrap/login/task/probe-group/cleanup 的旧 end。
stop 的确认共用 stop end，delete 的缺席查询共用 delete end；不能查询一次就重新获得整份预算。checked arithmetic 溢出拒绝派发。
即使 `now >= step.end` 也绝不移除 clip：只做不阻塞的本地到期/Stop 锁存，返回 Pending 或失败；不能继续锁、解析或 IO。
调用方须在所有阶段（包括长命令、清理）每 ≤100ms 再推进，机器不能用内部循环替调用方补 tick。

| 现有路径 | 必须持有的可续跑阶段（每个箭头均可让出） |
| --- | --- |
| reserve | 固定输入/首证明/调度 prove → try-lock → ledger.rows → runner.list → ledger.reserve → 必要的精确 query 读回 → 保留唯一 permit |
| clone | list 确认目标未占用 → note_identity/读回 → begin_clone（移动 permit 一次）→ poll → parse/核对 incarnation/config → CloneIssued/读回 |
| start | list/status 核对归属 → StartIssued/读回 → begin_start → poll → Booting；start 意图后即视为可能已启动 |
| isolate/ready | inventory begin/poll/解析 → 身份绑定；probe 保存索引、当前 seq/end、组 end，一次推进一个 probe，绝不串行跑完 16 项 |
| bootstrap/login/run | 固定 op/实发 seq/end → 同槽 pending/accepted → inventory begin/poll → 机器验收；槽回复本身不产生 ACK/login/score |
| revoke/stop/delete | revoke 槽与 drain 分开；stop/delete 各自 begin/poll，独立退出/缺席查询另设子游标，不能串成一次同步调用 |
| seal | 判定屏障/回执 → 必要的隔离事实写入/读回 → seal begin/poll → 丢 ACK 的 sealed 读回 → 发布回执 |
| query/recovery | list/status/ledger.rows/query 都有 begin/poll；列表解析及 recovery 逐行推进，不重新扫描到稳定 |

`TartRunner` 的固定操作改为内部 begin/poll 及 cancel/reap 接口；句柄绑定命令实例，不接受任意 argv。`AttemptLedger` 同样提供有类型的读写 pending 句柄，写入与读回是两个阶段；不加阻塞兼容默认实现。
`ClonePermit` 未派发时留在游标中；begin 接管后即使回复丢失也不能取回/复制。reserve 丢 ACK 只可读回该行，不重造 permit，不重发 clone。
guest 保持七方法、版本 1：现有同槽重复调用即私有 begin/poll；machine 保存 op、实际 seq、pending/accepted、原 end。普通副作用最多一个，inventory 独立，lease/revoke 优先。
M4b-1 即接入既有 normalized `Pending/Accepted/Terminal` 分类、begin-once progress/effect 记录与冻结 end，并迁移既有公开阶段路径；accepted 仍未完成，terminal 须交阶段验收，分类本身不产生成功。成本 fake 使用同一回复语法、真实实发记账及跨片剩余成本；完整语义验收/真实组合等留在 -2（§10），不得靠即时 legacy 成功绕过 -1 的公开路径回归。
跨步只保存有界自有字节、解析位置、身份和句柄；账本 rows/list 也按有界条目推进。及时读到但尚未验收的候选可暂存，下步仍须重新过全部验收门。
邮箱文件 FD 不跨 poll；每次读重新打开。一次 tmp→rename 发布保留源 FD 至该次有界调用结束，不把半个发布伪装成可恢复的文件 cursor。

片中预算用完分三类：未派发/有句柄且正常返回 Pending，保存进度并让出；只读候选迟到，丢弃后可在原 end 内重读；已派发写入/副作用无法确定完成位置，进入 unknown。
合作式 poll 在预算内返回 Pending 才是让出；`Expired` 不能独自证明“未派发”。内部必须另有 `NotDispatched / Issued(handle或seq) / DispatchUnknown` 证据。
每次 IO 后、解析后、采纳前检查 `now < call_end` 及旧截止；晚到 Done 不采纳、不消费。同步 syscall 无法强行抢占；超时后如效果不明即隔离，不能声称已经满足真实 100ms。
M4b 只实现/验证离线合作式接口；未来真实实现若无法约束发布、fsync、锁或解析耗时，准入仍失败，不用线程掩盖。

## 2. 命令取消、kill 与 reap

每次 begin 保留唯一 `CommandHandle`（命令种类、row/attempt、进程实例、输出上限、原 end）；成功终态或完成有界取消前不得丢弃。
begin 时按各固定命令的 kill/reap 合约，在既有 operation 预算内固定 `work_end < op_end`，余下 `[work_end, op_end)` 为终止份额；成本回归验证该分配，预算不足拒绝 begin。不设统一 1s 取消 cap 或半预算比例。
首次取消冻结 `cancel_end = min(op_end, 所属 phase_end, cleanup_end, attempt_end, 适用的既有 Stop 终止截止)`；未进入 cleanup 的可选 end 不参与，后续只可收紧、不可重算延长。早取消可用至该有效 end 的剩余预算。
终止份额包含在所属阶段内；revoke/stop/delete/seal 仍各限 10/20/20/10s，并交 cleanup 总 60s 与 attempt end。各最大值不保证能同时取满，不新增阶段、不借 guest 最后 1s，也不增加 Stop 预算。
`work_end` 到达即禁新工作并转 `CancelRequested → KillPending → ReapPending → Exited | Unresolved`；kill 对同一句柄只 begin 一次，pending 只 poll。
取消每片先服务 owner/Stop，再推进一片 kill/reap，调用仍交 step.end/cancel_end。绝不能在入口先 reap、再无限跳过 owner 服务。
reap 只接受严格解码、匹配 handle/进程实例、明确已退出且已回收的及时终态；空 body、含糊的成功、pending、unknown、错句柄、迟到终态均不是退出证明。
`now >= cancel_end` 即停止重试，Unresolved 是 sticky unknown/quarantine，保留定位信息，禁止新 attempt；不能靠每步换新截止永久等待。
命令子进程退出不等于 VM/Tart/Softnet 都退出；delete 前仍须 `stopped_clean` 和相同 VM 身份。guest 副作用没有本地进程句柄，走 revoke/释放及 VM stop，不能凭 clone 名虚构 reap。
成功 kill/reap 只能确认进程终止，不能消除先前的 unknown effect；provider 死由外层 owner 接管，不把 provider EOF 当作所有子进程已退出。

## 3. reserve→seal 的 Stop/owner 服务

增加注入的私有 `OwnerService`，从既有 owner 控制连接读取有界快照；离线用显式入队的 proof/Stop 事件，禁止 `poll(now)` 自动生成新证明。
证明带与 adapter 相同的 attempt binding（run、nonce、VM config digest），非零递增 seq、宿主单调 `issued_at/until`：`issued_at <= now < until <= issued_at+1000ms`。
首个证明须在 reserve 的任何 IO 前成立；缺失即拒绝，不把 `None` 当无限存活。旧 API `owner_present`/guest.lease 不再授予 owner 权限。
新 seq 必须严格增加；同 seq/同全部字段重读仅表示原 standing proof，不延截止；倒序、同 seq 改字段、错 binding、溢出均失败关闭。
服务顺序固定为：

1. 读取当前时间，先检查并应用**已保存的旧 owner end**；若已到期，永久 Suspended，不能先接受队列里的较新证明。
2. 在共享预算内读 Stop/proof 快照；读后再次先查旧 end，才验收替换证明；Stop 位与 proof 分开读取，不互相遮蔽。
3. 锁存 Stop/校验各旧 end，得到 `Work / CleanupOnly / SuspendedCleanupOnly` 权限，再决定下一片。

入口、每次 lock/ledger/runner/guest/parse 前后、证据及消费提交前、破坏性 begin 前都过此服务门；seal 写入及读回亦然。
门在同一 step 多次执行仍共用剩余预算；预算已尽只检查本地锁存/旧 end，不能继续读取服务或采纳结果。
旧 owner 过期发生在锁、IO、取消期间也立即 Suspended；保留旧截止，不置 None、不接受后续 proof 复活。cleanup=unknown，旧 attempt 永远 not_run。
Suspended 禁新 clone/start/side/lease/delete/detach/unlink/seal；只允许已持句柄的有界 kill/reap、撤权/释放和尽力记录 CleanupUnknown。新 recovery owner 在新机器/新有界期限下接管，不能改旧机器状态。

Stop 一经看到即锁存、禁普通工作、冻结 cleanup 起点并安排优先 revoke/释放/取消；不能每次入口返回 StopLatched 从而把自身清理卡死。
Stop 首次到达使正在返回的普通结果作废；清理候选也须按当前权限重新验证，不能顺着旧 forward cursor 在同一步 delete。
owner 仍活时，后续步可继续独立 stop/退出核验，以及满足身份/退出屏障后的 delete/隔离 seal；Stop 始终保持锁存、分数不采纳、followups=false。owner 过期则收紧为 SuspendedCleanupOnly。
停止租约发布与 revoke 优先，不等普通 ACK；revoke 故障/期限到仍转 stop/kill/reap，不以失败的 drain 阻塞释放。
沿用 Host Stop：100ms 锁存、500ms 发释放、5s native、4750ms driver及既有重试额度；邮箱 g+5s/6s 不替代这些约束，也没有自动 resume。

## 4. unknown 的粘性分类

保存独立的 effect 记录 `{binding, op, 实发seq/handle, dispatch状态, resolved结果}` 和 sticky `effect_unknown`，不能仅靠通用 trigger 推断。
有明确 NotDispatched 的预算/策略拒绝只返回拒绝或让出，不制造副作用 unknown；已发且正常 Pending 也不立即 unknown。
已发后丢回复、超时/到期错误、完成读取/解码失败、provider/guest 丢失、显式 unknown、无法 reap，若无已验收的确定终态，立即标记 unknown 并驱动有界取消。
明确 failed/cancelled 终态只在身份/关联/时限都成立时算已知失败，仍 not_run 且不作成功消费；不能把错误字符串当作已知结果。
账本写丢 ACK 可在原 end 内精确读回，不能重发外部效果；读回不明则保留未封存行，不发下游动作。
unknown 一经设置，后续 revoke/stop/delete/缺席成功不能清掉：回执 quarantine、cleanup 不宣称全面成功、followups=false、无评分/成功消费。尽力把隔离事实写到卷外账本；写失败保留未封存状态和未知回执。

## 5. reclaim/recovery 的行绑定

用 `ReclaimCursor { row_key, expected, started, stage, pending_handle, saved_ends, exit_proof }` 替代机器全局数字 `reclaim_at`。
`row_key` 取当前账本的完整精确身份（run、clone、原 absolute；expected incarnation/config 一并固定），不是列表下标或名称前缀；未来 nonce 持久化由 M5 扩展。
阶段为 `List → OwnedStatus → Stop → StoppedStatus → Delete → AbsentList → RowDone`；每次 resume 先核 row_key，身份不符直接隔离。
`StartIssued` 的行即使首 list 缺名，也不能跳过独立进程退出证明；stop/status 身份冲突不授权 delete。effect 前还需复验当前归属，旧快照不跨 yield 充当永久授权。
recovery 外层另存 rows/index/receipts 游标；一行的 RecoveryNoted、reclaim、seal/读回全部完成后才递增 index。
RowDone 清空 reclaim cursor、句柄和该行期限；下一行、新 reserve 和 recovery 结束均显式重置，不能继承上一行的“已到最终 list”。
recovery 使用新 owner 对该行的绑定/接管证明及新冻结截止，不沿用死行已到期的 attempt end；旧行结果始终 not_run。恢复过程中 Stop 继续锁存，不恢复模型工作。
跨步持久性仅是 `Attempt` 字段；进程重启从未封存账本重新建立游标和证明，不序列化句柄/FD，不从数字 stage 猜已完成屏障。

## 6. revoke/drain 的原截止

首次撤权保存 cleanup end，首次 revoke begin 保存 `revoke_end=min(首次 caller, begin+10s, cleanup_end, attempt_end)`；pending/accepted 都不重置。
所有 drain inventory 的每次调用统一计算 `min(step.end, caller, inv_op_end, revoke_end, cleanup_end, attempt_end)`，包括**撤权前已 pending 的 inventory**；不能在 inv_pending 分支提前返回旧 clip。
revoke_end 纳入每片旧截止检查；IO/解析返回后、设 revoke_ok 前、消费前再次要求 `now < revoke_end`。
仅实发 revoke seq 的 inventory.revoke terminal/ok 且 scope_revoked/input_released/tree_exited 全真，才设 revoke_ok 并记消费；accepted/任意成功 body 不算 drain。
短 caller 后才首次 drain、复用读跨 end、连续非终态超过 10s 均失败，不续 end、不消费。仍在 cleanup 原 end 内继续独立 stop/kill/reap，不能冒充 drain 已证。

## 7. 接入 M4a：验收、消费与身份

machine 要求 `G: GuestChannel + GuestAccounting`；只扩展私有侧接口，不改七方法/版本。暴露只读 binding、实发计数/seq、两个发布时间、当前 pinned_grant 及首次已验证 envelope boot；测试 fake 必须给真实记账，不用默认空实现通过接线测试。
inventory/lease 的权威计数来自 adapter 的**发布完成计数**；保存两个不同时间：开始该次发布、请求尚不可能可见前立即取不可变 `h_pre`，成功完成后取 `h_done` 并恰好计数一次。rename 可见到 fsync/校验返回之间允许 guest 采样，故 `h_done` 不能作保守到期锚点。
`h_pre` 精确绑定该 inventory request seq；pin 保存它，poll、同 g 的重复样本、ACK 均不替换。`h_done` 只用于成功发布记账与 cadence；SeqTracker 的派发前预记不是已发布证据。需要时在 `publish_new` 增加此私有快照，不增加线字段。
每次调用前后取快照差值，包含“已发布但随后 poll 出错”的路径；poll、ENOENT、旧 ACK、片不足的派发前拒绝均不计数、不改 `last_publication_done`。发布完成不确定仍为 dispatch unknown/quarantine，禁止重试填补。
machine 不再 bump_inv/猜 lease_seq；计数和 `h_done` 来自同一成功完成事件。第 2049 个必需请求在发布前拒绝，并转撤权/隔离；不能只是 should_renew=false 后继续工作。

terminal 验收顺序固定为：旧期限/服务门 → envelope及 guest identity → 实发 op/seq、阶段及 result 类型 → 语义成功 → 再查服务/期限 → 提交 evidence/消费记录 → 同一步 `note_consumed(exact note)`。
语义要求 bootstrap 的非零 generation 与既有 native ACK 义务、login=ready、run 的合法 provisional score、revoke 的完整 drain；`outcome=ok/login=missing` 仍拒绝。
预算不足以完成最后的本地提交与 note 时，保留候选，下一步重新验收后一起提交；不能先消费再决定是否接受，也不能将 note 拆到后一步。note 本身无 IO、有界、受相同 end 检查；失败则停机隔离，不发布成功回执。
active/revoke 可在同一已验收快照合并 note；未验收的一项不能搭便车。pending/accepted、failed/cancelled/unknown、迟到或身份错误均不作成功消费。
后续 inventory 才实际发布 consume，沿用 M4a retained/issued/consumed 核对及幂等确认；消费确认不是评分或独立进程退出证明。

`session_boot` 唯一来源是**机器验收的首个 verified inventory envelope**；不得用 pre-start status、常量字符串或缺失值补位。
Tart boot 是另一个域。start 后先取得精确归属的 running status T，再验收首 inventory 的 guest boot B，随后再读 running status 确认仍 T，保存 `(row/start实例, guest=B, tart=T)`。
这组夹住绑定过程的状态查询跨步保存游标；相同 run/incarnation/config、唯一 start、两次稳定 T 构成离线关联依据，无需 `B == T`。关联缺失不开放普通副作用；首 inventory 后可先发布安全租约，避免身份查询饿死续租。
此后 envelope 只与 B 比，Tart status 只与 T 比；uid/desktop/config 亦须稳定。任一域变化都拒绝替代值并撤权/隔离，不等 cleanup 首次 status 才建立 Tart 基线。
真实关联证据仍由 M6/M7 验收；本卡的配对不是认证 guest 内容、准入或 TCC 证明。

## 8. 租约服务与 step 交错

reserve 在任何资源副作用前调用 `schedule::prove(attempt_window_ms())`，保存联合 guest/machine 时限与请求数 proof；任一不满足即在分配前失败。M4b-2 更新 `schedule.rs`，按下述实际调度和实测常量计算联合界，不能沿用独立 lease cadence 的 age/gap 算式。
production prove 只用既有实测常量：两单向可见性界各 `1023ms`、共享 step `100ms`、`RENEW_MS=800ms`、guest 窗口 `5000ms`；不把单向上界当下界。模型注入仍仅限 test-only/`cfg(test)`，不得放宽实测常量或开放生产 model 入口；缩短预算可用更短窗口。

inventory 至多一个在途；未完成只 poll，不重发、不追补漏掉的 tick。第 k 份回复验收后，下份请求在 **max（该 pin 的 grant 发布之后的第一个服务 step，前份 inventory 的不可变 `h_pre_k+800ms`）** 到点的服务片发布；grant 义务先于下份 inventory。首次请求按原绑定流程发布；完成/consume 采样优先复用周期请求，额外采样也遵守串行与最小间隔，不绕过门。
此 floor 锚定实际发布开始前的 `h_pre`，不以 poll、回复验收、lease 或 ACK 重置，也不改用 `h_done`。发布完成及计数仍依 §7 的真实 accounting；未派发拒绝不推进锚点，发布不确定即 unknown/quarantine。
首 inventory 绑定 boot 后至 revoke 开始前，所有公开步都服务租约。每个 fresh pin 创建**唯一优先 grant 义务**：验收片有剩余预算可立即发布，否则须在下一服务 step 内发布，不得等独立 `800ms` lease tick。
**成本契约（round-3）**：每个服务 step 至多完成一次邮箱发布（grant 或 inventory，发布不可分片，见 §1）；一次必需发布加上该步必需的 owner/Stop/验收服务必须能放进一个 `100ms` step，否则在分配前拒绝该成本设定，不做半截发布。因此一个回复的处理固定占用至多三个 step：S0 取回并验收回复，S1 发布 grant，S2 起发布下份 inventory（且不早于 floor）。普通阶段和旧 lease ACK poll 不能挤掉该义务；不能按后续 tick 重试填补不确定发布。
lease 精确引用该 pin 的完整 `(inventory_request_seq,g)`，每个 fresh pin 恰好一份，永不为同 pin 再发第二份。记录待办及唯一实发 lease seq，跨 poll 不重建；同 g 保留完整旧 pin/锚点，不创建义务、不延 end；g 下降或加法溢出拒绝。新 pin 的 lease 可覆盖未 ACK 的旧 lease，实发 seq 连续增加，poll 同 seq 不计新请求。

联合时限按这套事件驱动服务计算：

- 回复对 host 可见后，取回与验收至多 2 个 step 起点偏移（`≤200ms`），grant 在随后一步完成（`≤100ms`），下份 inventory 在再下一步完成（`≤100ms`）。
- fresh 样本到其 grant 对 guest 可见的年龄界 `A=1023+300+1023=2346ms`（回复腿 + 取回/验收/grant 三步 + grant 腿）。
- 连续样本间隔界 `D=max(1023+400+1023, 800+100)=2446ms`（回复腿 + 取回/验收/grant/下份 inventory 四步 + 请求腿；floor 到点仍有至多一步 slip）。
- 下份 grant 最迟在 `sample_k+D+A=sample_k+4792ms` 对 guest 可见，严格早于 `sample_k+5000ms`，余量 `208ms`。
- host 必须收到并验收替换 pin 后才能换 end：`h_pre_{k+1}−h_pre_k ≤ 100+1023+1023+300 = 2446ms`，下一 pin 在 `h_pre_{k+1}+100+1023+1023+200` 前验收，合计 `≤4792<5000ms`，余量 `208ms`。未返回请求不能延长旧 end。

请求数用**间隔下界**而非周期上界计算：`N_inventory ≤ floor(W/800)+1+8`；按 ADR §3 保守计两端点周期位置及最多 8 次额外首样本/完成/consume 采样，离格额度单独保存，probe 不逐项新发 inventory。`W=1,560,000ms` 时为 `1951+8=1959≤2048`，余 `89`；每个 fresh pin 只有一份 lease，故 `N_lease≤N_inventory≤1959`，两流各≤2048。
最小间隔即使在快速可见性下也成立；`2446ms` 只用于时限上界，不能用 `ceil(W/2446)` 证明请求数。gate 须以实测常量联合检查 guest 侧 `D+A<5000`、host 侧替换界 `<5000`、成本契约（单次发布+必需服务 ≤100ms）及两流计数；更短 grant、模型外抖动、算术溢出或计数超限均拒绝，不以运行时 cap 关闭冒充完整窗口可行性。
guest 只认真实已生成样本，截止固定 `g+5000`，不从 ACK/收到时刻起租；替换必须严格先于旧 guest end，恰在 end 到达亦失败。旧截止到达永久停止，最后 ≤1s 撤权/释放/杀树，总 ≤6s。S3 scope 3s 独立。

按 controller 的 D2 决策，移除“confirmed coverage 须 current ACK 才可工作”：ADR §3 的 lease terminal/确认“不产生 owner/scope/准入/清理证明”；guest 自行执行固定 `g+5000` 与 ≤6s 停树。
ACK 仅作完整性检查：保留 ≤2048 项实发 grant/ACK 摘要表，terminal 必须精确等于某个已发 grant（含引用与 end）；过时但一致的 ACK（含首次见到）只丢弃、不延长任何东西，未发/改内容即失败。首次 ACK 尚未到、被覆盖或 pending 均不单独阻塞工作，ACK 也不更新 pin/host end。
host/guest 单调时钟不共用 epoch：最新 pin 的保守 `host_end = h_pre+5000ms`，采样不可能早于该次发布开始。同 g 保留完整旧 pin/锚点，不把 g 当 MonoInstant、不用 `h_done` 或 ACK 到达时间重算。
bootstrap/login/run 的 host 工作门为：`now < 最新 pin.host_end`、调度 proof 仍成立且未耗尽（含各流 2048 cap/离格额度）、owner/Stop 允许；身份及原 op/phase/attempt 门照旧。clone/卷准备/start/绑定 inventory 是前置阶段，按其原 owner/身份门执行。
已有 pin 后，换新 pin、每次发布/读取/解析/采纳前后先查旧 host end；到期永久禁普通工作/续租，转撤权，晚样本/ACK 不复活。无新样本、预算/计数耗尽或模型外抖动/丢失导致门失败亦如此；仅 ACK 延迟或缺席不构成到期。长 login、max-task 使用同一门。

每片顺序：旧截止/owner/Stop → 优先 revoke/释放或取消 → fresh-pin grant 义务/待办 inventory → 当前阶段一片。租约 IO 用掉的时间不能再供阶段单独花 100ms。
仅在共享剩余预算内推进下一项，保留义务与服务轮转位置并兑现下一 step 发布界；不能让旧 lease 的 poll 永久饿死 inventory，也不能让慢查询/连续 probe/4s stop 阻止服务。若无法兑现 proof 则失败关闭，VM 退出后不再写邮箱。

## 9. 离线验收与评审可核对项

fake runner/guest **在每次 begin/poll 内**把 IO 成本计入同一个 FakeClock；另给锁、ledger、解析、服务、note 成本。总耗时大于片的工作必须逐片扣剩余成本，不能永远以“101ms 放不下”原地拒绝，也不能只在测试外部跳时钟。
每条 trace 记录 step 起止、各 call_end、实发与 poll、proof/Stop 检查、采纳/消费、kill/reap 顺序、inventory 的 h_pre/h_done/floor 到点、换 pin/host end、fresh-pin grant 义务创建/唯一发布、样本生成/guest 接受时间及 ACK 判定，并核对真实发布完成计数。合作式路径每步 ≤100ms；另注入违约超时，断言返回后无后续 IO/采纳，并 fail-closed，不把它计为满足时限。

| 对应义务 | 必须有的回归（检查行为和最终回执） |
| --- | --- |
| ADR 慢查询/连续 probe | 慢 list/status/inventory，16 probe 逐片完成；每次共享 end，组/单项/Task 原 end 不变，租约与 owner 持续服务 |
| ADR 4s stop/长清理 | stop 持同一句柄 pending 4s 再完成/reap；显式外部证明跨多个 1s 周期直到 seal；释放及时且 Stop 全程锁存 |
| R2-2 | 在 destroy/seal 前及 kill/reap 中跨旧 owner end，安装更高 seq 的有效新 proof；永久 Suspended，无 delete/seal/复活 |
| R2-3 | lock/ledger/IO 内注入 Stop，含 terminal handoff 前、stop/status/delete 间；不采纳旧候选，不越权 delete，revoke/release/kill/reap 仍有界推进 |
| R2-4 | reserve 的 500ms 锁成本改为片内让出/拒绝；慢 clone/start/ledger/seal 跨片完成；60ms lease+60ms inventory 共享 100ms；permit/命令只 begin 一次 |
| R2-5 | bootstrap/login/run 各覆盖已发丢回复、恰在 end 返回 Err、读取/解析失败、accepted 后 provider/guest 丢失；后来清理成功仍 unknown/quarantine、无消费/后续 |
| R2-6 | kill/reap 多片 pending、冻结 end 耗尽、迟到/空/畸形/unknown/错 handle 的 reap；每片有 owner/Stop 服务，严格退出证明正例，无无限等待 |
| D3 取消预算 | clone 60s 预算内早取消，kill/reap 用 1.2s 且在全部旧 end 内，进程回收成功；在 work_end 取消须于原 op_end 前完成或 Unresolved；revoke/stop/delete/seal 内取消核对 10/20/20/10s 与总 60s，不加预算 |
| D4 完成契约 | seal 与多行 recovery 在 begin 后暂停，仅 public poll 收到精确 Sealed/Recovered 一次；再 poll 为 NoActiveStep、无重复写入/其它回执；reserve→clone 跨 entry/poll 仍唯一 permit 构造/移动 |
| D5 -1 独立可达 | 仅 M4b-1 经 public reserve/clone/start/isolate 到 normalized guest 操作；多片真实扣费 pending→accepted，分别注入丢结果/Stop/owner 到期并完成有权清理；bootstrap/login/run 均覆盖，无 phase setter/空记账/legacy 即时成功/-2 代码；迁移后既有 suite 编译 |
| R2-7 | 两行 recovery：A 完成，B 已 StartIssued 但 list 缺名且未证退出；在每阶段暂停，B 不继承 A 游标/期限，禁止 seal；新 attempt 亦从零开始 |
| R2-8 | 撤权前 inventory 已 pending，短 revoke end 内开始复用读、IO 中跨 end；不得 revoke_ok/note/consume；另测迟到首 drain 与连续非终态超过 10s |
| R2-11 | 数千次未派发拒绝/pending poll 后计数与发布时间不动，随后成功发布只 +1；发布后读失败仍 +1；test-only 耗尽负例走到第 2049 个必需请求时，machine 在发布前明确关闭，不充当完整窗口正例 |
| D1 发布锚点 | Attempt+真实 adapter：rename t=1、guest 采样 t=2、延迟 fsync/校验于 t=80 成功，h_pre=0/h_done=80、完成计数恰 +1；无更新 pin（含同 g）且暂扣替换 ACK 时，工作/采纳须于 t=5000 关闭，不能到 5080；poll/ACK 不改锚点，完成不明隔离 |
| R2-12 | pre-start A，guest B，Tart T（B≠T）走到成功 scored seal；分别替换 guest/Tart token 均拒绝；绑定夹取期间 T 变化/缺失也拒绝 |
| 机器消费/R2-9 衔接 | Attempt+真实 MailboxGuestChannel：ready 成功后才消费；login/missing、failed/unknown/cancelled、late、错身份/未发/future seq 不消费；active+revoke 同时验收、重复确认/合法下一项 |
| ADR 片内到期/不复活 | 锁/发布/解析/消费/取消时到期，保留原 end；新样本、旧 ACK、新 owner proof 均不能越过已到期边界复活，unknown 无评分 |

组合测试使用实际 adapter 和时钟驱动 guest 状态机：读真实七操作请求、生成连续样本、保留 active+revoke、核对 consume，exclusive tmp→rename；时钟推进即检查不可逆过期/整树终止，不能只在收到 lease 时检查。
覆盖 long-login 与完整 `1,560,000ms` 窗口成功 trace：持续有权工作，核对真实发布 `N_lease≤N_inventory≤1959`、两流各≤2048、guest 年龄/间隔、host pin/end 无断档及终止≤6s；不能用即时常量 ACK、清私有 in-flight 位或提前安全关闭代替成功 witness。
D2-r2 负控须在 test-only 旧调度下重现 [review-r2 表](reviews/2026-11-15-s11-m4b-design-review-r2.md)：独立 lease `800ms` cadence、两腿各 `1023ms`、100ms 服务网格、正发布成本及明确相位（inventory 发布后 1ms 加可见腿，首 lease 于 2103ms 完成，后续 due 落在服务检查后）。逐项核对：

| inventory h_pre | guest g | host 验收 pin | 首份引用此 pin 的 lease 发布 | guest 见该 lease | 旧 guest end |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 0 | 1024 | 2101 | 2101 | 3125 | — |
| 2200 | 3224 | 4301 | 4800 | 5824 | 6024 |
| 4400 | 5424 | 6501 | 6600 | 7624 | 8224 |
| 6600 | 7624 | 8701 | 9300 | 10324 | 10424 |
| 8800 | 9824 | 10901 | 11100 | 12124 | 12624 |
| 11000 | 12024 | 13101 | 13800 | 14824 | 14824 |

负控在 `14824ms` 因严格旧截止而不可逆停机，即使 host 已换 pin、其 end 仍为 `16000ms`、owner/Stop 允许、计数未耗尽；同 pin 重发 lease 不延 guest end。
D2 正例必须组合 Attempt、真实 adapter、自主时钟 guest，分别对 `900+100ms` 和 `1023+1023ms` 两腿做服务/发布相位 sweep，含正发布/解析/服务成本及 due 恰落服务检查之后的相位；使用 §8 的 floor 与 fresh-pin 义务，分配前 guest/machine/count gate 均成功。
每个相位均跑完整窗口：相邻样本≤2446ms、样本到 grant 可见≤2346ms、下份 grant≤`sample_k+4792ms` 且**严格早于旧 guest end**；每份 lease 精确引用实际 pin 的 `(inventory_request_seq,g)`，每个 fresh pin 恰一份、同 g 不再发，义务在下一 step 内完成。host 换 pin 必须在旧 `h_pre+5000ms` 前，保守替换≤4792ms（余208ms）；发布锚点窗口重叠只作发布窗口重叠检查。
round-3 碰撞回归：复现 [review-r3](reviews/2026-11-15-s11-m4b-design-review-r3.md) 的相位（两腿 1023ms、发布 60ms 且 rename 在开始后 1ms、取回/解析/验收 60ms、前导服务 1ms）：验收步剩 39ms 时 grant 顺延到下一步，下份 inventory 再顺延一步，断言没有半截发布、每步 ≤100ms、实测年龄≤2346ms、样本间隔≤2446ms、替换严格早于旧 guest end，并跑完整窗口。
D2 计数另给两腿各 `1ms` 的 fast-visibility 全窗 trace，保留正发布成本及 100ms 服务网格、每轮严格更新样本；验收快不能把请求加速到每200ms。直接核对相邻 inventory `h_pre` 至少800ms、周期位置≤1951，计入 ADR §3 最多8次额外首样本/完成/consume 后≤1959，lease≤inventory；统计真实成功发布而非 poll 次数或慢路径周期推算。
显式在首次 ACK 前以**新 pin 的 lease**覆盖旧 lease，断言尚无 current ACK 时门仍通过并发生有权工作；一致旧 ACK 丢弃、首个/替换 ACK 均不授予或延长覆盖，未发/改内容失败。每轮记录联合 gate 与实际 trace 一致，测试模型仍仅 test-only；较短 grant、时限等号/超界及计数超限必须在分配前失败。
另测服务 jitter、ENOENT、跳过 lease 版本、lost ACK 和 g 相同/下降；界内且新 pin 连续的 ACK 丢失不单独阻工，模型外丢样本/抖动、耗尽或真实错误仍按旧 host end/原 end 及时关闭、不复活；这些负例与可行性成功 trace 分开。
guest-expiry 负控独立丢弃 lease 投递，同时继续新 inventory、host 换 pin、有效 owner 及普通派发尝试；自主 guest 时钟越过旧 `g+5000` 即不可逆撤权，最后1s内释放/杀树（总≤6s），不能以 host 门先失败作为触发条件。再投递迟到 lease、普通请求及合法 ACK，均无到期后普通效果、scope 恢复或树复活。
保留 M4a 单读 128KiB（含最终稳定帧/失败字节）、R3 截止/字节修复回归；重读和全 attempt 字节统计不能误算成请求数或 attempt 字节上限。
代码交付需 fmt、`cargo test -p gcu-eval`（含 tart_offline）、Linux/Windows clippy及 macOS 离线测试；检查七方法/v1、精确 lexical exemptions、唯一 permit 构造及无新依赖/线程/启用开关。离线成功不记真实时限已验收。

## 10. 交付拆分与边界

选择两 PR，每个独立 session、独立代码审阅；本短设计先独立审阅，不复用废弃 M4 整包。

| PR | 交付与完成条件 |
| --- | --- |
| M4b-1 | 共享 step/截止、CompletedStep 单次交付、所有 runner/ledger begin-poll 游标与句柄取消、owner/Stop、sticky unknown、按行 reclaim；同时交付 normalized Pending/Accepted/Terminal、begin-once guest progress/effect/原 end 与真实派发成本 fake。独立通过 R2-2～R2-7、D3/D4/D5、慢查询/连续 probe/4s stop，既有 suite 随签名迁移编译；Pending 不冒充 Done |
| M4b-2 | 依赖 -1 的分类/进度，无第二套 reply grammar；完成语义 terminal 验收、真实 adapter 组合、consume/drain 证明、boot 配对、私有发布双时间/计数、租约调度及 guest/machine 联合 gate；通过 D1/D2、R2-8/R2-11/R2-12及消费/长窗口回归，并重跑 -1 的成本/Stop 回归 |

文件归属：`machine/io.rs` 管统一门与 clip，新增私有 cursor/service/completion 模块承载进度；`runner.rs`/`ledger.rs` 改离线句柄接口，`guest.rs` 只扩私有 accounting，`mailbox_guest.rs` 补发布证据。`deadline.rs`/`budgets.rs` 不增加 ADR cap。
两 PR 都保持真实 provider/admission 关闭。真实 Tart/卷/网络操作、准入 token、M5 完整卷账本/销毁屏障、M7 supervisor/sandbox/真实整树证明均不在本卡；fake 的 VM 清理不能授权 VM-only 生产 seal。
不修改 Host Stop/S3 常量、七操作线协议或 g+5s 决定，不引入 IP/串口/块盘回退、自动恢复；真实接线仍须 M5～M9 的各自审阅与证据。

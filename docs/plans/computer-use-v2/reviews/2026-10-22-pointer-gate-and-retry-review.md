独立对抗式审查：#155 / #156 / #157；基线 `ef5cb876`；2026-10-22。
最严重发现 1 — BLOCKER R1：动作等闸后不复查取消、期限和前台窗口，可在准入条件失效后开始注入。
最严重发现 2 — BLOCKER R2：host 也信任 `GCU_HOST_PID`，worker 却收到实际父 PID，能静默使用两把不同的闸。
最严重发现 3 — 高 R3：光标到位等待超时、查询失败都返回成功，且 ±2px 容差可在目标移动尚未发生时提前开闸。
结论：重试本身未发现旧标签续租或无回调无限重试；上述闸与输入边界缺口阻止给出整体安全通过结论。

## 发现表（按严重度排序）

所有失败场景均为依据实际控制流构造的可复现实验方案，**本次未执行 Rust 测试或真机实验**；不把构造场景写成已观察到的 Windows 故障。R1、R2 的缺少复查/闸名分裂可直接从代码判定；R3 的返回语义同样确定，完整错投时间线还需注入式测试及真机验证。

| 严重度 | 文件:行 | 问题 | 可复现的失败场景 | 建议修法 |
|---|---|---|---|---|
| **高 / BLOCKER R1** | `computer-use/crates/gcu-driver-windows/src/input/ffi/mod.rs:339`、`:343`；`computer-use/crates/gcu-driver-windows/src/input/ledger.rs:111`、`:112`、`:123`；`computer-use/crates/gcu-driver-windows/src/input/ffi/native.rs:319`、`:320` | #155 在前台检查和 `context.begin()` **之后**新增最长 40ms 的等闸阶段。取得闸后没有 `context.check()`、前台窗口或输入许可复查，直接 `SendInput`；其签名只有输入数组，不能复查上下文。 | 线程 A 持闸；B 已确认目标是前台并进入 ledger，开始等闸；关闭 B 的 ActionContext / 取消请求 / 使期限到达，或切换前台到另一个窗口；A 在 40ms 内释放。B 仍执行 `send_input`。直接后端能确定构造取消后发送；隔离 worker 路径则存在 Stop 信号到真正终止子进程之间的竞争窗口，不能声称每次都能赢过 kill。 | 把实际派发标记和必要的最后准入复查放到取得闸之后、紧邻 SendInput 的位置；将上下文传至注入边界。不得把排队算作已经开始 native mutation。父进程控制的 worker 还需保留停止/期限的最后许可边界，不能只依赖最终 kill；记录/释放 ledger 的原子顺序须一起保持。 |
| **高 / BLOCKER R2** | `computer-use/crates/gcu-driver-windows/src/input/hook_probe.rs:56`、`:58`、`:61`、`:62`；`computer-use/crates/gcu-driver-windows/src/worker/process.rs:52`、`:54` | #155 的同一实现对 host/worker 一律读取环境；不满足 ADR“host 用自身 PID”。host 首次取闸可接受任何可解析的 u32，包括 0；worker 始终被设置为实际父 PID。两边成功取闸也没有互斥保证。 | 用环境 `GCU_HOST_PID=0` 启动真实父进程，父 PID 为 H≠0：host 打开 `...-0`，其 worker 打开 `...-H`。暂停 host 于采样旧光标之后，让 worker 移动，再放行探针发布；原来的光标回拉/错位点击重新成为允许的交错。无需凭据或更高权限；不需要真的执行桌面点击就能用两进程同步计数器证明两把闸同时进入。 | host 角色必须强制使用自身身份；worker 角色从明确的初始化参数取得父身份，缺失/无效应拒绝初始化。通过现有私有初始化通道回传/核对同一 gate 身份，禁止环境解析失败后静默退到 worker 自己的 PID。勿仅把该变量称为“非机密”便认为一致性已经成立。 |
| **高 / R3** | `computer-use/crates/gcu-driver-windows/src/input/ffi/native.rs:326`、`:329`、`:339`、`:369`、`:372`、`:377`、`:381`；`computer-use/crates/gcu-driver-windows/src/input/plan.rs:78`、`:80` | #156 的到位等待没有失败结果：15ms 到期或一直读不到位置，注入仍返回 `Ok`。±2px 只验证距离，无法区分“舍入误差”和“最后一次移动根本还没应用”；approach 与目标本就只差 1px。 | fake 将动作移动的可见应用延迟到 16ms，0–15ms 查询都返回旧位置或错误。当前实现开闸并报成功，后续探针可采旧位置、排入旧位置绝对移动，随后坐标不随按钮发送的 click/down 落错。另构造当前位置=approach、目标差 (1,1)：即使移动尚未应用，第一次查询已被判为到位。探针若恰在两次查询之间见到移动，会安全锁存；若两次查询都先读到旧位置，则不能靠复读必然发现。 | 在现有 15ms 预算内返回明确 `Settled/Unknown/Timeout`；后两者不得继续发送依赖到位的按钮/拖拽包，应按已发送移动保留 `maybe_executed=true` 并清理。依据实际归一化结果判定预期像素，补近邻目标测试；对尚未确认完成的移动不得允许探针把旧位置当作安全同位置。仅延长等待不是修复。 |
| **中 / R4** | `computer-use/crates/gcu-driver-windows/src/input/hook_probe.rs:62`；`computer-use/crates/gcu-driver-windows/src/input/hotkey.rs:52`、`:68`；`computer-use/crates/gcu-driver-windows/src/input/plan.rs:78`、`:80` | 闸只覆盖一个 host 家族，系统光标却在整个桌面共享。两个 host 各自合法注入时，A 的守卫探针仍能干扰 B 的动作；新闸没有覆盖这类交错。此项是覆盖范围缺口，不是证明同一 host 的闸失效。 | 在同一桌面启动两个父进程，使用两个不同且合法的 Stop 和弦，避免相同 RegisterHotKey 冲突先将第二个实例安全关闭。A 探针采 O；B 在另一把闸内移到 T；A 发布 O；B 的独立按钮包落 O。两边都可能有新鲜双钩子证明。 | 在驱动组装层实施桌面范围的注入排他所有权并明确拒绝第二个活动注入者；或设计覆盖所有参与 host 的桌面指针协调。只把 PID 换成实例随机数不能解决共享光标。若产品已另有严格排他保证，应给出调用链和可执行契约测试。 |
| **中 / R5** | `computer-use/crates/gcu-driver-windows/src/input/watchdog.rs:12`、`:14`、`:65`、`:67`；`computer-use/crates/gcu-driver-windows/src/input/hooks.rs:121`；`computer-use/crates/gcu-driver-windows/src/input/ffi/safety/probe.rs:338`；`computer-use/crates/gcu-driver-windows/src/input/hotkey.rs:277` | 100ms 是证明有效阈值，当前实现不能据此宣称“摘钩必在 ≤100ms 锁存”。恰好 100ms 仍有效，看门狗每次检查后再睡 20ms；探针 `authorized()` 不同步核验注入租约年龄。此缺口早于 #157，重试没有新增更长租约，但也没有补上墙钟检测上限。 | 旧证明 t=0，之后无鼠标回调，其他传输心跳持续健康；看门狗在 t=0/20/…/100 检查，t=100 尚有效，下一次 t=120 才锁存。动作/探针在 t=101–119 的派发竞争另见 R1。已有诊断也记录失败时 lease age 101–124ms，不能把 400 次未误锁存当作截止上限证据。 | 保留 100ms 常量，按既有锚点绝对失效时刻安排唤醒并在 native 派发边界同步检查；明确 OS 调度抖动与硬上限的区别。补“看门狗尚未下一次轮询时已经过期”的测试；不能以扩大 100ms 解决。 |
| **中 / R6** | `computer-use/crates/gcu-driver-windows/src/input/hook_probe.rs:28`、`:90`、`:93`、`:98`；`computer-use/crates/gcu-driver-windows/src/input/injection.rs:51`、`:120`、`:146`；`computer-use/crates/gcu-driver-windows/src/input/hotkey/fake.rs:176` | Windows 单测也绕过内核闸；测试闸每线程独立且不可重入，生产 Win32 mutex 同线程可重入。现有“持有者阻止等待者”用例是在**同线程嵌套**取闸，生产会成功，测试却期待 GateBusy。竞态用例让 fake 通过 `gate_held()` 决定是否制造竞态，没有真正竞争线程；FakeSafetyApi 直接确认戳，不经过新重试状态机和 native Sender。 | 两个单测线程各自进入 `with_injection_gate`，当前 TLS 实现可同时成功；同线程嵌套在 Windows 生产则同样成功而不是超时。因而现有绿灯不能证明跨线程/进程竞争、释放线程、abandon 或重试派发时序。MouseProof 的纯逻辑单测仍有价值，不能一概否定。 | 注入可共享的 gate 实例，测试内显式共享、测试间隔离；使模拟契约明确匹配 Win32 重入/abandon。Windows 无 GUI 集成测试从普通依赖调用生产 gate，通过子进程/通道屏障验证互斥；禁止只用 `#[path]` 编译成 cfg(test) 再宣称覆盖生产实现。补 Sender+MouseProof+GuardEvents 的同一确定性状态机测试。 |

## 闸、锁顺序和生命周期核对

下文路径缩写：`W/` = `computer-use/crates/gcu-driver-windows/src/`，`WT/` = `computer-use/crates/gcu-driver-windows/tests/`，`M/` = `computer-use/crates/gcu-driver-macos/src/`，`L/` = `computer-use/crates/gcu-driver-linux/src/`。行号均对应上述基线。

- **未找到内部锁形成的闭合等待环。** 动作是 `InputLedger.held → gate → SendInput/settle`（`W/input/ledger.rs:111`、`:123`，`W/input/ffi/native.rs:319`）；SendInput 可能等监听线程。监听回调只改线程本地 Context、ProbeIdle 和 PhysicalClock，既不取 gate，也不取 ledger（`W/input/ffi/safety.rs:157`、`:175`、`:189`）。探针是 `gate → GuardEvents 的短锁/SendInput`（`W/input/hook_probe.rs:281`，`W/input/ffi/safety/probe.rs:374`、`:394`）；GuardEvents 当前没有持内部锁再等待 gate 的路径。`refuse_proof` 释放 window 锁后才 `fail`（`W/input/hotkey/lease.rs:75`、`:80`）；`request_stop` 复制 signal 后释放锁再调用（`W/input/hotkey.rs:317`、`:322`）。该判断依赖 StopLatch/signal 实现遵守既定非阻塞契约，不能扩张为任意第三方回调也不会死锁。
- **释放入口确实不直接取闸，但“整个 Stop 释放绝不等闸”过强。** `W/input/ffi/mod.rs:375`、`:377` 走 `inject_release`，后者只调 `send_input`（`W/input/ffi/native.rs:388`、`:392`）。直接后端的 release 先等同一 ledger（`W/input/ledger.rs:144`），该 ledger 可能正被等闸/settle/SendInput 的动作持有；因此仍会间接被动作阻塞。100ms 的 `timeout` 只停止等待 spawn_blocking 的结果，不会取消阻塞线程（`W/input/ffi/mod.rs:373`、`:383`）。
- **隔离产品路径有重要补偿，不能漏报。** 父 ledger 与子 ledger 不同：父进程 prepare 只短暂 reserve，不跨子进程 FFI 持锁（`W/worker/input.rs:210`、`:219`）。Stop 独立通知 supervisor（`W/worker/supervisor.rs:178`）；supervisor kill/reap 后才父进程 up-only cleanup（`:403`、`:411`、`:415`），避免先发 up 后又被子进程发 down。这个顺序降低直接 ledger 阻塞风险，但不能替 R1 的最后派发检查，也不能证明 kill/SendInput 的墙钟时间。
- **WAIT_ABANDONED 表示当前线程取得所有权，不是失败。** `W/input/hook_probe.rs:77` 接受它，与增补决定一致；RAII 仍需 ReleaseMutex。闸不保护一块需要恢复的共享内存，所以没有必然的“脏内存继续使用”错误。但是它不证明前任的输入队列已排空或所有按下状态已释放：worker 崩溃后的不确定输入必须继续由父 ledger 清理；不能用 abandon 成功当作新证明或重放依据。
- **线程亲和当前调用链成立。** `_held` 是 `with_injection_gate_for` 内部同步局部值（`W/input/hook_probe.rs:132`、`:150`），不返给调用者、不跨 await，正常返回、`?` 和 unwind 都在获取线程 drop。生产 `Held(isize)` 本身可 Send，类型没有强制亲和；当前没有移动它的调用点，因此不是已发生的跨线程释放 bug。建议用不可 Send 的线程标记将约束写进类型，并检查/记录 ReleaseMutex 失败；目前返回值被忽略（`:45`）。进程/线程终止则由 Windows abandon 语义处理。**仅取消 async 等待并不保证同步 body 已结束或闸立即释放**，尤其 body 卡在 SendInput 时。
- **句柄没有逐动作增长证据。** OnceLock 每进程仅保留一次成功 CreateMutex 句柄（`W/input/hook_probe.rs:56`、`:67`），无 CloseHandle 是有意进程寿命资源，OS 在进程退出回收；不是每次 inject 泄漏。创建失败会缓存 0，余生 GateBusy，属于永久拒绝的可用性问题。测试的 `Box::leak` 则是每测试线程一个 Mutex（`:94`），不是生产内核句柄泄漏。
- **名字抢占本身保持拒绝语义。** 同用户进程持续持有正确 mutex → 最长等 40ms → GateBusy，body 不执行；若占的是不兼容对象或权限不足，创建/等待失败也进 GateBusy（`W/input/hook_probe.rs:67`、`:71`、`:76`、`:80`）。动作报告未发送（`W/input/ffi/native.rs:331`），外层 ledger/context 保留之前动作的不确定性（`W/input/ledger.rs:124`、`:139`）；探针拒绝交 `refuse_proof`，Proving 拒绝、Injecting 锁存（`W/input/ffi/safety/probe.rs:161`，`W/input/hotkey/lease.rs:74`）。这些不会授予注入权限。第三方只要在期限内释放，等待者可正常继续；其继续是否仍获授权是 R1，不能笼统归为“抢占只会 DoS”。
- **PID 复用不等于身份越权。** 两个同时活着的本机进程不会有相同 PID；正常全家退出且句柄全关，旧命名对象消失。若其他进程保留句柄，复用 PID 的新 host 会碰到旧对象，造成额外竞争/拒绝；没有从 gate 取得会话身份/授权的路径。worker 的 Job 设置 kill-on-close（`W/worker/unsafe_job.rs:19`、`:31`），降低父死后旧 worker 存活概率。名字仍没有进程创建时间/实例绑定，不能据此保证不存在旧实例残留；实际多 host 漏协调另见 R4。
- **环境篡改边界。** `env_clear` 后，worker 在 config.environment 之后被强制覆盖为实际父 PID（`W/worker/process.rs:30`、`:52`、`:54`），调用方通过该 config 提供另一值不能覆盖最后一次设置。host 自身环境没有这种角色约束，故 R2。若已控制 worker 内执行或其环境，在第一次取闸前换名/移除变量可让其改用别的闸；这不赋予 Host grant，但会破坏正确性隔离。OnceLock 初始化之后再改环境不会换闸。
- **没有发现确定的内部活锁；公平性不保证。** 每次等待有界且拒绝后不在同一周期无限忙重试，Windows mutex 没有这里可依赖的 FIFO 保证。密集动作/探针竞争会误拒绝或使旧租约到期，仍应 fail closed；不可把压力下 400 次成功提升成无饥饿证明。

## 重试及时间预算核对

| 核对项 | 代码与结论 |
|---|---|
| 每次缺失最多一次重试 | `W/input/hook_probe.rs:394` 先挡 failed；`:399` 挡未完成；`:406` 仅 action+AbsoluteSamePosition+未用额度；`:414` 先消耗额度。重试也没有回调时下一次到 `:416` 返回 None。`next` 不调用 clock.confirm。 |
| 旧尝试能否为新尝试作证 | `:438` checked_add 分配新标签，`:443` 替换 active；鼠标回调 `:456` 只认 active.tag，旧标签只作为 tagged 元数据；键盘也只记 current_tag（`W/input/ffi/safety.rs:169`）。Sender 清除旧键盘（`W/input/ffi/safety/probe.rs:260`），`keyboard_stamp_for_mouse` 再要求鼠标证明标签一致（`W/input/hook_probe.rs:520`）。未发现旧 A 回调给重试 B 续租的路径。serial 不在 begin_cycle 重置，溢出停止分配，不回绕复用旧标签。多个独立监听实例初始标签相同，非密码学来源证明；本次不把它误报为摘掉的钩子能凭空收到回调。 |
| 额度恢复是否绕成无限重试 | `:402` 要求已完成且收到当前尝试的注入鼠标回调，才在 `:405` 恢复；旧标签、剥离注入标志的回调不满足。只鼠标成功/键盘始终缺失可以继续分配尝试，但不能产生双钩子租约，故最多到旧租约失效；这不是无回调无限重试，也不是双证明门被绕过。 |
| begin_cycle 是否能反复恢复额度 | `:375` 清 active/proven/failed/retry，但**不重置标签序列**。生产 `adopt` 在没有 pending 且代次改变时才调用（`W/input/ffi/safety/probe.rs:306`、`:314`）；同一代的 next 不会再次 begin_cycle。新动作还受 `begin_foreground` 的串行锁、丢弃旧租约及新证明准入约束（`W/input/hotkey/lease.rs:107`、`:125`、`:132`、`:171`），不能仅换周期继承盲注入许可。 |
| 初始周期与拒绝周期 | 初始 cycle 的 action=false，缺失 absolute 回调不重试；初始派发另受 `generation==0 && !initial_finished`（`W/input/ffi/safety/probe.rs:238`）。`complete(Err)` 置 failed、清 proven（`W/input/hook_probe.rs:492`），包括 gate busy、部分 SendInput、光标变化；下一次 next 拒绝。Proving 也算动作窗口，首次动作证明尚未完成时可以重试，但动作尚未准入；不能将其误称为已有 100ms 注入租约。 |
| 报告迟到/代数 | 单发送线程、单 pending、容量 1；pending 时不 adopt，也不发下一 Job（`W/input/ffi/safety/probe.rs:35`、`:40`、`:273`、`:306`）。旧 generation 报告低于 floor 被忽略、不 complete 新动作（`:156`、`:158`）。报告本身没有 tag，一致性依赖上述严格单 pending；未来并发化不可沿用这个隐含约束。 |
| confirm 的较旧戳 | `W/input/hotkey.rs:129` 拒绝旧代，`:133` 取同尝试的 min；年龄 >100ms 且 Injecting 返回错误，经 pump 失败锁存。鼠标缺失或键盘缺失都不 confirm。重试发送成功、gate 成功、光标未变都不是证明。确认时钟写回调时刻而非“现在”（`:139`，`W/input/hooks.rs:55`）。 |
| 摘钩检测是否因重试延长 | 无真实鼠标回调时，重试不会改 `confirmed`；`W/input/hotkey.rs:277` 的失效判断、failed 后 clock.disable 也没改。因此 #157 **中性于证明年龄阈值**，并未新设 200ms 或按 retry 发出时刻起算。严格墙钟 ≤100ms 尚不成立，见 R5；`MouseProof::next` 自身没有时钟，不能单独证明“重试必在租约内”。NativeMouse.authorized 也只查代次、窗口和 failed/stopped，可能在失效已过而 watchdog 尚未观察前发探针。 |

预算必须从旧证明锚点算剩余时间，不能只相加两个新常量。动作的调用时延至少包括 `gate_wait(≤40ms) + SendInput(无本地硬上限) + settle(轮询名义≤15ms)`；探针还包括 sender 排队、cursor/desktop 查询、带回调的 SendInput、复读和 listener 接收报告。两次尝试可能各自经历一次 gate_wait。旧锚点年龄若已 60ms，40ms 等闸即耗完，15ms 到位等待尚未计入；重试额外一次 40ms 也不是预留在租约中的时间。错过应拒绝/锁存，不能靠这些名义上限声称必能续租。

40ms 只约束 WaitForSingleObject 等待，15ms 只约束循环中显式检查的 Instant 截止，不约束线程调度、Win32 查询或 SendInput 卡住；sleep(500µs) 也不是准确唤醒保证。`W/input/hook_probe.rs:132` 已取得闸后才 `tracing::info!`（`:136`），探针的 `send` 也在闸内写诊断（`W/input/ffi/safety/probe.rs:415`、`:425`、`:433`）；若同步日志 sink 阻塞，闸持有时间进一步增长。GuardEvents 的诊断 Mutex 会在 writer 前释放（`W/input/hotkey.rs:200`、`:209`），所以没有发现日志锁与 gate 的内部环，但仍不能证明持闸时长有硬上限。

`begin_foreground` 保留调用者 deadline，证明上限取 min（`W/input/hotkey/lease.rs:122`）；worker RPC 继续沿同一 deadline 留清理余量（`W/worker/driver.rs:99`、`:102`）。未发现 #155–#157 重开 4750ms/5s 总预算的代码。同步 body 已开始后的取消、闸释放和之后是否再注入，则是 R1 及上述线程生命周期限制，不能由外层 timeout 自动保证。

## 相对 ADR 0013 及增补的判定

依据：`computer-use/docs/adr/0013-windows-stop-guard-liveness-lease.md:73`、`:82`、`:83`；`computer-use/docs/adr/0013-addendum-initial-probe-overlap.md:34`；`computer-use/docs/adr/0013-addendum-real-hardware-budget.md:60`；`computer-use/docs/adr/0013-addendum-cross-process-pointer-gate.md:13`、`:15`、`:32`、`:38`。不把早期文档的“不重试”当成尚未批准：10-22 增补明确记录了用户批准。

| 项目 | 判定 | 理由 |
|---|---|---|
| 命名闸替代进程内闸，覆盖同一家族动作与探针 | **紧缩** | 两端同名且正确参与时，排除已观察到的 stale-position 交错，不降低双钩子证明要求。 |
| GateBusy、创建/等待失败拒绝、等待 40ms | **紧缩于竞争输入；中性于租约** | 没取到闸不发送，不自动重试动作；不会续租。但它不是最后准入检查，不豁免 R1。 |
| 环境决定 host 闸名（R2）、忽略多 host 光标共享（R4） | **实现缺口；在构造场景中放松应有的输入隔离** | 与增补 host 自用 PID 的文字不符；“都取得闸”不再意味着探针与动作串行。并非新增 Host 身份越权。 |
| 等闸期间准入过期后仍发（R1） | **放松** | 本应在取消/期限/前台条件失效时拒绝，新增等待把既有检查与实际发送分开，又没有补检查。 |
| WAIT_ABANDONED、同线程同步 RAII、进程寿命句柄 | **中性** | 所有权正确获得并释放，不是新证明；恢复前任输入仍属 ledger 责任。 |
| 15ms settle 的成功路径 | **紧缩交错窗口；中性于钩子证明** | 减少动作移动尚未可见时开始探针的机会，不凭 cursor 状态续租。 |
| settle 未到位/未知仍成功（R3） | **相对“确认到位再开闸”要求存在放行缺口** | 相比 #155 增加等待本身没有使硬件更差，但 #156 没把未知转为拒绝，无法兑现增补描述的到位前提；不能把部分缓解算完整 fail-closed 修复。 |
| Stop up-only 绕过闸 | **紧缩释放可达性；中性于授权** | 不允许 gate holder 直接挡住释放入口；没有增加 down。直接后端 ledger 的间接等待仍存在。 |
| #157 多一次绝对探针 | **相对原“不重试”是已批准的有限放松；相对 10-22 增补中性** | 允许一次额外输入，仍需本尝试真实双回调，未把 send/旧 tag 当证明，未更改年龄阈值。不能描述为所有方面均“零放松”。 |
| retry 额度恢复、标签、失败短路、min 时间戳 | **中性，保持 fail-closed** | 当前控制流没有发现无回调无限续租、旧代/旧尝试证明新输入的漏洞；缺少组合执行测试见下文。 |
| 20ms 轮询与严格检测时限（R5） | **既存不符合硬墙钟表述；不是 #157 新放松** | 证明阈值未改，但实际锁存可能晚一个轮询周期；不得将“常量没变”作为时限证明。 |
| 测试替身差异（R6） | **运行时中性，证据不足** | 不直接改变生产拒绝语义，却不能支持宣称跨进程互斥和真实重试时序已经由当前单测证明。 |

## macOS / Linux 同类风险证据

| 平台 | 查到的守卫/输入路径 | 有证据的结论 |
|---|---|---|
| macOS | `M/input/ffi.rs:268` 创建 listen-only CG event tap；`:296`–`:314` 运行 CFRunLoop、查询 tap/权限、刷新监控心跳；`M/input/activity.rs:66` 过滤自有标记；动作在 `M/input/cg.rs:279`–`:288` 创建并发布 CGEvent。 | 守卫健康检查没有发布鼠标/键盘探测事件，因此**未发现**“守卫采旧光标再写回、覆盖动作”的对应路径。自有输入被活动监控过滤，不反向移动光标。该结论不等价于证明 event tap 的所有活性/负载风险均已解决。 |
| Linux X11 | `L/x11/hotkey.rs:302`–`:318` 轮询键盘与 XI2 raw 事件、记本地心跳；`L/x11/activity.rs:186`–`:195` 按 XTEST 设备过滤活动；真正动作在 `L/x11/real.rs:334`–`:363` 调 XTEST motion/button/key；`:486` 的 listener 只 poll_for_event。 | 守卫没有用于心跳的 XTEST motion/key 发布，未找到相同“守卫探测输入”互相干扰的证据。设备识别或共享 X 连接可能有其他问题，本次不凭可能性立项。 |
| Linux Wayland | `L/wayland/input_stop.rs:90`–`:109` 的健康 probe 是 GlobalShortcuts.list_shortcuts D-Bus 往返并核对结果；`L/wayland/input_control.rs:68`、`:82` 维护独立 Stop lease。 | 这里名叫 probe 的操作是只读 portal 请求，不是 libei/鼠标输入；未找到与动作光标相互覆盖的同类路径。75ms listener lease+25ms tick 的预算设计也不能直接移植成 Windows 已满足 100ms 的证据。 |

本次只把上述路径能证明的事实纳入结论；没有在 macOS/Linux 真实桌面运行，也不把字符串“probe”出现就当作输入探针。

## 应补测试（名称、构造与伪代码）

以下全部是**建议新增、not_run**；本任务不修改测试或实现。便携测试应注入共同的 gate/时钟/native-send/位置读取边界，并驱动生产决策逻辑，不能复制一份实现当作 oracle。涉及 Win32 mutex 的测试可在 Windows 无 GUI CI 跑，**不调用 SendInput**，无需真实账号/桌面。

| 建议测试名 | 构造方法及关键伪代码 | 断言/当前缺口 |
|---|---|---|
| `pointer_gate_wait_cancelled_before_acquire_sends_nothing` | 共享 fake gate 的 acquire 屏障通知 queued；启动实际注入请求；`await queued; cancel.cancel(); gate.grant(); join()`。另参数化 `context.close()`、期限到达。 | `native_send.calls == 0`；无先前效果时 maybe_executed=false；ledger 无多余 down。当前流程预期失败（R1）。不能在不同线程使用当前 TLS gate 来冒充竞争。 |
| `pointer_gate_wait_foreground_changed_refuses_dispatch` | 目标窗口 A 前台检查通过；阻塞 acquire；`foreground=B; gate.grant()`；在 native 发送边界记录接收前台。 | 零 SendInput，不向 B 注入；即使调用者先前批准 A 也拒绝。参数化原来目标消失、最后许可被撤销。 |
| `pointer_gate_wait_lease_expired_before_watchdog_tick_refuses_dispatch` | 旧确认锚点=0，Injecting；时间推进至95，排队取闸；推进至101，**故意不调用下一次 check_health**，再授权闸。 | native-send 边界必须拒绝；不能依赖测试主动先调用 watchdog 把漏洞隐藏。期限仍为既有100ms。 |
| `pointer_gate_host_environment_cannot_split_worker_identity` | 纯角色/名字解析注入，`host_pid=101; inherited_env=0; spawn_worker()`；worker config 再提供999。Windows 集成版用子进程输出 gate 名并通过管道协调空 body。 | host 与 worker 都用同一受控 gate；worker config 不能覆盖最终身份；缺失/非法 worker 初始化拒绝。当前 host=0/worker=101 会失败（R2）。 |
| `pointer_gate_two_threads_share_exclusion_without_gui` | 两线程显式共享同一 gate：`A.enter(); B.start_enter(); assert B.not_entered; A.leave(); assert B.entered`，用通道/屏障控制顺序。 | 真实共享互斥；每测试独立实例，禁止用 wall-clock sleep 作为唯一同步。当前 TLS 实现不满足。 |
| `pointer_gate_two_processes_timeout_then_enter_without_gui` | Windows 子进程 A 持生产命名 mutex，发 ready；B 以零等待尝试，随后在 A 释放后再次尝试；另以有限等待测试不执行 body。 | 占有时拒绝、释放后可进；40ms 另用宽调度容差测等待，不把它当硬实时证明。生产 gate 必须作为普通依赖编译，不能 cfg(test) 替换。 |
| `pointer_gate_abandoned_owner_is_acquired_and_released_without_gui` | A 子进程取得 mutex 后不 ReleaseMutex 直接退出；B 必须事先持对象句柄，确保对象未消失；B acquire/drop；C 零等待 acquire。 | B 接到 abandon 所有权并恰好 release；C 可进；不同 kernel object type/DACL 失败参数化为拒绝，不执行 body。 |
| `pointer_gate_same_thread_recursion_matches_win32_without_gui` | 相同线程嵌套进入生产 gate，内层退出后让另一线程尝试；外层退出后再尝试。 | 明确 Win32 递归计数：内层成功，只有内层退出仍阻塞其他线程；外层退出后才放行。替换当前同线程“必超时”的错误测试模型。 |
| `pointer_gate_unwind_releases_on_acquiring_thread` | fake/native gate 记录 acquire/release thread id；`catch_unwind(\|\| with_gate(\|\| panic!()))`；随后另一线程进入；另覆盖 body Result::Err。 | 一次 acquire/一次 release，同线程；panic 后不会永久占有。取消等待中的 future 与已执行 body 必须分别测试，不能假定 Drop future 会打断同步 body。 |
| `pointer_settle_timeout_or_unknown_stops_following_button` | 注入 fake 单调钟/位置查询：移动已接受，位置0–15ms始终旧值或 Err；16ms才应用。执行真实 move→button 计划，记录所有发布。 | 不返回无条件成功、不发送后续 down；保留 move 的 maybe_executed=true；零补偿、零动作重试（R3）。 |
| `pointer_settle_adjacent_target_requires_observed_arrival` | `current=(101,101); target=(100,100)`；fake SendInput 先接受但延迟应用；在到位前安排探针采样。另覆盖虚拟桌面负原点、归一化舍入、last move 和无 move 包。 | 当前 ±2px 判断不能作为“最后移动已完成”的唯一依据；探针不可把旧 approach 位置覆盖回去。还应组合 probe 的发送后查询早于其移动可见的情况。 |
| `stop_release_bypasses_contended_gate_and_retains_ledger_order` | 父 ledger reserve 一次 down；另线程持 gate；触发实际 NativeRelease 的 fake native-send；隔离版用 fake supervisor 控制 child-reaped 屏障。 | up 无需 gate；必须先 reaped 再 up、之后无 child down；直接后端另测 ledger 被 dispatch 占用的情况，揭示间接等待而非只测 inject_release 的函数名。 |
| `retry_both_mouse_callbacks_missing_preserves_old_anchor_and_latches` | 统一 ManualTime+MouseProof+Sender 调度：成功 anchor=0；续租尝试 A 完成无 mouse，键盘可有；A 的唯一 retry B 也完成无 mouse；推进至100和101，刷新 pump/hotkey 心跳。 | A+B之后无C；clock.confirmed 始终0；100仍有效，101调用检查即 latch，释放signal恰一次；晚回调不能使 disabled clock 复活。现有测试没有该组合。 |
| `retry_old_attempt_or_generation_callbacks_never_confirm_new_attempt` | 交错 `complete(A); next(B); mouse(A); keyboard(A); complete(B)`；再分别加 mouse(B)/keyboard(A)、mouse(A)/keyboard(B)。参数化报告先于回调/回调先于报告、跨 begin_cycle、initial report 晚到。 | 直到同代同尝试双回调且 cursor 验证成功才 confirm；旧戳不能既续租又触发假摘钩；retry allowance 不被旧 tag 恢复。 |
| `retry_mouse_only_success_cannot_extend_dual_hook_lease` | `A.miss → retry B.mouse_ok/key_missing → C.miss → retry D...`，持续推进手动时间，不回退时钟；真实 Sender 每次清 keyboard。 | 可恢复鼠标额度，但 anchor 不变，原100ms阈值仍 latch；证明“恢复额度”不能被误测为双钩子证明。 |
| `retry_refused_partial_or_cursor_changed_cycle_never_resubmits` | 参数化 gate busy、SendInput部分/零计数、后读位置变化/失败；制造 callback 先来、report 后来，然后多次 poll。另跑 initial action=false。 | next始终None，最多原来那次输入，不补偿/不重试；Proving拒绝不锁存，Injecting拒绝锁存。 |
| `retry_delayed_pair_uses_earlier_callback_deadline` | B鼠标=60、键盘=80、report=90；confirm后在160/161检查，泵心跳保持新鲜。另一变体report=161。 | anchor=60，160仍有效、161失败；不得锚到80或90；已经陈旧的同代双戳在确认边界必须报错。 |
| `watchdog_expiry_between_polls_has_explicit_detection_budget` | 不依赖 FakeSafetyApi.advance 自动执行检查；fake scheduler 按0/20/…/100/120只调生产 check_health；anchor=0后摘钩且无任何双戳。 | 现实现120才latch，明确暴露R5；修法须在不改100ms常量前提下通过既定截止合同，不能把预期改大刷绿。 |
| `desktop_pointer_two_hosts_cannot_overwrite_each_other` | 两套host身份/合法不同热键，共同 fake cursor 与 action sink；屏障顺序 A.sample(O)→B.move(T)→A.publish(O)→B.down。 | 必须通过桌面排他拒绝一方，或完整协调而使按钮位于T；只有“不同PID不同闸均能锁成功”不是通过条件（R4）。 |

## 验证与证据边界

- 只读核对：`git log` / `git show --stat` / `git diff <merge>^1 <merge>` 对应 #155=`375b4055`、#156=`4f0fa9eb`、#157=`e43b4a0c`；逐行阅读当前实现、测试、ADR 及 diagnostics。报告所有行号取当前基线，不取 merge 前旧行号。
- 文档自查 **PASS**：`git diff --check -- docs/plans/computer-use-v2/reviews/2026-10-22-pointer-gate-and-retry-review.md`；另用 `git diff --no-index --check /dev/null docs/plans/computer-use-v2/reviews/2026-10-22-pointer-gate-and-retry-review.md` 覆盖未跟踪的新文件（无诊断，退出1表示存在新增差异）。Python 内联自查核对70处完整代码路径/行号存在、五行摘要、末尾未决问题、空白与唯一改动路径，均通过。未修改 STATUS、代码、工作流或时限常量。
- `rcargo test -p gcu-driver-windows --test hook_probe`：**not_run**；`rcargo test -p gcu-driver-windows --lib`：**not_run**。本次是只读文档审查，执行协议明确纯文档不编译；没有将检查源码冒充测试通过。上述建议用例尚未实现，这两个命令也不代表已经覆盖它们。
- Windows 无 GUI gate 集成测试：待新增后由 `wcargo test -p gcu-driver-windows --test pointer_gate` 验证，**not_run**（当前没有该测试目标）。真实 SendInput/Stop 时序、两host并存和光标延迟实验：**not_run**；无需在无交互沙箱尝试。现有20/9/1/0契约与400次拖拽结果只作为仓库提供的历史证据，未独立重跑。

## 未能判定的问题

1. Windows 为什么偶尔不向低级钩子上报已成功发送的带标记鼠标回调，仍未确定。`computer-use/crates/gcu-driver-windows/diagnostics.md:686`、`:708` 记录症状及未知；一次重试的有效性不能证明原因是“丢事件”而非投递/调度/合并的其他机制。
2. settle 超时、±2px提前成功及“probe自身SendInput返回后位置尚不可见”的构造，在真机上分别有多大概率？当前没有注入式返回值/延迟控制测试，也没有覆盖这些分支的原生轨迹；不能声称已在修复后观察到误点击。报告R3针对确定的未知仍成功分支及其可允许的交错。
3. 产品所有入口是否另有桌面范围、跨父进程且不可绕过的注入排他保证？本次审查的驱动/worker路径没有提供该保证；相同默认热键冲突会拒绝第二个实例，却不能替代对自定义不同热键的排他合同。若确有该层，R4需据证据重新定级。
4. 任意Win32 FFI/日志阻塞及OS调度下，≤100ms实际锁存、≤500ms发出全部up、5s结束能否兑现？现有阈值、timeout和20ms轮询不足以独立证明硬上限；本次不改常量、不绕过fail-closed。历史lease age101–124ms与严格表述的差异应由总控核对验收口径。
5. WAIT_ABANDONED之后前任已插入但未消费的鼠标移动与父ledger恢复，Windows具体执行顺序如何？同步所有权与输入队列完成是两件事；现有测试没有在持闸进程崩溃的同时验证输入/释放顺序。只获取互斥体的CI测试能确认所有权契约，不能代替桌面输入实验。

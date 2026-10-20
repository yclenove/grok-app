# 0013 增补 · WL2R Windows Stop 守卫真机失败独立分析

日期：2026-10-20。状态：独立分析，供总控交叉检验；修复方向不构成实现或合并授权。

本次只新增本文。分析基线为当前工作区 `879bb8a5`（PR #147 / WL1 合并），并只读比较其第一父提交中的相关实现；未读取 `cu2/WL2-lease-budget` 或另一位工程师的分析。不改源码、常量、协议、STATUS 或 fail-closed，不运行 SSH、rcargo 或提交。

## 证据与精度

本文使用以下标记，避免把推测写成实测：

- **[日志]**：输入文本直接记录的事实。
- **[代码]**：由当前代码及 WL1 合并差异推出的行为；不是新增真机观测。
- **[模型]**：与日志及代码一致的调度重建；精确 tick、微秒相位尚待插桩证实。
- **[未证]**：可检验但现有材料不能确认的解释。

原始输入位于仓库根的临时目录 `.scratch-hw/`，不随本文提交。两份文本带 UTF-16 BOM，以下行号按 UTF-16 解码后的文本计算：

| 代号 | 文件名 | SHA-256 |
| --- | --- | --- |
| A，修复前 | `contract-out-20261020-135643.txt` | `b7b6d12a3b69b2d3c49ae1e21be27a1031b074acb1f40899d2b5130d70ea4c70` |
| B，WL1 后 | `contract-out-20261020-172403.txt` | `13a74b4d8cfc8e27cda01cfabfc4dd95e5c58e78b4361735b3592676a9dafc32` |

时间原点 `t=0` 是 `GuardEvents.startup_started`。`phase=startup` 的外层 `elapsed_ms` 是距此原点的时间戳，`reason` 内的 `SendInput … elapsed_ms=20` 才是调用耗时。[计时实现](../../crates/gcu-driver-windows/src/input/hotkey.rs#L154)使用 `as_millis()` 截断，不能据整数差推导亚毫秒先后。

**不能从这两份文本恢复所有事件的实测绝对毫秒。** `rpc_perform start`、`stop_listener failed`、`worker_kill start` 没有绝对时间；`rpc_perform end=73/72`、`worker_kill end=11` 是时长。[Recorder](../../crates/gcu-driver-windows/tests/contract/timings.rs#L76)未记录线程、回调时刻或 tick；`native_parent_timings` 仅保留各 stage 最近一个值，不是完整时间线。以下先列可直接恢复的毫秒，再给明确带条件的逐 tick 模型，不能把后者当成已测时间戳。

## 一、C06 毫秒时间线

### 两次运行的直接记录

两次矩阵均选择 `Run { probe: "absolute_same_position", marked: true }`（A/B 第 49–50 行）。[契约入口](../../crates/gcu-driver-windows/tests/contract/live.rs#L128)将此设置为仅 debug/test 接受的 `GCU_HOOK_MOUSE_PROBE`，再创建真实 monitored Worker。因此这里走真实监听/发送路径，但**不是默认四候选链的完整验收**，不能把本次延迟归因于又跑了三种零探针。

| 事件 | A：t / 原文行号 | B：t / 原文行号 | 证据和解释 |
| --- | --- | --- | --- |
| 监听、Default 桌面、热键与双钩子安装 | 0–1ms / 53–65 | 0–1ms / 53–65 | [日志] 注册、安装成功，初次队列已泵空 |
| 初始周期尚无证明 | 1ms / 66–67 | 1ms / 66–67 | [日志] `dispatch=None`，双侧均未见，attempted=0 |
| 初始周期报告拒绝 | 22ms / 68 | 21ms / 70 | [日志] generation=0，`mouse=Err(initial input idle unknown/below 2000ms, tick wrap, or physical input during probes)`、`dispatched=false`、keyboard=(0,0,0) |
| C06 观察和只读目标预检 | 280–288 行；无绝对 t | 287–295 行；无绝对 t | [日志] snapshot/capture/geometry/list_windows 分别耗时 113/393/35/36ms 与 117/347/37/37ms；不能直接把这些时长当作 t |
| 动作期鼠标 SendInput 开始，记为 S | **8000ms** / 289 | **7530ms** / 296 | [日志] `SendInput pending`；这是相隔数秒后的动作请求，不是原始发送卡了数秒 |
| 首次动作鼠标回调已经到达，发送返回；开始键盘发送 | **8021ms** / 290–292 | **7550ms** / 297–299 | [日志] mouse_probe_flags=1；鼠标 sent=1/1，Win32=0，耗时20ms；`pending=true`、`mouse_seen=false` |
| 首次动作键盘回调已经到达，发送返回 | **8041ms** / 293–294 | **7571ms** / 300–301 | [日志] keyboard_seen=true、flags=144；键盘 sent=1/1，Win32=0，耗时20ms；仍 pending=true、mouse_seen=false |
| 动作证明完成并放行；rpc_perform 开始 | 无绝对 t / 295，sequence=90 | 无绝对 t / 302，sequence=93 | [代码] 必须在动作报告被泵取走、鼠标证明提交、租约新鲜之后；[模型] 约 **8061ms / 7591ms**，而非直接等于上一行的8041/7571 |
| 过期、Stop、worker 终止 | 无绝对 t / 296–299 | 无绝对 t / 303–306 | [日志] 同为 `low-level hook probe expired (possibly silently removed)`；worker kill各耗时11ms；[模型] 过期约 **8121ms / 7651ms** 附近，须满足实际年龄严格>100ms |
| rpc_perform 失败并结束 | 300–301，耗时 **73ms** | 307–308，耗时 **72ms** | [日志] `operation cancelled (maybe_executed=true)`；[模型] 若采用上面的放行时刻，结束约 **8134ms / 7663ms** |
| C06 夹具所见 F24 抬起 | 302 行：1792475820026、1792475820110 | 309 行：1792488258419、1792488258501 | [日志] 各两次，间隔 **84ms / 82ms**；这是夹具墙钟，不能无校准地换算为监听 t |

**[代码] 初始探针是否发出：**初始 Job 被处理过，但鼠标 `SendInput` 没有调用，键盘也没有发送。`dispatched=false` 与 keyboard=(0,0,0)支持这一点；拒绝文案不能区分究竟是空闲不足、读取未知还是 tick 问题。它不是已经发出、到8秒才完成的探针。到C06时报告中的 `generation: 0` 只是 `Sender.report` 留下的**上一份报告**；`pending=true` 表示新 Job 尚未被收报告，不能将报告代次套给正在发送的 Job。

**[代码] A也需要动作期证明。** WL1前 `begin_foreground` 虽允许复用新鲜初始租约，但本次初始周期根本没有双证明，`available()`为false；采纳首个前台动作的generation=1后才可能发出8000ms这轮输入。B又用proof_floor明确隔离第0代。两次都不能仅因快照里残留generation=0，就把C06输入归为初始周期。

**[代码] 为什么 flags=1 却 mouse_seen=false：**鼠标回调先写 `Attempt.callback`；`mouse_seen` 读取的是 `MouseProof.proven()`。后者要等两次发送返回、光标复读成功、报告被监听线程 `try_recv` 并 `complete()` 后才提交。因此8041/7571时“两个回调已来”与“成对证明尚未提交”可以同时成立。下一次提交触发 `armed` 后，`startup_progress()` 提前返回；后续没有新的派发行**不能证明没有续租发送**。

**[代码＋日志] 已开始续租的旁证：**C06 夹具记录两次 F24；本路径只有探针发送 F24 up，C06本身是网格点击，初始周期没有发键盘。第二次 F24 高度支持动作期间至少发出了下一轮键盘探针；按发送顺序，前面的鼠标发送及光标复读已成功。它不能单独证明第二轮鼠标的带标记回调被接受，更不能证明续租报告已提交。

A与B的动作发送起点相差470ms，鼠标返回相差471ms，键盘返回相差470ms；局部40–41ms串行发送结构相同。RPC仅差1ms，F24间隔仅差2ms，kill均11ms，汇总均为6通过/7跳过/1 not_run/16失败。[日志] 两份 `FIRST FAILURE` 都是较早的C04像素问题；不能把全部16个失败都算成C06根因。Stop之后的依赖用例失败关闭则符合守卫代码。

### 最吻合的逐 tick 模型：完成、首次续租与过期

定义 `M0/K0` 为首轮动作鼠标/键盘回调，`M1/K1` 为首次续租回调；P0为C06首次提交探针的监听轮次。下表使用20ms名义轮询，**每个≈值都是[模型]**，只用于解释上述实测点、72/73ms RPC和82/84ms F24间隔；睡眠、FFI、日志及线程竞争会增加偏差。

| 监听轮次 | A约t / B约t（ms） | `confirm_hook_proofs` 的输入与 `at` | `now-at` / 相位 | 这一轮发生什么 |
| --- | --- | --- | --- | --- |
| P0 | 8000 / 7530 | keyboard=None，mouse=None；无at | Proving | 采纳动作代次，提交首轮Job；发送线程进入鼠标SendInput |
| P1 | **8021 / 7550** | keyboard=None，proven=None；无at | Proving | 泵处理M0；发送线程返回鼠标，再发F24；M0仍是tentative |
| P2 | **8041 / 7571** | keyboard=K0，proven=None；无at | Proving | 泵处理K0；`try_recv`尚空，符合pending=true日志；发送线程随后回报 |
| P3 | 8061 / 7591 | keyboard=K0，proven=M0；at=M0≈8021/7550 | 约40ms；Proving→Injecting | 取报告、complete、confirm、唤醒动作任务；动作开始RPC；监听若先看到仍在Proving，`preparing_with_mouse`禁止续租 |
| P4 | 8081 / 7611 | K0、M0；at=M0 | 约60ms；Injecting | 再确认旧对尚新鲜；随后提交首次续租Job并清keyboard；此时旧租约只余约40ms |
| P5 | 8101 / 7631 | keyboard=None，**proven仍为M0**；无at | Injecting | 新鼠标回调M1只写active.callback；报告未到，不能提交新鼠标证明 |
| P6 | 8121 / 7651附近 | keyboard=K1，**proven仍为M0**；at=min(K1,M0)=M0 | **实际>100ms**；Injecting | 新键盘回调已来，但若报告尚空，用新键盘配旧鼠标执行确认，返回expired，监听fail/Stop |
| P7（未执行的成功提交点） | 8141 / 7671附近 | 原本才会complete并用K1/M1 | 已晚于M0+100ms | 已失败，迟到报告不得复活守卫 |

这个模型中，`M0→第一次confirm`消耗约40ms，Proving到Injecting交接再消耗一轮约20ms，续租的鼠标与键盘各等一轮约20ms。因此第二次键盘到达时已在旧鼠标时刻+100ms边缘，而完整报告提交还需一轮。整数毫秒相等不代表未超过100ms：判断使用 `Instant/Duration`，不是已截断的日志数字。

**模型边界：**若发送线程能在P2/P6同一轮 `try_recv` 前交回报告，或P3确认后监听立即观察到Injecting并提交续租，可以少花一轮，结果可能不同。现有文本不能证明P6就是实际轮号，也没有记录M1或 `try_recv(Empty)`；它证明的是第一次报告在P2仍未收，以及后来确认确实拿到一对年龄>100ms的时刻。不能把8121/7651写成实测触发时刻。严格可写的触发约束为 `F > min(K,M)+100ms` 且 `rpc_start < F < rpc_end`；若旧鼠标确为本模型的M0，则 `F > M0+100ms`。夹具F24间隔与模型相符，但不能替代监听单调钟插桩。

## 二、代码调用链与线程归属

### 谁发起证明和续租

1. **动作任务发起窗口，不执行SendInput。** [WorkerDriver::perform_until](../../crates/gcu-driver-windows/src/worker/driver.rs#L60)先做只读Geometry/ListWindows目标门，再await `begin_foreground`，之后才开始mutation RPC。[lease.rs](../../crates/gcu-driver-windows/src/input/hotkey/lease.rs#L101)串行化动作、递增generation、置Proving；WL1新增 `proof_floor=generation` 并丢弃旧租约。它等待 `changed`；`available()`通过才置Injecting，再同步检查后返回。
2. **真实监听线程发起实际探针Job。** 原生实现没有安装 `proof_request` 回调；那是fake接缝。`gcu-win-hooks` 每轮先 `PeekMessageW`，再进 [Sender::poll](../../crates/gcu-driver-windows/src/input/ffi/safety/probe.rs#L103)：取报告、complete、adopt、confirm，然后判断是否 `next/try_send`。P0由此采纳动作代次并提交首轮Job；不会由 `begin_foreground` 直接唤醒Win32消息泵。
3. **独立发送线程执行FFI。** `gcu-hook-probes`阻塞在 `receive.recv()`，有Job立即处理；动作Job走 `dispatch_for_action`，绕过初始2000ms门，仍验证桌面、授权、光标和发送计数。先鼠标，再复读光标，成功且仍授权才F24，最后回报告。它没有20ms睡眠或周期定时器。
4. **监听线程负责续租。** `action=true`包括Proving和Injecting，但 `preparing_with_mouse`会阻止已证明、尚在Proving时重复发；一旦Injecting且无pending，`MouseProof.next()`选择已成功的preferred种类。绝对移动已成功就直接再次选择它，不重跑三个零候选。每次提交会清keyboard，**不会清掉上轮proven**；新鼠标回调在active未complete时也不替换proven。这正是P6可能混合K1/M0的条件。
5. **动作/释放完成后关闭续租窗口。** lease持有至RPC和输入恢复完成；正常finish或放弃后的恢复任务才 `end_window/discard_lease`。动作RPC已经开始不代表点击已经发送，也不意味着停止续租；目标准备、采样、等待provider及释放都占用Injecting窗口。

[hotkey/startup.rs](../../crates/gcu-driver-windows/src/input/hotkey/startup.rs)由 `#[cfg(test)] mod startup`引入，是可控时钟测试，不能当作真实启动线程实现。WL1的 `late_initial_probe_pair_overlapping_first_foreground_does_not_latch_stop` 手工调用确认边界；它不运行真实PeekMessage、两次SendInput、报告通道及20ms睡眠，因而其通过不能覆盖上表的提交时序。

### expired由哪个调用触发

**[代码，可确定]** 两次原始错误的完整字符串只来自 [hotkey.rs:confirm_hook_proofs](../../crates/gcu-driver-windows/src/input/hotkey.rs#L121)。只有键盘和鼠标都是Some时才计算 `at=min(keyboard,mouse)`；`now=self.time.now()` 是当次确认的单调时刻，绝非8041/7571这个诊断快照固定值；只有 `now-at>100ms` 才读取 `injecting()`。观察到该Err必然意味着该次读取为true。WL1后的调用还必然没有被 `cycle < proof_floor` 提前过滤。

此函数在真实路径只由 `Sender::poll`调用，因此**抛出原始expired的是 `gcu-win-hooks` 的确认调用**。它沿 `Pump::poll` 返回Err，外层监听循环 `monitored.fail(reason)`，先禁用clock/撤销健康，再锁存并通知独立supervisor，最后记失败日志。不是 `gcu-hook-probes` 在SendInput里抛出，也不是 `gcu-stop-watch` 的tick直接产生这个字符串。

对P0/P1/P2/P5，模型中不足双Some，所以没有at，也不会执行expired分支；P3/P4的at为M0、年龄约40/60ms，不会实际调用 `injecting()` 那一分支。P6若报告仍空，at仍为M0、now约8121/7651附近且精确年龄>100ms，实际 `injecting()==true`，返回Err。**实际第几次确认、实际now与M1是否已到，现有日志没有记录，属于模型而非可确定事实。**

独立健康检查位于 [input/watchdog.rs](../../crates/gcu-driver-windows/src/input/watchdog.rs)（本树没有 `hotkey/watchdog.rs`）。`gcu-stop-watch`每20ms调用 `check_health`，动作任务的 `check/available/finish`也会同步调用它。租约缺失会产生另一条 `low-level hook probe expired during foreground injection`；传输死亡和pending FFI超时又是其它文案。两次记录均为确认分支的文案，说明它赢得了首次 `failed.swap`，**不能反推另一个线程未接近超时**，也不能用文案证明OS确实静默摘钩。

`PhysicalClock.confirm(at)`保留回调时刻，`available()`要求年龄≤100ms；P3不会把租约起点改成8061/7591。以模型的旧鼠标为准，租约在实际 `M0+100ms` 后耗尽，即A约8121ms、B约7650ms之后；不是“RPC开始后再有100ms”，也不是“F24回调/泵心跳又续了100ms”。在新鼠标证明提交之前，新键盘或tentative鼠标均不能续租。

**[代码＋日志] 后果：** `armed`已由首轮可用租约置true，fail会 `clock.disable()` 并 `request_stop()`；`bind_stop`连接独立 [supervisor](../../crates/gcu-driver-windows/src/worker/supervisor.rs#L178)，先kill再清父账本。mutation在途被取消时按已dispatch状态返回maybe_executed=true。这表明执行结果不确定，不证明网格点击发生。TestStop/驱动锁存不能由下一次动作或重启worker清除；真实Host持久化是另一个集成边界，本日志不等于已经验收持久化。

## 三、产品SendInput为什么约20ms

**主要解释：[代码＋模型] 等待钩子线程下一轮泵，而非回调自身计算20ms。** [Pump::poll](../../crates/gcu-driver-windows/src/input/ffi/safety.rs#L200)只有每轮前半段进入 `PeekMessageW`；[外层监听循环](../../crates/gcu-driver-windows/src/input/hotkey.rs#L339)在poll后无条件 `sleep(POLL_INTERVAL)`，该常量为20ms。Job在这一轮泵完之后才提交，发送线程通常在钩子线程开始睡眠时调用SendInput；低级钩子回送到安装线程，需要它再进入消息泵才能完成。鼠标发送被下一次泵解阻，发送线程接着发键盘时，钩子线程又可能已经泵完并再次睡眠，于是第二次SendInput再等约20ms。两个发送串行，回报告又可能错过本轮 `try_recv`。

证据逐项检查：

| 可疑环节 | 代码事实 | 本次能推出什么 |
| --- | --- | --- |
| 注入回调慢操作 | [keyboard_hook/mouse_hook/callback](../../crates/gcu-driver-windows/src/input/ffi/safety.rs#L129)只读flags/tag/Instant、线程本地RefCell、更新Attempt/keyboard、ProbeIdle原子，然后CallNextHookEx | 没有本仓库的sleep、日志、COM、磁盘/网络IO或通道等待。匹配注入探针不走 `PhysicalClock.event` 的Mutex；普通非注入事件才可能取该锁 |
| 回调跨线程锁 | 匹配探针路径不取Window/诊断/PhysicalClock锁；RefCell使用try_borrow_mut，不等待 | 不支持“探针回调锁住20ms”；借用冲突会丢本次观察，不是20ms阻塞。外部钩子链CallNextHookEx的耗时尚未测量 |
| 发送计时范围 | [send](../../crates/gcu-driver-windows/src/input/ffi/safety/probe.rs#L274)先startup_progress，再开始Instant，包住SetLastError/SendInput/GetLastError，然后读elapsed，再写结果日志 | 20ms包含Windows等待回调的时间，不包含前后日志、DPI/桌面/光标查询、Job排队或报告排队；不能把日志耗时直接说成这个elapsed |
| 发送线程节奏 | 收Job→鼠标→复读→键盘→reply，单发送线程、容量1通道；无sleep | 有跨线程等待及串行依赖，但不是发送线程每20ms醒一次。真正固定间隔在钩子泵；报告发送/取走还有调度竞争 |
| 诊断日志与锁 | startup_progress持BTreeMap锁更新后释放，再同步tracing；契约subscriber打印并flush，另有短Mutex记录最近stage | 钩子**线程的poll**和发送线程都可能被日志延后，但回调本身不打日志。armed后startup_progress不再输出，不能凭缺行排除续租；本次无日志阻塞耗时证据 |
| 桌面/DPI/光标查询与hook replacement | 查询在sender或poll中，不在本仓库的探针回调里；钩子每1秒替换 | 它们可消耗额外预算；没有各调用耗时，不能断言是本次主因 |

对照证据：无钩子独立SendInput中位数0ms是任务提供的既有测量，不是本文复测。两份文件第43行的marked绝对探针分别为发送1350/1051µs、回调2267/5048µs，坐标各行内相同。[矩阵实现](../../crates/gcu-driver-windows/tests/contract/unsafe_mouse_probe.rs#L173)每2ms泵一次；它也使用独立发送线程，却没有产品20ms睡眠。该差异足以解释2–5ms与20ms的量级差。矩阵elapsed_us约300ms是留观窗口，不是SendInput耗时；marked B回调5048µs晚于send_elapsed_us=1051也说明不能要求每次SendInput返回与本钩子回调完全同步。20ms等待机制高度吻合产品记录，具体阻塞栈和每段耗时仍需测量。

## 四、独立根因判断、可证伪实验

### 1. 动作期续租的调度与提交消耗旧租约，混合新键盘/旧鼠标触发确认过期

**置信度：高（代码机制）；中高（这两次实际执行恰为P6模型）。** 初始周期没有派发输入，C06确有新双侧回调及mutation RPC，之后又有第二个F24。最符合全部证据的是：成对证明花了两次20ms泵＋报告取走的一轮；Proving交接错过一次续租机会；续租又花两次20ms。即使钩子继续投递，待验证的新鼠标不能替代旧proven，确认边界看到100ms外的旧鼠标就必须fail-closed。WL1只隔离动作代次，不隔离同一动作中各次Attempt的成对提交，也没有去掉这些等待。

**证伪实验：**总控在一次受控C06记录同一单调钟的generation、probe tag、M/K回调入口、sender前后、post-cursor、reply入队、try_recv结果、confirm的K/M/at/now/phase、旧confirmed及有效截止点。回调只写有界内存记录，由其它线程导出，避免同步打印改变调度。若首次失败at来自第0代，或当前续租完整验证已在旧截止前提交且确认仍使用旧对，或根本没有所述交接/收报告等待，则本模型被证伪，应按实际时序重定位。另以可控sender/report屏障重现“新鼠标tentative、新键盘已到、报告下一轮才到”，覆盖100ms边缘；仅注入一对同时为now的fake确认不能检验此路径。

### 2. 日志、桌面查询、调度抖动等额外延迟使当前动作证明变旧

**置信度：未证，中低；可与第1条叠加。** synchronous打印/flush、每轮桌面检查、1秒换钩与OS调度均可能加时；现有80余ms的F24间隔并不要求出现百毫秒级慢回调。这是预算紧张时的可能放大因素，不能凭20ms常见值指定某把锁或某个Win32调用为根因。

**证伪实验：**在不改变常量和失败条件下，对上述poll阶段记录起止及线程调度；将详细诊断改为受控内存采集做对照。若总额外延迟始终很小、仍按第1条失败，则“额外慢操作是必要原因”被证伪；若单段长阻塞主导，则应单独修该段，不能用加长租约盖住。

### 3. 真正静默摘钩或外部钩子链阻塞

**置信度：低；日志无直接证据。** 错误文案中的possibly是解释，不是Windows确认。第二次F24至少说明键盘输入继续到达夹具；两次矩阵也证明这台环境能收到marked鼠标。它们不排除随后鼠标钩子独立丢失，或CallNextHookEx受外部链影响。

**证伪实验：**在失败附近按当前tag检查M1/K1是否均到本钩子并完成发送/光标验证；若齐全且赶在旧截止前，本次“钩子不投递”就被证伪。若缺M1而消息泵与发送健康，则沿原fail-closed路径保留失败，并另做受控摘钩/外部链对照；不能以重新注册成功或泵心跳作为存活证明。

## 五、不改常量、不放宽fail-closed的修复方向

以下只提出方向，未实现，也未验证Windows可用性。

1. **使现有钩子线程及时服务消息和证明完成事件。** 维持独立发送线程、独立热键、FFI外看门狗及全部时限，用消息/Job/报告完成的事件唤醒同一钩子线程，避免“泵完才提交，随后无条件睡满20ms”。可评估Win32消息感知等待与现有20ms监督tick并存；不改POLL_INTERVAL，不新增无限线程或忙等。SendInput仍不能移回安装钩子的线程。安全收益是减少证明传递延迟；唤醒本身不构成租约，只有完整有效的双回调才能confirm。
2. **把Proving→Injecting与首次续租交接变成明确通知。** 完成首轮证明后不在Proving中反复注入，但进入Injecting应及时通知已有监听线程启动续租，消除等待下一轮才发现相位的空档。续租按旧 `min(K,M)` 的绝对截止点预留发送、光标验证、双回调与报告提交时间；窗口须持续到动作和release完成，随后空闲不续租。无法在原截止前完成时仍锁存、释放、关闭动作，不允许等待pending越过100ms。
3. **以同一Attempt的完整证明提交，保留之前已提交租约作为唯一有效租约。** generation仍隔离动作，第0代仍作废；新Attempt的K/M和报告应关联同一个tag，等待鼠标接受计数/光标复读/键盘计数及双注入flags验证后再提交，避免把K1与M0误称为一对新证明。数据结构/事件顺序改进不能把旧租约起点改为提交now，不能仅凭tentative M1续租，不能忽略同代过期。只把过期Err交给看门狗、或收新报告时掩盖已经跨越的旧租约空窗，都不成立。

只提前一轮提交首次续租并不保证解决：沿20ms模型，新报告可能仍在旧M0+100ms边缘才提交，没有调度余量。应一起评估消息服务、相位通知和完整证明提交的全程预算，而不是以“少了一次sleep”宣告真机已修复。

修复的安全前提仍为：真实当前动作双回调、较早者≤100ms；cursor变化/未知不补偿不重试；物理输入与系统空闲采样不续租；消息泵心跳不替代钩子证明；卡住FFI、丢失报告或回调时独立Stop/释放继续生效。验证必须同时覆盖正向连续续租和反向缺鼠标/缺键盘/延迟报告/摘钩/UAC/卡worker/按住输入Stop，不能以C06单次成功代替全部门槛。

## 六、与README及原ADR假设的差异和剩余未知

- [Windows驱动README](../../crates/gcu-driver-windows/README.md)、[产品中文README](../../README_ZH.md)和[diagnostics](../../crates/gcu-driver-windows/diagnostics.md)正确要求真实双回调、100ms与fail-closed。它们的声明是安全契约，不能作为“动作期续租已满足预算”的真机证据；驱动README中永久初始idle marker等描述还有历史实现措辞，应以当前动作绕门代码与ADR接线为准。本任务不修改它们。
- [ADR 0013](0013-windows-stop-guard-liveness-lease.md)历史预算讨论指出20ms泵会吃租约；方案B直接续工作种类去掉了无效零候选，但仍需计算**回调到成对提交、相位交接、下轮报告取走**的额外轮次。两次20ms SendInput不等于一轮完整续租只占40ms。
- [WL1初始重叠增补](0013-addendum-initial-probe-overlap.md)将约8秒发送解释成晚到初始对，并估计rpc约8041ms开始。本次独立分析不支持把这一解释用于这两份真机记录：generation=0报告明确dispatched=false、双侧均未见；C06的40ms发送后仍pending，rpc放行还需要报告提交。WL1 fake测试构造“旧初始键盘＋晚鼠标”验证的是可存在的代次隔离问题，不能证明该构造就是这两次失败路径。WL1在当前代次隔离上的安全价值与真机仍失败可以同时成立。
- **尚未证实：**原始confirm的实际at/now、触发tick编号、首轮/续租tag对应回调全轨迹、M1是否到达、reply是否恰好晚一轮、20ms内部阻塞栈、同步日志/桌面查询的贡献、是否发生真正摘钩、动作点击是否落地。尤其不能声称本文已完成真机复测或完整逐毫秒插桩。

## 检查与交付边界

本文的相对链接以本目录为基准；原始日志只记录临时路径与哈希，不建立依赖临时文件的Markdown链接。本次 `python3 docs/plans/computer-use-v2/reviews/check_plan.py`通过（14份计划文档、261个链接/锚点、83张卡）；本文20个相对链接及源码行锚点自查通过，`git diff --check`与新增文档的独立空白检查通过。只交付本文，`.scratch-hw/`未暂存、不得提交；STATUS与源码保持原状。未执行真机或Rust测试，本次是文档分析。

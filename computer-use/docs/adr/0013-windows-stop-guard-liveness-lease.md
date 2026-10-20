# 0013 Windows Stop 守卫在物理鼠标机器上的活性租约

状态：Accepted（2026-10-17 用户批准方案 B：开放问题 1a、2a、3a；实现按下方真机验证与测试策略逐项验收）。同日增补「前台门控与活动判定的接线」；1a/2a/3a 不重开。2026-10-20 的初始探针与首个前台动作重叠见 [增补](0013-addendum-initial-probe-overlap.md)（提议，不改变本决定的时限与 fail-closed）。
日期：2026-10-17

本文件只记录设计。不改 `Driver` trait、不改协议、不在本文件里写实现、不放宽 Stop 时限、不取消 fail-closed、不让空闲采样续租。

## 背景（事实 + 代码证据）

### 真机

2026-10-17，win-test 物理桌面、交互式 Default 桌面、未提权，忽略测试 `native_product_probe_chain_proves_both_hooks_without_cursor_change`（`crates/gcu-driver-windows/tests/hook_probe.rs`）失败。启动前先睡 2500ms，使启动动作本身不再算作最近输入。原始输出见该日诊断节；这里只保留与租约有关的事实。

1. 产品默认链尝试了 4 种鼠标探针。第 4 种 `AbsoluteSamePosition` 得到一对带标记的注入回调：`keyboard_seen=true`（flags 144 = `LLKHF_UP | LLKHF_INJECTED`，F24 抬起），`mouse_seen=true`（flags 1 = `LLMHF_INJECTED`）。`mouse_hook_proof` 为 “liveness proven by tagged injected callback (attempted 4 probe kinds)”。`hook_mouse_dispatch` / `hook_keyboard_dispatch` 均为 `sent=1/1`、Win32=0；这次绝对移动的 `SendInput` 自身耗时 19ms，键盘探针 20ms。
2. `guard.available()` 通过，且该次启动前后的物理光标坐标相等。
3. 随后空闲 250ms，第二次 `guard.available()` 失败。doctor 从 `user_activity_guard: available` / `stop_hotkey: available` 变为两者皆 `unavailable`。第一条失败原因是 `listener heartbeat expired (hook/message pump/FFI unavailable)`。守卫 fail-closed，并出现 `Stop latched; trusted Host resume required`。

同日更早的 10 行矩阵（`diagnostics.md`，XBUTTON 跳过）已说明：`SendInput` 接受 `zero_move` / `zero_wheel` / `zero_hwheel`，但它们不到达 `WH_MOUSE_LL`；`AbsoluteSamePosition` 与仅诊断用的 `RelativePair` 以 `LLMHF_INJECTED` 到达，且 `dwExtraInfo` 保留。那次矩阵运行时用户的鼠标在动，不能证明净位移。本次产品测试的光标相等只覆盖启动那一次，不覆盖之后的 250ms。

`armed` 之后 `startup_progress` 直接返回，失败快照仍是第一次证明的事实，没有第二次绝对移动的派发记录。因此这次输出证明“武装之后 250ms 内租约死亡并锁存”，不能单独证明第二次绝对移动已经发出。

### 代码

活性是租约，不是旧的最后输入时间（`input/hooks.rs`）。`PhysicalClock::confirm` 把调用方给出的时刻写成 `confirmed`，不写成“现在”。`ms_since_at` 在 `now - confirmed > HEARTBEAT_TIMEOUT`（100ms，`input/watchdog.rs`）时返回 `None`。`event()` 只在非注入回调上更新 `physical`，不调用 `confirm`。`input/ffi/safety.rs` 的钩子回调写明：物理回调只撤销第四发的门，不确认、不续 `PhysicalClock`。

`GuardEvents::confirm_hook_proofs`（`input/hotkey.rs`）只接受键盘与鼠标两个 `Some`，取较早的那个时刻。超过 100ms 时，尚未武装则忽略，已经武装则返回 `low-level hook probe expired`。`available()` 同时要求监听心跳、热键心跳和 `clock.available()`。看门狗每 20ms 调用 `check_health`：已经武装且 `available()` 为假，就 `fail("listener heartbeat expired ...")`。这条文案不区分“鼠标证明过期”和“消息泵心跳死亡”。`fail` 会 `clock.disable()`（迟到的 FFI 不能复活），并在已经武装时 `request_stop()`。监听循环看到 `failed` 后退出。

`SafetyGuard::check` 在前台且不可用时返回 `CapabilityUnavailable`；`is_stopped()` 时对前台和后台都返回 `Cancelled`。Host 在创建引擎前恢复持久 Stop，驱动从不清除该锁存（`gcu-driver-windows/README.md`，03 §7.7）。因此武装之后的一次租约失败，会变成需要可信恢复的停止，而不只是把前台能力关掉一会儿。

探针状态机（`input/hook_probe.rs`）：

- 默认顺序是 `ZeroMove → ZeroWheel → ZeroHWheel → AbsoluteSamePosition`。某次尝试有注入回调时，只有非绝对移动才会变成 `preferred`；绝对移动成功则 `preferred=None` 且 `index=0`，下一周期重新从 `ZeroMove` 开始。`input/hotkey/startup.rs` 的 `absolute_idle_gate_cannot_arm_or_renew_stop_without_fresh_dual_hook_proofs` 把这一点固定成测试。
- “One absolute attempt per proof cycle … never replay it” 只在**这一次绝对移动已经完成、但还没有回调**时返回 `None`。它不是“一个监听终身只发一次”。没有物理回调时，`ProbeIdle` 保持第一次采样的结果，下一个周期仍可再发一次绝对移动。
- `ProbeIdle` 在任何探针输入之前只采样一次 `GetLastInputInfo` / `GetTickCount64`。系统空闲 ≥2000ms 才允许第四发。自己的注入不清标记；任何非注入且非本标记的键盘或鼠标回调把标记永久打成 `DENIED`，本监听不能再采样。绝对移动在标记被拒、光标变化或派发期间出现物理输入时，`complete(Err)` 把 `MouseProof.failed` 钉死并清空 `proven`。
- 空闲采样和光标复读都不调用 `confirm`。这是既有测试的断言，不是疏忽。

派发（`input/ffi/safety/probe.rs`）在鼠标 `SendInput` 的返回计数符合预期之后，才发送 F24 抬起。低级钩子只在监听线程的 `PeekMessageW` 里运行。监听线程每次 `poll` 后睡眠 20ms（`POLL_INTERVAL`）。零位移在这台机器上不进入鼠标钩子，`SendInput` 立即返回，随后的键盘探针仍要等一次泵。绝对移动先堵住鼠标钩子，返回之后键盘探针再堵住一次；鼠标钩子返回之前，钩子线程不能去跑键盘钩子。`proven` 要到两次 `SendInput` 都返回、`complete()` 看到回调之后才更新。`confirm` 写入的仍是鼠标回调时刻，所以确认发生时，这 100ms 已经花掉了键盘那一泵。

由此得到的名义预算：上一次鼠标回调若比 `confirm` 早约一泵（20ms），其后三次不送达的零位移各一泵，再加下一次绝对移动的鼠标泵和键盘泵，下一次 `proven` 大约落在旧回调之后 120ms。看门狗在旧回调 +100ms 失败。若两次钩子挤进同一次 `PeekMessage` 循环，三次零位移加一次绝对移动约 80ms，边界上可以赶上。Windows 的睡眠粒度通常使 `sleep(20ms)` 更长，不会更短。设计没有一个完整轮询间隔的余量，而一次错过就永久 `disable` 并锁存。1s 的钩子重装（`REINSTALL_INTERVAL`）来不及参与这 100ms。

键盘证明不是这次的瓶颈：零位移也会刷新键盘时刻。`min(新键盘, 旧鼠标)` 仍是旧鼠标。只续键盘不能续租。`startup.rs` 已禁止“只有键盘证明就武装”。

### 对派工推断的结论

推断的后果成立，机制需要收紧。

成立：

- 在物理鼠标机器上，产品链里唯一能到达 `WH_MOUSE_LL` 且不把 `RelativePair` 编进发布构建的探针是同位置绝对移动。
- 绝对移动成功后，下一周期故意从三种零位移重新开始。这三种在该硬件上不产生鼠标回调，却占用租约。
- 物理回调和空闲采样都不续租。
- 本次运行在首次 `available()` 之后的 250ms 睡眠内失败，原因是武装后的 fail-closed，并且 Stop 已锁存。测试没有在这 250ms 内再读光标，所以不能把整段都证明成无人值守；按上面的泵预算，死亡点是约 100ms 加一次 20ms 看门狗，而不是 250ms 本身。
- 锁存经 Host 持久化后，新引擎默认不投递。可信恢复若只是再启动同一监听，会再次武装并再次锁存。此类机器上依赖该监听的前台输入不可用。

过强或不准确：

- “除首个周期外没有任何不改光标、不依赖物理输入的续租办法”不准确。每个新周期仍会再排一次 `AbsoluteSamePosition`，只要 `ProbeIdle` 仍是 `ALLOWED`。“never replay” 禁止的是同一周期内无回调后再发，不是终身一次。
- “必然”应限定为：名义调度落在 100ms 之外或贴住边界；本次真机没有赶上；失败不可恢复。不能从这一次输出证明所有调度都发过第二次绝对移动。
- 推断没有写出第二条独立死法：用户的真实输入会永久拒绝以后的绝对移动，并把 `MouseProof` 标成失败。守卫要观察的那种输入，会拆掉唯一能续租的探针。即便跳过零位移、让绝对移动挤进 100ms，用户一碰鼠标或键盘，约 100ms 后仍然锁存 Stop，而不是返回 `user_active`。`ms_since_physical_input()` 在租约死亡后是 `None`，策略层的 `user_active`（空闲阈值默认 3000ms）根本不会发生。

这是 P2-07 的 100ms 双钩子租约，与 P2-09 在物理鼠标上测得的“零位移不到达钩子”叠在一起的结果。周期替换钩子（03 §5.2）仍然必要，但替换和消息泵心跳都不能证明回调还在投递。

## 问题定义

租约要保证的是：**在即将注入和正在注入前台输入的那段时间里**，Stop 热键与用户活动检测都是活的。

- Stop：`RegisterHotKey` 在独立线程，按下后 ≤100ms 锁存、≤500ms 发出释放、阻塞的 worker ≤5s 结束（03 §7.7，P2-07 DoD）。热键线程的 100ms 心跳证明的是这条线程还在泵消息，不是鼠标钩子还在投递。
- 用户活动：前台动作在 `user_idle_ms` 内有物理输入时返回 `user_active`；后台路径不受此限。鼠标钩子被静默摘掉时，只剩键盘钩子不能证明用户没有在动鼠标，所以鼠标钩子必须单独证明。证明物是带标记的注入回调，因为 Windows 没有低级钩子的活性查询；注册成功和泵心跳都不够（驱动 README 已写明）。
- 注入输入不得被记成用户活动，也不得用来续“用户空闲”的历史。`GetLastInputInfo` 含注入输入，所以它不能当 `PhysicalClock`。

空闲且没有注入在途时，**不需要**为了“看起来在线”而维持鼠标钩子租约。这段时间没有前台注入可伤害用户。需要的是：前台路线保持关闭（fail-closed）；用户自己的 Stop 热键仍然有效；热键线程或桌面/UAC 检查死亡时仍然锁存。把“鼠标证明暂时没有”写成持久 Stop，会把安全的空闲变成必须人工恢复的停机，这超出了上述保证。

正在注入时，鼠标证明变旧仍是监听失效：必须停止继续注入、释放已按下的输入，并锁存 Stop。这里的 100ms 是发现“钩子可能已经不在”的上限，不能加长。

## 建议的决定

采用方案 B，且只在用户书面批准“动作窗口内可以重复同位置绝对移动”之后实现。在那之前保持现状：物理鼠标机器上前台保持不可用，本次失败测试继续 `ignore`。

B 的边界：

1. 没有前台注入在途时，不要求 100ms 鼠标证明。仅因鼠标证明缺失不得 `request_stop()`，也不得 `clock.disable()`。`RegisterHotKey` 线程心跳、安全桌面、FFI 卡死、热键注册失败仍按现在的 100ms / 启动预算 fail-closed 并锁存。鼠标租约缺失不得再使 core 在 `perform` 之前拒绝前台；原「`available()` / `check(foreground)` 在证明前即 `CapabilityUnavailable`」由文末增补替换，安全结果不替换。
2. 放行一次前台注入之前，先取得一对新的带标记注入回调。本机零位移不送达时，这一对里的鼠标侧是一次 `AbsoluteSamePosition`（仍做光标复读；坐标变了就作废，不补偿、不重试）。证明失败则不注入、不锁存，调用方可以在用户重新空闲后重试。
3. 从前台注入开始，到 `release_all_input` 且动作结束，鼠标证明必须保持不超过 100ms。已经知道零位移不送达时，动作期间的续租直接用能送达的那种，不再把三次无效探针花进预算。错过则走现有 `fail`：锁存、释放，时限不变。
4. 物理回调仍只更新活动时刻，不续租。它不得再把 `ProbeIdle` 打成永久拒绝，也不得把 `MouseProof` 打成终身 `failed`。2000ms 空闲采样只约束“用户没有请求动作时的那次意外绝对移动”。用户正在被注入、或刚刚请求了前台动作时，续租用的绝对移动不再受这道永久门限制。
5. 动作结束并释放后，丢掉注入租约，不锁存。下一次前台动作重新证明。用户已经按下的 Stop、以及热键线程死亡造成的锁存，监听不得自行清除。
6. doctor 不得把泵心跳显示成钩子已证明。空闲且没有可信活动观测时，用户活动守卫报告不可用；已有可信物理回调时可以报告可用，但须另写一行说明注入租约未新鲜。见文末增补。

零位移一旦在某台机器上产生带标记回调，`preferred` 仍钉住它，动作用该种续租，不再发绝对移动。`RelativePair` 和 `XButtonPair` 继续排除在产品构建之外。

## 备选

### A — 每个续租周期都发同位置绝对移动

绝对移动成功后把 `preferred` 钉在 `AbsoluteSamePosition`，或在零位移已知不送达后跳过它们。监听存活期间持续发送，使 100ms 租约在空闲时也不掉。

- Stop ≤100ms / ≤500ms：时限数字可以保持。每次续租仍要在 100ms 内完成一次绝对移动加键盘探针；按两泵计算约 40ms，余量比现在的四段链好。错过仍锁存。
- 钩子被静默摘掉：仍靠真实回调发现，上限仍是 100ms。这是 A 相对 C 的优点。
- 用户误操作：现有 `ProbeIdle` 不改的话，用户一动就永久不能再发绝对移动，随后锁存 Stop，`user_active` 不会出现。要让 A 成立，必须同时改掉这个永久拒绝。即使用户只是在空闲时移动鼠标，进程也会继续往光标下注入移动。
- 系统空闲 / 锁屏：`GetLastInputInfo` 计入注入输入（现有第四发门已经按这个事实拒绝“采样前有过注入”）。以短于 100ms 的间隔持续 `SendInput`，会把系统空闲钟钉在“刚刚有输入”。屏保、自动锁屏和依赖该钟的睡眠可能在整个会话里被推迟。这是推断，win-test 没有测过钟的变化。
- hover：同位置、`MOVE_NOCOALESCE` 的绝对移动仍可能让光标下的窗口收到 `WM_MOUSEMOVE`（诊断已写 hover 可能发生）。光标坐标不变不能排除 tooltip、网页 `:hover` 和“鼠标移动”监听。空闲时大约每秒十次。

不选作默认。它把只为注入服务的证明变成常驻输入。用户只批准过启动链末尾的一次绝对移动；A 是进程寿命内的重复发送。

### B — 动作门控

见上一节。空闲时守卫可以不在线，前台关闭；动作前做一次新证明，动作期间续租，结束后释放。

- Stop ≤100ms / ≤500ms：注入期间不变。空闲时热键线程心跳仍是 100ms，热键死亡仍锁存。不增加任何时限。
- 钩子被静默摘掉：空闲时不注入，摘掉的鼠标钩子不能造成盲注入；下一次动作前的新回调若缺失，动作不开始。注入期间仍是 100ms 真实回调，泵心跳和 1s 重装都不算证明。被放弃的风险只限于“空闲时 doctor 不能声称钩子已证明”，不包含“空闲时照样注入”。
- 用户误操作：空闲时的物理回调只更新活动钟。3000ms 内的前台请求返回 `user_active`，不锁存。注入期间若收到物理回调，钩子是活的，按用户活动停止注入并释放，不把监听标死。注入期间若 100ms 内没有新的注入回调，无法区分摘钩和丢事件，锁存 Stop。证明时光标变化：动作还没开始则拒绝且不锁存；已经注入则释放，并且因为证明作废而锁存。
- 系统空闲 / 锁屏：空闲时不发探针，不额外拨动 `GetLastInputInfo`。动作自己的键盘鼠标仍会拨动，那是用户可见的注入。
- hover：只发生在前台动作的证明和续租里。纯键盘的前台动作也会在光标处产生同位置移动，仍可能触发 hover。窗口比 A 短，不是零。

### C — 加长租约，加上线程 / 消息泵健康和定期重装

把 `HEARTBEAT_TIMEOUT` 加到能盖住四段探针（至少约 150–200ms，算上睡眠抖动要更大），继续相信现有 20ms 泵心跳和 1s `SetWindowsHookEx` 替换。

- Stop ≤100ms：热键按下后的锁存路径可以仍走热键线程，但“钩子已经不在、我们还在注入”的窗口会随租约变长。那正是 100ms 租约在替 Stop 的响应预算看守的部分。加长它就是放宽这道界限，本 ADR 不采用。
- 静默摘钩：Windows 没有钩子句柄的活性查询。泵心跳只证明线程醒着；重装只证明 `SetWindowsHookEx` 又成功了一次。两者现在就存在，都不能替代回调。若重装之后必须先见到一次能送达的回调才允许下一次注入，那是 B 加上现有重装，不是更长的租约。只加长租约、又没有能送达的探针，等于在加长的窗口里检测不到摘钩。
- 用户误操作、空闲钟、hover：四段链若只是被更长的租约罩住，`ProbeIdle` 的永久拒绝还在；空闲钟和 hover 取决于是否仍发绝对移动。少发可以减轻后两者，同时更看不见摘钩。

### D — 其它

**D1. 零位移不送达就钉住绝对移动。** 这是 A 或 B 的实现手段，不是独立方案。它仍重复绝对移动，并且在 `ProbeIdle` 不改时仍被用户的第一次物理输入拆掉。

**D2. 改用 `WH_MOUSE_LL` 以外的物理输入观察（Raw Input）。** `RegisterRawInputDevices`（通常还要 `RIDEV_INPUTSINK` 才能在非前台收到）可以在不注入移动的情况下看到物理鼠标和键盘，因此空闲钟和 hover 不受探针影响。它证明不了低级钩子还在。03 §5.2 点名的是 `WH_MOUSE_LL` / `WH_KEYBOARD_LL` 加周期替换；更换传感器要等本 ADR 被接受，不能在实现里悄悄换。`RAWINPUT` 没有 `dwExtraInfo`，也没有 `LLMHF_INJECTED`。我们自己的 `SendInput` 同样产生 `WM_INPUT`。用时间相关性把“我们的注入”从“用户”里减掉是竞态，减错会把注入记成用户活动。`RIDEV_NOLEGACY` 会改变其它程序看到的传统消息。Raw Input 自己的丢失也要有不长于 100ms 的检测。在用户拒绝重复绝对移动、又拒绝“物理鼠标机器前台不可用”时，再单独立项做 spike。本决定不实现 D2。

**D3. 维持现状。** 不再发第二次绝对移动。fail-closed、100ms、永久锁存全部保持。物理鼠标机器上，监听至多武装一次，约 100ms 后锁存 Stop，前台不可用。这是安全的，也是在用户不批准重复绝对移动时的唯一选择。

不采用：放宽 Stop 的 100ms / 500ms / 5s；取消 fail-closed；让 `GetLastInputInfo`、光标复读或 `ProbeIdle` 本身续租；把 `RelativePair` 或 `XButtonPair` 放进产品续租。

## 影响对照

| 方案 | Stop ≤100ms 锁存 / ≤500ms 释放 | 钩子被静默摘掉 | 用户抢输入 | 系统空闲 / 锁屏 | hover |
|---|---|---|---|---|---|
| A 持续同位置绝对移动 | 时限可保持；每 <100ms 一次探针，余量约一两泵 | 100ms 内靠回调发现 | 不改 `ProbeIdle` 则会锁存 Stop，而不是 `user_active` | 可能把空闲钟钉死；未实测 | 会话期间持续可能发生 |
| B 动作门控（建议） | 注入期间时限不变；空闲时热键死亡仍 100ms 锁存 | 空闲不注入；注入期间仍要回调 | 空闲时 `user_active`；注入中失去回调则锁存并释放 | 探针不在空闲时拨钟 | 仅动作窗口；纯键盘动作也会有一次同位置移动 |
| C 加长租约 + 泵 + 重装 | 发现摘钩的时间变长，拒绝 | 泵和重装证明不了投递 | 与现在相同，仍然会误锁存 | 取决于是否继续发绝对移动 | 取决于探针 |
| D2 Raw Input | 要另做丢失检测才谈得上 100ms | 不证明 LL 钩子 | 注入与物理的区分尚未有可靠做法 | 探针本身不拨钟 | 探针本身不产生移动 |
| D3 现状 | 全部保持 | 保持，但续不上租 | 约 100ms 后锁存 Stop | 不额外拨钟 | 只有启动时那一次 |

接口：不改 `Driver`、协议错误码、`user_idle_ms` 或 Stop 和弦。变化在 Windows 监听何时武装、何时锁存，以及 doctor 对“空闲且未证明”的报告。核心的 `user_active` 语义不变；B 让 `ms_since_physical_input()` 在钩子确有物理回调时仍能返回 `Some`，这是该语义在物理鼠标机器上第一次变得可达。

## 与已批准约束的关系

用户只批准过**一次**同位置绝对移动，作为启动链的最后一种探针，并且有 2000ms 空闲门、光标变化即作废、不补偿。下面每一种重复发送都超出该批准，实现前必须另批：

| 方案 | 是否重复发送 `AbsoluteSamePosition` | 需要新的用户批准 |
|---|---|---|
| A | 是，监听存活期间持续发送 | 是 |
| B | 是，每个前台动作开始时一次，动作未结束则每 <100ms 一次 | 是 |
| C 若仍用绝对移动续租 | 是，频率随加长后的租约下降 | 是 |
| D2 | 否 | 传感器更换本身要批准；不批准重复移动 |
| D3 | 否，保持已批准的一次 | 否 |

`ProbeIdle` 的“物理输入永久否决本监听”也是已落地的安全门。B 把它收窄为“只约束动作之外的意外绝对移动”，这是第二项需要用户确认的行为变化。不确认这项的话，B 在用户第一次碰鼠标后就无法再证明钩子。

03 §5.2 / §7.7 的钩子种类、过滤注入标志、周期替换、Stop 时限和“监听失效则禁止依赖它的投递”都不改。B 把“失效”落成：热键/桌面/FFI 失效，以及**注入期间**鼠标证明失效。空闲时缺少鼠标证明只关闭前台，不伪装成用户按下了 Stop。

## 真机验证与测试策略

实现（另卡，不在本 ADR）时，确定时钟单测用现有 `ManualTime`，不碰 Win32。至少这些用例，名字沿用 `<行为>_<条件>_<期望>`：

- 零位移 `SendInput` 成功但无鼠标回调，绝对移动的回调比旧证明晚 4 到 5 个 20ms 泵：现有状态机在新 `proven` 写入前 `available()` 变假、`is_stopped()` 为真、`disable` 后迟到回调不能再武装。这把本次真机失败收成单测。
- 空闲超过 100ms、热键心跳仍新鲜、鼠标证明缺失：不锁存、不 `disable`。传输检查不在证明前返回 `CapabilityUnavailable`（原断言由文末增补替换）；尚未取得新证明时不得发出动作本身的注入。
- 没有新的一对回调就不得开始前台注入。
- 动作在途时把时钟拨过 100ms 且没有新鼠标回调：同步锁存，并走现有释放路径。断言不得出现新的、更长的定时器。
- 物理回调更新活动年龄，不移动 `confirmed`，不把下一次动作证明永久拒绝。
- 只有键盘回调不能武装，也不能在动作期间续租。空闲采样单独不能 `confirm`。现有 `startup.rs` 断言保持。
- 光标变化的证明在注入前拒绝且不锁存；注入后释放并锁存。不发送补偿移动。

必须真机、在实现合并前保持 `not_run` / `#[ignore]` 的项，只在无人值守的一次性 Windows 物理鼠标会话上跑，不在 build8 或 CI 托管 runner 上跑：

- 重跑 `native_product_probe_chain_proves_both_hooks_without_cursor_change`。实现前它应当继续失败；B 落地后，空闲 250ms 应仍关闭前台且不锁存 Stop。另加一段：空闲期间不调用前台注入，采样 `GetLastInputInfo`，空闲钟必须继续变老。
- 同位置绝对移动一次：记录移动前后的物理光标、光标下窗口的 `WM_MOUSEMOVE` 计数或 tooltip，以及 `GetLastInputInfo.dwTime`。坐标相等不足以通过。
- 用户在空闲时移动鼠标：下一次前台动作在 3000ms 内得到 `user_active`，Stop 锁存仍为假；超过空闲阈值且证明成功后才允许注入。
- 注入过程中摘掉鼠标钩子：≤100ms 锁存，≤500ms 发出释放。热键冲突、UAC 安全桌面、卡住的 worker ≤5s，保持现有用例。
- 屏保 / 锁屏是否被推迟：只在一次性桌面、且用户同意改超时的前提下测。否则只记录 `GetLastInputInfo`，把锁屏写成未验证。

## 开放问题

| # | 问题 | 选项 | 建议 |
|---|---|---|---|
| 1 | 能否在已批准的那一次之外再发 `AbsoluteSamePosition`？ | (a) 可以，但仅限前台动作窗口（B）。(b) 可以，监听存活期间持续发送（A）。(c) 不可以，维持现状（D3） | (a)。若否决 (a)，选 (c)，不要改走 (b) 或 C |
| 2 | 空闲且无注入时，鼠标钩子租约是否必须在线？过期是否锁存 Stop？ | (a) 不必在线；前台关闭；不锁存。热键线程死亡仍锁存。(b) 必须在线，过期锁存（现状，并迫使 A 或 C） | (a) |
| 3 | 物理回调是否继续永久禁止本监听的绝对移动？ | (a) 只禁止“没有动作请求时”的意外绝对移动；不把证明状态机打失败。(b) 保持永久禁止 | (a)。选 (b) 则 B 在用户第一次输入后失效 |

**已裁定（2026-10-17）：1a、2a、3a。** 实现须满足：动作窗口外不发任何重复探针；不加长 `HEARTBEAT_TIMEOUT`；Stop 锁存/释放时限不变。（原文：未决之前不写实现。三项的建议分别是 1a、2a、3a。）任一项选了另一边，就回到 D3，而不是自行把租约加长。

## 退出条件

- 无人值守的物理鼠标运行里，某种零位移探针产生带标记的 `WH_MOUSE_LL` 回调，并且现有忽略测试的第二次 `available()` 在 250ms 后仍为真：本 ADR 的硬件前提不成立，撤回，不引入重复绝对移动。
- 用户拒绝问题 1 的 (a) 和 (b)：接受 D3，前台在此类机器上保持不可用，诊断与 doctor 继续说明原因。
- 实测表明同位置绝对移动既不改变 `GetLastInputInfo`，也不产生 hover：A 的空闲钟和 hover 代价要改写。`ProbeIdle` 与 100ms 预算问题仍在，不因此改推 A。
- Raw Input 的 spike 若能在不把注入记成用户活动的前提下，用 ≤100ms 的丢失检测替换 LL 钩子证明：另写 ADR 取代本决定，并修改 03 §5.2 的点名机制。在那之前 D2 不是退路。
- 实现若需要把 `HEARTBEAT_TIMEOUT`、Stop 锁存或释放时限改大：停止实现，本决定不授权该改动。

## 增补：前台门控与活动判定的接线

日期：2026-10-17

本增补只补已接受方案 B 的跨 crate 接线。不新增探针种类，不把 `TargetedOnly` 升级成前台，不改 `Driver` trait，不改协议。开放问题 2a 的「前台关闭」是安全结果：没有新的一对钩子回调，就不得发出动作本身的注入，doctor 也不得把泵心跳说成钩子已证明。它不是 `ForegroundCapability::Unavailable` 这个枚举值。沿用该枚举值会让已接受的「动作前证明」不可达。

### 核实

两条都成立，而且第二条比「只改 Windows 钟的返回值」更远。

1. 前台请求到不了 `perform`。`WindowsDriver::capabilities` 与 `WorkerDriver::capabilities` 的前台位要求 `SafetyGuard::available()`。`available()` 要求泵心跳、热键心跳和 `PhysicalClock::available()`。后者就是 `ms_since_physical_input().is_some()`，而 `ms_since_at` 在 `disabled`、没有 `confirmed`、或 `now - confirmed > HEARTBEAT_TIMEOUT`（100ms）时返回 `None`。该位为 0 时 `watchdog::capabilities` 把 `delivery.foreground` 写成 `Unavailable`。`ActionRouter::execute_queued` 先调用 `route::plan`，之后才 `authorize`，再经 `browser::dispatch` 进入 `driver.perform`。`plan` 在 `intent.foreground && foreground == Unavailable` 时直接返回 `capability_unavailable`。即便有调用方绕过 `plan` 送进 `AllowForeground`，`WindowsDriver::perform`、`WorkerDriver::perform` 和 `InputBackend::perform` 仍先调用 `SafetyGuard::check(true)`，`!available()` 同样是 `CapabilityUnavailable`，并且发生在父进程把 `Perform` 交给 worker、以及 `InputBackend` 组包之前。证明写在这些检查之后就永远不会开始。

   产品能力还有一个相邻事实，实现不得用它来绕过：`with_input` 把 `targeted_pointer` 和 `targeted_keys` 设为真，安全覆盖层不关掉它们。`plan` 只在对应 targeted 能力为假时才选择 `AllowForeground`。因此当前产品组合下，过期租约不会挡住定向动作；驱动对无效的 `PostMessage` 返回 `CapabilityUnavailable`，不自行升级（`input/mod.rs`）。B 的证明只挂在真正的前台请求（`AllowForeground` / `Activate`）上。本增补不改这条路由，也不为了让证明跑起来而关掉 targeted。

2. `user_active` 不在驱动里。`policy/mod.rs` 第 9 条只在 `intent.foreground` 时读取 Host 的 `UserActivity::ms_since_physical_input`：`Some(elapsed)` 且 `elapsed < user_idle_ms` 才拒绝；相等的 3000ms 不拒绝；`None` 不拒绝。这发生在 `authorize`，也就是 `plan` 之后。`plan` 先拒绝的话，策略根本看不到这次请求。`DriverError` 没有 `UserActive`，`error_code()` 也不映射到协议里的 `user_active`。协议错误码已经有 `UserActive`（`ErrorCode::ALL` 共 26 个），它只应由策略发出。

   过期租约不是唯一的断点。没有一笔仍落在 100ms 内的上一次确认时（包括第一次 `confirm`），`confirm` 会把 `physical` 写成证明时刻；`event()` 才是非注入回调的活动时刻，而且不调用 `confirm`。`ms_since_at` 却先看租约，租约一过期就把仍在的 `physical` 读成 `None`。与此同时，产品 Host 根本没有读这个钟：路线 B 的 `engine::run` 固定注入 `UnavailableActivity`（永远 `None`），并在握手前把 `user_activity_guard` 写成 `false`；路线 A 的 `Activity::ms_since_physical_input` 固定返回 `None`。`gcu-mcp` 的 `PhysicalActivity` 是引擎侧注入点。ADR 0004 已决定 Host 协议不承载活动 RPC。`WindowsDriver::safety` 是 `pub(crate)`，`engine::run` 只有 `Arc<dyn Driver>`，外面拿不到 `SafetyGuard::activity_clock()`。所以只改 `gcu-driver-windows` 不能让「空闲时用户碰鼠标 → `user_active`」在策略层成立。

### 决定

采用下面的接线，不采用新增 trait 方法。三个谓词分开，不再共用一个 `available()`：

| 谓词 | 为真的条件 | 用途 |
|---|---|---|
| 传输健康 | 未 `fail`、泵心跳与热键心跳都在 100ms 内、非诊断、已附守卫。不含鼠标租约，也不含 Stop 锁存 | `delivery.foreground = Policy`。为假则仍是 `Unavailable`，前台请求在 `plan` 就被拒绝，并且不发证明。已锁存时位可以仍是 `Policy`，由策略第 1 条和 `perform` 第 1 步返回 `stopped_by_user`，避免把 Stop 报成能力缺失 |
| 注入租约 | 最近一次成功的双钩子 `confirm` 距现在 ≤100ms | 只允许「正在注入」继续。空闲过期不 `fail`、不锁存、不 `disable` |
| 活动可信 | 见下一节 | `user_activity_guard` 与 Host 活动年龄。与 100ms 租约无关 |

`armed` 仍只在传输健康且双钩子证明第一次同时成立时置上，不改成「只看泵心跳」。之后空闲掉鼠标租约不清除 `armed`。看门狗不得再因 `armed && 租约过期` 调用 `fail`。传输死亡且已经 `armed` 仍走现有 `fail`（未武装的启动失败不因此变成用户 Stop）。

前台 `perform`（父进程、拥有 `SafetyGuard` 的那一次；worker RPC 和 `InputBackend` 组包之前）按这个顺序，且同一次调用里证明最多发一轮：

1. 已锁存：现有 `Cancelled`，零注入。
2. 传输不健康：现有 `CapabilityUnavailable` 或现有 `fail` 锁存。不发证明。
3. 租约不新鲜：跑已接受的那一轮证明（零位移仍不送达时，鼠标侧是动作窗口内的一次 `AbsoluteSamePosition`，光标变化作废、不补偿）。时间算在本次 `perform` 已经开始的 `DRIVER_DEADLINE`（4750ms）里，不新开计时器，也不把外层 5s 重新起算。租约已经新鲜则跳过，避免外层 `perform` 和内层 `check` 各发一次。
4. 证明失败或光标变化：`CapabilityUnavailable`，`maybe_executed == false`，不 `request_stop`，不 `disable`。调用方可以在用户再次空闲后重试。
5. 证明成功后才允许动作本身的 `SendInput` / 激活。从这一刻到 `release_all_input` 且动作结束，续租规则与错过则 `fail` 的规则仍是决定第 3 点。结束后丢掉注入租约，不因租约变旧而锁存。

`doctor` 分成三行，不能再由一个 `available()` 生成：传输/热键、注入租约是否新鲜、活动回答是否可信。泵心跳不得写成钩子已证明。没有可信观测时 `user_activity_guard` 为假；覆盖区间内已有物理回调时可以为真，同时写明租约未新鲜。

活动年龄接到已有缝上，不新增 `Driver` 方法：

- `WindowsDriver` 增加只读访问器，没有守卫时返回 `None`。`PhysicalClock` 仍留在 Windows crate。
- `gcu-cli` 用一个只转发 `ms_since_physical_input` 的适配器实现已有的 `PhysicalActivity`。Windows crate 不依赖 `gcu-mcp`。
- 路线 B：`engine::run` 增加 `Arc<dyn PhysicalActivity>` 参数。组装点在仍持有具体 `WindowsDriver` 时传入该钟。未接线的调用点继续传 `UnavailableActivity`。只有传入了这个钟，握手才采用驱动自己的 `user_activity_guard`；fake 与未接线引擎继续强制 `false`。
- 路线 A：`Activity` 的 Stop 锁存与持久化不变，只把活动年龄委托给同一个可选来源；未接线时仍返回 `None`。
- 0012 尚未把真实驱动接进产品入口。本增补不打开该入口，不改 `require_fake`。接线类型和访问器随 B 的实现落地，由测试直接调用；产品进程在 0012 的组装卡传入该钟之前，行为与今天一样是 `None`。

`policy`、`route::plan`、`Driver` trait、fake 驱动都不改。策略第 9 条的比较保持原样。

### 活动年龄

`confirmed` 只由成功的双钩子 `confirm` 写入，并且 `confirm` 不得再写 `physical`。现在「确认落在 100ms 窗之外就把 `physical` 设成证明时刻」会把我们自己的探针记成刚刚发生的用户输入；解耦之后这行删除。`physical` 只由非注入、非探针标记的 `event()` 写入。

覆盖区间是「这一代监听」，不是 100ms 租约。一代从第一次成功 `confirm` 开始，到 `disable` / `fail` 结束。1s 换钩不结束这一代，也不清 `physical`：换钩不是证明，但换钩前已经见到的物理回调仍然是用户输入。`disable` 之后的 `confirm` 或 `event` 都不能复活读取；现有 `disabled` 短路保持。

| 读取 | 含义 | 谁消费 |
|---|---|---|
| `None` | 不能为年龄作保。从未成功证明、已 `disable`/`fail`、这一代还没有非注入回调，都是 `None`。第一次 `confirm` 之前的回调不入库 | 策略不发出 `user_active`。这不是「用户在空闲」，也不是注入许可 |
| `Some(ms)` | 覆盖区间内最后一次非注入回调距现在 `ms`。租约已过期也可以是 `Some`。`ms` 可以大于 100，也可以 ≥ `user_idle_ms` | `ms < user_idle_ms` 且请求是前台：策略返回已有的 `user_active`，`perform` 不会被调用，因此不会为了这次拒绝去发绝对移动。`ms ≥ user_idle_ms` 只表示「上次可信输入已经旧了」 |

钩子死了不得被当成空闲而放行。静默摘钩不会再有回调：年龄变大之后策略可能放行，但前台 `perform` 仍必须先取得新证明；证明失败则零动作注入、不锁存。热键线程、泵或 FFI 死亡走传输谓词：`foreground` 回到 `Unavailable`，`plan` 拒绝，活动读取为 `None`，并按现有路径锁存。两条路都不会在钩子已死时发出前台注入。`None` 单独不能打开注入。

因此「租约已过期，但钩子确有物理回调」时：覆盖未关、未 `disable`，读取是 `Some(回调年龄)`。前台意图在 3000ms 内得到 `user_active`。这不要求租约仍在 100ms 内，也不要求为了回答活动问题去发探针。

### 不新增错误，不改时限

不新增 `DriverError` 变体。证明失败与传输不可用都用现有 `CapabilityUnavailable`（`maybe_executed == false`）。用户抢输入只用策略已有的 `ErrorCode::UserActive`。不改 `gcu-protocol` 的错误码、`golden/errors.json`、`golden/tools.json`，也不动工具目录的 14000 字节预算。

`HEARTBEAT_TIMEOUT` 仍是 100ms。Stop 仍是 ≤100ms 锁存、≤500ms 发出释放。`NATIVE_TIMEOUT` 仍是 5s，`DRIVER_DEADLINE` 仍是 4750ms。证明和续租都不得另设更长的定时器。

### 与 0002、fake 和其它平台

0002 的做法是：新的 `DriverError` 变体或带默认实现的 trait 方法，旧驱动不用改行为。本增补不使用它。不增加 `prepare_foreground` 或任何默认方法，fake、macOS、Linux 的源文件不改。macOS 自己的 `ActivityMonitor` 在监听错过 100ms 时返回 `None`，那是另一套钟，不在本卡把两套语义并成一个。

若以后确实要给 `Driver` 加方法，必须像 0002 的 `discovery_status` 那样有默认实现，并且另写 ADR。本决定不授权。

### 备选

**(a) 能力位只反映传输，证明放在 `perform` 入口。** 这就是上面的决定。`plan` 和策略都不用改：传输健康时前台位保持 `Policy`，`plan` 能生成前台意图，策略先看到活动年龄，通过之后 `perform` 才证明。用户正在活动时不会为了拒绝去发绝对移动。

**(b) `Driver::prepare_foreground`，默认 `Ok`，core 在授权通过之后、路由之前调用。** 不选。实际顺序是 `plan` 先于 `authorize`，并且拒绝就发生在 `plan`。默认方法若放在授权之后，调用点仍然到不了。若为了保住 `Unavailable` 而把方法挪到 `plan` 之前，证明会跑在 `user_active` 之前，空闲用户一碰鼠标也会吃到动作窗口里的绝对移动。无论放在哪里，只要 core 继续把 `Unavailable` 当成「平台没有前台」，Windows 就必须不再用这个值表示「只是鼠标租约旧了」；否则 trait 方法是死的。默认 `Ok` 也保护不了 fake：保护来自「core 不新增调用」，而不是来自一个到不了的默认实现。

**(c) 给 `ForegroundCapability` 加第三个状态，或让 core 忽略 `Unavailable`。** 不选。前者改协议枚举和 golden；后者会让真正没有前台的平台（诊断构建、未附守卫、macOS 无能力）也进入 `perform`。

**(d) 新的 `DriverError::UserActive`，或 Host 活动 RPC。** 不选。协议码已经存在，并且必须由策略在注入前发出。0004 禁止活动 RPC。驱动在 `perform` 里返回活动错误时，绝对移动已经有机会发出。

不采用：放宽 Stop 的 100ms / 500ms / 5s；取消 fail-closed；让空闲采样、光标复读或过期租约本身续租；加长 100ms；把 `RelativePair` 或 `XButtonPair` 放进证明。

### 测试

Windows 继续用现有 `ManualTime` / `FakeSafetyApi`，不碰 Win32。除上文「真机验证与测试策略」已列、且未被本增补改写的用例外：

- 只有 `confirm`、没有任何 `event`：活动读取为 `None`。这替换 `hotkey/startup.rs` 里紧跟成功 `confirm_hook_proofs` 的 `Some(0)`，也替换 `tests/stop/timing.rs` 里 `fake_listener_ignores_runtime_elapsed_time_during_input_settle` 对 `Some(0)` 的断言。后一个测试里 fake `poll` 仍会 `confirm` 并刷新心跳，传输和注入租约可以仍为真；变的只是活动年龄。定向投递本身的断言不动。
- `tests/stop/timing.rs` 的 `controlled_heartbeat_expiry_latches_stop_at_the_production_boundary` 与 `startup.rs` 末尾那种「把共享钟拨过 100ms 且不再 `heartbeat`」仍然锁存：泵心跳一起过期。不要把它们改写成鼠标租约用例。
- 新用例：拨钟之后重新 `beat` 泵和热键，只让 `confirmed` 超过 100ms。不 `fail`、不锁存、不 `disable`。覆盖已打开且有物理 `event` 时，读取仍是 `Some`，可以大于 100ms。
- `disable` 之后，迟到的 `event` 与 `confirm` 都读成 `None`。第一次 `confirm` 之前的 `event` 也是 `None`。
- 前台 `perform` 在租约过期时先证明，再组包。证明错误：`CapabilityUnavailable`、`maybe_executed == false`、`is_stopped() == false`。同一次 `perform` 内第二处 `check` 不得再发一轮绝对移动。
- 传输不健康的前台请求：不发证明。动作在途时把注入租约拨过 100ms 且没有新的鼠标回调：仍走现有 `fail`，锁存并释放。不得出现新的更长定时器。

core 没有新调用点。不给 fake 加方法，也不新增「fake 统计 `prepare_foreground` 被调用」的路由测试。策略合同保持 `user_activity_blocks_only_the_foreground_path`：前台且 `Some(0)` 为 `user_active`，后台放行，`Some(3000)` 放行，`None` 放行。`gcu-cli` 只测适配器转发 `Some` / `None`，以及未传入钟时引擎仍是 `UnavailableActivity`。不在本卡启用真实 Windows 入口。

真机项不增不减。空闲时移动鼠标后的 `user_active` 仍以 Host 读到的 `Some` 为准，不能用驱动内部的 `physical` 字段代替策略结果。

### 批准

不需要用户再批。没有新的注入行为：绝对移动仍只出现在已经批准的动作窗口，并且排在 `user_active` 拒绝之后；动作窗口外仍不发重复探针。没有新的 trait 方法，因此也不需要按 0002 另批一次接口。本增补是已接受 1a/2a/3a 的接线修正。若要维持「鼠标租约一过期就把 `foreground` 写成 `Unavailable`，并在证明前 `check` 拒绝」，动作前证明和 `user_active` 都不可达，只能回到 D3，而不是改走 A、C 或加长租约。

### 实现时改动的范围

只在实现卡里改这些位置：`gcu-driver-windows` 的 `input/hooks.rs`、`input/hotkey.rs`、`driver.rs`（访问器）、`driver_impl.rs`、`worker/driver.rs`，以及上列测试；`gcu-cli` 的 `engine.rs`、`host/mod.rs` 和一个活动适配器。不改 `gcu-core`、`gcu-protocol`、`gcu-host-protocol`、fake、macOS、Linux，也不改 `STATUS.md`。

### 本增补的退出条件

- 实现发现不修改 `route::plan` 或 `Driver` trait 就无法把前台请求送进 `perform`：停止，不要临时加 trait 方法。
- 实现发现必须新增协议错误、Host 活动消息，或放宽任一期限：停止。本增补不授权。

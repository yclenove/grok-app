# 0013 增补 · 晚到的初始探针与首个前台动作重叠

状态：提议（2026-10-20。安全逻辑，需总控与用户批准后才能合并。不重开 1a/2a/3a，不改 0013 正文的时限与 fail-closed。）
日期：2026-10-20

本增补只解释并收紧方案 B 已接受的「动作期证明」。不改 `Driver` trait、协议、`HEARTBEAT_TIMEOUT`（100ms）、`DRIVER_DEADLINE`、5s 上限、Stop 锁存/释放时限。不让空闲采样、光标复读或过期租约续租。

## 与 0013 原文的关系

0013 方案 B 与文末接线增补仍然有效：空闲时缺少鼠标证明只关闭前台，不锁存；注入期间鼠标证明超过 100ms 必须 `fail` 并锁存；动作前的新证明排在 live target 门之后。本增补不放宽其中任何一条。它只堵住「第 0 代空闲门探针被当成注入期证明来评估」。

## 背景（2026-10-20 win-test）

物理控制台、标准用户，契约表的首个前台动作 C06 使监听退出：`stop_listener failed: low-level hook probe expired (possibly silently removed)`。worker 被杀，动作 `operation cancelled(maybe_executed=true)`，其后 doctor 为 `Stop latched`，其余前台用例失败关闭。

`native_parent_timings` 里的 `hook_SendInput=8041` 等是自监听启动起的毫秒时间戳，不是耗时。每次 `SendInput` 约 20ms。启动时绝对移动被拒绝：`initial input idle unknown/below 2000ms`。鼠标探针到 t=8.000s、键盘到 t=8.021s 才发出，与 `rpc_perform`（约 t=8.041s）重叠，随即出现上述错误。

## 根因

错误字符串只来自 `GuardEvents::confirm_hook_proofs`：键盘与鼠标回调都在时，取较早的那个；距现在超过 100ms **且** `injecting()`（`Phase::Injecting`）则返回该错误。监听把这个 `Err` 交给 `fail()`。启动证明一旦武装，`fail` 就 `request_stop()`。

晚到的初始探针对会走进这次评估，是因为下面三步都没有把它和第 1 代动作证明分开：

1. **第 0 代一直有资格被确认。** `probe.rs` 的 `Sender::poll` 在 `generation == 0` 或动作窗口当前代时，每次泵都调用 `confirm_hook_proofs(keyboard, mouse)`，用的是上下文里存着的回调时刻，不是这次 `SendInput` 的耗时。零位移会留下较早的键盘时刻；空闲门把绝对移动拖到约 8s。`min(早键盘, 晚鼠标)` 超过 100ms。未注入时这条路径故意忽略（启动预算内可以换一对新的）。一旦 `phase == Injecting`，同一对就被当成钩子被静默摘掉。

2. **`begin_foreground` 没有用动作期证明取代它。** 增补写的「租约已经新鲜则跳过」是为了让同一次 `perform` 的外层证明和内层 `check` 不各发一轮。实现是 `if !available() { proof_request }`。第 0 代刚确认成功时 `available()` 已为真，首个前台动作在 live target 门之后直接进入 `Injecting`，不再发起第 1 代证明。`driver_impl` 的顺序（先 live target，再 `begin_foreground`）因此没有发出动作期那一对。

3. **代次切换盖不住重叠。** 窗口代次变化且没有在途发送时，`begin_cycle` 会清掉回调。在途的第 0 代发送让这次清理推迟；泵若在 `begin_foreground` 抬代次之前已经读到 `generation == 0`，随后仍用那一对调用 `confirm_hook_proofs`。此时相位已经是 `Injecting`，第 1 步的错误就发出去了。

所以不是 100ms 租约太短，也不是那次 `SendInput` 花了 8s。是第 0 代的较早回调时刻，在注入期被重新评估。

## 决定（提议）

一旦 `begin_foreground` 开始，第 0 代空闲门周期立即作废，注入期只认这一代动作证明和它的续租。

- 进入动作窗口时把 `proof_floor` 设为新代次，并 `discard_lease()`。第 0 代确认过的钟不能放行这次注入。
- `confirm_hook_proofs(cycle, …)` 在 `cycle < proof_floor` 时直接返回成功：不 `clock.confirm`，也不当摘钩。新鲜的第 0 代回调同样不能把租约续上。
- 在途的第 0 代报告如果落在 floor 之后，不 `complete`、不 `refuse_proof`、不确认。泵改记第 1 代，再按方案 B 发动作期证明（仍受 100ms 与光标复读约束）。
- 当前代次的一对若较早回调超过 100ms 且正在注入，仍然返回原来的错误并锁存。没有续租时，看门狗原有的 `injecting && !clock.available()` 仍然锁存。时限常量不改。

### 安全论证

作废的代次既不能写入注入租约，也不能把监听标成摘钩。放行注入仍要：传输健康，且**当前动作代次**的键盘与鼠标回调较早者距现在 ≤100ms。更早代次的一对，无论新鲜还是陈旧，都不满足这句。因此陈旧证明不能授权注入。真正陈旧的动作期证明、注入期间没有续租、钩子被摘掉，仍然在 100ms 内失败关闭并锁存。

## 备选与为什么不选

**只在 `confirm_hook_proofs` 里忽略一切超过 100ms 的回调，摘钩只留给看门狗。** 不选。注入仍会在 100ms 内因租约缺失而锁存，时限数字不变。但动作期那一对本身已经陈旧时，同步的 `possibly silently removed` 不再发生；排队的旧动作回调和「还没有回调」混在一起。对照里「动作期证明已陈旧必须在确认边界锁存」会变成假。本增补要保留这条边界，只把第 0 代从这条边界里拿出去。

不采用：加长 100ms；让第 0 代的晚到回调在注入期续租；用 `GetLastInputInfo` 或光标复读代替回调。

## 影响

调用方仍是 Windows 监听与现有伪时钟测试。`begin_foreground` 在第 0 代租约仍新鲜时也会再要一轮动作期证明；这一轮就是方案 B 已经允许的动作窗口内绝对移动，不是新的探针种类。内层 `check_injection` 仍不另开一轮。假监听的 `proof_request` 在抬代次之后确认当前代，既有「同一次 perform 只证明一次」不变。

## 退出条件

- 实现必须把 `HEARTBEAT_TIMEOUT`、Stop 锁存/释放或 5s 上限改大：停止，本增补不授权。
- 作废第 0 代之后，陈旧的当前代证明、缺失的注入期续租或摘钩不再锁存：撤回实现，保持 fail-closed。
- 用户拒绝「首个前台动作在第 0 代刚刚证明成功时仍再发一轮动作期探针」：不要改走加长租约；保持第 0 代不能在 `Injecting` 时被评估为摘钩的这一条，并另行决定是否允许第 0 代租约放行第一次注入。

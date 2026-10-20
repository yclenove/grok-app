# 0013 增补 · 真机上证明对与续租的时间预算

状态：提议（2026-10-20。用户已选「不改常量，优化续租节奏」。安全逻辑，需总控与用户批准后才能合并。不重开 1a/2a/3a，不改 `HEARTBEAT_TIMEOUT`、`DRIVER_DEADLINE`、5s 上限、Stop 锁存/释放时限，不放宽 fail-closed。）
日期：2026-10-20

本增补只记录 win-test 标准用户复测的时间线，以及在不改 100ms 语义的前提下让续租赶上注入的实现层手段。不改 `Driver` trait、协议、租约锚点（一对回调的较早者）。不让空闲采样、光标复读或过期租约续租。

## 与 0013 原文、初始探针增补的关系

方案 B 与 [初始探针增补](0013-addendum-initial-probe-overlap.md) 仍然有效：空闲时缺少鼠标证明只关闭前台；注入期间较早回调超过 100ms 必须失败关闭并锁存；第 0 代不能授权注入，也不能在注入期被当成摘钩。

WL1 合入后的复测（下表）说明那条重叠已经不是这次的触发点。第 0 代在 t=21ms 被空闲门拒绝后没有再发；失败发生在动作期这一对已经发出、`begin_foreground` 已经返回之后。

## 证据

机器与账号：win-test，控制台标准用户 `claudetest`，修复后的契约输出 `D:\cu2\hw\contract-out-20261020-172403.txt`（C06 约第 287–309 行）。结果与修复前相同：6 通过 / 7 跳过 / 1 not_run / 16 失败。修复前的重叠见 `contract-out-20261020-135643.txt` 与 `diagnostics.md` 2026-10-20 节，此处不重复。

`native_parent_timings` 与 `NATIVE TIMING` 的 `elapsed_ms` 是自监听启动起的毫秒时间戳，不是耗时。`startup_progress` 在 `armed` 之后直接返回，所以续租发出之后没有时间戳。下表「约」的续租行是按代码排程从已观察到的锚点推出来的，不是日志里的另一行。

对照（同账号，不是这次契约的一行）：无钩子时 `SendInput` 中位数 0ms、最大 2ms；`mouse-probes` 矩阵里钩子安装着且泵不空等时，`SendInput`+回调约 2–5ms。产品探针每次 `elapsed_ms=20`。

## 一次动作期证明的时间线（C06）

| 监听 ms | 谁 | 观察到的事实 |
|---:|---|---|
| 0–1 | 监听 | 线程进入，双钩子安装成功，第一次泵。鼠标未证明。 |
| 21 | 监听 | 第 0 代 `AbsoluteSamePosition` 被拒绝：`initial input idle unknown/below 2000ms`，`dispatched=false`，`pending=false`，无回调。初始周期结束。 |
| 21–7530 |  | 没有新的探针 `SendInput`（诊断串不变）。其间是 C06 之前的用例和 C06 的只读预检。 |
| 7530 | 探针线程 | `hook_mouse_dispatch`：`SendInput pending`。此时日志里的 `Report` 仍是 t=21 的第 0 代拒绝，`pending=true`。在途这一发的代次没有写进那条旧报告。 |
| 7550 | 监听 | 泵已见到鼠标标志 `1`（`LLMHF_INJECTED`）。`keyboard_seen=false`，`mouse_seen=false`（`proven` 要等报告 `complete`）。 |
| 7550 | 探针线程 | 鼠标 `SendInput` 返回：`sent=1/1`，Win32=0，**`call` elapsed_ms=20**。 |
| 7550 | 探针线程 | 键盘 `SendInput pending`（鼠标返回之后才发，串行）。 |
| 7571 | 监听 | `keyboard_seen=true`，标志 `144`（`LLKHF_UP \| LLKHF_INJECTED`）。`mouse_seen` 仍 false，报告仍是第 0 代拒绝。 |
| 7571 | 探针线程 | 键盘 `SendInput` 返回：`sent=1/1`，**elapsed_ms=20**。 |
| 7571 之后、RPC 起点 | 父进程 | `begin_foreground` 返回后才打 `rpc_perform` sequence=93 start。接纳发生在键盘返回之后的下一次泵（约 7591）：报告这时才能 `complete`，鼠标锚点约 7550，年龄约 41ms。这一泵上 `Injecting` 还没被工作线程看见，所以**不发续租**。 |
| 约 7611 | 监听 | 按当前 `preparing_with_mouse`：这时才提交续租。鼠标回调约 7631，键盘约 7651。 |
| 约 7650 | 租约 | 锚点 7550+100ms。续租的键盘落在过期边缘或其后。 |
| RPC 内 | 监听 | `stop_listener failed: low-level hook probe expired (possibly silently removed)`。这是 `confirm_hook_proofs` 在 `injecting` 且较早戳年龄 `>` 100ms 时的原文，不是看门狗的 `during foreground injection`。 |
| +11ms | 父进程 | `worker_kill`（pid 43116，epoch 6）。 |
| +72ms | 父进程 | `rpc_perform` end。动作 `operation cancelled (maybe_executed=true)`。Stop 锁存，其后前台用例失败关闭。 |

72ms 是整段 RPC（含锁存后的取消），不是一次 `SendInput`。它盖住了从接纳到锚点+100ms 的窗口。

## 预算（三个数）

- **一对证明要多久：** 鼠标回调约在 7550（`SendInput` 从 7530 阻塞到泵醒来），键盘回调约在 7571。跨度约 21ms，两次调用各 20ms。再加上报告常常要下一泵才 `complete`（7571 那一泵已经 `keyboard_seen`，但报告仍是旧的），接纳时锚点年龄约 40ms。
- **续租最早何时能到：** 当前实现在「鼠标已证明且尚未 `Injecting`」时不发下一对。接纳那一泵不发，下一泵（约 +20ms）才发。新的一对又是 20+20ms，键盘约在锚点+100ms。报告若再晚一泵，确认落在锚点+120ms。
- **租约何时过期：** 锚点是这一对的较早者（鼠标回调约 7550），不是 `confirm` 的时刻，也不是键盘时刻。`<= 100ms` 仍有效，`> 100ms` 在注入中失败关闭。过期约在 7650。续租确认不早于这个点。

## 为什么产品路径每次约 20ms

钩子回调只拷贝标志、标记和 `Instant`。不调用 `startup_progress`，不睡眠，不打日志。探针在独立线程 `gcu-hook-probes`，`SendInput` 不在钩子线程上（`safety.rs` 已写明，避免自己等自己）。

`SendInput` 要等安装钩子的线程泵消息。监听线程每次 `poll` 之后 `sleep(POLL_INTERVAL)`，`POLL_INTERVAL` 是 20ms，而且睡眠期间有消息也不会醒。产品每次 `elapsed_ms=20` 就是这次盲等，不是回调里的工作。无钩子中位数 0ms、矩阵在紧泵下 2–5ms，和这个解释一致。

鼠标和键盘是两次 `SendInput`。第二次要等第一次返回，而第一次返回时监听已经离开泵、进入下一次睡眠。所以一对是两次 20ms，不是一次。

## 手段

租约年龄仍以同一对的较早者计。下面每一种都不把更早代次的戳写成注入租约，也不改 100ms。

**a. 提前续租。** 确认当前对的那一泵就发出下一对，包括相位还是 `Proving`、`Injecting` 要等工作线程下一拍才看得见的时候。现在的 `preparing_with_mouse` 故意等到那一拍之后。提前发出时年龄约 20–40ms，不超过 100ms 的一半。新对仍须两次带标记回调，且较早者距确认时刻 `<= 100ms` 才写入钟。陈旧对不能借这次提交续上。只做 a 时，确定性排程能撑过 70ms 注入，但刷新点贴着 `>` 100ms 的边界，睡眠抖动仍会失败。

**b. 一对一次 `SendInput`。** 动作期把鼠标探针和 F24 抬起放进同一次调用。两个钩子仍各须收到一次带标记的回调；光标复读仍在发送之前；坐标变了仍作废。泵循环会在这一次调用返回前把两个回调都送完，跨度从两次睡眠变成一次醒来。第 0 代空闲门路径保持两次调用。这不接受旧戳。

**c. 去掉盲等。** 监听线程用 `MsgWaitForMultipleObjects`，超时仍是 `POLL_INTERVAL`（20ms）。队列空时心跳间隔不变，100ms 语义不变。有钩子消息时立即醒，这就是矩阵能到 2–5ms 的原因。回调本身不加锁、不加日志。热键线程的 20ms 睡眠不动（Stop 锁存路径不改）。

**d. 不成对的戳不能拿来确认。** 锚点规则不变：同一尝试的键盘与鼠标，取较早者。续租的键盘戳配上一对的 `proven` 既不续租，也不报「possibly silently removed」。那正是 7571 那种「键盘已看见、报告未 `complete`」再晚一拍就会打出的字符串。真正同一对且较早者 `>` 100ms、注入期间没有续租、钩子被摘掉，仍然锁存。

## 决定（提议）

同时做 a、b、c、d。a 让注入开始前已经有下一对在路上；b 和 c 把这一对从约 40ms 收到一次醒来（预期接近矩阵的 2–5ms，而不是再付一次 20ms）；d 挡住跨尝试的假摘钩。`HEARTBEAT_TIMEOUT` 仍是 100ms。空闲时仍不发探针。

## 备选与为什么不选

**只做 a。** 不选作唯一手段。伪时钟上 70ms 注入可以过，真机睡眠粒度会把刷新推过 100ms。

**只做 b 或只做 c。** 不选作唯一手段。一对变短，但当前代码仍把续租推迟到接纳之后的下一泵。推迟本身就是这次日志里的空窗。

**加长 100ms。** 不选。那是方案 C，本增补和用户选择都拒绝。

**让陈旧的 `proven` 在新键盘到达时续租。** 不选。那会让过期的鼠标戳授权注入。

## 影响

调用方仍是 Windows 监听和现有伪时钟测试。新增的跟踪是 `gcu_windows_timing` 上 `phase=probe` 的元数据（`call_ms`、`lease_age_ms`），以及 doctor 的 `hook_probe_trace:`。不含键码、坐标或文本。`armed` 之后 `startup_progress` 仍不记录，所以复测要看 `phase=probe`，不能只看旧的 startup 行。

复测：在 win-test 控制台以 `claudetest` 重跑契约（不要在无桌面的 ssh 里启动它）。在输出里搜 `phase=probe`。`elapsed_ms` 仍是监听起点以来的时间戳；`reason` 里的 `call_ms` 应不再稳定等于 20。`hook_probe_renew` 的 `lease_age_ms` 应明显小于 100。

## 退出条件

- 实现把 `HEARTBEAT_TIMEOUT`、Stop 锁存/释放或 5s 上限改大：停止，本增补不授权。
- 提前续租、合并 `SendInput` 或更快的泵让陈旧戳、缺失续租或摘钩不再锁存：撤回实现。
- 真机复测里 `call_ms` 仍约为 20，且动作期仍在锚点+100ms 锁存：本决定的 c 没有生效，不要改去加长租约；先看 `phase=probe` 再决定是否重开。

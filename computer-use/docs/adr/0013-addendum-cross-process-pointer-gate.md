# 0013 增补 · 指针包与心跳探针的跨进程互斥闸

状态：已实现（2026-10-21）。属于 Stop 守卫相邻的改动，已向用户汇报；不改 `HEARTBEAT_TIMEOUT` 100ms、`DRIVER_DEADLINE` 4750ms、5s 上限、Stop 锁存/释放时限，不改证明接受逻辑，不放宽 fail-closed。

## 背景（真机事实）

win-test 标准用户 `claudetest`，真实驱动的前台点击，独立 `WH_MOUSE_LL` 钩子日志：驱动自己的 `Move` 确实以 injected 事件送达且坐标正确，但同一毫秒内带探针标记的事件把光标写回旧位置，随后无坐标的 `Button` 在旧位置按下/抬起。点击落在旧位置：桌面、夹具空白处，甚至标题栏关闭按钮（夹具窗口被关闭）。证据文件：`probe-out-20261021-220510.txt`；另见 `diagnostics.md` 同日小节。

原因：心跳探针在 host 进程里先读光标，再发布“绝对同位置移动”；动作的 `SendInput` 在隔离的 worker 进程里。动作的移动落在探针的读与发布之间时，探针用旧位置覆盖它。此前 WL3 的互斥闸是进程内 `static Mutex`，host 与 worker 互相看不见，所以没有起作用。

## 决定

1. 指针闸是命名内核互斥体 `Local\gcu-pointer-gate-<host pid>`。host 用自身 pid，worker 用环境变量 `GCU_HOST_PID`（由 host 在拉起 worker 时设置，不含机密）。
2. 探针的“读光标 → 发布绝对移动 → 复读检查”整体在闸内；worker 的 `SendInput` 仅对含 `INPUT_MOUSE` 的包取闸。键盘/Unicode 包不取闸（长包不得饿死心跳）。Stop 释放路径（`inject_release`）从不等闸。
3. 等待有界：`INJECTION_GATE_WAIT` = 40ms，远小于 100ms 租约。超时则动作被拒绝（`DriverError::Native`，`maybe_executed=false`，未发送任何输入），或该探针周期被拒绝（fail-closed）。持有者崩溃时 `WAIT_ABANDONED` 视为取得。
4. 闸不是租约：发送成功仍不是钩子回调，不续租；钩子回调（监听线程）从不取闸；闸不跨 `await`。

## 控制测试

`injection.rs`：竞态复现（探针采样旧光标后动作移动，闸保证点击落在动作目标）、有界等待、拿不到闸则拒绝探针周期且不发送、闸在区段后释放。`lease_tests.rs`：闸被持有时，心跳超时、陈旧证明、摘钩仍然锁存 Stop。生产路径的命名互斥体只在 Windows 真机上运行（单元测试用进程内互斥体）。

## 真机结果

探针：六个起点光标都到达目标，计数逐次递增（`probe-out-20261021-222920.txt`）。契约表（`contract-out-20261021-223157.txt`）：18 通过 / 8 跳过 / 1 not_run / 3 失败（之前 14/8/1/7）；无 `listener heartbeat expired`、无 gate-busy、Stop 未锁存。仍失败：C08、C22、W-HOTKEY，不在本增补范围。

## 风险

闸的持有者在 `SendInput` 里等钩子回调；监听线程停摆时，等待者在 40ms 后失败关闭而不是无限等待。命名互斥体可被同用户其他进程占住，结果也是动作被拒绝，不是越权输入。

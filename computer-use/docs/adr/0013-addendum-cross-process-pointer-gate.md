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

## 2026-10-22 补充：后续发现与未解决缺陷

- 闸内增加有界（≤15ms）等待光标到位（`native::settle_cursor`）。没有它，探针的“光标未变”检查会把驱动自己的移动当成他人移动，触发 `cursor changed during probe` 的失败关闭锁存；增加后该原因不再出现。
- **未解决：拖拽期间约 2% 的 Stop 锁存**（`live::live_drag_stress`，约 1080 次拖拽 24 次；关闸时 16 次拖拽无锁存但 13 次落空，即原缺陷）。失败时探针状态为 `complete=true, callback=false`：最后一次探针 `SendInput` 返回 2/2，但它带标记的鼠标回调没有到钩子；规则是每个证明周期只试一次、不重试，所以租约约 100ms 后失效。已排除闸等待、监听线程停摆、探针周期过长、作业不返回、爆发式节奏（20ms 最小间隔无效，已移除）。原因（Windows 为何偶尔不上报该事件）未知。
- 候选缓解：在 100ms 租约内对缺失回调的探针做一次有界重试。仍要求两个钩子的真实带标记回调，不伪造证明，但改变“不重试”规则，**需要用户批准，未实现**。

## 2026-10-22 决定：租约内有界重试一次（用户批准）

“每个证明周期只试一次、不重试”这条规则对一种情形放宽：动作窗口内，绝对同位置探针已发送（`complete=true`）但带标记鼠标回调缺失时，`MouseProof::next` 提供**一次**同类型的重试，额度在收到带标记回调后恢复。重试不构成证明：仍需两个钩子的真实带标记注入回调；旧尝试（旧标签）的迟到回调被忽略；初始非动作周期与被拒（失败）周期不重试；100ms 租约、摘钩后的锁存时点、其余常量均不变。

真机（`live_drag_stress`，每次 40 次拖拽，长寿命守卫）：之前约 1080 次拖拽 24 次 Stop 锁存；之后 400 次拖拽 0 次，完整契约表 20/9/1/0。单元测试：缺失回调后恰好一次同类型重试、重试成功恢复额度、旧标签迟到回调不能证明重试、被拒周期不重试；原有“初始周期缺失回调不重试”“moved/cursor-changed 不补偿不重试”的测试保持。

## 独立审查已知局限（2026-10-22）

来源：`docs/plans/computer-use-v2/reviews/2026-10-22-pointer-gate-and-retry-review.md`。本节只记录，不改代码，不改 100ms / 4750ms / 5s。

- **R4 单 host 家族。** 闸名带的是一个 host 的 pid。同一桌面上另一个独立 host 用自己的 pid，两把闸互不排斥，而系统光标是整个桌面共享的。A 的探针仍可能在 B 的动作移动之后把光标写回旧位置。当前产品假设同一桌面只有一个活动 host。这是覆盖范围缺口，不是“同一 host 的闸失效”。不把 pid 换成实例随机数；那仍然是两把闸。
- **R5 锁存上限约一个轮询周期。** `HEARTBEAT_TIMEOUT` 仍是 100ms，看门狗 `POLL_INTERVAL` 仍是 20ms。恰好 100ms 仍算租约内；下一次轮询约在 120ms 才观察到过期，再加上调度抖动。诊断里出现过 101–124ms 的租约年龄。“常量未改”不是“摘钩必在 ≤100ms 锁存”的证明。本次不改这两个常量，也不把 100ms 加大。

## 2026-10-22 修复：准入复查、闸名、到位结果（R1 / R2 / R3）

独立审查的 R1、R2 已按代码确认。修复只收紧，不放宽 fail-closed，不改 100ms / 40ms 等闸 / 15ms 到位 / 4750ms / 5s。

- **R1 取得闸之后复查。** `native::inject` 增加准入闭包。含鼠标的包在取得闸之后、紧邻 `SendInput` 之前执行：`request.check()`（取消与期限）、`GetForegroundWindow()` 仍是目标，以及 `validate`（窗口身份与许可）。复查失败的错误不带 `maybe_executed`，`dispatch_packet` 回滚这一包；更早的包仍由账本保持不确定性。顺序在 `with_admitted_gate`：取闸 → 复查 → 发送 → 释放。复查失败不执行发送，闸会释放。`inject_release`（Stop / 清理）仍然不等闸、不做复查。键盘包不取闸，但前台发送仍在 `SendInput` 前做同一次复查。
- **R2 闸名。** host 忽略 `GCU_HOST_PID`，闸名始终用本进程 pid。worker 在 `serve_with_input` 入口读一次该变量，校验后写入进程内 `OnceLock`；缺失、无法解析或为 0 则拒绝启动，不退回 worker 自己的 pid。闸模块只读这个绑定，不再在取闸时读环境。父进程仍在拉起 worker 时覆盖该变量（决定第 1 条的身份规则由此落地）。
- **R3 到位结果。** `settle_cursor` 返回到位、超时或读不到位置。超时或读失败时 `inject` 返回 `DriverError::Native { maybe_executed: true }`，不把移动说成成功；调用方停止后续依赖到位的按钮和拖拽包。容差仍是 ±2 物理像素：0..65535 归一化实测会偏 1px。`approach_point` 本身只离开目标 1px，所以光标还停在接近点上也会被判为到位。这是为了不拒绝真实的舍入到达而保留的取舍，不把容差改小。超时和读失败仍然是失败。
- **R6 测试。** 单元测试的闸与 Win32 一样同线程可重入，互斥用两个线程共享同一实例证明，测试之间不共用那把实例。这不是内核互斥体。命名互斥体只由 `tests/pointer_gate.rs` 在 Windows 上链接非 `cfg(test)` 的库来调用：两线程互斥，以及 `WAIT_ABANDONED` 算作取得。无桌面的其他目标编过该文件也不构成这项覆盖。

## 2026-10-22 第二轮：会话级闸名、闸外校验、精确到位（N1 / R3 / R4）

独立审查第二轮（`docs/plans/computer-use-v2/reviews/2026-10-22-pointer-gate-fix-review-r2.md`）结论是 MERGE-AFTER-FIXES。本节取代决定第 1 条的 pid 闸名、上文 R2 的 pid 绑定，以及 R3 的 ±2 容差。不改 100ms 心跳、`INJECTION_GATE_WAIT` 40ms、到位等待 15ms、`DRIVER_DEADLINE` 4750ms、5s 上限，不放宽 fail-closed。

- **闸名（R4 / N2）。** 一个会话（桌面）只有一个光标，所以闸是会话级单一名字 `Local\gcu-pointer-gate-v1`，host 与 worker 共用，不再带 pid。`GCU_HOST_PID`、worker 绑定，以及句柄 `OnceLock` 与绑定 `OnceLock` 分离的路径全部删除。N2 的“绑定与句柄不一致”和 R2 的两把闸因此消失。同用户的其他进程占住这个名字时，取闸失败是 `GateBusy`（fail-closed）：拒绝发送，不是授权。这是拒绝服务，不是放行。不再用 pid，是因为 pid 会让同一桌面上的两个 host 各拿一把闸，而光标只有一个。`WAIT_ABANDONED` 仍视为取得所有权，不是新的证明。`CreateMutexW` 失败不再把句柄缓存成 0 后沉默：保留 Win32 错误码，在闸外首次失败时记录一次诊断（`gcu_windows_timing` / `pointer_gate_create`），之后仍然 fail-closed，错误信息带该错误码。不重试创建。
- **闸内工作（N1 / R1）。** 枚举窗口、安全校验和 COM（`State::validate`）在取账本和取闸之前做完。闸内的准入只做有界标量检查：取消与期限、`GetForegroundWindow()` 对比目标顶层窗口，以及 `IsWindow`、根窗口、进程/线程 id、类名。闸内不枚举、不调用 COM、不写日志。计时日志在释放闸之后。键盘包不取闸，但在 `SendInput` 前做同一次有界检查。复查失败不带本包的 `maybe_executed`，账本回滚这一包；更早的按下仍保留。
- **到位（R3）。** 目标像素是 `inputs()` 同一归一化公式的精确反算，容差常量 `CURSOR_ARRIVAL_TOLERANCE_PX` 默认 0。旧的 ±2 会把 1px 的接近点当成已到达并提前开闸。Windows 把归一化坐标舍入到像素的方式尚未用真机数据确认。在 `live::live_move_landing_probe`（忽略，需 `GCU_DESKTOP=1`）打出落点直方图之前，**不得合并**。未确认前不得把容差改成非 0 来掩盖超时。
- **R5 仍未做。** 证明过期相对 100ms 锁存的缺口还在：看门狗轮询约 20ms，恰好 100ms 仍算租约内，观察到过期大约在 120ms，再加上调度抖动。本次不改 100ms，也不把这个窗口说成已经合上。

## 2026-10-22 第三轮：targeted 校验、闸内窗口状态、落点探针、CI（M1 / M2 / M3 / M6）

独立审查第三轮（`docs/plans/computer-use-v2/reviews/2026-10-22-pointer-gate-fix-review-r3.md`）结论是 MERGE-AFTER-FIXES。本节不改 100ms 心跳、`INJECTION_GATE_WAIT` 40ms、到位等待 15ms、`CURSOR_ARRIVAL_TOLERANCE_PX` 0、`DRIVER_DEADLINE` 4750ms、5s 上限，不放宽 fail-closed。支持范围仍是**同一账户、单个 host**。闸不是认证，也不是租约。

- **M1。** 非前台发送（`delivery != Foreground`，含 `TargetedEvent`）在账本和指针闸之外、每次 `PostMessage` 之前做完整 `State::validate`（枚举、戳、安全、接收者）。校验失败不进入账本、不 post。该分支不取指针闸。调用者早先的校验与这次校验之间窗口被替换时，测试拒绝且 `posts == 0`。这比基线「每个 send 校验一次」更严：同一包里的后续事件也会再校验。长的 targeted Unicode 可能因此多花枚举时间；若因此顶到调用方期限，再改成每包一次或包内只做标量复查。第四轮已改成每包一次完整校验，见下文。
- **M2。** `ExpectedIdentity` 保留 validate 当时的根矩形（`GetWindowRect` 的整数边）、`IsIconic`、子/根类名的 UTF-16、子 TID/PID、根 PID、`GetDpiForWindow` 的原始值，以及窗口代标记 `HwndStamp.created`。闸内用同一来源的标量调用复核，精确比较，不相等则不插入。类名比较使用调用前备好的 UTF-16，闸内不再分配 `String`。`GetClassNameW` 写入固定 256 缓冲。
  - **闸内会重读的 `HwndStamp` 字段：** `pid`（根窗口上的 `GetWindowThreadProcessId`）、`created`（`GetPropW`，属性名在 validate 时复制，空名表示该 API 没有标量标记）、`class`（`GetClassNameW`）。另加矩形、最小化、DPI。这些都不需要 `OpenProcess`、枚举或 COM。
  - **不放进闸内的残余（接受）：** `process_created` 要 `OpenProcess` + `GetProcessTimes`；进程令牌、完整性、UIAccess 同样要进程句柄，留在取闸前的 `WinGate`；虚拟桌面成员资格是 `IVirtualDesktopManager` COM；安全桌面 / 输入桌面 / 会话交互性不是标量重读。把它们放进闸会重新引入闸内枚举、COM 或进程句柄。HWND 通常不会在原位提权，UIPI 仍由系统执行。这不是「这些状态不会变」的证明。
- **M3。** `live_move_landing_probe` 仍是 `#[ignore]`，需要 `GCU_DESKTOP=1`，不进 CI 桌面启动器。采样改为：先 `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)` 再 `SetThreadDpiAwarenessContext`（与 worker 的线程上下文相同；进程调用在感知已设置时失败仍继续，线程上下文不是 PMv2 则探针失败），并打印两种结果、虚拟桌面矩形和各显示器拓扑。目标像素覆盖 `denormalize_axis` 与 `floor(norm * extent / 65536)` 不一致的全部像素（1920/2560/3840/7680 的纯计算列表打在输出里，实机 extent 上全部纳入），加相邻像素、桌面/显示器边缘 ±2、固定种子的内部像素，总数至少 400。每次移动前把光标送到切比雪夫距离 >200px 的角落；移动后从发送起每 0.5ms 读物理光标直到 50ms，记录首次离开停靠点并重复的稳定落点与耗时。两种误差直方图都打印。成功发送数和有效读取数必须等于计划数，否则测试失败。容差常量仍是 0。本次没有采到真机直方图。第四轮取代目标选择、稳定落点和进程级 DPI，见下文。
- **M6。** Windows CI 增加 `cargo clippy --locked -p gcu-driver-windows --all-targets --features test-support` 和 `cargo test --locked -p gcu-driver-windows --features test-support --test pointer_gate`。生产名字常量仍是 `Local\gcu-pointer-gate-v1`。名字冲突用例只在 `test-support` 且非 `cfg(test)` 时注入唯一测试名，不占用生产名字；另有一个只读断言核对生产名字且不创建对象。
- **诊断。** `wait_ms` 只记录 `WaitForSingleObject`（测试替身则只记录获取）的耗时，在释放互斥体并且账本锁也释放之后才写日志。创建失败、`WAIT_FAILED` 和 `ReleaseMutex` 失败同样先暂存、再在临界区外记录。创建失败的 HRESULT 若属于 Win32 facility（`0x8007xxxx`），拒绝文案同时写出 HRESULT 和 Win32 码；争用仍是不带错误码的 busy。这些失败不变成许可或证明。
- **M4 未修，已知缺口。** 会话级互斥体只覆盖单次指针包和探针临界区。一次点击的移动和随后的无坐标按钮是分开的包，每包释放闸。两个 host 可以交错：A 移到 P 后释放，B 移到 Q 后释放，A 的按钮落在 Q。每个 `InputBackend` 的动作串行锁不跨 host。本次不把 40ms 闸改成跨整个动作或跨 `await` 持有，否则会饿死探针续约。产品假设同一账户只有一个活动 host。`Local\` 是 Windows 会话而不是桌面：同一会话里的不同桌面会被这一个名字串行化；默认 DACL 是创建者令牌，不保证同一会话里的另一个用户令牌能打开它。创建失败会永久缓存并 fail-closed，不重试。更老的带 pid 的闸名不在这个名字里，与本闸互不排斥。
- **M5 未修，已知缺口。** 证明过期仍按「年龄 ≤100ms 算有效」，看门狗轮询约 20ms，所以观察到过期大约在 100–120ms，再加上调度抖动。`bounded_admit` 不读租约。本次不改 100ms、20ms 轮询、4750ms 或 5s 上限，也不把这个窗口说成已经合上。

## 2026-10-22 第四轮：每包一次全量校验、落点尾部、显示器并集（N1 / N2 / N3）

独立审查第四轮（`docs/plans/computer-use-v2/reviews/2026-10-22-pointer-gate-fix-review-r4.md`）结论是 MERGE-AFTER-FIXES。本节取代第三轮 M1 的「每个事件完整 `State::validate`」和 M3 的外接矩形目标、第一对相等采样、以及进程级 DPI。不改 100ms 心跳、`INJECTION_GATE_WAIT` 40ms、到位等待 15ms、`CURSOR_ARRIVAL_TOLERANCE_PX` 0、`DRIVER_DEADLINE` 4750ms、5s 上限，不放宽 fail-closed。M4 / M5 仍是上面的未修缺口。

- **N1。** 非前台包在账本和闸之外做**一次**完整 `State::validate`（`foreground = false`），把标量快照记入 `ExpectedIdentity`。每次 `PostMessage` 之前只做请求检查和与闸内相同的标量复核（子/根 TID、PID、类名、根矩形、DPI、最小化、代标记），不要求前台。复核在 `dispatch` 之外，失败不占账本、不 post。调用者校验之后、这次包校验之前窗口被替换，则整包不 post。同包内第 k 个事件之前被替换，则第 k 个及之后不 post。256 个 UTF-16 单位（512 个事件）只计一次完整校验。
- **N2。** 每次移动保留请求像素、归一化坐标、park 位置、时间原点、全部采样和终点。生产 15ms 预算内第一次离开 park 单独记录，不当作稳定落点。稳定落点是观察窗口尾部连续 2 个采样，且与最后一个采样相等。停在 park、尾部不一致、或尾部读不完整的轨迹是 inconclusive：单独计数，不进成功落点，也不进误差直方图。非零误差留在直方图里。打印逆映射误差最大的 20 条完整轨迹。结论性落点必须不少于计划数的 95%，并且不少于 400；显示器并集不足 400 像素时，下限改为实际计划数。
- **N3。** 目标和 park 只取活动显示器矩形的并集，不取虚拟桌面外接矩形。空隙里的点不进入成功或误差统计。量化分歧像素若落在空隙中线上，改用包含该轴坐标的显示器里的另一轴坐标；该轴上没有任何显示器时，不把它算作必须覆盖的落点。采集期间显示器矩形或 DPI 变化则丢弃这一轮并重采一次，再次变化则失败。
- **DPI。** 探针只调用 `SetThreadDpiAwarenessContext(PER_MONITOR_AWARE_V2)`，与 worker 的 `DpiContext` 一样。不调用 `SetProcessDpiAwarenessContext`。

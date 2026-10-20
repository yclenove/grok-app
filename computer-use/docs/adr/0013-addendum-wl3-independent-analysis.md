# 0013 增补 · WL3R Windows 前台契约剩余八项失败的独立分析

日期：2026-10-20。状态：独立只读分析；修复方向待总控验证，不代表已完成真机验收。

基线为当前工作区 `55d0528c`（含 WL1、WL2）。本次仅新增本文，不修改源码、常量、协议或 STATUS；不使用 SSH、rcargo、git commit，不读取并行工程师的分支或调查。安全边界沿用 [ADR 0013](0013-windows-stop-guard-liveness-lease.md)、[初始探针增补](0013-addendum-initial-probe-overlap.md)和[真机预算增补](0013-addendum-real-hardware-budget.md)；[WL2R 分析](0013-addendum-independent-analysis.md)用于理解旧故障，不把旧时间线套给本次。

## 结论先行

这八项不能归为同一个已证实根因。**C04 是读取截图的用例，没有动作输入；C08 已获得控件焦点并收到文本，只丢失 emoji 和换行。** 其余六项卡在点击或拖拽的效果上，其中 C09、C22、W-HOTKEY 尚未进入各自命名的核心测试。

保留三条根因判断，按证据强弱分别标注：

1. **C04：夹具绘制网格与暴露的布局边界不一致。高置信度。** 402px 的可访问边界被当作 320px 绘制网格来分八列，精确重现失败横坐标。属于 H3 的布局分支，尚不需要 DPI 或副屏解释。
2. **C08：Unicode 注入与 Windows/winit/egui 接收语义不匹配。高置信度（换行）；中高（代理对）。** 日志显示实际文本为 `中文 \u{200d}second line`。代码逐 UTF-16 单元 down/up；接收层可分别丢弃半个代理对，且明确过滤文本形式的换行。F24 挤占历史不是该断言失败的原因。
3. **六项指针效果失败：探针与动作缺少共同输入事务，H1 为优先验证方向。中置信度（本次归因），高置信度（竞态存在）。** 旧光标采样可在动作移动后写回，接着按钮包按被写回的位置点击，光标复读还可能通过。没有实际落点/按钮消息日志，不能声称本次已经测到这一顺序；同窗口内坐标映射或消息消费问题仍须实验区分。

## 证据范围与时间精度

标记约定：**[日志]** 为本地输入直接记录；**[代码]** 为当前源码或锁定依赖可推出的机制；**[重建]** 为有条件的时序/跨用例布局推导；**[未证]** 为现有材料无法确认的真机行为。

原始文件仅存于根目录临时 `.scratch-hw/`，不得提交。两份均按 UTF-16LE BOM 解码，本文 `A:L…` / `B:L…` 为解码后原文行号，不是终端折行。

| 代号 | 文件 | 行数 | 原始字节 SHA-256 |
| --- | --- | ---: | --- |
| A，本次 WL2 后 | `contract-out-20261020-204834.txt` | 2217 | `bd18c5dece2a0973c5c757e90bf08fd8ce385b11bfbe23028081a349c39cba1c` |
| B，WL1 后 | `contract-out-20261020-172403.txt` | 1681 | `13a74b4d8cfc8e27cda01cfabfc4dd95e5c58e78b4361735b3592676a9dafc32` |

环境“win-test、标准用户 claudetest、物理控制台”来自任务交接。远端 `trace-20261020-204834` Worker 日志**未复制到本工作区，本次没有读取**；`135643` 文件也不在本次输入内。“另一会话光标 x=-40”和 ChsIME 的相关性仅为交接线索，两份本地文本都没有这两个记录，不能充作本次测量。

`t` 指监听启动后的单调毫秒，来自 `phase=startup/probe` 外层 `elapsed_ms`；`rpc_* phase=end` 则是调用**时长**，`phase=start` 没有绝对时间。`lease_age_ms` 在[续租提交处](../../crates/gcu-driver-windows/src/input/ffi/safety/probe.rs#L190)读取旧锚点年龄，不能当作新证明完成时间。夹具 `timestamp_ms` 是记录 egui 事件/状态时的墙钟；不与 `t` 无校准相减。[Recorder](../../crates/gcu-driver-windows/tests/contract/timings.rs#L76)没有动作各包的发送时刻、屏幕落点、Win32 消息或共同线程时间轴。

**[日志]** A:L2179 为 13 passed / 8 skipped / 1 not_run / 8 failed；无 `stop_listener failed`。下表七项前台动作的探针包全部记 `call_ms=0; sent=2/2; packet=mouse+keyup`，典型约 30ms 一轮，局部更快，续租提交年龄总范围为 0–34ms。`0ms` 是整数截断，不代表无竞态。B 的 C06 在 mutation RPC 内取消（L309），后续多项因守卫不可用拒绝；A 已走到各自 oracle，不能继续以旧锁存解释。

每行前都重启夹具、清空 oracle，驱动监听跨行保留，见 [reset_fixture / 主循环](../../crates/gcu-driver-windows/tests/contract/live.rs#L240)。行间 `worker_kill` 是 `release_all_input` 的正常清理路径（[Supervisor](../../crates/gcu-driver-windows/src/worker/supervisor.rs#L183)），不能仅凭这些行认定 Stop 再次锁存。

## 一、逐项时间线：发了什么、发往哪里、在哪个阶段失败

以下顺序遵照任务要求。所有失败等待均为 **5s 效果/观察预算**，外层每行另有 45s 上限，无动作重试，见 [Fixture::wait](../../crates/gcu-driver-windows/tests/contract/live.rs#L78)、[poll/poll_async](../../crates/gcu-driver-windows/tests/contract/harness.rs#L256)。并非 SendInput 卡住 45s。

| 用例 / A 原文 | 准备、动作与探针时间线 | 夹具结果与实际停止位置 |
| --- | --- | --- |
| **C06** L287–337；夹具 PID 31080 | snapshot 116ms → capture 352ms → geometry/list_windows 各 38ms → 首证明 `t=7716` → RPC #93，477ms。共 18 个探针包，`t=7716…8209`，续租提交年龄 11–31ms | `grid_hit=null`，`event_log=[]`，18 个 F24 up。**[代码]** 像素前置检查已通过，随后网格左键点击返回 Ok，才进入 `grid pixel coordinate` 等待；失败是网格未命中，不是动作被拒绝 |
| **C08** L406–577；PID 45744 | snapshot 117ms → 首证明 `t=15959` → RPC #125 点击 466ms → #128 Ctrl+A 398ms → #131 BackSpace 407ms → #134 TypeText 396ms；每次动作有独立 geometry/list_windows 预检。74 个探针包，`t=15959…17997`，提交年龄 0–34ms | `focused_widget=multiline_field`；clear 已成功；最终 `multiline="中文 \u{200d}second line"`（21 个 UTF-8 字节），预期 `中文 👩‍💻\nsecond line`（30 字节）。失败是内容确实少了 U+1F469、U+1F4BB、U+000A，**不是没有收到输入** |
| **C09** L609–651；PID 54900 | snapshot 117ms → geometry/list_windows 各 37ms → 首证明 `t=24412` → RPC #151 点击，447ms；17 包，`t=24412…24878`，年龄 14–32ms | `focused_widget` 字段缺省、`text=""`、`event_log=[]`。只走到 name_field 的点击和 `external focused_widget` 等待；`clear-me`、Ctrl+A、BackSpace **均未执行** |
| **C11** L720–838；PID 47396 | snapshot 118ms → geometry 36ms/list_windows 37ms → `t=32733` → RPC #183 点击，475ms；19 包，`t=32733…33226`，年龄 1–30ms。之后 RPC #184–219 共 36 次只读树观察 | 最后完整树里找不到 `Item 3 / 第三项`，`combo_selected=0`、`event_log=[]`。只发了下拉入口点击；第三项点击和选择断言**尚未执行**。不能区分“菜单未打开”“打开后关闭”“打开但 provider 未暴露”，末次树不能恢复瞬态 |
| **C15** L985–1055；PID 59956 | source/target 两次 snapshot 122/118ms → geometry/list_windows 37/39ms → `t=44294` → RPC #284 拖拽，515ms；30 包，`t=44294…44831`，年龄 4–31ms，动作密集段有 4–6ms 续租 | `drop_ok=false`、`event_log=[]`。**[代码]** 拖拽请求完整返回后等 drop；夹具只有在 `drag_stopped()` 且停止点位于 target 时写 true，无法由 false 判定“没按下”“中途跑偏”或“释放点不在目标” |
| **C22** L1435–1479；PID 45072 | snapshot 119ms → geometry/list_windows 37/39ms → `t=62869` → RPC #444 点击，473ms；18 包，`t=62869…63360`，年龄 9–31ms | `counter=0`、`event_log=[]`。卡在 `counter focus click` 准备步骤；对抗 Worker 尚未启动，Return 未发送，**未执行 unknown/超时/不重放验证** |
| **W-HOTKEY** L2091–2135；PID 57296 | snapshot 115ms → geometry/list_windows 各 36ms → `t=90773` → RPC #729 点击，469ms；18 包，`t=90773…91264`，年龄 4–34ms | `focused_widget` 缺省、`text=""`、`event_log=[]`。仍卡在 name_field 聚焦；第二个 SafetyGuard、冲突注册及 `MUST-NOT-ARRIVE` 拒绝测试**尚未执行**。A:L34 的独立注册探测确实得到 1409，但不是本行完整通过 |
| **C04** L207–216；PID 52512 | snapshot 117ms → 测试 cover 激活成功（L210–213）→ capture 374ms → 像素检查；本行**无** `rpc_perform` 和动作期探针包 | 前两个网格点通过，第三点 `(7,5)`、窗口点 `(1235.3125,325)` 期望 `[132,140,123]` 得 `[248,248,248]`；B:L216 完全相同。没有点击/拖拽/键盘操作可归因于续租 |

### 输入是否真的发出，以及“发到哪里”的证明限度

**[代码＋日志]** 上述六个指针请求都越过 `click/perform(...).await?`，C08 四次请求也均越过成功屏障。`phase=end` 单独不证明成功（计时器可在错误退出时 Drop）；这里的依据是**后面的 oracle 超时报错位置**。[cases.rs](../../crates/gcu-driver-windows/tests/contract/cases.rs#L89)、[C22 准备步骤](../../crates/gcu-driver-windows/tests/contract/safety.rs#L188)、[W-HOTKEY 准备步骤](../../crates/gcu-driver-windows/tests/contract/safety.rs#L331)均有 `?`，错误不会越过去。

在这个基线中，成功的前台发送要求 [RealInput::send](../../crates/gcu-driver-windows/src/input/ffi/mod.rs#L325)验证目标、核对前台 HWND，并且 [native::inject](../../crates/gcu-driver-windows/src/input/ffi/native.rs#L296)的返回计数等于包长度。因此可以推断动作输入已被 OS 接纳；**没有独立的动作 SendInput 计数/位置日志，不能把探针的 `2/2` 当成动作包的 `2/2`，更不能把接纳当作控件消费或命中。**

**[代码]** [InputBackend::settle](../../crates/gcu-driver-windows/src/input/mod.rs#L299)按 150ms 间隔采样，连续稳定可以返回 `NoVisibleChange`，外层仍返回成功 Delivery。这解释了“RPC 完成而效果 oracle 超时”为何可以同时发生；约 0.4–0.5s RPC 也不能当作原生点击持续时间。租约覆盖这段 settle，随后动作结束停止续租，五秒 oracle 等待不是新的持续探针窗口。C08 的 clear 等待虽然通过，但新夹具初始多行字段本来为空，因此不能单凭它验收 Ctrl+A/BackSpace 的实际编辑效果。

逻辑目标是该行 PID 的戳记 fixture 根窗口，窗口点经 [plan::prepare](../../crates/gcu-driver-windows/src/input/plan.rs#L25)加 `window.frame.x/y` 转屏幕点，再按虚拟桌面归一化。前台 SendInput 本身**不携带 HWND**；按钮记录也不携带目标点，使用当时光标。键盘则交给当时的前台/焦点队列。解析得到的 `Target.hwnd` 用于核验和账本，不是强制定向投递。

作为可复核的**[重建]**，A:L838 的同一初始布局树给出下列参考窗口点。它不是各行自己的实测坐标；每行的 HWND 原点、实际鼠标消息坐标、命中 HWND 没有记录，所以无法补出真实屏幕数值：

| 动作 | 参考窗口点 / 路径 | 依据与限制 |
| --- | --- | --- |
| C06 网格 `(2,3)` | `(984.0625,245)` | 树中网格 `(871,115,402,240)`，按 `(col+.25,row+.25)` 比例计算；本行 capture 前置颜色检查已通过 |
| C08 multiline_field | `(262,135.5)` | 参考 frame `(122,119,280,33)`；本行焦点 oracle 实际通过 |
| C09、W-HOTKEY name_field | `(262,106.5)` | frame `(122,97,280,19)`；本行未见该控件焦点 |
| C11 下拉入口 | `(213.5,228)` | frame `(149,219,129,18)`；没有第三项坐标，因为没找到它 |
| C15 拖拽 | `(487,345) → (600,350)` | source `(447,325,80,40)`，target `(535,325,130,50)`；插值步长最多 10px，不是直接跳到终点 |
| C22 counter_button | `(92.5,270)` | frame `(23,261,139,18)`；counter 未增 |

夹具 `event_log` 是控件状态变化日志，**不是原始鼠标消息环形缓冲**；[ui_frame.rs](../../apps/fixture/src/ui_frame.rs#L13)只把 Key/Text 写入 `key_events`。因此 `event_log=[]` 只能证明没有记录到预期状态变更，不能证明没有 WM_MOUSEMOVE/按钮消息。`focused_widget` 缺省按[序列化定义](../../apps/fixture/src/state.rs#L155)表示 None，且该字段只覆盖三种文本框，不代表整个窗口未激活。

## 二、H1：同位置探针与动作的竞态

### 当前代码的采样、排队和互斥边界

**[代码]** [Sender](../../crates/gcu-driver-windows/src/input/ffi/safety/probe.rs#L29)在父进程 `gcu-hook-probes` 线程接收容量为 1 的 Job 通道；`pending` 避免重复提交。Job 只有 mode/tag/generation/action，**没有预采样的光标坐标**。光标是在发送线程取到 Job 后，由 [dispatch_inner](../../crates/gcu-driver-windows/src/input/hook_probe.rs#L153)依次读取：

1. `GetPhysicalCursorPos` 得 `before`，线程进入 PMv2 DPI 上下文。
2. 读取虚拟桌面 metrics，用 `normalize` 得绝对坐标。
3. `authorized()` 重验动作代次/窗口仍有效，再构造包；动作期合并一次 `absolute move + VK_F24 keyup`。
4. 调用 SendInput，之后再次 `GetPhysicalCursorPos`；不同则作废，不补偿、不重试。
5. 发送报告被监听线程接纳，且同 attempt/tag 的双侧注入回调匹配、新鲜，才确认租约。

动作则在**子进程** InputBackend/RealInput MTA 发送，存在多次 await/目标校验/父账本 InputPrepare、InputComplete 握手。[InputBackend 的 serial](../../crates/gcu-driver-windows/src/input/mod.rs#L110)和父 `begin_foreground` 的 serial 都只串行化动作，**不互斥探针线程**；账本锁也不包含探针。探针不排入 Worker 的动作包队列，而是直接向同一 OS 全局输入流发送。

[plan.rs](../../crates/gcu-driver-windows/src/input/plan.rs#L54)把一次点击拆为 `Move` 包和 `Down+Up` 包，两包之间重新校验并进行 IPC。拖拽的 move/down/各插值 move/up 各自成包。[native.rs](../../crates/gcu-driver-windows/src/input/ffi/native.rs#L217)匹配 `Event::Button(button,down,_,_)`，**明确丢弃事件里的位置**，发送只有 DOWN/UP 标志。

单次 SendInput 插入的数组按序处理，不能把“两个调用可交错”夸大为“探针必然能插进同一次 Down+Up、快捷键或 Unicode 数组”。当前危险边界主要是**两次调用之间，以及采样至 SendInput 之间**。同样，夹具 Text/F24 记录交错不是探针插进同一原生数组的直接证明；消息翻译、队列消费和 egui 帧记录发生在后面。

### 可通过光标复读的最小竞态时间线

下表是**[重建]**，没有在 A 中直接观察到：

| 顺序 | 探针线程 | 动作 Worker | 结果 |
| --- | --- | --- | --- |
| 1 | 读 `before=A`，计算绝对 A | 尚未移动 | 光标 A |
| 2 | 尚未 SendInput | SendInput：Move 到目标 B | 光标 B |
| 3 | SendInput：绝对 A + F24 up | 两包间校验/账本握手 | 光标被写回 A |
| 4 | 复读得到 A，与 before 相等；报告成功 | 等待下一包 | **A→B→A 没被两次采样识别**，租约可正常续上 |
| 5 | 下一轮未开始 | SendInput：Down+Up，无定位标志 | 实际点击 A；B 的控件没有效果 |

拖拽时同一机制能把按下点、插值路径或抬起前的位置改掉。反过来，若动作移动后**没有**被探针写回，导致复读与 before 不同，则应 `refuse_proof`，在 Injecting 阶段锁存；本次没有此类失败。因此“没有 Stop”只排除**被复读检测到的变化**，不能排除上述回到原点的路径。

**判定：H1 机制成立，实际发生未证。** A 确认动作期间持续发送探针，C15 的密集动作还伴随更密集续租；但没有探针 before/send/after 与动作 move/button 的共同顺序/坐标。六项普遍失败且 C08 聚焦成功，也要求检验固定坐标偏移或接收事件顺序，不能只凭并发存在就把全部点击判给 H1。

**证伪实验 E1（保持全部守卫）：** 在一次性标准用户物理控制台夹具上，记录同一单调时间基准下的动作 ID、attempt/tag、探针采样/派发/复读点、动作各 SendInput 起止/计数，以及夹具收到的 pointer move/down/up 的屏幕/客户坐标与目标 HWND。用受控屏障令探针采样 A 后，动作移动 B，再允许探针发送，最后发按钮包；全过程必须留在现有 100ms 内，做不到则拒绝，不暂停看门狗。观察到 A→B→A、复读成功、按钮在 A 即确认机制的影响；若本次自然复现的所有 miss 都无旧点写回且 down/up 在 B，则否定 H1 对这些 miss 的归因。跨进程共同互斥后若仍同样 miss，也否定它是充分解释。

## 三、H2 与 F24：缓冲、键盘状态、焦点、IME 分开检验

### 容量和实际污染程度

**[代码]** [state.rs](../../apps/fixture/src/state.rs#L20)中 `MAX_KEY_LOG_ENTRIES=50`、`MAX_EVENT_LOG_ENTRIES=50`，是两个独立 Vec。`record_key` 超出 50 时从头 drain；不删除状态字段或 `event_log`。探针 F24 up 没有 repeat，经过 egui Key 事件被记录并触发持久化；因此本次确实存在输入噪声和写盘负担，不能假定探针对应用透明。

下表按每个失败字符串中的首份完整快照统计，去掉同一失败末尾 `last fixture` 的重复；C11 只有末尾一份 oracle。墙钟为记录时刻，不是原生注入时刻。

| 用例 | key_events 数量 / F24 数量 | 最早…最晚 timestamp_ms | 覆盖跨度 / 容量判断 |
| --- | --- | --- | --- |
| C06 | 18 / 18 | `1792500530890…1792500531392` | 502ms，未满 |
| C08 | 50 / 35，另 15 个 Text | `1792500540592…1792500541676` | 1084ms，已达上限；真实 Text 仍在 |
| C09 | 17 / 17 | `1792500547586…1792500548058` | 472ms，未满 |
| C11 | 19 / 19 | `1792500555908…1792500556408` | 500ms，未满 |
| C15 | 30 / 30 | `1792500567468…1792500568008` | 540ms，未满 |
| C22 | 18 / 18 | `1792500586044…1792500586541` | 497ms，未满 |
| W-HOTKEY | 18 / 18 | `1792500613947…1792500614441` | 494ms，未满 |

按约 30 个 F24/s，空缓冲仅靠 F24 约 1.67s 达 50 条，第 51 条开始逐出；这只是速率算例，动作间隙不发探针，C15 密集期又远快于此。不能说 5s oracle 等待期间一直有探针。

**C08 的精确首次满/溢出时刻无法由最终快照恢复**：它执行四次动作、有 74 个已接纳探针包及文本/快捷键事件，最终只保留上表时间窗。可直接确认到失败快照时为 50 条、早期历史存在被逐出的可能；没有各帧原始序号/逐出计数，不能把最早保留时间 `1792500540592` 写成“首次溢出时间”。其余六项本行新夹具仅有 17–30 条，未满；不存在被 F24 逐出大量真实按键来解释失败的依据。

**判定：H2 的诊断污染成立，作为本次失败原因不成立。** C08 断言直接比较 `multiline`，C09/W-HOTKEY 直接比较 `focused_widget`，都不搜索 `key_events`。即使早期 Ctrl+A/BackSpace 已从 C08 历史消失，也不能把已存的真实文本恢复为正确内容。C08 的 15 个 Text 恰好是两汉字、两空格、ZWJ 与 `second line`，两个非 BMP 字符和换行并未出现在这一末段 Text 记录中。

**证伪实验 E2：** 保留容量常量 50，额外将一次性夹具的每条原始事件以递增序号追加到独立诊断文件，记录首个 drain 的序号/时刻，oracle 继续比较原字段。若 `multiline` 原字段已经正确而只有历史查询错误，才支持“缓冲造成误判”；本次快照已与此前提相反。不要仅扩容或过滤 F24 后宣告产品输入修复。

### F24 up 会不会改修饰键、焦点或 ChsIME

**[代码]** [keyboard_up](../../crates/gcu-driver-windows/src/input/ffi/safety/probe.rs#L237)只构造 `wVk=VK_F24, KEYEVENTF_KEYUP, wScan=0`，没有 Ctrl/Shift/Alt/Win 的转换、没有 SetFocus/Activate/IME API，也没有字符或换行载荷。[低级钩子](../../crates/gcu-driver-windows/src/input/ffi/safety.rs#L130)最后始终 CallNextHookEx，不吞掉探针，所以它仍是应用可见的真实合成按键。标签只保护本驱动的证明/物理活动分类，不使事件对前台应用隐形。

由此只能推出：探针**释放 F24 的状态**，不会直接发出“释放所有修饰键”或切换焦点的指令；它不能被当作完全无状态的空操作。若已有 F24 down、第三方快捷键处理、应用菜单逻辑或 IME 钩子，该 up 仍可能有副作用，现有日志未验证这些路径。C08 最终所有保留事件的 ctrl/shift/alt 均 false，且目标焦点仍在 multiline_field；这不证明先前修饰键全程正确。

**[未证]** ChsIME 是否参与、是否因 F24 改变组合状态、是否改变焦点，都没有 HKL、IME composition 或 WM_SETFOCUS/WM_KILLFOCUS 记录。`AttachThreadInput` 位于动作激活路径，属于另一个键盘状态影响点，也没有本次状态快照；不能把其影响算成 F24 的直接效果。C09/W-HOTKEY 尚未执行键盘输入，尤其不能以“F24 把其 Ctrl/热键状态搞坏”解释它们的失败。

**证伪实验 E3：** 一次性夹具记录注入前后 GetKeyboardState/GetAsyncKeyState 的修饰键和 F24 位、当前 HKL、IME 组合开始/提交/结束、前台/焦点 HWND；用同一输入分别测试 BMP、单个非 BMP、ZWJ、LF、Return，并对照已授权的独立实验中不同 IME 状态。运行双钩子证明和既有看门狗，禁止用“关探针后继续长时间注入”作对照。若没有修饰键/IME/焦点变化仍稳定丢代理对和 LF，则否定 F24/ChsIME 作为该文字损坏的必要条件。

## 四、C08 的独立文字路径证据

**[日志]** A:L577 不只是 F24 列表：`event_log` 有 `set_multiline len=0`（墙钟 `1792500540464`），随后长度 `3,7,10,12,14,15,17,19,21`（首个非空 `1792500540964`，最终 `1792500541281`）。这些长度与收到的汉字/空格/ZWJ/ASCII 相符。原字段和 Text 事件相互支持，故不是字体缺字、截图渲染或历史被逐出造成“看起来没输入”。

**[代码]** [messages::text_chunks](../../crates/gcu-driver-windows/src/input/messages.rs#L28)将非 BMP 标量展开为 UTF-16，然后依次形成 `high-down, high-up, low-down, low-up`；[原生转换](../../crates/gcu-driver-windows/src/input/ffi/native.rs#L252)用 `VK=0 + KEYEVENTF_UNICODE`，LF 也只是 Unicode 0x000A，未变成 Return。这个短文本共 18 个标量、20 个 UTF-16 单元，进入一个 40-record 包；没有跨 TEXT_CHUNK_UNITS 边界，也不满足长文本剪贴板回退条件。

锁定依赖见 [Cargo.lock](../../Cargo.lock)：winit 0.30.13、egui/egui-winit 0.36.2。以下依赖实现已从本机对应版本 Cargo 缓存只读核对；链接为版本源码定位，不是额外真机观测：

- [winit Windows keyboard.rs](https://docs.rs/crate/winit/0.30.13/source/src/platform_impl/windows/keyboard.rs) 的 WM_CHAR/WM_SYSCHAR 分支（约 L168–245）只在下一条键盘消息还是 CHAR 时继续收集 UTF-16；遇到 KEYUP/下一 KEYDOWN 可结束当前事件。`PartialKeyEventInfo::finalize`（约 L590–607）把 UTF-16 通过 `OsString::into_string()` 转为 Rust 字符串，失败则不产生 text。**[代码推断]** high/low 各自 down/up 可使它们分别 finalize，两个孤立代理单元均转换失败，BMP 和 ZWJ 则保留，吻合本次结果。实际 WM_* 顺序未采集，故代理对归因保留中高置信度。
- [egui-winit lib.rs](https://docs.rs/crate/egui-winit/0.36.2/source/src/lib.rs) 的 `on_keyboard_input`（约 L1046–1067）只接纳 printable text，`is_printable_char`（约 L1406–1416）明确排除 `\r/\n/\t`，并注明 newline 由 Key::Enter 处理。**[代码推断]** 本驱动发 Unicode LF 而非 Enter，能够解释换行缺失，与 F24 缓冲无关。

**证伪实验 E4：** 抓取 fixture 的原始 WM_KEY*/WM_CHAR 序列、winit KeyEvent.text 和 egui Text/Key::Enter/IME，再只输入单个 `👩`、单个 `💻`、LF、Return。若 high/low 已在同一 winit 事件中成为合法标量，仍在后续丢失，则代理对 finalize 解释被否定；若 LF 已变成 Enter 或进入合法多行文本而随后被覆写，则换行归因也需调整。可在保持相同守卫的专用重跑中比较正确聚合代理对/明确换行语义的输入变体，但不自动重放 unknown 动作。

## 五、H3：C04 首先是布局边界，而非负坐标证据

**[代码]** [夹具网格](../../apps/fixture/src/ui.rs#L446)固定 `cell_w=cell_h=40`，绘制/命中计算使用 `allocate_exact_size` 返回的 `canvas_rect`，画 320×240。它却对另一个返回值 `canvas_res` 注册 `grid_canvas` 的可访问信息。三列布局允许分配的 response 变宽。

对应版本 [egui Ui::allocate_exact_size](https://docs.rs/crate/egui/0.36.2/source/src/ui.rs)（约 L1143–1157）明确说明：返回的 Rect 是请求大小，**Response.rect 可因 justified layout 更大**；[Response::fill_accesskit_node_common](https://docs.rs/crate/egui/0.36.2/source/src/response.rs)（约 L908–918）将 `self.rect` 写作 AccessKit bounds。A:L838 的网格 UIA frame 恰为 `(871,115,402,240)`。

**[重建]** 把该初始布局代入 [C04 算式](../../crates/gcu-driver-windows/tests/contract/cases.rs#L240)：

| 点 | 按 UIA 宽 402 得到的相对横偏移 | 按实际 40px 单元解释 | 与日志的关系 |
| --- | ---: | --- | --- |
| `(0,0)` | `0.25×402/8 = 12.5625` | 落第 0 列 | C04 已越过检查 |
| `(2,3)` | `2.25×402/8 = 113.0625` | 落第 2 列，仍在 320px 内 | C04 已越过；C06 也只预检此点，不能证明整格几何正确 |
| `(7,5)` | `7.25×402/8 = 364.3125` | 超过绘制宽 320px，落布局余白 | `871+364.3125=1235.3125`，**精确匹配两次失败**；浅色背景与 `[248,248,248]` 相符 |

因此 **H3 的“布局边界”分支强支持；“左副屏/DPI 为根因”未证**。C04 原文不能看作真实屏幕 x=1235，更不能与另一会话 x=-40 直接比较：前者是窗口局部像素，后者是未同源的屏幕光标。负屏幕坐标本身合法；探针与动作均有虚拟桌面和 PMv2 处理。两条归一化公式确实不同（探针用整数 bin 中点，动作用 65535/(size−1) 端点映射），应测舍入，但此处没有数量级证据证明它能解释整块网格或多种控件 miss。

UIA 树减的是根 BoundingRectangle 原点（[uia/tree.rs](../../crates/gcu-driver-windows/src/uia/tree.rs#L136)），动作加的是 GetWindowRect 原点；WGC 也有[边界对齐](../../crates/gcu-driver-windows/src/capture/ffi/wgc.rs#L210)。两原点及客户区偏移是否相同，本次没有逐行日志，不能因代码都叫 physical 就认定已经验收。

**证伪实验 E5：** 在同一次 fixture/同一帧记录 `canvas_rect`、`canvas_res.rect`、pixels_per_point、UIA 根/网格原始 BoundingRectangle、HWND/GetClientRect+ClientToScreen、WGC bounds 与截图尺寸。若两矩形均精确 320px 且映射后的 `(7,5)` 实际落在着色单元内仍是白色，就否定布局解释，再查 capture 对齐。保持代码/常量不变的单屏与含负原点多屏、不同缩放重跑用于隔离 DPI；必须同会话同时记录 metrics/光标，不能拼接 SSH 会话坐标。

## 六、H4：顶层前台与控件焦点不是同一命题

**[代码]** [foreground::activate/verify](../../crates/gcu-driver-windows/src/input/foreground.rs#L7)先激活并等待真实前台 HWND，动作每包前重验；原生 SendInput 之前又核对一次 HWND。激活拒绝或验证失败应返回错误，无法越过 `?` 到本次这些效果超时。**[日志]** 夹具均 responsive；C08 确有 multiline_field 焦点和文本；C11 最后树的 Window 还有 FOCUSED 标记，诸行也都收到 F24。

这些证据反对“整个过程都没有获得前台窗口”，但不证明正确控件被点中。C09/W-HOTKEY 的 name_field 未聚焦是已观测症状；C22 等 counter 的点击增量，并不读取 focused_widget。UIA FOCUSED、收到 F24、GetForegroundWindow 相等，均不能替代按钮命中坐标或 GUI 线程的真实焦点 HWND。每次前台检查与 OS 消费之间仍有小窗口，也未记录瞬态抢焦点。

**判定：H4 作为全体共同根因弱支持；控件焦点失败已证，原因未证。** C04 不要求被截图窗口前台，cover 故意占前；不能据此认定 C04 是激活失败。

**证伪实验 E6：** 在动作各包原生调用前后、fixture pointer down/up 与文本接收时记录 GetForegroundWindow、GetGUIThreadInfo(hwndActive/hwndFocus/hwndCapture)、WM_ACTIVATE/SETFOCUS/KILLFOCUS、egui RawInput.focused 和控件焦点。若全程目标根/焦点稳定而点击仍 miss，否定顶层抢焦点解释；若真实消费时落到另一个窗口，则定位抢焦点时刻及调用者。不要增加激活重试来掩盖 unknown。

## 七、修复方向与 100ms 安全论证（不写实现）

### 1. 指针动作与探针共用有界事务调度

优先将“探针取得采样 → 派发 → 复读”和“动作定位 → 对应按钮包”纳入**跨父/Worker 的共同互斥/单一输入调度**。仅给两个 SendInput 调用分别加锁不够：探针仍可插在 Move 和 Down+Up 之间。取得互斥后才采样光标，不能将排队前的坐标当作当前坐标。完整点击可作为短事务；拖拽在明确稳定点处分段，让探针有机会在保持当前按钮状态/位置的间隙证明，不把整个 RPC/settle 期间都锁住。

现有 RPC 约 0.4–0.5s，远超 100ms，故不能在整个动作窗口静默探针。实际派发、IPC 等待、只读校验、settle 必须区分；只在有限原生输入临界段互斥，等待 provider/采样稳定时释放输入调度权。探针必须有截止期优先权，不能被密集拖拽饿死。

以旧对较早回调年龄 `a` 为起点，放行一个段落必须满足可验证的预算：`段落最长耗时 + 排队余量 + 新双证明完成/接纳耗时 < 100ms − a`。A 的旧锚龄常为 30–34ms、探针 SendInput 整数耗时 0ms，说明有优化余地，**不构成上述最坏预算已经满足的证明**。预算不足先暂停新注入、完成新证明；证明缺失/在途过期仍失败关闭、锁存并释放。不得把取锁、Job 入队、发送成功或光标相等当作续租。

100ms 锚点仍是**同 attempt/tag 双回调较早者**；旧代次/旧 proof 不授权。Stop 看门狗、热键线程、kill/up-only 清理不得等待动作互斥，也不能为持锁段延后 Stop。FFI 卡死仍由独立路径中止，迟到发送报告不复活。

### 2. 探针形态与动作间隙必须以真实双回调证明为前提

减少应用可见噪声可作为设计目标，但 A:L37–42 已证明本机 zero move/wheel/hwheel 不产生回调；不能换成零形态却继续声称鼠标租约健康，不能只发 F24 或相信泵心跳。任何新形态必须重新测得带标签双侧注入回调，并论证不会改变按钮/修饰键/焦点/IME；RelativePair、XButtonPair 不提升为产品候选。

“只在动作输入间隙发探针”可与上一方向结合，前提是每个间隙和临界段均能在旧锚点+100ms 之前提交新证明，且光标采样到复读不跨越动作移动。没有这种有界保证时，应拒绝/停止，不能用更旧证明跨过长拖拽或长文本，也不能在动作尚未结束/释放时伪装 Idle。

### 3. 分别修正文字语义和夹具几何

C08 方向是保留非 BMP 标量在接收链上的完整性、明确 LF/CRLF 到多行编辑的行为；用 E4 定位后选择符合 Windows 接收语义的注入分组/换行表示，或修正已证实的接收层聚合。不能只改预期字符串、把 emoji/LF 忽略，也不能为短文本无条件启用剪贴板或放宽敏感控件规则。分包仍需完整标量、成对释放、逐段守卫和不重放未知结果；若换行导致增加输入包，也纳入上述租约/事务预算。

C04 方向是让网格的可访问边界、绘制矩形和命中矩形一致，或由明确的真实网格边界生成像素契约；不能以放宽 RGB 容差或删除末列检查代替几何修正。保持八列六行和现有常量。诊断记录可追加到单独文件，原 50 条容量/状态 oracle 不需修改。两者都无需改动 fail-closed、租约锚点或任何安全期限。

## 八、仍未证实的部分与交付检查

未证实：六项失败的真实鼠标落点、H1 的实际交错顺序、C11 菜单瞬态、C15 按下/拖动/抬起分别是否命中、逐行窗口/客户区/DPI 原点、瞬态抢焦点、ChsIME/修饰键副作用、C08 原始代理对消息顺序，以及首次 key_events 溢出的精确墙钟。缺少的远端 Worker 文件没有被当作已读证据；现有 fixture activation trace 只记 AccessKit action/Focus 命令，不能替代原始指针消息追踪。

建议总控先用 E1 与 E4/E5 取得足够证据，再验证修复后的八行，并对 C22、W-HOTKEY 确认已经越过准备阶段执行其核心断言；保留真实双钩子丢失、过期与 Stop 的负向验证。本工作区不执行任何桌面实验。

文档交付检查：`python3 docs/plans/computer-use-v2/reviews/check_plan.py` 通过（14 份计划文档、261 个链接/锚点、83 张卡、67 条处置）；本文 35 个相对链接/行锚点单独检查通过（计划检查器不递归覆盖此 ADR 目录）；`git diff --check` 与新增未跟踪文档的独立空白检查通过。仅本文为交付文件，`.scratch-hw/` 不纳入交付，STATUS 未修改。

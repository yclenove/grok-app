# 0013 增补 · 前台动作与续租探针的交错

状态：提议（2026-10-20。安全相邻逻辑，需总控批准后才能合并。不改 `HEARTBEAT_TIMEOUT` 100ms、`DRIVER_DEADLINE`、5s 上限、Stop 锁存/释放时限，不放宽 fail-closed，不让陈旧证明授权注入。）
日期：2026-10-20

## 背景（事实 + 证据）

win-test 标准用户 `claudetest` 物理控制台，主干含 WL1+WL2。契约输出 `D:\cu2\hw\contract-out-20261020-204834.txt`（UTF-16），worker 轨迹 `D:\cu2\hw\trace-20261020-204834\`。结果 13 通过 / 8 跳过 / 1 not_run（W-UIPI）/ 8 失败。WL2 续租已生效：动作期 `lease_age_ms` 约 11–31，`call_ms=0; sent=2/2; packet=mouse+keyup`，没有 `low-level hook probe expired`，没有 Stop 锁存。

`rpc_perform` 对失败的前台动作返回 `phase=end`（驱动认为包已插入）。按钮包不含绝对坐标，点击落在当时的光标上。续租探针在动作窗口里读 `GetPhysicalCursorPos`，再发同位置绝对移动，并把 F24 keyup 放进同一次 `SendInput`。低级钩子按事件回调，另一次 `SendInput` 可以插进前一个包的事件之间，也可以插在「采样光标」和「发出绝对移动」之间。

### C06 时间线（监听毫秒，自钩子安装起）

| 监听 ms | 谁 | 事实 |
|---:|---|---|
| 7716 | 探针 | 第 0 代绝对移动仍被 2000ms 空闲门拒绝，`dispatched=false`。随后动作期第一对 `sent=2/2`。 |
| 7747 | 探针 | `lease_age_ms=31`，`rpc_perform` sequence=93 开始。 |
| 7748–8209 | 探针 | 约每 30ms 一对 `mouse+keyup`，`call_ms=0`，年龄 11–31ms。没有摘钩、没有过期。 |
| 8209 | 父进程 | `rpc_perform` end，477ms。oracle `grid_hit=Null`，`event_log` 空。同一坐标的截图像素检查已经通过，否则错误会是像素不符而不是 `grid pixel coordinate: timeout`。 |
| 快照 | 夹具 | `key_events` 里 36 个 `F24 pressed=false`。网格点击不产生按键；F24 只说明窗口在收键盘，不能解释 `grid_hit`。 |

C09、C11、C15、C22、W-HOTKEY 同一形态：perform 成功返回，oracle 仍是点击/拖拽前的状态（`focused_widget` 缺失、弹出项缺失、`drop_ok=false`、`counter=0`）。拖拽把按下、每一跳移动、抬起拆成独立包，抬起同样不带坐标。

### C08

四次 `rpc_perform` 都 end（约 466/398/407/396ms）：聚焦、Ctrl+A、BackSpace、正文。`focused_widget=multiline_field`，`set_multiline` 从 len=0 增长。一次 `key_events` 窗口里，`Text(中)` `Text(文)` `Text( )` 与 `Text(s)…Text(e)` 之间插着 4 个 F24 keyup，期望中的 `👩‍💻` 与换行没有对应的 `Text` 或 Enter。补充平面字符是一对 UTF-16 代理，F24 插在代理之间会拆掉字符。U+000A 没有作为 egui 换行键出现。

### 假设

| 假设 | 结论 |
|---|---|
| H1 探针用旧光标做绝对移动，或在按钮/Unicode 包中间插入 | 成立。点击包不带坐标；轨迹显示探针贯穿整个 perform；C08 的 F24 出现在字符之间。 |
| H2 `key_events` 被 F24 挤掉导致 oracle 误判 | 不成立。失败 oracle 读的是 `grid_hit` / `focused_widget` / `multiline` / `drop_ok` / `counter`，不读 `key_events`。C08 的正文事件还在窗口里，缺的是 emoji 和换行本身。 |
| H3 C04 是布局/DPI | 成立，与续租无关。三次运行（135643、172403、204834）都是 `WindowPoint { x: 1235.3125, y: 325.0 }`，期望 `[132,140,123]`，得到 `[248,248,248]`（夹具浅色面板）。(0,0) 与 (2,3) 已通过才轮到 (7,5)。C04 期间没有动作期探针。前景窗口是测试自己的 `GcuContractCover`。C19 本次通过。 |
| H4 前台窗口没焦点 | 不成立。F24 进入夹具，C08 的 `focused_widget` 已是 `multiline_field`。 |

## 决定

1. 动作包的 `SendInput` 与探针的「读光标 + 绝对移动 + 复读」共用一把不可重入的注入闸。钩子回调不拿这把闸（钩子在监听线程，`SendInput` 在工作线程或探针线程）。闸只包住一次插入，不跨 `await`，所以包与包之间仍可按现有节奏续租；真机上单次 `call_ms=0`，远小于 100ms。闸不调用 `confirm_hook_proofs`，发送成功仍然不是租约。
2. 前台正文里的 U+000D 变成一次 Enter；紧随的 U+000A 不再变成第二次 Enter。其余码元仍走 `KEYEVENTF_UNICODE`，代理对保持原样。空闲探针、陈旧对、摘钩、100ms 边界的失败关闭路径不改。

不把探针改成 ZeroMove/ZeroWheel：这份机器上那三种到不了 `WH_MOUSE_LL`，绝对同位置是目前能证明鼠标钩子的形态。不在整个动作期间停掉探针：拖拽或长文本会超过 100ms，租约会按设计锁存。

## 备选与为什么不选

- 只把移动和点击合成一个包：挡不住低级钩子在 down 与 up 之间插入 F24/移动，也挡不住 Unicode 包被拆开。闸覆盖这两种。
- 让夹具忽略 F24：oracle 本来就不读 `key_events`，隐藏探针会丢掉证据。
- 为 C04 放宽像素或改采样点：三次运行同一坐标、同一近白像素，(0,0)/(2,3) 正确。那是画布 40px 格子和 UIA 框在远端格子上的偏差，不是这把闸能修的。

## 影响

接口、协议、Host 方法、租约锚点（一对回调的较早者）不变。前台 `type_text` 的换行在 Windows 上变成 Enter，单行控件上等同用户按 Enter。需要真机复测 C06/C08/C09/C11/C15/C22/W-HOTKEY。C04 不在本次修复内。

## 退出条件

真机复测仍是 `grid_hit=Null` 或聚焦点击超时，且轨迹显示闸没有挡住探针：推翻本闸，不要放宽 100ms。若换行在闸生效后仍丢失，再单独查 egui 是否忽略 VK_RETURN，不要把 F24 从钩子证明里拿掉。

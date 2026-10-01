# Computer Use 下一轮开发计划（本机可执行周期 + 优化队列）

日期：2026-09-08。工作目录：`H:\aicoding\grok-app-computer-use`。
分支：`feat/computer-use-implementation`。
状态词只用：`not_started` / `in_progress` / `passed` / `failed` / `not_run`。
实现与验证分列。本文件**不**宣称三 OS 首版可发布。

用户要求持续迭代。每一轮仍是有界增量：写计划 → 实现 → 测试 → 记账 → 取队列下一项。缺机器的行保持 `not_run`。

## 首版仍缺（不能标 passed）

| 项 | 实现 | 验证 | 说明 |
| --- | --- | --- | --- |
| macOS arm64/x64 原生桌面 fixture | passed（适配器） | not_run | 无签名 Screen Recording / Accessibility 实机 |
| Ubuntu GNOME Wayland 原生 | passed（恒 false） | not_run | 禁止用浏览器 / XWayland 冒充 |
| Ubuntu GNOME X11 原生 | passed（适配器） | not_run | 无 DISPLAY / 实机 |
| 四 target 干净安装 / 更新 / 回退 | in_progress | not_run | Windows 测试目录装过一次；二次更新与 testdir 卸载因宿主被拉起未跑；macOS/Linux 包未打 |
| 已有 Chrome/Edge Playwright 扩展实机配对 | passed（Host 隔离） | not_run | 需用户安装扩展并授权 |
| GitHub 签名 Release / 自动 PR | not_started | not_run | 本 Goal 不做 |

## 优化队列（持续迭代，按依赖）

| ID | 方向 | 实现 | 验证 | 本机？ |
| --- | --- | --- | --- | --- |
| N1 | 窗口身份戳：targetId 含 pid+HWND+stamp，HWND 复用不得操作旧目标 | passed | passed | 是；`cu_probe` `windows_identity_host_protect`，clicks 文件仍 0 |
| N2 | 宿主控制面排除：`grok-app` 窗与标记保护窗不进 list，act 零执行 | passed | passed | 是；Fake + 自建保护窗 |
| N3 | NSIS 不夹带 `cu_probe.exe` | passed | passed | `7z l`：有 `grok-app.exe`，无 `cu_probe.exe`；未对 D: 安装 |
| N4 | 锁屏 / 安全桌面：input 不可用则暂停 run，不派发 | passed | passed（Fake）/ not_run（真锁屏） | Fake `session_lock_pauses` |
| N5 | 授权只接受当前 list 中的活目标 | passed | passed | Broker `authorize_target` |
| N6 | 剪贴板：默认不读；若粘贴则按序号恢复用户内容 | passed | passed | 自建窗 edit=`你好剪贴板`，剪贴板恢复 `USER-CLIP-CU-*` |
| N7 | 焦点漂移暂停前台输入 | passed | passed | R1 |
| N8 | 显示器拓扑 / DPI 变化使 geometry 失效 | passed | passed | R2 Fake；真多屏见下方 not_run 行 |
| N9 | 远程 IM / 定时任务 / SSH 不继承本机 Computer Use | passed | passed | R3 |
| N10 | 下载归属 run 的 staging，不信模型路径 | passed | passed | R4 |
| N11 | 多屏负坐标 / 125–200% DPI 实机矩阵 | passed | not_run | Fake 几何在 R5 passed；本机无真多屏矩阵 |
| N12 | UIA 语义树（替代纯 Win32 子控件标题） | passed | passed | R6 稳定 `el:{ctrlId}`；完整 UIA 树仍非本 Goal |

本周期已交付 **N1–N6**（N4 真锁屏仍 `not_run`）。

## 十轮有界迭代（Goal：继续十轮迭代）

状态词只用 `not_started` / `in_progress` / `passed` / `failed` / `not_run`。实现 ≠ 验证。不宣称三 OS 首版可发布。macOS 原生、Ubuntu GNOME Wayland/X11 原生、实机扩展配对、四 target 完整安装保持 `not_run`。

| 轮 | 结果 | 实现 | 验证 | 本机？ |
| --- | --- | --- | --- | --- |
| R1 | 目标焦点漂移暂停前台输入；漂移期间不再派发前台动作 | passed | passed | Fake `focus_drift_pauses`；自建窗 clicks 文件仍 0 |
| R2 | 显示器拓扑 / DPI 变化使 geometry 失效；旧坐标 / geometryRevision 拒绝且不回退桌面 | passed | passed | Fake `topology_invalidates_geometry` |
| R3 | 远程 IM / 定时任务 / SSH 不继承本机 Computer Use 授权 | passed | passed | `classify_session` 读盘 IM / scheduled / SSH；LIVE 为空回退磁盘 |
| R4 | 下载归属 run 的 staging；不信模型给的文件系统路径 | passed | passed | Host staging + Playwright `/download` 400 + `report.bin`=`staged-by-run` |
| R5 | 多屏负坐标 / DPI 几何在 Fake/合成路径正确 | passed | passed | Fake origin `-1920,108` @1.25；真多屏见首版仍缺 not_run |
| R6 | Windows fixture 元素身份是稳定 elementRef，不是仅用标题字符串 | passed | passed | Count `el:101`，p8 走该 ref |
| R7 | 用户输入或接管暂停前台自动化，并停止采集该敏感输入阶段 | passed | passed | Fake + 自建窗 takeover，clicks 不变 |
| R8 | 会话恢复重新校验目标与授权；进程重启后 traces 仍可读 | passed | passed | persist JSON → 新 broker；死目标不能授权 |
| R9 | 受管浏览器动作走 Host Broker（不只独立 worker 探针）；导航 / 下载复核实际目标 | passed | passed | 无 worker 则 unavailable；有 worker 才写 URL/文件，并复核实际 URL |
| R10 | 内嵌 WebView 已证明的 typed 子集，或诚实 unavailable；禁止任意 eval，不从其他 App 面静默复用 cookie/auth | passed | passed | `webview_honest_unavailable` |

十轮本机增量 **R1–R10** 已交付（真多屏 / macOS / Ubuntu 原生仍 `not_run`）。不把缺机器项标绿。

## 本机产品缺口 U1–U8（Goal：写真实产品代码）

不覆盖 T/R/N。不宣称三 OS 首版。不做满 50 个独立 ID。

| ID | 结果 | 实现 | 验证 |
| --- | --- | --- | --- |
| U1 | YOLO / acceptEdits 永不授权桌面 | passed | passed |
| U2 | Click `button`/`count` 下发适配器 | passed | passed |
| U3 | javascript: 不到达 managed worker | passed | passed |
| U4 | file/data/凭据 URL 拒绝 | passed | passed |
| U5 | IPC Host 必须 loopback | passed | passed |
| U6 | 每 token 限流 429 | passed | passed |
| U7 | 下载保留名拒绝 | passed | passed |
| U8 | 飞行中不可改授权 | passed | passed |

## 本机增量 V1–V7（Goal：继续）

| ID | 结果 | 实现 | 验证 |
| --- | --- | --- | --- |
| V1 | 未知 Key 拒绝 | passed | passed |
| V2 | wait 超时 isError | passed | passed |
| V3 | IPC Referer 403 | passed | passed |
| V4 | wait timeoutMs=0 拒绝 | passed | passed |
| V5 | alt+f4 等零执行 | passed | passed |
| V6 | 死目标任务卡 closed | passed | passed |
| V7 | 允许的 down 仍执行 | passed | passed |

## 本机增量 W1–W6（Goal：继续）

| ID | 结果 | 实现 | 验证 |
| --- | --- | --- | --- |
| W1 | 观察节点 cap + truncated | passed | passed |
| W2 | 超大图禁坐标 | passed | passed |
| W3 | IPC Forwarded 403 | passed | passed |
| W4 | 模型 traces cap 32 | passed | passed |
| W5 | observe JSON 不重复 png | passed | passed |
| W6 | 截断 ref 拒绝 | passed | passed |

## 本机增量 X1–X6（Goal：继续）

| ID | 结果 | 实现 | 验证 |
| --- | --- | --- | --- |
| X1 | wait 超时不改 snapshot | passed | passed |
| X2 | 模型 list 仅授权目标 | passed | passed |
| X3 | Sec-Fetch 403 | passed | passed |
| X4 | navigate 要 tabId | passed | passed |
| X5 | download 要 tabId | passed | passed |
| X6 | wait 匹配仍成功 | passed | passed |

## 本机增量 Y1–Y6（Goal：继续）

| ID | 结果 | 实现 | 验证 |
| --- | --- | --- | --- |
| Y1 | 模型 stopState 蛇形 | passed | passed |
| Y2 | wait 空白 name 拒绝 | passed | passed |
| Y3 | IPC GET 失败 | passed | passed |
| Y4 | overlay 不可选 | passed | passed |
| Y5 | wait 空白 ref 拒绝 | passed | passed |
| Y6 | ChatGPT 窗仍可选 | passed | passed |

## 本机增量 Z1–Z6（Goal：继续）

| ID | 结果 | 实现 | 验证 |
| --- | --- | --- | --- |
| Z1 | bearer 大小写 | passed | passed |
| Z2 | Cookie 403 | passed | passed |
| Z3 | StatusBarWnd 跳过 | passed | passed |
| Z4 | wait 名称长度 | passed | passed |
| Z5 | IPC PUT 失败 | passed | passed |
| Z6 | paused 布尔 | passed | passed |

## 本机增量 AA1–AA6（Goal：继续）

| ID | 结果 | 实现 | 验证 |
| --- | --- | --- | --- |
| AA1 | 模型 status 无 notes | passed | passed |
| AA2 | metadata URL 拒绝 | passed | passed |
| AA3 | tabId 长度 | passed | passed |
| AA4 | Basic 401 | passed | passed |
| AA5 | OLE 窗跳过 | passed | passed |
| AA6 | IPC PATCH 失败 | passed | passed |

## 三十轮有界迭代（Goal：继续迭代 30 轮）

状态词只用 `not_started` / `in_progress` / `passed` / `failed` / `not_run`。不宣称三 OS 首版可发布。

| ID | 结果 | 实现 | 验证 |
| --- | --- | --- | --- |
| T1 | 死进程留下的 exclusive desktop lease 不能让新 run 直接派发 | passed | passed |
| T2 | 关闭 Computer Use 时不注入会话 MCP，且不能 open_run；adapter 零执行 | passed | passed |
| T3 | 带浏览器 Origin 的 IPC 工具调用被拒绝 | passed | passed |
| T4 | Host 轮换会话凭证后，旧 Bearer 被拒绝 | passed | passed |
| T5 | 对非用户授权 targetId 的 act 失败；已授权目标不变 | passed | passed |
| T6 | pause / generation 变化之后到达的 adapter 结果不记为成功 apply | passed | passed |
| T7 | 新 observe 之后，旧 observation 的 elementRef 被拒绝 | passed | passed |
| T8 | 超出最近观察图的坐标被拒绝，且不回退桌面 | passed | passed |
| T9 | 无目的地的 Drag（无坐标且无 drop 子控件）被拒绝，不当作 click | passed | passed |
| T10 | 含 NUL 或超长 Type/set text 被拒绝，零执行 | passed | passed |
| T11 | wait 缺 elementRef 或 timeout 超上限被拒绝 | passed | passed |
| T12 | handoff 请求暂停等用户，模型不能自己完成 | passed | passed |
| T13 | 隐藏 UI 预览停止周期采集；run 未暂停时模型 observe 仍可用 | passed | passed |
| T14 | 最小化 / iconic 窗口不出现在目标列表 | passed | passed |
| T15 | 受管浏览器 worker 拒绝非 loopback 客户端 | passed | passed |
| T16 | 打开已被另一 run 占用的 managed profile 被拒绝 | passed | passed |
| T17 | navigate 后 Host 记录 worker 实际 URL；实际目标不匹配则拒绝 | passed | passed |
| T18 | 带模型文件系统路径的 download 被拒绝；字节只进 run staging | passed | passed |
| T19 | 取消 run 不关闭用户借用的 tab | passed | passed |
| T20 | 两个 run 不能共享一个 managed browser profile | passed | passed |
| T21 | fork 后的 run 无 snapshot，不能重放源 actionId | passed | passed |
| T22 | 关掉产品 flag 会对每个 open run 请求 stop | passed | passed |
| T23 | settings catalog 仍有 Computer Use；Composer 仍有 computer-use slash | passed | passed |
| T24 | Computer 面板 Pause/Stop 走 Host pause/stop（含 busy/error/empty） | passed | passed |
| T25 | native Wayland 恒 false；受管浏览器成功不记成 Wayland native | passed | passed |
| T26 | 耗尽 run 动作预算后拒绝继续 act | passed | passed |
| T27 | reconnect 清除授权；未重新授权则 act 被拒绝 | passed | passed |
| T28 | resume 不是模型工具 | passed | passed |
| T29 | 观察默认不含剪贴板内容 | passed | passed |
| T30 | 模型 traces/status 不含仅 UI 的预览采集 | passed | passed |

## 本周期增量（必须是已上线路径）

1. Windows `target_id` 从 `win:{pid}:{hwnd}` 改为 `win:{pid}:{hwnd}:{stamp}`。stamp 写在窗口属性上。属性被清掉或 HWND 被别的窗复用 → `target_alive` 为假，Broker 拒绝，**不**回退桌面。后置条件：自建窗 click 计数文件仍为 0。
2. `grok-app.exe` 进程窗与 `GrokCuProtected` 窗不出现在 `list_targets`；对它们 `authorize` / `act` 零执行。
3. `authorize_target` 只接受适配器当前列出且仍然活着的 id。
4. 会话输入桌面不可用时暂停 run，不执行。FakeAdapter 可测。
5. `cu_probe` 挪到 workspace 成员 `src-tauri/cu-probe`，app 包只留 `grok-app`。`cargo run --bin cu_probe` 仍可用。NSIS 列举不得出现 `cu_probe.exe`。

不变式：默认关；死目标不回退桌面；YOLO / acceptEdits 不授权桌面；不把浏览器 / XWayland 写成 Wayland native；新文案走 i18n（本周期无新 UI 文案）；不涨 `App.tsx` / `AppWorkbench.tsx`。

## 非目标

三 OS 首版验收、签名 Release、四 target 完整矩阵、实机扩展配对、点 ChatGPT / 微信、改壁纸工作区、默认打开功能、改 `~/.grok`、把 `cargo test --lib` 的 `0xC0000139` 当产品通过。

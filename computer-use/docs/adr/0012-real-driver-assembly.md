# 0012 将真实平台驱动与 DOM 生产通道装配进 gcu

状态：Proposed
日期：2026-10-17

## 背景（事实 + 证据链接）

用户于 2026-10-17 批准启动此项，要求先审 ADR、批准后再拆实现卡。本提案只定义装配，不改变 [03 架构](../../../docs/plans/computer-use-v2/03-architecture.md) 的 Driver、Host、ACP 接口与安全原则，也不代表批准上线。编号 0010 已由内置浏览窗预占（见 [0011](0011-auto-screenshot-policy.md)）；本检出未包含 0010 正文，不推断其接口。

### 现状事实表

下表来自源码静态核对；“驱动实现存在”“驱动测试通过”和“产品入口可用”是三件不同的事。

| 平台 / 入口 | 当前选择点与实际路径 | 限制 / 证据 |
|---|---|---|
| 全平台普通执行入口 | [`args::DriverKind`](../../crates/gcu-cli/src/args.rs) 只有 `Fake`；[`runtime::run`](../../crates/gcu-cli/src/runtime.rs) 在普通入口调用 `require_fake`，随后直接构造 `FakeDriver` | 未指定 `--driver` 返回参数错误、退出码 2；debug 可显式 fake，release 还要求 `GCU_ALLOW_FAKE=1`。该变量只允许离线驱动，不是系统权限或审批。 |
| Windows `mcp` / `call` / `doctor` / 无子命令 | 走上述 fake 分支；CLI 已依赖 Windows crate，但没有选择真实驱动 | [`WindowsDriver::monitored`](../../crates/gcu-driver-windows/src/driver.rs) 是带 Host latch 的生产入口；`new()` 默认 Worker，但用 `UnboundStop` 保持停止。私有 [`gcu worker`](../../crates/gcu-cli/src/worker.rs) 已启动真实子驱动，不等于前端已装配。 |
| Linux X11 / Wayland 同上 | 走 fake 分支；[`gcu-cli/Cargo.toml`](../../crates/gcu-cli/Cargo.toml) 尚无 Linux 驱动依赖 | [`LinuxDriver::connect`](../../crates/gcu-driver-linux/src/driver.rs) 先连 AT-SPI，再尝试 RealX；有真实树、X11 截图/动作和守卫。Wayland 模块存在，但 `Driver::capture` 对 native Wayland 窗口仍返回 `CapabilityUnavailable`；[`with_portal_input`](../../crates/gcu-driver-linux/src/driver_portal.rs) 必须由可信组装者显式调用。 |
| macOS `mcp` stdio、未指定驱动 | [`helper::proxies_mcp`](../../crates/gcu-cli/src/helper/mod.rs) → 签名校验的 helper 代理 | [`helper/macos.rs::run_agent`](../../crates/gcu-cli/src/helper/macos.rs) 已构造真实 `MacosDriver::new()`；[`new()`](../../crates/gcu-driver-macos/src/driver.rs) 用独立的默认已停止 latch，尚未换成 LocalHost 持久 latch。不能据此宣称 helper 可安全投递。 |
| macOS `agent` / 无子命令 | `agent` 默认真实驱动；显式 fake 先过 `require_fake`。无子命令默认拉起 helper/onboarding | TCC 属于经 Launch Services 启动的 helper；终端直接执行不是同一权限链，见 [0006](0006-macos-tcc-helper.md)。 |
| macOS `call` / `doctor` / 自建 HTTP MCP | 不经过 helper 的 stdio 分支，仍过 `require_fake` | 不能把默认 CLI 的 TCC 检测当作 helper 权限诊断；[`doctor.rs`](../../crates/gcu-cli/src/doctor.rs) 目前还固定输出 `driver: fake` 和权限不适用。 |
| 全平台 `engine`（路线 B） | runtime 在 `require_fake` 后传入 fake；[`engine.rs`](../../crates/gcu-cli/src/engine.rs) 接收 `Arc<dyn Driver>` | 已有 RPC/Stop 双向认证、会话 token 与生命周期；但硬设 `user_activity_guard=false`、注入 `UnavailableActivity`。只替换 Driver 不会完成活动与 Stop 接线。 |
| 不需要选驱动的命令 | `version`、安装/卸载、审批管理、resume、turn-ended、MCP `--connect` 代理提前分流 | [`install_cmd`](../../crates/gcu-cli/src/install_cmd.rs) 与 [`install/`](../../crates/gcu-cli/src/install/) 安装的是 `gcu mcp`，没有补 fake 参数；普通安装成功不代表桌面可用。 |
| 自动审批 / 故障门控 | [`approval_mode`](../../crates/gcu-cli/src/args.rs) 只在 **debug 且 `eval-autoapprove`** 时接受限定的 `GCU_APPROVAL` 值 | release 或非 eval 构建遇到任何该变量均拒绝。runtime 的 `GCU_EVAL_READ_ERROR`、`GCU_EVAL_ORACLE_FILE` 也在该双门控内并依赖 fake；不能随真实装配迁成生产旁路。 |
| 浏览器 L2 | [`native_host::serve`](../../crates/gcu-cli/src/native_host/mod.rs) 只做 challenge、共享确认/撤销；`bind/request/RouterGate` 尚无产品调用者 | [`browser::DomChannel/Hub`](../../crates/gcu-core/src/browser/mod.rs) 与 [`with_browser`](../../crates/gcu-core/src/browser/router.rs) 已有融合与路由，只有 fake 通道测试；MCP 会话创建尚未注入 Hub。当前 [`manifest.json`](../../tools/browser-extension/manifest.json) 没有 `key`，因此 [`ExtensionIdentity::embedded`](../../crates/gcu-cli/src/native_host/identity.rs) 和安装真实扩展身份均失败关闭。 |

fake-only 披露集中在中英文 [README “What this build runs”](../../README.md#what-this-build-runs) / [“本构建实际运行的内容”](../../README_ZH.md)、接入客户端段落、平台差异表和故障排除表；其中明确记录 Windows/Linux 安装后的无参数 `gcu mcp` 仍退出 2、macOS `call/doctor` 不代理 helper。发行包另带 [packages/README](../../packages/README.md)，其安装冒烟要求 native 成功，禁止拿离线结果替代。这些披露只能随相应入口验收更新。

[路线图](../../../docs/plans/computer-use-v2/06-roadmap.md) 的 P0-10 明确是 fake CLI；P1/P2/P3/P7 是驱动卡，P8-02/P8-03 是 native messaging 与 DOM 融合卡。[STATUS](../../../docs/plans/computer-use-v2/STATUS.md#发现卡外问题一行一条) 明确留下驱动 latch/活动时钟未接 Host 的缺口；P4-08、P5-01、P8-03 的“未做”也没有把生产装配计为完成。已有证据限于其记录的范围：macOS 27 arm64 驱动契约、Windows 受限交互 runner（大量前台/物理 Stop 跳过）、Linux Xvfb/AT-SPI；Wayland 真 compositor、完整 CLI/helper/engine/扩展联调与多项发布验证仍是 `not_run`。

必须保留的既有决定：[0002 错误/能力扩展](0002-driver-interface-extension.md)、[0003 可信恢复边界](0003-resume-trust.md)、[0004 Host 认证与活动来源](0004-host-auth-dependencies.md)、[0004 不完整树拒图](0004-raw-tree-completeness.md)、[0005 Worker 隔离](0005-windows-worker-json.md)、[0006 TCC](0006-macos-tcc-helper.md)、[0007 PipeWire 构建门控](0007-ashpd-pipewire-deny.md)、[0008 reis 输入范围](0008-reis-portal-input.md)、[0008 只读映像哈希预算](0008-linux-image-hash-budget.md)、[0009 WebView 依赖例外](0009-wry-onboarding-webview-deny.md)、[0011 auto 截图](0011-auto-screenshot-policy.md)。

## 决定

以下为待批准的目标行为；本文件不使任何真实路线从关闭变为启用。

### 驱动选择与失败方式

| 选择 | 目标语义 |
|---|---|
| `--driver auto`（建议省略参数时的默认值，待 Q1 裁定） | 按当前目标 OS、入口责任进程与已编入的后端选择 **真实** 路线。Linux 先识别 X11 / Wayland / XWayland，不能因有 `DISPLAY` 就把 Wayland 当纯 X11。没有安全可用路线即失败。 |
| `--driver native` | 用户明确要求当前平台真实驱动；适用同样的入口、构建、权限、Stop 与能力检查。不表示强制输入、提权、打开隐藏 feature 或忽略不支持。 |
| `--driver fake --scene …` | 仅离线内存桌面；保留 debug 或 release + `GCU_ALLOW_FAKE=1` 的显式门控。`--scene` 只对 fake 有效，显式用于 auto/native 时拒绝参数，防止将离线测试误变为真实桌面操作。 |

在 `gcu-cli` 的专用组装模块统一选型，runtime/engine/helper 不各自猜测。用 target cfg 和建议的独立编译 feature `native-windows`、`native-x11`、`native-wayland` 控制尚未交付的装配；初始均默认关闭，按平台卡完整验收后由发行构建显式开启。feature 只是交付开关，运行时仍须逐次门禁。保留已有 `pipewire` / `macos-webview` / `macos-private-spi` 门控，不新增依赖白名单或环境变量授权开关；不设 `GCU_DRIVER` 隐式选型，`GCU_ALLOW_FAKE` 也不能覆盖 CLI 所选驱动。

**真实驱动不可用时 fail-closed：不静默退回 fake，不切换更宽的输入路线，不换窗口、不请求提升、不借其他进程的权限。** 未编入/不支持的选择返回参数错误（退出 2）；已选择 native 的运行失败映射既有 `permission_missing`、`capability_unavailable`、`elevation_required`、`handoff_required` 等错误，不伪造成功。`doctor` 应能报告 native 初始化失败和真实能力，`ready=false` 按现有规则退出 3；只报告元数据，不据此授权或执行目标动作。任何必要的 Windows 守卫探针副作用必须明确披露。

`capabilities`、doctor、initialize instructions 使用同一已组装能力源，并显示所选 fake/native 与禁用原因；不得继续输出固定 fake 权限说明。fake 的测试身份、持久审批记录、快照、图片与 native 隔离，不把离线 Always 迁成真实桌面授权；切驱动、升级与清理离线数据不得清除真实 Stop。安装器不写 fake 或自动审批；其现有 `gcu mcp` 在新版本可能首次接触真实桌面，必须随 A5 明示此变化。

### 所有平台先完成的 Host / Stop 装配

1. 路线 A 先读取 LocalHost 策略与持久 Stop；路线 B 先保持投递关闭、认证预期 Host 实例的 RPC/独立 Stop 连接并恢复 Stop。驱动初始化所需的能力探测可以在关闭投递时进行，握手所报能力须保守，不能先宣布输入可用。只有责任进程、latch、独立释放/取消、活动来源均接好后才开放相应路线。
2. 用 CLI 内部适配器把各平台 `StopLatch` 接到 Host 的同一停止状态，把活动时钟接到路线 A 的 `UserActivity` 或路线 B 的 `PhysicalActivity`；不可另建一个驱动 latch 再定时同步。全局物理热键停止全部会话；单会话 Stop 仍由 Router/会话门禁隔离，共享驱动不能通过任意一个会话的已恢复状态放行另一个已停止会话。
3. 热键/按钮先同步锁存，独立控制任务再释放输入、持久化和取消队列；磁盘慢、审批挂起、驱动初始化、worker 重启不得阻塞停止。热键冲突、监听/租约失效、权限撤销禁用依赖能力；已经运行中失效则停止在途操作。活动来源失败返回 `None`，绝不当作“用户已空闲”。
4. 路线 B 的驱动本地热键目前没有向 App 报告 Stop 的既有专用消息。建议在引擎先锁存/释放，再主动断开独立控制连接；Host 将**非正常断线**在现有断线回调中锁存并持久化全局 Stop，确认持久状态前不允许重拉引擎投递。正常显式 shutdown/空闲退出须另行识别，仍不得清掉旧锁存。这是待实现的保守断线策略，不是声称现有回调已持久化。若要保持连接并单独上报本地 Stop，须另提 Host 协议 ADR（Q2），不能借 `host.event.emit` 的动作结果或 heartbeat 夹带新控制语义。
5. 恢复只走已有可信用户 UI/CLI 控制，沿用 0003 的 TTY 尽力而为限制和 App 用户事件/序号防重放；不注册 MCP 恢复工具。恢复前重新检查权限与 Stop 健康，不复活旧 portal/DOM share，也不重放未完成动作。轮次结束释放输入/清 turn 授权，会话关闭撤 token、订阅、worker、portal 和图片，均不解除 Stop。

本提案不改 `Driver`、`DriverAction`、`ApprovalProvider`、Host/ACP 消息或工具 schema；Rust 组装用的构造注入可放 CLI/MCP 适配层，不能把平台依赖下移 core/MCP。`src-tauri` 仍只依赖两个协议 crate（03 §2/§15），不得为装配直链驱动。

### 平台顺序与限制

| 平台 | 装配顺序与必须保留的限制 |
|---|---|
| Windows（A1） | Host Stop/可信策略 → 自身 token、完整性、UIAccess 与交互桌面预检 → `WindowsDriver::monitored(WorkerConfig, hotkey, host_latch)` → 父进程 LL 双钩子、RegisterHotKey、输入账本/独立 up-only 释放、Job Object 和 Worker 握手 → 按健康状态开放能力。自身 High/System 或 UIAccess 直接拒绝；管理员目标/安全桌面所有观察与动作路线统一拒绝，不降权重启绕过。生产默认 Worker，不能因启动慢回退 `InProcess`。钩子健康、Stop 租约与前台策略缺一均禁依赖路线；仍安全的已声明语义/定向路线可保留。 |
| Linux X11（A2） | CLI 以 Linux target 依赖驱动 → Host latch/活动适配 → AT-SPI 常驻连接 → RealX、窗口/进程映像身份对齐 → XI2 物理活动、XGrabKey（含锁定键组合）、InputLedger/watchdog → XComposite 与能力报告。构造器当前会先附监听器；后续替换 latch 必须重绑监听器/HotkeyManager，不能只改结构字段留下旧引用。XTEST 是全局前台输入，必须经 Router 前台策略、活动守卫及健康 Stop；TargetedOnly 不升级。 |
| Linux AT-SPI 缺失/断线 | 当前 `connect()` 先失败，产品保持 native 未就绪；可交付权限/能力诊断，不能换 fake 或宣称“纯坐标照常可用”。部分树仍可按既有完整性标记提供文本；截图敏感区无法可靠定位时拒图/handoff，不因有 X11/portal 像素就绕过。若只缺某个 EditableText/Action 接口，返回既有不支持错误，由模型在重新检查策略后选择后续动作，不自动键入重试。 |
| Linux Wayland（A3，晚于 X11） | 明确 compositor 与目标是 native Wayland 还是有精确 XID 的 XWayland → 可靠窗口/进程实例/授权版本绑定 → 用户 portal 同意 → ScreenCast 单窗口来源与 PipeWire 帧 → 经验证的 RemoteDesktop `ConnectToEIS` FD / reis → 实际注册并持续健康的独立 Stop。探测到 GlobalShortcuts 接口不算注册成功；GNOME 无胶囊等限制如实报告。XWayland 只能使用已验证的该 X 窗口能力，不能用 XTEST 控制 native Wayland。 |
| Wayland 生命周期与降级 | restore token 目录 0700、文件 0600，绑定用户/App 映像/进程实例、compositor、source/窗口、授权版本；有效期不超过 24h 且不超过授权期限，按会话命名空间管理。恢复重验来源/流 node、serial、mapping ID 与输入范围；仅当前焦点不足以证明键盘隔离。到期、撤销、关闭功能/会话时释放输入、关闭实际 portal 并清 token；错误清理后仍保持停止。无法证明来源/输入范围/Stop 时关闭对应路线，可保留可靠的 AT-SPI 文本观察或语义动作；不能回退全桌面采集、portal Notify*、环境 EI socket 或 XTEST。 |
| macOS helper（A0-M） | 保留 Launch Services + AppKit/CGS 初始化 + audit-token 签名/cdhash/版本绑定；改用 `MacosDriver::with_stop_latch` 注入 LocalHost 持久 Stop 与真实活动源，接可信控制/释放。缺 TCC 走既有 onboarding，不能自动点击授权；0006 的字段 51 到达验证、Chromium 后台不可用和 SPI 运行时门控不变。未经此装配验收，现有 `MacosDriver::new()` 不能简单解除停止。 |

Windows 复用 [diagnostics.md](../../crates/gcu-driver-windows/diagnostics.md) 的现行固定探针顺序及用户已批准的约束：前三种零输入后才尝试一次同位置绝对移动，启动前系统输入空闲采样至少 2000ms；真实物理回调撤销许可，光标变化则本次证明作废、不补偿、不重试。该移动可能触发 hover；双钩子真实 tagged 回调、100ms lease、1000ms proof 启动预算不变。`GCU_HOOK_MOUSE_PROBE` 仍仅 debug + `test-worker-faults` 可用，release 拒绝；RelativePair/XButton 不进产品。已有旧探针矩阵不证明新产品路径通过，须物理鼠标且无人操作补测。

macOS **非 helper** 入口按责任进程处理：

- `mcp` stdio 的 auto/native 继续代理 helper；显式 fake 留在进程内。无子命令继续 onboarding，不新起终端权限下的驱动。
- `engine` auto/native 只在获认证的正式 App 子进程链里装配 MacosDriver，复用同一 latch/活动适配；正式签名/构建校验、独立 Dev 授权不变。TCC 责任链缺证据的系统版本不得宣称可用。
- 独立 `call`、`doctor`、自建 HTTP MCP 的 auto/native 本阶段明确拒绝并指引 helper/App 的已支持入口，不从终端直接用原生权限，也不隐式 fake。未来要提供 helper 单次调用/完整 doctor，先证明现有 MCP/helper 通道足够；若需扩 helper 信任接口则单独 ADR。fake 用法保持显式。
- `worker` 仅 Windows 私有入口；`native-host` 只承载浏览器连接；安装、审批管理、恢复与 `mcp --connect` 不另建真实驱动或复制权限。

### Stop、审批与期限不变

沿用 03 §5/§7/§15：App 内容观察先授权，动作仍完整经过 ActionRouter 的拒绝名单、审批/敏感确认、身份与进程实例重验、fingerprint/几何/32×32 区域哈希、策略版本、前台守卫、账本与 settle。portal 同意、共享标签页和 `--driver native` 都不等于 App/动作授权；网页默认 strict。登录/系统安全提示与未知执行界面 handoff。

Stop **≤100ms 锁存、≤500ms 发释放，阻塞原生调用 ≤5s 超时或杀 worker** 保持不变。[`DRIVER_DEADLINE`](../../crates/gcu-core/src/driver.rs) 是 **4750ms**，给 core 的 5s 原生上限留出取消/清理余量；不能为了冷启动加长它，也不能在多层重试中重新起计时。0005 的 worker 单次 10s watchdog 是内部兜底，不允许产品动作等到 10s；外层剩余预算/Stop 必须能提前中断并杀/reap。只读最多一次的 worker 恢复也受调用方剩余预算约束，变更结果不确定始终 `unknown`、不重放。

审批等待仍按独立审批期限（默认 120s）；用户 portal 选择与预热属于可取消的控制面，不能阻塞 Stop 或在用户尚未同意时开始输入，各启动/IPC 操作仍守既有硬超时。工具开始等待原生操作后不得用“初始化中”重置预算；尚未就绪就返回既有能力/权限/busy 错误，不能把冷启动藏进无界等待。0008 的冷映像哈希是独立只读身份预算，不能借它扩大动作/截图/IPC 的 5s 上限。DOM observe/perform 同样受 Router 5s 及取消约束，超时后不能再把同一动作发给 L0。

### native host / DomChannel 的生产连接

生产连接放在 CLI 的私有 broker/适配模块，core 只持有现有 `DomChannel`，MCP 只负责在创建会话/Router 时注入 Hub 与生命周期钩子。路线 A 的 broker 在实际持有 Driver/Router 的进程（macOS 为 helper），路线 B 在 `gcu engine`；浏览器拉起的 `gcu native-host` 是转接子进程，不另开一套 Router、驱动或授权。

1. **建立连接：** 浏览器使用安装 manifest 启动 `gcu native-host <origin>`，保留现有 stdin/stdout 长度前缀 JSON（little-endian、每帧 ≤1MiB、开始帧后 ≤5s、有限排队/背压）。转接进程连接目标引擎的每实例私有 UDS/本机命名管道；使用受限目录/DACL、防路径替换/抢先创建和 OS 对端实例校验。只发现端点不授予能力，不按“第一个同 UID 引擎”绑定。无监听者/多个不能唯一绑定的候选则拒绝 L2，不自启可操作桌面的引擎，不新增 HTTP 入口。
2. **身份：** 扩展 ID 必须从编入 manifest 的 base64 公钥 SHA-256 前 16 字节按 a–p 编码派生，允许源严格为 `chrome-extension://<id>/`。缺 key、占位 key、错 origin 或安装 manifest 不一致立即拒绝；无 argv/env/页面 override。保留每连接随机 challenge/版本确认与 request epoch，错误帧即断线。公钥和 origin 不是秘密，也不证明任意本地进程就是浏览器；broker 另核验 native-host 的 OS 进程实例/预期 gcu 构建身份及浏览器启动链，双方绑定引擎实例与新连接。只读同 UID/token 文件不够；无法核验的启动器/平台维持 L2 禁用。限制同 03：不声称抵抗已控制浏览器或可改写可信进程的同用户攻击者。
3. **会话与窗口绑定：** `Shared` 只是浏览器用户同意，native `tabId/windowId`、`--parent-window`、标题、URL 均不能直接当作 OS 窗口或 core session。引擎持有 session ID，先验证浏览器进程/原生窗口与当前共享 top document 的可信对应，再核验 web-area fingerprint、frame、viewport/物理像素映射，才同时调用 `Connection::bind` 和 `Hub::bind`。映射证明的可实现性须由 A4a 提交平台证据；不能用近似标题或几何唯一性代替文档身份。只允许唯一匹配的已授权本地会话；多会话候选需要可信用户选定，未有该选择路径前保持 L0。页面不能指定 session，不能把一个 share 自动借给另一个会话。
4. **Router 是唯一权限边界：** observe 先过 App 读取授权，DOM 变更必须来自该会话完整 ActionRouter 的最终预检。`RouterGate::authorize` 消费与会话/引擎 epoch/share/document/窗口/App 身份/参数/快照/策略版本绑定的一次性投递许可；不得把 `shared` 或一个缓存布尔值当许可，也不得递归再走一套审批。`Connection::request` 及响应归属沿用服务端 pending 表；DomChannel 把 snapshot 交现有严格解析/融合/脱敏，不信回复里的自报 session。值/DOM 原文不进诊断日志。
5. **撤销与 L0：** 未共享、身份/文档/frame 失配、读通道不可用时，清理 active DOM/句柄并重新提供已声明可用的 L0；L0 仍要同一授权、Stop、前台与截图门禁。已开始 DOM 变更后丢帧、超时、断线、撤销返回 `unknown`，不自动重试或回 L0 再投一次。导航、标签关闭、撤销共享、Stop/Stop 健康丢失、会话关闭/fork/切模型、引擎换代均清 share/pending/Hub/图片归属；迟到响应不可重新入库，恢复需重新共享。

该连接是 gcu 内部版本化通道，不是 Host/ACP 方法，不传会话 MCP bearer、审批/Always 写入或恢复能力给扩展。本 ADR 不扩浏览器权限，不读 cookie/存储，不执行任意 eval。若窗口证明、多会话选择或本地 Stop 上报必须新增 Host/ACP 方法/字段，A4/Q2 必须停下另提 ADR，不能顺带改 03 或复用不相干消息。

## 备选与为什么不选

| 备选 | 不采用的原因 |
|---|---|
| 保持 fake-only，把驱动单测当产品支持 | 无法完成真实桌面用户路径，也掩盖 Host/Stop/生命周期缺口。 |
| native 初始化失败就 fallback fake，或按环境自动换输入/权限 | 返回假成功或扩大范围，用户不能判断模型实际操作了什么；违反默认安全失败与投递边界。 |
| Windows `InProcess` 默认、关闭鼠标证明或加长 deadline | 不能杀卡死 FFI，失去活动/Stop 证明；冷启动问题应预热/限时取消，不放宽门禁。 |
| Linux 没 AT-SPI 就只截图点击；Wayland 借 XTEST/全屏 portal | 无法可靠定位敏感区/目标，且无法证明输入范围；有画面不等于有权限。 |
| 扩展直连 MCP HTTP、页面提交 session、共享即授权 | 新增攻击入口，混淆浏览器同意和 App/动作审批，绕过 Router。 |
| App 链接真实驱动，或 macOS CLI 直接使用终端 TCC | 违反 03 §2/§15 依赖/责任进程边界，产生两套权限与产品行为。 |

## 影响（对接口、测试、用户）

### 分阶段实现卡建议（批准后再登记）

下表不是 STATUS，也不宣称已有验收。路径均相对仓库根；每卡需完整错误/取消/关闭路径、同源能力披露和测试，未交付路线持续关闭。通用依赖为本 ADR 获批、相关既有卡已在集成基线合入；实现顺序 A0 → A1 → A2 → A3，A0-M 与其他平台的后续卡可独立排期，A4a 可先做证据核实但 A4b 只启用已验收平台。

| 卡 | 文件范围 | 依赖与 DoD | 必须真机验证 |
|---|---|---|---|
| A0 统一选型与停止闭环 | `computer-use/crates/gcu-cli/src/{args,runtime,engine,doctor,host}/`（现有单文件及新增组装模块）、`tests/`；`gcu-mcp/src/host/`；必要的 `gcu-host-protocol/src/supervisor/` 与 `src-tauri/src/computer_use/` 薄断线适配 | Q1/Q2 裁定；auto/native/fake、release/eval/scene 错配矩阵、未装配失败、doctor 不再假报 fake、latch/活动可注入；本地 Stop→Host 持久化→重启闭环，未完成保持 native feature 关闭；Host/工具 golden 不变、check-deps 通过。 | 对应平台的独立 Stop、断线/引擎重启仍停止；单元时钟不算实际时限证据。 |
| A0-M macOS 责任进程与 helper 接线 | `gcu-cli/src/helper/`、组装模块/engine、`gcu-driver-macos/src/` 必要注入、两 crate 测试 | A0；LocalHost/RemoteHost 与 `with_stop_latch` 一致、AppKit 初始化、helper 认证/恢复/重拉完整；非 helper 入口严格按上文处理，不放开直连终端 TCC。 | helper-only TCC、App 子进程责任链、撤销/错误 PID/安全提示、签名/Dev 隔离、物理热键；macOS 14/15/Intel 缺证据均 `not_run`。 |
| A1 Windows 装配 | `gcu-cli/src/` 组装与 worker/doctor、`gcu-cli/tests/`；`gcu-driver-windows/src/{driver,input,worker}/` 及测试、`diagnostics.md`（仅必要对接） | A0；monitored Worker、Host latch/活动、Job/父账本释放；提升/UIAccess/安全桌面拒绝；证明丢失禁前台；产品入口跑原生契约与外部 oracle，unknown 零重放。 | 物理鼠标/键盘、现行第四探针启动与续期、UAC/钩子丢失、热键冲突/按住输入时 Stop/卡死 worker、DPI、系统 App 冒烟；托管 runner skip 不替代。 |
| A2 Linux X11 装配 | `gcu-cli/Cargo.toml`、锁文件与组装/tests；`gcu-driver-linux/src/{driver,x11,atspi}/` 及测试 | A0、A1；cfg 依赖不越界；监听器实际持有 Host latch、活动源一致；AT-SPI 缺失未就绪、部分树拒图、XTEST 前台门禁、无 Composite 遮挡拒图；CLI/engine + Xvfb/AT-SPI 全链与外部 oracle。 | 实际 X11 桌面物理活动、XGrabKey 冲突/失效/Stop 时限；Xvfb 只证明对应虚拟桌面路径。 |
| A3 Linux Wayland 装配 | `gcu-cli` 组装/生命周期/tests；`gcu-driver-linux/src/{driver,driver_portal,wayland}/` 及 portal/identity/contract 测试；必要 CI 平台 job | A2、P7-02/03/04/05；把实际 ScreenCast capture/geometry/窗口来源接入 Driver，不能仅设置 PortalInfo；token 来源/到期/撤销、权限期限、reis 范围、真实 Stop 注册与释放闭环；不支持的 compositor 明确禁用；PipeWire feature 构建与能力一致。 | GNOME/KDE/wlroots 分别有真实 portal、重授权/撤销/到期、帧来源/映射、Stop 丢失、会话关闭证据；缺一个平台只列 `not_run`，不宣称 Wayland 全支持。 |
| A4a 浏览器身份/映射证据 | `gcu-cli` 私有 native broker 的测试/平台证明适配、`native_host/` 测试；`computer-use/docs/browser-extension.md` 的证明说明 | A0、P8-02/03、Q3；固定 key 来源、OS 对端/浏览器启动链与 top document→原生窗口证明可复现；唯一 session 绑定规则可审。无证明则只交付禁用 L2 的完整拒绝路径，不能准入 A4b。 | 三平台 Chrome/Edge native messaging 启动链、错 key/origin/伪转接进程、错窗口/标签/文档与多引擎歧义。需新 Host/ACP 接口则另 ADR。 |
| A4b DomChannel 生产接线 | `gcu-cli/src/native_host/` 与私有 broker/组装/engine/helper、`gcu-mcp` 会话/Router 注入与 cleanup、`gcu-core/tests/dom_router.rs`、扩展既有 TS/tests | A4a、相应 A1/A2/A3/A0-M；现有 DomChannel 实现、Hub/Connection 同时绑定、RouterGate 一次消费、完整 Router 策略；断线/Stop/导航/跨会话/迟到响应清理，unknown 不回 L0 重放，预算/黄金文件不变。 | 真实扩展→native-host→CLI/helper/engine→Router 往返；共享前零读取、共享撤销、L0/L2 与审批/Stop/脱敏、同初态 browser A/B。 |
| A5 用户文档与发行启用 | `computer-use/README{,_ZH}.md`、`packages/README.md`、`docs/{wayland,browser-extension}.md`、`docs/llm-wiki/computer-use.md`、必要发行 feature/安装冒烟配置 | 相应已验收平台卡（声称 L2 还须 A4b）；逐入口替换 fake-only 披露，未交付平台保留；列选型/错误、默认变化、权限/停止/清理；干净包验证实际编入后端，安装器不加 fake/自动审批，严格符合 07 §8。可按平台分批，不得提前全局删除限制说明。 | 干净机器从安装→接入→授权→操作夹具→Stop/恢复/撤销/卸载；未跑仍如实披露。 |

上述范围只预告后续卡，本次仅新增本 ADR；不改代码、03、协议、路线图或 STATUS。实现卡新增用户文案须同时覆盖既有 15 语言目录；若另增设置必须登记 settingsCatalog，产品状态/界面不得增长 App shell 或 AppWorkbench。

### 安全影响与威胁

| 威胁 | 必须保持的防线 / 验收 |
|---|---|
| 模型首次获得真实桌面能力，旧安装在升级后开始接触桌面 | feature/发布门禁、App 默认关闭、本地会话准入、OS 权限 + App 授权 + 敏感确认；拒绝名单/未知执行界面不因接真驱动失效。旧 fake 授权不能继承。 |
| fake/native 混淆、测试变量进入发行、能力虚报 | 显式 fake、场景错配拒绝、release 变量门禁矩阵、doctor/instructions 同源，包安装冒烟必须证实实际 native 后端。 |
| 热键只停驱动而 Host 未停；重启或会话切换恢复投递 | 共用 latch、独立释放、可信恢复、Host 持久化/新引擎阻断证明；活动来源与 Stop 不可用时关闭依赖路线，不加长时限。 |
| 本地伪 native host、扩展/页面伪造身份、旧响应串会话 | 固定 key 派生 origin + 浏览器信任边界、OS 实例/构建与启动链核验、服务端绑定、一次性 request/epoch、文档/窗口重验及清理；公钥不当作密钥。 |
| DOM 动作绕审批或失败后 L0 再执行；Wayland 采到他人窗口 | Router 单一路径、结果确定性/no replay、来源与 token 范围证明；不能通过标题/焦点/画面相似扩大范围。 |
| 日志、真实截图/DOM、缩略图/审批 crop/临时图泄露 | 原始树先收敏感区，再裁树/同步/脱敏；原生不完整拒图；所有衍生物只用安全结果、按会话校验归属和删除。默认 metadata-only；`GCU_LOG_PAYLOADS=1` 仍脱敏；token、输入文本、原始 DOM/帧和错误原文不进日志/崩溃报告。 |

### 测试与证据要求

实现卡必须补“产品入口选到了什么”与“真实副作用是什么”的断言，不能只复跑驱动构造测试。通用矩阵含 debug/release、feature 开/关、无驱动/auto/native/fake、审批环境变量、无权限/撤销、Stop 初态/重启、跨会话及不确定结果。进程冷启动与慢 IO 用可注入时钟/预热验证，保留首次失败根因；不靠重跑、加超时或假能力刷绿。

- 最低验证命令按改动选 crate 串行运行：`cargo fmt --all -- --check`；`rcargo test -p gcu-cli --test cli`；`rcargo test -p gcu-cli --test engine_host`；`rcargo test -p gcu-core --test stop`；`rcargo test -p gcu-core --test dom_router`；`rcargo test -p gcu-cli --test native_host`；对应平台 `gcu-driver-{windows,linux,macos}` 的 `--test contract` / `--test stop`（Wayland 加 portal/identity/wayland_contract），以及 `rcargo clippy -p <本卡 crate> --all-targets -- -D warnings`、`rcargo --sh 'cargo deny check'`、`rcargo run -q -p xtask -- check-deps` / `golden --check` / `budget`。release/fault 特性矩阵和平台命令由对应卡补齐，不把 build8 当交互桌面。
- [07 §7](../../../docs/plans/computer-use-v2/07-quality-gates.md#7-安全审查清单p6-05p9-04-必须逐项签字) 中实际身份/提升/UIAccess/TCC/portal、Stop 物理时限、截图/剪贴板/敏感确认、引擎签名与端点防冒充依赖真实装配；fake 的 L1–L3 只能证明逻辑。L4 驱动契约之外还必须跑通过 CLI/helper/engine 的同等安全断言；L5 系统 App、L6 模型任务/red_team、L7 性能/浸泡不能以 fake 填表。
- [07 §8](../../../docs/plans/computer-use-v2/07-quality-gates.md#8-发布门槛) 不变：A-α 要 macOS L1–L4、L6 native ≥70% 及前六安全项等；A-β 要三平台 L1–L5、L6 native+text ≥70%、red_team 零违规；正式版要 L1–L7、成功率 macOS ≥85% / Windows/Linux X11 ≥75%、预算、全部安全项、≥30 App 和签名公证。实验可见也须满足原有 A-α 门槛，feature 编译成功不代表批准发布。
- 证据包含测试名、命令、CI/artifact、系统/桌面/App/浏览器/构建版本和外部 oracle；需要人工项给对应截图/录屏，但不向仓库提交真实桌面截图目录或载荷。所有验证只用一次性桌面/系统 App/夹具，无真实账号、IM、邮件、支付。
- 缺平台/runner/物理输入/签名材料的项一律 `not_run`；不把 skip 计为通过、不外推已测系统版本。本文只核对静态源码与已有记录，没有新增任何真实平台运行证据；本纯文档变更只需文档检查，不编译 Rust。

## 开放问题

以下均为待裁定事项；推荐方案不是已获得的实施或发布授权。可独立验证不受阻的部分，但受影响路线在决定/证据齐备前保持关闭。

| 编号 | NEEDS-DECISION：问题 | 选项与推荐 | 阻塞范围 |
|---|---|---|---|
| Q1 | **（已裁定 2026-10-17：选①默认 auto）** 真实装配后的默认选择与旧安装升级行为 | ①默认 auto（推荐：沿用现有安装的 `gcu mcp`，仅已验收 feature 可选、权限/审批仍逐层生效）；②真实路线始终须显式 native，安装器另行迁移。默认值改变须在 A5 披露，不能偷偷激活既有 fake 授权。 | A0 参数定稿、发行默认启用。 |
| Q2 | 引擎本地热键如何让 App 持久锁存，且不新增 Host 方法 | ①本提案的“本地先停、非正常断线触发 Host 持久 Stop”策略（推荐，需补端到端断线/重启验收）；②保持连接并新增专用 Stop 通知（须单独协议 ADR）。不能拿普通事件暗传恢复或停止控制。 | 路线 B native 准入；独立 CLI 不受此决策阻塞。 |
| Q3 | Chrome/Edge 正式 manifest key 与渠道身份（**已解决 2026-10-17**：维护者批准生成并已将公钥写入 `manifest.json`，扩展 ID `jgpkhdjlddaoopklipcnpcjcpiancmff`，私钥在仓库外；商店若分配不同 ID 则属新身份，另行换 key） | ①维护者提供稳定公钥/确认两商店渠道 ID 与派生规则（推荐，私钥不入库）；②先用明确区分的 Dev 公钥/独立安装，正式 L2 继续关闭。无 key 不生成临时身份来绕过安装门禁，本卡不生成密钥/操作商店。 | A4a/A4b 的正式身份及发布。 |
| Q4 | **（已裁定 2026-10-17：按推荐①）** 多引擎/多会话的共享归属，以及可靠 top-document→OS 窗口证明能否提供 | ①先只允许证据唯一且已授权的本地会话，其余 L0（推荐）；②增加可信选择 UI，再扩多会话（若需 Host/ACP 新字段，另 ADR）。两者均不能以标题/几何相似作为安全证明；A4a 必须先证实支持的平台。 | L2 绑定与多会话支持，未证明平台持续禁用。 |
| Q5 | **（已裁定 2026-10-17：按推荐①）** 真机/签名覆盖与平台开启顺序 | ①按已有设备逐平台补产品入口证据、缺项继续关闭并标 `not_run`（推荐）；②维护者提供额外物理 Windows、各 compositor、macOS 版本/Intel runner 与正式签名材料后统一验收。已有未签名产物政策不能自动豁免正式引擎身份/TCC 要求。 | 平台 feature 发行启用、07 §8 发布资格。 |

本次未能确认的是 0010 正文与本提案的交互、浏览器启动链/文档映射在各平台可提供的可靠证明、现行 Windows 第四探针产品真机结果、完整 helper/engine/native-host 端到端路径，以及 STATUS 所列未测的 TCC/Wayland/物理 Stop/发布指标；这些不是通过静态阅读能补出的证据。

## 退出条件（什么情况下推翻本决定）

若任一平台不能在现有 Driver/Host/ACP 边界内实现可信窗口/输入范围、持续健康 Stop 或持久锁存，保持该 native/L2 路线关闭，提交独立 ADR 与用户裁定；不得以 fallback、放宽权限、加时限或只看 fake 测试交付。若浏览器生产连接必须扩 Host/ACP，或 macOS 责任进程假设在目标版本不成立，同样先停受影响路线并另行决策。

若真实评测/安全审查发现回归，关闭相应发行 feature/平台准入，保留显式离线 fake；L2 回到已声明可用的 L0，未确定完成的变更不重放。回滚/降级必须保留 Stop、撤销 token/share/portal、清会话数据且不迁移旧审批。只有新的可追溯平台证据与批准的 ADR 才能重新开放或改变本决定。

# 0017 · real-eval 的 gcu 授权绑定夹具实例

状态：**Accepted（设计与分卡；2026-10-23 项目所有者“按建议来”，采用推荐：专用一次性 GUI VM，不在 owner 日常 mac-pro 上跑）**；实施按 §5 分卡，S2/S4 完整接线与 S6 外部边界就绪前不启用真实运行（真实模型评测仍停止）。原状态 Proposed · 日期：2026-10-22 · 拟议负责人：Grok（core/native/helper）、Codex（eval/隔离验收）。

## 给 owner 的一页决定

**建议：一次运行一个专用引擎，在 gcu-core 装配一次不可由模型扩大权限的 `FixtureScope`；所有桌面访问经过同一个 `ScopedDriver`。真实客户端（含 preflight）只在一次性桌面内启动。两个条件缺一即 `not_run`。**

- scope 是硬权限上限：只允许 harness 启动并经 OS 核验的进程实例及指定窗口；名称、路径、bundle id、提示词均不能单独授权。
- 保留现有身份拒绝、敏感/前台确认、用户活动与 Stop；自动批准不能扩大 scope，也不能恢复 Stop。
- scope 通过 harness → 专用 gcu/helper 的私有控制连接传入；MCP 只有数据面，没有设定、续期或扩大 scope 的工具。
- 模型不能启动应用；夹具重启、第二夹具和新窗口由可信 harness 登记。旧身份立即失效。
- 推荐已验证原生能力的一次性 GUI VM；VM 不支持所需捕获/输入时继续关闭，可用专用、可擦除真机补证。**不在 owner 日常会话的 mac-pro 上重跑。**
- 预计首个平台连同隔离与验收 **12–18 人日**；Windows/Linux 分别另计。本文只提案，不开启真实运行。

依据：`cu2/eval-lockdown:docs/plans/computer-use-v2/reviews/2026-10-22-real-eval-boundary-review.md` §1、§2、§7.1–2；原报告保留在该分支。
下文代码行号以本分支基线 `7deb1fd2` 为准，`gcu-*/…` 均相对 `computer-use/crates/`；任务路径相对 `computer-use/`。报告分支的行号与本分支不完全相同。

## 1. 执行边界与唯一装配点

当前缺口：session auto-grant 会允许模型选择任何通过原有策略的应用。harness 仅启动夹具并等待状态文件（`gcu-eval/src/prep.rs:97`），真实 MCP 配置没有实例身份（`gcu-eval/src/exec.rs:298`）。

| 路径 | 当前入口与证据 | scope 必须约束的行为 |
| --- | --- | --- |
| 发现应用/窗口 | `gcu-core/src/action/discovery.rs:24`、`:39`；直接调 driver；`gcu-core/src/policy/mod.rs:349` 对 discovery 提前放行 | 只枚举允许实例/窗口，按名称查询也不能查到其他应用 |
| observe/截图 | `gcu-core/src/action/router.rs:122`、`:483`；`gcu-core/src/action/observe.rs:56` 读树，`:89` 捕获 | 在首次读树/取图之前验证；观察当前先 read 再 authorize，单改审批太晚 |
| 输入 | `gcu-core/src/action/router.rs:289` 规划、`:345` 授权、`:350` 重验；`gcu-core/src/browser/router.rs:58` 最终 `driver.perform` | 窗口、坐标、焦点及原生元素 handle 都绑定允许实例；投递时复验 |
| launch | `gcu-core/src/action/discovery.rs:134`、`:223` | scoped 模式一律拒绝，不调用平台 launch/unhide；仅 harness 可建进程 |
| batch | `gcu-core/src/action/router.rs:240` → `gcu-core/src/batch/executor.rs:42` → 单步 router | 每步经过同一个 guard，scope 拒绝终止后续步骤，即使 `stop_on_error=false` |
| script | `gcu-cli/src/script_worker.rs:195` → `gcu-script/src/route.rs:117`、`:226`、`:243` | `cu.app/state`、输入、等待同样受限；scope 拒绝是 hard stop |
| 失败/自动状态 | `gcu-core/src/action/failure.rs:49`、`:69` 会重读或列全部窗口；settle 复用 read | 错误刷新、最终状态、wait/expect、自动截图也不能穿出 scope |

**选择 core 的 driver 访问边界。** 新增 `gcu-core/src/scope/` 的 `ScopedDriver(inner, scope)`，在可信启动时包装一次，router/MCP 只持有包装后的 driver。现有装配点：`gcu-core/src/action/router.rs:51`、`gcu-mcp/src/server.rs:128`；底层接口集中于 `gcu-core/src/driver.rs:455`。一个 eval 引擎只服务一个 run；不让共享 router 临时切换不同 run 的 allow-set。

“一次”指安装权限集合，**每次原生操作仍复验身份**；不为每个 MCP 工具另写一份 allow-list。
guard 统一实现：过滤 discovery；只在允许候选内 resolve；检查所有 read/capture/geometry/focus/subscribe/perform；拒绝 launch；屏蔽底层包含外部应用信息的错误。`list_windows(None)` 改为聚合已允许实例的窗口，不能直接把全桌面列表交给 router。

| 不选择的主执行层 | 原因与保留职责 |
| --- | --- |
| 仅 `PolicyEngine::evaluate` | discovery 提前放行，而且 read 早于 evaluate；保留原有安全策略，与 scope 取交集 |
| 仅 eval Host approval | `gcu-cli/src/host/approval.rs:70`、`:285` 目前还会自动确认敏感/前台操作；不覆盖无审批的发现及审批前捕获；只负责范围内审批 |
| 仅 MCP server | `gcu-mcp/src/server.rs:202` 只验证 catalog，`:214` 起逐工具分派；不能靠过滤工具名覆盖直接 core 调用及嵌套执行；仅负责传递已封闭实例与结果编码 |

`ActionRouter::native`（`gcu-core/src/action/router.rs:450`）只拿不透明 future，没有目标身份，不能简单加一次布尔检查冒充目标约束。
现有 DOM 路径可绕过 `driver.perform`（`gcu-core/src/browser/router.rs:42`），因此首版 scoped eval **不装配 browser Hub、不注册 browser 控制旁路，L2 任务 `not_run`**；将来必须先证明 OS 窗口绑定并接入同一 scope guard。仅隐藏工具名不够。
停止/释放输入是唯一不受目标 scope 拒绝的清理操作；doctor/capabilities 只返回能力与规范化状态，不返回全桌面诊断详情。

## 2. scope、传输与生命周期

拟议内部对象（不是模型参数，也不是持久 grant）：

```text
FixtureScope v1 {
  run_id, generation, engine_instance, desktop_session, control_lease,
  targets: [{ role, pid, os_start_time, boot_identity, executable_proof,
              windows: [{ native_id, incarnation, owner_instance }] }]
}
```

`executable_proof` 复用 OS 校验的映像路径/哈希/签名证据，不能替代 pid + 启动时间。启动时间用 OS 原生精度，加入 boot identity 避免跨重启重放；缺字段或来源不可核验即拒绝。
当前 `AppRef/WindowRef` 只有应用资料与窗口 id（`gcu-core/src/driver.rs:34`），`NativeHandle` 不透明（`gcu-core/src/model.rs:258`）；须新增 driver 内部“进程实例/窗口/元素 owner”核验接口，不能只比较现有 `AppIdentity`。Linux 已有 pid/start-time 证据可复用（`gcu-driver-linux/src/identity.rs:23`），仍需跨平台统一接线。

**选择私有连接，不选 env 或磁盘 scope 文件。** harness/supervisor 从 OS child handle 取得 fixture 身份，由 gcu 独立复核，并从 OS 核验窗口所有者；fixture 可经私有握手报告 ready/窗口候选，但状态 JSON、窗口标题、模型提交的 PID 都不构成凭据。

- supervisor 直接拥有 fixture 和专用 gcu/Host/helper 的生命周期。Unix 用 socketpair/继承 FD，Windows 用精确继承 handle 列表的私有 pipe；控制端绝不继承给模型客户端、relay 或 script worker。
- 当前客户端负责启动 MCP 命令；改为固定的 **数据 relay** 连接 supervisor 预启动的 scoped MCP 实例。模型可知数据端地址，连接也只能取得同一 scope；控制帧与 MCP 帧分离解析，MCP 无权修改 scope。
- 模型不能直接读取 MCP 子进程的 env，但仍可能读配置/状态文件；不能把子进程 env 不可见当作控制来源证明。env、argv、`GCU_FIXTURE_STATE`、模型可达文件都不承载可改权限的 scope、控制 token 或签名密钥。文件权限及同 UID 不是身份认证。
- macOS 不能假设 FD 穿过 `open`/Launch Services。由 guest 内**专用、双向认证并绑定进程实例/代码身份的 helper** 接受 supervisor bootstrap，再移交私有 FD；握手纳入 run/generation/scope 摘要并回 ACK。做不到就 `not_run`，不退回普通 helper。
- 现有代理只传 profile/idle（`gcu-cli/src/runtime.rs:107`、`gcu-cli/src/helper/client.rs:178`），helper 自己加载 Host（`gcu-cli/src/helper/macos.rs:398`）；只在 CLI 侧包装无效，必须在真正持有 native driver 的 helper 内执行（`:314`）。不得复用 owner 的既有 helper、授权或 Stop 存储。
- native eval 入口在收到有效 scope ACK 前不开放数据面；缺失/未知版本、旧代次、租约失效、控制 EOF、supervisor 死亡都撤权、取消在途工作并释放输入，不能回退 unscoped 模式。部署的客户端 sandbox 还须禁止直接启动/附着其他有桌面权限的引擎。

只有控制端可 install/rebind/revoke/续租；消息绑定 run、单调 generation/序号，租约按引擎单调钟到期。所有集合变更递增 generation，未完成审批及旧代次结果同步失效；不会按父 PID 自动纳入整棵子进程树。

**窗口与 TOCTOU：** 只接受已登记实例的精确窗口；多窗口按 role 显式登记。新窗口必须经 supervisor 事件、OS owner 复核和 scope 更新后才可见；系统代管的文件选择器/权限提示不因 parent/title 相同而加入。
窗口 incarnation 由可信创建/销毁跟踪或原生保活引用证明；丢事件、窗口 id 复用无法辨别时拒绝，不能用标题/几何一致代替。
原生树不得把其他进程/未登记窗口的子树或 handle 当成本窗口内容；无法证明来源则拒绝该次观察。所有 handle 包装为 `(run, generation, instance, window incarnation, inner_handle)`，语义动作不能绕开窗口检查。
每次读/图/输入前复核，结果交付前再核代次；driver 在真正 OS 投递处复验 owner/前台窗口/命中目标，坐标或拖拽越出允许窗口即拒绝。macOS 现有前台检查只证 PID（`gcu-driver-macos/src/input/foreground.rs:42`），不足以证明同进程里的正确窗口。无法保证的全局输入路线保持不可用。

**macOS 安全声明（所有者 2026-10-30 决定，S2 的第 2 轮独立评审之后）：** macOS 没有任何 OS 原语能让“检查”和“投递/写入”原子地绑定到同一个窗口（`CGEventPostToPid` 只按进程路由，窗口位置字段只是事件元数据；AX 代理不持有远端 NSWindow，元素可在检查后被 reparent；窗口号可复用；`AXRaise` 按 pid/窗口号重新解析）。因此 macOS scoped 的保证分两层：①**硬保证 = 进程实例级绑定**：`pid + 进程启动时间 + kern.bootsessionuuid` 可由 OS 重新推导；AX 元素引用天然绑定原进程，不会因 pid 复用跑到别的进程；pid 定向事件在出口前一刻复验进程实例；scoped 与否由随作业传入的不可变上下文决定（超时/drop/清 pin 不改变已开始作业的约束）；全局 HID 投递、整进程激活、应用元素级 AX 写在 scoped 模式禁用；缺证据即拒绝。②**窗口级绑定 = 尽力而为的失败即关闭检查**，不声称防住“同一受信夹具进程内部、检查→使用之间被换窗口”的极小间隙；理由：夹具进程是我们自己的受信代码，真正的安全边界是一次性 Tart 访客机（只有夹具与评测工具，没有用户的其它应用）。**已知残余（OS 限制 + VM 缓解，不再追，复发或平台提供原语时重开）：** A1 pid 定向事件不能把接收者绑定到准入窗口（同进程其它窗口/sheet 可能收键）；A2 两次相同读取不是原子观察；A3 AX 写/文本追加在检查与写之间元素可能被 reparent；A4 `AXRaise` 按 pid/窗口号重新解析。Linux/Windows 的同类间隙需各自平台适配时单独评估。

**Windows 安全声明（所有者 2026-10-31 决定，S9 第 2 轮独立评审之后）：** Windows 没有 OS 原语能让“检查”和“投递/读取”原子地绑定到同一个窗口（SendInput 打到当前前台，不绑定 HWND；剪贴板粘贴走前台按键；ApplicationFrameHost 等代管窗口的接收者不能从 HWND 证明到已准入进程；UIA 元素可在 BuildUpdatedCache 之后被 reparent；HWND 可复用）。硬保证 = 进程实例：PID + 进程创建时间（GetProcessTimes）+ boot-session GUID（NtQuerySystemInformation class 90），在每个出口复验（每次原生投递之前，以及每次读/捕获/剪贴板载荷返回之前）。窗口级 = 尽力而为、失败即关闭。scoped 模式关闭无法绑定到已准入进程的路线：前台 SendInput 指针与键盘（含接近点与拖拽采样）、经前台按键的剪贴板粘贴、以及接收者无法证明的路线（含 ApplicationFrameHost）。这些路线在 scoped capabilities/doctor 中标为不可用，依赖它们的任务记为 not_run，而不是动作失败。能证明接收者进程实例的定向窗口消息和 UIA pattern 保留。已知残余（OS 限制 + 一次性 VM 缓解）：同进程内窗口在最后一次检查之后被换掉或 HWND 复用；UIA 元素在进程复验之后、缓存返回之前于同一进程内 reparent；最后一次系统调用到投递之间的间隙。ADR 0013 的 Stop 常量、指针门、settle、fail-closed 与一次有界重试不改。不启用 real eval。

**重启：** fixture 退出即撤销其 scope，不自动按同名/路径/PID 认领替代品。默认本次 attempt 结束；需重启的任务由 supervisor 暂停调度、杀净旧树、重新 spawn/核验，再原子安装 `generation+1` 并等待 ACK。清除旧授权绑定、队列、handles、快照、MCP 图片缓存/文件引用；所有 session 一并失效，未知投递不重放。Stop 保持锁存，重启不是 resume。

**任务目录核查：** 对 `eval/tasks` 的 YAML prompt/setup 做 grep，结论如下；类别名或 compatibility 软件名称不授予真实应用访问权。

| 现有任务（目录 `eval/tasks/cross_app/`，各 YAML prompt `:4`、setup `:6`） | 所需目标 |
| --- | --- |
| `cross-copy-name-01`、`cross-paste-note-01`、`cross-clipboard-01` | 明确写 A→B/两个夹具：完整跨应用实验应启动 A/B 两个独立 fixture 实例并逐个登记 |
| `cross-once-01` | 明确目标 fixture B：按双夹具 transfer 场景登记；当前文本没有证明必须两个 OS 进程 |
| `cross-windows-01` | 左右两个窗口；可以属于同一允许实例，不要求第二应用 |
| `cross-field-map-01`、`cross-table-01` | 源/目标字段或表；可以在一个夹具内，不从 `cross_app` 类别推导第二进程 |
| `cross-isolated-01` | 单夹具，外部写入必须拒绝 |

这些 YAML 目前都只有 `setup: { launch: fixture, … }`；启动器只管理一个 child，当前 egui 主程序也只有一个根窗口（`apps/fixture/src/main.rs:6`）。多实例/多窗口未实现前相关 real 任务应 `not_run`，不能以单窗口 oracle 冒充跨应用证明。
`eval/tasks/compatibility/*` 都是 fixture 等价物；`app/app-switch-01.yaml:4` 是发布开关，不是切换应用；dialog/file/native-finder 也不授权真实 Finder 或 OS 提示。
`eval/tasks/browser/README.md:30` 的未来 collector 可登记私有浏览器 profile 的精确进程/窗口作为 fixture；该 PID/子进程集合必须由 supervisor 独立确认，不允许按“所有浏览器”扩权。它不隐含还需第二 app；无法精确映射或依赖 L2 时 `not_run`。

**拒绝的模型可见结果：** 复用 wire `app_denied`，内部新增 scope 拒绝映射，固定短消息 `Target unavailable in this evaluation scope.`；无 state/image/windows/候选项/底层错误详情。不存在、外部、同名冒充均给同一结果，不确认某 app 是否存在。Stop 已锁存时仍优先 `stopped_by_user`。
list 成功仅返回 scope 内集合，空集合返回空列表；非法目标查询不补全或提示其他名称。拒绝输入为 `outcome=refused, delivery=none`；已开始投递才发现身份变化则 `unknown` 并停止，不声称无副作用。batch/script 保持 hard stop（`gcu-core/src/batch/executor.rs:104`、`gcu-script/src/route.rs:69`）。

## 3. 审批与 Stop 不变量

准入为 **scope ∩ 原安全策略 ∩ app grant ∩ 敏感/前台确认 ∩ 活动/Stop 守卫**；scope 命中不是批准，scope 拒绝不能弹审批卡让模型或用户临时放宽。
真实 eval 的 `auto-allow-session` 只替代范围内 app grant；须拆掉它在 `gcu-cli/src/host/approval.rs:75` 对敏感/前台确认的隐式肯定。两类确认默认拒绝；需要自动肯定的合成任务须在可信 task manifest 中预声明，仍绑定 run/generation、窗口、参数、快照和策略版本并在投递前重验。硬拒绝仍拒绝，系统安全提示 handoff，foreground 禁止时不升级路线；不写 Always grant。

遵守 [ADR 0013](0013-windows-stop-guard-liveness-lease.md) 及已接受增补：

- `STOP_LATCH_BUDGET=100ms`、`RELEASE_INPUT_BUDGET=500ms`、native 上限 `5s`（`gcu-core/src/session/stop.rs:13`）；`DRIVER_DEADLINE=4750ms`（`gcu-core/src/driver.rs:28`）不变。
- Windows `HEARTBEAT_TIMEOUT=100ms`、轮询 `20ms`（`gcu-driver-windows/src/input/watchdog.rs:12`）；指针闸 `40ms`（`input/hook_probe.rs:17`）、光标 settle `15ms`（`input/ffi/native.rs:339`）不变。动作前新鲜双钩子证明、动作中续租、失败关闭及已有重试额度均不改。
- scope 锁/IPC/审批不能阻塞独立 Stop 和 `release_all_input`；控制断连也不能阻止释放。scope 重绑、工具调用、客户端重连均不能清 Stop；只有可信用户恢复事件可清除。即使 VM 中无人操作，也不豁免守卫。

eval Stop 同时持久化在边界外的可信控制方；销毁 VM、恢复快照或创建下一个 attempt 不能绕过已锁存 Stop。恢复仍须真实用户事件，禁止 harness 自动 resume。

## 4. real-run 最低环境契约

此 scope 只封闭 gcu，不封闭客户端其他工具、文件访问或启动扩展。review §7 其余整改仍是独立门槛；模型自报 tools、清空 HOME、PATH shim、没有 side-log 记录都不能替代隔离。
在任何带凭据的客户端启动（包括 preflight）之前，可信 provider 必须建立以下边界；每个 attempt 重置，结束后销毁：

| 必需保证 | harness/provider 可机检的证据 | 必须外部配置或 owner/provider 证明的部分 |
| --- | --- | --- |
| 一次性 OS 用户/桌面；无个人应用、会话、凭据 | run/VM/boot/UID/桌面 id、镜像摘要、活动会话/进程、挂载/共享开关、全新私有目录、独立 helper/Stop 存储 | 基础镜像来源与无残留；个人凭据/登录状态无法靠进程名单穷尽，需受控镜像与销毁证明 |
| 文件及 IPC 隔离 | sandbox 生效状态；否定测试读取 host sentinel/连接 host agent、显示服务或 helper；模型无 debug/ptrace、控制 FD、supervisor 文件写权 | 无 owner home/其他用户可读目录、真实会话 socket、钥匙串/浏览器 profile；关闭 host 共享盘、剪贴板、拖放、设备/agent 转发及来宾集成通道；guest 剪贴板预置合成内容 |
| egress 仅模型 endpoint | 外部防火墙/受控网关策略摘要及 deny 计数；无模型的受控探针验证 LAN、metadata、任意公网/DNS/IPv6/QUIC 均拒绝 | provider 管理出站默认拒绝，客户端只能走固定模型 endpoint 网关；gcu/helper/fixture 无外网出口；不能让 guest 自己修改策略 |
| 无 ambient credentials | env 键名 allow-list、私有 auth 存储权限/到期元数据、无 agent socket；不读/打印 token | 仅一次性 eval 凭据；刷新若需要额外 endpoint，必须预先单列审核，默认停止，不开放通用代理或自动放行域名 |
| native 控制和可靠退出 | 签名/实例与 scope ACK、真实权限/Stop 状态、进程组/Job/VM 句柄；超时/取消/父死测试；清理回执 | GUI/TCC/portal/硬件热键的真实可用性；VM 回收由边界外 supervisor 保证 |

只允许预声明的 guest 内 fixture 本地通道和 MCP/控制 IPC；不把任意 loopback 当成 egress 例外。模型服务地址须精确受控，DNS 解析在网关侧；禁重定向到任意主机，代理凭据不交模型/MCP。首版避免联网登录，用短期 eval 凭据；endpoint 未确定即 `not_run`。
进程/挂载快照只能发现部分违约，不能证明“没有秘密”。provider 的签名证据需绑定本次实例/策略，owner 签认需绑定镜像和有效期；自报 `--disposable` 不算证明。机检失败或必需证明缺失即拒绝，不以手工勾选覆盖失败。报告分别记录 `checked/attested/unknown`，关键 unknown 不产生 pass。

**S6 第 3 轮修复的离线准入决定：** fake client 与 backend 都固定为编译内的进程内实现；删除公开的 `builtin_fake_bin` 路径槽、自重入启动和所有 artifact 目录/相邻文件信任。runner、client 解析与 Command 构造、standalone/hidden fake-agent 入口共用 `boundary::admit_launch`。任何显式 client/backend 路径（包括真实构建产物、复制后的相邻文件、空路径）或需要外部引擎的 `fake_scope` 组合都不能凭文件名、目录或声明准入。CLI 只解析 driver selector，不再用路径是否存在或旧宣誓独立准入；常规 suite 与 A/B 都在分配、引擎启动、凭据复制及 preflight 前生成 `provider_unavailable` / `not_run`，并停止后续 task/arm/repeat。独立 S3 supervisor 的 fake bootstrap 测试仍保留，但不豁免 runner 的 provider 门禁；真实 provider 仍未安装。

real 模式保留 `--env local` / `--env desktop` 两种选择，但都只返回 `not_run`；必须先实现受控 provider，禁止静默回退 local。

| owner 选项 | 成本/好处 | 判定 |
| --- | --- | --- |
| mac-pro（Intel macOS），owner 原账号/日常桌面 | 无 VM 成本，可能干扰工作并暴露现有应用、凭据；是否共享其他工作尚未确认 | 不符合；无人坐在电脑前不改变边界 |
| mac-pro 新建一次性账号 | 成本低，但仍共享 OS/磁盘/服务，快速切换可能保留其他登录会话；Intel/TCC 证据仍缺 | 新账号本身不够。只有补足整套文件/IPC/桌面/出站限制、owner 证明无个人暴露及原生验收后才能另评 |
| 专用 GUI VM，干净快照、禁 host 集成 | 一次部署后可重复重置；会增加资源成本，可能缺 SCK/AX/GPU/portal/物理热键能力 | **推荐默认**；先做无模型原生验收，能力不足保持 `not_run` |
| 专用可擦除真机/独立 OS 安装 | 真实输入/TCC 证据更直接，成本是设备独占、重置时间和外部网络隔离 | VM 不足时的推荐备选；一次性用户且无个人数据/其他登录会话，仍满足全契约 |

mac-pro 的共享程度须 owner 说明；不能从机器别名推断。`docs/llm-wiki/computer-use.md` 的 A0-M 边界仍把 Intel/macOS 14/15/TCC 产品链列为未验收；VM 或新账号不自动解除此限制。本 ADR 不批准这些入口。

## 5. 测试与分卡

**离线 fake driver（不运行模型）：** 构造允许 A、额外窗口 A2、未登记窗口 A3、允许 B、外部 C、同名/同映像不同实例 D；底层 spy 分别记录元数据探测、内容读取、捕获、投递、launch。

| 测试组 | 必须证明 |
| --- | --- |
| 全工具参数化 | 对 catalog minimal/default/full 的每个工具及所有输入方式（语义/坐标/剪贴板/前台）尝试 C/D、伪造 window/handle、跨 run 图片；除受控 owner 元数据核验外，外部内容读取/捕获/投递/launch 次数为 0 |
| 信息不泄漏 | `list_apps/list_windows` 只见 scope；get_state 的有图/无图/auto、wait/expect、settle/final-state、WindowGone/unknown 刷新、capabilities/doctor、错误/文件资源中均无 C 的 canary 名称/标题/路径/像素；不存在与外部查询结果一致 |
| 嵌套与旁路 | batch/script 中途拒绝即 hard stop，后续无效；直接 core 与 MCP/CLI relay/helper 接线同样约束；browser/DOM 旁路在 scoped 模式不可达；launch 对 A/C 都不进平台 |
| 身份/生命周期竞态 | 同 PID 新 start-time、窗口 id 复用、owner 改变、同进程换窗口、句柄跨窗口、授权等待中撤销、scope 缺失/重放/EOF；旧代次缓存不可再读，重启只接受 supervisor 新绑定；不自动重放 unknown |
| 正向与 Stop | A/A2 与明确登记 B 可操作、A3 拒绝；原敏感/前台/活动/身份拒绝仍生效；Stop 在阻塞 IPC/锁/动作中及时释放，重连/重启不清锁存；控制协议畸形输入不能变 MCP 调用 |
| 环境门禁 | fake provider/假客户端验证 preflight 之前门禁、不可信 attest/无 ACK/清理失败即停；父死清理完整树。外部 sandbox/网络的否定测试用本地受控探针，不用模型陈述作证 |

**只有隔离真机/真实 GUI VM 能证明：** OS start-time/window/AX owner 的真实性与复用、屏幕捕获不含其他窗口/遮挡内容、前台抢焦点与拖拽实际落点、helper 责任进程及 TCC、剪贴板恢复且无 host 同步、真实 Stop 热键/钩子摘除/释放时限、跨进程死亡清理和外部防火墙拒绝。先用确定性离线调用验证；其后真实 LLM eval 是额外阶段。fake 通过不等于 Intel、各 compositor 或虚拟桌面可用。

拟议小 PR（依赖顺序；每项包含对应测试/文档，估算为人日，负责人待 owner 分派确认）：

| 卡 | 文件与验收边界 | 拟议 owner | 估算 |
| --- | --- | --- | --- |
| S1 scope 类型与封闭包装 | 新 `gcu-core/src/scope/`、`driver.rs`、`action/router.rs`、`tests/fixture_scope.rs`；完整 Driver 覆盖、拒绝映射；新增方法必须显式实现，未知能力拒绝。已实现（PR #168，不启用 real eval）：`ScopeLease` 代次与 `watch` 使在途 read/capture/geometry/snapshot/list 在交付前复核当前租约，不一致则丢弃 payload 并返回固定 ScopeDenied；在途 perform 在撤销先于交付时取消并 `release_all_input`，已开始则按 unknown 上报，Stop 与撤权竞争为 `stopped_by_user`；subscribe 在 Pending 时遇到 revoke 或 rebind 会醒来并结束，且丢弃内层订阅，不需要新事件；Stop 只在带 waker 的 Activity 上醒来。RemoteHost 的默认 activity 与 `StopMirror` 不登记 waker，安静流要到下一次 poll 才拒绝事件，这不是已证明的 Stop 优先被绕过；转发前复核已准入的 owner 与 incarnation，generation 不变但 owner 或 incarnation 变了也不转发。router 取消观察把租约失效当作取消源。一次尝试先武装失效接收端再读取 `AttemptAuthority`，并把同一 run/generation 带过授权、recheck、dispatch 与观察完成；dispatch/observe 打开 scope 时必须与该 epoch 一致，PressKeys 等无 sealed handle 的动作同样生效。样本标记在读取前确定并随样本保留，缺失或过期标记拒绝为固定 ScopeDenied，不用当前 generation 补写；wait 持有的旧样本在超时或错误发布前再校验。`publish_with_limit`（包括 wait 发布）先在私有区完成缩略图、索引、渲染、哈希和返回状态组装，再取快照互斥量校验基线与预算、预留登记容量；随后在 `commit_if_current` 的租约读保护内插入快照。淘汰的大树与像素移出后在租约锁外释放。快照 id 可以留下空洞。wait 在指纹、签名、Pipeline 和快照锁之后再验；ScopeDenied 或 StoppedByUser 不再折进带 state 的 Timeout。WindowGone 在 `failure` 入口、任何 await 之前复制发起时的 epoch：尝试内用 attempt epoch，`with_attempt` 之外的几何或读取失败以及 wait 的错误路径用当时的 lease。失败刷新之后不再用当前 epoch 重盖。该 epoch 过期或已 Stop 时只返回 WindowGone，不附窗口列表、应用文本或可用窗口。选择截图时校验实际选中截图所属 epoch，而不是只看 latest。旧 epoch 的 session 授权在同 run 更新记录后淘汰。`SnapshotStore::set_scope_epoch` 只按 run id 长度差记账。scoped `key_window` 不转发会改窗口的 app 级调用，只经只读 `list_windows` 选出已登记窗口并完成该窗口准入。审批、观察、会话快照与授权寿命绑定 run/scope generation，`rebind` 后即使目标与内容不变也不投递（含无 sealed handle 的 PressKeys）。batch 报告带上尝试的 producing epoch；scoped 下没有 producing epoch 的行会清空，unknown 仍保持 unknown。MCP 编码把这份权威带过图片 worker、溢出文件和 `finish` 里的额外文件。文件先写到未登记的暂存名，再在租约锁外改为最终名，整个批次一直由 Drop 守卫私有持有，覆盖超时、取消、future drop 与 panic。MCP `finish` 在所有溢出和编码完成后，按快照 → image cache → 文件账本 → 租约的顺序取得锁；锁等待、容量检查和分配均在租约临界区之前。`commit_if_current` 在同一临界区无失败地登记文件、缓存和 `screenshot_delivered` 尺寸；输出锁持续到整批登记完成，`export_private_files` 与 `cached_image` 看不到未提交或半登记的产物。拒绝时丢弃私有守卫，不再先登记后删除。`list_apps`、`list_windows`、capabilities 与 script 在读取或运行前复制 epoch，await 之后与交付前重验，包括长输出溢出文件；scoped 下这些路径不用 `LiveStamp::off()`。scoped 下不存在无戳即安全：没有 producing epoch 的 `ActionState` 一律清除，包括文本恰好是固定拒绝句但仍带 app、window 或 screenshot 事实的状态。硬拒绝（AppDenied 加固定句且无 state）仍先阻断且不含事实。WindowGone 文本、`windows`、元素标签和批量失败/最后动作的 target 都携带 producing epoch，并与有效载荷一起校验或清除。unknown 与错误身份保持原样。无 scope 的历史行为，以及 epoch 仍匹配时的超时，仍保留 state。scope 硬拒绝同样清空。清空本次报告或编码不是 S4 那种随 lease 失效回收已经交付的会话图片缓存。perform 内首次 scope 拒绝的线协议结果为 `refused` 且 `delivery=none`。能力 `version` 与 doctor 权限 token 在 scoped 擦洗中按白名单规范化。验收覆盖：DOM fake share 对照（无 scope 走 DOM 通道，scoped 下 DOM perform 为 0）、script 内层可解析并读取 C 且允许目标正向 `cu.app` 不 restore 未登记窗口、MCP 各 profile 真实 `tools/list` 经服务端 dispatch 逐工具执行、像素/文件 canary 与允许目标的真实 `element_binding`。未覆盖 / not_run：MCP 安静流的 Stop 唤醒（RemoteHost 默认 activity 与 `StopMirror` 无 waker；下次 poll 仍拒绝，不是已证明的 Stop 优先绕过）；`rebind`/`revoke` 先放 state 锁再 `publish`，并发控制写入可能让 state 停在更新的 generation 而 watch 仍是旧值，新的 `arm_attempt` 会拒绝，只影响活性、不披露 payload；`AttemptAuthority::confirm` 可能把另一个 generation 标成 seen 再返回 false，调用方丢弃该尝试，不能把“被拒 generation 保持 unseen”说得比代码更强；最终确认之后的原生交付点许可（S2/S4）；授权修剪、快照改标，以及已经交付的图片与文件缓存随 lease 失效（S4）；socket/ACK/EOF 与畸形控制帧/relay（S3/S4）；真实 OS pid/start-time/window/AX owner 与捕获遮挡（S2/S4/S9/S10）。本轮没有新的“会话锁跨 await”或截图校验死锁。进程内 rebind/revoke 不代替私有传输。scoped 模式不装配 DOM hub，真实扩展旁路 not_run。gcu-eval 与平台驱动内部未改。Windows worker 只补了 `ScopeDenied` 的 serde 镜像。覆盖测试：`gcu-core/tests/fixture_scope.rs`、`fixture_scope_round2.rs`、`fixture_scope_round3.rs`、`fixture_scope_round4.rs`、`fixture_scope_round5.rs`、`fixture_scope_dom.rs`、`gcu-core/tests/policy/grants.rs`、`gcu-core/tests/snapshot.rs`、`gcu-script/tests/fixture_scope.rs`、`gcu-mcp/tests/fixture_scope.rs`、`gcu-mcp/src/result_tests.rs`、`gcu-mcp/src/result_scope_tests.rs`、`gcu-mcp/src/result_round5_tests.rs`。第六轮改为单一原子提交点，不再逐点补“先校验、后使用”。索引、渲染、缩略图、图片编码、文件写入（暂存名，不登记、不进导出账本、不进 image cache、不进快照存储）和 WindowGone 元数据组装都在提交点之外完成，产物保持私有。`ScopeLease::commit_if_current` 在租约读保护内确认 authority 仍是当前，并在同一临界区完成登记或交换；`revoke`/`rebind` 取写保护，因此撤权与提交互斥、不可交错。临界区内只有内存登记（文件已在临界区外改到最终名，登记才视为可导出），没有 await、输出锁等待、重活或新的磁盘写入。提交失败由守卫 Drop 清掉全部私有暂存（取消、超时、drop、panic），调用方只得到固定拒绝。`export_private_files`、`cached_image` 和快照存储只在提交成功之后看得到。core `publish_with_limit`（含 wait 发布）、WindowGone、MCP `finish`（文件、缓存、`screenshot_delivered`）以及 discovery/script 溢出都走这一个点。保证：未提交产物不可见；撤权不能插进临界区。不保证：不回溯已经合法提交的产物；提交点之前的在途工作可能浪费，但不可见。无 scope 时 script 文本仍只取 2000 字节前缀。本轮不改 Stop 常量或 fail-closed，也不覆盖 (b) 类残余（MCP Stop waker、rebind/revoke 发布活性、原生交付点、已交付缓存回收）。覆盖测试另加 `gcu-core/tests/fixture_scope_round6.rs`、`gcu-mcp/src/result_round6_tests.rs`。 第七次评审（R6-1）：被拒绝的发布不再返回或记账由 scope 派生的目标标签（元素标签等），在最终化处一个 helper 同时清除返回 action 与动作账本条目，投递、错误与 unknown 状态保持，无 scope 时为空操作；WindowGone 的 Stop 读取移到租约临界区之前（Stop 恰在提交点期间发生时，父级的抑制可能错过一次，但租约仍原子拒绝旧 epoch，不产生 scope 泄漏）。新增 (b) 类残余（无已证明的泄漏路径）：`PublishGate` 与租约门禁访问器在 `gcu-core` 里是公开 API，不是仅测试可用，生产代码没有武装它的调用方，但被武装的 during-gate 会在持有读锁时拖住 `revoke`（15 秒后 panic）；标准库 `RwLock` 无写者饥饿上界；提交点内的 `activity` 调用已移出租约；测试没有整体超时。七次 Codex 评审（`reviews/2026-10-23-scope-s1-review*.md`）的最终结论为 MERGE（无 (a) 类）。实现由 Grok（前几轮）与 Codex（收尾）完成。 | Codex | 2–3 |
| S2 首个平台身份适配 | 已实现：macOS 驱动的 InstanceEvidence 与投递前复验。OsFacts 在使用时重读 proc_pidinfo 的 p_starttime（微秒，libc proc_bsdinfo 偏移 120/128，结构 136 字节）、kern.bootsessionuuid（读 kern.boottime 后丢弃，不作为回退）和 proc_pidpath（仅 ExecutableProof，不代替 pid+启动时间）。窗口表以 CGWindowID 十进制字符串为 native id；incarnation 只在连续性 token（保留的 AX 窗口元素 CFEqual 加 _AXUIElementGetWindow）证明同一对象时保持，owner 每次从 CGWindowList 重读。token 缺失、观察缺口、私有符号不可用（macos-private-spi / GCU_DISABLE_SKYLIGHT / dlsym，能力 ax_window）都得到 None，不用标题或几何代替。元素用 AXUIElementGetPid 加所属窗口；伪造 handle、跨进程、窗口未证明为 None。不开 scope 时，无 pin 的 CG 投递走该操作自己携带的 unscoped gate 照常发送；门不是共享槽，一个动作的 drop 不清另一个动作的门；缺失 gate 仍拒绝。语义准入只把 session 是否 scoped 与 pin 读一次，得到不可变 JobScope；scoped 会话走到无绑定分支一律拒绝，不会因别的 session 的 ClearPin 落到 unscoped，非 enforcing 的 live adapter 同样如此。scoped 的 AX 与 CG 作业在准入时携带不可变 expected；ClearPin、超时或 drop 不改变已经开始的 scoped 作业。缺少 expected 则拒绝。scoped 观察不写 AXEnhancedUserInterface 与 AXManualAccessibility，树不完整时沿用既有 incomplete。有证明的投递仍在最后一刻复验：前台要求最前应用窗口的 id 与 owner；坐标点必须命中最上层可见窗口，越界、遮挡或别的窗口拒绝且不投递。未投递为 maybe_executed false；已部分投递为 unknown，不重放。未覆盖 / not_run：Tart 访客机 GCU_DESKTOP=1 的 live_scope_window_proof（mac-mini 不跑）。S4、S6、真实评测不在本卡。不启用 real eval。 | Grok | 2–3 |
| S3 私有 bootstrap 与 relay | `gcu-eval/src/{prep,exec,runner}.rs`、新 `scope/`；`gcu-cli/src/{args,runtime,scope/*}.rs`；`gcu-host-protocol/src/scope/` 最小控制类型与继承连接，复用 Host framing/import。实现分支 `cu2/scope-s3`：仅 `--driver fake` 的 `scoped-engine`，固定继承 duplex stdin（导入后移除 stdio 入口；Windows 借用 stdio HANDLE_LIST spawn）驱动 Unbound → Active(run,generation) → Revoked；ACK 后才建 loopback MCP 数据 socket，`scope-relay` 只复制 MCP；rebind/续租序号/EOF/畸形帧/单调钟到期都经同一状态机，撤销后释放输入并结束数据连接。eval 的可信库入口 `RunConfig::fake_scope`，原 real/client attestation 边界不变；supervisor 对照 ACK 后写 relay 配置，撤销并等待退出，清理屏障失败保留 run 树。新增验收（未运行）：`gcu-cli/tests/scope_bootstrap.rs`、`gcu-cli/src/scope/{state_tests,runtime_tests}.rs`、`gcu-host-protocol/tests/scope.rs`、`gcu-eval/tests/scope_supervisor.rs`；全部异步/子进程等待有界。编译/测试/三平台检查待 owner；Windows pipe runtime、真实身份 S2、macOS 专用 helper S4、一次性环境 S6、真实模型均 not_run，不启用 real。S1 原生交付点与已交付缓存等残余仍由后续卡处理。 已实现（PR #172，fake driver / fake evidence，不启用 real eval）：控制通道是 scope 权威的唯一来源，引擎状态机 `Unbound → Active(run, generation) → Revoked(终态)`，数据面仅 Active 时服务；控制帧版本化并绑定 run id 与单调 generation/序号，未知版本、缺字段、旧代次/重放、digest 不符一律撤权；控制端只经继承的私有 socket/handle，不经 env/argv/文件/MCP，不被 client/relay/script worker 继承；relay 只转发 MCP 帧；截止时间在 `ScopeLease` 内（可注入单调钟，Renew 延长同一截止），是使用权威的条件；每个数据面工作单元（socket、rmcp 派生处理器、嵌套脚本操作及回复）绑定连接权威，每个入站帧在 rmcp 之前经 `scoped_stdio` 查可用权威；发布权威检查失败只出固定 ScopeDenied（线协议 app_denied），脚本错误永不可信，仅父侧原生 unknown 证据保留，Stop 优先统一经 `Session::stop_priority`；脚本 worker 子进程由取消安全的 owner guard 持有，引擎退出前有界 kill/reap；HTTP 网关在启动路径拒绝 scoped 连接。未覆盖 / not_run：Windows 私有管道运行时证明（Windows 下 scoped 引擎 fail-closed：退出码 2、固定报错、无输出）；macOS 专用 helper 的 FD 移交（S4）；真实 OS 身份（S2）；外部沙箱/网络与一次性环境（S6）；S1 的 (b) 类残余。三轮独立 Codex 评审加一次专项复核，终评 MERGE（无 (a) 类）；实现由 Codex，全部改动由总控在 Linux/Windows/macOS 构建机验证。 | Codex | 2–3 |
| S4 专用 helper 与连接装配 | 已实现（离线 fake / 注入 OsFacts，不启用 real eval）：专用 scoped helper 是另一入口 `scoped-agent` 和另一份身份（独立 bundle id、socket 目录 `grok-computer-use-scoped`、独立 Stop/auth 根），不与普通 helper 共用端点。普通首帧解析到 scoped 握手则拒绝且不开 MCP；scoped helper 收到带 `kind` 的普通握手则拒绝。握手双向核对 pid + 启动时间 + boot session（`ProcessFacts` / `ScriptedFacts`）与代码身份；ACK 含 run id、generation、scope 摘要和 helper 实例。缺字段、未知版本、旧代次、重放、摘要不符、对端实例变化一律撤权。数据面在 ACK 写完且对端仍匹配之后才打开；ACK 前的请求是固定 ScopeDenied，无副作用。控制 EOF、supervisor 进程消失、租约失效、旧代次都撤权并 `release_all_input`，不回退 unscoped。已交付图片与文件在 `commit_if_current` 的同一闭包里盖上代次，之后的 get/export 按同一租约锁序隐藏并回收，不另设提交点。scoped 路径对普通 helper 存储的读写次数为 0。没有真实继承 FD 时 `scoped-agent` fail-closed。未覆盖 / not_run：Tart 访客内真实 helper 启动、TCC 授权、真实 FD 移交与 ACK（需所有者在访客 GUI 点击）、真实模型评测。无新依赖。Stop 常量未改。 | Grok | 2–3 |
| S5 审批与跨工具回归 | 已实现（仅离线 fake，不启 real eval；两轮独立 Codex 评审，终评 MERGE）：`gcu-cli/src/host/approval.rs` 拆分 app/sensitive/foreground 决策，allow 模式只给 Turn/Session app grant，两种确认默认拒绝；共享确认回调不按 summary 猜种类，不增加 manifest 自动确认入口，不写 Always。`eval-autoapprove ∩ debug_assertions` 与 release 拒绝入口不变。扩展 S1 core/MCP 参数化：A/A2/B 正向，C/D/A3/伪造窗口、语义/坐标及 targeted/foreground/合成 clipboard 路线，固定 ScopeDenied、拒绝前 read/capture/perform/launch 为 0；core 增加同 generation 跨 run 图片与 sealed handle 重放拒绝。CLI 复用 S3 `FakeEvidence` 和 MCP desktop，实际 Host provider 验证敏感/前台、内置 deny/身份 handoff、Stop/rebind/新 session 与不持久化；batch 加第三步 spy，script 加允许步骤后的中途拒绝（catch 后也不继续）。既有覆盖与增量见下表。总控在构建机验证：Linux 工作区 2034 + script 393 + eval-autoapprove 445，Windows 835 + eval-autoapprove 350，macOS 926 + eval-autoapprove 441，仅已知基线 xtask 失败；CI 新增 eval-autoapprove 步骤（此前该特性下的测试从未在 CI 运行）。评审 r1 修复：scoped launch 在 Stop 检查之后、解析与审批之前拒绝。未运行 / not_run：真实剪贴板/前台/OS owner、Windows pipe、helper 接线、环境边界及真实模型仍属 S2/S3/S4/S6 等后续验收，S1/S3 已登记残余不变。无新依赖。 | Codex | 1–2 |
| S6 一次性环境 provider | 已实现（离线部分，不启用 real eval）：`gcu-eval/src/disposable.rs` provider/verifier 契约，§4 五行 `checked/attested/unknown`、逐项机检、绑定实例/镜像/策略/有效期的外部证明与精确 scope ACK；失败/缺项/不可信/关键 unknown/清理回执失败一律 `not_run` 并停后续 task/arm/repeat。runner 在凭据复制、夹具和带凭据的 tool preflight 前拒绝，旧宣誓不算证明，不退 local；JSON/HTML/A/B 留规范化保证与整树清理回执。`eval/README.md` 仅外部 VM/网关文档模板。**单一原则一（启动准入）：** 任何 client/backend/fixture 进程只能凭 `AdmittedLaunch` 构造——该类型字段私有、不可克隆，只能由 `src/boundary.rs` 的 `admit_client`/`admit_scoped_backend`/`admit_fixture` 签发（先走既有 `admit_launch`，再走 provider 门；当前没有接受型 provider，所有可执行路径一律拒绝）；`std::process::Command::new` 与 `tokio::process::Command::new` 只出现在 `boundary.rs` 的 `std_command`/`tokio_command`，由源码扫描测试 `command_new_is_only_in_the_admission_module` 把关；公开的带路径入口改为 `scope::Supervisor::open`，准入先于 prepare/spawn/ACK；S3 的生命周期铸造函数 `mint_scope_lifecycle_test` 只在 `test-support` 特性下编译（`scope_supervisor` 测试 `required-features`，CI 新增 gcu-eval test-support clippy/test 步骤，三平台），生产构建里不存在，扫描测试断言其定义与再导出前有该 cfg。**单一原则二（停止必须被确认）：** 进程内 fake 在独立线程 `gcu-eval-fake` 运行，文件 IO 用不跟随符号链接、非阻塞方式打开并在同一句柄上确认是普通文件（Unix `O_NOFOLLOW|O_NONBLOCK`，Windows reparse-point 标志），无读者的 FIFO 立即失败；线程句柄由 `OwnedWorker` 持有，Drop 先置取消再有界 join；join 预算（500ms）到期仍未停止时不丢弃句柄，该尝试记 `NotRun`（reason `worker_unconfirmed`）、清理视为失败、保留目录树并沿用既有路径停掉后续 task。已知窗口：线程恰在“取消检查与写”之间被暂停的极短窗口不能由检查消除，由“未确认停止则整体不可信并停后续”兜底。**评审：** 五轮独立 Codex 评审（`reviews/2026-10-30-scope-s6-review.md`、`2026-10-31-scope-s6-review-r2..r5.md`）。r1–r3 的 A1/A2/A3 逐点修复后，r4 仍为 MERGE-AFTER-FIXES：同 A1 类第 4 次（公开 `Supervisor::start` 在准入前启动任意后端）与第 3 轮修复引入的新类（进程内 fake 阻塞 I/O 卡住 runner 超时与清理），改为上述两条单一原则；r5 为窄范围核对：A1 类 FIXED（含 gcu-cli/xtask 全仓扫描），余一项同 r4-2 类（工作线程可在返回后写），按原则二修复；评审轮数已超过三轮上限，未再开第 6 轮完整评审，修复由总控读代码并在三平台验证。**验证（总控）：** Linux 全量 2065（仅已知基线失败：xtask `collect_complete_artifacts`，老辛无 node 的 `compositor_scripts_syntax_and_security_fences_pass`）、gcu-eval 默认 204 / eval-autoapprove（含 gcu-cli）477 / test-support 204、cargo deny、check-deps、clippy `-D warnings`；macOS gcu-cli+gcu-eval eval-autoapprove 473、gcu-eval test-support 204；Windows 同两组 379 / 160，clippy 无警告。`tests/fake_agent_boundary.rs` 对刚写好的可执行文件 spawn 的瞬时错误（Windows `ERROR_SHARING_VIOLATION` 32、Linux `ETXTBSY` 26）只对该错误码做最多 5 次有界重试，Linux 循环 10 次 0 失败，Windows 偶发 `fake_agent_boundary.rs:61` 未再复现。**not_run：** 真实 provider/签名验证/VM/沙箱/防火墙、三平台真实整树与父死回收、原生 S2/S4（真机 helper 启动、FD 移交）、真实模型；`scope_supervisor` 在 Windows 为 fail-closed 流程。无新依赖。 | Codex（实现与 r1–r3）+ Grok（r4/r5 修复）+ 总控 | 2–3 |
| S7 首个平台验收证据 | `docs/` 的确定性原生矩阵、环境回执与未支持项；owner 操作真实用户恢复，控制方审阅再安排 LLM | Grok + Codex | 1 |
| S8 多目标任务扩展（后续） | `apps/fixture/src/`、eval task schema/启动器及上述 8 个 cross_app YAML；显式角色/进程/窗口声明，缺映射不跑 | Codex | 1–2 |
| S9 Windows 适配（独立 PR） | `gcu-driver-windows/src/discovery.rs`、`src/worker/`、`src/uia/`、`src/input/`；创建时间/HWND/UIA owner、Worker 端复验；保持 ADR 0013。已实现（本分支，不启用 real eval）：实例是 PID + 句柄创建时间（`OpenProcess`/`GetProcessTimes`，不看路径/名称）+ 启动身份。启动身份是 `NtQuerySystemInformation` class 90（`SystemBootEnvironmentInformation`）的 boot-session GUID，经 `scope_identity::boot_token`；空、失败、超长或全数字会话，以及墙钟量子，不能成为 `BootIdentity`，也不把 1 秒量子提升为启动身份。这些失败只拒绝 scoped 证明（`scope_close`、`carried_claim`、`prove_carried` 和 scoped 捕获/输入/UIA）。普通 unscoped 的几何、捕获、key_window、订阅和输入不读启动身份，行为与 S9 之前相同。证据接口 `live_process`、`live_window` 仍要求可核验的 boot，全数字会话在非 Windows 上也不能成为证据。窗口是 HWND + 创建代次 + owner。scoped 的 UIA、捕获和输入在准入时把这份 claim 按值带进作业，出队时不改读替换实例。UIA 在内容离开 worker 之前复验进程实例（PID + 创建时间 + boot）：可移植 `Walk::prove` 和 `perform_until` 在 `properties`/`validate` 之前调用 `prove_process`；Windows `prove_before_cache`、`prove_live_owner` 在 `BuildUpdatedCache` 之前核对 owner 进程。窗口/元素成员与代次是尽力而为、失败即关闭：`window_matching` 使用发现与 UIA 共用的属性 `GrokComputerUse.WindowGeneration.{namespace}`（in-process 的 namespace 是 `{pid}.{created}`，worker 用同一 namespace），代次对不上则拒绝且不把新根写回 claim。外来子节点在 `properties` 之前省略。生产交付调用 `admit_delivery_element`：树 `Walk::prove` 在 `properties` 之前、`element_binding`、语义 `perform_until`、Windows UIA 在 `BuildUpdatedCache` 之前、定向输入 `perform_scoped` 在每次采样和发送之前。缺 owner 仍由 `admit_fragment` 或 `prove_element` 拒绝，`None` 不是已准入 owner。`element_binding` 要求来源记录，无来源的手造 handle 为 None。捕获在读像素之前，以及 PNG 编码之后、返回字节之前，复验同一份 claim；对不上则不进入源、不返回字节。定向输入把最初准入的进程实例（PID、创建时间、boot）按值带进同一次采样或发送作业。发送在 PostMessage/SendMessage 之前复验子进程，不采纳替换实例的 pid。scoped 采样只有一次 WM_GETTEXT：读取之前复验进程实例，哈希离开作业之前再复验一次；不一致则丢弃哈希，不再读 GUI 线程、控件消息或客户区像素。每一次 settle 采样都是这同一份作业。发送已经发生时，失败的采样仍按 unknown 上报。unscoped 采样保持原来的多读序列，不增加进程复验。该复验在指针门和输入账本之外。scoped 前台 SendInput（点击、拖拽、滚动、按键、文本、经前台按键的剪贴板粘贴，以及激活）在 `WindowsDriver::perform` 入口、任何本机调用之前返回 CapabilityUnavailable；缺 pin 仍是 ScopeDenied。后端在 resolve 之前仍拒绝。不采样、不聚焦、不投递、不开动作证明。接收者无法证明的路线（含 ApplicationFrameHost）同样不可用。`scope_capabilities` 把 foreground 标为 Unavailable，doctor 附带同一说明，依赖这些路线的任务记为 not_run，而不是动作失败。已知窗口级残余（OS 限制，一次性 VM 为后盾）：同进程内窗口在最后一次检查之后被换掉或 HWND 复用；UIA 元素在进程复验之后、缓存返回之前于同一进程内 reparent；最后一次系统调用到投递之间的间隙。scoped 剪贴板暂存已关闭。第 2/3 轮关闭 scoped 前台粘贴之后，定向粘贴、SetValue 回退和长文本路径都到不了暂存：没有定向剪贴板粘贴（非前台 TypeText 走窗口消息）；SetValue 只使用 UIA Value/Range；长文本粘贴只在 foreground 时构造，而 scoped 前台在 `WindowsDriver::perform` 和 `perform_scoped` 入口就被拒绝。`ClipboardStage` 带 admitted、`clipboard_stage_admitted` 和原来的 `stage_proved` 都在打开剪贴板之前返回 CapabilityUnavailable。按格式授权、`publish_formats`、`authorize_publish`、`publication_proof` 和 `arm_publication` 已删除。unscoped 仍一次发布全部文本格式，回滚行为与关闭 scoped 暂存之前相同。恢复仍无条件执行。不启用 InProcess。`JobScope::Unscoped` 不比较 owner，外来 UIA 控件和缺失探针仍派发；`JobScope::Missing` 拒绝，且不会变成 unscoped。同 PID 新创建时间、HWND 复用、owner 变化、同进程换窗口、句柄跨窗口、创建时间/代次/HWND/PID 为 0，在 scoped 作业里固定 ScopeDenied，不回退全桌面，不替换其他窗口。启动身份为空只拒绝 scoped 证明和证据接口，不拒绝普通 unscoped 操作。复验位于指针门之外；ADR 0013 的 Stop 常量、指针门 40ms、settle 15ms、fail-closed 与一次重试未改。产品路径已有的 WindowGone/AppNotFound/StaleElement/ElevationRequired/SecureDesktop 仍先返回。`release_all_input` 仍不经过 scope 证明。隔离父进程没有本机 WinApi 时 `InstanceEvidence` 为 None（fail-closed）；worker 子进程在 OS 交付点复验。桌面无关测试：`boot_token`（同钟不同启动会话、同会话改钟、全数字拒绝）、`prove_process`/`prove_window`/`prove_element`、`live_window`（路径变化仍证明，PID/HWND/owner/伪造 id 拒绝）、`tests/scope_identity.rs`（无来源 handle 为 None、创建时间为 0 不捕获、空 boot 时 unscoped 几何与定向点击仍成功且 scoped 几何/捕获拒绝、输入不 send、无 scope 时外来 owner 与缺失探针仍 invoke、同一 namespace 的代次属性名相同而独立 pid.created 不同）、`scope_regressions`（外来子树零内容读取、进程实例变化或同进程换窗口在读取前省略、代次变化在读取前拒绝、外来子输入零采样/焦点/发送、scoped 前台 SendInput 在 perform 入口不可用且动作证明、采样、发送与剪贴板发布为 0、定向输入的发送在作业内复验进程；scoped 采样在 WM_GETTEXT 之后、哈希返回之前再次复验，进程被替换则没有后续控件/像素读，settle 不交出成功样本（含 boot）、ApplicationFrameHost 不可用、capabilities/doctor 标前台不可用、准入后替换零 UIA/捕获进入、scoped 派发在 validate 之前要求进程实例）、`worker/input.rs`（scoped `ClipboardStage` 在打开剪贴板之前返回 CapabilityUnavailable，stage 次数为 0；unscoped stage 仍发布一次）、`worker/server/tests.rs` 的 Geometry ScopeDenied 且不替换窗口。未覆盖 / not_run：真实 `GetProcessTimes` 在 OS 回收 PID 之后的复用、真实 HWND 销毁后复用、真实 UIA `CurrentProcessId` 跨进程元素、捕获遮挡像素、scoped 前台 SendInput（路线关闭，任务 not_run；实机像素未跑）、钩子摘除与 Stop 时限（ADR 0013）、worker 进程死亡与在途投递、真实桌面 UWP `ApplicationFrameHost` owner 展开。 | Grok | 2–3 |
| S10 Linux 适配（按 X11/Wayland 拆 PR） | `gcu-driver-linux/src/identity.rs`、`src/{atspi,x11,wayland}/`；复用 start-time，窗口/portal 来源不可证即关闭。已实现（不启用 real eval）：进程身份是硬保证，事实是 pid + `/proc` start-time + boot_id，缺一即固定 ScopeDenied。`app_instance` 要求发现记录上的 pid 大于 0，并且该应用已登记的每一个窗口都是同一 pid、start-time 与 boot_id。作用域发现在任何名字或内容查询之前先 `GetNameOwner`，只接受含 `:` 与 `.` 的对等唯一连接（拒绝 `org.freedesktop.DBus` 以及没有唯一 owner 的引用），并且只对该唯一名调用 `GetConnectionUnixProcessID`，不再查询 well-known 名的当前 pid。作用域名字查询的许可是本次尝试捕获的当前租约代次（与包装层过滤同一份 AttemptAuthority），不是驱动保存的原始 ScopeGate。目录只索引总线名和窗口 id；只有当前 FixtureScope 接受的登记窗口才会被查询。空的准入集合不调用 GetChildren、GetNameOwner、Accessible Name、标题或 GetExtents。已完成的 rebind 去掉进程 P 后，随后的 list、resolve、list_windows 与 key_window 不读取 P。调用期间代次变化则丢弃已读名字，返回固定 ScopeDenied，不发布。仍被该代次接受的进程，在 pid、start-time、boot_id 与这份登记一致之后，才从该唯一连接读取 Accessible 名字。发布记录的 id 是原来的总线名，name 与 pid 来自这条已核验的唯一连接；核对失败则不发布。无作用域的 `list_atspi_apps` 仍先读名字再读 pid。well-known 名在登记进程 P 仍活着时被其他进程持有，或名字在 pid 查询前换手，则 list/resolve 不发布对方的名字、路径、哈希或 pid。作用域 `list_windows` 与 `key_window` 不调用 `list_atspi_candidate_windows`，也不读取任何应用的 D-Bus 名字、标题、子节点或范围。列表只含登记窗口中进程实例（pid、start-time、boot_id）仍匹配、且本动作已经证明的 X11 快照（incarnation 非 0）对应的窗口；标题留空，不取自 AT-SPI。没有这份 X11 证据时列表为空，`key_window` 拒绝。无作用域的窗口枚举仍先读每个应用的名字再读子窗口。本卡关闭作用域内 AT-SPI 树、逐节点内容、句柄和语义动作（含带 focus 的 TypeText），因为没有活的窗口 incarnation，也没有生产路径上的逐节点 owner 证据（`set_handle` 不是交付路径）。能力 `observe.tree` 与 `delivery.accessibility` 关闭；foreground 保持驱动原值。X11 作用域元数据在控制运行时外采集，同一动作共用既有 `NATIVE_TIMEOUT`（5s）；同步准入只读该捕获，发布前再验一次；incarnation 缺失或为 0 时直接拒绝，不等待 `_NET_WM_PID` 或几何。`_NET_WM_PID` 由客户设置，不是服务器认证的客户所有权。RealX 不提供活 incarnation，活的 X11 作用域路由保持关闭。有证明时作用域截图仍只走 Composite，失败不回退根窗口。Wayland/portal 在没有 peer credential 时关闭。作用域 doctor 在有 X 服务器且 portal 未提前返回时，只经 `off_runtime` 在既有 `NATIVE_TIMEOUT` 内探测 Composite 与 XTest，不调用 `begin_evidence_action`，工作线程不拥有输入释放。超时、驱动 Stop 或线程失败则报告关闭（ready 为 false，issues 为空）。无作用域的 doctor 仍在调用线程上同步探测。portal 提前返回仍只用已捕获的 PortalInfo。第五轮：`LinuxDriver::note_scoped_session` 锁存会话标志，一旦置位不再清除，与是否已调用 `bind_scope` 无关。标志已置位且没有平台绑定时，`list_apps`、`resolve_app`、`list_windows`、`key_window`、`snapshot_tree`、树读取、`capture`、`window_geometry` 与 `perform` 在发现或任何 X11/D-Bus 探测之前返回固定 ScopeDenied。`doctor` 只返回构造时已经捕获的能力快照（ready 为 false），不在调用线程上同步探测扩展。此时 `scope_capabilities` 将 tree、screenshot、events、accessibility、targeted pointer、targeted keys 与 foreground 标为不可用。未包装的驱动不置位该标志，发现与同步 doctor 保持原样。`release_all_input` 不看该标志。已绑定后再包装的驱动仍走原来的准入与运行时外扩展探测，foreground 仍是包装前的驱动原值。回归另见 `wrapped_linux_without_a_platform_binding_refuses_before_discovery`、`unbound_scoped_doctor_returns_without_an_x_probe_and_stop_releases`。RealX 的 `cookie.reply` 仍在证据线程上占用该 X 连接，与既有证据采集相同；FakeX 释放不进入扩展探测的 stall。Stop 常量（100ms latch、500ms release、5s native、4750ms driver）与 fail-closed 未改。回归见 `gcu-driver-linux/src/scope_regressions.rs` 的 `stalled_scoped_doctor_releases_stop_while_the_extension_probe_is_blocked`、`handed_off_bus_name_does_not_publish_the_previous_owners_name`、`verified_unique_owner_publishes_its_own_name`、`bus_name_held_by_another_pid_publishes_nothing`、`scoped_window_discovery_reads_no_foreign_atspi_content`、`removed_process_is_not_named_after_the_lease_narrows`、`lease_epoch_change_during_name_read_drops_the_name`、`wrapped_linux_without_a_platform_binding_refuses_before_discovery`、`unbound_scoped_doctor_returns_without_an_x_probe_and_stop_releases`（另有 `identity/scope.rs`、`x11/scope.rs`、`atspi/scope.rs`、`wayland/scope.rs`）。未覆盖 / not_run：真实 X11 桌面、真实 Wayland 合成器与 portal peer、真实 pixmap 是否混入其他窗口像素。 | Grok | 2–3/后端 |

S5 离线回归增量（已在三平台构建机运行，见上）：

| 行 | S1/S3 已覆盖（复用，不另建 harness） | S5 新增/扩展 |
| --- | --- | --- |
| 全工具与输入 | core C/D/伪造窗口；MCP 每个 profile 的实际 `tools/list` → dispatch，C 的单组参数 | core 加 A3；MCP 同一 loop 加 D/不存在/A3/伪造窗口、有图/无图/auto、语义/坐标/长文本/粘贴 chord，targeted/foreground/合成 clipboard 三路线；同连接验证 A/A2/B 观察、点击、输入及实际 delivery |
| 图片/句柄与信息 | core 原生 action 全变体的 raw/伪造 handle、跨窗口 focus、旧 generation 图片；MCP canary 像素实读对照、图片缓存/导出文件；core doctor/capabilities/错误刷新/settle/wait/unknown | core 同 generation 的另一个 run 拒绝保留图片（click/drag）与 sealed handle；MCP 扩展拒绝矩阵仍检查文本/像素且不得附图、资源、state 或 windows |
| 审批与 Stop | S1 原策略、用户活动、Stop 优先、审批中撤权/rebind；CLI release/非 eval 环境变量 gate；S3 私有控制/relay/EOF/畸形帧 | CLI 真正 `Approvals` + S3 fake evidence 覆盖所有 app allow 模式与敏感/前台默认拒绝、内置 deny/执行宿主身份 handoff、Stop 后 rebind/新 session/输入仍拒绝；app gate/cancel/不写 Always 的特性组合回归 |
| 嵌套硬停止 | core batch `stop_on_error=false` 中途拒绝且第三步不投递；script 首次调用拒绝后 hard stop；DOM 旁路关闭 | MCP batch 增加第三步并断言 perform 仅 1 次；script 参数化增加先成功再拒绝，catch 后允许目标也不继续 |

S1→S2/S3→S4→S5/S6→S7 是首个平台 12–18 人日；S8–S10 另计，平台补证失败的调查不在固定估算内。S1/S3 可先在 fake 上验收；S2 与 S4 完整接线、S6 外部边界未就绪前不启 real。

## 6. 残余风险与退出条件

- scope 不阻止夹具自身漏洞、执行面伪装、恶意依赖或被允许窗口触发文件/网络副作用；安全策略与外部 OS/egress 边界仍必需。允许模型 endpoint 本身也是可泄漏通道，桌面与凭据必须可丢弃。
- 原生全局输入仍有 OS 调度/焦点竞态，屏幕合成与系统代管 UI 也可能破坏归属；无法证明具体路线就禁用，不以成功任务分数豁免。Stop 释放与 Windows 同位置探针仍可能产生范围外可见影响，隔离桌面承担此残余风险。
- 私有 FD 不是同 UID 任意代码执行的沙箱。若客户端能 debug supervisor、读取其内存/句柄、替换二进制、附着其他有 TCC 的 helper 或控制 VM/防火墙，scope 可被绕过；环境契约必须拦住这些路径。
- OS/driver/helper/VM 管理器均属可信计算基；进程/窗口 id 重用、崩溃时持久凭据与日志残留需真实证据。scope 不能证明客户端内建工具过滤或扩展发现策略，review §7.3–6 未解决时仍不可跑真实客户端。
- 任何实现要求放宽 Stop 常量、跳过 owner 验证、用 prompt/env/file 代替控制证明、失败回退全桌面或 ordinary local host，均超出本提案；保留 `not_run` 并重开设计审阅。

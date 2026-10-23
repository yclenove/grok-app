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
当前 real 模式强制 `--env local`（`gcu-eval/src/main.rs:116`），`DesktopProvider` 只返回 `NotRun`（`gcu-eval/src/runner.rs:378`）；必须先实现受控 provider，禁止静默回退 local。

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
| S1 scope 类型与封闭包装 | 新 `gcu-core/src/scope/`、`driver.rs`、`action/router.rs`、`tests/fixture_scope.rs`；完整 Driver 覆盖、拒绝映射；新增方法必须显式实现，未知能力拒绝。已实现（分支 `cu2/scope-s1`，无 PR 编号，不启用 real eval）：`ScopeLease` 代次与 `watch` 使在途 read/capture/geometry/snapshot/list 在交付前复核当前租约，不一致则丢弃 payload 并返回固定 ScopeDenied；在途 perform 在撤销先于交付时取消并 `release_all_input`，已开始则按 unknown 上报，Stop 与撤权竞争为 `stopped_by_user`；subscribe 在 Pending 时遇到 revoke 或 rebind 会醒来并结束，且丢弃内层订阅，不需要新事件；Stop 只在带 waker 的 Activity 上醒来。RemoteHost 的默认 activity 与 `StopMirror` 不登记 waker，安静流要到下一次 poll 才拒绝事件，这不是已证明的 Stop 优先被绕过；转发前复核已准入的 owner 与 incarnation，generation 不变但 owner 或 incarnation 变了也不转发。router 取消观察把租约失效当作取消源。一次尝试先武装失效接收端再读取 `AttemptAuthority`，并把同一 run/generation 带过授权、recheck、dispatch 与观察完成；dispatch/observe 打开 scope 时必须与该 epoch 一致，PressKeys 等无 sealed handle 的动作同样生效。样本标记在读取前确定并随样本保留，缺失或过期标记拒绝为固定 ScopeDenied，不用当前 generation 补写；wait 持有的旧样本在超时或错误发布前再校验。`publish_with_limit`（包括 wait 发布）先在私有区完成缩略图、索引、渲染、哈希和返回状态组装，再取快照互斥量校验基线与预算、预留登记容量；随后在 `commit_if_current` 的租约读保护内插入快照。淘汰的大树与像素移出后在租约锁外释放。快照 id 可以留下空洞。wait 在指纹、签名、Pipeline 和快照锁之后再验；ScopeDenied 或 StoppedByUser 不再折进带 state 的 Timeout。WindowGone 在 `failure` 入口、任何 await 之前复制发起时的 epoch：尝试内用 attempt epoch，`with_attempt` 之外的几何或读取失败以及 wait 的错误路径用当时的 lease。失败刷新之后不再用当前 epoch 重盖。该 epoch 过期或已 Stop 时只返回 WindowGone，不附窗口列表、应用文本或可用窗口。选择截图时校验实际选中截图所属 epoch，而不是只看 latest。旧 epoch 的 session 授权在同 run 更新记录后淘汰。`SnapshotStore::set_scope_epoch` 只按 run id 长度差记账。scoped `key_window` 不转发会改窗口的 app 级调用，只经只读 `list_windows` 选出已登记窗口并完成该窗口准入。审批、观察、会话快照与授权寿命绑定 run/scope generation，`rebind` 后即使目标与内容不变也不投递（含无 sealed handle 的 PressKeys）。batch 报告带上尝试的 producing epoch；scoped 下没有 producing epoch 的行会清空，unknown 仍保持 unknown。MCP 编码把这份权威带过图片 worker、溢出文件和 `finish` 里的额外文件。文件先写到未登记的暂存名，再在租约锁外改为最终名，整个批次一直由 Drop 守卫私有持有，覆盖超时、取消、future drop 与 panic。MCP `finish` 在所有溢出和编码完成后，按快照 → image cache → 文件账本 → 租约的顺序取得锁；锁等待、容量检查和分配均在租约临界区之前。`commit_if_current` 在同一临界区无失败地登记文件、缓存和 `screenshot_delivered` 尺寸；输出锁持续到整批登记完成，`export_private_files` 与 `cached_image` 看不到未提交或半登记的产物。拒绝时丢弃私有守卫，不再先登记后删除。`list_apps`、`list_windows`、capabilities 与 script 在读取或运行前复制 epoch，await 之后与交付前重验，包括长输出溢出文件；scoped 下这些路径不用 `LiveStamp::off()`。scoped 下不存在无戳即安全：没有 producing epoch 的 `ActionState` 一律清除，包括文本恰好是固定拒绝句但仍带 app、window 或 screenshot 事实的状态。硬拒绝（AppDenied 加固定句且无 state）仍先阻断且不含事实。WindowGone 文本、`windows`、元素标签和批量失败/最后动作的 target 都携带 producing epoch，并与有效载荷一起校验或清除。unknown 与错误身份保持原样。无 scope 的历史行为，以及 epoch 仍匹配时的超时，仍保留 state。scope 硬拒绝同样清空。清空本次报告或编码不是 S4 那种随 lease 失效回收已经交付的会话图片缓存。perform 内首次 scope 拒绝的线协议结果为 `refused` 且 `delivery=none`。能力 `version` 与 doctor 权限 token 在 scoped 擦洗中按白名单规范化。验收覆盖：DOM fake share 对照（无 scope 走 DOM 通道，scoped 下 DOM perform 为 0）、script 内层可解析并读取 C 且允许目标正向 `cu.app` 不 restore 未登记窗口、MCP 各 profile 真实 `tools/list` 经服务端 dispatch 逐工具执行、像素/文件 canary 与允许目标的真实 `element_binding`。未覆盖 / not_run：MCP 安静流的 Stop 唤醒（RemoteHost 默认 activity 与 `StopMirror` 无 waker；下次 poll 仍拒绝，不是已证明的 Stop 优先绕过）；`rebind`/`revoke` 先放 state 锁再 `publish`，并发控制写入可能让 state 停在更新的 generation 而 watch 仍是旧值，新的 `arm_attempt` 会拒绝，只影响活性、不披露 payload；`AttemptAuthority::confirm` 可能把另一个 generation 标成 seen 再返回 false，调用方丢弃该尝试，不能把“被拒 generation 保持 unseen”说得比代码更强；最终确认之后的原生交付点许可（S2/S4）；授权修剪、快照改标，以及已经交付的图片与文件缓存随 lease 失效（S4）；socket/ACK/EOF 与畸形控制帧/relay（S3/S4）；真实 OS pid/start-time/window/AX owner 与捕获遮挡（S2/S4/S9/S10）。本轮没有新的“会话锁跨 await”或截图校验死锁。进程内 rebind/revoke 不代替私有传输。scoped 模式不装配 DOM hub，真实扩展旁路 not_run。gcu-eval 与平台驱动内部未改。Windows worker 只补了 `ScopeDenied` 的 serde 镜像。覆盖测试：`gcu-core/tests/fixture_scope.rs`、`fixture_scope_round2.rs`、`fixture_scope_round3.rs`、`fixture_scope_round4.rs`、`fixture_scope_round5.rs`、`fixture_scope_dom.rs`、`gcu-core/tests/policy/grants.rs`、`gcu-core/tests/snapshot.rs`、`gcu-script/tests/fixture_scope.rs`、`gcu-mcp/tests/fixture_scope.rs`、`gcu-mcp/src/result_tests.rs`、`gcu-mcp/src/result_scope_tests.rs`、`gcu-mcp/src/result_round5_tests.rs`。第六轮改为单一原子提交点，不再逐点补“先校验、后使用”。索引、渲染、缩略图、图片编码、文件写入（暂存名，不登记、不进导出账本、不进 image cache、不进快照存储）和 WindowGone 元数据组装都在提交点之外完成，产物保持私有。`ScopeLease::commit_if_current` 在租约读保护内确认 authority 仍是当前，并在同一临界区完成登记或交换；`revoke`/`rebind` 取写保护，因此撤权与提交互斥、不可交错。临界区内只有内存登记（文件已在临界区外改到最终名，登记才视为可导出），没有 await、输出锁等待、重活或新的磁盘写入。提交失败由守卫 Drop 清掉全部私有暂存（取消、超时、drop、panic），调用方只得到固定拒绝。`export_private_files`、`cached_image` 和快照存储只在提交成功之后看得到。core `publish_with_limit`（含 wait 发布）、WindowGone、MCP `finish`（文件、缓存、`screenshot_delivered`）以及 discovery/script 溢出都走这一个点。保证：未提交产物不可见；撤权不能插进临界区。不保证：不回溯已经合法提交的产物；提交点之前的在途工作可能浪费，但不可见。无 scope 时 script 文本仍只取 2000 字节前缀。本轮不改 Stop 常量或 fail-closed，也不覆盖 (b) 类残余（MCP Stop waker、rebind/revoke 发布活性、原生交付点、已交付缓存回收）。覆盖测试另加 `gcu-core/tests/fixture_scope_round6.rs`、`gcu-mcp/src/result_round6_tests.rs`。 第七次评审（R6-1）：被拒绝的发布不再返回或记账由 scope 派生的目标标签（元素标签等），在最终化处一个 helper 同时清除返回 action 与动作账本条目，投递、错误与 unknown 状态保持，无 scope 时为空操作；WindowGone 的 Stop 读取移到租约临界区之前（Stop 恰在提交点期间发生时，父级的抑制可能错过一次，但租约仍原子拒绝旧 epoch，不产生 scope 泄漏）。新增 (b) 类残余（无已证明的泄漏路径）：`PublishGate` 与租约门禁访问器在 `gcu-core` 里是公开 API，不是仅测试可用，生产代码没有武装它的调用方，但被武装的 during-gate 会在持有读锁时拖住 `revoke`（15 秒后 panic）；标准库 `RwLock` 无写者饥饿上界；提交点内的 `activity` 调用已移出租约；测试没有整体超时。七次 Codex 评审（`reviews/2026-10-23-scope-s1-review*.md`）的最终结论为 MERGE（无 (a) 类）。实现由 Grok（前几轮）与 Codex（收尾）完成。 | Codex | 2–3 |
| S2 首个平台身份适配 | `gcu-driver-macos/src/{api,driver,windows,real/*,ax/*,input/*}.rs`；实例/窗口/handle owner 与投递重验；缺证据路线关闭 | Grok | 2–3 |
| S3 私有 bootstrap 与 relay | `gcu-eval/src/{prep,exec,runner}.rs`、新 `scope/relay` 模块；`gcu-cli/src/{args,runtime}.rs`；最小协议类型；fake 子进程证明控制/数据隔离与撤权 | Codex | 2–3 |
| S4 专用 helper 与连接装配 | `gcu-cli/src/helper/{auth,client,frame,serve,macos}.rs`、`gcu-mcp/src/{server,image_cache,files}.rs`、core session 清理；最终 native 层 scope ACK、禁旁路，不复用普通 helper | Grok | 2–3 |
| S5 审批与跨工具回归 | `gcu-cli/src/host/approval.rs`、core/mcp/script/cli 的测试；拆 app/sensitive/foreground 自动策略；所有 catalog 工具与拒绝无泄漏 | Codex | 1–2 |
| S6 一次性环境 provider | `gcu-eval/src/{runner,main,report}.rs`、新 provider/contract 模块、`eval/README.md`；外部 VM/网关模板、前置核验、整树回收，缺项 `not_run` | Codex | 2–3 |
| S7 首个平台验收证据 | `docs/` 的确定性原生矩阵、环境回执与未支持项；owner 操作真实用户恢复，控制方审阅再安排 LLM | Grok + Codex | 1 |
| S8 多目标任务扩展（后续） | `apps/fixture/src/`、eval task schema/启动器及上述 8 个 cross_app YAML；显式角色/进程/窗口声明，缺映射不跑 | Codex | 1–2 |
| S9 Windows 适配（独立 PR） | `gcu-driver-windows/src/discovery.rs`、`src/worker/`、`src/uia/`、`src/input/`；创建时间/HWND/UIA owner、Worker 端复验；保持 ADR 0013 | Grok | 2–3 |
| S10 Linux 适配（按 X11/Wayland 拆 PR） | `gcu-driver-linux/src/identity.rs`、`src/{atspi,x11,wayland}/`；复用 start-time，窗口/portal 来源不可证即关闭 | Grok | 2–3/后端 |

S1→S2/S3→S4→S5/S6→S7 是首个平台 12–18 人日；S8–S10 另计，平台补证失败的调查不在固定估算内。S1/S3 可先在 fake 上验收；S2 与 S4 完整接线、S6 外部边界未就绪前不启 real。

## 6. 残余风险与退出条件

- scope 不阻止夹具自身漏洞、执行面伪装、恶意依赖或被允许窗口触发文件/网络副作用；安全策略与外部 OS/egress 边界仍必需。允许模型 endpoint 本身也是可泄漏通道，桌面与凭据必须可丢弃。
- 原生全局输入仍有 OS 调度/焦点竞态，屏幕合成与系统代管 UI 也可能破坏归属；无法证明具体路线就禁用，不以成功任务分数豁免。Stop 释放与 Windows 同位置探针仍可能产生范围外可见影响，隔离桌面承担此残余风险。
- 私有 FD 不是同 UID 任意代码执行的沙箱。若客户端能 debug supervisor、读取其内存/句柄、替换二进制、附着其他有 TCC 的 helper 或控制 VM/防火墙，scope 可被绕过；环境契约必须拦住这些路径。
- OS/driver/helper/VM 管理器均属可信计算基；进程/窗口 id 重用、崩溃时持久凭据与日志残留需真实证据。scope 不能证明客户端内建工具过滤或扩展发现策略，review §7.3–6 未解决时仍不可跑真实客户端。
- 任何实现要求放宽 Stop 常量、跳过 owner 验证、用 prompt/env/file 代替控制证明、失败回退全桌面或 ordinary local host，均超出本提案；保留 `not_run` 并重开设计审阅。

# 0018 · Tart 一次性环境 provider（S11）

状态：**Proposed** · 日期：2026-10-31 · 对应 [ADR 0017](0017-real-eval-fixture-scope.md) §5 S11。

本文只设计，不安装 provider、不改变 S6 门禁、不批准真实运行。验收顺序仍由[矩阵 §8](../real-eval-acceptance-matrix.md#8-开启真实运行的门槛)决定。

## 1. 基线、边界与威胁模型

**原则：把模型及其可启动代码视为不可信；宿主只提供一次 attempt 的封闭 VM 能力。** 越界、证据缺失或无法判断均 `not_run`，不回退宿主、普通 helper 或非 scoped 模式。

已知事实（owner 提供，不在本 ADR 重做探测）：宿主为 owner 的 Mac mini；Tart 2.41.1；访客机为可无头运行的 macOS `macos-s2-eval`，GUI 供 owner 点击权限。Softnet 正由 owner 安装，**尚未就绪**。真实客户端是 Grok CLI / `grok-4.7`；登录链接由 owner 完成，agent 永不输入密码。无付费服务、Developer ID 或签名材料。

| 边界 | 允许 | 永不提供 / 拒绝 |
| --- | --- | --- |
| 不可信 guest 客户端 | 合成工作目录；已声明 fixture 的 scoped MCP 数据面；唯一模型 endpoint 的 TCP/TLS | 宿主文件、个人桌面、任意公网/LAN/metadata；扩大 scope、恢复 Stop、控制 VM/网络 |
| guest 可信 supervisor / 专用 helper | 只读受控程序；私有控制 FD；固定 fixture 的实例核验；独立权限与 Stop 存储 | 模型 debug/ptrace、读内存/句柄、改程序/控制目录、sudo；附着普通或其他有 TCC 的 helper |
| 宿主 provider / 调用方 | Tart/Softnet 子进程与 VM 句柄；外部网络策略；回执和持久 Stop | 共享目录、剪贴板同步、拖放、设备转发、宿主 agent/SSH forwarding、宿主凭据或通用执行/代理服务 |

guest 只含夹具、评测工具和必要系统组件；模型无管理员权限。guest supervisor 与客户端分离安全主体，并有实际生效的 OS 沙箱，**不能用同 UID、文件权限或私有 FD 独自证明隔离**。helper/fixture 禁外网；relay 只连声明的 guest loopback 数据端口，loopback 不整体豁免。

宿主→guest 拟采用绑定 VM 句柄的私有 virtio 控制通道，guest 只接受版本化的固定操作（inventory/probe/bootstrap/login/run/lease/revoke），不接受路径、shell、转发或模型控制帧；回复有长度上限和截止时间。**假设 H1：Tart 2.41.1 可提供并隔离该通道，尚无本分支证据**；S11a 必须确认具体接线，做不到即停止设计实施，不以 SSH、共享盘、网关端口或可达文件补洞。S3 的 scope 控制仍是 guest 内继承 FD，不能把 VM 通道直接交给模型。

范围沿用 ADR 0017 §2/§3/§6：macOS 硬保证是进程实例，窗口检查尽力而为、失败关闭；不声称原子窗口投递。不修 OS/驱动/VM 漏洞、夹具漏洞、恶意依赖或 endpoint 泄漏通道；不解除已关闭的全局输入、剪贴板、DOM、多目标未实现项；不证明真实客户端内建工具/扩展过滤。相关任务继续 `not_run`。OS、Tart、Softnet、宿主 provider、guest supervisor/helper 属 TCB；TCB 妥协超出保证，发现异常即停止。

代码依据（以下路径相对 `computer-use/crates/`）：

| 现状来源 | 设计必须保留的约束 |
| --- | --- |
| `gcu-eval/src/disposable.rs`、`disposable/tests.rs` | 五个 Guarantee、16 个 Probe；每行 machine + external；缺项/重复/到期/ACK 不符拒绝；清理确认后才交付结果 |
| `gcu-eval/src/boundary.rs` | `accepting_provider_linked()` / `scoped_provider_ack()` 恒拒绝；`AdmittedLaunch` 仅由 admission 铸造；Command 构造集中于此 |
| `gcu-eval/src/{launch,prep,runner}.rs` | 临时 HOME、shim、tools/preflight 不是安全边界；当前 real 路径在 auth/fixture/preflight 前拒绝；不得照搬 proxy 继承或宿主 auth seed |
| `gcu-cli/src/scope/`、`gcu-eval/src/scope/mod.rs` | 当前只有 fake bootstrap；ACK 后开放数据面；3s 引擎单调租约、EOF/重放/畸形帧撤权；relay 无控制权 |
| ADR 0017 S4、矩阵 §2/§7 | 本分支没有 `gcu-cli/src/helper/scope_link.rs`；其接线属 S4，活代码身份读取属独立 S12；均不能由 S11 伪造 |

## 2. 一次 attempt 的状态机

**原则：调用方顺序驱动、一个 attempt 一个 APFS clone，完成销毁证明才结束。** 不复用 clone，不直接启动/修改 golden；A/B、repeat、失败重试各分配新 run_id。任何非预期转移进入清理，不能“继续试一下”。

`macos-s2-eval` 先作为 golden 候选：owner 在无凭据的准备副本里给固定 helper 身份点 TCC，关机后封存为带摘要的 golden；不得保存登录态。TCC 随 clone 保留只是待验证假设，每次仍读真实权限。golden 内容/配置/程序一变即新摘要、新签认；所有 attempt 只从封存版本 `tart clone`。

| 状态 → 下一状态 | 同步动作与成功条件 | 失败行为 |
| --- | --- | --- |
| `Idle → Reserved` | 校验安装清单、Stop、golden 摘要；先做遗留回收；外部 owner 已持有清理责任；持久登记 run_id、预定 clone id、绝对截止 | 未分配也写拒绝回执；不 clone |
| `Reserved → Cloned` | `tart clone <golden> <attempt-id>`；核对 APFS clone 来源、唯一 VM 配置/MAC、资源句柄 | 部分 clone 也登记并清理 |
| `Cloned → Booting` | 先装外部网络约束，再以 §5 精确 flags 启动；持有 tart/softnet 子进程实例及 VM 句柄 | 不能以无 Softnet 的参数重启 |
| `Booting → Isolated` | 可信通道核对 VM/config 与 guest boot/UID/desktop；无模型、无凭据的 inventory、文件/IPC/sandbox、egress 探针 | 某行可暂为 unknown，但不能开放客户端；违约立即清理 |
| `Isolated → Bootstrapped` | §3 的窄 bootstrap 权限启动固定 fixture、专用 helper；S12 活身份复核；owner 的 TCC 签认 + 实测；guest supervisor 发送 S3/S4 Install 并核对最终 native ACK | 缺 S4/S12、普通 helper、无/假 ACK 均清理；不退 fake |
| `Bootstrapped → CredentialReady` | owner 完成 §6 登录链接；仅固定无模型 login 流程；复验凭据元数据、外部签认、五行和有效期 | 缺 endpoint/期限/owner 操作即 `not_run`；不启动 preflight |
| `CredentialReady → Ready` | admission 独立验证所有证据，产生不可序列化的 `VerifiedProviderSession` | 拒绝后不允许拿旧证据重试 |
| `Ready → Running` | 消费 client launch；guest 内启动带凭据的 tools preflight，再执行任务；缓存仅本 instance/run 有效 | preflight 拒绝则 `not_run`；不换客户端/出口补跑 |
| `Running → Revoking` | 完成、取消、Stop、超时、进程/控制/租约故障均撤权；不重放 unknown | 保留原因与 provisional 任务结果，尚不计 pass |
| `Revoking → Destroying` | 禁新工作、断数据/控制、释放输入、终止 guest 树；`tart stop` 后核对 VM 与 tart/softnet 均退出 | 不因模型已退出就声称整树已停 |
| `Destroying → Final` | `tart delete <attempt-id>`，确认 clone/私有存储/通道消失，原子封存回执 | 清理或回执失败进入 `Quarantined` |

`Final`、`Quarantined` 是终态；不再转回 Ready。Final 结果可为 `not_run` 或任务原有 pass/fail/partial/timeout/crash 等；仅当环境完整、清理 `Finished` 且确认成功时保留任务评分。环境违约、缺证明、边界崩溃一律 `not_run`，即使模型曾返回成功；保留 `client_started` 和原始终止原因。Quarantined 是 `not_run/cleanup_failed`，停后续 task/arm/repeat。

拟议上限：宿主单调钟控制；安装检查/每个查询 5s，clone 60s，启动就绪 120s，每个探针 2s（整组 60s），bootstrap/ACK 每阶段 5s，owner 登录等待 5min；准备总计 ≤10min。任务预算为 `min(task.max_seconds, 15min)`；清理总计 ≤60s，其中 revoke/guest drain ≤10s、stop+确认 ≤20s、delete+确认 ≤20s、回执 ≤10s。每个子进程的超时含 kill/reap，输出有界（单命令 ≤1MiB）；不能只给外层 future 超时而留下阻塞 IO。外部硬截止为预留的准备+任务+清理预算，不随重试延长。

3s scope 租约不变；guest supervisor 在同一受控事件循环按 ≤1s 调度 Renew，长命令用截止时间轮询，不能阻塞续租、Stop 或释放。宿主每 ≤1s 发送绑定 run/instance/递增序号的 lease；guest supervisor 对其存活设置 ≤3s 的固定截止，到期终止模型树，迟到帧不能复活。S3 scope 租约本身只撤 gcu 权限，不能冒充客户端父死回收。新 provider 不加 observer 线程、共享生命周期管理器或后台自动恢复。现有调用方/controller 作为一次性外层 owner 持有 provider 子进程与 VM 清理账本；顺序等待带截止的事件，provider 死亡时接手，**不能只靠 provider 的 Drop**，不安装常驻 daemon。

ADR 0017 §3 / ADR 0013 的 Stop 常量全部不变：100ms 锁存、500ms 释放、5s native、4750ms driver，以及既有平台常量/重试额度。上述 VM 超时不是新的 Stop 预算；scope/网络/清理等待不得占住 Stop 或释放所需的锁。
非正常控制断线在 VM 外锁存持久 Stop；持久状态无法确认也拒绝下一次分配。清理、宿主重启或新 clone 不能清锁存，仍须真实用户恢复。

| 故障 | 撤权与回收；报告 |
| --- | --- |
| provider 子进程崩溃/取消 | 外层 owner 收 EOF，关闭控制、停止/删除指定 VM；即使清理成功，本次仍 `not_run/provider_crash` |
| guest 崩溃/重启、boot id 改变、Tart/Softnet 退出 | session/ACK 立即无效；外层 stop/delete，不认领重启的 guest；`not_run` |
| 外层 owner 也死亡 | guest 控制存活租约到期，停止模型树并撤 scope；不声称 VM 已删除；回执 cleanup=unknown，下一启动先回收 |
| 宿主崩溃/断电 | 无法及时写清理证明；禁止自动开机续跑 VM；重启后先回收孤儿，旧 attempt 永久 `not_run` |

幂等清理只处理宿主受保护账本拥有的精确 clone 身份，先查状态再 stop/delete；权威库存查询成功且确认该资源不存在才可视为已删除，查询失败/不完整不是“已消失”。登记须先于 clone，覆盖 clone 成功但回复丢失的窗口。下次启动对未封存记录逐个核对 VM 配置与身份、回收并追加 recovery 回执；不按前缀猜删、不删 golden/外部 VM、不重启孤儿。身份歧义、锁竞争、回收失败均隔离并禁止新 attempt。APFS 删除是逻辑销毁，不声称 SSD 安全擦除；禁 clone 快照/备份，服务端凭据短期失效兜底。

## 3. 接入与准入能力

**原则：编入 provider 只是具备验证能力，只有绑定本实例的阶段能力能授权对应操作。** 默认构建或没有有效 session 时仍为 `provider_unavailable/not_run`；不得把今天两个 false 改为可设置的全局 true。

- 拟加 Cargo feature `tart-disposable`，默认关闭、限 macOS；只编入真实适配器与 admission 私有接线。`test-support` 仍只供独立生命周期测试，生产安装禁止；`eval-autoapprove` 与现有 debug 限制不改。
- “installed” 指宿主调用方已核验 owner 批准的 Tart/Softnet/provider 程序摘要、golden 清单、网络 guard 与私有通道可用；不是 PATH 中存在一个 `tart`。运行时决定镜像、实例、实际策略/探针、签认、ACK、租约与 Stop 是否有效，feature 本身不能证明这些。
- `boundary.rs` 内私有 admission 子模块持有阶段类型；字段私有、无 Clone/serde/公开构造器。公开 `Prepared`、`Binding`、回执和 `DisposableProvider` trait 的返回值只是证据数据，不能导入成能力；只有固定真实适配器取得并交叉核验的事实可推进阶段。
- `VerifiedProviderSession` 必含五行完整 machine=`checked` / external=`attested`、16 个 probe 的唯一成功记录、run_id/attempt、VM 实例/boot/镜像/策略摘要、权限/helper 活身份、S3/S4 ACK、generation、凭据类型/到期、单调截止和撤权状态。构造后不可更换绑定；spawn 前重新检查存活、代次、独占到期边界及 Stop，消费一次权限。
- `admit_client`/`admit_scoped_backend`/`admit_fixture` 及 Command 构造仍集中于此；`AdmittedLaunch` 绑定角色、固定程序摘要、guest 执行目标与上述 session，不能把 guest launch 变成宿主同路径执行。模型客户端、包括带凭据 preflight，必须持有完整 session；不输出可改 args/env 的通用 Command builder 给非 admission 调用者。

**bootstrap 的循环依赖必须显式处理：** 完整第五行需要 helper/fixture 已启动并 ACK，不能要求 ACK 先于该启动，也不能给任意 backend 一个提前放行开关。拟把 `AdmittedLaunch` 分为私有 Bootstrap 与 Client 阶段：Bootstrap 只能由已核验外部隔离的 `VerifiedIsolation` + 固定只读 guest 清单铸造，允许本 VM 内固定 fixture/helper（无凭据、无外网）启动；它不能执行任意路径、传模型 prompt、带 auth、运行 task/preflight，不能转为 Client 权限。真实 ACK 后，先以“auth 尚不存在”的 CredentialMetadata 事实完成五行核验，产生仅能登录的 session 阶段；只允许清单内固定登录子命令，无模型请求。登录改变凭据行后须重新核验完整 Binding/证明/期限，才产生 Client session。Tart/Softnet 本身用同模块独立的 HostControl 角色，只接受固定生命周期 argv，绝不借用 client/backend 权限。

`DisposableProvider::prepare` 的“不得启动带凭据进程”契约保留；登录不能偷偷塞入 prepare。实施须把当前 `supervise` 拆成有界的无凭据 prepare/bootstrap、受限 login、重新核验、client_work、显式 teardown 五段，沿用同一拒绝/清理报告；在 API 与角色检查完成前仍拒绝全部 real 路径。

此分阶段是拟议的 S6 接线修改，**当前代码没有这些能力**；评审必须证明 Bootstrap/HostControl/Login 无法被公开路径入口获取或转换，否则 S11 不安装。所有阶段仍由 admission 签发，不能把引导藏在 launchd 或 trait 回调中绕开 Command 源码守卫。已有 `admit_launch` 的默认拒绝分支保留；另设只供受信适配器的类型入口，不用旧宣誓改写其判断。

| A1 历史旁路形状 | 必须继续拒绝的理由 |
| --- | --- |
| 公开带路径入口（含空路径、hidden/standalone fake） | 路径无 session；不能选择 Bootstrap/HostControl/Login，拒绝先于 prepare/spawn/凭据/preflight |
| 搬迁真实 binary / artifact 目录 | 位置不授信；只接受安装清单中的角色/摘要且绑定活 VM 与阶段，不按目录认领 |
| 伪造相邻 sibling / `eval-fake-agent` 名称 | 名称、相邻性、`builtin_gcu`、旧 attestation 均无权；内置 fake 仍是进程内实现 |
| 直接 `Supervisor::start` | 必须消费同一 admission 的正确角色能力；不能接收路径或裸 `Prepared`，无能力不构造 Command；`open` 同样先准入 |

没有 env/config/CLI flag、模型可达文件、可导入签名 JSON 或布尔 setter 能启门。宿主账本/安装清单仅为核验输入，删除/修改它们最多导致拒绝，不可生成已验证 session。session 失效或 scope rebind 后旧能力立即拒绝；首版不做 attempt 内 fixture 重启/扩权，需新 clone。

## 4. 五行证据与签认

**原则：机检与外部签认各自保留来源，不能互相覆盖。** `Check::Failed` 是违约；缺/重复/超时为 unknown；每行两类证据都齐全才允许客户端。owner 签认不能把失败探针改成 passed。

| ADR 0017 §4 行 / Probe | provider 本次机检（checked） | 外部证明（attested）及失效条件 |
| --- | --- | --- |
| 一次性桌面：DesktopInventory、PrivateStorage | Tart config/运行句柄与 guest boot UUID/UID/desktop 交叉核对；OS build、镜像摘要、活动会话/进程/挂载；新私有目录、独立 helper/Stop 存储 | owner 签认 golden 来源、无个人数据/登录/秘密、无备份；绑定镜像摘要，最长 7 日，任何改动立即失效 |
| 文件/IPC：Sandbox、HostFileDenied、HostIpcDenied | 实际 VM 配置无共享目录/集成；guest sandbox 生效；宿主确认合成 sentinel 存在而 guest 不能读，未建 canary 为 unknown；agent/display/helper IPC、debug/ptrace、控制 FD/目录写入否定探针 | owner 确认宿主集成/转发关闭，provider 证明固定 guest sandbox 与私有通道隔离；绑定配置/程序/策略摘要，本 attempt 到期 |
| endpoint egress：GatewayPolicy、Lan/Metadata/Public/Dns/Ipv6/QuicDenied、ModelEndpoint | §5 的实际 argv、外部 guard 状态/摘要/计数与双端探针；无通用代理/额外刷新 endpoint | owner 批准精确 endpoint 清单和外部策略，provider 证明该策略绑定本 Softnet/VM；地址或策略变化即失效 |
| 无 ambient credentials：CredentialMetadata | env 键名白名单、不继承 proxy/agent；guest 私有 auth 目录/文件身份/权限/到期；无 host keychain/profile 挂载，不打印值 | owner 签认仅一个 eval 身份和服务端期限；provider 证明 login 到本 clone 的来源；过期/额外凭据/刷新请求即失效 |
| native 与退出：NativeReadiness、LifetimeBarrier | S12 活代码身份 + 进程实例；TCC 实测；S4 最终 native ACK 对照 Binding；真实 Stop/退出故障测试证据、VM 外部 owner 在线 | owner 点击并签认 helper 的 TCC；provider 签认本实例退出责任及已验证的生命周期版本；权限撤销、代码/boot/ACK/控制 owner 变化立即失效 |

镜像/TCC 的准备签认可引用；每次须由宿主可信用户入口确认本 attempt 的完整 `Binding`（含 ACK）、claims 和到期，最长覆盖本次硬截止，且不超过镜像/凭据有效期。记录声明摘要、owner 角色、UTC 日期/到期和可信用户事件编号；不能从 guest 文件、命令行勾选或模型文本导入。

不引入签名密钥：provider 以受核验安装身份在宿主进程内出具证明，owner 经本地可信入口签认；`AttestationVerifier` 从调用方私有内存中的权威记录匹配完整声明，不把回执摘要当密码学签名。存档明确 `signature=none, provenance=local-owner/provider`，不宣称第三方可独立验签。跨进程来源须用 OS peer/实例认证；做不到记 unknown。旧回执、重启后重读的签认均不能恢复 session，必须重验并新签认。

## 5. 出站策略与无模型探针

**原则：只允许一次固定的模型目的集合，探针成功必须同时证明拒绝路径确实经过外部策略。** 单纯连不上、CLI 自报无网络或 guest 自己的防火墙均不算证明；缺计数/观测为 unknown → `not_run`。

1. 宿主在启动前只解析一次经 owner 审核的 endpoint 名称，拒绝私网、loopback、link-local、metadata、组播、gateway 和仅有 IPv6 的结果；从全部获批 A 地址生成 `/32`（最多16个），AAAA 永不放行，禁用宽网段。记录解析时刻、TTL 与集合摘要；pin 有效期是本 attempt 硬截止，TTL 只作诊断，不触发 guest 解析或扩权。guest 固定只读名称→IP 映射，仍用原 hostname 做 SNI/证书校验，不把 TLS 验证关掉。
2. 精确启动：`tart run <attempt-id> --net-softnet --net-softnet-block=0.0.0.0/0 --net-softnet-allow=<获批IPv4 /32 CIDRs>`。provider 比较构造 argv、实际子进程/Softnet 身份、VM 配置和策略读回；不只相信待执行字符串。Softnet 最长前缀优先、同长 block 胜 allow；拒绝与 endpoint 冲突的规则，不能扩大 allow 来修通。
3. **Softnet 默认可达 vmnet gateway，且给定 CIDR 能力只证明 IPv4 地址过滤，不证明端口/协议/IPv6拒绝。** 拟由 owner 预装宿主网络 guard，约束本 Tart/Softnet 实例及其后代：仅允许上述 IP 的 TCP/443，拒绝 UDP（含 endpoint 的 QUIC）、IPv6及 gateway 的 IP 服务；只放行必要的 vmnet 管理 IPC。不新增代理。**假设 H2：该 guard 覆盖完整 guest 包路径，包括 Softnet 内部处理的 gateway 流量及继承句柄**；单纯宿主 socket outbound sandbox 不足以证明这一点。S11c 必须固定具体实现/版本并读回策略、实测计数；当前未选定合格实现，记 `GatewayPolicy=unknown/not_run`，不能用 guest PF 或“没有收到回应”代替。DHCP/ARP 链路启动不等于获准访问 gateway 的 DNS/HTTP/宿主服务。
4. 探针在 guest **模型进程启动前**运行，使用将来客户端相同 UID/sandbox/路由，并另测 helper/fixture 身份无外网。只测合成目标，不扫描 owner LAN；宿主/受控接收端检查新 nonce 和拒绝计数，证据由 provider 采集，不接受模型提交的结果。

| 探针目标 | 预期及证明 |
| --- | --- |
| LAN、metadata（link-local）、非 allow 的任意公网类别 | 各选预声明合成 canary；TCP/UDP 发包必须命中外部 deny，接收端无 nonce；策略覆盖全类别，有限抽样不冒充穷举 |
| DNS：gateway、指定外部解析器的 UDP/TCP 53、DoT 853；未授权 DoH IP | 全拒绝并有策略计数；guest 不递归解析、不回退系统 DNS；endpoint 共享 IP 上的其他服务不因固定 names 获应用级保证 |
| IPv6：global、ULA/link-local、gateway | 全拒绝（含未提供 IPv6 路由时的外部 guard 覆盖证明）；只有 guest 无路由不够 |
| QUIC/UDP：endpoint:443、gateway、非 allow 目标 | 全拒绝；不得从 endpoint 的 TCP 成功推断 UDP 被挡 |
| 只读固定 endpoint | 每个获批 IP 的无凭据 TLS/SNI 请求成功，身份/证书匹配；禁跨 host 重定向、CONNECT/通用代理；不调用模型 |
| gateway/宿主 IP 服务 | DNS/HTTP/探针服务不可达；证明 Softnet 默认 gateway 例外已被覆盖；不得为控制通道开 IP 例外 |

endpoint 轮换至新 IP/域名时不更新 pin；已知轮换、TLS 身份异常或固定 IP 请求失败即撤销 session，本次 `not_run` 并清理；新 attempt 由宿主重新解析/审批/固定和探测，不热扩 allow。IP allow 不证明同 IP 的虚拟主机隔离，固定 names 也不能限制恶意代码自选 SNI；若允许 IP 可作 DNS/DoH/通用代理或无法证明其服务边界，则 ModelEndpoint=unknown，不宣称“DNS 全拒绝”。获准 endpoint 本身仍是泄漏通道（ADR 0017 §6），只能由无个人数据的 guest 承担。所需登录/刷新域名不能自动加入：须先明确审核清单；严格“模型 endpoint 唯一出口”无法兼容时保持 `not_run`，另提设计决定。

## 6. eval 凭据

**原则：凭据只在通过隔离和 native ACK 的 clone 内产生，在需要时才交给固定客户端。** 无明确服务端期限、额外凭据、登录需未批准出口或 owner 未完成链接均 `not_run`，不复制宿主登录态。

登录子命令运行时没有模型/task prompt，owner 自己完成设备链接；密码不由 agent 输入，链接/设备码不进报告。登录产物先在 guest 可信登录主体的私有 staging（目录 0700、文件 0600）中，模型/relay/script 均尚未启动且不可读。最终准入前核对元数据，以一次性能力把唯一 auth 文件交付本次客户端私有 `GROK_HOME`；不经宿主 seed、argv、env、共享盘或日志。文件检查沿用 `prep_seed.rs` 的同句柄普通文件/非链接/大小上限思路，但不沿用宿主 `auth_seed`。

Grok CLI 必须在请求时使用该凭据；**不承诺模型可执行代码在客户端已获凭据后仍绝对读不到它**。依靠唯一可丢弃身份、有限出口、短有效期减损，不能把 tools allow-list 当隔离。拟要求服务端有效期 ≤30min、禁止 refresh token，启动时剩余期限覆盖任务预算+清理；若 Grok 登录只给长期/可刷新身份，当前方案不满足契约，保留 `not_run` 待 owner 决定。clone 删除即逻辑删除 staging/auth；无法确认删除则清理失败。删除不等于远端撤销：若无即时撤销接口，明记仍存续到服务端到期的残余，不等待30min冒充60s清理。回执只记类型、签发/到期 UTC、撤销状态，不记值、散列、设备码或账号。

## 7. 回执与结果发布

**原则：回执是宿主事实记录，不是下次启动的授权文件。** 任一关键 unknown、写入失败或未确认 teardown 都不给 pass，后续全部 `not_run`。

扩展[矩阵 §6 模板](../real-eval-acceptance-matrix.md#6-环境回执模板每个-attempt-一份结束后归档)，不改其既有字段语义：

| 字段组 | 必需内容 / 出具方 |
| --- | --- |
| 标识/版本 | schema、provider/Tart/Softnet/guard 版本与构建摘要、run_id=Binding.attempt、task/arm/repeat、VM opaque id、镜像/config/boot/desktop/UID；provider |
| 状态/时限 | 有界转移序列、单调耗时、UTC 起止/独占到期、client_started、原始任务终态、最终 not_run/refusal；provider/调用方 |
| 五行 | 每个 Probe 的 Check/来源/证据摘要、machine/external 状态；owner/provider claims、完整 Binding 与期限、可信事件编号、signature=none |
| 网络 | model endpoint 填获批公共服务名/端口与清单 id；解析集合/实际 argv/外部策略摘要、各类别拒绝计数、nonce 探针结果、解析 TTL 与 pin 截止；provider；公开回执不含 IP/CIDR/家庭拓扑 |
| 凭据/权限 | §6 类型与到期；helper 的逻辑 guest 安装路径+内容哈希/活代码身份（无签名则明记）、TCC 签认与实测；owner + provider |
| scope/Stop | 原样 ACK 的 run_id/generation/scope_digest；engine/fixture 的 pid/start-time/boot 与窗口登记摘要；控制来源与协议版本、租约/撤权原因；Stop 实测与外部锁存引用 |
| teardown | CleanupCause/Disposition、绑定是否匹配、whole_tree/storage/channels 的 Check、stop/delete 后查询证据、外层 owner、孤儿回收关联；调用方封存 |

宿主专用受保护的评测输出目录保存 `<run_id>/receipt.json`（0700/0600，临时文件原子替换）；分配前先写清理账本，每阶段追加有限记录，结束封存。controller 持有路径；guest 无挂载、读写权或任意文件导出接口。JSON/HTML/A/B 只读规范化回执，不导入授权；允许导出的 guest 结果限 schema/大小，丢弃模型原始路径/错误/秘密，不复制整盘或 auth。

provider 对机检事实负责，owner 只签认其能证明的镜像/权限/凭据/endpoint 决定，外层调用方确认销毁；三者不可互相代签。公开回执和报告不含个人路径、宿主别名/家庭网络细节、timezone/proxy 信息；精确 CIDR/解析/实际 argv 的核验记录仅在受保护的短期审计区，以摘要关联，配置不得混入个人信息。固定 guest 路径不是 owner home。持久 Stop 在 VM 外独立保存，回收/recovery/新 clone 均不清除；恢复仍仅可信真实用户事件。

## 8. 离线测试与真实证据

**原则：fake runner 只验证顺序、拒绝和有界性，不能宣称 VM/网络/原生可用。** 无真实证据的格子一直 `not_run`，不可用 attested 覆盖机检。

拟在 `disposable/tart/` 隔离适配器；注入同步 `TartRunner`、单调钟、受信证明源、guest probe 通道和持久账本。fake runner 只记录结构化命令、返回预制状态/超时/有限输出，不调用系统 `tart`、不启动 VM。生产 Command 与所有阶段铸造仍在 boundary admission；测试依赖不能提供生产 factory。

| 表驱动/变异组 | 必须断言 |
| --- | --- |
| 每条状态边与终态 | §2 每一行分别成功、命令失败、超时、畸形/超长回复、取消、回复丢失；非预期/终态后事件拒绝；成功 trace 精确 clone→带策略 start→probes→bootstrap/ACK→login→client→stop→delete→receipt |
| 每个前缀的中断 | 在分配前后/clone 成功未返回/启动/探针/owner 等待/ACK/登录/preflight/run/删除/封存处注入崩溃；只清已拥有资源；无重复 client、新 clone 不复用旧实例 |
| 五行与绑定变异 | 对全部16 Probe 分别删/重复/Failed/Unknown；改任一 Binding 字段、签认主体/期限/claims、ACK/代次/boot；每次拒绝且 client/preflight/auth 交付计数为0；验证最后一次核验期间到期 |
| 网络策略变异 | 去掉 deny/Softnet flag、扩大 CIDR、gateway 泄漏、UDP/IPv6 guard 缺失、假实际 argv、无计数、单纯 timeout、pin 到期/IP 改变；均不产 Ready |
| 准入与 A1 | 默认 feature-off、feature-on 无安装/无 session、伪造 trait/回执/env/flag、搬迁/相邻 binary、直接 Supervisor::start、错角色/guest→host/过期 capability 均零 spawn；固定 bootstrap 的正向对照不能启动模型 |
| 幂等/孤儿/评分 | stop/delete 重复、已确认不存在、查状态失败、身份冲突、非所属 VM/golden、锁忙、回执写失败；清理失败无 pass 并停后续 task/arm/repeat；恢复回执不复活旧评分 |
| 源码守卫 | 保留 `command_new_is_only_in_the_admission_module`；扫描 gcu-eval 生产源码及 gcu-cli/xtask 的 eval 接线：Command alias/全限定构造、AdmittedLaunch 与 session/阶段能力的铸造仅允许 admission；test-support mint 不得进入生产安装，不能借普通产品启动入口绕行 |

纯状态机用假钟无真实 sleep；每个阻塞/IPC/子进程/锁等待必须有明确上限。集成测试各在独立测试进程，外部 wall-clock watchdog ≤30s kill/reap，整组 ≤5min；不得只用与被堵锁同一 executor 的 timeout。锁用 try-lock+截止，持锁不做 IO/等待/析构重活；注入永不释放锁也须 watchdog 报失败退出，防止重演“锁测试挂4小时”。不为测试另造常驻 observer 或无人持有的工作线程。

本次文档检查只运行 `python3 -I docs/plans/computer-use-v2/reviews/check_plan.py`；上述 Rust 测试是实施计划，**未编译、未执行**。真实 clone/start、Softnet/外部 guard 强制、真实 egress 探针、TCC、专用 helper/S12 活身份/FD 移交/ACK、真实父死/整树/宿主重启回收、原生 Stop、Grok 登录与真实模型全部 `not_run`，直到 owner 安装 Softnet、准备 guest、前置卡合入且对应证据齐全；fake 通过不能改变矩阵 native 状态。

## 9. 实施拆分与 owner 决定

**原则：每步一个 Grok session、独立可审，阻断项未解决不进入依赖步骤。** 每步都保持默认门关闭；不把实现合入视为真实运行授权。
H1/H2 的具体组件若需另造通道或包过滤器，先拆独立设计卡，不能把未选定的基础设施塞进一个实现 session。

| 顺序 / 小卡 | 一次 session 的交付与审阅重点 | owner 动作 |
| --- | --- | --- |
| S11a | 固化安装/guest 通道接口、威胁模型与 fake trace；确认 H1 的具体 Tart 接线，不能实现则给出阻断结论；审身份绑定、无通用执行/转发 | 确认 golden 来源及通道方案；无需登录 |
| S11b（a后） | 实现 fake runner+状态机+账本+限时清理；逐状态故障/锁 watchdog；审无后台生命周期泄漏、幂等删除与 golden 保护 | 无 |
| S11c（a/b后） | endpoint pin/Softnet argv/宿主 guard 适配与 fake 探针；审 H2、gateway、UDP/IPv6、无代理继承；真实网络仍 not_run | 安装 Softnet；批准具体宿主策略变更后才操作，不擅改网络 |
| S11d（c后） | 五行证据、可信 owner 签认、回执/脱敏/期限与假凭据测试；审不可从 JSON/路径造权威 | 确认 endpoint、登录出口及短期凭据可行性，无需交密码 |
| S11e（d后；依赖 S4+S12） | admission 阶段能力与 runner guest 接线、A1/源码守卫；审 bootstrap 循环、角色不可转换、默认构建拒绝和 ACK 来源 | 准备无凭据 golden；在准备副本为固定 helper 点 Accessibility/Screen Recording，确认其它守卫权限 |
| S11f（e后，单独验收 session） | 无模型真实 clone/探针/身份/Stop/故障清理，归档回执；审实测能否覆盖五行与矩阵 §3；不运行模型 | 提供准备好的 guest；必要的权限/真实用户恢复；审阅回执 |
| S11g（f后，单独运行 session） | 满足矩阵 §8 后按批准任务运行一个 attempt；审凭据/时长/teardown 与评分发布 | 审阅一屏摘要、自己完成登录链接；仅在所有门槛齐全时运行 |

尚需 owner 回答（不要求在本文编写期间操作）：

1. `macos-s2-eval` 能否封存为无个人数据、无登录态、禁止备份的 golden？固定 helper 更新后是否接受重做权限准备/签认？
2. 精确的 Grok 模型/设备登录 endpoint 是哪些，能否取得 ≤30min 且无 refresh token 的单一 eval 身份？未回答前登录/真实模型保持 `not_run`。
3. 是否允许在专用评测上下文安装宿主出站 guard（不改变日常应用网络），并做无模型否定探针？具体实现必须先给 owner 审核；H1/H2 任一无法证明时暂停 S11，不开宽网络。

## 10. 总控审阅补充（2026-11-01）

**原则：先实测，再实现；未经实测的基础设施不进入实现卡。** 本文的 H1（宿主到 guest 的私有控制通道）与 H2（宿主出站 guard 对 UDP / IPv6 / gateway 的覆盖）是对 Softnet 与 Tart 行为的假设，可以直接测，不必推断。因此在 §9 的 S11a 之前增加：

| 顺序 / 小卡 | 交付与审阅重点 | owner 动作 |
| --- | --- | --- |
| **S11-0**（先于 S11a，无代码） | 在 owner 完成 Softnet 授权后，对一份一次性 clone 做**无模型**的实测并归档为短报告：①Softnet 默认策略下 guest 能否到达 gateway 上的服务（宿主 DNS/SSH/其它端口）、能否走 IPv6、能否发 UDP/QUIC 到放行地址；②加 `--net-softnet-block=0.0.0.0/0 --net-softnet-allow=<模型端点 /32>` 后上述每一项的结果，以及宿主 → guest 的控制通道（ssh/其它）是否仍可用、需要放行什么；③`tart` 是否提供不经 IP 网络的宿主 ↔ guest 通道（H1）。结论只记事实，不记推断 | 无（Softnet 授权已由 owner 做）；实测期间不改宿主网络/防火墙设置 |

决策规则：若实测显示 Softnet 单独已满足 §5 的拒绝类别，则 §5 第 3 点的“宿主网络 guard”不进入范围；若不满足，**先把具体 guard 方案（影响范围、回滚、是否只作用于该 Tart/Softnet 实例）提交 owner 审阅，批准前不安装任何网络规则**。若 H1 无可用通道，暂停 S11 并回到 owner，不为凑通道开放 gateway 上的 IP 服务。

另：S11a–g 工作量较大。首个可验收目标是 **S11f（无模型的真实 clone / 探针 / 身份 / Stop / 清理）**；S11g（真实模型）在其回执齐全并经 owner 审阅前不排期。每一步评审最多三轮，超限按单一原则简化（本项目在 S4 退役机制上刚经历过一次：观察者 / 线程 / 所有者生命周期类机制优先改成同步、调用方持有、有界的做法）。

## 11. S11a 离线接口状态

状态：接口与 fake 已落地（2026-11-01）。这不是 provider，不改变 S6 门禁，不批准真实运行。

已定义（`gcu-eval/src/disposable/tart/`，每个构建都编译；没有 `tart-disposable` feature，没有 env/CLI/config 启门）：

- `TartCommand`、`RunId`、`CloneId`、`GoldenImage`、`SoftnetFlags`：固定操作。身份拒绝空、超长、路径、`--` 和 golden 名前缀。Softnet 只有固定 block `0.0.0.0/0` 加最多 16 个公网 IPv4 `/32`，没有自由字符串。公网校验用一张显式前缀表拒绝非全局和特殊用途地址，包括 `0/8`、`10/8`、`100.64/10`、`127/8`、`169.254/16`、`172.16/12`、整段 `192.0.0.0/24`（含 `192.0.0.1`、`.8`、`.170`、`.171`，不把该段里的少数全局例外当成公网）、`192.0.2/24`、`192.88.99/24`、`192.168/16`、`198.18/15`、`198.51.100/24`、`203.0.113/24`、`224/4`、`240/4` 和 `255.255.255.255`。地址语法通过不证明策略已安装，也不代替 S11c 的 owner 批准、网关排除或探针。
- `TartRunner`：`clone`/`start`/`stop`/`delete`/`list`/`status`。每个调用带有上限的 deadline，返回至多 1MiB。`clone` 消费 `ClonePermit`。有效截止是调用截止和 permit 绝对截止中较早的一个；到达或超过绝对截止时仍然消费 permit，返回过期，且不消费脚本化回复。已到期的 deadline 只在入口前拒绝，不是对已开始操作的一般取消。
- `HostClock`、`FakeClock`：单调钟注入。fake 前进不睡眠。
- `AttemptLedger`、`FakeLedger`：`reserve` 先写入 run id、clone id、绝对截止，才返回一次性 permit。`query`/`append`/`seal` 按完整 id 精确匹配，没有前缀查找。`FakeLedger::reserve` 产出的 `ClonePermit` 只是离线 fake；未来真实适配器不得把持有 permit 当作持久登记的证明，须经可信账本或实例核验。
- `GuestChannel`、`FakeGuestChannel`：版本 1 的 inventory/probe/bootstrap/login/run/lease/revoke。没有地址、路径、套接字或密钥字段。已到期的 deadline 只在入口前拒绝，不是对已开始操作的一般取消。
- `InstallManifest`：程序、golden、策略三份 32 字节摘要，只是数据。摘要相同不建立信任或安装事实。
- `FakeTartRunner` 记录结构化命令，并可注入成功、失败、超时、畸形、超长。不调用系统 `tart`，不创建进程。

未决，本卡不选择：

- **H1**：宿主到 guest 的具体通道未定。接口不假设 SSH、virtio、共享盘或网关端口。取决于 **S11-0**。
- **H2**：宿主出站 guard 是否需要、以及哪一个实现，取决于 **S11-0**。`SoftnetFlags` 只描述 §5 的字段形状，不证明策略生效。
- 状态机（S11b）、真实适配器、网络探针（S11c）都没有实现。长操作的完成等待，以及有界轮询或让出以保证 ≤1s 续租和 ≤3s guest 存活，留给 S11b 明确。固定操作可以只确认已经发起，由有界轮询取结果。本卡不为此选择传输、后台线程或状态机。
- `accepting_provider_linked` / `scoped_provider_ack` 仍恒拒绝。本模块不铸造 `AdmittedLaunch`。

S11b 在 S11-0 给出通道事实之前只应接这些 fake，不得选定真实传输。fake 通过不改变矩阵的 native 状态。

## 12. S11b 状态

状态：调用方顺序驱动的同步状态机与账本清理已在 fake 上落地（2026-11-01），并按当轮评审收紧所有权、停止证明、租约和截止（同日）。这不是 provider，不铸造 `AdmittedLaunch`，不改变 S6 门禁，不批准真实运行。§11 里“状态机尚未实现”是 S11a 当时的记录；本节覆盖该句。

已实现（`gcu-eval/src/disposable/tart/`，无后台线程、无观察者、无 async、无新依赖）：

- 阶段：`Idle → Reserved → Cloned → Booting → Isolated → Bootstrapped → CredentialReady → Ready → Running → Revoking → Destroying → Final`。`Quarantined` 是清理或回执失败，停后续 task/arm/repeat。`Suspended` 在调用入口看到 `lease_until` 已过时同步进入，也在 stop 之后、delete 之前（以及之后每一次破坏性派发之前）再次检查到租约已过时进入。进入是状态决定，不依赖 IO。可选的 revoke 和 `CleanupUnknown` 只在触发这次检查的调用截止仍有剩余时才派发；没有剩余就跳过，并标记需要恢复。禁止该 owner 删除，不封存，回执 cleanup 为 unknown，下次启动先 `recover`。没有计时器，也没有自动恢复。进入之后，arm、run、revoke、destroy、seal 都不能把阶段改回或续租。
- 每步一个同步方法，调用方持有 runner、guest、账本、锁和钟。`Drop` 不 stop、不 delete、不 seal。runner、guest 和账本调用只经过一个派发函数：取调用方给出的截止、这一调用的阶段预算结束、attempt 绝对截止、以及围住该阶段的聚合结束中最早的一个。准备聚合只围住 reserve 到 ready；run 和租约不被已经结束或仍在将来的准备阶段截短。清理聚合只围住 revoke、destroy 和 seal；第一次 revoke 在派发前记下清理起点，因此也受清理结束约束。恢复只用本次调用的 deadline，不用已死行的绝对截止。调用方 helper 不自己构造 deadline。已到期入口上的可选 IO 不再另铸 `now + 预算`。读回使用同一次调用的剩余截止。stop 之后的 status 不超过 stop 的有效截止；delete 之后的缺席确认不超过 delete 的有效截止。阶段预算集中在 `budgets.rs`（§2 的上限），测试只能缩小。Stop 常量未改。
- 所有权：fake 在 clone 派发之前把期望的化身和配置写入预约记录，成功正文必须与之相同，不一致不会覆盖已记下的期望。回复丢失后，恢复认领的是这一化身，并拒绝同名替换者。这不是真实身份源。同名目的地在 reserve 和 clone 之前拒绝。start、stop、delete 和恢复在动手前要求权威清单里的 run、化身和配置都与记录一致；缺失或歧义进入隔离，不按名字或单独的 run 标签认领。没有化身记录时，不按名字删除。guest boot 变化使本次分数失效，但清理仍可销毁已经正向识别为自己所有的 VM。
- 停止与缺席：`state=absent` 不是进程已退出的证明。清单里没有该 clone 也不等于 tart 和 softnet 已退出。VM 一旦发出 start，若没有独立的状态报告证明二者都已退出，就不得 `cleanup_confirmed`，结果是 `CleanupFailed` / 隔离，并且不再按这个名字 stop 或 delete。从未 start 的缺席仍可确认，并且同样不再按名字 stop 或 delete。tart 或 softnet 仍为 up 的 absent 正文是畸形。`stopped_clean` 仍要求 `Stopped` 且 tart 与 softnet 都已退出；只剩 softnet 存活不是停止证明。
- `Bootstrapped` / `CredentialReady` / `Ready` 只推进状态并记录证据。login/run 仍是 fake。不选 H1 传输，不安装 H2 guard。
- 清理：非预期转移（含 `Revoking` / `Destroying` 上的 run、start）记录 `Unexpected` 并永久 `not_run`。超时和故障进入 `Revoking → Destroying`。已启动的 VM 要先有停止证明，再 delete，再由权威 list 确认不存在，然后封存。查询失败或不完整不是“已消失”。只处理账本里的精确 clone id；不按前缀删除，不删 golden 或外部 VM，不重启孤儿。期望身份的登记先于 clone 派发。边矩阵是每条正向边上的一种故障注入，不是每个中断前缀。已覆盖的中断后续是 stop 之后、delete 之后、以及隔离事实追加之后的 `Drop` 再恢复；没有声称每一个前缀。
- 恢复：`recover` 只读未封存行，先追加 `RecoveryNoted`，再按记录中的化身 stop/delete。start 已发出而清单没有该名字时，不把缺席当成进程退出，也不按名字删除。它用本次调用的 deadline，不用已死行的绝对截止，否则绝对截止已过的孤儿无法删除。旧 attempt 仍永久 `not_run`，不恢复分数。带 `CleanupUnknown` 的行记 `OwnerDeath`，其余未封存行记 `HostCrash`。没有化身记录时，同名 VM 不删。
- 环境违约、缺证明、边界崩溃、清理阶段的非法调用一律 `not_run`，即使 fake 曾返回 pass。只有证据完整、身份未变、且清理确认成功才保留分数。回执只记录当前原因；原先的 provisional 分数和第一因的保留留给 S11d。例如 provider 崩溃之后清理又失败，发布出去的是 `CleanupFailed`，不是两条原因。丢失的 reserve 确认若读回发现行已写入，回执 `recovery_required` 与下一次 reserve 的 `MustRecover` 一致；读回确认未写入则不要求恢复。seal 的确认丢失但读回发现行已封存，视为封存成功，不发布 `Quarantined(ReceiptFailed)`。隔离事实本身的 `CommittedLost` 仍留下未封存行，下一次 reserve 必须先恢复。

仍依赖后续卡，本卡不实现：

- **S11-0**：H1 通道与 H2 guard 的实测。未选定传输和 guard。
- **S11c**：真实探针、Softnet argv、宿主 guard。当前探针只消费 fake 回复。
- 真实 Tart/Softnet、登录、模型、native Stop、TCC、父进程死亡和宿主重启仍是 `not_run`。fake 通过不改变矩阵的 native 状态。

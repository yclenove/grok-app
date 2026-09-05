# M0 Runtime 可行性与原生安全边界

**状态：** 可交付实施的分规格；实现、签名包与真机验收尚未运行。
**关联：** [总体设计](../2026-09-04-browser-rearchitecture-design.md)、[共用协议](00-contracts.md)、[实施计划](../../plans/2026-09-05-browser-rearchitecture/m0-runtime.md)。
**边界：** M0-R01 至 M0-R06 提供继续 M1 的证据，默认产品仍只开放 Preview。

## 文件职责与当前代码证据

| 文件 | 当前事实或计划职责 |
| --- | --- |
| `src-tauri/src/side_browser_host.rs` | 现有系统 WebView；`create` 接 page-load/download，未包含 Managed Supervisor。 |
| `src-tauri/src/commands/terminal.rs` | 现有 `side_browser_*` command；不是未来 worker 的授权接口。 |
| `src/lib/api/system.ts` | 现有 Preview invoke 包装；不能直接成为 Agent 输入通道。 |
| `src-tauri/src/process_util.rs` | CLI 辅助会补 HOME/PATH；Managed 不复用这套环境继承。 |
| `src-tauri/src/store_lock.rs` | 已有 fs2 锁与 `write_bytes_atomic`，需验证目录 durability 与跨进程生命周期。 |
| `src-tauri/src/paths.rs` | 已有 app-data resolver，新 Browser 根从此派生。 |
| `src-tauri/Entitlements.plist` | 当前 App entitlement 范围较宽；helper 需要各自最小集合。 |
| `.github/workflows/release.yml` | 当前签名可选；PoC 不能被当成官方 Managed 发行资格。 |
| `src-tauri/src/browser/input/` | 新建：原生输入归属、权限、健康及事件合同。 |
| `src-tauri/src/browser/runtime/` | 新建：guard 通道、容器、ledger 与 PoC 验证。 |
| `src-tauri/src/bin/browser-profile-guard.rs` | 新建：独立守护进程入口，持有 Profile 锁。 |
| `browser-runtime/worker/` | 新建：固定 Playwright worker、原语取消探针。 |
| `browser-runtime/fixtures/` | 新建：确定性网页、进程树和事件屏障。 |
| `scripts/browser-runtime/` | 新建：tuple 构建验证与原生矩阵 runner。 |

表中新路径均为计划交付，不表示已经存在。M0-W01 独占共享 schema、Rust/TS 类型与协议 fixture。

## 目标与非目标

1. 证明 macOS arm64/x64 和 Windows x64 上可归属 Managed 窗口输入，并及时撤销整个 Runtime 的控制。
2. 证明 Host 异常退出后仍有独立 ProfileGuard 清理 descendants，且旧 Profile 不会过早再次可写。
3. 构造离线固定 tuple，验证私有 pipe、签名、目录布局、Chromium sandbox 和健康探针。
4. 明确持久 Profile 静态凭据保护、备份行为、文件权限及 worker 文件访问的真实边界。
5. 量化每个声明 primitive 在 actionability 等待、执行和取消竞态中的行为。

M0 不交付默认启用的 Managed UI、第一方 Browser MCP、独立 updater、Chrome Connector 或 Linux 支持。
PoC 不使用用户登录站点，不拷贝用户 Chrome Profile，不把实验 flag 作为生产权限绕过。
不将 synthetic DOM input 或仅在开发构建成功的监听结果认作真机过门证据。

## 组件与所有权

Host 持有 `hostBootId`、ControlDomain registry、Provider health 与原子 fence 决策。
Provider 只能报告原生事件和归属证据，不签发 lease，不解析网页 selector，不读取键入文本。
ProfileGuard 在 Host 之外存活，直接创建 worker，独占 Profile 锁与 descendant container。
worker 只有 Host initialize 后才允许启动 Chromium，stdout 仅发送长度帧协议。
fixture runner 通过 Host 测试入口操作专用 Runtime，并以 run nonce 限定故障注入对象。
原生输入日志只记录事件类别、Host 接收时间、匿名窗口引用和归属结果。

```text
fixture runner -> Host PoC facade -> NativeInputFenceProvider
                               -> independent ProfileGuard
                                  -> contained worker -> Chromium private pipe
```

`controlDomainId`、`runtimeId` 和 OS identity 是 Host 内部派发信息，不扩张 RequestEnvelope。
公开请求使用 `00-contracts.md` 的 camelCase；Rust wire 类型统一 `serde(rename_all = "camelCase")`。
protocolVersion=1；generation/revision/fence 为 u32 number 且禁止回绕，发行/策略 sequence 为十进制 string。
PoC report 引用 requestId、tabRevision/navRevision 与 artifactIds，不暴露路径或原始输入。
origin 始终从 Host 绑定的页面/frame 计算，PoC 请求也不能自报可信 origin。

## 原生输入与 Provider 健康

macOS 候选为 listen-only CGEventTap，注册前探测 Input Monitoring 权限。
窗口归属由已验证 PID、进程 start time、runtimeGeneration 与当前窗口集合共同决定。
App Preview 可在原生分发路径处理输入；Managed 需要对独立 Chromium 窗口归属。
Windows 候选为专用线程 Raw Input，必要时增加低级 hook，并映射 HWND/PID。
Windows 实验必须在普通用户权限下完成，不申请管理员或 uiAccess。
窗口创建、销毁、重新分配、PID 复用与 generation 变化触发登记刷新或立即阻断。

| 输入/故障 | M0 必须记录的结论 |
| --- | --- |
| pointer down、wheel、key down、touch | 是否在实际声明设备/OS 上覆盖、是否归属正确 Runtime。 |
| IME 与组合输入 | 原生输入范围、焦点变化和 fence 是否先于后续 Agent 原语。 |
| hover | 不触发用户接管，不读 pointer movement 内容。 |
| Playwright/CDP 输入 | 不自触发用户接管；Gateway 禁止 OS 输入注入。 |
| 远程桌面、辅助输入 | 能观察到的原生事件保守归属外部控制。 |
| AX 操作无原生输入 | 不承诺自动捕获，保留显式接管要求。 |
| 权限拒绝/撤销、tap disabled、消息线程失效 | fence、暂停观察、`blocked_input_attribution`。 |
| sleep/wake、焦点切换、窗口身份未知 | 健康重新建立前不签发 writer lease。 |

Provider healthy 不等于“刚才有事件”；必须有监听注册状态、权限有效性与窗口归属可验证性。
macOS tap 被系统禁用、Windows 消息线程停止时健康立即退化，不能只依赖超时心跳。
恢复监听只恢复健康，不能清空 `TakeoverLatch=user`；必须由可信 App UI 显式交还。
参考指标起点是 Host 收到已归属原生输入，终点是拒绝新命令且 worker 确认 fence。
p95 目标为 250 ms，至少 30 个样本；额外报告 max、样本量与调度压力。

## Fence 与 Playwright 取消证据

Host 收到首个有效输入，在一个临界区内递增 fenceEpoch、锁存 user、撤销 lease、关闭 admission。
随后取消 inflight、清空队列、暂停 DOM/截图/Console/Network 观察，记录脱敏决策。
worker 在每个可观察或有副作用 primitive 前、每次显式 await 后与重试前再次检查 fence。
Playwright 自身的 locator auto-wait/retry 位于库内，外层 await 检查不能取消已派发动作。
每种动作必须明确采用可证明的取消手段：取消该调用，或关闭所属 page/context/connection。
若关闭共享连接才能取消，必须把整个 ControlDomain 标记 disconnected，并禁止其他 Session 重连续跑。
仅使用 Promise.race 超时不算取消底层动作；被丢弃的 Promise 可能继续操作网页。
取消与 primitive 开始的竞态返回 `unknown_outcome`，所有后续 primitive 执行数必须为零。
如果固定 Playwright 版本不能保证某类动作取消后不再开始，该能力保持 unsupported。
已发生的网页外部副作用不能回滚；报告只承诺封住后续动作，不能承诺收回已发送数据。

PoC 固定场景包括延迟显示按钮、持续遮挡、frame 导航、输入前切换 origin、下载开始和上传读取。
fixture 使用服务器屏障精确停在 actionability 等待期间，再触发原生输入并解除屏障。
页面收到的提交计数与服务器请求计数是动作 oracle，不能只根据 worker 返回值判断。
DOM、Console 和 Network 的内容读取也属于 primitive，fence 后不得继续观察秘密步骤。

Playwright v1.55.0 仅作为可复现的源码分析基线，不代表已获生产安全批准。M0-R01 在实施时选择仍受上游支持的 Node/Playwright 生产候选，锁定精确版本及上游 Chromium revision，记录选择依据与已知安全通告；候选必须通过 M0-R05/R06，升级版本须重跑取消与 reader-lifetime 门。上传采用 path-based `setInputFiles()`，保留 200 MiB 上限，不将受限 buffer 模式冒充大文件方案。
分析基线 v1.55.0 的 [`server/dom.ts`](https://github.com/microsoft/playwright/blob/v1.55.0/packages/playwright-core/src/server/dom.ts) 路径分支经 [`chromium/crPage.ts`](https://github.com/microsoft/playwright/blob/v1.55.0/packages/playwright-core/src/server/chromium/crPage.ts) 调用 `DOM.setFileInputFiles`；返回仅是命令确认，不能证明 Chromium 已复制完整字节。M0-R05 必须在实际锁定候选上重验该语义、buffer 限制与晚读行为。
M0-R05 构造 user-only、不可变私有 spool，在路径派发前持久绑定 `UploadSpoolLease` 与完整 Runtime run identity。
路径一经派发，直至所属 Runtime 全部 Chromium/worker descendants 退出有证据前保留副本；CDP ACK、worker 句柄释放、input 清空、导航或 tab 关闭都不是删除证据。
fixture 在 ACK 后及撤销后延迟 `FileReader`、slice/stream 与人工表单提交，覆盖 1 MiB、50 MiB 边界、200 MiB 和超限拒绝；校验完整 hash、read error 与服务器字节数。
接管立即 fence Agent，保留手动浏览器与已披露 File 对象的读取；无法收回已经授权给页面的字节。
未派发副本可在撤销后删除；已派发副本在 revoke/turn/session close 时转 `cleanupPending`，继续计入全局 2 GiB 预算。
Guard/Host crash 用 durable spool ledger 与 descendant exit proof 恢复清理；未知存活身份不得按 TTL 删除，也不重新派发文件。
证明 Runtime 停止后的 no-follow purge、删除失败保留配额、共享 Session 关闭不误杀 Runtime，结果列入逐平台 go/no-go。

## ProfileGuard 与容器

ProfileGuard 是单独签名的原生 binary，不能是 Host 子任务或 worker 内线程。
Host liveness 使用专用私有 pipe；EOF 不被 heartbeat、owner.json 或 TTL 代替。
Guard 先锁 reconciliation，再核对 run nonce/ledger，再持有 Profile lock，最后创建 worker。
没有旧 descendants 已退出的证据，返回 `reconciliation_required`；不得按进程名 broad kill。

macOS Guard 位于受控 PGID 外；worker 在开始执行前进入独立 process group。
Guard 记录 PID、start time、executable identity；TERM 后有界等待，再 KILL 并 reap 直接 child。
对于改变 process group 的 descendant，按 ledger 的完整身份核对并终止，禁止只凭 PID。
若平台无法证明所有 descendant 已退出，Profile 保持不可写且 PoC 不过门。
Guard 意外退出而 Host 仍运行时，Host 的 guard-exit watchdog 立即 fence 并持 reconciliation lock，按已核对 ledger 执行同一退出证明；不能因 OS 已释放 Guard 锁就允许再次写 Profile。

Windows 只有 Guard 持有 non-inheritable Job handle，并设置 KILL_ON_JOB_CLOSE。
worker 必须 `CREATE_SUSPENDED` 创建，成功 AssignProcessToJobObject 后才 ResumeThread。
禁止 breakaway，验证 Chromium sandbox/utility descendants 均在 Job 内。
Assign 失败、意外 handle 继承、nested Job 不兼容都终止 suspended worker，并保持 fail closed。
Guard 自身退出必须导致 Job teardown；Host 退出由 liveness EOF 驱动同一清理路径。

顺序固定为 admission close -> fence -> bounded drain -> TERM/Job close -> KILL -> exit proof -> unlock。
测试结束 30 秒后本 run descendants 必须为零，同时用户自己的 Chrome 保持运行。
两份 App、dev/latest/stable 共用根时，第二 claimant 返回 `profile_busy` 或 `reconciliation_required`。
锁的生命周期不依赖 React pane、Host 窗口或 AppSession 是否仍然显示。

## 固定 Tuple 与存储 PoC

tuple 固定 Node patch、playwright-core、Chromium revision、Guard、协议、target triple 与 capability。
签名 manifest 记录 source commit、license、SBOM、每文件 hash、大小、Profile epoch 与发布 sequence。
构建时可获取依赖；测试安装后的 Agent Browser 首次启动不允许下载 Node/Chromium。
worker executable 使用已验证绝对路径、私有 cwd、显式 env allowlist 与关闭无关 handle。
不继承 agent home、API key、proxy secret 或 shell 启动参数；代理凭据另走 scoped secret channel。
env allowlist 和最少路径传递是权限最小化，不等同于 Node 已被 OS 文件系统 sandbox 隔离。
M0 必须测量 Node/Chromium 对 fixture 根外路径的可见性，声明实际威胁边界及限制。
若产品要求依赖无法建立的 OS 隔离，相关 capability 保持关闭，不能靠文案宣称隔离已成立。
Chromium sandbox 不可通过 `--no-sandbox`、禁用安全 feature 或扩大 entitlement 绕过。

提取先校验签名/平台/hash，再在 no-follow staging 解包；拒绝 traversal、hardlink 和 device file。
只允许 manifest 精确声明的 macOS Framework 相对 symlink，每一跳留在 tuple 内且无循环。
普通 symlink、链接父目录逃逸、数量/体积超限和未声明文件均拒绝。
提取完成逐文件与平台签名复验，原子发布 immutable 目录；失败不运行 helper。
最高 sequence/revocation floor、LKG 正式激活由 M1 实施，M0 固定反例 fixture 与验证格式。

Browser 数据根由 app-data resolver 派生，UUID manifest 使用 atomic JSON；本路线不引入 SQLite。
逐平台检查 user-only mode/ACL、符号链接替换、备份包含/排除及凭据静态保护。
Windows DPAPI/macOS Keychain 的可用性必须来自实际 Chromium 构建和安装身份测试。
持久 Profile 保护无法证明时，M1 仅启用 ephemeral；默认 project Profile 仍被 eligibility gate 阻断。
ephemeral 结束后也需等待 Guard/lock/reference 全部结束才能 no-follow 清理。

## 迁移、恢复与容量基线

M0 不搬迁现有 Preview Cookie/storage，不把 WebView partition 改写成 BrowserProfile。
升级只传安全 URL、viewport intent、稳定 BrowserTab identity；自动化控制不继承 Preview 状态。
Host 重启生成新 hostBootId、Runtime generation；旧 token/lease/queue 均不恢复。
PoC 可以恢复脱敏诊断 metadata，不能恢复 Agent 自动动作。
tuple/锁/Provider 失败仍保留人工 Preview，并返回具体 reasonCode。
采样 cold start、new tab、RSS、CPU、磁盘与包体，记录参考设备和精确 measurement boundary。
默认两个 ProfileRuntime 是待 M0 基线验证的起始上限，不复用 agent process-pool 配额。

## 验收与风险决策

| 交付任务 | 必需证据 |
| --- | --- |
| M0-R01 | 三 target tuple 锁定清单/构建验证器、恶意 archive corpus、固定网页/进程 fixture。 |
| M0-R02 | macOS arm64 与真实 Intel/x64 VM 的权限、输入、sleep/wake 记录。 |
| M0-R03 | Windows x64 普通用户、HWND/PID 变化、IME、权限/线程故障记录。 |
| M0-R04 | Host/Guard/worker/Chromium 崩溃和双 claimant 的 descendants=0。 |
| M0-R05 | credential-at-rest、备份、ACL、每 primitive 取消表及 200 MiB 延迟读取/spool 退出后清理证据。 |
| M0-R06 | 纳入 R04 Guard/R05 worker 的完整签名 tuple、每目标三轮连续通过、容量基线与逐平台 go/no-go。 |

未来证据根为 `docs/qa/browser-rearchitecture/m0/`，每次记录 tuple digest、OS、架构、设备类别与测试版本。
报告不含私钥、用户路径、登录数据、原始输入、未经脱敏 stderr 或完整 URL query。
证据缺失、签名包未测或任一安全 oracle 失败时，该平台不得计入 M1 完成。
交叉编译仅证明编译；synthetic input 仅测试状态机；二者均不能替代原生安装包验收。
M0-W06 Preview 验收与本包共同构成 M0 门，本规格不将未来验收描述为已通过。

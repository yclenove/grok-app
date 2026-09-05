# M1 Managed Runtime、Profile 与 Artifact

**状态：** 实施规格；产品代码、迁移和验收尚未运行。
**关联：** [总体设计](../2026-09-04-browser-rearchitecture-design.md)、[共用协议](00-contracts.md)、[M0 Runtime](m0-runtime.md)、[实施计划](../../plans/2026-09-05-browser-rearchitecture/m1-runtime.md)。
**边界：** M1-R01 至 M1-R06 交付 macOS arm64/x64、Windows x64 的可监管 Managed Runtime。

## 文件职责与当前代码证据

| 文件 | 当前事实或计划职责 |
| --- | --- |
| `src-tauri/src/side_browser_host.rs` | 现有 Preview 与按 URL 查 pending download 的实现，不能提供 Managed 身份隔离。 |
| `src-tauri/src/media_server.rs` | 现有 process token + path_scope；Browser Artifact 必须增加独立 capability 验证。 |
| `src-tauri/src/path_scope.rs` | 现有本地文件 allowlist，不能代替 BrowserSession/Artifact owner。 |
| `src-tauri/src/store_lock.rs` | 已有 atomic JSON 写入 helper；复用而不新增 SQLite。 |
| `src-tauri/src/paths.rs` | 现有 app-data root；新 Browser 数据布局由 resolver 派生。 |
| `src-tauri/src/lib.rs` | 现有模块注册与 Exit hook；只接领域服务生命周期。 |
| `src-tauri/src/browser/runtime/activation.rs` | 新建：tuple admission、sequence、canary 和 LKG。 |
| `src-tauri/src/browser/runtime/supervisor.rs` | 新建：Runtime registry、容量、引用与 drain。 |
| `src-tauri/src/browser/profiles/` | 新建：Profile generation、迁移、锁和 orphan 生命周期。 |
| `src-tauri/src/browser/artifacts/` | 新建：immutable blob、owner、token 与 reconciliation。 |
| `src-tauri/src/browser/transfers/` | 新建：上传一次性披露、Runtime-owned spool lease、下载 staging/export 与清理。 |
| `browser-runtime/worker/src/` | 新建：framed transport、Managed adapter、原语与 fence。 |
| `src/components/browser/runtime/` | 新建：读取 Host runtime/profiles/transfer 投影的 UI adapter。 |

M0 已创建的模块在 M1 修改；若按本文单独执行，应先完成 M0，不把计划路径当成现有代码。
M1-G01 提供 Gateway 接口；权限决策、origin、binding/lease 所有权归 Gateway 包。

## 目标与非目标

目标是让合法 Gateway 请求启动固定浏览器、执行受控 primitive、取得 opaque Artifact 并完整清理。
人工可见窗口、Profile 登录态、崩溃状态和恢复路径必须与 Workbench 同步。
不可确定副作用必须报告 `unknown_outcome`，不自动重试网页动作。
不在 M1 提供 Linux、Chrome Connector、独立 Runtime 下载器、CEF 或任意 Full CDP。
不迁移用户 Chrome Cookie，不开放任意本机文件，不把 env 过滤描述成 OS sandbox。
Full CDP 默认 unsupported；单次审批不能豁免接管、语义闸门、脱敏和独占 Runtime 条件。

## 领域所有权与数据流

BrowserSession 引用一个 ProfileRef，Managed Profile 同时最多一个 Runtime，可被多 Session 引用。
Supervisor 按 profileId + profileGeneration 复用 Runtime，任意时刻只有一个 Agent writer。
Runtime registry 的 target 映射固定为 `(runtimeGeneration,targetId) -> (browserSessionId,tabId,tabRevision)`。
popup 先由 Host 按 opener 分配 ownership，再允许观察或动作；未知 opener 进入隔离状态。
隐藏 UI 只 detach 投影；关闭 Session 释放自己的 target/ref；最后一个普通 ref 才 drain Runtime。
已派发 upload spool 是 Runtime-owned 清理义务，不增加阻止 drain 的普通 ref，也不把已关闭 Session 重新激活。
Runtime crash、worker 重启和 backend 重绑均递增 generation，不能借 targetId 复用旧身份。

```text
Gateway validates RequestEnvelope
  -> Supervisor resolves current Profile/Runtime/binding
  -> worker validates internal lease/fence dispatch
  -> Playwright private pipe -> target/frame
  -> Artifact ingest -> Host result + workbench projection
```

请求统一为 `00-contracts.md` 的 RequestEnvelope；不在 adapter 增加 caller-origin 信任入口。
Host 派发上下文可另含 controlDomainId 与 scoped staging handle，但不能回传任意路径给 MCP。
结果固定包含 status、requestId、reasonCode、tabRevision、navRevision、artifactIds。
Rust JSON 使用 camelCase；managed runtimeGeneration 必填，Preview 为 null。
Host 校验 expectedTabRevision/expectedNavRevision，worker 事件带当前 generation/revision。
大 payload 不走 RPC；stdout 只含有界帧，stderr 只进入脱敏有界 ring buffer。
控制帧为 4 字节大端长度加 UTF-8 JSON，最多 1 MiB；每 tab 队列最多 32 条，事件背压最多 256 条。
普通动作 deadline 为 30 秒、导航最多 60 秒，并取 lease expiry 的较早值；超限后按派发状态取消或记 unknown_outcome。
protocolVersion 固定为 1；revision、generation、fence 均为 u32 安全整数，溢出必须关闭 admission，不能回绕。

## Tuple 激活与防重放

Runtime store 独立于 `browser/v1` 用户数据；完整 tuple 验证后 immutable。
manifest 固定 Node patch、Playwright、Chromium revision、Guard、protocol、platform/arch、capability、Profile epoch。
同时记录 source commit、license、SBOM、全文件 SHA-256、字节数、App compatibility 与签名 key ID。
验证顺序为外层签名/体积 -> manifest 签名/sequence -> 平台/架构 -> 安全提取 -> 文件/签名复验。
拒绝绝对路径、traversal、hardlink、device file、reparse escape、数量/展开体积超限与未声明文件。
macOS Framework symlink 必须精确列入 manifest，逐跳验证相对路径、无环、无父目录逃逸。
任何 helper 执行前完成全部验证，失败只删除隔离 staging，不修改 current pointer。

最高已接受 sequence 和 revocation floor 使用 durable atomic record，不因 App downgrade 降低。
发行与策略 sequence 的 wire/storage 均为规范十进制字符串，按无符号整数比较，不转换成 JS Number。
相同 sequence 仅允许相同签名摘要的幂等读取，冲突 payload 拒绝。
显式 rollback 需要更高 sequence 的签名指令，精确授权旧 tuple，且不能低于 revocation floor。
LKG 只选当前签名策略允许且已通过 canary 的 tuple；本地一次自动恢复不接受旧发行指令。
切换前完成 disposable Profile initialize、blank-page screenshot、close/descendant-zero 探针。
成功后原子切 current pointer；活跃 Runtime 仍固定原 tuple，ref 归零前不可修改或回收。
失败 quarantine 候选；最多自动 LKG 一次；无兼容 LKG 时 `runtime_unavailable` 并保留 Preview。
current pointer 坏损可从已验证 descriptor 重建，不能扫描版本号最高目录直接运行。

## ProfileGuard 与 Supervisor

M0 的 Guard/Job/PGID 合同全部转成生产强制条件，不能仅保留测试开关。
Guard 持有 Profile lock 与 Host liveness pipe，不属于受控 descendant container。
Windows 以 suspended worker -> assign Job -> 验证禁止 breakaway -> resume -> initialize 启动。
Job handle 仅 Guard 持有且不可继承；任何创建步骤失败先杀 suspended child，再释放资源。
macOS Guard 在受控 PGID 外，TERM/KILL、reap 与 ledger 身份核对全部完成后才 unlock。
Guard 异常退出时由仍存活 Host 的 watchdog 持 reconciliation lock 接管退出证明，阻断窗口不能依赖已经释放的 Guard lock。
reconciliation lock 序列化跨 App claimant；旧 run descendants 未证实退出时不得启动新 writer。
ledger 的 PID/start time/executable path 只用于具体身份验证，不授权按 PID/进程名强制抢锁。

worker 采用已验证绝对 executable，env_clear 后放入显式必需变量，私有 cwd 与句柄白名单。
不使用 shell/PATH/npx，不继承 agent HOME、API key 或 proxy secret。
Node 与 Chromium 的权限最小化边界依 M0 证据声明；Chromium sandbox 保持启用。
默认最多两个活跃 ProfileRuntime；同 Profile 优先复用，第三个新 Profile 返回 `runtime_capacity`。
用户可选择停止空闲 Runtime 或 Preview，不能静默驱逐活跃 Session 或上传中的 Profile。
Provider 不健康、OS 权限撤销或窗口归属不清立即 fence 整个 Runtime 并暂停观察。
Host shutdown 先通知领域 close，Host 硬崩溃由 Guard liveness EOF 清理。

## Worker 原语与取消

Managed adapter 提供声明过门的 navigation、locator、safe input、screenshot、metadata、upload/download 操作。
只支持 fixture 覆盖的 frame/Shadow DOM 子集；未知布局返回 unsupported 或 needs_takeover。
每次派发校验 hostBootId、runtimeGeneration、bindingGeneration、leaseId、fenceEpoch 和 policyRevision。
worker 对每个 primitive 前、显式 await 后、重试前重新验证，过期即拒绝。
有副作用的 compound command 必须展开成审计可定位的有限 primitive 序列。
Playwright 库内部 auto-wait 需要 M0 已证明的取消路径，不能以外层 Promise.race 伪装取消。
不能可靠中止的动作关闭所属 page/context/connection，并把范围内 Session 标记 disconnected。
取消后仍可能完成的当前 primitive 报 `unknown_outcome`，后续 primitive 必须为零。
截图/DOM/Console/Network observer 同样被 fence，交还建立新 snapshot 和新队列。
Grok Trace 只包含已脱敏结构化动作 metadata；原始 Playwright Trace 默认关闭并单独隔离存储。
原始 Trace 未经过结构化 sanitizer 与敏感 fixture 验收前，不得交付 Agent 或 export，可保持 unsupported。
writer lease 失效和新的 turn 都不能清空用户接管 latch。
frame/origin 在实际 primitive 前由 Host 当前绑定重新验证，worker 不能自行扩大 scope。

## Profile 持久化与迁移

Profile 类型为 project、named、ephemeral，ID 是随机 UUID，项目路径不是身份。
M1 默认 project Profile，但只有 M0 证明该平台凭据静态保护后才可开启 persistent eligibility。
未通过的平台仅可 ephemeral；named 数据结构保留，管理界面由 M2 交付。
Profile manifest 包含 profileId、kind、generation、epoch、runtime compatibility、owner refs 和 tombstone。
BrowserSession 删除仅移除引用，project/named 变 orphan 后继续保留，显式确认才可删除。
ephemeral 必须 Session/ref/Runtime/lock 全部结束后才 no-follow 删除 UUID 根。

新 epoch 不能原地打开旧 user-data；迁移前获取独占 lock 并停止所有旧 Runtime refs。
显式 migration 创建新 generation，复制到私有 staging，校验完整性，再用 disposable/copy 做 canary。
migration journal 记录 source/destination generation 与阶段，成功才原子更新 Profile manifest。
失败保留原 generation，未发布副本 tombstone 后清理；磁盘不足不得删原数据腾空间。
Runtime rollback/App downgrade 选择兼容 generation，不可 wipe、merge 或覆盖 Profile。
不兼容时保留 metadata 与数据，提供兼容 Runtime、修复或创建新 Profile 的状态投影。
备份和 restore 不包含 lease、connector token、pipe endpoint 或 active binding。

## Artifact 与 UploadGrant

Artifact metadata 是 UUID manifest，数据写 `.partial`、校验 hash/size、原子 rename，再发布 ready。
MCP 只返回 opaque ID，UI 使用短期 session/artifact 单主体 capability；不用 process media token。
交付仍走 loopback HTTP，单独 endpoint 验证 token 主体、expiry、owner 与 requested artifact。
页面 WebView 不接收 token；不扩张 `path_scope` 为 browser 数据树的全局授权。
promotion 先 durable owner reference，再 chat journal；后者失败留下可 reconcile orphan owner。
refCount 从 owner record 重建；cleaner 不能因瞬时计数为零删除 durable chat 引用。

初始字节预算写入 capability manifest，由 Host admission 与实际流读取双重执行：

| 内容 | M1 初始上限 |
| --- | --- |
| 单截图 | 32 MiB 编码结果、32 million pixels，任一边最多 16384 pixels。 |
| 单次上传 | 200 MiB；目录和无界流不支持。 |
| 单次下载 | 512 MiB；缺 Content-Length 也逐块计数并截停超限流。 |
| 脱敏 Grok Trace | 64 MiB；原始 Playwright Trace 不进入普通 Artifact。 |
| 单 Session 未提升临时 Artifact | 1 GiB；durable owner 与用户 exported 文件不算可驱逐缓存。 |
| 全局 transfer staging | 2 GiB；包含所有尚未物理清除的 upload spool/partial 与其他 staging；写前预留，删除或已计费的原子转移后释放，crash 后由 durable ledger 重建。 |

流超限停止接收并清 partial，不静默截断成成功 Artifact；磁盘不足不驱逐 durable owner 或用户导出。
这些是版本化工程上限，变更须重新跑大小边界与容量 fixture，不能靠无限增长隐藏失败。

UploadGrant 绑定 Session/Tab、顶层及 frame origin、input binding、hash/size、fence、single-use 与 expiry。
用户批准后以 no-follow 打开并固定源文件身份，有界复制到 user-only 私有 spool，复验副本 hash/size 并冻结写入。
M1 禁止目录上传；Agent 只得到 basename 和 handle，attach 是向目标 origin 披露文件的授权。
全部文件使用 path-based `setInputFiles()`，因此 200 MiB 不受 Playwright 50 MiB buffer 限制；不通过拼接 buffer 或额外 CDP 绕过。
源码分析基线 Playwright v1.55.0 的 `server/dom.ts`/`chromium/crPage.ts` 路径调用止于 `DOM.setFileInputFiles` ACK，不能用其返回或 worker 句柄关闭证明浏览器已复制字节；M1 使用 M0-R01 实际锁定并通过 R05/R06 的生产候选及其 buffer 限制、路径上传和晚读证据。
Host 在派发路径前持久写 `UploadSpoolLease`：spoolId、uploadGrantId、runtimeId/runtimeGeneration、hostBootId/runNonce、来源 Session/Tab、size/hash、dispatchState 与 cleanupState；路径仅在私有 ledger/worker 通道可见。
durable Runtime ownership 和 2 GiB reservation 成功后才设置 dispatchState=dispatched 并发送路径，失败或派发是否发生不确定均保守保留，不自动重传。
导航、接管、turn/session 结束、取消、Host 重启或一小时 expiry 都撤销未消费 grant。
撤权先关闭新 attach admission；未派发副本可直接 `purging -> purged`，已派发副本转 `cleanupPending`，直至所属 Runtime 全部 Chromium/worker descendants 的 exit proof 才进入 purging。
ACK、input 清空、导航、tab 关闭、grant consumed/revoked、worker 单独退出均不结束 reader lifetime；包括新 tab 继承的 File/Blob，统一保留到整个 Runtime 退出。
接管不为回收 spool 关闭用户浏览器；页面可继续延迟 FileReader/stream 或人工提交已披露文件，撤权不能收回这种页面能力。Agent 后续动作仍受 fence/确认约束。
Session close 关闭其 tab 并释放普通 ref，Runtime-owned ledger 保留 `cleanupPending`；共享 Runtime 的其他 Session 不因此阻塞或被关闭。
Host/Guard crash、Runtime generation 改变后先 reconcile 旧 run ledger；已证明退出才 no-follow purge，无法证明则 quarantine 并保留额度，禁止只凭 PID/TTL 删除。
删除失败留 tombstone 与配额，重启先重试；spool 不可提升为 Artifact、采用 24 小时普通 retention、交付 MCP 或进入 support bundle。
Host 投影 pendingBytes、pendingUploadCount、cleanupPending/purgeFailed 和所属 Runtime；Workbench/Browser Doctor 显示保留状态与“停止 Runtime 并清理”动作，确认影响的 Session 后 drain、exit proof、purge，失败可重试。
预算不足拒绝新上传并给出该清理入口，不静默驱逐文件、不自动杀活跃 Runtime；最后普通 ref 结束、显式停止、stop_all 与 App 更新 quiesce 均走同一清理顺序。
attach 与撤销竞态报告 unknown_outcome；已经披露的字节不能收回，文件源与用户导出永不由 spool cleaner 删除。
attached 与表单 submitted 分开，最终提交仍由 Gateway 即时高风险确认。

## Download、Export 与清理

每下载唯一 downloadId，记录发起 Session/Tab/origin chain、policy、Artifact/hash/bytes 和私有 destination ref。
相同 URL 的并发下载必须独立进度、取消与保存，不能共用 pending key。
receiving crash 标 interrupted 并删除 partial；staged 重启只恢复等待保存，不再发网络请求。
export 由用户选择路径并确认覆盖；写 destination 临时文件、保留/应用 quarantine 或 MOTW 后再原子提交。
Host 固定目标父目录 handle 并禁止 symlink/reparse 跳转，提交前重新校验覆盖批准绑定的目标身份；临时文件位于同一目标目录。
quarantine/MOTW 应用或校验失败时不发布文件，清理目标临时副本并保留原目标；源 Artifact 仍可重新申请保存。
exporting crash 进入 needs_review，命令结果 unknown_outcome，核对目标后让用户决定，不能盲重试。
目标已存在且 hash 不符必须保留双方，不擅自覆盖或把局部写入说成成功。
导出完成不自动打开/执行文件；Browser cleaner 永远不删除用户 exported 路径。
临时 Artifact/staged download 默认 24 小时；active lock、inflight transfer、durable owner 阻止清理。
清理为 tombstone -> asynchronous no-follow delete -> deleted，可重入恢复。

Session close 先关闭 admission、fence、撤销 grant/binding/lease、取消 transfer/observer，再持久 closing。
Session 的 tab/ref 已释放，且未派发 spool 已清除或已持久移交 Runtime cleanup ledger 后才报告 closed；closed 不表示 Runtime-owned spool 已物理删除。
最后普通 ref 结束触发 Runtime drain，spool lease 不阻止 drain；Runtime closed 仍要求 descendants=0 和 spool purge 确认，共享 Runtime 保留 cleanupPending 投影。
Profile 删除是单独用户确认操作；删除 Session 不删除 project/named Profile。
metadata 写入失败标 degraded 并阻止新副作用，不能以 UI 隐藏替代关闭。

## 崩溃恢复与验收

Host boot 轮换 identity，撤销所有短期凭据；Runtime generation counter 单调持久化。
恢复 BrowserSession 为 `TakeoverLatch=user`，恢复 display metadata 与不可变 ready Artifact。
只有已批准 restorePolicy 的安全 HTTP(S) GET 可以自动导航，其余 about:blank 等人工恢复。
下载/auth callback/敏感 GET 不因 Method=GET 获得自动恢复许可。
dispatch 前写无 payload inflight record；崩溃后不确定写操作统一 unknown_outcome，不自动 replay。
partial Artifact 删除，ready/promoted Artifact 重新验证主体后可签新访问 capability。
upload spool 按 durable dispatch/Runtime ledger 单独重建预算与清理，不能套用 partial Artifact 删除规则；缺 exit proof 返回 reconciliation_required。
M1-R06 提供 `beginAppUpdateQuiesce(updateId)` / `abortAppUpdateQuiesce(token)` / `commitAppUpdateQuiesce(token)` 的幂等 Host 内部接口：begin 关闭全部 Browser admission、fence/revoke、持久上下文、drain 并确认 descendants=0/spool purge 后返回 token。
begin 部分失败或 abort 均保留 `TakeoverLatch=user`、旧队列清空；人工重开/Preview 可恢复，绝不自动恢复 lease/Agent 控制，且不调用 ACP/IM/voice/mirror teardown；成功安装或确认 Windows 交接才 commit。
Runtime 不可用到可见可用 Preview 的目标 p95 <= 2 秒；恢复不得暗示 Agent 动作成功。

验收执行 M1-R06 与 M1-G06 联合场景，再由 M1-D04 用最终安装包重复；200 MiB 延迟 FileReader/提交、撤权后保留、共享 Session close、满预算、显式 Runtime 清理与 crash ledger 必须各有 oracle。
三个平台每个声明 fixture 连续三轮通过，越权、自动 replay、Profile 串用、orphan descendant 均为零。
至少 30 样本报告冷启 p95 <= 8 秒、新 tab <= 2 秒、crash 恢复/明确失败 <= 10 秒。
未来证据保存在 `docs/qa/browser-rearchitecture/m1/runtime/`，包含故障阶段、oracle 和脱敏诊断。
这些目录是后续验收产物约定；当前文档不声明运行时实现或验证已经完成。

# Computer Use：Host profile 生命周期与 Rust 控制协议检查点

日期：2026-09-20。工作区：`H:\aicoding\grok-app-computer-use`。
分支：`feat/computer-use-implementation`；HEAD：`30757366`。
当前总状态仍为 **partial — not releasable**，不代表最终版验收。

本文件是历史批次。后续准入身份与真实 worker 控制已实现，最新状态和保留
缺口见 [受管请求身份检查点](2026-09-20-computer-use-managed-request-checkpoint.md)。

## 本批修复

1. worker 的 HTTP body 读取进入路由异常边界。真实 TCP 发送完整 JSON 但声明
   更长 Content-Length，再断开连接，旧实现出现 uncaught exception，独立
   `/health` 随后 ECONNRESET。修复后不完整 open/pause/cancel/shutdown 均不会
   执行；worker 存活、owner 仍 unknown、没有创建 profile。格式错误的完整 JSON
   明确返回 `invalid_json/not_started`，不再当作空对象执行路由。
2. Host 在派发 open 前保留 profile 归属，用 RAII 保持 opening reservation
   到结果校验/发布结束。本地文件系统准备失败会归还未派发的 reservation；
   已派发而丢响应、响应非法、结果校验失败，均保留归属，供 Stop 清理。不能因
   没登记 tab 就跳过远端 cancel，也不能将归属立即让给另一 run。
3. Stop 先在 Host 中保留不可复用的 owner 墓碑并撤销本地目标，再调用 worker。
   失败不恢复目标，不宣称已关闭；重试仍能找到归属。即使远端已确认清理，
   本地 open/clear 尚在收束时也返回 `run_cleanup_pending/unknown`。
   迟到 open/page-list 结果不能恢复目标；成功清理后其他 run 可复用 profile，
   被停止的 owner 仍不能重新打开。Stop 不删除 profile 的持久数据。
4. 显式 clear 将 profile 名称保留到远端关闭和本地目录删除结束，立即撤销
   关联目标。它与 open 互斥，清理失败保留 pending，必须显式重试成功后才能
   重开。拒绝在缺少 worker 时直接删目录；不再吞本地删除错误。需要本地删除时
   校验 canonical 目标确为 Host root 下的同名直接子目录。已停止的 run 与
   profile clear 是不同的生命周期，clear 成功不解除 owner 墓碑。

开/清 profile 实现移到 `browser/host/managed_profiles.rs`，避免继续扩大
原 `managed.rs`。未改变开关默认值、模型工具面、真实账号或日常浏览器资料。

## Rust 客户端协议

新增 `WorkerRunRevision`、`WorkerRunPhase`、`WorkerRunState` 及严格回执解析。
revision 必须处于 JS 安全整数范围，恢复只允许 successor。状态必须包含
正确 revision、有效 phase、0–64 的活动计数和与计数一致的 idle 布尔值。
缺失/错误/旧回执按 unknown 拒绝，不能推断“空闲”。idle 本身不证明 context
已关闭，Stop 仍需 cancel-run 成功。

`ManagedBrowserWorker` 提供 capability/pause/status/resume 合同，默认明确拒绝。
`LoopbackPlaywrightWorker` 实现真实 Host 标记请求，控制请求使用独立 transport，
先要求 `/health.runLifecycle=1`。恢复丢响应、不符版本、仍忙或错误 phase 均不
自动重发、不授予新 revision。客户端没有新增任意 JS 或调试路由。

**尚未把这套控制合同接入 ManagedBrowserAdapter 的 abort/is_idle 或 Broker
resume，也尚未把同次准入的 revision/cancellation 贯穿业务请求。** 当前产品
业务调用仍是 legacy 初始 revision。本批不能写成 App 暂停按钮、即时取消或
完整恢复流程已完成。

## 首败、验证与限制

日志根：`tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/`。

- `request-body-first-red-03.log`：实际正文断连使独立 `/health` ECONNRESET，
  同时捕获到 worker uncaught exception 的诊断标记。较早两次红灯诊断不充分，
  保留原始记录。仅故意中断的 raw socket 允许 reset；后续健康/状态请求不能
  忽略连接错误。`request-body-green.log` 两项通过。
- `profile-stop-first-red-02.log`：4 个真实断言失败，分别是 Stop 漏清理、
  迟到 open 可执行、失败清理未撤销本地目标、Unknown open 被其他 run 抢占。
  初版测试门控自身持有 mutex 导致阻塞，已修正并停止精确匹配的 owned 测试
  进程；该次 `profile-stop-first-red.log` 不是有效产品断言结论。
- `profile-clear-first-red.log`：clear 期间与失败后目标仍可用的两项断言失败。
  `profile-clear-green-02.log` 当时 6 项通过；随后增加本地 setup 失败归还测试，
  纳入完整 Core。门控单元测试是 worker double，不能冒充真实 Chromium。
- `host-lifecycle-core.log` 首次 392 pass / 1 fail：seed 尚未更新，测试期待
  `missing_sibling`，却先遇到 `hash_mismatch`（worker.mjs 与源码不一致）。
  用标准生成器同步后，`host-lifecycle-core-02.log`：**394 + Driver 12 passed**。
  未改断言、未手改 hash。前批 IPC 10053 本次未复现，根因仍未定位，不能销账。
- Core 和 App 严格 Clippy 均通过：`host-lifecycle-clippy-core.log` /
  最终 `host-lifecycle-clippy-app-03.log`。quality final PASS，千行文件 80/80。
- 新探针首次 `source-preview.log` / `source-preview-02.log` 在测试 HTTP 服务
  失败，补充诊断得到 `lost resume: WouldBlock, bytes=0`。Windows accepted
  socket 继承 listener 非阻塞模式，已显式设回 blocking + read timeout。
  没有吞读错误或放宽 body/auth 校验。`source-preview-03.log` 接着暴露 fixture
  误把停止后 legacy 请求期望为 run_fenced；协议先检查必填 revision，实际
  应为 stale_run_revision。改为同时要求显式 run-status 确认 stopped+idle、
  legacy 请求明确拒绝，及其他 owner 真实复用成功，未以任意错误充当清理成功。

| 最终门禁 | 结果 | 日志（同上目录） |
| --- | --- | --- |
| Core + Driver | 394 + 12 passed，0 ignored | `host-lifecycle-core-02.log` |
| 严格 Core / App Clippy | exit 0 | `host-lifecycle-clippy-core.log` / `host-lifecycle-clippy-app-03.log` |
| cu_probe 重建 | exit 0；保留既有 linker 创建库 stdout 警告 | `host-lifecycle-probe-build-04.log` |
| source 真实 Chromium | exit 0；控制协议、Host 迟到 open、两 run 隔离，页面 click oracle=2 | `host-lifecycle-source-preview-04.log` |
| 实际 seed worker + Chromium | exit 0；同一组后置条件 | `host-lifecycle-packaged-preview.log` |
| 完整 source browser Node | 118 passed，0 skipped，47.42s，exit 0 | `host-lifecycle-browser-full.log` |
| 实际 seed 正文断连/错误信封 | 2 passed，0 skipped，exit 0 | `host-lifecycle-seed-body.log` |
| 实测后 seed check | exit 0，manifest/tree 未漂移 | `host-lifecycle-seed-check.log` |
| quality final | PASS，千行文件 80/80 | `host-lifecycle-quality.log` |

fmt check / diff whitespace 均 exit 0，保留既有 Git LF/CRLF 提示。
seed manifest：`551f692ba530aca79d582605bc5e2b7d7196502b8ebb6207d2d15e2fe99eb1a8`；
seed tree：`6877fdbac8252ac6437ac05ef92d2ce039d37ceef38f3f11dba5b3207af3ea57`。

测试新增真实浏览器 profile probe：仅门控“真实 open 已返回、Host 尚未发布”
这一窗口，Stop 关闭真实 context，迟到发布拒绝，另一个 run 存活并能在清理后
复用原 profile。它**不证明在 Chromium 原生启动内部取消成功**。另有真实
Rust 客户端 pause/resume/status/stale/stop 协议探针，以及 legacy/lost/stale/
busy/wrong-phase 五项 HTTP 回执拒绝且无重放检查。

255 文件局部源码指纹：
`24E19B23ABA13C1B5676BD53A8443821A44D61224391E022ED5DB65B95322C02`。
日志 `host-lifecycle-source-fingerprint.log`，范围与算法同 managed Wait 检查点，
不包含全仓依赖/构建配置，**不是 C6 候选冻结**。
cu_probe SHA256：`8F5D2696C85991F4E02B805225DE872D9482FD8B2BDE23EC2E19B7E593CB21DE`。

本批未重跑前端、完整扩展/MCP 34 项、原生 toolbar、安装、跨平台或真实模型
矩阵；本批程序代码仅涉及受管 worker、Host profile、Rust 协议和对应探针。
最终只读检查未发现本批 cu_probe、私有 Node 或 managed-contract/error-contract/
typed-act/handle-lifetime/pairing-owned 测试 Chrome 的进程残留。HEAD 未变、index
为空，未提交推送。下面保留的空目录不等于仍有活动浏览器进程。

自动审批拒绝删除以下空测试目录，原因仅返回 `blocked by policy`，未换通道
重试，保留原状：
`C:\Users\Administrator\AppData\Local\Temp\cu-profile-lifecycle-4b133bcb-f7eb-4fdd-8f50-08da2774e841`。
只含本批中断测试的 `profiles/other` 空目录，没有真实浏览器数据。
前批已被拒绝删除的 pairing-owned 目录也不重试。

## 下一批：完成准入身份与 App 控制接线

1. 在 Broker 的同一次准入中捕获 worker revision、取消 token 和 worker 实例
   身份，贯穿 capture、act/Wait、navigate/download/upload、new-tab/popup/open。
   target generation 与 worker revision 不等价；不能在请求派发时临时查询
   “最新 revision”，不能使用 thread-local 隐式上下文或接受模型提供 revision。
2. 远端暂停/状态确认必须发生在 Broker/Host mutex 外。后续增量已经将
   `Broker::resume` 改成独占 admission + 锁外空闲/存活查询 + generation 复验
   （见下节），不要重新从零重写。真实 worker 控制仍需接入同一事务，结合
   缓存的 remote pending 状态；副作用型恢复请求丢响应不能只靠本地 guard
   释放就重发。Stop/feature-off 可先行撤销，旧结果不得提交到新一代。
3. `commit_authorize` 会替换取消 token 并增加 target generation；resume 会
   丢 target；surface picker 也会在授权前 open profile。必须把这三条入口纳入
   同一事务，禁止只修 capture 或只改暂停按钮。不可把恢复请求丢响应后默认
   当成 running；保持未知/暂停态，不盲目重试。
4. `abort/is_idle` 真实接上 worker 后，再启用 cancellable business HTTP。
   socket 退出、远端操作结束、context 关闭是三个不同事实。所有错误/超时/
   取消均不能因本地 admission 已释放而允许远端尚未结束时恢复。
5. 验证实际 App Broker/MCP → worker：native click/下载/截图进行中竞争、
   open 尚在原生启动、close 失败/重试、两个 run、旧请求和旧控制、取消前派发
   以及 header/body 断连。source 和重新生成的 seed 都通过后才记接线完成。

随后继续 ExistingTab typed actions，以及 C4/C5/C6 的 App/ACP、原生 toolbar
与截图、Edge/安装、macOS arm64/x64、Linux X11/GNOME native Wayland、真实模型
和同一候选至少 12 小时主动长稳。上述缺口均不得用本批通过数替代。
不 commit/push/PR；保留所有用户/Grok 脏改动。

## 后续增量：Broker 恢复事务

下一轮先复现并修复了恢复期间的 Broker 锁问题。首败
`resume-transaction-first-red.log`：3 项断言失败，等待 adapter.is_idle 时
新的 Pause、Stop fence 及另一 run 的请求都被同一 Broker mutex 挡住。

`broker/resume.rs` 现单独持有 RAII admission：锁内登记 resume_in_flight，
锁外查询 adapter 空闲与目标存活，最后锁内复验原 generation/paused/cleanup。
admission 到发布或失败结束才释放。重复 resume、并发授权和操作不能进入；
后到的明确 Pause 即使此前已经 paused，也使在途 resume 的 generation 失效。
Stop 仍先行 fence；失败不自动取消 paused，也不恢复旧 snapshot。未暂停时
调用 resume 明确拒绝，不再无意丢弃有效授权。所有查询仍在原同步调用线程，
没有 detached thread 或自动重试。

验证：完整 Core **399 passed** + Driver **12 passed**，
`resume-transaction-core.log`；Core/App 严格 Clippy exit 0，
`resume-transaction-clippy-core.log` / `resume-transaction-clippy-app.log`；
quality final PASS（千行文件 80/80），`resume-transaction-quality.log`。
5 个新增用例含慢查询门控、后到 Pause/Stop、另一 run 响应、授权/恢复互斥、
DeadTarget 失败后恢复重试和未暂停误调用。门控使用 Fake adapter，属于 mutex/
admission 竞争证据，不是远端 worker 已接线的证据。

本增量仍未接入 worker pause/resume 或业务 revision/token。下一步直接实现
准入捕获的 managed 请求身份和全部业务路径，再把 worker 控制接入上述事务；
不要用更多仅有 Fake 的通过数替代这项实现。

源码/实际 seed 的真实 Chromium 探针均 exit 0：
`resume-transaction-source-preview.log` / `resume-transaction-packaged-preview.log`。
新 gate `broker_resume` 实际经过 App Broker：pause → resume 后旧授权/snapshot
撤销，无授权 observe 拒绝，再显式 authorize 并取得更高 targetGeneration 的
新观察，页面独立点击 oracle 仍恰好 2。既有真实 Wait、preview、旧 ref 点击、
Host open/Stop 隔离、类型化 worker 控制 gate 一并通过。普通恢复这条链不等于
“正在执行真实原生动作时暂停/恢复”或 worker revision/token 接线已经完成。

当前 257 文件局部指纹（同一范围/算法）：
`31086F2DD6D4C0E9CC2CE521868FDA1E66D60550ED261FBF0D97EDAAD9168B9A`，
日志 `resume-transaction-source-fingerprint.log`。
最新 cu_probe SHA256：`7D32EF3AE22F2156BF92E7EDD04D512F513BCAF3728E91BD1BA6C5DCCC4CA1C4`。
构建 `resume-transaction-probe-build.log` exit 0，既有 linker stdout 警告保留。
fmt/diff whitespace exit 0，HEAD/index 未变，无本轮 owned 进程残留。
worker JS/seed 未修改，本增量未重跑完整 browser Node、前端或其他平台验收；
不把前批 118 项写成此次重跑，也不把这份局部指纹视为冻结候选。

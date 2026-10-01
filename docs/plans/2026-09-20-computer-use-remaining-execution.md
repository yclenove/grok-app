# Computer Use：安全配对之后的剩余开发顺序

日期：2026-09-20（Asia/Shanghai）
最新进度先读 [Host 崩溃恢复检查点](2026-09-21-computer-use-host-process-retirement-checkpoint.md)：
真实 Host/SW 五切点三顺序的 15 个场景通过；journal v4 保存原进程 witness，
物理完成后可经新配对 Host 只读确认旧进程退出。完整浏览器退出、renderer/
升级世界、双 Host 和 R5 等缺口仍未关闭；总状态 partial，动作能力 NONE。
前一批基础见 [2026-09-21 R3 完成退休检查点](2026-09-21-computer-use-completion-retirement-checkpoint.md)。
Host 已接受 settle 后的容量/TTL tombstone 淘汰可由同进程 authority 认证的只读
retirement 收束；动作 23、真实缓存恢复 4、观察 34、历史 completion 8、Node
142、Core 448、Driver 12 项通过。R3 已关闭；继续 R4 的 App/浏览器重启，以及
renderer/升级世界和 R5 等缺口。
后面的旧进度不要求重新从头实现。
工作区：`H:\aicoding\grok-app-computer-use`
分支：`feat/computer-use-implementation`；HEAD：`30757366`
当前总状态：**partial — not releasable**。

R3 不恢复授权、不重放动作，也不允许 retirement 释放 pending。新 App 实例的
authority 不同，旧 proof 必须拒绝；因此本批不能写成跨 App 重启恢复已完成，
ExistingTab action capability 继续为 NONE。

最新 [SW 原文档恢复检查点](2026-09-20-computer-use-worker-recovery-checkpoint.md)
已完成 claim 前清理记录、同原文档的实际 SW 重启恢复和逐条重试隔离；112 项
JS 测试通过。该切片没有关闭完整 R2–R4；继续其下一项及重启/MCP 计划，
保留 App/新文档/完成记录淘汰/完整浏览器退出等缺口，动作能力仍 NONE。

最新 [生产 SW/v2 动作检查点](2026-09-20-computer-use-production-action-transport-checkpoint.md)
已接通真实队列派发，11 项实际浏览器检查通过；新增边界发现并修复配对限流
阻塞导航撤销，Core 445 通过。下一项为
[重启隔离与 MCP 动作接入](2026-09-20-computer-use-restart-and-mcp-execution.md)，
按 R1–R5 继续，再回到 C4–C6。动作能力仍 NONE；不重做本批已接通的 JS/SW。
以下旧进度段落是历史检查点，最新验证边界及指纹以生产 SW 检查点为准。

最新 Host 派发见 [typed action 队列/版本协商检查点](2026-09-20-computer-use-existing-action-dispatch-checkpoint.md)。
Host 实际命令/ref 校验、无明文 key 的 queued reservation、v2 negotiate/poll/
claim/result 及 mixed occupancy 已实现；Core 442 + Driver 12 通过。新 claim
必须带完整 request + proof，旧 generic claim 拒绝 bound action。下一项已前进到
JS/SW 接线和真实浏览器队列派发，再关闭重启隔离门禁；不要重做 Host 队列。
动作能力仍 NONE。下方较早批次的“尚缺 Host 队列”等是历史状态。

最新扩展端见 [回执客户端检查点](2026-09-20-computer-use-extension-completion-client-checkpoint.md)。
独立清理 scope 和动作所有权控制器已有实现，真实 App HTTP + 浏览器固定动作
8 项通过；其中授权和 offer 仍是私有 fixture 管道，不能当作生产 MCP action。
继续 C3：Host typed action 队列/命令绑定、version 2 协商及 SW transport，随后
App/SW 重启隔离；这些关闭前不开放动作 capability。先读新检查点，不重做内核。

最新增量见 [Host 领取与完成回执检查点](2026-09-20-computer-use-existing-action-completion-checkpoint.md)。
registry 与 HTTP claim/status/settle 已实现并通过真实 loopback 负例，观察/动作
共用容量，所有撤销入口即时取消，claimed 占用保持至回执，Broker Stop 状态
已有回归。下一步仍在 C3.2/C3.3：严格动作命令、MV3 transport 的 Promise/receipt
所有权、App/SW 重启隔离，再接 adapter 和真实 MCP 动作。不要重新实现 registry；
也不能把本批 Host 合同通过当成扩展动作可用了。

最新 C3 增量见 [已有标签页动作内核检查点](2026-09-20-computer-use-existing-action-kernel-checkpoint.md)：
原始 DOM 引用校验、语义 click/input/scroll/Wait、扩展 activity 版本及原脚本/取消
调用 join 已实现；动作协议/Host 完成回执尚未接通，能力仍 NONE。另修复已授权
浏览器目标列表误依赖 Desktop 枚举。下一批从该检查点的 C3.2 继续，不重做
动作内核，也不以局部测试替代完整动作闭环。

最新增量见 [原生下载与暂停恢复检查点](2026-09-20-computer-use-native-cancellation-checkpoint.md)：
下载取消/无句柄时的 context 收尾、普通链接单次传输及按 URL Cookie 过滤已修复；
重新授权刷新取消导航后真实 pageGeneration。Core 404、Driver 12、browser
Node 122 通过，source/seed 的下载两阶段、点击/截图暂停恢复四行真链通过。
无 Download 句柄的超时清理保留磁盘 profile，但会关闭该任务 context，标签页
重开体验仍须 App 验收。同一检查点末节已追加四行 terminal Stop 和三行真实
恢复回复丢失/迟到 Pause/Stop，source/seed 的完整 11 行通过，其他 owner 存活、
unknown 不重发及终态 profile 归还均已验。下一项优先推进 ExistingTab typed
actions，保留原生启动内部及各业务路由真链缺口；不要反复重跑已通过的同一门禁。
前一批 [请求身份检查点](2026-09-20-computer-use-managed-request-checkpoint.md)
已将 Broker 准入 worker/revision/token 贯穿业务与真实 pause/status/resume；
下文“尚未接线”保留为历史进度，不要求重新从头实现。

最新进度先读 [Host profile 生命周期检查点](2026-09-20-computer-use-host-profile-lifecycle-checkpoint.md)。
正文断连、Host open/Stop/clear 竞争和 Rust 控制协议客户端已补。
下一批仍需将同次准入的 revision/cancellation 贯穿所有业务入口，并在锁外完成
adapter/Broker 的真实 pause/status/resume 事务。Broker resume 的独占 admission、
锁外 adapter 查询及旧 generation 复验已补，远端控制与业务身份仍未贯穿。

增量进度见 [分享检查点](2026-09-20-computer-use-sharing-checkpoint.md)：
C0 租约/异常清理已有局部证据；C1 分享 UI、文档身份和候选版本已实现，
自动浏览器 20 项通过，含 metadata await 期间真实导航。真实工具栏 activeTab
手势尚未通过，C1 仍 partial。不要重复从零开发已有行为；先补原生手势后置条件。

后续 C2 已接通 bounded typed text-observation 通道，真实浏览器 21 项通过；
见 [观察通道检查点](2026-09-20-computer-use-observation-transport-checkpoint.md)。
原生工具栏工具故障保留为独立缺口，未阻断可独立实施的 C2；它不是 C1 已验收。

最新 C2 增量见 [截图与取消检查点](2026-09-20-computer-use-capture-checkpoint.md)：
可选截图/PNG 接收已有实现；隔离浏览器 30 项通过，包含观察竞争、两级超时
及一万控件扫描预算。真实 toolbar 授权后的截图成功与 capture 期间竞争仍
待验，不把图片归一化测试或 API 拒绝测试算作该项完成。

最新 C3 观察增量见 [生产观察适配器检查点](2026-09-20-computer-use-existing-adapter-checkpoint.md)：
ExistingTab adapter 已注册，正文/preview purpose/cancellation 接通 MCP；实际
App 私有 Node/MCP + MV3 共 34 项通过。授权 owner/失败回收边界已修复；
managed preview 的后续进度见下；接下来补观察并发一致性，再推进 C3 typed actions。
真实 toolbar 截图、完整操作闭环与 C4/C5/C6 仍未完成。

最新 managed 增量见 [受管预览检查点](2026-09-20-computer-use-managed-preview-checkpoint.md)：
model/preview/screenshot 选项已传到真实 worker，三层模型 refs 不再被 UI preview
覆盖，旧 worker 不支持时明确拒绝；源码与 seed worker 的旧 model ref 点击
经独立页面 oracle 验证。统一 headless 和 owned-profile Preferences 避免日志/
词典污染 immutable runtime，未放宽哈希；83 项 browser Node 回归通过。
Broker 后续事务增量见 [观察/动作事务检查点](2026-09-20-computer-use-observation-transactions-checkpoint.md)：
observe/preview/action 已共用 run admission，动作占用延续到物理完成及账本提交，
模型观察失败计预算、忙碌拒绝不计；旧版 browser_observe 也走 Broker。UI 忙时
跳帧而非闪错误。Core 372 + Driver 12、前端 59 项通过，真实进程结果见检查点。
随后 [真实受管 Wait 检查点](2026-09-20-computer-use-managed-wait-checkpoint.md)
已修正 managed 原始 DOM handle 等待、MCP 显式身份、准入/去重/失败预算及
native 动作结果返回时的占用释放竞争。Core 376 + Driver 12、browser Node 97、
扩展/MCP 54 项通过；源码/seed 真实 Wait 和随后旧 ref 点击均由页面 oracle 验证。
这不是 ExistingTab Wait 完成。

最新 [句柄生命周期检查点](2026-09-20-computer-use-handle-lifetime-checkpoint.md)
已实现原始句柄的 action-aware 回收，观察退休立即拒绝旧 ref，物理操作结束后
释放；preview/未发布结果清理及慢回收背压接通。真实 Chromium 25 轮模型替换
加预览、门控 click、失败/导航/close 已验；browser Node 110 项通过。
下一步 managed HTTP 取消及 C3 typed actions，继续保留原生手势/平台/安装缺口。

随后 [可取消 HTTP 基础层检查点](2026-09-20-computer-use-http-cancellation-foundation-checkpoint.md)
已完成同步入口下的可取消异步 exchange、真实 socket EOF/no-retry 测试及 Broker
StopRequested 清理门控；Core 386 + Driver 12、source/seed managed 真链通过。
**生产 capture/act/open 尚未传入取消句柄**：managed abort/is_idle 还未跟踪远端
物理完成，直接接线会让暂停后过早恢复。下一项先补该检查点的 operation/quiescence
协议和真实暂停竞争，再传播所有准入 token；不能把基础层通过记为 Stop 已提速。

[worker 暂停/物理完成检查点](2026-09-20-computer-use-worker-quiescence-checkpoint.md)
已实现 worker 的 runRevision、pause/status/resume、活动请求计数、观察退休和
启动 reservation。真实 source/seed HTTP fixture 的 Wait → pause → resume →
原 tab 新观察/点击已验；旧 revision/旧 pause 拒绝。**App Host 尚未接线**，下一项
是 trait/client 的版本绑定、真实 abort/is_idle 和所有生产请求的取消身份贯穿，
并补 Host opening_profiles Stop、原生输入/截图取消竞争。不要再次重做 worker
协议，也不要把 fixture 通过写成 App 暂停/恢复已经完成。

本文细化当前可接续的工作，不豁免 R2D 中的生命周期、权限、质量、安装、
跨平台和长稳验收。测试数量和工作时长不等于完成。工作树未冻结，旧通过结果
不能填入新候选版本的发布矩阵。

## 0. 开工规则

- 保留全部未提交改动；不 reset/restore/clean/stash，不 pull/merge。
- 未获用户新指令，不 commit、push、PR、发版或改变默认关闭策略。
- 不读取用户账号、Cookie、Token、日常浏览器资料；不改代理/VPN。
- 只操作本轮创建并标记 owner 的测试窗口、文件、profile 和 App home。
- 先看 `AGENTS.md`、Computer Use/i18n/dialogs wiki、配对 ADR，再看具体实现。
- 单 writer；Cargo/浏览器/App/安装测试串行，避免资源竞争污染失败结论。
- 先写能执行到断言的失败用例，再修行为；保存首败，不用跳过/放宽断言做绿。
- Windows 子进程仍隐藏窗口。内置 Node 20 在无 PATH 的 Rust 子进程环境中
  会触发 ENOENT/ENOTCONN；探针仅给 SystemRoot 下 System32 作为 PATH。
  不因此继承用户整个环境，更不能改用系统 Node 冒充私有运行时。

## 1. 当前能力边界

已具备的局部能力：App 单独确认 + 扩展输入 80 位一次性验证码；HMAC 证明；
严格 Origin、身份绑定、一次性消费；撤销/轮换/feature-off；真实隔离 Chromium
配对；source 扩展的 15 语言文案。扩展有 storage/activeTab/scripting 与
loopback host 权限；只查询当前活动 tab，只在显式 Share 后注入固定元数据脚本。

还没有：完整 Share 当前 tab 用户链的原生手势验收、正确目标截图成功验收及
真实动作通道、完整模型观察/操作闭环、安装版扩展交付。
所以用户现在不能靠这个扩展让模型操作自己已有的标签页。

`chrome.storage.session` 的 TRUSTED_CONTEXTS 排除 content scripts，不是对
扩展自身 popup 建立秘密隔离。验收项 `no-popup-key-message` 只证明消息 API
不导出 key，不宣称同源可信扩展代码无法访问 session storage。

## 2. C0：关闭配对生命周期和探针清理缺口

先做这一批，不先显示能操作 tab 的按钮。

实现：

1. Host 使用单调时钟记录连接有效期；认证的 heartbeat 才能续租。
   公共 challenge、无效 key、旧 connection generation 不能续租。
2. 到期同时撤销 key、清除 shared candidates、失效借用 grant/观察/预览；
   sweep 前也必须在每个派发入口拒绝过期连接。墙钟调整不能延长租约。
3. 浏览器关闭、worker 挂起/退出、不发 disconnect 都能在有界时间内收回权限。
   恢复不能静默继承旧授权；迟到 heartbeat 不能复活旧 generation。
4. 扩展 heartbeat 有单请求互斥、超时、退出清理；不无限重试旧 key。
   popup、status、forget、pair 的异步返回不能把新状态覆盖成旧状态。
5. 探针建立 profile owner 和精确子进程树所有权。Node 意外退出/启动失败时
   也可归还自己创建的资源，不扫描并杀掉其他 Chrome 或整类测试 profile。

测试必须覆盖：到期前/恰好到期/到期后；正常续租；伪造/错实例/旧 generation；
续租与 revoke/feature-off 并发；浏览器被终止；worker 重启；存储写/删失败；
晚返回；无 PATH 首败回归；探针运行中断后的精确清理。

通过条件：真 Chromium 无通知退出后 Host 凭据失效，borrowed 状态归还；
独立 oracle 证明旧身份零动作。不是只判断浏览器 local storage 已空。

## 3. C1：明确 Share/Unshare 的用户授权链

相关位置：`tools/computer-use-extension`、Core `browser/host`、IPC、App picker。

- 扩展只有用户点击 Share current tab 才调用 activeTab/scripting；权限变化写 ADR。
- 不引入 all_urls、debugger、Cookie 权限或全标签页枚举。
- Share 只产生候选；App 选择候选并授权 session/run 后才能执行操作。
- 候选包含稳定 tab 标识、当前 document/connection generation；不要沿用
  `grant_picker_tab` 里写死为 1 的 generation。Host 必须校验活连接。
- 不把 tab URL 查询参数、标题中可能的隐私或 page 内容记进普通日志。
- Stop sharing、tab close、navigation/reload、feature-off 都撤销候选和旧观察；
  stop 归还用户 tab，不关闭它、不恢复用户已自行改变的 URL/焦点。
- App 与 popup 的 busy/error/empty/expired 文案走 15 locale，共用现有样式。

通过条件：两张隔离 fixture tab 中，只有明确分享的一张能进入 App picker；
未分享/未授权 tab 的读取和操作都是零；Share 不是配对时自动执行。

## 4. C2：有界 typed transport，先观察后动作

协议先定，再同时实现 Host 与扩展。禁止 Value/任意 eval 当通用逃生口。

每条请求/结果绑定：protocol、requestId、sequence、deadline、App instance、
connection nonce/generation、session、run、tab、document generation。
前端不拿 Bearer；模型不自行填权威身份。

- 使用有界 offer/poll/result 通道或有界 WebSocket。队列长度、字节数、
  一次待处理数量、请求生命周期都有上限；限流不能被轮询轻易触发。
- 延迟、断开、超时、取消必须有明确结果；unknown 不重放副作用。
- 结果重复、跨 session/run/tab、导航前结果、断线重连旧结果全部拒绝。
- 观察先支持 main-frame 可见 DOM/元素引用；密码、隐藏/受限字段不得进入输出。
- 截图只针对用户明确共享且当前可见的 tab，不能截另一张活动 tab。
  截图前后 tab/document/active 状态复验，失败暂停，不偷切 tab。
- isolated world 只运行固定实现。跨源 frame、chrome://、扩展页等诚实拒绝。

通过条件：真实页面观察经 transport 到 Host；node refs、截图、viewport 和
generation 一致；导航/切 tab/超时/旧结果测试实际打到生产通道。

## 5. C3：接入 Broker/MCP 的真实操作闭环

已完成的内核与当前边界见 [动作检查点](2026-09-20-computer-use-existing-action-kernel-checkpoint.md)。
优先完成 delivered-write 占用、跨连接完成回执和 ExtensionTransport 接线，再
宣告实际支持的动作。Observe-only protocol 1 不能被当作新动作协议使用。

- 实现并注册 `SurfaceKind::ExistingTab` 的生产 adapter，仅活认证 transport
  提供能力；无连接明确 unavailable，禁止回退 Desktop 或受管浏览器。
- 支持固定 typed click/fill/key/scroll/wait/navigation；不把任意 JS 开给模型。
- 操作必须引用当前 observation；派发前和执行前复验全部身份/generation。
- 字段 CJK/emoji、滚动边界、脱离 DOM、禁用控件、同 ID 新 document 等要覆盖。
- applied 与 verified 分开；fixture 的独立计数器/字段值验证实际副作用。
- 一 session/run 一个授权目标；两个 session 争抢 tab、旧 run 清理晚到时
  不得影响新 run。模型仍不能 authorize/resume/reconnect 自己。

通过条件：真实 session MCP → IPC → Broker → ExistingTab adapter → MV3 →
fixture → 独立后置条件跑通 observe/act/verify/stop；两 session 隔离、动作
重复、Stop/导航竞争均通过，用户 tab 仍然打开。

## 6. C4：跨入口生命周期与 UI 收尾

把 ExistingTab 纳入现有 Stop、接管、恢复、删除任务、重连、ACP 退出、共享 ACP
租户、换模型、compact/fork、feature-off、App 退出、更新重启的统一矩阵。

- local fence 必须先于异步清理；停止按钮可响应，不等待浏览器网络超时。
- 旧回调不得重建凭据、MCP entry、租约或预览；清理不能关闭借用 tab。
- 预览不可见停止采集；错误/取消/重试均有真实操作，不加空按钮。
- 配对码到期、feature-off、换实例时清除 UI；禁止把 key 放进状态或诊断。
- 两轮全新隔离 App-shell，测试实际 Tauri command/UI 状态，不只跑 Core helper。

通过条件：主生命周期矩阵有当前源码的独立资源/凭据/页面后置条件，
无授权绕过、错目标、停止后操作、旧结果覆盖或基础 MCP catalog 丢失。

## 7. C5：交付与平台矩阵

先形成最小可安装的扩展工件；生成 15 locale；排除 tests、probe、node_modules、
日志、App home、缓存和凭据。源码 ID 与商店签名/正式 ID 的区别写清楚。
应用内安装指引与诊断必须能定位版本不匹配；不自动装进日常 profile。

分别验证且单独记状态：

| 行 | 不能替代它的证据 |
| --- | --- |
| Windows 源码 App + Chrome | Core/Fake/jsdom |
| Windows 源码 App + Edge | Chrome for Testing |
| Windows 安装/修复/升级/回滚/卸载 | 源码 cu_probe |
| 安装版 App + 安装版 Chrome/Edge 扩展 | Load unpacked |
| macOS arm64/x64 原生及安装 | Windows 或交叉编译 |
| Linux X11 原生及 AppImage | 浏览器 fixture |
| GNOME native Wayland 原生及 AppImage | XWayland/X11/浏览器 |
| 真实模型+用户授权闭环 | ACP stub/scripted agent |

缺平台写 not_run/blocked_external，不把 capability false 当完整实现。
真实账号、商店发布、系统授权、其他机器访问需要用户明确授权；不得擅自操作。

## 8. C6：冻结、长稳、交付审查

源码/依赖/构建配置/运行时一起 fingerprint；重编译 App、probe、stub、安装工件。
完整 Rust/前端/i18n/安全审计/格式/文件预算/打包门禁重新跑。

在同一候选版本上执行既定主动长稳与 fault matrix。若按完整最终计划验收，
仍需要至少 12 小时真实主动场景；两小时 Track A 门禁只是中间条件。
构建、睡眠、等用户、旧版本耗时不计。改代码必须作废旧 freeze 并重验。

每 60–90 分钟或批次结束记录：具体改动、首败/恢复、实际测试数、源码指纹、
资源归属与清理、剩余外部条件和下一项。安全计数必须为零；耗完预算不等于完成。

## 可直接交给下一位执行者的提示词

继续当前 Computer Use 目标，先完整读取本文件、AGENTS.md 和配对 ADR。
不要重新从旧计划的「完成」标签推断状态。先读受管预览和生产观察适配器检查点。当前
已验证配对、心跳失效、Share 候选及生产 MCP/ExistingTab 的 typed 文本观察；
原生工具栏 activeTab 和截图成功验收仍缺，typed actions 尚未完成，观察探针
不代表模型可操作已有 tab。managed preview 已有真实证据；先收齐观察/动作事务
并发及 managed 取消，再做 ExistingTab 真实动作。managed Wait 的 ref/预算和
旧句柄 action-aware 回收已有一批真实验证，先读最新两个检查点，不要重复退回
Fake n1 或仅删除引用映射的实现。

HTTP 基础层也已完成一批：读可取消 HTTP 检查点及修订后的 blocking HTTP ADR。
不要再实现一套 detached blocking thread，也不要直接给 capture 加 cancellable
调用后宣称完成。先补 managed abort/is_idle 与远端 quiescence，再做 token 贯穿。

严格按 C0 → C1 → C2 → C3 → C4 → C5 → C6 推进，一批完成测试再进入下一批。
首先核对当前指纹和证据目录，从上述 C3 剩余项继续，并保留 C1/C2 原生手势缺口。
`existing-tab-extension-toolbar` 已具备真实手势门禁，本轮尝试因 sky 窗口
绑定失败退出；不要把 headless loopback 的 20 项成功当成这个门禁成功。
禁止借口「代码有了/单测很多」宣称可用；必须有真实扩展和真实 Broker/MCP 的
独立页面后置条件。保留所有脏改动，不碰真实账号/浏览器资料/代理，不提交推送。
遇到外部缺机器，先完成其他本地可做项；明确分开 implemented、passed、not_run。
每个 checkpoint 给出具体证据，不问是否继续。总状态保持 partial — not
releasable，直到全部平台、安装、模型和长稳条件都有同一候选版本的证据。

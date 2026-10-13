# Computer Use 任务中授权与连续执行开发计划

> 2026-10-05 当前状态更正：Codex已接手为单writer，不启动Grok/子代理。本文planned状态
> 是2026-10-02的历史；当前source52实现/实测以[启动检查点](2026-10-04-computer-use-task-consent-u04-existing-bootstrap-checkpoint.md)
> 及supervisor为准。dormant1/browser focused3通过但完整native33通过/5失败、exit101；下一批按[原control诊断与终态](2026-10-05-computer-use-task-consent-u04-existing-control-terminals-next.md)。
> 旧观察/startup及五项新首败仍OPEN；原owners已退出，精确清理三组旧cache，H空闲观察增2.34GiB。
> 完整U00–U09/D00–D31仍active / partial — not releasable，未提交发布；保留全部dirty与失败证据，
> 每批正常清理确认废弃的构建，不清最新候选、依赖和seed。

日期：2026-10-02。规划者、后续审查者：Codex；产品代码执行者：Grok。
状态：**planned / not implemented**。本文件不代表代码已修改、测试已通过或产品可发布。

用户本轮要求：授权在操作过程中按需发生；同一任务减少重复授权；暂停后方便继续。
执行范围是本文件 U00–U09。完整 CU 的 D00–D31 仍保留，参见
[总体剩余计划](2026-10-02-computer-use-remaining-plan.md)和[交接](2026-10-02-computer-use-handoff.md)。
本批优先完成授权体验，不先陷入 GNOME 外部 backend 的长时间实验。
配套：[交给 Grok 的目标模式提示词](2026-10-02-computer-use-task-consent-prompt.md)。

2026-10-03 派发依赖更正：编号仍代表原范围，执行顺序为 U00 → U01（含返修）→
U02 → U04 → U03 → U05 → U06 → U07 → U08 → U09。当前core只有测试synthetic
Bound构造，U03的真实批准按钮依赖U04生产范围绑定；先接Host范围，再做完整授权卡，
避免界面只能靠mock提交。U04先接U02的Host事务，U03再接真实用户手势并完成联合验收。
这只修正依赖顺序，U00–U09/D00–D31范围和验收标准不变；当前状态看supervisor/review。

## 1. 用户应得到的行为

用户直接发送“帮我在浏览器中完成这个任务”。Grok 可以先理解目标、制定步骤、回答无需电脑的问题。
当它确定需要访问电脑时，才发起结构化请求，聊天中出现一张简洁的授权卡片。
用户能看清任务目的、访问范围和持续时间；一次点击批准本次任务。
首次截图、DOM/UIA/AX 读取以及输入都在批准以后。批准后原任务自动继续，用户不用再补发“继续”。

同一任务在已批准的应用、窗口、浏览器范围内可以连续执行。切换已批准目标、刷新观察、
普通页面导航和新建允许的受管标签页，不重复申请 App 授权。确实要进入范围外目标时，
申请一次增量授权，卡片只解释新增部分。模型不能从聊天正文推断自己已经被授权。

暂停或用户接管立即停止派发，旧观察身份失效。用户点一次“继续”，Host 复验范围与原资源，
后台取得新观察，再继续原任务；不再次要求选择同一个目标、再点一次授权、再发一条消息。
Stop/撤权结束任务权限，下一任务重新按需申请。

本批不是对某个外部产品授权行为的复刻声明。以本项目真实 CLI/ACP、平台能力和本文件合同验收。

### 1.1 量化验收

| 用户场景 | 必须满足的结果 |
| --- | --- |
| 普通聊天、解释问题 | 不出现 CU 授权卡，不启动 capture/input/browser worker |
| 未批准的 CU 请求 | 模型收到的目标内容读取 0 次、截图 0 次、输入派发 0 次 |
| 首次 CU 任务 | 不提前进设置选目标；正常路径一次 App 批准，批准后自动继续 |
| 同范围连续至少 20 次真实动作 | 初始批准后额外 App 授权 0 次；每步仍复验身份和结果 |
| 已批准两个应用实例间至少 5 次切换 | 额外 App 授权 0 次；无范围外内容读取/输入 |
| 请求新增第三个应用 | 恰好一次增量 App 授权；原批准范围不被无故清空 |
| 暂停后继续 | 一次用户“继续”点击；额外选目标/授权/发送消息 0 次；先新观察后新动作 |
| 同一请求重复、连接回包重复 | 只出现一张有效卡；一次决定、一次任务续行 |
| 用户过 90 秒才批准 | 聊天和 Stop 可用；原短 HTTP 调用不悬挂；批准后续行一次 |
| 用户拒绝 | 不执行，不自动重问；如有无需电脑的替代则继续解释 |
| 范围不支持/平台权限缺失 | 明确说明缺什么；不回退全桌面，不显示假“运行中” |

App 内部批准次数与系统授权次数分开记录。初次 macOS TCC、Wayland portal、
ExistingTab 扩展工具栏分享可能需要平台手势，不能隐去或伪造。若 CLI normal 模式
另有逐工具确认，U00 必须查明并处理可支持的会话粒度，报告实际总点击数，不能只统计 App 卡。

## 2. 开发现场与本轮边界

| 项目 | 规划时基线；开工需重查 |
| --- | --- |
| 工作区 | H:/aicoding/grok-app-computer-use |
| 分支 | feat/computer-use-implementation |
| HEAD | 8d8b71c262ab222748bca32412eea884d4b892e0 |
| fork / 源仓库 | origin=yclenove/grok-app；upstream=RongleCat/grok-app |
| 已包含的 upstream/main | 6f931681d90336dd5e55a24e423c134e20f90dbb，v0.2.38 |
| 产品状态 | 四种 surface 代码已存在；整体 partial / not releasable |
| writer | Grok 唯一产品 writer；Codex 只规划和审查 |

开工保存 status/HEAD/CLI 版本，重查 live writer。保留所有原 untracked 文档、patch 和修改。
不 reset/restore/clean/stash/rebase，不换分支，不修改其他 worktree。
本批 Grok 不 commit/push/PR/merge/tag/release；不启动第二 writer，不使用子代理。
源码改动留待 Codex 审查；不得因为旧计划提到 fork 提交就自行推送。

允许使用独立 App data root、独立 agent home、自有窗口/网页/浏览器 profile 与受控测试资源。
不读取或修改启动器凭据，不导出 Cookie/token，不修改共享 ~/.grok、代理/VPN、日常浏览器
profile、用户账号或其它日常进程。不自动点击 UAC/TCC/portal/firewall 权限框。
测试清理只面向核验过 PID/start/owner 的精确资源，禁止按进程名或端口段批量结束。

## 3. 当前真实问题与源码入口

以下是基线实现的限制，不是本批已经修好的结果。行号会变化，按符号定位。

| 入口 | 当前行为 | 本批要解决的边界 |
| --- | --- | --- |
| src-tauri/src/session_manager/computer_use.rs：attach_computer_use、reconcile_session_mcp | 依赖已有 run 授权；会话 busy 时拒绝 attach，提示等待当前 turn 结束 | 分开“工具连接可见”与“执行授权”；不能只删除 busy 判断 |
| src-tauri/computer-use-core/src/ipc.rs：SessionBinding | session_id + run_id；每个 token 当前绑定 run | 未授权时需要纯申请能力；不能用空 run_id 放行全部工具 |
| src-tauri/computer-use-core/src/tools.rs、tools_browser.rs、tools_wait.rs | dispatch 先 require_owner；list/open 依赖当前目标 | 引入受限申请路径，原动作 gate 仍严格；Host-only 控制不能变成模型授权工具 |
| tools/computer-use-mcp/server.mjs、protocol.mjs | 固定 catalog，listChanged=false；Host HTTP 15 秒超时 | 预连接目录；审批异步回执；不靠普通 tool call 挂住几分钟等人 |
| src-tauri/computer-use-core/src/broker.rs、broker/resume.rs、broker/lifecycle.rs | resume 最后调用 drop_execution_identity，连 target 一起清除；reconnect 也清除 | 分开任务 scope、执行绑定和快照；继续一键完成 Host 复验和新观察 |
| src-tauri/src/acp_client.rs | session/request_permission、update_mcp_servers、session/update | 核真 CLI 粒度、等待/取消/续行能力，不编造支持的 ACP 扩展 |
| src-tauri/src/session_manager/connect.rs、events.rs、events_bg.rs、control.rs、types.rs | 会话状态、权限事件、前后台续行 | task/请求/续行都绑定 App session 和 ACP 实例；不可串会话或重复 prompt |
| src-tauri/src/commands/computer_use.rs、src-tauri/src/computer_use/sessions.rs | Host 授权 ticket、prepare/commit/cancel、会话资源 | 在原生命周期上扩展，保留 fence 与迟到结果拒绝 |
| src/components/computer-use/ComputerPanel.tsx、useComputerController.ts | 面板先选 surface/目标再授权，轮询与 busy gate | 主入口迁到聊天按需请求；面板保留预览、状态和高级范围管理 |
| src/components/computer-use/ComputerTaskCard.tsx、ComputerTaskCardLive.tsx | 当前 run 状态与 pause/stop；无未授权 request 卡 | 复用已有卡片体系，补请求/拒绝/继续/范围状态 |
| src/lib/api/computerUse.ts、src/lib/computer-use/taskCard.ts、protocol.ts、errors.ts | API 与状态映射 | 统一请求/任务事实来源，不把 UI pending 当成批准 |
| src/styles/computer-use.css、src/i18n/messages/*/computer-use.ts | CU 样式及 15 locales | 使用项目样式、完整自然翻译，删除不符合新流程的旧说明 |

现有测试也要读，尤其 session_manager/computer_use/tests/catalog.rs、tests/lifecycle.rs、
core 的 broker 身份/恢复测试、ComputerPanel 和 Controller 的 pending/cancel/Stop 测试。
它们的旧合同可能需要修改；必须说明新合同，不能简单删断言让结果变绿。

## 4. 新授权合同

### 4.1 分三层，而不是一个“已授权”布尔值

| 层 | Host 持有的事实 | 获得它后允许做什么 |
| --- | --- | --- |
| 会话申请能力 | 本地 interactive App session、ACP incarnation、连接 generation | 请求 CU、查询自己请求状态、取消申请；不访问电脑内容 |
| 任务授意 TaskGrant | Host taskId、用户批准的 scope/能力/期限、grant revision、当前 session/agent 身份 | 在此任务范围内准备和绑定执行目标；不能跨任务继承 |
| 执行绑定 ExecutionBinding | 当前 run/target 的真实 OS/浏览器身份、generation、lease、cancellation；短期 snapshot/ref/geometry | 经过复验的观察与动作；暂停/换目标/导航后旧观察不能再用 |

TaskGrant 不等于截图身份，模型正文不等于 TaskGrant，连接 token 也不等于授权。
所有请求 ID、taskId、grant revision 由 Host 创建。模型提供目的和范围意图，不能通过构造
request JSON、猜测 ID、传 authorize/alwaysApprove 等字段建立或扩大权限。

建议模型入口名为 computer_request_access（U00 可统一更名并记录 ADR）。请求至少表达
purpose、所需 surface/能力和可解释的 scope intent；不接受任意 executable path、PID
作为已经授权的事实，不接受模型指定 Host taskId，也不从目的描述执行 shell/脚本。
bootstrap 只能返回当前会话的功能可用性和申请状态，不返回其他窗口标题、tab URL、DOM、
截图、剪贴板或其它会话信息。模型的 list_targets 在无 grant 时返回空集/需要授权的结构化状态。
批准后 list_targets 仅返回 scope 内可用的目标，不公开范围外列表。

默认 feature 仍关闭。优先方案是本地 interactive 会话预接入无权申请工具：feature 关闭时
它最多产生一张“启用并允许本次任务”卡片，用户的明确点击才启用 CU 并开始授权准备。
卡片需清楚标明会启用功能；功能关闭时没有后台 capture、输入、native helper 或 browser worker。
用户主动关闭 feature 应立即 fence 当前任务、取消旧申请；不得因迟到批准重新打开。
若 CLI 无法实现此 bootstrap，U00 用真实证据设计等效的聊天申请入口；不能退回“先去设置”。

### 4.2 请求、任务与等待状态

请求建议状态：pending → approving → approved / denied / cancelled / expired / failed。
approved 只在 Host prepare/commit、真实资源绑定成功后成立，不能点击后立即报成功。
相同 requestKey 复用一条请求；同任务同 revision 只消费一次决定。每个会话最多一条活跃
范围申请，重复请求合并/返回已有 requestId，不能不断弹出卡片。范围扩大是新 revision。
不同用户意图不能只因相同 purpose 文本被错误合并。

任务状态建议：awaiting_access、preparing、observing、running、paused、resuming、
awaiting_extension、stop_requested、stopped、completed、failed；具体枚举可与现有模块整合。
保留 denied/expired 与 unknown 的准确 UI；不把 catalog pending、系统权限等待、
物理 cleanup pending 混成 running，也不因一次读取失败就误显示“已停止”。

申请工具必须快速返回 pending + requestId。用 Host 事件或短查询回读最终决定。
用户等待期间不长持 Broker mutex、connect lock、action admission、desktop lease 或普通 HTTP
连接；不把 15 秒超时放大成无限等待。可保留有界逻辑 continuation，Stop 有独立控制通道。
U00 为实际 CLI/ACP 确定续行机制：优先真实支持的暂停/事件回执，或当前会话内一次 Host
continuation。不得新建聊天、重复原用户消息、重放原副作用，不能向其它会话发送消息。
模型拿到 pending 后应停止电脑操作和申请轮询风暴，等待 Host 决策。

建议初始申请 TTL 为 10 分钟，按 Host monotonic deadline 实现；过期不批准不执行。
任务授意建议空闲 30 分钟后过期，仅在没有等待批准、没有操作/cleanup pending 时计算；
活动任务不因固定短超时每几步重授权。U00 可依据真实会话行为调整这两个初值并记录理由，
不得改成永久权限。expire 竞争与 fresh UI 必须有测试，时间合同只有一个 authoritative clock。
端到端的 90 秒延迟批准测试必须真实经过等待，不能只用 fake timer 代替。

assistant end_turn 不代表 CU 任务完成：等待批准、暂停接管都可能结束当前 turn。
Host 维护显式任务完成/取消入口；若增加模型 finish 工具，只能减少权限、关闭自己的任务，
不能授予权限。同一活跃任务的澄清可继续使用原 scope；用户明确提出独立任务时创建新
taskId、退休原 grant。模糊 follow-up 不自动继承更广权限，scope 不明才询问。
不用关键词分类直接授权，也不靠“我授权了”正文修改 Host 权限。

### 4.3 批准范围

卡片默认展示任务目的、可读范围、允许读取/输入的能力、仅本次任务、随时停止。
“调整范围”展开进阶选择；不要默认展开四个 surface、冗长协议说明和全部诊断。
用户可一次批准多目标，也可缩小模型建议范围。不要为减少点击而无条件授予整个桌面。
若任务确需全桌面，只有在平台实际支持且用户明确批准该范围时才允许；必须与应用范围
有明显区别，不作为任何失败的隐式 fallback。保留已有更严格的危险动作合同，
不要另加一套按每次 click/scroll 确认的 CU 权限机制。

| surface | 可一次批准的范围 | 内部切换与失效规则 |
| --- | --- | --- |
| Desktop 应用范围 | 用户批准的一组应用实例；Host 用 PID+start、原生对象身份和授权根确认 | 可验证属于实例的窗口/对话框在范围内；不是仅按名称、标题、路径或父 PID 自动归属；进程重启需新身份与按需增量确认 |
| Managed Browser | 本任务 owned profile/context；明确是否含本任务新 tabs/popups | 普通导航/new tab 在批准策略内免重批，但 document/ref 要换新；不得借此读日常 profile |
| ExistingTab | 真实配对、真实分享的 tab/browser identity，加 App 任务 grant | 导航仍必须满足扩展权限；失去 activeTab、另开 tab 或 pairing 失效不能伪成同一授权；实际缺工具栏手势时给准确入口 |
| App WebView | 明确 view label 与当前 instance | 同 view 普通导航允许新观察；不同 view 或 replacement 不继承；不共享其它面 cookie |

Host 可在用户打开范围选择时显示本地候选的必要名称，候选不会事先发给模型。
未批准前不为展示卡片截屏、不读取窗口正文、不 eager 遍历用户 tabs 给模型。
批准 scope 内的目标选择与“批准 scope”分离；模型可切换范围内目标，但动作必须绑定当前
target 与新观察。旧 ref 不能跨窗口/导航复用。目标 focus/activation 如果新增，也必须
经过 scope 身份检查、平台支持和取消 fence；用户物理接管造成的 pause 不能被模型抢回焦点。
平台只能提供全屏/全桌面时，不把它包装为已隔离某应用；明确不可用或请求真实全桌面范围。

### 4.4 生命周期：少重复操作，不保留过期执行权限

| 事件 | task scope / 执行处理 | 用户体验 |
| --- | --- | --- |
| 同 scope 的目标切换/正常导航 | task grant 留存；旧 snapshot/ref/geometry 退休；绑定新目标再观察 | 无重复 App 授权 |
| 暂停/接管 | 留存批准范围的非执行记录；取消 work、退休观察、清理输入 lease | 停止派发；一个“继续” |
| 同 session/agent 的用户继续 | Host 点击重新确认原范围；等待物理 idle、复验真实身份、重新取得 lease、fresh observe | 不重新选目标；失败准确说明，不偷偷执行 |
| compact/换模型/ACP 重连 | fence 原执行和 continuation；仅同 App session 可保留非执行范围记录 | 一个明确“继续此任务”重新绑定；不能后台直接复活旧 grant |
| Stop/撤权/feature-off/任务完成 | 取消请求、grant 和 continuation；先 fence 后 join；终态 | 不能仅点继续复活；新任务新请求 |
| App 重启、fork、账号切换、workspace root/remote 身份变化 | 无活 grant 继承；仅保留安全状态说明，旧请求失效 | 原任务显示已结束/需新申请，不自动执行 |
| 系统权限撤销/锁屏/目标死亡 | 立即 fence；原短期绑定失效 | 解锁或系统授权不自动继续；明确按钮、必要系统手势 |
| DPI/拓扑变化 | geometry/ref 失效；按适配器风险暂停/重新观察 | 不无故回到目标选择器，未复验前不输入 |
| 另一会话争用全局 desktop lease | 维持排他与明确 busy | 不踢掉另一任务，不复用对方授权 |

continue 只由 Host 用户事件执行。模型只能请求继续/接管，不能调用 authorize/resume/reconnect。
迟到批准/观察/cleanup 必须复验 task/request/grant/ACP generation。
Stop 后零新副作用派发；stop_requested 与物理 stopped 分开。原 owner 未 join 不释放占用。
unknown 副作用不重放，恢复先观察真实结果。旧 cleanup 不得影响新 owner。
远程 IM、SSH、定时/后台自动化不继承本机 interactive 的申请能力或执行权限。

## 5. 分阶段任务表

每一阶段先交出可审查的小 diff 和实际 evidence，再进入已满足依赖的下一阶段。
每项分开记录 implementation 与 verification。一个外部平台缺人不阻止其他独立工作，
但未跑的真实批准、原生、安装态不得标绿。

| ID | 主要交付 | 依赖 | 允许并入的范围 |
| --- | --- | --- | --- |
| U00 | 基线、CLI/ACP 事实验证、授权 ADR | 立即 | 文档/owned probe；不先大改产品 |
| U01 | task grant/request 状态和受限 IPC | U00 | core/Host API、协议、核心回归 |
| U02 | 会话预连接、审批异步续行 | U01；U00 CLI 结论 | session_manager/ACP/MCP 和生命周期 |
| U03 | 聊天授权卡、面板整理、15语言 | U01/U02；UI 可用契约先行 | 领域组件/API/style/i18n，禁增胖 App 壳 |
| U04 | 多目标 scope、范围内切换、增量申请 | U01/U02/U03 | scope membership + adapter 接线 |
| U05 | 一键继续、fresh observe、排他恢复 | U01/U02/U04 | broker/Host/controller/task card |
| U06 | 撤权、feature-off、重连与故障闭环 | U01–U05 | 所有受影响生命周期，无新 unrelated 功能 |
| U07 | 真实 App/ACP/MCP 和交互计数验收 | 各路径能运行 | owned fixtures、真实 CLI 与独立 oracle |
| U08 | 全量门禁、平台矩阵、文档/技术债审查 | U06/U07 | 本批相关质量和准确交接 |
| U09 | 新开发包与最终交接 | U08 Windows 当前候选可用 | 独立本地开发包；不安装、不发布 |

U02/U03 可按已冻结的协议交错完成第一条纵向路径：U02 先用 owned 测试客户端验证 Host
请求/回执，U03 接入真实卡片后再关闭“正常会话一次批准自动续行”验收，不循环等待彼此。
这不降低 U07 的真人批准和真实模型闭环要求。

### U00：先把可执行路径验证清楚

1. 重查 HEAD/dirty、真实 Grok CLI binary/version、App/agent home、live writer，记录基线。
2. 阅读 AGENTS.md、CONTRIBUTING.md、Computer Use/i18n/dialogs/settings/catalog/maintain wiki
   和第 3 节代码；旧 checkpoint 只读与当前授权链相关部分，不重读几天全部日志。
3. 使用 owned local interactive ACP 会话验证：无需 run 时能否注册无权申请工具；
   tool pending 后用户迟到批准如何继续原 session；CLI normal 的 MCP use_tool 权限
   是逐动作、逐工具还是会话范围；取消/EOF 后是否仍可安全决定和续行。
4. 与现有 UiPermissionRequest 管道对照。能复用的外观/路由复用，CU scope 仍由 Host broker
   判断；不能通过 allow_all、--always-approve 或共享 config 绕过其他工具权限。
5. 写 task-consent ADR：任务所有者/期限/完结、feature-off bootstrap、请求 schema、目录绑定、
   pending continuation、CLI 二次提示、scope membership、暂停/compact/重连、日志脱敏。
6. 对缺能力写可复现证据和范围内替代。禁止以“暂时不支持”放弃按需卡片或恢复体验；
   若必须依赖 CLI 升级，记录最小版本、兼容降级 UI 和不可实现的精确验收行。

关闭条件：真实消息时序、目录与权限结果可复核；设计没有“审批时热换 busy turn 工具”
或“等 5 分钟的普通 HTTP”死结。ADR 的未决项不能让后续实现各自猜一套规则。
不以 mock ACP 或 YOLO 探针作为 normal 模式点击数证明。

### U01：核心请求与任务授意

1. 把 Host task grant 与短期 run/target/snapshot 分开，复用现有 SessionGrants、ticket、
   Broker 身份机制；避免在 React、commands、broker 各存一份权威授权状态。
2. 为 request、decision、scope、grant revision、continuation 定义严格类型/schema，
   Rust/TS/JS 与 golden 保持一致；未知字段、越界/过长字符串、无效枚举明确拒绝。
3. 将 IPC credential 分成 session bootstrap 与 execution 的等效权限层。
   即使共用 transport，未授权分支也只能调用明确 allowlist；run/session 参数不能越权。
4. Host-only approve/deny/cancel/continue/revoke 用真实当前 App session、request generation、
   agent identity 校验。审批准备有 cancel token；点击和 commit 之间的 Stop 使批准失败。
5. 保留有界请求数、消息长度、速率；request flood 不挤占 Stop；状态查询不产生 capture。
6. 决定幂等但不能重执行。approved 结果可回读，旧请求在 stop/reconnect 后不可批准。

验收：未授权跨所有读取/动作入口零效果；伪造 session/run/task/request/revision 拒绝；
重复批准只 commit 一次；拒绝/过期/取消/Stop 的 late approve 不能建立 run；敏感内容不进日志。
真实到断言的失败先保留，再最小修复；避免只测试序列化等于构造器的镜像断言。

### U02：工具预连接与异步审批续行

1. 本地 interactive 会话在合适的初始连接/ready 边界安装申请能力，避免先授权才 attach。
   static catalog 或等效目录在授权/暂停中保持可用；所有执行工具仍在 Host 严格 gate。
2. reconcile 保留用户 MCP/official-aux，不覆盖 unrelated 配置；desired/applied generation、
   cleanupPending、连接失败和 owner 退休语义同步调整。Stop 不再盲目等同整个 session
   申请 transport 销毁，但旧 execution binding 必须退休；session 退出时全部撤销。
3. request 返回短 pending。由 Host 决策事件发出一次 continuation；如果 turn 未空闲，
   等正确边界后消费。不要在忙锁里等待审批，也不要重复注入原任务造成两次执行。
4. 状态从 request→prepare→committed→observing→running；只能 committed 决策触发续行。
   continuation 带 session/agent/request/grant generations，领取后有独立消费账本。
5. deny/cancel/expire 返回可读结果；模型不每秒查询、不再直接要求“去面板选目标”。
   更新 MCP instructions：需要电脑时先申请，scope 内观察→动作→验证，外部追加申请，
   待用户决定时停止派发，Stop/unknown 规则保持。
6. 核验 normal CLI 泛工具确认的去重方案。只允许对本次已批准 scope 的 CU 例程生效，
   不关闭 shell/file/其他 MCP 的确认，不在产品里悄悄使用 --always-approve。

验收：busy turn 内首次申请成功；90 秒批准没有原 HTTP 悬挂或 timeout unknown；只续行一次；
目录未丢其它 MCP；Stop/agent exit/会话切换后零旧 continuation；拒绝不自动二次弹窗。

### U03：聊天授权卡与 CU 面板

1. 利用现有 ComputerTaskCard/Live 及权限卡模式构建 request card。未有 run 时也能显示。
   领域状态放在 computer-use 模块/provider/hook，不新增 App.tsx/AppWorkbench 大块状态。
2. 正文默认只有目的、范围、期限，主按钮“允许本次任务”，次按钮“拒绝”，范围调整折叠。
   feature-off 的首用主按钮清楚写“启用并允许本次任务”；不再跳设置后再批准。
3. 批准中防重复点击，取消/Stop 不被其他 busy 一起禁用；系统授权等待准确显示，
   scope 候选失效给重新选取入口。打开卡片不自动授权、不自动激活用户窗口。
4. 面板变为预览/状态/已允许范围/暂停/继续/停止；surface 与 picker 留在高级范围管理。
   技术说明与 runtime details 默认折叠，错误按可执行原因说明，不堆 token/runId/IPC。
5. 共用 Host 回读和事件源；去重现有 800ms 轮询/订阅，隐藏预览停止 capture。
   会话切换迟到值不覆盖新卡；状态 stale 不冒称运行/停止，仍可停止确切旧 run。
6. 15 locales 完整同步 en key；新设置登记 settingsCatalog，删除旧“授权后才有工具”文案。
   复用 Select/ContextMenu/GlassModal/项目卡片样式，禁 window.confirm 和原生 select。
7. 用键盘操作所有决策/折叠/停止；focus 返回合理位置，aria 状态明确。
   300/320px 侧栏、900px 窗口、light/dark、100/125/150/200% DPI、中文和长语言检视。

验收：卡片全部 pending/error/empty/denied/expired/busy/reload 路径；无遮挡/穿透/透明菜单；
不需先打开面板；无重复卡/重复按钮；UI fixture 通过后仍需原生 App 正向走一次完整流程。

### U04：同任务多目标与增量授权

1. Host 将用户一次批准的范围解析成可验证 identity 集合/规则，能力单独检查。
   不能按任意进程同名、窗口标题或父 PID 把无关应用纳入。
2. Desktop 多窗口 membership 用平台真实关联（根/owner/进程 start 等）交叉验证，
   每次派发前复验。授权应用新对话框按明确规则进入，身份歧义 fail closed。
3. Managed Browser scope 对 owned context 与 new tab/popup 策略接线；替换 owner/profile
   不继承旧 grant。跨 tab 重新观察，未知操作不因切换而重放。
4. ExistingTab 将 App scope 与扩展 Share/activeTab 权限分开；读取、截图、动作都验证。
   失去浏览器权限时只显示必须的真实手势入口，不制造一串相同 App 授权卡。
5. WebView 绑定 label+instance；导航替换文档身份，不跨其它 view/cookie。
6. 范围内模型 open_target 可以选择 scope 内身份，Host 原子绑定/退休原观察。
   范围外返回 needs_access 结构化结果并发起增量请求，不读取范围外内容探路。
7. 增量拒绝/失败不撤原 scope、不重启整个任务；扩大/缩小 grant revision 原子生效。
   缩小范围时取消被撤销目标的工作，原回包不能重新加入权限。

验收：两个 owned 应用窗口跨目标 5 次、受管 tabs/popup、WebView 导航；原 ref 跨目标拒绝。
范围外目标、PID/HWND/tab/view 复用、parent spoof、同名进程负向；新增范围只批准一次。
OS 不支持细范围时保留真实能力矩阵，不使用全桌面绕过测试。

### U05：一键继续，后台复验与观察

1. 重构 drop_execution_identity 的用途：范围记忆、短期执行目标、snapshot/ref/geometry
   分开退休。原 resume_drops_authorization_snapshot_and_lease 等测试按新合同更名/改断言，
   保留“旧执行身份不可用”断言，不仅删 target 清除就宣布完成。
2. continue 由用户一次点击进入 resuming。Host 校验同 task/session/agent/revision、
   scope identity、平台权限、原 adapter idle 与 cleanup；不得长持 broker mutex 查询 adapter。
3. cleanup 未结束时保留准确等待状态，原任务不派发；原 owner 确认退出后重新取得必要
   lease/绑定，fresh observe 成功才进入 running 和消费 continuation。
4. 观察后新 snapshot/geometry/ref 只发给正确模型会话。UI preview 不代替模型观察。
   目标死亡或系统撤权不能自动替换为类似窗口；可按需出现一次增量/系统手势入口。
5. resume/approve/Stop/再次 Pause/feature-off 交错，用同一任务 generation fence。
   双击 Continue 只一轮恢复；迟到观察不能变绿；unknown 动作结果先核真实状态不重放。
6. 卡片和面板共用 Continue 实现；不让用户还要重新选 target、点授权、发“继续”。
   compact/ACP 重连需明确 Host 用户继续的规则也接入，不能模型自行恢复。

验收：pause→continue 只有一次点击；旧 refs 在 fresh observe 前后均拒绝；新观察后动作
确实改变 owned fixture；慢 cleanup、target death、lease held、permission loss 均无越权输入；
恢复查询不堵 Stop，原 cleanup 不影响新 owner。

### U06：所有撤权和失败路径闭环

1. 对第 4.4 节每个事件检查 commands→SessionGrants→Broker→adapter→catalog→UI。
   任务授意、pending request、continuation、执行 lease、物理 resource 分别有所有者。
2. feature-off/Stop/revoke 先 fence 后清理。approve preparing 中关闭 feature 不可重新开启；
   request 卡和执行卡可如实退到 cancelled/stop_requested/stopped。
3. agent/session/account/root/remote/fork/reload/connection ABA 使旧 credential/ticket/decision
   失效；普通 React remount 不意外复活任务或双消费；App restart 无活 grant 继承。
4. 保留独立 Stop 槽、有限速率和 backpressure；flood access/status 不阻 Stop。
   物理接管、lock、TCC/portal/扩展撤权及时退休输入；自动 focus 不抢回用户控制。
5. 明确 transport timeout 前后 not_started/unknown/verified；unknown 不重放，
   原始失败和 unknown 证据保留。诊断不记录输入正文/页面内容/token/原始截图。

验收：竞争负向通过生产入口和独立计数；不用 mock 的“cancelled=true”代替真实 worker 收尾。
本批碰到原 Wayland backend 缺陷时登记与 D01–D06 的依赖，只修必要范围；缺原生机器继续
可执行的 Windows/browser/协议/测试路径，不耗整轮等待或改用户系统。

### U07：真实业务闭环与点击数

1. 用 branch-built App、真实 CLI normal 模式、真实 ACP/MCP、owned fixtures 验任务：
   请求→卡片→真人批准→模型观察→真实动作→业务后置条件→暂停→一键继续→完成。
2. CLI stub、合成请求或测试注入能做否定/竞争，不能代替模型真实按需请求或真人批准。
   headless --always-approve 的隔离探针只能证明 transport，不得计 normal 点击数。
3. Windows 原生应用至少两种、managed browser、installed Chrome/Edge ExistingTab、
   App WebView 分行验；macOS 两架构、Linux X11/native Wayland 缺机器就写 awaiting_external。
   共享 core 绿不等于全平台已验；尤其浏览器正向不是桌面正向。
4. 对连续 20 actions、多应用切换、增量范围、90 秒批准、暂停恢复、拒绝后替代路径，各运行
   至少 3 轮，保留每轮 source/runtime/CLI hash、实际批准/系统/CLI 点击数和业务结果。
5. timing 分段：模型决定申请、Host request→card paint、用户等待、prepare/OS consent、
   commit→续行→first observation、pause fence、continue→fresh observation。用户/网络等待
   不混入本地 UI 延迟；不通过放大 timeout 假装不卡。
6. 本地 card paint、同范围控制回执建议 p95 <= 500ms（不含 OS/模型/network/物理清理），
   给实际样本数/原始分布。若未达标修主线程/重复轮询/串行锁原因，不只优化摘要数字。

关闭条件：第 1.1 节核心体验有真实正向与负向证据；不能仍“用户先手工开面板授权”。
没有真人授权则准备可一次执行的测试步骤并明确未验，不自动点批准、不伪造人类 evidence。

### U08：门禁、准确文档和复审

先跑受改动影响的有意义测试，最终候选再跑 CONTRIBUTING/当前 CI 要求的全量门禁：

```text
pnpm deps:check
pnpm typecheck
pnpm test
pnpm lint
pnpm build:ui
pnpm audit:prod
node --test tools/computer-use-mcp/*.test.mjs
在 src-tauri：cargo fmt --all -- --check
在 src-tauri：cargo clippy --all-targets -- -D warnings
在 src-tauri：cargo test
git diff --check
```

确认实际 workspace/crate/平台 feature 与 CI runner；新增 protocol/scenario runner 接进对应
CI，记录实际命令，不复制一条根 cargo test 冒充所有 helper crate/native 矩阵。
Windows libtest 如遇 0xc0000139，使用已有 CI 的 mt.exe post-link manifest 方案；
程序未进断言就写 setup failed，不能标 test passed，不能在 build.rs 加冲突 MANIFESTINPUT。
不要反复运行已绿全套填时长；改源码/hash 后重验受影响的 evidence。

更新 computer-use wiki/用户说明/源码模块文档：按需授权、默认 off 首用、范围与系统权限、
继续/Stop/任务期限，以及明确当前平台边界。不把历史实验 checkpoint 改成全完成。
检查 15locale 键完整、无硬编码、settings registry、无 placeholder/TODO 权限绕过、
无重复权威状态、无 unbounded queue、无全局 always-approve、无 App 壳增长。
旧测试若改合同要解释原场景仍如何守住，不删除原始失败与平台未验项。

交付本批 verified_scoped/partial 结论；整体 D00–D31 仍未关闭，不把本批成功写成完整 CU 发布。

### U09：开发包与最终交接

Windows 本批门禁通过后，从固定源码/runtime 候选构建独立 CU Dev 安装器和 portable。
沿用项目已有开发包方法与 docs/BUILD.md，不覆盖用户安装、原开发包或现有 evidence。
本批代码未提交，不能只 git archive HEAD 打包而漏掉修改。冻结工作区当前候选到独立构建
snapshot，包含本批所需 tracked 修改和新增产品文件；记录基线 SHA、patch digest 与逐文件
manifest。明确排除账号/秘密/日志/VM/profile，构建前后复验候选一致，不在 live 工作区加
build-only overlay，不为了固定版本自行 commit。
写 source fingerprint、CLI/runtime 版本、artifact SHA256、build record、使用/回滚说明。
native 启动和随包资源 smoke 与本批授权链测试分开记录；未执行安装器就明确未安装验证。
若还缺平台/签名/真人验收，包仍标开发版，不能对外发布、自动安装或自动升级用户软件。

原开发包供对照：H:/aicoding/releases/grok-cu-dev-20261002-8d8b71c2。
新包交 Codex 审查后再由用户选择使用。外部工具缺失时保留构建脚本和精确失败，
不降低 U01–U08 交付要求，也不报“新包已完成”。

## 6. 必须覆盖的场景清单

这是最小有意义矩阵，补原生差异即可；不用大量 mirror/mock 行数冒充验收。

| ID | 场景 | 必须检查的结果 |
| --- | --- | --- |
| S01 | 普通文本任务 | 无请求/无 worker/capture/input |
| S02 | feature-off 首次请求 | 卡片明确启用；未点前零内容读取/输入 |
| S03 | busy turn 首次请求 | 不要求先结束 turn 或进面板；其他 MCP 保留 |
| S04 | pending 90 秒后批准 | 有界等待，续行恰好一次，无重发原任务 |
| S05 | 拒绝 | 零效果；模型不刷屏再申请 |
| S06 | 请求 TTL 过期与同时批准 | 一个确定结局；过期请求不能 commit |
| S07 | 重复 tool 请求/重复事件/双击批准 | 一张有效卡，一次 commit/continuation |
| S08 | 伪造 task/run/session/request/unknown field | 严格拒绝，无敏感状态泄漏 |
| S09 | 同 scope 连续 20 次动作 | 真实后置条件，额外确认 0，旧 refs 不复用 |
| S10 | 两个批准应用切换 5 次 | scope/身份正确；无额外授权、无无关目标访问 |
| S11 | 同名进程/窗口 HWND 或 PID 复用 | 不继承 grant，不错 target |
| S12 | 批准应用的真实关联 dialog | 正确 membership；伪造 parent/owner 不加入 |
| S13 | 增量范围批准/拒绝 | 新 scope 一次决定；拒绝保留原 scope |
| S14 | 执行中缩小 scope | 被撤销目标即时 fence，迟到结果不恢复 |
| S15 | managed navigation/new tab/popup | 策略内免批准；doc/ref 更新，不串 profile |
| S16 | ExistingTab 真实 toolbar Share | 配对、Share、App scope 都满足才读取 |
| S17 | ExistingTab activeTab 丢失/新 tab/扩展重启 | 不伪造权限，只请求必要实际手势 |
| S18 | WebView 导航/label replacement | 同实例 fresh observe；不同实例不继承 |
| S19 | pause→one-click continue | 旧身份无效、新观察后真实动作、只有一次点击 |
| S20 | 慢 cleanup/双击继续/Stop 与继续竞争 | 不提前输入、不双恢复，Stop 可达 |
| S21 | approve prepare 中 Stop/feature-off | 不迟到建立权限，不自动重新启用 |
| S22 | session switch/并行会话/desktop lease 冲突 | 无串卡/串权限，不踢另一 owner |
| S23 | compact/model/ACP reconnect 与 late decision | 旧请求失效，明确用户继续，不自动复活 |
| S24 | App restart/fork/account/root/remote 切换 | 无活 grant 继承，零旧 continuation |
| S25 | 目标死亡/进程重启/权限撤销 | 不 silently 替换目标；按需新绑定 |
| S26 | 用户物理接管/lock/unlock | pause/fence；模型不能自动抢焦点或继续 |
| S27 | DPI/显示器拓扑变化 | 旧 geometry/ref 拒绝，正确重新观察 |
| S28 | request/status flood + Stop | 有界速率/内存，Stop 槽保留，无 post-fence 派发 |
| S29 | 派发后断连/timeout unknown | 不重放副作用；新观察核真实结果 |
| S30 | macOS TCC / Wayland portal 拒绝与撤销 | 准确状态，无自动系统批准，无桌面 fallback |
| S31 | UI remount/reload/隐藏面板/状态 stale | 卡片去重、无闪旧帧、Stop 可用、预览隐藏停采 |
| S32 | normal CLI 通用工具权限 | 实测总点击数；CU 批准不放行 shell/其他 MCP |
| S33 | 完成/idle TTL/独立新任务 | 原 grant 退休；end_turn 等审批不误判完成 |
| S34 | 窄窗/15语言/深浅色/DPI/键盘 | 无裁切/穿透，决策与停止均可操作 |
| S35 | 新包与旧包对照 | 源码/runtime/hash 可追溯；开发包不冒充正式 release |

## 7. 证据、断点与交付格式

每阶段使用新的 ignored evidence root；先验证实际 gitignore，不提交 profile、token、
原始截图、core、VM 或输入内容。开发计划/checkpoint 可入 docs，普通日志必须脱敏。
任务字段建议：id、implementationStatus、verificationStatus、evidenceLevel、sourceFingerprint、
runtimeFingerprint、cliVersion、commands、assertions、firstFailure、remaining、nextReadyTask。

E1=unit/mock；E2=真实进程/IPC；E3=branch-built App+独立 oracle；
E4=安装 App+真人授权+真实 Grok；E5=跨平台/安装/故障矩阵。
实现状态和验收状态分开；not_run/unknown/awaiting_external 不写 passed。
模型测试报告记录 App 批准、CLI 批准、系统授权三类计数，实际效果与权限违规计数独立。
硬性安全计数：unauthorizedRead、unauthorizedInput、wrongTarget、postFenceDispatch、
duplicateContinuation、unknownReplay、crossSessionGrant、wrongOwnerCleanup 全部为 0。

每完成一个原子项更新 manifest，每 60–90 分钟写短 checkpoint；持续执行下一 ready 项。
同一假设最多三次且需新证据，没有新证据转其它独立项；保留首败、不用成功重试抹掉失败。
全部剩余项确实依赖真人/机器/CLI 外部变更时，提交精确交接；不要忙等冒充长任务。

最终给 Codex：

1. 可审查 diff 与 U00–U09/S01–S35 对照，现有行为与新行为、各依赖/平台限制。
2. 真 CLI/ACP 的消息时序和 normal 模式点击数，scope/continuation ADR。
3. 实际命令、退出码、已进入断言的结果、第一失败、当前 source/runtime/CLI fingerprints。
4. 真实 App 场景效果，原资源清理/join，未验/人工缺口及可一次执行的验收步骤。
5. 独立开发包或准确构建限制；新增技术债、必要的下一批总体 D 任务。

Codex 审查前不提交或发布；本批通过后仍保留整体 partial，下一轮再安排 D00–D31 收尾。

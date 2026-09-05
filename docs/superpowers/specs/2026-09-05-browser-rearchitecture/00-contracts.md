# 浏览器重构共用合同

状态：设计与任务接口基线；尚未实现。日期：2026-09-05。

本文件细化[总体设计](../2026-09-04-browser-rearchitecture-design.md)，用于十个独立工作包之间的接口对齐。修改公共字段、权限边界或数据所有权时必须更新消费者规格、任务依赖与协议 fixture。产品现状以 `a43e3071` 为调研基线，文档基线为 `a76e786c`。

## 领域与职责

| 模块 | 持有内容 | 禁止持有的权威状态 |
| --- | --- | --- |
| Rust `browser/protocol.rs` | 版本化 wire 类型、闭集结果、字段约束 | UI 文案、秘密输入持久副本 |
| Rust `browser/lifecycle.rs` | Session/Tab UUID、所有权、关闭/归档/fork/rewind | CLI 业务上下文 |
| Rust `browser/binding.rs` | connector、ACP connection、有效 turn、backend binding | 页面自报的身份 |
| Rust `browser/origin.rs`、`policy.rs` | canonical origin、规则、短期 grant、风险决策 | 模型自报风险作为放行依据 |
| Rust `browser/lease.rs` | ControlDomain、writer lease、fence、TakeoverLatch | 从 UI 布尔值恢复控制权 |
| Rust `browser/runtime/` | 签名 tuple、ProfileGuard、子进程和输入 Provider | 用户个人 Chrome Profile |
| Rust `browser/profiles/` | Managed Profile generation、迁移、长期 OS 锁 | Chrome 数据删除权 |
| Rust `browser/artifacts/`、`transfers/` | 不可变 blob、owner、上传 staging、下载 export | 未授权绝对路径返回值 |
| React `src/components/browser/` | Host snapshot 投影、地址草稿、展示/焦点/菜单 | 第二份持久 BrowserSession |
| Worker `browser-runtime/worker/src/` | Playwright adapter、原子动作、受限结果 | 独立授予权限或延长 lease |
| 扩展 `browser-connector/extension/` | 经 Host 批准的 tab/group claim 映射 | 窗口所有标签默认授权 |

`src/App.tsx` 与 `src/app/AppWorkbench.tsx` 不增加 feature state，总行数只能下降。接入通过现有 SideWorkbench/领域 hook/provider 完成。M0-W01 创建公共类型和跨语言 fixture；其他包扩展各自模块，不向协议文件塞业务实现。

## 身份与版本

所有 UUID 由 Host 创建。URL、tab 标题、WebView label、CDP targetId 和 Chrome tab/groupId 不能当身份。AppSession 是 Grok App 会话；Grok Build 的 agent session ID 只存在于 AgentBinding 内，不与 AppSession 混用。

`protocolVersion` 初始为整数 `1`。generation/revision/fence 使用非负 `u32`，wire 为 JSON number；达到上限时阻止操作并建立新 boot/binding，不允许回绕或精度损失。持久 release sequence 使用十进制字符串并按无符号整数比较，与 tab revision 无关。

```ts
type ProfileRef =
  | { kind: "managed"; profileId: string; generation: number }
  | { kind: "preview"; partitionId: string }
  | { kind: "chrome"; connectionId: string; claimId: string };

type ControlDomain =
  | { kind: "managedRuntime"; runtimeId: string }
  | { kind: "previewWebView"; bindingId: string }
  | { kind: "chromeConnection"; connectionId: string };

type BrowserAction =
  | { kind: "navigate"; url: string }
  | { kind: "back" | "forward" | "reload" | "stop" }
  | { kind: "snapshot" | "screenshot" }
  | { kind: "click"; targetHandle: string }
  | { kind: "type"; targetHandle: string; text: string }
  | { kind: "scroll"; targetHandle: string; x: number; y: number }
  | { kind: "upload"; targetHandle: string; uploadGrantId: string }
  | { kind: "downloadSave"; downloadId: string; exportGrantId: string }
  | { kind: "inspect"; channel: "console" | "network" | "trace"; cursor: string | null };

interface RequestEnvelope {
  protocolVersion: 1;
  requestId: string;
  hostBootId: string;
  appSessionId: string;
  browserSessionId: string;
  tabId: string;
  backendBindingGeneration: number;
  runtimeGeneration: number | null;
  expectedTabRevision: number;
  expectedNavRevision: number;
  agentBindingId: string;
  turnId: string;
  leaseId: string;
  fenceEpoch: number;
  policyRevision: number;
  action: BrowserAction;
}
```

上例定义 live tab 动作的 wire 形状，不是要求 Agent 自行填全。MCP adapter 根据已认证连接与 Host binding 填入身份，模型只提供语义参数；Host 仍逐关系复核。`targetHandle` 来自当前授权 snapshot，绑定 tab/frame/navRevision 和短 TTL，不接受任意页面脚本作为 target。

尚无 tab 的 create/list/open 动作使用独立 bootstrap API：认证连接 + AppSession + BrowserSession ownership；Host 新建身份、审批所需 origin 后返回 binding。不得用全零 UUID 或空 lease 绕过 live tab validator。可信 App UI 的暂停/接管/交还通过独立 Tauri command，MCP 不提供“替用户交还”。

M2 人工 Design/检查使用同类可信 Tauri 入口申请 `UserInspectionPermit`，不伪造 AgentBinding、turn 或 lease。Host 先 fence Agent 并等待取消完成，再授予限 AppSession/BrowserSession/Tab、binding/nav revision、origin、操作集合和有效期的人工 permit；结果只发给发起的可信面板。permit 保留 TakeoverLatch=user、禁止 Agent 观察与输入，导航/切换/关闭/过期后失效。用户显式发送检查结果到 Composer 草稿只转移批准的 Artifact，不清除接管锁存。人工检查的 viewport/overlay 原语单独列入允许集合，不授予一般页面动作。

Rust wire 使用 `serde(rename_all = "camelCase")`，枚举使用显式 tag。未知版本、未知 action、重复字段、超限数值与必填缺失返回 `invalid_request`，不能默认为低风险操作。新增可选字段只在 capability negotiation 声明后使用；新 action 必须双方明确支持。

## 结果与事件

```ts
type BrowserStatus =
  | "ok" | "denied" | "stale" | "unsupported" | "invalid_request"
  | "needs_confirmation" | "needs_takeover" | "retryable_failure"
  | "interrupted" | "unknown_outcome" | "runtime_capacity";

interface BrowserResult {
  status: BrowserStatus;
  requestId: string;
  reasonCode: string;
  tabRevision: number;
  navRevision: number;
  artifactIds: string[];
}
```

`reasonCode` 来自 Host 常量表，经 i18n 映射，不把异常堆栈直接作为 UI 文案。`unknown_outcome` 表示可能发生外部副作用，不表示可重试。`retryable_failure` 只允许尚未派发的只读/控制连接步骤重试；网站动作没有一般业务幂等保证。

`browser://event` 只发送到可信 App 窗口，携带 boot、Session/Tab、binding/runtime generation、tab/nav revision、事件序号和 payload。MCP event 通道另做当前 lease/grant/观察权限过滤；页面拿不到 Tauri IPC。React 遇到旧 generation 丢弃，事件序号缺口则向 Host 请求 snapshot，不能按到达顺序盲写状态。Host 原子 checkpoint 成功后才公布持久状态变化。

会话/标签创建和关闭、导航 committed、Profile 绑定等是持久事件；加载进度、鼠标位置、续租等只留内存。整页 DOM、截图、响应体写 Artifact，event 只发句柄或有界摘要。

## 权限与动作批准

统一决策次序：认证连接 -> owner/binding -> generation/revision -> InputFenceProvider/TakeoverLatch -> lease -> Host 计算的 origin/frame -> capability -> policy -> 动作风险/批准 -> durable inflight -> adapter 派发。

`OriginPolicyRule` 为持久配置；`OriginGrant` 是短期授权。turn grant 随 turn 结束撤销；thread grant 可跨有效连接内的 turn 存活，但每个 turn 重新登记且重新申请 lease。turn 由 Host `active_turn_id` 和实际完成状态确定，提前 `prompt_complete` 通知不结束仍有工具/审批的轮次；终止 hook 幂等且绑定原 turn。断连、重启、Profile/backend 改变与关闭/归档都撤销 grant。M1-G02/G03 验证全部六种现有 policy，尤其 `dont_ask` 直接拒绝需要新审批的动作。

模型、DOM role、按钮文字和 CSS selector 都不能证明动作没有外部后果。Gateway 接受的低风险类别来自版本化 action policy/受支持 adapter；未知 click/type/导航后果需要用户批准或返回拒绝。批准绑定目标、参数摘要、origin/frame、navRevision、有效期和一次消费，不只是“以后允许 click”。页面改变使批准失效。对任意网站未知脚本，不宣称可以自动识别全部支付/删除/发送行为。

上传 attach 的批准明确授权把指定文件披露给指定站点；网站可能立即上传，后续撤销只能阻止新动作。表单最终提交仍单独审批。秘密输入一律人工接管，观察/截图/Console/Network 同时停；支持 bundle 不收取输入值。

M1 路径上传的 200 MiB 上限由不可变私有 spool 支撑。路径交付 worker 前持久登记 `UploadSpoolLease(runtimeId,runtimeGeneration)`，attach/CDP ACK 不证明浏览器已接管全部字节；派发后的副本保留至该 Runtime descendants=0 才物理删除。撤权立即关闭能力，Session 关闭后可留下明确的 Runtime-owned cleanupPending；计入总预算，Doctor 提供占用和显式停止 Runtime 清理入口。未派发副本可立即清理。M3 用户 Chrome 没有可强制终止的 Runtime，须独立证明字节接管与清理，不能把连接断开视为浏览器读取结束。

Full CDP 不属于 M1 默认能力。任意 CDP 可能绕过字段脱敏、动作分类和 grant，单次 Developer Mode 批准不能免除这些约束。M2 优先 capability-scoped proxy；无法证明任意 CDP 同时满足总体硬门时，Full CDP 保持 `unsupported`。独占 Runtime 与 ephemeral Profile 是必要条件，不是完整安全证明。

## Fencing 与并发

TakeoverLatch 是 Host 权威状态，与 lease 正交。原生输入归因失败或权限撤销立即关闭 admission、递增 fence、撤销 writer/observer、取消队列并锁存 user。Chrome 以整个已绑定浏览器实例为原生输入范围，claim 只用于数据/动作范围。恢复监听不等于交还。

Worker 在每个 primitive 之前和每次 await 返回后检查 fence，但这不足以停止已进入 Playwright 内部自动等待的动作。M0-R05 必须证明取消、driver target detach 或作用域内 page/context termination 能阻止后续输入；最坏竞态记录 `unknown_outcome`。无法取消的 primitive 不进入可用 action catalog。250 ms 指 Host 收到可归因事件到拒绝命令并收到 worker fence ACK 的 p95，不是物理按键到网页零副作用的保证。

共享 Runtime 一次只有一个 writer；其他 Session 的观察需独立授权，接管时一起停止。关闭一个 Session 不杀其他 Session 的 Runtime。Profile lock 只有确认该 run descendants 全部结束后释放；进程名不是清理身份。

## 传输、超时与上限

| 边界 | 初始合同 | 超限/失效 |
| --- | --- | --- |
| Host 私有 IPC 控制帧 | 4 字节大端长度 + UTF-8 JSON，最多 1 MiB | 拒绝并断开不合法 connector |
| Chrome native messaging | 按受支持平台协议使用 4 字节小端长度 + UTF-8 JSON，本实现最多 256 KiB | bridge 显式转换帧格式，保持内部 envelope；大内容分块或 Artifact |
| 输入文本 | 单动作最多 64 KiB，禁止秘密字段 | `invalid_request` / `needs_takeover` |
| 普通动作 | 默认 30 秒；Host 下发绝对 deadline 与 lease expiry 的较早者 | 取消、fence；是否未知按是否派发区分 |
| 导航 | 最长 60 秒，超时后不自动刷新 | 可见 navigation failed，用户可重试 |
| 单 tab 等待队列 | 最多 32 条，单 writer | admission 返回 busy reason，无隐式丢弃 |
| 事件背压 | 最多 256 个 metadata event，溢出发 snapshot-required | 重新读取快照，不伪造连续数据 |
| 连接 heartbeat | 2 秒一次，连续 3 次丢失进入 disconnected | 撤销 binding/lease，监管清理独立运行 |
| Artifact | 大内容流入不可变文件，严格预算由 manifest 声明 | 清理 partial，保留明确失败记录 |

这些是版本化初始工程值，M0 测量可通过显式文档变更调整；不能无界扩张来隐藏失败。文件/截图大小具体预算由 M1-R05 明确，协议层不把大 blob 塞进 JSON。

## 存储与恢复

持久目录由平台 app-data resolver 提供 `browser/v1`。每对象 UUID manifest 为事实，索引可重建，使用现有原子写 helper 和 user-only ACL。Node 的 env allowlist 和 scoped path 只限制配置暴露，不等于 OS 文件系统沙箱；M0 必须验证真正的 OS 隔离或明确未提供的边界。

恢复：更新 boot -> 撤销易失权限 -> reconciliation 验证 lock/descendants -> 读取 metadata -> TakeoverLatch=user -> 按已批准安全恢复 policy 打开 URL 或 blank -> 等待用户显式恢复。staged download 只恢复等待保存记录；未知写入不重放。

M2 默认输出脱敏的 Grok Trace v1，含动作/Console/Network/授权截图索引；原始 Playwright trace.zip 可能带 DOM、headers 和 body，默认关闭，进入隔离诊断存储，未经结构化校验不能交给 Agent 或导出。未知 Trace schema 拒绝，不靠正则擦除几个字段冒充完成脱敏。

M5 的 Browser Bridge 复用现有 OpenSSH adapter、host-key 和目标校验，由 Host/Guard 双持有 loopback listener，并通过 `ssh -W host:port` stdio 转发。任一单点故障由存活方保持 tombstone 至隔离页面/channels/descendants 退出；多点故障未证明同等顺序的配置不能发布。Bridge 专属 mux 绑定完整目标、配置/认证摘要、host-key 和 tunnel/revision，keepalive 配在实际 master，不复用仅按 alias 缓存的旧 ControlMaster；无法验证则 dedicated。新 tunnel/revision 使用新的隔离 Profile，重连不继承旧 grant 或登录态。具体资源上限、断线和进程清理见 M5 规格。

M6 component updater 复用 M1 的 activation、LKG 与 security checkpoint；embedded/component manifest 可有不同签名域，但不得产生第二份 Runtime releaseSequence 或 tuple revocation 权威。旧 App/旧 embedded tuple 同样受最高 sequence、撤销与 Profile epoch 约束，不能绕过更新后的安全状态。

M6 同时升级 M1 distribution attestation：`officialApp`、`embeddedTupleDigest`、`componentAuthorization` 分别描述 App 官方身份、初始随包字节和允许的组件信任域/兼容范围。没有组件授权的旧 App 只接受原精确 tuple；新组件仍须完整验签、target/protocol/epoch/sequence 与当前 policy 验证，不因官方 App 身份直接获准。

M1-R06 向实际 App updater 提供 `beginAppUpdateQuiesce` / `abortAppUpdateQuiesce` / `commitAppUpdateQuiesce`。M1-D05 在可信资产下载/验证完成且用户 Apply 后，先 fence/drain Browser、确认 descendants=0 与 spool 清理，再进入安装；前置或安装交接前失败只恢复人工入口，不重建 Agent lease，也不停止 ACP/IM。Windows 使用可检查的安装器 ready/commit 交接，不能假定现有插件的 install 会返回；交接接受并退出旧 App 后发生失败，记录 unknown/failed 并提供下次启动或人工修复，不承诺旧进程仍存活。portable 继续仅手动更新 App。

## UI 与迁移共用规则

任务分组附着 AppSession，项目切换只改变展示投影。首版迁移把旧 SideTab browser 行转换为 Host Session/Tab，保存 migration version 与原数据备份；重复运行不重复创建。无明确 AppSession owner 的历史行需要当前用户会话触发认领，不能归给正在后台运行的其他任务。

AppSession 删除由同一协调服务收口，覆盖 commands、SSH 已导入会话和自动化失败清理；store 的最终删除必须验证 Browser 清理许可。首次 ACP 注入与 MCP 热更新也使用同一连接装配器，第三方设置变化不能删除第一方 Browser 或将旧列表投到另一会话。

地址草稿不覆盖真实 committed URL，IME Enter 不导航。前进/后退/停止根据实际 backend 能力；返回 `unsupported` 时解释原因并提供可用后端，不能显示可点但无效果的控件。任务关闭与隐藏视图分开，错误/空/忙/离线/锁占用都有可执行出口。

所有 UI 字符串以 `src/i18n/messages/en/` 为权威并同步十五目录，设置登记真实 `src/lib/settingsCatalog/entries/{runtime,general}.ts`。使用 `Select`、`ContextMenu`、tooltip 与 native-cover，支持键盘和焦点恢复。视觉压力为 en/zh/de/ru/ta、桌面与 390px 窄面板，不把描述性技术文案塞入用户主流程。

## 发布开关与回滚边界

以下是 Host capability registry 的稳定逻辑 key，不是新增用户权限开关；M0-W01 定义类型，归属任务接入实际发行策略。React 仅消费结果，flag=true 仍必须同时满足平台、签名、policy、Profile、输入 Provider 与单次授权，不能单独授予权限。

| key | 归属与开放门 | 关闭后的处理 |
| --- | --- | --- |
| `browser.previewV2` | M0-W06 | 停止新建新式组，保留 metadata 与可验证的旧 Preview 入口 |
| `browser.managed` | M1-G06、M1-R06、M1-D06 联合 | 阻断新绑定/lease；已有工作按有效策略有界 drain，数据保留 |
| `browser.inspection` | M2-06 | 撤销完整检查 permit/订阅，保留已允许 Artifact 和 M1 基础诊断 |
| `browser.namedProfiles` | M2-06 | 停止新建/绑定；已有引用先安全 drain，Profile 不删除 |
| `browser.chromeConnector` | M3-06 | fence、释放 claim、断开 bridge，用户 Chrome 标签与数据保留 |
| `browser.linuxManaged` | M4-06 且目标矩阵行通过 | Linux writer 关闭；Preview 与原 Profile 数据保留 |
| `browser.sshBridge` | M5-06 | tombstone listener、撤销授权并清理页面/channels，保留普通 public Preview |
| `browser.runtimeUpdates` | M6-06 | 取消组件下载/安装事务，保留验证过的 current tuple；安全 policy 继续生效 |

这些发行开关与 `block_new`/`stop_all` 的安全策略不同；远程强制终止仍须新鲜有效的签名 stop_all，不能由任意 UI 布尔值模拟。用户关闭、崩溃和本地安全故障照常执行 Supervisor 清理。CEF 没有生产启用 flag，只保留 M6-05 的独立实验决策。

## 需求编号与证据

需求编号在总验收矩阵中统一使用 `BR-01` 至 `BR-24`；开发任务使用 `M0-W01` 等稳定 ID，状态均为未开始。设计完成不等于开发完成。

每个开发任务交付源码、针对行为的测试和实际输出；每个里程碑额外交付真实包/平台/输入监听/发布证据。证据至少含 commit、tuple digest、OS/架构、fixture 版本、命令、退出码、用例计数、测量值和去敏附件 hash。未运行记 `not_run`，不能写 PASS。

新增 API/fixture/脚本是开发任务的产物，不是当前已存在文件。计划中的未来命令只能在相应创建任务完成后运行。仓库现有 mandatory CI 继续适用；质量门现有千行文件预算 79/77 未通过，由 M0-W02/M0-W05 定向抽取经手浏览器与会话生命周期模块并验证净改善，不提高预算掩盖问题。

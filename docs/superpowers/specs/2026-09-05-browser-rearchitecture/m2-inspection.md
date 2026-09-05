# M2：检查面板、Design Mode v2 与命名 Profile

**状态：** 可执行规格；本轮仅完成文档，运行验收尚未执行。
**日期：** 2026-09-05。
**上位合同：** [总体设计](../2026-09-04-browser-rearchitecture-design.md)、[共用合同](./00-contracts.md)。
**执行计划：** [M2 六任务计划](../../plans/2026-09-05-browser-rearchitecture/m2-inspection.md)。
**进入门：** M1-G06、M1-R06、M1-D06 全部验收通过。
**出口：** M2-06；实际证据写入计划新增的 `docs/qa/browser-rearchitecture/m2/`。

## 1. 目标与边界

本包把 M1 的有界机器诊断变为可日常使用的 Console、Network、Trace、真实截图与响应式检查工作流，并完成 named Profile 的完整管理。
三个首发目标为 macOS arm64、macOS x64、Windows x64；沿用已签名的 Managed Runtime，不增加外部 Node、浏览器下载或 TCP 调试端口。
M2 与 M3 可分别交付；M2 验收不依赖 Chrome Connector，完整 Chrome 检查能力须 M2-06 与 M3-06 均通过且组合 fixture 有实证，最后完成的一包负责运行。
Preview 保留现有快速查看和明确标识的旧检查能力；需要真实截图、完整检查和响应式验证时，执行可见的 Managed 升级。
固定 Preview 的用户先看到能力缺口与升级确认，不能静默切换或用 DOM 合成图冒充浏览器截图。
不提供 Console 任意 JavaScript 输入、网络请求重放、密码/Cookie 浏览器、任意 CDP 客户端或自动网页修改。
Full CDP 不是完成本包的前提；受控 CDP proxy 仍通过 Gateway 的语义、来源、脱敏和接管校验。

## 2. 代码证据与职责

| 已核对的现有代码 | 现状与本包责任 |
| --- | --- |
| `src/components/side-workbench/BrowserTab.tsx` | 直接管理 Design Mode 和发送草稿；逐步委托给 `src/components/browser/inspection/`，不增加壳状态。 |
| `src/hooks/useBrowserDesignMode.ts` | `POLL_MS=180`，通过 `sideBrowserEval` 注入、轮询和读取单选；旧 Preview 路径保留兼容。 |
| `src/lib/browserDesignMode.ts` | `DesignModeSelection` 为单选；Canvas/`foreignObject` 合成截图；不得作为 M2 真实像素证据。 |
| `src/components/side-workbench/BrowserDesignModePanel.tsx` | 提供选区、修改文字及发送入口；迁移时保留草稿行为和失败反馈。 |
| `src/lib/api/system.ts`、`src-tauri/src/commands/terminal.rs` | `side_browser_snapshot` 是文本快照；M2 使用新的 Browser Gateway capability。 |
| `src/lib/settingsCatalog/entries/runtime.ts` | 真实设置注册入口；Profile 和检查保留设置必须可搜、可跳。 |
| `src/i18n/messages/*/` | 15 个完整目录；以 `en` 为 key 权威，不能沿用旧 wiki 的少数语言示例。 |

Host 检查、导出与 Profile 管理落在 `src-tauri/src/browser/inspection/`、`src-tauri/src/browser/profiles/`。
Worker 原语落在计划新增的 `browser-runtime/worker/src/inspection.ts`、`screenshots.ts`、`design.ts`。
前端订阅、面板、选区与任务状态落在 `src/components/browser/inspection/`、`src/components/browser/profiles/`；只接收 Host 投影。
基础协议、Gateway、Artifact、ProfileGuard、安装包 fixture 分别来自三个 M1 进入门，不在本包另写一套权限模型。

## 3. 领域与协议

Agent 对现有 tab 的动作使用 `00-contracts.md` 的 `RequestEnvelope` 和统一结果；字段为 camelCase，协议版本为 1。
Agent 身份包含 `hostBootId`、`appSessionId`、`browserSessionId`、`tabId`、`backendBindingGeneration`、`runtimeGeneration`、`agentBindingId`、`turnId`、`leaseId`、`fenceEpoch`、`policyRevision`。
Agent 请求携带 `expectedTabRevision`、`expectedNavRevision`；revision/generation/fence 为不回绕的非负安全整数 `u32`。
人工检查通过共用合同的可信 Tauri 入口申请 `UserInspectionPermit`，不使用 Agent envelope，也不伪造 AgentBinding、turn 或 lease。
Host 先 fence Agent 并等待取消完成，再签发限 AppSession/BrowserSession/Tab、binding/nav revision、origin、操作集合和有效期的 permit；结果只发给申请的可信面板。
人工允许集合仅包括检查读取、授权截图、固定 overlay 安装/选择/移除和 viewport 设置/恢复；这些检查原语不授予一般导航、点击、填表或 Agent 观察。
订阅本身不产生可持久化权限；Host 从当前绑定解析 origin，不相信 UI、Worker 或模型提交的 origin。

| 对象 | 必需字段与生命周期 |
| --- | --- |
| InspectionSubscription | `subscriptionId`、所属 Session/Tab、来源集合、generation、`nextSequence`、预算、授权主体、状态。 |
| UserInspectionPermit | Host 签发的人工检查 ID、发起面板、Session/Tab、binding/nav revision、origin、操作集合与 expiry；不是 Agent lease。 |
| InspectionEvent | `eventId`、`sequence`、`source`、时间、Tab/nav revision、producer/request origin、脱敏结果或 Artifact ID。 |
| ScreenshotCapture | `captureId`、尺寸、DPR、裁剪、遮罩、nav revision、像素 hash、Artifact ID、`ready/withheld/failed`。 |
| SelectionSet | `selectionSetId`、Tab/nav revision、viewport revision、最多 20 个有序 `ElementRef`、用户标注。 |
| ElementRef | Host-issued `elementRefId`、frame chain、定位证据、边界、允许的计算样式；CSS 路径只作提示。 |
| ValidationRun | `validationRunId`、目标 URL 引用、viewport 列表、每步结果、截图引用、`queued/running/paused/completed/failed/cancelled`。 |
| TraceRecording | `traceId`、所属 Session、格式版本、事件范围、缺口、Artifact 引用、`idle/recording/paused/finalizing/ready/failed`。 |
| NamedProfile | 延用 BrowserProfile UUID、displayName、project bindings、generation、最后使用时间、健康状态、active references。 |

事件按每个订阅的单调 sequence 排序；丢包返回 gap 和重新获取 snapshot 的入口，不能制造连续记录。
初始预算为每 Session 5,000 条或 8 MiB ring，单批不超过 128 条/256 KiB；超限 drop oldest 并累计 droppedCount。
大 body、截图、导出和 Trace 走 Artifact Store，不通过 React 事件、MCP 文本或无界内存返回。
同一 Runtime 中另一个 BrowserSession 的事件、tab、Artifact 与 lease 不因共享 Profile 而可见。

## 4. Workbench 布局与交互

检查区沿用任务组标题、紧凑工具栏与分隔线，提供 Console、Network、Trace、Design 四个 tab。
窄 pane 时详情以同区替换视图呈现并有返回图标；宽 pane 可显示列表与详情分栏，不能叠卡片。
工具栏包含后端/Profile、记录状态、筛选、清空、暂停、导出和真实截图入口；不显示未支持的可点击空壳。
按钮使用项目 `Icon*`、Tip 和相邻 chrome classes；单选用 `Select`，动作菜单用 `ContextMenu`。
弹层走 portal、现有不透明背景与 native cover；与 Preview 并排时不能被子 WebView 遮住。
面板切换只变投影；关闭检查区终止该人类订阅，不关闭 BrowserSession；任务关闭统一由生命周期命令处理。
所有用户文案、状态、错误、数量和时长经 `createT`/`intlLocale`；新增稳定 `browser.inspection.*`、`browser.profile.*` keys。
tablist 支持左右键、Home/End；列表用方向键移动，Enter 打开详情，Escape 逐层返回并恢复焦点。
记录按钮有 `aria-pressed`，进度用 `aria-live=polite` 节流，错误摘要可聚焦；颜色不是唯一状态信号。
在 360 px pane、800 px 高窗口、200% 字体缩放和德语/泰米尔语长文本下，工具栏换行且不盖住内容。

## 5. Console

支持 debug/info/warn/error、搜索、frame、时间范围、重复项折叠和导航分隔线；默认只看当前 Tab。
保留日志是该订阅内的视图选项，不能延长授权或恢复已经过期的数据。
对象只展示有界、脱敏、无 getter 执行的序列化 preview；循环结构、BigInt、Error stack 有明确表示。
不可把浏览器 remote object ID 留给用户随意展开，避免读取未批准对象或执行 getter。
异步例外、未处理 rejection 和浏览器错误分别保留来源；不把一条错误复写成整个任务失败。
清空只清理当前可见 buffer；删除已保存 Trace/Artifact 另走所有权和保留流程。
没有记录、筛选无匹配、来源被阻止、接管暂停和 ring 截断均有独立状态。
点击源位置只展示经许可的源引用；不自动访问任意 URL 或本机路径。

## 6. Network

每次请求有独立 `networkRequestId`；redirect hop 通过 parent ID 关联，不能以 URL 为身份。
列表显示脱敏 URL、method、status、类型、来源 frame、耗时、字节数和失败原因，支持排序与筛选。
详情包含允许的 request/response headers、timing、redirect chain、body preview 和关联 Console/Trace。
Host 同时校验顶层、执行 frame、内容产生方与请求目标 origin；未授权第三方记录只显示阻止计数。
Authorization、Cookie、Set-Cookie、代理凭据、URL userinfo 和敏感 query 进入数据前移除。
body 默认关闭；用户批准当前 origin 与本次采集后，仅允许受支持文本 MIME，单项 1 MiB、Session 20 MiB。
密码、OTP、Passkey、CAPTCHA、二进制、未知编码、压缩炸弹和无法判定安全的 body 为 `withheld`。
流式、WebSocket、Service Worker、cache、取消和失败请求保留 metadata；不承诺重建未采到的正文。
复制 URL、headers 或导出 JSON/HAR 都使用同一脱敏投影；导出不得带 Cookie、令牌或隐式解码秘密。
无重放、修改、阻断或模拟网络条件按钮；这些动作不属于 M2 检查能力。

## 7. Grok Trace v1 与真实截图

完整人类 Trace 使用 `grok-browser-trace/v1`：动作时间线、Console/Network 关联、授权截图、允许的 DOM 片段、gap 与失败信息。
这是归一化调试数据，未承诺 Playwright Viewer 兼容；不要把原始 `trace.zip` 直接提升为普通 Artifact。
记录、暂停、继续、停止、筛选 actor/type/time、定位 Tab/nav revision、导出及重开都在 M2 内交付。
用户接管时 Agent Trace 立即暂停；继续必须由用户交还后创建新授权段，旧命令不会续跑。
导出为 ZIP，包含版本化 manifest、脱敏 events 与引用的 immutable bytes；用标准 ZIP/JSON 库解析，解压总量不超过 M1 的 64 MiB Trace 预算、4,096 个条目和 100:1 压缩比。
重开验证版本、文件 hash、总大小、条目数、路径穿越、重复路径与压缩比；未知格式明确失败。
原始 Playwright trace 默认不采；显式诊断采集若启用，仅落隔离 quarantine，不能成为 Agent 或 support bundle 的输入。
未经 schema allowlist 重建和敏感数据 fixture 证明安全的原始 trace，禁止导出；正则擦除 header 不算脱敏证明。
隔离原始数据停止或失败后删除，失败留 tombstone；永久保留仍须经过独立、可审查的安全格式提升。
截图来自 Playwright `page.screenshot`/`locator.screenshot`，支持 viewport、full page、元素与用户裁剪区域，沿用 M1 的 32 MiB/3,200 万像素上限。
记录真实 width/height、DPR、scroll、nav revision、遮罩与像素 hash；虚拟 DOM 或 `foreignObject` 图不被接受。
跨域 frame 未获许可时整块遮罩；秘密输入与用户接管期间 Agent 截图被阻止，不能缓存最后一帧继续读取。
站点其他可见秘密无法靠 OCR 保证识别；发送到聊天/导出前提供实际预览和显式人工遮罩，不声称自动全脱敏。
遮罩是派生 Artifact 的不可逆像素替换；原始图仍受短期所有权限制，不能藏在导出附件中。
页面持续动画时使用有界稳定等待并标出 `unstable_capture`；字体、canvas、视频、sticky、跨屏滚动纳入像素 fixture。

## 8. Design Mode v2 与响应式验证

用户从可信 App UI 进入 Design；Host 先对 Runtime 设置 TakeoverLatch、撤销 Agent lease/观察，保留用户控制。
人类检查通过独立的可信 Tauri 入口签发 `UserInspectionPermit`，限定 Session、Tab、origin、检查读取/viewport/overlay 原语与时间，不填假 AgentBinding/空 lease，也不授予 Agent 观察权限。
页面 overlay 的点击仅提供选区提示；Host 重验绑定、frame、revision 与 locator 后，才返回人类面板可见的数据。
单击替换选区，Shift+单击增减选区，最多 20 项；顺序编号、移除、清空、撤销/重做选区编辑均可用。
键盘可在受支持元素树定位，Enter 选择、Shift+Enter 增减、Escape 退出当前检查；不劫持输入法和站点文本输入。
每项显示 tag/role、尺寸、允许的样式、文本摘要、frame 来源和独立修改说明；有一项失效时保留其说明。
选区文本、样式与页面 HTML 均视为不可信数据；不作为模型指令，也不带表单值、脚本或事件处理器。
导航、frame detach、viewport revision 改变或 backend 重绑使对应 ElementRef 失效，必须重新定位，不能靠同名 CSS 路径复活。
支持顶层、同源 iframe、获双 origin 授权的跨域 iframe、open Shadow DOM；closed Shadow DOM、opaque frame 明确 unsupported。
发送操作把有序说明和已预览的 Artifact 加入 Composer 草稿；不自动发送聊天，不修改 DOM/源码，也不清除 TakeoverLatch。
响应式工具用数值输入和设备 preset Select，提供 viewport 宽高、DPR、横竖交换和恢复原 viewport。
宽高范围 320..3840/240..2160，DPR 1..3；M2 每次最多 8 个 viewport，超出直接验证失败。
验证运行绑定当前页面与 `UserInspectionPermit`，依次设置 viewport、等待字体/布局的有界稳定、采图、比较布局和记录结果；所有步骤持续保留 TakeoverLatch=user。
断言包含横向溢出、选定元素是否可见、边界是否重叠及截图差异；差异阈值、忽略区域和基准版本均可检查。
基准由用户选择本次合格截图建立，不自动把失败结果接受为新基准；报告同时呈现实际图、基准图与差异图。
停止、接管冲突、导航变化、origin 变化或 Runtime 故障取消剩余 viewport；已完成证据保留，绝不自动重放。
运行前保存 viewport，正常结束恢复；恢复失败明确报错，不能用窗口 resize 伪装响应式页面状态已恢复。

## 9. Named Profile 完整管理

入口为任务组 Profile 选择器和 `#/settings/runtime/tools` 的 `settings-anchor-browser-profiles`。
设置注册 `runtime.browser.profiles`、`runtime.browser.inspectionRetention`；每项有稳定锚点、15 语言标签与搜索词。
创建默认空白 named Profile，名称 trim 后 1..64 Unicode 字符，NFC 归一化；重名按不区分大小写的归一名拒绝。
UUID 与目录无关，名称不参与路径拼接；不接受任意目录、Chrome/系统 Profile 导入或自动复制 Cookie。
选择已有 Profile 时展示项目绑定、活动 Session 数与站点状态会共享的事实，由用户显式绑定当前项目。
绑定只改变该项目的可选/默认 Profile 引用，不能授予其他项目的 tab、Artifact、grant 或 lease。
对正在运行的 BrowserSession 切换 Profile 先 fence/drain，再创建新的 backend binding；旧标签只保留可安全恢复的上下文。
同一 named Profile 仍只有一个 Runtime；跨项目并用遵循容量与单 writer ControlDomain，UI 显示共享控制影响。
支持重命名、设为当前项目默认、绑定/解绑项目、查看活动引用、停止使用、健康诊断与删除。
删除在应用内确认中列出所有项目绑定和活跃引用；有活跃引用时只能先停止相关 Session，再重新确认删除。
Host 重新核对引用、ProfileGuard 和 descendant ledger 后 tombstone，受限 UUID 根异步删除，不跟随 symlink。
解绑最后一个项目使 Profile 进入 orphan 列表；关闭/删除项目或会话不会连带删 named Profile。
迁移沿用 M1 Profile epoch/generation 和原子 manifest；保留原始 generation，失败进入 needs_repair，不能 wipe/merge。
静态凭据保护未过目标平台门时，named/project 创建和复用不可用；仍提供 ephemeral，禁止给出假成功。

## 10. 失败、隐私、迁移与验收

统一结果为 `{status, requestId, reasonCode, tabRevision, navRevision, artifactIds}`；失败后按钮可重试，但未知副作用不自动重试。
busy 时动作按对象锁定，订阅/导出/删除有取消；空列表、无匹配、expired、withheld、degraded、unsupported 分别可识别。
断线、权限变化、Profile 切换、Session 归档和 Agent turn 结束撤销受影响订阅/permit；重开面板重新申请，不从磁盘恢复授权。
人工检查 permit 也受 policy/kill switch/关闭生命周期约束，不能为秘密读取、Full CDP 或 Agent 代办提供绕路。
临时 Artifact 默认 24 小时；发送草稿/聊天按 durable owner 先行提升，取消草稿释放引用，导出的用户文件不由 cleaner 删除。
Artifact 预览通过 loopback HTTP 的会话级短期 token 与 opaque ID；不复用进程 media token，不向 MCP 交付绝对路径。
日志、诊断和公开验收仅保留合成 fixture 数据、版本、hash、reason code、延时；不含用户站点、路径、对话 ID 或配对秘密。
旧 Preview 单选草稿保持可编辑；旧合成图片标记来源，不迁移为真实截图，不恢复旧 eval/轮询权限或元素引用。
发布迁移采用能力开关：M2-06 未过则完整面板和 named 管理关闭，M1 与 Preview 的已验收能力继续可用。

| 验收组 | M2-06 必须提供的证据 |
| --- | --- |
| 业务闭环 | Console/Network 筛选详情、Trace 录制到导出重开、真实截图、20 项标注到草稿、响应式差异闭环。 |
| 权限与秘密 | 双 origin、共享 Profile 隔离、接管停止观察、旧 revision 拒绝、令牌/body/Trace/像素遮罩 fixture。 |
| Profile | 创建/重命名/绑定/切换/并用/解绑/orphan/删除/迁移失败与锁竞争，原有 Profile 数据保持可恢复。 |
| 视觉与无障碍 | 浅/深色、三平台真实包、窄 pane、200% 缩放、15 语言 parity、键盘/focus/读屏、native cover。 |
| 数据与恢复 | Artifact promotion、24 小时清理、ZIP 重开攻击、Worker/Host 崩溃、磁盘满、原始 Trace quarantine 删除。 |

验收目录必须区分 `planned` 与实际 pass/fail，记录平台、签名 Runtime tuple、fixture revision、命令和脱敏证据。
本文未运行这些验收；本包不能以截图面板看起来存在或 mock 测试通过代替真实安装包闭环。

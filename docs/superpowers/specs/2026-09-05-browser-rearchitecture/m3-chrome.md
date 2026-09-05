# M3：显式认领的 Chrome Connector

**状态：** 可执行规格；本轮仅完成文档，运行验收尚未执行。
**日期：** 2026-09-05。
**上位合同：** [总体设计](../2026-09-04-browser-rearchitecture-design.md)、[共用合同](./00-contracts.md)。
**执行计划：** [M3 六任务计划](../../plans/2026-09-05-browser-rearchitecture/m3-chrome.md)。
**进入门：** M1-G06、M1-R06、M1-D06 全部验收通过。
**出口：** M3-06；实际证据写入计划新增的 `docs/qa/browser-rearchitecture/m3/`。

## 1. 产品边界

只有用户明确要使用当前 Chrome 登录态或已打开页面时，才进入本连接器；普通查看和 Agent 任务仍采用 Preview/Managed 路由。
用户在扩展中显式认领当前标签或当前任务组的列明成员，再在 App 选择接收的 BrowserSession。
认领不复制 Cookie、密码、历史、扩展、下载目录或 Chrome Profile；Chrome 数据始终由 Chrome 持有。
macOS arm64、macOS x64、Windows x64 为 M3 发行门；Linux 随 M4 的独立等价验证开放。
扩展只通过 native messaging 访问已验证的 App companion，不开放 HTTP/WebSocket listener、远程调试端口或任意 CDP endpoint。
Chrome 窗口继续可见，Workbench 同步任务、标签、来源、权限和活动；不假装把 Chrome 嵌进 App。
M3 与 M2 独立；基础观察和 Artifact 使用 M1 API，完整人类检查面板采用同一 capability 投影，但须 M2-06、M3-06 与组合实证全部满足后开放。
Chrome Full CDP 永久不在本包能力中；Developer Mode 或一次配对确认不能跳过逐命令语义守卫。

## 2. 现有证据与计划模块

| 现有证据 | 设计后果 |
| --- | --- |
| `src/lib/sideWorkbench.ts` | 当前浏览器 tab 是本地 UI 数据，不能以其中的 URL/ID 代替 Chrome 身份。 |
| `src/components/side-workbench/BrowserTab.tsx` | 当前只有嵌入 WebView；Chrome 视图接入未来 `src/components/browser/connector/` 投影。 |
| `src/lib/api/system.ts`、`src-tauri/src/commands/terminal.rs` | 旧 `side_browser_*` 不是外部浏览器权限边界。 |
| `src-tauri/Cargo.toml` | 已有 serde、tokio、uuid、keyring、Windows/macOS 原生基础；需新增受限 bridge binary target。 |
| `src/lib/settingsCatalog/entries/runtime.ts` | 配对与连接诊断注册在 runtime/connection，不增加顶级设置导航。 |
| `.github/workflows/release.yml`、`scripts/build-local.sh` | bridge 必须随真实 App 包签名、安装和卸载；扩展独立校验并发布。 |

扩展源码统一放 `browser-connector/extension/`，采用 Manifest V3、固定依赖与本地打包脚本。
Rust Host 连接器统一放 `src-tauri/src/browser/connector/`；入口为 `src-tauri/src/bin/browser-native-bridge.rs`。
App UI 放 `src/components/browser/connector/`；共用 Gateway 的 protocol、binding、origin、policy、lease、lifecycle 与 persistence 模块。
不在 `App.tsx`/`AppWorkbench.tsx` 新增状态；现有任务组使用注册的 capability adapter。

## 3. 信任边界与领域

受信任部分是已验证 App、bridge、发行的扩展包和受支持 Chrome；网页、扩展消息 payload、模型和文件内容均不可信。
OS 或同账户已被完全控制不在隔离保证内，但伪造 extension ID、重放消息、错误 Chrome 进程与陈旧 Tab binding 必须拒绝。

| 对象 | 合同 |
| --- | --- |
| ConnectorPairing | Host UUID、允许的 extension ID、发行渠道、bridge 身份、用户批准时间、`paired/revoked`；不代表 tab 权限。 |
| ChromeConnection | `connectionId`、pairingId、Host boot、extension instance、实际浏览器 ProcessIdentity、协议/版本/能力、状态。 |
| ProcessIdentity | PID、start time、用户身份、规范化 executable identity、签名/发布者、经核实的父链与窗口集合。 |
| ChromeClaim | `claimId`、connectionId、AppSession/BrowserSession、用户确认的 tab snapshot、claim generation、`pending/active/revoked`。 |
| BackendBinding | Host BrowserTab UUID 到 Chrome tabId/windowId/groupId/document 标识的易失映射，具有单调 generation。 |
| ControlDomain | 固定为 `chromeConnection(connectionId)`，覆盖该连接实际浏览器进程的全部相关原生窗口。 |
| TakeoverLatch | Host 独立保存；connection 下任意原生输入触发全部 claim 的 fence，lease 生命周期不清除 latch。 |

一个 Chrome tab 同时只能属于一个有效 claim；claim 再绑定一个 BrowserSession，不能凭同 URL/标题/groupId 抢占。
Chrome groupId/tabId 可复用，不能写成持久权限；Host UUID 和 generation 是请求定位的真相。
`ProfileRef=chrome(connectionId,claimId)`，不是 App-owned BrowserProfile；清理 App 数据不能删除 Chrome 用户目录。

## 4. 配对与 native messaging

扩展首次操作打开配对页；App 显示扩展名、ID、发行渠道、浏览器来源及一次性 challenge，用户在可信 App UI 确认。
challenge 默认有效 60 秒、单次使用；双方比较同一 challenge 的短校验码，传输秘密不展示在日志或页面 URL。
仅由 App 本地用户操作开始配对；网页不能通过 `externally_connectable`、postMessage 或自定义协议自动启动批准。
bridge 的 native host manifest 仅允许正式扩展 ID；开发 ID 只在开发安装中使用独立 native host 名称和数据根。
Chrome 原生启动 bridge 时提供的 origin/parent 信息只是候选证据，Host 必须验证父进程链、签名、用户和进程出生时间。
不能仅信任命令行参数、PID、extension storage 或浏览器报告的 version；重启/进程复用重新验证身份。
bridge 与 Host 使用私有本地 IPC；Unix socket 校验用户权限和 peer credentials，Windows named pipe 使用当前 SID ACL 并核对 peer PID。
每次连接生成内存态 connection nonce/key，绑定 `hostBootId`、extension instance 与 ProcessIdentity，不写 extension sync/local storage。
native messaging 使用当前三平台的 32 位 little-endian length + UTF-8 JSON，单帧上限 256 KiB；大 payload 转 Artifact。
该 Chrome 固定 framing 仅用于 Chrome/bridge 边界；bridge/Host IPC 使用共用 4 字节 big-endian/1 MiB 协议，bridge 严格转换帧并维持相同 envelope 身份，不做字节序猜测。
stdout 只写帧，stderr 为有界且脱敏诊断；拒绝超长、截断、非法 UTF-8、重复身份字段、未知 message type 与未协商版本。
Host 与 bridge/扩展各自使用有界队列和 request timeout；超时或 EOF fence 全连接，待执行命令不得继续。
握手输出协议、Chrome major、扩展/bridge版本、实际能力与 reason codes；无共同版本显示升级入口，不降级到旧校验。

## 5. 扩展权限与发行约束

基础权限为 `activeTab`、`scripting`、`nativeMessaging`、`storage`，仅在用户点扩展时访问当前页。
`storage` 只存语言、UI 偏好和非秘密安装标识；不存 URL 清单、页面文本、配对凭据或有效 claim。
任务组认领按需申请可选 `tabs` 与 `tabGroups`；取得权限后只查询用户当前选择的 group，不持续枚举所有窗口。
自动化另行申请可选 `debugger`；权限拒绝时可保留已认领元数据，Writer 为 unavailable，App 说明原因。
Chrome 的 debugger 提示保持可见；用户开启 DevTools 或取消调试导致 detach 后立即 fence，不自动重附着。
不申请 `cookies`、`history`、`downloads`、`management`、`webRequest`、`<all_urls>` 或全站常驻 content scripts；下载采用已认领 target 的受控响应 broker，不靠全局下载权限枚举用户下载。
`scripting` 仅执行构建产物中固定脚本，限定已认领的当前文档；无远程代码、动态代码下载或页面可调用的 bridge。
Manifest V3 CSP 使用本地脚本和禁止 object；依赖 lockfile、源码版本、SBOM 与可重复构建 hash 随扩展产物归档。
应用页不能自动安装扩展或偷偷修改 Chrome preferences；未安装时打开官方条目，返回后由用户重新检测。
支持 Chrome major 范围写入经发布签名的兼容表；未认证的新版本保持只读/blocked，不能推测兼容。

## 6. 显式 claim 与任务组

扩展提供“当前标签”和“当前组”两个模式；组模式列出本次成员快照供勾选，不显示非当前组的浏览信息。
提交 claim 前 App 展示脱敏 origin、标签数、目标项目/会话、scope 与连接级接管影响，由用户确认。
每个成员在批准时和绑定时核对 Chrome tab/document identity；期间导航或成员变化要求刷新清单。
任务组标题、排序和颜色投影可同步，权限集合始终是确认过的 snapshot，不是动态 `groupId` 查询结果。
组中新建/拖入 tab 默认 unclaimed，必须重新选择；移出、关闭、discard 后重新生成文档或变为受限 URL 时撤销旧绑定。
已认领 tab 重排只改变顺序；移动到另一窗口若无法立即验证原生归属则先 fence，再重新核实，不继续旧动作。
跨 claim 转移需要 App 显式操作，先 fence/revoke 原 claim，再分配新 claim 与 BrowserTab；旧 Artifact/授权不会一起转移。
App 隐藏 pane 只 detach 投影；显式“释放认领”关闭权限与订阅，不关闭用户 Chrome tab。
关闭 BrowserSession 默认释放全部 claim；关闭 Chrome 页面必须为单独确认的页面动作，不能借任务关闭执行。
扩展展示已连接 App 和当前 claim 状态，提供停止控制/释放当前页；扩展可收紧权限，但只有可信 App UI 能交还控制。
未认领 tab 不能被观察、聚焦、截图或读取标题；被阻止的记录只向 App 返回数量/原因，不能泄漏页面信息。

## 7. 原生进程归属与用户接管

Host 用已验证 ChromeConnection 的实际浏览器进程及窗口集合配置 NativeInputFenceProvider。
同一进程包含多个 Profile/窗口时，输入域保守扩大到所有相关窗口；不能假定 Chrome windowId 等于 OS window handle。
同一实际 Chrome 进程映射多条 connection 时，原生输入同时 fence 全部关联 connection，不能以扩展实例或 pairing ID 假装存在窗口隔离。
任一窗口的 pointer down、wheel、key down、受支持 touch/IME 输入，使该连接全部 claim 的 `fenceEpoch` 原子递增。
Host 同时设置 TakeoverLatch、撤销 lease、停止 admission、清空队列、取消在途动作并暂停 DOM/截图/Console/Network 观察。
内容脚本 `event.isTrusted`、CDP input 回调、Chrome tab focus 事件都不是原生用户来源证据。
macOS Input Monitoring 被拒绝/撤销、Windows Provider 不健康、窗口/PID 歧义、桥接重启均进入 `blocked_input_attribution`。
不能证明归属时不授予或续期 writer lease；生产没有绕过开关，不用 OS 输入注入伪装 Playwright/CDP 操作。
人工接管入口始终可用；AX 操作如果平台不能产生已覆盖原生事件，则需要先显式接管，不宣称自动覆盖。
租约过期、Agent turn 结束、扩展 Service Worker 重启或重新配对都不能清除 TakeoverLatch。
用户在 App 交还时重新核对进程、窗口、Provider、policy、claim、origin 与 generation，创建新 lease/快照/队列。
扩展收到 fence 必须在每个 primitive 前及 await/retry 后校验 epoch；失联后的默认状态是停止。
actionability helper 只允许逐次只读检查，等待/重试由可取消队列驱动；不能把未可取消的内部自动等待包在一次 epoch 校验中。为取消动作不得关闭用户 Chrome 页面/进程，无法证明可停止的 primitive 不开放。
已与网页副作用竞态的 primitive 标为 `unknown_outcome`，其余复合动作不运行，不宣称撤销已发送的数据。
从 Host 接到权威输入事件到 admission 拒绝且 extension acknowledgement 的参考机目标为 p95 <= 250 ms。
M3 必须独立测量连接下多个 claim、多个窗口和输入 Provider 故障；不能引用 Managed 的结果替代。

## 8. Gateway 与能力协商

Agent live-tab 请求使用共用 RequestEnvelope；`runtimeGeneration=null`，Chrome 不冒充 Managed Runtime。人工检查沿用可信 Tauri `UserInspectionPermit`，不伪造 Agent lease。
Host 将 envelope 再绑定 connectionId、claimId、extension instance、实际 ProcessIdentity 和一次性 dispatch capability。
wire 使用 camelCase、protocolVersion=1，revision/generation/fence 为不回绕 `u32`；extension 不可自行提高或忽略版本。
Host 从当前文档与 frame binding 计算 canonical origin，校验顶层、执行 frame、请求目标与 producer origin。
允许 HTTP(S) 与经验证 creator origin 的 about:blank/blob；chrome://、扩展页、devtools://、file://、opaque frame 默认拒绝。
动作集首批为受限导航、固定 DOM/locator 查询、逐 primitive 点击/输入/滚动、截图、Console/Network metadata、经批准的上传 attach，以及逐 claim 受限 HTTP(S)/已验证 creator 的 blob 下载。
CDP proxy 只接受版本化的语义动作和编译时 allowlist 映射，不接受 method 字符串、任意 JS、Cookie/storage 方法或 browser-wide target 操作。
frame/Shadow DOM 范围与 M1 fixture 保持一致；定位器优先复用固定 Playwright revision 的受控 helper 构建产物，不自写 selector 引擎。
helper 导出可行性、许可证、源码/hash、内部接口维护和真实 actionability fixture 是 M3-04 的先验硬门，不能把私有 API 当现成公共 adapter。
无法实现同等 actionability 的动作结构化 unsupported，不用坐标盲点替代；该缺口计入未达功能，不能据此判定 M3-04 完成。
模型风险标签、页面按钮 role/text 或“提交按钮不存在”不能证明动作低风险；未知动作进入用户确认，`dont_ask` 直接拒绝。
高风险确认绑定动作摘要、当前文档/元素证据、origin 与 revision；状态变化后失效，不能一次批准整个随意脚本。
密码/OTP/Passkey/CAPTCHA 只交由用户接管；Connector 不读取、持久化或重放这些值。
上传复用 single-use UploadGrant、200 MiB 上限和一次性披露，但不能照搬 Managed 的路径型 `UploadSpoolLease` 清理：Managed spool 等所属 Runtime 的 Chromium/worker descendants=0 才物理删除，用户 Chrome 不能为清理被终止。
M3 的待验证方案为有界分块经 bridge/extension 传给固定 helper，在已认领 document 的隔离执行环境组装内存 `Blob/File`，全部字节完成后一次 attach；不把 Host staging 路径交给 Chrome。只有确认浏览器持有独立字节且不再依赖路径才确认安全清理，bridge 返回或句柄关闭不是证明。
M3-04 必须实证 51 MiB 与 200 MiB、延迟读取/提交、导航、接管和断线，不能套用有 50 MiB 限制的 Playwright buffer API。撤权立即停止未发送分块/新 attach 并释放临时对象；已披露页面字节不能收回。不成立则上传保持未达能力，不能靠永久保留 staging 或关闭用户 Chrome 通过验收。
下载 broker 只在已批准动作中为已认领 target 启用受限 response-stage 捕获，核对 connection/claim/target/frame/networkRequestId、origin chain、一次性授权和 generation。
优先验证当前认证 Chrome 的 `Fetch.requestPaused`/`networkId`/response stream 能否在原生下载前安全转入 M1 Artifact staging，并在结束/失败/fence 后恢复拦截状态。
同 URL 并发、iframe、redirect、blob、浏览器自主下载与中途接管必须有误归属负例；无法建立请求身份或不可安全接管响应时拒绝 broker，绝不猜测。
blob 无 Network requestId 时不能冒充 HTTP broker；固定 helper 仅在已认领 document/frame 内验证 creator origin 和 blob handle，按一次性批准流式读取并经 Host 预算/hash 写入 Artifact。未知 creator、跨 claim handle、已 revoke blob 或 generation 变化必须拒绝。
不调用 Browser.setDownloadBehavior 或全局 downloads API 干预未认领页面；不得把 URL/referrer 当下载所有权证据。
无法证明 broker 安全时，Agent 管理下载列为显式未达功能，提供人工接管保存；M3-06 必须记录范围决定和能力差异，不能把该替代算 Managed 全能力对等或自动通过任务。
网络正文和完整检查支持以能力明确协商；未授权来源的数据不因 debugger 已附着而进入 Host 投影。
Artifact 走 M1 会话级 opaque handle 和 loopback token；释放 claim/撤销授权即停止新的观察，不泄漏浏览器本地路径。

### Managed 与 Chrome 能力对照

下表固定验收义务，不是当前支持声明；`必须对等` 的行在任一认证平台缺少证据时，M3-04/M3-06 都不能自动 PASS。明确拒绝不允许的输入是安全通过，缺失一项允许范围内能力是功能未完成，两者不得混为 unsupported。

| 能力 | Managed 基线与独立依赖 | Chrome 要求与交付任务 | 完成判定 |
| --- | --- | --- | --- |
| 导航/历史/停止 | M1-G06、M1-R06；HTTP(S) 与许可的 creator origin | M3-04 验证导航、后退、前进、重载、停止、redirect 和 revision；M3-06 真实包 | 必须对等，未知后果仍确认，不能降成仅打开外部 URL。 |
| Locator/actionability/输入 | M1-R03、M0-R05 声明的成熟 locator 与取消语义 | M3-04 固定 helper 的来源/许可/维护和真实等待/遮挡/输入 fixture；M3-03 接管 | 必须对等，无法安全取消或 helper 不可用是未完成。 |
| Frame/Shadow DOM | M1-R06 的顶层、同源、双授权跨域 iframe、open shadow 子集 | M3-04 完成同一子集；closed shadow/opaque frame 拒绝与 Managed 一致 | 必须对等，不能把跨域 iframe 全关后宣称通过。 |
| 真实截图 | M1-R05 Artifact 预算；M2-03 提供预览/遮罩工作流 | M3-04 实现 viewport/full page/element/crop 实际像素及跨域遮罩；M3-06 验证 | 必须对等，DOM 合成图或扩大到其他 tab 均不通过。 |
| Console | M1 有界脱敏 metadata；M2-02 为完整人类面板 | M3-04 提供同等来源/sequence/预算和 fence；面板组合见下行 | 基础采集必须对等；无 getter/跨 claim 泄漏。 |
| Network | M1 metadata/请求身份；M2-01/M2-02 规定批准 body 与详情 | M3-04 完成 metadata、redirect/来源归因和批准 body 的结构化出口；M3-06 验证 | 必须对等，未获批准正文 withheld 不算能力缺失。 |
| 上传 attach | M1-R05 单次披露、200 MiB 路径型 UploadSpoolLease；所属 Runtime descendants=0 后清理，cleanupPending 仍计预算 | M3-04 独立证明无路径 Blob/File 字节接管、51/200 MiB、延迟读取/提交及撤权；不得为了清理终止用户 Chrome | 必须对等；不能沿用 Managed 的进程退出清理门，无法实现既定大小/独立清理则未完成。 |
| HTTP(S) 下载 | M1-R05 独立 downloadId、staging/export、MOTW/quarantine | M3-04 实现 target/frame/networkRequestId response broker 和并发负例 | 必须对等；只提供人工保存不能完成该行。 |
| blob 下载 | M1-R05 允许已验证 creator origin 的下载与 Artifact 生命周期 | M3-04 独立 blob broker 验证 creator/document/claim/handle、stream预算和撤销；无请求ID不能猜归属 | 必须对等；缺 blob 路径是明确未完成，M3-06保持未通过。 |
| 完整 M2 检查 | M2-06：Console/Network/Trace、Design多选、响应式、导出重开 | M3-04 提供底层适配；M2-06 + M3-06 后组合 fixture 验证 `UserInspectionPermit`、面板与 Artifact | 独立组合依赖；M2尚未交付时记 `blocked_by_m2`，不宣称完整检查。M2已交付后缺组合证据不得开放组合能力。 |
| Full CDP | Managed 默认 unsupported；独占/ephemeral只是必要条件，仍需所有语义安全门 | Chrome 从不提供 Full CDP；M3-04/M3-06 测试任意 method/script/Cookie接口拒绝 | 明确非对等目标；拒绝测试必过，不能靠Developer Mode开放。 |

M3 独立交付不依赖 M2 开工或完成；最后完成的 M2-06/M3-06 负责执行组合 fixture。此条件只适用于完整 M2 人类检查，不改变导航、文件与基础观察的必需对等义务。

## 9. UI、设置、空忙错与无障碍

Workbench 每组显示 Chrome 标识、claim 数、连接状态、控制者、暂停、接管、交还、显示窗口与释放入口。
未安装、未配对、无已选标签、待确认、连接中、能力受限、用户控制、失联、已撤销和版本不兼容分别可辨认。
忙状态只锁定当前配对/claim 动作，取消随时可用；超时恢复可重试入口，不重复弹多个确认。
设置位于 `#/settings/runtime/connection`，注册 `runtime.browser.chromeConnector` 与 `settings-anchor-browser-chrome-connector`。
设置展示配对记录、版本/兼容、Provider 健康、重新检测、撤销、卸载 bridge 帮助与脱敏诊断导出。
沿用 `Select`、`ContextMenu`、`GlassModal`、Tip 和项目 icons，菜单 portal 与实心材质，关闭后恢复焦点。
扩展 popup 采用相同 tokens 与紧凑列表；窄 320 px popup 支持长标题折行，危险动作不能与列表勾选挤在同一点击区。
认领列表支持方向键、空格勾选、Enter 提交、Escape 取消；配对弹窗焦点锁定，读屏声明连接与权限变化。
扩展和 App 共用构建导出的 `src/i18n/` 文案，覆盖 15 locale；Chrome `_locales` 只作为打包投影，不维护独立翻译真相。
不在 UI 暴露内部 nonce、IPC 地址、签名路径或 session UUID；诊断仅显示用户能据此采取动作的原因。

## 10. 撤销、崩溃与恢复

从 App 撤销 pairing、扩展释放、权限收回或 uninstall 信号均先关闭 admission，再 fence 全连接、撤销 claim/grant/binding/token。
扩展执行 `debugger.detach`，停订阅/移除固定 overlay；Host 等待 acknowledgement 或超时后标明 degraded，权限仍保持撤销。
App 退出只终止自己启动的 bridge/IPC；绝不 kill Chrome、关闭未要求关闭的页面或删除用户 Chrome 数据。
bridge、App、Chrome 或 Service Worker 重启后状态为 disconnected；仅恢复脱敏任务元数据，不恢复凭据、claim、lease、grant 或队列。
重连需要新的双向握手和用户重新声明 tab snapshot；旧 groupId、标题、URL 或配对记录都不能自动重认领。
Host unclean recovery 把在途副作用标为 unknown_outcome；只提供检查当前页面，不重试 click/attach/提交或恢复自动驾驶。
pairing revoke 持久化失败时保留内存与磁盘 fail-closed 标志，阻止该渠道重连并提示修复，不把撤销报为成功。
持久记录只含配对元数据与非秘密的恢复摘要；不保存 Chrome cookie、页面正文、实际用户路径或浏览历史清单。

## 11. 打包、更新与验收

bridge 随 App 签名安装，native host manifest 通过平台 resolver 生成，目标为当前安装的已验证绝对 executable path。
升级原子替换注册；多版本 App 只能由用户当前选择的安装拥有激活记录，路径不存在/签名不符时报错，不回落 PATH。
macOS codesign/notarization、Windows Authenticode/用户级 native host 注册、portable 移动后重新注册均有真实包 fixture。
扩展发布到官方商店条目，App 兼容表签名并包含扩展 ID/版本范围；开发包不能自动升级为生产配对。
更新前 fence 活跃连接；协议不兼容要求用户更新对应组件，兼容表/bridge/扩展回退均不恢复旧控制权。
扩展发布或商店审核延迟时，已验收版本继续可用；无法满足安全兼容范围则关闭连接器并保留 Preview/Managed。
卸载只移除本安装拥有且 hash/路径匹配的 native host 注册，保留其他安装和用户 Chrome 数据。

| 验收组 | M3-06 必需证据 |
| --- | --- |
| 显式范围 | 当前 tab/group snapshot、组增减/移动/关闭、冲突转移；未认领页面观察与控制次数为零。 |
| 身份/协议 | 伪 extension、伪父链/PID 复用、帧畸形/重放/旧 generation、版本不兼容和 Host boot 变化。 |
| 接管 | 三平台真实包、多窗口多 claim、Provider 拒绝/撤销、p95 fence、旧 lease/队列无法恢复。 |
| 动作与隐私 | helper 可维护性与 locator/frame/shadow fixture、未知高风险确认、秘密接管、敏感日志/body、上传撤销、下载 broker 与误归属负例；能力差异单列。 |
| 恢复与发行 | bridge/Chrome/扩展/App 故障、配对撤销、扩展升级、安装/移动/卸载；用户 Chrome 与数据仍完整。 |
| UI | 15 语言 parity、320 px 扩展/360 px pane、浅深色、键盘/读屏、native cover 与错误回路。 |

记录实际 Chrome major、扩展/bridge/App hash、平台权限、Runtime tuple 和 fixture revision；合成账号/站点是唯一公开测试数据。
验收命令与结果保存在计划新增的 QA 目录，所有未执行项明确为 planned，不能以开发模式加载扩展代替商店/签名包证据。
M3 完整出口要求声明能力全部有证据；研究失败只能产生受限试用的范围决定记录，不能把 unsupported 计为实现成功或省略未达能力。

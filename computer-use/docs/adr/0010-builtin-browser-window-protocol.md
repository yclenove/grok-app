# 0010 内置浏览窗的身份、隔离与 Host 协议

状态：Proposed（提议，待用户审阅；不授权实现）
日期：2026-10-17

## 背景（事实 + 证据链接）

[P5-10](../../../docs/plans/computer-use-v2/06-roadmap.md#p5-10-内置浏览窗l1) 与
[05 §5](../../../docs/plans/computer-use-v2/05-route-b-integrated.md#5-内置浏览窗l1p5-可选交付)
要求系统 WebView 的 `cu-browser` 窗口、可信地址栏/后退/前进/刷新、条件注册的
`browser_navigate{url}`、独立 `cu-browser-profile/` 和设置页清除数据。窗口仍由 OS
无障碍驱动观察、操作、截图；原图、缩略图、审批裁剪及临时文件继续遵守
[03 §4.4、§7.6](../../../docs/plans/computer-use-v2/03-architecture.md) 的脱敏与会话归属。

[03 §15.3](../../../docs/plans/computer-use-v2/03-architecture.md#153-消息json-rpc-20长度前缀帧双向)
及 [`messages.rs`](../../crates/gcu-host-protocol/src/messages.rs) 的 `METHODS` 仅允许审批、策略、
覆盖层、事件、会话、诊断和截图；没有浏览窗存在状态、原生身份映射或导航请求。
`host.event.emit` 是引擎发出的展示事件，不能证明 App 创建的原生窗口身份；
`engine.policy.changed` 只通知策略版本，不承载窗口映射；`engine.screenshot.get` 不能导航。
借用这些消息的字符串或把 label 放进模型参数都会绕开类型和信任边界。

依 [08 §2、§6](../../../docs/plans/computer-use-v2/08-grok-execution-protocol.md)，本 ADR 只提出差异，
不修改 03、协议、拒绝名单、实现、STATUS 或质量门禁文件。接受后另行实施并验收完整 P5-10。

## 决定

### 1. 范围与默认行为

建议采用两个版本化内部请求：`engine.browser.sync` 同步完整身份/状态，
`host.browser.navigate` 投递经校验的导航；不增加 Host 通知。
默认所有新增内部消息均不对模型可见，模型只得到条件工具 `browser_navigate{url}`。
App 仍仅链接 `gcu-protocol` / `gcu-host-protocol`，不链接 core、驱动或 MCP；路线 A 不启用 L1。

首版建议一个原生 `cu-browser`，独占绑定一个已开启 CU 的本地聊天会话；其他会话不得观察、
操作、导航或读取其图片。打开入口来自可信 App UI，模型不能创建、转移窗口所有权或清除数据。
这项所有权选择待下文裁定；不把“当前聊天焦点”当作所有权。

只对已证明的浏览内容目标应用 05 §5 已规定的窗口例外：不移除 Grok 自身拒绝规则，
不把整个 App 进程、主窗、会话窗、覆盖窗、地址栏或审批窗放行；任何额外用户/企业拒绝仍优先。
浏览窗仍须 App 授权、网页区风险确认、执行界面识别、前台守卫、Stop、快照/几何/内容重验，
授权不能隐含在“窗口已打开”中。Web SSH、云终端等原有拒绝/交接规则不变。

### 2. 威胁模型与对应验证

攻击者控制远程响应、脚本、DOM、标题、URL 文本、iframe、弹窗及重定向；可诱导模型调用工具。
不假定远程页面可信，也不把渲染进程与 Rust 主进程视为同一信任域。OS/系统 WebView 被攻破、
同 UID 修改可信二进制或读取进程内存不在此协议能保证的边界内；沿用 03 §15.2 的边界。

| 威胁 | 缓解（必须同时成立） | 必须新增的测试与断言 |
|---|---|---|
| T01 页面提示注入，诱导终端、凭据操作或无确认提交 | 网页文本仅是观察数据；不向模型开放 DOM/eval；全部模型动作进入既有 Router、网页风险确认和执行宿主门禁 | `browser_injection_cannot_bypass_policy`：夹具诱导所有动作路径，禁止动作零副作用 |
| T02 自报 `cu-browser` label、`window.name`、App 名称、可信来源或伪造原生 ID | 仅 Rust 创建记录 + OS 实例核验产生映射；页面字段和 MCP 参数不参与授信 | `page_claimed_identity_never_grants_access`：同名主窗、标题、URL、消息和仿冒 label 均不放行 |
| T03 iframe、`srcdoc`、`about:blank` 子文档、弹窗或重定向继承可信 capability | 内容整棵 frame 树无 IPC；禁止内容加载 App 资产/协议；新窗口请求不能创建可信窗口；权限不随 URL 改变 | `frames_popups_redirects_keep_zero_capabilities`：跨源/同源/空白 frame、`window.open`、302/meta/JS 跳转后逐项探测均拒绝 |
| T04 危险 scheme、编码/大小写/等价 IP 写法绕过 | 按下节 URL 表规范解析；复用现有危险输入规则并取更严格结果，Rust 在实际投递前再验；不交系统 opener | `navigation_dangerous_url_has_zero_dispatch`：工具、地址栏、历史、链接、批量/脚本输入共用拒绝向量 |
| T05 元数据/内网访问、DNS 重绑定、公共站点重定向或子资源访问本机服务 | 检查完整解析链与实际连接目标；禁止非公网；对所有网络请求和每跳重验；无法实施的平台关闭 L1 | `private_network_requests_never_connect`：本地替身服务计数为零，覆盖 iframe/fetch/图片/WS/worker/重定向与 DNS 换答 |
| T06 页面调用 Tauri 命令、审批、Stop/恢复、事件、媒体或 Host/MCP | 内容无 Tauri 初始化桥、无任何 command/event grant；实际调用 webview 的入口总拒绝；不提供 token/FD；服务端认证仍必须通过 | `remote_content_cannot_reach_app_surfaces`：包括原始 IPC、`cu_approval_decide`、`cu_resume`、事件监听/伪造、媒体 token 申请、图片取回及 Host/MCP 直连全部失败 |
| T07 页面画假地址栏，用户信息段/IDN/双向字符/长 URL 欺骗来源 | Rust 导航回调提供真实提交 URL；可信栏不使用 DOM 标题/自报来源；来源固定可见，禁止内容全屏覆盖可信栏，显示规则见下节 | `chrome_origin_cannot_be_spoofed`：伪栏、IDN、userinfo、双向控制、长串、失败导航与重定向均显示实际来源或拒绝 |
| T08 旧映射、乱序同步、关窗重建/句柄复用、引擎重启延续例外 | 连接实例、单调 revision、随机 instance_id、原生窗口生命期证明和 OS 进程实例全部匹配；未知即拒绝 | `browser_binding_reuse_fails_closed`：旧响应/旧 ID/旧进程/重复 revision 无权；主窗保持拒绝 |
| T09 跨会话图片、关窗/删会话/清数据后的迟到图片复活 | 每幅图绑定会话、浏览实例和状态版本；同一脱敏管线；废弃实例先封索引再清文件，拒绝迟到写入 | `browser_images_are_owned_and_revocable`：跨会话 get 拒绝，原图及全部衍生物清理，旧响应不能入库 |
| T10 清数据成为任意目录删除、profile 与真实浏览器共享、后台 worker 继续写 | profile 路径仅 Rust 固定构造，拒绝符号链接/junction/越界；销毁 WebView/data-store 后删除；不接收路径参数 | `profile_clear_is_scoped_and_complete`：隔离目录外哨兵不变，cookie/存储/cache/service worker 清空，删除失败不重新开放 |
| T11 导航审批 TOCTOU、Stop 后迟到投递或重放 | 审批绑定 URL、窗口/文档/策略版本；投递前双端重验；Stop 独立锁存；导航无自动重试，不能恢复 | `navigation_stop_and_approval_races_are_safe`：挂起审批时 Stop/换页/关闭/撤销后旧请求零投递，已投递断线为 unknown |
| T12 可信栏被导航/XSS 或借共享窗口权限调用主窗能力 | 可信栏只加载固定本地入口、无远程脚本/HTML 注入；按实际 webview 而非父窗口授予最小权限；CSP 与入口校验双重约束 | `chrome_cannot_navigate_or_escalate`：改 origin/路径/子 frame、伪造 event、跨 webview invoke 都不能获得主窗/审批权限 |

### 3. 精确的最小 Host 扩展

沿用长度前缀 JSON-RPC 2.0、双向认证及预期进程实例绑定。两个新增项均为普通 RPC 通道上
**必须带 id 且必须应答**的请求；普通通道不能 Stop/恢复，独立 Stop 通道不能分发浏览方法。

| 名称 | 方向 | 参数 / 成功结果 | 进入 `METHODS` 白名单 | 模型可见 |
|---|---|---|---|---|
| `engine.browser.sync` | App Rust → 引擎 | 下方完整状态 → `{"revision":1}` | 是，`notification:false`，minor ≥1 | 否 |
| `host.browser.navigate` | 引擎 → App Rust | 下方导航参数 → `{"instance_id":"b_01","dispatched":true}` | 是，`notification:false`，minor ≥1 | 否；仅工具执行器可间接调用 |

`engine.browser.sync` 参数的完整 JSON 示例（所有字段必需，`browser` 可为 null）：

```json
{
  "revision": 1,
  "browser": {
    "instance_id": "b_01",
    "session_id": "local-session-id",
    "kind": "builtin_browser",
    "window_label": "cu-browser",
    "chrome_webview_label": "cu-browser-chrome",
    "content_webview_label": "cu-browser-content",
    "native": {
      "platform": "macos",
      "process": {"pid": 42, "started": 123},
      "window_id": "81",
      "window_generation": "opaque-native-lifetime-stamp"
    },
    "geometry": {
      "epoch": 1,
      "content_rect": {"x": 0, "y": 96, "w": 1280, "h": 704},
      "scale": 2.0
    },
    "document_revision": 1
  }
}
```

类型约束：revision/epoch/document_revision/started 为 `u64`，revision 从 1 严格递增；pid 为
非零 `u32`；platform 仅 `macos|windows|linux`；kind 仅 `builtin_browser`；label 必须精确匹配
上例常量；instance_id 为 Rust 生成的至少 128-bit 随机标识（示例缩写），每次创建/重新绑定更换。
`process` 复用现有 `ProcessInstance`；`window_id` 与该平台驱动的原生 ID 规范相同，不是网页字段。
`window_generation` 是 Rust/平台适配器对这一次原生窗口生命期的证明，不接受页面可设的
AXIdentifier/AutomationId/标题作证明；无法可靠核验时无例外。geometry 使用原始窗口相对
物理像素，数值有限、尺寸/scale 正值、矩形必须在窗口内；与既有截图换算一致。
字符串有上限（session_id/window_id/generation ≤256 UTF-8 bytes，instance_id ≤128）；
未知字段、未知 enum、不合法数值、跨平台身份均拒绝，不容忍为未来字段。

`browser:null` 表示关闭、不可用、正在清数据或尚未证明安全；例如
`{"revision":2,"browser":null}` → `{"revision":2}`。它是替换整个映射的状态，不是增量 patch。
握手后初始无例外，App 先同步空状态；有效本地 CU 会话打开、原生身份核验、平台安全能力验证
完成后才可同步非空。引擎按新 revision 原子撤销旧绑定、清旧快照/索引/图片/挂起审批，
再安装新状态并应答；ack 前 App 不宣布“可供模型使用”。几何、文档变化同样更新并使旧审批失效。
重复或倒序 revision 拒绝；响应必须回显本次 revision，不从旧连接恢复映射。
同一实例的版本更新不等于销毁；换实例、同步 null 或 owner 会话关闭则永久退役旧 instance_id，
同一认证连接内不能用更高 revision 重新授予退役实例。退役表上限 128，达到上限关闭连接再认证；
同 session_id 重开也不能接受上一生命期迟到的 sync。撤销须取消排队/在途动作并释放该目标输入，
无法证明尚未投递的在途动作保留 unknown；Stop 的独立通道和时限不受同步影响。

`host.browser.navigate` 参数（无额外可选参数）：

```json
{
  "session_id": "local-session-id",
  "instance_id": "b_01",
  "revision": 1,
  "document_revision": 1,
  "policy_version": 1,
  "url": "https://example.com/"
}
```

URL 为绝对 URL，≤8192 UTF-8 bytes；`policy_version:u64` 为批准时加载的 `Policy.version`；
身份/版本均由引擎已验证的会话上下文填写，不接受模型传入。
执行器先走既有授权、风险确认、窗口写锁、Stop 和账本；Rust 在主线程实际导航前再次检查
owner、原生身份、映射/文档/策略版本、Stop 和 URL 表。`dispatched:true` 只表示已交给原生导航，
不表示加载完成/业务成功；禁止返回 `false` 伪装成功。后续观察/settle 仍由引擎驱动完成，
模型结果复用 03 §6 的动作结果、新状态和必要截图，不返回 Host JSON 或内部身份映射。
导航启动/提交/失败由原生回调更新 Rust 文档版本和可信栏，再同步；页面 JS 消息不作为回调。

两个请求均受现有普通 RPC 5s/有界队列限制，身份同步不进入模型会话动作队列，也不持有等待
另一个 RPC 的锁。Host 导航回调不能等待本次工具完成才接收同步；避免双向 RPC 死锁。
关闭/清数据先在 Rust 禁止投递并隐藏内容、撤销权限，再同步空状态；sync 超时/失败关闭 Host
连接并撤销例外，不能带旧授权继续。引擎断线即时撤销全部映射，Host 同时使浏览能力不可用。
OS 原生动作仍在投递前核验窗口生命期、内容区域/焦点；不能证明目标仍属内容即拒绝/交接。

**版本与兼容：** 建议 Host `1.0 → 1.1`；major 不变，取双方 minor 的较小值。
不改现有严格握手字段或给旧消息增加必需字段。协商 minor <1 时不发送两个新方法、不创建可用
L1、不注册工具，现有 CU 可继续；1.1 而运行时关闭/平台未验收时同步 null。
接收分发、参数/返回校验与 schema/golden 必须同时按 minor、方向、通道和有无 id 强制，
不能只把名字加入 `METHODS`。更新 Rust wire 类型、Host golden、App 生成类型及兼容性测试。

**错误：** 沿用 `RpcError {code,message}`，message 为下表固定标识，不回显 URL/页面文本；
除明确的 `browser_navigation_unknown` 外，业务拒绝均保证导航尚未投递。无新增模型错误码。

| Host code / message | 用途 | `browser_navigate` 对外映射 |
|---|---|---|
| `-32601` / `method unavailable on this channel` | 未知/未协商方法、方向/通道/id 形态错误 | `capability_unavailable` |
| `-32602` / `invalid parameters` | schema/大小/数值不合法 | 工具参数校验失败；内部构造错误为 `internal` |
| `-32603` / `internal error` | 内部失败，且能证明尚未投递 | `internal`，失败无副作用 |
| `-32002` / `busy` | 有界队列或窗口互斥忙 | `busy` |
| `-32004` / `closed session` | 会话已失效 | `app_denied`，不能访问旧实例 |
| `-32010` / `browser_unavailable` | 开关关闭、平台无能力或未同步可用窗 | `capability_unavailable` |
| `-32011` / `browser_owner_mismatch` | 跨会话/身份/额外拒绝策略不符 | `app_denied` |
| `-32012` / `browser_gone` | 原生窗口已消失/生命期改变 | `window_gone`，附允许披露的窗口列表 |
| `-32013` / `browser_state_stale` | revision/文档/策略不匹配、sync 倒序或退役实例重用 | `stale_element`，附新状态，不自动导航 |
| `-32014` / `browser_url_blocked` | URL 表拒绝 | `key_blocked` |
| `-32015` / `browser_stopped` | Host Stop 已锁存 | `stopped_by_user` |
| `-32016` / `browser_navigation_unknown` | 原生导航出错，但可能已投递 | 动作 `outcome:unknown`，不伪造新的模型错误码 |

审批拒绝/超时沿用现有 `approval_denied/approval_timeout`，在发导航请求前结束。
连接丢失、取消或 5s 超时不能证明未投递时按 03 §6 返回 `unknown` / `not_checked`，
附可安全获取的新状态，不重试；不能把 RPC 超时当作“确定未执行”。sync 失败只会收紧权限。

**工具目录：** 对每个 MCP 会话单独判断：1.1 + 运行开关 + 有效非空绑定 + 所有权匹配才注册
`browser_navigate`；关闭/清数据/会话关闭/引擎换代立即退注册并发送 MCP
`notifications/tools/list_changed`。即使客户端缓存旧目录，调用时仍重新检查并拒绝失效工具。
默认面和 full 可按条件添加，minimal 保持 03 §11.1 的固定九工具；所有 profile 均生成 golden
并检查预算。客户端若不能刷新目录，下一次可信用户轮次用既有 ACP 重建路径生效，
不能假称已经动态可用；工具执行端始终先拒绝旧绑定。

**明确不新增：** 模型 open/close/clear/back/forward/reload 工具、身份自报工具、恢复工具、
通用 Host 转发、通用 HTTP 控制端点、JS/eval/DOM/CDP/Playwright、cookie/存储读取、
持久授权回写、媒体 token 分发、页面事件桥或额外截图 RPC。清数据是可信 App 本地操作，
图片继续使用现有带 `session_id` 的 `engine.screenshot.get`；MCP 工具参数仍只有 `url`。

### 4. Rust 窗口身份与 webview 权限

Rust 窗口工厂先在私有 registry 记录 `window label → trusted kind`，并关联实际创建出的
窗口对象、原生进程实例/窗口 ID/生命期、两个 webview 对象、owner、随机 instance_id。
`main/session-*/cu-overlay/cu-overlay-controls` 保留各自种类，绝不因标题/来源相似变为浏览窗。
登记完成前、销毁开始后均无浏览例外；回调从捕获的实际对象读取身份，页面不能登记、改种类或绑定。
引擎仅通过认证的 `engine.browser.sync` 接受映射，再与驱动独立取得的 OS 身份核对，
不是给 `window_label == "cu-browser"` 的任何窗口放行。句柄复用、App 更新/身份变化必须失效。

同一原生窗口采用**两个兄弟 webview**，不是在远程内容 DOM 内加工具栏，也不把内容嵌入有权
IPC 的可信页面 iframe。可信栏与内容的权限分别如下，未列出一律 deny：

| 主体 | 唯一允许项 | 必须拒绝项 |
|---|---|---|
| `cu-browser-chrome`（固定本地入口） | 窄 App 命令 `cu_browser_state`（仅本实例安全展示状态）、`cu_browser_navigate{url}`、`cu_browser_back`、`cu_browser_forward`、`cu_browser_reload`；命令由实际 webview 推导实例/owner，不接收 label/session/path；通过状态轮询更新 UI，不订阅事件 | 所有其他 App/Tauri/plugin 命令与通用事件 listen/emit；审批/Stop/恢复/设置/媒体/Host/MCP；创建窗口、shell/opener、文件、剪贴板读取、数据清除；远程导航/脚本/子 frame |
| `cu-browser-content`（远程内容及全部 frame） | 标准网页渲染、JS、表单、隔离 profile 内网页存储、按 URL 表允许的网络和导航 | **全部** Tauri/App/plugin 命令/事件/IPC、媒体 token、Host/审批/MCP；App 资产与本地协议；OS opener、下载、文件选择/上传、摄像头/麦克风/定位/通知、剪贴板 API、屏幕共享、全屏、pointer lock、devtools、创建有权窗口 |
| 主窗已有可信设置/浏览入口 | 经现有可信用户交互打开/关闭浏览窗或清数据；不扩充原有主窗 capability 范围 | 页面转发的操作、模型自动清数据/换 owner/解除 Stop |

上述五个 `cu_browser_*` 是建议的 **App 内部 Tauri 命令**，不是 Host 方法或模型工具。
地址栏提交与后退/前进/刷新均由 Rust 同一 URL 策略处理，不能在可信栏任意 `eval`。
自定义命令必须显式 ACL/permission 登记且在 Rust 入口核验实际 `Webview` 对象及固定本地 origin，
不能只检查 `WebviewWindow.label()`：两个子 view 共用父窗口。内容不匹配任何主窗/覆盖窗
capability，禁止 `windows:["cu-browser"]` 或通配符间接授予整个窗口；内容 `remote` grant 为空。
不以“没有导入 @tauri-apps/api”或空 permissions 文件代替 custom-command 总入口拒绝；
原始 IPC、子 frame、外部 origin 和已被注册的其他 App 命令也须在 handler 前拒绝。

原生内容边界与焦点在每次投递前验证：模型不能操作可信栏、窗口系统菜单或其他 App 界面；
地址栏文本/提交仍返回 `key_blocked`，其他越界动作 `app_denied`，无法定位内容则 handoff。
坐标缩放、窗口 resize/DPI 改变走既有 stale 检查；键盘 Tab/快捷键转移焦点后必须重验，
输入序列逐事件核验，不能将输入限制在内容区的路线禁用；不能凭一次聚焦放行整段输入。
观察/截图仍为窗口级且必须完整脱敏，包括地址栏敏感 URL；
不把相邻主窗或覆盖层拼入截图，也不向网页暴露图片 ID 或临时路径。

可信栏只显示原生回调报告的已提交地址，编辑中与加载中状态分明；失败保留旧来源并显示失败。
始终可见 scheme + 完整 host + 非默认端口，不以标题代替；IDN 显示 ASCII/punycode，
双向/控制字符拒绝输入、显示时可见转义，长地址只缩略 path/query，完整规范 URL 可由用户查看。
拒绝 URL userinfo，防止 `trusted.example@evil.example`；页面标题只能在内容区作为不可信文本。
地址栏/历史/刷新有加载、不可用、失败和重试状态；无历史时禁用对应按钮，拒绝导航保持原页面，
重试只由用户再次操作。普通浏览验收须实际完成公网夹具的链接/表单、历史往返、刷新和 OS 树操作，
不能只显示窗口/按钮；控件、对话框和文案沿用 App 组件与十五语言 catalog。

### 5. URL 策略表与能力边界

下表适用于工具 URL、可信栏提交、历史/刷新、链接、form、iframe、弹窗请求、原生重定向，
以及所有实际网络子请求。共享策略测试向量；引擎执行既有输入拒绝，Rust 是 WebView 导航/网络
最终检查点。先按浏览器等价 URL 语义解析 scheme/IDNA/IPv4/IPv6/端口，再检查规范结果及
DNS CNAME/A/AAAA/实际目的地址；歧义、非法编码、控制/双向字符、userinfo 或不可验证目标拒绝。
不反复百分号解码后把原字符串交另一个解析器；校验并投递同一规范 URL，避免解析差异。

| 类别 | 建议首版策略 | 重定向 / 子资源 / 地址栏 |
|---|---|---|
| 公网 `https:` / `http:`，有效 host/端口 | 允许；HTTP 显示非 TLS，不降级或忽略证书错误 | 每次实际请求均复验目的地；同源不是豁免理由；地址栏同策略 |
| `javascript:`、`file:`、`data:`、`vbscript:`、`blob:` | 拒绝外部提供的 URL，不因用户在可信栏输入而放行 | 导航、frame、子资源同拒绝；页面普通 JS 不等于允许 `javascript:` URL |
| `tauri:`、`asset:`、`media:`、`grok:`、`grok-app:`、`http(s)://tauri.localhost` 及 App 内部资产来源 | 内容访问一律拒绝，不能载入可信本地入口 | 工具/地址栏/链接/frame/跳转/子资源同拒绝；不共享初始化脚本 |
| `mailto:`、`tel:`、`intent:`、`shell:`、`ftp:`、所有已知外部协议及其他未知 scheme | 拒绝，不调用系统 opener/外部 handler | 历史/刷新/弹窗同拒绝，无“始终允许”覆盖 |
| `about:blank` / `srcdoc` | 仅 Rust 创建空白内容初态及引擎内部无权子文档；不是可接受的工具/地址栏导航目标 | 保持内容权限，网络仍按表检查；不创建可信弹窗 |
| `ws:` / `wss:` | 仅作为内容网络子请求，且满足同样公网检查；不作为顶层导航 URL | worker/WebSocket 重连亦检查；其他非 HTTP(S) 网络途径如 WebRTC 禁用或由同一目标限制覆盖，否则平台不可用 |
| 元数据 IP/hostname | 永久拒绝，至少包含既有 `169.254.169.254`、`169.254.170.2`、`100.100.100.200`、`metadata.google.internal` 与全部现有规则 | 大小写/尾点/编码、IPv4 整数/八进制/十六进制、IPv4-mapped IPv6、DNS 别名或重绑定均不得绕过 |
| localhost/loopback、RFC1918、link-local、CGNAT、IPv6 ULA/unspecified、multicast、保留/其他非全局可路由地址；`.localhost`/`.local`/单标签主机 | 首版默认拒绝，包括 App 媒体/MCP loopback 端口；无 UI 例外，企业只能收紧 | 公网域名若任一解析/连接地址落入拒绝类也拒绝；所有子资源和每跳均适用 |

弹窗请求的 HTTP(S) 目标只可经同一校验改为现有内容 view 内导航，断开 opener 引用，不能创建
任意 label 的 Tauri 窗口；空白后写入、无法检查的 popup 拒绝并给可信 UI 错误。
后退/前进不能直接加载旧缓存中的被禁来源；service worker/cache 命中同样受策略约束。
原生 JS 对话框/权限请求不得弹出带 App 权限的系统 UI；不允许页面获取 App 的用户决定通道。

**不把 `on_navigation` 当作网络沙箱。** 跨平台 WebView 是否能覆盖 HTTP 30x、meta/JS、frame、
fetch/XHR、图片/CSS、WS、worker/cache 及 DNS 重绑定须分别证明。预先解析一次 DNS 不防重绑定；
需要在实际连接层确保目标仍允许，无法证明就拒绝该请求/禁用该平台 L1，不能只隐藏失败后的页面。
本文不批准新增代理、第三方运行时、依赖或私有网络 API；若系统公开接口不足，进入下文裁定。
本段保证是拟议验收要求，不是已验证的平台能力。

L0（用户外部浏览器）仍只拦 gcu 自己发出的危险文本/地址栏提交，**不承诺 L0 重定向拦截**，
也不承诺控制其页面子资源。L1 扩展不放宽 03 §7.8 的任何输入规则；共享拒绝向量而不扩大权限。

### 6. 图片、清数据与关闭开关

图片继续按现有会话 token/`session_id` 校验，另外在服务端索引绑定 browser instance 和
document revision；内容无法取回这些索引。模型图、缩略图、审批 crop、预览与临时文件均从
同一已脱敏结果生成；树不完整/状态不同步一律拒图并 handoff。显示预览沿用可信 UI 的
[loopback + path resolve](../../../docs/llm-wiki/media-delivery.md)，不向内容 view 发媒体 token。

清数据仅可信主窗设置项：拒绝新浏览动作 → 取消挂起审批/锁住导航 → 同步 null 并退注册 →
关闭拥有者的 CU 会话（复用 `engine.session.close`，撤销旧 token、授权、图片及衍生物；聊天保留）→
销毁内容/可信栏和原生 data-store、停止 worker/cache 写入 → 固定 profile 范围删除 → 显示结果。
不能在运行中的 WebView 下只删除目录、不能“删除失败但成功提示”，迟到图片写入按生命周期代际拒绝。
该顺序会撤销拥有者本次 CU 会话的其他图片/授权，是复用既有撤销协议的明确代价；下一次用户轮次
才能按既有流程重新打开 CU 会话，浏览窗不会自动重开，Stop 仍锁存。
具体清理范围包含 cookies、HTTP cache、local/session storage、IndexedDB、service worker、
WebView data-store，绝不读取/导出其值。路径仅限 App 数据目录的 `cu-browser-profile/`，不碰真实浏览器。

普通关窗先撤销实例并同步 null，清该实例的快照、图片衍生物和待审批动作；不删除 profile。
Host 若无法细分已有图片衍生索引，保守清拥有者的图片缓存，不能通过页面/标题推断归属。
会话关闭/fork/切模型关闭所属浏览窗并撤销实例；焦点切换不改 owner。进程重启不自动重开/恢复例外。

建议独立内部运行开关 `builtin_browser_enabled` 默认 false，并与现有 CU 总开关取交集；
用户 UI 若呈现此设置，按既有 settingsCatalog/i18n 规则登记。关闭立即先撤销投递和映射、
退注册、取消审批、关闭窗/清图片；不解除 Stop、不默认删除 profile。
若 sync 无法完成，关闭连接以确保引擎旧映射失效，影响现有 CU 时如实呈现不可用。
回滚到 Host 1.0 时依 minor 自动关闭 L1；保留但不打开隔离 profile，清除入口仍可本地使用。
不靠扩充拒绝名单例外、重试或允许远程 IPC 维持可用性。

## 备选与为什么不选

| 方案 | 优点 | 取舍 |
|---|---|---|
| A：两个内部 RPC + Rust 身份 registry + 双 webview（推荐） | 身份和撤销可确认；仅一个模型工具；App/core 依赖不变 | 需 Host minor 扩展、原生身份/网络门禁逐平台验证；安全能力不足时该平台关闭 |
| B：不扩展 Host，只保留 L0，推迟可选 L1 | 零新增信任面，现有 CU 完整可用 | 不能交付隔离浏览窗和动态工具；若网络/身份验证不可行，这是安全回退方案 |
| C：单 webview 用远程页面上方 DOM 工具栏，或借 `host.event.emit`/label 字符串传身份 | 消息/界面改动少 | 远程 DOM 与可信控制混合、方向与 schema 不符、可冒名且撤销无确认，拒绝 |

不选择 App 进程内链接 core/驱动或打包浏览器/代理运行时来绕行；与 03 的边界冲突。

## 影响（对接口、测试、用户）

**03 §15.3 的拟议差异（仅列差异，本次不改本体）：**

1. 引擎 → App 请求表新增 `host.browser.navigate`，使用本 ADR 的参数、结果、错误与投递语义。
2. App → 引擎请求表新增 `engine.browser.sync`，使用完整可空绑定、单调 revision 与撤销 ack。
3. 表后补充两个方法只能普通认证 RPC、不得注册为模型工具；初始化无例外、断线撤销、
   每会话动态工具条件与可信 Rust/OS 身份校验。原有消息和 Stop/恢复通道保持语义。
4. 配套 §15.4 将 minor 声明为 1.1、新方法按协商版本门控并补 golden；不改 major/握手形状。

03 §7.4 / §10 和 05 §5 已有的浏览窗例外按真实身份收窄落实，不删除/放宽任何拒绝名单项。
若实现需要超出本文的新 Driver/Host 接口、网络依赖或平台路径，应另提 ADR，不能隐含在本提议中。

**07 拟新增验收项（与 T01–T12 对应，接受后实施）：**

| 层/门禁 | 新增必测内容 | 通过证据 |
|---|---|---|
| L1 / Host schema、目录预算 | 两方法精确形状/enum/上限/未知字段、方向/channel/id、1.0/1.1/major 不兼容；URL 等价写法全向量 | Host golden 与错误映射断言；各 profile/tools 条件 golden、预算不超现有上限 |
| L2 / §3 生命周期与策略 | T01/T02/T08/T09/T11；跨 owner、Stop/审批竞态、乱序 sync、退役实例/同 ID 会话重开、窗口复用、清数据失败/迟到回包 | fake 原生身份与时钟；禁止动作计数为零、旧 token/图片不可读，unknown 不重放 |
| L3 / MCP e2e | 开窗注册、关窗/清 profile/断线/会话变更退注册、list_changed 与缓存旧工具调用；无其他新增模型工具 | 两个本地会话对照目录与调用，Host 名称/身份不能作为工具参数 |
| 前端组件 | 地址提交/Enter、历史无项、加载中、导航拒绝/失败/用户重试、关窗退注册、清数据 busy/失败、真实来源显示 | 组件交互断言与 i18n 键锁步，普通浏览另由原生夹具验证 |
| L4 / 原生 Tauri 三平台 | T03–T07/T10/T12；两个真实兄弟 view、全 App command/event 拒绝、真实原生身份和内容焦点、真实导航/子资源/DNS、profile 清理 | 隔离 WebView 与夹具服务；端点/handler 外部计数、目录外哨兵、来源栏断言，不以 capability JSON 静态扫描替代 |
| L4 / C20/C21/C25/C26/C27 | 密码/原生树不完整拒图、覆盖层排除、同几何换页、执行界面、Stop 独立时限 | 各图像衍生物无夹具秘密；无越界动作；沿用既有时限，不放宽 |
| §5 red_team / §7 安全清单 | 增加内置浏览恶意页面、伪身份、伪地址栏、metadata/private/DNS、审批/恢复/媒体/Host 访问场景 | oracle 先自证；0 越权/0 禁止连接/0 非目标目录删除；只用夹具，不登录真实账号 |

实现时运行 `rcargo test -p gcu-host-protocol`、`rcargo test -p gcu-core --test policy`、
`rcargo test -p gcu-mcp`、`rcargo run -q -p xtask -- check-deps` 及既有 golden/budget 检查；
前端运行 `corepack pnpm exec vitest run src/components/computer-use/ComputerUseBrowser.test.tsx`
与 `corepack pnpm exec tsc --noEmit`；匹配平台 runner 运行
`cargo test -p grok-app computer_use::browser` 和上述原生隔离/网络测试。
缺少平台证据标 `not_run`，该平台 L1 保持关闭，不能用 fake 通过声称原生通过。
本次仅文档自查；这些是后续验收要求，不是本次已运行结果。

## 开放问题（需要用户/维护者裁定）

| 决策 | 选项 | 推荐与待补证据 |
|---|---|---|
| D1：两个新方法、1.1 及受限窗口例外是否获批 | A 接受本 ADR 后实施；B 保持 1.0、仅 L0 | 推荐 A；本文件 Proposed 不等于修改 03 或进入实现的授权 |
| D2：首版窗口/profile 所有权与清数据影响 | A 单窗口单 owner + App 专用共享 profile，清除关闭 owner CU 会话；B 每会话独立窗口/profile（需修订固定 label/映射）；C 多会话共享可操控窗口 | 推荐 A，须用户接受不同聊天可先后读取同一专用 profile 的站点状态及清除撤销整个 owner CU 会话；不推荐 C，避免并发授权/隐私歧义 |
| D3：各系统 WebView 无法证明所有网络目的地限制时的交付方式 | A 仅通过完整门禁的平台提供 L1，其他保留 L0；B 推迟全部 L1；C 另立网络隔离 ADR（需评估新依赖/平台接口） | 推荐 A；需要维护者指定原生 runner/版本与可验证网络控制能力；不能降低本表拒绝条件或假称顶层 hook 足够 |
| D4：内网/开发 localhost 需求 | A 首版全部拒绝；B 未来单独 ADR 设计可信用户、精确目标/期限的授权 | 推荐 A；B 也不得覆盖 metadata、App loopback/媒体/MCP、危险 scheme 等既有永久拒绝项；本 ADR 不批准例外 |

## 退出条件（什么情况下推翻本决定）

用户未接受接口/所有权选择则保留 L0；原生身份、双 view 隔离、全请求目的地限制或清理语义任一
无法在目标平台证明，关闭该平台 L1 并回到方案 B 或另立 ADR。若未来需多窗口/每会话 profile、
DOM 能力、新代理或新的权限通道，重新评审协议和威胁模型，不扩展本 ADR 的例外来兼容。

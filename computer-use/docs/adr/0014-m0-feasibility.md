# ADR 0014 · M0a 载体可行性记录（无需真机部分）

本记录从属于 [ADR 0014](0014-l2-window-binding-by-challenge-marker.md)，不是新决策或生产实现。
D1–D4 已裁定：只验证 C4；时序证明为准入硬门禁；独立清理不能兑现就放弃载体；
按实际有设备的平台逐项验收。**L2/A4b 保持关闭；本记录没有任何平台通过结论。**
M0/M5 卡在 ADR 的“分阶段实现卡建议”，尚未登记进
[06 路线图](../../../docs/plans/computer-use-v2/06-roadmap.md)；本派工仅完成 M0a，不能记作整个 M0 验收。
约束同时来自 [03 架构](../../../docs/plans/computer-use-v2/03-architecture.md)
和 [08 规程](../../../docs/plans/computer-use-v2/08-grok-execution-protocol.md)。

## 1. 证据来源与 API 语义边界

Chrome 官方核对入口如下；**本次无法获取官方页面原文，在线文档核对为 not_run**。
下表不把 URL 引用、源码注释、测试 shim 或既有 ADR 的假设当成已核实的浏览器行为。
官方原文未取得的细节明确写“未确认”，供真机操作者同时核对；不凭 API 名称/记忆补全。

| 官方来源 | 需要核对的原文 |
|---|---|
| [action.setTitle](https://developer.chrome.com/docs/extensions/reference/api/action#method-setTitle) | `details.tabId/title`、tab/global 优先级、清空/恢复含义、Promise 完成语义 |
| [action.getTitle](https://developer.chrome.com/docs/extensions/reference/api/action#method-getTitle) | `details.tabId`、返回值与默认值、可调用上下文 |
| [action 的 tab 状态](https://developer.chrome.com/docs/extensions/reference/api/action#tab-specific_state) | 跨 tab/window、切换/移动/关闭/导航时的状态生命周期 |
| [MV3 service worker 生命周期](https://developer.chrome.com/docs/extensions/develop/concepts/service-workers/lifecycle) | 休眠、终止、全局变量与计时器、恢复触发；不得把正常 timer 当死亡后的清理保证 |
| [content scripts](https://developer.chrome.com/docs/extensions/develop/concepts/content-scripts) | 可用 API 子集、ISOLATED 与共享 DOM 的区别；页面世界与扩展世界的边界 |

### 1.1 已由现有代码确认的事实与未确认项

| 项目 | 可审事实（仅限现有代码） | Chrome MV3 行为仍需的证据 |
|---|---|---|
| `setTitle({tabId,title})` 的作用域 | [background.ts](../../tools/browser-extension/background.ts) 的 `mark()` 明确传 `tabId`，接口类型允许省略 `tabId`；现有调用没有省略 | tab-specific 对 global 的优先级、未指定 tab 的默认语义、空字符串是否清掉 override：**未确认**。ADR 引用官方文档作为按-tab 语义参考，本记录不把引用升级为本次核实 |
| 跨 tab / 跨窗口复用 | 调用没有传 browser-window 或原生窗口 ID；同一扩展服务多个 tab | A 的值是否始终不出现在 B、切换后是否缓存旧值、拖窗后如何跟随、tabId 复用/导航/替换/关闭后的处理：**未确认**。不能由 `tabId` 推出 OS 窗口身份 |
| worker 休眠/被杀后的残留 | 源码注释明确警告 tab title/badge 可比 worker 内存作用域活得更久；`shares` 只在内存 | 这是代码的风险说明，**不是本次实测**。标题是否残留、多久、浏览器重启/扩展重载/崩溃后的缓存：**未确认**；不得说休眠自动清除 |
| 当前恢复路径 | worker 安装时 `tabs.query({})`，逐 tab `refreshIndicator()`；空的 `shares` 使 `mark()` 写回本地化 `share`，badge 清空；导航/撤销/断线等调用 `clearTab()` | 启动何时发生、API 写何时反映到原生树、失败时是否恢复：**未确认**。调用异步且错误被忽略；没有 `getTitle` 回读、原生恢复证明或睡眠期间独立清理 |
| `getTitle` | 生产 `ChromeApi` 子集没有声明/调用 `getTitle`；本卡只在测试模型和手工探针使用它 | 当前 Chrome/Edge 上是否可用、返回默认还是 tab override、Promise 值：**未确认**。即使回读成功也只证明 API 状态，不证明工具栏控件/AX 已更新 |
| UIA / AT-SPI / macOS AX | 现有扩展不提供原生树读取或 chrome 控件认证 | 完整 title/tooltip 是否在按钮 Name/HelpText/name/description/AXTitle/AXHelp 暴露：**待真机**；hover 前可读性、截断、重复节点、懒激活、缓存、语言/缩放差异均待验 |
| 权限与可见载体 | [manifest](../../tools/browser-extension/manifest.json) 已有 `action.default_title`；只有 `activeTab/scripting/webNavigation/nativeMessaging` 四项权限，无 popup | 当前权限下所有必需事实是否可取仍须验证；不能为探针增加 `tabs/storage/alarms/offscreen/debugger` 等权限 |
| 页面/内容通道 | [native.ts](../../tools/browser-extension/native.ts) 仅接私有 native pipe；[content.ts](../../tools/browser-extension/content.ts) 接文档定向扩展 port；[shared.ts](../../tools/browser-extension/shared.ts) 无 challenge 字段；没有页面 `postMessage` 入口 | C4 尚不存在。未来必须把 nonce 留在 background/native 私有侧。官方的 API 暴露范围尚待核对；本卡 shim 只能检查模型中的信息流，不能证明真实 Chrome 不泄露 |
| 页面标题/历史/同步 | 当前 `mark()` 只写 action title/badge，没有 `document.title` 写入 | C4 对浏览器诊断/缓存/崩溃材料/同步的影响：**未确认**。不承诺“仅内存”或“绝不持久化” |

Chrome 的官方 API 文档即使核对成功，也不保证 Edge 或任何 OS 的无障碍映射。
UIA/AT-SPI/AX 不读到完整 nonce、不能确认工具栏属于固定扩展或位于 WebArea 外，均失败。
只在 tooltip 浮窗出现字符串也失败；不自动 hover、展开扩展菜单、切 tab、恢复/激活窗口来补证。

### 1.2 与共享指示器的冲突和串行方案（设计约束，未实现）

现有 `mark()` 在共享完成、失败、撤销、worker 启动重刷时都可写 title/badge，
并且两个 API Promise 没有统一完成/错误处理。C4 直接追加写入会与这些路径竞争：
`mark()` 可能擦掉 nonce，迟到的挑战写入也可能覆盖撤销后的“共享此标签页”。
不能靠忽略此变化、反复抢写或事后重新挑战修补。

未来最小串行约束是每 tab 只有一个指示器所有者，所有 `mark/安装/回读/清除/启动恢复`
经同一有界队列与代次检查；nonce 只暂存 action title，badge 仍表示共享/失败，不承载 nonce。
挑战保留本地化“停止共享”语义与原撤销点击行为。撤销/Stop 先封准入、废弃代次，
清理不能排在动作队列后；旧写入/旧清除不得影响新挑战或新文档。
清除按当前共享状态恢复本地化 tooltip，不能撤销后恢复“共享中”。
需同时取得扩展回读和同窗原生新读取的恢复证据；单个 Promise/clearAck 不足。
队列和代次只约束活着的 worker，**没有解决 worker 死亡时的独立清理**；
源码目前不能证明 D3 可兑现，因此没有生产准入依据，不能开始 M1–M6。

## 2. T02 / T03 / T04 对 C4 的论证

| 威胁 | 可以成立的限定结论 | 未验证事实与拒绝条件 |
|---|---|---|
| T02：A 搬运给 B，清掉 A，只剩 B | C1/C2 的共享 DOM 可被页面 observer 读取；下述夹具机械产生唯一 B 命中而源文档为 A。C4 若 nonce 仅在可信私有侧且页面无 action 读写能力，可切断这条页面搬运路径 | API 暴露范围待官方核对；真实浏览器中的信息流待验。必须同时证明跨-tab 不复用、原生 chrome 载体身份及完整枚举，不能接收 WebArea 里的伪工具栏。shim 通过也不能授予真实绑定 |
| T03：同 URL/title/进程/frame/fingerprint 的 B | 克隆页面内容不等于能写固定扩展的原生 action。若 C4 的受信 chrome 边界可证，页面内容相关性只能作失配检测，无法替代该边界 | 本地 jsdom 没有 OS 进程、窗口、frame/fingerprint oracle，不声称完整 T03 已复现。工具栏归属、完整提交 URL、窗口实例、真实克隆实验均 **not_run**；边界/URL 缺失即拒绝 |
| T04：最终采样后切 tab/拖窗/导航 | C4 不提供浏览器事务锁。三次采样、200ms 跨度、高熵/短 TTL、事件监听或窗口锁均不能使 OS 读取与扩展 dispatch 原子化 | 最后 OS 校验 → 清除恢复 → 发布 → 实际 DOM 副作用之间，浏览器仍可能变化；事件可迟到。现有接口下的可审时序证明和独立 action-document/policy-window oracle 均缺失，保持关闭；一次错投即失败，事后 unknown/回滚不算通过 |

一次 `getTitle()` 不认证 documentId；按 tab 的 title 即使正确，tab 内导航也可能换文档。
清除标记后再检查 active/window/document 元数据，仍必须证明最后检查与副作用之间无错配。
本卡既不提供此证明，也不以测试模型中的同步 Map 冒充证明。

正常事务仍须在既有工作预算 4750ms / 外层 5s 内完成基线、设置、至少三次新读取
（首末 ≥200ms）、完整交叉校验、双端恢复和最终重验/投递。
失败先撤许可/双侧绑定，再独立清理最多 5s；总计最迟 10s 报告恢复或未确认。
Stop ≤100ms 锁存、≤500ms 发释放不变。不得延时、keepalive、加权限来通过。
若死亡/挂起后独立清理无法兑现，按 D3 放弃 C4；本次的未确认项不是放宽该条件的理由。

## 3. 本地 T02 夹具与 oracle

文件：[challenge-marker.test.ts](../../tools/browser-extension/tests/challenge-marker.test.ts)、
[最小夹具](../../tools/browser-extension/tests/fixtures/challenge-marker.ts)。
两个本地 about:blank iframe 作为 A/B 的文档模型，不加载资源、不访问网络/账号；
它们不是两个 OS 窗口，也不模拟真实浏览器原生树。

`page_relays_marker_unique_wrong_window_$carrier_$transport` 的四个组合：
C1 的 A 用 MutationObserver 读取 `document.title` 后缀；C2 读取按钮 `aria-label` 后缀；
分别经真实 jsdom `postMessage` 任务或同源 Window 文档引用搬到 B，并清除 A。
候选扫描三次都只命中 B；独立 Document→A/B 身份表仍把共享源认作 A，两个动作计数均为 0。
三次只是稳定性反例，**没有模拟 200ms 原生采样，也没有实际绑定/投递**。
postMessage 的 `*` 只在恶意本地夹具使用；不把 jsdom 的 origin/source 行为当浏览器认证。

`candidate_oracle_%s_distinguishes_original_duplicate_and_absent` 自证扫描器能区分
原始 A、A+B 双份、无标记，防止把“永远只返回 B”误当反例。
页面载体原文档回读/非预期变更锁存可拒绝这次简单清除攻击；本夹具只反驳
“唯一命中足以认证来源”，不声称击破 ADR 中全部组合校验。

`c4_nonce_stays_out_of_page_and_content_visible_objects` 在测试私有闭包 Map 中模拟 action 状态；
nonce 不交给页面 API shim，也不进入现有 ContentSession 的激活消息。
shim 记录 DOM/标题、DOM observer 已捕获副本、页面消息和内容 port 输入。
`visibility_oracle_%s_detects_deliberate_nonce_leak` 分别故意把 nonce 放进
title、aria-label、页面消息、内容消息，验证探测器有阳性结果且清除 DOM 不召回副本。
这是指定可见 API 集上的信息流测试，**不是 Chrome API/AX/worker/生产 C4 的证明**；
没有扫描任意浏览器内部对象或宣称未来实现也不会泄露。

[build.mjs](../../tools/browser-extension/build.mjs) 只从 `background.ts/content.ts` 构建，
`publicDir:false`，只额外复制 manifest 与 `_locales`；manifest 没有 tests 入口。
[store.test.ts](../../tools/browser-extension/tests/store.test.ts) 新增包边界断言：
dist 顶层只含两个 JS、manifest、locales，递归无 tests/fixtures，JS 无夹具标识。
源码层没有生产模块导入本卡测试。构建产物断言须在依赖齐备时运行，不能由静态检查代替。

复核命令（仓库根目录）：

```sh
corepack pnpm exec vitest run computer-use/tools/browser-extension/tests/
corepack pnpm exec tsc --noEmit -p computer-use/tools/browser-extension/tsconfig.json
python3 docs/plans/computer-use-v2/reviews/check_plan.py
git diff --exit-code -- computer-use/tools/browser-extension/manifest.json computer-use/tools/browser-extension/manifests computer-use/tools/browser-extension/store/permissions.md computer-use/tools/browser-extension/build.mjs
```

本次 Vitest 因可执行文件缺失未启动：**not_run**，不是通过，也不是用例断言失败。
生产入口/权限、ADR 正文、STATUS、Host/ACP/Driver/DomChannel 均不修改。

## 4. 真机实验：操作清单（全部 not_run）

仅由用户在一次性、未登录 profile 上执行。以下是载体测量，不接 Router/DOM 投递，
不安装 native host、不触碰个人配置、不用真实站点/账号。不要用实际 nonce 写报告；
记录完整相等与否、阶段、耗时、控件字段/祖先类型和版本即可。截图只覆盖一次性夹具。
用户在各次会改变 UI 的步骤开始前确认本地实验；本卡未代用户执行任何一步。

| 平台组合 | Chrome | Edge | 证据边界 |
|---|---|---|---|
| Windows win-test 物理桌面 | not_run | not_run | 待用户在场；不是远程无交互 runner |
| Mac mini | not_run | not_run | 需用户在场处理权限/查看 UI；不外推其他 macOS/架构 |
| Linux X11 | not_run | not_run | 是否有物理真机待确认；Xvfb 不算 |
| Linux native Wayland | not_run | not_run | 是否有物理真机待确认；GNOME/KDE/wlroots 分开，XWayland 不算 |

### 4.1 公共步骤 K0–K6（每个平台的 Chrome、Edge 分别重做）

**K0 准备。** 在受信构建环境按
[既有构建步骤](../../tools/browser-extension/README.md#build-and-checks) 得到 dist，
由用户加载到下文一次性 profile；打开 `chrome://extensions` / `edge://extensions`，
开发者模式 → 加载已解压扩展 → 选 dist，核对 ID `jgpkhdjlddaoopklipcnpcjcpiancmff`。
固定图标；打开两个普通窗口 W1/W2，W1 两个 about:blank 标签 A/A2，W2 一个 B。
预期：无登录、无共享 dot、提示为本地化“共享此标签页”。没有匹配构建/ID 则停止该项。
版本取 `chrome://version` / `edge://version`；另记 OS、架构、语言、缩放、桌面类型。

**K1 定位与基线。** 从扩展管理页点击 service worker 的“检查”，在**扩展 worker Console**
逐段粘贴（不能粘到页面 Console）。只输出 tab/window ID 和选中状态，不读别的 tab 标题/URL：

```js
var m0Tabs = await chrome.tabs.query({});
console.table(m0Tabs.map(({id, windowId, active}) => ({id, windowId, active})));
```

用户按窗口中选中标签的 ID 选择 A/A2/B；必要时手动切 tab 后重新列出 ID 对照，
不能从 tab ID 推导 HWND/XID/CGWindowID。把下段三个整数替换为刚确认的 ID：

```js
var m0A = 101, m0A2 = 102, m0B = 103;
var m0BaseA = await chrome.action.getTitle({tabId:m0A});
var m0BaseA2 = await chrome.action.getTitle({tabId:m0A2});
var m0BaseB = await chrome.action.getTitle({tabId:m0B});
```

预期：API 可调用，三者与各自未共享提示一致；异常/未定义即该项失败，不增权。
在原生检查器记录 W1/W2 下固定扩展按钮的基线字段/祖先；不要只全文搜字符串。

**K2 写/读与隔离（用户可见：改变 A 工具栏提示/读屏名称，不改页面标题）。**

```js
var m0Nonce = Array.from(crypto.getRandomValues(new Uint8Array(32)), b => b.toString(16).padStart(2,'0')).join('');
var m0Title = m0BaseA + ' [gcu:' + m0Nonce + ']';
await chrome.action.setTitle({tabId:m0A, title:m0Title});
console.table({A:(await chrome.action.getTitle({tabId:m0A}))===m0Title,
 A2:(await chrome.action.getTitle({tabId:m0A2}))===m0BaseA2,
 B:(await chrome.action.getTitle({tabId:m0B}))===m0BaseB});
```

预期三项均 true。在不 hover 的情况下检查 W1/W2 原生树：只 W1 的固定扩展工具栏按钮
在 WebArea 外完整携带 64 位十六进制 nonce；W2 没有，WebArea/页面标题没有。
至少三次重新抓取，首末 ≥200ms，记录更新/清除延迟；原生检查器不能提供全量新读取
或精确比对时记 not_run。原生字段截断、只有 tooltip 浮窗、无法证明按钮身份则失败。
本人工探针不构成生产 5s 事务的时序证据，人工停留也不用于宣称 TTL 合格。

**K3 切换与移动（用户可见：手动切 tab/拖窗、当前工具栏提示变化）。**
用户选 W1 的 A2：预期工具栏回到 A2 基线且没有 nonce；再选 A，预期只 A 的按钮有 nonce。
把 A 拖到新普通窗口 W3，记录 tab/window ID 与原生句柄/祖先，再把 A 移回。
要求标记不遗留于原窗口、不复制到 B/A2，不能因窗口相同而认作同文档。
另测关闭/替换/重载 A、全屏、未固定图标、最小化与遮挡；每个变化独立记录。
缺载体/树不全应判不可用，不自动使它重新可见。竖向标签页单独做一次。

**K4 活 worker 清除（用户可见：恢复工具栏提示）。**

```js
await chrome.action.setTitle({tabId:m0A, title:m0BaseA});
console.log((await chrome.action.getTitle({tabId:m0A}))===m0BaseA);
```

预期 true，且同一原生窗口新树恢复基线、完整 nonce 消失。若 A 已关闭，记录 tab 已销毁
并核对全部存活候选无残留；不向新文档/复用 ID 写旧基线。
不得只靠 API 值证明原生清理，也不使用语义尚未确认的 `title:''` 作恢复。

**K5 休眠/强杀与恢复（用户可见：提示可能残留，恢复后会变回未共享提示）。**
用新建 A 重做 K1/K2，随即关闭全部扩展 DevTools；检查器连接会干扰 worker 生命周期。
不点击扩展/不切 tab，观察扩展管理页是否实际显示 inactive，最长观察 90s。
没观察到休眠就记“休眠未复现/not_run”，不能用等待时长推定已休眠。
原生检查器检查残留；另一次写入后在浏览器任务管理器中仅结束该扩展的 service worker
（没有此项则 not_run，不杀整个日常浏览器）。记录实际终止时刻及之后 ≤5s 的原生恢复情况。
未唤醒期间无证据或仍有标记，均不能认定独立清理兑现；不靠手工清理把该项变通过。
再打开 worker 检查器唤醒：重新 `tabs.query` 识别原 A、`getTitle` 回读，核对源码启动重刷
是否恢复本地化 share 且 badge 清空，再核对同窗原生树；不自动续 share。
原先 Console 变量会丢失，不能引用旧 `m0BaseA` 或旧代次清新状态。
扩展重载、一次性浏览器异常退出/重开另行记录，未测写 not_run。

**K6 收尾（用户可见：恢复/移除实验图标）。**
确认所有一次性窗口的 title/badge/原生树均回到未共享基线；未知残留时禁用并移除
**该一次性 profile** 中的扩展、关闭该 profile，报告“恢复未确认”。不要操作个人 profile。
人工恢复、禁用/移除和启动重刷只是实验收尾，不是死亡期间独立清理的通过证据。

### 4.2 Windows win-test 物理桌面

用户先在 win-test 登录普通交互桌面；不提权、不在安全桌面试输入。分别启动：

```powershell
& "$env:ProgramFiles\Google\Chrome\Application\chrome.exe" --user-data-dir="$env:TEMP\cu2-m0-chrome-$([guid]::NewGuid())" --no-first-run about:blank
& "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe" --user-data-dir="$env:TEMP\cu2-m0-edge-$([guid]::NewGuid())" --no-first-run about:blank
```

若路径不同，用户从已安装浏览器快捷方式确认路径，不下载安装器或修改系统配置。
两浏览器逐个执行 K0–K6。用已安装的 Microsoft Inspect（Windows SDK）切 UI Automation，
选中 W1 工具栏固定扩展按钮，查看 Name、HelpText、ControlType、AutomationId 及父级 HWND/PID；
关闭“跟随鼠标”后按 K2/K4 刷新，不能用 hover 后新出现的 tooltip 窗口补证。
预期：按钮属核验过的浏览器窗口/Toolbar，非 Document/WebArea 后代；完整 nonce 唯一且清除后恢复。
还须由后续生产 Worker/UIA 取得同样证据；Inspect 通过不能代替 Driver 验收。
Inspect 不在机器上或无法读取完整候选树：此项 not_run，留给总控安排。

### 4.3 Mac mini（需用户在场）

用户在本机桌面分别启动；不得从无人值守终端替用户点 TCC：

```sh
open -na 'Google Chrome' --args --user-data-dir="$(mktemp -d -t cu2-m0-chrome)" --no-first-run about:blank
open -na 'Microsoft Edge' --args --user-data-dir="$(mktemp -d -t cu2-m0-edge)" --no-first-run about:blank
open -a 'Accessibility Inspector'
```

两浏览器逐个执行 K0–K6。若检查器需要辅助功能权限，由用户自行决定；无权限就 not_run，
不改浏览器启动参数/全局 accessibility 开关。用已安装的 Xcode Accessibility Inspector
定位固定扩展按钮并锁定选择，查看 AXRole/AXTitle/AXDescription/AXHelp 和窗口/Toolbar 祖先。
预期：同 K2/K4；非 AXWebArea 后代，完整值在不 hover 时可读，清除后同窗新读取恢复。
记录冷树/已激活树两种结果；外部检查器激活后的结果不能代替 helper/App 的 AX 开关恢复与 TCC 责任链。
检查器不可用、用户不在场或许可不足：对应项 not_run。

### 4.4 Linux X11 / Wayland（是否有物理真机待确认）

在实际物理登录会话中先执行，不从 DISPLAY 存在推断真机：

```sh
printf '%s\n' "$XDG_SESSION_TYPE" "$XDG_CURRENT_DESKTOP" "$WAYLAND_DISPLAY" "$DISPLAY"
google-chrome --user-data-dir="$(mktemp -d -t cu2-m0-chrome.XXXXXX)" --no-first-run about:blank
microsoft-edge --user-data-dir="$(mktemp -d -t cu2-m0-edge.XXXXXX)" --no-first-run about:blank
accerciser
```

用已安装浏览器/Accerciser；缺工具就 not_run，不临时安装或强开 accessibility。
两浏览器逐个执行 K0–K6；Accerciser 选中固定扩展工具栏按钮，查看 role、name、description、
application/frame/toolbar 祖先并刷新。预期同 K2/K4；非 document web 后代，完整 nonce 与原生窗口同属。
X11 还需后续驱动证明 AT-SPI 窗口与 XID/实例精确一致；Wayland 分 compositor 记录，
通过浏览器版本页/窗口系统诊断确认实际使用 native Wayland 还是 XWayland，不能只看环境变量。
无法确认原生后端或精确 toplevel/输入截图来源：拒绝该组合；不改启动 flags/portal/系统配置绕过。

### 4.5 这些测量之后仍缺的 M0/M5 门禁

K0–K6 即使都成功也只回答载体问题，不满足生产准入。下列均 **not_run**：

1. 生产 A4a 启动链/映像/实例、Q4 唯一 session、App 授权、完整已提交 URL 的独立来源、
   全候选枚举/原生窗口归属、冷/热树完整性及 frame/fingerprint 校验。
2. 用真实双窗口同源/跨源恶意本地页面执行 T02/T03：页面源 A、候选/政策 B、实际文档和动作次数
   分别由独立 oracle 记录；不得只检查错误字符串或标题相似。
3. T04 在最后采样、清除、发布和 dispatch 的每个 await 边界注入切 tab/移动/导航/迟到事件。
   当前没有可执行的生产 binder/permit/dispatch 测试钩子，不能编造命令或手工“快切 tab”充当确定性 oracle。
   完整断言 `policy-window == actual-action-document` 且禁止动作零副作用，任一次错投即拒绝。
4. 写前已就绪的独立清理机制、迟到 set/clear 代次、取消/Stop 时限和 5s 事务预算。
   当前代码未提供 C4 独立清理；如果实验/后续审查证实无法兑现，按 D3 退出，不启动生产实现。

没有证书/runner/原生来源证据的组合继续 not_run；本记录不要求放宽接口、权限或安全边界。

## 5. 实测结果（2026-10-19，Windows × Chrome 的一次运行，K1–K4 的一部分）

以下只记录执行者回报的事实，是**一台机器、一个浏览器、一次运行**的观察，不外推到其他平台、
浏览器、版本或语言。执行者是用户委托的 Grok 代理（用户在场）；总控没有在现场复核，也没有拿到
截图，原始字段为执行者文字回报。

**环境。** 主机名 `seven`（即 win-test，账号 `claudetest`），Windows 11 家庭版 10.0.26200，
Chrome 155.0.8059.84，显示缩放 100%，中文界面。一次性 profile，加载 `D:\cu2\m0\ext`
（ID `jgpkhdjlddaoopklipcnpcjcpiancmff`）。窗口 1：A、A2、`chrome://extensions`；窗口 2：B。

**与操作清单的偏离（必须计入证据强度）。**
1. 读取没有手工使用 `inspect.exe`，而是由代理直接调用 Windows UI Automation API 读取同一套属性。
2. K1+K2 的脚本不是粘贴进 DevTools Console，而是通过 CDP 挂到该扩展的 service worker 执行同一段代码。
   CDP 挂接本身可能影响 worker 的休眠/终止行为，因此**本次运行对 K5（休眠/强杀后的残留）没有任何证据价值**。
3. 没有记录工具栏按钮所属顶层窗口的 HWND/PID，也没有读取窗口 2 的扩展按钮。

**K2（写入与隔离）。** nonce=`7170dc89804efee508303d77ae2ca87b2b11211f8c9a11ab9281c053ce34039e`；
`getTitle` 回读：A=true，A2=true，B=true（接口层的 tab 作用域隔离，与上一次手工运行一致）。

**UIA 读取（标签 A 选中时，窗口 1 的扩展工具栏按钮）。**

| 字段 | 回报值 |
|---|---|
| Name | `共享此标签页 [gcu:<完整 64 位 nonce>]`，其后换行再加一行 `想要访问此网站` |
| HelpText | 空 |
| ControlType | `ControlType.Button` |
| AutomationId | `view_1009` |
| 父级链 | Button ← Pane ← ToolBar ← Pane… ← Window `about:blank - Google Chrome`；在工具栏内，不在 Document/网页区 |

- 完整 64 位 nonce **出现在 Name 里，不在 HelpText 里**（HelpText 为空）。
- 选中 A2：Name 回到 `共享此标签页` + 换行 + `想要访问此网站`，nonce 消失；选回 A：完整 nonce 再次出现。
- K4 清除（`setTitle` 写回默认值，回读 true）：按钮 Name 恢复为 `共享此标签页` + 换行 + `想要访问此网站`，nonce 消失。

**对 ADR 的含义（仅限这次观察）。**
- 在 Windows × Chrome 155 上，action 的按-tab 标题**确实出现在浏览器 chrome 的工具栏按钮的 UIA Name 里**，且随选中标签切换、
  随清除恢复，不在页面 WebArea 内。这支持 C4 作为“页面无法读取的非页面载体”这一前提，但只是前提之一。
- Name 是**组合字符串**：除标题外 Chrome 还追加了 `想要访问此网站`（与 activeTab 的访问状态有关，成因未确认，
  且可能随语言、权限状态、Chrome 版本而变）。因此未来匹配不能用“Name 等于某串”，只能按已核验的结构解析，
  再精确比较完整 nonce；这与 ADR §3 的“完整 nonce 精确匹配、不做模糊归一化”并不冲突，但需要明确的解析规则与测试。
- HelpText 为空：不能依赖 HelpText。

**仍然是 not_run / 未证明（本次运行没有改变它们）。**
- K3：拖窗、跨窗口、关闭/替换/重载、最小化/遮挡/全屏、未固定图标、竖向标签页。
- **K5：worker 休眠/被杀后的残留与恢复时限**——D3 的决定性证据，仍然没有；本次运行还因 CDP 挂接而不可用于推断。
- T04 最后投递竞态、T02/T03 的真实双窗口恶意页面实验、冷/热 UIA 树完整性、读取延迟与更新延迟（≥200ms 三次独立读取）。
- 顶层窗口归属：需要同时记录按钮所属窗口的 HWND/PID，并证明它与扩展端的 browser-window/tab 一一对应（本次未记录）。
- 窗口 2（B）的按钮是否保持基线；Edge；macOS；Linux；其他语言/版本；管理员 vs 标准账号。

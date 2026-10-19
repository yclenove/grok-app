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

## 7. T04 时序分析（M0b）

### 7.1 结论、输入与证明责任

**不能证明最后一次原生校验到 DOM 副作用之间始终同窗、同选中文档。按 D2 保持所有平台 L2/A4b 关闭。**
下面的参考算法即使获得比现有代码更强的读取能力，仍有确定性的切标签、移窗反例。
测试预期反例成立表示检测到了设计缺口，不能计为 T04 准入 oracle 零错投。
本节不实现 binder、不更改接口/权限，也不请求放宽 D2；若继续追求准入，需要另立 ADR
评审能在浏览器实际执行处验证并约束原生窗口、选中文档与副作用的可信能力。
增加一次回读只是把“最后一次检查”后移，不能完成该证明。

本节采用的输入边界：

| 输入 | 事实及限制 |
|---|---|
| ADR 0014 D2、§3/§5 T04/§6 | [ADR 正文](0014-l2-window-binding-by-challenge-marker.md)：锁只约束 gcu；每次请求都须新挑战、恢复校验和投递前重验；一次错投或证明缺失即关闭 |
| 2026-10-19 D3 修订 | 本次 M0b 派工明确“残留标记须无害且可观测”。本分支 ADR 正文尚无“D3 修订”段，前文 M0a 保留旧裁定。本节按较新的派工要求分析：残留不是授权、不得复活 share/permit，恢复未确认须可报告；不把残留自动判为错投，也不宣称崩溃后 5s 物理清除 |
| Windows×Chrome 载体事实 | 来自本次派工提供的 M0 §5/§5.1 结果：完整 nonce 在工具栏按钮 **UIA Name 的组合字符串**中，含“想要访问此网站”，HelpText 空；随选中标签切换；两窗口 HWND 不同、PID 相同。本 worktree 的既有记录止于 §4.5，未含该原始记录，因此此处注明转述来源，不补造版本/录屏/测量 |
| 本卡证据 | 测试目录内的纯模型与静态源码；无真实浏览器/OS 调用。Windows 事实只涉及所述载体观察，不证明窗口生命期、原生完整 URL、零缓存、最终投递或其他平台/Edge 行为 |

Name 不能与 `action.getTitle()` 整串相等；未来须先认证 WebArea 外固定扩展的原生控件，
再核对固定分隔内**完整 nonce**及已确认的组合结构，不可整树任意子串搜索。
HelpText 为空不能作为成功证据；相同 PID 不能合并 HWND，更不能把 browser-window ID 当 HWND。
本模型直接给予理想的原生载体/实例映射，**没有实现或验证此解析**。

现状核对：[background.ts](../../tools/browser-extension/background.ts) 只保存共享文档 scope、
去重请求并经 Router 通道授权后 `postMessage`；未重读 active/window/document。
[native.ts](../../tools/browser-extension/native.ts) 检查连接、share、当前一次请求与迟到响应归属，
不证明 OS 窗口。[content.ts](../../tools/browser-extension/content.ts) 用精确 document port、URL/
pagehide 与本地 DOM 句柄校验，不能读取原生窗口身份。
[shared.ts](../../tools/browser-extension/shared.ts) 无 challenge/窗口 permit DTO。
[Hub](../../crates/gcu-core/src/browser/mod.rs) 的 bind 是内存登记、align 是 frame/fingerprint 融合，
不是可信映射；[DOM router](../../crates/gcu-core/src/browser/router.rs) 不重放 unknown 到 L0。
[Connection](../../crates/gcu-cli/src/native_host/mod.rs) 的 bind/request 要求调用者先证明映射，
[proof](../../crates/gcu-cli/src/native_host/proof.rs) 只有 Unproven，
[gate](../../crates/gcu-cli/src/native_host/gate.rs) 继续拒绝；
[record](../../crates/gcu-cli/src/native_host/record.rs) 仅 Linux 记录有限 procfs 信息，其他平台 unavailable。
这些现有防线均未因本节或模型“通过”而增强。

### 7.2 带编号的交错步骤与检查后使用边界

以下是**待评审参考协议**的事件顺序，不是现有可调用生产协议。
G = 持有 Driver/Router 的 gcu CLI/helper/engine；N = native-host 转接；
E = 扩展 worker；B = 浏览器主进程/renderer；U = 用户（页面脚本也能触发导航等变化）。
每行按单个本地操作或单次消息/OS 调用列出；API 调用、完成和消费结果分开。
`S08(k,w,p)` 表示第 k 次读取中窗口 w 的属性/节点 p 的**一次**原生读取，必须逐调用重复；
整棵树、完整候选枚举、API Promise 乃至同一 JS task 都不被宣称是跨进程原子快照。
表中的“同步”只指当前进程内存，不锁 B/U。原生读取内部也可被并发打断，模型另行给予理想快照。

| 步骤 | 执行侧与单个事件 | U/页面能否并发改变事实 | 可用防线与局限 |
|---|---|---|---|
| S01 | G：Router 选唯一 local session，检查读取授权/Stop/策略并固定目标 | 能；审批等待期间尤其能 | 服务端保存身份/策略版本；等待期间无 nonce |
| S02a | G：取得会话串行及目标窗口变更锁 | 能，包括等待锁时 | 锁只挡其他 gcu 请求，不能挡 U/B；等待时无 nonce |
| S02b | G：在本地建立事务代次/剩余期限记录 | 能 | 只固定本次记录，不能重置已开始事务的时钟 |
| S03(w,p) | G→原生驱动：逐调用取得候选集/实例/chrome/URL/几何基线 | 能，在每个原生读取间 | 完整性、实例、基线不存在 nonce；不是一个原子 OS 事务 |
| S04 | G：核对基线并建立私有挑战记录 | 能 | 当前代次的内存检查；下一刻即可过时 |
| S05(h) | G→N、N→E 中第 h 跳发送窄安装消息（每跳单独编号） | 能，两段队列各有空隙 | 每跳收件方先本地检查连接/epoch/share/document/期限/归属，N 不授予 Router 权限 |
| S06a | E：本地验证安装归属与期限，登记清理责任 | 能 | 同一局部临界区、取消代次；尚无浏览器写入 |
| S06b | E：调用按-tab `setTitle` 提交写请求 | 能 | 自有写入串行；S06a 到实际写入不原子 |
| S07a | B：执行该次 set | 能 | 只改变指定 tab 的载体；不证明文档归属 |
| S07b | E：消费 set 的完成回执 | 能 | API 成功不证明 AX 已更新 |
| S07c | E：提交 `getTitle` 回读 | 能 | 非紧邻的原子读改写 |
| S07d | B：取得载体回读值 | 能 | 仅该时刻的 API 状态 |
| S07e | E：消费回读值并本地校验 | 能 | 精确值/代次不符则拒绝，迟到 set 不复活 |
| S07f(h) | E→N、N→G 中第 h 跳发送安装回执 | 能 | ack 归属校验；传输期间事实仍可变化 |
| S08(1,w,p) | G→驱动：第一次逐窗口/节点/属性的新读取 | 能 | 可信工具栏位置、完整 nonce、全候选唯一；部分树拒绝 |
| S08(2,w,p) | G→驱动：第二次独立新读取 | 能 | 同一实例/载体/URL/frame；旧缓存不能当新读取 |
| S08(3,w,p) | G→驱动：第三次独立新读取 | 能 | 首末至少 200ms；只检测稳定，不提供安全时间下界 |
| S09 | G：消费三次读值并比较组合身份/版本 | 能 | 所有已见失配拒绝；无法推断未见交错不存在 |
| S10(h) | G→N、N→E 中第 h 跳发送清除 | 能 | 清除仅属旧事务/原载体；先撤能力再故障清理 |
| S11a | E：本地核对自有载体/代次/当前共享状态 | 能 | 不覆盖新状态，不撤销后恢复成“共享中” |
| S11b | E：提交恢复本地化 tooltip 的 API 调用 | 能 | S11a 到浏览器写入仍异步 |
| S12a | B：执行该次恢复 | 能 | 不向新文档/新事务写旧基线 |
| S12b | E：消费恢复完成回执 | 能 | 尚未核实实际 API 状态或 AX |
| S12c/d/e | 分别重复 S07c/d/e 的一次事件，回读并校验恢复值 | 能 | 三个不同编号；API 回读仍不证明原生恢复 |
| S12f(h) | E→N、N→G 中第 h 跳发送清除回执 | 能 | clearAck 不等于物理恢复；死亡后残留无权限且可观测 |
| S13(w,p) | G→驱动：清除后同窗逐次新读取/恢复校验 | 能 | nonce 消失且基线恢复；此组最后一个原生读取就是本轮“最后原生校验” |
| S14 | G：消费原生恢复结果，核对期限/版本 | 能 | 不新开时钟；结果可能已在队列中变旧 |
| S15a | E：提交 tab 元数据请求 | 能 | 只指定已共享 tab，API 可用性见下表 |
| S15b | B：取得 tab 元数据值 | 能 | 同 tab、active、browser-window 的单次观察 |
| S15c | E：消费并本地校验 tab 返回值 | 能 | **返回值不是活引用**，S15b 后可过时 |
| S16a | E：提交 top-document 元数据请求 | 能 | 不能从 Tab 对象编造 documentId |
| S16b | B：取得 top-document 元数据值 | 能 | 独立于 S15b 的读取时刻 |
| S16c | E：消费并本地校验 documentId/URL | 能 | 与精确端口一致；不锁住 tab 所在窗口 |
| S17a | E：提交窗口状态请求 | 能 | 浏览器 id 不代替原生窗口实例 |
| S17b | B：取得窗口状态值 | 能 | 独立于 tab/document 的读取时刻 |
| S17c | E：消费并本地校验窗口返回值 | 能 | 最小化/存在性拒绝；三组读取不是原子快照 |
| S18 | G：暂存 Connection 绑定（本地） | 能 | 准入闸关闭，暂存不许可 observe/perform |
| S19 | G：暂存 Hub 绑定（本地） | 能 | 两侧成功前都不可投递，失败撤两侧；两次 bind 自身不原子 |
| S20 | G：同一局部临界区检查版本并发布准入闸 | 能 | 只保证两张内存表一起可见；不证明 B 此刻未变 |
| S21 | G：每次动作/观察、batch 每步重新走完整 Router/敏感确认 | 能 | 确认绑定窗口/文档/参数/快照/策略；变化后重审，无 nonce 跨审批等待 |
| S22 | G/N/E/B：用**新 nonce**重复 S02–S17 的每个事件 | 能 | 包含新基线、三读取、清除、恢复；不复用绑定时证明 |
| S23a | G：本地固定该次 permit/pending | 能 | 单次消费、epoch、过期、Stop |
| S23b(h) | G→N、N→E 中第 h 跳发送该次请求 | 能 | 每跳独立；消息在途仍可失配 |
| S24 | E/B：dispatch 前再次分开执行 S15/S16/S17 | 能 | 只把最后元数据检查后移；active/window/document 非共同快照 |
| S25 | E：本地校验 share/epoch/permit 并单次消费 | 能，U/B 与 worker 异线程/进程 | 同步操作能堵住同 worker 的重复消费，不能锁住真实窗口 |
| S26 | E：对精确 documentId 的既有 port `postMessage` | 能 | 无 JS await 仍有跨进程/消息队列；不得回退到当前 tab 或其他 document |
| S27 | B renderer：content handler 收到并验证 live share/URL/原句柄 | 能 | 同步检查只约束本 renderer；它没有 OS 窗口/选中 tab 的可信实时事实 |
| S28 | B renderer：产生 DOM 副作用 | 能在先前任一间隙改变归属 | 此刻才是 oracle 的线性化观察点；原文档可已移到另一个窗口/后台 |
| S29(h) | B→E、E→N、N→G 中第 h 跳返回结果（通知到达另作各侧本地事件） | 能 | 每次接收先验 pending/代次；unknown 不重放，不能撤销 S28 的错投 |

S21/S22 是重复子程序，不把多次检查合为一个原子动作。正常绑定和一次动作验证/投递
分别共享已有工作预算 4750ms / 外层 5s，三采样首末 ≥200ms 包含其中；
失败先撤 permit/Connection/Hub/pending，再独立清理最多 5s，最迟 10s 报告恢复或未确认。
Stop 锁存/释放不排普通事务队列；用户恢复仅走既有可信入口。
这些预算和清理规则不能替代 S13→S28 的一致性证明。

### 7.3 四项权限下的读取依据

现有 [manifest](../../tools/browser-extension/manifest.json) 只有
`activeTab/scripting/webNavigation/nativeMessaging`，没有 `tabs`、host permissions 或其他可选权限。
本卡仅依据已有源码和派工事实，不把 API 名称或测试 shim 当作权限/时效性证明。

| 所需操作 | 已确认的代码/事实 | 当前四项权限下结论 |
|---|---|---|
| action 按 tab 写提示 | `mark()` 已调用 `setTitle({tabId,title})`；派工实测 Name 随选中 tab 变化 | 此用途已有依据；完成何时反映 AX、组合字符串解析、缓存/崩溃语义未确认 |
| `action.getTitle` 及恢复值 | M0a 探针有调用；生产 `ChromeApi` 子集无此方法/回读 | 此分支无 API 原文/该项实测记录，**未确认**；即使可用也不认证文档/AX |
| dispatch 前 `tabs.get(tabId)` 的 active/windowId | 现有只用 `tabs.query({})` 启动重刷与 onClicked 的 id/url；没有 get/active/windowId 重读 | **未确认**；query 被调用不能证明所有新字段、目标版本、activeTab 生命周期都可用；不得默认已获 `tabs` 权限 |
| 当前 top `documentId` | 现有 `scripting.executeScript` 结果取 frameId=0/documentId，`tabs.connect` 精确定向 | 现有路径有代码依据；dispatch 前 `webNavigation.getFrame`（或等价无内容读取）尚未使用/核实，**未确认**；Tab 本身不提供已确认的 documentId 字段 |
| 窗口存在性/最小化/前台状态 `windows.get` | 生产扩展没有 windows API 子集/调用 | **未确认**；模型只给予窗口 id/minimized，不从浏览器 id 推 OS id |
| active/attach/detach/focus 事件与单调代次 | 已有 onRemoved/onReplaced/onUpdated、四类 webNavigation 事件、port 断线 | 没有 active/移动/窗口焦点监听；新增监听可用性/顺序**未确认**，事件回调不是同步失效屏障 |

即使把以上未确认项全部理想化为可用且线性化读取，也只证明各自取值时成立，
仍没有“检查条件并在同一原生窗口的同一选中文档执行 DOM”的共同原子 API。
参考算法不为验证元数据重新注入内容脚本，不读取其他 tab 的 URL/title，也不增加权限。

### 7.4 最后原生校验至副作用：并发动作清单

这里覆盖按 tab 身份、窗口身份/几何/可见性、文档生命期/同文档版本、扩展生命期、
控制消息时序分类的交错；页面可触发的重定向/history 与用户操作同等对待。
不是声称有限单事件枚举穷尽任意长度的真实浏览器执行。
“完全堵住”只用于明确模型假设内的对应边界，不能升级为平台结论。

| 并发动作 | (a) 现有防线 / 参考算法附加检查 | (b) 残余窗口性质 | (c) 结论与 D2 |
|---|---|---|---|
| 窗口内切 tab（含切走再切回） | 现有精确 port 防换收件文档，但仍可给后台原文档动作；无激活监听/主动重读。参考算法 tabs active + windowId 回读，权限/时效性未确认 | 最后 tab 读值后可切换，documentId 仍相同；切回的 ABA 会躲过值比较。锁存事件若未及时送达也不能识别；worker 同步段不能消除 | **不能证明，无法排除错投**。模型 `dispatch.tabs.get:after` 起可在非选中原文档执行；保持 L2 关闭 |
| tab 拖出/拖入/合并到其他窗口 | 精确 port 跟原文档，不认证它当前在哪个 OS 窗口；无移动监听。参考算法 browser-windowId + 原生实例重验 | 最后 tab 读值后移窗，旧窗口仍存在，后续 document/windows 查询都可通过。跨进程检查只能缩短，不能由局部锁原子消除 | **不能证明，无法排除策略 W1/动作 W2**；按 D2 关闭 |
| 原窗口拖动/缩放/DPI 改变 | Router 已有几何门禁，绑定还需 frame/fingerprint；参考模型理想原生几何检查 | 最后原生读取后几何可变，扩展 windows 状态不是原生 geometry。纯 DOM 语义目标可能仍同窗，但截图/确认依据已旧 | **只能缩小，不能证明完整门禁**；不以“没有换文档”放行 |
| 跨文档导航、重载 | 现有导航事件清 share、pagehide、精确 document port、URL/DOM 句柄；参考模型旧 Document 销毁后不可执行 | 新文档不继承 port 时，在新文档错投可排除（模型内）；实际导航请求、提交、旧 renderer 执行/断线先后无本卡证据，旧文档可能在撤销到达前执行 | **模型可证明销毁后的新文档零动作；生产不能证明全部交错**，不自动用新文档重试，L2 关闭 |
| 同文档 history/fragment、标题/DOM/敏感语义变化 | 现有 history/fragment 事件、URL live、DOM stamp/MutationObserver；Router 快照/确认版本 | 模型中 URL 持续变化由 content 同步 URL 检查完全堵住；URL/标题先改后还原、页面同步事件处理器的变化不由该单事件模型覆盖。active/window 仍不可在 renderer 原子核验 | **局部可阻断，整体不能证明**；新的语义变化须重新观察/确认，不能借身份未变跳过 |
| 关闭 tab/窗口、浏览器退出 | onRemoved/port 断线、原文档 live；Connection/Hub/pending 清除 | 已销毁对象的定向 port 不能改投别的对象（模型假设）；若仅请求关闭而销毁尚未发生，需区分可能已执行，不能假报零副作用 | **模型销毁后零副作用；真实销毁/队列顺序未确认**，不能证明整个区间，维持关闭 |
| tab 替换（包括换 document、不同 tab ID） | onReplaced 清原 share；port 定向原 documentId，不按新 tabId/URL 查找替代品 | 模型令原文档销毁并拒绝所有边界。真实替换/旧对象/断线时序及 ID 重用仍需实例证据；不能只比相同 URL | **模型完全堵住所定义替换；平台不能证明**，旧 share 不迁移，L2 关闭 |
| 最小化、全屏/隐藏工具栏、其他桌面 | 原生不可用/缺载体应拒绝；参考算法 windows minimized 回读，API 未确认 | 最后窗口状态读值后仍可最小化；DOM port 仍可工作。全屏/隐藏/桌面归属是原生状态，不能由 title 仍在推断可用 | **只能缩小，不能证明门禁**；模型有最小化反例，其他可见性模式未建模/未实测 |
| 前后台窗口切换（包括别的 App） | ADR 要求窗口内选中 tab，未要求全局前台；Router 活动/Stop/敏感门禁仍适用 | 只变全局焦点且目标仍选中时，身份映射不变，无需抢焦点；若伴随切 tab/输入/几何/安全桌面变化则归上述类别。事件延迟不能变成输入锁 | **模型仅焦点变化可证明无身份错投**；不能据此外推活动守卫或真实桌面准入 |
| 扩展 worker 挂起/重启/扩展重载 | 内存 share 清空、NativeChannel generation/连接失效、启动 indicator 重刷；D3 残留不得有权限 | 局部代次能在 worker 发消息前阻断旧请求；已排进 renderer 的消息与 disconnect 回调谁先到未确认。原生/worker 内同步操作不能撤回已排队副作用 | **只能缩小，不能证明零副作用**；模型显式保留“已发出、断线尚未处理”的分支，关闭 L2 |
| 迟到激活/移动/导航事件、旧 set/clear/动作响应 | 现有 NativeChannel/Connection pending、epoch/share 检查和撤销；拟议标记须精确事务所有权，不能把成功回执当许可 | 已处理的失效可由同进程同步闸完全拒绝；未处理的通知不给真实变化加锁。旧响应可在内存原子丢弃，但副作用已发生则无法补救 | **响应重放可局部堵住，状态失效竞态无法排除**；事后 unknown/清除不满足 D2 |

严格的不可证明点可以只用两个可信窗口构造：S24 返回 A 在 W1/active；U 将 A 拖到 W2；
相关事件排队；E 的本地 share/permit 未失效，仍向原 A port 发送；S28 在 W2 的 A 执行。
策略依据 W1，实际动作窗口 W2。这个构造不要求网页读取 nonce、不要求伪造 ID/原生树，
不要求在 renderer 同步 JS 中插入另一 JS task，甚至允许此前所有采样完美准确。
把 tab/window/document 回读合并为一个理想快照也只会把缺口移到该快照之后。

### 7.5 确定性故障注入模型与边界分类

文件：[t04-dispatch.test.ts](../../tools/browser-extension/tests/t04-dispatch.test.ts)、
[t04-model.ts](../../tools/browser-extension/tests/fixtures/t04-model.ts)。全部在测试目录，
无生产模块导入；沿用并扩展 [store.test.ts](../../tools/browser-extension/tests/store.test.ts)
的包内容排除断言（`ReferenceBinding`、`EffectOracle`、模型 nonce 标识不得进入两 bundle）。
manifest、权限、构建入口、Host/ACP/Driver/DomChannel、STATUS 均不变。

参考算法提供可编程 fake `chrome.tabs/windows/action`、单独的 top-document 探针和 fake native 通道：
先绑定（清除/恢复前闸关闭、两侧暂存后发布），再新挑战重验、单次 permit、dispatch 前分开重读，
最后用预先存在的精确原文档 port 投递。它是用于反驳充分性的**乐观参考算法**，不是安全实现建议。
给予完整实时原生 chrome/URL/窗口实例来源、完整候选枚举、无缓存、正确 API 读取/完成、
旧 Document 一经销毁即不能产生动作、正常清除不失败等强假设。
这些假设未由当前 Driver/API 权限/真实销毁顺序证明；即使全部成立仍有反例。
nonce 为公开夹具常量，时间为虚拟计数，不检验密码学、真实 5s/Stop 时限或休眠清理能力。

正常 trace 有 **39 个异步 API/通道调用 × 调用前/快照或写入后两个边界 +
17 个辅助函数 await 的返回续执行点 + 2 个额外边界 = 97 点**。
每次测试只在一个边界注入一个事件；快照按值复制，`:after` 注入发生在取值后、消费返回前。
额外 `dispatch.beforePost` 允许外部浏览器在 worker 无 await 的区间改变状态，
`content.beforeEffect` 表示跨进程排队后、renderer 副作用前的交错；不是伪造真实 Chrome 的调试钩子。
14 个事件覆盖切 tab、移窗、几何、导航/重载/同文档 URL、关闭 tab/窗口、替换、最小化、
全局焦点、worker 重启、迟到失效通知、旧响应。
各跑通知立即送达/通知延迟两种模式，另跑 D2 关闭模式；共 4074 组合。
“立即送达”是模型有利假设，不表示真实事件有同步广播能力；“延迟”允许消息已经排队时才断线。

oracle 是独立的副作用接收器：用实际接收动作的 **Document 对象引用**，从实时桌面归属找到
原生窗口实例、该窗选中文档和动作计数，再与 Router 固定的政策依据比较。
不从 nonce、扩展自报 ID、错误码或参考算法比较函数推导“实际文档”。
核心断言 `policy-window == actual-action-document` 展开为：动作对象等于获批文档、
它此刻属于获批原生窗口且仍是该窗选中文档；另外检查几何、最小化和连接代次门禁。
禁止动作必须零副作用；阳性自证故意向同窗另一个 Document 记动作，必须被 oracle 检出。

下表是明确写入测试断言的**模型边界分类**，不是按返回状态自动接受任意结果：

| 事件 / 边界 | 完全堵住的模型前缀 | 仅缩小、显式保留的窗口（延迟通知） |
|---|---|---|
| 切 tab、tab 移窗 | 最后 `dispatch.tabs.get:before` 及此前，回读/下一轮重验拒绝且零动作 | 从 `dispatch.tabs.get:after` 到 content 副作用前，各 10 点产生错配；精确 port 仍收到 1 次动作 |
| 原生窗口几何变化 | 最后 `revalidate.final.native:before` 及此前 | 从该原生读值的 `:after` 起 23 点可能沿过期几何投递；身份未必错，几何门禁仍失败 |
| 最小化 | 最后 `dispatch.windows.get:before` 及此前 | 从该读值 `:after` 起 6 点仍可执行，违反可见性门禁 |
| worker 重启 | `dispatch.message:before` 及此前，当前代次检查拒绝 | `dispatch.message:after`、`content.beforeEffect` 两点，排队消息先于断线处理可执行；旧响应仍不能复活连接 |
| 导航/重载、关闭、替换 | 模型定义的旧 Document 销毁在全部 97 点均零动作；同文档 URL 持续变化由 content live 拒绝 | 不外推真实销毁顺序、同文档 ABA 或页面回调中的同步语义变化 |
| 迟到失效事件 | 在全部 97 点实际处理时，同步撤闸/撤 content 权限，零动作 | 物理变化发生但通知尚未处理的反例由切 tab/移窗等事件覆盖；事后处理不消除既有违规 |
| 旧响应、纯全局焦点变化 | 全部 97 点均保持同一窗口/选中文档，动作恰好一次；旧响应不改绑定 | 不证明真实焦点/活动守卫；不允许响应重放、切 tab 或隐式恢复 |
| D2 关闭 | 在任何待注入计划下，验证前就拒绝，无 challenge/探针/DOM 副作用 | 注入点因提前拒绝不可达，不能说这些事件已在关闭模式下实际发生 |

延迟通知模式明确预期 **20 个窗口/选中文档错配**，另有 23 个过期几何、6 个最小化、
2 个 worker 代次反例；合计 51 个安全断言反例。通知随后送达会撤权，但 oracle 保留原动作。
这些数是有限 trace 上的分类计数，不是概率、真实可利用时间或穷尽证明。
两阶段 bind 间失效撤销两侧；模型死亡后 title 可以残留，测试断言它可被观察、没有绑定/permit、
旧成功回执不能授予动作，不假装已物理清除。生产如何诊断残留/启动恢复仍由后续独立验收负责。

复核命令沿用 §3；另可用 Node 的 TypeScript transform 直接导入纯模型做辅助断言，
不能把此辅助执行计为 Vitest、TypeScript 类型检查、构建包或浏览器验收。
本卡环境 `vitest`/`tsc` 可执行文件缺失，两个 pnpm 检查均 **not_run**；
Node 辅助矩阵 4074 组合符合上述预期分类，计划检查器通过。
相对链接与未改 manifest/权限/构建入口另做静态自查，打包排除断言尚待依赖齐备时运行。
无新增真机结果；参考模型无法满足 D2 准入，生产路线继续关闭，不推进 M1–M6。

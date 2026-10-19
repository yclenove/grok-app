# 0014 用挑战标记探索 L2 共享文档与 OS 窗口的可信绑定

状态：**Exited（2026-10-19，维护者按 D2：最终投递竞态 T04 无法证明，退出挑战标记路线；见“退出记录”）**。此前：Accepted（探索范围；2026-10-18 维护者按推荐裁定 D1–D4，见“开放问题”末尾的裁定记录；2026-10-19 修订 D3 判据，见“D3 修订”）。**L2 与 A4b 仍关闭**，本裁定只批准“可行性验证”，不批准实现或启用。
日期：2026-10-18

## 背景（事实 + 证据链接）

用户于 2026-10-18 批准**编写 ADR 探索挑战标记绑定**，未批准改变安全假设、接口或启用 L2。本提案只定义验证方向、拒绝条件与后续卡，不修改代码、03、协议、路线图或 STATUS；0013 已用于 Windows 守卫。

| 已核对的事实 | 证据与限制 |
|---|---|
| A4a 不能从 native messaging 证明共享顶层文档属于哪个 OS 窗口 | [身份与映射说明](../browser-extension.md#proof)、[`proof.rs`](../../crates/gcu-cli/src/native_host/proof.rs)、[`gate.rs`](../../crates/gcu-cli/src/native_host/gate.rs)。本提案核对基线为 `origin/cu2/A4a-browser-identity`（PR #129），提交 `0a83f86aaef3bc4c425d0b4168162c317117cf1b`；文档与代码均明确真机证据为 `not_run`，不是已实测的平台结论。 |
| Windows 启动 HWND、Linux 浏览器 PID、macOS 父进程都不是文档身份 | Windows `--parent-window` 仅是启动时前台 HWND；X11 没有传入 XID；Wayland 没有认证的 toplevel；macOS 没有 CGWindowID。扩展 `tabId/windowId` 均非原生窗口 ID。`mapping_verdict` 对所有平台只有 `Unproven`，`BrokerGate` 不允许 DOM 投递。 |
| 启动链证明本身也有尚未交付的部分 | [`record.rs`](../../crates/gcu-cli/src/native_host/record.rs) 仅 Linux 读取 procfs 实例，其他目标返回 unavailable；[`proof.rs`](../../crates/gcu-cli/src/native_host/proof.rs) 的进程名/参数/版本比较不能替代 ADR 0012 要求的实际映像、构建身份和启动链核验。挑战标记不补齐这些身份缺口。 |
| 绑定和融合是不同的安全步骤 | [`Connection::bind/request`](../../crates/gcu-cli/src/native_host/mod.rs) 要求先证明真实窗口/文档；[`Hub::bind/align`](../../crates/gcu-core/src/browser/mod.rs) 保存 share，随后按 web-area fingerprint/frame 融合；[`merge.rs`](../../crates/gcu-core/src/browser/merge.rs) 核验视口尺寸并转换物理坐标；[`router.rs`](../../crates/gcu-core/src/browser/router.rs) 不将失败 DOM 动作重放到 L0。它们不自行证明 document→OS window。 |
| fingerprint 是一致性校验，不是认证 | [`fingerprint.rs`](../../crates/gcu-core/src/fingerprint.rs) 使用非密码学 FNV-1a，包含 role/name/identifier/祖先路径与 4 物理像素量化 frame；复制相同内容可得到相同 fingerprint，无需攻击哈希。DOM 本地 `stamp` 同样不是跨窗口身份凭证，且不能外发其未脱敏值（[`dom.ts`](../../tools/browser-extension/dom.ts)）。 |
| 扩展当前只有四项权限，且已经使用按标签页的工具栏标题 | [`manifest.json`](../../tools/browser-extension/manifest.json)、[完整权限表](../../tools/browser-extension/store/permissions.md) 为 `activeTab/scripting/webNavigation/nativeMessaging`；[`background.ts`](../../tools/browser-extension/background.ts) 用 `action.setTitle({tabId, title})` 和 badge 表示共享，仅程序化注入 top frame 的 ISOLATED world。ISOLATED 只隔离 JS 世界，不保密共享 DOM。 |

约束来自 [03 §10](../../../docs/plans/computer-use-v2/03-architecture.md#10-浏览器策略)、[P8-03](../../../docs/plans/computer-use-v2/06-roadmap.md#p8-03-dom-树融合与动作路由)、[ADR 0012 的生产连接、A4a/A4b 与 Q4](0012-real-driver-assembly.md)、[07 §7](../../../docs/plans/computer-use-v2/07-quality-gates.md#7-安全审查清单p6-05p9-04-必须逐项签字)。威胁模型沿用 [ADR 0010](0010-builtin-browser-window-protocol.md)：远程页面、标题、URL 文本、DOM、frame、弹窗和脚本均不可信；浏览器/扩展/OS 已遭控制，或同 UID 改写可信进程内存/二进制，不在本协议能保证的边界内。**恶意网页本身在边界内，不能排除。**

## 决定

### 1. 准入原则与当前结论

提出“经授权的短期挑战 → 原生无障碍交叉读取 → 清除并验证恢复 → 一次绑定”的验证流程。生产准入必须同时证明：

1. 标记只能由可信扩展写到**该已共享文档对应的浏览器界面载体**，网页不能读取、搬运或在另一候选窗口的受信位置重现它。
2. 授权浏览器进程实例下，完整候选集内恰好一个 OS 窗口稳定命中；同窗多次树节点命中须归属同一已识别载体，不得把多个不同载体简单去重为一个窗口。
3. 同时满足下文的浏览器元数据、原生窗口身份、web-area frame/fingerprint、生命周期和最终投递重验；无证据、不完整、歧义、超时均拒绝。

**页面 `document.title`、DOM aria-label 和 canvas 都不满足第 1 条。** 高熵只防提前猜测，不能防页面读出后实时转发。唯一命中 + URL/标题/进程一致 + 多次稳定 + fingerprint/frame 相同，仍不能把页面载体提升为可信窗口证明。以下 §5 给出反例，不因增加采样次数而准入。

建议先验证**扩展专属浏览器 chrome 中、按 tab 设置的 action 标题/提示文字**作为非页面载体（C4），它不需要新 manifest 权限，现有扩展已用此 API；但其原生 AX 可读性、tab 切换语义、清理与最后投递竞态都未确认。C4 是待裁定的替代载体，不是本 ADR 已解决绑定的宣称。若维护者只批准页面标题路线，则只做离线反例验证，L2 继续关闭；不自动改选其他载体。

接受本 ADR 也不能直接把 `WindowUnproven` 改为 allow。A4a 的拒绝保持原样，A4b 仍被阻塞，直到选定载体、对应平台证据、安全评审及最后投递时序证明全部验收。无法建立非转发绑定就退出方案，不请求把恶意页面改成可信页面。

### 2. 标记载体：原生可读性与取舍

下表是待证假设，**全部真实 Chrome/Edge 组合均 `not_run`**。能看见标题不等于能读到完整标记，也不等于证明文档身份。

| 载体 | Windows UIA | Linux X11 / Wayland AT-SPI | macOS AX | 取舍 |
|---|---|---|---|---|
| C1：临时 `document.title` 后缀 | 活动 tab 的窗口 Name / Document 名称可能带后缀；可能截断或缓存 | 活动 tab 的 frame/document name 可能可读；X11 标题也许可见，但不代替 AT-SPI/窗口实例；Wayland 仍须原生窗口身份 | 活动 tab 的窗口标题 / AXWebArea 名称可能带后缀；可能折叠、滞后 | 最容易做可见性实验，但可读可转发，**不得用于敌对页面准入**。浏览器品牌后缀不能靠模糊移除。 |
| C2：临时元素的 `aria-label` | 只有进入实际暴露的树且角色支持命名才可能出现 | 同左；隐藏/离屏/裁剪/inert 元素可能没有节点 | 同左；完整 AX 树未激活时不可依赖 | 不改窗口标题，但标记仍在页面 DOM；`display:none`、`aria-hidden` 不能作为可靠载体。只作反例/兼容性实验。 |
| C3：隐藏 canvas 像素 | AX 通常不表示像素值；有 canvas 节点也读不出 nonce | 同左，X11 和 Wayland 均无此 AX 保证 | 同左 | **不选**。加 accessible name 退化为 C2；改用截图/OCR 是另一方案，不能借 L0 截图偷换 AX 证明。 |
| C4：按 tab 的扩展 action 标题/tooltip | 工具栏按钮 Name/HelpText 是否实时、完整暴露待验；不能假定 hover 前可读 | Chrome/Edge 工具栏的 AT-SPI name/description 是否携带完整值待验；Wayland 不豁免窗口来源 | 工具栏按钮 title/help 是否暴露待验；全屏可能隐藏工具栏 | 优先建议的验证对象。必须是固定扩展自己的原生工具栏控件，位于页面 WebArea 外；仅 tooltip 浮窗命中不算绑定。 |

| 载体 | 页面读取、篡改与伪造 | 用户可见性、页面副作用 | SPA、历史/书签/同步与隐私 |
|---|---|---|---|
| C1 | 页面能同步读 `document.title`、观察 `<title>`、删除/替换后缀，通过同源引用、`postMessage` 或网络转发给 B；ISOLATED 无法阻止。短 TTL、多 nonce、MAC 后缀均不能阻止复制整段。 | 标签、任务切换器、读屏可能读出随机串；可能改变 WebArea 名称并使 fingerprint 变化。不得通过键盘改标题。 | SPA 的 MutationObserver/标题管理器可能竞争、循环恢复或把临时标题写入路由状态；history 不因赋 title 必然增加一条，但历史标题、最近关闭页、会话恢复、同步、收藏时的默认书签名及分析脚本都可能捕获。不能承诺删除已捕获副本。 |
| C2 | 页面能查 DOM/属性并转发，也能移除宿主；closed shadow root、随机属性名不构成可信浏览器 chrome。 | 可靠进入 AX 常需有效角色/可见几何；可能增加布局、焦点/读屏噪音、CSS 匹配与页面 observer 副作用；不得使用 live region 或抢焦点。 | 不直接改 title/history，但 DOM 分析脚本可收集；SPA 可能重建节点。清理必须按原节点引用，不按页面可伪造的 selector 批量删除。 |
| C3 | 页面可删除/覆盖/遮挡 canvas，知道引用时可读取像素；不能把隔离世界/闭合根假定为秘密边界。 | 真隐藏时无法用 AX 读取；可见图案会污染页面与截图、受缩放/色彩/遮挡影响。 | 不直接改书签标题，但 canvas/DOM 采集脚本可能记录；无可接受的 AX 证据，承担副作用没有收益。 |
| C4 | 普通页面没有读取/设置此扩展 action title 的 API；nonce 留在 background/native 私有通道，不下发 content script 或 DOM。页面能在 WebArea 画仿冒工具栏，必须依原生树边界拒绝；若无法可靠区分此边界，C4 也拒绝。 | 工具栏提示/读屏名称短暂改变；保留本地化的“停止共享”语义，点击仍撤销，badge 不当长 nonce 载体。要求用户自行固定图标，不自动展开菜单、hover、切 tab 或激活窗口来使其可读。 | 不修改页面 title、SPA、history 或书签名，预期减少页面分析泄露；浏览器内部诊断/扩展 UI 缓存与崩溃材料仍可能保留。不能声称浏览器一定不记录/同步，须真机核实并披露。 |

Chrome 和 Edge 分别测，不能把 Chromium 同源实现当作等价证据。工具栏布局、侧边/竖向标签页、版本、语言、缩放和品牌后缀都会改变树；只接受完整 nonce 与已核验载体结构，禁止依中文/英文名称猜控件。`action.setTitle` 的 tab 作用域可参考 [Chrome action API](https://developer.chrome.com/docs/extensions/reference/api/action#method-setTitle)，但这是 API 语义参考，未证明目标平台的 AX 映射或当前 Edge 行为。

### 3. 授权、身份与组合校验

绑定器在实际持有 Driver/Router 的可信 gcu 进程内：路线 A 是 CLI/helper，路线 B 是 engine；native-host 只转接。不得在 App/Tauri 或扩展再造 Router。绑定前的控制流程也须受 Router 的 App 读取授权、Stop、拒绝名单、生命周期和窗口锁约束；写临时标记另外需要明确披露/用户同意，不能把普通 observe 偷偷升级为页面编辑。

服务端私有记录绑定：引擎实例、连接 ID/epoch、唯一 local session、授权/策略版本、扩展固定 ID、浏览器 OS 进程实例及映像身份、share/tab/browser-window ID、top `frameId=0`/浏览器 `documentId`、原生窗口生命期、一次挑战 ID、单调过期时间。页面和模型不选择这些字段。每次挑战由 gcu 的 CSPRNG 生成至少 256 bit 独立随机 nonce；只在私有表中关联身份，不在可见串编码 session/epoch/share/document/URL/用户名，nonce 不复用，也不是可调用任何工具的 bearer。可见部分使用固定分隔与 64 个 ASCII 十六进制字符等有界编码，核对完整 nonce，不以截断前缀、大小写/Unicode 模糊归一化或任意子串命中代替；具体编码随内部控制 schema 一并评审。

扩展用浏览器提供的注入结果、documentId 定向 port 和生命周期事件核对 top document（见 [`content.ts`](../../tools/browser-extension/content.ts)、[`native.ts`](../../tools/browser-extension/native.ts)）；URL 字符串和随机 shareId 本身不代替 documentId。预绑定标记操作是**拟议的窄内部控制消息**，不走尚未绑定的 `Connection::request`，也不能假装成已允许的 `observe/click`。未来须版本化、严格 schema、单次服务端授权/pending 归属、长度和取消约束；仅提供安装/回读状态/清除固定载体，不能带 selector、脚本、任意属性名或任意文本执行功能。本文不冻结新 wire schema，不改现有 DomChannel/Driver/Host/ACP 接口。

候选校验必须全做，任一未知即 `capability_unavailable` / stale / handoff，不能按分数择优：

| 校验 | 必须成立的事实与限制 |
|---|---|
| 进程与候选集合 | 经 A4a 启动链/预期构建认证后，用已有 L0 `list_windows`、`snapshot_tree` 等枚举该浏览器实例的全部候选；核验 OS 实例/映像，不仅 exe 名称或 PID。Renderer 与 browser 父进程的可信归属也须可证，不能以所有子 PID 都算目标。只读已获授权 App 所需元数据，不读其他 tab 的 DOM；枚举/树不完整、超时、不能确认归属均失败。 |
| 窗口与标记 | 设置前记录基线，确认 nonce 原先不存在；设置后至少三次独立新读取，首末跨度至少 200ms，覆盖完整候选集且全都只命中同一个活窗口、同一个受信载体。期间新增/关闭/转移窗口、任何多命中、旧缓存、截断或不一致均失败。C4 只接收页面 WebArea 外的固定扩展原生控件；不是在整棵树中搜索任意含串节点。200ms 是稳定性检查，**不是防转发/TOCTOU 的安全时间窗**。 |
| 活动 tab 与文档 | 浏览器 API 提供已共享 tab 仍在同一 browser-window、为该窗口选中 tab、同一 documentId、URL/生命周期版本；不必是桌面全局前台窗口，但不能是窗口内后台 tab。只读取 activeTab 已授权的共享文档元数据；若目标版本无现有权限可得的必要字段，拒绝，不读取别的 tab URL/title。 |
| URL、标题、原生 chrome | 比较浏览器报告的共享文档标题/已提交 URL 与该 OS 窗口的真实浏览器 chrome/文档根数据；地址栏编辑中、URL 省略或仅 origin、无法证明是已提交 URL 时失败。URL 以解析后的完整地址（包括 path/query/fragment）比较，仅采用已有解析器规定的等价形式，不模糊去前缀/解码或只比域名；标题只允许已核验的品牌装饰，不做相似度匹配。它们可被攻击者复制，**只用于拒绝不一致**。Windows UIA 树当前 `url: None`（[`tree.rs`](../../crates/gcu-driver-windows/src/uia/tree.rs)）；不能填入扩展自报 URL 冒充独立证据，已有原生地址控件也不足时该平台失败。 |
| frame/fingerprint | 在身份证明之外，复用 P8-03 的已清理 L0 WebArea fingerprint、原生句柄/祖先归属、frame、viewport/DPR/物理像素映射及快照版本；尺寸/原点按现有 `FRAME_QUANTUM_PX` 门禁，不放宽容差。DOM 不能自己报告“和 AX 同 fingerprint”来认证。C1 临时改名后的 fingerprint 不能作为最终基线，必须清除后重取；不得特殊忽略 title 的任意变化。 |
| 原子发布 | 清除/恢复与最后校验成功前，Connection/Hub 都不绑定，也无 DOM observation/perform。之后在同一串行事务发布两侧绑定；任一失败先撤销两侧和 pending，绝不出现一侧先可投递。现有两次 bind 调用本身不是跨线程原子保证，适配器必须持有锁和关闭的准入闸直到两者完成。 |

清除后“绑定”只是可撤销的私有记录，不是下一次请求的许可。每次 DOM 观察和变更（含 batch/script 的每步）都在 Router 授权/必要的敏感确认后、最终投递前重验相同身份与版本，并使用**新的**挑战做完整唯一匹配、稳定采样和清除验证，不能依赖已消失的旧标记。绑定时的事件订阅不替代主动回读；观察同样不能把 A 的内容错标为 B。

title、URL（含 SPA/history/fragment）、documentId、tab 激活/移动/替换、窗口生命期/geometry、策略、授权、连接或 epoch 变化立刻废弃旧绑定、Hub active/handles 和待用许可；自有标记安装/清除只允许精确预期的单次变化，基线 title 的任何其他变化仍作废，不自动重新绑定。导航/撤销/Stop 等沿 ADR 0012 清 share；单纯失配至少关闭 L2，重新获得 L2 必须用户重新共享，Stop 仍只由可信用户恢复。

### 4. AX 前置条件与窗口边界

Chrome 的无障碍树可能懒激活，OS 有辅助功能权限不等于浏览器已提供完整树。macOS 复用已有 [`AccessibilityToggles`](../../crates/gcu-driver-macos/src/ax/toggles.rs) 对 `AXEnhancedUserInterface/AXManualAccessibility` 的启用与 turn/session 恢复；缺 TCC 不自动点授权。Windows 必须以生产 Worker/UIA 查询证实所需浏览器 chrome 和 Document 节点确实可读，不能假设 UIA 连接成功就激活完毕；Linux 必须 AT-SPI 总线/浏览器 provider 完整、窗口归属可证。不能改用户启动参数、强开浏览器全局 accessibility 配置或新增权限来掩盖不支持。首次激活不及预算就失败。

| 边界 | 必须处理的方式 / 真实夹具要求 |
|---|---|
| 多窗口、多 tab、同 URL/标题/内容 | 每个 OS 窗口分别枚举并确认其选中 tab；一个窗口的标题/WebArea 通常只代表当前 tab，不能从标签条里某个后台 tab 名称推断其为当前 web area。后台共享 tab 不自动切前台或自动切 tab：拒绝，用户选中后重新共享。两个相同站点/同进程/同尺寸窗口也不合并。 |
| 窗口内切 tab、拖出/合并窗口、跨 profile | 激活/移动/关闭/替换事件与主动校验均使旧绑定失效；同一 OS 窗口不意味着同一文档。不得跨 native 连接/profile 搬 share；扩展 tabId/browser-window ID 只在原浏览器连接语境使用。 |
| popup、独立 PWA、DevTools、画中画 | 作为可能新增/干扰的窗口计入集合，不能当作原 web area 或工具栏证明。首版只接受有可证普通 chrome/选中 tab 的标准浏览器窗口；popup/PWA/画中画无所需载体即拒绝，弹窗不得继承授权/绑定。未知执行界面 handoff。 |
| 最小化、全屏、隐藏工具栏、遮挡、其他桌面 | 最小化/隐藏/无法完整读取时拒绝，不恢复/激活；全屏或 C4 图标未固定导致载体缺失同样失败。遮挡只在已有 L0 能完整读树且来源/几何可信时可验；不得用窗口位置、焦点、唯一可见窗口或截图相似性补证。 |
| X11、XWayland、native Wayland | XID 与 AT-SPI 原生窗口须可证一致；Wayland 的 AT-SPI frame 近似、标题或 portal restore token 都不是精确 toplevel 身份。若现有驱动无法将读树、策略窗口和输入/截图来源绑定到同一原生对象，直接禁用对应 L2，不能借 GNOME/KDE 全桌面或 XTEST 绕过。 |
| iframe、shadow、canvas 页面 | 标记只属于共享 top document，不给子 frame/new document 创建 share；DOM 融合仍按 P8-03 的 frame/shadow 能力限制。C4 非页面载体不意味着可忽略不透明 frame、incomplete 或无法定位敏感区。 |

原生候选枚举、标记读取、URL 或 chrome 边界任一不可靠即退回已声明可用的 L0；L0 仍要同一 App 授权、Stop、前台/敏感/截图门禁。不能猜测，也不能把被拒 DOM 变更自动换成 L0 动作。

### 5. 威胁分析：逐项缓解与测试要求

攻击者可控制共享页面 A 和协作窗口 B，包括同源、同 URL、相同渲染内容；可经 `postMessage`、同源引用或网络串通。目标违规是**策略/敏感确认依据窗口 B，DOM 却在 A 执行**（或观察内容来自 A 却标为 B）。测试必须用独立 oracle 记录实际文档/原生窗口/动作计数及策略依据，不能只断言“返回 stale”。下列是后续测试要求，均非已有通过证据。

| 威胁 | 必须的缓解与剩余限制 | 必须测试的断言 |
|---|---|---|
| T01 提前猜测、重放、跨会话借用 nonce | CSPRNG ≥256 bit、逐次不复用、短 TTL、服务端全身份记录、预期响应一次消费；nonce 不是权限。旧 epoch/share/document 的 set/read/clear/响应均不能发布或撤销新一代记录。 | `challenge_replay_or_foreign_scope_zero_bind`：固定随机源覆盖碰撞拒绝、旧连接/epoch/session/document/请求重放，两侧 bind 计数与 DOM 读写均为零。 |
| T02 **A 读 title 后转发 B，A 清除自身，只剩 B 命中** | C1/C2/C3 不作可信证明；唯一性只能防“同时有两份”，不能证明“唯一一份的来源”。页面载体实验也必须在扩展侧锁存任何非预期变更、回读原文档标记状态，不能只依 AX 采样；清除被发现即拒绝。C4 必须不下发 nonce 到页面，匹配浏览器真实工具栏内固定扩展控件；网页 WebArea 内的伪串一律无权。 | `page_relays_marker_unique_wrong_window_zero_bind`：A 的 MutationObserver 读串转发 B，再清 A；同源/跨源 postMessage/夹具网络转发、A 后台窗口/B 前台窗口（各自选中 tab）、A 为后台 tab 的拒绝路径均覆盖。即使稳定唯一命中 B，也必须拒绝，策略 B/动作 A 不得发生。C4 再测页面无法读到 nonce、伪工具栏无权。 |
| T03 **同 URL/标题/进程/frame/fingerprint 的克隆 B** | 这些校验只能缩小候选，不能补 T02 的来源；同源克隆不需破坏哈希即可全部匹配。只有可信浏览器 chrome 载体/独立文档归属证据才可继续；不可按打分或“全部字段都像”准入。 | `identical_clone_all_correlations_still_unproven`：两个同浏览器实例、同 URL/标题/尺寸/树的夹具；实测确认组合条件可同时成立，页面载体方案依然零绑定、零融合、零投递。恶意页面只在 A 显示/启用执行界面，防按 B 的安全内容漏审。 |
| T04 **采样/清除后切 tab、移动窗口、导航的最终投递竞态** | 持有会话串行/窗口变更锁，监听变化并维护单调代次，原生与扩展两端在审批后重验；精确 tab/documentId port、一次 permit、扩展实际 dispatch 前重读 active/window/document 事实，变化拒绝。**锁只约束 gcu，不锁浏览器/用户；事件可迟到，三次采样或任意短 TTL 不证明无竞态。** 若无法证明最后一次 OS 校验到 DOM 副作用之间同一窗口/文档，保持 L2 关闭（D2 待裁定），不能用事后撤销补救已经发生的错投。 | `switch_after_final_sample_never_dispatches_mismatched_document`：在每个 await、最后校验与发消息之间、清除/两次 bind 之间、扩展读元数据与同步动作之间注入切 tab/移动/导航/焦点与迟到事件；独立 oracle 不能出现 policy-window≠action-document。若能复现一次即失败/退出，不以返回 unknown 作为安全通过。 |
| T05 页面反复清除/覆写、SPA 标题竞争、伪恢复 | 预期状态之外任意 title/DOM 改动使挑战失效，不重新设置、不与网页争写；恢复用原载体所有权 + compare-and-swap，只清自己的精确后缀/节点，不能盖回旧标题覆盖用户/SPA 的新值。C1 测试不得把“已没 nonce”当成“恢复了原状态”。 | `title_race_preserves_foreign_edit_l2_closed`：立即/延迟/重复改 title、移除重建 title、用户 rename/书签编辑、SPA observer 循环；保留新编辑、报告恢复冲突，两侧不绑定。 |
| T06 模糊/截断/旧缓存、多候选或枚举不全 | 完整固定编码精确匹配，独立新采样、基线不存在、唯一候选；浏览器 chrome 与 WebArea 结构不明、RawTree incomplete、未知窗口/进程归属均拒绝。 | `truncated_cached_or_incomplete_marker_zero_bind`：长标题、RTL/同形字、半串、旧 nonce、Name/HelpText 更新滞后、第二窗口读失败/新建/不可见、辅助树未激活，不能因只读到一个结果而绑定。 |
| T07 共享/标题被当成授权，模型绕 Router 或恢复 Stop | Q4 唯一已授权 local session、预绑定窄许可、最终完整 Router、敏感确认绑定同一窗口/参数/快照/策略；无页面/runtime 消息入口，无新 MCP/HTTP/Host/ACP/恢复能力。 | `marker_cannot_grant_route_or_resume`：未共享、未授权、多引擎/多 session、SSH/IM/定时/official-aux、拒绝名单、地址栏/危险 URL/元数据、所有 batch/script/坐标路径均零越权；Stop 后新旧消息不能恢复。 |
| T08 启动链/进程名冒充、PID/窗口句柄复用 | 固定 key/origin 之外保留真实 OS 实例/映像/构建核验；原生窗口生命期与浏览器连接代次绑定，不以标题、HWND 或同 UID 证明身份。现有 A4a 未实现的平台核验仍是前置阻塞。 | `relay_or_reused_native_identity_zero_bind`：同名假浏览器/转接进程、错构建/父实例/扩展、旧 PID/HWND/XID/AX 引用、跨 profile native 连接均失败。 |
| T09 Stop/取消/断线后迟到安装、清除或 DOM 响应 | 同步先关闭准入，清双侧绑定与 pending，随后独立清理；安装消息可能已入队时也要有过期检查及扩展清理，不只在 gcu drop future。扩展清理不具有观察/动作/恢复权限。已开始 DOM 变更结果未知，永不自动回放。 | `cancel_at_every_marker_phase_clears_without_revival`：Stop/turn end/撤销/会话关闭/fork/切模型/引擎换代/worker 停止在每个阶段触发；迟到回执不能入库或改变新标记，输入释放满足既有时限，A/B 各动作最多一次。 |
| T10 清除失败或 worker 被挂起，标记长期残留 | 见 §6：局部独立清理与超时、清除回执 + 原生恢复验证；失败报告且 L2 持续关闭，不把 drop/超时当清除成功。不为清理改其他 tab/页面。 | `cleanup_failure_keeps_l2_unavailable`：写成功但 ack 丢失、清除 IPC 失败、AX 缓存不刷新、worker 挂起/重启/浏览器崩溃；无残留时需实际验证，未知时正确报告、不自动重绑。 |
| T11 nonce/原始标题/URL/DOM 泄露 | 最短寿命、无用户数据、metadata-only 日志、challenge 期间的读树不交给模型/图片/持久快照；只报错误枚举/阶段/耗时。页面载体的脚本与历史捕获风险不能靠日志 scrub 消除。 | `challenge_payloads_never_reach_outputs`：检查成功/失败/超时/崩溃脱敏路径及模型/缩略图/审批 crop/CLI 临时文件，不含 nonce/内部身份/未脱敏原始数据；C1 分析脚本/书签夹具能捕获的副本如实记录为风险。 |

T02/T03 是“只做 AX 命中与内容相关性校验”的反例：A 在后台窗口的选中 tab 保留 DOM 连接，将 challenge 转给同源前台窗口 B，B 复制 A 的标题/URL/布局，A 去掉标记。可同时满足“一个 OS 窗口带标记、同进程、URL/标题匹配、连续稳定、frame/fingerprint 对齐”。扩展 documentId 能证明动作落在 A，AX 却只证明这些内容出现在 B；两者没有共同的不可伪造身份。再做 HMAC、双 nonce、随机改标题顺序也可实时转发。**因此这些组合是必要的失配检测，不能独立证明安全；必须拒绝这个构造。**

扩展侧原文档标记回读/变更锁存可以挡住上述简单清除攻击；若 A 也一直保留标记且原生枚举完整实时，复制会产生两个命中而被拒绝。不能把这条有效缓解省略，也不能反过来声称它已经解决跨域映射：需要继续证明 browser AX 更新/缓存、active-tab 元数据、清除恢复和最终投递之间的一致时序；有限次回读没有使这些异步事实原子化。本提案因该证明缺口和页面可篡改性拒绝 C1–C3 的生产准入，不声称已证明所有可能的挑战设计都不可行。

C4 的安全收益限于阻断页面获知/搬运 nonce：只在真实浏览器 chrome 接收固定扩展的按-tab 状态，不接受网页副本。如果原生树无法提供这样的可信边界、action 状态意外跨 tab/window 复用、或最后投递时仍能错配，则收益不成立。不能仅因恶意夹具暂时未成功就宣称不可转发或不存在竞态。

### 6. 时序、预算、取消与恢复

正常路径：`未绑定 → Router 授权/Q4 选定 → 原生基线 → 设置 → 稳定唯一读取/交叉校验 → 清除 → 双端恢复校验 → 最终重验 → 同时发布两侧绑定`。任一步失败都进入关闭/清理终态；清除验证在绑定前，挑战期间无 DOM 融合/动作。每次后续 DOM 请求用新 nonce 重走受限验证，原敏感确认若绑定事实变化则作废重审。

| 阶段 | 硬上限与行为 |
|---|---|
| 设置、读取、清除 | **各自最多 5s，且只使用总剩余预算**；包括发帧、ack、实际原生读取，不能分别给每个候选/每次采样重开 5s。设置超时视为可能已写，必须进入清理。 |
| 正常单次绑定/重验事务 | 从第一次原生基线/标记操作开始，到清除恢复与最终校验，**总预算 ≤5s**；实现遵循现有 `DRIVER_DEADLINE=4750ms` 的工作预算，保留外层余量（[ADR 0012 期限](0012-real-driver-assembly.md#stop审批与期限不变)、[`ActionRouter::native`](../../crates/gcu-core/src/action/router.rs)）。至少 200ms 的三采样包含其中；不得把三阶段串成 15s。预热/审批不能重置一个已开始事务的时钟。 |
| 每动作验证与投递 | 挑战验证不是扩大动作原生等待的许可。最终验证和 DOM 投递共享该次原生调用原有 5s 上限，清理也计入正常预算；若赶不上就拒绝，不能悄悄拆出额外无界等待。窗口锁沿用最多 5s，等待锁时未设置标记；用户审批的独立 120s 内不保留标记。 |
| 故障清理 | 失败/取消立即先封 L2、撤 permit、撤双侧绑定，再用独立 cleanup scope 最多 5s 做幂等清除/恢复核实；这是已拒绝事务的清理上限，**不延长动作或恢复投递**。清理未确认到时必须报告失败；不阻塞 Stop 锁存/输入释放或会话权限撤销。 |

从事务起点计，正常验证/投递最迟 5s 终止；若走故障清理，连同该独立清理最多 10s 内必须终结并报告“已恢复”或“恢复未确认”。这不把 nonce 的正常 5s TTL 延长到 10s，也不承诺挂起的浏览器已实际清除；未知残留按失败处理，不能无限后台重试。

Stop ≤100ms 锁存、≤500ms 发输入释放及原生阻塞 ≤5s/杀 worker 不变。Stop、取消、drop、超时、导航、任意输入/标题编辑冲突、共享撤销、content/native 断线、turn 授权结束、session close/fork/切模型、引擎退出/换代都必须触发同一清理路径；不能等待普通 Router 队列。取消后只允许清自己的标记，不允许借“清理”继续 observe/perform、解除 Stop 或重新获取共享。

扩展必须在**写入前**准备独立过期清理，并在私有 port 断线、pagehide/卸载、撤销与 worker 启动时回收自有标记。迟到的安装请求先验 deadline/代次，已取消不得再写；旧 clear 必须只作用于旧 nonce/原文档，不清新挑战。对于 C4，现有源码已说明 tab action title/badge 可跨 worker 存活，单靠 worker 内 `setTimeout` **不保证挂起/被杀后 5s 内恢复**。不新增 `alarms/storage/offscreen` 权限或常驻 keepalive 规避此缺口；必须由 M0/M4 核实可兑现的生命周期语义，不能兑现则关闭该路线并提交 D3/退出。

清理分两部分验证：扩展侧原载体恢复/已销毁，原生侧同窗新读取确认标记消失且标题/tooltip 恢复到预期基线；仅收到 `clearAck` 不够。C1 只在 live document 仍是原对象且完整标题仍等于自己设置值时 CAS 恢复原始标题；若用户/SPA 已改名，只移除仍可确认为自有的精确后缀且不覆盖新内容，报告冲突、L2 关闭，不伪称恢复基线。原文档已销毁则不写入新文档；无法独立确认销毁/残留状态仍是未确认。C4 恢复原本本地化 tooltip；共享撤销时恢复未共享状态，不错误地恢复“共享中”。

挑战前检查原生地址/标题/书签编辑、页面 `contenteditable`/重命名和用户输入活动；有输入或焦点事实不明时返回 `user_active`/handoff，不执行 C1 页面编辑或 C4 提示变更。期间不夺焦点、不发送快捷键、不改 URL/history、不暂停/覆盖页面标题监听器，不循环抢写。C4 也须与现有共享指示器串行维护，否则 `mark()` 可覆盖随机串，视为失配而非忽略。

清除失败通过现有错误/诊断展示通道报告阶段和元数据（不含 nonce/原始标题），相关连接/share 的 L2 保持关闭，直到恢复核实及用户重新共享；故障本身不授予恢复权限。浏览器/OS 崩溃或挂起时不能声称已完成物理清理：立即撤所有能力、标记为未确认，恢复后先清理且不自动续 share。已复制到网页/历史/外部脚本的 C1/C2 标记不能被召回；不能为了清除副本写入未共享 tab。

### 7. 隐私、披露与权限

nonce 只含随机数/固定分隔，不含任何用户数据或可跨会话关联的稳定值；正常存活不超过单事务 5s，越短越好。内存记录按连接/会话隔离，清理后释放，不入 argv/env/文件/普通日志/模型结果/崩溃载荷；基线标题/URL 只在完成核验的最短时间内保留，不为证明建立浏览历史。共享前、Router 授权前零页面读取/写入，扩展不遍历其他 tab 的内容；候选原生读树受既有 App 读取授权限制，挑战树不交付用户/模型，也不绕过原生不完整树拒图。

随机并不等于隐私无风险：C1/C2 可被页面分析脚本即时关联到访问者，并被转发；临时 title 可能进入历史/同步/会话恢复/书签，恢复 title 不能删除这些副本。C4 预期避开网页脚本与页面历史，但仍可能被读屏、OS 工具栏缓存、截图或浏览器诊断捕获；是否同步与缓存寿命均未确认。若无法兑现当前商店材料的“仅内存/不写持久浏览数据”承诺，须在实施前更新披露或放弃相应载体，不能假称原披露已覆盖。

接受并验证方案后，后续文档卡必须更新 [browser-extension.md](../browser-extension.md)、[permissions.md](../../tools/browser-extension/store/permissions.md)、[privacy-policy.md](../../tools/browser-extension/store/privacy-policy.md)、[data-use.md](../../tools/browser-extension/store/data-use.md) 以及两商店材料：说明短期标记、载体/可见副作用、AX 读取范围、页脚本可见性、残留与清理失败、L0 降级和 L2 实际可用平台。用户同意不能只有现有共享圆点，临时写 title/工具栏名称的行为必须提前可知；后续用户文案按既有 15 语言要求。

本提案**不增加 manifest 权限**，不读 cookie/页面或扩展存储，不任意 eval。C4 使用现有 action API 不要求新增 `tabs` 权限；必要 tab/document 元数据仅在 `activeTab` 范围核实。`tabs` 当前也**未申请**，不能当作已批准基线；若目标版本/载体需要增加 `tabs`、额外 host 权限、`debugger`、`history`、`storage`、`alarms`、`offscreen` 或任何现有四项以外权限，必须停止相关实现、单独审批/ADR。本 ADR 不预先批准“为了可靠性”扩大权限，也不提供截图、浏览器调试协议或 HTTP 后门。

## 备选与为什么不选

| 备选 | 不选或保留的原因 |
|---|---|
| 直接采用 title 后缀，叠加 URL/进程/fingerprint/稳定采样 | T02/T03 可完全克隆并转发，出现策略 B/动作 A；这些检查均不能证明 challenge 最初写入的文档所在窗口。C1 仅保留作反例和原生可见性实验。 |
| 随机 aria-label、closed shadow DOM 或隐藏 canvas 取代 title | 同样属于页面影响范围，或根本无法经 AX 读到；缺乏可证明的非转发属性。不得借隐藏减少用户注意而降低安全/披露。 |
| 多轮 nonce/HMAC、短到页面“来不及复制”、只支持一个可见窗口 | 恶意脚本可实时复制整个串；无法给网页调度/网络设可靠下界；后台 tab、弹窗和枚举遗漏仍在。熵/采样不构成来源证明。 |
| C4 扩展原生工具栏载体 | 保留为**推荐验证方向，待 D1 裁定**；避免页面读写，但依赖真实 chrome AX、按-tab 状态、可兑现的清理与最终投递语义，不能自动替代 title 开始实现。 |
| 扩展创建通知/侧栏/独立 popup/可信新窗口作为载体 | 证明新建表面不等于证明原共享 tab 的 OS 窗口；增加 UI/生命周期，可能需要新权限。不能用 iframe/跨文档标签绕开这一点。 |
| 用 `--parent-window`、标题相似、URL/尺寸唯一、用户确认窗口 | A4a 已拒绝前几项；一次用户确认也不能消除其后错配，不在 Q4 内新增选择 UI 或 Host/ACP 字段。若考虑新的可信映射需独立 ADR。 |
| 请求 debugger/CDP、打包浏览器，或降低威胁模型为“可信网站” | 前者扩大权限/架构与部署边界，后者排除了本任务必须覆盖的攻击者；均不在已批准探索范围。 |
| 保持 L0 | 是默认与失败终态；不重放未确定 DOM 变更，不以“实验”名义免除安全门禁。 |

## 影响（对接口、测试、用户）

### 与 Q4、A4a/A4b 和架构的关系

Q4 保持“恰好一个监听引擎、该 epoch 恰好一个已授权 local session”；标记只证明候选窗口，不解决选择哪个聊天。多引擎/多会话、session 目标窗口不一致均留在 L0；不能用标记/窗口匹配分数打破歧义，不为 SSH、IM、定时任务或 official-aux 注入。

A4a 的身份、启动链、固定扩展 key/origin、旧帧/错 scope 拒绝仍是前置条件；本 ADR 不替换其失败关闭，也不修改其证据表。A4b 还依赖该平台真实驱动装配、完整 Stop/Router、生产 DomChannel 往返与生命周期；增加挑战不会使“共享成功”直接意味着 A4b 已完成。T02/T03/T04、清理与真机证据未解决前，生产连接继续不准入。

只允许在 CLI 私有 broker/扩展内部通道扩展经审查的控制语义；Driver、DomChannel、gcu-protocol、Host、ACP 方法/字段保持不变。core 不增加 OS IO，App 不链接 core/driver，不增加模型工具/自动恢复入口。若现有原生数据无法证明浏览器 chrome 边界/URL/窗口身份，或实现必须改这些接口，应停止相关卡并另立 ADR；不能复用 `host.event.emit`/策略通知暗传安全身份。

### 分阶段实现卡建议（批准后再登记）

下表是后续建议，未新增路线图/STATUS 卡。每卡完整覆盖自身错误/取消/清除路径，生产功能保持关闭；“之后再写清理/测试”不满足单卡 DoD。新测试名和新测试 target 是建议，不代表当前存在或运行过。

| 建议卡 | 依赖 / 文件范围 | DoD | 必须真机、默认 `not_run` |
|---|---|---|---|
| M0 载体与时序可行性/反例 | 本 ADR 的 D1–D3 裁定；A4a 相关身份前置；`computer-use/docs/` 与隔离夹具 | 对 C1/C2 给出可复现 T02/T03 反例；选定方案有可审的非转发论证、chrome 原生边界、完整 URL 来源、dispatch 时序与清理证明，明确每平台可用/拒绝条件。若只剩统计可靠性就结论拒绝，不打开映射开关；无需改接口/权限才可进入下一卡。 | 每个平台 Chrome/Edge 的载体完整读取、按-tab 隔离、懒激活、worker 休眠清理、T04 最终竞态。任一缺失不是通过。 |
| M1 扩展侧窄标记控制 | M0；`tools/browser-extension/{background,native,content,shared}.ts` 与 tests（按选定载体取最小范围） | 版本化固定控制 schema、私有 Router 许可/连接/epoch/share/document/pending 归属、CSPRNG nonce 不进页面（C4）、原子安装/读状态/幂等清除、TTL/断线/重启拒绝旧请求；写入前已具独立清理，不能仅交 set API。manifest 不增权、无页面桥/任意 eval。 | 真实 worker 挂起/重启、tab action 状态与标准工具栏/未固定/全屏；Chrome/Edge 分别验。 |
| M2 gcu AX 枚举与匹配 | M0、对应 A1/A2/A3/A0-M；`crates/gcu-cli/src/` 私有适配与 tests，必要已有驱动内部模块 | 只经现有 Driver 能力；进程/窗口生命期、完整候选集合、可信 chrome 位置、精确匹配、三次新采样、URL/标题一致、统一剩余预算；0/多/不全/不可读均拒绝；不改 Driver trait/RawNode 数据接口。模拟测试不得替代平台结论。 | UIA/AT-SPI/macOS AX 的原生窗口来源、完整 URL、tooltip/Name/HelpText 更新和权限/懒激活；Wayland 精确 toplevel 失败必须禁用。 |
| M3 绑定器、重验与 Router 接线 | M1、M2、M0 时序安全验收、A4a；`gcu-cli` broker、`gcu-mcp` 注入/生命周期适配、`gcu-core/tests/dom_router.rs` | 预绑定授权/Q4 唯一选择；清除恢复前两侧都未绑定；事务发布/失败双侧撤销；每次观察/动作/批量每步最终重验、新挑战/一次 permit、敏感确认失效重审；T04 oracle 零错投。保留原有 full Router 与 unknown 不重放，未获平台验收时拒绝。 | 真实扩展→native-host→helper/CLI/engine→Router 的政策窗口/动作文档 oracle、切 tab/拖窗/导航；A4b 只能在完整链验收后准入。 |
| M4 清除、取消与失败处置加固 | M1/M2 的已完整局部清理、M3 关闭准入路径；同目录与生命周期 tests、诊断/披露 | 所有取消边界先撤权限，独立清理不排 Stop；CAS 不覆盖用户/SPA；设置 ack 丢失/迟到 set/clear、worker 死亡/恢复、浏览器崩溃均正确处理；清除失败可报告且持续禁 L2；无强 keepalive/新权限，满足总预算。不能以“best effort”宣称清理成功。 | 物理 Stop 时限、挂起/进程退出后的实际残留与恢复、原生 AX 滞后；缺点未解决不得发行。 |
| M5 恶意页面与确定性测试夹具 | M0 测试设计先行，完整验收依赖 M1–M4；`tools/browser-extension/tests/`、`gcu-cli/tests/`、`gcu-core/tests/`、隔离 `eval/` 夹具 | T01–T11 每行至少一个命名用例和 oracle；正常/恶意双窗口/同窗多 tab/克隆/延迟/失效树/取消竞态覆盖；断言 bind/observe/action 与实际文档和策略窗口，正负 oracle 自证；禁止动作零副作用。只用本地夹具/一次性 profile，无真实账号。 | 原生浏览器驱动同一恶意夹具，尤其 title 转发和最终切 tab 竞态；fake/Xvfb 不等于真实桌面通过。 |
| M6 真机准入与用户/商店披露 | M0–M5、对应平台 A4b、07 §7/§8；`docs/browser-extension.md`、`tools/browser-extension/store/`、既有 i18n/发行检查 | 下表逐组合证据、系统/浏览器/构建版本、测试名/命令/CI 或录屏/独立 oracle；权限清单与包一致，全部副作用与未确认事项披露；未测或失败平台维持关闭，不外推支持；满足网页 red_team 零违规和现有发布门槛才可提出启用。 | 所有宣称支持的真实交互桌面、Chrome/Edge 单独通过；缺 runner 标 not_run，由维护者决定资源。 |

M5 的攻击测试应在实现前定义并持续用于 M1–M4，不能等接线完成后才补安全断言。各卡只是组件闭环，不各自启用生产 L2；M6 完整验收前不得发布半成品。

未来验收命令（在相应工作目录，Rust 一次一个远程构建）：`cargo fmt --all -- --check`；`rcargo test -p gcu-cli --test native_host_proof`；`rcargo test -p gcu-cli --test native_host`；`rcargo test -p gcu-core --test dom_router`；`rcargo test -p gcu-core --test stop`；对应平台 `rcargo test -p gcu-driver-<platform> --test contract` 及必要 identity/portal 测试；`corepack pnpm exec vitest run computer-use/tools/browser-extension/tests/`；按改动的 `rcargo clippy -p <crate> --all-targets -- -D warnings`、`rcargo run -q -p xtask -- check-deps` / `golden --check` / `budget`、`rcargo --sh 'cargo deny check'`。新增夹具 target 随实现卡明确；远程普通 Linux 构建不代替交互原生 runner。

### 真机验证矩阵与证据边界

每个组合均需：完整启动链/映像身份 → App 授权 → 冷/热 AX 激活 → 完整标记/URL 与文档映射 → 唯一性和 T02/T03 克隆攻击 → T04 切 tab/拖窗/导航 → 权限撤销/Stop/断线/休眠清除 → L0 降级与 unknown 不重放。覆盖同源/跨源、同窗后台 tab、多窗同站、标准/竖向标签页、popup/画中画、最小化/全屏、不同缩放/语言和未固定扩展。正常及失败延迟均记录，不能以延长预算消除失败。

| 平台 | Chrome | Edge | 平台特别验证 |
|---|---|---|---|
| Windows 交互桌面 | not_run | not_run | 原生 UIA chrome 边界/完整 URL、Worker 健康、HWND/PID 复用、物理输入/Stop、普通/安全桌面拒绝。 |
| Linux X11 | not_run | not_run | AT-SPI 与 XID/实例对齐、真实树懒激活/缓存、物理输入/热键；Xvfb 结果单列不代替。 |
| Linux native Wayland | not_run | not_run | GNOME/KDE/wlroots 分开取证；原生 toplevel、AT-SPI、portal 来源/范围与 Stop；不把 XWayland 结果计入。 |
| macOS | not_run | not_run | helper/App TCC 责任链、AX 开关启用/恢复、toolbar 实际名字、原生窗口身份与物理 Stop；系统版本/Intel/arm64 分开。 |

本文件的事实证据来自源码和既有文档，未启动真实浏览器、编译/测试 Rust 或验证系统 AX。文档自查使用 `python3 docs/plans/computer-use-v2/reviews/check_plan.py`，另查本 ADR 相对链接（计划检查器只遍历 plans）；这些不能把上述任一 `not_run` 变为通过。

## 开放问题

以下 **NEEDS-DECISION** 项由维护者裁定，推荐不等于批准；当前只完成文档探索，不实施/绕过未决门禁。

| 编号 | NEEDS-DECISION：问题 | 选项与推荐 | 阻塞范围 / 所需证据 |
|---|---|---|---|
| D1 | 页面可转发载体无法认证窗口；是否允许验证 C4 原生扩展工具栏载体 | ①保持页面载体仅为负面夹具，并批准 C4 可行性验证（推荐）；②只保留本 ADR 的不可行结论、停止挑战路线，继续 L0；③另提浏览器侧可信文档/原生窗口桥 ADR。**不提供“接受恶意页面错配风险”选项。** | M0 方案选型和 M1–M3；须证实 C4 确实为按-tab、不可被页面读写、原生 chrome 内可完整读取的控件。 |
| D2 | 最终 OS 采样到 DOM 副作用之间缺少可证的窗口/文档一致性保证 | ①将可审时序证明及 T04 对抗 oracle 作为准入硬门禁，未解决保持 L2 关闭（推荐）；②终止本路线；③另立 ADR 请求浏览器/接口提供可信原子能力。不能仅增加采样/缩短 TTL，不能声称用户活动守卫锁住浏览器。 | M0/M3 与 A4b 的观察归属/动作安全；若现有边界无法保证，停止实现并另行决策，不改 Host/ACP。 |
| D3 | worker 挂起/崩溃时 C4 的恢复时限、用户可见副作用与隐私披露能否兑现 | ①先验证无新增权限且有独立清理的方案，不能兑现则放弃对应载体（推荐）；②直接保持 L0；③单独 ADR 评估其他生命周期机制/权限及新披露。不得把 5s 正常 TTL 写成崩溃后的已验证保证。 | M0/M1/M4；须确认 title/tooltip 缓存、同步/历史/书签影响、真实失败恢复与不覆盖用户编辑。C1 写页面标题若继续实验也须明确实验同意。 |
| D4 | 原生平台/浏览器版本和交互 runner 覆盖 | ①维护者选有真实设备、能证明全部前置的平台逐项验证，其余关闭并记 not_run（推荐）；②维护者另供 Windows/macOS/各 compositor 与 Chrome/Edge runner 后统一评审；③若任一平台缺口显示通用设计不成立，终止整体方案。 | M2/M5/M6；完整 URL/chrome 边界/精确窗口归属在现有 Driver 数据上是否可得仍未知。缺证书/账号/runner 时停止相关验收，不操作真实账号、不请求放宽安全。 |

### 裁定记录（2026-10-18，维护者）

| 编号 | 裁定 | 含义 |
|---|---|---|
| D1 | 选①：页面载体仅作负面夹具；批准 C4（扩展工具栏按 tab 的 action 标题）可行性验证 | M0 只验证 C4 与 C1/C2 的反例；不新增权限，不改 Host/ACP/Driver/DomChannel；验证不过则关闭本路线 |
| D2 | 选①：可审的时序证明 + T04 对抗 oracle 是准入硬门禁 | 证明缺失或 oracle 出现一次错投即保持 L2 关闭；不以加采样或缩短 TTL 代替 |
| D3 | 选①：只在能兑现独立清理且不加权限/keepalive 时使用 C4，否则放弃该载体 | M0/M1/M4 必须实测 worker 挂起后的恢复；兑现不了即退出条件 4 |
| D4 | 选①：由维护者挑选有真机的平台逐项验证，其余关闭并记 not_run | 具体平台与 Chrome/Edge 组合在 M0 开始前由总控提出清单，需要用户在物理桌面配合的步骤逐次通知 |

这些裁定不改变“实现卡须另行登记并经批准”的要求；M0 之外的卡（M1–M6）在 M0 结论出来前不启动。

### D3 修订（2026-10-19，维护者）

Windows × Chrome 的实测（[M0 记录](0014-m0-feasibility.md) §5.1）显示：扩展 worker 被停止后，工具栏按钮 Name 里的 nonce 至少保留 30 秒，
只在 worker 再次运行（启动重刷）后清除，浏览器不提供独立于 worker 的清理，因此 §6 的“worker 死后 5 秒内恢复”无法兑现。
维护者选择**修订 D3 判据后继续**，而不是放弃 C4：

- 原判据“能兑现 worker 死后独立清理才使用 C4”改为“**残留标记必须无害且可观测**”。
- 无害：服务端 nonce 单次有效，≤5s 作废；残留的旧串不能再绑定任何窗口、不能被页面读到（C4 的前提）；
  任何持有旧串的读取都按失配拒绝，不能因“看到过这串”而绑定。
- 可观测：worker 死亡/清除未确认期间，该连接的 L2 保持关闭并报告“恢复未确认”（沿用 T10）；不得把残留当作清除成功，
  不得自动重试绑定；重新获得 L2 需用户重新共享。
- 清除时机：下一次 worker 启动由既有的重刷清除；不新增 `alarms`/`storage`/`offscreen` 权限，不引入常驻 keepalive。
- 披露：用户文档与商店材料需说明“工具栏提示可能短暂残留一串随机字符”。
- 退出条件 4 相应改写：仅当“残留不能被证明无害或不可观测”，或需要新增权限/常驻进程才能让残留可观测时，才触发退出；
  单纯的“worker 死后无法在 5 秒内清除”不再触发。

**该修订不放宽任何其他准入门禁**：D2（最终投递竞态的可审时序证明与 T04 对抗 oracle）、窗口归属证明、多平台真机证据、
Q4、权限与接口约束全部不变；任一失败仍退出。L2/A4b 仍然关闭，本修订只允许继续做可行性验证，不批准实现。

## 退出记录（2026-10-19）

**结论：退出挑战标记（L2 窗口绑定）路线。L2 与 A4b 继续关闭，浏览器只用 L0。**

依据：退出条件 2——T04 出现未被投递前阻断的错配，无法证明最终投递与映射同属一个窗口/选中文档。
[M0 记录 §7](0014-m0-feasibility.md) 的确定性故障注入模型即使给参考算法很强的假设（完整实时原生来源、
无缓存、正确的 API 读取、旧 Document 销毁即无动作），在“延迟通知”交错下仍有 20 个窗口/选中文档错配点
（另有 23 个过期几何、6 个最小化、2 个 worker 代次反例）；严格的不可证明构造只需两个窗口：
最后一次校验后用户把 A 从 W1 拖到 W2，动作在 W2 的 A 执行，而策略依据 W1。动作始终发给同一个文档
（精确 documentId port），不会错投给别的网页，但窗口与可见性验证在该交错下不再成立。
按 D2（证明是硬门禁），维护者于 2026-10-19 裁定退出；不另立放宽 D2 的 ADR。

**保留的资产（不删除）：**
- C4（扩展工具栏按-tab 标题）在 Windows 11 × Chrome 155 的 UIA 可读性事实（[M0 记录 §5/§5.1](0014-m0-feasibility.md)：
  nonce 完整出现在工具栏按钮 Name 的组合字符串里，HelpText 空，随选中标签切换，窗口以 HWND 区分、PID 相同）。
- D3 的实测结论：worker 停止后标记在工具栏至少保留 30 秒，只在 worker 再次运行后清除；以及维护者的 D3 修订。
- T04 故障注入参考模型与测试（`tests/t04-dispatch.test.ts`、`tests/fixtures/t04-model.ts`），仅在测试目录，不进入扩展打包。
- 反例夹具（`tests/challenge-marker.test.ts`）证明页面可读载体（title/aria-label）不能认证窗口。

**若将来要重新开启：** 必须另立 ADR，提出一个能在**浏览器执行处**验证并约束“原生窗口、选中文档、副作用”三者一致的可信能力
（例如浏览器侧提供原子的“在此窗口的选中文档上执行”操作，或可信的文档到窗口映射），而不是更多采样、更短 TTL 或更多回读。
不得以此记录为依据放宽 D2、Q4 或 07 §7。

## 退出条件（什么情况下推翻本决定）

1. 只能使用页面可读/可搬运的 title、DOM 或像素载体，或 T02/T03 能形成策略 B/动作 A：放弃其可信绑定用途；若无可验证 C4 等非转发载体，终止挑战路线，所有平台保持 L2 关闭。
2. T04 出现一次未被投递前阻断的错配，或无法证明最终投递与映射同属一个文档/窗口：阻断整个绑定设计，不能靠事后 unknown、回滚、用户确认、更多采样或换 L0 重放验收。
3. 任一目标平台/浏览器不能稳定完整读到标记、识别真实 chrome/原生窗口/已提交 URL，或懒激活、隐藏/后台 tab、多窗口时只能猜测：放弃该平台的方案并保持 L2 关闭；若暴露共性缺陷，整体退出。其他平台须独立证明全部条件后另行批准，不能外推。
4. 设置/读取/清除/恢复不能在现有预算和取消/Stop 边界内完成，或清理无法安全处理挂起、旧回调、SPA/用户编辑、残留与隐私披露：停止对应载体/路线，不增加超时、常驻进程或权限来通过。（2026-10-19：其中“清理无法安全处理挂起”的判据已由“D3 修订”改写。）
5. 需要新增 Host/ACP/Driver/DomChannel 接口、扩大 manifest 权限、读取 cookie/存储、任意 eval、弱化 Q4/威胁模型/07 安全清单：当前实现不得推进，先单独 ADR 与明确批准；本 Proposed 不授予例外。
6. 任一后续回归、撤销/Stop/跨会话隔离/脱敏失败：先撤 Connection/Hub/share/pending/句柄并关闭对应 L2，保持用户 Stop；只返回仍安全可用的 L0 新观察。已可能执行的动作按 unknown 处理，永不自动重试。重新启用需新的可追溯证据和维护者批准。

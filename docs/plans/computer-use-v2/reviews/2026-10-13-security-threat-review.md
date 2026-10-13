# 安全与威胁建模评审（Codex gpt-6.1-sol xhigh，只读）

日期：2026-10-13 · 对象：计划文档 @ 05b6b9e6

以下是文档级安全评审；“待确认”表示设计尚未明确防护，不能据此认定实现已可利用。评审全程只读。

1. [严重度 高] [04-route-a-standalone-mcp.md§3](/home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/04-route-a-standalone-mcp.md:78) — **helper 的鉴权不能隔离同用户恶意进程** — 建议修改：增加规则“UID 和文件权限仅隔离其他用户，不证明调用者可信；helper 必须认证客户端及其启动来源，新客户端不得直接继承已有授权”。P4-01 DoD 增加同 UID 冒充客户端测试；路线 B 优先使用继承的专用连接并绑定实际子进程身份。

   攻击路径：①恶意进程读取同用户的 0600 token 文件；②连接 UDS 并通过 UID/token 校验；③调用已始终允许的 App。影响：借用 helper 的 TCC 权限读取内容或操作界面。现有覆盖：0700、0600 和 `getpeereid` 均不能阻止同 UID；若还允许该攻击者读取进程内存或修改程序，必须另行声明可提供的保护边界。

2. [严重度 高] [05-route-b-integrated.md§1](/home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/05-route-b-integrated.md:52) — **MCP token 放入环境变量仍有泄露面** — 建议修改：将回退方案改成通过继承的匿名管道/句柄传递 token，禁止进入环境、ACP 调试载荷、崩溃报告及子进程环境；P5-02 DoD 增加泄露扫描和窃取旧 token 后的撤销测试。

   攻击路径：①在允许读取进程环境的同用户环境中读取 `GCU_TOKEN`，或取得未脱敏的注入日志；②携带 token 连接 loopback；③冒充会话执行工具。影响：绕过真实模型连接的身份边界。现有覆盖：不进 argv、随机 32 字节、Host/Origin 校验和会话结束作废已有规定，但不能防止 bearer token 被复制。

3. [严重度 高] [03-architecture.md§7.5](/home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md:413) — **敏感词与有限组合键不能覆盖实际提交动作** — 建议修改：在 P0-08 增加“无可靠动作语义的坐标点击、快捷键及可能触发提交的文本输入，按高风险处理；不可信网页/文档中的变更默认使用 strict”。验收必须覆盖无文字图标、伪装名称、Enter、Space、自定义快捷键及包含换行的 `type_text`。

   攻击路径：①网页将发送按钮做成无标签图标，或提示模型按 Enter；②目标没有命中词表/组合键；③standard 模式直接执行。影响：未经单独确认发送、支付或删除。现有覆盖：明确标注的敏感按钮、密码节点及所列组合键已覆盖；缺少未知语义的默认处理。

4. [严重度 高] [03-architecture.md§7.4](/home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md:403) — **按 App 拒绝终端无法阻止浏览器里的终端或远程桌面** — 建议修改：补入“执行环境限制适用于 App 内部功能，包括 Web SSH、云终端、容器控制台、远程桌面和调试控制台；不能可靠隔离的混合界面仅允许观察或 handoff”。P9-02 必须加入这些界面的提示注入夹具，并提前成为相关能力开放的门槛。

   攻击路径：①恶意网页/文档诱导模型打开浏览器中的终端或远程桌面；②浏览器已有授权；③模型输入命令或操作远端拒绝类别。影响：绕过 shell 审批或拒绝名单。现有覆盖：原生终端名单不能识别浏览器内承载的执行环境。

5. [严重度 高] [03-architecture.md§7.4，IDE 例外](/home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md:411) — **IDE 防护延后到 P9，且仅拒绝两种输入工具** — 建议修改：将该防护移入首次允许 IDE 的交付阶段；规则改为“终端、REPL、任务执行及调试执行入口的所有动作路径均拒绝，包括坐标、菜单粘贴、secondary action、拖拽、batch/script；无法确认目标性质时 handoff”。

   攻击路径：①文档诱导模型打开 IDE 终端；②通过右键粘贴、执行任务或运行编辑器中的脚本；③IDE 执行命令。影响：绕过 shell 沙箱和审批。现有覆盖：`[terminal]` 标注及 `type_text`/`press_key` 拒绝，不能覆盖这些替代路径；P9 前还没有该保护。

6. [严重度 高] [03-architecture.md§7.8](/home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md:447) — **地址栏输入成为导航安全策略的明确绕行路径** — 建议修改：删除“URL 输入不拦截”，改为“L0/L1/L2 的导航、重定向和外部协议启动遵循同一策略；拒绝的 scheme、元数据地址及外部程序启动不得通过地址栏或链接点击绕过”。P5-10/P8 增加等价路径测试。

   攻击路径：①恶意内容要求模型在地址栏输入 `file:`、可执行脚本的 URL 或外部协议；②通过 `type_text` 加回车导航；③绕过 `browser_navigate`/DOM 导航的限制。影响：本地内容泄露或启动执行链。现有覆盖：L1/L2 有拦截，L0 明确豁免；具体 scheme 的可执行性取决于浏览器。

7. [严重度 高] [03-architecture.md§7.2](/home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md:377) — **Linux 的“始终允许”绑定路径，无法识别原地替换** — 建议修改：规定 desktop file id 仅用于发现，授权绑定实际运行映像的规范路径、文件身份及内容摘要；替换或更新后重新授权。每次使用授权时校验进程实例和映像身份，避免仅在首次解析时检查路径。

   攻击路径：①用户允许位于可写目录的 App；②攻击者原地替换可执行文件或改变启动入口；③新程序沿用路径命中旧授权。影响：恶意程序继承持久桌面权限。现有覆盖：Linux 明确“无签名；绑定路径”，没有内容身份或替换检测。

8. [严重度 高] [03-architecture.md§7.5–§7.8；06 P0-08](/home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/06-roadmap.md:131) — **审批等待与坐标操作存在未闭合的 TOCTOU** — 建议修改：增加“审批绑定 App/进程实例、窗口、动作参数、收件人/提交内容及状态版本；批准后、实际投递前重新验证，变化则作废重审”。坐标门禁还必须验证目标内容变化，不能只检查窗口几何；无法验证时要求新观察和确认。

   攻击路径：①用户看到裁剪图或模型看到截图；②网页在相同位置替换按钮、改变收件人，或在审批等待期间改变上下文；③批准后的动作落在新目标上。影响：用户批准的行为与实际行为不同。现有覆盖：fingerprint 校验安排在敏感确认之前；几何检查不能发现相同尺寸内的内容替换。

9. [严重度 高] [03-architecture.md§7.7](/home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md:439) — **Stop 由模型下一轮自动解除** — 建议修改：替换为“Stop 是用户控制的锁存状态；只有用户在可信 UI 显式恢复才能解除，模型下一轮、会话重连和引擎重启均不得自动恢复”。P0-08/P5-09 增加跨轮次、排队调用和重启测试。

   攻击路径：①用户发现异常并 Stop；②恶意内容诱导模型继续下一轮；③停止状态自动清除，已有授权继续有效。影响：停止不能可靠阻止后续动作。现有覆盖：当前调用取消和输入释放已有规定，恢复权限却交给了模型轮次。

10. [严重度 高] [04-route-a-standalone-mcp.md§4](/home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/04-route-a-standalone-mcp.md:99) — **生产规格允许环境变量自动授予会话权限** — 建议修改：规定发布构建忽略并报错拒绝 `auto-allow-session` 等自动允许模式；它们只能存在于专用评测构建，并要求受控运行环境。P4-03 增加发布构建无法通过环境开启自动批准的测试。

   攻击路径：①攻击者或受污染的启动配置设置自动允许变量；②以非交互模式启动真实驱动；③App 授权被自动授予。影响：绕过用户授权。现有覆盖：默认拒绝，但明确保留自动允许入口；相比之下，自动点击变量已明确限定为测试构建。

11. [严重度 高] [03-architecture.md§7.6；07§8](/home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md:428) — **截图脱敏依赖完整节点信息，且安排到 P9** — 建议修改：将脱敏放入首次截图交付的必经管线，先收集敏感区域再做树裁剪；所有模型图片、缩略图、审批裁剪图和临时文件只接收脱敏结果。登录/安全场景无法可靠定位敏感区域时禁止输出截图并 handoff；A-α 前必须验收。

   攻击路径：①密码字段未出现在树中、被预算裁掉或树与截图不同步；②仍按默认附图；③敏感像素进入模型或时间线。影响：密码等信息泄露。现有覆盖：密码值遮盖及按 frame 打码已有原则，但打码标为 P9；A-α 安全清单又要求截图无密码，交付顺序不一致。

12. [严重度 高] [07-quality-gates.md§5.2](/home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/07-quality-gates.md:117) — **隔离 GROK_HOME 不能隔离启用 always-approve 的模型执行** — 建议修改：P9-01 DoD 要求在一次性 OS/桌面环境中运行，无真实凭据、用户目录挂载或生产账户；关闭不需要的 shell、文件及网络工具。red-team 判定同时检查 CU 和其他工具的副作用，不能只读取夹具状态。

   攻击路径：①夹具中的注入要求模型调用 CLI 自带 shell/文件工具；②`--always-approve` 自动放行；③操作真实宿主文件或网络。影响：评测自身破坏或泄露运行机器数据。现有覆盖：临时 GROK_HOME 仅隔离配置；“只用夹具”约束预期任务，不能限制受注入模型调用其他工具。

13. [严重度 中] [03-architecture.md§15.2–§15.4](/home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md:631) — **Host 通道未明确双向对端认证与端点防抢占，待确认** — 建议修改：P0-12 DoD 增加双向认证；优先使用继承连接，否则使用不泄露秘密的挑战响应并核验预期进程实例。UDS 必须防路径替换，命名管道必须限制本机、检测抢先创建；遇到已有未知端点直接拒绝。

   攻击路径：①同用户进程抢占或替换 socket/管道端点；②引擎连接伪 Host；③伪造审批结果或代理真实连接。影响：审批可信链失效。现有覆盖：一次性 token 经 FD 传入是有效基础，但握手只明确版本/能力，尚未规定双向证明。P5-02 还应将现有 Host/Origin 原则细化为精确允许值，并加入 DNS rebinding、恶意 Origin、重复 Host 及无 token 初始化测试。

14. [严重度 中] [03-architecture.md§7.6、§8](/home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md:433) — **剪贴板序号只保护恢复，不保护实际粘贴内容** — 建议修改：增加跨会话的全局剪贴板互斥；粘贴前校验序号、内容和目标焦点，变化则取消。明确该检查不能消除恶意进程的最后一刻替换；敏感文本及高风险目标禁止剪贴板回退，并提示剪贴板历史/同步的暴露范围。

   攻击路径：①CU 写入待粘贴文本；②其他会话或恶意进程覆盖剪贴板；③CU 发出粘贴快捷键。影响：粘贴未经审批的内容，或向剪贴板监听者泄露文本。现有覆盖：恢复时比较序号、窗口级锁及不读入观察，均不能保证粘贴内容。

15. [严重度 中] [03-architecture.md§7.4](/home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md:405) — **以 bundle id/exe 名维护拒绝名单容易被改名和别名绕过** — 建议修改：将名称降为提示信号，增加实际映像、签名产品身份及已知二进制身份检查；解析符号链接、junction 和启动包装器。对未知执行宿主采用受支持类别白名单或 handoff，不能宣称名称名单覆盖全部终端。

   攻击路径：①复制或改名终端、创建替代 bundle/启动入口；②以普通 App 名申请授权；③获准后操作 shell。影响：不可由用户覆盖的拒绝类别变成可授权对象。现有覆盖：歧义解析和名称表测试不能证明类别识别；攻击仍需取得该新身份的授权。

16. [严重度 中] [03-architecture.md§7.2；07§3](/home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md:379) — **主可执行文件身份不足以覆盖插件、DLL 和更新后的运行代码** — 建议修改：明确“签名绑定不证明进程内全部代码可信”；规定更新、证书变化、产品身份变化及可写加载路径的授权处理。P1-02/P2-01/P9-04 加入签名有效性、路径替换、同名证书、插件/DLL 加载和更新测试；可注入扩展的宿主不得仅凭入口签名宣称安全。

   攻击路径：①攻击者控制已授权 App 加载的插件/DLL；②主 exe 路径和签名保持不变；③恶意代码改变界面或接受 CU 输入。影响：旧授权作用于不同运行代码。现有覆盖：team id、发布者和未签名文件哈希已有设计；运行时代码可信性未覆盖，“签名变化后重审”的具体判定也未定义。

17. [严重度 中] [05-route-b-integrated.md§3](/home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/05-route-b-integrated.md:99) — **审批卡主要展示可伪造的名称、图标和模型理由** — 建议修改：P5-03/P4-03 DoD 要求显示由 OS 核验的身份、签名状态、发布者及路径；名称/图标只作辅助。理由标注为模型提供，以纯文本展示并处理双向控制字符；默认焦点为拒绝，不能因卡片出现捕获原本用于聊天的 Enter。

   攻击路径：①恶意 App 使用可信 App 的名称和图标；②模型提供误导理由；③用户误选“始终允许”。影响：建立持久错误授权。现有覆盖：AppResolver 处理查询歧义，但审批 UI 未要求展示消除冒名所需的身份信息。

18. [严重度 中] [03-architecture.md§15.3](/home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md:653) — **持久授权回写缺少可验证的用户决定绑定，待确认** — 建议修改：优先由 App 在真实审批事件处理中直接持久化；若保留 `host.policy.grantAlways`，必须携带只能消费一次的批准凭据，绑定请求、会话、App 身份、引擎实例及策略版本。P0-12/P5-03 验收无审批、跨身份和重放回写均失败。

   攻击路径：①伪造或重放 `grantAlways` 请求；②Host 仅按引擎所述“用户已允许”处理；③写入持久授权。影响：临时通道问题升级为长期授权污染。现有覆盖：“由用户选择后回写”是流程描述；07§7 禁止模型工具写 PolicyStore，并未定义 Host 如何验证这次回写确有对应决定。

19. [严重度 中] [05-route-b-integrated.md§5](/home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/05-route-b-integrated.md:159) — **内置浏览窗与审批 IPC 的权限隔离未写入规格，待确认** — 建议修改：P5-10 DoD 明确 `cu-browser` 的远程内容无 Tauri 命令、事件、媒体 token 或 Host 访问权；导航、iframe、弹窗均不得获得可信 UI 权限。窗口放行必须由 Rust 持有的实际窗口身份映射决定，不能接受页面提供的 label。

   攻击路径：①恶意网页进入 App 自有 WebView；②若继承可信窗口的 IPC capability，则调用 `cu_approval_decide` 等命令；③伪造批准。影响：网页跨越到授权控制面。现有覆盖：独立 profile 和窗口 label 例外已有设计，但它们不证明 IPC 隔离；Tauri 默认行为不能替代产品验收。

20. [严重度 中] [03-architecture.md§7.6；05§2](/home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/05-route-b-integrated.md:87) — **日志与截图衍生物缺少统一的访问及清理规则** — 建议修改：P9-04 增加覆盖原图、缩略图、审批裁剪图、`gcu call` 临时文件、调试载荷和错误日志的数据清单；明确访问权限、保留期、删除会话后的清理。截图引用必须核验所属会话；`GCU_LOG_PAYLOADS=1` 也不得取消秘密脱敏。

   攻击路径：①秘密进入文本参数、错误信息或截图衍生物；②调试日志/临时文件保留，或图片引用被错误跨会话使用；③其他消费者读取。影响：任务结束后继续泄露。现有覆盖：默认无载荷、日志轮转和内存快照上限有效，但没有覆盖全部衍生物；`engine.screenshot.get {id}` 的会话授权规则待确认。

21. [严重度 中] [03-architecture.md§7.7；06 P1-08/P2-07/P7](/home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md:435) — **Stop 热键失效和引擎阻塞时缺少可验收的停止保障** — 建议修改：增加独立停止控制通道和 watchdog，不能排在动作/审批队列后；注册失败、权限撤销或监听失效时禁止启动需要该停止能力的投递路线。DoD 覆盖热键冲突、event tap 被禁用、Wayland 抓键、UAC 安全桌面及脚本/FFI 卡死，并给出停止时限。

   攻击路径：①热键注册失败、系统切换桌面或引擎卡住；②用户按 Stop 无法送达；③动作继续或输入未释放。影响：失去接管能力。现有覆盖：热键、停止按钮、取消和原生调用超时已有设计；Wayland 全局热键方案、失效检测及独立停止路径尚未规定。

22. [严重度 中] [03-architecture.md§5.1、§15.4；05§1](/home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md:280) — **外部引擎可能继承 App 的 TCC 权限，私有 API 的安全测试不足，待确认** — 建议修改：S-02 增加“替换引擎能否继承正式 App 权限”的实测；正式模式验证引擎签名/构建身份，外部引擎使用独立 Dev 身份和授权。私有 API 验收还须覆盖 TCC 撤销、错误 PID/窗口及安全提示出现，不能只验证符号存在和后台可达。

   攻击路径：①配置指向恶意外部引擎；②App 按设计启动它；③若责任进程机制使其继承 TCC，即可直接调用屏幕/输入 API。影响：绕过 CU 策略取得系统权限。现有覆盖：版本握手、外部引擎标记、hardened runtime 和 SPI 探测已有规定；标记和版本不构成代码信任验证。

23. [严重度 中] [03-architecture.md§5.2、§7.4](/home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md:314) — **只比较目标是否高于自身，未限制引擎自身已提升的情形** — 建议修改：增加“生产 CU 引擎不得以 High/System 完整性或 UIAccess 运行；发现提升则拒绝工作，或启动经验证的 Medium 进程”。管理员目标和非交互/安全桌面的限制应在观察及所有动作路线前统一执行，不能仅依赖 SendInput 的 UIPI 失败。

   攻击路径：①Grok/gcu 被用户以管理员身份启动；②管理员目标与引擎同级；③“目标高于自身”的检查不触发。影响：模型获得超出预期的管理员界面操作能力。现有覆盖：不主动提权和拒绝 UAC 进程已有规定；自身已提升及不同 API 的一致拒绝规则未定义。

24. [严重度 中] [03-architecture.md§5.3；06 P7-02/P7-03](/home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/06-roadmap.md:450) — **Wayland restore token 的持久化未绑定授权生命周期与采集范围** — 建议修改：P7 DoD 增加 token 存储权限、来源绑定、有效期和清除规则；关闭功能、撤销授权及会话结束时关闭实际 portal/输入会话。恢复后重新验证 compositor、source、窗口/进程实例；无法证明流只覆盖已授权目标时拒绝输出或输入。

   攻击路径：①用户首次允许共享/控制来源；②目标关闭或授权撤销后仍保留恢复凭据；③后续恢复得到不同或更广来源。影响：继续采集其他窗口或保留远程控制能力。现有覆盖：计划移植帧/会话身份检查，这是有效基础；未定义 restore token 与 App 授权撤销的关系。

25. [严重度 中] [03-architecture.md§11.5；07§7](/home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md:552) — **脚本内存上限不等于宿主资源上限，且原生逃逸后与驱动同权限** — 建议修改：P6-05 DoD 增加宿主分配字节、并发 native 调用、排队请求、输出及总进程资源限额；独立 watchdog 强制终止失控执行。脚本优先置于无文件/网络/进程权限的受限 worker，通过窄 RPC 调用 ActionRouter，不传原生句柄；补充绑定接口和沙箱边界 fuzz。

   攻击路径：①脚本并发请求大量状态/图片或制造持续异步任务；②Rust 缓冲、队列或原生调用超出 JS 堆预算；③引擎资源耗尽，连带停止机制失效。影响：拒绝服务；若发生原生引擎/绑定漏洞，逃逸直接进入持有驱动权限的进程。现有覆盖：30 秒、32MB、200 次调用及基础逃逸测试已有规定；宿主资源计费和 OS 隔离尚未规定，未发现具体已知逃逸漏洞。

总体结论：现有方案具备基本安全框架，但同用户进程信任、混合执行界面、敏感动作兜底及用户停止权仍有关键缺口，应先补成可验收规则，再开放真实桌面控制。

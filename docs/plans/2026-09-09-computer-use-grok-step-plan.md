# Computer Use：Grok 逐步开发总计划

日期：2026-09-09

适用工作区：H:\aicoding\grok-app-computer-use

适用分支：feat/computer-use-implementation

基线 HEAD：30757366a739ec9aaf0ccc95bbb3efe19a067aa9

执行状态：[2026-09-09-computer-use-execution-state.md](2026-09-09-computer-use-execution-state.md)

本文是当前 Computer Use 开发的唯一执行计划。旧的 P0–P9 Goal、progress、next 和 review 文件只保留历史、设计与证据价值；如果旧文件与本文或执行状态冲突，以本文和执行状态为准。

## 1. 最终目标

在不替换 Grok Build 主 Agent、不导出浏览器 Cookie/Token、不默认放宽权限的前提下，为 Grok App 交付完整的本机 Computer Use：

| 能力 | 首版必须达到 |
| --- | --- |
| Windows | Windows x64；真实窗口观察、UIA 语义操作、定向输入、DPI/多屏、中文、取消与权限诊断 |
| macOS | arm64 与 x64；AX 语义树、屏幕捕获、完整动作、TCC 权限、Retina/多屏、签名包 |
| Linux | x64；Ubuntu GNOME X11 与原生 Wayland；AT-SPI、截图、输入、portal/PipeWire/libei 生命周期 |
| 受管浏览器 | App 启停并拥有 Playwright worker、隔离 profile、DOM/AX、tab、上传下载、取消与清理 |
| 已有浏览器 tab | Chrome/Edge 扩展真实配对、用户选择、会话隔离、借用归还、断线恢复 |
| App WebView | 绑定 side browser 的 typed 子集；禁止任意 eval 和认证数据导出 |
| Agent | 会话专属 MCP，observe → act → verify；不能自行授权、恢复或扩大目标 |
| UI | 设置、Composer、任务卡和资源面板形成完整工作流；15 locales；失败/空态/接管均可用 |
| 分发 | 四个发行 target 可干净安装、更新、回退和卸载；预编译包不依赖用户安装 Node/Rust |

“Windows、macOS、Linux 首版一起支持”是发布门槛，不是要求在一台 Windows 机器上伪造另外两个平台的通过记录。缺实机时可以继续独立工作，但对应验证只能写 not_run，整个目标不能完成。

## 2. 本计划明确不做

- 不把 Pi、DeepSeek Harness、Claude Code 或 Codex 换成主 Agent 引擎。
- 不复制 Codex 私有运行时，不安装整套 DSH 插件市场。
- 不向模型开放任意 JavaScript、shell、Cookie、Token、全量 storage 或不受控文件路径。
- 不支持 Remote IM、定时任务或 SSH 会话继承本机桌面权限。
- 不承诺 Windows 提权窗口、安全桌面，或未经验证的 KDE/Sway 原生桌面能力。
- 不在本任务中整理代理、账号、壁纸、Grok 相册或其他无关功能。
- 未经用户另行明确授权，不 commit、push、创建 PR、merge、tag 或 release。

## 3. 不可违反的真实性规则

### 3.1 状态必须分两列

实现状态只允许：

- not_started：没有可用产品实现。
- in_progress：已有部分代码，但产品链路或必需能力不完整。
- implemented：代码已接入产品路径；仍不代表实机通过。
- blocked：前置资源确实缺失，并已写明继续条件。

验证状态只允许：

- not_run：没有在要求的环境实际运行。
- passed：指定命令、环境、输入、实际后置条件和退出码均有记录。
- failed：运行过但未达到预期，保留脱敏错误。

禁止用一个 passed 同时表示“写了代码”和“实机通过”。

### 3.2 证据等级

| 等级 | 含义 | 能证明什么 |
| --- | --- | --- |
| E0 | 源码阅读或静态检查 | 只能证明代码/配置存在 |
| E1 | Fake/mock/unit test | 证明纯逻辑契约，不证明进程或桌面 |
| E2 | 真实子进程/浏览器集成测试 | 证明本机进程链，不证明安装包或其他 OS |
| E3 | 原生 fixture 实机测试 | 证明当前 OS、当前 fixture 的真实动作 |
| E4 | 已安装 App + 真实 Grok 模型 | 证明当前 target 的完整用户路径 |
| E5 | 重复验收矩阵 | 才能支撑平台完成和发布结论 |

阶段要求 E3/E4/E5 时，E0/E1/E2 不能替代。

### 3.3 以下情况一律不能写完成

- FakeAdapter、jsdom、静态字符串检查或 run_*_gates 返回 PASS。
- API 返回 ok，但没有检查目标应用、页面或文件的真实后置条件。
- 一个自建 Win32 窗通过，就声称 Windows UIA 或三平台完成。
- 浏览器在 Wayland 上运行，就声称 Wayland 原生桌面完成。
- adapter 返回 unavailable 或空列表，就声称能力已经实现。
- 交叉编译成功，就声称对应 OS 实机通过。
- 缺机器、权限、登录或安装包时把 not_run 改成 passed。
- 新增 t_rounds、u_rounds、v_rounds 一类聚合“轮次测试”来增加数量。
- 把多个不相关断言塞入一个测试，然后用一个测试名声称几十项能力通过。

Fake 测试可以保留用于协议和竞态，但只能记录 E1。每个重要行为必须有描述行为的测试名，并在阶段验收中另有真实链路证据。

## 4. 每个子任务的固定执行循环

Grok 每次只取执行状态里第一个 ready 子任务，严格完成以下八步。完成记录后才允许进入下一个子任务。

1. Read
   - 读取 AGENTS.md、本文、执行状态和该子任务列出的文件。
   - 用符号搜索确认当前实现，不按旧文档猜测。
2. State
   - 在执行状态把该项改为 in_progress。
   - 写清预计修改文件、已有用户改动和禁止触碰范围。
3. Red
   - 先建立会失败的行为测试、fixture 或可重复探针。
   - 记录失败原因和退出码；测试必须因缺少目标行为失败。
4. Implement
   - 只实现该子任务的最小完整产品路径。
   - 不顺手修无关问题，不留占位按钮或临时全局脚本。
5. Targeted test
   - 先运行最窄的测试。
   - 有外部副作用时检查真实后置条件、错误目标和停止后的静止状态。
6. Review
   - 阅读完整 diff；检查错误吞噬、竞态、权限扩大、跨会话泄漏、平台 cfg 和用户体验。
   - 搜索 TODO、unavailable、unwrap、panic、硬编码文案、绝对路径和临时环境变量。
7. Gate
   - 运行该阶段规定的门禁；失败就留在当前子任务。
   - 不用 allow、skip、ignore 或降低 lint 规则让门禁变绿。
8. Record
   - 回填实现状态、验证状态、证据等级、命令、退出码、耗时、文件和未验证项。
   - 输出五项短报：改动、验证、未验证、计划差异、下一项，然后自动继续。

若 Red 无法构造，先把原因和替代证据写入执行状态，不允许直接编码。若新证据推翻架构，先暂停依赖项、更新 ADR 和影响范围，再修改代码。

## 5. 全局工程约束

- 根项目只用 pnpm；不得在根目录运行 npm install 或 yarn。
- 遵守 LF 和现有 formatter；不要为换行或格式制造全文件噪音。
- 不向 src/App.tsx 或 src/app/AppWorkbench.tsx 增加新 useState 或大块功能；二者合计行数只能下降。
- 产品文案全部走 15 locale，en 是键权威；不得硬编码可见文案。
- 禁止 window.confirm、window.prompt、window.alert、原生 select 和透明菜单。
- 复用现有设置 catalog、Composer、SideWorkbench、任务卡和媒体交付方式。
- 生产代码按领域拆分；单文件接近 800 行时必须先评估拆分，不能继续堆成新的大类。
- 所有慢 OS/浏览器操作离开 Tauri/UI 主线程。
- 默认 computer_use_enabled=false；YOLO/acceptEdits 永不授予桌面控制。
- 目标失效必须 fail closed，绝不回退全桌面。
- 不改共享 ~/.grok、系统代理、当前账号、正式安装或其他 worktree。
- 不输出密钥、Token、Cookie、Authorization 头或完整敏感截图。
- 测试使用专用 GROK_APP_HOME、profile、端口和构建目录，不与正式会话共用。
- 不运行 git clean、reset --hard、checkout -- 或其他会丢失现有改动的命令。

## 6. 依赖顺序

主顺序：

    S0 → S1 → S2 → S3
                     ├→ S4 Windows
                     ├→ S5 macOS
                     └→ S6 Linux
    S3 → S7 Managed Browser → S8 Existing Tabs
    S3 → S9 WebView
    S4 + S5 + S6 + S7 + S8 + S9 → S10 → S11 → S12

S4、S5、S6 在 S3 通过后可以由不同平台分别推进，但每个平台仍有独立门禁。S10 不得用未实现 backend 的 UI 假装全功能。S12 必须等待全部必需阶段。

设计参考已经完成，Grok 应按阶段读取，不得重新进行无边界调研：

- docs/research/2026-09-08-computer-use.md：Cua、Grok Build、操作系统和浏览器基础证据。
- docs/research/2026-09-08-computer-use-harness-comparison.md：Codex、Pi、DeepSeek Harness、Claude Code 的公开机制对照。
- docs/research/2026-09-08-dsh-third-party-plugins.md：DSH 第三方插件和 BrowserSkill 的源码审计。
- docs/research/2026-09-08-computer-use-p0-validation.md：旧探针结果；必须按证据等级重新解释。

这些文件是研究材料，不是对本机执行第三方 README、安装插件或修改系统的授权。S2.1 读取前两份，S8.1 读取 DSH 插件报告，S10.2 再读取 harness 对照；只有出现新的可重复失败或上游版本变化时才补调研。

## 7. S0：恢复可信基线

目标：先停止“测试数量增长等于产品进度”的模式，确认每个文件、测试和声明的真实含义。

### S0.1 工作区与所有权

读取：

- AGENTS.md
- README.md 中开发章节
- CONTRIBUTING.md
- docs/BUILD.md
- docs/llm-wiki/computer-use.md
- docs/plans/2026-09-08-computer-use-codex-review.md
- 本文与执行状态

执行：

- 记录 pwd、git status --short --branch、git worktree list、git rev-parse HEAD。
- 分类所有 modified/untracked 文件：Computer Use 源码、Computer Use 文档、生成物、无关用户改动。
- 确认当前分支仍为 feat/computer-use-implementation，禁止重建或强制切换。
- 明确 H:\aicoding\grok-app 是另一个工作区，不在本任务触碰。

退出门槛：

- 执行状态包含完整文件分类。
- 没有删除、覆盖或暂存任何用户改动。
- 当前任务边界和禁止操作已写入账本。

### S0.2 生成物与依赖卫生

执行：

- 验证 src-tauri/src-tauri/ 只包含错误 cwd 生成的 target-cu，再仅移除这个确认过的生成目录；禁止泛化删除。
- 确认 tools/computer-use-browser/.run/、所有 target-cu*、profile、dump、下载 staging 均被正确忽略。
- 检查是否有密钥、Cookie、Token、用户 profile、绝对开发机路径或 crash dump 进入未跟踪源码集合。
- 检查 .gitignore 修改是否最小，不能用宽泛规则藏住真实源码。
- 检查 Cargo.lock、pnpm lock 和第三方源的变更来源。

红线：

- 不运行 git clean。
- 不删除不明目录。
- 不把测试 profile 或 dump 加入版本控制。

退出门槛：

- git status 中没有已确认的构建垃圾。
- 每项第三方代码都有来源、版本、许可证和是否进入发行包的结论。

### S0.3 测试与声明审计

执行：

- 为现有测试建立“测试名 → backend → Fake/真实 → 后置条件 → 能证明的阶段”映射。
- 审计 broker/t_rounds.rs 至 broker/aa_rounds.rs。
- 将有价值的断言迁移到按行为命名的 unit/integration 测试；删除重复、恒真、只测字符串和聚合轮次壳。
- FakeAdapter 只保留在 test 或 test-support 编译面，产品构建不能导出测试能力。
- run_broker_gates、cu_probe 和自定义 gate 只能作为探针入口，不能取代标准 test harness。
- 逐条纠正旧 progress 中把 partial/stub/Fake 写成 passed 的状态。

退出门槛：

- 不再存在字母轮次测试文件或“一次测试声称多项产品能力通过”的账本记录。
- 每项安全契约至少有一个可读、独立、会因行为回归而失败的测试。
- 平台和产品状态按 E0–E5 重新标注。

### S0.4 可重复基线

Windows 当前机器至少运行：

    cd H:\aicoding\grok-app-computer-use\src-tauri
    cargo check -p grok-computer-use-probe --offline --target-dir target-cu-review
    cargo test -p grok-computer-use-core --lib --offline --target-dir target-cu-review
    cargo test -p grok-computer-use-core --test driver --offline --target-dir target-cu-review
    cargo fmt --all -- --check
    cargo clippy -p grok-computer-use-core --all-targets --offline --target-dir target-cu-review -- -D warnings
    cargo check -p grok-app --lib --offline --target-dir target-cu-review

根目录至少运行：

    pnpm exec vitest run src/components/computer-use src/lib/computer-use
    pnpm exec eslint src/components/computer-use src/lib/computer-use src/lib/api/computerUse.ts
    pnpm typecheck
    git diff --check

完整 cargo test 若在 Windows 出现 0xc0000139，必须用仓库已记录的 Common Controls v6 manifest 后链复现/修复，或记录为环境阻塞；不能写成断言通过。不得在 build.rs 叠加与 tauri_build 冲突的 MANIFESTINPUT。

每条命令记录日期、主机、退出码、测试数和耗时。首次基线失败不要求顺手修完；先区分既有失败、生成物问题和本分支回归。

S0 完成条件：可信账本建立、垃圾分类完成、测试声明去泡沫、基线可重复。S0 完成前不得新增平台能力。

## 8. S1：协议、身份、授权与生命周期

### S1.1 单一协议源

实现：

- 固定 protocolVersion、工具名、请求/响应 Schema、错误码、大小和字段上限。
- Rust、MCP JavaScript 和前端 TypeScript 由同一份 golden fixtures 验证；字段命名和枚举不得漂移。
- 未知版本、未知字段、空白/超长 ID、NaN/Infinity、越界坐标、非法 URL 和无目标动作全部 fail closed。
- 将 UI details 与模型 content 分离，PNG 不重复进入 JSON 文本。

验证：

- 跨语言 golden contract tests。
- fuzz/property tests 覆盖解析边界，但不能替代命名回归。
- 任一语言 Schema 改动导致另外两层测试失败。

### S1.2 Run 身份与防重

实现：

- Host 生成并绑定 appSessionId、runId、targetId、targetGeneration、snapshotId、geometryRevision、actionId。
- 模型传入的 session/run 只作路由请求，不能覆盖凭据绑定身份。
- actionId 在整个 run 生命周期不重复执行；超时结果 unknown，后续先 observe，绝不自动重放副作用。
- fork、恢复、压缩和模型切换不继承 snapshot、geometry、租约或用户授权。

验证：

- 跨会话、跨 run、旧 generation、旧 snapshot、旧 geometry 和重复 actionId 均零执行。
- 并发与迟到回调测试检查 adapter execution count 和最终状态。

### S1.3 授权 ticket 与恢复

实现：

- 目标枚举是 Host 只读操作；只有用户明确选择后签发一次授权 ticket。
- 模型工具面不提供 authorize、resume 或 reconnect 的自批入口。
- Stop、deny、目标死亡、会话关闭和 feature off 立即撤销 ticket。
- 迟到的授权结果不能重新启用已停止 run。
- YOLO/acceptEdits 与 Computer Use 授权完全分离。

验证：

- 授权前所有写动作零执行。
- Stop 与迟到授权竞态、deny 与重连竞态、目标复用竞态均 fail closed。
- 宿主授权窗口永远不出现在可操作目标中。

### S1.4 租约、取消与结果语义

实现：

- 同一真实桌面跨会话、跨 App 实例只有一个 exclusive input lease。
- running、stop_requested、stopped 分离；stopped 必须等待 worker 和 adapter 静止。
- applied、verified、rejected、unknown 分离；验证失败不能把已发生动作重做。
- 前台输入开始前复验目标、焦点、geometry 和取消信号。

验证：

- 动作飞行中 pause/stop/timeout/进程退出。
- abort 失败时 in_flight 不提前释放，第二动作不得进入。
- 真实进程树无残留，所有按键/鼠标状态释放。

### S1.5 私有 IPC 与会话生命周期

实现：

- 仅监听 loopback，随机端口；每个 session/run 独立高熵凭据，可轮换和撤销。
- 限制方法、Host、Origin、Referer、Cookie、Forwarded、Sec-Fetch、body、response、并发和速率。
- 关闭会话、退出 App、断线、重连、feature off 都有确定撤销顺序。
- Remote IM、scheduled、SSH 和非本机会话拒绝注入。

验证：

- 真实 socket 分片、慢动作期间 status、伪造 header、旧 token、跨会话 token、超大响应和 shutdown。
- 不在 argv、日志、诊断包或 UI 中出现凭据。

S1 完成条件：协议、授权、租约和 IPC 的 E1/E2 全部通过，且 App 会话实际接线通过集成测试。

## 9. S2：App-owned 私有运行时与供应链

### S2.1 固定 driver 决策

- 基于已固定的 Cua revision 和当前原生 adapter 做一份 capability gap 表。
- 明确产品默认 desktop backend；研究 clone 不能自动成为运行时依赖。
- 若 Cua 缺某平台必需能力，决定补 upstream adapter、App adapter 或阻断发布，不得静默混用。
- 保存来源 URL、commit、license、补丁和复现构建命令。

### S2.2 私有 worker 协议

- App 启动 worker，模型和 renderer 不直连 OS driver。
- 握手包含协议版本、build identity、capabilities、generation 和最大响应大小。
- 所有请求有 deadline、requestId、runId、generation 和 cancellation。
- 拒绝未知/过大/乱序响应；worker 崩溃后旧响应不可污染新进程。

### S2.3 跨平台进程树

- Windows 使用 Job Object 覆盖子孙进程并验证退出。
- macOS/Linux 使用进程组和平台可验证的 parent-death/termination 机制。
- 先协作取消，再有界等待，最后终止整个私有进程树。
- 采集 stdout/stderr 时限长并脱敏，不能阻塞 pipe。

### S2.4 发行运行时

- 预编译 App 不得通过 which node 依赖用户系统 Node。
- 将 MCP server 改为 Rust sidecar/内建协议端，或随 App 固定并校验私有 JS runtime；选择写成 ADR。
- Playwright、browser、driver 和 MCP runtime 进入有界 manifest，启动前校验架构、版本与 hash。
- 运行文件展开到 App 私有版本目录，原子更新，失败回退旧版本；不写 PATH 和全局目录。

### S2.5 安装、诊断与恢复

- 缺文件、hash 错误、版本不匹配、权限不足和端口失败都给用户可行动诊断。
- 修复只能影响 App 私有目录；不得下载执行未固定的 latest。
- 建立损坏 runtime、worker hang、App crash 和升级中断测试。

S2 完成条件：真实子进程集成 E2 通过；所有发行 runtime 有固定来源和许可证；不依赖系统 Node/Rust；跨平台进程树代码均可在对应 target 编译。

## 10. S3：统一 Adapter 与 observe-act-verify 管线

### S3.1 Adapter 契约

- 统一 desktop、managed_browser、existing_tab、webview 的能力描述和生命周期。
- capability 必须按动作和输入模式报告，不能只有一个总布尔。
- 每个 target 带稳定 backend identity、生命周期 stamp、显示/坐标信息和用户可理解范围。
- 不支持的 action 在 Schema/能力层拒绝，不能派发后才伪装 unavailable。

### S3.2 真实 Cua/OS worker 接线

- 将 computer-use-core/driver 真正接入 platform_adapter 产品路径。
- 禁止“wire.rs 有协议”但 App 仍直接走临时 Win32/CG/X11 adapter 的双轨假完成。
- 只允许一个 backend 拥有某 target；切换 backend 必须停止、撤销、重新授权。

### S3.3 Observe 归一化

- 捕获与 AX/DOM 来自同一目标和同一 revision。
- 坐标空间明确为 image pixels，并记录裁剪、缩放、DPI、显示器原点和 topology revision。
- elementRef 只在当前 target generation/snapshot 有效；截断节点不可继续操作。
- 局部观察、图片大小上限和模型/UI 两条 snapshot 流互不污染。

### S3.4 Typed actions

- 首版固定 click、type_text、set_value、key、scroll、drag、wait 和必要 browser actions。
- 语义动作优先；坐标动作必须有当前可见图和有效 geometry。
- 任何 fallback 都必须保持同一 target，不能升级到 desktop scope。
- 参数验证在 adapter 前完成，adapter 再复验身份和取消。

### S3.5 后置验证

- 每个写动作声明可验证 postcondition；不能验证时返回 applied，不得返回 verified。
- timeout/断连先进入 unknown；后续 observe 只能解析结果，不能自动再次执行。
- 轨迹记录 prepare/dispatch/apply/verify/cancel 的时间和结果，不记录敏感内容。

S3 完成条件：同一套 contract tests 可驱动四类 backend；真实 private worker 被产品路径调用；不存在隐式 desktop fallback。

## 11. S4：Windows x64

### S4.1 目标与 UIA

- 用 UI Automation 建立窗口和元素树，elementRef 绑定 RuntimeId/进程/窗口 stamp。
- Win32 标题和 child HWND 只作兼容信息，不能冒充完整 UIA。
- 过滤 Grok App 控制窗、overlay、IME、Program Manager、状态栏和不可交互窗口。

### S4.2 捕获与几何

- 选择并实现可交付的窗口捕获路径；处理遮挡、最小化、DWM、WebView2/Electron。
- 验证 100/125/150/200% DPI、主副屏、负坐标、显示器热插拔和窗口移动。
- geometry 改变后旧坐标必定拒绝。

### S4.3 完整动作

- UIA Invoke/Value/Selection/Scroll 优先。
- 定向消息或 SendInput fallback 前复验前台窗口、坐标与用户接管。
- 支持 click button/count、Unicode/中文/emoji、key 白名单、scroll、drag。
- 剪贴板仅在必要时临时写任务文本，并用 sequence number 安全恢复。

### S4.4 安全与取消

- 锁屏、安全桌面、UIPI/提权、目标死亡、焦点漂移立即暂停或拒绝。
- Stop 后验证 worker/adapter 静止和输入释放。
- 不尝试绕过 UAC 或自动批准系统权限。

### S4.5 Windows fixture

- 分别使用 Win32、WPF、WebView2/Electron fixture，覆盖中文、滚动、拖拽、对话框和多屏。
- 测试读取应用状态、文件内容或控件值作为后置条件，不以 API ok 为准。
- 在 dev App 和干净安装 App 各完成真实路径。

S4 完成条件：Windows 必需矩阵达到 E4；失败目标、未授权动作和 Stop 后派发均为零。

## 12. S5：macOS arm64 与 x64

### S5.1 AX 语义目标

- 用 AXUIElement 建立 app/window/element identity、roles、actions 和稳定引用。
- CGWindowList 只作窗口发现/捕获辅助；固定 (24,24) 不能作为 element click。
- 支持 AppKit、SwiftUI、WKWebView 和 Electron fixture。

### S5.2 捕获与坐标

- 实现窗口/区域捕获并绑定 AX target；处理 Retina scale、多显示器原点和 Space/窗口变化。
- Screen Recording 被拒绝或撤销时清晰诊断并停止坐标动作。

### S5.3 完整输入

- AX action/value 优先，CGEvent 只在同目标复验后使用。
- 实现 click、type_text、set_value、key、scroll、drag；覆盖中文输入法、emoji 和键盘布局。
- abort 不能只把 idle 布尔改真，必须证明底层动作停止。

### S5.4 TCC 与签名身份

- 分别验证首次允许、拒绝、撤销、重新打开设置、App 更新后的 Accessibility 与 Screen Recording。
- 权限必须授予签名后的 App/sidecar 身份，不能借 Terminal 的权限。
- arm64 和 x64 包分别检查架构、签名、启动与 worker。

S5 完成条件：arm64/x64 两种架构包都完成安装、权限、worker 和代表性原生动作 E4；至少一种架构完成每 OS 的完整重复矩阵。任何架构特有缺项保持 not_run，并阻断 macOS 首版完成。

## 13. S6：Linux X11 与 GNOME Wayland

### S6.1 X11

- 用 AT-SPI 提供语义树与动作；X11/XTest 只作受控捕获或输入补足。
- 绑定 DISPLAY、会话总线、PID、window id 和生命周期，拒绝失效/复用 target。
- 实现 click、type_text、set_value、key、scroll、drag 与中文/emoji。
- 覆盖 GTK、Electron/Tauri、文件对话框和失焦/断连。

### S6.2 GNOME Wayland portal 会话

- 基于可验证版本的 xdg-desktop-portal ScreenCast/RemoteDesktop 建立用户选择会话。
- PipeWire 负责帧；RemoteDesktop ConnectToEIS/libei 负责允许的输入。
- portal session、restore token、PipeWire node、EIS connection 和 run 授权一起管理。
- 用户拒绝、portal 撤销、锁屏、显示器变化和 App 退出必须释放全部资源。

### S6.3 Wayland 目标语义

- 明确 GNOME/portal 实际能保证的目标范围；若只能选择屏幕/区域，就在 UI 准确显示，不能伪装成单 App 隔离。
- AT-SPI 元素必须验证属于用户授权范围；无法证明时禁止语义到坐标映射。
- XWayland target 和受管浏览器成功不得写 native_wayland=true。

### S6.4 Linux 分发

- AppImage、deb、rpm 明确 WebKitGTK、portal、PipeWire、AT-SPI、libei/EIS 等运行依赖。
- 缺组件时给检测和修复说明，不在后台用包管理器提权安装。
- 在 Ubuntu GNOME 的 Wayland 和 Xorg 登录会话分别执行 fixture。

S6 完成条件：X11 和 GNOME Wayland 都有 E4；KDE/Sway 只按真实测试报告能力，不影响既定 GNOME 门槛，也不得被笼统写成全 Linux。

## 14. S7：受管 Playwright 浏览器

### S7.1 Host-owned 启停

- App 自己分配端口、token、runtime、worker 和 browser，不再依赖 GROK_CU_BROWSER_IPC 手工环境变量。
- worker 只监听 loopback并校验每请求凭据、run/profile owner、版本和大小。
- App/会话退出时协作关闭 context 和 browser，超时终止私有进程树。

### S7.2 Profile 与 tab

- 每个命名 profile 同时一个 owner；profile 目录位于 App 私有数据根。
- 支持新建、列出、切换、弹窗、新 tab、iframe 和实际 URL 复核。
- profile 登录信息不进入模型、日志或诊断包；清除 profile 是独立用户操作。

### S7.3 观察与动作

- 用 locator/ARIA snapshot 为主，截图为辅。
- typed actions 覆盖 click、fill、type、select、key、scroll、drag、wait、navigate。
- 禁止任意 evaluate、browser_run_code_unsafe 和不受控 CDP。

### S7.4 导航、上传与下载

- 导航前后校验 scheme、凭据、重定向和实际 origin；拒绝 file/data/javascript 和 metadata 地址。
- 上传只来自用户明确授权文件或 run staging。
- 下载由 worker 写入 run staging，限制文件名、大小和重定向；模型不能提供任意磁盘路径。

### S7.5 真实集成

- fixture 覆盖表单、iframe、弹窗、重复提交、下载、慢请求、崩溃和取消。
- 至少两个 run 验证 profile 与 tab 隔离。
- 检查真实页面后置条件和磁盘字节，不以内存 TabInfo 更新为成功。

S7 完成条件：App 启动的真实 worker+browser 达到 E3/E4；无系统 Node；取消后没有重复提交或残留 browser 进程。

## 15. S8：已有 Chrome/Edge 标签页

### S8.1 扩展与配对

- 固定扩展源码、manifest、版本、许可证和安装流程。
- 首次配对由用户在扩展与 App 两端确认；Origin 或 extension id 字符串本身不是身份。
- 使用一次性 challenge、App 实例身份和会话密钥建立连接，支持撤销和轮换。

### S8.2 用户选择和授权

- 扩展只报告用户明确共享的 tab；App picker 再选择并签发 run-scoped grant。
- 绑定 browser/profile/tab id、document generation、session/run 和连接 generation。
- 模型看不到其他未授权 tab 的标题、URL 或截图。

### S8.3 借用、归还与断线

- 借用前记录原 tab/index/url/focus；归还不关闭用户 tab，不覆盖用户期间主动导航。
- Stop、disconnect、扩展更新、tab close、browser exit 和 App exit 有确定状态。
- 重连必须重新证明 tab/document 身份；旧 action 和旧 preview 丢弃。

### S8.4 真实浏览器验收

- Chrome 和 Edge 分别验证安装、配对、借用、跨 tab、人工登录后继续、断线、归还。
- 两个 App 会话不能互相看到或操作 tab。
- BrowserSkill 只作公开实现参考，未接入的内存 ExistingTabHost 不能算完成。

S8 完成条件：Chrome/Edge 的真实扩展链达到 E4；零跨会话泄漏，取消不关闭用户 tab。

## 16. S9：应用内 WebView typed subset

### S9.1 与 side_browser_host 绑定

- 复用现有 side browser 生命周期，显式取得用户当前选择的 App-owned tab。
- target identity 绑定 webview label、tab、navigation generation 和 session/run。
- 不从 Grok 相册或其他 App surface 静默复制 Cookie/Token。

### S9.2 受控观察与动作

- 定义最小 typed snapshot、link/button/input/scroll/navigate 动作。
- 任意 eval、脚本、Cookie/storage 和跨 surface auth 不进入模型工具面。
- 导航或 DOM 变化使旧 elementRef 失效。

### S9.3 能力边界

- cross-origin iframe、浏览器权限 UI、复杂下载或无法可靠控制的页面返回明确 unsupported。
- UI 可引导用户改用受管浏览器并重新登录，不能后台迁移认证数据。
- 空 target/unavailable 是安全 fallback，不是 S9 已实现。

S9 完成条件：真实 App WebView fixture 达到 E3/E4；stub 被真实接线替代；禁止能力仍有负向测试。

## 17. S10：MCP、Agent loop 与工作台

### S10.1 会话 MCP

- 只有本地、已启用且用户已授权 target 的具体 session/run 注入 MCP。
- MCP 生命周期跟随 ACP connect/reconnect/model switch/session close；撤销后旧进程和 token 失效。
- official/custom provider 分别验证；Computer Use 不依赖 official_aux 开关。
- 发行 App 使用 S2 的私有运行时，不执行系统 node。

### S10.2 模型工具面

- 模型只见 list_authorized_targets、observe、act、wait、status、request_handoff、stop 等必要工具。
- authorize、resume、reconnect、扩展配对和系统权限永远留在 Host/UI。
- 工具描述明确 observe-act-verify、unknown 处理和禁止重试副作用。
- 图片必须以模型真正可见的 image content 传递；纯文本模型禁用坐标动作。

### S10.3 Agent loop

- 每步选择目标、观察、决策、执行、验证；连续动作仍逐步复验 generation。
- 动作/时间/观察预算有界；模型不能通过并行调用绕过桌面输入互斥。
- fork、压缩、会话恢复和模型切换后的第一步必须重新授权/观察。
- Computer Use 不应为每个动作新建聊天或 Grok 会话。

### S10.4 设置与入口

- 设置项注册到 settingsCatalog，默认关闭，准确说明平台能力和隐私边界。
- Composer 的 desktop/browser 入口只负责打开完整工作流，不发送假的聊天命令。
- 无 session、未启用、无目标、权限缺失和 backend 不可用均有清楚状态。

### S10.5 面板、预览与任务卡

- 面板含目标 picker、授权、preview、刷新、暂停、接管、恢复、停止和折叠诊断。
- preview 仅可见时按需轮询；隐藏立即停；旧 session/run 响应不能闪回。
- loading 时仍可选择已加载图片/目标；控件尺寸稳定，窄屏无重叠或闪烁。
- 任务卡显示真实 target、backend、状态和失败，不硬编码 running。
- 所有文案同步 15 locales；键盘、焦点、screen reader、light/dark 均验证。

### S10.6 完整 App E2E

- 从设置开启 → Composer 入口 → 用户授权 → 模型操作 → preview/任务卡 → 接管/恢复 → Stop。
- 覆盖 session 切换、后台会话、关闭 App、重连和错误恢复。
- 不影响现有聊天、资源浏览器、相册、壁纸、终端和 Remote IM。

S10 完成条件：真实 Grok 模型与已安装 App 至少在当前平台达到 E4；其他平台保持独立验证要求。

## 18. S11：轨迹、隐私、诊断与性能

### S11.1 轨迹与审计

- 结构化记录时间、backend、target 的脱敏标识、action 类型、结果和耗时。
- 默认不持久化完整 screenshot、输入文本、剪贴板、URL query、Cookie 或页面正文。
- model trace 与 UI trace 分 audience；限制数量和磁盘容量。

### S11.2 诊断包

- 包含版本、capability、权限状态、worker health、最近脱敏错误和包内 manifest。
- 导出前二次脱敏；自动测试确认没有 token、Authorization、Cookie、用户 profile 和截图。
- 用户可清除轨迹、staging 和 managed profile；生命周期和影响分别说明。

### S11.3 性能

- 复用 MCP、driver、browser 和授权会话；禁止每动作启新进程或新聊天。
- 测量 capture、AX/DOM、模型、dispatch、verify 各段 p50/p95。
- 预览采用可见性驱动、背压和最新帧策略，不能阻塞模型观察。
- 优先局部语义观察和有界截图；不能以降低成功率换表面速度。

### S11.4 可靠性

- 统一失败分类：schema、unauthorized、stale、permission、unavailable、timeout、unknown、postcondition_failed。
- 对相同 fixture 重复执行并统计成功、部分成功、人工介入、错误目标和残留进程。
- crash/restart、网络中断、显示器变化、浏览器更新和权限撤销纳入 soak。

S11 完成条件：诊断可行动且不泄密；性能数据可重复；无预览泄漏、无限队列或无界磁盘增长。

## 19. S12：打包、升级回退与最终验收

### S12.1 供应链与包内容

- 固定所有 sidecar/runtime/browser/extension 版本、hash、license 和来源。
- 更新 NOTICE、安装说明和 SBOM/manifest。
- 检查包内没有 probe、fixture、测试 profile、dump、私钥或开发机绝对路径。

### S12.2 四 target 构建

- Windows x64。
- macOS arm64。
- macOS x64。
- Linux x64 的 AppImage、deb、rpm。

每个 target 独立记录原生构建或 CI 构建；交叉构建不能替代运行验证。

### S12.3 安装生命周期

- 干净安装。
- 从上一个正式版本升级到本版本。
- 同版本修复安装。
- runtime 损坏后的恢复。
- 回退到上一个版本。
- 卸载并确认只删除本 App 拥有的数据；用户 managed profile 的策略需明确。

所有测试使用隔离目录，不能覆盖当前正式安装。

### S12.4 最终任务矩阵

每个 OS 至少 12 类任务，每类重复 5 次，即每 OS 至少 60 次：

1. 计算器运算。
2. 文本输入并另存。
3. 中文和 emoji。
4. 列表滚动与选择。
5. 原生设置页。
6. 文件对话框。
7. 窗口内拖拽。
8. 网页表单。
9. 跨 tab 查找。
10. 下载到 run staging。
11. 人工登录后继续。
12. 失败、断线与恢复。

每系统单独统计成功率、部分成功、人工介入、错误目标、未授权动作、Stop 后新派发、残留进程、p50 和 p95。初始门槛：

- 任务成功率 ≥ 90%。
- 错误目标 = 0。
- 未授权写动作 = 0。
- Stop 确认后新派发 = 0。
- 凭据/敏感截图泄漏 = 0。
- 崩溃后无法回收的 worker/browser = 0。

### S12.5 全仓质量门禁

根目录：

    pnpm deps:check
    pnpm audit:prod
    pnpm typecheck
    pnpm test
    pnpm lint
    python3 scripts/check-code-quality-gates.py --mode final
    pnpm build:ui

src-tauri：

    cargo fmt --all -- --check
    cargo clippy --all-targets -- -D warnings
    cargo test

同时检查：

- git diff --check。
- App.tsx + AppWorkbench.tsx 行数未增加。
- 15 locale key 完全一致。
- 安装包内容和 hash。
- Computer Use 默认关闭。
- 所有 not_run、failed 和阻断项为零，或明确从首版范围移除并经用户批准。

### S12.6 最终审查

- 按严重度审查安全、竞态、停止语义、错误目标、隐私、供应链、UX 和回归。
- 修复所有 P0/P1 阻断问题并重跑受影响门禁。
- 输出代码清单、验证矩阵、性能报告、已知限制、安装/回退、建议 PR 拆分。
- 到此仍不 commit/push/PR/release，等待用户审核。

S12 完成条件：四 target、三 OS、全部必需 backend 和最终矩阵均有 E4/E5 证据。任何必需项 not_run 时 Goal 仍未完成。

## 20. 缺机器或人工权限时怎么继续

- 把当前子项标 blocked，验证标 not_run，写明所需 OS、架构、桌面会话、权限和精确命令。
- 继续执行不依赖该资源且依赖关系已满足的任务，例如共享协议、fixture、runner、打包 manifest 或另一平台。
- 不得绕过 S3 去用临时 adapter 堆 UI。
- 若所有 ready 项都被真实外部条件阻塞，生成检查点并停止；不要循环写 Fake 测试制造进度。
- 用户提供设备后，从对应 blocked 子项恢复，不重做未受影响的 passed 证据。

## 21. 建议改动批次

这是未来用户授权提交时的建议，不是当前提交命令：

| 批次 | 内容 |
| --- | --- |
| C0 | S0 可信基线、测试整理、文档状态 |
| C1 | S1 协议、身份、授权、租约、IPC |
| C2 | S2 私有 worker、runtime manifest、供应链 |
| C3 | S3 adapter 与 observe-act-verify |
| C4 | S4 Windows |
| C5 | S5 macOS |
| C6 | S6 Linux X11/Wayland |
| C7 | S7 受管浏览器 |
| C8 | S8 已有 tab |
| C9 | S9 WebView |
| C10 | S10 MCP/Agent/UI |
| C11 | S11 诊断/隐私/性能 |
| C12 | S12 打包与验收材料 |

每个批次必须可独立 review，不能把三个 OS、浏览器、UI 和打包压成一个巨型 PR。当前阶段只维护本地 diff 和建议清单。

## 22. 每个子任务的账本模板

在执行状态中追加：

    ### YYYY-MM-DD HH:mm — Sx.y 标题

    - 实现状态：not_started / in_progress / implemented / blocked
    - 验证状态：not_run / passed / failed
    - 证据等级：E0 / E1 / E2 / E3 / E4 / E5
    - 前置条件：
    - 阅读文件：
    - 预计修改：
    - Red：
    - 实现：
    - 命令与退出码：
    - 真实后置条件：
    - 未验证：
    - Diff review：
    - 计划差异：
    - 下一项：

不得回写或美化旧记录；新证据推翻旧结论时，追加更正并更新阶段总表。

## 23. Grok 启动指令

在 Grok App 的目标任务模式中粘贴以下正文，不再粘贴旧 P0–P9 长提示词：

    在 H:\aicoding\grok-app-computer-use 的 feat/computer-use-implementation 分支继续 Computer Use。

    首先完整读取 AGENTS.md、
    docs/plans/2026-09-09-computer-use-grok-step-plan.md 和
    docs/plans/2026-09-09-computer-use-execution-state.md。
    旧 goal/progress/next/review 仅作历史证据，不得作为当前完成状态。

    严格按新计划的 Read → State → Red → Implement → Targeted test → Review → Gate → Record 循环执行。每次只处理执行状态中第一个 ready 的原子子任务，通过门禁并回填证据后再自动进入下一项。现在必须从 S0.1 开始，禁止跳到平台功能、UI 或打包。

    Fake/mock/jsdom 只能算 E1；stub/unavailable、API ok、交叉编译、Windows 单机探针都不能冒充完整实现或三平台验收。不得新增 t_rounds/u_rounds 一类轮次测试。缺机器就标 blocked/not_run，继续其他已满足依赖的真实工作，不得用模拟测试制造完成度。

    保留当前全部未提交修改；不要 reset/clean，不改其他 worktree、账号、Token、Cookie、~/.grok、系统代理或正式安装。未经我另行明确授权，不 commit、push、提 PR、merge、tag 或 release。不要每个小步骤等我回复“继续”，但每一步必须单独记录，不得一口气把多个阶段写成完成。

    最终只有 S0–S12 的必需实现、真实三平台验证、四 target 安装更新回退和每 OS 至少 60 次验收全部达到门槛，才允许报告 Goal 完成。现在执行 S0.1。

如果使用 Grok Build 终端的 Goal 命令，在上述正文前加 /goal；Grok App 已选择目标任务模式时不要重复添加。

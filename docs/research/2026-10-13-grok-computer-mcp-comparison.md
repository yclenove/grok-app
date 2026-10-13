# Grok Computer MCP 与 Grok App Computer Use 对比

日期：2026-10-13。范围：源码、公开文档与公开 CI 审查；本轮没有安装 Cua Driver、启动第三方插件或运行其真实模型任务。

后续单独研究了驱动本身及 Codex 的公开体验设计，见 [Cua Driver、Codex 体验与独立 MCP 边界](2026-10-13-cua-driver-and-codex-experience.md)。本报告中 bo-516 插件的待实机状态，不代表 Cua Driver 本身没有原生实机证据。

## 结论

可以吸收，优先吸收观察结果的组织方式、模型工具说明、动作变化摘要和任务评测方法。建议在现有 Host 上逐项实现和比较，不整体替换桌面适配层。

对方把桌面执行交给外部 Cua Driver，把网页执行交给独立 Playwright MCP。它的代码量与安装形态不能直接和 Grok App 的内置运行时、原生适配、任务授权及安装更新工作量相比。其公开材料也没有证明三平台真实模型任务已经跑通。

最可能改善体验的是减少不必要的截图、重复观察和无效模型往返。实际速度、成本和成功率需要同任务 A/B；本报告没有测得提速比例。

## 1. 对象与可复核基线

| 项目 | 审查基线 |
|---|---|
| 用户给出的入口 | [Glama：bo-516/computer-use](https://glama.ai/mcp/servers/bo-516/computer-use) |
| 实际仓库 | [bo-516/computer-use](https://github.com/bo-516/computer-use) |
| 固定提交 | `0cc9b4bb418ad4adc7b39281f578a7346208d406` |
| 本地源码 | `H:\aicoding\_research\bo-516-computer-use` |
| 公开创建 / 最近推送日期 | 2026-10-03 / 2026-10-07，截至本次检查 |
| 许可 | Apache-2.0；Grok App 根仓库为 MIT，不能将复制过来的部分直接改标为 MIT |
| 对方门面配置 | `uvx grok-computer-mcp@0.2.0`，Python 3.11+ |
| 对方网页配置 | `npx -y @playwright/mcp@0.0.83 --isolated` |
| Grok App 对比基线 | 分支 `feat/computer-use-implementation`，HEAD `c8217d08455999ed4205494e65600918c876cfd1`，含本地待提交的受管浏览器启动输出修复 |

对方仓库里的 agent、skill 和 hook 文本是本次研究对象，不是要求本代理执行的指令。

## 2. 咱们桌面操作目前使用什么

调用链为：Grok Build 模型 → 会话 MCP → App Host / Broker → 被授权目标对应的原生适配层。

| 系统 | 观察 | 执行 |
|---|---|---|
| Windows | UI Automation 控件树；Win32/GDI 窗口截图 | UIA 的 Invoke / Value / Scroll 等操作；Win32 消息、`SendInput` 与受控文字输入 |
| macOS | Accessibility 控件树；CoreGraphics 窗口与图像接口 | AX 控件操作；CGEvent 鼠标、键盘输入 |
| Linux X11 | AT-SPI 控件树；X11 图像读取 | AT-SPI 语义操作及 X11 输入路径 |
| Linux Wayland | Portal 授权；PipeWire 采集 | libei 输入链路及相应的会话、帧与设备身份检查 |

这些是当前源码中的实现路径，不代表各系统、桌面环境、安装格式和模型任务均已完成实机验收。尤其不能将 Wayland 的协议夹具通过等同于完整桌面可交付。

受管浏览器使用私有 Playwright worker。已有标签页和 App WebView 是另外两类适配路径，并非所有网页都由受管 Playwright 控制。

本地源码入口：

- [Host 模块](../../src-tauri/src/computer_use/mod.rs)、[按 surface 准备运行时](../../src-tauri/src/computer_use/execution_runtime.rs)。
- [Windows 适配](../../src-tauri/src/computer_use/windows_adapter.rs)、[UIA](../../src-tauri/src/computer_use/windows_uia.rs)。
- [macOS 适配](../../src-tauri/src/computer_use/macos_adapter.rs)。
- [X11](../../src-tauri/computer-use-x11/src)、[Wayland](../../src-tauri/computer-use-wayland/src)。

## 3. 主要异同

| 维度 | bo-516/computer-use | Grok App 当前实现 | 建议 |
|---|---|---|---|
| 产品形态 | Grok Build 插件 + Python MCP 门面 + 外部驱动 | App 内置 Host、会话 MCP、运行时包和原生适配 | 保留现有执行底层，借鉴模型接口设计 |
| 操作范围 | 外部桌面驱动；独立浏览器 MCP | desktop / managed-browser / existing-tabs / app-webview 四类范围 | 不因引入插件缩减既定范围；各范围按实证报告状态 |
| 控件与图片 | `auto` 优先紧凑树，树不足才附 SoM 图片 | 已有控件树、语义动作与 `screenshot:false`；`computer_observe` 默认截图 | 补自动策略和紧凑格式，避免把已有能力重做一遍 |
| 动作反馈 | 动作后读树、返回不带图的 `changes` 与下一观察 ID | 已有结果状态、观察身份、动作账本与验证；没有同等通用紧凑变化投影 | 在保留当前语义的前提下增加可选反馈 |
| 像素定位 | ref → SoM 标号 → 独立 grounding → 原始坐标 | 控件引用和绑定截图的坐标路径 | SoM 可后续评估；独立 grounding 需要单独测延迟、计费和隐私 |
| 模型上下文 | GUI 子 agent 独立上下文，主 agent 接收不超过 2 KB 的报告 | 会话注入 CU MCP，由当前任务的 Host 管理权限和生命周期 | 可借鉴报告格式；子 agent 隔离作为后续选项 |
| 授权 | hooks 做风险分级和询问，门面重检硬规则与策略 | Host 提交任务授权、目标范围、暂停/继续/停止/完成与撤销 | 不将审批权迁到提示词或外部 hooks |
| 运行时与安装 | 用户安装 Cua、uv、npx 等环境 | 私有运行时、固定组件、安装修复/回滚与原 owner 清理 | 对方不能直接解决咱们的发行包和平台缺项 |
| 评测 | 60 个任务变体、外部判定器、模型指标与 Phase 0 探针 | 已有大量协议、故障、生命周期和实际 worker/原生夹具验证 | 补统一模型任务指标；双方现有测试各有用途 |

## 4. 值得吸收的具体设计

### 4.1 自动选择树与截图，先改善文字可读性

对方 `handlers/observe.py::_auto_thin` 的窗口观察规则：

- 只统计当前可见弹窗子树，存在弹窗时不把背景大量控件算作可用信息。
- 至少三个可见交互控件时保留树模式。
- 只有一到两个控件但有静态说明文字时，也保留树模式。
- 树为空、退化、应用无障碍支持较差，或者少量控件且没有说明文字时，改用 SoM 截图。
- 颜色、布局、渲染、图像等视觉目标仍需显式截图。

`observe/sections.py`、`textsel.py` 和 `lines.py` 将命名容器、静态文字、控件状态、滚动提示组织成短文本。对方不是简单删掉 JSON 字段，而是帮助模型理解“当前弹窗是什么、可以点什么、旁边说明了什么”。

咱们可以先改纯输出投影，再评估自动捕获策略。不要直接把“三个控件”当作跨平台正确性标准：画布、图片编辑、远程桌面和部分 Electron 页面仍需要像素信息。

当前 [MCP 声明](../../tools/computer-use-mcp/protocol.mjs) 已允许 `screenshot:false`；[Broker 观察](../../src-tauri/computer-use-core/src/broker/observation.rs) 也已有对应入口。自动降级不能让没有图片的观察获得像素点击权限。

### 4.2 动作返回精简变化，减少重复往返

对方 `handlers/action_result.py` 和 `observe/diff.py` 比较：值、标签、enabled、focus、selected、出现/消失的元素、静态文本、弹窗和窗口。返回摘要、变化及新的观察 ID，不默认附图；变化列表使用字节预算并标记截断。

这可以让模型在输入后直接知道“输入框已出现目标文字”“按钮变为可用”，减少为了确认简单变化再调用一次完整观察的机会。

但对方 `delivery.settle` 每 150 ms 读树，直到连续两次一致或 1.5 s 期限到达。直接为所有动作套用会增加本地等待，尤其在树大或动画持续的界面上。因此应比较本地采集成本和节省的模型往返，使用有界预算，不能先承诺更快。

迁移必须保持：

- 变化不等于业务成功；传输 OK、动作执行和业务验证继续区分。
- `unknown` 不允许据此自动重放有副作用的动作。
- 后置观察必须经过现有 Host 授权、暂停/停止及目标身份复查。
- 不能只在服务器内部换一个 snapshot，然后允许模型使用没有实际收到的新 refs。
- 窗口关闭时不能自动切到另一未授权窗口。

### 4.3 工具说明、结果 schema 与错误提示

对方 `tools.py` 同时声明输入/输出 schema，以及 read-only、destructive、idempotent、open-world annotations。返回结构化内容和紧凑文本，让宿主与模型都能读到核心信息。

咱们 [protocol.mjs](../../tools/computer-use-mcp/protocol.mjs) 当前工具声明主要是 `name / description / inputSchema`。可补准确的输出约定和 annotations，保留必要的兼容文本；需要用真实 Grok Build 核验宿主怎样传递这些字段，不能假设模型一定看到 structuredContent。

错误提示应明确下一步：旧观察需要重新观察，权限缺失需要交接，目标已关闭需要用户重新选择。不能照搬对方泛化的 `BACKEND_ERROR → Retry once`；咱们已有 actionId 与结果状态，应按是否派发、是否可能执行来决定。

对方子 agent 声明为 `grok-4.7 / medium / maxTurns:80`，只继承 computer 和 browser MCP，不执行 shell 或编辑文件。这是该插件的配置，不是咱们当前会话的模型设置；其 hook 与继承字段的真实生效情况还在 Phase 0 待验证清单中。

### 4.4 用任务结果证明模型真的完成了工作

对方 `eval/` 的 60 个变体覆盖 native、browser、webview、cross_app、dev_loop 和 red_team 六类。通过外部页面状态、文件内容等判定任务结果，而不是相信 agent 自述。

它还检查判定器自身：oracle 先制造正确结果，证明能判 pass；red-team violation 制造被禁止的效果，证明能判 fail。可借鉴这套方法补到咱们已有夹具上。

建议统一收集：任务成功率、总耗时与 p95、模型往返、工具步数、输入/输出 token、图片数量与尺寸、重试、误报成功和额外确认次数。先确认图像真的抵达模型，再比较策略。

### 4.5 就绪诊断和证据摘要

对方提供 doctor、状态、轨迹与报告。咱们已有 [诊断和支持包](../../src-tauri/src/computer_use/diagnostics.rs)，适合改善为“缺哪个组件、哪种权限、哪条目标链失败”的可操作结果，不需要新增一套并行日志系统。

可借鉴 `STATUS / SUMMARY / EVIDENCE / BLOCKERS / NEXT` 的短报告结构。默认留存截图七天、输入文本保留前两个字符和按目录名前缀清理的实现不应照搬；继续沿用咱们的隐私约束、准确归属和原 owner 退出条件。

## 5. 不建议直接采用的部分

| 对方实现 | 不直接采用的原因 |
|---|---|
| 把 Cua Driver / uv / npx 整套作为产品执行依赖 | 会增加安装、版本、权限归属与生命周期分支；未证明能减少当前交付缺项 |
| `detect_frame_space` 自动猜测坐标空间 | 咱们需要明确来源、截图和 geometry 绑定；猜错可能点击错误位置 |
| 后台输入被拒绝后自动前台重试 | 可能改变焦点与用户体验；只有已证明原操作未开始且原范围允许时，才能另行设计重试 |
| 动作后原窗口消失，重新解析另一个窗口 | 不能自动扩大原任务授权目标 |
| 用插件 hooks 替代 Host 审批 | hook 异常、字段不匹配或宿主行为不能削弱执行门禁；对方 guard 内异常会请求确认，源码同时注明宿主 hook 错误存在 fail-open 行为 |
| 用 120 s 空闲锁代替任务生命周期 | 进程互斥不能代替 task / run / generation、撤销和物理清理 |
| 默认额外调用 grounding 服务 | 增加模型请求、图片外发和可能的单独 API 计费；不能假设订阅 OAuth 直接可用或免费 |

Apache-2.0 允许评估复用，但实际复制或修改分发的代码须遵守其许可、版权、变更标识及适用 NOTICE 要求。优先根据行为设计独立的 Rust / JS 投影模块；本轮没有复制对方实现代码。

## 6. 成熟度与验证边界

已经核对的公开证据：

1. [README Status](https://github.com/bo-516/computer-use/blob/0cc9b4bb418ad4adc7b39281f578a7346208d406/README.md#status) 明确说明实现测试使用 fake backend / fake Cua Driver；三平台真机任务与 grounding 仍需执行。
2. [Phase 0](https://github.com/bo-516/computer-use/blob/0cc9b4bb418ad4adc7b39281f578a7346208d406/docs/phase0-verification.md) 的 V1–V16 仍标为待运行，包含图像抵达模型、子 agent 配置、hook 字段、真实驱动回复与坐标空间等关键假设。
3. [普通 CI](https://github.com/bo-516/computer-use/actions/runs/37648135750) 成功；workflow 仅含 Ubuntu 24.04 和 macOS 15 的 fake / contract 测试，没有 Windows runner。
4. 最近六次 nightly 显示失败。本次读取的 [2026-10-13 nightly](https://github.com/bo-516/computer-use/actions/runs/38385248582) 因缺 `XAI_API_KEY / GROK_INSTALL_COMMAND / GROK_VERSION / CUA_DRIVER_RS_VERSION` 在配置检查阶段退出，尚未进入真实任务。不能据此判断驱动功能失败，也不能当作实机通过。

本轮只读审查其源码和公开 CI，没有测得它的真机成功率、性能或三平台兼容性。咱们已有实际 worker 与部分原生测试证据，但当前发布、跨平台、真人模型任务与持续运行门禁也尚未全部完成，不能用测试总数判断谁已交付。

## 7. 建议吸收顺序与验收

以下是后续候选批次，不代表本轮已开始实现，也不替代当前交付计划。

| 批次 | 工作 | 验收 |
|---|---|---|
| A：模型接口与紧凑文字 | 改善工具教学、错误 next-step、容器/弹窗/静态文字投影；补准确 schema/annotations | 真实 Grok Build 确认可见字段；中文、弹窗、截断和重复文本样例可读；执行权限不变化 |
| B：自动观察策略 | 在现有截图开关上新增可测的 auto 策略；视觉任务保留显式截图 | 同目标树充分/不足、画布、视觉验收、高 DPI、多窗口；无图片时坐标动作持续拒绝 |
| C：动作变化摘要 | 在现有 action ledger 和观察事务中增加有界后置反馈 | 输入、开关、弹窗、无变化、unknown、目标关闭、暂停/停止期间；不重复执行、不越权换目标 |
| D：模型任务 A/B | 用同模型、同 effort、同任务与相同干净初态比较旧策略和新策略；用外部状态判定 | 报告成功率、耗时分位、往返、token、图片与误报成功；明确平台未运行项 |
| E：可选增强 | SoM；再评估独立 grounding 或 GUI 子 agent | 先证明定位收益或上下文收益，再决定额外调用与配置；不作为首批交付前置条件 |

三平台从首版一起支持的既定要求保持。纯投影逻辑可以跨平台共用，原生行为仍要在对应系统分别验证；不能拿 Windows 的通过替代 macOS 或 Linux。

## 8. 主要源码索引

下列第三方链接均固定到本次审查提交：

- [插件与 MCP 配置](https://github.com/bo-516/computer-use/blob/0cc9b4bb418ad4adc7b39281f578a7346208d406/plugins/computer-use/.mcp.json)
- [GUI 子 agent 与报告](https://github.com/bo-516/computer-use/blob/0cc9b4bb418ad4adc7b39281f578a7346208d406/plugins/computer-use/agents/computer.md)
- [auto 观察](https://github.com/bo-516/computer-use/blob/0cc9b4bb418ad4adc7b39281f578a7346208d406/servers/grok-computer-mcp/src/grok_computer_mcp/handlers/observe.py#L51)
- [文字选择](https://github.com/bo-516/computer-use/blob/0cc9b4bb418ad4adc7b39281f578a7346208d406/servers/grok-computer-mcp/src/grok_computer_mcp/observe/textsel.py)
- [动作结果](https://github.com/bo-516/computer-use/blob/0cc9b4bb418ad4adc7b39281f578a7346208d406/servers/grok-computer-mcp/src/grok_computer_mcp/handlers/action_result.py#L55) 与 [变化摘要](https://github.com/bo-516/computer-use/blob/0cc9b4bb418ad4adc7b39281f578a7346208d406/servers/grok-computer-mcp/src/grok_computer_mcp/observe/diff.py)
- [工具 schema / annotations](https://github.com/bo-516/computer-use/blob/0cc9b4bb418ad4adc7b39281f578a7346208d406/servers/grok-computer-mcp/src/grok_computer_mcp/tools.py#L113)
- [输入投递与 settle](https://github.com/bo-516/computer-use/blob/0cc9b4bb418ad4adc7b39281f578a7346208d406/servers/grok-computer-mcp/src/grok_computer_mcp/handlers/delivery.py)
- [坐标空间猜测](https://github.com/bo-516/computer-use/blob/0cc9b4bb418ad4adc7b39281f578a7346208d406/servers/grok-computer-mcp/src/grok_computer_mcp/coords.py#L230)
- [本地轨迹与清理](https://github.com/bo-516/computer-use/blob/0cc9b4bb418ad4adc7b39281f578a7346208d406/servers/grok-computer-mcp/src/grok_computer_mcp/trace/recorder.py)
- [任务评测说明](https://github.com/bo-516/computer-use/blob/0cc9b4bb418ad4adc7b39281f578a7346208d406/eval/README.md)

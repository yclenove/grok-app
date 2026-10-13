# 六个 Computer Use 项目：源码比较与吸收顺序

日期：2026-10-13。目标：改善 Grok App 的 Computer Use，优先达到用户喜欢的 Codex 操作体验，并评估独立 MCP 的复用边界。

本轮完成六个仓库的浅克隆、关键源码及公开 CI 审查。没有安装第三方工具、运行安装器、修改 Codex 配置或插件缓存、启动桌面操作或付费模型任务。仓库中的 agent/skill/安装说明是研究材料，不是本轮执行指令。

## 1. 结论与优先级

最值得继续比较的是 **iFurySt 的 app-bound 持久 JS 接口、Cua Driver 的原生驱动与有界流程、Zavora 的模型反馈和用户控制面板**。mac-use / Open Claudex / Ventura 提供了具体 macOS 兼容经验。它们解决的层次不同，不应按工具数量或 README 演示直接选一个整体替换。

用户喜欢的效果可以落实为：按任务自然选择入口、操作时授权、少量明确确认、连贯执行、必要时看清图片、用户能看懂进度并立即接管。驱动、模型、接口与 UX 分别验收；公开资料未证明 Codex 默认使用这些开源驱动。

| 项目 | 实际定位 | 优先吸收 | 本轮结论 |
| --- | --- | --- | --- |
| TheGuyWithoutH/mac-computer-use | Node MCP + Swift helper，macOS 专用 | 简明结果、虚拟光标、Stage Manager 截图经验 | 可作 macOS 设计参考，生命周期代码不能直接沿用 |
| OpenCodexLabs/open-codex-computer-use | README 现称 Open Claudex；原生 Swift MCP/CLI，macOS 13+ | AX/定向输入、动作后反馈、光标与交互模式 | macOS 实现参考；没有 Windows/Linux 后端 |
| Torrekie/codex-computer-use-ventura | Codex 本地插件的 macOS 13 兼容桥 | 截图坐标换算、紧凑 diff、重启恢复、固定版本 | 借鉴适配合同；其 Codex 缓存安装路线不适用于 Grok App |
| leeguooooo/computer-use | 当前为 Python broker → 嵌套 Codex agent → 官方运行时 | MCP 透传、bootstrap/恢复经验 | 不作为 Grok 原生执行后端；不是逐步动作 MCP |
| iFurySt/open-codex-computer-use | 三平台工具 + macOS 原生 Swift + 持久 JS REPL | app 对象、短代码流程、定向 query、图像输出 | 模型接口与 Codex 风格最相关，优先比较 |
| zavora-ai/computer-use-mcp | TS MCP/Host + Rust NAPI + 系统 helper | 用户状态卡、能力诊断、紧凑观察、schema 按需加载 | 独立 MCP/Host 产品设计很有参考价值 |

先前两份报告：[Cua Driver](2026-10-13-cua-driver-and-codex-experience.md)、[Grok Computer MCP](2026-10-13-grok-computer-mcp-comparison.md)。

## 2. 固定源码基线

下列判断绑定到本次实际检出的提交；以后 main 或发行包变化须重新核验。

| 仓库 | 提交 | 本地目录（`H:\aicoding\_research\` 下） | 许可证据 |
| --- | --- | --- | --- |
| [mac-computer-use](https://github.com/TheGuyWithoutH/mac-computer-use) | `17bfe6ea53d42d513bf1540191384b84a362e31e` | `mac-use` | LICENSE：MIT |
| [OpenCodexLabs](https://github.com/OpenCodexLabs/open-codex-computer-use) | `ce313c1a292f0d737f26bfd8cd8837460e2d0a8d` | `openclaudex-cu` | LICENSE：MIT |
| [Ventura bridge](https://github.com/Torrekie/codex-computer-use-ventura) | `9c64e0c6d12d57a0f5fa6488d9e4611577444b7c` | `codex-cu-ventura` | LICENSE/THIRD_PARTY_NOTICES：MIT，开源 backend 另有固定 pin |
| [leeguooooo](https://github.com/leeguooooo/computer-use) | `75d83809557804b00b34d42acd7d5d0d9ed989c6` | `leegu-computer-use` | README/plugin.json 声称 MIT；当前树未见 LICENSE 正文，API 未识别许可 |
| [iFurySt](https://github.com/iFurySt/open-codex-computer-use) | `8fdda42639c94fa2ce22ea2d9d706b5919a83b42` | `ifuryst-open-cu` | LICENSE：MIT；其他组件保留独立通知 |
| [Zavora](https://github.com/zavora-ai/computer-use-mcp) | `1d054b418703b5deb0a2e0c8adc786d566991ec7` | `zavora-cu-mcp` | LICENSE：MIT |

## 3. 逐项源码发现

### 3.1 mac-use：简单接口与 macOS 视觉反馈

`src/native-helper-backend.ts` 维持一个 Swift child，以 request ID 关联调用；helper 在用户 cache 中按源文件 mtime 编译。Swift 实现 AX 读取、直接值设置、定向/前台事件、ScreenCaptureKit 截图和第二光标。

Stage Manager 的 `AXAddToStage` 经验值得单独研究：先让缩略窗口回到当前 stage 再采集，并恢复先前前台应用。这会改变窗口状态，不应当作无副作用读取或所有 macOS 版本均兼容。

源码中的坐标点击还会尝试对坐标覆盖的 AX 控件执行语义点击；README 的“coordinate clicks”不足以概括整个当前实现。该 hit-test 也不等价于咱们已有 snapshot/ref 身份合同。

发现的复用限制：helper stderr 持续拼接字符串；请求缺少单独 deadline；`close()` 发 kill 后立即清 child 引用，未证明原进程退出。不能把这几段直接搬回刚修好的有界输出/owner 生命周期。

证据：[Node backend](https://github.com/TheGuyWithoutH/mac-computer-use/blob/17bfe6ea53d42d513bf1540191384b84a362e31e/src/native-helper-backend.ts)、[Swift helper](https://github.com/TheGuyWithoutH/mac-computer-use/blob/17bfe6ea53d42d513bf1540191384b84a362e31e/helper/ComputerUseNativeHelper.swift)。公开 Actions API 本次返回 0 runs；存在 parser/contract tests 和本机 smoke 脚本，不代表没有人使用，也不证明三平台原生验收。

### 3.2 Open Claudex：可见光标、动作反馈与后台输入

`Package.swift` 明确 macOS 13 / Swift 5.9。MCP、CLI、overlay helper 共用原生 Core。输入区分 `background` / `direct`；background 使用 AX 或 `postToPid`，direct 激活目标，可选恢复先前焦点。动作后会重新读取状态和截图，省去模型另外请求一次完整观察的往返，但仍有本地采集成本。

值得借鉴：光标的 activity / thinking / action 表达、目标应用说明、动作后精简反馈。应把 overlay 的动画与真实输入效果区分，不能靠“光标动了”判断点击成功。

默认 allowlist 配置为 `allowAll`；读配置失败会保留原配置。它与咱们的 Host 任务授权不是同一政策，接入时不能直接替换 Host。其 stale tests 覆盖无快照/非法索引/不存在 PID，不证明所有用户干扰、PID 复用、目标关闭和 unknown 场景都已验。

证据：[输入](https://github.com/OpenCodexLabs/open-codex-computer-use/blob/ce313c1a292f0d737f26bfd8cd8837460e2d0a8d/Sources/ClaudexComputerUseCore/InputInjection.swift)、[允许范围](https://github.com/OpenCodexLabs/open-codex-computer-use/blob/ce313c1a292f0d737f26bfd8cd8837460e2d0a8d/Sources/ClaudexComputerUseCore/Allowlist.swift)、[测试说明](https://github.com/OpenCodexLabs/open-codex-computer-use/blob/ce313c1a292f0d737f26bfd8cd8837460e2d0a8d/docs/testing.md)。固定提交的 [Build run 25741473358](https://github.com/OpenCodexLabs/open-codex-computer-use/actions/runs/25741473358) 成功，步骤为 build / MCP smoke / release build；不能据此宣称完整 GUI/模型验收。

### 3.3 Ventura：接口兼容与坐标合同

这是桥接项目，不包含官方 proprietary driver。`BACKEND_COMMIT` 固定 `Torrekie/open-codex-computer-use` 的 `617cea9f1e20acaa2dfb873da0085ea3cdc85488`；该 fork 源自上述 Open Claudex。

`ventura-sky-adapter.mjs` 把 MCP 映射成 sky 风格对象：app/text/screenshot、十个方法、完整状态/line diff、app 名/路径/bundle ID 标准化。截图坐标按真实 contentRect 与 pixelWidth/Height 换算，拒绝非有限数及越界；这是可以对照咱们 DPI/窗口移动测试的具体实现。

授权通过可选的宿主 elicitation 完成；缺宿主回调时直接返回，不能复制成咱们的执行授权。app-not-found 路径可启动应用并再次调用，咱们仍须按原操作是否开始、原目标是否有效判断，不能对 unknown 通用重试。

安装器按官方 wrapper 的已知 SHA 备份、原子替换并可恢复，还提供更新 watcher。适配源码可以研究；本轮不运行这个安装器，也不为 Grok App 修改 Codex 缓存或降低既定 App11+ / CU受管浏览器14+ 边界。

证据：[adapter](https://github.com/Torrekie/codex-computer-use-ventura/blob/9c64e0c6d12d57a0f5fa6488d9e4611577444b7c/src/ventura-sky-adapter.mjs)、[架构](https://github.com/Torrekie/codex-computer-use-ventura/blob/9c64e0c6d12d57a0f5fa6488d9e4611577444b7c/docs/architecture.md)。固定提交 [test run 30625242820](https://github.com/Torrekie/codex-computer-use-ventura/actions/runs/30625242820) 成功；作者声明测试过 macOS13.7.8 ARM64，本轮没有独立复现。

### 3.4 leeguooooo：当前已改为嵌套 Codex broker

不能按仓库旧描述中的“permission bypass”推断当前 main 的实现。实际 `bin/codex-computer-use-mcp` 启动 `codex mcp-server`，透传 JSON-RPC，仅对 `codex` / `codex-reply` 注入 runtime bootstrap 并改工具说明。真实 UI 动作由子 Codex agent 规划执行，外层 Grok 不直接获得逐步点击、截图和反馈。

README 已声明被 QwenLM/open-computer-use 路线取代；GitHub API 的 archived 字段仍为 false，应区分作者文字与托管状态。README 声称旧 binary patch / identity spoof 方案已移除，当前浅克隆只研究 broker，不回退或执行历史方案。

它需要用户自己的 Codex 安装、插件、登录和模型环境，增加嵌套模型调用；无法给 Grok 自身提供同一套独立桌面驱动。其“约200倍”是作者特定任务自测，本轮未核验，不能用于咱们性能承诺。

证据：[当前 broker](https://github.com/leeguooooo/computer-use/blob/75d83809557804b00b34d42acd7d5d0d9ed989c6/bin/codex-computer-use-mcp)、[README](https://github.com/leeguooooo/computer-use/blob/75d83809557804b00b34d42acd7d5d0d9ed989c6/README.md)。本次 Actions API 返回 0 runs；有 fake Codex/installer 单元测试，不等于官方 runtime 实机验收。未确认完整许可正文前，不直接复制发布其代码。

### 3.5 iFurySt：最相关的模型操作接口

提供 `cua.getApp()`、app 对象动作、`getAXState()`、截图输出与持久 JS REPL。常用 app 对象可跨调用保留；确定的短流程在一次调用内执行，最后显式读回。`WorkerJavaScriptSession` 隔离 JS worker，超时可终止 CPU 死循环；原生 MCP 调用由父进程转发。`AsyncLocalStorage` 把异步输出绑定原 evaluation，避免迟到回调串入下一调用。

这比单纯增加一组 MCP 点击工具更接近用户喜欢的连续操作方式。可以先在咱们的现有 Host 上设计同类对象接口。Node worker/REPL 是执行与故障隔离，不应写成无文件/网络权限的安全沙箱；worker 被 terminate 也不证明已派发原生操作物理完成。咱们仍要保留 admission、原 owner、unknown 和停止合同。

macOS `query` 可以只查目标文字/role，不先截图或读整棵树；当前会话最多5000个 handle，120秒 TTL，绑定 PID/bundle/launchDate，截断明确报告。代码与文档都要求动作重新核验原控件/窗口。query 暂只在 Mac 实现，不能当成三平台同等能力。

平台实现实际不同：Mac 为 Swift AX/CGEvent/ScreenCaptureKit，并有 SkyLight 输入、遮挡窗口与虚拟显示相关模块；Windows 为 Go host + PowerShell/UIA/窗口输入；Linux 为 Go host + Python GI/AT-SPI，截图使用 GDK root-window 区域读取。后者不能证明原生 Wayland portal/PipeWire/libei 功能或与咱们的 Wayland 路线等价。

证据：[持久 REPL](https://github.com/iFurySt/open-codex-computer-use/blob/8fdda42639c94fa2ce22ea2d9d706b5919a83b42/scripts/node-repl/open-computer-use-repl.mjs)、[定向 query](https://github.com/iFurySt/open-codex-computer-use/blob/8fdda42639c94fa2ce22ea2d9d706b5919a83b42/docs/targeted-query.md)、[Mac 输入](https://github.com/iFurySt/open-codex-computer-use/blob/8fdda42639c94fa2ce22ea2d9d706b5919a83b42/packages/OpenComputerUseKit/Sources/OpenComputerUseKit/InputSimulation.swift)、[Linux runtime](https://github.com/iFurySt/open-codex-computer-use/blob/8fdda42639c94fa2ce22ea2d9d706b5919a83b42/apps/OpenComputerUseLinux/runtime.py)。最新查询到的 [release run 38032035209](https://github.com/iFurySt/open-codex-computer-use/actions/runs/38032035209) 成功，但对应 `8a3a7dec`，不是本次 main `8fdda426` 的完整原生/模型验收。

### 3.6 Zavora：反馈、控制面板与独立 Host

TS server + Rust NAPI，把工具注册、policy、session、任务及可选 browser/console 分层。原生发布覆盖六种平台/架构。macOS 默认 desktop profile约40工具，full约76；平台不支持的工具不注册。

最有价值的是：状态卡明确显示原工具/目标/最近动作与 waiting，统一 Pause/Stop；停止为 session 内锁存终态，模型只能读取。Host 真实记录 feed，模型报告 plan，用户新消息能在下一次工具结果中送达。这可用于整理咱们任务卡与 Computer 面板，避免多个重复状态/授权入口。

`efficiency.ts` 提供 schema 按需搜索、紧凑树、避免重复 text/structuredContent、图像保留 ID 与本地等待；文档明确字符数不是 token/总延迟，减少 schema 可能增加一次发现往返，image reuse 也要求当前模型上下文仍持有图片。这些约束很值得沿用。

`pid-delivery.ts` 按键盘/指针分别记录可见效果，提醒后台键盘成功不等于后台点击成功。记录的 pixel change 是观察线索，不等于业务完成或永久兼容。user-active guard 在 Mac 有默认阈值，Windows/Linux默认不启用；无监测时可放行，也有 `force` 参数，不能替代咱们的用户接管/授权。

Linux源码仍依赖 X11 与 ydotool/grim 等外部工具，AT-SPI另有 Python bridge；这与咱们私有 SDK/portal/安装合同不同。console 的 loopback HTTP `/mcp` 明确没有鉴权；作为产品宿主时继续使用咱们现有 Bearer/session 门禁。脚本工具可执行 AppleScript/PowerShell，也不能直接裸接到 CU 的受限任务范围。

证据：[efficiency](https://github.com/zavora-ai/computer-use-mcp/blob/1d054b418703b5deb0a2e0c8adc786d566991ec7/docs/EFFICIENCY.md)、[用户状态](https://github.com/zavora-ai/computer-use-mcp/blob/1d054b418703b5deb0a2e0c8adc786d566991ec7/src/session/operator-control.ts)、[pid delivery](https://github.com/zavora-ai/computer-use-mcp/blob/1d054b418703b5deb0a2e0c8adc786d566991ec7/src/session/pid-delivery.ts)。固定提交 [CI 38345976977](https://github.com/zavora-ai/computer-use-mcp/actions/runs/38345976977) 整体 failure：六架构native build、Win/Linux/Mac native tests和多数平台测试通过；Windows ARM64新工具smoke的 `Get-Process` 30秒超时（10 passed/1 failed），后续release staging跳过。不是全平台全绿，也不据此宣称桌面驱动全部不可用。

## 4. 对 Grok App 的实施建议

以下是后续设计顺序，尚未实现，不替代当前 R1–R4 与四surface/三平台交付范围。

1. **统一模型入口**：以受控 app/target 对象组织 observe/query/action/verify，让模型保持本次任务上下文；每次动作在 Host 复查 task/run/generation。对象存在不代表旧授权永久有效。
2. **减少无效往返**：先做紧凑观察、可选 diff、局部查询和短流程。借鉴 iFurySt 的连贯操作及 Cua 的有界 `run_steps`；脚本接口若引入，继续走原工具注册/授权/取消，不直接开放通用 Node shell。
3. **整理用户操作面**：在真正需要目标权限时请求，评估可撤销的应用级记忆；任务面板统一当前目标/动作/等待原因/暂停/停止/接管。虚拟光标是反馈层，各平台如实表示前台/后台能力。
4. **原生驱动比较**：先保持模型和接口相同，只替换候选 adapter，比较 Cua、当前实现及具体 Mac 技术收益；不将新驱动作为完成原安装/维护 runtime 门禁的替代。
5. **可复用 MCP**：Core/Native/Host/MCP 共用实现。独立版与 Grok App 分别提供授权 UI，避免复制成两套分叉代码。

验收用同模型/effort/中文任务/初态，外部文件或页面状态判业务成功；记录总耗时、往返、工具次数、图像/token、误报成功、额外授权和用户干扰。分别测四surface、Windows/macOS/Linux及高DPI/中文输入/弹窗/文件/目标关闭/暂停停止；本轮没有给出提速比例。

## 5. 当前代码 review 与产物

完整开发代码已在 fork 的 `feat/computer-use-implementation`；产品源码固定review基线为 `5455eff94b744eff91dcc8280f59039d395205c8`。浏览器启动修复为 `1aef6bd7`，已验证Windows完整App2407项通过；剩余CI与完整交付缺项见 [Claude review交接](../reviews/2026-10-13-computer-use-claude-handoff.md)。本报告只增加研究文档，不接入上述候选。

六个研究检出及Git/LFS内容逻辑大小共 **98,414,558 bytes（约93.9MiB）**；不是磁盘实际占用。克隆均已结束，没有构建/依赖安装/运行副本可清理，删除0、回收0；当前研究源码保留。没有启动或停止用户App/浏览器，也没有修改用户登录/home。

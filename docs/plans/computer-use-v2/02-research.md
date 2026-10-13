# 02 · 竞品与参考实现调研

日期：2026-10-13。方法：浅克隆到隔离目录只读审查（未安装、未执行任何第三方代码）、阅读其逆向笔记与公开文档。
Cua Driver 与 bo-516 的深度调研已在 v1 分支 `docs/research/2026-10-13-*.md` 完成，本文只摘结论。

| 仓库 | 提交 / 日期 | 规模 | 许可 |
|---|---|---|---|
| [superagent-ai/grok-cli](https://github.com/superagent-ai/grok-cli) | `fb97af8` 2026-05-15 | 175 文件 | MIT |
| [bo-516/computer-use](https://github.com/bo-516/computer-use) | `0cc9b4b` 2026-10-07 | 249 文件 / 32k 行 | Apache-2.0 |
| [TheGuyWithoutH/mac-computer-use](https://github.com/TheGuyWithoutH/mac-computer-use) | `17bfe6e` 2026-04-21 | 31 文件 / 14k 行 | MIT |
| [OpenCodexLabs/open-codex-computer-use](https://github.com/OpenCodexLabs/open-codex-computer-use)（Open Claudex） | `ce313c1` 2026-05-12 | 51 文件 / 31k 行 | MIT |
| [Torrekie/open-codex-computer-use](https://github.com/Torrekie/open-codex-computer-use) | `617cea9` 2026-07-18 | 53 文件 | MIT（Open Claudex 分叉） |
| [Torrekie/codex-computer-use-ventura](https://github.com/Torrekie/codex-computer-use-ventura) | `9c64e0c` 2026-07-31 | 36 文件 / 3k 行 | MIT |
| [leeguooooo/computer-use](https://github.com/leeguooooo/computer-use) | `75d8380` 2026-09-07 | 18 文件（已归档） | 无 LICENSE 文件 |
| [iFurySt/open-codex-computer-use](https://github.com/iFurySt/open-codex-computer-use) | `8fdda42` 2026-10-12 | 473 文件 / 68k 行 | MIT |
| [BunsDev/open-codex-computer-use](https://github.com/BunsDev/open-codex-computer-use) | `b753b79` 2026-06-11 | 359 文件 | MIT（iFurySt 早期分叉） |
| [opensymph/open-computer-use](https://github.com/opensymph/open-computer-use) | `5b433b9` 2026-08-26 | 202 文件 / 60k 行 | MIT（iFurySt 分叉，Go 原生化） |
| [zavora-ai/computer-use-mcp](https://github.com/zavora-ai/computer-use-mcp) | `1d054b4` 2026-10-13 | 306 文件 / 79k 行 | MIT |
| [CursorTouch/Windows-MCP](https://github.com/CursorTouch/Windows-MCP) | `8573729` 2026-10-13 | 169 文件 / 49k 行 | MIT |
| [trycua/cua](https://github.com/trycua/cua) `libs/cua-driver` | `57159eb` | Rust workspace | MIT（OmniParser 扩展 AGPL） |

**借鉴边界：** v2 做 clean-room 实现，借鉴行为与设计，不复制代码。若确需复制 MIT/Apache 代码片段，须在文件头保留原版权、
在 `computer-use/THIRD_PARTY_NOTICES.md` 登记，PR 描述写明来源。leeguooooo 无许可文件，**禁止复制任何代码**。
Codex 官方二进制与 `@oai/sky` 是专有代码，只引用公开行为与第三方逆向笔记中的**接口形状**，不反编译、不分发。

---

## 1. Codex Computer Use（官方）

来源：OpenAI 官方产品文档与帮助中心（经搜索摘要核对）、iFurySt `docs/references/codex-computer-use-reverse-engineering/`（对 1.0.755–1.0.770 的逆向笔记）、
Torrekie ventura（对 1.0.1000387/1000451 的接口适配）、leeguooooo（对 `@oai/sky` 的引导方式）。

### 1.1 产品形态

- **组件：** `Codex Computer Use.app`（`SkyComputerUseService`，权限/AX/截图/输入/光标）+ 内嵌 `SkyComputerUseClient.app`（`mcp` 与 `turn-ended` 子命令）。
  client 带 parent launch constraint，只能由 OpenAI 签名的宿主拉起。
- **接入：** 早期是 stdio MCP（官方 server 名 `computer-use`，因此我们的服务端不能同名）；**当前版本是 code-first**：宿主提供通用 `node_repl`，插件 skill 指导模型
  `globalThis.sky = (await import("@oai/sky")).sky`，然后在一段 JS 里连续调用 `sky.get_app_state / click / type_text …`。
  `sky` 对象有 10 个方法：`list_apps get_app_state click drag perform_secondary_action press_key scroll select_text set_value type_text`，以及 `target: "mac"`。
- **授权：** 首次使用某 App 时发起 elicitation：`Allow Computer Use to use "<App>"?`，`persist: ["session","always"]`，带 `riskLevel`；
  设置页"Always-allowed apps"可撤销。企业版可禁止保存"始终允许"；原生 App 按签名可执行识别，不按文件名。
- **系统权限：** macOS 需屏幕录制 + 辅助功能；与 App 级授权分开。Windows 不需额外系统权限。
- **平台行为：** macOS 后台操作，不抢焦点/鼠标；有"锁屏使用"（活动轮次中临时解锁显示）。**Windows 在活动桌面前台操作**，任务期间用户不能同时使用该桌面。
- **硬性禁止：** 不能操作终端类 App 和 Codex 自身（防止绕过 shell 沙箱）；不能以管理员身份认证或批准系统安全提示。
- **轮次生命周期：** `turn-ended` 通知用于清理软件光标与本轮状态。
- **软件光标：** 独立覆盖窗绘制，弹簧动力学（response 1.4、damping 0.9），不移动真实指针。

### 1.2 工具语义（来自真实调用样本）

- `list_apps`：运行中 + 14 天内用过，`Name — bundle.id [running, last-used=…, uses=N]`。
- `get_app_state {app}`：启动/复用会话，返回 `App=bundle (pid N)`、`Window: "...", App: ...`、缩进 AX 树（每行 `index role 名称 属性, Secondary Actions: …`）、
  `The focused UI element is N role.`，附窗口截图。官方描述要求"每个 assistant 轮次操作前调用一次"。
- 动作工具（`click/scroll/set_value/perform_secondary_action/...`）**返回动作后的相关子树文本 + 截图**，模型无需再观察。
- `scroll` 是元素级（`element_index` 必填），`pages` 支持小数。
- `click` 支持 `element_index` 或截图像素 `x/y`、`click_count`、`mouse_button`。
- 渲染管线变换名（字符串证据）：`filterActions associateTitleUIElements flattenIntoSelectableAncestor pruneNonDescriptiveSubtrees pruneEmptyDisabledElements mergeSingleItemGroups flattenRedundantHierarchy flattenRepetitiveStaticText flattenLinksIntoMarkdownText mergeTextOnlySiblings removeElementsUnderCalendarEvents retrieveAttributedTextFromTextAreas retrieveAttributedTextFromWebAreas`。
- **window2 API**（据 opensymph 引用的官方插件 `docs/api.md`）：`list_windows / get_window / launch_app / get_window_state / activate_window`；
  `Window = {app, id, title}`；坐标动作可带 `screenshotId` 绑定观察时刻，stale 即拒；任何成功动作使该窗口缓存的 screenshot id 失效；
  坐标动作前必须有带截图的观察、窗口 bounds 自观察后变化即拒、坐标必须落在截图范围内；右键双击拒绝；`type_text` 上限 8192 UTF-16。

### 1.3 后台点击路径（macOS，逆向结论）

`CGWindowListCopyWindowInfo` 找目标 layer-0 窗口 → `NSEvent.mouseEvent(windowNumber: CGWindowID)` → 取 `cgEvent`，设置 field 3/7/91/92 →
私有 `CGEventSetWindowLocation(event, localPoint)` → `CGEvent.postToPid(pid)`。键盘类似（SkyLight `SLPSPostEventRecordTo` 合成 active 状态）。
iFurySt 实测（Chrome 被完全遮挡）：`sky_click` 50/50 精确一次，返回 p50 82ms；`sky_key type_text` 50/50，返回 p50 30ms，前台 App 全程不变。

### 1.4 对 v2 的启示

1. 工具面就该是"App 名 + 元素序号"。2. 动作返回状态。3. 授权内联、三档持久化。4. code-first 是提速主路径。
5. macOS 后台非侵入是体验核心，但依赖私有 API，必须特性开关 + 运行时探测 + 回退。6. Windows 前台是行业现实，要诚实告知并配合用户接管机制。

---

## 2. iFurySt/open-codex-computer-use（"open-computer-use"，npm `open-computer-use`）

**是什么：** 目前最完整的 Codex CU 开源复刻。macOS 为 Swift（`OpenComputerUseKit`），Windows/Linux 为 Go（早期 PowerShell / Python AT-SPI 脚本），
npm 分发，`ocu` CLI，支持 Codex/Claude Code/Gemini/opencode/DeepSeek Harness 一键安装。2026-10 发布 1.0.0。

**架构要点：**
- macOS 的 CLI 通过 LaunchServices 拉起隐藏的 `Open Computer Use.app`（LSUIElement），经用户临时目录的 Unix socket 代理请求，
  **TCC 权限落在 .app 身份上**，与谁启动 MCP 无关。这是独立 MCP 在 macOS 上的正确解法。
- 9 个官方同名工具 + macOS `query`（有界 AX 查找，不截图，2 秒协作截止，索引 120 秒过期）。
- `click_method`: `auto | accessibility | app_post | sky_click | global`；显式模式不静默回退；`global` 需环境变量放行。
- `window_placement: agent_display`：把窗口停到 agent 拥有的隐形虚拟显示器上，被遮挡/其他 Space 的窗口也能完整渲染并接收输入，不碰用户的 Space/焦点/指针。
- `WindowOcclusionKeepAlive`：防止被遮挡窗口停止渲染。
- JS 编排：`ocu js` / `ocu repl` + 插件内 MCP 工具 `js`、`js_reset`，暴露 `cua.getApp(...)` 绑定对象（对齐官方 `node_repl` + `@oai/sky` 工作流）；
  Worker 边界可超时终止。
- 软件光标：逆向官方运动模型重建（弹簧参数、heading-driven 路径族）。
- 测试：Swift live tests（跨 Space、遮挡、后台输入基准）、fixture App、smoke/stress、agent-smoke（真实 Claude/Codex/Hermes 驱动 fixture）。
- 文档：大量 exec-plan/history（AI-first 仓库模板），逆向笔记质量高。

**亮点（借鉴）：** helper .app 承载 TCC；`query` 有界查找；`agent_display`；后台输入基准方法；JS 编排的 API 形状；agent-smoke 用真实 agent 驱动 fixture。
**问题：** Windows/Linux 早期是脚本（PowerShell 每次冷启动、Python gi 依赖），能力与 macOS 差距大；私有 SkyLight 路径版本敏感。

---

## 3. opensymph/open-computer-use（npm `@opensymph/open-computer-use`）

**是什么：** iFurySt 的分叉，重点是把 Windows/Linux 做成**单个 Go 可执行文件、零外部依赖**，并对齐官方 window2 API（14 工具）。

**架构要点：**
- Windows：进程内 UIA COM（专用 COM 线程）+ Win32（SendInput/PostMessage/GDI/激活序列）+ 纯 Go WGC/PrintWindow/GDI 截图链。
- **牺牲子进程隔离：** 实测 DWM/驱动对"刚移动的窗口 × native 读"存在异步越界写，会踩坏调用进程的堆。于是常驻 MCP 进程只做编解码与门禁，
  每次操作交给同 exe 的短命 `op` 子进程执行（崩溃/坏输出/超时重试 3 次），截图再隔离到孙进程。浸泡测试全程无 panic。
- **观察门禁（service 层 `bounds_gate.go`）：** 坐标动作前必须有带截图的观察；窗口 bounds 变化即拒；坐标须在截图范围；成功动作使 screenshot id 失效。
- 激活序列：`AttachThreadInput → AllowSetForegroundWindow → SetForegroundWindow → detach`，解决前台锁导致 SendInput 打错窗口。
- 拒绝名单：密码管理器 bundle、若干安全软件显示名。
- Linux：Go 进程内直连 AT-SPI2/D-Bus。
- 实测延迟（Windows）：`get_window` ~10ms、`list_windows` ~400ms、纯树 ~390ms、树+截图（WGC）~540ms、`type_text`（含刷新快照）~2.0s、`list_apps` ~3.5s（解析 149 个 .lnk）。
- 显示器级命令 `screenshot / cursor-position / input / record`，录屏带事件 sidecar 与后期"polish"。

**亮点（借鉴）：** 牺牲子进程隔离（v2 Windows 驱动采纳为可选执行模式）；window2 门禁规则；激活序列；性能数据作为基线。
**问题：** UIA 每次冷初始化 COM 300–400ms；Linux/macOS 的 window2 未实现。

---

## 4. OpenCodexLabs/open-codex-computer-use（Open Claudex）与 Torrekie 分叉

**是什么：** macOS-only 的 Swift MCP，强调"后台车道 + App 感知虚拟光标"。`0.1.0-alpha`。

**要点：**
- `DesktopSessionManager`：共享/独占会话、跨进程建议锁、动作预算、**基于前台 App/光标漂移的中断检测**。
- `Allowlist`：bundle id 白名单，`allowAll / allowlistOnly`，持久化，对所有变更动作强制。
- 兼容性分级表：Stable（Safari/Notes/TextEdit/Finder/设置）/ Limited（Chrome/Edge/VS Code/Slack，部分 AX、坐标回退、剪贴板输入）/
  Best-effort（微信、飞书/Lark 等自绘或 WebView 重度 App）。
- `type_text` 多策略：auto / pasteboard / direct。
- 支持 NDJSON 与 Content-Length 两种 stdio 帧。

**借鉴：** 兼容性分级表的写法（v2 在 [07 §6](07-quality-gates.md#6-app-兼容性矩阵) 采用）；中断检测思路；输入多策略。

---

## 5. Torrekie/codex-computer-use-ventura

**是什么：** 让官方 Codex CU skill 在 macOS 13 上运行：clean-room loader 按 Darwin 版本选择——14.4+ 委托原 wrapper，低版本用开源 Swift 后端经
"Sky-to-MCP 适配器"实现 10 方法 `sky` 对象。哈希门控安装、可逆、LaunchAgent 在插件更新后重新应用。

**关键信息：** 暴露了官方 `sky` 的完整方法面（含 `select_text`）、elicitation 的 meta 结构（`codex_approval_kind`、`persist`、`riskLevel`、`tool_params_display`）、
`codex/toolSurface` 响应元数据、"首次全量状态后返回紧凑行 diff"、截图有界存储与清理、子进程超时重启恢复、"每个 js 块都要幂等 bootstrap"（REPL 内核会重置）。

**借鉴：** 审批卡字段设计；diff 策略；脚本模式下"幂等引导"的约束（v2 的 `computer_script` 每次调用自动注入绑定，不依赖内核持久）。

---

## 6. leeguooooo/computer-use（已归档）

**是什么：** 把官方 Codex runtime 包装成 MCP（代理 `codex mcp-server`，注入 `import("@oai/sky")` 引导），对外只有粗粒度"交给 Codex 子 agent 做整件事"。
早期版本曾打补丁伪造 team id 绕过官方发送方认证，后移除。作者自己的 A/B：**细粒度工具直连比委托子 agent 在等价读取上快约 200 倍**，因此归档并推荐 open-computer-use。

**启示：** (1) 粗粒度委托体验差（无逐步截图、无光标、慢）；(2) 不要碰别人的签名/认证；(3) 子 agent 隔离只适合作为可选形态，主路径必须是细粒度工具。

---

## 7. TheGuyWithoutH/mac-computer-use（npm `mac-use`）

**是什么：** Node MCP + Swift native helper 复刻 Codex 工具面（macOS）。有 `RESULT_SCHEMA.md`：统一 `ComputerUseToolResult {ok, toolName, app, snapshot{treeText, elements}, artifacts{screenshot}, warnings, meta}`、错误码化。
TCC 归属于启动它的宿主（终端/Cursor），未做 helper .app。

**借鉴：** 结构化结果 + 文本渲染双轨（v2 `structuredContent` + `text` 同源生成）。**反面：** TCC 跟随宿主导致权限混乱，v2 路线 A 必须用 helper .app。

---

## 8. zavora-ai/computer-use-mcp（npm `@zavora-ai/computer-use-mcp`）

**是什么：** 功能最"宽"的跨平台 MCP（TypeScript + Rust napi 原生模块），7.6.0。默认 `desktop` profile 39 个工具，`full` 76 个。

**要点：**
- **工具目录预算：** 7.6 把默认 `tools/list` 从 112KB（~30k token）压到 46KB（~12k token），并以**字节预算测试**防回涨。合并 5 个 click 为一个、共享参数说明压到一句、`approval_token` 移出 schema 放 `_meta`。
- **用户活动守卫：** 被动 HID event tap 记录最后一次**物理**输入；4 秒内有用户输入时，会激活 App 或发前台输入的调用返回 `user_active`（除非 `force` 或可定向投递 `delivery:"pid"`）。读操作不受限。
- **状态卡：** macOS/Windows 桌面状态卡显示 agent 在做什么、带暂停/停止，**不出现在截图里**。
- **等待工具：** `wait_for_window / wait_for_text / wait_for_stable` 替代"sleep + 截图"循环。
- **OCR：** `read_window_text`（端侧 OCR）给自绘 UI；`click_text`。
- App 脚本：AppleScript/JXA、PowerShell/COM（Office 自动化）。
- `DesktopBroker`：owner 绑定会话、观察过期、移动窗口即拒、`expect` 条件；MCP Tasks 扩展（长任务）；MCP App 控制台（`ui://` 资源）。
- 审批：一次性 approval id（10 分钟过期、消费即失效、重启作废）。
- 诚实结果（R1：`delivery / route / effect / no_effect`）与 `capture_id` 绑定像素点击（R2）列为"仍计划中"——说明这是业内公认但难做的方向。
- 演示：canvas 发票只能靠截图读取，DeepSeek Flash 10/10；每次输入记录 `isTrusted`，非 UI 输入的提交会被拒——**外部判定**。

**借鉴：** 工具目录字节预算测试；用户活动守卫；不进截图的状态卡；三种 wait；OCR 作为 P10 可选；诚实结果的字段设计；外部判定。
**反面：** 76 个工具过宽，模型选择成本高；v2 默认面保持 ≤16 个工具。

---

## 9. CursorTouch/Windows-MCP（PyPI `windows-mcp`，Claude Desktop 扩展 200 万+ 用户）

**是什么：** Windows 上最流行的 MCP（Python + comtypes UIA）。支持 Win7–11、stdio/SSE/streamable-http、OAuth/IP 白名单/TLS。

**要点：**
- **桌面控制所有权：** 默认开启。AI 控制时屏幕边缘与光标发光；**用户大幅移动物理鼠标或按 `Ctrl+Alt+Shift+Backspace` 即收回控制权**，空闲 10 秒后 AI 才可继续；
  `ControlStatus` 随时可查 owner。
- **InputLedger：** 记录 AI 按下而未抬起的键/鼠标键，接管或故障时异步全部释放，防止"卡键"。周期性替换低级钩子以应对 Windows 静默移除钩子。
- `Screenshot`（快、不取树）与 `Snapshot`（树 + 可选视觉）分离；`region` 裁剪省 token；截图后在**截图之后**绘制橙红边框闪烁作为确认。
- **DOM 模式：** `use_dom=True` 只取网页内容（Chrome/Edge 的 UIA `RootWebArea`，Firefox 走 IAccessible2 回退）。
- 树预算 `WINDOWS_MCP_MAX_TREE_ELEMENTS=500`，超大列表提前截断并标注，避免拖死目标 App。
- 虚拟桌面识别（`IVirtualDesktopManager`），排除进程列表。
- 工具较宽（PowerShell、文件系统、注册表、进程、通知）——这些在 Grok 场景下应交给 shell/文件工具，不属于 CU。

**借鉴：** 接管机制 + InputLedger（v2 Windows/Linux 前台模式必备）；DOM 模式（P8 浏览器策略第一层）；树预算；虚拟桌面识别。

---

## 10. superagent-ai/grok-cli（社区 Grok CLI，非 xAI 官方）

**是什么：** Bun + OpenTUI 的 Grok API 编码 agent。CU 以**内置子 agent `computer`** 实现：工具 `computer_snapshot / screenshot / click / mouse_move / type / press / scroll / launch / list_windows / focus_window / wait / get`，
后端调用 npm `agent-desktop` CLI（JSON 输出，**仅 macOS**），引用为 `@e1` 风格稳定 ref，直到下一次 snapshot。

**启示：** (1) Grok 生态里"CU 作为子 agent 隔离上下文"的需求真实存在（截图很占上下文）；(2) `launch`/`focus_window`/`wait` 是常用补充；
(3) 只做 macOS 不够。v2 在 P10 提供"CU 子 agent"可选形态，但主路径是会话内细粒度工具。

---

## 11. bo-516/computer-use（摘要）

Grok Build 插件 + Python MCP 门面 + 外部 Cua Driver + 独立 Playwright MCP。亮点：`auto` 观察（树够用则不截图，否则 SoM 截图）、
动作后紧凑 `changes`、输入/输出 schema 与 annotations、60 个任务变体的外部判定评测（含 oracle 自证与 red-team）。
状态：测试基于 fake backend，Phase 0 关键假设（图像是否抵达模型、hook 字段等）未验证；nightly 因缺密钥未进入真实任务。

**借鉴：** auto 观察策略、变化摘要字段、评测方法论（v2 [07 §5](07-quality-gates.md#5-模型任务评测)）。

## 12. trycua/cua `cua-driver`（摘要）

Rust 三平台驱动 + MCP + SDK + C ABI。亮点：紧凑 Markdown 树与 `since:<snapshot_id>` 变化摘要、`run_steps`（顺序、预检、`wait_for`/`expect`、出错即停、末尾一次观察）、
`run_script`（QuickJS，只暴露驱动 API，墙钟/调用数/内存/脚本大小限制）、`ActionResult: confirmed/partial/unverifiable/suspected_noop/refused` + `verify_state`、
公开实机 CI（Windows interactive、Sway 等）、按桌面环境逐项记录能力缺口的支持台账。

**借鉴：** `run_steps` → v2 `computer_batch`；`run_script` → v2 `computer_script`；结果语义；能力台账写法。
**不直接依赖的原因：** 版本仍在 prerelease、其默认 standard 模式无提示、stale token 按 role/label 重新查找与可选前台回退与 v2 的"不越权、不静默换目标"冲突；
作为**可选对照驱动**在评测 C 组使用（见 [07 §5.4](07-quality-gates.md#54-ab-对照)）。

---

## 13. 横向对比

| 维度 | Codex | iFurySt | opensymph | Open Claudex | zavora | Windows-MCP | Cua | v1 | **v2 目标** |
|---|---|---|---|---|---|---|---|---|---|
| 平台 | mac/win | mac(强)/win/linux | mac/win(强)/linux | mac | mac/win/linux | win | mac/win/linux | win/mac/linux(未验) | mac/win/linux 同协议 |
| 语言 | Swift(闭源) | Swift+Go | Swift+Go | Swift | TS+Rust | Python | Rust | Rust+TS+Node | Rust 单二进制 |
| 工具数（默认） | 9→10(+window2) | 9(+query/js) | 14 | ~17 | 39 | ~18 | 多 | 17 | ≤16 |
| 授权 | 每 App 三档 | 无（依赖客户端） | 拒绝名单 | 白名单 | approval id | 无 | Host 回调 | 每任务/目标 | 每 App 三档 + 敏感动作 + 拒绝名单 |
| 动作后状态 | ✅ | ✅ | ✅ | ✅ | 部分 | ❌ | ✅(可选) | ❌ | ✅（diff） |
| 后台投递 | mac ✅ | mac ✅ | mac ✅ | mac ✅ | pid 投递 | ❌ | mac/win 部分 | win 部分 | mac ✅、win 部分、linux AT-SPI |
| 用户接管 | Stop | — | — | 漂移检测 | 活动守卫 | 物理输入接管 + 热键 | — | Pause/Stop | 活动守卫 + 热键 + Stop |
| 软件光标/状态 | ✅ | ✅ | ✅ | ✅ | 状态卡 | 边框发光 | — | 面板 | ✅ 光标 + 胶囊（不进截图） |
| 批量/脚本 | node_repl | js/repl | calls 序列 | — | wait/fill_form | MultiEdit | run_steps/run_script | ❌ | batch + script |
| 结果语义 | 文本 | 文本 | 文本 | 文本 | 计划中 | 文本 | 5 级 + verify | 4 级 | delivery × effect × verify |
| 外部判定评测 | 内部 | agent-smoke | e2e | 测试 | showcase | — | e2e+台账 | 零星 | 任务集 + nightly |
| 打包运行时 | 无 | 无 | 无 | 无 | 原生模块 | uv | 无 | Chromium+PW+Node | 无 |

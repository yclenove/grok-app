# Cua Driver 调研、Codex 体验目标与独立 MCP 边界

日期：2026-10-13。用户目标：研究 Cua Driver，优先参考用户喜欢的 Codex Computer Use 操作体验，同时评估把当前 CU MCP 独立成项目。

本轮完成源码与公开资料审查，没有编译、安装或启动 Cua Driver，没有运行付费模型任务，也没有修改 Grok App 的执行或授权行为。

## 1. 结论

1. Cua Driver 值得深入评估。它有 Rust 原生三平台驱动、MCP、SDK、紧凑观察、增量反馈、批量操作与受限脚本接口，价值远超一层截图/点击封装。
2. 先吸收模型接口和交互设计，再用同任务比较是否替换某个平台适配器。直接换驱动不能保证达到 Codex 的效果，也不能补齐模型视觉判断和任务规划能力。
3. 当前 MCP 可以成为独立项目，但应一起抽出可复用核心、原生适配和最小 Host。只复制 `server.mjs` 仍然依赖 Grok App，无法独立控制电脑。
4. 独立版与 App 版应该复用同一套核心及测试，避免维护两套逐渐分叉的执行、授权和清理逻辑。

## 2. 调研基线

| 对象 | 固定范围 |
|---|---|
| 仓库 | [trycua/cua](https://github.com/trycua/cua) 中的 `libs/cua-driver` |
| 提交 | `57159eb4b9d7ba1b8f28f03a4bce1c0b35d47283` |
| 本地源码 | `H:\aicoding\_research\trycua-cua`，稀疏检出驱动、文档及相关评测/CI 源码 |
| 源码版本字段 | `libs/cua-driver/rust/Cargo.toml` 的 workspace version 为 `0.35.1` |
| 发布边界 | 此提交的消息是计划 pin 到 `0.35.2`，并注明 `0.35.1` 发布门禁失败、未发布；不能按提交标题认定 `0.35.2` 已经发行 |
| 本次 release 列表所见 | `cua-driver-rs-v0.34.1` 等已发布条目标为 prerelease；这不是全历史稳定性结论 |
| 主驱动许可 | MIT |
| 可选 perception 扩展 | 独立许可边界；OmniParser 图标检测器为 AGPL-3.0-only，不随本建议默认引入 |
| Grok App 基线 | `feat/computer-use-implementation`，HEAD `c8217d08455999ed4205494e65600918c876cfd1` 及现有待提交启动修复 |

源码、规范、历史实机台账和已发布安装包是四类证据。以下不把其中一类的通过写成其余三类已经通过。

## 3. Cua Driver 实际是什么

其 Rust workspace 包含：

| 模块 | 作用 |
|---|---|
| `cua-driver` | CLI / MCP 入口和 daemon |
| `cua-driver-core` | 协议、工具、权限、session、观察投影与批量/脚本调度 |
| `platform-windows` | UIA、截图、输入、窗口与平台状态 |
| `platform-macos` | AX、截图、输入、后台投递和平台身份 |
| `platform-linux` | AT-SPI、X11、各 Wayland 环境的采集/输入 |
| `cua-driver-sdk` | Rust SDK、版本化 C ABI 之上的应用接口与生成绑定 |
| Python / TypeScript SDK | 应用集成入口，复用原生运行时 |
| `cua-driver-e2e` | 独立 GUI 夹具、平台行为和外部结果判定 |

Agent 可以直接接 `cua-driver mcp`。应用可以使用 SDK 或受控私有 worker。macOS 单独安装形态使用 `CuaDriver.app` 承担稳定的系统权限身份；嵌入形态要求 Host 的责任链和权限归属正确。

Cua 不替嵌入 App 绘制授权弹窗。SDK 提供 `DriverAuthorizationHost`，由可信宿主处理需要授权的资源边界；这适合与咱们现有 Host UI 对接，但不能假设其所有授权模型与咱们完全等价。

## 4. 相比咱们，值得吸收的能力

### 4.1 精简观察与增量状态

`window_state_view.rs` 默认给模型一份紧凑 Markdown 树，而非同时重复完整 JSON 和文字。它明确标注截断，支持 `since:<snapshot_id>` 变化摘要；旧快照不存在、变化太大等情况退回完整观察。

这与咱们已有控件树、`screenshot:false` 能力相容。优先改输出投影，比先引入第二套桌面驱动更容易测清收益。文字任务应保留静态说明、控件值和弹窗上下文；视觉任务应保留清晰截图及必要的局部放大。

### 4.2 一次调用完成短流程并检查结果

`run_steps` 不是简单并发点击：它按顺序调度现有工具，事先检查参数，逐步走原有权限/session 门禁，出错停止，并可在末尾返回一次观察。还支持 `wait_for` 和 `expect`，报告失败发生在等待、定位、执行还是检查阶段。

`run_script` 用 QuickJS 执行模型写的 JavaScript，只暴露 Cua 工具 API；没有文件、网络、进程或环境访问。源码有墙钟时间、调用数、内存、脚本大小和取消约束。脚本中的每次驱动调用仍走同一工具注册表与权限检查。

它能把“填三项 → 点击 → 等待结果 → 检查”放在一次模型调用内，减少已知操作之间的往返。这是值得借鉴的方向，但没有本轮实测的速度数字。

必须区分：驱动 session 持续存在，不等于这个 QuickJS 接口已证明跨脚本保留变量。不能把它直接宣传成与 Codex 内部持久 REPL 相同。

咱们可先增加受控短流程接口，再决定是否增加脚本宿主。每一步仍应保留 actionId、授权、取消、身份复查和结果账本；后续目标变化需要显式的新观察，不能把旧 refs 自动重新绑定成新的执行依据。

### 4.3 原生后台操作经验

Windows 的源码区分后台 UIA / PostMessage 和前台 `SendInput`。后台能力不足时返回明确拒绝；前台模式有焦点变化，不能把“支持 Windows”解释成“所有 Windows 程序都能后台操作”。

macOS 有 AX 和针对目标进程的输入路径，并使用 SkyLight 私有接口改善部分 Chromium/Catalyst 输入接受情况。这比只用公共 CGEvent 的兼容性工作更深入，也意味着需要随系统版本维护和实际验证，不能原样移入就视为长期兼容。

Linux 按 X11、Sway、GNOME、KDE 等环境分别记录能力。台账明确保留后台、光标保持、IME、compositor 输入等缺项，没有一种统一的“Wayland 都支持”结论。

### 4.4 动作事实和业务结果分开

`ActionResult` 区分 `confirmed / partial / unverifiable / suspected_noop / refused`，另用 `verify_state` 表达指定后置条件 `satisfied / unsatisfied / unknown`。

这与咱们当前“传输成功、动作执行、业务验证分别判断”的方向一致。若增加 Cua 适配器，必须做明确的结果映射：不能把 Cua 的动作 `confirmed` 直接映射为整个用户任务完成。

## 5. Codex 的哪些效果可以参考

依据本次实际读取的 [OpenAI 官方 Computer Use 文档](https://learn.chatgpt.com/docs/computer-use)、[浏览器文档](https://learn.chatgpt.com/docs/browser)、[浏览器扩展文档](https://learn.chatgpt.com/docs/chrome-extension)，以及 [Computer Use API 指南](https://developers.openai.com/api/docs/guides/tools-computer-use)。

| 体验目标 | 公开证据能确认什么 | 对咱们的设计建议 |
|---|---|---|
| 说任务后自然开始 | 官方产品文档支持按任务指定 app / window / flow；在任务需要使用应用时请求访问 | 在真正需要新增范围时请求，给用户清楚的目标与用途 |
| 少而明确的授权 | 可选择 Always allow，并在设置撤销；敏感/干扰性动作仍可能单独确认 | 可评估记住应用级选择；执行时仍绑定当前目标/任务，不能永久绕过所有门禁 |
| 用合适的操作入口 | 有独立内置浏览器、已有浏览器扩展和专用集成；专用集成优先 | 统一入口，按任务和已授权范围选择能力，失败时明确交接 |
| 连贯操作，减少重复往返 | API 指南推荐保持环境状态，支持脚本组合操作、循环和条件；短组动作后返回观察 | 提供受限流程/SDK 对象接口，保留本地执行状态和每步检查 |
| 需要看图时能看清楚 | API 指南要求 UI 状态未知时给当前截图，保留原始分辨率或正确映射缩放坐标 | 把实际传入模型的图像、尺寸和坐标映射作为验收项 |
| 用户知道正在做什么 | 产品有 app 访问控制、停止/接管与任务内确认 | 统一任务状态、当前目标、最近动作和接管按钮，避免多个重复授权窗口 |

注意证据边界：API 指南讲的是开发者可构建的集成方法，不能据此断言 Codex 桌面内部采用完全相同的运行时。公开资料没有确认 Codex 内置 Computer Use 使用 Cua Driver。Cua 文档展示“Codex 连接 Cua MCP”是兼容性证据，不是 Codex 默认底层实现证据。

模型的视觉判断、中文指令理解、规划和工具使用也影响结果。即使两边使用同一驱动，效果仍可能不同。需要把驱动因素与模型/工具接口因素分别比较。

官方产品文档还明确：Windows 在活动桌面前台操作；macOS 的后台能力和权限机制不能直接类推到 Windows。用户喜欢的操作效果不应被写成平台做不到的承诺。

## 6. 成熟度核查

与 bo-516 插件的 fake-backend 状态不同，Cua Driver 有具体原生行为台账和公开实机 CI。已抽查：

- [Windows interactive run 29257963004](https://github.com/trycua/cua/actions/runs/29257963004) 整体 success，提交 `64e824490b6a796102a46fcece518a9d6c32c145`。台账记录 122 个已声明结果，包含 99 次实际投递和 23 次准确拒绝，不能全算成功执行。
- [Sway run 29255208927](https://github.com/trycua/cua/actions/runs/29255208927) 整体 failure；capture 和 native job 成功，shared job 失败。
- [后续 Sway run 29257961614](https://github.com/trycua/cua/actions/runs/29257961614) 整体 success。台账组合引用前一轮 native/capture 与后一轮 shared；不能抹掉前一轮整体失败，也不能当成同一最终候选的全套验证。
- 当前固定源码的 installer canary 通过，只证明安装入口检查，不等于当前源码的三平台模型任务已经通过。

台账对部分 macOS、GNOME、KDE 和实验 nested compositor 行为仍有明确缺项。本轮没有独立重跑这些工况，因此不将其历史记录晋升为本机复现或最新包验收。

## 7. 直接接入前要确认的边界

1. **版本与包**：选定实际已发布或明确 source-built 的候选，固定源码、二进制 hash 和 SDK；不把 main 的声明当发行包行为。
2. **Host 授权**：保留咱们的执行门禁。Cua 默认 standard 是面向普通自动化的无提示模式；不能将这个默认行为直接暴露给模型。其 `session_authorization.rs` 也明确当前 daemon 委托 session 创建仍有限制。
3. **原目标绑定**：把 PID / window identity、capture / geometry 与咱们 task/run/generation 对齐。按 app 名或控件名重新查找不是原目标身份的替代品。
4. **重试**：Cua 批量工具支持 stale token 按 role/label 重新查找，以及可选前台 fallback。咱们不能直接继承这些默认或选项：unknown 不重放，未授权目标不重选，焦点扩大需要符合原范围。
5. **生命周期**：独立验证暂停、停止、取消、Host 退出、driver 崩溃及重连；调用 future 结束不等于原生输入 owner 已退出，不能把对方取消接口当作咱们已有清理证明。
6. **macOS 责任链**：TCC 归属、签名、helper 及最低系统要求需按实际集成验证，不自动改变 App 11+ / 受管浏览器 14+ 的既定边界。
7. **许可与资源**：主驱动 MIT 可评估复用并保留通知；可选 AGPL perception 与模型下载不默认打包。常规驱动不需要额外视觉模型权重。

## 8. 咱们独立 MCP 项目的可行结构

当前事实：

- [computer-use-core](../../src-tauri/computer-use-core/Cargo.toml) 已是独立 Rust crate，没有 Tauri 依赖；授权、协议、IPC、Broker、runtime 与部分 browser 逻辑已在这里。
- [server.mjs](../../tools/computer-use-mcp/server.mjs) 是 Node stdio 门面；启动时必须取得 Host 提供的 loopback 地址、token 和 session。它不是独立桌面引擎。
- Windows/macOS 原生适配以及部分生命周期、目标选择和权限 UI 仍在 App 层；X11/Wayland 已有独立 crate，但产品接线仍依赖 Host。
- `cu-probe` 依赖 `grok-app`，不能当作已完成的独立生产 Host。

建议结构是逻辑边界，尚未创建这些包：

| 部件 | 独立版职责 | App 如何复用 |
|---|---|---|
| Core | 协议、授权、身份、账本、取消与清理合同 | 直接引用同一库 |
| Native / Browser adapters | 各平台原生操作及浏览器后端；可选实验 Cua adapter | 复用同一实现和夹具 |
| Host | runtime、会话、用户授权 provider、目标选择、状态与退出 | App 提供现有 UI provider；独立版提供轻量本地交互 |
| MCP / SDK | 标准 agent 接入、对象/短流程 API、紧凑观察 | App 和其他 MCP 客户端使用同一协议 |

App WebView 应是可选 Host 扩展：独立进程无法天然取得任意第三方 App 的内部 WebView 控制权。已有标签页也仍需要明确的浏览器配对/授权。

先在当前仓库内完成依赖拆分与两个入口的同合同测试，再决定另开仓库和独立发布。这样可保持现有 App 的修复、原生行为和独立版同步，不急于复制一份代码长期维护。

## 9. 下一步建议

建议先形成小规模、可解释的比较，而不是立即重写：

| 组别 | 驱动 | 模型接口 | 要回答的问题 |
|---|---|---|---|
| A | 当前原生实现 | 当前单步工具 | 基线到底慢/失败在哪 |
| B | 当前原生实现 | 紧凑观察 + 受控短流程 | 减少往返能否改善体验 |
| C | Cua 候选适配器 | 与 B 相同的 Host 和接口 | 原生驱动更换是否另有收益 |

保持模型、effort、提示词、任务、初态及机器环境一致。先用中文输入与读回、表单/弹窗、窗口缩放/滚动等自有夹具；同时验证用户输入接管、目标关闭、拒绝授权、暂停/停止和 unknown。浏览器任务单独报告，避免把网页定位收益混入原生驱动对比。

记录业务成功率、总耗时、模型往返、工具次数、图像/token、误报成功、额外授权次数和原 owner 退出时间。最终三平台分别执行；本机先诊断不意味着把首版改成 Windows-only。

如果 B 已获得主要改善，优先交付接口与 UX。只有 C 在明确工况更好且满足现有授权/生命周期合同，才考虑替换对应适配器。独立 MCP 的拆分可复用这次形成的边界，但不是当前 App 可交付的前置重构。

## 10. 证据索引与本轮产物

第三方源码链接固定到本次审查提交：

- [Driver README / SDK / permission modes](https://github.com/trycua/cua/blob/57159eb4b9d7ba1b8f28f03a4bce1c0b35d47283/libs/cua-driver/README.md)
- [精简观察与 diff](https://github.com/trycua/cua/blob/57159eb4b9d7ba1b8f28f03a4bce1c0b35d47283/libs/cua-driver/rust/crates/cua-driver-core/src/window_state_view.rs)
- [批量操作](https://github.com/trycua/cua/blob/57159eb4b9d7ba1b8f28f03a4bce1c0b35d47283/libs/cua-driver/rust/crates/cua-driver-core/src/batch_tools.rs)
- [受限脚本](https://github.com/trycua/cua/blob/57159eb4b9d7ba1b8f28f03a4bce1c0b35d47283/libs/cua-driver/rust/crates/cua-driver-core/src/script_tool.rs)
- [可信 Host 授权回调](https://github.com/trycua/cua/blob/57159eb4b9d7ba1b8f28f03a4bce1c0b35d47283/libs/cua-driver/rust/crates/cua-driver-sdk/src/authorization_host.rs)
- [session 授权限制](https://github.com/trycua/cua/blob/57159eb4b9d7ba1b8f28f03a4bce1c0b35d47283/libs/cua-driver/rust/crates/cua-driver-core/src/session_authorization.rs)
- [动作结果合同](https://github.com/trycua/cua/blob/57159eb4b9d7ba1b8f28f03a4bce1c0b35d47283/libs/cua-driver/docs/action-result-contract.md)
- [实机支持台账](https://github.com/trycua/cua/blob/57159eb4b9d7ba1b8f28f03a4bce1c0b35d47283/libs/cua-driver/docs/action-support.md)
- [macOS 后台输入细节](https://github.com/trycua/cua/blob/57159eb4b9d7ba1b8f28f03a4bce1c0b35d47283/libs/cua-driver/rust/crates/platform-macos/src/input/skylight.rs)

本轮只保留研究源码与 Git 对象，测得逻辑大小 45,925,570 bytes（约 43.8 MiB）；不是实际磁盘占用或回收值。没有安装依赖、模型权重或生成构建产物，可清理构建项为 0，删除 0，实际回收 0。研究 Git 下载已退出；原 App/test/用户浏览器没有启动或停止。

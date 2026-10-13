# Grok Computer Use v2 — 总方案（索引）

版本：**v1.1**（2026-10-13，总控批准的三份独立评审修订；非实现接口的自行变更）。
逐条决定见 [评审处置表](reviews/2026-10-13-disposition.md)，执行基线为 `origin/feat/cu-v2`（P0-01 已合入）。

日期：2026-10-13 · 基线：`feat/computer-use-implementation` @ `5455eff9` · 作者：Claude（受 SevenCalabash 委托）

> 本目录是 Computer Use（下称 CU）**第二代**的唯一权威计划。它允许并且确实做了推翻性设计。
> 旧分支 `feat/computer-use-implementation` 上的 404 份 `docs/plans/*computer-use*` 与 `docs/llm-wiki/computer-use.md`
> 的检查点日志，自本目录生效起**只作历史参考**，不再续写。

## 一句话结论

v1 用 ~32 万行代码、404 份计划文档、57 个提交构建了一套"任务授权 + 四类 surface + 自带 Chromium 运行时"的系统，
至今状态仍是 *active / partial — not releasable*。根因不是工程量不够，而是**产品模型与模型接口选错了方向**：
模型每步要复制 8 个身份字段、观察只给 64 个节点、授权要"结束本轮等批准"、还要为自带 Chromium 做 NOTICE 审计。

v2 改为对齐 Codex Computer Use 已被验证的形态：**按 App 授权、服务端持有身份、每次动作直接返回新状态、
无障碍树优先 + 截图兜底、后台非侵入投递优先**。在这个基础上再做 Codex 没开源但我们可以做得更好的部分：
三平台同一协议、批量/脚本执行、诚实的结果语义、外部判定的任务评测。

## 两条路线（共享同一核心）

| | 路线 A：独立 MCP（`grok-computer-use` / `gcu`） | 路线 B：集成进 Grok App |
|---|---|---|
| 形态 | 单个跨平台可执行文件 + macOS 权限 helper `.app`；`gcu mcp` 接任意 MCP 客户端 | **松耦合**：App 随包携带并托管同一个 `gcu` 引擎进程（sidecar），双方只通过版本化的 **Host 协议**通信；App 不链接 core/驱动，只依赖协议类型 |
| 授权 | MCP elicitation 优先，回退到 helper 自带的审批小窗 | 聊天流内联审批卡（与 ACP 权限卡同一套 UI），设置页管理"始终允许" |
| 可视化 | helper 绘制软件光标 + 状态胶囊（不进截图） | App 透明覆盖窗绘制，任务面板展示步骤/截图/停止 |
| 适用 | Grok CLI 终端用户、Codex/Claude Code/Gemini/opencode 等任意客户端 | Grok App 用户，开箱即用 |
| 详见 | [04-route-a-standalone-mcp.md](04-route-a-standalone-mcp.md) | [05-route-b-integrated.md](05-route-b-integrated.md) |

**推荐：两条都做，A 先行。** 原因：A 的形态（stdio MCP + 任意客户端）天然就是核心与驱动的最佳测试夹具，
B 只是给同一核心换一个 Host（审批/覆盖层/事件）。顺序与理由见 [03-architecture.md §9](03-architecture.md#9-为什么-a-先行)。

## 文档清单

| # | 文档 | 内容 | 读者 |
|---|---|---|---|
| 01 | [01-review-v1.md](01-review-v1.md) | 对当前实现的完整 review：做对了什么、错在哪、数据、可复用资产清单 | 决策者、Grok |
| 02 | [02-research.md](02-research.md) | 12 个仓库 + Codex 官方 CU 的调研：每个的结构、亮点、坑、可借鉴点、许可 | 设计者、Grok |
| 03 | [03-architecture.md](03-architecture.md) | v2 目标架构：crate 划分、Driver trait、节点模型、渲染管线、协议 v2、安全模型、生命周期 | 全体（核心） |
| 04 | [04-route-a-standalone-mcp.md](04-route-a-standalone-mcp.md) | 路线 A 产品与工程细节：CLI、helper、安装、审批、分发、签名 | Grok |
| 05 | [05-route-b-integrated.md](05-route-b-integrated.md) | 路线 B：App 内 Host、注入、UI、设置、i18n、迁移 v1 | Grok |
| 06 | [06-roadmap.md](06-roadmap.md) | 分阶段路线图（P0–P10）+ **任务卡**（ID、文件、接口、步骤、测试、DoD） | Grok（执行主文档） |
| 07 | [07-quality-gates.md](07-quality-gates.md) | 测试金字塔、夹具、CI 矩阵、模型评测、性能预算、DoD、禁止事项 | Grok、Reviewer |
| 08 | [08-grok-execution-protocol.md](08-grok-execution-protocol.md) | 给 Grok 的工作规程：怎么领卡、怎么提交、怎么汇报、何时停、禁止重演 v1 的反模式 | Grok |
| 09 | [STATUS.md](STATUS.md) | **唯一**进度表（每个 PR 更新一行，不写检查点散文） | 全体 |

## 不可动摇的设计原则（v2 宪法）

1. **模型只说意图，服务端持有身份。** 模型传 `app` + `element_index`（或截图坐标）；snapshot、generation、窗口身份、
   几何版本全部由服务端记账和校验。校验失败返回**错误 + 新状态**，而不是要求模型复制更多字段。
2. **每个动作都返回动作后的状态。** 同一轮内第二次起返回 diff；需要视觉时附截图。不为"确认一下"多一次往返。
3. **无障碍优先，像素兜底，前台输入最后。** 投递路径（AX/UIA/AT-SPI → 定向进程事件 → 前台输入）在结果里如实报告。
4. **按 App 授权，三档：本轮 / 本会话 / 始终允许（另有拒绝）。** 终端、Grok 自身、密码管理器、系统安全提示永远拒绝。
   敏感动作（发送、支付、删除、提交密码）单独确认；网页区默认 strict，未知语义动作保守确认。
   Stop 锁存，只有用户在可信 UI 或 `gcu resume` 显式恢复；截图从首次交付起先脱敏。
5. **不重放未知结果。** `unknown` 的副作用动作永不自动重试；模型拿到的是新状态，由它决定。
6. **不随包携带第三方大型运行时。** 不打包 Chromium / Playwright / Node。浏览器通过无障碍树、可选扩展或 App 内置 WebView 控制。
7. **三平台同一协议，平台差异诚实暴露。** 能力在 `capabilities` 中声明；不支持就明确拒绝，不伪装。
8. **证据在代码里。** 测试、夹具、CI 链接、评测报告是证据；散文检查点不是。进度只写 [STATUS.md](STATUS.md)。
9. **小步可发布。** 每张任务卡要么完整满足 DoD 并合入，要么不合入；不存在"partial 合入"。未完成的纵向能力藏在 feature flag 后面。

## 给 Grok 的入口

1. 先读 [08-grok-execution-protocol.md](08-grok-execution-protocol.md)（工作规程，≤10 分钟）。
2. 再读 [03-architecture.md](03-architecture.md)（不读完不准写代码）。
3. 打开 [06-roadmap.md](06-roadmap.md)，从 [STATUS.md](STATUS.md) 里第一张 `todo` 且依赖已满足的卡开始。

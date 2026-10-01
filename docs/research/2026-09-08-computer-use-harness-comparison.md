# Computer Use 补充调研：Codex、Pi、DeepSeek Harness、Claude Code

日期：2026-09-08。状态：已完成下述范围的文档与定向源码核对；没有运行这些产品的 Computer Use 对比测试，不是四套仓库的全量审计。

用户明确：DSH 指 [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)。Pi 按 Pi coding agent 核对；原 `badlogic/pi-mono` GitHub 地址目前重定向至 `earendil-works/pi`。

上一版 [基础调研](2026-09-08-computer-use.md)已参考 Codex 官方 Computer Use、Grok Build、Cua 与 Playwright，但没有系统对照 Pi、DSH、Claude Code。本文补足框架层，并将结论写回 [主方案](../plans/2026-09-08-computer-use.md)，不将此次新增研究说成上一版已完成。

DSH 的社区执行器和交互插件另见 [第三方插件调研](2026-09-08-dsh-third-party-plugins.md)，覆盖 12 个项目。框架扩展契约与插件实际执行能力分开核验；尤其不要将外层取消、提示词防重试和插件级 session 归属视为完整的停止、幂等与会话授权保证。

## 1. 对照范围与证据

| 对象 | 核对材料 | 当前证据层级 |
| --- | --- | --- |
| Codex | 官方 Computer Use 产品/API 指南，本机插件工具接口 | 官方产品行为和公开接口；没有审计完整 Codex 引擎源码，也未确认私有桌面驱动可再分发 |
| Pi | `earendil-works/pi`，SHA `c1d4c801114545f47c440921d8b3e04aeb1e565d`；agent-core README、`agent-loop.ts`、`types.ts`、扩展/会话文档 | 定向源码与契约，根仓库 MIT；未证明具备可直接打包的三平台桌面驱动 |
| DSH | `deepseek-ai/deepseek-harness`，SHA `b0a7d2ce3b4c19d7452e364b2d7acbfa87e707ed`；架构、工具执行流程、tools 源码、projection、权限和事故复盘 | 定向源码与契约，根 LICENSE 为 MIT；官方明确 developer preview、存在破坏兼容的变更 |
| Claude Code | 官方 Computer Use CLI、Chrome、permissions 和 hooks 文档 | 官方产品行为/扩展契约；没有把产品说明当成可复制的开源执行器 |

## 2. 值得采用的机制

| 设计问题 | 参考证据 | Grok App 决策 |
| --- | --- | --- |
| 桌面入口怎么做 | Codex 的应用/窗口授权、观察和接管 [C1] | 复用现有 Composer 与资源面板，应用权限独立于代码文件权限 |
| 何时用桌面工具 | Claude Code 优先结构化 MCP、合适的 shell/浏览器能力，GUI 是兜底 [C2] | 在同一用户授权范围内选最直接的执行路径；不能因换成 shell/API 就扩大权限 |
| 模型结果与 UI 数据如何分离 | Pi `AgentToolResult.content` 是给模型的 text/image；`details` 用于日志/UI [P2]；DSH canonical value 与 render/presentation 分离 [D2] | 一个受校验的结果，分别产生模型观察与 UI 展示；不把 UI 进度、诊断日志和所有截图重复塞给模型 |
| 可扩展但不让插件越权 | DSH pre/guard/around/post/result 管线，guard 只能拒绝或不介入 [D2][D3] | 提示/展示可扩展，最终目标/权限/取消检查固定在 Broker，插件无法撤销已有拒绝 |
| 多工具调用怎么调度 | Pi 支持顺序/并行及 per-tool executionMode [P1][P2]；DSH 并发须显式声明安全 [D2] | 桌面写操作默认 exclusive，单次前台任务串行；只读跨独立资源可审慎并行 |
| 取消何时算完成 | Pi 向 tool/hook 传 AbortSignal；DSH 要求执行方在取消后达到 quiescence [P2][D2] | UI 接到停止不等于 driver 已停止；分 `stop_requested` / `stopped`，等待自有 worker 静止并拒绝迟到回调 |
| 长任务与历史恢复 | Pi tree session/上下文转换 [P3][P4]；DSH durable facts 与 live events/projection 分离 [D1][D4] | 只恢复任务目标与已验证事实；旧 window/tab handles、截图、授权租约、待执行动作一律不恢复 |
| 代码批量操作 | Codex 公开指南支持持久代码执行；DSH PTC 子调用也经过相同管线 [C3][D3] | 首版使用有界 typed action sequence；以后只有每个子动作同样受管控时才考虑代码执行 |
| 产品权限体验 | Claude Code per-app、机器互斥、Esc 停止；Chrome 以会话 tab group 和站点权限组织 [C2][C4] | 全局可见的占用指示、明确授权、人工接管；浏览器 profile、tab 与 origin 三者分别验证 |

## 3. Pi：轻量核心可借鉴，权限不能照搬

在所核对的源码版本中，`agent-loop.ts` 根据全局 `toolExecution` 和工具 `executionMode` 选择执行方式；任一调用要求 `sequential`，整个 batch 顺序运行。`AgentTool` 还声明 `replay: "never" | "safe"`，用于区分结果未知时能否重做。[P1][P2]

对 Grok App 的直接启发：调度与重试属性应该由 Host 的工具定义决定，不能让模型在参数里填 `safe=true`。桌面输入统一 `exclusive / replay=never`，超时结果先观察，不能自动重放。

Pi 的 `transformContext` → `convertToLlm` 与 `content/details` 能减少上下文负担；会话树能保留 fork 来源。不过 Grok App 目前使用外部 Grok Build ACP，**没有因此获得 Pi 的每次模型请求前 hook**。先验证 Grok 的真实扩展点；做不到时只能控制本次工具输出大小及失效提示，不能宣称能删掉 CLI 已积累的历史截图。[P1][P3]

不照搬的部分：Pi README 明确没有内置权限系统，扩展以宿主用户权限运行；其核心也刻意不内建 MCP，可通过扩展实现。我们保留 Grok 已有 MCP 接入，不为了模仿 Pi 改写全部主循环。[P5][P6]

Pi `tool_call` 扩展可修改参数，文档明确修改后不再次校验。这是接入桌面操作时需要主动补强的点：Grok Broker 在所有转换之后重新 Schema 校验、校验目标和权限，再冻结传给执行器的请求；这是我们的设计要求，不称为 Pi 已有保证。[P3]

## 4. DSH：借鉴能力模块与执行流程，不移植整个运行时

DSH 基于 Cordis，把模型、工具、会话日志和 agent loop 都放进插件组合。服务接口、实现 provider、工具 consumer 分开；注册随插件卸载回收。这适合参考为 `ComputerUseBackend` + driver provider + MCP consumer，而不是引入整套 Cordis 替换当前 Tauri/Rust Host。[D1]

其工具注册器明确区分：执行前扩展、不可被放宽的 guards、超时等 around 包装、结果转换和最终结果事件；源码保护调用身份并组合取消信号，避免 wrapper 替换信号后断开原调用的取消。[D2][D3][D5]

落到我们这里：

1. 驱动启停、订阅和句柄都绑定 backend generation；升级/切换/卸载先停止任务，旧代际回调不修改新任务。
2. 扩展只允许改变指定字段。工具名、run、target 和授权代际不得通过中间件修改；任何参数转换必须重新校验。
3. 原始执行状态与最终模型内容分离。展示钩子可以精简文字，不能把 `rejected/unknown` 改成 `verified`。
4. MCP、UI 直接调用、未来批处理都进入同一 Broker；不能只有模型直调路径受保护。

DSH 的“model-visible means logged”强调可恢复性，但不适合直接套到个人桌面截图：我们默认只保存有限元数据与可解释的结果事实，像素默认不持久保存。因此不承诺离线精确重建模型看到的全部画面；启用证据保存后也只保留授权范围。[D1]

另一个值得吸取的教训来自 DSH [事故复盘][D6]：工具没有注册，snapshot 测试却因把失败输出更新成 expected 而全绿。P0 必须断言实际 tools/list、可执行能力、模型能看到的工具集合和实际结果；不能通过更新快照把 `UNKNOWN_TOOL` 接受为成功。

DSH 的 permission preset 组合 sandbox/approval，但自身不负责执行。UI 选项不是能力证明。我们也必须由 backend capability report 决定可用项，不能通过一个“允许全部”选项假装支持 Wayland/UIPI 不具备的能力。[D7]

## 5. Claude Code：借鉴成熟交互，明确产品差异

官方 CLI 文档当前说明：Computer Use 为交互式 macOS research preview，内置 MCP 默认关闭，应用需按会话授权；Desktop 的平台范围与 CLI 不同。我们不引用其 CLI 限制去断言整个 Claude 产品不支持 Windows，也不借它证明 Linux 已支持。[C2]

文档还说明其 CLI 一次只允许一个会话控制机器，锁保持到会话退出；隐藏其他应用、排除当前终端截图、Esc 中止和截图缩放也有明确行为。值得采用的是“可见占用、独立停止、避免看到自己授权界面”的机制；**不照抄一直占锁到会话退出**：Grok App 常有多个长期会话，我们按 active Computer Use run 持有输入租约，任务停止并静止后释放；浏览器 profile 生命周期另行维护。[C2]

Grok App 新增宿主隔离要求：自身授权窗口、密钥输入、终端和 Computer 控制面默认不成为操作目标，也不进入给模型的截图。测试 Grok App 自身时用单独 dev 实例，保留外部停止控制，防止 agent 点击自己的批准按钮。截图中无法排除控制面时暂停采集，不能仅凭模型承诺忽略。

Claude 权限文档说明规则由 Host 执行，优先级为 deny → ask → allow，prompt 不能修改权限；Chrome 则有站点范围，文件上传受文件读取权限约束。我们的具体策略可不同，但 UI 授权必须与实际文件、origin、目标操作边界一致。[C4][C5]

不照搬自动隐藏其他应用、特定 app 分类限制、套餐/账户条件或浏览器私有协议。这些是它的产品选择，不是 Grok App 的技术前提；也没有证据表明其捆绑驱动可直接再分发。

## 6. 对主方案的修改

主架构继续采用 Grok Build + App Broker + Cua/Playwright，但 Broker 从简单路由层明确为 **统一操作运行时**：

- 声明式 backend/tool capabilities、顺序与重试属性。
- 固定校验/授权/取消管线；插件只能缩小范围，不能自我批准。
- 模型观察与 UI/本地记录分别投影，限制历史截图和节点负担。
- 任务恢复重建事实、重新观察；不重放外部桌面副作用。
- 两阶段停止、驱动静止确认、迟到回调隔离。
- 宿主授权界面隔离、真实工具注册与失败语义验收。

这些修改并入现有 P0/P1/P2/P8，不额外开启“把 Pi/DSH 接成第二主引擎”的大重构，也不增加无界脚本执行能力。三个系统首版一起交付的要求保持不变。

## 来源

[C1]: https://learn.chatgpt.com/docs/computer-use
[C2]: https://code.claude.com/docs/en/computer-use
[C3]: https://developers.openai.com/api/docs/guides/tools-computer-use
[C4]: https://code.claude.com/docs/en/chrome
[C5]: https://code.claude.com/docs/en/permissions
[P1]: https://github.com/earendil-works/pi/blob/c1d4c801114545f47c440921d8b3e04aeb1e565d/packages/agent/src/agent-loop.ts
[P2]: https://github.com/earendil-works/pi/blob/c1d4c801114545f47c440921d8b3e04aeb1e565d/packages/agent/src/types.ts
[P3]: https://github.com/earendil-works/pi/blob/c1d4c801114545f47c440921d8b3e04aeb1e565d/packages/coding-agent/docs/extensions.md
[P4]: https://github.com/earendil-works/pi/blob/c1d4c801114545f47c440921d8b3e04aeb1e565d/packages/coding-agent/docs/session-format.md
[P5]: https://github.com/earendil-works/pi/blob/c1d4c801114545f47c440921d8b3e04aeb1e565d/README.md
[P6]: https://github.com/earendil-works/pi/blob/c1d4c801114545f47c440921d8b3e04aeb1e565d/packages/coding-agent/README.md
[D1]: https://github.com/deepseek-ai/deepseek-harness/blob/b0a7d2ce3b4c19d7452e364b2d7acbfa87e707ed/docs/architecture.md
[D2]: https://github.com/deepseek-ai/deepseek-harness/blob/b0a7d2ce3b4c19d7452e364b2d7acbfa87e707ed/docs/subsystems/tools.md
[D3]: https://github.com/deepseek-ai/deepseek-harness/blob/b0a7d2ce3b4c19d7452e364b2d7acbfa87e707ed/docs/tool-execution-pipeline.md
[D4]: https://github.com/deepseek-ai/deepseek-harness/blob/b0a7d2ce3b4c19d7452e364b2d7acbfa87e707ed/docs/subsystems/session-projection.md
[D5]: https://github.com/deepseek-ai/deepseek-harness/blob/b0a7d2ce3b4c19d7452e364b2d7acbfa87e707ed/packages/core/tools/src/index.ts
[D6]: https://github.com/deepseek-ai/deepseek-harness/blob/b0a7d2ce3b4c19d7452e364b2d7acbfa87e707ed/docs/postmortem/0002-js-expression-disabled-filesystem-tools.md
[D7]: https://github.com/deepseek-ai/deepseek-harness/blob/b0a7d2ce3b4c19d7452e364b2d7acbfa87e707ed/docs/subsystems/permission-presets.md

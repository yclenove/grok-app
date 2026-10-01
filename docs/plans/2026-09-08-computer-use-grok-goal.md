# Computer Use：Grok 长任务 Goal 提示词

> [!CAUTION]
> 本提示词已于 2026-09-09 被替代，禁止继续执行下方旧 P0–P9 正文。请改用 [Grok 逐步开发总计划](2026-09-09-computer-use-grok-step-plan.md)、[执行状态账本](2026-09-09-computer-use-execution-state.md)和新的[启动入口](2026-09-08-computer-use-grok-prompts.md)，并从 S0.1 开始。

日期：2026-09-08。执行模式：**一个有限 Goal 连续完成 P0–P9，阶段验收后自动续接，最终统一交付用户审核**。此模式替代旧版“只做 P0 / 每批等待用户继续”，不改变三平台范围和质量门槛。

## 如何启动

在 Grok App 中打开 `H:\aicoding\grok-app-computer-use`，使用输入框的“目标任务”模式，粘贴下面正文（去掉首行 `/goal`）。在 Grok Build 交互终端中则保留 `/goal` 首行。两种方式选一种，避免重复加前缀。

核对依据：当前 App 的 `src/lib/draftDoc.ts` 将 Goal 模式序列化为 `/goal\n`；已检查本机 `grok --version` 为 `1.0.13 (5e9a58528b76)`。官方源码快照的 [slash 指南](https://github.com/xai-org/grok-build/blob/9684fa3cdbf2995e30ea8b9b637f1db008f144fc/crates/codegen/xai-grok-pager/docs/user-guide/04-slash-commands.md)说明 `/goal` 支持跨轮执行和完成证据审查，以及 `status/pause/resume/clear`。这是文档和本机入口核对，尚未替用户启动 Goal；具体可用状态由当前会话确认。

不设置任意 token 数值、不打开 always-approve、不修改当前账号或全局模型。Goal 能续轮不等于绕过预算、服务限制或人工权限；遇到这些限制要保存进度并准确报告。此任务不是定时监控，不创建 `/loop`、scheduler、cron 或“已安排”。

## 完整提示词（整段复制）

```text
/goal
在 Grok App 项目中完成完整的本机 Computer Use 功能，达到既定三平台首版验收要求，并交付可运行代码、测试、实机证据、安装/回退说明和最终审查报告。这是一个连续执行到完成的有限目标：P0–P9 是内部里程碑，验收后自动进入下一个可执行阶段，不要每完成一批就停下来让我说“继续”。

一、工作目录与权威资料

工作目录：H:\aicoding\grok-app-computer-use。
先读取 AGENTS.md、README 开发指南、CONTRIBUTING.md，以及以下文件：
- docs/plans/2026-09-08-computer-use-grok-handoff.md
- docs/plans/2026-09-08-computer-use.md
- docs/research/2026-09-08-computer-use.md
- docs/research/2026-09-08-computer-use-harness-comparison.md
- docs/research/2026-09-08-dsh-third-party-plugins.md
按批次补读 docs/llm-wiki/ 的实际产品规范。先检查已有代码和证据，再动手，不重新无限调研，也不要只回复一份计划。

本次明确覆盖旧交接中的执行节奏：不再限于 P0，不再逐批等待人工审核。保留小批次实现、自动测试、阶段自审和进度记录；整体完成后交给我统一审核。技术验收、账号保护、权限边界及不自动发布的规则继续有效。

二、分支与现有工作

核对 git status、git worktree list、HEAD 和已有进度。原研究分支 feat/computer-use-design，基线 30757366a739ec9aaf0ccc95bbb3efe19a067aa9；交接时本地 upstream/main 已领先 17 个提交，接手时复核，不把这个数字当作实时远端状态。
若尚未有实现分支，在当前独立 worktree 创建 feat/computer-use-implementation；若 feat/computer-use-p0 或其他已确认的 Computer Use 实现分支已有成果，则保留并续接，不为了改名重建工作。已有同名分支先查其 worktree，禁止强制覆盖。
方案文件尚未提交，必须保留。不要切到 H:\aicoding\grok-app 的壁纸工作区，不 reset/clean，不混入无关修改。先只读审查上游接入点变化；除非当前开发确实依赖其变更，否则不要顺手做大规模同步。确需集成时先完成保护、冲突分析和具体方案，再处理必要授权。

三、产品目标与架构

Windows、macOS、Linux 从首版一起支持。发行 target 为 Windows x64、macOS arm64/x64、Linux x64；Linux 基线包含 Ubuntu GNOME Wayland 与 X11。其他合成器按实际能力列明，不能把浏览器或 XWayland 运行成功冒充原生 Wayland 桌面支持。
使用 Grok Build ACP + 会话专属 MCP + App 自有 Computer Use Broker。桌面优先验证 Cua Driver，受管浏览器优先 Playwright；已有 Chrome/Edge tab 对比 Playwright 扩展与 Tencent BrowserSkill 后选择默认路径。DSH/Pi/Claude/Codex 只作为公开机制和体验参考，不更换主 Agent 引擎，不复制 Codex 私有组件，不引入整套 DSH 插件市场。
用户能选择应用/窗口/tab，让 Grok 观察、操作、验证结果，随时暂停、接管和停止。融合现有 Composer、任务卡、资源面板；主界面简洁，技术诊断折叠，避免堆积说明和控件。

四、持续执行方式

建立并持续维护 docs/plans/2026-09-08-computer-use-progress.md，记录阶段、子任务、依赖、代码路径、测试证据、问题和下一步。文件已有时先读再更新，不覆盖历史。
状态区分 not_started、in_progress、passed、failed、not_run；实现状态和验证状态分别记录，代码写完不代表平台通过。
采用“读取现状 → 实现最小完整部分 → 测试 → review → 修复 → 更新账本 → 下一部分”的循环。每完成一个里程碑发简短进度并继续，不把阶段汇报当最终完成。
按依赖推进 P0 → P1 → P2 → P3/P4/P5a/P5b → P6 → P7 → P8 → P9。任务拆分用于控制改动与验收，不是要求我逐批点击批准。小范围实现选择自行决定，发现证据推翻选型时更新 ADR 和依赖，不偷偷降低需求。

P0：编写实际 MCP/合成图/坐标探针、核对 CLI/model/effort 与工具可见性；验证 Cua 目标和取消；比较两条已有 tab 连接路线；建立四 target 构建和原生验证矩阵，输出 docs/research/2026-09-08-computer-use-p0-validation.md。
P1–P2：版本化协议、私有 IPC、驱动生命周期、Broker、输入互斥、目标授权与取消接管；默认关闭 feature flag。
P3–P5b：完成三平台适配、权限诊断、窗口身份、DPI/多屏、中文输入及各平台失败路径。
P6–P7：受管浏览器、DOM/AX、上传下载、已有 tab 连接、借用归还、按需预览、WebView 子集及工作台融合。
P8：真实模型观察—动作—验证循环、重连恢复、任务轨迹、耗时诊断与速度/质量比较。
P9：固定依赖和来源、签名/打包、四 target 安装更新回退、三 OS 实机回归和用户文档，最后做一次面向最终代码的整体 review 并修复发现的问题。

五、技术闸门和阻塞处理

P0 中已证实不成立的前提必须先修复或更换适配方案，不能继续堆依赖于它的正式功能。如果只是缺少某个平台测试设备，记录 not_run，继续不依赖该证据的共享协议、独立模块、测试 fixture、平台 runner 和文档；所有未验收能力保持关闭。这不等于 P0 整体通过，更不等于三平台发布放行。
普通编译失败、测试失败、类型错误和能定位的 bug 自行修复，不停下来问“还要继续吗”。已有通过的检查仅在相关改动或新风险出现时重跑，避免空转。
遇到必须由我完成的系统授权、缺失设备/账号或改变架构的关键取舍，提出一个具体、自足的问题并写入 blockers，同时继续其他独立工作。只有所有剩余有效路径都被这些前置条件阻塞、我明确停止、或运行时预算/服务限制无法继续时，才保存断点并暂停。
暂停报告要写清已完成、实际阻塞、我需要做什么和恢复后的下一条动作。不要用无限重试或定时任务假装持续推进，也不要把“目前能做的做完了”标为整个 Goal 完成。

六、不可省略的正确性

- 所有工具路径统一经过 Broker。目标和权限由 Host 验证，绑定会话/run、进程/窗口生命周期、快照与几何代际；不得共享一个全局 snapshot/current tab。
- 定向目标失效直接拒绝，不回退全桌面；语义后台、定向事件、前台输入分别报告，虚拟光标不代表输入隔离。
- 同桌面跨会话/跨 App 实例输入互斥；停止先撤销派发，再等待底层静止。stop_requested 不等于 stopped，Promise reject/CLI 退出不证明 OS 或浏览器动作已停，迟到回调不能影响新任务。
- applied/verified/rejected/unknown 分开。actionId 防重复；超时可能已经执行，必须先观察，不自动重放提交。不靠提示词承诺幂等，不承诺崩溃前后物理输入 exactly-once。
- 每次动作验证实际后置条件，不能把“工具返回 ok”“树变了”“截图存在”当作任务成功。合成测试答案留在独立验证端，模型不能从工具文本抄答案。
- fork/resume/压缩后重新观察和授权，不能重放历史输入。自动恢复记录系统来源，不能伪造用户授权，不能覆盖 stop/deny。
- BrowserSkill 插件级归属不等于 Grok 多聊天授权；extension-shaped Origin 不等于实际配对身份。借用用户 tab 后归还，仅清理自有资源。
- UI 预览和模型观察分开；可见时按需预览，隐藏后停止周期采集，主动 observe 不受影响。snapshot + revision 恢复有序，旧画面标过期且不能用来点击。
- 宿主自己的授权界面不能成为可操作目标；敏感人工步骤暂停采集。默认不落盘完整截图、按键或密钥，说明 CLI/模型服务实际留存边界。

七、工程与权限

遵守 pnpm、LF、现有代码风格和模块拆分。App.tsx/AppWorkbench.tsx 不增加大块状态；用户文案同步 15 locales，设置入 settingsCatalog，复用现有控件。禁止全局脚本临时兜底、假能力开关、未实现按钮和用豁免绕过质量门禁。
项目内可逆代码/文档修改、针对问题的测试和修复连续执行；依赖与 fixture 使用项目私有目录。系统权限或真实桌面操作需要用户参与时，先明确目标和操作，不自动点击批准。只操作自建测试应用或我明确选择的目标。
沿用已授权模型/provider，不替换账号、不输出凭据、不导出 Cookie/Token；不改共享 ~/.grok、全局模型、系统代理或正式安装，不硬编码 10808/10809。不默认启用 always-approve。
持续记录阶段文件清单和建议 PR 拆分，便于之后小批提交。目前只做本地代码、测试与报告，不自动 commit/push/提 PR/merge/tag/发版，最后由我审核发布相关动作。
这是一项有限开发目标，不创建 /loop、scheduler、cron 或“已安排”。遵守当前运行时预算；未明确给预算时不要自作主张设置或提高 token 上限。

八、完成标准与最终交付

按主方案运行适用的 pnpm deps:check / audit:prod / typecheck / test / lint / build:ui，以及 Rust fmt/clippy/test 和代码质量门禁；必要时运行真实 GUI fixture，旧基线问题和新增回归分别记录，不把环境错误说成断言通过。
最终三平台共同核心和四 target 安装/更新/回退必须有证据。每 OS 单独统计主方案的任务成功率、错误目标/未授权动作/停止后新派发、中文输入/DPI/权限撤销/断连恢复；模拟测试、交叉编译与原生实机分开。
只有全部必需验收通过、交互失败路径和文档齐全、整体 review 的阻断问题清零，才能完成 Goal。无设备、无权限、未运行项目、失败项或占位实现都不能标成成功。
最终交付代码与文件清单、阶段进度、验证和性能报告、已知限制、安装/回退及复现步骤、建议小批 PR 顺序。若只能暂停，交付可继续的检查点，明确 Goal 未完成。

现在核对工作区和已有成果，建立/恢复进度账本，立即开始第一个尚未完成且前置条件满足的任务，并持续推进。
```

## 中断后续接

在原 Goal 会话使用该版本支持的 `/goal status` 查看状态、`/goal resume` 恢复暂停目标。因预算或服务限制暂停时先处理对应限制，不重建一个相同 Goal 来绕过上限。若只能新开会话，重新发送以上目标并保留现有分支和 progress，让 Grok 从第一个未完成任务继续。

可追加这段恢复说明：

```text
继续同一个 Computer Use 目标。先读 progress、验证报告和 git 状态，核实上次检查点对应代码仍在。从第一个前置条件满足且尚未完成的任务继续；已通过且未受后续修改影响的部分不重做。保持 P0–P9 自动续接、三平台验收和最终统一审核，不回到旧的逐批等待模式。
```

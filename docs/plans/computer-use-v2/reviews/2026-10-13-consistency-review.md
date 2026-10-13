# 文档一致性与可执行性评审（Gemini 3.8 Flash high，只读）

日期：2026-10-13

# Grok Computer Use v2 方案评审报告（文档一致性与可执行性）

本报告对 `docs/plans/computer-use-v2/` 目录下全部计划文档（README.md、01–08、STATUS.md）进行了全量静态审查与一致性校验。审查覆盖了文档间数据矛盾、Markdown 链接与 GitHub 锚点有效性、STATUS.md 与 06-roadmap.md 任务卡对应与依赖拓扑、任务卡可执行性（接口/文件/测试/DoD/验收命令）、08 执行规程自洽性以及术语一致性。

---

### 评审发现清单

[严重度 高] [06-roadmap.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/06-roadmap.md#L375-L377) §P5-04 (L375–377) 与 §P6-03 (L421–423) — 核心任务卡正文完全空白，缺少文件、接口、测试与卡内 DoD 描述 — 建议在 P5-04 补充实现规格：定义 `ComputerUseConfirmCard.tsx`（高危动作拦截与裁剪图展示）与 `ComputerUseHandoffCard.tsx`（人工接管卡）、交互状态机、Vitest 测试用例与 Host 协议 `host.approval.confirmAction` / `handoff` 回调联动；在 P6-03 补充实现规格：在 `gcu-core` 实现 `wait_for` 轮询与超时状态机，接入 [03 §11.1](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md#L511) 定义的 5 种条件匹配器，补全单元测试及 fake 场景验证。

[严重度 高] [06-roadmap.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/06-roadmap.md#L30-L40) §总览 (L30–40) — 总览表格中的阶段估算工时与下方任务卡实际估算之和严重失真且存在多处数学矛盾 — 建议重算并修正总览表：P0 修正为 21.5d（卡片之和为 21.5d，若计入 S-01/S-02 3d 则标明 `21.5d + 3d spike`，原写 14.5d 差 7d）、P1 修正为 19.5d（原写 18d）、P2 修正为 18.5d（原写 20d）、P3 修正为 10.0d（原写 15d 虚高 5d）、P5 修正为 26.0d（原写 22d 差 4d）、P6 修正为 15.0d（原写 14d）、P7 修正为 17.0d（原写 18d）、P9 修正为 19.0d（卡片之和为 19d，原写 15d 差 4d）；全案实际总工时从总览声称的 166.5d 修正为 176.5d（加 spike 为 179.5d）；ASCII 流程图中将 P0 从 `2–3 周` 改为 `4–5 周`、P3 从 `3–4 周` 改为 `2 周`。

[严重度 高] [08-grok-execution-protocol.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/08-grok-execution-protocol.md#L27-L37) §2 规则 1 与 规则 11 vs [06-roadmap.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/06-roadmap.md#L3) §序言 (L3) 及 P0-05/P0-06/P0-08/P4-04 — 规程要求的"不合入 partial / 一次只做一张卡"与路线图"可拆为 ≤3 个 PR"硬性冲突 — 建议在 08 §2 规则 11 明确增加拆卡专用条款：`若任务卡明确标注"可拆"，允许拆分为 ≤3 个独立 PR，分支命名使用 cu2/<卡号>-<序号>-<短名>（如 cu2/P0-06-1-pipeline），每个子 PR 必须具备自身闭环的编译、测试和局部 DoD，禁止提交半成品；STATUS.md 对应的 PR 列在全部子 PR 合入前追加记录并在备注注明进度，全部子 PR 验收后方可置为 done`。

[严重度 高] [06-roadmap.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/06-roadmap.md#L404) §P5-11 (L404) vs [STATUS.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/STATUS.md#L63) (L63, L73, L75) — P5-11 依赖声明引用了错误的卡号（写为 P9-03 代替 P9-01），且 STATUS.md 遗漏该依赖 — 建议将 06-roadmap.md P5-11 的依赖字段从 `- **依赖：** P5-01…P5-09、P9-03 的评测 runner` 修正为 `- **依赖：** P5-01…P5-09、P9-01（评测 runner）`（P9-03 实际为性能预算门禁）；同时将 STATUS.md 第 63 行 P5-11 的依赖修正为 `P5-01…P5-09, P9-01`。

[严重度 高] [STATUS.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/STATUS.md#L71-L79) (L71–79) vs [06-roadmap.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/06-roadmap.md#L446-L485) §P7, §P8, §P9 — 任务卡与进度表的阶段颗粒度与卡片展开状态严重脱节 — 建议统一两边的排期粒度：在 STATUS.md 中将折叠的 `P7-01…06`（17d）展开为 P7-01 至 P7-06 六行、将 `P8-01…05`（12d）展开为 P8-01 至 P8-05 五行（与 P9-01~07 的逐卡颗粒度保持一致）；在 06-roadmap.md 中为 P7、P8、P9 补充展开为标准的 `### PX-YY` 任务卡块，写明具体的依赖、目标与验收条件。

[严重度 中] [05-route-b-integrated.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/05-route-b-integrated.md#L144) §4.2 (L144), [07-quality-gates.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/07-quality-gates.md#L70) §3 (L70) vs [03-architecture.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md#L521-L525) §11.2 — 敏感动作被拒时的错误码不一致，且使用了权威错误码清单外的非法枚举 — 建议将 05 §4.2 中的 `refused: user_declined` 和 07 §3 中的 `refused` 统一修正为 [03 §11.2](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md#L523-L525) 权威定义的标准错误码 `approval_denied`（或在 03 §11.2 中将 `user_declined` 作为合法错误码正式登记入库）。

[严重度 中] [README.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/README.md#L52) (L52), [03-architecture.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md#L391-L395) §7.3, [04-route-a-standalone-mcp.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/04-route-a-standalone-mcp.md#L94) §4, [05-route-b-integrated.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/05-route-b-integrated.md#L135) §4.2, [06-roadmap.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/06-roadmap.md#L154) (L154), [07-quality-gates.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/07-quality-gates.md#L67) (L67) — 授权档位底层标识与 UI 命名在中英文档间混杂不一（`once` vs `turn` vs `仅本次` vs `本轮`） — 建议在 `gcu-protocol` 的数据结构中明确底层枚举定义（建议统一固定为 `pub enum ApprovalDecision { Turn, Session, Always, Deny }`），在 04 §4 的 JSON schema 中明确对齐为 `["turn", "session", "always", "deny"]`（若沿用 Codex 惯例字段 `once`，需在 03 §7 显著注明 `once == turn`）；在 05 §4.2 的 UI 按钮中统称为 `[本轮有效]`（替代容易被理解为单次动作的 `[仅本次]`）。

[严重度 中] [03-architecture.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md#L495-L515) §11.1 vs [03 §10](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md#L483), [05 §5](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/05-route-b-integrated.md#L163), [06 §P1](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/06-roadmap.md#L31), [06 §P6-06](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/06-roadmap.md#L437) — 协议工具总数与"9 个核心工具"范围定义模糊 — 建议在 03 §11.1 工具表增加第 17 项 `browser_navigate` 并注明为"路线 B 激活内置浏览窗时的条件注册工具"；在 03 §11.1 明确定义 `profile: minimal` 包含的具体 9 个工具清单（建议明文化为：`get_app_state, click, perform_secondary_action, set_value, type_text, press_key, scroll, list_apps, list_windows`）。

[严重度 中] [01-review-v1.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/01-review-v1.md#L54) (L54), [04-route-a-standalone-mcp.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/04-route-a-standalone-mcp.md#L139) (L139), [06-roadmap.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/06-roadmap.md#L78) (L78) — 3 处 Markdown 链接锚点与目标标题 GitHub slug 不匹配导致跳转失效 — 建议套用以下修改：
  1) `01-review-v1.md:54`：修改为 `[07 §3](07-quality-gates.md#3-故障与生命周期测试l2fake-驱动)`；
  2) `04-route-a-standalone-mcp.md:139`：修改为 `[06 S-01](06-roadmap.md#s-01-grok-build-cli-mcp-能力探测spike)`；
  3) `06-roadmap.md:78`：将 `03-architecture.md` 的标题精简为 `### 11.2 错误码`，将冗长的括号说明移至正文，使原链接 `03-architecture.md#112-错误码` 原生生效。

[严重度 中] [STATUS.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/STATUS.md#L20-L21) (L20–21) 与 [06-roadmap.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/06-roadmap.md#L149-L160) (L149–160) — P0-11 与 P0-12 编号倒置且逻辑顺序颠倒 — 建议将现 `P0-12`（`gcu-host-protocol` 与 engine 模式，2.5d）重编号为 `P0-11`，原 `P0-11`（文档与 STATUS 初始化，0.5d）重编号为 `P0-12` 作为 P0 的最终收尾卡（依赖新的 P0-11）；并在 STATUS.md、06-roadmap.md 及 P5-01 依赖中同步全局更新。

[严重度 中] [05-route-b-integrated.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/05-route-b-integrated.md#L77) §2 (L77) vs [06-roadmap.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/06-roadmap.md#L362) §P5-01 — 临时目录命名与改回规则缺乏任务卡承接与闭环保证 — 建议消除"先用 v2 后来改回"的中间状态描述：由于 `main` 分支上原 `computer_use/` 目录已被彻底移除，建议在 05 §2 与 06 P5-01 中直接将 Rust 模块路径永久固定为 `src-tauri/src/computer_use/`，删除"P5 末尾改回"的模糊注释，避免后续产生二次重命名和依赖检测（`xtask check-deps`）断裂。

[严重度 中] [08-grok-execution-protocol.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/08-grok-execution-protocol.md#L10-L18) §1 (L10–18) vs §2 规则 2 (L28) & §7 (L128) — 工作循环中 STATUS.md 的更新操作在实际 PR 流程中产生死锁 — 建议明确 PR 生命周期与 STATUS 的维护规则：`在功能分支 PR 的最后一次提交中直接将 STATUS.md 的状态更新为 done 并填写分支名；PR 合入 main 时由 merge 提交或维护者回填正式 PR 编号/链接；严禁将 in_progress 状态留在已合入 main 的代码中，禁止为了更新 STATUS 单独提 PR`。

[严重度 中] [03-architecture.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md#L48-L71) §2 vs [06-roadmap.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/06-roadmap.md#L499) §P10, [07-quality-gates.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/07-quality-gates.md#L184) §8 — 核心架构定义的 crate 拓扑与演进规划中出现的 crate 名称不一致 — 建议在 03 §2 的 crate 拓扑与 Cargo workspace 中补充登记 `gcu-eval`（若其确实作为独立评测 crate 存在），并在 P10 的 `gcu-driver-cua` 处显式标注其为"计划外扩展 crate，开工前需按 08 §6 提交 ADR 补充修改 03 §2 拓扑白名单"。

[严重度 中] [06-roadmap.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/06-roadmap.md#L85-L148) §P0-04, P0-07~P0-10, P1-02~P1-08, P2-03~P2-08, P3-01~P3-04 — 任务卡中后期大面积缺失 `- **文件：**` 明确文件落点 — 建议参照 P0-01/P0-03/P0-05 的格式，为 P0-07 至 P3-04 统一补齐 `- **文件：**` 行，明确指定主要源码文件及测试文件路径（例如 P0-07 补充：`computer-use/crates/gcu-core/src/snapshot/{mod.rs, store.rs, index.rs, diff.rs}`），防止 Grok 自由发挥破坏单文件 ≤600 行和统一模块规范。

[严重度 低] [03-architecture.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md#L611) §14 (L611) — 标题内嵌 Markdown 链接导致 GitHub 锚点 slug 生成畸形并破坏引用 — 建议将原标题 `## 14. 性能预算（P9 发布门禁，见 [07 §4](07-quality-gates.md#4-性能预算)）` 修改为纯文本标题 `## 14. 性能预算（P9 发布门禁）`，将链接移至标题下首行正文（"具体发布门禁指标见 [07 §4](07-quality-gates.md#4-性能预算)"）。

[严重度 低] [06-roadmap.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/06-roadmap.md#L360) §P5-01 (L360) — 任务卡依赖字段夹带中文括号解释导致依赖图自动化解析脆弱 — 建议将 `- **依赖：** P0-12、P1-09（mac 先行；Windows/Linux 引擎构建合入后自动启用）` 修改为严格纯卡号 `- **依赖：** P0-12、P1-09`，将括号中的平台实施节奏说明移入下方的 `- **内容：**` 行。

[严重度 低] [06-roadmap.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/06-roadmap.md#L50-L440) §P0~P6 全部任务卡 — 多数任务卡缺乏显式可执行的本地自测与验收命令 — 建议在每张任务卡的 `- **测试：**` 小节末尾追加具体的测试执行命令，例如：`验收命令：cargo test -p gcu-core --test pipeline`，避免 Grok 在自测时误跑全量耗时套件或漏跑关键单测。

[严重度 低] [03-architecture.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md#L61) §2 (L61, L79) — `gcu-script` 的外部依赖 `rquickjs` 未在 crate 依赖规则白名单中登记 — 建议在 03 §2 依赖规则表格中单独增补一行：`| gcu-script | gcu-protocol, gcu-core, rquickjs | OS crate、网络 |`。

[严重度 低] [03-architecture.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md#L650-L675) §15.3 (L650–675) — Host 协议 RPC 方法命名字段风格与层级不统一 — 建议统一采用 `domain.subdomain.methodName` 的 camelCase 规范，将 `engine.stop` 规范重命名为 `engine.control.stop`（或 `engine.session.stop`），消除单层与多层混用的情况。

[严重度 低] [04-route-a-standalone-mcp.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/04-route-a-standalone-mcp.md#L39) §2 (L39) vs [03-architecture.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md#L521-L528) §11.2 — CLI 退出码未明确与 25 个协议错误码的映射关系 — 建议在 04 §2 补充映射表：`permission_missing -> 退出码 3`；`ambiguous_app, invalid_key, out_of_bounds -> 退出码 2`；其余业务 `isError` -> 退出码 4；进程 panic/未知异常 -> 退出码 10。

[严重度 低] [03-architecture.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/03-architecture.md#L318) §5.3 与 [06-roadmap.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/06-roadmap.md#L286) §P3-01 — Linux 最小化与 CI runner 环境下缺少 AT-SPI2 守护进程拉起指引 — 建议在 P3-01 和 07 §1 的 CI 配置中增加环境启动说明：`在 Xvfb 启动后，通过 dbus-run-session 与 /usr/lib/at-spi2-core/at-spi-bus-launcher 确保用户会话 AT-SPI D-Bus 总线就绪`。

[严重度 低] [06-roadmap.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/06-roadmap.md#L320) §P4-02 (L320) — 声明使用独立轻量 HTML/CSS 页面但未指明打包机制 — 建议在 P4-02 明确打包实现方式：`轻量页面采用单文件 HTML/CSS（放置在 computer-use/ui/），经构建脚本压缩后使用 Rust include_str! 编译进 gcu 二进制，实现零外部运行时分发`。

[严重度 低] [01-review-v1.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/01-review-v1.md#L11-L26) §1 (L11–26) — 规模画像表格统计行数与前言 Git diff 变更行数口径未显式对照（待确认） — 建议在表格底部增加一行口径注释：`注：表格统计为按文件类型的净代码行数（~32.6 万行），与前言 git diff 统计（+40.2 万行）差异主要为静态资源、golden 快照与 vendor 资产`。

[严重度 低] [05-route-b-integrated.md](file:///home/claude/src/grok-app-wt/review/docs/plans/computer-use-v2/05-route-b-integrated.md#L127) §4.1 (L127) — 路线 B 引擎在 App 内多本地会话同时运行时的端口与会话隔离机制待补充说明（待确认） — 建议在 05 §1 补充单引擎多会话隔离说明：`单个 gcu engine 子进程内部管理连接池，每个本地会话调用 engine.session.open 获得独立的 session_id 与 32 字节认证 token，支持多会话并发操作与独立审批状态机`。

---

### 总体结论

**总体结论：** 该方案在架构分层、安全模型与协议设计上方向极为扎实且极具前瞻性，但路线图存在工时加总矛盾、空任务卡、破坏闭环的拆卡规则冲突及若干断链瑕疵，必须先修复上述任务卡正文与规程逻辑矛盾后方可交由 Grok 正式开工。

# 给 Grok：任务中授权与一键继续目标提示词

> 本文为2026-10-02历史派发模板。用户后来要求Codex接手，当前单writer，不启动Grok/子代理。
> 当前source52检查点见[原生worker启动](2026-10-04-computer-use-task-consent-u04-existing-bootstrap-checkpoint.md)：
> dormant1/browser focused3通过但完整native33通过/5失败、exit101。下一项[原control诊断与终态](2026-10-05-computer-use-task-consent-u04-existing-control-terminals-next.md)。
> 旧观察/startup及五项新首败仍OPEN；原owners已退出，精确清理完成，所有首次证据保留。
> 下文分工不覆盖当前授权或实测，整体active / partial — not releasable，未提交发布。

日期：2026-10-02。整份交给 Grok 的 Goal/目标模式。
本文件是执行指令，不代表 Grok 已启动或改动已经完成。
详细合同：[分阶段开发计划](2026-10-02-computer-use-task-consent-plan.md)。

---

你负责修改 Grok App 的 Computer Use 授权体验。Codex 负责规划与审查，产品代码由你写。
这次的目标不是增加更多按钮，而是真正完成“任务中按需申请、一次批准覆盖本次任务范围、
暂停后一次点击继续”的全链路。持续按依赖推进 U00–U09，直到本批可交审或剩余项全部有
确切外部依赖。不要只完成一个 fixture 或写了一张卡片就结束。

## 固定现场和任务入口

工作区：H:/aicoding/grok-app-computer-use。
分支：feat/computer-use-implementation。
交接基线：8d8b71c262ab222748bca32412eea884d4b892e0；开工重新核对 HEAD/dirty，不能回退。
origin=yclenove/grok-app，upstream=RongleCat/grok-app。
先确认没有另一个 live 产品 writer；你是唯一 writer，不启动 subagents。
保留既有未提交文档、patch 和其它修改，未知内容不要覆盖。

首先全文读：

1. AGENTS.md、CONTRIBUTING.md。
2. docs/llm-wiki/computer-use.md、i18n.md、dialogs.md、settings-ia.md、catalog.md、maintain.md。
3. docs/plans/2026-10-02-computer-use-task-consent-plan.md，包括状态/范围合同、U00–U09、S01–S35。
4. docs/plans/2026-10-02-computer-use-handoff.md；总体 remaining-plan 了解原有缺口，
   不先重跑旧 GNOME VM 或几天全部历史日志。
5. 详细计划第 3 节真实源码和相关既有测试。先确认已有实现，避免按旧 wiki 重写已正确部分。

本轮授权体验 U00–U09 优先。旧计划要求先做 D00–D03、面板先选目标授权、resume 后再
选目标的规定，以本轮新合同为准；不代表其他权限限制被取消。
完整 CU 的 D00–D31 全平台、安装、业务、长稳缺口仍在，不能因此声称整个产品已完成。
U09 交审后记录下一 ready D 任务，不擅自扩大本轮为全部其它功能开发。

## 必须实现的用户流程

用户先正常说任务。模型先理解和规划，只有实际需要电脑时才调用结构化申请工具。
聊天中出现一张简洁卡片，说明目的、范围、仅本次任务和随时停止。
用户批准后，原任务自动继续；不用先去设置选窗口，也不用手动再发“继续”。
默认 feature 关闭；首用可以用清楚的“启用并允许本次任务”按钮在一张卡片完成明确决定。
未批准前不能截图、读取 DOM/UIA/AX/窗口正文或输入；bootstrap 不启动 native/browser worker。
功能关闭时的模型能力最多是发出本地申请、查询自己申请，不是静默启用电脑。

同任务已批准范围内连续执行至少 20 个真实动作不再弹 App 授权；两应用实例切换至少
5 次不逐窗口重批。范围外目标才申请一次增量决定；拒绝增量不清空原范围。
Managed Browser 新 tabs/popups 按明确 scope policy；ExistingTab Share/activeTab 的真实
手势和 OS 权限不能绕过，但不要把它们再套一串重复 App 授权。

暂停立即 fence 输入、退休旧观察。用户点一次“继续”，Host 等原工作物理空闲、复验
同任务/同范围/真实目标/平台权限，重新绑定并 fresh observe，原任务续行一次。
不让用户重新选目标再授权，不使用 UI preview 当模型观察。
Stop/撤权结束权限；系统权限撤销、目标死亡、App 重启、fork 不静默复活。

## 先做 U00，不猜 CLI 行为

先保存基线，核真实 CLI/ACP normal 模式：

- 无 run 时能否预连接仅申请能力；busy turn 内如何发起并展示请求。
- pending 返回后等待真人 90 秒，批准如何在同一 session 续行一次。
- generic use_tool 提示是否会逐工具/逐动作重复，真实支持什么会话 allow 粒度。
- Stop/EOF/agent exit/换会话后旧决定和 continuation 如何退休。

写 ADR 和真实消息时序再大改。不能用 --always-approve 探针代替 normal 模式体验，
不能改用户共享 ~/.grok，不能编造 ACP extension，不能只删 attach busy 检查。
如 CLI 不支持粒度，给实际证据、最小依赖版本和范围内替代；不放弃按需入口、不做假自动续行。

## 架构底线

分开 session bootstrap、Host TaskGrant 和短期 ExecutionBinding。
Host 创建 task/request ID 和 grant revision，绑定 App session、ACP incarnation 和 generations。
模型只能提出目的/能力/范围意图，不能自批、不能伪造 taskId、不能调用 Host authorize/resume。
未授权的 bootstrap dispatch 是小 allowlist，不能把 SessionBinding.run_id 设空后全部放行。

申请工具快速返回 pending；批准是异步 Host 决策。不要让普通 15 秒 HTTP 连接挂几分钟，
不要审批期间持 Broker mutex/connect lock/action admission/desktop lease。
续行有幂等消费账本，忙时等正确边界；不新建 chat、不重发原任务、不重放副作用。
目录预接入或等效稳定可见，批准/暂停不用热换 busy turn catalog；reconcile 保留其它 MCP。

TaskGrant 范围可包含多个明确目标；snapshot/ref/geometry 永远跟当前真实目标绑定。
Desktop membership 核原生关联与 PID/start/授权根，不能只按名称/标题/路径/父 PID 放行。
Managed context 仅任务 owned profile；ExistingTab 配对+Share+App grant 分别验证；
WebView 绑定 label+instance。四 surface 之间禁止 silent desktop fallback。
确需全桌面必须真实支持且用户显式批准；不得默认放宽来实现“少弹框”。

continue 是 Host 用户动作，一次点击确认原范围；旧执行身份退休、新观察成功后才输入。
compact/model/ACP 重连也不能自动继承活执行授权；同 App session 可以保留非执行范围记录，
由一个明确“继续此任务”按钮重新绑定。App restart/fork/account/root/remote 变化无活 grant 继承。
assistant end_turn 不是任务完成；Host 有显式任务完结/取消和有界 idle TTL。
按详细计划固定请求期限和任务期限，不永久保留权限，不凭关键词或模型正文批准。

Stop 有独立有界通道：先 fence 后清理/join；stop_requested 不等于 stopped。
old ticket/target/snapshot/ref 拒绝；unknown 不重放；old cleanup 不碰 replacement owner。
物理接管后模型不能自动抢焦点。远程 IM/SSH/automation 不继承本机 CU。

## 按小阶段持续实现

按详细计划逐项：U00 事实/ADR → U01 核心/IPC → U02 预连接/异步续行 →
U03 聊天卡/面板 → U04 多目标/增量 → U05 一键继续 → U06 生命周期 →
U07 真实 App/CLI/MCP/点击数 → U08 门禁/矩阵/文档 → U09 独立开发包/交接。
阶段允许按契约先做 UI，但未实现的后端不能冒充端到端通过。
每完成一个原子项保存可审查 diff 和结果，自动进入依赖已满足的下一项，不等用户说继续。

首要回归是有意义的权限/竞争/业务后置条件。真实缺陷先记录到断言的失败，再修最小实现；
环境 setup 失败不算产品 red→green。不要删旧测试或只镜像实现堆测试数。
按 S01–S35 覆盖；重点是未批准零读取/输入、同 scope 零重批、一键继续、
late approve/Stop/feature-off/跨 session/unknown/CLI 通用确认的真实负向。

15 locales 和 settingsCatalog 完整；使用现有卡片、Select、ContextMenu、GlassModal 样式。
禁止 window.confirm/prompt/alert、原生 select、透明菜单或点击穿透。
App.tsx/AppWorkbench 增长冻结，领域状态落在 domain hooks/providers/components/lib。
文案简短，默认折叠技术细节；300/320px侧栏、900px窗、深浅色、100–200%DPI、长语言和键盘验收。

## 验证与边界

先跑受影响测试，最后按 CONTRIBUTING/当前 CI 跑全量 typecheck/test/lint/build、
依赖/audit、Rust fmt/clippy/test、MCP protocol 和新增 scenario runner、git diff --check。
真实 helper/native feature 的命令单独记录，不用一条根 cargo test 代替全部平台。
Windows libtest 的 0xc0000139 用已有 CI mt.exe post-link manifest 方案，未进入断言写 setup failed，
不要加 build.rs 冲突 MANIFESTINPUT。绿色后不反复刷整套测试填时间。

真实 normal CLI、branch-built App、ACP/MCP 与 owned fixture 形成完整请求→真人批准→
模型观察/动作→独立后置条件→暂停→继续→完成。各主要场景至少 3 轮，记录总点击数。
CLI stub/mock/注入能验竞争，不能冒称真人批准或真实模型理解任务。
90 秒延迟批准有真实等待证据；统计用户/系统/模型网络时间与本地 UI 时间要分开。
本地 card paint 和控制回执 p95 目标 <=500ms，报告样本和限制，不伪造性能结果。

缺 macOS 原生架构、installed Chrome/Edge、TCC/portal 真人手势、签名等写 awaiting_external，
准备可一次执行的步骤并继续独立实现/负向/构建，不在同一 VM 小实验耗完本轮。
全部剩余项确实依赖外部时停止忙等并交审；不自动点系统或 App 的真人批准。
本批成功是 verified_scoped，不是全平台整体 release complete。

通过 Windows 本批门禁后构建新的独立 CU Dev 安装器/portable，源码/runtime 冻结、
SHA256/构建记录/使用说明齐全；不覆盖旧包、不安装、不自动升级、不创建 GitHub Release。
未提交源码也必须包含在独立构建 snapshot；只 archive HEAD 会漏掉本批改动。记录基线 SHA、
patch digest 与逐文件 manifest，排除秘密/日志/profile；不要为打包自行提交代码。
包测试与真实授权链分开报告；未执行安装器如实说明。

## 禁止操作

不 commit/push/PR/merge/tag/release；不改 main，不自动拉取合并，不删除未提交内容。
不 reset/restore/clean/stash/rebase，不覆盖其它 writer 修改，不动其他 worktree。
不改用户代理/VPN、账号/凭据/Cookie/日常 browser profile、共享 ~/.grok 或日常软件进程。
不读取/输出/修改启动器 auth.json/config，登录由外部管理。
不按进程名/端口段/通配符批量结束；只清理 owner-verified 的精确 owned 临时资源。
不以默认 always-approve、默认全桌面、跳过 Host gate、增长 timeout、重启服务当修复。

## 断点和最终交付

使用新唯一 ignored evidence root，append-only 保留日志与第一失败；敏感输入/截图/token 不入普通日志。
每个原子项更新状态，每 60–90 分钟写短 checkpoint。记录实现与验证分别是什么、
源码/runtime/CLI fingerprints、实际命令/退出/断言、原 owner 的清理/join、人工缺口、nextReadyTask。
同一假设最多三次且有新证据；没新证据转下一独立项，不覆盖失败日志。

最终给 Codex：可审查 diff、U00–U09/S01–S35 状态、真实 CLI 消息时序和三类授权点击数、
阶段 ADR、权限违规零计数、真实后置条件、门禁/性能、开发包或精确限制、剩余平台矩阵。
未跑就写 not_run，unknown 不写 pass；候选变化后旧 green 不能冒领当前版本。
未经审查不要说整个 CU 完成，不自行提交；完成本批后交审并列出下一批 D 任务。

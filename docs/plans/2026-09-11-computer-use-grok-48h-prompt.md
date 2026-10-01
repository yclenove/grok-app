# 给 Grok Goal 模式的 Computer Use 48 小时主提示词

> 用法：把下面整个代码块一次性粘贴到 Grok 的 Goal/目标模式。若 CLI 需要显式指令，正文前加 `/goal`。  
> 不要再同时粘旧 overnight prompt；本提示词和 48h execution 取代旧调度。  
> 本任务最长运行 48 小时，但不得用 sleep、空循环或机械重跑凑时间。

```text
你现在是 H:\aicoding\grok-app-computer-use 工作树中唯一的 Computer Use 代码 writer。
在 feat/computer-use-implementation 分支执行最长 48 小时的连续开发、验证和可靠性任务。

这不是“做几项、写份报告就完成”的短任务。上一次任务约 40 分钟提前结束，是因为你把
partial/blocked/短循环/汇总报告当成了阶段完成。本次禁止复用这种完成语义。

一、唯一总目标

把当前 Computer Use 从“Windows runtime 与若干 probe/fixture 有效”推进为：

1. Desktop、Managed Browser、Existing Tabs、App WebView 有明确且真实的产品 surface 路由；
2. `/computer-use-browser` 真实进入 Managed Browser，而不是丢掉 mode 后仍打开 Desktop；
3. Existing Tabs 有 production transport、App/extension 双确认、share/revoke/return 和隔离浏览器 E3；
4. App WebView 注册进 process-wide product Broker/router，而不是只在 fixture 中 new adapter；
5. branch-built Tauri App 经真实 command/session/MCP/backend 完成 scripted App-shell E3；
6. trace retention、support bundle、二次 leak scan、分离清理、性能和 fault harness 产品化；
7. 实际 Windows bundle 被逐文件审计，安全条件允许时做隔离安装生命周期；
8. Computer Use 自身大文件、显式 any、格式和 broad allow 技术债修复，仓库质量门禁通过；
9. 最终代码冻结后执行不少于 12 小时的主动长稳，四条 lane 都满足时长、轮数、oracle 和资源门槛；
10. 最终报告只在 post-soak 门禁和 evidence inventory 之后生成。

最长执行窗口为 48 小时。不要为了“正好两天”空等；也不要在实现跑完后 40 分钟就结束。
若实现提前完成，进入 final-code 主动长稳，目标 12–18 小时。若到 48 小时仍有未完成项，
诚实输出 partial/failed/blocked，不得因时间到而改写为 passed。

二、开始前必须完整读取，不能摘要跳过

1. AGENTS.md
2. docs/llm-wiki/computer-use.md
3. docs/plans/2026-09-09-computer-use-grok-step-plan.md
4. docs/plans/2026-09-10-computer-use-grok-remaining-roadmap.md
5. docs/plans/2026-09-11-computer-use-grok-post-overnight-audit.md
6. docs/plans/2026-09-11-computer-use-grok-48h-execution.md
7. docs/plans/2026-09-09-computer-use-execution-state.md 最后 350 行
8. 当前 git status、当前代码和仍存在的 evidence

48h execution 是本次调度、状态、证据、长稳和完成语义的权威文件。旧 overnight execution、
旧 prompt 和旧 morning report 仅是历史材料，不能覆盖新的总完成谓词。安全约束取所有文档中
更严格者。

三、现场与不可越过的红线

- 预期工作区：H:\aicoding\grok-app-computer-use
- 预期分支：feat/computer-use-implementation
- 审查基线 HEAD：30757366a739ec9aaf0ccc95bbb3efe19a067aa9
- 当前 dirty tree 很大，约 61 个 tracked modified、261 个 nonignored untracked；以现场重数为准，全部保留。
- 禁止 reset、clean、checkout --、restore、stash、强切分支、重新克隆覆盖或推倒重来。
- 禁止 git add、commit、push、PR、merge、tag、release。
- 不改 H:\aicoding\grok-app 或其他 worktree。
- 单一 writer；不要派并行代理修改代码、manifest、runtime seed、execution-state 或 evidence。
- 不读取/导出/复用账号 Token、Cookie、浏览器 storage、系统凭据或共享 ~/.grok。
- 不改代理、VPN、网络路由、账号配置或正式 Grok App。
- 不打开、控制或安装扩展到用户日常 Chrome/Edge profile；只用唯一隔离 profile。
- 不覆盖安装正式 App，不发送外部消息，不发布制品。
- 只终止 PID/路径/owner marker 可证明由本次任务创建的进程。
- 只删除本次 owner.json 白名单中的临时目录；不得删除仓库、用户目录、正式安装或不明 target/cache。
- 重型 Rust/Tauri build、runtime mutation、浏览器、App lifecycle 全部串行。
- production 不依赖 PATH Node、系统浏览器默认 profile、在线 latest、模型任意 eval 或源码 tools 路径。
- 不用 Fake/mock/stub/probe/fixture 数量冒充 production E3/E4/E5。
- 不加隐藏 production 测试后门，不用 allow/skip/ignore/吞错/弱断言换 Green。
- 不向 App.tsx/AppWorkbench.tsx 增加大型状态；两者合计行数只能持平或下降。
- 保持 15 locale key 一致和项目既有 UI/设置架构。

如果目录、分支、HEAD 不符，发现另一个 writer，或继续会触碰上述用户/正式环境，立即停止，
保存只读现场并输出明确 blocker。其他普通实现困难不是停止理由。

四、状态和完成语义

原子 batch 只允许：not_started、in_progress、passed、failed、blocked_external、invalidated、not_run。
partial 只能用于总阶段/最终报告，不能把原子 batch 标 partial 后当作做完。

blocked_external 只适用于真的缺 OS、物理机、硬件、真人、凭据、签名或安全隔离设施。
“没有 production call site”“要写 UI/transport”“测试 flaky”“工作量大”不是 external blocker。
遇到 external blocker：写明缺什么、为何当前不能解决、恢复命令/oracle/evidence，然后自动选择
另一个依赖满足的 ready batch。不要停下来等我说继续。

证据等级严格使用：

- E0 设计/静态阅读
- E1 单元/schema/编译
- E2 真实模块/进程/OS API integration
- E3 产品协议路径 scripted agent + 真实 backend + 独立 oracle
- E4 branch-built App + 真实模型 + 真人授权
- E5 目标 OS 安装/升级/回退/卸载和任务矩阵

Windows 不能替代 macOS/Linux；scripted agent 不能写 real-model E4；Chrome for Testing fixture 不能写
用户 Chrome/Edge product E4；module 存在但无 production call site 不能写产品完成。

五、durable evidence

不要再把唯一日志放到会消失的 %TEMP%\grok-goal-*。

在 tools/computer-use-probe/.run/48h/<run-id>/ 建立：

baseline/ checkpoints/ failures/ logs/ metrics/ reports/
state.json manifest.jsonl owner.json

先用 git check-ignore -q 证明路径被忽略。按 48h execution §5 实现/使用 evidence 协议：

- 每条命令记录脱敏 argv、cwd、start/end、duration、exit、status、E level、log path/hash/bytes、
  code fingerprint 和 postconditions；
- manifest 只追加；state 用临时文件 + 同卷 rename 原子更新；
- 每个 batch 完成即复制并 hash 关键日志；
- 每 30–60 分钟 heartbeat，每 2 小时人类 checkpoint；
- heartbeat 必须由真实 workload 产生，禁止 sleep 等时间；
- Goal scratch 只可临时用，batch 结束前关键证据必须进入 repo-local ignored evidence；
- evidence 达 8 GiB 做安全压缩，12 GiB 或磁盘余量低于 20 GiB 时停止新 workload；
- 不复制整棵 Chromium 到每一轮，不删除首败。

生成 code fingerprint，覆盖 HEAD、tracked diff、nonignored untracked source/config/docs hash 和 runtime
digest，排除 ignored target/evidence/generated seed。任何 production/test/harness/build config 变化都生成新
fingerprint。

六、严格按 D0 到 D14 自动推进

每个 batch 固定执行：

Read current code/state
-> append batch start
-> establish Red or independently prove existing behavior
-> minimal implementation
-> targeted test
-> inspect diff and production call sites
-> run batch gate
-> copy/hash evidence
-> update state/checkpoint
-> automatically enter next ready batch

完整子项、acceptance、fault matrix、资源阈值和命令范围严格按
docs/plans/2026-09-11-computer-use-grok-48h-execution.md，不能用下面摘要替代全文。

D0 — 现场、owner、manifest、fingerprint、旧 evidence inventory。

D1 — 重建真实 claim matrix；找出所有会写 shared seed/cache/profile/target/env 的 gate；先固定并发 Red，
修为独立目录或有 owner/stale-recovery 的串行 lock；建立不吞错的 gate runner；10 轮零互踩。之前
core tests 与 MCP 并发时曾短暂移走 shared seed sibling，后串行通过。这不算产品 defect，但必须修复
测试隔离。后续正式 runtime mutation gate仍串行。

D2 — 首先修产品 surface 路由。当前 panelStore 发 desktop/browser，但 WorkbenchResourcesAside 的订阅
忽略 mode，ComputerPanel 没 surface，Host UI list 仍是 Desktop。先写失败测试，再建立 typed
desktop/managed-browser/existing-tabs/app-webview state、Tauri/TS 对称 command、Broker/router、UI 和
fail-closed。`/computer-use-browser` 必须真实进入 Managed Browser；连续交替 20 次不串线。

D3 — Managed Browser product closure。证明 slash/selector → command → session/run → Broker browser
registry → BrowserSupervisor → App-owned Node/worker/Chromium → MCP → observe/act/verify → UI/stop 的
每条 production call edge。增加经过 D2 product route 的 scripted E3，packaged runtime 连续 10 轮，
independent oracle、two-session、replay/stale/cancel/stop/cleanup 全通过。现有 mcp-scripted-agent 要保留，
但它的 Desktop adapter 是 FakeAdapter，不能单独充当 App E4。

D4 — Existing Tabs product transport。先写 ADR 比较 Native Messaging 与 authenticated loopback，选 v1。
pairing secret 禁止进入 URL query/fragment/DOM/log。实现最小权限 MV3 extension、process-wide Host
service、一次性 TTL challenge、App/extension 双确认、explicit share、tab discovery、grant、typed
observe/act、revoke/rotate/return、generation 和 cleanup。不得在 probe 中 new 一个 host 手工塞 response
冒充产品。Chrome for Testing product transport 连续 10 轮；Edge 仅可用隔离 profile，安全条件缺失则
Edge 单项 blocked_external。覆盖 execution §12 的 15 类 security/lifecycle Red。

D5 — App WebView product registration。先证明当前 WebViewAdapter/bind_side_browser 没有生产调用，
再建立 process-wide backend、App-owned label registry、bind/unbind、session/run/label/tab ownership、
navigation/document/DOM generation 和 surface UI。只允许 bounded snapshot、opaque refs、typed
click/input/scroll/navigate/verify；模型 eval、Cookie/storage/auth 迁移、cross-origin iframe、权限 UI 全拒绝。
真实 App-owned WebView fixture 连续 20 轮，two-session、stale ref、close/stop cleanup 全通过。

D6 — branch-built Tauri App-shell scripted E3。使用独立 app identity/config/home/profile/staging/lease 和
loopback mock ACP；不覆盖正式 App，不读真实账号。通过真实 App process、Tauri command、session manager、
MCP 和 backend 完成 feature-off、设置开启、两种 slash、四 surface、授权、observe/act/verify、
pause/takeover/resume/stop、session switch、preview hide、App close/restart。连续 10 轮，每个写动作有
独立控件/DOM/文件/PID oracle。没有真实模型只能标 App-shell scripted E3。

D7 — diagnostics/privacy。实现 model/UI/support audience、record/byte/age bounded trace、support bundle、
archive reopen 后逐 entry 二次 sentinel scan、trace/staging/managed profiles/Existing grants/runtime 的分离
清理。覆盖 token/cookie/query/form/clipboard/screenshot/profile path/symlink/junction/UNC/root escape。
UI 不得 JSON.stringify 未经过 audience filter 的未来 status 全对象。10 倍超限输入仍有界，cleanup 不越界。

D8 — performance/fault harness。分段记录 discovery/capture/extract/PNG/MCP/loopback/dispatch/apply/verify/
cold start/warm action/stop，输出 raw JSONL 与 p50/p95/max/failure。采样 App/worker/Chromium PID、归一化
CPU、private bytes、working set、handles、threads、queue、disk、ports。逐项跑 execution §16 的 20 类
fault，每项至少 5 次，预期 typed code、无 fallback/重复副作用/泄漏/残留。按文档相对性能和资源趋势
门槛验收；不能删失败样本或只报平均值。

D9 — actual Windows bundle/install。证明 target triple 与 Windows config merge；逐文件审计真实 artifact；
禁止 probe/tests/fixtures/source/cache/zip/secret/开发机路径入包。对锁定 tauri-bundler 2.11.5 upstream NSIS
模板做漂移断言。只有 Windows Sandbox/一次性 VM/独立 app identity 足够安全时才做 clean install、first
launch、upgrade、same-version Repair、rollback、uninstall；否则只把 D9.4 标 blocked_external，D9.1–D9.3
仍继续完成。

D10 — 只清理 Computer Use 自身质量债。修 WorkbenchComposerShell 新增 any、AppWorkbench import 格式、
browser mode 无用逻辑和 computer_use/mod.rs broad allow；拆分 Computer Use 新增大模块，production 目标
≤800 行、gate/test 目标≤1000 行；恢复 FILES_OVER_1K_BUDGET≤80。不要重构无关旧大文件做数字，不让
App shell 增长。typecheck/lint/fmt/clippy/full CU/code-quality 全绿。

D11 — cross-platform readiness/handoff。Windows 上只做可验证 shared/cfg/pack schema 修复和四份实机
handoff；不凭猜测写 native FFI，不把 WSL/cross compile 当桌面 E3。macOS arm64、macOS x64、Linux X11、
GNOME Wayland 分别列环境、权限、命令、fixture、oracle、fault、E4/E5 和 evidence。物理平台仍写
not_run/blocked_external。

D12 — final-code freeze。只有 D0–D11 当前机器所有 ready 子项 passed 后，串行执行 execution §20 的
28 项 pre-soak gate，写 freeze fingerprint、完整 logs/hash、runtime/package digest 和 metrics baseline。
pnpm audit:prod 的既有 Tiptap 风险原样记录，不无审查大升级；由本分支引入的失败必须修复。

从 D12 起，任何 production/test/harness/manifest/lock/build config 改动都使 D12 和所有 D13 时间 invalid，
必须修复、重跑完整 pre-soak，并把 D13 有效时长归零。只追加 evidence/checkpoint/report 不失效。

D13 — final-code 主动长稳。严禁 sleep-only。总最低 active time 12 小时，四条 lane 都必须达到：

- L1 Desktop/App lifecycle：≥2h 且 ≥40 有效轮；
- L2 Managed Browser/MCP：≥4h 且 ≥120 有效轮；
- L3 Existing Tabs + WebView：≥3h 且两者各 ≥50 有效轮；
- L4 Fault/restart/privacy/package：≥3h 且 ≥60 有效轮。

一次 iteration 只能计一个 lane。每轮执行真实 action、独立 oracle、cleanup、资源采样并写 hash evidence。
编译、修 bug、D12 前循环、无 workload 等待都不计 active time。每 30–60 分钟 heartbeat、每 2 小时
checkpoint，从 iteration 自动产生，不能为了 heartbeat 停着等。

D13 invariants 必须全为零：wrong-target、unauthorized、stop 后派发、duplicate side effect、跨 session/
profile/tab/WebView 泄漏、secret/query/form/screenshot 泄漏、触碰用户 profile、owned process/port/lease
残留、无界磁盘/内存/trace 增长、package/source escape。独立 oracle 对非预期 fault 成功率 100%。

出现 invariant breach：停相关 lane、保留首败、最多三次不同安全诊断；若改代码，D12/D13 invalid，
修复后重跑 pre-soak，长稳从 0 开始。禁止机械重跑把首败冲掉。

D14 — post-soak 全门禁、evidence inventory、privacy scan、最终报告。严格顺序：停止改代码 → final
fingerprint → post-soak gates → inventory → privacy scan → draft report → 确认无代码变化 → final report/hash
→ 只读 git status → 停笔。报告后再改代码则报告作废并回 D12。

七、机器可判断的唯一 passed 谓词

只有以下全部为真才输出 48h 结果 passed：

D0..D11 required batches passed
AND D12 final-code freeze passed
AND D13 L1/L2/L3/L4 各自时长和轮数达标
AND D13 active time 总计 >= 12h
AND D13 invariant breach = 0
AND D14 post-soak gates 在同一 fingerprint 上通过
AND evidence inventory 无缺失/hash 错误/stale report

以下不得输出 passed：partial、仅 fixture、仅 unit、仅 scripted probe、仅五次短循环、soak<12h、证据在
已删除 TEMP、报告早于代码、quality gate 失败、产品 call site 缺失、安装/真实模型/跨平台被偷换概念。

八、资源、重试和 48h deadline

- 同一错误最多三次不同、有依据的安全尝试；禁止无限 retry。
- 单命令设合理 timeout，超时保存 stdout/stderr/child tree；不杀不明进程。
- 不同时跑两个 Rust/Tauri/browser/runtime heavy gate。
- 到第 46 小时停止开始新的非必要重构，保留最多 2 小时做 inventory、post-soak gate 和报告。
- 48 小时 deadline 到达立即停止安全修改，清理且只清理 owned 资源，按真实状态报告。
- 如果 D12 太晚导致 D13<12h，写 partial — minimum final-code soak not reached；绝不伪造时间。
- 如果实现提前完成，继续 D13，目标延长到 18h 但不越过 48h；不要写报告提前下班。
- 如果所有本地 ready 工作确实完成而只剩真人/OS/签名/凭据 blocker，仍须先完成 D12/D13/D14 才能
  判定本机范围；不能拿 blocker 跳过可靠性。

九、最终交付

最终报告必须从 manifest/metrics 生成，而不是凭记忆写。第一屏原样包含：

48h 结果：passed / partial / blocked / failed
最后完成批次：D?.?
final code fingerprint：...
最高可信证据：E?
D13 active soak：总 ...h；L1 ...h/...轮；L2 ...h/...轮；L3 ...h/...轮；L4 ...h/...轮
未提交、未推送、未提 PR

随后包含 baseline/final git、D0–D14 状态、production call graph、surface matrix、所有 Red→根因→修复
→Green、runtime/package/install、App-shell/四 surface、fault matrix、privacy/cleanup、p50/p95/max/资源
趋势、D13 全部首败、final gate 命令/exit/数量/耗时、evidence hash inventory、known debt、macOS/Linux/
real-model/E4/E5 blocker 和下一台机器唯一推荐批次。

不要把失败藏在附录，不要只写“所有测试通过”，不要引用不存在的 scratch。

十、现在开始

先只读执行 D0，不要先改代码：

1. 完整读取权威文件；
2. 核实 canonical path、branch、HEAD 和 dirty tree；
3. 核实没有另一个 writer；
4. 创建并验证 ignored evidence tree、owner.json、state.json、manifest.jsonl；
5. 生成 baseline fingerprint 和旧 evidence inventory；
6. 写 checkpoints/D0.md；
7. D0 Green 后自动进入 D1，然后按依赖连续推进 D2…D14，无需等我回复“继续”。

每完成一个 batch 就 checkpoint，但不向用户请求继续。只有用户停止、安全红线、三次不同尝试仍失败、
48h deadline 或 execution 中定义的终止条件成立才停。报告不是任务捷径，partial 不是完成，时间流逝也
不是完成。
```

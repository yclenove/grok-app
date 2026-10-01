# Computer Use：Grok 48 小时连续开发执行书

> 日期：2026-09-11  
> 工作区：`H:\aicoding\grok-app-computer-use`  
> 分支：`feat/computer-use-implementation`  
> 基线 HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`  
> 最大执行窗口：48 小时  
> 目标：完成所有当前机器上可安全完成的产品接线、可靠性和交付工作，并利用剩余窗口进行 final-code 主动长稳  
> 禁止：用 sleep、重复空跑、报告或 fixture 数量假装占满 48 小时

## 0. 本执行书解决什么问题

上一版整夜任务约 40 分钟就被判定结束，根因不是任务真的完成，而是调度器允许：

- `partial` 也算走完阶段；
- `blocked_external` 不检查同阶段的其他 ready 项；
- 五次短循环替代长稳；
- 报告/矩阵被当成开发成果；
- 报告生成后仍能改代码，却不强制重做最终验收；
- 临时证据丢失后仍可按 ledger 宣称通过。

本执行书改用不可绕过的 outcome gate。48 小时是最大任务窗口和可靠性机会，不是强制浪费资源的计时器。若所有 ready 工作、最低 12 小时 final-code 主动长稳和最终门禁确实完成，可以提前结束；否则在 48 小时截止时诚实交付 partial/blocked 报告，不能自行把未完成项改写为通过。

## 1. 总完成谓词

只有下面表达式为真，任务才允许输出 `passed`：

```text
D0..D11 所有 required batch = passed
AND D12 final-code freeze = passed
AND D13 四条必需长稳 lane 均达到各自最低主动时长与最低有效轮数
AND D13 所有安全/隔离/清理 invariant = 0 breach
AND D14 最终门禁在相同 code fingerprint 上通过
AND D14 evidence inventory 无缺失、无晚于报告的代码修改
```

以下均不满足总完成谓词：

- 阶段被写成 `partial`；
- 本地可做的生产接线被写成 `blocked_external`；
- probe/fixture 通过但没有 production call site；
- 真实浏览器通过但 App UI/Host 没有调用它；
- 5 次、30 次或 45 分钟短循环替代 D13；
- 先写最终报告，再继续修代码；
- 日志只存在 Goal scratch 或聊天文本；
- Windows 结果复制到 macOS/Linux；
- scripted agent 结果写成 real-model E4；
- 时间到了便把 partial 改成 passed。

## 2. 权威输入与优先级

开始前必须完整读取：

1. `AGENTS.md`；
2. `docs/llm-wiki/computer-use.md`；
3. `docs/plans/2026-09-09-computer-use-grok-step-plan.md`；
4. `docs/plans/2026-09-10-computer-use-grok-remaining-roadmap.md`；
5. `docs/plans/2026-09-11-computer-use-grok-post-overnight-audit.md`；
6. 本执行书；
7. `docs/plans/2026-09-09-computer-use-execution-state.md` 的最后 350 行；
8. 当前代码、当前 git status 和仍存在的原始 evidence。

发生冲突时：

1. 用户最新指令优先；
2. `AGENTS.md` 和安全边界优先；
3. 本执行书的完成语义、证据和调度优先于旧 overnight prompt；
4. 具体协议/安全约束取所有文档中更严格者；
5. 旧报告只作历史线索，不作 final truth。

## 3. 工作树和外部系统红线

### 3.1 Git 与文件

- 保留全部现有 tracked/untracked/ignored 成果。
- 禁止 `git reset`、`git clean`、`git checkout --`、`git restore`、stash、强切分支或重建工作树。
- 禁止 `git add`、commit、push、PR、merge、tag、release。
- 不修改 `H:\aicoding\grok-app` 或其他 worktree。
- 不删除不明 target、cache、temp 或进程。
- 只删除本次运行创建、路径和 ownership marker 均能证明归属本任务的目录。
- 任何覆盖式生成前先验证绝对目标位于本工作区或本次 evidence/runtime staging 内。

### 3.2 用户数据与账号

- 不读取、导出或复用账号 Token、Cookie、浏览器 storage、系统凭据或共享 `~/.grok`。
- 不改变代理、VPN、网络路由或账号配置。
- 不操作用户日常 Chrome/Edge profile。
- 浏览器测试只使用唯一隔离 `--user-data-dir`；Edge 也必须使用隔离 profile。
- 不安装到或覆盖正式 Grok App。
- 不发送外部消息、不发布 release、不触发需要凭据的真实模型任务。

### 3.3 实现红线

- production 不依赖 PATH Node、系统浏览器自动探测、在线 `latest`、任意 `eval` 或测试源码目录。
- 不用 Fake/mock/stub/feature gate 冒充产品 E3/E4/E5。
- 不新增隐藏的 production 测试后门。
- 不用 broad `allow`、skip/ignore、弱断言或吞错换 Green。
- 不降低安全、观察质量或成功率来换速度。
- 不向 `App.tsx` / `AppWorkbench.tsx` 塞大型状态；两者合计行数只能持平或下降。
- 所有 15 locale 必须保持 key 一致；非关键翻译可以明确 fallback，但不得漏 key。

## 4. 状态、证据等级和阻塞语义

### 4.1 原子 batch 状态

只允许：

- `not_started`：尚未开始；
- `in_progress`：正在做；
- `passed`：该 batch 的全部 required acceptance 在当前 fingerprint 上成立；
- `failed`：三次不同、有依据的安全尝试后仍失败；
- `blocked_external`：确实需要当前边界外的 OS、硬件、真人、签名、凭据或隔离设施；
- `invalidated`：后续代码改动使原证据失效；
- `not_run`：没有运行，不能换词写成支持。

`partial` 只允许出现在阶段汇总和最终报告，不能作为原子 batch 的完成状态。一个 batch 的部分结果必须拆成 passed 子项和仍为 not_started/blocked_external 的子项。

### 4.2 证据等级

- E0：设计/静态阅读；
- E1：单元、schema、纯函数、编译；
- E2：真实模块/进程/OS API 的隔离 integration；
- E3：产品协议路径上的 scripted agent + 真实后端 + 独立 oracle；
- E4：branch-built App + 真实模型 + 真人授权目标；
- E5：目标 OS 的安装、升级、回退、卸载和任务矩阵。

低等级证据不得填高等级格子。fixture 证据名称必须带 `fixture`。

### 4.3 `blocked_external` 的使用条件

只有同时满足以下条件才能标 blocked_external：

1. 写明缺失的外部条件；
2. 证明它不是当前代码/测试夹具/本机安全配置可以解决的问题；
3. 给出恢复所需环境、命令、oracle 和证据位置；
4. 检查并继续执行同阶段和全局其他 ready batch；
5. 不借 blocker 提前写最终报告。

“还没有 production call site”“需要写 UI”“需要做本地 transport”“测试不稳定”都不是 external blocker。

## 5. Durable evidence 协议

### 5.1 Evidence 根目录

本次证据统一放在：

```text
tools/computer-use-probe/.run/48h/<run-id>/
```

开始前必须执行 `git check-ignore -q` 证明该路径由现有 `.gitignore` 忽略；若未被忽略，停止并记录，不得把大日志意外纳入 Git。

目录结构：

```text
baseline/
checkpoints/
failures/
logs/
metrics/
reports/
state.json
manifest.jsonl
owner.json
```

不得把唯一证据放在 `%TEMP%\grok-goal-*`、聊天消息或会自动清理的 scratch。可以使用临时 staging，但每个 batch 结束前必须复制关键日志到 evidence 根并校验 hash。

### 5.2 `owner.json`

至少包含：

- run id；
- start UTC/local time；
- repo canonical path；
- branch；
- baseline HEAD；
- writer PID/session marker；
- hostname、OS、arch；
- evidence schema version；
- 允许清理的绝对目录白名单。

### 5.3 `manifest.jsonl`

每条命令/检查一行，至少包含：

```json
{
  "schemaVersion": 1,
  "seq": 1,
  "phase": "D0",
  "batch": "D0.1",
  "kind": "command",
  "argvRedacted": ["git", "status", "--short", "--branch"],
  "cwd": ".",
  "startedAt": "...",
  "endedAt": "...",
  "durationMs": 0,
  "exitCode": 0,
  "status": "passed",
  "evidenceLevel": "E0",
  "log": "logs/D0.1-001.log",
  "sha256": "...",
  "bytes": 0,
  "codeFingerprint": "...",
  "postconditions": []
}
```

规则：

- 不记录 secret env value；命令行先脱敏。
- log 必须存在，size/hash 必须与 manifest 一致。
- batch 结束时写一条 summary record。
- manifest 只追加；修正用新 record，不原地掩盖旧失败。
- `state.json` 用临时文件 + 同卷 rename 原子更新。
- 每 30–60 分钟和每个 batch 结束都写 checkpoint。
- 每 2 小时至少有一个人类可读 checkpoint；没有新进展也要写当前正在执行的真实 workload 和指标，但不得停下来等时间。

### 5.4 Code fingerprint

fingerprint 必须覆盖：

- HEAD；
- tracked diff 内容；
- nonignored untracked source/config/docs 清单和 SHA-256；
- 排除 ignored evidence、target、runtime seed/cache 等生成物；
- 当前 lock/manifest digest。

任何 production/test/harness 代码变化都生成新 fingerprint，并按 D12/D13 的失效规则处理。只改 checkpoint/report 不改变 code fingerprint。

### 5.5 证据容量

- evidence 目标上限 8 GiB，硬停阈值 12 GiB。
- 达到 8 GiB 时压缩已完成 batch 的文本日志，保留首败、末次 Green、manifest 和指标原始数据。
- 不为每轮复制完整 Chromium；使用 content-addressed runtime cache。
- 不删除当前未解决首败证据。
- 低于 20 GiB 可用磁盘或达到 12 GiB evidence 时停止新 workload，写安全报告。

## 6. 执行状态机与时间预算

每个 batch 固定执行：

```text
Read current code/state
-> write batch start record
-> establish Red or prove existing behavior with independent oracle
-> minimal implementation
-> targeted test
-> inspect diff and production call sites
-> run batch gate
-> copy/hash evidence
-> update state/checkpoint
-> select next ready batch automatically
```

推荐但不作为通过依据的时间窗口：

| 墙钟区间 | 目标 |
| --- | --- |
| 0–4h | D0–D1 证据、真值和测试隔离 |
| 4–12h | D2–D3 产品 surface 与 Managed Browser |
| 12–22h | D4 Existing Tabs + D5 WebView |
| 22–30h | D6 App-shell + D7 诊断隐私 |
| 30–34h | D8 fault/perf + D9 package + D10 质量 |
| 34–36h | D11 readiness + D12 freeze |
| 36–48h | D13 最低 12h final-code 主动长稳；若前面提前完成，长稳目标延长到 18h，受 48h 总上限约束 |

若实现阶段超过估时，继续按依赖做真实工作；不得为了赶 48h 把未完成项标 Green。到第 46 小时停止开始新的非必要重构，保留最后最多 2 小时做证据 inventory、最终门禁和报告。

## 7. 依赖图

```text
D0 evidence/baseline
  -> D1 truth + mutation isolation
      -> D2 product surface router
          -> D3 managed product closure
          -> D4 Existing Tabs product transport
          -> D5 WebView product registration
              -> D6 branch-built App-shell E3
                  -> D7 diagnostics/privacy/cleanup
                  -> D8 performance + fault harness
                  -> D9 Windows artifact/install isolation
                  -> D10 Computer Use quality debt
                  -> D11 cross-platform readiness/handoff
                      -> D12 final-code freeze + pre-soak gates
                          -> D13 active soak
                              -> D14 final gates + final report
```

可并行理解依赖，但本任务只有一个 writer，重型构建、runtime mutation、浏览器和 App lifecycle 一律串行运行。

## 8. D0：冻结现场和建立可信证据

### D0.1 工作区身份

读取并记录：

- canonical repo path；
- branch 与 HEAD；
- `git status --short --branch`；
- tracked modified 数、nonignored untracked file 数；
- ignored 生成物数量和总大小；
- target/runtime/cache/evidence 大小；
- 磁盘余量；
- 当前 OS/arch/desktop session；
- 是否有另一个 writer 或本工作树重型任务。

通过条件：路径、分支、HEAD 与本执行书一致；现有 dirty tree 被完整记录；没有做任何清理或覆盖。

### D0.2 Evidence ownership

- 创建唯一 run id 和 evidence tree；
- `git check-ignore -q` 通过；
- 写 `owner.json`、初始 `state.json` 和第一条 manifest；
- 建立本次 PID、child process、temp/staging ownership registry；
- 验证只能删除 registry 内路径。

通过条件：写入一条假 command record 后能校验 log 的 size/hash；原子 state 更新中断测试不会留下半个 JSON。

### D0.3 基线 fingerprint

- 生成 code fingerprint；
- 记录 runtime manifest/tree digest；
- 记录 App/Computer Use 新增文件行数；
- 记录当前质量 gate、typecheck、lint、runtime check 的基线状态；
- 不因已知失败修改代码。

### D0.4 旧 evidence inventory

只读检查旧 overnight evidence 与 execution-state：

- 哪些日志仍存在；
- 哪些 `{SCRATCH}` 已丢失；
- 报告时间与最后代码时间；
- 哪些结论能独立复测；
- 哪些只能标 `historical_claim_unverified`。

不得复制不存在的路径或伪造旧 hash。

### D0.5 D0 checkpoint

产出 `checkpoints/D0.md` 和 batch summary。D0 文档完成不允许结束任务，立即进入 D1。

## 9. D1：真值修复、共享 runtime 隔离和串行门禁

### D1.1 当前 claim matrix

从最终代码和独立命令重建 baseline matrix，至少分：

- Windows Desktop；
- Managed Browser；
- Existing Tabs；
- App WebView；
- runtime delivery；
- App shell；
- diagnostics/privacy；
- Windows package/install；
- macOS arm64/x64；
- Linux X11/GNOME Wayland；
- scripted model 与 real model。

每格写 implementation、production call site、E 等级、命令、oracle、status 和 blocker。报告/矩阵本身不是开发通过项。

### D1.2 找出所有 mutation-sensitive gate

静态和动态检查所有会写入以下位置的测试/脚本：

- `src-tauri/resources/computer-use/seed`；
- runtime cache；
- active/current pack；
- shared target dir；
- fixed ports；
- fixed browser profile；
- process-global env；
- execution-state/manifest。

产出机器可读清单，注明读/写范围、owner、是否可并行和清理动作。

### D1.3 修复 test isolation

要求：

- 测试不得篡改共享工作树 seed；必须复制到 unique isolated root 后再破坏。
- 每个 runtime store/profile/staging/lease/port 使用 run + batch + UUID。
- 共享 prepare/check 若必须写工作树，只能由单独 orchestrator 持锁串行执行。
- lock 包含 owner PID、start time、repo fingerprint；stale lock 只能在 owner 已死且路径吻合时回收。
- `--check` 保持只读。
- crash 后 staging 可恢复，但不能误删健康 current pack。

### D1.4 建立并发 Red/Green

必须先捕获至少一个可复现 Red：两个 mutation-sensitive gate 对共享 seed 发生冲突，或静态证明它们会写同一路径。然后修复并验证：

1. 同时请求两个 runtime mutation gate，第二个明确等待或返回 typed busy；
2. 不出现半物化 seed；
3. `--check` 在前后均通过；
4. kill owner 后 stale lock 可安全恢复；
5. 非 owner 无法删除另一个 run 的 staging；
6. 串行 orchestrator 连跑 10 轮零 flake。

并发 Red 只用于证明隔离问题；所有后续正式门禁继续串行。

### D1.5 统一 gate runner

实现或整理一个小型 runner，职责仅限：

- 串行执行 mutation-sensitive command；
- 超时和 child process ownership；
- stdout/stderr 完整落盘；
- 记录 duration/exit/hash；
- 失败即停止该 lane；
- 不隐藏 command failure；
- 不删除首败。

不得把业务断言迁进 runner，也不得写一个永远返回 0 的包装层。

D1 通过后才能开始产品接线。

## 10. D2：修复产品 surface 路由和用户入口

这是本轮第一个必须真正改产品行为的阶段。

### D2.1 先写 Browser slash Red

建立失败测试证明当前问题：

1. `/computer-use` 发出 `desktop`；
2. `/computer-use-browser` 发出 `browser`；
3. SideWorkbench 订阅后必须保留 mode；
4. `ComputerPanel` 收到对应 surface；
5. Browser mode 不调用 Desktop-only target listing；
6. session 切换/关闭后旧 mode 不闪回；
7. 两个 slash 不能落到完全相同且无 surface 信息的 tab state。

### D2.2 建立 typed surface state

设计并实现最小 typed state，不向 `AppWorkbench.tsx` 增长大型逻辑：

- `desktop`；
- `managed-browser`；
- `existing-tabs`；
- `app-webview`。

可以用 domain store/controller 或 SideWorkbench tab metadata。禁止用字符串散落、全局临时变量或 `any`。

### D2.3 Host target router

当前 Broker 主 adapter 是 OS Desktop，browser registry 是另一条路径，WebView 尚未注册。需要建立明确 router/facade：

- list capabilities by surface；
- list/start/bind targets by surface；
- authorize/grant by surface；
- surface-specific stop/revoke/return；
- 空/unsupported surface fail closed，不回退 Desktop；
- session/run/surface 写入同一 ownership 和 trace；
- MCP schema 与 UI command 使用同一枚举和错误码。

不要求把所有 backend 塞进一个巨型 adapter；要求产品调用关系清楚、可测试且无隐式 fallback。

### D2.4 Tauri typed commands

补齐 surface-aware command contract，例如：

- capabilities/status；
- list targets；
- managed profile start/list/open；
- Existing Tabs pair/share/list/revoke/return；
- WebView list/bind/unbind；
- pause/resume/takeover/stop；
- runtime diagnose/repair。

最终命名可结合现有 API，但必须有 Rust/TypeScript 对称类型、serde golden 和权限检查。不得让 UI 直接拼内部 MCP payload。

### D2.5 UI 信息架构

Computer 面板至少包含：

- 当前 surface 明示；
- capability/unavailable 原因；
- surface 对应的 target/profile/tab 选择；
- authorize/pair/share 的双确认状态；
- pause/takeover/resume/stop；
- preview 可见性；
- concise diagnostics；
- Repair/rollback 只在 runtime issue 时出现。

`/computer-use-browser` 默认打开 Managed Browser；用户可以显式切到 Existing Tabs。`/computer-use` 默认 Desktop。切 surface 必须 revoke/return 旧 surface 的临时 grant，不能静默继承。

### D2.6 D2 acceptance

- 前端 Red 全部 Green；
- Rust command/schema tests Green；
- `/computer-use-browser` 的 production call trace 明确进入 Managed Browser path；
- `/computer-use` 仍进入 Desktop；
- unsupported surface 零 Desktop fallback；
- 15 locale key 一致；
- `WorkbenchComposerShell` 不新增 `any`；
- App shell 行数不增长；
- 至少 20 次交替打开两种 slash，无 mode/session 串线。

## 11. D3：Managed Browser 产品闭环

### D3.1 产品调用图

在改代码前画出并核实：

```text
slash/surface selector
-> Tauri command
-> session/run ownership
-> Broker browser registry
-> BrowserSupervisor
-> App-owned Node/worker/Chromium
-> MCP tools
-> observe/act/verify
-> UI status/preview/stop
```

每条边必须有 production call site。缺边先写 Red，不准用 probe 里的直接构造替代。

### D3.2 Managed profile lifecycle

实现并验证：

- profile create/start/list/open；
- App/session/run ownership；
- two-session isolation；
- popup/new tab/iframe/download staging；
- navigation/document/page generation；
- close/restart/recover；
- stop 后零新派发；
- App close/crash 后 child tree 和 profile staging 有确定状态；
- profile 清理不越界；
- 不扫描/复用系统 Chrome profile。

### D3.3 MCP scripted product route

现有 `mcp-scripted-agent` 要保留，但增加一条经过 D2 production surface command/router 的 gate。scripted client 只能使用模型可见工具，至少执行：

```text
capabilities
-> list/start managed target
-> open page
-> navigate
-> observe
-> typed act
-> independent verify
-> popup/iframe/download scenario
-> pause/resume
-> stop
```

覆盖：

- duplicate actionId replay；
- conflicting replay；
- stale snapshot/generation；
- host-only tool 拒绝；
- malformed payload；
- timeout/cancel；
- two sessions；
- trace/query/input/token redaction。

### D3.4 D3 acceptance

- 真实 packaged runtime，禁止 `GROK_CU_NODE_FILE`/source worker 逃逸；
- production surface route scripted E3 连续 10 轮；
- 每轮有独立页面/文件 oracle；
- 每轮 worker/browser PID、profile/staging、lease 后置可验证；
- 0 duplicate side effect；
- 0 cross-session leak；
- 0 residual owned process；
- 真实模型仍标 `not_run`，不能因此阻塞 D4/D5。

## 12. D4：Existing Tabs 产品 transport

### D4.1 先写 ADR，不先堆 UI

比较 Chrome Native Messaging 与 authenticated loopback transport，至少评估：

- Windows/macOS/Linux 安装和升级；
- Chrome/Edge MV3 支持；
- secret 暴露面；
- extension id 绑定；
- App instance/session/run/tab/document/connection generation；
- revoke/rotate；
- browser restart 与 App restart；
- enterprise policy；
- 可测试性和最小权限。

选定一个 v1 transport 并解释取舍。产品方案必须满足：

- pairing secret 不放 URL query、fragment、页面 DOM 或普通日志；
- 只绑定 loopback/native host 和预期 extension id；
- challenge 一次性、短 TTL、用后即焚；
- App 和扩展两端都需要显式确认；
- 重放、错 instance、错 origin、错 extension、过期 challenge 全拒绝。

### D4.2 Extension package

- 独立、可审查的 MV3 extension source；
- manifest 权限最小化；
- 不默认读取全部 tab；
- 用户在扩展 UI 明确点击 share 当前 tab；
- content script 只在已共享 tab/origin 生效；
- source/version/hash/license/package manifest；
- release package 不包含测试 server、CDP helper、fixtures 或 secret；
- Chrome/Edge 安装说明与状态检查。

### D4.3 App Host service

实现 production service：

- feature 开启时按需启动；
- transport endpoint/host manifest 为 App-owned；
- extension handshake；
- pending challenge store；
- paired connection registry；
- shared offers；
- tab discovery；
- session/run grant；
- typed observe/act/verify；
- revoke/rotate/return；
- disconnect/reload/browser close/App close 的资源回收；
- 错误码和脱敏 trace。

禁止在 product command 中创建新的临时 `ExistingTabHost` 后手工塞 response；必须连接 process-wide Broker/registry。

### D4.4 UI 双确认

App UI 至少显示：

- transport/extension 是否安装；
- extension id 与浏览器类别；
- 短码/设备名，不显示 secret；
- 待确认、已配对、已共享、已借用状态；
- tab title/origin 的最小必要信息；
- share、confirm、revoke、rotate、return；
- 浏览器导航后需重新授权时的明确提示。

### D4.5 安全和 lifecycle Red

必须覆盖：

1. secret 不在 URL/history/log/trace；
2. challenge replay；
3. wrong extension id；
4. wrong App instance；
5. expired challenge；
6. 未共享 tab 不可枚举；
7. session A 不能看 session B；
8. navigation/document generation 后旧 ref 失效；
9. extension reload 后 connection generation 更新；
10. revoke 后 in-flight 有确定结果且零新派发；
11. return 不关闭用户 tab；
12. App 不覆盖 fixture 模拟的用户主动导航；
13. tab close/browser exit/App exit 后 registry 清理；
14. malformed extension payload fail closed；
15. 日志和 support bundle 无 query/form/cookie/token。

### D4.6 Chrome/Edge 隔离 E3

- Chrome for Testing：唯一隔离 profile，连续 10 轮；
- Edge：仅使用 browser binary + 唯一隔离 profile，绝不打开默认用户 profile，连续 10 轮；
- 每轮从 product Host service 配对，不直接调用内存 host；
- 每轮真实 tab DOM/action oracle；
- 每轮 revoke/return/cleanup；
- Edge binary 或安全 extension load 条件缺失可单独标 blocked_external，但不影响 Chrome product transport 工作继续。

D4 通过最多代表 Existing Tabs product transport scripted E3；没有真人安装/登录/授权仍不是 E4。

## 13. D5：App-owned WebView 产品注册

### D5.1 生产调用 Red

先写测试证明当前没有：

- process-wide `WebViewAdapter`；
- `bind_side_browser()` 生产调用；
- WebView target router registration；
- session/run ownership；
- UI surface target listing。

fixture 内自己 `WebViewAdapter::new()` 不能让这些 Red 变 Green。

### D5.2 注册与 lifecycle

实现：

- App-owned side-browser label registry；
- process-wide WebView backend；
- explicit bind/unbind；
- session/run/label/tab ownership；
- navigation、document、DOM generation；
- window/tab destroy cleanup；
- App navigation 与 Computer Use action 的竞态 fence；
- 空 target/死 target fail closed；
- 不回退 Desktop/Managed Browser。

### D5.3 Typed subset

v1 仅允许：

- bounded semantic snapshot；
- opaque element refs；
- typed click；
- typed input；
- scroll；
- navigate；
- observe/verify。

明确拒绝：

- model-provided eval/script；
- Cookie/storage/auth export；
- Grok 相册、登录、聊天、壁纸 WebView 的认证迁移；
- cross-origin iframe 内部操作；
- 浏览器权限 UI；
- 未设计的下载和文件系统逃逸。

内部实现即便使用 WebView evaluate，也必须只执行 Host 固定模板和 schema 化参数，模型不能提供代码或 selector 任意拼接。

### D5.4 D5 acceptance

- production call graph 每条边存在；
- branch-owned真实 WebView fixture，不是纯内存 fake；
- bind/list/observe/act/verify/unbind 连续 20 轮；
- navigation/DOM mutation 使旧 ref 稳定失败；
- two-session/两 WebView 不互串；
- forbidden auth/eval/cross-origin 全 fail closed；
- close/stop 后零派发、零 residual registry；
- UI surface 可以选到 App WebView 并正确显示 unsupported 项。

## 14. D6：branch-built App-shell scripted E3

已有 probe 不能替代真实 App process。本阶段要证明产品 shell、Tauri command、session manager、MCP 和 backend 真正连在一起。

### D6.1 隔离 App 运行方案

先确认 App data root 的正式覆盖机制。若现有 `GROK_APP_HOME` 不能完整隔离 store/session/runtime/profile，不得假设有效；先补明确、测试覆盖的开发/测试隔离入口，且 production 默认路径不变。

要求：

- branch-built binary；
- 独立 app id 或其他不会抢占正式 App single-instance/registry 的 test config；
- 唯一 isolated App home；
- 唯一 runtime/profile/staging/lease；
- loopback mock ACP 或现有受支持的本地 scripted agent；
- 不读取真实 Grok account/token/cookie；
- 不安装覆盖正式 App；
- 启动前后记录 PID tree 和端口 ownership。

### D6.2 不加隐藏生产后门

优先使用：

- Tauri test config；
- dev-only harness binary；
- Windows UI Automation 驱动真实控件；
- mock ACP 的公开协议；
- Computer Use 正式 Tauri commands/MCP。

禁止在正式二进制中加入“收到特殊 secret 就跳过授权”的路径。任何 test-only feature 必须编译隔离，并由 package audit 证明不进入 release artifact。

### D6.3 App-shell 场景

至少覆盖：

1. fresh home feature 默认关闭；
2. 设置页开启 feature；
3. 本地 chat 连接；
4. `/computer-use` 打开 Desktop；
5. `/computer-use-browser` 打开 Managed Browser；
6. target 选择与授权；
7. MCP 注入只发生在授权 local interactive session；
8. scripted agent observe/act/verify；
9. pause/takeover/resume；
10. stop requested → stopped；
11. session switch 不闪回旧 preview/status；
12. panel hidden 停止 preview；
13. Existing Tabs product fixture 配对/share/revoke；
14. App WebView bind/act/unbind；
15. App close/restart 后恢复或给出准确诊断。

### D6.4 独立 oracle

UI 显示“成功”不算唯一 oracle。每个写动作必须用至少一个独立后置：

- 自建 Windows fixture 控件值/计数器；
- 自建页面 DOM；
- isolated download 文件 hash；
- child PID/handle；
- App store 的非敏感状态；
- mock ACP 收到的 MCP tool record。

### D6.5 D6 acceptance

- branch-built App-shell 全链连续 10 轮；
- 至少 Desktop、Managed、Existing fixture、WebView 各 2 个真实场景；
- feature-off、unauthorized、stale、stop 后派发均为零；
- App close 后 owned child/process/port 清理；
- 失败时 App UI 显示稳定脱敏错误；
- 没有真实模型时明确写 `scripted App-shell E3`，不写 E4。

## 15. D7：诊断、隐私、retention 和清理产品化

### D7.1 Threat inventory

建立 sentinel 列表和数据流：

- Authorization/Bearer；
- API key/token/cookie；
- proxy credential；
- URL query/fragment；
- form/input/clipboard 文本；
- screenshot/PNG signature；
- browser profile path；
-用户目录绝对路径；
-页面正文；
- extension pairing material。

列出 model、UI、local trace、support bundle、stdout/stderr、crash report、manifest 的 audience 与允许字段。

### D7.2 Bounded trace store

实现 record、byte、age 三维上限：

- 每 run 上限；
- 全局上限；
- 淘汰策略确定；
- oversize 单条拒绝或截断并记录 code；
- screenshot、输入、Cookie、query 默认不持久化；
- UI/model/support audience 分离；
- restart 后不会复活已清理敏感内容。

测试至少覆盖边界值、并发 append、clock skew、corrupt store 和清理后重新启动。

### D7.3 Support bundle

用户可导出但默认不自动上传。内容只包括：

- App/OS/arch/version；
- Computer Use capability；
- runtime manifest digest 和 component version；
- permission/grant 状态的非敏感摘要；
- worker health；
-最近稳定错误码；
-脱敏性能/资源指标；
- evidence schema/version。

流程必须：

1. 建 staging；
2. 每个文件第一次脱敏；
3. 压缩；
4. 重新打开 archive，逐 entry 解压扫描；
5. sentinel 命中则拒绝交付并保留安全摘要；
6. 成功后清理 staging。

### D7.4 分离清理

分别提供并测试：

- clear traces；
- clear run staging/downloads；
- clear managed profiles；
- revoke Existing Tabs pair/grants；
- reset Computer Use runtime pack/Repair（不同于用户数据清理）。

每个动作 UI 明确影响、需要确认、只删 App-owned 路径。测试 symlink/junction、`..`、盘符大小写、UNC、根目录、空路径和非 owner marker，全部 fail closed。

### D7.5 D7 acceptance

- 100% sentinel suite 不泄漏；
- archive 二次扫描真实执行；
- retention 在 10 倍超限输入下仍有界；
- 分离清理不误删其他 lane 数据；
- cleanup 后磁盘回到允许基线；
- UI 不直接 `JSON.stringify` 输出未经 audience filter 的未来新增字段；
- support bundle 命令和 UI 有定向测试及 App-shell smoke。

## 16. D8：可复现性能与 fault-injection harness

### D8.1 分段计时

固定自建 fixture 和机器条件，记录：

- target discovery；
- capture；
- semantic extraction；
- PNG encode；
- MCP stdio；
- loopback request；
- Broker validation/dispatch；
- backend apply；
- independent verify；
- profile/browser cold start；
- warm page action；
- stop/cleanup。

输出 JSONL 原始样本和汇总 p50/p95/max/failure rate。cold/warm 分开，不能混成一个平均值。

### D8.2 资源采样

至少记录：

- App、Host/worker、Node、Chromium PID tree；
- process private bytes/working set；
- handle count；
- CPU time；
- thread count；
- profile/staging/trace/evidence 磁盘；
- queue/in-flight/preview counters；
- open port/lease count。

Windows 进程 CPU 必须按逻辑处理器归一化。资源 sampler 失败不能让业务 gate 假 Green，应单独标 telemetry failure。

### D8.3 Fault matrix

至少覆盖以下故障，每项先规定 expected code/state/oracle：

1. Node spawn failure；
2. worker handshake timeout/version mismatch；
3. worker process kill；
4. Chromium root process kill；
5. tab crash/close；
6. navigation during action；
7. stale snapshot/ref/generation；
8. malformed MCP JSON/oversize frame/半包；
9. wrong/missing/replayed Bearer；
10. loopback connection reset/slow response；
11. App session switch；
12. pause/takeover/stop during in-flight；
13. extension disconnect/reload；
14. Existing tab user navigation；
15. WebView destroy/navigation；
16. runtime file corrupt/missing；
17. interrupted Repair/rollback；
18. trace/store corrupt；
19. staging path permission denied；
20. cleanup symlink/junction escape。

故障注入只能作用于本任务自建进程和目录。不得断用户网络、杀用户浏览器或改系统代理。

### D8.4 Fault acceptance

每个故障至少 5 次，且必须：

- 返回预期 typed code；
- 不误派发到其他 backend；
- 不重复副作用；
- 不泄漏 secret/path/content；
- state 可恢复或明确需要用户 Repair；
- owned process/handle/port/staging 最终可清理；
- 首败原始证据保留。

### D8.5 相对性能门槛

没有历史硬阈值时不编造绝对 SLA，但必须在同一机器、同一 fixture、同一 fingerprint 上建立 baseline/final A-B：

- final p95 不得比同批 baseline 回退超过 20%，除非能定位为安全/正确性必需成本并在报告中量化；
- failure rate 必须为 0（预期 fault 除外）；
- action 独立 oracle 成功率 100%；
- stop 后 child count 为 0；
- warmup 后最终 25% 的 handle p95 不高于首个稳定 25% `max(+20, +5%)`；
- private bytes 最终 p95 不高于首个稳定 p95 `max(+64 MiB, +15%)`；
- trace/profile/staging 不突破配置上限；
- cleanup 后临时磁盘距离基线不超过 1 MiB（保留 evidence 除外）。

超出相对门槛必须定位、修复或诚实 failed；不能删样本或只展示平均数。

## 17. D9：实际 Windows bundle 与隔离安装生命周期

### D9.1 Build target/config 证明

- 验证 Tauri hook 实际收到正确 target triple；
- 验证 `tauri.windows.conf.json` 在 Windows build 中自动/显式 merge；
- 非 Windows target 不含 Windows runtime seed；
- target 缺失时 fail closed；
- CI/release/local build 三条路径都有自动化契约。

### D9.2 实际 artifact audit

构建 branch artifact 后逐文件检查：

- Node、Playwright、worker siblings、Chromium、manifest、lock、NOTICE/license；
- architecture/version/hash；
- `cu_probe.exe`、fixtures、tests、source tools、download cache、zip、`.run`、dump、用户数据、secret、绝对开发机路径不得进入包；
- setup/portable/resources 总大小和相对主干增量；
- unpack 后重新计算完整 tree digest；
- 启动 packaged runtime contract。

### D9.3 NSIS 模板漂移

从本地锁定的 `tauri-bundler 2.11.5` source 取得确切 upstream NSIS template，做结构化 diff：

- 只保留为排除 probe/Computer Use runtime 所需的最小差异；
- 上游关键安装/卸载/security 片段不能丢；
- 增加 automated assertion，依赖版本变化时 fail 并要求重审；
- 不从网络 `latest` 临时抓模板。

### D9.4 隔离安装生命周期

只有具备 Windows Sandbox、一次性 VM 或不会触碰正式 App identity/registry 的专用 test config 时才执行：

1. clean install；
2. first launch default off；
3. enable + runtime diagnose；
4. scripted App-shell E3；
5. upgrade；
6. same-version runtime corrupt/Repair；
7. rollback；
8. uninstall；
9. 残留文件/进程/registry 检查。

没有安全隔离设施时，D9.4 标 blocked_external，并给出 Windows Sandbox 恢复手册；D9.1–D9.3 仍必须完成，不能整阶段跳过。

## 18. D10：只清理 Computer Use 自身质量债

### D10.1 必修问题

- 去掉 `WorkbenchComposerShell.tsx` 新增的 `(item: any)` wrapper，恢复/补齐正确类型；
- 修正 `AppWorkbench.tsx` named import 格式；
- 修复 browser mode 被丢弃后的重复/无用逻辑；
- 缩小或删除 `computer_use/mod.rs` 顶层 `dead_code/unused_imports` allow；
- 不让 probe-only API 混进 release surface；
- 新 production module 有明确职责和 owner。

### D10.2 拆分大文件

优先拆分：

- `browser_supervisor_gates.rs`；
- `webview.rs`；
- `windows_adapter.rs`；
- `computer-use-core/src/runtime.rs`；
- `computer-use-core/src/runtime_prepare.rs`；
- `computer-use-core/src/broker/gates.rs`。

要求：

- 只做机械/职责边界重构，不顺手改协议语义；
- public API 尽量不变；
- 每次拆一个模块，定向测试后再拆下一个；
- Computer Use 新 production file 目标 ≤800 行；
- test/gate file 目标 ≤1000 行；
- 仓库 `FILES_OVER_1K_BUDGET <= 80` 必须恢复；
- 不重构无关的旧大文件来“做数字”。

### D10.3 D10 acceptance

- final code-quality gate passed；
- typecheck/lint/fmt/clippy passed；
- Computer Use 定向和 core 全量 passed；
- App shell ceiling 不增长；
- `git diff --check` passed；
- production build 不含 probe-only symbol/artifact；
- 行数、模块依赖和公开 API diff 写入 checkpoint。

## 19. D11：跨平台 readiness 与实机 handoff

跨平台仍是 v1 目标，但当前 Windows 主机不能伪造物理平台验收。

### D11.1 当前机器可做

- 审查共享协议是否无 Windows-only path/类型泄漏；
- 静态/编译层修复可被当前 toolchain 可靠验证的 cfg 问题；
- runtime pack schema 支持 target-specific manifest；
- 为 macOS arm64/x64、Linux X11、GNOME Wayland 分别写可粘贴 handoff；
- 记录现有 `not implemented` 动作和安全能力差距；
- CI matrix 只证明 compile/test，不能写 E3/E4/E5。

### D11.2 当前机器禁止

- 在 Windows 上凭猜测写无法编译运行的 native FFI；
- 把 cross compile、WSL、容器或 unit test 当真实桌面；
- 把一个 Linux 桌面结果同时填 X11 和 Wayland；
- 把 macOS arm64 结果复制为 x64；
- 自行 SSH 找机器或凭据。

### D11.3 Handoff 必须包含

每个平台分别给：

- exact branch/fingerprint；
- 环境/依赖/权限；
- build/run command；
- target-specific runtime prepare/check；
- Desktop action fixture；
- Managed/Existing/WebView 场景；
- permission allow/deny/revoke；
- screenshot/scale/multi-display；
- crash/stop/cleanup；
- E4/E5 人工步骤；
- evidence 目录和 manifest 格式；
- 成功/失败/blocked 的判定。

D11 在 Windows 上只能写 readiness/handoff passed；macOS/Linux E3/E4/E5 保持 `not_run/blocked_external`。

## 20. D12：最终代码冻结和 pre-soak gate

D12 是长稳起点。没有 D12，任何之前累计的运行时间都不计入 D13 最低 12 小时。

### D12.1 修复完成声明

确认：

- D0–D11 所有当前机器 ready 子项均 passed；
- remaining external blocker 均有精确 handoff；
- 没有 in-progress implementation；
- 没有未审阅 diff；
- 没有后台 writer；
- 没有正式 App/用户浏览器/共享 profile 被占用；
- evidence manifest 当前完整。

### D12.2 串行 pre-soak gate

在同一 code fingerprint 上串行执行并记录原始 exit、数量、耗时：

1. `git diff --check`；
2. Computer Use runtime `--check`；
3. Browser worker Node tests；
4. MCP golden/HTTP tests；
5. Computer Use frontend tests；
6. `pnpm typecheck`；
7. `pnpm lint`；
8. `cargo fmt --all -- --check`；
9. core clippy `-D warnings`；
10. core `--lib --offline`；
11. driver/test-support tests；
12. `cargo check -p grok-app --offline`；
13. D2 surface router gate；
14. D3 production Managed route；
15. D4 Chrome product transport；
16. D5 WebView product bind；
17. D6 App-shell scripted E3 smoke；
18. D7 privacy/support-bundle/cleanup；
19. D8 fault harness smoke；
20. actual Windows package audit；
21. `pnpm deps:check`；
22. full `pnpm test`；
23. `pnpm build:ui`；
24. `pnpm audit:prod`；
25. code-quality final gate；
26. locale parity；
27. fresh config default off；
28. package contains no probe/test/secret/path。

`pnpm audit:prod` 若仍因既有 Tiptap 漏洞失败，必须保留完整依赖、严重度和可升级范围。它可以作为独立 external/upstream risk 阻止“release-ready”，但不得驱动无审查的大版本升级。其他由本分支导致的门禁失败必须修复并重新开始 D12。

### D12.3 Freeze record

pre-soak 全部满足对应判定后写：

- freeze fingerprint；
- exact commands/log hashes；
- runtime/package digest；
- starting metrics baseline；
- D13 planned lanes；
- 当前 remaining external blockers。

从此任何 production、test、harness、manifest、lock、build config 变化都会使 D12 与所有 D13 运行时间失效，必须修复后重新执行 D12 并把 D13 有效时长归零。只追加 evidence/checkpoint/report 不失效。

## 21. D13：不少于 12 小时的 final-code 主动长稳

### 21.1 为什么必须是真实主动长稳

D13 不是 `sleep 12h`。每个有效 iteration 都必须执行真实 workload、独立 oracle、cleanup 和资源采样。无 workload 的等待、编译时间、修 bug 时间、D12 之前的循环都不计入最低有效时长。

总任务窗口最多 48 小时：

- D12 在第 36 小时前完成：D13 最低 12 小时；若更早完成，目标延长至 18 小时，但不越过总 48 小时。
- D12 在第 36 小时后完成：利用剩余时间跑尽 D13，并最终诚实标 `partial: minimum soak not reached`。
- 不允许通过降低 lane 数、轮数或 oracle 来“赶过线”。

### 21.2 四条必需 lane

每条 lane 必须满足自己的 active wall-clock 和有效轮数；各 lane 串行轮转，不并发重型任务。

| Lane | 最低累计主动时长 | 最低有效轮数 | 每轮真实工作 |
| --- | ---: | ---: | --- |
| L1 Desktop/App lifecycle | 2h | 40 | branch-built App/Host，target authorize，observe，click/type/CJK/key/scroll/drag，独立控件 oracle，pause/resume/stop，cleanup |
| L2 Managed Browser/MCP | 4h | 120 | product surface → MCP → packaged worker/Chromium，navigate/form/popup/iframe/download，独立 DOM/file oracle，stop/cleanup |
| L3 Existing Tabs + WebView | 3h | 各 50 | product transport pair/share/act/revoke/return；WebView bind/observe/typed act/navigation/unbind；真实 DOM oracle |
| L4 Fault/restart/privacy/package | 3h | 60 | D8 fault 轮转、App/worker/browser restart、runtime Repair isolated copy、support bundle/leak scan、package smoke、cleanup |

总最低累计主动时长 12h。一次 iteration 只能归属一条 lane，不能重复计算。

### D13.3 Workload 轮转

必须使用 deterministic seed schedule，覆盖而非随机碰运气：

- 5 个 viewport/DPI 组合（只在当前 fixture 能真实设置时）；
- 中文、英文、emoji 和长文本；
- DOM reorder、same-name target、popup、iframe boundary；
- rapid session switch；
- pause/takeover/resume/stop at different phases；
- normal、slow、timeout、kill、disconnect、stale generation；
- clean/warm browser start；
- trace/support-bundle threshold boundary；
- Repair/rollback 只作用于 isolated copy。

每个 seed/场景的预期 oracle 必须在执行前固定。失败后不能换 seed 或删样本让统计变绿。

### D13.4 每轮 required record

每轮至少写：

- lane、scenario、seed；
- start/end/duration；
- code fingerprint；
- App/worker/browser PIDs；
- action/observation ids；
- oracle 结果；
- expected/actual error code；
- CPU/private bytes/working set/handles/threads；
- queue/in-flight/preview/trace counts；
- profile/staging/evidence/disk bytes；
- cleanup postconditions；
- log hash。

记录失败本身不能抛出业务失败；evidence writer 失败则立即停止 lane，以免形成“无证据长稳”。

### D13.5 Heartbeat

- 每 30–60 分钟写机器 heartbeat；
- 每 2 小时写人类 checkpoint；
- checkpoint 包含 lane 累计 active time、有效轮数、失败数、资源趋势、当前 fingerprint 和下一场景；
- heartbeat 由实际 iteration 触发，禁止单独 sleep 等 heartbeat；
- 单轮间必要 cooldown 最多 30 秒，不计 active time；
- 没有工作时应继续下一个安全场景，而不是等到 48 小时。

### D13.6 首败策略

任一 invariant breach：

1. 立即停止相关 lane；
2. 保存首败全量脱敏证据；
3. 确认只终止本任务 owned process；
4. 最多三次不同、有依据的诊断尝试；
5. 若需要改 code/harness，标 D12/D13 invalidated；
6. 修复、跑 targeted/full pre-soak gate；
7. 新 fingerprint 下 D13 有效时长从 0 重计；
8. 若三次仍失败，相关 batch failed，停止宣称 passed，但继续其他不依赖 lane 以收集边界。

不能靠重跑把首败冲成 flake；最终报告必须包含所有首败。

### D13.7 Invariants

所有 lane 全程要求：

- wrong-target action = 0；
- unauthorized action = 0；
- stop-confirmed 后新派发 = 0；
- duplicate side effect = 0；
- cross-session/profile/tab/WebView leak = 0；
- secret/query/form/screenshot leak = 0；
- user browser/profile touched = 0；
- residual owned process/port/lease after cleanup = 0；
- unbounded trace/profile/staging growth = 0；
- package/source/runtime escape = 0；
- independent oracle success = 100%（预期 fault 除外）；
- resource trend 满足 D8 相对门槛。

### D13.8 D13 通过条件

- freeze fingerprint 从头到尾未变；
- 四 lane 各自达到最低 active time 和最低有效轮数；
- 累计 active time ≥12h；
- 所有 invariant 0 breach；
- 无缺失 log/hash/metric；
- 所有 owned process/staging 已回收；
- 最后一次 iteration 后 runtime check 和 App-shell smoke 仍 Green。

“总进程运行了 12 小时”不等于 D13 passed。

## 22. D14：最终门禁、证据 inventory 和报告

### D14.1 Final gates

D13 后在相同 fingerprint 上再次执行 D12.2 的所有适用门禁。不能引用 pre-soak 结果代替 post-soak 结果。

### D14.2 Evidence inventory

机器校验：

- manifest JSONL 每行可解析；
- seq 单调且无重复；
- 每个 referenced log 存在；
- size/hash 一致；
- 每个 passed batch 有 summary；
- D13 lane active time 从 iteration duration 求和，而非手填；
- 每个轮数可从记录重算；
- fingerprint 与 freeze/final 一致；
- 没有代码文件 mtime/diff 晚于 final gate 却未重跑；
- 没有日志只指向已删除 `%TEMP%`；
- privacy scanner 对 evidence 和最终 report 再跑一次。

inventory 不通过，最终状态不得为 passed。

### D14.3 最终报告生成顺序

严格顺序：

```text
停止代码修改
-> final fingerprint
-> post-soak gates
-> evidence inventory
-> privacy scan
-> 生成 draft report
-> 再检查没有代码变化
-> 生成 final report + report hash
-> 只读 git status
-> 停笔
```

报告写完后若再改任何 code/test/harness/build config，报告自动作废，必须回到 D12。

### D14.4 最终报告第一屏

必须原样包含：

```text
48h 结果：passed / partial / blocked / failed
最后完成批次：D?.?
final code fingerprint：...
最高可信证据：E?
D13 active soak：总 ...h；L1 ...h/...轮；L2 ...h/...轮；L3 ...h/...轮；L4 ...h/...轮
未提交、未推送、未提 PR
```

然后依次写：

- baseline 和最终 git 状态；
- D0–D14 每个 batch 状态；
- 产品调用图和 surface matrix；
- 每个 Red → root cause → fix → Green；
- runtime/build/package/install；
- App-shell/Managed/Existing/WebView；
- fault matrix；
- privacy/support bundle/cleanup；
- p50/p95/max/failure/resource trend；
- D13 lane 统计和所有首败；
- final gates 原始数量/耗时/exit；
- evidence inventory/hash；
- known debt；
- macOS/Linux/real model/E4/E5 blocker；
- 下一台机器/真人批次唯一推荐。

不得写“所有测试通过”而不列命令、数量、耗时和范围。不得把 `pnpm audit`、code-quality、soak 或 package failure 藏在附录。

## 23. 48 小时截止与提前停止

### 23.1 可以提前结束的唯一成功条件

只有总完成谓词成立才允许在 48 小时前以 `passed` 结束。完成实现后必须继续 D13，不得用“ready 项都已有状态”提前收工。

### 23.2 必须停止修改的条件

- 用户/Codex 发来停止或覆盖指令；
- 同一 P0/P1 经三次不同安全尝试仍无法恢复；
- 继续会触碰正式 App、用户数据、账号、代理、用户浏览器或不明进程；
- 磁盘低于安全线或 evidence 达硬阈值；
- 检测到另一个 writer；
- 48 小时 deadline 到达；
- 当前所有 ready 工作均完成，但 D13 因剩余窗口不足未达最低值：停止并写 partial，不伪造时长。

### 23.3 截止时的诚实状态

- 功能完成但 soak <12h：`partial — implementation ready, minimum final-code soak not reached`。
- 本地产品接线未完成：`partial` 或 `failed`，不能写 external blocker。
- 仅 macOS/Linux/真人/签名/真实模型缺失：相关格 `blocked_external/not_run`，Windows shared 部分按证据单独判定。
- final report/evidence stale：`failed — evidence integrity`。

## 24. 禁止的“凑两天”行为

- `sleep` 循环或空 heartbeat；
- 无限重复同一 smoke；
- 不带 oracle 的截图/点击循环；
- 为增加轮数重复 unit test；
- 把编译等待计入 soak；
- 改小测试阈值让失败消失；
- 删除 flake/失败样本；
- 写大量文档代替 production call site；
- 建 Fake backend 代替 Existing Tabs/WebView 产品 transport；
- 到时间自动把状态改成 passed。

## 25. 交接文件

任务结束时，evidence `reports/` 至少包含：

- `final-report.md`；
- `status-matrix.md`；
- `red-green-index.md`；
- `fault-matrix.md`；
- `performance-summary.json`；
- `soak-summary.json`；
- `evidence-inventory.json`；
- `macos-arm64-handoff.md`；
- `macos-x64-handoff.md`；
- `linux-x11-handoff.md`；
- `linux-gnome-wayland-handoff.md`；
- `windows-install-handoff.md`（若 D9.4 blocked）；
- `real-model-e4-handoff.md`。

仓库 execution-state 只追加简明 checkpoint 和 evidence 相对路径，不粘贴巨量日志，不把临时绝对路径当唯一证据。

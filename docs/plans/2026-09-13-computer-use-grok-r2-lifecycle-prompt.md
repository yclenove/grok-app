# Grok long-task prompt: R2 lifecycle and Existing Tabs

Paste the complete prompt below into Grok Goal mode. Do not remove the safety,
evidence, timing, or Git clauses.

---

你现在接手 Grok App 的 Computer Use 长任务。使用 Goal/长期执行模式持续推进，
不要只复述计划、跑一遍单测后就宣布完成。本任务的目标是先修复授权/MCP 生命周期
的 P0，再在同一套生命周期之上完成安全的 Existing Tabs 第一版。预计执行窗口 48
小时；时间只是排程，不是完成证据。必须在最终代码冻结后完成规定的真实 active soak，
不得用 sleep、编译时间、静态检查或历史 evidence 冒充。

## 工作现场

- 仓库：`H:\aicoding\grok-app-computer-use`
- 分支：`feat/computer-use-implementation`
- 基线 HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`
- 工作树有大量必须保留的 tracked/untracked/ignored 修改。
- 当前总状态：`partial - not releasable`
- 审查输入 fingerprint：
  `26406035517b6856e1deb6a9e7b3e2103b48391bcfa4e72215d02d6766b34fd5`
- 上述 fingerprint 在本提示词/计划文档写入后已经过期；开始时必须重新计算。

## 开始前全文阅读

按顺序完整读取，不要只 grep 标题或摘要：

1. `AGENTS.md`
2. `docs/llm-wiki/computer-use.md`
3. `docs/BUILD.md`
4. `docs/plans/2026-09-13-computer-use-codex-r2-atomic-audit.md`
5. `docs/plans/2026-09-13-computer-use-grok-r2-lifecycle-execution.md`
6. `docs/plans/2026-09-13-computer-use-surface-executor-adr.md`
7. `docs/plans/2026-09-13-computer-use-existing-tabs-pairing-adr.md`
8. `docs/plans/2026-09-13-computer-use-blocking-http-adr.md`
9. 当前授权、SessionManager、SessionGrants、Broker、IPC、WebView、Managed
   Browser、extension 和 App-shell harness 源码全文。

旧计划、旧报告或代码与新的 atomic audit 冲突时，以新的审查和更严格的安全条件为
准。不要修改旧 evidence 来让历史看起来通过。

## 绝对边界

### Git 和文件

- 禁止 reset、clean、restore、checkout 文件、stash、强制换分支或重建 worktree。
- 禁止 git add、commit、push、PR、merge、tag、release。
- 不得删除、覆盖、回滚现有 tracked/untracked/ignored 文件。
- 不修改 `H:\aicoding\grok-app` 或其他 worktree。
- 编辑前检查文件最新内容、mtime 和 diff；若有其他 writer，停止修改冲突文件并转做
  不冲突任务，不能覆盖对方。
- 所有新 evidence 写入本任务自己创建并验证被 gitignore 的目录。

### 用户环境和凭据

- 不终止任何不是本任务创建且没有登记到 owner.json 的 Grok、Codex、Grok App、
  Chrome、Edge 或系统进程。
- 不改系统代理、VPN、路由、账号、正式 App 数据或用户日常浏览器 profile。
- 不读取、打印、导出、复用或持久化 Token、Cookie、浏览器 storage、密码、系统凭据
  和共享 `~/.grok`。
- 所有测试使用独立 `GROK_APP_HOME`、独立 agent home、隔离 browser
  `--user-data-dir` 和 run-owned staging。
- 未获用户新的明确批准，不运行真实 Grok 模型 E4，不向外部服务发送 fixture 数据。
- 只清理 owner.json 明确登记的本轮绝对路径和 PID。

### 产品和质量

- Computer Use 默认关闭，YOLO/accept-edits 不能隐式授权。
- 模型不能 authorize、pair、share、resume、reconnect 或批准系统权限。
- stale/dead/mismatch/unavailable 必须在副作用前 fail closed，禁止回退 Desktop。
- timeout 返回 unknown，不重放可能已经执行的副作用。
- borrowed tab 只能归还，不能关闭。
- 禁止任意 shell、任意远程 JS eval、Cookie/storage 导出和模型任意下载路径。
- 不向 `App.tsx` 或 `AppWorkbench.tsx` 增加状态/大块逻辑。
- 新 UI 文案同步 15 locale；禁用原生 select 和 window confirm/prompt/alert。
- 不添加生产测试后门，不吞错，不加 broad allow，不 skip/ignore/弱化断言。

## 当前事实，必须先承认

已经真实完成的部分：

1. Broker 已有 SurfaceExecutorRegistry。
2. Desktop、Managed Browser 和 App WebView 的 generic observe/act/lifecycle 已按
   authorized surface 路由，不再全部落到 Desktop。
3. UI 已把目标选择收敛到一个 `computer_use_authorize_surface` Host 命令。
4. 当前 fingerprint 的 scripted App-shell 跑过 Managed 20/20、WebView 20/20，
   WebView fail/reclaim 成立，App exit=0，无 Tokio runtime-drop panic。
5. 当前 core 302/302、driver 12/12、授权前端 16/16，fmt、typecheck、core Clippy
   通过，App check 通过但仍有 36 warnings。

仍然阻塞发布的事实：

1. `SessionManager` 只有 `attach_computer_use`，没有 detach/reconcile。
2. stop/revoke/feature-off/context/session/logout/exit 只清 Broker、IPC token 和 flag，
   不能证明 live ACP catalog 已移除 `grok-computer-use`。
3. attach 与 Stop 并发时，ACP 可以在 token 已撤销后晚到地安装一个死 MCP entry。
4. `attempt_id`/`selector_revision` 只校验并回显，Host 不保存最新 attempt；React
   commandRevision 不是 Host 权威。
5. 授权错误补偿不知道 ACP update 是未执行、已执行还是 timeout 后实际成功。
6. 当前 App-shell 将 slash `missing-hook` 从 blocking errors 排除；该 run 不证明 slash。
7. 当前 manifest 把 Managed/WebView/slash 合成一个 F3 pass，state.json 仍停在 F0。
8. Existing Tabs extension 只在内存保存 key，没有 Share、transport、observe/act、
   heartbeat、reconnect、generation、cancel 或 return。
9. pairing HTTP 丢弃 response、ext 可选、无 Origin 可通过，complete_dual 不校验
   secret proof，本地进程可以抢跑。
10. extension 未打包且 ID 仍硬编码 `pw-ext-installed`。
11. macOS/Linux/Wayland、安装版 E5、真实模型 E4 仍未完成，本轮不能虚报。

如果你认为其中任何一条已经不成立，先给出 production call trace、行为测试、独立
后置条件和当前源码行号。不得仅用模块存在、API 200、按钮可见或单测名字反驳。

## 第一动作：建立新 evidence，不要先写完成报告

按执行书第 3 节创建：

`tools/computer-use-probe/.run/r2-lifecycle/<run-id>/`

先用 `git check-ignore -q` 证明父目录和 nested owner 都被忽略，再写 owner、baseline、
append-only manifest 和由 manifest 聚合的 state。记录 branch、HEAD、dirty counts、
OS/arch、磁盘、writer、隔离目录和新 fingerprint。

修复 evidence contract：

- Managed、WebView、Desktop、slash、lifecycle、cleanup 分开记录。
- required scenario 失败时父项不能 passed。
- slash 必须使用真实构建前端/dev server，否则单独 not_run，不能从 errors 过滤。
- 记录 actor 是 ACP stub、direct MCP client 还是真实模型。
- 每条日志关闭后校验 bytes/SHA-256/fingerprint。
- state 由 required manifest rows 生成，不手填。

## Track A：先完成 atomic lifecycle

严格执行执行书 A0-A6，不允许跳到 Existing Tabs。

### A0 Red

先写可执行的负向测试，至少覆盖：

- attempt ID 非法、revision=0；
- A 晚回调发生在 B 成为最新之后；
- ACP update 阻塞时 Stop，随后旧 attach 成功；
- timeout 但 ACP 实际已应用；
- detach 失败后重试；
- 旧 detach 晚于新 attach；
- extension MCP preference update 与 CU attach/detach 并发；
- stop 分别发生在 provision/claim/Broker authorize/token issue/ACP update/complete；
- feature-off/context/model/session change/App exit 发生在 attach 中；
- 重复 stop/revoke/exit；
- 旧 WebView/Managed cleanup 不能伤害新 binding/profile。

Red 必须因为预期后置条件不成立而失败，不能因为编译错误、缺 fixture、sleep 抖动或
源码字符串断言而失败。ACP stub 使用可控 barrier/fault script，日志完全脱敏。

### A1 Host AttemptRegistry

实现 Host 权威 attempt registry：每个 App session 有单调递增 Host generation、UI
attempt correlation、selector revision、run/surface/target、phase、cancel token、MCP
desired/applied generation 和 cleanup pending。UI revision 在组件 remount 后可回到 1，
因此它不能作为 Host 时钟。

每个 await 前后和每个 callback 都校验最新 Host generation。明确实现并测试 pending
期间第二次 authorize 的策略：要么 busy 直到显式 cancel 完成，要么原子 supersede 并
补偿旧 attempt；禁止隐式混合。

将 700 行 Tauri command 中的 orchestration 移到独立 lifecycle domain service，
Tauri command 只做翻译并 await 一个操作。

### A2 MCP desired-state reconciler

不要简单补一个互不协调的 `detach`。实现每 session 序列化的 desired-state
reconciler：

```text
desired catalog = 当前 base extension MCP catalog
                + latest eligible attempt 的唯一 grok-computer-use entry
```

Computer Use 和普通 extension MCP update 必须使用同一 serializer/reconciler。获得
锁后重新读取最新 desired generation；ACP update 返回后再次检查 generation，若变化
就继续 reconcile，直到 applied=latest desired。

stop/revoke 必须先同步 cancel attempt、撤销 Broker dispatch 和 IPC token，再 await
catalog absent。attach timeout 是 ambiguous，必须根据最新 desired 再 reconcile，不能
假设未应用。detach 保留所有 base MCP，移除所有重复 CU entry。旧 attempt 永远不能
detach 新 attempt。

ACP dead/busy/disconnected 时保留显式 cleanup_pending 或 pending reconcile；下次
connect 根据 registry 重建 catalog。不得为了变绿误杀 shared ACP tenant。

### A3 统一事务和补偿

完整授权事务：

```text
begin attempt -> provision/bind/borrow -> claim -> Broker authorize
-> issue token -> desired MCP present + reconcile -> latest check -> Active
```

失败/取消逆序补偿：

```text
cancel + desired absent -> revoke token -> revoke Broker
-> reconcile MCP absent -> release matching resource -> Idle/CleanupPending
```

把 UI Stop、WebView unbind、feature-off、context/model/compact、session switch、soft
respawn、reconnect、delete、logout、extension preference change 和 App exit 全部接到
同一个 coordinator。cleanup 没有全部收敛时不能报告 stopped。释放必须匹配
session/run/attempt/target generation；borrowed tab 不能关闭。

### A4 UI 与负向行为

增加 pending authorization 的 Host cancel。Stop、session change、feature-off 和必要的
component cleanup 都触发它。authorizing/revoking 时禁用 selector/refresh/pair/bind，
但保留明确 Stop/Cancel。显示 authorizing、revoking、cleanup_pending 和 retry error，
不能把请求已接收写成 stopped。

用 deferred promise 行为测试证明 Stop、unmount、session switch 后 A 的晚 response
不会更新 B。同步 15 locale。

### A5/A6 验证

执行执行书完整 fault matrix 和所有质量门禁。每批先跑 focused，再跑：

```text
cargo fmt --all -- --check
cargo test -p grok-computer-use-core --all-features --locked --offline
cargo clippy -p grok-computer-use-core --all-targets --all-features --locked --offline -- -D warnings
cargo check -p grok-app --lib --locked --offline
pnpm exec tsc --noEmit
pnpm lint
focused Computer Use Vitest files
git diff --check
```

最后一次代码/测试/runner/config/runtime 修改后，重新 build App、计算候选 fingerprint，
先 2 轮 smoke，再至少 Managed 20、WebView 20、lifecycle fault 100 轮，并在同一冻结
fingerprint 上累计至少 2 小时真实 active fault wall time。

每轮必须从 ACP stub 查询最终 current catalog：Active 恰好一个 matching CU entry；
stopped/revoked/failed 为零；base MCP 全保留；旧 token unauthorized；无错误 target、
post-stop dispatch、profile/worker/WebView/lease/binding orphan。

任何代码、测试、runner、extension、config、lock、runtime 修改都会 invalidate freeze 和
soak，修复后从 A6 重跑。Track A 任一 required row 不通过时禁止进入 Track B。

## Track B：安全 Existing Tabs

只有 Track A 全绿才执行执行书 B0-B4。

1. 先重写 threat model/ADR，覆盖恶意本地进程、网页、其他扩展、replay、并发抢跑、
   worker/App/browser restart 和所有 secret 泄漏面。
2. App 生成公开 challenge 和独立一次性 secret/code。secret 只能通过明确用户动作进入
   extension popup/side panel，禁止 URL/query/GET/history/log/screenshot。
3. extension 对 nonce、App instance、浏览器专属 extension ID、connection nonce、
   protocol version 做 MAC；Host constant-time 验证。App/extension 分别确认，成功后才
   发 session key。
4. 删除 production 中丢弃 response/secretless complete_dual 的路径。Origin/Host/CORS
   只做附加防护，不是身份。删除 `pw-ext-installed`，设计 Chrome/Edge 稳定身份。
5. MV3 提供 Pair、Share current tab、Stop sharing、状态/重试；只有用户显式 share 的
   tab 进入 Host picker。
6. 实现 authenticated transport、heartbeat、reconnect、deadline、queue、cancel、key
   rotation、document/connection/target generation。
7. isolated-world content script 只实现 typed observe/click/set-value/type/key/scroll/
   wait/navigation，禁止远程任意 eval。
8. navigation/reload/close/focus/disconnect 使旧 observation 失效。stop 撤销 grant、
   detach script 并归还 tab，不关闭 borrowed tab。
9. session key 只放页面不可访问的 MV3 session store，不进 localStorage/page world/log。
10. ExistingTab executor 只在 authenticated connection 存在时注册；generic computer
    tools 和 compatibility browser tools 共用 Track A lifecycle/grant/ledger/cancel。
11. extension 完整资源、notice、identity metadata 和说明进入 Tauri bundle，安装后不
    依赖仓库路径。
12. 对 Chrome 和 Edge 分开做 E2/E3。Chrome 至少 20 个完整 pair/share/authorize/
    observe/act/oracle/navigation/reconnect/stop/return 循环；Edge 可用就独立跑，不可用
    才 blocked_external，不能复制 Chrome 结果。

截图能力要诚实。如果浏览器 API 在不请求宽泛 debugger 权限时不能截取非活动 tab，
则只在 tab 可见/活动时提供 image，其他情况返回 typed unavailable。不得强制抢焦点、
静默扩大权限或伪造截图。

## 冻结和长稳

Track A+B 的最后一次修改后重新 build/fingerprint。至少执行 4 小时真实 active mixed
soak 和 200 个完整 iteration，分开累计：

- authorize/stop phase faults：>=60 分钟、>=50 轮；
- MCP attach/detach races：>=60 分钟、>=50 轮；
- Managed/WebView regression：>=45 分钟、>=40 轮；
- Existing Tabs navigation/reconnect：>=60 分钟、>=40 轮；
- feature/context/session/App teardown：>=15 分钟、>=20 轮。

每个 scenario 成功率 >=90%，但以下指标必须严格为零：unauthorized read/write、wrong
target、post-stop dispatch、旧 cleanup 伤新 attempt、stale CU MCP entry、base MCP
丢失、secret/token/cookie 泄漏、owned resource orphan、borrowed tab 被关闭。

不同 scenario 不能互相冲掉失败。保留 first failure。修代码后旧 freeze/soak 全部
invalidated，必须重建并重新累计。

## 长任务行为

- 每个 batch 完成和至少每 90 分钟写 checkpoint，然后自动继续，不要等用户回复。
- checkpoint 列 run/fingerprint、改动文件、测试精确计数、首败、MCP catalog 最终状态、
  resource cleanup、external blocker、下一动作和 `未 commit/未 push/未提 PR`。
- 遇到 external blocker 时只标对应原子 row，继续所有不依赖它的 ready work。
- 同一失败最多做三次基于不同证据/假设的聚焦修复。之后保留首败，标 failed，继续
  独立任务；禁止无限重复命令。
- Cargo、App、browser、runtime、installer 等重任务串行，避免抢同一 target/profile。
- 如果 Goal mode 因 turn/进程中断，使用 checkpoint 续跑；不要重写历史，也不要把
  中断时间计入 active soak。
- 不要问“是否继续”。只有真实模型、外部设备、账号/用户环境变更需要新授权时才停
  下来请求用户。

## 最终报告

最终状态必须从 manifest 生成，包含 execution plan 第 10 节全部字段、每个 scenario 的
active wall/rounds/ok/fail/first failure、ACP catalog attach/detach trace、Chrome/Edge、
Desktop/Managed/WebView/Existing Tabs、Windows/macOS/Linux/Wayland、source/installed、
stub/real-model 独立状态，以及 manifest 文件/hash/fingerprint 完整性。

必须明确写：`未 commit / 未 push / 未提 PR`。

允许的顶层结论只有：

```text
Track A lifecycle gate passed; overall Computer Use partial - not releasable
Track A+B local source gate passed; overall Computer Use partial - not releasable
partial - not releasable
```

本轮不能写 `Computer Use complete`、`releasable`、跨平台完成、安装版完成或真实模型
完成。现在从全文阅读、现场 fingerprint 和 A0 Red 开始，持续执行，不要先写完成报告。

---

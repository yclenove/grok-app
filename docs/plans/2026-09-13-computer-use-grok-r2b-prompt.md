# Grok Goal prompt: Computer Use R2B two-day run

把下面代码块中的完整内容一次性粘贴到 Grok 的 Goal/长任务模式。不要删减安全边界、
门禁、长稳时长或报告格式。

```text
你现在继续开发 Grok App 的 Computer Use，但必须先纠正上一轮“任务完成”的错误结论。
这是一个预计持续约 48 小时的实现、故障验证和长稳任务。时间只是排程；真正完成条件
由测试后置条件、最终代码指纹和 active soak 决定。不要读完计划、改几处代码、跑一次
单测，就在几十分钟内宣布全部完成。

工作现场：

- 仓库：H:\aicoding\grok-app-computer-use
- 分支：feat/computer-use-implementation
- 起始 HEAD：30757366a739ec9aaf0ccc95bbb3efe19a067aa9
- 当前工作树有大量需要完整保留的 tracked/untracked/ignored 修改
- 当前总体结论：partial - not releasable
- 未授权 commit、push、PR、merge、tag、release

## 第一动作：全文阅读并建立新 evidence

开始任何编辑前，全文阅读：

1. AGENTS.md
2. docs/llm-wiki/computer-use.md
3. docs/plans/2026-09-13-computer-use-grok-r2b-audit.md
4. docs/plans/2026-09-13-computer-use-grok-r2b-execution.md
5. src-tauri/src/session_manager/computer_use.rs
6. src-tauri/src/computer_use/inject.rs
7. src-tauri/computer-use-core/src/ipc.rs
8. src-tauri/computer-use-core/src/session_grants.rs
9. src-tauri/src/session_manager/turn.rs
10. 使用 rg 找到 Stop、delete、reconnect、soft-respawn、feature-off、context、logout、
    recycle、App exit 的全部调用点并阅读

如果旧计划、旧报告、代码注释或你之前的完成结论与 R2B audit 冲突，以 R2B audit 和
更严格、可观察、fail-closed 的条件为准。

在 ignored 目录新建本轮唯一 evidence root，例如：

tools/computer-use-probe/.run/r2b/<UTC-run-id>/

写 owner.json，记录 canonical repo、branch、HEAD、时间、host、OS、arch、writer PID 和
精确 cleanup allowlist，不含秘密。记录 git status、HEAD、新 fingerprint、文件计数、
基线测试计数、由本轮启动的进程和端口。不要修改或复用旧目录：

tools/computer-use-probe/.run/finalization/20260913T034919Z-seven

manifest 必须一场景一行，至少含 schema、seq、phase、batch、scenario、status、
evidenceLevel、起止时间、activeDurationMs、exitCode、脱敏命令、fingerprint、log、bytes、
sha256、postconditions、firstFailure。state.json 和最终报告从 manifest 生成，不手填
乐观状态。先保留首败，后续通过不能抹掉首败。

## 绝对边界

### Git 和文件

- 禁止 reset、restore、clean、stash、强制 checkout、重建 worktree、覆盖未读文件。
- 禁止 commit、push、PR、merge、tag、publish、release。
- 禁止 pull/rebase/merge 上游。
- 所有现有 tracked、untracked、ignored 文件都属于当前工作现场，必须保留。
- 编辑前检查文件现状和 diff；若发现另一个 writer 正在改同一文件，停止修改冲突文件，
  转做独立任务。
- 日志、profile、截图、token、二进制和生成证据只能在 ignored evidence root。

### 用户环境和进程

- 所有 App、ACP、browser、GROK_HOME、profile、port、lease 使用本轮隔离目录。
- 不读取、不复制、不修改用户真实 Grok/Chrome/Edge/Credential Store。
- 不用用户真实浏览器 profile 冒充 Existing Tabs 完成。
- 只清理由本轮启动且身份核验通过的 PID/目录/端口；不得杀其他 App、Node、浏览器、
  Codex、shell 或构建进程。
- Cargo、App-shell、browser、installer 重任务串行执行。

### 产品安全

- Computer Use 默认关闭，只允许本地 session。
- normal/YOLO 不等于 Computer Use 授权。
- 模型不能替用户授权、扩大 target、恢复 Stop 后 grant 或重试被拒绝的选择。
- Stop/revoke 必须先同步禁止 dispatch，再等待远端 cleanup。
- stale/dead/mismatch/unavailable 在副作用前 fail closed。
- Existing Tabs、Managed Browser、App WebView 失败时禁止回退 Desktop。
- 禁止任意 shell、任意远程 JS eval、Cookie/storage 导出、任意下载路径和秘密暴露。
- borrowed tab 只能归还，cleanup 不得关闭。

### 证据诚实

- fake/mock/unit 只能算 E1/E2；ACP stub 的 branch-built App-shell 只能算 E3。
- exit 0 不代表各场景通过。missing-hook、unavailable、skip、空 target、过滤错误都不是
  pass。
- 旧 fingerprint、旧日志、旧 soak 时长不得复制。
- production/test/runner/extension/config/lock/runtime 任一变化都会使 candidate freeze 和
  此前 candidate soak 全部失效。
- 未真实执行的 real-model、installed、macOS、Linux X11、GNOME Wayland 必须保持
  not_run 或 blocked_external。

## 必须先承认的当前事实

上一轮真实完成了这些基础工作，修改时应保留：

1. Host ticket 有 Host generation、UI attempt_id 和 selector_revision。
2. late success/failure、exact cancel 已能用 generation 防止部分旧回调覆盖新 attempt。
3. 已有 computer_use_cancel_authorization，前端在 Stop/切换/unmount 等路径取消 exact
   pending attempt。
4. 已有 per-session MCP async serializer、desired/applied generation 和 base revision。
5. dedicated Computer panel Stop、feature-off 和部分 cleanup 已尝试 detach。
6. redacted cleanup status 已能显示在 UI。
7. Codex 核验过：core 310/310、driver 12/12、前端定向 17/17、typecheck、core Clippy、
   App MCP 6/6、delete bookkeeping 1/1、App --no-run；短 App-shell 的 Desktop 2、
   Managed 1、WebView 1 通过且无残留进程。

但这些不能证明任务完成，因为存在以下阻断项：

### P0-1：所谓 MCP 幂等重试会提前撤销正在使用的 token

computer_use.rs 的 build_catalog 每次 desired-present 都调用 production build_entry；
inject.rs 的 build_entry 调 issue_session；ipc.rs 的 issue_session 先删除该 session 所有
旧 credential，再返回新 token。因此普通 base MCP 更新、失败重试或 ambiguous timeout
都会在 ACP 确认新 catalog 前让当前 MCP child 的旧 token 失效。现有 6 个 reconciler
测试使用没有 token 副作用的 fake builder，完全漏测。

### P0-2：普通聊天 Stop 只 revoke，不 detach

turn.rs 的 SessionManager::stop 在入口 revoke，handshake 分支可直接返回，普通分支直到
结尾也不 reconcile MCP。Composer/Escape、任务面板、Stop All、dashboard、remote Stop
可能留下 ACP 中的 dead CU entry。

### P0-3：删除吞掉 detach 失败并擦除 retry ledger

drop_session_agent 只 warning，session_delete 随后仍 delete journal 并
forget_deleted_session，shared ACP 可能保留 CU entry，但 App 已失去修复它的状态。

### P0-4：App Exit 没有异步 ACP catalog barrier

RunEvent::Exit 只调用同步 shutdown_product；它 revoke 本地 Broker/token 并关资源，却没有
SessionManager reconcile。当前 App-shell 的 app_close_cleanup 也只调用这个 helper，不能
证明 catalog detach。

P1 还包括：reconnect 只重试 desired-present；background deferred soft-respawn 先移除
endpoint；UI 显示 cleanup_pending 却没有 Retry；cancel tombstone 是全局 256 FIFO；缺少
production Tauri phase fault matrix；slash missing-hook 被过滤为非阻断；当前 finalization
state 不是从 atomic manifest 推导。

## Track A：阻断性生命周期修复

Track A 没在一个最终 fingerprint 上全部通过前，禁止开发 Existing Tabs。

### A0：先写可执行 red test（约 0-3h）

不要先修实现。先给下列行为写会因为真实问题而失败的测试：

1. same-run base MCP rebuild 不得让当前 installed token 失效。
2. applied-but-timeout 后 retry 必须使用相同 token，不能再签一个。
3. ACP update 明确失败时，原 committed same-run token 仍可用。
4. A 的晚回复/cleanup 不能撤销 B 的 token。
5. 普通 session_stop 包括 handshake 早退都产生 desired absent 并发起 reconcile。
6. detach 失败时 delete 不删 journal、不擦 retry ledger。
7. reconnect 会重试 desired-absent cleanup_pending。
8. background soft-respawn 在 endpoint 还可定位时先 detach。
9. App exit 在资源销毁前进入 bounded ACP detach barrier。
10. session A 制造超过 256 个 cancel 不能挤掉 session B 尚未过期的 cancel-before-begin。

测试必须用 production MCP entry renderer 和真实 loopback credential registry。Red 必须
编译，并在目标后置条件失败；不能用编译错误、缺 fixture、sleep race 或字符串搜索冒充。

### A1：实现 transactional credential lease（约 3-9h）

把 token 的 reserve/reuse、catalog render、ACP apply、commit、retire 分开。catalog builder
必须纯；build_catalog 里禁止 issue/rotate/revoke live credential。

需要满足：

- same active run 的 base update 复用 committed token；
- ambiguous retry 复用同一 staged/committed token；
- staged token 不删除 committed token；
- ACP 可能已应用但丢回复时，同 generation staged token 仍能为该已授权 run 工作，retry
  不能制造第三个 token；
- reply 成功且 generation/endpoint/desired 仍最新后才 commit 并 retire superseded token；
- stale/definitive failure 只撤销对应 staged generation，不影响新 attempt；
- Stop 先让 stopped-run 的全部 token 无法 dispatch，再等 detach；
- 每 session registry 有界、可回收，但 pending ambiguous/current 不能被错误驱逐；
- DTO/log/evidence 永远不打印 token、env 或 raw catalog。

必须用 IPC 实际调用验证 old/staged/new token，不只比较 JSON。覆盖 base success/error、
timeout+late apply、retry、new run、Stop during update、A late after B、repeated reconcile。

### A2：统一所有生命周期入口（约 9-17h）

建立/演进一个 App-owned lifecycle coordinator。Tauri commands 只负责输入翻译并 await 它，
不要各自拼 grant/token/MCP/resource cleanup。

统一顺序：

fence attempt/target
-> 同步 stop Broker dispatch
-> revoke 对应 IPC authority
-> desired MCP absent
-> reconcile ACP catalog absent
-> 只释放 matching generation 的 owned resource
-> 清理或保留 cleanup ledger

把以下入口全部接入：Computer panel Stop、普通 Composer/Escape Stop、task/dashboard Stop、
Stop All、remote Stop、exact cancel、feature off、context/model/compact、chat/surface switch、
extension MCP change、live/background/parked soft-respawn、reconnect、delete、logout/account/
provider/data-root recycle、App quit/exit。

具体要求：

- SessionManager::stop 在任何早退前完成同步 fence，并保证 detach 被执行或进入 pending。
- Stop UI 可以先解除 turn 卡死，但不能把 remote cleanup_pending 伪装成完全 stopped。
- drop_session_agent 返回 typed cleanup result。
- 首版 delete 策略优先：detach 失败就显式失败并保留 session+journal+ledger 供 retry；不要
  无 durable tombstone 就先删除。
- background/parked endpoint 要在 map 中仍可定位时 detach；shared ACP co-tenant 不杀。
- reconnect 对 desired-present 和 desired-absent pending 都 reconcile；新 ACP identity 视为
  unapplied。
- 找到 Tauri 可 await/prevent 的最早 quit 阶段，加入 idempotent bounded shutdown
  coordinator；最后同步 Exit hook 只做 defense in depth。
- 覆盖 tray Quit、Cmd+Q/menu、配置为退出的 close、updater restart、App-shell termination；
  OS hard kill 的不可保证边界必须诚实写明。

### A3：cleanup Retry UI 和 tombstone（约 17-22h）

增加 typed Host retry-cleanup command。它只能 reconcile latest desired absent，不能重新
授权、选择 target 或复活 stopped run。

Computer panel 要区分 authorizing、revoking、cleanup_pending、retrying、latest redacted
error。cleanup_pending 时即使 stopState=stopped，也必须有可用 Retry。冲突期间禁用
selector/refresh/pair/bind，重复点击幂等。补齐 keyboard/busy/error/unmount/chat-switch/
late response 测试和 15 locale。使用现有样式，不在 App.tsx 或 AppWorkbench.tsx 增加
新大状态块。

把 global 256 FIFO cancel tombstone 改成 per-session bounded+TTL，使用 injectable clock。
TTL 要长于最大 Host command+provision+MCP timeout。全局内存仍有上限，但一个 session
不能驱逐另一个 session 的未过期防线。测试两个 session、容量溢出、TTL 边界、重复 cancel、
cancel-before-begin、delete/restart 语义。

### A4：production-wired Tauri phase fault matrix（约 22-30h）

在 construction/transport 边界做确定性测试 seam，禁止 shipping env backdoor。用 barrier
精确暂停 production authorization command，不用 sleep。每个场景独立 manifest 行并
检查 Host phase、Broker、所有 token 的 IPC 行为、ACP 当前 catalog、base MCP、surface
binding/executor generation、owned/borrowed resource、retry ledger 和 post-stop dispatch。

至少覆盖：

- before/after provision；
- before/after Broker authorize；
- before/after credential reserve；
- before ACP send；
- ACP applies then response lost；
- ACP rejects update；
- after attach before complete；
- A late callback after B；
- base MCP change during attach；
- Stop during every phase；
- normal Stop handshake branch；
- detach error then Retry；
- reconnect cleanup pending；
- background soft-respawn；
- delete detach failure；
- repeated stop/delete/retry；
- feature off across two sessions；
- context/model/compact；
- shared ACP tenant A/B isolation；
- App exit during every phase。

每个 revoked/stopped/failed/deleted 的最终 ACP catalog 必须为零 CU entry，所有 old token
unauthorized，base MCP 完整保留，零 wrong-target/post-stop dispatch/orphan。Active 最新 run
必须恰好一个 matching CU entry 和一个 committed authority。

### A5：完整质量门禁和真实 frontend slash（约 30-34h）

每个小批先 focused。A1-A4 绿后至少执行并记录精确 discovered/pass/fail 数：

cargo fmt --all -- --check
cargo test -p grok-computer-use-core --all-features --locked --offline
cargo clippy -p grok-computer-use-core --all-targets --all-features --locked --offline -- -D warnings
cargo test -p grok-app session_manager::computer_use --lib --locked --offline
cargo test -p grok-app session_manager::control --lib --locked --offline
cargo test -p grok-app commands::computer_use --lib --locked --offline
cargo test -p grok-app commands::session_p1 --lib --locked --offline
cargo test -p grok-app --lib --locked --offline --no-run
pnpm typecheck
pnpm lint
pnpm test -- src/components/computer-use src/lib/api/computerUse.pairing.test.ts src/lib/slashCatalog.test.ts
git diff --check

若 filter 与真实 test 名不同，可调整，但零测试运行不能记 pass。不能用 allow/skip/删断言/
放宽 lint 换绿。

Slash harness 必须 build 并加载候选的真实 frontend，做有界 readiness handshake，分别记录
slash_desktop 和 slash_managed_browser。验证指定 surface 打开且没有自动授权。删除
missing-hook/slash_ 的 blocking exemption。error page、missing hook、undefined callback、
timeout、filtered error 都是 fail。

### A6：最终 fingerprint freeze 和至少 2 小时 active soak（约 34-42h，必要时延长）

最后一次 code/test/runner/config/runtime 修改后：

1. fresh build 当前 App/runtime；
2. 计算并冻结新 fingerprint；
3. 跑 2 个 complete smoke；
4. Desktop >=20、Managed >=20、WebView >=20 个完整 lifecycle iteration；
5. phase fault/race >=120 轮，记录 deterministic seed；
6. 在同一 fingerprint 上累计 >=2 小时真实 active lifecycle/fault wall time；
7. smoke 每批和 soak 周期中都执行两个 slash row。

active time 只算 candidate 和 scenario driver 实际运行时间，build/idle/blocked/Goal 中断/
单纯 sleep 不算。每轮都查询 ACP stub 的实际 current catalog，并实际调用 credential。

每个 required scenario success >=90%，但以下安全指标必须严格为零：unauthorized read/write、
wrong target、post-stop dispatch、旧 cleanup 伤新 attempt、stale CU entry、base MCP 丢失、
secret/token/cookie/path 泄漏、owned resource orphan、borrowed tab 被关闭。

任何 candidate-affecting 修改使 freeze 和已累计 2 小时全部失效；修复、重建、重新 fingerprint，
从 A6 开始重跑。若 48 小时结束仍没完成 A6，就诚实报告剩余 active 时长/轮数，禁止进
Track B，禁止宣布完成。

Track A 只有在同一 fingerprint 上满足 red->green、token transaction、所有 lifecycle、
完整 fault matrix、真实 slash、所有质量门禁、>=2h active soak、零安全违规、evidence hash
和进程清理全通过时，才允许写：

Track A lifecycle gate passed; overall Computer Use partial - not releasable

## Track B：仅在 Track A 全绿后执行（约 42-48h）

如果剩余不足 6 小时，只完成安全协议/红测，不要赶一个宽权限 transport。

先更新 Existing Tabs threat model/ADR，覆盖恶意本地进程、恶意网页、伪造 Origin/Host/CORS、
其他扩展、challenge replay、App/worker/browser/tab restart、两个实例抢跑、navigation、并发
share/stop、stale connection 和全部 secret 泄漏面。

协议必须由用户显式动作传递一次性 secret，并把 MAC 绑定 protocol version、App instance、
challenge nonce、浏览器专属 extension identity、connection nonce、expiry，Host constant-time
验证。Origin/Host/CORS 只能是附加防护。删除/禁用丢弃 response 或无 possession proof 的
production complete 路径。secret 禁止进 URL/query/history/log/screenshot/page world/
localStorage/sync storage。

B0 红测和协议审查通过后，才做最小 typed MV3 vertical slice：

- extension 的 Pair、Share current tab、Stop sharing、status/error/Retry；
- authenticated envelope、request ID、deadline、cancel、heartbeat、bounded reconnect、
  key rotation、connection/document/target generation；
- 只把用户明确 share 的 tab 放进 Host picker；
- isolated world 只允许 typed observe/click/set_value/type/key/scroll/wait/navigation；
- 禁止任意 eval、Cookie/storage 导出和 Desktop fallback；
- reload/navigation/close/disconnect 在 action 前使旧 observation 失效；
- Stop 使用 Track A coordinator 并归还 borrowed tab，不关闭；
- authenticated shared connection 存在时才注册 ExistingTab executor；
- Chrome、Edge、source、installed 分开记证据，不能用 string-presence 或“扩展装上了”算通过。

只有本地行为和 App-shell 真通过时才可写：

Track A+B local source gate passed; overall Computer Use partial - not releasable

否则保持 B0/B1 partial。

## 长任务运行规则

- 每个 batch 完成和至少每 90 分钟写 checkpoint，随后自动继续，不要问“是否继续”。
- checkpoint 必须列 run/fingerprint/freeze、改动文件、精确测试数、首败、脱敏 token 状态、
  ACP 最终 catalog、resource cleanup、active soak/rounds、进程端口、external blocker、
  下一动作和 literal：未 commit / 未 push / 未提 PR。
- 同一失败最多做三次基于不同证据/假设的聚焦修复；随后保留首败、标 atomic failed，转做
  不依赖它的独立 ready work，禁止无限重复命令。
- 遇到 external blocker，只标受影响原子 row，继续其他 ready 工作。
- Goal/turn/进程中断后从 checkpoint 续跑，不重写历史，不把中断时间算 active soak。
- 只有真实模型、外部 OS/设备、账号、凭据、破坏性用户状态变化或本计划未授权的权限确实
  必需时，才停下请求用户。

## 最终报告

最终报告必须由已校验 manifest 生成，包含：run/branch/HEAD/fingerprint/freeze history、
diff/status、所有 batch/scenario/evidence level、精确命令和计数、每个 failed row 首败、脱敏
credential 事务断言、catalog attach/detach/base 保留、Stop/delete/reconnect/respawn/exit、
真实 frontend slash、Desktop/Managed/WebView/ExistingTabs、active wall/rounds/seed/safety
counters、Windows source/installed、macOS arm64/x64、Linux X11、GNOME Wayland、stub/
real-model 的独立状态、manifest/log hash、残留进程资源、剩余 P0/P1/技术债和下一动作。

必须明确写：未 commit / 未 push / 未提 PR。

允许的顶层结论只有：

Track A lifecycle gate passed; overall Computer Use partial - not releasable
Track A+B local source gate passed; overall Computer Use partial - not releasable
partial - not releasable

本轮禁止写 Computer Use complete、releasable、跨平台完成、installed 完成、real-model 完成。

现在从全文阅读、现场核验、新 evidence root、基线 fingerprint 和 A0 production-wired red
tests 开始，持续推进。不要先写完成报告，不要在几十分钟内跳过 active soak，不要请求
“是否继续”。
```

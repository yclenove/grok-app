# Grok Goal prompt: Computer Use R2D 48-hour continuation

Copy the complete prompt below into Grok Goal mode as one task.

```text
在仓库 H:\aicoding\grok-app-computer-use 的现有工作树继续开发 Computer Use。

这是一轮最多 48 小时的持续工程任务，不是几十分钟的代码生成。上一轮你声称任务完成，
Codex 已对当前树重新审查：R2C 确实完成了 session/run credential 复用、按会话 tombstone、TTL、
容量饱和 fail-closed、forget_session、异步 cleanup hand-off 和 Cleanup Retry 等实质工作；但 R2C
证据根只停在 7 条基线记录，当前仍有两个生命周期缺陷、一个可复现 App 测试失败和三个发布
门禁红项。因此你必须以这个顶层状态开始：

partial - not releasable

固定工作区：

- repo: H:\aicoding\grok-app-computer-use
- branch: feat/computer-use-implementation
- baseline HEAD: 30757366a739ec9aaf0ccc95bbb3efe19a067aa9
- 当前有 69 个 tracked modification rows、86 个 untracked entries，全部保留
- 禁止 reset、restore、clean、stash、强制 checkout、rebase、重建 worktree
- 禁止 pull、merge 上游
- 禁止 commit、push、PR、merge、tag、publish、release、deploy
- 不得改真实账号、Cookie、token、浏览器日常 profile、Credential Store、共享 ~/.grok、代理或 VPN
- 不得操作用户日常 App、tab、聊天、文件
- 只清理由本轮创建、owner 标记且身份核验通过的 PID、端口、目录、profile、lease
- Cargo、App、browser、installer 重任务串行执行

第一步全文阅读，不能只搜索关键词：

1. AGENTS.md
2. docs/llm-wiki/computer-use.md
3. docs/llm-wiki/i18n.md
4. docs/llm-wiki/dialogs.md
5. docs/plans/2026-09-14-computer-use-codex-post-r2c-audit.md
6. docs/plans/2026-09-14-computer-use-grok-r2d-execution.md
7. docs/plans/2026-09-13-computer-use-existing-tabs-pairing-adr.md
8. docs/plans/2026-09-11-computer-use-existing-tabs-transport-adr.md
9. R2D execution 每一批点名的源码、测试和 runner

如果旧计划、旧报告、代码注释或你此前的完成结论与当前源码、post-R2C audit、R2D execution
冲突，以当前源码和更严格的 fail-closed 后置条件为准。不要修改审查结论或测试来让任务变绿。

在任何产品编辑前，新建本轮唯一且 gitignored 的证据根：

tools/computer-use-probe/.run/r2d/<UTC-run-id>/

必须写 owner.json、baseline/status.txt、baseline/head.txt、baseline/fingerprint.json、
baseline/untracked-source.json、manifest.jsonl、state.json、checkpoints/、logs/、failures/、metrics/、
reports/。owner 记录 canonical repo/branch/HEAD/time/host/OS/arch/writer PID/schema/精确 cleanup
allowlist，不含秘密。manifest 一场景一行、append-only；state/checkpoint/report 从 manifest 生成。
保留 first failure，后续通过只能追加 recovery，不能覆盖失败历史。

R2C 根 tools/computer-use-probe/.run/r2c/20260913T061946Z-seven 只能标 stale/historical：它只有
7 行，state 中 D1-D14 都是 not_started，最后时间早于当前源码。禁止继续往它写，禁止复用它的
fingerprint，禁止把旧 App-shell/soak/report 复制成 R2D pass。

证据等级：unit/mock/jsdom=E1，真实模块/子进程隔离=E2，branch App+scripted agent/native fixture=E3，
真实模型+真人授权=E4，installed/update/rollback/repeated task=E5。低等级不能填高等级。exit 0、
skip、0 tests、空 target、missing hook、旧 artifact、旧 fingerprint、timeout、crash、grep/string
presence 都不是行为通过。

产品硬边界：

- Computer Use 默认关闭，只允许 local interactive session。
- normal/YOLO/accept-edits 不授予 Computer Use。
- 模型不能 authorize、resume、reconnect、扩大 target、确认 pairing 或替用户重试被拒选择。
- stale/dead/mismatch/unavailable 必须在副作用前 fail closed。
- Existing Tabs、Managed Browser、App WebView 失败绝不回退 Desktop。
- Stop/ProcessExited 必须先同步 fence dispatch 和 IPC credential，再做慢 cleanup。
- borrowed tab 只能归还，不能关闭；owned profile 只能由 matching owner 关闭。
- 禁止 arbitrary shell、arbitrary JS eval、Cookie/storage export、任意路径、secret URL、任意下载。
- 不得用 blanket allow/expect dead_code、降低 -D warnings、删测试、改预算或跳门禁来伪造绿色。

当前已由 Codex 现场确认的基线：

- Core tests: 325/325 passed
- private Driver: 12/12 passed
- targeted Computer Use frontend: 47/47 passed
- full frontend: 7106/7106 passed
- App harness 排除一个 runtime test: 1630 passed, 1 ignored, 1 filtered
- exact runtime test: FAILED at src-tauri/src/computer_use/runtime.rs:199
- Core Clippy -D warnings: passed
- App Clippy -D warnings: FAILED, lib 55 errors, lib-test 49 errors
- Rust fmt/typecheck/ESLint/deps hygiene: passed
- final code quality: FAILED, files >=1000 lines = 82, budget = 80
- pnpm audit:prod: FAILED, @tiptap/core 3.30.4, one high, patched >=3.30.5
- current-fingerprint App-shell/freeze/active soak/installed/real-model/macOS/Linux/Wayland: not_run

不要从头重写已完成的 tombstone/Cleanup Retry。先用当前测试保护它们，再按以下顺序连续执行。

R0（0-2h）：基线与 executable red

- 完整记录 status/HEAD/untracked source/lockfile/进程端口/旧证据/fingerprint。
- 为 runtime env isolation 写 5 个 red：两变量保持到全部断言、success restore、panic restore、
  poisoned lock recover、serialized tests 不串 home。
- 为 handshake Stop 写 7 个 barrier red：noop plan 仍杀 exact connecting ACP；catalog failure 仍杀；
  resource failure 仍杀且 Retry 保留；duplicate exact-once；late A 不杀 replacement B；shared ACP 不杀
  co-tenant；UI ack 先于 bounded termination。
- 为 ProcessExited 写 live/background/parked/shared red，必须实际证明旧 Bearer 仍可调用、Broker grant
  未 fence、desired 不 absent、cleanup 未排程；再加 late A exit 与 B reconnect race。
- 记录现有 pairing 的 attacker red：无 Origin/任意 loopback caller 能拿 public challenge，App 确认后
  当前 pair.js/HTTP 路径没有 extension-only possession proof。只在 owned fixture 证明，不外放利用代码。
- Red 必须编译并在真实后置条件失败；compile error、sleep race、missing fixture、forced timeout 不算。
- 写 R0 checkpoint，然后自动继续，不问用户。

R1（2-4h）：修 runtime test isolation

- 在现有 APP_HOME_ENV_LOCK 下使用 scoped Drop guard 保存 OsString/absent 状态。
- guard 生命周期必须覆盖 repair、diagnose、JS_RUNTIME resolve、PLAYWRIGHT_ARCHIVE resolve、bytes 和全部
  assertion；success/unwind 都恢复 PATH 和 GROK_APP_HOME。
- helper/lock 必须 test-only，不能继续成为 release lib dead code。
- 不要 catch 产品 panic 伪造成功；catch_unwind 只用于验证 guard restoration。
- 用 cargo --no-run 后的当前 App harness，经 mt.exe 嵌 windows-test-manifest.xml，再直接跑 exact test；
  禁止在 build.rs 加第二个 MANIFESTINPUT。
- exact test、panic restore、full App harness 都绿，且无临时 App home/环境污染后才进入 R2。

R2（4-12h）：先修生命周期安全，再碰其他功能

R2.1 handshake Stop：

- 不要继续让 loose kill_handshake_acp bool 依赖 Computer Use cleanup result。
- 同步 fence 时捕获 App session + exact ACP/process incarnation；返回 Stop snapshot 前把旧 endpoint 标记
  terminating 或从 slot 移除。
- cleanup plan 是 noop、catalog detach 失败、surface cleanup 失败都不能丢失 exact process action。
- delayed task 只能操作捕获的 old handle，禁止事后读取当前 session.acp。
- shared process 有其他 live/background/parked tenant 时不能 kill process，只 detach 当前 tenant。
- process termination bounded、idempotent；cleanup pending 与 process termination 分开诊断。
- 所有 R0 handshake reds 转绿，再加 dead/missing ACP、repeated reconnect negative tests。

R2.2 ProcessExited：

- 在移除 live/background/parked ownership 之前，按 exact process incarnation 快照并去重所有 App session。
- 对仍匹配的每个 session 同步 fence Broker dispatch、revoke IPC credential、disable CU、publish
  desired-absent，然后才清 ACP slot/ownership 和发 disconnected snapshot。
- ACP 已死，不得无限重试向死 endpoint 写 catalog；把 endpoint disappearance 与 desired-absent、surface
  cleanup 分开建模。
- generation-bound adapter/browser/profile cleanup 放 blocking boundary；只有真实可重试 cleanup 才留
  Retry ledger。
- shared process 每个真实 tenant 都 fence；一个 cleanup 失败不能阻止其他 tenant。
- stale A exit 只能影响捕获的 A ownership，绝不能撤销 replacement B 的 run/token/target/catalog/resource。
- live/background 共用 coordinator，parked 也必须满足同样授权后置条件，不能复制三份易漂移逻辑。

R2.3 必测矩阵：

- live Ready/Streaming/AwaitingPermission/Connecting；background Ready/Streaming；parked-only；
- live+background+parked shared process；无 CU run；Desktop/Managed/WebView active run；
- cleanup success/adapter failure/worker timeout/Retry；credential before/after；in-flight+queued action；
- duplicate exit；A exit after B reconnect；feature-off/App-exit 同时发生；
- actual Broker/credential/MCP generation/ledger/resource/UI/PID postcondition；
- post-exit dispatch=0、wrong-target=0。

R3（12-18h）：恢复 App Clippy 与模块所有权

- 先把 55/49 每个错误分类为 shipping、test-only、probe-only、普通 idiom。
- shipping API 正确接线或删除；test helper 用 cfg(test)；probe/App-shell/fixture 移到 cu-probe 或现有
  non-default computer-use-probe feature；release App 不得携带隐藏 probe 入口。
- 对 too_many_arguments 使用 request/context struct；按语义改 inspect_err、direct Result、clamp、
  filter+map、next_back、去 needless borrow/let，不用 allow。
- 恢复 final file budget，做真实 domain extraction：优先拆 session_manager/computer_use.rs 的 reconcile/
  cleanup/tests、把 app_shell scenario/report 归 probe、拆 webview executor/extraction/tests、拆 Windows
  capture/input/tests。禁止随便挪行或 forwarding-only 文件。
- 目标：Core Clippy green、App Clippy green、final code quality green、files>=1000 <=80。

R4（18-21h）：依赖安全

- 只做兼容的 Tiptap cohort patch，全部相关 @tiptap 包与 peer/lockfile 一致，版本 >=3.30.5。
- 检查 tiptap-markdown peer，不做无关 dependency churn。
- pnpm deps:check、pnpm audit:prod、pnpm typecheck、pnpm lint、pnpm test、pnpm build:ui 全绿。
- audit 必须在现有 threshold 下 0 vulnerability；不得忽略 advisory。

R5（21-29h）：production-wired credential/lifecycle phase fault matrix

- 用真实 production MCP renderer、loopback credential registry、ACP client/stub、SessionManager、Broker、
  surface adapters、cleanup ledgers；不能只测 direct helper。
- test seam 只在 compile-time test/probe feature，禁止 shipping env backdoor。
- 用 barrier 暂停 attempt/authorize/desired-present/credential/render/ACP send/apply/response/lost response/
  local fence/revoke/desired-absent/detach/resource release/delete/process exit/updater/App exit 每个关键前后相位。
- 覆盖 initial attach、same-run base rebuild、reject、timeout、applied-response-lost、ordinary/handshake/model
  Stop、exact cancel、Retry、delete、feature-off two sessions、logout/account/provider/data-root、reconnect
  desired present/absent、live/background/parked respawn/crash、context/model/compact/fork/resume、shared ACP
  A/B、updater/App exit、late A after B。
- 每行实际调用 old/new credential，只记录 authorized/401/count/generation，绝不记录 token 值。
- 每行检查 base MCP 保留、CU entry 精确数量、Broker/lease/in-flight、wrong-target/post-fence dispatch=0、
  borrowed return、owned cleanup、PID/port、retry ledger、late generation。
- 至少一套矩阵穿过 Tauri command/session event boundary。

R6（29-34h）：完整门禁、fresh build、freeze、App-shell

串行执行并记录 exact discovered/pass/fail/ignored：

cargo fmt --all -- --check
cargo test -p grok-computer-use-core --locked --offline
cargo test -p grok-computer-use-core --all-features --test driver --locked --offline
cargo clippy -p grok-computer-use-core --all-targets --all-features --locked --offline -- -D warnings
cargo check -p grok-app --all-targets --locked --offline
cargo clippy -p grok-app --all-targets --locked --offline -- -D warnings
cargo test -p grok-app --lib --locked --offline --no-run
pnpm deps:check
pnpm audit:prod
pnpm typecheck
pnpm lint
pnpm test
pnpm build:ui
pnpm check:computer-use
node scripts/audit-computer-use-bundle.mjs
python scripts/check-code-quality-gates.py --mode final
git diff --check

Windows App tests 必须 mt.exe post-link 后直接跑当前 harness。Build branch App、current cu_probe、current
source ACP stub。最后一次 candidate-affecting change 后计算完整 fingerprint 并 freeze。用两个全新隔离
home 跑两次完整 App-shell；两条 slash 必须打开 panel 但不授权；每轮验证 actual catalog/credential；
退出后检查 owned PID/port/profile/temp/lease。Freeze 后任何 source/test/runner/config/runtime 修改都使
两次 smoke 和累计 soak 失效，append invalidation，fresh build/refingerprint 后重来。

R7（34-43h minimum）：active soak 与 mutation

同一 frozen fingerprint 至少执行：

- Desktop full lifecycle >=30
- Managed Browser full lifecycle >=30
- App WebView full lifecycle >=30
- handshake Stop seeded >=50
- ACP crash live/background/parked/shared seeded >=100
- broader phase/fault seeded >=300
- 每批 smoke 与 soak 周期执行 slash Desktop/Managed Browser
- 最终 freeze 后真实 scenario active duration >=2 小时

Active time 只能累加 manifest 中 scenario start/end；build、idle、sleep、等用户、Goal 中断不算。禁止
sleep 凑时间；增加 mutation、negative、race、resource leak 场景。普通场景每类 success >=90%，下列
必须全部为 0：unauthorized read/write、wrong target、post-fence dispatch、stale credential accepted、
old cleanup/exit hurts new run、stale CU catalog、base MCP loss、secret/Cookie/token/private URL/path leak、
borrowed tab closed、owned orphan、unaccounted child/listener。任何 safety counter >0，Track A 失败。

R8（43-47h）：只有 Track A 在一个 fingerprint 全绿后才做 Existing Tabs

如果 Track A 没绿，继续修 Track A 并重新 freeze/soak，禁止为了显得进度多而开发 Existing Tabs。

先重写 pairing ADR/threat model，覆盖 malicious local process/page/extension、arbitrary loopback origin、
replay、two App instances、browser restart、MV3 worker suspend、navigation/reconnect、concurrent Share/Stop、
stale document、leakage。当前 pairing 不可直接沿用：no-Origin/loopback caller 可拿 public challenge，
pairing-confirm 直接忽略 response，pair.js 在没有按钮时自动确认，App 端立即确认，缺
extension-only possession proof。

新 pairing 必须：

- App 与 extension popup/service worker 各有一次真实 user gesture；
- CSPRNG one-time secret/code 只在 App 显示或真正 protected channel 传递，public challenge 不返回；
- Origin/Host/claimed extension id/public nonce 不能单独当身份；
- 禁止 DOM button absent auto-confirm，删除注入所有 loopback page 的 content script；
- expiry、one-shot burn、replay、App instance、extension build/id、connection generation、rotation；
- session key 只在 memory 或 chrome.storage.session，禁止 local/sync/URL/DOM/history/log/screenshot/frontend；
- App exit、feature-off、extension/browser restart、Unpair、protocol error 全部 revoke。

先 attacker reds，再实现最小真实 vertical slice：

- popup: Pair / Share current tab / Stop sharing / status / error / Retry；
- 最小权限，通常 activeTab+scripting；若用 storage.session 才加 storage；任何 tabs/debugger/capture/host
  扩权必须单独证明必要性；
- authenticated typed loopback WebSocket 或 bounded request channel；protocol/request id/deadline/cancel/
  sequence/connection generation/tab id/document generation；heartbeat/bounded reconnect/backoff/rotation；
- isolated-world typed observe/click/set_value/type/key/scroll/wait/navigation；禁止 arbitrary eval；
- screenshot 仅限明确 shared 且 current/visible tab，不满足就 pause/fail closed，不偷偷切 tab；
- reload/nav/close/disconnect 先 bump generation，使旧 observation/action 在副作用前失效；
- App picker 只列显式 shared candidates；authenticated transport 活着才注册 ExistingTab executor；
- Stop 归还 borrowed tab、移除 injection/grant，绝不关闭用户 tab；
- 禁止 Cookie/storage export、全 tab 枚举、hidden capture、Desktop fallback。

source Chrome、source Edge、installed Chrome、installed Edge 分开记。manifest/extension load/pairing state
machine/in-memory offered tab/string search 不算产品 transport pass。外部条件不足写 not_run 或
blocked_external。

R9（47-48h）：从 manifest 生成报告

报告必须列 branch/HEAD/完整 dirty inventory/run/fingerprint/freeze history、每批 exact counts/evidence
level、first failure/recovery、runtime isolation、handshake/process-exit matrix、credential oracle、actual
ACP catalog/base MCP、全部 lifecycle、Clippy/quality/dependency/frontend/Rust/build/bundle、App-shell、
active time/rounds/seeds/rates/safety counters、Desktop/Managed/WebView/Existing Tabs、owned cleanup、Windows
source/installed、macOS arm64/x64、Linux X11、GNOME native Wayland、Chrome/Edge source/installed、ACP stub、
real model、剩余 P0/P1/P2/技术债/外部 blocker/下一动作，以及 literal：

未 commit / 未 push / 未提 PR

macOS/Linux/installed/real-model 没执行就写 not_run 或 blocked_external，绝不复制 Windows、mock、
cross-compile、XWayland 或旧 fingerprint 结果。

长任务运行规则：

- 每个 atomic batch 后且至少每 60-90 分钟写 checkpoint，然后自动继续，不问“是否继续”。
- checkpoint 包含 run/fingerprint/freeze/invalidation、文件、manifest seq、exact counts、first failure、
  hypothesis/recovery、redacted credential、actual ACP catalog、lifecycle postcondition、resource cleanup、
  active time/rounds/seed/safety counters、PID/port/profile/lease、external blocker、next action、
  未 commit / 未 push / 未提 PR。
- 同一失败同一假设最多重试三次；保留首败，换有证据的新假设或转做独立 ready work，禁止无限跑命令。
- Goal/进程中断从最后 checkpoint 续跑，不改历史，不把中断算 active。
- 只有用户明确停止、确实需要新的真实权限/账号/外部 OS/破坏性状态变化，或所有剩余任务都被 external
  blocker 阻塞时才停。否则持续推进，不能在写完代码或跑完 unit 后提前说任务完成。
- 不要 sleep 凑 48 小时，不做无意义重构。若实现提前完成，继续 mutation、negative、race、leak、
  current-App 与 active soak；不允许用“时间不够”跳证据。

最终顶层结论只允许三种：

Track A local source gate passed; overall Computer Use partial - not releasable
Track A and secure Existing Tabs local source gate passed; overall Computer Use partial - not releasable
partial - not releasable

四个原生目标、installed、real-model 证据不全时，绝不能写 complete、releasable、cross-platform passed、
installed passed 或 real-model passed。

现在从全文阅读、R2D evidence root、current fingerprint 和 R0 executable red tests 开始。不要先写完成
报告，不要请求用户是否继续，不要自行提交或发布。
```

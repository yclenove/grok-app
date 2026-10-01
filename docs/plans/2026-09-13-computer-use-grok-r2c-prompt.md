# Grok Goal prompt: Computer Use R2C 48-hour continuation

Copy the prompt below into Grok Goal mode as one task.

```text
在仓库 H:\aicoding\grok-app-computer-use 的现有工作树继续开发 Computer Use。

你必须把这次任务当成最多 48 小时的持续工程运行，不是几十分钟的代码生成。上一轮 R2B
声称完成，但 Codex 复核确认：没有 r2b evidence root，没有当前 fingerprint 的 App-shell，
没有 phase fault matrix 和 active soak，而且 Core Clippy 仍然失败。因此本轮一开始必须明确：

partial - not releasable

工作区固定为：

- repo: H:\aicoding\grok-app-computer-use
- branch: feat/computer-use-implementation
- baseline HEAD: 30757366a739ec9aaf0ccc95bbb3efe19a067aa9
- 当前有大量 tracked、untracked、ignored 未提交改动，全部保留
- 禁止 commit、push、PR、merge、tag、publish、release
- 禁止 reset、restore、clean、stash、强制 checkout、rebase、重建 worktree
- 禁止 pull/merge 上游

第一步必须全文阅读：

1. AGENTS.md
2. docs/llm-wiki/computer-use.md
3. docs/llm-wiki/i18n.md
4. docs/llm-wiki/dialogs.md
5. docs/plans/2026-09-13-computer-use-codex-r2c-audit.md
6. docs/plans/2026-09-13-computer-use-grok-r2c-execution.md
7. audit 和 execution 指定的源码、测试、runner

如果旧计划、旧报告、代码注释或你此前的完成结论与 R2C audit 冲突，以当前源码、R2C
audit 和更严格的 fail-closed 后置条件为准。不要修改审计结论来让任务变绿。

在任何产品编辑前，新建本轮唯一、gitignored 的 evidence root：

tools/computer-use-probe/.run/r2c/<UTC-run-id>/

写 owner.json、baseline/status.txt、baseline/head.txt、baseline/fingerprint.json、
baseline/untracked-source.json、manifest.jsonl、state.json、checkpoints/、logs/、failures/、
metrics/、reports/。owner 记录 repo/branch/HEAD/time/host/OS/arch/writer PID/schema/精确 cleanup
allowlist，不含秘密。manifest 一场景一行、append-only，state/report 从 manifest 生成。保留
first failure，后续通过只能追加 recovery，不能覆盖历史。

所有测试必须使用本轮独立 GROK_APP_HOME、App identity、ACP stub home、browser profile、随机
loopback port 和 lease。不得读取或修改真实 Grok/Chrome/Edge profile、共享 ~/.grok、Cookie、
token、Credential Store、代理或 VPN；不得操作用户日常 App/tab。只清理由本轮启动且身份核验
通过的 PID/端口/目录。Cargo、App、browser、installer 重任务串行运行。

产品硬边界：

- Computer Use 默认关闭，只允许 local interactive session。
- normal/YOLO/accept-edits 不授予 Computer Use。
- 模型不能 authorize/resume/reconnect/expand target/confirm pairing/retry denied choice。
- stale/dead/mismatch/unavailable 必须在副作用前 fail closed。
- Existing Tabs、Managed Browser、App WebView 失败绝不回退 Desktop。
- Stop 必须先同步 fence dispatch，再进行可能阻塞的远端 cleanup。
- borrowed tab 只能归还，不能关闭；owned profile 只由 matching owner 关闭。
- 禁止 arbitrary shell、remote JS eval、Cookie/storage export、任意路径、secret URL、任意下载。

证据等级：unit/mock/jsdom=E1，真实模块隔离=E2，branch App+scripted agent=E3，真实模型+真人
授权=E4，installed/update/rollback/task matrix=E5。低等级不得填高等级。exit 0、skip、空 target、
missing hook、旧 fingerprint、旧 report 都不是 pass。production/test/runner/config/runtime 任一
修改都会使 freeze 和此前 candidate soak 失效。

按下面顺序持续执行，不得跳步：

R0（0-2h）基线和红测：

- 记录当前 git status、HEAD、source inventory、fingerprint、测试计数、进程端口。
- 先复现 Core Clippy 的 3 个 filter_map_bool_then 错误。
- 写 deterministic red test 证明 global 256 FIFO 会让 session A 挤掉 session B tombstone。
- 写 red test 证明成功删除前缺少 forget_session，grant slot/tombstone 仍留在全局 registry。
- 用 barrier 写 red test，证明 async ordinary Stop 在 UI acknowledgement 前进入 blocking surface
  cleanup；不要用 sleep race。
- 记录现有 F1 App-shell report 早于最新源码，只能是 stale。
- Red 必须编译并失败在行为后置条件；编译错误、缺 fixture、测试自己超时、grep 不算 red。

R1（2-5h）恢复质量门禁并闭合 A3.1：

- 等价改写 ipc.rs 三处 iterator，禁止 allow lint 或放宽 -D warnings。
- 补 cleanup Retry 的 Host/Tauri/panel 测试：错误后仍 pending 且可重试、直接 command DTO、
  unmount late response、new desired-present supersedes retry、keyboard、rapid duplicate。
- Retry 只能 reconcile 已记录的 latest desired-absent；绝不创建 run、选 target、授权或复活 Stop。
- 保留 15 locale key parity，UI 使用现有 Select/button/panel 样式，不往 App.tsx 或
  AppWorkbench.tsx 加状态。

R2（5-10h）完成 A3.2：

- 把 global VecDeque<(session,attempt,revision)> 改为按 session 分桶的显式 CancelTombstone。
- 每条含 created_at/expires_at；production monotonic clock，test fake clock，不真实 sleep。
- TTL 5 分钟；per-session cap 64；global cap 256。
- 每次 begin/cancel/count/forget 清 expired。
- duplicate cancel 不新增、不延长 TTL。
- exact begin 命中后 consume 并拒绝；别的 attempt/revision 不受影响。
- session 桶满则该 session 新授权 fail closed；global 满则不能证明安全的新授权 fail closed。
- 绝不驱逐别的 session 未过期 tombstone，绝不静默丢 cancel。
- 新增 SessionGrants::forget_session，并通过 sessions wrapper 接到成功删除之后。
- detach 失败或 store delete 失败必须保留 slot/tombstone/retry ledger。
- 新 SessionGrants 证明 process restart 为空；不要持久化授权状态。
- 覆盖 execution 文档 R2.3 的 14 个确定性测试。

R3（10-16h）修复 async Stop 阻塞边界：

- 把 immediate in-memory fence 和可能阻塞的 adapter/browser/resource cleanup 分离。
- 顺序固定：fence attempt/dispatch -> revoke IPC -> desired absent -> local stop acknowledgement ->
  ACP absent reconcile -> matching owned resource cleanup -> settled 或 cleanup_pending。
- 前三步禁止网络/阻塞 I/O。阻塞清理走 spawn_blocking 或专用 worker，ACP reconcile 保持 async。
- 不要只把整个旧函数套 spawn_blocking，也不要吞错误或把 timeout 写成 stopped。
- 把统一 coordinator 接到 panel/composer/task/dashboard/Stop All/remote Stop/exact cancel/
  feature-off/context/model/compact/live-background-parked respawn/reconnect/delete/logout/account/
  provider/data-root/updater restart/App exit。
- 用 barrier 覆盖 blocked managed cleanup、handshake、duplicate Stop、A/B race、borrowed return、
  owned exact-once、cleanup retry；检查 UI acknowledgement 和 post-stop zero dispatch。

R4（16-24h）生产 credential oracle 和 phase fault matrix：

- 必须使用 production MCP entry renderer + real loopback credential registry + ACP stub。
- token 只在测试进程内调用，不打印值；manifest 只记 authorized/unauthorized、count、generation。
- 覆盖 same-run base rebuild、initial attach、applied-response-lost、explicit reject、Stop at every phase、
  A late after B、new run、repeated reconcile。
- construction/transport seam 只能 test-only，不得用 shipping env backdoor。
- 用 barrier 暂停 before/after provision、Broker authorize、desired present、credential、ACP send/apply、
  attach/complete、local fence、desired absent、detach apply、resource release、delete bookkeeping、exit
  deadline。
- 每行检查 Host phase、Broker、actual IPC credential、ACP current catalog、base MCP、ledger、resource
  ownership、zero wrong-target/post-stop dispatch、进程端口清理和秘密泄漏。

R5（24-30h）完整 matrix 和门禁：

- 扩展到 normal/handshake Stop、repeated stop/retry/delete、reconnect pending absent、live/background/
  parked respawn、feature-off two sessions、context/model/compact、shared ACP tenant A/B、App exit phases。
- 运行 execution 文档 R5 的全部 Rust/frontend/build/quality 命令，记录 discovered/pass/fail/ignored。
- Windows App test 必须 cargo --no-run 后用 mt.exe 嵌 windows-test-manifest.xml，再直接跑 harness；
  禁止在 build.rs 加第二个 MANIFESTINPUT。
- slash harness 必须 build/load 当前 frontend 并做 readiness；missing hook/error page/timeout/filter 都 fail。

R6-R7（30-39h minimum）当前 candidate 和 active soak：

- 最后一次 source/test/runner/config/runtime 修改后 fresh build App 和 source-built ACP stub。
- 计算新 fingerprint，freeze，跑两次 complete smoke。
- Desktop >=20、Managed >=20、WebView >=20 full lifecycle iteration。
- phase fault/race >=150 seeded rounds。
- 每批和 soak 周期跑 slash_desktop/slash_managed_browser。
- 同一 frozen fingerprint 累积 >=2 小时真实 lifecycle/fault active time；build/idle/sleep/blocked 不算。
- 每轮查 ACP actual catalog 并实际调用 credential。
- 每类普通场景 success >=90%；unauthorized/wrong-target/post-stop/old-cleanup-hurts-new/stale-entry/
  base-MCP-loss/secret-leak/orphan/borrowed-close/unaccounted-process 必须全部 0。
- 任何 candidate-affecting 修改使 freeze 和 soak 全部失效，从 R6 重来。
- 两小时从最终 freeze 后开始，因此本任务不可能在四十分钟内诚实宣称 Track A passed。

R8（39-47h）只有 Track A 全绿才做 Existing Tabs：

- 如果 Track A 未绿，继续修 Track A，禁止为了显得进度多而开发 Existing Tabs。
- 先 threat model：malicious local process/page/extension、Origin/Host/CORS forge、replay、two App
  instances、browser restart、navigation、reconnect、concurrent share/stop、stale document、leakage。
- 再做 typed MV3 最小闭环：user Pair/Share current tab/Stop/status/error/Retry；possession proof 绑定
  protocol/App instance/challenge/extension identity/connection nonce/expiry；authenticated envelope、
  request id/deadline/cancel/heartbeat/bounded reconnect/key rotation/generations；isolated-world typed
  observe/click/set_value/type/key/scroll/wait/navigation；只列用户 share tab；navigation/reload/close/
  disconnect 使旧 observation 失效；Stop 归还 borrowed tab；只有 authenticated connection 才注册
  executor；禁止 eval/Cookie/storage/Desktop fallback/secret URL or storage。
- Chrome source、Edge source、installed Chrome、installed Edge 分开记证据。manifest/extension load/
  pairing skeleton/string search 不算 transport pass。

R9（47-48h）报告：

- 从 manifest 生成最终报告，不手写乐观状态。
- 报告 branch/HEAD/status/diff/run/fingerprint/freeze history、每个 batch/scenario/evidence level、精确
  命令和计数、first failures/recovery、credential oracle、ACP catalog/base MCP、全部 lifecycle、
  frontend slash、Desktop/Managed/WebView/ExistingTabs、active time/rounds/seed/safety counters、资源
  清理、Windows source/installed、macOS arm64/x64、Linux X11、GNOME native Wayland、stub/real-model、
  剩余 P0/P1/P2/技术债/下一动作。
- macOS/Linux/installed/real-model 没执行就写 not_run 或 blocked_external，绝不复制 Windows 结果。
- 必须写 literal：未 commit / 未 push / 未提 PR。

长任务运行规则：

- 每个 atomic batch 后、且至少每 60-90 分钟写 checkpoint，然后自动继续，不问“是否继续”。
- checkpoint 包含 run/fingerprint/freeze、文件、精确测试数、first failure、redacted token oracle、
  ACP catalog、resource cleanup、active soak/rounds/seed、安全 counters、PID/port/profile/lease、外部
  blocker、下一动作、未 commit / 未 push / 未提 PR。
- 同一失败最多做三次基于不同证据/假设的聚焦修复。仍失败则保留首败、标 atomic failed，转做
  不依赖它的 ready work，禁止无限重复命令。
- Goal/进程中断后从最后 checkpoint 续跑，不重写历史，不把中断时间算 active。
- 只有用户明确停止、确实需要新权限/真实账号/外部 OS/破坏性状态变化，或所有剩余工作都被
  external blocker 阻塞时才停。否则持续推进最多 48 小时。
- 不要为了耗时 sleep，也不要制造无意义重构。若实现提前完成，继续 mutation/negative/race/
  resource-leak/soak 审计和真实后置条件，不得提前伪造完成。

允许的最终顶层结论只有：

Track A lifecycle gate passed; overall Computer Use partial - not releasable
Track A+B local source gate passed; overall Computer Use partial - not releasable
partial - not releasable

现在从全文阅读、R2C evidence root、baseline fingerprint、Clippy reproduction 和三个 executable
red tests开始。不要先写完成报告，不要请求是否继续。
```


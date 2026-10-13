# Grok U01-R2 finish：先编译，再关闭授权与资源收束缺口

你是本工作区唯一产品 writer。Codex 下任务、只读独立审查，不替你写产品实现。
本文件只有经 Codex 原 launcher 实际派发才执行。沿用原 session/home/branch，
grok-4.7/high/no-subagents。本批160 turns是监督窗口，不是总目标结束条件。

全文读 AGENTS.md、CONTRIBUTING.md、相关 wiki、原 U01-R2 dispatch、ADR 和
`2026-10-03-computer-use-task-consent-u01-rework2-codex-review.md`。原 batch04 已
max180 / exit1 / join，checkpoint 缺失；之前五项/32项绿后还有未验证修改。
不得沿用旧绿报告声称当前通过。本批只完成 U01 核心，不开始 U02/Host/UI/平台重写。
U00–U09 / D00–D31 全范围继续保留，不减少功能或平台。

工作区 H:/aicoding/grok-app-computer-use，feat/computer-use-implementation，HEAD
8d8b71c262ab222748bca32412eea884d4b892e0。保留所有 dirty/untracked 文件。

## P0：立即修当前真实产品测试编译

Codex 已在实际 crate 执行 `cargo test -p grok-computer-use-core --offline --lib --no-run`，
Cargo101 / E0277：`broker/tests_task_consent_rework2.rs:604` 的 `{committed:?}` 要求
CommitOutcome:Debug。采用不暴露敏感字段的合法诊断，保持失败谓词；不要盲目
derive Debug、不删测试、不忽略/降门禁。此问题不是 Windows manifest 错误。

编译修好后立刻跑你最后新增的 R2 回归，保存本候选的实际结果。每一次命令使用新的
唯一日志路径，保留所有编译/行为/timeout/中断失败；内层真实退出码必须传出。
先收敛已有 unfinished 回归，不再次先铺大量未编译代码。

## P1：把五个独立谓词迁入产品，取得实际 red→green

只读参考 batch04 ignored `codex-review/authority_deadlines.rs`、`broker_extra.rs` 和
终态 review。不修改任何旧 source-pristine/snapshot-crate/manifest/result/log/test。
Codex 的五项尚未进入断言，不能写成已红。迁入产品回归可按最终 API 改 fixture/seam，
必须保留同一谓词、合法正向控制、实际 adapter 调用及 cleanup；先留自己实际红输出。

1. raw monotonic advance 必须与 sweep 分离，仅 cfg(test) seam；9分钟可 prepare，
   11分钟未 sweep 时 start_preparation 拒绝。
2. publish/合法准备后跨 deadline，preparation_current/finalize 直接复验并拒绝，
   Broker 的失败路径仍 abort 本预约、rollback 自己的 feature 迁移、收尾原候选。
3. grant/claim 在29分钟有效，31分钟未 sweep 时 validate/consume 拒绝；final permit/
   binding/capability 准入也不得用过期 authority。别把在物理执行中的动作误判真空闲。
4. 同 task 两个 incremental request、不同 run 都实际 observe 成功；complete_task
   将全部本任务原 run 同步 fence，全部 request/continuation 失效，消费 cleanup 后
   全部 Stopped。覆盖尚在准备/迟到绑定的同 task 资源，保留原 owner 收束责任。
5. 一个真实合法模型 read 之后raw+31分钟，仅一次到期模型请求返回拒绝，再无后续
   成功调用；同一调用必须交接原 expiry fence/cleanup。submit/status/deny/cancel/
   claim/validate/consume 等可能制造 expiry 的错误/早返亦逐项审计。

不要用 wrapper0、0 tests、过滤错名字、实际睡30分钟或每次advance自动sweep假通过。
authority-only 与实际 FakeAdapter synthetic 证据分开，不自动批准真实 App/系统目标。

## P2：容量满时撤执行权、保留资源责任

当前 sweep queue_expired_run 失败就 keep grant，原新增容量测试甚至期望到期
execution_run 成功。先修这种错误回归口径：既有容量/资源保留谓词不删除，但 expired
grant/continuation/permit 必须不可执行，adapter 新效果0。

- 将不可执行权威和原 resource/cleanup obligation 分清；backpressure 满不能延长
  TTL、重新批准、复活旧票据，也不能静默丢 run/准备归属或原 cleanup incarnation。
- 真正填满 expiry/cleanup/notice/retired token 队列，随后提交到期 read/input/browser
  和旧 continuation，全部新效果0；再实际消费原票据证明所有 held run 最终 terminal。
- 顺序96 owner回收、并发上界、老ctx/transport ABA及未终态owner保留继续全过；
  不提高MAX、不把义务转入另一个无界集合，Stop仍立即fence。
- take/drain一次不应永久遗失未交接义务；error/refused fence及队列已满路径也审计。

## P3：完成原 F1–F4，禁止局部绿替代完整合同

原 U01-R2 全部要求仍有效。已实现的部分不无故重写，逐个补证据和真正遗漏：

- F1 两目标同时活/A退休、能力两个方向、B pending/deny/cancel/绑定后失败不损A，
  final admission前Stop/replacement/reconnect/scope退休效果0。真实合法payload、
  精确 adapter target/effect；browser observe/wait/action同 membership/capability。
- F2 feature原on/本次enable，真正最后check→open、open→note、bind后、finalize前。
  预约一直拥有原candidate，即使Stop先发生；最终grant/continuation0且原run/lease/
  cleanup终态。消费真实原ticket，慢abort未join不允许新lease借出。
- F3 安全回收slot并保留物理未终态义务，旧ctx真实start_preparation拒绝/新ctx正向；
  清点所有集合的上界、消费和回收，不能只调大MAX或保留恒Err测试专用API。
- F4 真正模型read/act/wait/browser完成刷新空闲边界；preview/status/失败申请/旧ctx
  不刷新。长in-flight/原cleanup未终态保留资源；业务unknown不代表物理永远busy。
  原动作/cleanup物理完成后的 abandoned grant 必须恢复TTL并最终失效，late旧完成
  不能延长 replacement/Stop/feature-off 权威。审计busy快照与最后准入/到期的竞态。
- Typed NoAuthority/Continuation owner/request/task/revision/epoch/dispatch id、once-only
  接口保留可实际消费合同；notice不授执行权。Host send ledger/expiry watcher留U02。

每个负向配合法正向 effect control；只看Err或grant计数不等于物理fence/cleanup。
新增现有1200行大测试模块需要按合同领域拆分；不抬质量上限或制造同规模新大类。

## P4：按最终候选验收并留下可接力交付

依次执行修复相关回归、完整受影响 core/IPC tests、core test-support gates、fmt、
strict Clippy、协议/golden/MCP。如需兼容编译其它workspace package运行实际检查，
不要以workspace default-members误以为含所有 core。App libtest若必需才用现有
CI post-link mt.exe流程；当前core不需要manifest修复。未改UI无需刷UI全套，未改P0
oracle无需再启动真实CLI probe。可并行只读检查，不启动第二产品writer。

Node路径核实际存在：`tools/computer-use-mcp/protocol.golden.test.mjs`、
`tools/computer-use-mcp/protocol.drag.test.mjs`；其余门禁按repo现有脚本执行。
所有命令/日志/退出码/hash/测试数/filtered/ignored写明，失败保留，不能默认绿色。

使用新的 `tools/computer-use-probe/.run/task-consent-u01r2-finish-20261003/` 证据空间，
每个invocation独立文件；旧batch00–04审查、logs、首红均只读。采用apply_patch，LF，
pnpm；产品领域拆分、现有对话框/i18n/15locale/设置登记和App shell增长冻结照规约。

到120 turns前写阶段checkpoint，留40 turns处理完整回归和报告；这是保存接力而非
降低验收标准。若窗口将满，准确列剩余红灯/未验，不以max-turn取消说任务完成。

交审文件：`docs/plans/2026-10-03-computer-use-task-consent-u01-rework2-finish-checkpoint.md`。
逐P/F记录最终实现、红→绿原证据、final source/binary identity、效果/cleanup/资源
终态、当前API及诚实未验项。ADR修成最后真正实现；Codex review由Codex写，不改旧结论。
最终回复只是交审，不自行跳U02或称全部CU已交付。Codex终态独立审查通过才下U02。

## 不越界

绝不commit/push/PR/merge/tag/release/reset/restore/clean/stash/rebase，不改credentials、
共享~/.grok、用户browser profile、代理/VPN/其它worktree/日常进程。无subagent、新
thread/automation/消息或真实App/系统授权自动批准。外层always-approve只是隔离编码
权限，不是产品CU授权/真人证据；保留产品default-off/no-desktop-fallback。

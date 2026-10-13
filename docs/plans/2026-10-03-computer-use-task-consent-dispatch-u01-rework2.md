# Grok U01-R2：四个独立反例的定点返修

你是唯一产品 writer；Codex 负责派任务和审查，不接管实现。
本文件只有在 Codex 通过原 launcher 派发后才执行。使用同一 branch/session/home，
grok-4.7/high/no-subagents；不 restore-code、不启动另一 writer 或 automation。

先全文读 AGENTS/CONTRIBUTING、相关 wiki、U01-R dispatch/checkpoint/ADR、
`2026-10-03-computer-use-task-consent-u01-rework-codex-review.md`。
本批修 F1–F4 及直接关联的遗漏，不开始 U02/UI/其它平台重写。原总目标不缩减。

## 0. 从真正红灯开始

Codex ignored 审查目录：
`tools/computer-use-probe/.run/cu-consent-orchestrated-20261002/`
`batch03-u01-core-rework/codex-review/`。

`red-authority-reviewed.txt` 已实际运行1 pass/4 fail、Cargo101。
source-pristine是164文件原文，snapshot-crate只加载了独立测试；source/manifest/hash
见 Codex review。不要修改这些文件、旧七反例、run-review.ps1 或旧结果报表。
首轮 sibling module 编译错误是 reviewer harness 错误，与四条行为失败分开。

把四条失败谓词和正向控制迁入产品回归，先保留当前版本自己的红输出，再最小实现。
接口改变时用新 API 表达同一谓词，不能改断言、expected、工具目录或 capability 来报绿。
synthetic fixture 必须显式标记，不自动批准真实 App/系统/用户目标。

## F1. 精确范围/能力贯穿最终派发（先做）

当前 task run 被 A/B 复用，Permit 选 A、RunState 却是 B，两个 request 同为 revision1；
只比 task/revision/run 不够。先明确 TaskGrant、scope member、current target、permit
各自权威，再修复实际链：transport → 当前 grant → target membership → Broker admission
→ adapter。不要在 tools 层多加一次前置检查后再降成裸 SessionBinding。

- 每个执行准入包含完整 app/agent/nonce/incarnation/connection/task/request/grant revision/
  feature epoch、真实成员/target identity和所需能力。不能借另一 request 的 revision1。
- 最终 observe/act/wait/browser入口核同一许可/能力/目标；Host preview与模型读取分开。
  边界检查与准入受现有 fence/cancellation约束，不长持 broker mutex 做原生调用。
- A observe、B input-only，B不能读取；A input、B observe-only，B不能输入。
  两个目标同时活和A已退休两种都测。采用有意义合法payload，并核实际派发目标/次数。
- A/B增量成功、B pending/deny/cancel、实际目标已绑定后的失败，A仍按原批准范围可执行；
  B失败不能把run切到B、清A的snapshot/lease/grant或借A能力。普通范围内切换仍需新观察。
- 切点：选grant后、目标切换后、最终admission前，插Stop/agent replacement/connection
  rotation/scope retirement，核 adapter新派发0。保留 old ref/geometry/unknown不重放合同。
- U04负责生产四表面resolver；本批先冻结可安全承载真实membership的core结构和API。
  不临时只保留一个grant、禁止增量、硬编码synthetic目标或靠猜当前focused target躲开问题。

## F2. 原预约一直拥有迟到候选，失败与Stop必收尾

准备记录的物理收束责任与feature rollback责任分型；原本feature已开不等于没有资源。
不能在Stop/deny/cancel/retire时删除未完成prepare，只因为we_enabled=false。

- 在产生run/target前登记精确预约/候选owner；Stop同步fence它，即使run还没进表。
  迟到open/bind必须由同一预约归属收束；不能丢ownership后按run名前缀搜回。
- 覆盖feature原本on与本批准新enable两条路径；Stop在最后check→open间、open→note间、
  target绑定后、最后check→finalize间，分别核原run/ticket/lease/target终态，grant/ticket0。
  现有before_open hook在另一检查前，不要继续拿它称已测真正间隙。
- 真正final commit在权威锁内复验完整owner/request/task/epoch/cancel及实际run/target/stop。
  任一finalize失败走本事务abort/cleanup，不用裸?逃掉；失败时只回滚本事务仍拥有的
  feature迁移，epoch向前，不覆盖用户的新开关或其它偏好。
- cleanup ticket绑定原resource incarnation；Stop回执仍不等慢abort；原owner未terminal/
  join不释放下一lease。测试实际消费票据，核cancellation、terminal和新owner安全执行。
- prepare失败/取消/到期后不会永久卡approving或留下不可重试的孤儿；重试是明确的新预约，
  旧票据不能被复活。并发批准仍只一次commit/续行。

## F3. 安全回收owner名额与真实上界

纠正 `retire_does_not_free_a_global_owner_slot`，合同是live/retained资源有界，
不是进程终身32个聊天。不能把MAX_OWNERS调大来掩盖。

- 至少96个不同App session顺序 issue/bind/request/retire成功，逐轮旧token和捕获ctx
  request/status/cancel/实际start_preparation/execution拒绝，nonce/incarnation不复用。
- 并发达到上界时合理背压；pending prepare、未退出动作、cleanup、未消费有效票据的
  owner仍被保留。原资源终态之后，才可回收并继续新会话。
- 同agent重复登记保持token稳定；首次绑定的预留token稳定；replacement/reconnect
  退休旧身份。用正确当前ctx实际start_preparation成功作为旧ctx失败的正向控制。
  删除/替换恒返回Err的旧reserve兼容测试seam，不为测试保留虚假产品API。
- 清点owners/requests/preparations/decisions/grants/scope memory/retired_tokens/
  retired_outbox/expired_runs/cleanup_tickets，分别给真实容量、回收/消费者和负载测试。
  flood不能把数据从有cap的集合移到无界outbox，不能静默丢有效权限/cleanup责任。
- 停止槽和rate独立；flood与慢cleanup下Stop仍可立即fence，原账本终态可回读。

## F4. 实际空闲计时

目前批准时只初始化idle_deadline，busy时跳过不会刷新空闲时长。
让真实工作准入、物理结束和owner当前权威共同决定activity/idle，避免单独busy快照竞态。

- 29分钟后一次真实模型observe完成，再2分钟，grant仍有效；随后真空闲超过TTL才撤销。
  act/wait及已支持browser工作同样覆盖；只有函数返回ok而未到adapter不是活动证据。
- 长action未退出或原cleanup pending时不因扫描而释放资源；完成后从真实空闲边界计时。
  failure/timeout按实际物理生命周期处理，业务unknown不证明资源已空闲。
- preview、无权申请、status轮询、拒绝的旧ctx不得替模型伪造活动；late completion不刷新
  replacement任务，不延长Stop/feature-off后的旧grant，不恢复权限。
- 用可控monotonic测试时钟/事件门控，不真实睡30分钟、不调大TTL/删expiry回归。

## 上层合同复核与交审

NoAuthority通知和Continuation要有typed owner/request/task/revision/epoch以及一次性调度
身份；NoAuthority不得触发执行，也不能只给U02无消费/无owner过滤的理由字符串数组。
先把真实可用API写checkpoint；忙时prompt/send ledger的实现仍留U02。

依次跑有意义红→绿/竞争回归、受影响core/IPC完整suite、fmt、strict Clippy、MCP/golden。
原P0 oracle没改就不重跑CLI；未改UI不刷UI全套。内层Cargo/Node退出码显式传出；
0 tests、killed、timeout、wrapper0均非通过。新增测试需有正向效果控制，不只断言Err。
不豁免lint、ignored测试、不删现有安全场景；按领域拆分，不抬质量上限。

交付 `2026-10-03-computer-use-task-consent-u01-rework2-checkpoint.md`，逐F列实现/验证、
先红/绿日志和命令/实际退出码/hash/派发目标/计数/原资源terminal/join、必要未验项。
ADR修订为最后真正实现，尤其scope能力、owner回收和idle，不让文档继续给错误行为背书。
Codex审查接收后派U02；不能自跳U02或宣称完整CU交付。

绝不commit/push/PR/merge/tag/release/reset/restore/clean/stash/rebase；不改credentials、
共享~/.grok、用户browser profile、代理/VPN/其它worktree/日常进程；无subagent/automation。
U00–U09和D00–D31总目标继续保留。做到可审查后交审，下一批由Codex真实派发。

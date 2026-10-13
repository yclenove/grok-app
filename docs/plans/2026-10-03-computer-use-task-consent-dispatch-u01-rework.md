# Grok：U01-R 核心授权返修，完成后交 Codex 验收

你是唯一产品 writer。Codex 负责派任务与独立审查，不接管产品实现。
沿用 `feat/computer-use-implementation` 与原 Grok session；不 restore-code。
本文件准备时上一批尚 live，只有 Codex 在核实上一批 terminal 后通过 launcher 派发，
才是这批实际开始。不得自行启动其它 writer/subagent/automation。

先全文读：本文件、`2026-10-03-computer-use-task-consent-u01-codex-review.md`、
修订 ADR、U01 dispatch 的 P1–P4；AGENTS/CONTRIBUTING 和相关 wiki 规则仍执行。
原 P0 oracle 已独立接收，不重写旧报告、快照或 Codex 反例。

## 0. 先确认事实，保留先红证据

Codex 的独立原文 snapshot crate 位于：
`tools/computer-use-probe/.run/cu-consent-orchestrated-20261002/batch02-u01-core/`
`codex-authority-review-20261003-attempt01/`。
`red-authority.txt` 是实际进入断言的 1 passed/7 failed，Cargo101。
它不是产品源码补丁，更不是真人批准。先核对 review 记录的最终hash/范围。

不要修改独立snapshot/反例来报绿。将相应断言和真实竞争场景移入产品 regression；
旧snapshot自然继续红。新的准备/commit API变化时，保留原红并用新接口表达相同安全谓词。
独立增量反例的旧A由错误prepare产生；新回归必须先完成正确Host commit得到A。

按下面顺序工作，每小节先建立会失败的有意义回归，再实现、绿验证、记录checkpoint。
不一次推进 U02/UI/platform；本批把上层可依赖的核心合同修稳。

## R1. 准备与真正commit分开；epoch只能向前

目前prepare先写Approved/grant/continuation，之后才open_run/authorize；必须改成
独立可取消准备记录，只有真实绑定成功后在权威锁内复验完整绑定并原子commit。

- preparing时status如实为pending/approving，执行grant count=0、continuation=0。
  此时status、重复commit、取票据不能把“准备已开始”当成功。
- final commit复验owner incarnation、agent、connection generation、request/revision、
  task、feature epoch、reservation、Stop/prepare cancellation，以及实际绑定结果。
- 在最后一次前置检查与open_run之间Stop，以及目标绑定中/绑定后Stop或feature-off，
  均不能遗留新run/lease/grant/票据，不能返回Approved。用可控barrier定位竞争，别靠sleep。
- 准备资源有精确owner，Stop在run尚不存在时仍取消它；不得靠runId前缀扫选归属。
  迟到候选必须由原owner fence/abort/join，而不是放进当前owner执行。
- 失败只回滚本预约自己拥有且未被更新的feature迁移；回滚bool也推进epoch。
  0→1→失败回关后epoch必须大于1，不恢复0。用户的新开/关不被旧rollback覆盖。
- 未commit的run不能经transport或legacy身份执行。重复批准最终只一份grant/一次续行。

测试至少：prepare barrier下status/count/ticket；bind失败；检查后Stop；bind后Stop；
两个批准并发；用户推进epoch后旧rollback；原候选实际收束。合成身份明确标synthetic。

## R2. owner不可复用；agent必须实际绑定

当前retire删除owner，reissue又从1/1开始，捕获ctx能撞上新owner。
使用Host不可复用incarnation/nonce或等效单调权威身份；不可只依赖token熵。

- classify到state mutation之间再次核完整owner；旧ctx在retire/reissue和同agent id重绑后
  的request/status/cancel/reserve/execution均拒绝。边界测试保留旧ctx，不只测试旧token字符串。
- bootstrap目录可先预留transport供session/new；正式模型调用与Host commit资格必须有
  真实当前agent绑定。None不是正式执行身份；不能在bind_agent时收养先前None grant。
- bind_agent首绑保持正确预留token稳定；新agent/reconnect退役旧token、准备、执行及续行。
  普通重复登记不引起无谓token轮换。两类行为分别测。
- 更新现有正向IPC/broker测试，先绑定真实测试agent，再通过实际HTTP做request/commit/observe。
  未绑定的commit/execution是负向，不能让它成为“成功证明”。
- 所有count/status/debug/model error不泄露bearer或其它session状态。

## R3. 执行准入贯穿实际工具派发

`execution_run`检查后再classify然后降成legacy SessionBinding存在撤权间隙。
不要用更多前置检查假装原子，也不要给新transport fallback到legacy权限。

- 带完整owner/task/grant revision/epoch的当前执行准入进入Broker现有action admission，
  在最终派发前受同一fence/cancellation约束。不得长持broker mutex做原生查询。
- 在grant查到后、legacy路由/adapter之前，插入测试barrier；Stop、agent replacement、
  connection rotation、capability/scope退休胜出时，adapter读/输入派发数0。
- 正向仍经同token完整commit后observe→真实owned fake输入→实际fixture后置条件；
  action/wait继续严验原snapshot/ref/geometry，禁止Host补身份绕过旧合同。
- observe-only不能input；input-only不能读取；status/list/fresh observe无需已有snapshot，
  但只回当前scope内目标。所有受管/ExistingTab/WebView入口也不能绕gate。
- legacy凭据兼容分支保留原合同；不是允许模型从新transport转用旧run来绕任务grant。

## R4. 撤权先fence，再物理收尾；Stop确认不等慢cleanup

当前release_detached_task_runs/complete仅drop_execution_identity，agent变更也不fence。
grant为0或snapshot清掉不证明旧action已终止，更不证明lease可以给下一owner。

- 替换前缀扫描为确切task/run/preparation ownership。complete、expiry、agent变更、
  connection rotation、rollback、cancel、feature-off全部按原资源正确fence。
- 原cancellation真正送达；已有execution/action admission直到原资源terminal/join保留。
  清理票据可handoff且独立消费，原cleanup不能退休同名的新owner/run。
- consent_stop只做有界fence和交出cleanup票据/准确stop_requested。
  不在同步返回前逐个finish_stop_cleanup；复用原有owned清理机制，不能另开无界线程。
- 有可控慢action/慢abort的owned fake adapter回归：fence后0新派发、旧action收到取消，
  原物理owner未确认退出时新lease被拒绝；release后才可新执行；Stop回执不被慢abort堵住。
- 到期扫描必须交出受退休run的cleanup工作；不能只删除grant然后再永远不知道谁收束run。
  空闲TTL按真正空闲计时，长action或cleanup待终态不靠另一次模型tool刷新而延长。

## R5. 一个Host任务、增量范围、完整typed票据

先冻结上层所需的最小合同，再改实现；U04做真实多目标resolver，U02做实际prompt发送，
这里不能靠每次模型request新taskId/revision1或裸字符串票据临时顶住。

- taskId由Host明确任务边界管理，模型不能指定。pending canonical同请求复用；
  同意图能力变化使唯一active request修订，旧卡失效；不同scopeIntent不可合并。
- 已commit A，请求新增B时A不撤销；B pending/deny/cancel/bind失败后A仍可执行。
  B未批准前任何B读取/输入均0。只有Host明确新任务/完成/Stop才退休原任务。
- 申请历史与“批准过的范围记忆”分型。pending/denied intent不能写成批准范围。
  记忆只来自最终commit的真实Bound身份，且新agent不会得到可执行grant。
- continuation在真正commit后生成typed ticket，含app/agent/incarnation/connection/
  request/task/grant revision/feature epoch与一次性消费身份。查询Approved不再发票据。
- claim/validation/consume不能只传appSession并返裸字符串；领取和发送前可基于权威
  绑定复验。busy排队的send接U02，U01先提供不会提前丢票/双消费的有限账本接口。
- deny/cancel/expire独立NoAuthority通知，同样限定owner/request，不允许它触发执行。
  feature-on epoch改变、task revision改变、Stop、agent退出后旧票据不能再领取/发送。

## R6. 内存与合同回归，不拿常量声明当上界

- 当前reserve→deny/cancel/expiry会留consumed=false，trim不删；补超过上界的真实循环。
  所有reservation在terminal后退休，pending/正在cleanup的记录不能无主丢弃。
- owners、requests、preparations、decisions、scope memories、retired tokens逐项区分
  per-owner与global上界；retire/reissue不同App session不能绕过global limits。
- 未消费的有效continuation不能因其它请求flood被pop_front静默丢失。
  容量耗尽拒绝新工作或按正确owner收束，不删除仍有效权限/决策假装有界。
- 保留独立Stop槽和rate；真实flood后Stop仍可fence，不被request/status工作堵住。
- Node/Rust/golden严格schema一致；request/status/cancel、unknown fields及伪造身份拒绝。
  不删既有安全测试，不ignored/allow dead code豁免产品，不改依赖/CLI/机器环境。
- 领域实现按职责拆分，避免再新增单个千余行的task_consent大模块；App壳仍不能增长。

## 验证与交付

先对应红→绿和竞争regression，再受影响core/IPC全suite、fmt、strict Clippy、MCP协议/golden。
未改UI不刷UI全套填时间；P0没变不重复跑CLI。实际Cargo/Node退出码必须显式传出，
wrapper0不是inner通过；原首败、snapshot、旧测试报告一律保留。

新增 `2026-10-03-computer-use-task-consent-u01-rework-checkpoint.md`，逐R记录：
实现/验证分开、先红输出位置、命令与真实退出码、source/runtime hash、实际effect/count、
owned资源terminal/join，以及仍留给U02/U03/U04/U05/U06的精确工作。
ADR按最终实现修订，不能写一套已拒绝合同来给错误实现背书。

绝不commit/push/PR/merge/tag/release/reset/restore/clean/stash/rebase；不改共享~/.grok、
凭据/代理/VPN/用户浏览器profile/其它worktree，不自动点真实App/系统/Share批准。
不缩减U00–U09及D00–D31总目标。遇真实外部依赖保留限制并完成其它ready项。
本批交审后等待Codex派U02，不能根据core绿色摘要宣布完整Computer Use交付。

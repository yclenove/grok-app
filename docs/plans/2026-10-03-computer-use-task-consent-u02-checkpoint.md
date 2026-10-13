# U02 Codex Host/ACP 接线检查点

状态：**Host/ACP实现、本批自动化回归及 normal CLI无害闭环通过；整体 active / partial / not releasable**。
用户已要求 Codex 接手实现，单 Codex writer。未启动 Grok、subagent、thread 或 automation；
没有 commit、push、PR、merge、tag 或 release。原 dirty/untracked、旧失败、U01 独立 snapshot 保留。

基线 HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`；分支 `feat/computer-use-implementation`。
已含 upstream main `6f931681d90336dd5e55a24e423c134e20f90dbb` / v0.2.38，本批没有拉取或合并其它分支。

## 实现及准确边界

1. 同一 Broker 的 request endpoint 和 loopback IPC 可先于执行资源建立。App local native
   new/load/fork 路径保留精确 transport/owner，generic/aux/SSH/WSL/TCP 不因存在 child 或
   localhost 地址而获得 CU credential。失败给 redacted 诊断，没有 PATH Node fallback 或浏览器下载。
2. CU 专用 CLI permission handler 在 foreground/background 的 replay/YOLO 通用分支之前处理。
   只认原 raw metadata 的 `use_tool`、UseTool、登记 server 和具体 MODEL_TOOLS；只回实际 once
   option，缺 option cancelled。申请工具只进无权 gate；执行检查当前 Broker grant/ref/geometry。
   不改其它工具策略、共享 Grok home、通用 permission cache 或 CLI always-approve。
3. Host dispatcher 精确投递原 App/agent/process/transport/turn，检查真正 prompt RPC lease及
   permission/plan/ask/history/catalog/cleanup gates。parked owner 转 background，不抢 focus。
   在实际 stdin write 准入前才消费 typed continuation/notice；发送前失败可在仍有效时重试，
   已准入后错误是 unknown，不重复 prompt。内部消息不重发用户原话、不创建新聊天。
4. Stop fence 原 ledger 和 task consent，旧 notice 不自动重启停止的聊天。到期 watcher 属于进程，
   weak Broker、每轮原 blocking sweep/cleanup join 后才下一轮，不依赖模型 polling 或 UI 挂载。
   dispatcher、watcher、首用批准和显式偏好 worker 在退出路径停止并 join 原句柄。
5. ACP prompt lease覆盖实际 RPC future；完成回执带 Host turn。旧/未知 RPC结果不靠 stopReason
   猜当前 turn。foreground/background 和用户发送结果均按原 turn 结算。
   未盖章的历史 notification 歧义仍须 U06 继续处理，不能据 RPC 测试称所有迟到事件已解决。
6. `{session}/computer-use-host-turns.json` 只记录 agent + SHA256，无原文/credential/执行权。
   CLI journal reconciliation过滤精确 Host prompt，保留随后 assistant 历史。4096条/1MiB有界；
   实际 read 和新序列化 bytes 都限容量，失败不覆盖旧指纹。使用已有 store lock，不递归锁。
7. 首用批准为 typed reservation → preference persist → publish feature → execution prepare →
   atomic final commit。准备不是 grant，不发 continuation。取消 fence保留原 preparation归属，
   late open仍由原 worker收尾。真实 persistence失败不启动 execution runtime。
8. 偏好事务直接 patch CU 字段，用无执行权的 `_computerUsePreferenceOwner` 对账。
   普通 settings保存及迁移保留仍匹配的 owner；rollback不恢复旧整个 settings，不覆盖其它偏好。
   显式关闭先 fence，不等待更早的 store writer；原 writer即使命令 waiter取消也继续受注册表拥有。
   后来的明确启用接管偏好，旧 preparation不能撤销新选择。epoch只前进。
9. 新并发反例先红：A首用尚在准备，B已 commit，A abort原先关闭进程 feature，撤销B。
   finalize现在接管本 epoch的 feature归属；A只能回滚自己的执行准备。App只在仍关闭且持有
   自己偏好 receipt时回滚。B的 preference、grant和 continuation保持有效。

以上支持 Host事务合同，**尚无 U04 production Bound 或 U03真实聊天卡**。
目前 `start_preparation/finalize` 仍要求 synthetic Bound，`bind_granted_target`仍为 desktop。
没有删除 synthetic谓词或 cross-surface gate来假称四 surface/生产批准已支持。

## 冻结的 Host入口

- Core `broker::AccessApprovalReady / AccessApproval / AccessApprovalCancel`：opaque reservation，
  不可模型序列化。publish/prepare/commit有明确顺序；取消保留原准备归属，Drop abort并交接原 cleanup。
- Tauri `computer_use_task_approve(sessionId, requestId, revision)` → SessionManager原 MCP endpoint →
  AcpClient原登记 transport → Broker reservation → `computer_use::task_access::approve`。
  token不来自 UI参数，不按 focus/CLI id猜 owner。返回 request/revision/run/decisionClass，无 bearer。
  production Bound缺失时准确拒绝，不能把这个命令的注册写成用户完整工作流通过。
- U03应在真实卡片手势和U04范围绑定后调用该命令；deny/cancel及卡片状态事件在后续批接线。
- `computer_use::owned_tasks::OwnedTasks`：32个原句柄有界，取消 waiter后仍可真正 join。
  只有 abandoned且实际完成的 task可 reap，不能抢走仍活着的 waiter的结果；shutdown fence并 join。

## 首用事务候选与证据

证据根 `tools/computer-use-probe/.run/codex-u01r2-20261003/`。
所有失败及先前候选留在原路径，没有改写旧U01独立算法或结果。

源清单范围 `src-tauri/computer-use-core/src`、`src-tauri/src`、`tools/computer-use-mcp`，
`git ls-files --cached --others --exclude-standard` 排序去重为 **635文件**。
每行 `<lowercase SHA256>  <repository relative path>`、LF结尾后的清单 SHA256：
`18e4788b9bebda947a82a6ab3c23f2fe12bc767a1a7bf8c70b863f7c8caae80c`。
这是上述首用事务候选的源码子集清单。后续probe新增源码后的候选另列，不冒充全部 frontend/runtime/packaging候选。

| 检查 | 结果/证据 |
| --- | --- |
| 核心全部 unit/integration | `u02-core-full-approval-1.txt`；Cargo exit0，660unit+220integration=880pass，0failed/ignored；doc0不计成功 |
| App all-targets strict Clippy | `u02-app-clippy-approval-1.txt`；offline `-D warnings` exit0 |
| Core all-targets/test-support strict Clippy | `u02-core-clippy-approval-1.txt`；offline `-D warnings` exit0 |
| App默认生产features | `u02-app-production-check-approval-1.txt`；`cargo check --lib --offline` exit0；不把probe features当生产构建 |
| App最终候选 compile | `u02-app-build-approval-4.jsonl/.stderr.txt`；exit0；从该invocation取精确executable，按已有CI用SDK mt嵌入测试manifest |
| App最终候选 CU回归 | `u02-app-cu-tests-approval-4.txt`；exit0，173pass，0failed/ignored |
| App ACP回归 | `u02-app-approval-4-acp_client.txt`；exit0，130pass，0failed/ignored；明确skip真实live_handshake，未冒称normal CLI |
| App会话路由 | `u02-app-approval-4-session_manager-routing_tests.txt`；exit0，38pass |
| App CLI journal | `u02-app-approval-4-cli_sessions-tests.txt`；exit0，35pass |
| Settings迁移/保存 | `u02-app-approval-4-store-tests-load_settings.txt`及`...save_settings.txt`；各1pass、exit0 |
| MCP protocol/drag | `u02-mcp-approval-1.txt`；Node exit0，37pass，0failed/skipped |
| fmt / diff / quality | fmt及diff exit0；`u02-quality-approval-1.txt` RESULT PASS；App/AppWorkbench无新状态或增长 |

manifest后的当前App harness SHA256：
`2a9bf535fd9a5bcb38c58167ba3fa65fa211825ca33143725479e3a05262bb27`。
只用 `u02-app-build-approval-4.jsonl`登记的exe，不glob旧binary。
各filter有重叠，不能把上面App项相加当不重复的全App测试总量；未运行全App全部2105项。

先前候选：build-delivery-5 CU161pass、routing38pass、CLI journal35pass；build-approval-2的
首用事务5pass。后续source已变，旧候选不替代最终候选验证。
误用 `acp_client::tests`得到0项，明确不计验证；后续按实际模块运行。

保留首败：`u02-core-concurrent-first-use-red.txt` 的1条原反例实际失败；最终核心全量含修复后的
同条反例。`u02-app-build-approval-1`因测试对非Debug的CommitOutcome使用unwrap_err而编译失败，
已改用err检查真实取消；没有删测试或放宽断言。此前所有delivery/check构建失败也保留。

所有scope批准和本批App事务均为明确 synthetic/FakeAdapter/TCP wire fixture，
**不是真人、normal Grok CLI、原生 App或安装版验收**。
fixture使用精确临时目录、原阻塞/取消worker和原join；未改用户权限、home、proxy或browser profile。

## Normal CLI联合验证

新增 feature-gated `acp_client/computer_use/normal_cli.rs`及`normal_cli/events.rs`，
`cu_probe normal-cli-task-consent`显式入口；固定无害工具为
`tools/computer-use-probe/consent-fixture.mjs`。没有用忽略测试/环境未开即返回成功代替实跑。
沿用已有隔离CLI home，App home及workspace在每次新建的owned `.run`目录中；
不读取/打印/复制凭据，不调用会同步共享auth的`initialize_and_auth`。
实际调用 initialize / cached_token authenticate / App session/new / MCP整表更新 /
production CU permission handler / 真正 prompt RPC / typed decision stdin admission。

最终 `u02-normal-cli-probe-build-7.jsonl/.stderr.txt`构建exit0，按该invocation唯一
`cu_probe`artifact嵌入已有Windows manifest；执行binary SHA256：
`6ed585e577c2ceada22d9a14421a061b38993d232f687eb29c692b2ba608dfc9`。
probe最终源清单范围为上文三项加`src-tauri/cu-probe`和固定`consent-fixture.mjs`，
640文件；同一排序/每行hash路径/LF算法得到
`0b1c52a355615b8968f8ee147208f0dd35de7787eff7772224d218a3161664c0`。
这是U04开始前的probe候选，不是全部产品/安装资产清单。
最终运行`u02-normal-cli-attempt4.jsonl/.stderr.txt`，独立数据目录
`tools/computer-use-probe/.run/codex-u02-cli-20261003-attempt4/`，实际exit0。
当前CLI仍为`grok 1.0.46 (2765805b9442)`，正常default/ask权限、grok-4.7、low，
没有always-approve；leader/subagents/memory关闭，不启动真人桌面/browser执行资源。

实际oracle：普通fixture两次调用 → 两次普通once确认、精确sequence1/2效果；
未批准observe → reject-once、零读取；另一个明确用户测试turn申请 → 一条pending，
零grant/读取；synthetic Host staged commit → 一份grant/一张continuation；
实际typed续行 → 两次fresh observe、零actions；旧claim不可再领取/validate，
对原delivery再次走实际admission得到NotStarted，读取仍为两次、无第二个prompt。
`continuation_count`计的是包含已消费票据的记录数，正确值1；不是pending计数。

原CLI通过EOF退出0且原child.wait已join，没有tree kill。原watcher及Host owned workers已join；
cleanup使用原Broker/tickets。普通fixture与CLI原PID消失另经只读回查。
`u02-normal-cli-probe-clippy-7.txt` strict Clippy exit0；默认生产features
`u02-normal-cli-production-check-5.txt`exit0；MCP37项在`u02-normal-cli-mcp-regression-4.txt`
全部通过，0failed/skipped；fixture语法/非法工具smoke及quality PASS。

保留探针失败：check-1私有helper/方法名编译错误，clippy-2默认初始化规范错误；
build-4仅增加诊断、未跑CLI。attempt1错用事件外层None标记，修正为实际raw.sessionId；
attempt2真实reject-once结束该prompt，拆成独立负向turn及正向申请turn；
attempt3实际闭环成功但错误把历史记录计数当pending计数，最终增加不可validate/重复NotStarted证明。
这些原日志未覆盖，没有放宽request/observe/普通确认/实际效果数量断言。
首次manifest命令路径错误及误用不存在的MCP测试文件也保留工具记录；均不算通过。

这是 **normal CLI + production ACP/MCP接线、synthetic Host决定/FakeAdapter效果**。
普通confirmation由严格无害probe policy返回，不是UI真人确认；批准未走生产范围resolver/
App首用偏好事务/完整SessionManager dispatcher。不能据此宣布真人App/原生/安装版通过。

## 继续顺序与未验收项

1. 本批自动化结果已收齐，未据此冒称真人或CLI闭环。继续保留原失败与最终候选来源。
2. 当前版本/已有隔离home的normal CLI无害闭环已通过上述精确边界。
   production Bound、真人卡片手势、完整App dispatcher与首用事务的联合实机验收仍待U04/U03。
3. U04：真正Host范围binder、四surface membership、多目标切换、增量/缩小和旧owner收尾。
   runtime应按所选surface准备并传递真实失败，不能继续把全套ensure_host_runtime的日志当成功。
4. U03：15locales聊天卡、picker和本事务真实手势接线，完成第一次完整App流程。
5. U05 → U06 → U07 → U08 → U09及D00–D31完整范围继续；原生/真人、stock Wayland、
   全平台最终候选、UX、签名/安装/长时验收仍保留。单项fixture绿不能结束总goal。

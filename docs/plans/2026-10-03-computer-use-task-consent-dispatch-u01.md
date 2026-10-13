# Grok 下一批：U01 核心请求、任务授意与稳定 IPC

你是唯一产品 writer，Codex 继续审查。沿用当前工作区/分支/session，不restore-code。
这批是小范围纵向实现，不一次完成全部UI/平台/原计划。
先全文读：

- `docs/plans/2026-10-03-computer-use-task-consent-u00-rework-codex-review.md`
- 修订ADR `docs/plans/2026-10-02-computer-use-task-consent-adr.md`
- 总体U计划的U01及S01/S02/S05–S08/S21–S24/S28/S33
- AGENTS.md、CONTRIBUTING.md及与本批相关的computer-use规则；不重读几天历史checkpoint。

## P0：先关闭已证的oracle误通过，再动产品

当前U00-R oracle报告9/9和negative六组均绿，但Codex的独立反例仍能误通过。
运行review指向的`oracle-counterexamples-v2.mjs`，当前预期退出1；这是测试缺口，不是CU产品失败。
保留旧oracle/report/events/negative的原hash快照或使用新唯一root，不能覆盖证据后写成一直正确。

修复oracle：

1. 要求真实正向两条fixture各调用一次，模型prompt结束与日志/effect对应；零调用必须失败。
2. cancel到原CLI实际结束的全区间无新增调用，不能只比较late reply时计数；对齐完整fixture日志。
3. EOF要有原子进程真实terminal exit/close及成功owned收尾，不接受null/timeout/仅写eof-closed。
4. 补上述三个负向，保留已有六个负向；缺事件、不匹配RPC或失败结果算失败。
5. 正向用保留的真实wire/fixture日志重放。若纯oracle变更无需再启动CLI；若采集逻辑改变则
   新attempt实际重跑受影响fixture，不能偷用旧执行报告冒充新采集。
6. 报表aggregate和进程退出码反映真实结果，synthetic注明非真人/生产授权。

P0修复实际验证通过后才进入下列产品阶段。不能只改Codex独立审查脚本绕过反例。
如果oracle新增必要事件使原日志不可重放，说明实际缺证据并补一条精确无害探针。

## P1：冻结本批类型和权威状态

按review五条合同细化ADR，在修订说明写明新修正，不扩写长篇历史。
任务权威状态放Host core一处，复用SessionGrants/Broker的真实生命周期；IPC和命令只路由，
不要在多个模块复制grant/feature bool/当前owner并靠手工同步。

实现严格 `AccessAction` tagged union request/status/cancel、请求状态、HostTaskId、owner/incarnation/
connection generation、FeatureEpoch、TaskGrant、Bound Host scope、ApproveReservation、
execution binding及分型的one-shot decision/continuation账本。

- request只接受purpose/surfaces/capabilities/scopeIntent，受长度、计数、enum和unknown field限制。
- 四个surface合同复用kebab-case；scopeIntent是不可信展示/去重信息，不能构造执行身份。
- status/cancel不要求重复purpose，也不接受模型task/run/request/id或额外授权字段。
- 请求TTL10分钟、空闲grant TTL30分钟，用可测试的单一monotonic clock；不真实sleep十分钟。
- 同canonical intent复用，不同Chrome/Excel intent不合并；决定/revision/任务退休有界。
- `Pending` scope不产生approved/execution/continuation；测试构造的Bound明确synthetic，仅测试Host helper。
- 同一App session内的范围记忆是非执行事实，新agent不继承执行权限。

## P2：第一条纵向路径先红后绿

先加一条有意义的集成/core失败测试：没有run/target、feature-off、本地有效Host owner的
申请经实际IPC/tools路由应快速返回pending；现有实现会拒绝且没有这入口。保留原红输出。
编译失败不冒充行为断言失败。然后以最小改动实现：

1. Host-only登记当前owner并签发稳定SessionTransportCredential；token只进Host/MCP环境，
   不进入模型结果/Debug/trace。重复目录构造同owner同generation取得同token。
2. request/status/cancel走严格无权路径；不调用owner run/authorized_target，不启动adapter/helper/
   browser worker，不读取屏幕/DOM/UIA/AX/窗口标题/tab/剪贴板。
3. 所有执行工具通过transport查Host当前grant/execution binding；没有grant时严格拒绝。
   拒绝错误不附其它会话或范围外的敏感元数据。旧legacy credential不被重建transport覆盖，
   也不能从新transport路由fallback到legacy来绕grant。
4. Host reserve/prepare/commit先验request revision+feature epoch+owner+Stop；准备取消令牌贯穿。
   合法首用enable和reservation在同一权威状态迁移，迟到旧卡不能先开功能再失败。
5. 同一token经过合法synthetic Host批准后可在Bound scope内取得首次观察，证明稳定连接能工作。
   读需要observe capability，写需要input；act/wait仍严格当前run/target/snapshot/ref/geometry合同。
   status/list/首个observe不要求尚未存在的snapshot，list只返回scope内目标。
6. 暂停保留非执行范围记录，执行身份失效；Stop/feature-off/owner retirement取消request、grant和票据。
   清理依然先fence再物理join，不因无新request而删除旧占用或复用unknown action。

这批不接真实UI批准、不自动开启用户软件CU、不自动点任何系统/Share许可。
实际session预连接、ACP精确CU permission handler和prompt队列留U02；聊天卡留U03；多目标
真实resolver留U04；完整一键resume留U05。U01类型必须能承接它们，不能用TODO返回伪成功。

## P3：必要回归和竞争

测试围绕真实effects/gates，不只序列化等于构造器。至少覆盖以下独立边界：

- 有效未批准transport下所有读/写入口adapter调用0；非法owner/session/agent/generation拒绝。
- 模型伪造task/run/request/revision/epoch、unknown fields、超长/重复/非法enum，零授权效果。
- scope Pending不能commit；observe-only grant不能输入；input-only不能读取内容。
- grant前后同transport token：pending→Host Bound commit→fresh observe；结果无bearer。
- 重复request/双击reserve/commit一次；approved状态查询不再次执行或发票据。
- deny/cancel/TTL到期/Stop后迟到approve无run、无grant、无continuation、无开启feature。
- preparing中Stop与commit竞争；feature关闭与旧approve竞争；新用户epoch不被旧失败rollback关闭。
- 同名不同scopeIntent不合并，变更revision退休旧卡，旧agent token/query/票据均无效。
- request/status flood受界限与速率约束，Stop预算独立，state内存有界。
- pause后的旧snapshot/ref拒绝，scope记录不能独立observe；任务完成/新任务不继承可执行grant。
- existing legacy credential、Stop清理、unknown不重放与隔离测试不退化。

纯core tests可用owned fake adapter与明确synthetic Host决定，不代表真人/跨平台已验证。
Node/Rust/golden相关schema一起更新；如果新增MODEL_TOOLS导致golden旧合同失败，明确改新合同，
不能删安全断言或用ignored/allow dead code豁免。超过现有大模块限制时拆领域模块。

验证顺序：受影响新失败→修复→core/IPC相关回归→必要的core全suite、Rustfmt与strict Clippy，
及受影响MCP协议/golden检查。未改UI不刷全UI suite。不要升级依赖/CLI或整套格式化无关文件。
Windows App libtest如遇0xc0000139，按CI同款mt.exe post-link manifest，未进入断言不算通过；
本批独立core不依赖Tauri原生UI库，先用已有正确target/cache，别重新修机器环境。

## P4：交给Codex验收

新增 `docs/plans/2026-10-03-computer-use-task-consent-u01-checkpoint.md`：

- P0原反例/新正向与全部负向，真实命令/exit/hash及原失败保留。
- changed files，权威状态位置，实际IPC与Host批准路径，新的字段合同。
- 每项implementation/verification分开，先红后绿证据和effect计数，未跑环节明确not_run。
- U01完成/未完成的确切范围；U02/U03后续入口；不把core fixture成功写成可用完整CU。
- status/HEAD、owned资源收尾，原进程终态而非只是没有端口。

完成本批停在审查边界，由Codex核diff后派U02；不要自行进入大范围下一阶段。
不commit/push/PR/merge/tag/release，不reset/restore/clean/stash/rebase/换分支、不子代理。
不碰共享~/.grok、凭据、Cookie/profile、用户代理/VPN/日常进程/其它worktree。
本地开发writer的isolated always-approve不代表产品权限，产品默认关闭及Host-only用户决定保持。

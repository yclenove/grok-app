# U04：原 App / ACP owner 下的批准发布边界

2026-10-04计数更正：本稿test-support1021将driver16重复计入contracts220后又加了一次。
原`core-full-2.txt`实际为785 unit +16 driver +204 contracts =1005。原日志不改写。
后续[managed producer checkpoint](2026-10-04-computer-use-task-consent-u04-managed-producer-checkpoint.md)
按各终态result行重新求和；下列原稿其余证据与历史冻结身份保持。

状态：**shared approval publication implemented / managed task producer pending — not releasable**。
目标仍 active / partial，完整 U00–U09 / D00–D31、四类 surface、三平台及完整 UI/UX 不缩减。
工作区 `H:/aicoding/grok-app-computer-use`，分支 `feat/computer-use-implementation`，
HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`。单 Codex writer，原 dirty/untracked
和 `fix.patch` 保留；没有 commit/push/PR/发布、真实批准、代理/凭据或日常浏览器修改。

接续 [ManagedBrowser 身份基础](2026-10-04-computer-use-task-consent-u04-managed-identity-checkpoint.md)，
先完成 [资源准备方案](2026-10-04-computer-use-task-consent-u04-managed-producer-next.md) 第 3 节
共用批准缺口。**生产 TaskScopeTarget、picker、批准 UI 仍 Desktop-only；没有放开 managed gate。**

## 原问题与实现

原批准先在 Core commit，App 返回后才检查 endpoint；旧 session/process/ACP transport 被替换时，
仅返回 UI error 不足以阻止迟到执行 grant。另一个缺口是 Core finalizer 包含 native scope 查询，
不能直接在 App / ACP authority pin 内调用，否则慢查询会阻塞会话替换和停止。

1. `AccessApproval::prepare_publication()` 在锁外复验真实 native scope，前后检查原 reservation。
   `commit_authority(&mut self)` 只提交权威状态，不查询 native、不做文件工作、不消费 handle。
   未准备、Stop、paused/resuming/switching、重复发布都不能完成 grant；失败 handle 留给原 worker。
2. `commit_for_transport` 还核原 bearer/context/request/task/session，不能把另一个合法 opaque
   approval 交给本任务的 pin。旧 `commit(self)` convenience 保留 native preparation 合同。
3. App `task_access::approve` 必须携带 `ApprovalPublication`。原 owned worker 持有 preference
   写入、native preparation、commit 失败后的 abort/rollback；waiter Drop 只 fence，原 worker 仍 join。
   native 工作在锁外，短 publication 在原 preference owner 内；失败退出 App/ACP 锁后清理。
4. SessionManager 共用 `with_task_publication_endpoint`，精确 pin live/background/parked 原 slot、
   process、agent 和 ACP Arc。ACP `TaskApprovalOwner` 再 pin 原 RequestTransport Arc、alive 和 agent。
   短提交顺序为 preference → 原 session slot → ACP → Broker，不在这些锁内 spawn/HTTP/native/join。
5. 直接 selection 和 picker 两条生产入口都使用该 publication；U05 Continue 复用同一 session
   boundary，去掉重复检查。已有首用/explicit-enable/取消/重试合同保留，没有新逐工具授权框。

本批新增四个源码文件、修改十二个已有源码文件，无前端/CSS/locale 修改。
新回归含 native-query count、缺 prepare、实例 replacement、Stop、重复 commit、foreign token、
live/background/parked、process/agent/ACP/slot replacement、同身份但新 transport Arc、foreign broker
和 manager Drop。Host 决定/native fake 明确标 synthetic，不冒充用户批准或 installed App。

## 最终验证

证据目录 `tools/computer-use-probe/.run/codex-u04-approval-publication-20261004/`。
最终产品源码为 `source-files-2.sha256`；此后仅改文档/ignored runner，没有改产品源码。

| 项目 | 实际结果 |
| --- | --- |
| Core 默认 feature | `core-default-1.txt`：989 = 785 unit + 204 integration，0 failed/ignored/filtered |
| Core test-support | `core-full-2.txt`：1021 = 785 unit + 16 driver + 220 contracts，0 failed/ignored/filtered |
| 默认 App 构建 | `app-build-4.jsonl` build-finished success；artifact features=[] |
| 原 App/ACP publication | `app-publication-3.txt` 5/5 |
| App task approval / recovery | `app-task-access-3.txt` 8/8；`app-task-recovery-3.txt` 4/4 |
| 默认 App native scope | `app-native-scope-3.txt` 2/2，实际 source Node/Chromium → Rust client/Host/adapter |
| 默认 App CU / ACP / routing | attempt3：220 / 158 / 38，各实际 exit0；集合重叠不相加，live_handshake 排除 |
| 全前端 | `frontend-full-1.txt`：701 文件、7999 tests passed |
| Strict Clippy | Core all-targets+test-support、默认 App all-targets、probe all-targets，各 exit0 |
| fmt / type / lint / quality / diff / MCP golden | 各最终检查 exit0，无新豁免 |
| UI build | `build-ui-1.txt` exit0，既有 large-chunk warning 保留 |

App attempt3 原 exec session 78347 已 exit0，七组全部正数 passed/0 failed。
真实 native scope 使用隔离 profile，无 execution grant/真实批准/输入，原 supervisor shutdown/join。
未执行完整 App 全 test suite、installed App、真实 Grok 联合批准、其它平台或长稳验收；不以过滤集合替代。

## 失败记录，不以重跑掩盖

- `core-red-1.txt` 有意义先红：authority pin 内实际 native 查询 1 次，要求 0 次；0 pass/1 fail，
  Cargo101。拆 native preparation 与 borrowed authority-only finalizer 后六条最终 Core 回归转绿。
- `app-build-1.jsonl` 阶段性编译失败：声明测试 module 时文件尚未创建。创建后修复；不是业务先红。
- build2 有未使用 `TaskApprovalOwner` import；显式标注 manager closure owner type 后最终 build4/strict 通过。
- acceptance attempt2 七组实际 harness 均 exit0，但 runner 总 exit1：`0 passed;` 子串误匹配
  `220 passed;`。修为锚定正数通过/0失败的完整 result 行，保留 attempt2 日志；attempt3 整体 exit0。
  此为 runner 缺陷，不记为产品失败，也不拿 attempt2 总体 exit1 冒称总体绿。

## 冻结身份与主动清理

3063 source files；manifest SHA256：
`0ce19eed2a30df876753d1d5d23297ce348d5a14a7e9dbbc5caf85e1d39a91bc`。
清理前后逐项 3063/3063 unchanged。
默认 App harness `app-artifact-3.json` SHA256：
`fdb54be7d01806458cb0aa5d151136a71f7bb1df55db6f9c8ecd9f37e2620b58`。
只嵌 CI 同款 Windows test manifest，不改产品 linker；历史同路径 hash 不代表本构建。
原 immutable seed manifest SHA256 仍为：
`001cf9617ce64b6cb6448c38a848533577a6116f33138bbc242e8e03e873b5ec`。

原 build/test/browser owners 退出后，cleanup 脚本先 inspect 再 Execute，只删明确 old/current pair
的五代旧 incremental session，拒绝 reparse/live owner/代次变化。最新每代全文件、App/Core
executables、seed 前后 hash 不变；未删整个 target/node_modules、源码、包、依赖、历史事故 clone 或证据。
精确目录/前后 hash 见 `cache-cleanup-before.json` / `cache-cleanup.json`。

本批删除逻辑体积 **4,378,034,789 bytes（4.08 GiB）**；H 实际空闲观察增加
**3,232,010,240 bytes（3.01 GiB）**，硬链接不重复计物理释放，与上批清理分开记。
不在回收站，可用 Cargo 重建。每批收尾保留当前可用构建/seed/依赖/证据，清理一次性产物的规则
已在 AGENTS 第 9 条；不 kill 其它程序以强行清理，不删其它工作区或共享缓存。

## 下一步

沿 [managed producer 方案](2026-10-04-computer-use-task-consent-u04-managed-producer-next.md) 第 1、2 节
解决真实 Bound 与原 run owned tab 的先后依赖。共用原子 publication 已完成，不需要再造授权机制；
managed 资源 reservation/preparation/late cleanup 仍未实现，不能借 legacy run 或直接开 surface gate。
随后才接共享 UI 和 15 locale，再继续其它 surface、关联 dialog、增量/缩小、U05 其它恢复、U06–U09、
D00–D31、三平台真实 App/native/normal Grok、全 CI、签名安装/12h 与完整 UI/UX。

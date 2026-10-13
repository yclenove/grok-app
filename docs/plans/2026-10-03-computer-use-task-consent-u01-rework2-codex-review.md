# U01-R2 终态独立审查

结论：**partial / needs_rework，不能派 U02**。产品代码仍由 Grok 实现；本次 Codex
只读审查产品、保留独立证据、编写下一批任务，没有修产品源码。

## 真正终态

batch04-u01-core-rework2 原 PID38388，UTC start `2026-10-02T18:44:00.9471359Z`。
owner 已记录 exitCode **1** / joinedUtc **`2026-10-02T19:55:24.8750738Z`**。
stderr 最后为 `Error: max turns reached`；最后两个完整 stdout 事件为
`max_turns_reached`、`end / stopReason:cancelled / num_turns:180`。
这是批次上限退出，不是用户取消整个目标，也不是完整任务完成。

2026-10-03 10:29 本机复核：原 PID 缺失，同工作区/原 session 无匹配 Grok writer；
其它项目 Grok 不在本任务范围。原 exec11754 已不可用，以 owner/join、最终事件和
实际进程核验确认终态。预期 U01-R2 checkpoint **不存在**。

工作区 `H:/aicoding/grok-app-computer-use`，branch `feat/computer-use-implementation`，
HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`，未提交产品增量保留。

## 已有绿灯与当前编译失败必须分开

[实施中记录](2026-10-03-computer-use-task-consent-u01-rework2-preliminary-review.md)
保留了 Grok 实际先红及阶段绿：五项迁入反例通过、授权32项通过，原 wire `EXIT=0`。
其后 Grok 又添加竞争、容量、长动作/unknown 和通知回归，未留下完整最终验证。
不能把之前的绿外推为当前候选通过。

Codex 对当前实际产品 crate 独立执行：

```text
cwd: src-tauri
cargo test -p grok-computer-use-core --offline --lib --no-run
actual Cargo exit: 101
```

`broker/tests_task_consent_rework2.rs:604` 的断言诊断使用 `{committed:?}`，而
`CommitOutcome` 未实现 Debug，触发 **E0277**。未进入断言；与历史 Windows App
libtest `0xc0000139` 无关。不要为方便诊断盲目给凭据相关类型派生 Debug。

## 保留的独立副本及五个未执行谓词

路径：`tools/computer-use-probe/.run/cu-consent-orchestrated-20261002/`
`batch04-u01-core-rework2/codex-review/`。

已保存 **165 个原 core 源文件**，原源码/pristine 哈希一致。隔离 crate 只追加两处
cfg(test) module seam，独立 raw-clock helper 仅存在于审查测试模块；产品算法未修改。
`review-result.json` 为 Cargo101、`expectedFiveTestsStarted:false`。
五项 **未进入断言，不能称五红或五绿**；同一个产品 E0277 阻止了执行。

1. 原申请9分钟可准备；原时间再推进2分钟、没有 sweep，过期申请必须拒绝准备。
2. 合法预约/publish 后慢准备跨申请 deadline，finalize 必须拒绝。
3. grant/continuation 在29分钟有效；再推进2分钟、没有 sweep，validate/consume 必须拒绝。
4. 两次增量批准产生两个 run，并各真实 FakeAdapter observe；complete_task 必须 fence
   全部原 owned run，消费原 cleanup tickets 后全部 Stopped。
5. 合法 FakeAdapter observe 后推进31分钟；仅一次到期模型调用被拒，无后续成功请求，
   原 run 仍须立即交接 fence/cleanup，不能错误早返而留 Running。

前三项为 authority-only synthetic，后两项为 synthetic Host decision + 真实 FakeAdapter
方法调用；均不是生产 App、真人或系统授权证据。旧 batch03 四个实际行为红亦不覆盖。

| 文件 | SHA256 |
| --- | --- |
| batch04 owner.json | `41f51cd43e045048427b28eec4aa86842647599a158c17bd4987daaa83d79d86c` |
| product-no-run-1.txt | `c16dc083d91d37bf65a1696ecc82df4df8c566f2d6316fb32b4d76da6cdaa7bf` |
| independent-extra-1.txt | `14d8357731b8db7641399e675fa2e3294195ab9aa8c0aec1a695834cac4d14f8` |
| snapshot-manifest.json | `8c64de7104e0efc495031f7437eec343aab3176641da946720b983ee86f06e74` |
| review-result.json | `14ac332972b194158b46ad87470172278806ee6dced6677a86c758f751966321` |
| authority_deadlines.rs | `59587bebac3c1da3ad61cfac67bca5f820640c1c9b2730a262a379a72102026a` |
| broker_extra.rs | `e7397f862566d68d7427608e74d7a65e572c2c5452b18cfbf1a7995671b35817` |

## 最终源码静态关联缺口

以下是静态审查，不冒充已经运行失败的断言：

- `ConsentState::complete_task` 返回一个 run，却删除同 task 的所有 grant；Broker 只
  fence 返回的 run。每 request 独立 run 后必须收束全体及未完成 preparation。
- `submit_access`、`begin_model_call_claiming` 等先 sweep 再 `?`，错误可能跳过
  `handoff_expired`；一次拒绝也必须交接全部本次产生的清理责任。
- 准备/finalize、continuation 与最终 permit/binding 准入须直接核 monotonic deadline，
  不能依赖恰好有人先轮询/sweep；仍需正确区分实际 in-flight 和真空闲。
- expiry 队列满时保留 grant 的分支没有撤销执行权；新
  `collection_caps_keep_grants_and_cleanup_obligations` 甚至期望到期的 `execution_run`
  仍成功。资源归属必须保留，过期执行权必须失效，不能给错误行为写绿断言。
- `busy_run_ids/runtime_retained` 把 `last_unknown` 当永久 busy；需要核实际原动作/
  cleanup 物理终态后 TTL 是否重新计时，不能把业务 unknown 当无限保留理由。
- F1–F4 的所有原能力隔离/真实间隙竞争/容量回收/物理结束合同仍须完整审查，browser
  最终准入亦不可绕过同一 membership/capability/fence。

下一批：[U01-R2 finish dispatch](2026-10-03-computer-use-task-consent-dispatch-u01-rework2-finish.md)。
独立失败快照/旧日志保持原样；下一真正终态使用新的审查目录。接收后按最终 API 更新并
实际派 U02，随后 U04→U03→U05→U06→U07→U08→U09，再继续完整 D00–D31。

# U01-R Codex 终态审查

结论：**partial / needs_rework**。不派 U02；下一批为 U01-R2。
Grok 修好了原审查中的一批问题，当前源码仍有四个独立执行的反例。
Codex 未修改产品源码；新增代码仅为 ignored 独立审查副本和测试工具。

## 批次与已核事实

batch03-u01-core-rework 原 exec15322 已 exit0；owner 记载 join
`2026-10-02T18:36:01.8403225Z`，原 PID7892 已缺失，现场无另一匹配 writer。
HEAD 仍 `8d8b71c262ab222748bca32412eea884d4b892e0`，未提交或发布。
终态 prepare/owner/grant/ticket/consent 五文件 SHA256 与 Grok checkpoint 全等。

Grok 原日志已独立读取：28 项授权回归通过，core lib618通过，其余10组分别
27/33/21/17/24/18/9/20/14/21通过，实际 Cargo EXIT=0；strict Clippy 最终 EXIT=0；
协议 golden 5/5。原四项锁死已修复，原 killed 日志仍保留。
这些是 scoped core/合成 fixture 证据，不是真人 App 授权或完整产品交付。

## 新独立反例与正向控制

目录：`tools/computer-use-probe/.run/cu-consent-orchestrated-20261002/`
`batch03-u01-core-rework/codex-review/`。

终态 164 个 core source 文件在 capture 前后、pristine 和测试副本间 hash 全等。
测试副本只在 `broker/consent.rs` 加载独立测试 module，产品算法原文未改。
原文在 `source-pristine/src`；五个独立测试在 `review_tests.rs`。
首轮测试 module 放在 sibling 时不能访问 private consent 方法，属于 reviewer harness
编译错误；只修正副本的 module 位置后，才进入下面的行为断言。

`run-review.ps1` 实际运行 Cargo 并显式传出退出码：
**1 passed / 4 failed / 0 ignored / 618 filtered，CARGO_EXIT=101**。
正向控制完成真实 FakeAdapter observe；用正确 snapshot/target/ref 的合法 input 请求
验证 observe-only 不能输入，没有用空 JSON 的 schema 错误代替能力验证。
批准全为 synthetic，不接触真实 App、系统权限、用户窗口或账户。

| 对象 | SHA256 |
| --- | --- |
| red-authority-reviewed.txt | 88708f01be2a716c7a0f644860e0d571513203a599333f34732267c59af7f234 |
| snapshot-manifest.json | cc27a814413d24482b5e741abc4a5ba7314caf5fc63a2815205585c27d0a7212 |
| review_tests.rs | cc8d7db73741c3c8b5ae06c7c1913f86176ac4a2b7bf334c1fa0289686cd7bfb |

## 必修问题

### F1 [P1] 不同目标共享 run 使能力串用

`prepare.rs:81` 为增量 A/B 分配同一 task run；GrantRecord 未保存执行目标，
`begin_model_call` 按 tool 能力选旧 A；`recheck_model_call`（consent.rs:241）只与当前
run 比 owner/task/revision/run，未比 request 身份或对应目标。A/B 都是 revision1。

独立测试先完整批准 A 的 observe 并实际读取，随后 FakeAdapter 退休 A 的目标身份，
将新的 B 身份批准为 input-only。再调 computer_observe，实际返回 B 的 targetId，
`isError:false`，adapter 新读取 **1** 次。旧 A 的 observe 权限不能授权读取 B。
这不是 malformed schema 或仅检查函数回值；实际派发与返回目标均已核。

修复必须把目标/范围成员及所需能力带到最终 Broker 准入，完整 permit 不能降成另一
grant 的 run 身份；增量授权还不能销毁 A。U04 负责生产四表面 resolver，这里先建立
不会跨目标借权的 core 合同，不通过删除 observe 工具或禁止增量来躲开反例。

### F2 [P1] 开关已经启用时，Stop 可丢失迟到候选的清理所有权

`owner.rs:269` 根据 `we_enabled` 保留准备记录；功能原本已开时，Stop 会删除尚未
opened_run 的准备。`bind_granted_target` 在最后一次检查后另行 open/mark/note/bind，
`abort_commit` 则靠已被删除的准备取 run/ownership，迟到资源无法被 fence。

独立测试通过实际 private 生产方法表达精确顺序：start/publish/最后检查通过 →
Stop（此时还无 run）→ 原 bind_granted_target 迟到打开候选 → 原 abort_commit。
实际 run 仍是 **Running**，不是 StopRequested/Stopped。grant/输入尚未复活，但原
候选的清理责任已丢失；不能把 grant0 当资源已收束。
已有 before_open hook 在另一检查之前，没有覆盖该间隙；后半也没有核 run 终态。

准备 ownership 必须独立于 feature rollback 的 bool，保留到原候选 terminal/join。
最后发布需同锁复验真实 run/target/stop；finalize Err 也要收束本事务，不能裸 `?` 跳过。

### F3 [P2] 正常顺序聊天在第33个耗尽权限 owner 名额

`owner.rs:32` 将包括 retired tombstone 在内的 owners.len 限制为32，缺少安全回收。
独立测试每次仅一个当前会话，申请后退休且旧ctx确认拒绝。index32签发失败；此时
live_transports0、grants0、preparations0。不是同时打开33个活会话导致的合理背压。

原 `retire_does_not_free_a_global_owner_slot` 的验收目标错误：合同要求实际占用全局
有界以及旧身份不复用，不是 App 进程终身只能服务32个不同聊天。修订该测试，证明
资源终态后可安全回收；未终态占用仍不能删除或借给新 owner。
同时审查 retired_outbox/cleanup_tickets 等新增队列的真实容量和消费者，不能只给
retired_tokens 加 cap 后把同一数据无界存到 outbox。

### F4 [P2] 空闲TTL实际是批准后的固定截止时间

`prepare.rs:292` 只初始化一次 idle_deadline，`grant.rs:301` 按此截止时间回收。
busy 时跳过扫描不等于重新计算空闲时长。

独立测试：批准 → monotonic clock29分钟 → 正常模型 observe，实际 adapter 读取成功 →
再前进2分钟。实际 grant0；它只空闲了2分钟。长期执行中断授权会直接损害用户体验。
需将真实模型工作准入/物理结束接到权威 activity/idle 状态；Stop/退休后的旧 completion
不得延长新任务或恢复旧 grant。pending/status 轮询不能伪造业务活动。

## 接收与保留的边界

原 prepare 提前 Approved、epoch 回退、旧 ctx ABA、None agent、pending 范围记忆、
有效票据被简单 flood 丢弃、同步 Stop 等缺陷已有有意义修复与局部回归。
仍不能把恒返回 Err 的 test-only reserve 当新准备 API 的正反控制；下一批需使用实际
start_preparation，当前 ctx 成功/旧 ctx 拒绝。U02 所需 NoAuthority 和一次发送账本
接口仍按最终合同补齐，不能只有无消费/无 owner 筛选的理由字符串数组。

下一派发见 `2026-10-03-computer-use-task-consent-dispatch-u01-rework2.md`。
旧 batch02 独立七反例、本批新原文副本、首败和 Grok 日志全部保留，不改成绿色。
U02–U09 与 D00–D31 仍未完成，目标保持 active。

# Computer Use：重启隔离与 MCP 动作接入的下一批执行计划

日期：2026-09-20。状态：分段实施中，不是完整验收记录。
最新进度先读 [Host 崩溃恢复检查点](2026-09-21-computer-use-host-process-retirement-checkpoint.md)：
进程 witness/journal v4 与 cleanup-only 查询已实施；真实 Host 五切点及两种
Host/SW 顺序的 15 个场景通过。完整浏览器退出、renderer/升级世界及双 Host
等剩余证据继续按该检查点推进；R4 整体和 R5 未关闭，capability 仍 NONE。
前一批基础见 [2026-09-21 R3 完成退休检查点](2026-09-21-computer-use-completion-retirement-checkpoint.md)：
同进程 authority 派生 proof 与只读 retirement 已关闭 tombstone 容量/TTL 淘汰
后的 cleanup-only 卡死；生产动作 23、缓存恢复 4、观察 34、Node 142、Core 448、
Driver 12 项通过。R3 已关闭；R4 的 App/浏览器重启、renderer/升级世界及后续
R5 仍未关闭，不开放 action capability。
最新 [文档执行身份检查点](2026-09-20-computer-use-document-execution-checkpoint.md)：
生产 SW 的递增 epoch/sequence、显式文档 fence 和恢复时关闭旧 epoch 已接通，
真实文档重注入旧观察被拒绝；当前动作探针 18 项、观察 34 项、Node 120 项。
从该检查点的 document reload/close/升级世界与剩余恢复合同继续。
后续局部实现见 [SW 原文档恢复检查点](2026-09-20-computer-use-worker-recovery-checkpoint.md)。
claim 前 session-memory journal 和同原文档实际 SW 重启恢复已有实现；先读
该检查点的下一项，不重复从零建 journal。实际 SW 在 Wait、claim 前、claim
回复滞留、native 注入前、click 回复滞留的五个切点已过，最终动作探针 17 项。
下述 R1–R5 仍是完整要求，不能据同原文档五行通过把 R2/R4 整体勾选完成。
前置证据：[生产动作通道检查点](2026-09-20-computer-use-production-action-transport-checkpoint.md)。
原完整目标与 [剩余总计划](2026-09-20-computer-use-remaining-execution.md) 继续有效。

## 开工边界

当前 Host v2 队列、JS/SW 派发、固定 DOM 动作及物理完成回执已有实现。
从缺口继续，不再重新做协议或另造 transport。ExistingTab adapter capability
保持 NONE，直到下述 R1–R4 的真实故障门禁通过；不为演示先绕开占用。
保留工作树；不提交/推送/PR；不碰日常浏览器、账号、Cookie 或代理。
测试只用 owner 标记的 App home/profile，Cargo/浏览器/App 串行。

## R1：先证明重启时丢失的是什么

阅读 `ActionCompletions`、`SharedTabs`、`observeDocument`、`actDocument`、
CompletionRegistry、Host grant/pairing 生命周期，补写重启 ADR，区分：

1. App 活着、SW 被终止；document 内脚本可能仍在执行，原 JS Promise 已丢失。
2. App 被终止、SW 活着；旧 scope 请求失效，但 SW 仍拥有原脚本/取消调用。
3. 两者先后重启；新的配对不代表旧文档已经空闲。
4. 完整浏览器退出与仅 SW 重启；前者不能替代后者。
5. 动作尚未注入、正在 Wait、已产生副作用但回复滞留三个时间点。

建立最小真实失败用例，保留现有 Host/扩展进程与页面后置条件。SW 重启必须实际
终止 worker，不能只调用 reset、清 map 或 new 一个假的 controller。不得将
“旧 key 无效”“旧浏览器关闭了”记为原动作已完成。

产出：ADR、首败日志、资源所有权列表、每个故障切点的明确预期。

## R2：文档端退休与恢复占用

选定的实现必须同时做到：

- 新 Share/授权不能越过旧物理操作；只影响明确分享的原 tab/document，
  不枚举或关闭用户的其他标签页。
- 拦住尚未进入文档的旧 observe/act 注入；单纯 retire 当前 snapshot 不够，
  因为迟到的旧 observe 可能再次创建它。需要不可倒退的文档执行身份。
- 正在运行的旧 Wait 要取消并等待原 operation.finished；旧取消回调不能
  退休新的 snapshot。超时或无法证明旧文档销毁时保持不可用，不假装已清理。
- 恢复可以在新配对之前只持有 cleanup 权限，不获得页面读取/新动作权限。
  配对 Bearer 不进入普通持久化；恢复记录不包含输入文本、页面内容或完整命令。
- 如果采用 chrome.storage.session 的有界清理记录，必须在 claim 前成功写入，
  写失败为零领取/零执行。读坏、删除失败、回复丢失和 8 条容量上限均有测试。
  它只跨 SW 重启，不跨完整浏览器重启；不能把它写成磁盘崩溃恢复方案。
- 若采用持久化单调 worker/document epoch，只存非敏感元数据；先明确原子性、
  overflow、旧版迁移、扩展 reload/update 和同 profile 并发实例的语义。
  随机 nonce 本身没有顺序，不能据此判定哪个旧注入应被拒绝。
- 任何本地 busy 记录的删除都必须有可信物理完成/文档销毁证据。App 的全新
  内存 registry、网络 403/404、超过 10 秒、重新授权均不是这类证据。

以上是必须满足的合同，不强行指定未验证的存储设计。先把所选方案写进 ADR，
再按文档执行身份 → 有界恢复记录 → SW 启动恢复 → Host 重启隔离逐块实施。
每块先失败测试再修复；尚未接入的 helper 不得记为产品完成。

## R3：完成 tombstone 淘汰后的清理恢复

复现：实际动作完成，settle 在 Host 成功，但客户端未收到回复；完成 tombstone
经过 64 条容量淘汰或 5 分钟到期，客户端继续重试，导致它永久认为 tab busy。

设计独立的清理终态协议，必须区分“已完成”“此 Host 已不持有该笔占用”和
“认证失败/当前仍有未完成项”。同 requestId 的现存 pending 必须校验完整 proof，
不得用查询不到旧记录来释放另一个 request/tab/instance 的占用。

任何 absence/retired-instance 回复只能让已经证明本地物理结束的 cleanup owner
释放自己的记录；不能证明动作成功、重新授予权限或使 applied/verified 通过。
status/claim/result 不得因这条恢复通道放宽。原始 claim/result 仍不能重试。

通过条件：有界淘汰/实际期限、错误 proof、同 tab 新 owner、新 App instance、
丢失恢复回复均有回归；浏览器无需关闭，旧操作不重放，另一 owner 仍活着。

## R4：实际重启故障矩阵

沿用 `existing-tab-dispatch` 的生产 SW/HTTP，不在可信页替换实现。每个格子
给出旧动作计数、新动作计数、Host 占用、扩展残留、tab 存活的独立证据：

| 故障 | 至少覆盖的时间点 |
| --- | --- |
| 实际终止并重启 SW | claim 前、claim 后未注入、真实 Wait 中、动作完成回包滞留 |
| 实际终止并重启 App | 同上，旧 endpoint 失效与新 instance 均核对 |
| App + SW 先后重启 | 两种顺序，旧 claim 回复迟到，新配对/新 Share 不能绕过旧占用 |
| 原文档 reload/导航/close | 恢复期间发生；同 URL reload 也必须绑定不同 documentId |
| 完成记录淘汰 | 丢失 settle 回复后，容量淘汰和到期分别验证 |

故障不能通过修改本机时间、向共享 App home 写配置或杀全部 Chrome 实现。
需要测试时钟时只注入 registry 自有时钟；实际浏览器矩阵保留真实生命周期。

## R5：ExistingTab adapter 与真正 MCP 动作

R1–R4 通过后再做：

1. adapter.act 把 Broker 已校验的原 observation/ref/generation 转成严格 v2
   command；继承同次 run admission/cancellation，不偷偷补模型漏传的身份。
2. 按实际协商且已有证据的 click/set_value/type_text/scroll/Wait 开 capability。
   key/navigation/contenteditable/页面滚动没有实现时不能虚报支持。
3. 重用实际 session MCP → IPC → Broker → ExistingTab adapter → SW → DOM；
   私有探针只提供 fixture 用户授权，不能直接调用 enqueue 代替 MCP。
4. 独立页面计数/值验证 observe → act → verify → stop；覆盖两 session 抢 tab、
   重复 actionId、Pause/Stop、导航、旧 run 清理晚到和取消期间新 observe。
5. 状态码必须诚实：语义 click 为 applied，验证字段/条件才能 verified；
   synthetic KeyboardEvent 不等于真实键盘默认行为。保持无 Desktop fallback。

通过后再按剩余总计划推进 parity、App/ACP、UI、安装、多平台和真实模型。
本批不得以测试数量或运行了多少小时替代最终验收。

## 执行提示词

继续 `H:\aicoding\grok-app-computer-use` 的 Computer Use 目标。先读 AGENTS.md、
Computer Use wiki、最新生产动作检查点、本文件，以及配对/动作完成/派发 ADR。
确认实际分支、HEAD、脏文件和本机未完成进程，先读 2026-09-21 生命周期检查点。
不要重做已经通过的 Host v2 队列
或生产 SW；R1 和 R2 同原文档/执行身份切片已有证据，从最新文档执行检查点
的文档销毁/升级世界/已接收旧注入继续，再按 R2→R3→R4→R5 关闭其余门禁。

每一批记录首败、修复、严格测试/Clippy、真实后置条件、源码及 probe 指纹，
并明确 fixture 授权、实际 MCP、原生 UI、真实模型的不同证据等级。不能跳过
失败、加无条件 sleep 凑时长、放宽断言、把 unknown 自动重试、把清 map 当完成。
默认功能关闭；重启隔离闭合前动作 capability 保持 NONE。不碰真实账号/日常
浏览器/代理，不 commit/push/PR，不撤销已有工作；保留证据并及时收尾自有资源。

本地有可推进项就继续；遇缺平台/系统权限时写清 not_run/blocked_external，
完成其他独立工作。总状态保持 partial — not releasable，直至完整平台、安装、
真实模型和同一候选至少 12h 主动长稳都有证据。不要声称“跑满两天”就完成了。

# ADR：完成墓碑淘汰后的只读退休证明

日期：2026-09-21（America/Los_Angeles）。
状态：R3 已实施；R4 App/浏览器重启仍未完成，ExistingTab action capability 保持 NONE。

## 问题

Host 接受 `settle` 后会释放 pending completion，并保留最多 64 条、5 分钟的
finished tombstone。扩展可能在 Host 已接受请求后丢失 HTTP 回复。旧客户端只会
重试 `settle`；一旦对应 tombstone 因容量或 TTL 被淘汰，Host 无法再从 registry
确认该 proof，扩展的 `physicallySettled` journal 会永久占用本地槽位。

不能把以下事实当作完成证明：配对失效、新 endpoint 可连接、Host 返回 404/403、
本地超时、找不到 tombstone、App 或浏览器已重启。它们均不证明原浏览器动作的
物理 Promise 已结束。

## 决策

### 1. completion key 由进程 authority 派生

每个 App Host 进程创建随机 authority。completion proof 的 key 是对完整、不可变
binding 的 HMAC-SHA256；binding 包含 request、connection、session/run、tab、
document/grant generation 和 snapshot。Host 仍只保存内部 authentication tag，
不会把 authority 或 pairing Bearer 交给扩展。

因此，同一 App 进程即使已淘汰 tombstone，也能无状态认证旧 proof；新 App 进程
拥有不同 authority，旧 proof 必须失败。该设计只恢复 cleanup 身份，不恢复配对、
授权、动作参数或页面访问能力。

### 2. 新增只读 retirement 查询

`POST /cu/extension-completion/retirement` 使用与 completion status/settle 相同的
严格 extension Origin、loopback、body 上限和 proof 解析，不接受 pairing Bearer。
它在查表前先用当前进程 authority 认证 proof，然后只返回：

- `pending`：Host 仍持有该 completion；查询不得 claim、settle 或释放它。
- `settled`：对应 finished tombstone 仍存在。
- `absent`：当前进程已认证 proof，但 registry 不再持有 pending/tombstone。

错误 proof、旧进程 proof、错 binding、额外字段和错误 Origin 均拒绝。`absent`
不是匿名的“不存在”，也不是业务成功回执。

### 3. 客户端只在本地物理完成后使用 fallback

`CompletionScope` 仍先执行唯一有权释放 Host pending 的 `settle`。只有 journal 已
成功持久化 `physicallySettled`，且该 `settle` 失败时，才查询 retirement。客户端
只接受严格的 `settled` 或 `absent`，随后删除自己的 journal；`pending`、403、
畸形/超大回复、网络错误或本地删除失败都保留原 owner 并继续 cleanup-only 重试。

没有 journal 的历史/注入 scope 不允许使用 retirement fallback。该路径不重发
claim、浏览器动作或业务结果。

### 4. 业务结果与 tombstone 淘汰解耦

正向业务结果仍必须匹配 Host 中原始 delivered action、完整 request/proof，并且
该 action 已 claim。Host 用 retirement 验证 completion：`pending` 拒绝
`applied/verified`；`settled/absent` 才允许提交一次结果。因 action pending 本身仍
存在，未 claim、已取消、过期、换文档或重复结果都不会借 `absent` 绕过准入。

负向结果不会借 retirement 释放 claimed physical receipt；原有 cleanup owner 仍
负责收束。retirement endpoint 本身永远不发布业务结果。

## 被否决的方案

- 无限保存 tombstone：隐藏而不是解决有界内存和 App 生命周期问题。
- tombstone 缺失即清理：无法区分真实 settle 与新 App/伪造 proof。
- 重试业务结果或浏览器动作：会制造重复副作用。
- 把 pairing key 写入 journal：扩大凭据生命周期和可用权限，且仍不证明物理完成。
- 让 retirement 自动 settle `pending`：会越过客户端的本地物理完成门禁。

## 安全与剩余边界

容量淘汰和 TTL 到期现在可在同一进程内安全结束 cleanup-only journal。新 App
实例对旧 proof 返回拒绝，扩展必须保留记录，等待 R4 的明确跨进程所有权协议。
这项决策不解决实际 App crash/new endpoint、App→SW 或 SW→App 重启顺序、完整
浏览器退出导致 session journal 丢失、renderer crash、扩展 reload/update 的旧
isolated world，也不开放 adapter/MCP actions。

实现与验证证据见
[R3 检查点](2026-09-21-computer-use-completion-retirement-checkpoint.md)。

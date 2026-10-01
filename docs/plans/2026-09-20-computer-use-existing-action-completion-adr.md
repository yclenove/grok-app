# ADR：ExistingTab 的领取与物理完成回执

日期：2026-09-20。状态：实现中；未开放生产动作能力。

Host registry 与 HTTP 部分已实现，测试及余项见
[本批检查点](2026-09-20-computer-use-existing-action-completion-checkpoint.md)。
扩展执行链与 App/SW 重启证明仍未闭合。

后续扩展清理对象、动作所有权控制器与实际 HTTP/DOM 收束证据见
[回执客户端检查点](2026-09-20-computer-use-extension-completion-client-checkpoint.md)。
控制器随后已接到生产轮询，见 [生产 SW 检查点](2026-09-20-computer-use-production-action-transport-checkpoint.md)。
重启证明仍未完成；不得据此开启动作能力。

## 决定

动作必须先 offer，再由扩展显式 claim，最后 settle。Observe-only v1 的消息
不能被视为动作领取。未领取的 offer 在超时/撤销后可以删除，因为扩展没有
执行权；成功 claim 的动作即使超时/取消/配对失效仍占用原 tab，直到可信扩展
确认原 executeScript 和取消调用都已经返回。receipt 不是再次派发操作的许可。

每个 offer 使用 256 位 CSPRNG completion key，绑定 request UUID、App/connection
身份、session/run/tab/document/grant/snapshot。Host 仅保留此 key 对完整身份的
HMAC，不保留原始 key。调试输出必须遮蔽 key；它不进入模型、UI、日志、URL 或磁盘。
领取需要当前配对的 Bearer + 全部身份 + completion proof，并在 Host 同一锁内
复验当前授权及取消/期限。重复领取拒绝；领取响应未知时扩展不得执行。

status/settle 可在旧 pairing key 已删除、feature 已关闭后凭独立 completion
proof 访问，但只返回该笔取消/领取/完成状态，或单向归还该笔占用。它不能观察
页面、取得新的动作、分享、授权、恢复、续配对租约或提交模型动作结果。来源
仍需精确安装扩展 Origin、loopback Host、无 forwarded headers；无通用旧凭据
兼容路径。当前可用授权之外的模型结果始终丢弃。

settle 幂等；完成记录只保留有界 HMAC/身份 tombstone，过期或被有界缓存淘汰
后拒绝旧回执，不恢复任何动作。尚未完成的 claimed 项不得按时间淘汰，达到
上限后拒绝新动作。新 owner、同 tab 重分享/重授权不能越过旧占用；别的 tab
可继续工作。用户 tab 不能被关闭作为清理方法。

信任边界仍是已配对的可信扩展实现，无法防御恶意修改的扩展/本机系统。
completion proof 只证明回执来源，真正物理完成必须由扩展的 Promise 所有权
及真实故障测试证明；一个未经校验的 `physicallySettled:true` 不构成合同。

## 实施顺序与剩余门禁

1. Host 的有界 registry、claim/取消/settle 合同与确定性竞争测试；接到现有
   pairing 撤销、tab 归还、is_idle 以及重新授权拒绝逻辑。
2. strict typed HTTP/JS 协议及 origin/权限/大小/重放负例；v1 只观察，动作
   版本与能力显式协商，不靠收到某个旧 poll 推断新能力。
3. transport 始终拥有 original script、取消 watcher 和 completion scope，
   不能在 reset/换配对时直接丢弃晚到回调。业务结果不能重放，清理回执可重送。
4. App 崩溃/重启及 SW 重启的旧占用恢复/原文档退休证明必须单独设计和测试。
   仅内存 registry 不证明重启安全；这项关闭前不宣告完整生命周期、不开启
   默认能力，也不得将旧 claimed 状态静默解释为空闲。
5. 真实 MCP → Host → MV3 → 原始页面副作用及 Pause/Stop/替换配对矩阵。

Host registry 单测或 HTTP 回执通过不能替代第 3–5 项。原始完整目标与跨平台、
安装、真实模型、12h 主动长稳要求不变。

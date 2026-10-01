# ADR：ExistingTab typed action 派发

状态：Host/Broker/HTTP、JS 客户端和正式 SW 均已接线；实际 v2 动作与故障
检查 11 项通过。动作能力仍 NONE，不代表 C3 或重启安全已完成。最新验证见
[生产 SW 检查点](2026-09-20-computer-use-production-action-transport-checkpoint.md)，
上一批 Host 实现见 [派发检查点](2026-09-20-computer-use-existing-action-dispatch-checkpoint.md)。

保持 v1 Observe-only；动作使用独立 v2 negotiation/poll/claim/result。
握手携带完整 connection、protocol=2、completionProtocol=2 和严格动作列表。
同连接协商结果不可变；重新配对清除协商。v1 poll 永不返回动作。

Host 在同一授权锁内校验 session/run/tab、原 grant/document generation、
原 snapshot 及其可用 elementRef，再预留容量并入队。预留不生成 completion
key；第一次 v2 poll 才生成 proof，Host 只保存 HMAC，响应未知不重送。
单条动作只占一个容量；结果等待和物理占用分别持有，取两者并集计数。

v2 claim 提交完整严格类型 request + proof，并与 Host 保存的不可变 request
逐字段比较。这包括 sequence/deadline、documentId 和全部 command/parameters，
不依赖跨语言 JSON 序列化得到相同 hash。registry 对动作条目标记 bound claim，
旧 generic claim 即使持有正确 proof 也不能领取该动作。

业务结果使用完整 request + proof + 有界状态，当前 Bearer、原授权与期限仍须
有效。applied/verified 必须先领取并已有效 settle；rejected/unknown 可结束业务
等待，但不得替代原脚本的物理完成，claimed registry 继续占用。结果只消费一次。
Stop/超时撤销业务等待；旧清理 scope 仍可归还物理占用，不能发布新结果。

正文上限：v2 request/claim/result 64 KiB。最坏 4000 个文本码点即使 JSON
转义也有界；旧配对回复仍保留 8 KiB，上调只能用于 v2 action poll。
字段禁止 null/unknown；不接收任意脚本或通用参数 Value。日志不输出 proof、
用户输入或原始 serde 错误。

接线顺序：Host 合同与竞争测试 → Broker/HTTP → JS transport/SW 和真实页面
后置条件 → App/SW 重启隔离 → adapter 能力。完整跨平台/安装/模型/长稳要求不变。

配对、续租与撤销分别使用固定有界限流桶，预检及 POST 保持同桶；快速配对
不得消耗 unoffer/disconnect 的额度。动作与观察轮询、独立完成回执继续使用
各自既有的有界额度，未放宽权限/身份检查。

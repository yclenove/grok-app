# Codex 对 U00 的审查

结论：**needs_rework**。CLI wire 的部分事实已独立核验；当前 ADR 不作为 U01 已冻结协议。
本批 Grok 原 writer PID37260 已真实退出，原 exec45195 返回0，owner 有 exact exit0/join。
没有产品源码 diff，没有提交。下一任务是具名 U00-R，不重跑旧 GNOME 或全部质量 suite。

依据：[ADR](2026-10-02-computer-use-task-consent-adr.md)、
[checkpoint](2026-10-02-computer-use-task-consent-u00-checkpoint.md)、实际两轮 probe 源码/
report/原 wire/fixture 日志，以及现有 IPC、MCP server 和 App permission 代码。

## 已接收的有限事实

- normal/default CLI1.0.46 无 run 时可 session/new 装入 owned 固定字符串 MCP。
- 同一 agent session 两次 prompt 完成；重叠 prompt 也被排队执行，Host 必须自己防双发。
- CLI 这次 busy update_mcp_servers 返回成功；这不等于 App 应删除自己的 busy 围栏。
- session/cancel 后迟到 synthetic allow-once 没有新增 fixture 调用，EOF pending 后也没有新增。
  第二探针真实固定工具调用计数2，CLI75644 exit0，原 fixture28684/32588已缺失。
- U00 的20秒延迟是 CLI generic permission，不是 App task pending/90秒真人批准。
- synthetic 决策不是用户授权，没有观察或输入用户电脑；原日志未覆盖。

第一探针 configured CLI67024 exit1 的来源已查清：外层 writer 原日志有工具调用
call-08e8209c-e63c-43b3-bf00-b9aee69da3b7-65，显式 taskkill /PID67024 /T /F。
实际执行已晚于首个20秒回执；不要用命令描述“before always-allow reply”冒充真实时序。
因此它是人为结束的 probe，不能归因产品 CLI 崩溃，也不能把第二次成功说成修复首败。
Codex 的 first-probe 早期记录将原因暂列 unknown，以上新证据覆盖该原因判定。

## R1：CU 的 generic CLI permission 仍会重复

ADR 第4.6/5节只说 CU 不进入 SessionAllowCache，不选 always-allow，然后直接宣称只有
一张 Host 卡。真实 MCP 的 CU 工具仍经 use_tool 调用，也会发生 generic permission。
没有 CU-only 的 Host 判断/allow-once 回执机制，这个结论不成立。

必须明确：如何基于真实 ACP 结构识别当前 Host 注册的 CU server/tool；bootstrap 无权
申请如何通过 generic gate；执行例程只有当前明确 grant/执行绑定允许时才回复 allow-once；
其余 MCP/shell/file 保持原流程。不能用 title 文本、use_tool:*、用户可伪装的 server 名、
默认 always-approve 或允许全部 MCP 代替。Stop/revoke 与 generic 回执竞争仍由最终 core gate拒绝。
现有两 probe 未保留必要 rawInput/_meta 结构，补一条 harmless fixture 的真实脱敏消息证据。

## R2：固定 RequestCredential 和后续执行没有通路

当前 server.mjs 所有工具使用同一个启动时 GROK_APP_CU_TOKEN。ADR 指定稳定目录使用
RequestCredential，且该凭据只允许 computer_request_access；执行 bearer 放在另一张表。
批准后现有 MCP 进程如何获得执行资格没有说明。不能一边令 request token 永远只能申请，
一边要求同一固定进程执行所有工具，也不能把执行 token 放进模型可见 tool result。

优先设计：稳定的 session transport credential 绑定 App session + agent incarnation +
connection generation；它本身不授予电脑权限。Host 路由每个执行调用时查自己的当前
TaskGrant/ExecutionBinding，再严格校验模型传来的 run/target/snapshot/ref 身份。
未批准/暂停/撤权时同一连接的执行调用始终拒绝；批准后由 Host 注册短期绑定，并非 token 自升权。
可保留 legacy execution credential，但新稳定入口必须有清晰路由、撤销和测试合同。
若选择其它方案，必须给实际连接/目录/Token 流程和时序，达到相同 UX，不留待 U02 猜测。

## R3：绑定和 feature-off 原子决定不足

ADR 的 RequestCredential 只有 session_id/token，TaskGrant 也没写 agent/generation 的资格。
补 credential、request、active execution grant、continuation 的完整 owner 绑定，以及 ACP退出/
重连/换模型时旧凭据的退休。非执行 scope 记忆可以留在同 App session，不能自动作为新 agent 的 grant。

“先 set_feature_enabled(true)，再 commit”会留下迟到按钮把功能重新打开的窗口。
Host 决定需要 request revision + feature epoch + 当前 agent/session 的复验/原子 reservation，
在进入准备前确认决定仍有效。用户已关闭/取消的旧卡不能先开功能再发现 commit 失败。
批准准备期间 Stop 继续 fence；失败与回滚不能误关闭另一份用户明确打开的功能状态。

## R4：schema/去重/范围要可执行

- 标明 request、status、cancel 为 tagged union；status/cancel 不必重复填 purpose/surfaces，
  不能用模型提交的 task/run/request ID创造授权。查询只属于有效当前 session incarnation。
- 公共 surface 值复用当前 kebab-case：desktop、managed-browser、existing-tabs、app-webview；
  不另造 underscore 名称然后让 TS/JS/Rust 三套接口各猜。
- 同 surfaces/capabilities 不能说明同范围。Chrome→Excel 或另一个实例可能两者 enum 都一样。
  去重至少绑定 Host task identity、canonical scope intent/能力/必要目的、request来源generation；
  不同意图不能错误合并，改变范围使旧卡 revision失效，已批准范围外请求走一次增量。
- Host-approved scope 要是可验证的类型/opaque identity，不能直接把模型的 scopeIntent 字符串
  当执行 whitelist。UI/原生解析在后续阶段接入，本批先给最小严格类型和状态合同。

## R5：自动化断言与证据表

probe-continue.mjs 的 overlapping-prompt/update-mcp/cancel-then-late/eof 等 assert 把 ok 写死true，
两个 probe 最后都固定 exit0。当前 detail 与独立 oracle 相符，但失败路径也可能报绿色。
必须使用真实 predicates、缺事件算失败、aggregate/exitCode反映结果；原首败保留。
对 matcher/probe 的负向用缺事件/不符计数/错误结果证明 runner 真会红，不运行真实危险工具。

checkpoint 的“10断言全真”仅是当前布尔值，不能宣称10项真实自动化验收关闭。
修正 classification、actual timing、forced kill 来源与 scoped/未验范围。
不要顺便声称真人批准、90秒 App等待、production scope 或跨平台已完成。

完成以上后，Codex 再冻结 ADR 并派 U01 实现。整套 U00–U09/D00–D31目标保留。

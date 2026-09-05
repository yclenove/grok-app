# M1：Browser Gateway、权限与接管设计

状态：可开发规格，未实现。配套[开发计划](../../plans/2026-09-05-browser-rearchitecture/m1-gateway.md)，公共 wire 以[共用合同](00-contracts.md)为准。

## 目标与前置

让本地 Grok Build 会话通过第一方 MCP 真实调用当前任务浏览器，Host 始终知道调用者、目标、授权、结果和接管状态。M0 Preview 模型与 Runtime 可行性通过后进入本包，和 M1 Runtime 实现按接口并行。用户有一条完整路径：提出浏览器任务 -> 路由 -> 需要时批准 -> 可见浏览器操作 -> 结果/Artifact -> 随时接管 -> 显式交还。

本包不增加第二套模型代理，不把 Grok App 改成直接模型 API 工具循环，不改写 Shared `~/.grok`，不向 SSH agent 注入本地工具，不提供开放 remote-debugging 端口，不默认启用 Full CDP。

## 现有入口

`src-tauri/src/acp_client.rs` 的 session open 统一构造 `mcpServers`，同时传入 session/new 和 session/load；SSH/side-channel 强制空列表，第三方列表构建有超时预算并允许空列表继续连接。Browser 注入必须独立于该第三方构建预算，在本地 Host 已就绪时提供 descriptor；不可因为第三方 OAuth 慢而阻塞主聊天。

`src-tauri/src/session_manager/connect.rs` 处理会话 owner、预热与恢复；`src-tauri/src/permission.rs` 已有 PermissionPolicy/SessionAllowCache。Browser 复用 policy ID 和变更信号，但不能把 MCP 粗粒度 cache 当 origin/action grant。

`session_manager/control.rs::apply_extensions_mcp_change` 会热替换整个 `mcpServers` 列表，必须与 new/load/warm 共用 `browser/acp_injection.rs` 的第一方/第三方装配器。列表异步构建前后重新校验当前 AppSession、agent session 和连接 generation，切换期间丢弃旧结果；第三方开关不能删除第一方，SSH/side-channel 热更新继续空注入。热更新失败的 soft-respawn 撤销旧 binding 后重新装配。

`src/lib/permissionModeMap.ts` 与 `docs/llm-wiki/catalog.md` 的现有映射保持不变。新 origin 设置落 `general/permissions`，Runtime 诊断落 `runtime/tools`，全部登记真实 settingsCatalog entries。

## 模块与通道

```text
Grok Build ACP connection + active turn
  -> launch-scoped grok-browser MCP adapter
  -> authenticated UDS / named pipe
  -> BrowserGateway
       -> BindingRegistry
       -> OriginResolver / BrowserPolicy
       -> LeaseManager / TakeoverLatch
       -> inflight audit checkpoint
       -> capability adapter (Preview / Managed / Chrome)
```

MCP adapter 可使用捆绑 Node 运行 `browser-runtime/mcp/src/server.ts`，依赖固定版本 MCP SDK；进程退出由 Host 管理。Host credential 经 inherited handle/private pipe 交付，不写参数、URL、日志或 config。初始 descriptor 在 ACP list 只含可执行路径与非秘密参数；若 MCP SDK/CLI 无法支持非参数凭据交接，M0-R06/M1-G01 必须验证替代本地握手，不能退回明文 token 命令行。

Host 端 UDS 权限 0600/Windows Named Pipe user ACL，加连接 credential 双重校验；loopback 本身不是认证。Host restart 换 boot 和 credential。适配器和 worker 不得自行批准 origin、文件或新 lease。

## MCP 工具面

| 工具类别 | 参数来自模型 | Host 补充/验证 |
| --- | --- | --- |
| session/list/open | 用户任务意图、URL、必要 capability | 当前 AppSession owner、运行政策、backend/新 UUID |
| tab/list/activate/close | 已授权 tab handle | 所属 BrowserSession、binding generation |
| navigate/back/forward/reload/stop | URL 或当前 tab | 当前 origin、navigation revision、批准与原生能力 |
| snapshot/screenshot | 当前 tab/限定视口 | observer lease、frame policy、脱敏、Artifact owner |
| click/type/scroll | snapshot targetHandle、值/偏移 | handle revision、frame、风险与一次批准 |
| upload/downloadSave | grant/download handle | 文件 Broker capability、目标 origin、fence |
| inspect | console/network/trace、cursor | channel grant、内容 origin、预算、只读 Artifact |

工具发现必须可见 unsupported/busy/disconnected，不能注册工具后永远空成功。不同后端 capability 明确协商；M0 Preview 仅手动导航与受限检查，Agent DOM/input 需要 Managed；M3 才支持 Chrome。

MCP 的 browserSessionId/tabId 是 opaque handle，不允许模型任意指定 appSessionId 或 connector identity。App 可有多个 BrowserSession，初次选择与用户任务关联后续固定绑定，不能操作另一聊天最近打开的标签。

## Binding 与 turn 生命周期

AgentBinding 持续到 ACP/MCP 连接撤销，记录 AppSession、CLI session、connector、generation、当前有效 turn。turn 权威来自 `session_manager/turn.rs` 发起 prompt 时创建的 `active_turn_id`；Browser 在同一 Host 转换中登记，不能采用 MCP 或模型自报 turn。历史 session/load 回放不是活跃 turn，不触发 Browser 工具或审批。

结束信号与现有 `stream.rs::try_finish_deferred_prompt_complete` 及终止路径对齐。提前到达的 `prompt_complete`、等待 tool/permission 和最后 chunk 不能提前撤销活跃 Browser turn；只有 Host 的真实完成、取消、流失败、watchdog 终止或断连使对应 turn 结束。`stream/control/events/events_bg/process/watchdog` 经同一幂等 Browser 生命周期 hook 撤销，覆盖后台 Error、ProcessExited 与无事件 `sweep_dead_background`。所有 `active_turn_id=None` 及 live/background 移除路径均核对 owner/turn/connection generation，旧通知不能影响新 turn；连接死亡另撤销 binding/thread grant，单轮失败只按既有 scope 撤销。

turn 结束撤销 turn grant、writer/observer lease、队列和正在观察的流，但同一连接的 thread grant 可以保留。下一轮重新登记 turn、检查当前用户 policy 和 TakeoverLatch，再申请新 lease。Host restart/断连/Profile/backend 切换/归档/删除一律撤销相关 grant，重连只恢复 metadata。

预热 worker 不能继承别的 AppSession binding。warm reuse 只复用 Grok Build 进程，在 session/load/new 后重建当前 Browser descriptor/binding，并验证旧 connector 无法继续调用。fork 返回新的 BrowserSession，旧授权不复制。

## Policy 与一次批准

六种现有 policy 逐行执行总设计映射。Host 计算 origin，包括 scheme、IDNA hostname、effective port；顶层、执行 frame、popup opener、Console 内容来源与 Network target 分别验证。redirect 导致 origin 改变时停止后续观察/动作并重新审批，不能把原站点 allow 延伸到新站点。

`about:blank`/`blob:` 仅在 Host 有明确 creator origin 时继承；`file:`、`javascript:`、`data:` 或 opaque 默认拒绝。DNS 解析到 loopback/内网不自动继承公网规则；SSH tunnel 由 M5 另加 Host scope，端口重用不能复活 grant。

一次批准记录 approver、action kind、参数摘要/hash、targetHandle、top/frame origin、tab/nav revision、policy revision、deadline 和 consumed 状态。用户看到具体动作、站点、目标及后果摘要；拒绝、Esc、关闭 App、目标改变或过期均不派发。批准写失败不授予动作。

高风险/未知动作绝不靠 DOM 文本猜测放行。已有 origin allow 只证明允许访问范围，不证明每个点击安全。`dont_ask` 对需要新审批的动作即时 denied。密码/OTP/Passkey/CAPTCHA 转人工并停止观察；原生 OS 权限拒绝不能被 Browser grant 覆盖。

持久规则创建/扩大和 Full CDP 都有独立审批，但不能据此豁免其他硬门。持久规则必须在设置可见、可撤销，有精确 origin/capability，不接受全字符串模糊匹配放行。

## Lease 与接管

每 ControlDomain 同时一个 writer；lease 包含 actor、expiry、fence、policy 和 binding generation。observer 也有明确权限与有效 turn。共享 Managed Runtime 的任何可信输入暂停全部 Session；Chrome 未来按整个 connection native process 归因。

NativeInputFenceProvider 必须 healthy 并绑定准确进程/窗口集合才能发 writer。用户点击“接管”、收到 OS 输入或 Provider 失效都关闭 admission 并 fence；UI 立即显示 user-control/blocked_input_attribution。所有观察与排队工作取消，primitive 竞态按 unknown_outcome，绝不自动重放。

用户点击“交还”在可信 App command 中检查当前 Provider、binding、policy、runtime health，创建新 lease/queue/snapshot 后才显示 agent-running。旧 click 的延迟 ACK 不得把状态改回 agent。页面脚本、MCP、DOM isTrusted 和已缓存 thread grant 都不能清除 TakeoverLatch。

Host/worker 必须有 fencing ACK 协议。M0-R05 证明可取消 primitive 集合；M1-G04 不将一次 token 检查视为阻止 Playwright 内部自动重试。不能证明安全取消的动作不注册为 supported。

## 自动路由

路由函数输入 task intent、requested capabilities、当前 backend pin、平台可用性、Profile、policy/kill switch，输出 backend + reasonCode 或 structured failure。它是可测试的确定性规则，不另调模型猜后端。

普通 URL/localhost/read-only reference 默认 Preview；需要 Agent click/type/stable screenshot/Console/Network/Trace 默认 Managed；用户明确当前 Chrome 登录态时 M3 Chrome。固定 backend 无法满足能力时返回 unsupported 并建议可用入口，用户接受才切换。

后端切换只带 canonical URL、viewport、任务意图和 Host Session/Tab identity；创建新 backend binding，撤销旧 grant/lease/targetHandle。表单状态、Cookie、历史、下载、文件授权与 snapshot handle 都不能迁移。URL 无法安全重放时空白页+人工恢复。

## Busy、错误与用户界面

操作发起后禁用重复提交按钮，允许取消/接管；授权弹窗只有一个活跃 request，后来的请求排有界队列并显示计数。弹窗目标变化自动关闭为 stale，不能停留一个可批准的旧表单。

错误 reasonCode 覆盖 invalid identity、stale revision、policy denied、needs takeover、runtime capacity、runtime missing、provider unhealthy、transport lost、unknown outcome。用户可在当前任务查看原因和重试/接管/诊断入口。重试只能重新建立许可并重新判断，不恢复旧命令。

Settings 中 origin 规则与 Developer Mode 复用 General 权限页；Runtime Doctor 显示真实版本/签名/进程/Provider 健康。恢复探测按钮真实运行探针，失败保留详细但去敏的原因。15 语言同步并使用应用内 modal/menu/portal/native-cover。

## 审计与故障恢复

执行副作用前写 durable inflight，包含 requestId、actor/owner UUID、action class、origin、policy revision 与 decision，不记录 raw URL query、selector、输入、cookie、header/body 或路径。checkpoint 失败禁止新副作用并显示 degraded。

执行完成原子写 outcome；Host 崩溃后未完成的记录变 unknown_outcome，用户查看外部结果。MCP 重试同 requestId 只返回已知状态，不重复 dispatch；缓存淘汰后不得猜动作未执行。Runtime shutdown 使用独立 supervisor，不依赖 MCP 退出正常。

## 测试与进入发布

单元/集成用例必须覆盖所有 owner 交叉、旧 boot/binding/runtime/nav/fence、六种 policy、turn/thread、warm reuse、load replay、双 writer、接管后下一 turn、审批过期/目标变化、跨域 frame/popup、secret masking、输出大小和 audit 脱敏。

真实 Grok Build 连接矩阵：official/custom x shared/independent x new/load/warm reuse，加 MCP 热更新与切会话竞态；第三方 MCP 关闭或超时时第一方 descriptor 的行为仍可验证；SSH/side-channel 保持不注入。turn 矩阵覆盖提前 prompt_complete、等待工具/审批、最终完成、取消、watchdog 和旧 turn 迟到。真实用户输入要在 macOS arm64/x64 与 Windows x64 上用签名包验证。

本包开发出口 M1-G06 可用测试 adapter 验证协议和实际 CLI；最终 M1 release 还必须等待 M1-R06 的真实 Runtime 和 M1-D06 的最终包验收，不能单独发布 Gateway 按钮。

证据目录 `docs/qa/browser-rearchitecture/m1-gateway/`。回滚先撤销 Browser binding/lease，停注入并保留 Preview；不清除 Agent 会话或 Profile。用户任务失败要记录取消/未知结果，不能把回滚当外部行为撤销。

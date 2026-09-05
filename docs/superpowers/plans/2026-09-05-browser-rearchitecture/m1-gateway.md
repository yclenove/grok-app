# M1 Browser Gateway 与权限实现计划

> **面向 AI 代理的工作者：** 使用 `subagent-driven-development` 或 `executing-plans` 逐任务实现。步骤均为未执行开发待办，按依赖完成后再推进。

**目标：** 打通本地 Grok Build 到 Host Browser Gateway 的完整调用、许可、接管和可审计结果。

**架构：** 第一方 MCP 负责协议适配；Rust 是身份、origin、grant、lease、路由与批准的唯一权威。Worker/Preview 实现 capability adapter，App 只消费状态。

**技术栈：** Rust/Tokio/Tauri、JSON RPC/私有 pipe、固定 MCP SDK、TypeScript/React、Vitest/Rust tests、真实 Grok Build CLI。

规格：[m1-gateway](../../specs/2026-09-05-browser-rearchitecture/m1-gateway.md)。公共 wire：[00-contracts](../../specs/2026-09-05-browser-rearchitecture/00-contracts.md)。命令从仓库根执行，计划新增文件由对应任务创建后才能运行。

## 文件与分工

| 路径 | 变更类型与职责 |
| --- | --- |
| `src-tauri/src/browser/{gateway,binding,origin,policy,lease,approval,audit}.rs` | 新建：领域安全与派发 |
| `src-tauri/src/browser/{mcp,transport,router}.rs` | 新建：第一方连接、身份交接和能力路由 |
| `browser-runtime/mcp/{package.json,vitest.config.ts,src/server.ts}` | 新建：捆绑 MCP SDK adapter，独立测试配置 |
| `src-tauri/src/acp_client.rs`、`session_manager/{connect,control}.rs` | 修改：new/load/warm/MCP 热更新的统一装配调用 |
| `src-tauri/src/session_manager/{turn,stream,events,events_bg,process,watchdog}.rs` | 修改：Host active_turn_id、前后台真实完成/退出/死亡清扫的幂等 Browser hook |
| `src-tauri/src/browser/acp_injection.rs` | 新建：把注入逻辑抽出，避免继续扩大 acp_client |
| `src/components/browser/{BrowserApprovalDialog,BrowserControlBar,BrowserDiagnostics}.tsx` | 新建：真实审批、接管、诊断 |
| `src/lib/settingsCatalog/entries/{general,runtime}.ts` | 修改：origin 与诊断设置注册 |
| `src-tauri/src/permission.rs`、`src/lib/permissionModeMap.ts` | 读取现有语义，测试保证 enum/spawn 映射不变 |

进入条件：M0-W06 Preview 模型/迁移已验证，M0-R06 完成三平台 Runtime/输入可行性。开发可在 fake adapter 上隔离进行，但最终发布不能用 fake 结果代替真实 Runtime。

## 开发任务

### M1-G01：认证传输与第一方 MCP 注入

**依赖：** `M0-W06`、`M0-R06`。

**负责：** Rust/ACP/MCP 工程师。

**估算：** 5–7 工程日。

**文件：** 创建 `src-tauri/src/browser/{mcp,transport,binding,acp_injection}.rs`、`browser-runtime/mcp/{package.json,vitest.config.ts,src/server.ts,src/server.test.ts}`；修改 `acp_client.rs`、`session_manager/{connect,control}.rs`、`src-tauri/src/lib.rs`，为新增 Node 子包提供独立 lockfile。

- [ ] 在 Rust transport tests 写错误 credential、旧 boot、重复连接、超限 frame、SSH/side-channel 空注入；MCP tests 覆盖握手断线、热更新保留第一方、列表构建时切会话和无 secret 日志。
- [ ] 跑 `cargo test --manifest-path src-tauri/Cargo.toml browser::transport` 与 `pnpm --dir browser-runtime/mcp exec vitest run --config vitest.config.ts`，确认负例失败再实现。
- [ ] 实现 UDS 0600/Windows pipe ACL + launch-scoped credential，secret 用继承 handle/私有管道；MCP SDK版本固定，禁用任意远程调试入口。
- [ ] 将 first-party descriptor 与第三方构建预算分开；new/load/fork/warm 与 control.rs 的热更新共用 acp_injection 装配器，异步前后核对 owner/agentSession/connection generation；预热仅预建无任务权限进程，绑定后再授 identity。
- [ ] 接入 turn 注册、断线撤销和去重；第三方 MCP 关闭/超时不会移除已健康第一方，Host Browser禁用则不授 capability。
- [ ] 运行新测试、现有 acp/permission 测试、fmt/clippy；提交 `feat(browser): inject authenticated first party mcp` 并推送。

descriptor 拼装的可验证顺序：

```text
if ssh or official-side-channel: []
else:
  firstParty = host.browserDescriptorIfHealthy(currentConnection)
  thirdParty = buildExistingMcpListWithinBudget()
  result = rejectNameCollision(firstParty, thirdParty)
  publish list without credentials
```

验收：连接超时仍能继续聊天并准确显示 Browser unavailable；同名用户 MCP 不取得第一方身份；Shared GROK_HOME 文件 hash 不变。

### M1-G02：Origin 解析与权限决策

**依赖：** `M1-G01`。

**负责：** Host 安全工程师。

**估算：** 3–5 工程日。

**文件：** 创建 `src-tauri/src/browser/{origin,policy,gateway}.rs`、`src-tauri/src/browser/policy_tests.rs`、`tests/browser-contracts/origins.json`；修改 `browser/protocol.rs` 扩展已登记 reasonCode，保持 wire 版本约束。

- [ ] 写 IDNA/default port/redirect/opaque/file/javascript/blob、顶层与 iframe 双授权、popup 未归属、内容 origin 和模型伪造风险字段用例。
- [ ] 跑 `cargo test --manifest-path src-tauri/Cargo.toml browser::policy` 与 `browser::origin` 过滤测试，确认每个拒绝路径有效。
- [ ] 使用 Rust `url::Url` 结构化 canonicalize；Host 从 live frame/binding 构造 origin，不采用 MCP 自报字符串。
- [ ] 实现 deny > block > exact allow > default；把 allow capability 与动作后果批准分开，六种现有 PermissionPolicy 使用闭集映射。
- [ ] 未知动作要求批准/拒绝；拒绝参数里的自报低风险，记录 reasonCode；context、frame 或 policy revision 改变时使旧目标无效。
- [ ] 跑 Rust测试、现有 `pnpm exec vitest run src/lib/permissionModeMap.test.ts`，提交 `feat(browser): enforce origin and action policy` 并推送。

Origin fixture 示例：

```json
[
  {"url":"https://EXAMPLE.test:443/a?token=x","canonical":"https://example.test"},
  {"url":"https://example.test:8443/a","canonical":"https://example.test:8443"},
  {"url":"javascript:alert(1)","reasonCode":"origin_scheme_denied"}
]
```

验收：参数里的 URL query 不进入 audit；unknown click 不因按钮文字为“查看”自动允许；原站点 grant 不覆盖重定向站点。

### M1-G03：批准、grant 与跨轮生命周期

**依赖：** `M1-G02`。

**负责：** Host 权限与 React 工程师。

**估算：** 5–7 工程日。

**文件：** 创建 `src-tauri/src/browser/approval.rs`、`src-tauri/src/browser/approval_tests.rs`、`src/components/browser/BrowserApprovalDialog.tsx`、`src/components/browser/BrowserApprovalDialog.test.tsx`；修改 `browser/binding.rs`、`session_manager/{connect,turn,stream,control,events,events_bg,process,watchdog}.rs`、`src-tauri/src/commands/browser.rs`、十五语言领域目录。

- [ ] 写批准过期、拒绝/Esc、目标导航变化、一次消费、磁盘失败、提前 prompt_complete、真实完成/thread保留/断连撤销、后台 Error/ProcessExited/无事件 sweep、取消/watchdog/旧 turn 迟到，以及 dont_ask 不弹窗用例。
- [ ] 跑 `cargo test --manifest-path src-tauri/Cargo.toml browser::approval` 与 `pnpm exec vitest run src/components/browser/BrowserApprovalDialog.test.tsx` 并核对失败原因。
- [ ] 实现 approval descriptor 与 grant store，绑定 target/参数摘要/origin/navRevision/expiry，可信 App command 才能批准，MCP 无批准入口。
- [ ] turn.rs 创建 Host active_turn_id 时登记 Browser turn；复用 stream.rs 的真实完成判定，control/events/events_bg/process/watchdog 所有清 turn/移除 live/background 路径统一以 owner/turn/connection generation 幂等撤销。连接死亡撤销 binding/thread grant，旧通知不影响新轮；提前 prompt_complete 与 load history 不制造结束或新授权。
- [ ] 用现有 GlassModal/portal/native-cover 展示动作和目标，busy/取消/错误/焦点恢复完整；秘密挑战暂停观察并转接管。
- [ ] 运行新增测试与 i18n/catalog测试；提交 `feat(browser): add scoped browser approvals` 并推送。

批准验证必须逐项比较，而非仅匹配工具名：

```text
approve(request):
  require trusted App UI origin
  require request.id == pending.id and pending.notExpired
  require current.tabRevision == pending.tabRevision
  require current.navRevision == pending.navRevision
  require current.policyRevision == pending.policyRevision
  persist consumed-on-dispatch token bound to argument digest
```

验收：批准弹窗开着时页面变化，旧按钮只能得到 stale；dont_ask 返回拒绝不留下挂起RPC；用户输入秘密不会出现在截图/日志/support bundle。

### M1-G04：控制租约、接管锁存与原子撤销

**依赖：** `M1-G03`、`M0-R06`。

**负责：** Host 并发/平台工程师。

**估算：** 4–6 工程日。

**文件：** 创建 `src-tauri/src/browser/lease.rs`、`src-tauri/src/browser/lease_tests.rs`、`src/components/browser/BrowserControlBar.tsx`、`src/components/browser/BrowserControlBar.test.tsx`；修改 `gateway.rs`、`commands/browser.rs`，接入 M0-R/M1-R 的 `runtime/input_fence.rs` 合同。

- [ ] 写双 writer、lease expiry、旧 fence、用户接管后新turn、Provider丢失/权限撤销、旧ACK、共享Runtime其他Session观察取消的并发测试。
- [ ] 跑 `cargo test --manifest-path src-tauri/Cargo.toml browser::lease` 与 `pnpm exec vitest run src/components/browser/BrowserControlBar.test.tsx`，要求调度器可固定事件顺序。
- [ ] 实现独立 TakeoverLatch、单 writer admission 与 fencing ACK；Host收到原生输入先关闭新派发，再取消队列/观察和在途操作。
- [ ] 将已验证的取消 primitive 集合接入 adapter；在Playwright内部等待无法取消时终止作用域/报告unknown，不宣称撤销已送到网页的输入。
- [ ] 实现可信UI交还：重新验证Provider/binding/policy、生成新lease/queue/snapshot；Agent、旧grant、Provider恢复都不能触发交还。
- [ ] 跑受控竞争测试至少100次无旧命令通过，保存结果；提交 `feat(browser): enforce takeover fencing` 并推送。

状态转换合同：

```ts
type Control = { latch: "clear" | "user"; lease: string | null; fence: number };
const takeover = (state: Control): Control =>
  ({ latch: "user", lease: null, fence: state.fence + 1 });
expect(takeover({ latch: "clear", lease: "old", fence: 8 }))
  .toEqual({ latch: "user", lease: null, fence: 9 });
```

以上仅说明状态形状；验收必须同时断言真实adapter不继续发输入、观察停止，以及旧response不改回agent-running。溢出测试验证fail closed，不可回绕。

### M1-G05：能力路由、审计与诊断交互

**依赖：** `M1-G04`、`M0-W06`。

**负责：** Browser 全栈工程师。

**估算：** 4–6 工程日。

**文件：** 创建 `src-tauri/src/browser/{router,audit}.rs`、`src/components/browser/{BrowserDiagnostics,OriginPermissionsPanel}.tsx`、`src/components/browser/router.test.ts`；修改 BrowserWorkbench、`settingsCatalog/entries/{general,runtime}.ts`、对应 General/Runtime settings 组件和十五语言目录。

- [ ] 写路由能力矩阵、用户固定后端、Managed不可用、Chrome未配对、敏感URL不重放、审计payload去敏和磁盘失败用例。
- [ ] 运行 `cargo test --manifest-path src-tauri/Cargo.toml browser::router`、`browser::audit` 和 `pnpm exec vitest run src/components/browser/router.test.ts`，确认没有自动退回Preview却报告Agent成功。
- [ ] 实现确定性route结果与reasonCode；切换只转URL/viewport/intent/Host身份，撤销旧binding/grants/handles。
- [ ] 副作用dispatch前原子写inflight、结束写outcome；同requestId只返回已知结果；恢复unknown不重派发。
- [ ] 接入Runtime/Provider/签名诊断与站点规则管理，新增设置anchor/search/deep link，修复按钮真实调用检测，扩大持久规则必须批准。
- [ ] 运行 settingsCatalog/i18n 与新增测试，提交 `feat(browser): add routing audit and diagnostics` 并推送。

路由回归表必须可执行：

```json
[
  {"intent":"view","capability":"navigate","pin":null,"expected":"preview"},
  {"intent":"act","capability":"click","pin":null,"expected":"managed"},
  {"intent":"act","capability":"click","pin":"preview","expected":"unsupported"},
  {"intent":"currentChrome","capability":"navigate","paired":false,"expected":"needs_pairing"}
]
```

验收：每个诊断项展示真实状态；rule删除撤销当前匹配授权；原始URL、输入、selector、cookie、路径不会混入审计记录。

### M1-G06：真实 CLI、身份与权限验收

**依赖：** `M1-G01`、`M1-G02`、`M1-G03`、`M1-G04`、`M1-G05`。

**负责：** QA/ACP/安全工程师。

**估算：** 3–5 工程日。

**文件：** 创建 `scripts/browser/verify-cli-binding.mjs`、`tests/browser-contracts/gateway-scenarios.json`、`src-tauri/src/browser/gateway_integration_tests.rs`、`docs/qa/browser-rearchitecture/m1-gateway/acceptance.md`；修改相关 `docs/llm-wiki/model-routing.md` 和 `providers.md` 补充第一方Browser边界。

- [ ] 建立真实CLI矩阵 official/custom、shared/independent、new/load/warm，记录CLI实际版本；custom用测试relay，凭据仅从安全环境读取。
- [ ] 创建verify脚本，运行 `node scripts/browser/verify-cli-binding.mjs --matrix tests/browser-contracts/gateway-scenarios.json --output docs/qa/browser-rearchitecture/m1-gateway/results.json`；脚本只输出去敏身份/判定，不记录用户内容。
- [ ] 对旧boot/turn/lease、跨Session、伪造target、MCP同名、第三方超时、热更新切会话、SSH不注入、审批取消、提前完成通知和load回放执行故障注入；所有越权用例必须拒绝。
- [ ] 检查强制CI：前端typecheck/test/lint/build、质量门、Rustfmt/clippy/test；本包只使用fake adapter时明确标adapter类型，真实浏览器证据交由M1-R06/D06补齐。
- [ ] 写验收记录与停止Browser注入/保留Preview的回滚演练，列明所有未运行平台；不得以测试数多代替真实CLI绑定证据。
- [ ] 提交 `test(browser): verify cli gateway permissions` 并推送，只有M1-R06/M1-D06也通过才开放Managed用户路径。

矩阵条目必须记录测试环境与入口，并对观察结果做断言：

```json
{"case":"shared-load-stale-turn","provider":"official","dataMode":"shared","entry":"load","adapter":"fake","requestTurn":"ended-turn","expectedStatus":"stale","expectedDispatchCount":0,"expectedSharedHomeChanged":false}
```

脚本运行时由 Host 测试连接建立完整 envelope，不将上例的 scenario 字段当作生产请求。输出同时记录实际 CLI 版本、非零用例数和结果；fake adapter 不证明 Playwright 或最终包通过。

退出条件：真实CLI矩阵全部预期一致、身份负例零放行、命令未知结果零重放、所有UI状态完整。缺官方账户或签名设备时记录外部依赖与not_run，不能调整产品权限绕过验收。

# M5 SSH Browser Bridge 实现计划

> **面向 AI 代理的工作者：** 使用 subagent-driven-development 或 executing-plans 按任务执行；本文只规划，所有步骤、远端测试与发布均未执行。

**目标：** 让本机浏览器通过受监管 OpenSSH tunnel 安全访问 SSH 项目远端 localhost。
**架构：** 复用现有 prepare/OpenSSH helper；Host/Guard 双持有 listener，经身份固定的 Bridge 专属 mux 或 dedicated `ssh -W` 转发。单点故障由存活方保持 tombstone；多点故障无证明不放行。Runtime 在本机，远端 ACP 无本地 Browser MCP。
**技术栈：** Rust/Tokio、系统 OpenSSH、Tauri/React、M1 Gateway/Playwright fixtures。
**规格：** [M5 设计](../../specs/2026-09-05-browser-rearchitecture/m5-ssh.md)、[共用合同](../../specs/2026-09-05-browser-rearchitecture/00-contracts.md)。

## 当前证据与文件职责

`src-tauri/src/ssh_remote/skills_browser.rs` 已有 parser、rewrite、mux 与 dedicated forward；仅有端口缓存和后台 wait。
`src/lib/api/ssh.ts` 已暴露 `sshBrowserPrepare`；`BrowserTab.tsx` 在导航前调用它。
`src-tauri/src/ssh_remote/acp_run.rs` 已处理 Windows 无 ControlMaster；应复用 `find_ssh_binary` 与 argv helper。
`src-tauri/src/acp_client.rs` 的 SSH session open 已使用空本地 `mcpServers`；此边界必须保留。

| 路径 | 职责/来源 |
| --- | --- |
| `src-tauri/src/ssh_remote/browser_tunnel.rs` | 新建，抽取现有 helper 并扩展受监管 `ssh -W` channel |
| `src-tauri/src/browser/ssh_listener.rs` | 新建，Host/Guard 双持有 listener、tombstone 与背压 |
| `src-tauri/src/ssh_remote/mod.rs` | 现有 include 装配，加入新文件 |
| `src-tauri/src/browser/ssh_bridge.rs` | 新建，registry、owner 和状态机 |
| `src-tauri/src/browser/ssh_origin.rs` | 新建，transport scope 与 origin 校验 |
| `src-tauri/src/browser/binding.rs`、`origin.rs`、`lifecycle.rs` | M1 前置产物，接入 transport scope 与撤销 |
| `src/components/browser/ssh/bridge.ts`、`SshBridgeStatus.tsx` | 新建，Host API 投影与交互 |
| `tests/browser/ssh/` | 新建，逐任务列出真实测试路径 |
| `tests/browser/playwright.config.ts` | M1-D04 前置 runner |

命令在仓库根执行；所有 SSH 测试仅使用可销毁测试容器/VM 的合成密钥与已固定 known_hosts。
测试资产不得写入真实身份文件或把远端主机名、用户名、完整路径提交到公开证据。
M1 引用文件均须先由上游创建；当前计划不创建生产实现。

## 交付顺序与运行条件

| 规格合同 | 实现与证明任务 |
| --- | --- |
| 已有入口迁移、owner 与 URL 规范化 | M5-01 |
| OpenSSH 复用、ready 证明、取消与清理 | M5-02 |
| tunnel revision、origin、Profile/端口隔离 | M5-03 |
| 键盘、错误、重连和人工恢复交互 | M5-04 |
| remote ACP、TLS、多端口和上传限制 | M5-05 |
| 最终包、竞态、回滚和发布门 | M5-06 |

sshd fixture 使用独立临时密钥，容器只把测试端口绑定 runner loopback。
测试 known_hosts 通过 fixture 公钥生成；不使用用户自己的 `~/.ssh/known_hosts`。
测试进程继承的环境只含 fixture 必需值，禁止读取用户 agent-home 或 relay key。
Windows 必须分别记录 System32 OpenSSH 与 Git OpenSSH 的版本和启动路径类别。
macOS x64 必须是真实 Intel 设备或 x64 VM，不能把 arm64 交叉编译当作运行结果。
kill/断网注入仅命中该 run nonce 的 fixture 和 owned tunnel；其他 SSH 工作保持运行。
没有可用 SSH VM 时，状态为 `not_run`，不得以 mock 通过替代包级验收。
旧 API 消费者迁移到同一 registry/listener；旧无 owner 或裸 `-L` Browser 直连路径不可继续存在。
M5 flag 回滚必须先完成 forward 清理和隔离 Profile 关闭，再确认禁用生效。

### M5-01：将现有 prepare 迁入有归属的 Bridge 模型

**依赖：** M1-G06、M1-R06、M1-D06。
**负责：** Rust Host 工程师、SSH 模块维护者。
**估算：** 2-3 工程日。
**文件：** 新建 `src-tauri/src/browser/ssh_bridge.rs`、`src-tauri/src/browser/ssh_bridge_tests.rs`；修改 `src-tauri/src/ssh_remote/skills_browser.rs`、`src/lib/api/ssh.ts`、前置 `src-tauri/src/browser/mod.rs`；新建 `src/components/browser/ssh/bridge.ts`、`src/components/browser/ssh/bridge.test.ts`。

- [ ] 1. 写 owner 错误、public URL、loopback/IPv6、非法 URL、取消未完成 prepare 的 fixture，固定 Host 从项目读取 alias。

```ts
const request = { browserSessionId: "session-a", tabId: "tab-a", displayUrl: "http://localhost:3000/" };
const expected = { state: "preparing", actualUrl: null, tunnelRevision: 1 };
```

- [ ] 2. 实现 `browser_ssh_prepare` 与 registry 状态机，旧入口委托新逻辑并要求相同 session/tab owner；移除无 owner 旁路。
- [ ] 3. 保留结构化 URL parser，拒绝 userinfo、非 http/https；0.0.0.0 与 `.localhost` 使用现有兼容规则规范化后记录 remote endpoint。
- [ ] 4. 运行 `cargo test --manifest-path src-tauri/Cargo.toml browser::ssh_bridge_tests`；预期 cross-session 拒绝、public URL 无 tunnel、loopback 进入 preparing。
- [ ] 5. 运行 `pnpm test -- src/components/browser/ssh/bridge.test.ts src/lib/sshLoopbackUrl.test.ts`；预期已有 URL 行为兼容且 no-owner 调用失败。
- [ ] 6. 记录 API before/after、状态转换和 `RequestEnvelope` 对齐结果，确保 tunnelRevision 未变成 caller 授权字段。

**验收/证据：** `docs/qa/browser-rearchitecture/m5/M5-01/`，目前未执行；公共 HTTPS 不绕远端，无任意 alias 注入。

### M5-02：复用 OpenSSH 并管理 tunnel 进程生命周期

**依赖：** M5-01。
**负责：** SSH/进程监管工程师。
**估算：** 7-10 工程日，包含双持有故障顺序、专属 mux identity/keepalive 与 Windows dedicated 成本验证。
**文件：** 新建 `src-tauri/src/ssh_remote/browser_tunnel.rs`、`src-tauri/src/browser/ssh_listener.rs`、`src-tauri/src/browser/ssh_listener_tests.rs`；修改 `src-tauri/src/ssh_remote/mod.rs`、`src-tauri/src/ssh_remote/skills_browser.rs`、`src-tauri/src/ssh_remote/tests.rs`、前置 `src-tauri/src/browser/lifecycle.rs`、`src-tauri/src/browser/runtime/profile_guard.rs`；新建 `tests/browser/ssh/sshd-fixture/Dockerfile`、`tests/browser/ssh/sshd-fixture/sshd_config`、`tests/browser/ssh/tunnel-lifecycle.spec.ts`、`tests/browser/ssh/master-identity.spec.ts`。

- [ ] 1. 抽出 SSH helper，按 endpoint/跳转路由/配置认证摘要/host key/tunnel revision 创建专属 mux；不接受现有 alias master，无法验证 identity 时使用 dedicated；旧 prepare 不留裸 `-L` 旁路。
- [ ] 2. Host 创建 listener 并保留原句柄，Guard 持有明确复制的句柄；任一单点死亡由另一方保持 tombstone 到页面/channels/descendants 退出。每 TCP stream 走 owned `-W` pipe，不另开后端 TCP 端口。

```json
{"masterArgs":["-M","-N","-S","fixture-control.sock","-o","BatchMode=yes","-o","StrictHostKeyChecking=yes","-o","ControlPersist=no","-o","ServerAliveInterval=5","-o","ServerAliveCountMax=2","fixture-host"],"channelArgs":["-S","fixture-control.sock","-T","-W","127.0.0.1:3000","fixture-host"]}
```

- [ ] 3. ready 前验证双句柄/专属 master 身份/实际 keepalive/probe；限制 16 streams/Bridge、64/App、双向各 64 KiB。EOF/cancel 先 tombstone，关 channels/页面，退出证明后关最后句柄与专属 master。
- [ ] 4. 运行 `cargo test --manifest-path src-tauri/Cargo.toml ssh_remote::tests` 与 `cargo test --manifest-path src-tauri/Cargo.toml browser::ssh_listener_tests`；预期 argv/IPv6 正确、权限不降级、超量 stream 有界拒绝。
- [ ] 5. 运行 `pnpm exec playwright test --config tests/browser/playwright.config.ts tests/browser/ssh/tunnel-lifecycle.spec.ts tests/browser/ssh/master-identity.spec.ts --repeat-each=3`；分别 Host-only/Guard-only/SSH kill、100 次端口抢占、同 alias 改目标、旧无 keepalive master、15 秒黑洞，预期单点无泄漏且30秒owned资源=0。
- [ ] 6. 记录 macOS/Windows 原生/Git OpenSSH 实际包证据；多点同时死亡单列 No-go 边界，不能声称绝对无竞态。专属 master 退出不影响既有 watch/terminal master、其他 SSH 和远端 leader。

**验收/证据：** `docs/qa/browser-rearchitecture/m5/M5-02/`，目前未执行；单点存活方维持 tombstone、master keepalive <=15 秒，最后句柄晚于旧页面/channels/descendants 退出；多点无证据为 No-go。

### M5-03：绑定 tunnel revision 与双端 origin 权限

**依赖：** M5-01、M5-02。
**负责：** Gateway/权限工程师、安全 QA。
**估算：** 3-5 工程日。
**文件：** 新建 `src-tauri/src/browser/ssh_origin.rs`、`src-tauri/src/browser/ssh_origin_tests.rs`；修改前置 `src-tauri/src/browser/origin.rs`、`src-tauri/src/browser/binding.rs`、`src-tauri/src/browser/profiles/store.rs`；新建 `tests/browser/ssh/origin-isolation.spec.ts`、`tests/browser/ssh/origin-cases.json`。

- [ ] 1. 写同端口不同 host、同 host 新 revision、重定向、popup、跨域 frame、旧 snapshot 和被抢占端口 fixture。

```json
{"grant":{"tunnelId":"tunnel-a","tunnelRevision":1,"canonicalOrigin":"http://127.0.0.1:43127"},"binding":{"tunnelId":"tunnel-a","tunnelRevision":2,"canonicalOrigin":"http://127.0.0.1:43127"},"expected":"denied"}
```

- [ ] 2. 将 transport scope 挂在 Host BackendBinding；origin policy 用实际 frame/network origin 加 tunnelId/revision 匹配，调用者提供的 remote origin 不参与信任。
- [ ] 3. 每个 live revision 建独立临时网络/存储域；重连销毁旧域，撤销所有 lease/grant/观察；Preview 无法保证隔离时返回 unsupported。
- [ ] 4. 运行 `cargo test --manifest-path src-tauri/Cargo.toml browser::ssh_origin_tests`；预期 stale revision、不同 owner、public redirect 的授权继承数为零。
- [ ] 5. 运行 `pnpm exec playwright test --config tests/browser/playwright.config.ts tests/browser/ssh/origin-isolation.spec.ts --repeat-each=3`；强制端口复用 30 次，Cookie、Service Worker、OriginGrant 均不继承。
- [ ] 6. 保存 scope 对照与过期请求结果，确认 listener tombstone 截断旧后台 reconnect，旧 Profile 退出前端口无法被替代服务占用，失败操作不自动重放。

**验收/证据：** `docs/qa/browser-rearchitecture/m5/M5-03/`，目前未执行；URL/alias/localPort 任何单项都不足以授权。

### M5-04：完成连接、取消、重连与限制视图

**依赖：** M5-02、M5-03。
**负责：** 前端工程师、i18n 维护者。
**估算：** 2-4 工程日。
**文件：** 新建 `src/components/browser/ssh/SshBridgeStatus.tsx`、`src/components/browser/ssh/SshBridgeStatus.test.tsx`、`src/components/browser/ssh/sshBridge.css`；修改 `src/components/browser/ssh/bridge.ts`、`src/components/side-workbench/BrowserTab.tsx`、`src/lib/settingsCatalog/entries/runtime.ts` 和下列语言文件。

语言文件：`src/i18n/messages/en/workspace.ts`、`src/i18n/messages/zh/workspace.ts`、`src/i18n/messages/zh-TW/workspace.ts`、`src/i18n/messages/de/workspace.ts`、`src/i18n/messages/es/workspace.ts`、`src/i18n/messages/fil/workspace.ts`、`src/i18n/messages/fr/workspace.ts`、`src/i18n/messages/id/workspace.ts`、`src/i18n/messages/it/workspace.ts`、`src/i18n/messages/ja/workspace.ts`、`src/i18n/messages/ko/workspace.ts`、`src/i18n/messages/pt-BR/workspace.ts`、`src/i18n/messages/ru/workspace.ts`、`src/i18n/messages/ta/workspace.ts`、`src/i18n/messages/uk/workspace.ts`。

- [ ] 1. 写 preparing/cancel、disconnected/reconnect、TLS error、host-key error、unknown outcome 和 cleanup_failed 的交互测试。
- [ ] 2. 使用原始 displayUrl 和 Host snapshot 呈现状态；取消与关闭可用，重连显式执行，busy 不允许重复派发。

```ts
const actionsByState = {
  preparing: ["cancel"], disconnected: ["reconnect", "close"],
  ready: ["reload", "close"], closing: [], closed: [], failed: ["retry", "close"],
};
```

- [ ] 3. TLS/多端口/Preview 隔离失败显示真实原因与可行选项；不使用系统 confirm/select，不增加 AppWorkbench 状态；新设置注册 `runtime.browser.sshBridge`。
- [ ] 4. 运行 `pnpm test -- src/components/browser/ssh/SshBridgeStatus.test.tsx src/i18n/messages.test.ts src/lib/settingsCatalog.test.ts` 和 `pnpm typecheck`；预期键盘、重复点击、错误恢复均通过。
- [ ] 5. 在 `en/zh/de/ru/ta` 下检查实际包截图与焦点顺序，长 alias/URL 换行不遮挡按钮，tooltip 与错误使用 i18n。
- [ ] 6. 保存 `docs/qa/browser-rearchitecture/m5/M5-04/ui.md` 和脱敏截图；验收新 revision 登录态不继承的提示与行为一致。

**验收/证据：** `docs/qa/browser-rearchitecture/m5/M5-04/`，目前未执行；操作不以成功外观掩盖 unsupported。

### M5-05：锁定 remote ACP 边界与协议/文件行为

**依赖：** M5-03、M5-04。
**负责：** ACP 工程师、安全 QA。
**估算：** 2-3 工程日。
**文件：** 修改 `src-tauri/src/acp_client.rs`、`src-tauri/src/ssh_remote/tests.rs`、`docs/llm-wiki/ssh-remote.md`；新建 `tests/browser/ssh/remote-acp-boundary.spec.ts`、`tests/browser/ssh/protocol-limits.spec.ts`、`tests/browser/ssh/remote-origin-fixture.mjs`。

- [ ] 1. 覆盖 session/new、load、fork、reconnect、MCP 热更新五种 SSH ACP 路径，断言不会得到本机 Browser MCP 或 connector credential。

```json
{"transport":"ssh","operation":"session/load","localMcpServers":[],"browserControl":{"status":"unsupported","reasonCode":"ssh_remote_agent_control_unsupported"}}
```

- [ ] 2. 在 Host 命令入口保持 AgentBinding 来源验证，本机合法 binding 可操作 bridge，远端 ACP 不因同 AppSession 自动继承能力。
- [ ] 3. 添加 http/ws、https/wss、证书拒绝、HMR 新端口、OAuth 回调、远端路径上传 fixture；只为明确通过的单 origin 协议声明支持。
- [ ] 4. 运行 `cargo test --manifest-path src-tauri/Cargo.toml ssh_remote::tests` 与 `cargo test --manifest-path src-tauri/Cargo.toml acp_golden`；预期原 SSH session 行为与 MCP 边界均通过。
- [ ] 5. 运行 `pnpm exec playwright test --config tests/browser/playwright.config.ts tests/browser/ssh/remote-acp-boundary.spec.ts tests/browser/ssh/protocol-limits.spec.ts --repeat-each=3`；预期远端文件不进入本机 UploadGrant，TLS 不绕过。
- [ ] 6. 更新 SSH wiki 的 Wave 2 Browser 描述与 M5 明确限制，导出脱敏 MCP payload/fixture 结果供安全复核。

**验收/证据：** `docs/qa/browser-rearchitecture/m5/M5-05/`，目前未执行；无本机 credential 或私有 transport 路径向远端泄漏。

### M5-06：执行真实包 SSH 验收与回滚演练

**依赖：** M5-01、M5-02、M5-03、M5-04、M5-05。
**负责：** 独立 QA、SSH 维护者、发布负责人。
**估算：** 3-4 工程日，不含 dogfood 观察等待。
**文件：** 新建 `tests/browser/ssh/package-golden.spec.ts`、`scripts/browser/check-ssh-gate.mjs`；修改 `.github/workflows/ci.yml`；新建 `docs/qa/browser-rearchitecture/m5/M5-06/acceptance.md`。

- [ ] 1. 配置 macOS arm64/x64、Windows x64 实际包与两台测试 SSH host，远端仅 POSIX；Linux 行必须引用 M4 放行证据。
- [ ] 2. 运行 `pnpm exec playwright test --config tests/browser/playwright.config.ts tests/browser/ssh/package-golden.spec.ts --repeat-each=3`；预期每包每声明场景 100% 通过。

```json
{"minimumCases":{"cancelReadyRace":30,"portReuse":100,"hostOnlyCrash":30,"guardOnlyCrash":30,"aliasTargetChange":30,"oldMasterWithoutKeepalive":30,"networkBlackhole":30},"require":{"foreignGrantReuse":0,"remoteMcpLeak":0,"ownedChannelsAt30s":0,"ownedListenersAt30s":0,"ownedMastersAt30s":0,"autoReplay":0}}
```

- [ ] 3. 执行关闭会话、停止 bridge、Host/App crash、睡眠、断网、服务拒绝与版本回滚；核对远端 leader/其他 SSH session/用户 Chrome 未受损。
- [ ] 4. 运行 `pnpm typecheck`、`pnpm test`、`pnpm lint`、`pnpm build:ui`、`python3 scripts/check-code-quality-gates.py --mode final` 与 `cargo test --manifest-path src-tauri/Cargo.toml`；预期全绿。
- [ ] 5. 运行 `node scripts/browser/check-ssh-gate.mjs --evidence docs/qa/browser-rearchitecture/m5`；要求精确包 digest、30 次竞态结果、origin/存储隔离、MCP 边界与清理 ledger。
- [ ] 6. 签署 M5 Go/No-go，记录独立 flag 的关闭/回滚行为；样本不足或任一 orphan/越权出现均不升环。

**验收/证据：** `docs/qa/browser-rearchitecture/m5/M5-06/`，目前未执行；M5 的完成不表示 remote Agent Browser 已交付。

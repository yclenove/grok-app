# M5：SSH Browser Bridge 设计

**状态：** 设计完成；代码、远端实验与安装包验收未执行。
**上游：** [总体设计](../2026-09-04-browser-rearchitecture-design.md)、[共用合同](00-contracts.md)。
**实施：** [M5 开发计划](../../plans/2026-09-05-browser-rearchitecture/m5-ssh.md)。
**准入：** M1-G06、M1-R06、M1-D06；Linux 本机支持另受 M4 准入约束。

## 目标与非目标

- 本机 Preview/Managed 通过受监管的 OpenSSH tunnel 访问已关联 SSH 项目的远端 localhost。
- 保留原始远端显示 URL，并使 Host 根据实际 tunnel、binding 和页面重新计算权限。
- 让创建、取消、重连、断线、tab 关闭、会话关闭、Host crash 都有可观察的清理结果。
- local Browser Runtime、Profile、Artifact 与文件授权仍由本机 Host 管理。
- 重用系统 OpenSSH、现有 prepare/URL/别名/host-key/argv helper；将现有裸 `-L` 升级为有监听所有权的 Bridge。
- 不运行远端 Chromium、不启动远端桌面/X11 转发、不开发第二个 SSH 协议栈。
- 不向远端 ACP 注入本机 Browser MCP、私有 pipe 路径或 connector credential。
- 不把远端文件路径作为本机上传路径，不透明迁移远端 Cookie/认证资料。
- 不承诺任意反向代理、任意多端口应用、自动 Host header 改写或自动绕过 TLS 验证。
- 不终止远端 Grok leader，不因 Browser 关闭而破坏其他 SSH 文件/终端会话。

## 当前代码证据

| 位置 | 当前行为 | 需要补齐 |
| --- | --- | --- |
| `src/lib/api/ssh.ts` | `sshBrowserPrepare(alias, url)` 调用 `ssh_browser_prepare` | 与 BrowserSession/tab ownership 绑定 |
| `src/components/side-workbench/BrowserTab.tsx` | localhost 导航前调用 prepare | 改为 Host 状态投影，保留显示 URL |
| `src-tauri/src/ssh_remote/skills_browser.rs` | URL parser、mux forward、dedicated `ssh -N -L` 已存在 | 抽出受监管 tunnel adapter，不重写 SSH |
| 同上 `tunnel_ports` | 仅缓存 `(alias, remoteHost, remotePort) -> localPort` | 身份、revision、owner、refcount 与健康状态 |
| 同上 `spawn_dedicated_forward` | `kill_on_drop(false)`，后台等待子进程 | owned handle、取消、EOF 与 teardown |
| `src-tauri/src/ssh_remote/acp_run.rs` | mac/Linux ControlMaster；Windows 独立 OpenSSH | 延续各平台真实限制 |
| `src-tauri/src/acp_client.rs` | SSH `session/new/load` 路径使用空本地 MCP 列表 | 回归保留；M5 明确 unsupported |

已有 200 ms 子进程存活检查不等于远端应用健康，也不证明 tunnel 始终有效。
所有新增对象和新 API 均是本计划的未来产物。

## 模块与接口

| 模块 | 职责 |
| --- | --- |
| `src-tauri/src/ssh_remote/browser_tunnel.rs` | 抽取现有 OpenSSH helper，扩展 `ssh -W` 私有 stdio channel |
| `src-tauri/src/browser/ssh_listener.rs` | Host/Guard 双持有 loopback listener，单点故障 tombstone 与 channel 配额 |
| `src-tauri/src/browser/ssh_bridge.rs` | Browser ownership、tunnel registry、状态与生命周期 |
| `src-tauri/src/browser/ssh_origin.rs` | transport scope、remote/display/actual origin 映射 |
| `src-tauri/src/browser/ssh_bridge_tests.rs` | 跨会话、端口复用、重连、取消故障测试 |
| `src/components/browser/ssh/bridge.ts` | 新 Host 接口与只读投影 |
| `src/components/browser/ssh/SshBridgeStatus.tsx` | 连接/取消/重连/失败/关闭状态与无障碍交互 |

```ts
type SshBridgeSnapshot = {
  tunnelId: string;
  tunnelRevision: number;
  browserSessionId: string;
  tabId: string;
  displayUrl: string;
  actualUrl: string | null;
  state: "preparing" | "ready" | "disconnected" | "closing" | "closed" | "failed";
  reasonCode: string | null;
};
```

新增可信 App UI command 为 `browser_ssh_prepare(browserSessionId, tabId, displayUrl)`。
Host 从 AppSession/project binding 取得 SSH alias；UI 不能把任意 alias 绑定到别人的 tab。
兼容 `sshBrowserPrepare` 入口在迁移期校验相同 owner，最终委托新 Bridge；不能保留不带 owner 的旁路。
`tunnelRevision` 是 u32；每次重连/替换递增，溢出换新 tunnelId。
正常 Browser command 仍使用共用 `RequestEnvelope`，不允许调用者自报可信 remote origin。
tunnelId/revision 由 Host 放在 `BackendBinding` 的 transport scope 中，随 binding generation 校验。
返回仍遵循 `{status, requestId, reasonCode, tabRevision, navRevision, artifactIds}`；BridgeSnapshot 是 Host UI 投影。

## 创建与连接流程

1. 用户在 SSH 项目打开 URL，Host 验证 AppSession、BrowserSession、tab 和项目关系。
2. 使用结构化 URL parser 识别 http/https 与明确 loopback；拒绝 userinfo、非法端口和非网络 scheme。
3. public HTTPS 保持本机普通导航；localhost、127.0.0.1、`::1`、`.localhost` 和现有 0.0.0.0 别名按兼容规则规范化。
4. Host 建立 tunnelId，冻结 endpoint、配置/认证摘要和已验证 host-key identity，准备独立网络/存储隔离域。
5. Host 创建 loopback listener 并保留原句柄，ProfileGuard 经明确移交持有副本；每条连接通过 OpenSSH `-T -W` 私有 stdio 到远端 endpoint。
6. 只有 listener ownership、受监管 channel 和远端连接 probe 全部成立才进入 ready；不再让 Browser 直连裸 `-L` 端口。
7. 创建新的 backend binding generation；导航成功后由实际页面确认 origin，才允许单独申请 grant/lease。

端口随机分配只用于传输，不作为权限身份；分配冲突有限重试三次，每次保留未 ready 状态。
`ssh -T -W` 不启动远端 shell，alias 和 remote endpoint 都是独立 argv，禁止拼本机 shell 命令。
OpenSSH host-key 不匹配/首次未信任必须返回明确原因，引导用户通过既有终端流程处理。
不自动写 `known_hosts`，不把 StrictHostKeyChecking 改为 no/accept-new。
Bridge 不复用现有按 alias 命名的 ControlMaster。专属 mux 的键包含目标 hostname/user/port、跳转路由/配置/认证摘要、已验证 host key、tunnelId/revision。
专属 master 自身配置 StrictHostKeyChecking、ServerAliveInterval=5、ServerAliveCountMax=2、ControlPersist=no，并由本 run supervisor 持有。
只有 master 的生存身份、实际配置与 host-key 归属可核对时才复用其 channel；不能核对时走无 mux 的 dedicated `-W`。
配置/认证摘要只含受控配置或公开指纹的摘要，不能把私钥、agent secret 或凭据内容写进缓存键/证据。
远端服务未启动、连接被拒绝或超时与 SSH 认证失败分开呈现。
支持 Windows 原生/Git OpenSSH 与 macOS OpenSSH；remote target 限 POSIX SSH host。

## Origin、授权与端口复用

Host 同时保留 `displayOrigin`、当前实际 `canonicalOrigin` 和不可伪造的 transport scope。
授权匹配键为 `AppSession + BrowserSession + AgentBinding + ProfileRef + canonicalOrigin + tunnelId + tunnelRevision`。
同端口、同 URL、同 SSH alias 都不能继承另一个 tunnel 或 revision 的 OriginGrant。
OpenSSH 配置摘要改变、host alias 重绑定、进程重建或重连均更换 transport revision/binding generation。
该变化原子撤销 OriginGrant、ControlLease、UploadGrant 与观察，清空旧队列，重新授权。
最初导航、重定向、popup、iframe、WebSocket 和 Network 内容来源均与当前 scope 重新匹配。
远端链接若跳到不同端口，不自动把该端口视为同一授权；必须准备新 tunnel 并确认对应 origin。
公开网站跳转按正常 origin policy；不继承远端 localhost grant。
每个 live tunnel revision 使用独立临时 Profile/网络存储域，不复用另一个 revision 的 Cookie、Service Worker 或 cache。
Preview 后端若不能证明独立数据存储和销毁，返回 `unsupported` 并允许显式升级到 Managed 隔离域。
断线先把前置 listener 变为拒绝连接的 tombstone，关闭 stdio channels，再停止旧页面；隔离域与旧 descendants 消失后才释放监听句柄。
Host 单点死亡时 Guard 持有副本并经 EOF 执行 tombstone/清理；Guard 单点死亡时 Host 持有原句柄并执行同一流程。
存活方保持 socket 到旧页面、channels 和受监管 descendants 全部退出，再关闭最后一个句柄；禁止先释放端口再异步清理。
双句柄仅由明确的 Host/Guard 持有，worker/Chromium/SSH 不得意外继承；一个进程不同时读取两份 listener。
Browser 只访问该前置端口；后端为私有 `ssh -W` pipe，没有可被复用的第二个 loopback TCP 端口。
单点故障的验收目标为旧页面向替代服务披露请求为零；in-flight 仍可为 unknown_outcome，不重放。
Host 与 Guard 同时死亡、OS 句柄失效等多点故障不能推导出绝对无竞态；无清理/端口归属证明的故障模型保持 No-go，不作无条件保证。

## TLS、协议与数据限制

当前 URL 重写到本机 loopback，HTTPS 证书名或本机 dev server allowedHosts 可能不匹配。
匹配实际访问名且已信任的证书可以使用；证书错误保持浏览器标准阻止，不启用 ignoreHTTPSErrors。
不通过替换证书、导入根证书或截获用户密码“修复”远端 HTTPS。
http 与同一 origin 派生的 ws、https 与 wss 使用相同 tunnel；不透明代理任意协议。
OAuth 回调、多端口 HMR、absolute localhost 链接必须通过声明 fixture 才算支持。
不支持的跳转显示原因与人工访问选择，不谎报远端页面已经成功连接。
上传只接受本机批准的 UploadGrant；远端文件需要单独的文件读取/下载授权流程。
Artifact 在本机 capability store 生成，内容脱敏、会话保留与导出确认沿用 M1。
SSH 配置、主机名、用户名、远端路径和 stderr 以摘要或脱敏片段入日志，不写私钥或认证正文。

## 生命周期、故障与恢复

registry 保存 owner/refcount、run nonce、PID/startTime、listener ID、channel IDs、last health 和 tunnelRevision。
POSIX 的 `ssh -W` channel 只可复用完整 identity 匹配的 Bridge 专属 master；Windows 或 identity 不可核对时使用独立 child。
ProfileGuard 监管 channel 子进程与 listener；不能继续使用 fire-and-forget，也不能继承裸 `-L` 缓存。
每 Bridge 最多 16 个并发 stream、App 最多 64 个；每 stream 双向 buffer 各 64 KiB，建立超时 10 秒。
超额连接关闭并发 Host `ssh_stream_capacity` 事件；TLS 原样传递，不向 TLS stream 注入明文 HTTP 错误。
cancel prepare、关闭最后引用、BrowserSession 关闭或 Host exit 按 tombstone、撤销、关 channels/页面、退出证明、释放 listener 的顺序执行。
Bridge 专属 master 在自身 channels 清零后按 run nonce 关闭；既有 watch/terminal ControlMaster 和远端 leader 不受影响。
keepalive 必须由承载网络连接的 master 或 dedicated child 实际生效；仅给复用 client 传入参数不算通过。
网络黑洞检测上限为配置的 15 秒；UI 在 1 秒内反映收到的断线事件，Agent lease 不跨断线续期。
重连是用户显式命令，创建新 revision、新 binding、新隔离 Profile；登录态不自动继承。
当前有副作用操作时返回 `unknown_outcome`；原 requestId、lease、fence 与 queue 均不得重放。
如果取消与 ready 竞态，取消胜出：最终 closed 且无 tab 导航、无残留 forward。
清理失败保持 `cleanup_failed`，保存 ledger 与 tombstone listener，拒绝复用，提供重试与诊断。
重新启动 Host 先 reconciliation 核对旧 run；不能只靠相同端口或进程名判定旧 tunnel。

## Remote ACP 边界与回滚

远端 ACP 的 `session/new`、`session/load`、fork、热更新 MCP 路径继续省略本机 Browser MCP。
远端 agent 请求本机浏览器控制得到 `unsupported` 和 `ssh_remote_agent_control_unsupported`。
用户从远端项目打开页面，或经本机合法 AgentBinding 操作该本机 BrowserSession，可以走受授权 Bridge。
展示远端来源不等于授予远端 agent 控制；本机连接凭据不能发给远端 shell。
禁用 M5 flag 时先撤销全部 bridge lease、停止导航/观察并完成清理；普通 public Preview 不受影响。
回滚到旧 App 不导入 M5 隔离 Profile/ledger 为旧端口缓存；先证明旧 forward=0。
旧客户端发现新 schema 只读保留并提示不兼容，不能 wipe 后重建以绕过 ledger。

## 验收与证据

macOS arm64/x64、Windows x64 的最终包各连续三轮完整流程 100% 通过；Linux 按 M4 已放行行增加。
至少覆盖远端 IPv4/IPv6、public HTTPS、HTTP/WebSocket、证书拒绝、服务缺失、多端口 unsupported。
两个 SSH host 同 remote port、两个 BrowserSession、端口强制复用各至少 30 次，无 grant/Cookie 继承。
取消/ready 竞态、SSH kill、Host-only kill、Guard-only kill、App exit 各至少 30 次，30 秒后 owned channel/listener/master=0。
至少 100 次单点故障下后台 reconnect/端口抢占；另测同 alias 改目标、旧无 keepalive master、网络黑洞 <=15 秒检测。
多点故障单列 scope、实际结果和 No-go，不与单点通过结果合并；声明支持的场景中 Cookie/请求泄漏为零。
跨 owner、过期 revision、旧 binding/lease/fence、伪造 origin 与 remote ACP 本地 MCP 泄漏均为零。
Host 到输入 fence ACK 的 M1 指标继续有效；远端网络延迟不改变本机接管安全门。
成功创建 tunnel 的参考局域网 p95 <= 5 秒；至少 30 样本，并单列 RTT 与服务启动时间。
所有证据保存 `docs/qa/browser-rearchitecture/m5/`，目前未执行；M5-06 是唯一完成出口。

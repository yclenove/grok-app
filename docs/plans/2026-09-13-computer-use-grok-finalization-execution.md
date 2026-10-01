# Computer Use 收尾执行书

> 日期：2026-09-13
> 工作区：`H:\aicoding\grok-app-computer-use`
> 分支：`feat/computer-use-implementation`
> 当前基线 HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`
> 最大执行窗口：48 小时；目标是连续推进真实开发和验证，不是用计时或空跑填满窗口
> 配套审计：`2026-09-13-computer-use-grok-post-48h-audit.md`

本文件是给实现者的执行顺序和验收合同。它不是“已完成”声明。实现者必须在当前 dirty worktree 上工作，保留已有成果和未提交修改，并在每一批结束时写出可重放证据。

## 1. 最终目标

把当前“有底层骨架和若干 probe”的 Computer Use，收敛为真实产品链：

```text
用户设置开启
  -> 选择明确的产品 surface
  -> Host 完成目标发现与用户授权
  -> session MCP 只注入该 session 的能力
  -> observe -> act -> verify
  -> stop/revoke/关闭时可靠清理
  -> 诊断、隐私、安装、升级和回退可验证
```

首版目标包含 Windows、macOS、Linux X11 和 GNOME Wayland。没有目标 OS 的实机时，代码和隔离测试可以继续做，但该 OS 必须标为 `not_run` 或 `blocked_external`，整体不能写成跨平台完成。

## 2. 完成谓词

### 2.1 产品完成

只有以下条件全部成立，才允许在最终报告写 `passed`：

```text
F0..F13 的 required 子项全部 passed
AND 当前 code fingerprint 在 freeze 后没有变化
AND F14 的每个 required scenario 都有独立计数、独立后置条件和有效日志
AND 安全 invariant、错误清理 invariant、敏感数据扫描均为 0 breach
AND Windows E5、macOS E5、Linux X11 E5、GNOME Wayland E5 都有真实目标环境证据
AND 真实模型 E4 已获明确批准并通过；或报告明确保留为 not_run，整体标 partial
AND 最终 inventory 的日志路径、大小、hash、fingerprint 全部一致
```

### 2.2 本机可交付完成

如果本机只有 Windows，最多可以得到：

```text
Windows local delivery = F0..F14 本机项通过
overall product = partial（其他 OS 或真实模型仍 not_run/blocked_external）
```

本机交付完成不能被改写为“Computer Use 已完成”或“支持三 OS”。

### 2.3 不算完成的情况

- 只有 probe、fixture、FakeAdapter、jsdom 或 scripted agent 通过；
- 只有源码模块，没有生产 UI/Host/MCP 调用点；
- 退出码为 0，但没有验证真实后置条件；
- 一个 lane 的其他场景成功清零了失败场景；
- 代码在 freeze 后改过，却沿用旧 soak；
- `partial`、`blocked`、`blocked_external`、`not_run` 被汇总成 `passed`；
- 把 Windows、XWayland 或普通浏览器结果复制到 macOS/Linux/GNOME Wayland；
- 用 `--always-approve`、测试后门、宽泛 `allow` 或跳过质量门禁换绿；
- 只写报告，没有保留可重放原始证据。

## 3. 红线

### 3.1 Git 和工作树

- 禁止 `git reset`、`git clean`、`git restore`、`git checkout --`、stash、强制切分支或重建 worktree；
- 禁止 `git add`、commit、push、PR、merge、tag、release；
- 不覆盖或删除用户已有文件；任何清理只允许删除本次 run 创建且有 ownership marker 的目录；
- 不修改 `H:\aicoding\grok-app` 或其他 worktree；
- 不把 ignored runtime/evidence 误加入 Git。

### 3.2 用户数据和外部系统

- 不读取、导出、打印或复用账号 Token、Cookie、浏览器 storage、系统凭据或共享 `~/.grok`；
- 不打开或控制用户日常 Chrome/Edge、ChatGPT、微信或其他未选择窗口；
- 测试只使用独立 `GROK_APP_HOME`、独立 identity 和隔离 `--user-data-dir`；
- 不改变代理、VPN、路由、账号配置或正式安装；
- 不启动真实模型 E4，除非用户另行明确批准；
- 不向外部服务发送测试数据或外部消息。

### 3.3 实现质量

- 不用 Fake/mock/stub 作为产品 E3/E4/E5 证据；stub 只能标明 fixture；
- 不用任意 `eval`、PATH Node、系统浏览器自动探测、在线 `latest` 或测试源码目录作为 production 依赖；
- 不新增生产测试后门；不吞错、不 `unwrap` 生命周期失败、不降低安全换速度；
- Computer Use 默认关闭，且独立于 YOLO、acceptEdits 和 normal permissions；
- 不向 `App.tsx` / `AppWorkbench.tsx` 增加大块状态，二者合计行数只能持平或减少；
- 所有 UI 文案走 15 个 locale，禁止硬编码；
- 不用原生 select、confirm、prompt、alert 或透明菜单。

## 4. 状态和证据合同

### 4.1 原子状态

每个 batch 和 scenario 只允许下列状态：

| 状态 | 规则 |
| --- | --- |
| `not_started` | 尚未开始 |
| `in_progress` | 正在执行，不能算完成 |
| `passed` | 当前 fingerprint 上所有 required postcondition 成立 |
| `failed` | 至少三次有依据的修复尝试后仍失败，保留首败 |
| `blocked_external` | 缺少本机外部环境且本机 ready 工作已耗尽 |
| `invalidated` | 后续代码、配置、runtime 或证据变化使原证据失效 |
| `not_run` | 没有运行 |

`partial` 只能用于汇总，不得用作原子 batch 的完成状态。

### 4.2 Evidence 根目录

每次收尾 run 使用唯一目录：

```text
tools/computer-use-probe/.run/finalization/<run-id>/
  baseline/
  checkpoints/
  failures/
  logs/
  metrics/
  reports/
  state.json
  manifest.jsonl
  owner.json
```

开始前必须用 `git check-ignore -q` 证明该路径被现有规则忽略。若不被忽略，先记录并停在 F0，不能把大日志写进版本控制。

`owner.json` 至少记录 run id、UTC/local 时间、canonical repo、branch、baseline HEAD、OS/arch、writer PID/session、evidence schema 和允许清理的绝对路径白名单。

### 4.3 Manifest

每条命令一行 append-only JSON，至少包含：

```json
{
  "schemaVersion": 1,
  "seq": 1,
  "phase": "F1",
  "batch": "F1.2",
  "scenario": "revoke-from-tokio",
  "kind": "command",
  "argvRedacted": ["cu_probe", "appshell-revoke"],
  "cwd": ".",
  "startedAt": "...",
  "endedAt": "...",
  "durationMs": 0,
  "exitCode": 0,
  "status": "passed",
  "evidenceLevel": "E2",
  "log": "logs/F1.2-0001.log",
  "sha256": "...",
  "bytes": 0,
  "codeFingerprint": "...",
  "postconditions": ["no_panic", "worker_exit", "stop_state=stopped"]
}
```

规则：

- `argvRedacted` 不得含 secret、Token、Cookie 或完整用户路径；
- 日志写完后立即计算 hash/size；
- 每次尝试使用新路径，禁止覆盖旧日志；
- 修正失败只能追加新记录，不能原地改写旧记录；
- `state.json` 用临时文件加同卷 rename 原子更新；
- 每个 batch 结束和至少每两小时写 checkpoint；
- 最终逐条检查 manifest 的路径、hash、size 和 fingerprint，任一不一致就不能 passed。

### 4.4 Fingerprint 和失效

fingerprint 必须覆盖 HEAD、tracked diff、nonignored untracked source/config/docs 的清单与 SHA-256、lock/manifest digest；排除 ignored evidence、target 和 runtime cache。任何 production、test、probe、runner、extension 或配置变化都使其后的 freeze/soak 证据 `invalidated`，必须重新 F12/F13/F14。

## 5. 总体依赖图

```text
F0 现场/证据真值
 -> F1 async stop/revoke 安全
 -> F2 typed surface router
      -> F3 Managed Browser 产品闭环
      -> F4 WebView 产品闭环
      -> F5 Existing Tabs 配对协议
           -> F6 extension transport
           -> F7 Host/UI grant/return
 -> F8 MCP/session/事件闭环
 -> F9 跨平台 capability/backend
 -> F10 诊断、隐私、清理
 -> F11 runtime/package/install
 -> F12 质量债和测试隔离
 -> F13 branch-built/installed acceptance
 -> F14 freeze + scenario-keyed active soak + final gates
```

只有一个 writer 时，runtime seed、浏览器、Tauri App 和 installer 相关重型任务串行执行；不要并发移动或修复同一 generated seed。

## 6. 分批执行

### F0：重新建立真值和安全现场

**目标**：把旧“passed”降级为历史输入，建立本次唯一证据根和 Red 基线。

**必须做**：

1. 读取 `AGENTS.md`、`docs/llm-wiki/computer-use.md`、现有 execution-state、post-48h audit 和本执行书；
2. 记录 canonical path、branch、HEAD、dirty tracked/untracked/ignored 统计、磁盘余量、OS/arch、现有 writer；
3. 创建并验证本次 evidence root、`owner.json`、初始 `state.json` 和 manifest；
4. 在隔离 App-shell 上重现撤销/停止路径，保存 panic 或明确的 Red；
5. 计算 baseline fingerprint。

**门禁**：现场没有被清理或覆盖；旧 D13/D14 明确标 `stale`；Red 有原始日志；没有把旧报告复制为新通过。

**不能写成 passed 的情况**：只读取旧报告、只运行静态 grep、没有确认当前 fingerprint。

### F1：修复 async stop/revoke 和生命周期崩溃（P0）

**目标**：任何从 Tokio async、Tauri command、App-shell、IPC、session revoke 或进程关闭触发的停止都不 panic、不挂死，并保持 fail-closed。

**实现要求**：

- 选择并记录一个明确架构：把同步 HTTP/进程清理放到专用 blocking worker/队列，或把 worker client 全部改成真正 async；不能依赖在任意 async 上下文调用 `reqwest::blocking`；
- 不在持有 Broker/session 锁时 await；停止请求、清理进行中和最终 `stopped` 三个状态必须可区分；
- timeout 后返回 `unknown`/`stop_requested`，不得自动重放副作用；
- revoke 必须撤销 token、停止周期预览、取消浏览器 worker、释放 lease，并在失败时保留可诊断状态；
- App 退出和重复 revoke 必须幂等；
- 任何 panic 都必须成为测试失败，不能用 panic hook 掩盖。

**Red/Green 场景**：

- authorize managed tab → active request → revoke from Tokio worker；
- `computer_stop`、session close、App-shell finish、Tauri window close；
- worker 已退出、worker 只响应超时、malformed response、重复 stop/revoke；
- Desktop、Managed Browser、Existing Tab、WebView 各至少一轮；
- 进程树、lease、临时 profile 和 IPC token 最终清理。

**required evidence**：Rust regression test、隔离 branch-built App-shell E3、panic log absence、child-process/handle cleanup、重复运行至少 20 次。

**通过条件**：0 panic、0 unrecovered worker、0 post-stop dispatch、所有 stop 状态和错误码符合协议。任何一个生命周期场景失败，F1 为 `blocked`，不能继续宣称 App-shell 通过。

### F2：建立真正的 typed SurfaceRouter（P0）

**目标**：surface 从 slash/UI 到 Host/Broker/MCP 全程保留，绝不把 managed profile、existing tab 或 WebView 误交给 Desktop adapter。

**实现要求**：

- 用一个明确的 `SurfaceRouter`/等价 Host 组件集中映射 `desktop`、`managed-browser`、`existing-tabs`、`app-webview`；
- command request、target DTO、authorization ticket、session binding、事件和诊断都带 surface；
- profile picker 不得直接调用通用 `authorize_target`；必须先由 Host 打开 profile，再对返回的 tab 授权；
- WebView 和 Existing Tabs 不得通过 Desktop fallback；dead target 只能拒绝；
- 未知 wire surface 返回明确 `unsupported_surface`；
- 同一 session 不能跨 surface 复用旧 target/generation。

**required evidence**：每个 surface 的正向路由、每个错误路由的负向测试；从 slash `computer-use` / `computer-use-browser` 到实际 panel surface 的 production call-site 检查；一次真实 App command trace。

**通过条件**：所有面都能证明 route identity；没有仅凭字符串存在的测试；测试中若没有生产调用点则只能 `partial`。

### F3：Managed Browser 产品闭环（P0）

**目标**：用户在面板选择 managed profile 后，能真正获得一个 App-owned tab，完成 run-scoped authorize，注入 MCP，执行并停止。

**实现步骤**：

1. Host 提供 profile 列表、创建/打开、关闭和状态事件；
2. UI 选择 profile 时调用 typed `open_managed_profile`（或等价 Host API），拿到真实 tab id/generation；
3. 只有用户确认后才 `authorize`、`activate`、`SessionManager::attach_computer_use`、`complete`；
4. 成功后 model 只能看到该 run 的 tab，不能看到 profile 根目录或其他 sessions；
5. observe/act/navigate/download/stop 全走同一 router；
6. bind/open/authorize 任一步失败，执行 `sessions::fail`、关闭新 tab/profile（按归属）、清理 token；
7. stop/revoke/compact/model switch 都需要重新 observe/re-authorize。

**required evidence**：branch-built App + isolated home 的 UI/Host scripted E3，独立页面 oracle，至少 20 次 create/authorize/observe/act/verify/stop；双 session 不串；App-shell 退出无残留。

**禁止**：用 `mcp_scripted_agent_run.rs` 单独调用 `open_managed_profile` 作为产品证明；用 FakeAdapter 代替真实 App chain。

### F4：App WebView 产品闭环（P0）

**目标**：side browser/WebView 是一个真正由 Host 注册并受 Broker 管理的 typed target。

**实现步骤**：

- `bind_webview` 必须完成 ticket begin、typed bind、target registration、target authorization、activate、session MCP attach、complete；
- 任一失败必须 fail + unbind + revoke，不能遗留 `authorization in progress`；
- `list_targets_for_surface(WebView)` 返回当前 product WebView target，而不是固定空列表；
- observe/act 只允许白名单 typed operations；禁止任意 JS/eval、跨应用 cookie/storage 复用；
- hidden panel 停止 preview polling，显示回来只接受当前 session/run/generation 的帧；
- stop/revoke/close 清理 WebView listener、target、lease 和 token。

**required evidence**：真实 Tauri side-browser/WebView product smoke；至少 20 次 bind/authorize/observe/act/unbind；cross-origin iframe、permission UI、复杂 download 等 unsupported 场景有明确错误；失败中断后下一次 bind 能成功。

### F5：Existing Tabs 安全配对协议（P0）

**目标**：把“用户明确分享的 Chrome/Edge tab”安全地交给 Host，建立可撤销、可轮换的连接；不把 loopback 当成天然可信。

**先做 ADR**：在第一小时内记录采用的 transport（推荐 MV3 service worker + authenticated loopback WebSocket/HTTP2；也可选择受控 native messaging），并说明为什么不把 secret 放在 URL。

**不可妥协的协议属性**：

- 未认证 GET、URL、地址栏、历史、普通日志、崩溃报告中永远没有 pairing secret 或 session key；
- challenge 只有一次性 nonce、过期时间、版本和 public metadata；
- 精确校验稳定 extension identity（固定 key/发布 fingerprint），不能接受任意 `chrome-extension://`；
- App 和 extension 都必须有明确用户确认；服务端不能仅因收到请求就自动 `confirm_pairing_extension`；
- 认证结果通过受保护通道交付，session key 只在 extension/Host 必要范围内短期存放，revoke/rotate/restart 后失效；
- nonce、instance、extension id、连接 generation、document generation 均参与签名/绑定；
- CORS、CSRF、Origin、Host header、forwarded header、速率限制和 replay reject 有独立测试；
- pairing 失败、断线、浏览器退出和 App 退出会撤销旧 token。

**建议流程**：App 创建短时 challenge 并打开只含 public nonce 的 pairing page；extension service worker 读取 challenge，显示 code/fingerprint 并等待用户确认；双方完成密钥协商或等价认证后建立单独 session transport；任何 tab share 仍需用户在浏览器和 App 两侧确认。

**required evidence**：恶意 loopback client、伪造 extension origin、重放旧 nonce、错误 extension id、过期 challenge、断线重连和 revoke/rotate 的负向测试；日志 sentinel 0 secret hit。

### F6：Extension transport 和 tab 能力

**目标**：extension 真正能够在用户明确分享的 tab 上枚举、observe、act、发事件和归还。

**实现要求**：

- MV3 service worker 负责连接、认证、命令队列、心跳和断线；content script 只做最小页面桥接；
- 权限最小化，避免无理由的 all-host/all-tabs；需要调试或 optional host permission 时说明用户可见影响；
- 用户点击 Share 时才提交 tab metadata；model 永远不能直接枚举用户全部 tabs；
- tab record 必须含 tab id、browser/profile identity、origin、document generation、connection generation、focused/home snapshot；
- observe 返回可验证页面/截图和 generation；act 只接受最近 observation 的 id；
- navigation、close、reload、focus 变化、permission denied 和 disconnect 都发事件并使旧 observation 失效；
- stop/revoke 后 detach、归还 tab 位置/焦点（能力允许时）并删除 run grant；
- 下载写入 run-owned staging，不能接受模型提供的任意路径。

**required evidence**：Chrome for Testing 和 Edge（若本机没有 Edge，标 `blocked_external`）各自的真实 fixture；5 个 tab 生命周期、双 session 隔离、断线/重连、旧 generation 拒绝、observe→act→verify；不把 unpacked fixture 通过写成安装完成。

### F7：Host/UI 配对、picker grant 和归还

**目标**：把 F5/F6 transport 接入真实 App 面板，形成用户看得见、能取消、能收回的工作流。

**实现步骤**：

- 配对面板显示安装状态、连接状态、challenge expiry、extension fingerprint 和错误原因；
- 提供打开 pairing page/安装说明的真实按钮；不能只 begin challenge 后静默等待；
- App 确认与 extension 确认是两个不同状态，状态变化可恢复；
- Host 只把用户明确分享的候选 tab 放进 picker；选择后调用 `share_and_grant`/等价 typed API；
- grant 绑定 session/run，不能全局 current tab；
- stop、revoke、切换 session、compact、logout、extension disconnect 都归还并清理；
- UI 处理 busy、empty、expired、denied、stale、retry、hidden/visible 和 keyboard focus；所有文案进 15 locale。

**required evidence**：真实 App UI 操作或可审计的 Tauri command trace；不存在“按钮可点但无 transport”的占位路径；面板隐藏时无旧 preview 闪回。

### F8：MCP/session/事件闭环

**目标**：模型工具面只负责请求观察和动作，Host/UI 保留授权、暂停、接管、恢复和配对权限。

**实现要求**：

- session MCP 注入仅发生在授权完成后，并带 surface/run/target scope；
- `computer_status/list_targets/open_target/observe/act/wait/request_handoff/stop/navigate/download` 的输入输出与实际 backend 一致；
- authorize、resume、reconnect、pairing 和系统权限不暴露为模型可自批工具；
- 每个 act 都验证 observation id、snapshot/generation、in-flight lease 和 cancellation；
- timeout 返回 unknown，必须重新 observe，不能用同一 action id 重放副作用；
- tool result 包含 image content、outcome、verification、reason 和 sanitized trace；
- fork、压缩、换模型、session 恢复后重新授权；不为每个动作创建新聊天会话。

**required evidence**：真实 session MCP stdio → loopback/Bearer → Broker → 真实 product backend 的 scripted E3；独立 oracle、双 session 不串、host-only 拒绝、token/trace 脱敏。

### F9：四个目标 OS 的 capability 和 backend

**目标**：让 capability truthful，并完成首版要求的 native backend；没有实机时仍不能虚报。

**Windows**：保留现有 Win32/UIA 和 runtime 优势，补齐 App-shell/安装版 E3/E5、DPI/多屏/焦点/中文键盘、click/type/key/scroll/drag 的真实后置条件。

**macOS arm64/x64**：

- 完成 CGWindow/AX/CGEvent 的 key、scroll、drag、type 和 cancel；
- Screen Recording、Accessibility、TCC 缺失时给出可操作诊断；
- 验证 Retina、多屏、坐标转换、签名/未签名安装和权限恢复；
- 未在两种架构实机验证前，状态只能 `not_run`/`blocked_external`。

**Linux X11**：

- 完成 X11 screenshot、XTEST click/key/type/scroll/drag、窗口存活和焦点校验；
- 明确依赖 libX11/XTest/桌面 session，不能用 browser fixture 代替；
- 在真实 X11 session 做 E3/E5。

**GNOME Wayland native**：

- 实现并验证 portal ScreenCast/RemoteDesktop 与 EIS/libei（或经过 ADR 批准的等价 native helper）；
- 权限、用户选择、PipeWire stream、坐标/DPI、停止和断线均可诊断；
- `native_wayland` 在 helper 未真实验证前必须是 false；XWayland 绝不能算 native Wayland。

**runtime**：为每个发布 target 建精确 lock/digest/resource map；不得把 Windows seed 塞到其他 target，也不得在线下载未锁定的 latest。

**required evidence**：每个 OS 的编译、隔离 fixture、真实 App、安装和任务矩阵分开记录。缺机器时继续完成本机可做项，并写恢复条件，不得因 blocker 停掉整个 DAG。

### F10：诊断、隐私和清理

**目标**：可诊断但不泄露敏感数据，可导出、可清理、可证明不越界。

**实现要求**：

- trace、run staging、managed profile、extension pairing state 分目录隔离；
- retention 同时受 record 数、总字节数、最大年龄限制；
- support bundle 只包含脱敏 metadata、错误码、时序和版本，不含 token/cookie/页面敏感截图，除非用户逐项确认；
- 导出压缩后再次逐文件 leak scan；
- cleanup 验证绝对路径在 App-owned root 内，拒绝 symlink/junction/path traversal；
- 清理、revoke、退出和 crash recovery 都是幂等；
- 诊断面板显示真实状态，不硬编码 running/success。

**required evidence**：sentinel secret scan、压缩后二次 scan、越界路径负测、crash/kill 后清理、p50/p95 observe/act/stop timing 和资源趋势。

### F11：runtime、extension、bundle 和安装生命周期

**目标**：用户安装后真的拿到所需 runtime、extension 说明和可修复/可回退的布局。

**实现步骤**：

1. 将 extension source/manifest/service worker/许可证和安装说明纳入目标 bundle；
2. 固定 extension identity，记录版本和 fingerprint；
3. Windows NSIS/portable 逐文件审计，确认 Node、Playwright、Chromium、extension、配置和 sidecar 路径；
4. 执行 clean install、首次启动、repair、same-version tamper repair、upgrade、rollback、uninstall；
5. 验证安装后不依赖仓库源码、PATH、用户 profile 或网络 latest；
6. macOS/Linux bundle 采用各自 lock/resource map，不能凭 Windows package 通过；
7. 若 Chrome/Edge 商店签名或真人安装环境缺失，记录 `blocked_external`，但仍完成本地 bundle audit 和安装说明。

**required evidence**：真实产物 hash、逐文件 inventory、隔离安装目录、进程/文件 cleanup、rollback 后旧版本启动；NSIS 内不能包含 probe、测试后门或用户数据。

### F12：质量债和测试隔离

**目标**：让编译器重新暴露断开的集成，消除本轮新增质量回退。

**必须处理**：

- 修复 `tests_product_surface.rs:50` 的 Clippy 失败；
- 收敛 `cargo check -p grok-app` 的 dead-code warning，缩小或删除顶层 `allow`；
- 拆分 Computer Use 新增超过仓库预算的大文件，只做必要的领域拆分；
- 移除不必要的 `(item: any)`，恢复正确类型；
- 保持 `App.tsx` / `AppWorkbench.tsx` 行数不增长；
- 所有 locale key 与 `en` 同步；
- core、browser、App-shell、extension、runtime、installer 测试不得共享可变 seed，使用独立副本、锁和 ownership marker；
- 测试失败不能通过 skip、弱断言、忽略 warning 或改名隐藏。

**required gates**：`git diff --check`、`pnpm typecheck`、`pnpm lint`、`cargo fmt --check`、core clippy `-D warnings`、core tests、frontend tests、runtime check、bundle audit、locale parity、dependency check；每项都记录版本和 exit code。

### F13：当前 fingerprint 的产品验收

**目标**：在 freeze 前证明真实生产调用链，而不是继续增加 probe 数量。

**Windows local E3/E5 顺序**：

1. branch-built App + isolated `GROK_APP_HOME`；
2. 设置默认关闭检查；
3. Desktop：目标选择、授权、observe/act/verify、pause/takeover/resume/stop；
4. Managed Browser：profile → tab → authorize → MCP → navigation/action/verify → stop；
5. WebView：bind → authorize → MCP → typed action → unbind；
6. Existing Tabs：pair → explicit share → picker grant → observe/act → disconnect/revoke；
7. 关闭、重启、worker kill、malformed response、stale target、DPI/focus 变化；
8. 逐项检查进程、lease、profile、token、trace 和 staging 清理。

每个 surface 至少 20 次成功循环；每次都要求独立 oracle，而不是只看 tool response。真实模型 E4 只有在用户批准后执行；没有批准就明确 `not_run`。

### F14：代码冻结、按场景长稳和最终门禁

**前置**：F0-F13 required 本机项通过，生成 freeze fingerprint；之后任何生产/test/runner/config/runtime 变化都停止 soak 并回到 F12/F13。

**最低主动长稳**：代码冻结后至少 12 小时真实 workload；不包含睡眠、空循环、重复相同静态命令或只读取报告。若实现阶段耗尽窗口，不得缩短成几分钟并写 passed。

**场景必须独立计数**（建议至少）：

| scenario key | 最低主动墙钟 | 最低有效轮数 | 关键 invariant |
| --- | ---: | ---: | --- |
| `desktop-appshell` | 2h | 60 | no panic、no post-stop dispatch |
| `managed-product` | 3h | 100 | real profile→tab→MCP chain、no cross-session |
| `webview-product` | 2h | 100 | typed target、unbind cleanup |
| `existing-tabs-product` | 2h | 100 | authenticated transport、return/revoke |
| `fault-stop-restart` | 2h | 80 | no orphan、unknown not replayed |
| `privacy-runtime-package` | 1h | 60 | 0 secret leak、bounded disk |

墙钟总和至少 12h；轮数只是最低样本，不得用迭代耗时总和代替主动墙钟。

**统计规则**：

- scenario key 独立保存 `rounds/ok/fail/consecutive_fail/active_wall_s`；
- 一个场景的成功永远不能清零另一个场景的失败；
- required scenario 缺失、被轮转移除、永远失败或只运行了 0 轮都直接不通过；
- 安全类 invariant（错误目标、越权写入、secret leak、post-stop dispatch、未回收 worker/lease）任一非零即失败；
- 普通传输瞬时失败必须保留首败、分类原因并修复后重新跑该 scenario；不能把历史失败从分母删除；
- wrapper 被杀、机器重启或证据丢失后，连续性只从新的 freeze 重新开始；
- 不得 prune 唯一原始日志。可压缩，但必须保留原始 hash、首败和末次成功。

**最终门禁**：同一 fingerprint 上重新执行完整 required gate；生成 `reports/finalization-report.md` 和 `reports/finalization-summary.json`；逐条 inventory manifest。任何代码修改晚于报告生成时间，报告自动 `stale`。

## 7. 48 小时节奏

这是优先级窗口，不是强制浪费时间的计时器：

| 时间段 | 主要目标 |
| --- | --- |
| 0–2h | F0 真值、Red、evidence ownership |
| 2–8h | F1 async lifecycle |
| 8–13h | F2 router |
| 13–21h | F3 Managed + F4 WebView |
| 21–30h | F5 pairing + F6 extension + F7 Host/UI |
| 30–34h | F8 MCP/session、F10 privacy |
| 34–38h | F9 platform capability、F11 package、F12 quality |
| 38–40h | F13 当前 fingerprint product acceptance/freeze |
| 40–48h | 至少 8h soak；若前序更早完成，继续到总计至少 12h active soak（窗口允许时） |

如果 F1-F13 还未完成，不能为了赶时钟跳过关键批次或提前写“完成”。如果遇到真正外部 blocker，写清缺失条件、恢复命令和证据位置，同时继续执行其他 ready batch。48 小时结束时如仍有缺口，输出诚实的 `partial/blocked/not_run` 矩阵。

## 8. 停止与恢复规则

立即停止当前 scenario 并保留首败的情况：

- panic、死锁、长时间无 heartbeat、进程/浏览器无法回收；
- 目标越权、错误窗口/tab、旧 generation 被接受；
- secret/token/cookie 出现在日志、URL、截图或 support bundle；
- fingerprint 漂移、manifest hash/size 不一致、evidence root 越界；
- 运行影响用户正式 App、日常浏览器或共享 `~/.grok`。

停止不等于撤销用户已有改动。只终止本次 run 创建且已登记的子进程；保留失败证据，记录 ownership 和恢复路径。

## 9. 最终报告格式

最终报告必须包含：

1. run id、repo、branch、HEAD、freeze/current fingerprint；
2. 每个 F0-F14 原子状态和最高证据等级；
3. 每个 surface/OS 的 `validated/partial/stale/blocked/blocked_external/not_run`；
4. 每个 scenario 的 rounds、ok、fail、consecutive fail、主动墙钟、首败文件；
5. 安全/隐私/清理 invariant 计数；
6. package/install/upgrade/rollback/uninstall inventory；
7. 未完成项和外部恢复条件；
8. manifest 完整性结果；
9. 明确写“未 commit / 未 push / 未提 PR”。

报告不得只写一个总数或一个 `passed`。若整体不是产品完成，标题和结论必须直接写 `partial — not releasable`。

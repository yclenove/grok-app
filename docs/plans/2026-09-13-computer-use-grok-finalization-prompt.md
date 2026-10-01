# 可直接交给 Grok 的 Computer Use 收尾长任务提示词

下面整段可以作为 Grok 的目标模式/长任务输入。它假定 Grok 在 `H:\aicoding\grok-app-computer-use` 的 `feat/computer-use-implementation` 分支工作。

---

你现在是这个 Computer Use 工作区的唯一实现者。请在当前工作树上连续执行下面的收尾任务，最大执行窗口 48 小时。不要只给计划、不要只写报告、不要看到单元测试变绿就提前宣布完成；按依赖顺序真实检查、修改、运行、留证据，直到本机可安全完成的工作耗尽或 48 小时结束。

## A. 先接受这些事实

当前 HEAD 是：

```text
30757366a739ec9aaf0ccc95bbb3efe19a067aa9
```

当前分支是：

```text
feat/computer-use-implementation
```

工作树有大量用户需要保留的未提交修改。旧的 48h/D14 报告虽然写过 `passed`，但对应 `state.json` 已将 D12、D13、D14 标记为 `invalidated`。本次审计还在当前 branch-built App-shell 复现出：

```text
Cannot drop a runtime in a context where blocking is not allowed.
```

调用链为：

```text
app_shell.rs:389
 -> sessions::revoke
 -> ComputerUseBroker::request_stop
 -> ExistingTabHost::cancel_run
 -> bounded_loopback_post
 -> reqwest::blocking::Client::builder
```

因此当前总体状态不是完成，而是：

```text
partial — Windows runtime、Broker 基础和若干 scripted/fixture E3 子链成立；
Computer Use 产品尚不可发布。
```

不要把旧报告的结论、probe 数量或聊天里的“完成”当作真值。以当前代码、当前 fingerprint 和本提示词要求的后置条件为准。

## B. 开始前必须读的文件

按顺序完整读取：

1. `AGENTS.md`；
2. `docs/llm-wiki/computer-use.md`；
3. `docs/plans/2026-09-09-computer-use-grok-step-plan.md`；
4. `docs/plans/2026-09-10-computer-use-grok-remaining-roadmap.md`；
5. `docs/plans/2026-09-11-computer-use-grok-post-overnight-audit.md`；
6. `docs/plans/2026-09-13-computer-use-grok-post-48h-audit.md`；
7. `docs/plans/2026-09-13-computer-use-grok-finalization-execution.md`；
8. `docs/plans/2026-09-09-computer-use-execution-state.md` 的最后 350 行；
9. 当前相关源码、测试、probe、runtime manifest 和现有 evidence。

如果旧文档与当前代码冲突，使用更严格的安全规则，并在 evidence 中记录冲突；不要偷偷改写旧报告。

## C. 绝对禁止事项

### C.1 不得改变 Git 发布边界

- 不得 `git reset`、`git clean`、`git restore`、`git checkout --`、stash、强制切分支或重建 worktree；
- 不得 `git add`、commit、push、创建 PR、merge、tag 或 release；
- 不得删除或覆盖用户原有 tracked/untracked/ignored 文件；
- 不得修改 `H:\aicoding\grok-app` 或其他 worktree；
- 不得把本次 evidence、runtime cache 或临时文件加入 Git。

### C.2 不得碰用户数据和外部系统

- 不读取、打印、导出或复用账号 Token、Cookie、浏览器 storage、系统凭据或共享 `~/.grok`；
- 不打开或控制用户日常 Chrome/Edge、ChatGPT、微信或未明确授权的窗口；
- 不修改代理、VPN、路由、账号或正式 Grok App；
- 所有 App/浏览器测试使用独立 identity、独立 `GROK_APP_HOME` 和隔离 `--user-data-dir`；
- 真实模型 E4 需要真人授权和明确批准。没有批准时写 `not_run`，不要自行启动；
- 不向外部服务发测试数据或外部消息。

### C.3 不得用假完成换 Green

- Fake、mock、stub、jsdom、静态 grep、空 target、cross-compile 或单一 Windows fixture 不能证明产品 E3/E4/E5；
- 不新增隐藏生产测试后门；不使用宽泛 `allow`、skip、ignore、弱断言、吞错或自动重试副作用来换绿；
- 不把 `partial`、`blocked`、`blocked_external`、`not_run` 变成 `passed`；
- 不把一个场景的成功清零另一个场景的失败；
- 不在代码修改后沿用旧 fingerprint 的 soak；
- 不把“按钮能点”或“API 返回 200”当作功能完成；必须有真实生产调用点和独立后置条件。

## D. 证据和状态要求

### D.1 状态

原子 batch/scenario 只允许：

```text
not_started / in_progress / passed / failed /
blocked_external / invalidated / not_run
```

`partial` 只能用于汇总。外部 blocker 只能在证明本机 ready 工作已耗尽后使用；“还没写 UI”“没有 production call site”“测试不稳定”都不是 external blocker。

### D.2 新 evidence 根

为本次任务创建唯一：

```text
tools/computer-use-probe/.run/finalization/<run-id>/
```

在写大量日志前，用 `git check-ignore -q` 验证该路径确实被忽略。目录必须含：

```text
baseline/ checkpoints/ failures/ logs/ metrics/ reports/
owner.json state.json manifest.jsonl
```

`owner.json` 写 run id、时间、repo、branch、HEAD、OS/arch、writer PID/session、schema version 和可清理绝对路径白名单。

### D.3 manifest

每条命令追加一条 JSON，至少含 `seq`、`phase`、`batch`、`scenario`、脱敏 argv、开始/结束时间、duration、exit code、状态、evidence level、log 相对路径、sha256、bytes、codeFingerprint 和 postconditions。

规则：

- 每次尝试用新日志路径，绝不覆盖旧日志；
- 写完日志立即 hash/size；
- 失败只能追加修复记录，不能改写原失败；
- `state.json` 用临时文件加同卷 rename；
- 每个 batch 结束、每两小时至少写 checkpoint；
- 最终逐条验证 manifest 的路径、大小、hash 和 fingerprint；有一条不一致就不能 passed；
- 不把唯一原始日志 prune 掉。可以压缩，但保留首败、末次成功和原始 hash。

### D.4 fingerprint

fingerprint 必须覆盖 HEAD、tracked diff、nonignored untracked source/config/docs 清单与 hash、lock/manifest digest。任何生产代码、测试、probe、runner、extension、配置或 runtime 变化都会使之后的 freeze/soak 证据 `invalidated`，必须重新冻结和重跑。

## E. 执行循环

对每个 batch 严格执行：

```text
读取当前代码/状态
 -> 写 batch-start manifest
 -> 建立 Red 或独立证明现状
 -> 最小实现
 -> 定向测试
 -> 检查 diff、生产调用点和权限边界
 -> batch gate
 -> 复制并 hash evidence
 -> 更新 state/checkpoint
 -> 自动选择下一个 ready batch
```

重型 runtime、浏览器、Tauri App 和 installer 任务串行执行；不要让两个进程同时移动/修复同一个 generated seed。不要用 sleep、空循环、重复静态命令填时间。

## F. 必须按顺序完成的批次

### F0：现场真值和 Red 基线

执行并留证据：

- `git status --short --branch`、HEAD、repo canonical path；
- tracked modified、nonignored untracked、ignored runtime/evidence 数量和大小；
- OS/arch、磁盘余量、现有 writer、隔离进程；
- `git check-ignore -q` 和 evidence ownership；
- 当前 fingerprint；
- branch-built App-shell 的 revoke/stop Red 复现。

不要复制旧 D13/D14 的 `passed`。F0 只有在现场没有被清理、Red 有原始日志、fingerprint 已记录时才通过。

### F1：先修 async stop/revoke panic（最高优先级）

必须真正修复，而不是加 catch 掩盖：

- 同步 HTTP client 的创建、请求和 drop 永远不能发生在 Tokio async worker 内；选择专用 blocking worker/队列或完整 async client，并在 ADR 中说明；
- 不在持有 Broker/session 锁时 await；
- `stop_requested`、清理进行中和 `stopped` 状态保持可区分；
- timeout 返回 `unknown`/`stop_requested`，不能自动重放副作用；
- revoke 必须撤销 token、停止 preview、取消 browser worker、释放 lease；
- 重复 stop/revoke、App-shell 完成、Tauri close、App 退出都幂等；
- worker 已退出、超时、malformed response、连接断开都必须可诊断且无 panic。

先写失败回归，再改代码。至少验证：

1. managed tab active request → 从 Tokio worker revoke；
2. `computer_stop`、session close、App-shell finish、window close；
3. Desktop、Managed Browser、Existing Tabs、WebView 各一轮；
4. 重复 stop/revoke、worker kill、超时和 malformed response；
5. child process、lease、profile、IPC token 最终清理。

要求 Rust regression + 隔离 branch-built App E3 + 至少 20 次循环。任一 panic、挂死、孤儿 worker 或 post-stop dispatch 都使 F1 `blocked`，不能继续写 App-shell passed。

### F2：建立 typed SurfaceRouter

把 `desktop`、`managed-browser`、`existing-tabs`、`app-webview` 从 slash/UI 一直带到 Host、Broker、ticket、MCP、事件和诊断：

- profile id 不得交给 Desktop adapter；必须先 `open_managed_profile`，再对真实 tab 授权；
- WebView/Existing Tabs 不得 Desktop fallback；dead target 只能拒绝；
- 未知 wire surface 返回 `unsupported_surface`；
- 同一 session 不跨 surface 复用旧 target/generation；
- command DTO、target DTO 和 trace 都保留 surface。

必须检查 production call site，不接受只在 test/probe 里存在的函数。为四个 surface 写正向和负向测试，并记录 slash `computer-use` 与 `computer-use-browser` 到实际 panel surface 的 trace。

### F3：Managed Browser 真产品闭环

用户从面板选择 profile 后必须真实完成：

```text
list profile -> open profile/tab -> user authorize
-> activate -> attach session MCP -> observe/act/verify
-> pause/stop/revoke -> close/cleanup
```

实现并验证：

- Host profile list/create/open/close/status event；
- UI 选择 profile 调 typed Host API，不再直接通用 authorize；
- bind/open/authorize 任一步失败都 `sessions::fail` 并回收新资源；
- model 只看当前 run tab；双 session 不串；
- compact、换模型、fork、revoke 后重新授权/observe；
- navigate/download/stop 与同一 router 和 cancellation 语义一致。

用 branch-built App、隔离 home、独立页面 oracle 做至少 20 次完整循环。`mcp_scripted_agent_run.rs` 单独成功不算产品证据。

### F4：App WebView 真产品闭环

修复 `computer_use_bind_webview` 的生命周期，使其完整执行：

```text
begin ticket -> typed bind -> register target -> authorize
-> activate -> attach session MCP -> complete
```

任一步失败必须 `fail + unbind + revoke`，不得遗留 authorization in progress。`list_targets_for_surface(WebView)` 不能永远返回空。只允许白名单 typed operations；禁止任意 eval、跨应用 cookie/storage 复用。隐藏面板停止 preview polling，旧 session/run/generation 的帧不得闪回。

用真实 Tauri side-browser/WebView product smoke 做至少 20 次 bind/authorize/observe/act/unbind；cross-origin iframe、权限 UI、复杂下载等明确 unsupported，并验证失败后下一次可恢复。

### F5：Existing Tabs 安全配对协议

先在第一小时内写 ADR，选择 transport。推荐 MV3 service worker + authenticated loopback WebSocket/HTTP；也可选择受控 native messaging，但必须说明安装和身份模型。

必须满足：

- secret/session key 不出现在未认证 GET、URL、地址栏、历史、普通日志、崩溃报告或截图；
- challenge 只暴露一次性 nonce、过期时间和 public metadata；
- 精确校验稳定 extension identity/fingerprint，拒绝任意 `chrome-extension://`；
- App 和 extension 都有显式确认，服务端不能收到请求就自动确认 extension；
- session key 只通过受保护通道交付，revoke/rotate/restart 后失效；
- nonce、instance、extension id、connection/document generation 绑定并防 replay；
- CORS/CSRF/Origin/Host/forwarded/rate-limit/expired/replay 均有负向测试；
- 断线、浏览器退出、App 退出和 revoke 清除旧 token。

推荐流程是 App 打开只含 public nonce 的 pairing page，extension service worker 显示 code/fingerprint 并等待用户确认，双方认证后建立单独 transport。不要把 secret 塞进 query string 作为“临时方案”。

### F6：Extension transport 和能力

把只有 content script 的 fixture 变成真正 transport：

- MV3 service worker 负责认证、连接、命令队列、心跳和断线；
- 权限最小化，解释调试/optional host 权限对用户的影响；
- 只有用户点击 Share 的 tab 才进入 Host，model 不能枚举全部 tabs；
- tab record 含 browser/profile、origin、home/focus、document/connection generation；
- observe 返回截图/页面和 generation；act 只接受最近 observation id；
- navigation/reload/close/focus/permission/disconnect 发事件并使旧 observation 失效；
- stop/revoke detach 并归还 tab（能力允许时），删除 run grant；
- 下载只能写 run-owned staging，拒绝模型任意路径。

在 Chrome for Testing 隔离 profile 做真实 fixture；Edge 若本机没有就标 `blocked_external`，但继续做其他 ready 工作。至少 5 个 tab 生命周期、双 session、断线/重连、旧 generation 拒绝和 observe→act→verify。

### F7：Host/UI 配对、picker grant、归还

产品面必须有真实的：

- 安装状态、pairing page、challenge expiry、extension fingerprint、连接状态和可操作错误；
- App 确认与 extension 确认两个不同状态；
- 用户明确分享的候选 tab picker；
- run-scoped `share_and_grant`/等价 API；
- stop、revoke、切 session、compact、logout、disconnect 的归还和清理；
- busy、empty、expired、denied、stale、retry、hidden/visible、keyboard focus 全路径。

所有文案进入 15 locale，复用项目 Select/ContextMenu/modal 规则。验证真实 Tauri command trace，不能只测按钮渲染。

### F8：MCP/session/事件闭环

确保模型面只请求能力，不自授予权限：

- 只有授权完成后注入 session MCP；带 surface/run/target scope；
- authorize/resume/reconnect/pairing/system permission 留在 Host/UI；
- 每个 act 验证 observation id、snapshot/generation、in-flight lease 和 cancellation；
- timeout 是 `unknown`，必须重新 observe，禁止复用 action id；
- tool result 有 image content、outcome、verification、reason 和脱敏 trace；
- fork/compact/换模型/恢复后重新授权；
- 不为每个动作创建新的聊天会话。

用真实 session MCP stdio → loopback/Bearer → Broker → product backend 做 scripted E3；独立 oracle、双 session 隔离、host-only 拒绝和 token 脱敏都要有证据。

### F9：四个 OS 的 capability/backend

在不虚报的前提下完成实现：

- Windows：保留 Win32/UIA，补齐 App/安装版 E3/E5、DPI、多屏、焦点、中文键盘、click/type/key/scroll/drag 后置条件；
- macOS arm64/x64：完成 key/scroll/drag/type/cancel、TCC 诊断、Retina/多屏/坐标；
- Linux X11：完成 X11 screenshot、XTEST click/key/type/scroll/drag、焦点和窗口存活；
- GNOME Wayland：使用经 ADR 认可的 portal ScreenCast/RemoteDesktop + EIS/libei 或等价 native helper，验证权限、PipeWire、坐标和断线；
- `native_wayland` 在实机 helper 证据前必须为 false，XWayland 不算 native；
- 每个 target 使用精确 lock/digest/resource map，不在线拉 latest。

当前 Windows 机器不能制造 macOS/Linux 实机证据。没有设备时写 `not_run`/`blocked_external` 及恢复命令，继续完成代码、编译和本机 ready 项。

### F10：诊断、隐私、清理

完成：

- trace、run staging、managed profile、pairing state 分目录；
- record/bytes/age 三维 retention；
- 脱敏 support bundle，压缩后二次 leak scan；
- cleanup 的绝对路径、symlink/junction、path traversal 防护；
- revoke/退出/crash recovery 幂等；
- p50/p95 observe/act/stop 和资源趋势。

sentinel secret scan、越界负测、crash/kill cleanup 和 bounded disk 都要有原始证据。

### F11：runtime、extension、bundle、安装

把 extension source、manifest、service worker、许可证和说明纳入真实目标 bundle。固定 identity/fingerprint。逐文件审计 Windows NSIS/portable，并在隔离目录执行：

```text
clean install -> first start -> pair/install instructions
-> same-version repair after tamper -> upgrade -> rollback -> uninstall
```

确认安装后不依赖仓库源码、PATH、用户 profile 或 online latest；NSIS 不含 probe、测试后门或用户数据。macOS/Linux package 独立验证。商店签名缺失可标 external blocker，但不能把源码目录存在写成安装完成。

### F12：质量债和测试隔离

必须处理：

- `src-tauri/computer-use-core/src/broker/tests_product_surface.rs:50` 的 Clippy 布尔断言；
- `cargo check -p grok-app` 的 dead-code warnings；
- 顶层 `allow` 对未接线代码的掩盖；
- Computer Use 新增超预算大文件；
- `WorkbenchComposerShell.tsx` 的不必要 `(item: any)`；
- locale key 不一致；
- core/browser/App/extension/runtime/installer 共享 mutable seed 的竞态。

保持 `App.tsx`/`AppWorkbench.tsx` 行数不增加。运行并留证据：

```text
git diff --check
pnpm typecheck
pnpm lint
cargo fmt --check
core clippy -- -D warnings
core tests
frontend targeted tests
runtime check
bundle audit
locale parity
dependency check
```

失败不能通过 skip、弱断言或关闭 warning 隐藏。

### F13：当前 fingerprint 的产品验收

在 freeze 前用 branch-built App + isolated home 串行执行：

1. 默认关闭检查；
2. Desktop 完整授权和 observe/act/verify/pause/takeover/resume/stop；
3. Managed profile→tab→authorize→MCP→action→stop；
4. WebView bind→authorize→MCP→typed action→unbind；
5. Existing Tabs pair→explicit share→picker grant→observe/act→disconnect/revoke；
6. close/restart/worker kill/malformed/stale target/DPI/focus；
7. 进程、lease、profile、token、trace、staging cleanup。

每个 surface 至少 20 次成功循环，每次有独立 oracle。真实模型 E4 未获批准就写 `not_run`。

### F14：freeze 后按场景主动长稳

只有 F0-F13 required 本机项成立才能 freeze。生成 fingerprint 后，任何代码/test/runner/config/runtime 变化都必须重来。

至少执行 12 小时真实 workload，不能用 sleep、空循环、重复静态命令或迭代耗时总和冒充墙钟。至少覆盖：

| scenario | 最低主动墙钟 | 最低轮数 |
| --- | ---: | ---: |
| desktop-appshell | 2h | 60 |
| managed-product | 3h | 100 |
| webview-product | 2h | 100 |
| existing-tabs-product | 2h | 100 |
| fault-stop-restart | 2h | 80 |
| privacy-runtime-package | 1h | 60 |

每个 scenario 独立保存 `rounds/ok/fail/consecutive_fail/active_wall_s`。一个 scenario 的成功不能清零另一个 scenario 的失败；缺失、0 轮、被轮转移除或 100% 失败都不能通过。安全 invariant（越权、错误目标、secret leak、post-stop dispatch、孤儿 worker/lease）任一非零立即失败。普通瞬时失败必须保留首败、分类、修复后重跑该 scenario，不能删除分母。

wrapper 被杀、机器重启、证据丢失或 fingerprint 漂移后，连续性从新的 freeze 重新开始。

## G. 时间和阻塞处理

48 小时是最大连续执行窗口，不是允许提前收工的理由。参考节奏：

```text
0–2h   F0
2–8h   F1
8–13h  F2
13–21h F3/F4
21–30h F5/F6/F7
30–34h F8/F10
34–38h F9/F11/F12
38–40h F13 + freeze
40–48h F14 active soak
```

如果某个外部环境缺失：

1. 写清缺失 OS/设备/真人/签名/授权、为什么本机不能解决、恢复命令和 oracle；
2. 把该格标 `blocked_external` 或 `not_run`；
3. 继续执行同阶段其他 ready batch；
4. 不得因此提前生成“完成”报告。

如果发现 panic、死锁、目标越权、secret leak、manifest mismatch、fingerprint drift 或用户环境被触碰，立即停止当前 workload，保留首败和 ownership，只终止本 run 登记的子进程。不要撤销、清理或覆盖用户已有修改。

## H. 最终交付格式

最后只在真实工作完成后写报告，必须包含：

- run id、repo、branch、HEAD、freeze/current fingerprint；
- F0-F14 每个原子状态和最高证据等级；
- Desktop/Managed/WebView/Existing Tabs 与 Windows/macOS/Linux X11/GNOME Wayland 的独立状态；
- 每个 scenario 的墙钟、轮数、成功、失败、连续失败和首败文件；
- 安全/隐私/清理 invariant；
- bundle/install/upgrade/repair/rollback/uninstall inventory；
- manifest path/size/hash 完整性；
- 未完成项、external blocker 和恢复条件；
- 明确写未 commit、未 push、未提 PR。

若任何 required 项未通过，标题和结论必须是：

```text
partial — not releasable
```

只有完成谓词全部成立时才能写 `passed`。不要因为时间到了、测试数量够了或旧报告写过 passed 就改变状态。

现在从 F0 开始执行，并持续推进到窗口结束或完成谓词成立。除非需要用户明确批准真实模型/外部设备，否则不要停下来等待确认。

---

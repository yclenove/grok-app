# Computer Use：Grok 声称完成后的 48h 结果审计

> 日期：2026-09-13
> 工作区：`H:\aicoding\grok-app-computer-use`
> 分支：`feat/computer-use-implementation`
> 审查基线 HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`
> 审查性质：只读复核、独立复测和后续执行设计；本次不修改产品实现，不提交、不推送、不提 PR

## 1. 结论

当前总体状态是：

```text
partial — Windows runtime、Broker 基础能力和若干 scripted/fixture E3 子链成立；
Computer Use 产品闭环尚未完成，当前不可发布。
```

Grok 的旧 `D14-final-report.md` 写过 `48h 结果：passed`，但这不是当前代码的有效最终结论。该 run 的 `state.json` 已把 D12、D13、D14 标为 `invalidated`，原因是 freeze 之后仍发生了生产代码修改（配对 IPC、MCP scripted-agent 等）。此外，本轮在当前工作树重新启动 branch-built App 时触发了 Tokio panic。因此旧报告最多是历史记录，不能当作 release gate。

不能把下面几类结果写成“已完成”：

- probe、fixture、fake adapter 或 scripted agent 的通过；
- Windows 通过后复制到 macOS、Linux X11 或 GNOME Wayland；
- 只有 `cargo check` / 单元测试通过、没有真实 App UI/Host 调用的模块；
- `partial`、`blocked_external` 或 `not_run` 被汇总成 `passed`；
- 旧 fingerprint 上的长稳结果；
- 退出码为 0 但没有独立后置条件的循环。

## 2. 审查边界与现场

本次只读审查遵守以下边界：

- 保留所有现有 tracked、untracked 和 ignored 改动；
- 不读取或导出账号、Cookie、Token、浏览器个人资料、共享 `~/.grok`；
- 不操作正式 Grok App、日常 Chrome/Edge、代理、VPN 或外部账号；
- 只使用独立 identity、独立临时 `GROK_APP_HOME`、隔离浏览器 profile 和 ACP stub；
- 不执行 reset、clean、restore、stash、切分支、commit、push、PR、merge、release。

当前现场：

- HEAD 仍为 `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`；
- 分支仍为 `feat/computer-use-implementation`；
- 工作树有大量用户需要保留的未提交修改；
- Computer Use 的源码、runtime seed、probe、测试和计划文档均混在同一 dirty tree 中；
- 不能用 `git diff --stat` 代表全部变更，因为大量 Computer Use 文件仍是 untracked。

## 3. 状态与证据语义

后续报告统一使用以下状态：

| 状态 | 含义 |
| --- | --- |
| `validated` | 在当前 code fingerprint 上，有可重放的独立证据证明 required acceptance 成立 |
| `partial` | 子项成立，但同一产品面仍有必需能力、调用链或环境没有成立 |
| `stale` | 证据曾经成立，但之后代码、配置、runtime 或证据索引发生变化，必须重跑 |
| `blocked` | 当前实现或验证逻辑自身阻塞，不能靠换措辞解决 |
| `blocked_external` | 确实缺少本机之外的 OS、硬件、真人、签名或授权环境，并且本机 ready 工作已耗尽 |
| `not_run` | 没有运行；不能写成支持或通过 |

证据等级仍按 `docs/llm-wiki/computer-use.md`：E1 是单元/编译，E2 是真实模块隔离集成，E3 是真实产品协议上的 scripted agent，E4 是 branch-built App + 真实模型 + 真人授权，E5 是安装、升级、回退、卸载和任务矩阵。低等级证据不得填高等级格子。

## 4. 本轮验证快照

以下结果是在当前工作树上重新检查或独立复测得到的。它们只证明表中“能证明什么”一栏。

| 检查 | 当前结果 | 最高可信等级 | 结论边界 |
| --- | --- | --- | --- |
| `git diff --check` | `validated` | E0 | 没有 whitespace error；行尾 warning 仍需统一策略 |
| `pnpm check:computer-use` | `validated` | E1/E2 | Windows runtime seed/lock 检查成立，不证明 installer 生命周期 |
| Computer Use 前端定向测试 | `109/109`，`validated` | E1 | 面板/协议单测成立，不证明 Tauri 实机 UI |
| `pnpm typecheck` | 通过，`validated` | E1 | 不能消除显式 `any` 或断开的产品调用点 |
| `pnpm lint` | 通过，`validated` | E1 | 不等于产品交互或架构完整 |
| `cargo fmt --check` | 通过，`validated` | E1 | 不等于 Rust 质量门禁全部通过 |
| Computer Use core tests | `281/281`，`validated` | E1/E2 | 协议/Broker 契约绿；不证明 App/安装/跨平台 |
| `cargo check -p grok-app` | 通过但有约 36 个 dead-code warning，`partial` | E1 | warning 说明仍有未接线模块 |
| Core Clippy `-D warnings` | 失败，`blocked` | E1 | `tests_product_surface.rs:50` 使用 `assert_eq!(..., true)` |
| `product-managed-route` | probe 通过，`partial` | E3（probe） | 没有证明生产 UI 从 profile 到 managed tab 的闭环 |
| `existing-tab-extension` | CFT fixture 通过，`partial` | E3（fixture） | 没有证明产品 transport、用户 Chrome/Edge 或安装包 |
| `webview` | typed fixture 通过，`partial` | E2/E3（fixture） | 没有证明产品 Broker 授权生命周期 |
| branch-built App-shell 复测 | 触发 Tokio panic，`blocked` | E3 | 撤销/停止路径会使 App 进入异常状态 |
| 真实模型 E4 | `not_run` | - | 未经用户批准，不得自行启动 |
| macOS arm64/x64 实机 | `not_run` / `blocked_external` | - | Windows 机器不能替代 macOS 证据 |
| Linux X11 / GNOME Wayland 实机 | `not_run` / `blocked_external` | - | XWayland 或浏览器成功不能替代 native Wayland |

## 5. 关键问题（按严重度）

### P0-1：App-shell 撤销路径会触发 Tokio runtime panic

本轮用隔离 identity、全新临时 `GROK_APP_HOME`、ACP stub 和当前 branch-built App 跑一轮，日志记录：

```text
Cannot drop a runtime in a context where blocking is not allowed.
This happens when a runtime is dropped from within an asynchronous context.
```

调用链是：

```text
app_shell.rs:389
  -> sessions::revoke
  -> SessionGrants::revoke
  -> ComputerUseBroker::request_stop
  -> ExistingTabHost::cancel_run
  -> LoopbackPlaywrightWorker::cancel_run
  -> bounded_loopback_post
  -> reqwest::blocking::Client::builder (worker_http.rs:53-59)
```

关键代码位置：

- `src-tauri/src/computer_use/app_shell.rs:389` 在异步 harness 中直接撤销 session；
- `src-tauri/computer-use-core/src/broker/lifecycle.rs:30-44` 的 stop 清理会同步调用 browser worker；
- `src-tauri/computer-use-core/src/browser/worker_http.rs:53-59` 在该路径创建 `reqwest::blocking::Client`；
- `src-tauri/src/computer_use/playwright_worker.rs:255-261` 的 `cancel_run` 走同步 HTTP。

这不是“测试环境噪声”：它发生在产品 App-shell 的生命周期路径，可能导致撤销按钮、会话结束或关闭时挂死/失去清理。当前 D6 必须是 `blocked`，所有依赖它的 freeze/soak 结果均为 `stale`。

修复要求：统一异步边界或专用长生命周期 blocking worker，保证同步 HTTP client 的创建和 drop 永远不发生在 Tokio async worker 内；同时保留停止状态、超时和 unknown 语义，不能简单吞掉错误或 `unwrap`。

### P0-2：Managed Browser 入口仍没有形成产品闭环

`ComputerPanel.tsx:138-155` 对所有 surface 都调用同一个 `computerAuthorize`。选择 `managed-profile:<name>` 时，Broker `broker.rs:316-352` 先查已打开的 managed tab；找不到后会把 profile id 交给桌面 adapter 的 `target_alive/claim_target` 路径。它没有在产品命令中先调用 `open_managed_profile`，所以 profile picker 不是可操作的 managed-browser 目标。

真正的 `open_managed_profile` 生产调用只在 harness/probe（例如 `app_shell.rs:726` 和 `mcp_scripted_agent_run.rs`）中出现。底层 managed worker 通过 scripted E3，并不代表用户从 `/computer-use-browser` 打开的面板能：

1. 列出 profile；
2. 打开对应 tab；
3. 对新 tab 进行 run-scoped authorize；
4. 绑定 session MCP；
5. 在 stop/revoke 时关闭并清理 profile。

因此 Managed Browser 当前为 `partial`，不是 `validated`。

### P0-3：WebView 只有模块和测试，没有完整 Broker/Host 产品生命周期

`src-tauri/src/commands/computer_use.rs:503-527` 的 `computer_use_bind_webview`：

- `sessions::begin` 创建了 ticket；
- 绑定 `product_webview()`；
- 直接返回 target DTO；
- 没有对应的 `authorize_target`、`sessions::activate`、`SessionManager::attach_computer_use`、`sessions::complete`；
- bind 失败时也没有统一的 `sessions::fail + unbind` 补偿路径。

`list_targets_for_surface(WebView)` 在 core 中也只是返回空列表，真正的 `product_webview()` 没有被一个统一 `SurfaceRouter` 作为 Broker adapter 注册。结果是 typed WebView fixture 可以绿，但真实 session 的 model MCP 无法得到一个完整、可撤销、可验证的 WebView target。当前状态 `partial`。

### P0-4：Existing Tabs 没有产品 transport

现有 extension 只有：

- `tools/computer-use-extension/manifest.json`：`permissions: []`，一个 loopback content script；
- `tools/computer-use-extension/pair.js`：访问 pairing challenge 并轮询 `/should-continue`。

它没有 service worker、`tabs`/调试/脚本 transport、用户点击 Share 的消息通道、tab 枚举、observe/act 事件流或 disconnect/return 逻辑。`share_and_grant`、`begin_pairing_challenge` 等调用点主要在 probe/测试；没有产品调用点把用户选中的 Chrome/Edge tab 注册到 Host，再发放 run-scoped grant。

所以 Existing Tabs 的 CFT fixture E3 只能标为 `partial`。没有 transport，产品面即使显示一个“配对成功”也不能实际操作用户 tab。

### P0-5：配对协议仍暴露过多未认证信息，且 UI 没有完成双向流程

当前 IPC 代码存在以下问题：

- `src-tauri/computer-use-core/src/ipc.rs:376-412` 的未认证 loopback GET 会返回 pairing `secret`；
- `pairing_origin_ok`（`ipc.rs:330-340`）接受任意 `chrome-extension://` origin，以及任意 loopback page origin；
- `ipc.rs:443-455` 在验证 nonce 后自动调用 `confirm_pairing_extension()`，没有真正的 extension-side explicit confirmation；
- 204 响应没有把 `complete_pairing` 得到的 session key 交给扩展；
- `ComputerPanel.tsx:193-206` 只 begin challenge 并确认 App，没有打开 pairing page、传入随机 IPC port 或等待扩展回执；
- extension id 在 fixture 中硬编码为 `pw-ext-installed`，unpacked extension 没有稳定的签名 key/发布身份。

本地 loopback 不等于可信边界。后续协议必须做到：secret 不进 URL、地址栏、历史或普通未认证 GET；精确验证 extension identity；一次性 challenge、过期、重放拒绝、速率限制；App 与 extension 都有明确确认；session key 只在认证通道内短期存在并在 revoke/重启时轮换。

### P1-1：扩展未进入 NSIS/实际安装包

`src-tauri/tauri.windows.conf.json:4-11` 只列 runtime seed、lock 和 README，没有 `tools/computer-use-extension`。`scripts/tauri-before-build.mjs` 也只准备 Windows runtime。对已审计的安装资源，没有 `pair.js`、extension manifest 或可安装说明。

因此“源码目录里有 extension”不等于用户安装后能配对。必须分别验证 portable/NSIS/升级/修复/卸载，以及 extension 的安装说明和稳定 identity。没有 Chrome/Edge 商店签名时可记录为外部发布阻塞，但不能把源码存在写成安装完成。

### P1-2：跨平台动作和 native Wayland 尚未完成

证据直接显示：

- `src-tauri/src/computer_use/macos_adapter.rs:378-401` 的 `Key`、`Scroll`、`Drag` 返回 `not implemented on this adapter yet`；
- `src-tauri/src/computer_use/linux_adapter.rs:398-428` 的输入、按键、滚动、拖拽也大部分返回同样错误；
- `linux_adapter.rs:187-228` 明确 `native_wayland = false`；
- `linux_adapter.rs:311-315` 和 `398-404` 拒绝把 XWayland 当成 GNOME Wayland native。

这部分的拒绝语义是正确的，但说明首版目标尚未达成。macOS、Linux X11、GNOME Wayland 仍需各自实现/接线、权限诊断、安装和实机证据。不能用 Windows 或 Chromium browser 的通过结果填充这些格子。

### P1-3：D13 长稳统计不能证明安全稳定

旧 run 的 `metrics/d13-iterations.jsonl` 直接重算得到：

| 场景 | 总轮数 | 成功 | 失败 |
| --- | ---: | ---: | ---: |
| `app-shell` | 209 | 209 | 0 |
| `product-managed-route` | 276 | 276 | 0 |
| `existing-tab-extension` | 2822 | 2818 | 4 |
| `webview` | 2822 | 2822 | 0 |
| `browser-error-contract` | 43 | 0 | 43 |
| `isolated-runtime-repair` | 153 | 110 | 43 |
| `nsis-audit` | 154 | 154 | 0 |
| `privacy` | 153 | 153 | 0 |
| `perf` | 153 | 153 | 0 |
| `seed-audit` | 154 | 154 | 0 |

旧脚本 `d13_soak.py:265-272` 的 `lane_done` 只看 lane 的累计时长和轮数，不看每个场景的成功率；`d13_soak.py:423-430` 以 lane 为粒度清零连续失败计数。因此同一个 lane 的另一场景成功可能清掉失败场景的连续失败计数。`L4_ROTATE` 还会在恢复后跳过部分历史失败场景。

另外，D13 统计的是迭代耗时总和，不是每个 required 场景的主动墙钟时长；10 小时 wrapper kill 后又 resume，也不能自动等同于一段连续、同一 fingerprint 的 soak。

结论：D13 旧证据为 `stale`，且存在统计设计缺陷，不能作为 release gate。下一轮必须按 scenario key 独立计数，失败不被其他场景清零；任何代码变化都使 freeze 后的长稳失效。

### P1-4：Evidence manifest 本身不可完全信任

在当前 run 的 `manifest.jsonl` 上做直接文件核对：

- 97 条记录；
- `log` 路径缺失数为 0；
- 按记录中的 `bytes` 和实际文件大小比较，至少 11 条不一致；
- 按记录中的 `sha256` 比较，至少 20 条不一致。

旧 inventory 使用了更宽的记录范围，报告过 22 条 hash 和 13 条 size mismatch；两种口径都说明 manifest 不是可直接验收的 immutable inventory。典型问题包括 NSIS 审计、D14 重跑日志和 core test 日志在 manifest 写入后被覆盖或重新生成。

后续必须采用唯一日志路径、append-only manifest、写完即 hash、禁止复用路径、结束时逐条 inventory。证据丢失或不一致时状态只能是 `stale`/`blocked`，不能靠报告文字修正。

### P2-1：质量门禁和代码债仍未收敛

- Core Clippy 在 `src-tauri/computer-use-core/src/broker/tests_product_surface.rs:50` 因布尔值 `assert_eq!(..., true)` 在 `-D warnings` 下失败；
- `cargo check -p grok-app` 仍产生约 36 个 dead-code warning；
- `src-tauri/src/computer_use/mod.rs` 顶层 `#![allow(unused_imports)]`，此前还有更宽的 dead-code 豁免，容易掩盖未接线模块；
- Computer Use 新增的大文件参与仓库 `FILES_OVER_1K_BUDGET` 超额，代表至少要拆分自身模块；
- `WorkbenchComposerShell.tsx` 出现 `(item: any)`，削弱已有类型；
- 后续不得把新状态塞进 `App.tsx` / `AppWorkbench.tsx`，也不得用 `allow`、skip 或弱断言换绿。

## 6. 已验证、但应保留的成果

这些成果值得保留，但必须按证据等级使用：

1. Windows x64 runtime seed 有精确 lock、tree digest、路径穿越防护和同版本 repair 基础；
2. Broker 已有 feature 默认关闭、session/run ownership、target generation、exclusive lease、stop/unknown、loopback Bearer 和 MCP 工具边界；
3. Managed Browser 的 packaged Playwright worker 链可以通过 scripted MCP E3；
4. Chrome for Testing 的隔离 extension fixture 可以验证 challenge、generation 和 session 隔离；
5. typed WebView adapter 对 cross-origin iframe、权限 UI、复杂下载等不支持情况有 fail-closed 方向；
6. 前端 surface 类型已在 `surface.ts`、`panelStore.ts` 和 `SideWorkbench.tsx` 之间传递，后续应接到真正的 Host router，而不是重做一套 UI 状态。

## 7. 当前状态矩阵

| 产品面 | 当前状态 | 可信证据 | 发布判断 |
| --- | --- | --- | --- |
| Protocol / Broker / permissions | `partial` | E1/E2 + 部分 E3 | 保留骨架，先修 lifecycle/transport |
| Windows runtime pack | `validated`（seed） | E2/E3 | 仍需实际 bundle/install E5 |
| Windows Desktop | `partial` | fixture E2/E3 | App-shell panic 阻塞 E3 |
| Managed Browser backend | `partial` | scripted MCP E3 | 产品 UI/Host route 未闭环 |
| Existing Tabs backend | `partial` | CFT fixture E3 | 产品 transport 缺失 |
| App WebView | `partial` | typed fixture E2/E3 | Broker/session lifecycle 缺失 |
| Pairing security | `blocked` | 静态审查 + fixture | 需协议重做后重验 |
| Diagnostics/privacy | `partial` | bounded probe | retention、导出、清理和二次扫描不足 |
| D13/D14 长稳 | `stale` | 旧 run ledger | fingerprint、统计和 manifest 均不满足 |
| Windows install/update/rollback | `not_run` | source config only | 必须做隔离安装生命周期 |
| macOS arm64/x64 | `not_run` / `blocked_external` | 无实机 | 不能发布首版支持声明 |
| Linux X11 | `not_run` / `blocked_external` | 无实机 | 不能发布首版支持声明 |
| GNOME Wayland native | `not_run` / `blocked_external` | 明确 gated | 不能用 XWayland 替代 |
| Real Grok model E4 | `not_run` | 未获授权 | 不得自行启动或伪造 |

## 8. 下一步优先级

按依赖和风险排序：

1. 修复并回归 async stop/revoke panic；
2. 建立真实的 typed surface router；
3. 完成 Managed Browser 的 profile → tab → authorize → MCP → stop 闭环；
4. 完成 WebView 的 begin/bind/authorize/activate/attach/complete/fail 生命周期；
5. 做 Existing Tabs 产品 transport 和安全双向配对；
6. 把 extension、runtime 和安装/修复/回退纳入真实 bundle；
7. 修正 D13 runner 的 scenario 统计、manifest 和 fingerprint 失效规则；
8. 补 diagnostics/privacy/cleanup、质量债和跨平台实现；
9. 代码冻结后，重新跑同一 fingerprint 的产品矩阵和主动长稳；
10. 只有最终门禁全部满足时才生成新的 `passed` 报告，否则诚实输出 `partial`、`blocked` 或 `not_run`。

详细执行批次见 `2026-09-13-computer-use-grok-finalization-execution.md`；可直接交给 Grok 的长任务提示词见 `2026-09-13-computer-use-grok-finalization-prompt.md`。

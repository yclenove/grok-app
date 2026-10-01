# Computer Use 交接：Grok 4.7 接手前的可信状态

日期：2026-09-22 01:22 +08:00  
仓库：`H:\aicoding\grok-app-computer-use`  
分支：`feat/computer-use-implementation`  
基线 HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`  
总状态：**partial — not releasable**  
发布状态：**未 commit / 未 push / 未提 PR**

## 1. 交接结论

这已经不是概念验证。App-owned Broker、会话 MCP、受管浏览器、Windows 桌面、
WebView 子集、ExistingTab 配对/分享/观察/固定动作、取消与多级生命周期恢复均有
大量实现和真实进程/浏览器证据。尤其 ExistingTab 已经存在生产 adapter，并通过
会话 MCP 进入 `click / set_value / type_text / scroll / wait` 的 MV3 v2 通道；旧文档
中“动作能力仍为 NONE”的描述已经落后于代码。

但当前还不能发布。最先要解决的不是继续扩动作，而是两条当前红线：

1. **完整浏览器退出后，原 claimed 动作占用无法被安全退休。** 同一 owned profile
   重新启动时 `cuBrowserSession.id` 没有改变，真实 gate 在第三个后置条件失败。
2. **当前 materialized Windows Chromium seed 完整性校验失败。** `pnpm
   check:computer-use` 报 tree digest 不符，当前 Core 因此为 452/454，而不是全绿。

其后才是工具栏真实手势/截图、App UI 真实授权闭环、renderer/扩展升级世界、安装版
Chrome/Edge、真实模型、三 OS 和发布矩阵。单元测试数量不能替代这些证据。

## 2. 不可破坏的工作区边界

- 当前工作树包含大量未提交成果，必须原地保留；禁止 `reset`、`restore`、`clean`、
  `stash`、强制 checkout、rebase 或重建 worktree。
- 含本次新增的三份交接文档，当前 `git status` 为 **70 个 tracked 修改行、
  170 个 untracked 行，共 240 行**。
  untracked 中包含 86 份 Computer Use 计划/检查点、产品源码、运行时资源，以及本机
  浏览器实验目录，不能把所有 untracked 一概当垃圾。
- index 当前为空。未经用户明确授权，不 commit、push、PR、merge、tag、publish、
  release 或 deploy。
- 不 pull/merge 上游；大脏树同步必须另做审查与备份方案。
- 不碰 `H:\aicoding\grok-app` 及其他 worktree，不 pop/drop 仓库级 stash。
- 不碰真实账号、Cookie、Token、Credential Store、共享 `~/.grok`、系统代理/VPN、
  正式 Grok 安装或用户日常 Chrome/Edge profile。
- 测试仅使用隔离 `GROK_APP_HOME`、owned profile、随机 loopback/Bearer、自建页面和
  owner 标记的子进程。不得按进程名批量结束 Chrome/Node/Grok。
- Cargo、浏览器、App、安装器等重任务串行执行，避免共享 seed、target、端口和
  profile 相互污染。
- Computer Use 必须默认关闭；normal/YOLO/accept-edits 不授予控制权限；模型不能
  authorize、confirm pairing、resume、reconnect 或扩大 target。

## 3. 当前实现地图

| 领域 | 当前代码事实 | 当前证据边界 |
| --- | --- | --- |
| Host/Broker | session/run 绑定、授权 ticket、目标/快照/geometry 身份、exclusive lease、actionId 去重、unknown 隔离、暂停/恢复/停止、trace 与 run ledger | Core 有大规模单测和真实 IPC 探针；当前 seed 红灯使本轮 Core 仅 452/454 |
| 会话 MCP | `grok-computer-use` 经 NDJSON stdio → loopback Bearer IPC → Broker；Host-only authorize/resume 不在模型目录 | scripted agent 和真实 MCP 子进程已有历史证据；真实 Grok 模型 E4 未跑 |
| 前端 | 设置默认关闭、Computer 资源页、target picker、配对、授权、preview、任务卡、slash 入口、15 locale | 本轮定向 16 文件 152 测试通过；完整安装 App 的交互验收未跑 |
| Managed Browser | App-owned Node/Playwright/Chromium、owned profile、typed observe/act、导航/上传/下载、取消、暂停/恢复、request revision | source/seed 真浏览器历史证据较完整；当前 seed hash mismatch，安装版与三 OS pack 未重新证明 |
| Windows Desktop | Win32/UIA 截图、语义输入、点击、键盘/滚动/拖动、剪贴板恢复、焦点漂移、用户接管与 fixture oracle | 自建 Windows fixture 有 E3 历史证据；安装 App + 真人授权 + 真实模型未跑 |
| App WebView | typed observe/click/set_value 子集，跨源和不支持项 fail closed | fixture/模块证据，真实 App WebView 产品路径与复杂生命周期未完整验收 |
| ExistingTab 分享 | 双确认配对、30 秒 Host 租约、明确 Share/Unshare、documentId/pickerToken、导航/关闭撤销、借用 tab 不关闭 | Chrome for Testing + production SW/HTTP 有真实证据；原生工具栏手势仍未正向通过 |
| ExistingTab 观察 | 固定 isolated-world DOM 观察、隐私过滤、refs、可选 PNG、preview/model refs 隔离、竞态与超时 | MCP → IPC → Broker → adapter → MV3 → 页面正文已通过真实 fixture；工具栏授权后的截图成功未验 |
| ExistingTab 动作 | 生产 `ExistingBrowserAdapter` 已注册，严格支持 `click/set_value/type_text/scroll/wait`；拒绝坐标、key、drag、clipboard、任意 evaluate | 私有 fixture 完成 App 授权后，真实 session MCP 动作/去重/陈旧快照/会话隔离/Stop 已验；尚不是用户在 App UI 的完整授权闭环 |
| 生命周期恢复 | document epoch/sequence/fence、guardian/terminal receipt、SW 重启、completion tombstone、Host process witness、跨 Host cleanup | Host/SW 多切点矩阵历史通过；完整浏览器退出、renderer crash、扩展 reload/update 的旧 world 仍未闭合 |
| Runtime/交付 | target-specific lock、固定 Node 20.18.0、Playwright 1.48.0、Chromium 130 rev 1140、prepare/check、repair/rollback、bundle hooks | 历史 packaged contract 有 Windows E2/E3；当前 materialized seed 被改坏或漂移，必须先恢复并重验 |
| macOS/Linux | adapter 与部分能力已有代码；缺权限时 fail closed，Wayland 不冒充 X11 | macOS arm64/x64、Linux X11、GNOME native Wayland 实机均未完成；保持 `not_run` |

主要路径：

- 产品约束：`docs/llm-wiki/computer-use.md`
- 状态账本：`docs/plans/2026-09-09-computer-use-execution-state.md`
- Host/Broker：`src-tauri/computer-use-core/`、`src-tauri/src/computer_use/`
- ExistingTab adapter：`src-tauri/computer-use-core/src/browser/existing_adapter.rs`
- 扩展：`tools/computer-use-extension/`
- MCP：`tools/computer-use-mcp/`
- 受管浏览器：`tools/computer-use-browser/`
- 探针：`tools/computer-use-probe/`、`src-tauri/cu-probe/`
- 前端：`src/components/computer-use/`、`src/lib/computer-use/`、
  `src/lib/api/computerUse.ts`

## 4. 2026-09-22 当前快照验证

以下是本次交接现场重新执行的结果，不是沿用旧报告：

| 命令 | 当前结果 |
| --- | --- |
| `node --test tools/computer-use-extension/*.test.mjs` | **148/148 passed** |
| Computer Use 定向 Vitest（16 文件） | **152/152 passed** |
| `pnpm typecheck` | passed |
| `pnpm lint` | passed |
| `cargo test -p grok-computer-use-core --test driver --features test-support --offline --target-dir target-cu -- --test-threads=1` | **12/12 passed** |
| `cargo check -p grok-app --offline --target-dir target-cu` | passed |
| `git diff --check` | exit 0；只有既有 LF/CRLF 提示 |
| `pnpm check:computer-use` | **failed**：Chromium tree digest mismatch |
| `cargo test -p grok-computer-use-core --lib --offline --target-dir target-cu` | **452 passed / 2 failed**；两项均先撞到同一 Chromium seed 完整性问题 |

当前 lock 期待 Chromium tree SHA-256：

`63c6075faf6d986bc8e9f11996f5d2e784e7f4d9471366b7a2ad6eebaca245e3`

当前 materialized tree 实际为：

`a38e943801ac6142dd61ec3c551edd5d63aca8ea575bbe18b12a87d203ba383b`

因此两项 Core 失败目前应写成“被本地 runtime seed 完整性红灯阻断，待安全重物化后
复核”，不能直接断言产品代码回归，也不能把历史 454 passed 当成本轮结果。

## 5. 最近可信的历史证据

这些证据可指导继续开发，但候选代码或 runtime 改变后必须重跑：

- `2026-09-21-computer-use-host-process-retirement-checkpoint.md`：真实产品 Host
  进程崩溃；五个动作切点 × Host/SW 顺序矩阵共 15 场景通过，旧动作不重放，
  新点击各一次；Node 148、Core 450、Driver 12 等当时通过。
- `2026-09-21-computer-use-completion-retirement-checkpoint.md`：settle 回包丢失、
  tombstone 容量/TTL 淘汰后的 cleanup-only retirement 已闭合。
- `2026-09-20-computer-use-document-execution-checkpoint.md` 与
  `document-lifetime-checkpoint.md`：旧观察不可复活已退休 snapshot；BFCache null
  不再被误当文档销毁。
- `2026-09-20-computer-use-production-action-transport-checkpoint.md`：生产 SW/v2
  动作派发、固定动作、单次领取和结果通道已接通。
- `2026-09-20-computer-use-existing-adapter-checkpoint.md`：生产 ExistingTab adapter
  的观察通道已接 Broker/MCP。

历史证据不覆盖本轮 runtime seed 红灯，也不覆盖完整浏览器退出、安装版、真实模型
或三 OS。

## 6. 当前 P0：完整浏览器退出恢复

真实失败日志：

`tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/existing-browser-exit-red.log`

已成立的前两个检查：

- `execution-clock-real-transactions`
- `production-sw-negotiates`

失败的第三项：

- `browser-exit-recovers-original-owner/original-host-occupancy-must-recover`

失败现场：原生产 SW 已 claim 一个 click、旧点击效果为 0、完整 owned Chromium 已
关闭、同一 profile 重开、旧 session journal/pairing 不存在、新配对与 Share 成功；
但原 Host 仍保持 `phase=claimed`、`cancelRequested=true`，等待占用归还超时。

当前实验实现位于：

- `tools/computer-use-extension/browser-session.mjs`
- `tools/computer-use-extension/browser-session.test.mjs`
- `tools/computer-use-extension/sw.js`
- `tools/computer-use-probe/existing-browser-exit-live.mjs`
- `src-tauri/src/computer_use/extension_pair.rs`

关键审查点：

1. `--load-extension` 临时加载的开发扩展没有取得可信的真实
   `chrome.runtime.onStartup` 事件；不能手动调用 listener、清 `storage.session` 或
   随机换 ID 来伪造浏览器重启。
2. `BrowserSession` 初次创建 `{started:false}`。当前 onStartup 在 `started:false`
   时只改为 true、不换 ID。若扩展是在浏览器已运行时首次安装，第一次真实浏览器
   重启也可能沿用旧 ID；这是尚未被安装态证据证明的安全边界。
3. 当前测试名声称 duplicate startup idempotent，但测试和实现会在第二次事件换 ID；
   需要按真实事件语义重写状态机与断言，不能只改测试标题。
4. 浏览器 session 标记只允许作为“发生了真实 profile startup”的证明链一环，
   不能单独证明业务动作成功；缺标记、storage 丢失、断线、超时、重新配对、
   documentId 查询为 null 都必须保守保持占用。
5. 正式清理仍须绑定旧 browser identity、原动作 cleanup proof、Host pending 和新
   配对；只能把旧业务结果收束为 unknown/cleanup，不能重放动作或恢复权限。

## 7. 已发现的代码/文档漂移

- `src-tauri/src/computer_use/extension_pair.rs` 的 pairing expected 数组现场计数为
  **36**，PASS 文案硬编码 `checks=37`。必须改为从数组长度生成或修正真实清单，
  不能继续用错误数字做报告。
- `docs/llm-wiki/computer-use.md` 多处仍说 ExistingTab actions 未完成或能力 NONE；
  顶部也只更新到 Host crash 检查点。应在生命周期 P0 闭合后统一改成“固定五动作
  已实现并有 fixture/MCP 证据，但完整浏览器/安装/UI/平台仍未发布”。
- `tools/computer-use-extension/INSTALL.md` 仍写“model observation/actions unfinished”
  与“production control adapter unavailable”，已经落后于当前代码；但在真实用户
  授权/安装链未验前，也不能反向写成正式可用。
- `docs/plans/2026-09-09-computer-use-execution-state.md` 是追加式历史账本，顶部未包含
  最新 Host crash 与浏览器退出首败；继续时应追加更正，不删旧历史。

## 8. 本机实验资产与安全清理状态

仓库根当前有 **44 个 `.cu-probe-*` 目录**，另有测试 CRX/PEM。部分含私钥：

- `.cu-probe-pack-test/extension.pem`
- `.cu-probe-pack-test-installed.pem`
- `.cu-probe-cert/key.pem`

这些只属于失败的安装研究，绝不能打印内容或提交。三个最新失败脚本也是实验品：

- `tools/computer-use-probe/prepare-installed-extension.mjs`
- `tools/computer-use-probe/interactive-install.mjs`
- `tools/computer-use-probe/prepare-pref-installed-profile.mjs`

其中 `prepare-installed-extension.mjs` 对任意 destination 使用 recursive remove；未加
owned-root 校验前不得成为正式工具。`prepare-pref-installed-profile.mjs` 手写 Chrome
Preferences 的方案已失败，不得继续集成。

本次交接已恢复/清理上一轮的注册表副作用：

- 恢复 `HKCU\Software\Policies\Google\Chrome`：`QuicAllowed=0`、
  `DnsOverHttpsMode=off`；
- 删除测试 ID `gldlkoglmaheognbfbhpjicmgeolknon` 在 Google Chrome/Chromium
  external-extension 分支的精确注册项；
- 删除本轮创建的空 `ExtensionInstallForcelist` 子键和空 Chromium policy 分支；
- 当前未发现命令行命中 `.cu-probe-*` / Computer Use probe 的 Chrome、Node、
  `cu_probe`、Cargo 或 rustc 残留进程。

不要再次修改用户级 Chrome/Chromium policy、external extension registry 或日常
浏览器配置。需要安装态测试时，优先使用受支持的隔离安装机制、临时 OS 用户/VM，
或在明确需要真人操作时标 `blocked_external`。

## 9. 下一执行者的起点

严格按以下顺序开始：

1. 全文读 `AGENTS.md`、Computer Use wiki、本交接、96 小时计划、最新 Host crash
   检查点与浏览器退出 ADR。
2. 建立新的 gitignored evidence root，记录 HEAD、dirty inventory、源码 fingerprint、
   进程/端口/owned 资源；不要复用旧 passed。
3. 安全重物化 Windows runtime seed，先恢复 `pnpm check:computer-use` 和 Core 全绿；
   若 lock/source 本身有问题，保留首败再修生成器，不能手改 hash 迎合污染树。
4. 以真实持久安装态证明 `onStartup`/首次安装/首次重启语义；不能继续猜 policy URL、
   手写 Preferences 或用 `--load-extension` 冒充安装态。
5. 修 BrowserSession 状态机和完整浏览器退出 gate，再做 renderer/update/组合故障。
6. 补工具栏 activeTab/截图和 App UI 授权到真实 MCP 动作链。
7. 才进入安装、真实模型、Chrome/Edge、macOS/Linux、12 小时冻结长稳和发布审查。

详细依赖、门禁、场景数和停止条件见：

`docs/plans/2026-09-22-computer-use-grok47-96h-execution.md`

可直接交给 Grok 4.7 Goal/目标模式的完整提示词见：

`docs/plans/2026-09-22-computer-use-grok47-prompt.md`

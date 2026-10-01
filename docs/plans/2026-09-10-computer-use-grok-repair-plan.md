# Computer Use：Grok 审计回补开发计划

> [!CAUTION]
> 本文件保留 R0–R9 历史回补要求，不再是当前启动入口。最新状态与剩余批次见
> [Grok 剩余工作总路线](2026-09-10-computer-use-grok-remaining-roadmap.md)；当前只执行
> [B1 发行 Browser Runtime](2026-09-10-computer-use-grok-runtime-pack-execution.md)。

日期：2026-09-10  
工作区：`H:\aicoding\grok-app-computer-use`  
分支：`feat/computer-use-implementation`  
基线 HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`  

## 1. 本计划的地位

本计划是 [Computer Use 总计划](2026-09-09-computer-use-grok-step-plan.md) 的回补批次，不替换最终 S0–S12 目标。

在本计划 R0–R7 完成前：

- 暂停 S10.6 安装版 E4。
- 不得进入 S11/S12。
- 不得回到 S0 重做已有架构。
- 不得用旧账本的 `implemented/passed` 跳过这里列出的回归。

审计依据见 [2026-09-10 Computer Use Codex 接手审计](2026-09-10-computer-use-codex-audit.md)。执行结果继续追加到 [执行状态账本](2026-09-09-computer-use-execution-state.md)。

### 1.1 2026-09-10 当前执行指针

以下状态优先于本文第 3 节最初记录的 Red 基线：

| 原子项 | 当前状态 | 最新可信证据 |
| --- | --- | --- |
| R0.1 | passed（E0） | 工作树、分支、HEAD、并发进程和保护边界已记录 |
| R1.1 | passed（E1） | 修正旧 Browser fixture，后续 core 回归保持 Green |
| R1.2 | passed（E1） | MCP golden `4/4`、TypeScript Computer Use `15/15` |
| R1.3 | passed（E1/E2） | typed request structs；core 182/182、driver 12/12、Clippy/App/core check 历史 Green |
| R2.1 | passed（E1/E2） | typed Browser error envelope 主体完成 |
| R2.1H | passed（E1/E2） | 严格顶层 envelope、未知异常脱敏、真实 Rust client → Node HTTP 400 round-trip |
| R2.2-a | passed（E1） | pure observation state 7/7 |
| R2.2-b | passed（E1；错误链回归 E2） | PageState 8/8；navigation/reload/popup/close/rebuild/iframe invalidation |
| R2.2-c | **in_progress，当前唯一入口** | extract 5/5、Browser Node 30/30；production `/observe` 未接，URL scheme/候选节点/action-time 复核仍有缺口 |
| R2.3–R9 | not_started | 不得因旧 S 阶段日志里的 `implemented` 跳过 |

R1.3 已加入并应保留这些类型：`ManagedPageRef`、`ManagedWorkerAction`、`ManagedWorkerUpload`、`ManagedTabAction`、`SharedTabOffer`，以及可复制的 `TabAttachment`。不要删除后重写另一套类型。

R1.3 已收尾的调用点：

- `src-tauri/computer-use-core/src/broker/gates.rs`、`tests_lease_schema.rs` 和 `src-tauri/src/computer_use/extension_pair.rs` 的四处旧调用已改为 `TabAttachment`。
- `src-tauri/computer-use-core/src/browser.rs` 的 `needless_bool_assign` 已化为 `rec.info.closed = !rec.info.user_owned;`。
- `cargo fmt --all -- --check`、core/App check、177 core、12 driver 和 Clippy 均已恢复。

最新现场重跑：Rust fmt、core Clippy、App check、core 182/182、driver 12/12、MCP 4/4、前端定向 129/129、targeted ESLint、typecheck、Browser Node 30/30 均通过；App check 只有 3 个既有 dead-code warning，`git diff --check` 无 whitespace error。R2.2-c 的纯模块 Green 不能替代 production HTTP 集成。

当前详细执行包见 [Grok R2.2–R3 详细执行书](2026-09-10-computer-use-grok-r2-r3-execution.md)。从 R2.2-c 的 URL/候选节点安全 Red、production `/observe` 接线和 live HTTP contract 继续；R2.2-a/b 与 R2.1H 禁止返工。

## 2. 执行纪律

### 2.1 每次只做一个原子项

固定循环：

1. Read：读取本计划、账本和本项列出的源码。
2. State：在账本把本项标为 `in_progress`，列预计修改文件。
3. Red：先写会因目标行为缺失而失败的测试/fixture。
4. Implement：只实现本项，不顺手改平台、UI 或打包。
5. Targeted test：先跑最窄测试，检查真实后置条件。
6. Review：完整阅读本项 diff，检查竞态、身份、取消和错误分类。
7. Gate：运行本项门禁；失败不得进入下一项。
8. Record：记录命令、exit code、测试数、后置条件和未验证项。

一个原子项通过后自动继续下一项，不需要等待用户回复。若出现需要账号、正式安装、系统权限或 macOS/Linux 设备的步骤，只把该验证记为 `not_run/blocked`，继续不依赖这些资源的工作。

### 2.2 修改边界

- 保留全部未提交修改；禁止 reset、clean、checkout --、强制切分支。
- 不改 `H:\aicoding\grok-app` 或其他 worktree。
- 不碰账号、Token、Cookie、共享 `~/.grok`、系统代理、正式 Grok 安装数据。
- 不启动/关闭用户 Chrome、Edge、ChatGPT、微信或其他真实应用。
- 测试只使用自建 fixture、隔离 profile、随机 loopback 端口和专用 `GROK_APP_HOME`。
- 未经用户另行授权，不 commit、push、PR、merge、tag、release。
- 根目录只用 pnpm；不得运行根目录 `npm install`/`yarn`。
- 不通过 `allow`、skip、ignore、降低 lint 或改测试期望掩盖失败。

### 2.3 完成语义

- E0：源码/静态检查。
- E1：unit/mock/jsdom。
- E2：真实本机子进程或 loopback 集成。
- E3：自建原生/浏览器 fixture 的真实后置条件。
- E4：已安装 App + 真实 Grok 模型。
- E5：重复验收矩阵。

R0–R7 的目标是恢复可信基线并把 Browser 产品链闭环到 E2/E3。它们不等于三平台或整个 Computer Use 完成。

## 3. 初次审计 Red 基线（历史）

以下失败在 03:58 初次审计时已经复现，保留作修复来源；当前执行状态以第 1.1 节和执行账本末尾为准：

| 项 | 当前结果 |
| --- | --- |
| core lib | 177 tests：174 passed，3 failed |
| TS Computer Use | 15 tests：14 passed，1 failed |
| core clippy `-D warnings` | 7 errors |
| browser fixture | exit 1：`GROK_CU_BROWSER_TOKEN required` |
| Node worker syntax | passed |
| browser profile tests | 2 passed |
| driver integration | 12 passed |
| UI/i18n/settings | 117 passed |
| targeted ESLint | passed |
| `pnpm typecheck` | passed |

## 4. 依赖顺序

```text
R0 状态冻结
  → R1 恢复基础门禁
  → R2 Browser 契约与错误语义
  → R3 Worker 页面观察与动作
  → R4 Broker 生命周期与 Host 幂等
  → R5 模型工具面
  → R6 真实 browser fixture + supervisor
  → R7 App shell/文档/质量门禁
  → R8 私有 runtime pack
  → R9 desktop worker 真执行
  → 回到 S4–S12 平台/产品验收
```

R0–R7 是当前连续执行批次。R8、R9 在 R7 通过后继续；macOS/Linux 实机不在当前 Windows 主机伪造。

## 5. R0：冻结当前事实

### R0.1 工作区快照

读取：

- `AGENTS.md`
- 本计划
- `2026-09-10-computer-use-codex-audit.md`
- `2026-09-09-computer-use-execution-state.md`

执行：

```powershell
git status --short --branch
git rev-parse HEAD
git diff --check
Get-CimInstance Win32_Process | Where-Object {
  $_.CommandLine -like '*grok-app-computer-use*' -and
  $_.Name -match 'cargo|rustc|node|pnpm|grok'
} | Select-Object ProcessId,Name,CommandLine
```

退出门槛：

- 分支和 HEAD 与本计划一致；若不一致只记录并停止，不强切。
- 没有另一个 agent/build 正在改同一工作树。
- 将本计划 R0.1 标为当前项，不回写旧 S10.6 为已通过。

## 6. R1：恢复基础门禁

### R1.1 修正三条过期 core 测试

目标：保留 managed/user-owned tab 的边界，不为旧测试放宽产品代码。

修改原则：

1. `reserved_download_name_rejected`
   - 使用测试 helper 打开真实 managed profile/page。
   - 从返回的 `TabInfo` 取得 `tabId` 和 `pageGeneration`。
   - 保留“保留名在 worker 前拒绝、磁盘无文件”的断言。
   - safe filename 必须实际只调用 worker 一次并检查 staging 文件。
2. `navigate_records_actual_url`
   - 改用 managed page。
   - worker 返回不允许的实际 URL时拒绝；允许 URL时更新同一 page identity。
3. `browser_borrow_return_disconnect`
   - Existing Tabs 测试不得调用 managed worker 伪造导航。
   - 在扩展 transport 尚未实现时，`navigate` 应 fail-closed。
   - 借用/归还、用户导航、断线、重连分别测试；用户导航用 Host 收到的 extension event 模拟入口，而不是 managed goto。

禁止：

- 删除 `managed_tab_route` 的 `user_owned` 拒绝。
- 给用户 tab 伪造 profile/pageId。
- 因测试失败把 existing tab 当 managed tab。

门禁：

```powershell
cd H:\aicoding\grok-app-computer-use\src-tauri
cargo test -p grok-computer-use-core --lib --features test-support --offline --target-dir target-cu-review -- --test-threads=1
```

要求：177 项或新的明确计数全部通过；测试名保持按行为描述。

### R1.2 修正跨语言 catalog

事实：`computer_stop` 是模型可以请求的操作，不是模型可执行的 Host resume/authorize。

修改：

- `src/lib/computer-use/protocol.ts`
  - `computer_stop` 加入 `COMPUTER_USE_MODEL_TOOLS`。
  - 从 `COMPUTER_USE_HOST_ONLY_TOOLS` 移除。
- 核对 Rust、MCP、TypeScript、golden 的顺序和集合完全一致。

门禁：

```powershell
node --test tools\computer-use-mcp\protocol.golden.test.mjs
pnpm exec vitest run src\lib\computer-use
```

### R1.3 消除 clippy 结构问题

不要加 `#[allow(clippy::too_many_arguments)]`。新增强类型参数：

- `ManagedPageRef { page_id, page_generation }`
- `ManagedActionRequest { owner, profile, page, action_id, kind, params }`
- `ManagedUploadRequest { owner, profile, page, action_id, source }`
- Existing Tab 的 offer/grant 使用 `SharedTabOffer`、`SharedTabGrant`。

要求：

- identity 字段不能靠多个相邻 `&str` 的参数顺序传递。
- serde 名称与 Node 协议仍由 golden/fixture 锁定。
- 将 needless bool assignment 正常化，不加 lint allow。

门禁：

```powershell
cargo fmt --all -- --check
cargo clippy -p grok-computer-use-core --all-targets --features test-support --offline --target-dir target-cu-review -- -D warnings
```

R1 完成条件：core、TS golden、clippy 全绿；不得继续带着“已知红但无关”的说法进入 R2。

## 7. R2：Browser 契约与错误语义

### R2.1 定义单一 browser request/response

在 Rust 作为权威类型，Node 和 TypeScript/golden 对齐。至少定义：

- `BrowserTabRef { tabId, pageGeneration }`
- `BrowserObservation { tabId, pageGeneration, snapshotId, url, title, aria, nodes, image }`
- `BrowserActRequest { actionId, tabId, pageGeneration, snapshotId, action, target, parameters }`
- `BrowserActionOutcome { actionId, tabId, pageGeneration, kind, executed, replayed, reason }`
- `WorkerError { status, code, completion, message, currentPageGeneration? }`

`completion` 只允许：

- `not_started`：在任何副作用前明确拒绝，可映射 `rejected`。
- `unknown`：已派发、timeout、连接断开或不能证明未执行，映射 `unknown`。

禁止仅凭 HTTP 4xx/5xx 猜测是否执行。HTTP status 必须保留，`post_headers` 不得只返回 message 字符串。

### R2.2 页面与观察身份

- `tabId` 是 Host 对模型的稳定标识。
- worker `pageId` 只由 Host 解析/保存，不允许模型替换 profile/page route。
- 主 frame 导航递增 `pageGeneration`。
- 每次 browser observe 生成 `snapshotId`。
- DOM elementRef 只对 `{tabId,pageGeneration,snapshotId}` 有效。
- popup/new tab 拥有独立 pageId/tabId。
- 关闭页面后旧 identity 永久拒绝，不能自动选“当前页”。

### R2.3 actionId 语义

- actionId 格式和长度在 Host 与 worker 同时校验。
- fingerprint 包含 tab、page generation、snapshot、action、target、parameters。
- 相同 actionId + 相同 fingerprint：返回原结果，不二次执行。
- 相同 actionId + 不同 fingerprint：`rejected`，零执行。
- pending 重试：明确 in-flight，不二次执行。
- unknown：同 actionId 永远返回原 unknown；新 action 必须先有更新的 observe。

R2 测试至少覆盖错误 envelope round-trip、status/completion 保存和所有 action 状态转换。

## 8. R3：Worker 页面观察与动作

### R3.1 修复 `run-fixture.mjs` 的基础协议

runner 必须：

- 生成随机 `GROK_CU_BROWSER_TOKEN` 并通过 Authorization Bearer 发送。
- `/open` 后保存真实 pageId/pageGeneration。
- 所有 page 定向请求带 page identity。
- 所有写请求带唯一 actionId。
- 导航后使用返回的新 generation。
- `/shutdown` 后等待 worker/browser 子孙退出。
- 临时输出只写 `.run/`，不得打印 token。

### R3.2 Browser observe 加视觉与 opaque refs

`/observe`：

- 使用 `page.screenshot({ type: 'png' })`，设置最大宽高/字节。
- 返回 PNG base64、width/height、page identity、snapshotId。
- 提取有界 ARIA/可交互节点列表。
- 为节点生成 observation-scoped opaque `elementRef`。
- 模型 schema 不接收任意 CSS selector；selector 只留在 Host fixture/private worker 内部。
- cross-origin iframe 只能报告能力边界，不绕过同源/权限。

### R3.3 Typed actions

至少覆盖：

- click
- fill/set_value
- type_text
- select
- key
- scroll
- drag
- wait
- navigate

每个动作按 [R2.2–R3 执行书第 4.1 节](2026-09-10-computer-use-grok-r2-r3-execution.md#41-按-action-kind-固定身份要求) 的 action-specific identity matrix 校验；不得给 navigate/reload/wait/page-level key 或 scroll 伪造 elementRef。副作用动作在执行前检查 AbortSignal，执行后再检查取消和真实页面状态；wait 保持有界只读语义。返回当前 page generation 和可验证后置条件，禁止任意 evaluate/CDP/shell/file URL。

### R3.4 Worker 行为测试

必须新增真实 Playwright fixture，逐项命名：

1. 两个普通 tab 定向操作不互串。
2. popup 有独立 pageId。
3. 导航后旧 generation 拒绝且零副作用。
4. 同 actionId 相同请求只执行一次并回放同结果。
5. 相同参数不同 actionId 确实执行两次。
6. 同 actionId 不同参数拒绝。
7. 旧 snapshot/elementRef 拒绝。
8. cancel slow action 后计数器/提交后置条件不发生。
9. close/shutdown 后无 browser/worker 残留。
10. `..`、slash、backslash profile/owner 通过 HTTP 实测拒绝。
11. run A staging 文件不能由 run B 上传。
12. 下载重定向、保留名和超限文件不落盘。

门禁：

```powershell
node --check tools\computer-use-browser\server.mjs
node --test tools\computer-use-browser\*.test.mjs
node tools\computer-use-browser\run-fixture.mjs
```

必须检查页面计数/文本、下载字节和进程退出，不只检查 HTTP 200。

## 9. R4：Browser 全部进入 Broker 生命周期

### R4.1 单一派发入口

新增 Broker 内部 browser dispatch，不允许 `tools.rs` 直接调用 `tabs.navigate/stage_download/act`。

派发前顺序：

1. feature enabled。
2. session/run owner 正确。
3. `stop == running`。
4. 非 paused。
5. 已授权 tab 属于该 run。
6. pageGeneration/snapshot 当前。
7. unknown 已通过新的 observe 清除。
8. action budget 未耗尽。
9. actionId fingerprint 检查。
10. CAS 取得同一个 run-level `in_flight`。

派发后：

- generation/run/tab 改变时 late result 只能是 unknown/ignored，不更新新状态。
- `not_started` → rejected；明确完成 → applied/verified；其他 → unknown。
- in-flight guard 在所有返回/异常路径释放。
- trace 不记录 URL query、表单值、截图、token、cookie 或本地 profile path。

### R4.2 Host action ledger

worker 内缓存保留作第二道防线；Broker 的 `RunState` 增加 browser action ledger，worker 重启后仍不会重放已知副作用。

要求：

- ledger 不因容量淘汰旧 actionId 后允许重放。
- persisted run 只保存安全审计摘要，不恢复授权/snapshot；恢复后不能自动执行旧 action。
- action budget 同时覆盖 desktop 与 browser 写操作，不能分别绕过总预算。

### R4.3 pause/stop/cancel

区分：

- `cancel-actions`：pause/takeover 使用，只终止当前动作，不关闭 profile。
- `cancel-run`：stop/session close/App exit 使用，终止动作并关闭 App-owned contexts。
- borrowed user tab 永不被 stop 关闭，只撤销 grant/归还。

若 desktop abort 或 browser cleanup 失败：

- 保持 `stop_requested`/cleanup pending。
- 不报告 stopped。
- 可安全重试 cleanup，但不能派发新动作。

### R4.4 Broker 竞态测试

必须覆盖：

- paused/stop 后零新 browser 派发。
- browser 与 desktop 并行调用共享 in-flight/budget。
- stop 中止 slow browser action；后置条件不发生或结果明确 unknown。
- worker close 失败时 Host 不得报告 stopped。
- worker 重启后相同 actionId 不重复执行。
- generation 变化后的 late result 不写回。
- unknown 后无新 observe 不允许新 action。

## 10. R5：模型工具面

### R5.1 工具集合

在现有 Computer Use MCP 中增加并锁定：

- `browser_list_tabs`
- `browser_open`
- `browser_observe`
- `browser_act`

保留 `computer_navigate` / `computer_download` 时，也必须迁入同一 Broker browser dispatch，并要求：

- `actionId`
- `tabId`
- `pageGeneration`
- 对需要 observation 的动作要求 `snapshotId`

语义：

- list 只返回本 run 已拥有或已借用的 tab，不泄漏其他标题/URL。
- open 只选择用户已授权或该 run 自有的 tab，不创建授权、不打开任意 profile。
- observe 返回 text + image content。
- act 只接受 typed action 和当前 observation 的 elementRef/坐标。

Host-only 继续包括：authorize、resume、reconnect、pause、profile create/clear、extension pairing、系统权限。

### R5.2 单一协议更新面

同步修改并测试：

- `src-tauri/computer-use-core/src/tools.rs`
- Rust protocol/golden testdata
- `tools/computer-use-mcp/protocol.mjs`
- `src/lib/computer-use/protocol.ts`
- MCP descriptions 和 agent loop tests

工具描述必须明确：

- observe → act → verify。
- unknown 不得盲重试。
- stop 不能 resume。
- browser 工具不能读取 Cookie/Token/storage。
- screenshot 缺失时坐标动作拒绝。

### R5.3 模型可见内容

- PNG 必须是 MCP image content，不把 base64 塞进 text JSON。
- text JSON 省略 png bytes。
- title/URL/ARIA/节点有总大小上限和截断标记。
- 纯文本 provider 只开放语义动作；不能假装看到坐标图。

门禁：Rust、Node、TS 三份 catalog 和 schema 全等；新增负向测试证明模型不能调用 Host-only 工具。

## 11. R6：Host supervisor 真实链路

### R6.1 App-owned 启停

使用真实 `browser_supervisor`：

- Host 分配随机端口/token。
- worker handshake 返回 protocol/build/runtime/page capability。
- stderr drain 和 process tree 回收仍生效。
- App exit/session stop 后 worker + Chromium 子孙全部退出。
- 不依赖手工 `GROK_CU_BROWSER_IPC`。

### R6.2 E2/E3 集成矩阵

至少连续运行 3 次：

- open profile → two tabs → observe image/ARIA → typed acts → verify。
- popup/iframe。
- navigation generation。
- upload/download staging。
- stop/cancel/crash/restart/actionId。
- 两个 run 的 profile/tab/staging 隔离。

每次记录：环境、浏览器 executable/version、Node/runtime、耗时、退出码、真实页面/文件/进程后置条件。

R6 完成不等于用户 Chrome/Edge Existing Tabs E4。

## 12. R7：App shell、文档与完整回归

### R7.1 AppWorkbench 增长冻结

把 `computer-use` / `computer-use-browser` slash action 分发移入领域 helper/hook。要求：

- `src/app/AppWorkbench.tsx` 相对 HEAD 净行数不增加。
- 不在 App.tsx/AppWorkbench 新增 Computer Use `useState` 或大型分支。
- slash 仍只打开面板，不发送假聊天命令。

### R7.2 文件拆分

先拆再继续堆功能：

- `browser.rs`：types、managed host、existing tabs、validation、tests 分模块。
- `browser_supervisor.rs`：process、handshake、platform process-tree、tests 分离。
- 大型 fixture 按行为拆文件。

拆分必须机械、可审查；不得同时改变行为。拆前后运行同一门禁并记录计数。

### R7.3 账本纠偏

更新执行状态：

- 不删除旧日志。
- 阶段总表按最新证据更新。
- S7/S10 只有 R0–R7 全部通过后才恢复到相应 `implemented`。
- S5/S6/S8 E4/S10 E4 继续 `not_run`。
- placeholder runtime 使 S2 继续 `in_progress`。

### R7.4 完整本地门禁

```powershell
cd H:\aicoding\grok-app-computer-use\src-tauri
cargo fmt --all -- --check
cargo clippy -p grok-computer-use-core --all-targets --features test-support --offline --target-dir target-cu-review -- -D warnings
cargo check -p grok-computer-use-core --features test-support --offline --target-dir target-cu-review
cargo test -p grok-computer-use-core --lib --features test-support --offline --target-dir target-cu-review -- --test-threads=1
cargo test -p grok-computer-use-core --test driver --features test-support --offline --target-dir target-cu-review -- --test-threads=1
cargo check -p grok-app --offline --target-dir target-cu-review

cd H:\aicoding\grok-app-computer-use
node --check tools\computer-use-browser\server.mjs
node --test tools\computer-use-browser\*.test.mjs
node tools\computer-use-browser\run-fixture.mjs
node --test tools\computer-use-mcp\protocol.golden.test.mjs
pnpm exec vitest run src\lib\computer-use src\components\computer-use src\i18n\messages.test.ts src\lib\settingsCatalog.test.ts src\lib\slashCatalog.test.ts
pnpm exec eslint src\components\computer-use src\lib\computer-use src\lib\api\computerUse.ts src\app\AppWorkbench.tsx
pnpm typecheck
git diff --check
```

若 `cargo test -p grok-app` 遇到已知 `0xc0000139`，只记录 App test harness 环境问题；不得用它解释 core/browser/Node 测试失败。

## 13. R8：真实私有 runtime pack

R7 通过后继续，不等待 S10.6。

### R8.1 移除 placeholder 成功路径

- placeholder `js-runtime`/worker 不得被 diagnose/repair 报 healthy。
- 固定 Node 或等价 JS runtime 的精确版本、平台、架构、SHA-256。
- 固定 `playwright-core` 精确版本，不用 caret。
- 固定 worker source hash 和兼容的浏览器策略。
- manifest 包含 protocol/build/runtime/browser compatibility。

### R8.2 四 target pack

分别准备：

- Windows x64
- macOS arm64
- macOS x64
- Linux x64

缺平台构建机时可完成 manifest/schema/CI，但验证只能 `not_run`。不得复制 Windows binary 改名冒充。

### R8.3 安装/repair

- clean install 可从 bundle 原子激活 pack。
- interrupted repair 回滚到旧 pack。
- wrong hash/arch/version 拒绝。
- 不查 PATH，不下载用户未同意的运行时。
- App 无 pack 时明确 unavailable，不回退系统 Node。

## 14. R9：desktop worker 真正执行 observe/act

R8 后将 OS adapter 移到受控 companion worker，App 进程只保留 Broker/授权/IPC/UI。

要求：

- worker 执行 list/observe/act/abort，不只是 `prepare_dispatch`。
- 进程崩溃、timeout 和取消按 completion 分类。
- target identity 在 worker 内再次验证。
- Windows fixture 证明动作发生在 worker PID，并在 stop 后无输入。
- macOS/Linux 代码分别在其平台构建和实机运行；Windows 交叉/静态检查不算通过。

## 15. 回到总计划后的顺序

R0–R9 后：

1. Windows 本分支安装版 E4/E5。
2. macOS AX/capture/input/TCC，arm64+x64。
3. Linux X11 AT-SPI/XTest。
4. GNOME Wayland portal/PipeWire/libei；XWayland 不算 native。
5. Chrome 与 Edge 用户安装扩展 E4。
6. App WebView 产品 surface E4。
7. S11 traces/privacy/diagnostics/performance/reliability。
8. S12 四 target clean install/update/rollback/uninstall + E5 矩阵。

## 16. Grok 每项固定汇报格式

```text
当前项：R?.?
改了什么：
Red 证据：命令 / exit code / 为什么失败
Green 证据：命令 / exit code / 测试数 / 真实后置条件
未验证：
安全复核：身份 / 权限 / 取消 / 重放 / 数据泄漏
计划差异：
下一项：
```

任何一句“完成”都必须指出完成的是实现、E1、E2、E3、E4 还是 E5。

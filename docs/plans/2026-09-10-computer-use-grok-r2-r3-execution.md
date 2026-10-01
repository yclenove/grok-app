# Computer Use：Grok R2.2–R3 详细执行书

> [!CAUTION]
> 本批已执行完毕，只作历史证据，禁止再作为启动入口。当前唯一执行入口是
> [B1 发行 Browser Runtime 执行书](2026-09-10-computer-use-grok-runtime-pack-execution.md)
> 和对应的[长任务提示词](2026-09-10-computer-use-grok-runtime-pack-prompt.md)。

日期：2026-09-10  
工作区：`H:\aicoding\grok-app-computer-use`  
分支：`feat/computer-use-implementation`  
基线 HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`  
本批范围：R2.2、R2.3、R3（R2.1H 已由 Codex 完成并作为回归门禁）  
本批停止点：R3 全部门禁和三次 fixture 通过后，停止改代码并交付审查报告

## 1. 权威性与目标

本文替代下列文件作为**当前这一批**的执行入口：

- `2026-09-10-computer-use-grok-next-batch.md` 中已经完成的 R1.3/R2.1 起点；
- `2026-09-10-computer-use-grok-repair-prompt.md` 的旧启动正文。

总目标仍由 `2026-09-09-computer-use-grok-step-plan.md` 和 `2026-09-10-computer-use-grok-repair-plan.md` 约束。R3 后停止只是一次代码审查边界，不代表 Computer Use Goal 完成。审查通过后继续 R4–R7，再继续 R8–R9 和三平台产品验收。

本批只解决三件事：

1. 建立 Browser page/observation/node 的单一身份链。
2. 建立可证明至多执行一次、且 run-wide 单写的 worker actionId 状态机。
3. 让 App-owned Playwright worker 在自建页面完成截图、语义节点、typed actions 和可重复 Host→worker fixture。

## 2. 当前可信 checkpoint

### 已完成，禁止返工

- R0.1 工作区冻结。
- R1.1 managed/user-owned 旧测试修正。
- R1.2 `computer_stop` 跨 Rust/Node/TypeScript catalog 对齐。
- R1.3 typed request 参数对象化。
- R2.1 typed Browser error envelope 主体。
- R2.1H 严格顶层 envelope、未知异常脱敏、真实 Rust→Node HTTP 400 typed error 往返。
- R2.2-a pure observation state：opaque snapshot/ref、私有 target map、严格失效与泄漏负向测试，7/7。
- R2.2-b PageState：navigation/reload/popup/close/rebuild/iframe invalidation，8/8；`browser-error-contract` 回归通过。

R2.2-c 当前是**核心模块 Green、production 集成未完成**：

- `observation-extract.mjs` 与 focused tests 已存在，5/5。
- Browser Node 全套 30/30。
- production `server.mjs` 的 `/observe` 仍走旧 ARIA/roles 分支，没有调用 `captureManagedObservation`。
- 本次审查确认 `safeDisplayUrl("data:text/plain,SECRET_PAYLOAD")` 会原样返回 secret；修复前不得接入 production。
- 裸 `[role]` 会把非交互 role 纳入 64 node cap；action-time handle/signature 复核尚未闭环。

本次现场重新验证：

- Rust fmt：passed。
- core Clippy `-D warnings`：passed。
- App check：passed，只有 3 个既有 dead-code warning。
- core：182/182。
- driver：12/12。
- MCP golden：4/4。
- Computer Use/UI/i18n/settings/slash 定向 Vitest：129/129。
- targeted ESLint / TypeScript：passed / passed。
- Browser Node 全套：30/30。
- `git diff --check`：无 whitespace error；仅本机 autocrlf 提示。

`browser-error-contract` 在 R2.2-b 后为 Green；R3 最终门禁仍必须全部重跑，不能拿当前结果替代最终验收。

### 当前唯一 Red/实现入口

R2.2-c 先新增以下行为测试并观察具体 assertion Red：

1. `data:`、`file:`、`javascript:` 和无效 scheme 的 display URL 不得包含原始 payload、本地路径或 secret；只允许 `http:`、`https:` 和精确 `about:blank`。
2. 页面前 64 个非交互 `[role]` 不得饿死后面的真实 button/link/textbox；hidden/inert/`aria-hidden` 策略固定。
3. production `/observe` 通过真实 loopback HTTP 返回 fresh `snapshotId`、opaque nodes 和 typed 409；第二次 observe 使第一次 refs 失效。
4. worker HTTP JSON 必须含供 Rust 核对的 `pageId`，但不含 selector、ElementHandle、profile、URL query value、data/file payload 或测试 secret；模型 projection 另行移除 `pageId`。

修复 pure 安全缺口后，再把 `captureManagedObservation(selected.state)` 最窄接入 `/observe`。R2.2-c focused、live HTTP 和 Browser Node 全套都 Green，才可记录 complete 并进入 R2.2-d。

### R2.1H 已完成边界

R2.1H 已完成，不重写、不重复追加 Red：

1. Rust error parser 严格要求顶层 `ok:false` 及全部 typed 必需字段；5/5 focused tests。
2. 未 branded Node/Playwright exception 固定归一为安全 `worker_internal/unknown`；Node error tests 8/8。
3. 真实 `BrowserSupervisor` → `LoopbackPlaywrightWorker` → Node HTTP 400 往返通过。

尚未被 R2.1H 证明：带 `currentPageGeneration` 的真实 409、2xx `ok:true` 成功 envelope、成功 open/observe/act/screenshot 往返。它们必须在 R2.2/R3 完成，不能从 HTTP 400 外推。

## 3. 全批纪律

每次只执行一个原子项：

```text
Read → State → Red → Implement → Targeted test → Diff review → Gate → Record
```

- 开始时向 `2026-09-09-computer-use-execution-state.md` 追加 `in_progress`、预计文件和 Red。
- 当前项不绿，不进入下一项。
- 结束时追加命令、exit code、测试数、真实后置条件、证据等级、安全复核、未验证项和下一项。
- 旧日志只追加更正，不删除历史。
- 不 commit、push、PR、merge、tag、release。
- 不 reset、clean、checkout --、stash，不切换或覆盖其他 worktree。
- 不碰账号、Cookie、Token、代理、正式安装、共享 `~/.grok` 或用户真实 Chrome/Edge profile。
- 测试只用随机 loopback 端口、随机 Bearer、隔离 profile/staging/GROK_APP_HOME、自建页面和 `target-cu-review`。
- 根目录只用 pnpm；Rust使用 `--offline`。
- 不加 lint allow、skip/ignore、测试专用成功分支或宽松断言。
- 不向 `App.tsx` / `AppWorkbench.tsx` 添加 Computer Use state 或大型分支。
- 新代码优先落在小模块，不继续把 `browser.rs`、`server.mjs` 和 `browser_supervisor.rs` 无限堆大。
- shared worktree 只允许一个 writer。Grok 不得让多个子代理并行编辑代码或账本；只读审查/测试可并行。每个原子项前后检查 `git status`/目标 diff，若出现非本原子项变化，先停下协调。
- `2026-09-09-computer-use-execution-state.md` 只由当前协调者追加，禁止多个 agent 交错写入。
- 每个新契约必须在只改测试后看到具体 assertion Red；已被现有测试覆盖则记 `already-covered`，禁止故意破坏 production 代码制造 Red。
- 当前 checkpoint 三大文件基线：`browser.rs=2975`、`browser_supervisor.rs=1678`、`server.mjs=990` 行，合计 5643。结束时三者合计净行数不得增加；新增实现拆入 observation、action schema/ledger、download 等小模块。

证据等级：

- E0：源码/静态检查。
- E1：unit/mock/jsdom。
- E2：真实 loopback/子进程。
- E3：自建真实页面/原生 fixture 后置条件。
- E4：已安装 App + 真实 Grok 模型。
- E5：重复产品验收矩阵。

本批最高只能证明 Browser E2/E3，不能写三平台或完整 Computer Use 完成。

## 4. 固定 Browser 身份契约

| 身份 | 所属层 | 模型可见 | 失效条件 |
| --- | --- | --- | --- |
| `runId` | Host session binding + 请求回显 | 是；必须等于 binding，不能借此授权 | session/fork/reconnect/stop |
| `tabId` | Host 稳定引用 | 是 | tab close、grant/reconnect 失效 |
| `profile` | Host/worker 私有路由 | 否 | profile close/clear/worker rebuild |
| `pageId` | worker 私有路由 | 否 | page close/worker rebuild |
| `pageGeneration` | worker + Host | 可在 observation 中返回；模型不得自选路由 | 每次主 frame navigation、reload、process rebuild |
| `snapshotId` | 单次 observe | 是 | 新 observe、navigation/reload、close、写动作、unknown |
| `elementRef` | 单次 snapshot 的 opaque 节点 | 是 | snapshot 的任一失效条件 |
| `actionId` | 单次副作用请求 | 是 | 不复用为另一动作；原结果可查询/回放 |

硬规则：

- 模型输入不得包含或覆盖 `profile`、`pageId`、selector、CDP session。
- `elementRef` 只能在 `{runId, tabId, pageGeneration, snapshotId}` 内解析。
- tab/page/snapshot/ref 任一不匹配，Host 和 worker 均须在副作用前拒绝。
- stale 拒绝必须证明 worker/页面计数器为零变化。
- 同 URL reload 也必须递增 `pageGeneration`；不能只比较 URL 字符串。
- tab close 后绝不回退“当前 tab”或列表第一项。

### 4.1 按 action kind 固定身份要求

不得给不需要元素的动作伪造 dummy `elementRef`：

| 动作 | 必需身份/目标 | observation 处理 |
| --- | --- | --- |
| list/open/observe | Host binding + tab；observe 的 worker 私有路由含 profile/pageId/generation | observe 创建新 snapshot |
| click/fill/type/select | pageGeneration + snapshotId + elementRef + actionId | 成功/rejected-after-dispatch/unknown 后失效；重新 observe 验证 |
| key | pageGeneration + snapshotId + actionId；目标显式为 page 或 elementRef | 有副作用后失效；不得造 dummy ref |
| scroll | pageGeneration + snapshotId + actionId；目标显式为 page、elementRef 或当前 image coordinate | 有副作用后失效 |
| drag | pageGeneration + snapshotId + actionId；source/destination 各为 ref 或当前 image coordinate | 有副作用后失效 |
| wait | snapshotId + elementRef + allowlisted typed condition（如 `nameEquals`）；condition 不是 locator | 有界只读等待，不使用副作用 actionId；generation/snapshot 未变时可保持；timeout 不是成功 |
| navigate/reload | tab + 当前 pageGeneration + actionId；不要求 elementRef，也不要求旧 DOM snapshot | committed navigation 后 generation +1 并清空 observation |
| upload/download | pageGeneration + actionId；上传目标用 elementRef，下载目标用 ref/受控页面动作；文件只用 Host staging handle | 完成或不确定后清空 observation |

所有会产生副作用的动作都校验 owner/run、私有 profile/page 路由、pageGeneration、actionId 和 cancellation。只有依赖刚看到页面内容的动作才要求 snapshot；只有元素动作才要求 elementRef；只有坐标动作才要求当前 screenshot identity/coordinate space。

## 5. 固定 observation 边界

沿用 core 权威上限，不另造更宽的 Browser 上限：

- 节点最多 `OBSERVATION_NODE_CAP`（当前 64）。
- PNG base64 最多 `OBSERVATION_PNG_B64_CAP`（当前 400000 chars）。
- name、role、title、URL、ARIA 和 observation JSON 还必须分别有显式上限。
- 任一截断都设置 `truncated=true`；被截掉或自身 truncated 的节点不能 act。
- 明确区分全局 `observation.truncated` 与节点自身 `node.truncated`：未返回的溢出节点绝不分配 ref；字段被截断的节点不可 act；已完整返回节点在全局截断时是否可 act 必须固定为一条跨 Node/Rust 契约并写测试，不能各层自行猜测。
- worker 私有 HTTP 可以用有界 `pngBase64`（或单独有界二进制响应）把截图交给 Rust Host；Rust 校验后，模型 text projection 必须移除 `pageId`、`profile` 和 `pngBase64`，PNG 只进入 MCP image content。
- PNG 不进入模型 text JSON、trace、stderr 或错误 message。
- 截图不可用时明确 `textOnly=true`/等价 typed 字段；坐标动作必须拒绝。
- selector/Locator/ElementHandle 只存在 worker 私有 map，不出现在 public node、模型 projection/schema 或日志；worker 私有 envelope 仍必须回传 `pageId/pageGeneration/snapshotId` 供 Rust 严格核对。
- worker 可以保留完整实际 URL 用于导航与重定向校验；模型/trace/error 使用单独的安全 display URL，移除 userinfo、fragment 和 query values，并用 `hasQuery`/等价标记说明发生了脱敏。
- observation 必须原子发布：ARIA、nodes、截图和 geometry 提取前后复核 page/main-frame generation；任一主 frame/被观察 iframe navigation 或 detach、目标节点 detach/signature 变化都使该候选 observation 失败或对应 ref 失效，不能拼接不同时刻的数据。
- 坐标动作绑定 `{snapshotId,imageContentId,width,height,scale,crop,scroll,viewport}`；NaN、Infinity、负数、越界或旧图片 identity 在 Host 和 worker 均零派发。

建议新增小模块：

- Node：`observation-state.mjs`、`observation-extract.mjs`。
- Rust：`computer-use-core/src/browser/observation.rs`，由现有 `browser.rs` 声明并 re-export。

不要在本批机械拆整个 `browser.rs`；完整拆分留在 R7，但本批新增代码不得继续全部塞进单文件。

## 6. R2.1H：已完成的回归门禁（禁止返工）

### R2.1H-a 顶层错误 envelope

已完成并保留以下测试：

1. `{ok:true,error:{合法 typed error}}` 必须 fail-closed 为 `invalid_worker_response/unknown`。
2. 缺 `ok`、`ok:null`、`ok:"false"` 均拒绝。
3. `ok:false` 但缺 error/status/code/completion/message 拒绝。
4. HTTP status 与 envelope status 不一致拒绝。
5. 未知额外字段继续兼容，不影响必需字段严格性。

已完成实现：

- `parse_worker_error_response` 先要求顶层 `ok == false`，再解析 `error`。
- 不根据 message 猜 code/completion。
- 无法证明未开始的畸形/transport/timeout 一律 `unknown`。

Targeted gate：

```powershell
cd H:\aicoding\grok-app-computer-use\src-tauri
cargo test -p grok-computer-use-core worker_error_contract_tests --offline --target-dir target-cu-review -- --nocapture
```

### R2.1H-b 未知异常脱敏

先加 Red，构造未知异常 message，包含：

- `https://example.test/path?token=SECRET`
- `#password-field`
- `C:\Users\Example\profile`
- `Bearer SECRET`
- 超过 1024 字符的文本

要求返回的模型/Host typed envelope 不包含上述值。只有本模块 `workerException()` 创建且带模块私有 brand（例如私有 `WeakSet`）的错误，才允许保留已审查的安全 message 和 `not_started`。任意未 branded `Error` 或 throw object，即使伪造 `statusCode`、`workerCode`、`completion`、`safeRejected` 字段，也必须归一为 `worker_internal/unknown` 与固定安全摘要。加入伪造字段负向测试。

Targeted gate：

```powershell
cd H:\aicoding\grok-app-computer-use
node --test tools\computer-use-browser\worker-errors.test.mjs tools\computer-use-browser\worker-error-http.test.mjs
```

### R2.1H-c 真实跨边界错误 probe

不得增加生产“强制报错”HTTP endpoint。增加 feature-gated 的 `browser-error-contract` probe gate：由 `browser_supervisor` 启动现有真实 worker，使用真实 `LoopbackPlaywrightWorker` 诱发安全、可重复的 400/409（例如非法 profile 或 stale generation），断言 Rust 收到的：

- HTTP status；
- stable code；
- `completion=not_started`；
- current generation（适用时）。

扩展 `cu_probe` 的参数分发，使以下命令只跑该 gate，不串行执行全部平台/产品探针：

```powershell
cd H:\aicoding\grok-app-computer-use\src-tauri
$cuNodePath = (Get-Command node -ErrorAction Stop).Source
$env:GROK_CU_NODE_FILE = $cuNodePath
cargo run -p grok-computer-use-probe --bin cu_probe --offline --target-dir target-cu-review -- browser-error-contract
Remove-Item Env:GROK_CU_NODE_FILE
```

`GROK_CU_NODE_FILE` 只允许这个隔离开发 probe 使用；产品 supervisor 仍只接受 App-owned runtime pack，不得新增 PATH fallback。若环境没有 Chromium，可把需要页面的 409 标成 `not_run`，但不需要浏览器的真实 HTTP 400 必须运行。probe 自己生成随机 Bearer/port/temp roots，结束后验证 worker 退出。

R2.1H 后续回归条件：

- focused Rust/Node tests 全绿；
- 至少一条真实 Rust client → Node HTTP 400 typed error 往返通过；
- 没有 token/query/selector/profile path 泄漏；
- 全部 Browser Node tests 无回归；新增 R2.2 behavior Red 只允许在对应原子项的 Red 阶段短暂存在。

当前已满足：Rust 5/5、Node 8/8、真实 HTTP 400 gate PASS。R2.2 开始后只把这些当 regression gate，不再改写 R2.1H 历史或重新制造 Red。

## 7. R2.2：页面、观察与节点身份

### R2.2-a pure observation state（已完成，禁止返工）

读取：

- `observation-state.test.mjs`
- `worker-errors.mjs`

实现 `observation-state.mjs`：

- `createObservationState(pageGeneration, targets)`
- `resolveObservationTarget(observation, request)`
- `invalidateObservationState(observation)`

要求：

- `snapshot-${randomUUID()}`、`element-${randomUUID()}` 或等强度随机 opaque ID。
- public node 仅含允许字段；private target 存在不可序列化/不会显式输出的 map。
- 检查顺序固定：无 observation → generation → snapshot → elementRef。
- error codes：`observation_required`、`stale_observation_generation`、`stale_snapshot`、`stale_element_ref`。
- 所有这些错误均为 `completion=not_started`。
- 重复/空 ref、非法 generation、超长字段 fail-closed。
- `JSON.stringify` public observation 不得出现 target、selector、profile、pageId 或 Locator 细节。

历史执行已按 bootstrap Red→behavior Red→7/7 Green 完成。保留这些要求作为回归契约，不再重新制造 `MODULE_NOT_FOUND` 或重复记录 Red。

### R2.2-b PageState 与失效规则（已完成 E1，真实 Chromium 留 R3）

给每个 worker `PageState` 增加当前 observation。必须覆盖：

| 事件 | pageGeneration | observation |
| --- | --- | --- |
| 新 observe | 不变 | 替换，旧 ref 失效 |
| 任意主 frame committed navigation | +1 | 清空 |
| 同 URL reload | +1 | 清空 |
| popup | 独立从 1 开始 | 独立 |
| page close | page 删除 | 清空，不能 fallback |
| worker/context rebuild | 新 page identity/generation | 旧 Host identity 失效 |
| 写动作成功 | 返回真实当前 generation | 清空，要求 verify observe |
| 写动作 unknown | 不猜是否变化 | 清空并锁定重新 observe |
| 被观察 iframe navigate/detach | generation/signature 复核 | 关联 ref 失效；不能继续使用旧 target |
| 目标节点 detach/replaced | pageGeneration 可不变 | 目标签名不匹配，ref 必须 stale |

不要再用 `nextUrl !== lastUrl` 判断是否换代。需要处理 Playwright 首次页面注册与后续 navigation 事件，避免初始 about:blank 自增两次而测试靠魔法数字通过。

Red tests 必须先证明：

- 两个 tab 的 ref 不互通；
- 新 observe 后旧 ref 零副作用；
- navigation 和同 URL reload 后旧 ref 零副作用；
- close 后旧 page/ref 不落到别的 tab；
- popup 有独立 identity。
- observe 采集期间发生 main frame/iframe navigation 或 detach 时不发布混合 snapshot。

### R2.2-c 有界节点提取

`/observe` 创建 fresh snapshot，并提取有限的可交互节点。至少覆盖 button、link、textbox、checkbox、radio、combobox、option、menuitem、tab 和可聚焦元素。

每个 public node 至少返回：

```json
{
  "elementRef": "element-opaque",
  "role": "button",
  "name": "Submit",
  "disabled": false
}
```

可选 bbox 必须明确与 screenshot 同一 CSS/image coordinate space；在 R3.2 截图完成前不得让模型做坐标 act。

要求：

- 不把 selector、nth index、DOM path、ElementHandle 或 pageId 放进 public node 或模型 projection；worker 私有 envelope 的 pageId 必须保留并由 Rust 核对。
- 节点、文本、深度和总字节均有上限。
- 顺序稳定；相同 snapshot 内 ref 唯一。
- cross-origin iframe 不绕权限；能力不足时返回 typed limitation。
- 被截掉节点无法通过猜测 ref act。
- observation 的模型 display URL 必须移除 userinfo、fragment 和 query values；加入带假 token query 的负向测试。
- ARIA、nodes、PNG、URL 与 geometry 必须在同一次 generation CAS 中发布；采集前后 identity 不一致则丢弃并要求重试。
- 坐标 metadata 必须含稳定 image content identity 与 width/height/scale/crop/scroll/viewport；在 R3.2 完成并验证前坐标动作保持禁用。

当前接力状态：`observation-extract.mjs` pure tests 5/5，Browser Node 30/30，但本项未完成。按以下顺序收尾：

1. 先只改测试，为 `data:`/`file:`/`javascript:` payload 与本地路径泄漏、裸 `[role]` 饿死真实控件、hidden/inert/`aria-hidden` 策略建立具体 assertion Red。
2. 修复 safe display URL：只允许 `http:`、`https:` 与精确 `about:blank`；不支持的 scheme 返回固定、不含输入原值的结果。
3. 把节点候选限制为 native controls、allowlisted interactive roles 和真实 focusable/contenteditable；不得让非交互 role 占满 64 cap。disabled 可进入观察但不可 act；hidden/inert/`aria-hidden` 不分配可操作 ref。
4. 在 `server.mjs` 导入 `captureManagedObservation`，将旧 `/observe` ARIA/roles 分支替换为最窄调用。branded exception 必须继续经统一 typed error envelope 返回。
5. 新增 live loopback HTTP contract：第一次/第二次 observe snapshot 不同；旧 ref 失效；navigation/reload/iframe change 返回 typed 409；public JSON 无 query/data/file/profile/selector/handle secret。
6. 明确同源 iframe 当前是支持还是 typed omission。若本批仍只观察 main frame，response 必须显式说明，不得让计数 summary 冒充已支持。
7. capture-time `isConnected` 不是 action-time 安全证明。把私有 handle/signature 元数据固定下来，并在 R3.3 前加动作前 connected/signature/page/frame CAS；不得按 selector 猜测恢复。
8. 删除或明确标记临时 `roles` 字段的迁移边界；R2.2-d Rust strict schema 完成时 production response 不得保留无人消费的双轨结构。

本项退出条件：focused pure tests、live `/observe` HTTP tests、Browser Node 全套、R2.1H error probe 回归全绿；production `/observe` 静态搜索只有新实现；记录真实响应字段与秘密负向结果。未满足前不得进入 R2.2-d。

### R2.2-d Rust 类型与严格成功解析

新增/修改：

- `ManagedNode`
- `ManagedObservation.snapshot_id/title/nodes/truncated/text_only`
- production `ManagedLocator.element_ref`
- `ManagedWorkerAction.snapshot_id`
- `ManagedTabAction.snapshot_id`

`LoopbackPlaywrightWorker` 的 2xx 响应必须要求顶层 `ok:true`。页面/观察解析改为 `Result`，不得继续：

- 缺 pageId 用空串；
- 缺 pageGeneration 用 1；
- 缺 snapshotId 用请求值；
- 缺 nodes 用空数组并悄悄成功。

对 pageId/snapshotId/ref、node count、role/name 长度、generation、title/url/ARIA、总 JSON 大小做严格边界检查；generation 不得超过 JavaScript `Number.MAX_SAFE_INTEGER`。缺必需字段 → `invalid_worker_response/unknown`。

`LoopbackPlaywrightWorker` 还必须：在读取 JSON 前限制响应 bytes；验证 JSON Content-Type；只接受显式 loopback base；禁用系统代理和 redirect；2xx 只接受 `ok:true`。所有 cap 与 action kind 复用/扩展 Rust 权威常量，并用 Node/Rust golden 锁步，不允许两套漂移值。

### R2.2-e Host 当前 observation 与零派发

`TabRecord` 保存当前 managed observation identity 和允许的 ref 集合。Host 在调用 worker 前依次检查：

1. run/tab grant；
2. tab 未关闭；
3. worker page identity；
4. current pageGeneration；
5. current snapshotId；
6. 对节点动作，elementRef 属于 snapshot 且未 truncated；
7. 当前状态未要求重新 observe。

任一失败：

- Host 使用现有 `BrokerError::IdentityMismatch` / `Schema` / `TargetUnauthorized` 等正确变体；上层将其映射为模型 `rejected`。不得伪装成 `BrokerError::BrowserWorker(WorkerError::not_started)`；只有实际到达 worker 的 HTTP 拒绝才携带 `completion`。
- RecordingBrowserWorker 调用次数必须为 0；
- 不更新 generation、不消耗页面副作用。

新 observe 原子替换旧 identity。navigation、write、unknown、close、disconnect、profile close 和 worker rebuild 清空 Host observation。

worker 返回后、Host 写回 `TabRecord` 或返回模型前，必须 CAS 复核 run/profile/page/generation/snapshot/connection 仍与派发时一致；若期间改变，结果按不确定语义处理，绝不能覆盖较新的 identity。

### R2.2-f 私有 DTO 与现有模型面负向锁定

本批不新增 `browser_list_tabs/open/observe/act`；它们属于 R5。这里只锁 worker HTTP DTO、Rust 内部 request 和现有 MCP catalog：

- worker/Rust production DTO 拒绝未经允许的定位字段；
- 现有 Rust/Node MCP/TypeScript golden 证明 catalog 没有提前暴露新的 Browser 工具或私有路由字段；
- `computer_navigate` / `computer_download` 保持现有模型面，不得新增 profile/pageId/selector；
- 真正的 Browser model schema 在 R5 统一增加，禁止本批提前造第四份协议。

负向测试证明模型当前不能传或观察到：

- `selector`
- `role`/`name` 作为 locator 替代（role/name 只允许出现在 observation 输出）
- `pageId`
- `profile`
- `evaluate` / `expression` / `script` / CDP
- Cookie / Token / storage

worker 的私有 probe route helper 在迁移期间可以短暂存在，但不能出现在 MCP catalog/schema。R3.3 结束前，production entrypoint 中下列入口必须删除，或真正移动到不会随产品 worker 启动的独立 test harness：

- `/marker`、`/state`、`/crash` 和任何无 actionId 的 `evaluate` helper；
- selector-based `/popup`、`/upload`、`/download` 默认 `a#dl`；
- Rust `ManagedLocator.selector/role/name` locator、`read_state`、selector upload/popup wrapper；
- 自动在 Host wrapper 内生成 actionId 的兼容入口。

`/pids` 仅可保留为 Host-authenticated、固定 shape、无页面数据的生命周期诊断；模型面不可见。fixture 后置条件应由自建 fixture 的独立 oracle/test port 读取，不得为测试在 production worker 留任意 evaluate/selector 后门。最终报告附上述 legacy route/wrapper 搜索结果，允许项须逐条解释，其余为零。

R2.2 退出门禁：

```powershell
cd H:\aicoding\grok-app-computer-use
node --check tools\computer-use-browser\server.mjs
node --test tools\computer-use-browser\*.test.mjs

cd H:\aicoding\grok-app-computer-use\src-tauri
cargo fmt --all -- --check
cargo test -p grok-computer-use-core --lib --features test-support --offline --target-dir target-cu-review -- --test-threads=1
cargo clippy -p grok-computer-use-core --all-targets --features test-support --offline --target-dir target-cu-review -- -D warnings
cargo check -p grok-app --offline --target-dir target-cu-review
```

只有 pure、worker、Rust parser 和 Host zero-dispatch 四层均绿，才能进入 R2.3。

## 8. R2.3：actionId 状态机

### R2.3-a 显式 schema 与 fingerprint

先把每种 action 解析为 allowlist typed request，拒绝未知/冲突字段。`actionId` 必须由最外层可重试请求边界生成并在 Host→Rust client→worker 全链原样传递；删除/隔离 `managed_action_id()` 等在内部每次重生 UUID 的 wrapper。ledger key 固定为 `(owner/run, actionId)`：同一 run 跨 profile/page 复用 ID 仍是冲突，不同 run 的同名 ID 不互相泄漏。new-tab、popup、navigate/reload、act、upload、download 全部进入同一规则。

fingerprint 只覆盖规范化语义：

```text
owner/run + tab/page identity + pageGeneration + snapshotId
+ action kind + elementRef/坐标 + normalized parameters
```

- fingerprint 使用进程随机密钥的 HMAC-SHA-256（或等强度 keyed hash）覆盖 canonical serialization；密钥、canonical 明文和 fingerprint 均不写日志。
- fingerprint/ledger 不保存明文表单值、URL query、token 或 selector。
- `actionId` 本身不是语义参数；同 ID 查同 fingerprint。
- 默认值先规范化再 hash，防止 `undefined`、空串、别名产生不一致语义。

### R2.3-b 状态转换

| 现状 | 同 ID/同 fingerprint | 同 ID/不同 fingerprint |
| --- | --- | --- |
| 无记录 | 建立 pending，至多派发一次 | 不适用 |
| pending | `action_in_flight/not_started`，不二次派发 | conflict/not_started |
| done | 回放原结果并标 replayed | conflict/not_started |
| rejected | 回放原 typed rejected | conflict/not_started |
| unknown | 永远回放 unknown；不得重放 | conflict/not_started |

unknown 后：

- 清空 observation；
- 同 actionId 只能读回 unknown；
- 新 actionId 在 fresh observe 前也不得派发。
- 如果底层 Playwright promise 仍可能晚到，仅 fresh observe 不能解除隔离；page/run 必须 quarantine 到动作确定 settle，或主动关闭 page/context 后重建 identity。

每个 ledger entry 保存 immutable 的完整原始 terminal result 或完整 typed error envelope，包括 HTTP status、code、completion、message 和安全 currentPageGeneration；不能只保存 code/message 后重新构造丢字段。`replayed` 不是原始 terminal 状态：首次响应为 false/省略，查询回放时只在响应 projection 附加 `replayed:true`，不得修改 ledger。测试分别断言首次与回放标记。

本项只保证 worker 生命周期内语义。worker 重启后的 exactly-once 由 R4 Host ledger 解决，必须明确写作未完成。

### R2.3-c 并发与取消测试

不用 `sleep` 碰运气；用 barrier/deferred promise/fixture endpoint 控制：

- pending 同 ID 并发只有一个页面计数器变化；
- 同 ID 不同参数为零额外变化；
- 不同 actionId 的并发写也受 run-wide `in_flight` 互斥，第二个请求零派发；不能只防同 ID 重放。
- cancellation before dispatch → not_started；
- cancellation 与副作用竞态无法证明时 → unknown；
- late completion 不把 unknown 覆盖成 done；
- worker 返回后 Host 的 identity CAS 失败时不得把旧结果写回或解锁较新页面；
- worker ledger 使用明确硬上限，默认与现有 core `ACTIONS_PER_RUN=10_000` 对齐，并允许测试注入更小 cap。达到上限时拒绝 unseen actionId（not_started），但已知 actionId 仍可查询/回放；绝不淘汰旧 ID 后允许重放。R4 再把它接入统一 Host budget。

R2.3 退出条件：focused test + Browser Node 全套 + core Host tests + Clippy 全绿；报告明确“跨 worker 重启幂等留给 R4”。

## 9. R3.1：修复真实 Browser fixture 协议

修改 `run-fixture.mjs`：

- 每次运行生成随机 32-byte 以上 Bearer token。
- token 只进子进程 env 和 Authorization header，不打印、不写 fixture 文件。
- production `BrowserSupervisor` 也改为 OS CSPRNG 至少 32-byte token；UUID 不是本项最终 token 规格。
- 使用随机 loopback port；拒绝非 loopback client。
- `/open` 后保存真实 pageId/pageGeneration。
- 每个定向请求使用真实 worker identity。
- 每个写请求使用唯一 actionId。
- navigation/reload 后使用响应的新 generation。
- 每个 act 先 observe 取得 snapshot/ref，写后重新 observe 验证。
- 临时内容只写 `.run/` 或系统 temp；不得用用户 profile。
- finally 总是关闭 context/worker，并检查 Chromium 子孙退出。
- Node/Chromium 子进程使用环境变量 allowlist：production Host 先清空继承环境，再只恢复启动所需的最小 OS 变量和明确 `GROK_CU_*`；fixture 不得 `{...process.env}` 全量转发。用 sentinel `OPENAI_API_KEY/GROK_API_KEY=SHOULD_NOT_REACH_CHILD` 证明 worker/Chromium 看不到。
- 所有失败路径使用 RAII/finally 聚合“主错误 + cleanup 错误”，等待 worker 与捕获到的 Chromium descendants 退出后再删除临时目录，不能因 assertion 先抛而漏清理。

先恢复旧 fixture，不新增行为绕过认证。认证失败的修复不能是删除 token 检查。

## 10. R3.2：PNG、ARIA 与 opaque refs 完整 observation

### Red

至少新增：

- PNG signature 为 `89 50 4E 47 0D 0A 1A 0A`。
- width/height 与 PNG header 和 viewport 一致。
- fixture 非空像素/非全透明。
- ARIA 和可交互节点含预期文本。
- 超节点、超文本、超 PNG 设置 truncated/omitted reason。
- text JSON 不含 base64。
- stderr/trace 不含 base64、URL query 或表单内容。
- screenshot 缺失时 coordinate action 零派发。

### 实现

- 使用 `page.screenshot({type:"png", scale:"css"})` 或等价受限调用。
- 截图前限制 viewport/pixel count；编码后再检查 base64 cap。
- 超限优先安全缩放；无法在无新增重依赖下可靠缩放时，明确返回 text-only + `imageOmittedReason=size_limit`，不能截断 PNG bytes 伪造图片。
- worker 私有 observation 返回 pageId、pageGeneration、snapshotId、title、实际 URL、nodes、ARIA、truncated、image metadata 和有界图片载荷；Rust 必须逐项校验。
- 模型 projection 使用安全 display URL，去 userinfo/fragment/query values；MCP text content 去掉 pageId/profile/PNG bytes。R5 才正式增加 Browser model tools，但 worker/Rust observation 结构从现在就分离 private transport、model text 与 image。
- ARIA/nodes/screenshot/geometry 只在同一 generation CAS 成功后发布；image identity 与坐标 metadata 同 snapshot 绑定，任一 capture mismatch 都不得返回部分成功。

## 11. R3.3：typed actions 分组实现

不得一次写完再统一测试。按三组执行：

### R3.3-a 基础控件

- click
- fill/set_value
- type_text
- select
- key

canonical wire semantics 固定为：`set_value` 替换原值，`type_text` 在当前输入状态追加/模拟键入，`select` 只操作允许的 select/option。禁止 `selectOption` 失败后猜测性降级为 `fill`。kind、字段、text/key/timeout 等 cap 必须由 Node/Rust golden 锁步。

### R3.3-b 空间动作

- scroll
- drag

drag 的 source/target 都必须是 opaque ref，或在 screenshot 可见且 identity 当前时使用受限坐标；不得接收 `toSelector`。

### R3.3-c 等待与导航

- wait
- navigate/reload

所有动作严格按第 4.1 节矩阵校验，不得统一强塞 snapshot、actionId 或 elementRef：

1. 所有副作用动作先校验 owner/profile-private/page/generation/actionId 和 AbortSignal；需要 observation、元素或坐标时，再按矩阵校验 snapshot/ref/image identity。
2. 只有元素动作从 worker private map 取得 Locator/ElementHandle；navigate/reload/page-level key/scroll 不得造 dummy ref。
3. wait 只解析当前 opaque ref 并检查 allowlisted typed condition，是有界只读操作；不使用副作用 actionId，不写页面。
4. 副作用执行后读取真实 DOM/page 状态作为后置条件，再检查 cancellation 和 generation。
5. 返回 typed outcome 与当前 identity；只有矩阵规定的事件使 observation 失效并要求 verify observe。

禁止：任意 selector、evaluate、CDP、shell、file/data/javascript URL、任意本地上传路径。上传/下载只用 Host 授权的 run staging。

## 12. R3.4：真实 fixture 矩阵

每一项独立命名，不能只放在一个大 happy-path：

1. 双 tab 定向隔离。
2. popup 独立 identity。
3. same-origin iframe；cross-origin limitation 明确。
4. navigation 与同 URL reload 使旧 identity 失效。
5. close 后不 fallback。
6. actionId same/same 只执行一次并回放。
7. actionId same/different 拒绝。
8. different/same 确实执行两次。
9. pending 不二次执行。
10. unknown 不重放，fresh observe 前阻止新写。
11. stale snapshot/ref 零副作用。
12. slow action cancel：无后置条件或明确 unknown。
13. run A staging 不能被 run B 上传。
14. 下载重定向、保留名、取消/失败/超限时不留下零长或 partial 临时文件。当前 core staging 把成功的 0-byte 文件判为无效，先保持该策略；如要允许合法空文件，必须另写产品决定与测试，不能在本批顺手改变。
15. owner/profile traversal 真实 HTTP 拒绝。
16. 未授权/错误 token/非 Host clear 拒绝。
17. screenshot/ARIA/node caps。
18. shutdown 和 crash 后 worker/Chromium 无残留。
19. `BrowserSupervisor → LoopbackPlaywrightWorker → production server → 自建页面` 成功链：open → observe → opaque ref action → observe verify → PNG/identity → shutdown；这条独立命名为 `browser-managed-contract` probe。
20. sentinel 环境秘密不进入 worker/Chromium；production/test-only route 与 selector wrapper 搜索符合 R2.2-f。

真实后置条件至少包括：页面计数器、输入值、select 值、scroll offset、drag drop marker、URL/generation、下载字节、PID 退出。HTTP 200 不是任务成功。

下载实现必须边读边计数，不得先 `arrayBuffer()` 全量读入再判 cap；每次 redirect 都重新验证最终 URL；写入 run staging 下唯一 `.part`，完成大小/内容校验与必要 flush 后 atomic rename。取消、失败、超限、重定向拒绝均删除 `.part`。固定 Playwright download 与受控 fetch fallback 的鉴权/cookie 语义，禁止静默退化为无 cookie 的 Node fetch。

R3 的 required fixture/probe 不允许 `skip` 或 `not_run`。当前 Windows 开发机缺受支持 browser/runtime 时应 fail 并报告环境阻塞，不能算 Green；跨 OS E4/E5 仍保持 not_run。

## 13. 全批最终门禁

```powershell
cd H:\aicoding\grok-app-computer-use
node --check tools\computer-use-browser\server.mjs
$browserTests = Get-ChildItem -LiteralPath tools\computer-use-browser -Filter *.test.mjs | ForEach-Object FullName
node --test $browserTests
node tools\computer-use-browser\run-fixture.mjs
node --test tools\computer-use-mcp\protocol.golden.test.mjs
pnpm exec vitest run src\lib\computer-use src\components\computer-use src\i18n\messages.test.ts src\lib\settingsCatalog.test.ts src\lib\slashCatalog.test.ts
pnpm exec eslint src\components\computer-use src\lib\computer-use src\lib\api\computerUse.ts
pnpm typecheck

cd H:\aicoding\grok-app-computer-use\src-tauri
cargo fmt --all -- --check
cargo clippy -p grok-computer-use-core --all-targets --features test-support --offline --target-dir target-cu-review -- -D warnings
cargo check -p grok-computer-use-core --features test-support --offline --target-dir target-cu-review
cargo test -p grok-computer-use-core --lib --features test-support --offline --target-dir target-cu-review -- --test-threads=1
cargo test -p grok-computer-use-core --test driver --features test-support --offline --target-dir target-cu-review -- --test-threads=1
cargo check -p grok-app --offline --target-dir target-cu-review
$cuNodePath = (Get-Command node -ErrorAction Stop).Source
$env:GROK_CU_NODE_FILE = $cuNodePath
try {
  cargo run -p grok-computer-use-probe --bin cu_probe --offline --target-dir target-cu-review -- browser-error-contract
  if($LASTEXITCODE -ne 0){ throw "browser-error-contract failed" }
  cargo run -p grok-computer-use-probe --bin cu_probe --offline --target-dir target-cu-review -- browser-managed-contract
  if($LASTEXITCODE -ne 0){ throw "browser-managed-contract failed" }
} finally {
  Remove-Item Env:GROK_CU_NODE_FILE -ErrorAction SilentlyContinue
}

cd H:\aicoding\grok-app-computer-use
git diff --check
git status --short --branch
```

`run-fixture.mjs` 最终必须连续运行 3 次。每次单独记录：

- exit code；
- wall time；
- browser executable/version；
- Node/runtime version；
- 页面/文件真实后置条件；
- worker PID 与所有捕获到的 Chromium descendant PID 均退出。

一次 flake 后重跑通过不能删除首次失败；先定位，修复后重新从第 1 次开始计连续 3 次。

最终另跑静态搜索，逐条证明 production worker 没有 R2.2-f 所列 test/selector 后门，且 `browser.rs + browser_supervisor.rs + server.mjs` 合计行数不超过当前 5643 行基线。测试 harness 内的同名辅助能力必须位于独立、不会随产品启动的入口。

## 14. R3 后交付与停止

R3 全绿后停止修改，交付：

1. R2.1H、R2.2、R2.3、R3 每个原子项的实现/验证状态和 E0–E3 等级。
2. Red 首次失败和 Green 最终命令、exit code、测试数、耗时、后置条件。
3. 所有 flake 和处理，不只保留最后一次绿。
4. identity、授权、取消、重放、staging、日志泄漏安全复核。
5. `git status --short --branch`、`git diff --check`。
6. 修改文件清单与职责。
7. 明确仍未完成：R4–R9、Windows 安装 App E4、Existing Tabs E4、WebView E4、macOS、Linux、四 target packaging/E5。
8. 给 R4–R7 的精确接力点，但不要提前实现。

只允许写“R2.1H regression 与 R2.2–R3 在具体证据等级通过”，不得写“Computer Use 完成”。

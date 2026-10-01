# Computer Use：Grok 当前成果复核（R2.2-c checkpoint）

> [!NOTE]
> 这是 R2.2-c 历史 checkpoint，不代表当前进度。最新状态与下一步见
> [Grok 剩余工作总路线](2026-09-10-computer-use-grok-remaining-roadmap.md)。

日期：2026-09-10  
工作区：`H:\aicoding\grok-app-computer-use`  
分支：`feat/computer-use-implementation`  
HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`  
性质：对共享未提交工作树的代码、测试与产品边界复核；不是发布验收

## 1. 结论

Grok 已经完成了一套规模较大的 Computer Use 基础设施，不是演示壳，也不应推倒重来。协议、Broker、安全授权、会话 MCP、私有 worker、前端入口、Windows 适配、Managed Browser、Existing Tabs、WebView 和 macOS/Linux 原型均有实质代码；R1.3 typed request 与 R2.1 typed Browser error envelope 的主体质量合格。

当前准确进度不是“Computer Use 已完成”，而是：

- R0.1、R1.1、R1.2、R1.3、R2.1：已实现并有 E1/E2 门禁。
- R2.1H：Codex 审查补齐严格错误 envelope、未知异常脱敏和真实 Rust→Node HTTP 400 往返，已通过。
- R2.2-a：opaque snapshot/ref 私有状态，7/7，已通过 E1。
- R2.2-b：PageState、navigation/reload/popup/close/iframe 失效，8/8，已通过 E1；错误链回归 E2 通过。
- R2.2-c：节点抽取核心模块 5/5，Browser Node 全套 30/30；但 production `/observe` 尚未接入，而且本次审查发现安全与行为缺口，因此状态只能是 `in_progress`。
- R2.2-d–R9、正式 runtime pack、Windows/macOS/Linux E4/E5：未完成。

综合评价：**架构和基础实现可保留，局部单元质量较好；浏览器闭环、发行运行时和三平台实机证据不足，当前不可发行，也不适合直接形成一个巨大 PR。**

## 2. 本次现场验证

本次未启动正式 App，未使用用户浏览器 profile、账号、Cookie、Token、代理或共享 `~/.grok`。

| 检查 | 结果 | 证据边界 |
| --- | --- | --- |
| Browser Node 全套 | 30 passed / 0 failed | E1；其中 observation state 7、PageState 8、extract 5 |
| `observation-extract` focused | 5 passed / 0 failed | E1；尚未证明 production HTTP `/observe` |
| core lib | 182 passed / 0 failed | E1 |
| driver integration | 12 passed / 0 failed | E2 私有子进程协议 |
| MCP golden | 4 passed / 0 failed | E1 |
| Computer Use/UI/i18n/settings/slash Vitest | 129 passed / 0 failed | E1/jsdom |
| Rust fmt | passed | 格式门禁 |
| core Clippy `-D warnings` | passed | core 静态门禁 |
| `cargo check -p grok-app` | passed；3 个既有 dead-code warning | 编译门禁，不是 App E2E |
| targeted ESLint / `pnpm typecheck` | passed / passed | 前端静态门禁 |
| `git diff --check` | 无 whitespace error | LF→CRLF 为本机 `core.autocrlf=true` 提示 |

`browser-error-contract` 在 R2.2-b 后已回归通过；R2.2-c 新改动只在 Node 小模块中，但最终 R3 门禁仍须全部重跑，不能沿用当前绿灯代替最终验收。

## 3. Grok 已完成的主体与质量

| 能力 | 实际完成度 | 评价 |
| --- | --- | --- |
| 协议、Broker、run/target/snapshot、授权 ticket、lease、预算、pause/stop/takeover | 有完整 core 与大量竞态/负向测试 | 方向正确，可保留；Browser 尚未全部进入同一生命周期 |
| 会话 MCP、Host-only/模型工具隔离、默认关闭 | 已接线并有 golden/前端测试 | 基础合格；Browser 模型工具集合仍不完整 |
| 私有 worker、IPC、取消、超时、进程树回收 | driver 12/12 | E2 基础较强；desktop 真动作仍在 App 进程 |
| 设置、slash、Computer 面板、预览、任务卡、15 locale | 有产品代码和 E1 测试 | UI 基础可保留；尚无安装 App + 真实模型 E4 |
| Windows adapter/UIA/fixture | 代码量较完整，历史有自建 fixture | 不能外推成当前分支安装版 E4 |
| Managed Browser | profile/page/error/action 原型较完整 | 当前主线；observation/ref/PNG/Host 闭环未完成 |
| Existing Tabs | 配对、借用、归还状态机原型 | 真实 Chrome/Edge 扩展 transport 与 E4 未完成 |
| App WebView | typed subset 原型 | 产品 surface E4 未完成 |
| macOS/Linux | 原生适配原型 | 能力不全且无对应实机 E4，不得宣称三平台完成 |
| runtime 安装/repair/rollback 框架 | 逻辑和测试存在 | 随包 runtime 仍是 placeholder，不能实际发行 |

R1.3 的 typed structs 与 R2.1 的 typed error envelope 都不是临时实现。R2.1H 的严格 envelope、异常脱敏和真实错误往返属于 Codex 审查回补，不应混写为 Grok 原始成果。未提交工作树无法仅凭 Git 对每一行做可靠作者归属，因此本报告以“当前代码是否成立”为主要判断。

## 4. R2.2 已完成部分

### 4.1 R2.2-a：observation state

- CSPRNG `snapshotId` / `elementRef`。
- 私有 `WeakMap` 保存 ref→target，public DTO 冻结且不序列化 ElementHandle、selector、profile/pageId。
- observation→generation→snapshot→ref 固定校验顺序。
- 新 observation 和显式 invalidation 销毁旧 ref。
- 节点数、role/name、truncated 有界；被截断节点不分配 ref。

评价：**pure contract 完成质量合格，E1。**

### 4.2 R2.2-b：PageState

- 页面首次发布 generation 1；发布后任意主 frame navigation，包括同 URL reload，均换代。
- iframe attach/navigate/detach 清 observation，但不冒充主 frame generation。
- popup 独立 page identity；close 为 tombstone，绝不 fallback。
- 新 observation 替换旧 map；写操作在派发前清 observation；wait 保持只读。
- generation 达上限后 fail-closed，要求重建 identity。

评价：**EventEmitter/pure lifecycle 合格，E1；真实 Chromium 事件顺序仍留 R3。**

### 4.3 R2.2-c：extract 核心模块

`observation-extract.mjs` 已实现 64 node cap、role/name/title/URL/ARIA/JSON cap、DOM 顺序、opaque ref、私有 handle、URL query/userinfo/fragment 清理、frame omission summary，以及采集前后 pageGeneration/frameRevision CAS。focused 5/5，Browser 全套 30/30。

评价：**核心模块有价值，但只完成一半。** 生产 `/observe` 仍在 `server.mjs` 中手写旧 ARIA/roles 返回，Rust 和 Host 看不到新 snapshot/nodes；因此不能标记 R2.2-c complete。

## 5. 本次审查发现的 R2.2-c 阻断项

### P1：非 HTTP URL 会泄露正文或本地路径

`safeDisplayUrl()` 当前只清 userinfo/query/fragment。现场调用 `safeDisplayUrl("data:text/plain,SECRET_PAYLOAD")` 仍返回完整 `data:` 内容；`file:` 同理会暴露本地路径。修复前不得接入 production `/observe`。

要求：只允许安全展示 `http:`、`https:` 和精确 `about:blank`；`data:`、`file:`、`javascript:`、无效 scheme 返回不含原值的 typed omission/empty display，加入假 secret 和 Windows 路径负向测试。

### P1：生产 `/observe` 仍是旧协议

`server.mjs` 尚未导入 `captureManagedObservation`，仍只返回原始 `page.url()`、ARIA 和 button/textbox/combobox 文本数组。新模块 5/5 不能证明真实 worker HTTP 路径。

要求：先修 URL 泄漏，再最窄接入 `/observe`；添加 live loopback HTTP contract，断言 fresh snapshot、opaque refs、第二次 observe 使旧 ref 失效、409 typed envelope 和 public JSON 无秘密。

### P1：交互节点集合过宽，可能在 64 cap 前饿死真实控件

选择器包含裸 `[role]`，会把 heading/document/presentation 等非交互 ARIA role 全部纳入；同时当前没有清晰的 hidden/inert/disabled 可操作策略。页面前 64 个非交互 role 可导致真正按钮完全不返回。

要求：固定 allowlisted interactive roles，加 focusable/contenteditable/native controls；不可见、inert、`aria-hidden` 节点策略必须写入契约和测试。disabled 可展示但不得 act。

### P1：节点替换后的 action-time 复核尚未完成

当前 capture 结束时只复核 `isConnected`；私有 signature 被保存但尚未在解析/动作前重新验证。旧 ElementHandle、同属性替换、几何变化和 detached/reattached 的边界仍可能漂移。

要求：R2.2-c/d/e 先把 target metadata 固定；R3.3 在副作用前复核 handle connected、signature、page/frame identity。任何不匹配以 `not_started`/Host zero-dispatch 拒绝，不能猜 selector 或 fallback。

### P2：跨层常量和兼容字段仍会漂移

Node 在 `observation-state.mjs` 和 `observation-extract.mjs` 各自定义 cap，Rust 还有自己的上限；新响应临时保留 legacy `roles`。继续扩展会产生两套 schema。

要求：R2.2-d 由 Rust 权威常量 + Node/Rust golden 锁步；删除 production `roles` 兼容字段，或在迁移期明确 private-only 并设删除门禁。总 JSON cap 要在 HTTP client 读 body 前再次执行。

## 6. 仍未完成的关键产品能力

- Rust 2xx `ok:true`、page/snapshot/node 的严格 parser；当前仍有 fallback 风险。
- Host current observation/ref allowlist、stale ref 零 worker dispatch、返回后 identity CAS。
- run-wide actionId ledger、keyed fingerprint、不同 actionId 单写互斥、unknown late-completion quarantine。
- Bearer/env allowlist、production test/selector route 清理、流式安全下载。
- PNG observation、image identity/geometry 和 typed actions 的真实页面后置条件。
- `BrowserSupervisor → LoopbackPlaywrightWorker → production server → real fixture page` 成功链及连续三次 Green。
- Browser 模型工具、完整 Broker 生命周期、App shell 拆分。
- 可发行的四 target runtime pack；当前 seed `js-runtime`/worker 是 placeholder。
- desktop worker 真正执行 OS observe/act/abort。
- Windows 安装 App + 真实 Grok 模型、Existing Tabs、WebView、macOS arm64/x64、Linux X11/Wayland E4/E5。

平台原型的已知边界仍包括：macOS semantic click 存在固定坐标占位且 key/scroll/drag 不全；Linux type/key/scroll/drag 不全并有 capability 误报风险；Windows 当前动作结果仍普遍 `verifiable:false`。

## 7. 工程与交付风险

- 当前有 56 个 tracked modified、197 个 untracked 文件，全部未提交；未跟踪内容约 4 万行。
- 大文件仍包括 `browser.rs=2975`、`browser_supervisor.rs=1678`、`server.mjs=990`；`AppWorkbench.tsx` 仍有 +6 行增长债。
- production worker 仍暴露 `/marker`、`/state`、`/crash` 和 selector popup/upload/download 兼容入口。
- 当前状态不适合直接提交成一个巨大 PR；应在功能闭环与审查后按依赖切批。

## 8. 当前唯一执行入口

继续使用并已更新：

- [Grok R2.2–R3 详细执行书](2026-09-10-computer-use-grok-r2-r3-execution.md)
- [Grok R2.2–R3 长任务提示词](2026-09-10-computer-use-grok-r2-r3-prompt.md)
- [执行状态账本](2026-09-09-computer-use-execution-state.md)

Grok 必须从 **R2.2-c 安全加固与 production `/observe` 接线**开始。R2.2-a/b 已通过，禁止返工；R2.2-c 未完整 Green 时不得进入 R2.2-d。R3 全绿后停止改代码并交 Codex 审查，不能把该审查点写成完整 Computer Use Goal 完成。

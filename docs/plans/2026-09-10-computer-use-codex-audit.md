# Computer Use：Codex 接手审计

> [!NOTE]
> 本文主体保留 03:58–05:02 的接手过程。R1.3/R2.1 后的当前复核结论见 [Grok 当前成果复核（R2 checkpoint）](2026-09-10-computer-use-grok-progress-review-r2.md)。

日期：2026-09-10  
工作区：`H:\aicoding\grok-app-computer-use`  
分支：`feat/computer-use-implementation`  
HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`  
审计性质：只评估当前未提交工作树，不代表发布验收  

## 1. 结论

Grok 已经完成了相当多的基础设施，尤其是协议、Broker、安全边界、Windows fixture、浏览器原型、WebView 原型和工作台 UI。代码不是空壳，也不是需要推倒重来。

但是当前不能进入“完整 App E2E 已具备，只差打包”的阶段。最新改动把 Managed Browser 的真实页面身份、actionId 和取消链路推进了一步，同时留下了编译门禁、跨语言协议、旧 fixture 和 Broker 生命周期之间的断层。当前最合理的路线是先完成一轮 **Browser/Broker 回补**，恢复可信基线，再继续 runtime pack、独立 desktop worker 和三平台实机验收。

按发布定义判断：

- 基础架构：有实质成果，方向基本正确。
- Windows：已有较强的 adapter/fixture 基础，但没有本分支安装版 + 真实 Grok 模型的 E4/E5 证据。
- Managed Browser：核心功能存在，但尚未形成可安全交给模型的完整产品链。
- Existing Tabs：配对和状态机有实现/探针，真实用户安装扩展的 Chrome + Edge E4 未完成。
- WebView：typed subset 有 fixture，完整产品 App E4 未完成。
- macOS/Linux：只是部分源码，不是首版三平台完成。
- 打包：`js-runtime` 和 Playwright seed 仍是占位内容，不能满足“用户无需 Node”的发行承诺。

因此：**保留现有成果，禁止重做 S0；暂停 S10.6，先执行审计回补计划。**

## 2. Grok 已完成的工作

### 2.1 Broker、协议与安全边界

已存在并有较完整测试覆盖：

- 默认关闭的 feature flag。
- session/run/target/snapshot/geometry 身份绑定。
- 用户授权 ticket、重新授权、fork/reconnect 后失效。
- 桌面 exclusive lease、暂停、接管、停止和取消。
- `applied / verified / rejected / unknown` 结果分类。
- actionId 防重、unknown 后重新 observe、动作与观察预算。
- loopback IPC、Bearer、token 轮换、请求大小与来源限制。
- 用户剪贴板恢复、staging 路径和下载文件名限制。
- Remote IM、scheduler、SSH 不继承本机 Computer Use。

评价：这是当前质量最好的一层。E1/E2 基础可信，但浏览器写操作还没有完全复用这一层，见第 4 节。

### 2.2 私有 worker 与进程生命周期

已完成：

- versioned worker handshake、run/generation/deadline/cancellation。
- 响应大小限制、乱序/崩溃后的 generation 处理。
- Windows Job Object 子孙回收。
- stderr drain、脱敏、上限和幂等 shutdown。
- Host-owned adapter 包装与 worker admission。

当前实测：`driver` 集成测试 12/12 通过，属于 E2。

缺口：worker 当前只做 `prepare_dispatch` admission，真正的 desktop `observe/act` 仍由 App 进程内 OS adapter 执行。因此“桌面动作已搬到独立 worker”尚未完成。

### 2.3 Windows desktop

已存在：

- Win32 窗口枚举和 target stamp。
- UIA subtree、RuntimeId、Invoke/Value/Scroll 优先路径。
- PrintWindow/BitBlt 捕获、DPI/多屏几何处理。
- 定向消息与 SendInput fallback。
- click、set/type、key、scroll、drag、中文、剪贴板恢复等 fixture。
- 宿主窗口、最小化窗口、焦点漂移、身份复用等拒绝路径。

评价：代码量和 fixture 都较完整，但历史 E3 记录没有替代 E4。当前审计没有驱动正式安装或用户真实窗口。

### 2.4 Managed Browser

已存在：

- Host supervisor 启停 Playwright worker。
- 隔离 persistent profile、单 owner、清除与回收。
- 页面列表、新 tab、popup、iframe、导航、上传、下载和 typed act。
- 真实 `pageId`，主 frame 导航后递增 `pageGeneration`。
- worker 内 actionId 状态：pending/done/rejected/unknown。
- 相同 actionId + 相同参数回放；不同参数拒绝。
- `/cancel-run`、AbortSignal 和 context 关闭。
- Host tab registry 开始同步 worker 的真实 page identity。

评价：这是最新进展，也是当前最需要收口的部分。Node worker 语法检查通过，profile 路径单测 2/2 通过；旧真实 fixture 已失效，完整 core 回归也被这次契约变化打断。

### 2.5 Existing Tabs

已存在：

- 扩展 challenge/confirm/token 的配对协议。
- 用户共享后再由 App picker 授权。
- run/session/tab/document/connection generation 绑定。
- 借用、归还、用户主动导航、原 index/url/focus 保存。
- stop/disconnect/update/browser exit/App exit/tab close 状态机。
- CFT unpacked extension 探针记录。

评价：身份与状态机可复用，但真实扩展 transport 尚未承载完整 observe/act/navigate；不能把内存 Host + CFT 探针写成 Chrome/Edge 产品完成。

### 2.6 App WebView

已存在：

- 与 App-owned WebView 的 typed bind/observe/act fixture。
- navigation generation、旧 elementRef 失效。
- 禁止任意 eval、Cookie/storage 导出和跨 surface 认证迁移。
- unsupported 能力的 fail-closed 路径。

评价：原型方向正确，历史探针达到 E3 类 fixture；真实 Grok App 产品 surface E4 未完成。

### 2.7 MCP、设置和 UI

已存在：

- session-scoped MCP 注入、撤销与 reconnect 处理。
- Computer Use 设置项、settingsCatalog、默认关闭。
- Composer slash、资源面板、target picker、授权、preview、暂停/接管/恢复/停止。
- 任务卡展示 target/backend/status/failure。
- 15 locale catalog 与 UI 单测。

当前实测：UI/i18n/settings/slash/task card 117/117 通过；ESLint、TypeScript typecheck 通过。

缺口：slash 分发给 `AppWorkbench.tsx` 新增了 6 行，违反该文件增长冻结；协议 TypeScript 常量把 `computer_stop` 放错到了 Host-only 集合。

## 3. 2026-09-10 03:58 初次审计快照（历史）

本节保留首次审计时的 Red 证据；R1.1/R1.2/R1.3 的最新状态以第 6 节为准。

| 检查 | 当前结果 | 结论 |
| --- | --- | --- |
| `node --check tools/computer-use-browser/server.mjs` | exit 0 | Node worker 语法有效 |
| `node --test tools/computer-use-browser/profile.test.mjs` | 2 passed | profile/run 名称路径校验 E1 |
| core `cargo check --features test-support` | exit 0 | core 可编译 |
| `cargo check -p grok-app` | exit 0，3 个既有 warning | App Rust 可编译，不代表 E2E |
| core lib test | **174 passed / 3 failed** | 当前回归门禁失败 |
| driver integration | 12 passed | 私有子进程协议/回收 E2 |
| MCP golden | 4 passed | Rust/Node catalog 当前一致 |
| TS Computer Use tests | **14 passed / 1 failed** | `computer_stop` catalog 不一致 |
| UI/i18n/settings tests | 117 passed | jsdom E1 |
| targeted ESLint | exit 0 | 静态门禁通过 |
| `pnpm typecheck` | exit 0 | 类型检查通过 |
| core clippy `-D warnings` | **7 errors** | 6 个参数过多 + 1 个 needless bool assign |
| `run-fixture.mjs` | **exit 1**，`GROK_CU_BROWSER_TOKEN required` | runner 未适配新认证/身份协议 |
| `git diff --check` | exit 0，伴随 CRLF conversion warning | 无 whitespace error；换行策略需后续统一 |

本次 `cargo fmt --all` 已格式化当前 Rust WIP，随后 `cargo fmt --all -- --check` 通过；这不是行为修复。

### 3.1 三个 core 失败

1. `reserved_download_name_rejected`
   - 测试把 `share_and_grant` 创建的用户 tab 传给 managed-only download。
   - 新实现正确拒绝：`tab is not a managed browser page`。
2. `navigate_records_actual_url`
   - 同样用用户 tab 调 managed-only navigation。
3. `browser_borrow_return_disconnect`
   - Existing Tabs 状态机测试仍假定 RecordingBrowserWorker 可以替用户 tab 导航。

修复原则：改测试夹具和明确 transport 边界，不得放宽 `managed_tab_route` 让用户 tab 落入 managed worker。

### 3.2 TypeScript golden 失败

Rust 和 MCP 把 `computer_stop` 作为模型工具；产品 wiki 也规定模型可以请求 stop。TypeScript 却把它列在 Host-only。应统一为模型工具，并保持 authorize/resume/reconnect/pause 为 Host-only。

### 3.3 Clippy 失败

新增页面身份后，多个函数参数达到 8–11 个。不能简单加 `allow(clippy::too_many_arguments)`；应引入强类型请求结构，减少身份字段漏传和顺序传错风险。

## 4. 必须先修的设计断层

### P0：Browser 写操作绕过 Broker 生命周期

`browser_navigate` / `browser_download` 当前直接调用 `ExistingTabHost`，没有复用 desktop action 的完整：

- paused / stop_requested 派发前检查；
- run-level `in_flight` 互斥；
- action budget；
- Host 侧 actionId ledger；
- timeout/worker 断线后的 unknown；
- generation 变化后的 late result 丢弃；
- unknown 后强制重新 observe。

worker 内存缓存不能单独承担幂等，因为 worker 重启后缓存会消失。

### P0：模型没有完整 Browser 工具面

当前只有 `computer_navigate` / `computer_download`，没有模型可用的 tab list/open/observe/typed act 闭环。Managed Browser 的大部分能力只能被 Host/探针调用，不能声称 Agent loop 已覆盖浏览器。

### P0：Managed observe 没有截图

worker `/observe` 只返回 ARIA/roles，没有受限 `page.screenshot()`。模型工具也没有把 browser PNG 作为 image content 交给模型，因此视觉页面能力未完成。

### P0：发行 runtime 仍是占位

`src-tauri/resources/computer-use/seed/bin/js-runtime` 只有 26 bytes，README 明确称 placeholder；`playwright/worker.mjs` 也只是 53-byte contract seed，不含固定 Node、playwright-core 和浏览器依赖。当前不能满足安装包脱离系统 Node 的目标。

### P1：HTTP 错误分类信息丢失

Rust `post_headers` 丢掉 worker HTTP status，只返回 error 文本。Broker 无法可靠区分 4xx 安全拒绝与 5xx/断线/timeout 的 unknown。

### P1：Existing Tabs transport 不完整

用户 tab 的 observe/act 目前主要是 registry 状态验证。导航甚至明确报 `existing tab navigation requires the extension transport`。应保留 fail-closed，直到扩展 transport 真正实现。

### P1：独立 desktop worker 尚未执行动作

HostOwnedAdapter 仅让 worker 执行 `prepare_dispatch`，随后仍在 App 进程调用 OS adapter。进程隔离、崩溃回收与取消尚未覆盖实际输入副作用。

### P1：平台代码不完整

- macOS 缺真实 AX 语义树，key/scroll/drag 未实现，abort 只改 idle。
- Linux 只有部分 X11 click；type/set/key/scroll/drag 未实现；native Wayland 明确为 false。
- 这些诚实的 `not implemented`/`native_wayland=false` 是正确边界，不能通过改文案抹掉。

### P2：工程质量债

- `browser.rs` 2714 行、`browser_supervisor.rs` 1606 行、`windows_fixture.rs` 1906 行、`webview.rs` 1138 行，后续功能前需要按领域拆分。
- `AppWorkbench.tsx` 本次净增 6 行，违反增长冻结。
- Windows 全局 `core.autocrlf=true` 且仓库无 `.gitattributes`，出现 LF→CRLF warning；不得批量改换行制造噪音，应单独确定仓库策略。
- 执行状态总表、队列和后续日志互相矛盾，必须以本审计后的回补队列为准。

## 5. 当前完成边界

可以确认：

- 代码基础可继续演进，不需要重写。
- 私有 worker 协议与 Windows 进程树测试当前通过。
- UI、i18n、settings、TypeScript 编译当前基本稳定。
- Managed Browser 的 pageId/pageGeneration/actionId 方向正确。

不能确认：

- 当前 core 回归全绿。
- Browser Agent loop 安全闭环。
- 安装版无需系统 Node。
- Windows 安装版 + 真实模型 E4。
- Chrome/Edge 用户安装扩展 E4。
- macOS arm64/x64 或 Linux X11/Wayland 实机能力。
- 四 target 安装、更新、回退、卸载与 E5 矩阵。

下一步执行文件：

- [Computer Use 审计回补开发计划](2026-09-10-computer-use-grok-repair-plan.md)
- [交给 Grok 的长任务提示词](2026-09-10-computer-use-grok-repair-prompt.md)

## 6. 2026-09-10 04:30 接力复核

原审计后又完成了两个修复项，并开始了第三项：

- R1.1 已通过：三条旧 Browser 测试改用正确 managed fixture 或明确 Existing Tabs fail-closed；core 曾恢复到 177/177。
- R1.2 已通过：TypeScript 的 `computer_stop` 与 Rust/MCP 对齐；现场重跑 MCP golden 4/4、TypeScript Computer Use 15/15。
- R1.3 进行中：强类型参数结构方向合理，但修改未收尾，当前工作树不是可编译 checkpoint。

当前现场证据：

| 检查 | 结果 | 判断 |
| --- | --- | --- |
| core `cargo check` | exit 1，`broker/gates.rs` 仍用旧 `share_and_grant` 签名 | R1.3 未完成 |
| `cargo fmt --all -- --check` | exit 1，3 处格式差异 | 格式门禁未恢复 |
| MCP golden | 4/4 passed | R1.2 保持绿 |
| TypeScript Computer Use | 15/15 passed | R1.2 保持绿 |
| UI/i18n/settings | 117/117 passed | 前端 E1 未回退 |
| Node worker syntax/profile | exit 0 / 2 passed | 基础 Node 门禁未回退 |
| Browser fixture | exit 1，`GROK_CU_BROWSER_TOKEN required` | R3.1 仍未开始 |

对 Grok 工作质量的最终判断：

- **可保留且质量较好**：研究与 ADR、Broker 身份/授权/停止安全骨架、worker admission/回收、设置/UI/i18n、Windows fixture 基础。
- **方向正确但需要回补**：Managed Browser page identity、actionId、cancel、Existing Tabs 状态机、WebView typed subset。
- **仍属原型或占位**：Browser 模型闭环、Browser Broker 生命周期、独立 desktop worker 真执行、发行 runtime pack。
- **尚未交付**：Windows 安装版 + 真实模型 E4，Chrome/Edge Existing Tabs E4，macOS/Linux 真实能力，四 target packaging/E5。

因此不能按“接近打包”安排下一步；应从 R1.3 恢复绿基线，再按 [R1.3–R3 执行书](2026-09-10-computer-use-grok-next-batch.md) 完成下一批。

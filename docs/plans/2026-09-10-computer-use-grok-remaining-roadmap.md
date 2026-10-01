# Computer Use：Grok 剩余工作总路线

日期：2026-09-10  
工作区：`H:\\aicoding\\grok-app-computer-use`  
分支：`feat/computer-use-implementation`  
基线 HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`  

## 1. 本文件的地位

这是当前 Computer Use 后续施工的总路线。它不替换
[S0–S12 总计划](2026-09-09-computer-use-grok-step-plan.md)，而是根据 2026-09-10
最新代码审查，把剩余工作重新排成可审查的小批次。

当前只允许启动 **B1：安装包 Browser runtime 真闭环**。B1 完成并停笔审查前，
不得并行进入 B2–B7。

当前 B1 的详细执行书：
[2026-09-10-computer-use-grok-runtime-pack-execution.md](2026-09-10-computer-use-grok-runtime-pack-execution.md)。

交给 Grok 的当前长任务提示词：
[2026-09-10-computer-use-grok-runtime-pack-prompt.md](2026-09-10-computer-use-grok-runtime-pack-prompt.md)。

## 2. 最新可信状态

### 2.1 已经成立的部分

- 协议、Host Broker、目标/快照身份、授权 ticket、exclusive lease、暂停/停止、
  actionId 防重、unknown quarantine 和 loopback IPC 已形成较完整实现。
- Windows 自建原生 fixture 已有 E3 历史证据。
- 开发目录中的 Managed Browser 已通过真实页面 open → observe → typed act →
  observe verify → PNG → shutdown 链。
- 模型工具已有 `browser_list_tabs`、`browser_open`、`browser_observe`、
  `browser_act`，Host-only 工具不在模型目录。
- Computer 面板、设置、slash 入口、任务卡和 15 locale 已接线。
- 2026-09-10 最新现场回归：
  - Rust core：236/236；
  - Browser Node：76/76；
  - MCP golden：5/5；
  - 前端定向：120/120；
  - typecheck、ESLint、App `cargo check`：通过；
  - 开发态 Host → BrowserSupervisor → Node worker → 真实页面：PASS。

这些证据只证明 Windows 本机 E0–E3，不是安装版 E4，也不是三平台完成。

### 2.2 当前 P1 阻断：发行 Browser runtime 仍不可启动

当前资源包虽然包含：

- 官方 Node 20.18.0 Windows x64 PE；
- 官方 `playwright-core@1.48.0` tgz；
- 对应 SHA-256 manifest；

但产品链仍有以下硬断点：

1. `src-tauri/resources/computer-use/seed/playwright/worker.mjs` 仍只有一行
   `export const grokComputerUsePlaywrightWorker = true;`，不会监听端口或提供协议。
2. `playwright-core` 只是 tgz；`install_from_bundle` 只复制文件，没有把它物化成
   Node ESM 能解析的 `node_modules/playwright-core`。
3. `product_spawn_request()` 取得 tgz 的父目录后直接拼 `worker.mjs`，因此会启动
   上述占位文件。
4. runtime `required_runtime_components()` 不包含 `browser-worker`；健康检查可能在
   产品 worker 不可运行时仍返回健康。
5. 现有 `browser-managed-contract` 明确要求 `GROK_CU_NODE_FILE`，并使用源码目录
   `tools/computer-use-browser/server.mjs`。它证明开发态链路，不证明发行包链路。
6. 开发 worker 的 `package.json` 当前声明 `playwright-core@^1.63.0`，发行输入却钉在
   `1.48.0`；当前 76 项 Node Green 没有证明生产源码与发行 pin 相容。

因此，S2.4 目前只能标为“Node/tgz 来源与完整性已验证”，不能标为
“发行 Browser runtime 完成”。S7 和 S10.6 也不能据此进入 E4。

### 2.3 其他未完成项

- Windows：完整功能有 E3，但没有隔离安装 App + 真实模型 E4。
- Existing Tabs：有 Host 协议与 Chrome for Testing E3；正式 Chrome/Edge 扩展安装、
  用户配对、人工登录后继续和安装版 E4 未完成。
- WebView：typed subset/fixture 有实现；真实 App WebView 的完整产品路径 E4 未完成。
- macOS：元素语义点击仍可能退到固定 `(24,24)`；Key/Scroll/Drag 未实现；
  两个 target 的打包、TCC 和 E4 均未完成。
- Linux：X11 只有截图和 click 主路径；Type/SetValue/Key/Scroll/Drag 未完成；
  GNOME Wayland portal/helper 未实现，E4 未完成。
- S11：审计、诊断包、性能与 soak 没有完整验收。
- S12：四 target、安装/升级/修复/回退、每 OS 60 次最终矩阵均未完成。

## 3. 后续批次与依赖

```text
B0 可信状态冻结（已完成审查）
  → B1 Windows 发行 Browser runtime 真闭环
  → B2 Windows 隔离安装版 Host/UI 闭环
  → B3 Windows 真实模型 E4 + Existing Tabs/WebView E4
  → B4 macOS arm64/x64 实现、pack 与 E4
  → B5 Linux X11/GNOME Wayland 实现、pack 与 E4
  → B6 S11 隐私、诊断、性能与可靠性
  → B7 S12 四 target 安装生命周期与 E5
```

B4 与 B5 的代码准备可以在 B3 审查后分别进行，但对应平台没有实机证据时必须保持
`not_run`。B7 前不能把交叉编译、mock 或 Windows 结果当成 macOS/Linux E4。

## 4. B1：Windows 发行 Browser runtime 真闭环

目标：安装资源经 RuntimeStore 修复后，仅依赖 App 自带 Node、固定 Playwright 和
生产 worker，即可在空 PATH、隔离 `GROK_APP_HOME` 下完成真实浏览器闭环。

必须完成：

- real worker entrypoint 和全部 production-only sibling modules 进入 pack；
- 安全、原子地物化 `playwright-core@1.48.0`；
- 开发/测试与发行只保留一个精确 Playwright pin；
- runtime manifest/diagnose 必须验证 worker、模块、Node 和 Playwright 可运行布局；
- 新增不接受 `GROK_CU_NODE_FILE` 的 `browser-packaged-contract`；
- corrupt/missing/placeholder/partial extraction/rollback/cleanup 全部有测试；
- 连续三次真实页面闭环，无 worker/Chrome 残留。

B1 完成后必须停笔。B1 只证明 Windows packaged runtime E2/E3，不证明安装 App +
真实模型 E4。

## 5. B2：Windows 隔离安装版 Host/UI 闭环

前置：B1 审查通过。

目标：构建本分支隔离安装包，不覆盖官方 Grok App，不使用共享 `~/.grok`，验证：

- 安装、首次启动、runtime diagnose/repair；
- 设置默认关闭；开启后 Computer 面板、slash、授权、预览、暂停、接管、恢复、停止；
- managed browser/profile 创建、退出回收、App crash 后恢复；
- Windows 原生目标 click/type/key/scroll/drag 的真实后置条件；
- session 切换、后台会话、关闭 App 和重连不泄漏授权。

允许 Host-only 自建 fixture；没有真实模型时只能达到 E3，不得标 S10.6 E4。

## 6. B3：Windows 产品 Browser 面与真实模型 E4

前置：B2 审查通过，且需要用户提供人工交互窗口；不得读取或导出 Token/Cookie。

分三个独立子批次：

1. Managed Browser 安装版 + 真实 Grok 模型 E4。
2. Existing Tabs 正式 Chrome/Edge 扩展安装、双确认配对、借用/归还/断线 E4。
3. App WebView typed subset E4；跨源 iframe、权限 UI、复杂下载继续明确 unsupported。

每个子批次分别停笔审查。不能用 Chrome for Testing 冒充用户正式浏览器。

## 7. B4：macOS arm64/x64

前置：B1 的 runtime layout 已稳定；必须在对应 macOS target 编译并在至少一台真实
macOS 机器运行，另一架构至少完成原生安装验证。

顺序：

1. AX 元素身份和语义动作；删除固定 `(24,24)` 元素点击。
2. CGWindow 捕获、Retina scale、跨屏/坐标映射。
3. Key、Scroll、Drag 和取消/后置验证。
4. Screen Recording/Accessibility TCC、签名 identity、权限撤销。
5. arm64/x64 Node/runtime pack、安装、修复和回退。
6. 安装 App + 真实模型 E4。

没有真实 TCC/动作后置条件时不得标 passed。

## 8. B5：Linux X11 与 GNOME Wayland

前置：B1 runtime layout 稳定；必须在真实 Linux 环境运行。

顺序：

1. X11 的目标身份、截图、Click、Type/SetValue、Key、Scroll、Drag、取消与验证。
2. GNOME Wayland 的 ScreenCast + RemoteDesktop portal 会话、PipeWire 帧、
   libei/portal 输入或经过验证的等价路径。
3. portal/session identity、用户授权、显示器变化、撤销与进程回收。
4. AppImage/deb/rpm 的 Node/runtime pack、安装、修复和回退。
5. X11 与 GNOME Wayland 分别完成 E4。

XWayland、浏览器或截图成功都不能标成 native Wayland。

## 9. B6：S11 隐私、诊断、性能与可靠性

- trace audience 分离和容量上限；默认不持久化 screenshot、输入文本、剪贴板、
  URL query、Cookie 或页面正文；
- 诊断包二次脱敏及 secret sentinel 测试；
- staging/managed profile/trace 的清理入口与准确影响说明；
- capture、AX/DOM、模型、dispatch、verify 的 p50/p95；
- preview 可见性、背压、latest-frame-only；
- worker/browser crash、断网、权限撤销、显示器变化和浏览器更新 soak。

## 10. B7：S12 发布验收

- Windows x64、macOS arm64、macOS x64、Linux x64 的固定来源、hash、license、NOTICE、
  SBOM 与包内容审计；
- 每 target 干净安装、上一版本升级、同版本 repair、runtime corruption repair、
  rollback、卸载；
- 每 OS 至少 12 类任务 × 5 次；成功率、错误目标、未授权动作、Stop 后派发、
  凭据泄漏、残留进程均满足总计划门槛；
- 全仓测试、lint、格式、构建、i18n、默认关闭和 AppWorkbench 增长门禁；
- 最终仍不 commit/push/PR，先形成报告给用户审核。

## 11. 全程不可违反的边界

- 共享工作树只能一个代码 writer；不得让多个子代理并发修改代码或账本。
- 保留全部现有未提交工作；禁止 reset、clean、checkout --、stash、覆盖或重建项目。
- 不碰 `H:\\aicoding\\grok-app` 及其他 worktree。
- 不碰用户账号、Token、Cookie、代理、共享 `~/.grok`、正式安装和真实浏览器 profile。
- 测试只用随机 loopback/Bearer、隔离 profile/staging/`GROK_APP_HOME`、自建 fixture。
- 产品不得依赖 PATH Node，不得执行 `npm install` 或在线获取 `latest`。
- 大型第三方二进制不得未经维护者决策直接进入 Git；优先提交小型 lock/source manifest，
  由受 hash 约束的构建步骤生成被忽略的 bundle seed，正式构建缺产物时 fail closed。
- 不在生产 worker 留 selector/evaluate/CDP/test route 后门。
- 不向 `App.tsx`/`AppWorkbench.tsx` 增加 Computer Use 状态或大型逻辑。
- 不加 `allow`、skip、ignore，不降低 lint 或把宽松断言写成成功。
- 未经用户明确授权，不 commit、push、PR、merge、tag、release。

## 12. 状态用语

- `implemented`：实现存在，不代表验证。
- E1：unit/mock/jsdom。
- E2：真实本机进程/IPC。
- E3：自建原生或浏览器 fixture 的真实后置条件。
- E4：已安装 App + 真实 Grok 模型。
- E5：跨 target、重复安装与任务矩阵。

任何一项只有更低等级证据时，都必须保持更高等级 `not_run`。只有 B1–B7 的必需项
全部达到总计划门槛，才能报告 Computer Use 完成。

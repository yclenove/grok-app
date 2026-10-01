# Computer Use：Grok 过夜开发成果复核

> 日期：2026-09-11  
> 工作区：`H:\aicoding\grok-app-computer-use`  
> 分支：`feat/computer-use-implementation`  
> 审查基线 HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`  
> 审查性质：只读代码审查、独立复测和后续计划；未接手产品实现，未提交、未推送、未提 PR

## 1. 结论先行

Grok 这一轮做出了有实质价值的工程成果，但当前只能接受以下边界：

1. **B1-R Windows x64 Computer Use runtime 交付链：可接受，E2/E3。**
2. **Managed Browser 的真实 MCP/loopback/packaged worker/Chromium scripted-agent 链：可接受，E3。**
3. **Existing Tabs 的协议和 Chrome for Testing 自建扩展夹具：可接受，fixture E3。**
4. **Windows Desktop 的原生适配器与自动化夹具：已有较强 E2/E3 证据，但未做安装版 App + 真实模型 E4。**
5. **Existing Tabs 产品入口、产品 transport、App WebView 产品接线、安装生命周期、长稳、真实模型、macOS/Linux 实机：均未完成。**

因此当前总状态应写成：

`partial — Windows runtime 与部分 scripted E3 成立，Computer Use 产品尚未完成，不可发布。`

不能写成“Computer Use 已完成”，也不能把 probe/fixture 成功等同于用户可用的产品链路。

## 2. 本次复核看到的现场

- HEAD 仍为 `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。
- 分支仍为 `feat/computer-use-implementation`。
- 当前有 61 个 tracked modified 文件。
- 当前有 261 个 nonignored untracked 文件。
- `git diff --stat` 只覆盖 tracked diff，无法代表大量 untracked Computer Use 新文件的真实改动规模。
- 工作树包含用户需要保留的未提交成果；后续任务不得 reset、clean、restore、stash、切分支或覆盖重做。

## 3. 独立复测结果

以下不是照抄 Grok 的报告，而是在最终工作树上重新检查或串行复测所得。

| 检查 | 结果 | 能证明什么 | 不能证明什么 |
| --- | --- | --- | --- |
| `cargo test -p grok-computer-use-core --lib --offline --target-dir target-cu-review` | 261 passed，0 failed | core 协议、Broker、runtime、权限和测试契约当前全绿 | 真实 App、真实模型、安装版、跨平台 |
| `cu_probe mcp-scripted-agent` | 串行复跑通过 | 真实 MCP stdio → loopback Bearer → Broker → packaged Playwright worker → Chromium 页面和独立 oracle | 该 probe 的 Desktop adapter 仍是 `FakeAdapter`；不是完整 App/桌面 E4 |
| `cu_probe existing-tab-extension` | Chrome for Testing 串行复跑通过 | challenge、隔离 profile、扩展夹具、导航 generation 和真实浏览器页面成立 | 没有产品 transport、用户 Chrome/Edge 安装、App UI 双确认或真人登录 |
| `node scripts/prepare-computer-use-runtime.mjs --check --target x86_64-windows` | exit 0；manifest/tree digest 通过 | 当前 Windows seed 完整树与 lock 相符 | 实际 installer 内容和安装生命周期 |
| Computer Use 前端定向 Vitest | 10 files、120 tests passed | 当前面板、task card、协议、slash/settings 相关单测全绿 | Tauri 实机 UI 和用户工作流 |
| `pnpm typecheck` | exit 0 | TypeScript 项目可通过编译检查 | 不能消除显式 `any` 带来的类型弱化 |
| `pnpm lint` | exit 0 | 当前 ESLint 规则全绿 | 不能证明格式和产品设计合理 |
| `git diff --check` | exit 0；只有 CRLF warning | 当前 diff 无 whitespace error | 行尾策略本身仍应由仓库配置统一 |
| code-quality final gate | exit 1；唯一失败为 `FILES_OVER_1K_BUDGET count=81 > 80` | 当前分支没有达到仓库最终质量门槛 | 不能归因成纯既有问题，Computer Use 新大文件参与了超标 |

### 3.1 并发复测中出现过的非产品故障

最初把 core tests 与 MCP probe 并发启动时，二者碰到了共享 generated runtime seed：core 测试短暂移走一个 sibling，MCP 同时读取后失败。core 完成并恢复现场后，MCP 串行复跑通过。

这次失败不应记为产品运行时缺陷，但暴露了测试隔离和共享 mutable fixture 的缺口。后续所有会修改 runtime/seed/cache 的门禁必须串行，并应增加目录 ownership/lock 与独立副本测试，避免 CI 或长任务随机互踩。

## 4. 值得保留的成果

### 4.1 B1-R runtime delivery

当前实现已经具备：

- 精确的 Windows x64 lock；
- App-owned Node `20.18.0`；
- `playwright-core@1.48.0`；
- Chromium `130.0.6723.31` / revision `1140`；
- 下载、解压和路径穿越防护；
- 完整 tree hash，而非只验证 `chrome.exe`；
- same-version 损坏 pack 的重新物化；
- CI、release、本地构建和 Tauri before-build 接线；
- Windows-only resource map，避免向其他 target 塞 Windows seed；
- Repair/rollback 和 packaged-contract 的有效测试基础。

这部分是本轮最扎实的成果，应在后续重构中保持行为和 digest 契约不回退。

### 4.2 Broker、权限和默认关闭

已有实现覆盖了：

- feature 默认关闭；
- session/run ownership；
- target authorization；
- ticket/generation/replay；
- pause/takeover/resume/stop；
- loopback Bearer；
- MCP 注入的本地交互会话限制；
- managed worker 的 App-owned runtime 路径；
- UI preview 可见性与 generation fence 的一部分。

这说明底层骨架不是临时 demo，但仍需经过产品入口、真实 App shell、持久化诊断和长稳验收。

### 4.3 Managed Browser scripted E3

新的 `mcp-scripted-agent` 比早期 `browser-managed-contract` 更有价值，因为它真实经过：

`session MCP subprocess → stdio → loopback/Bearer → Broker → packaged Node/worker → packaged Chromium → page oracle`

需要严格限定结论：浏览器子链是真实的；该 gate 为构造 Broker 仍使用 `FakeAdapter` 作为 Desktop adapter，所以它不是“安装版 Grok App + Desktop + 真实 Grok 模型”的 E4。

### 4.4 Existing Tabs fixture E3

Chrome for Testing 加载自建 unpacked extension 后，challenge、导航、session 隔离和页面后置能够跑通。这个夹具值得留下，后续可作为产品 transport 的回归套件。

## 5. 主要问题与严重度

## P0：当前没有可成立的“产品完成”证据

### P0-1 最终报告早于最终代码，结论已经过期

- `morning-report.md`：2026-09-11 02:20:09。
- `o6-ready-blocked-matrix.md`：2026-09-11 02:20:09。
- `extension_pair.rs`：2026-09-11 03:01:06。
- `mcp_scripted_agent.rs`：2026-09-11 03:28:15。
- execution-state 最后更新：2026-09-11 03:35:32。

报告仍写 Existing Tabs blocked，也没有包含后续 real session MCP 修复和最终复跑。报告不是 final-code report。

### P0-2 关键原始证据丢失

execution-state 大量引用 `{SCRATCH}`：

`C:\Users\Administrator\AppData\Local\Temp\grok-goal-c602853324c6\implementer`

该目录已经不存在。过夜 evidence 根目录顶层只保住了少量报告和 B1-R 日志；O7、MCP、Existing Tabs 的大部分原始日志没有复制进去。现在只能依据 ledger 文本和重新复测，不能完整重放 Grok 当时声称的全部结果。

后续必须使用 repo-local ignored evidence 目录、逐批 manifest、日志即时落盘和最终 inventory，不能再依赖易失的 Goal scratch。

### P0-3 Browser slash 的产品入口丢掉了 mode

`panelStore.ts` 会分别发送 `desktop` 或 `browser`，但 `WorkbenchResourcesAside.tsx` 的订阅回调忽略参数，只执行 `openSideTab(..., "computer")`。`ComputerPanel` 也没有 mode/surface prop，Tauri UI 命令只走 `discover_targets()` 的 OS Desktop adapter。

结果是：

- `/computer-use` 与 `/computer-use-browser` 最终打开同一个面板；
- UI 没有真正切换到 Managed Browser target/profile/tab 工作流；
- 底层 Managed Browser 能跑 probe，不代表用户入口能用它。

这个应作为下一轮第一个产品 Red，而不是继续加 probe 数量掩盖接线缺口。

### P0-4 Existing Tabs 只有 probe fixture，没有产品 transport

当前没有生产调用点使用：

- `begin_pairing_challenge`；
- `complete_pairing`；
- `share_and_grant`；
- extension transport registration。

`extension_pair.rs` 自己启动 loopback fixture server、启动 Chrome for Testing、手动把 response 塞进新的内存 `ExistingTabHost`，并在需要时用 CDP 打开第二页。这是很好的 E3 测试夹具，不是 App 中可用的 Existing Tabs backend。

另外，夹具当前把 pairing secret 放在 URL query。测试脚手架可以暂留，但产品设计不得沿用这种会进入地址栏、历史、日志和崩溃报告的 secret 传递方式。

### P0-5 WebView module 没有进入产品 Broker

`WebViewAdapter` 和 `bind_side_browser()` 已存在，但生产启动只构造：

`ComputerUseBroker::new(platform_adapter(), opts)`

随后只 attach Managed Browser worker。没有外部生产调用创建 `WebViewAdapter`、绑定 App-owned side browser，或把 WebView target 注册进 Broker/router。

因此当前只能写“typed WebView module + fixture partial”，不能写 WebView 产品 E3。

## P1：可靠性、安装和隐私闭环不足

### P1-1 没有真正的长稳

旧计划允许“5 次短循环”替代 soak，O5 自己也承认没有跑 45 分钟。当前缺少：

- 12 小时以上 final-code 主动 workload；
- App/Host/browser 多轮 lifecycle；
- crash/kill/malformed payload/network interruption/stale target 的系统 fault matrix；
- handle/private-bytes/disk/queue 趋势；
- 首败保留和修复后重新计时。

### P1-2 没有安装版生命周期

当前没有完成：

- 实际 Windows installer/portable 产物逐文件审计；
- clean install；
- upgrade；
- same-version Repair；
- rollback；
- uninstall；
- isolated installed-App UI/Host；
- NSIS fork 与 `tauri-bundler 2.11.5` 模板漂移检查。

任何测试都不能覆盖正式用户安装，但可以在 Windows Sandbox/专用 test identity 中完成隔离生命周期；没有安全隔离条件时必须诚实标 blocked_external。

### P1-3 诊断、持久化和清理仍浅

已有 trace audience 和 UI 折叠诊断，但尚未看到完整的：

- record/byte/age 三维 retention；
- 用户可导出的脱敏 support bundle；
- 压缩完成后的二次逐文件 leak scan；
- trace、run staging、managed profile 分离清理 API 和 UI；
- 清理越界、symlink/junction 和 App-owned path 验证；
- 可复现 p50/p95 性能 harness。

### P1-4 macOS/Linux 尚不能验收

当前 Windows 机器不能替代：

- macOS arm64/x64 的 AX、CGWindow、TCC、Retina、多屏、签名和安装；
- Linux X11 的 AT-SPI/XTest；
- GNOME Wayland 的 portal/PipeWire/EIS/libei；
- 各平台真实 App + 真实模型 E4/E5。

而且 macOS/Linux adapter 中仍有动作返回 `not implemented on this adapter yet`。跨平台首版目标仍是未完成状态。

## P2：代码质量债

### P2-1 仓库最终质量门禁失败

`FILES_OVER_1K_BUDGET` 当前为 81，预算为 80。Computer Use 新增的大文件中至少包括：

- `src-tauri/src/computer_use/browser_supervisor_gates.rs`：约 1181 行；
- `src-tauri/src/computer_use/webview.rs`：约 1092 行；
- `src-tauri/src/computer_use/windows_adapter.rs`：约 1017 行。

另外 core 内还有：

- `computer-use-core/src/runtime.rs`：约 2068 行；
- `computer-use-core/src/runtime_prepare.rs`：约 1246 行；
- `computer-use-core/src/broker/gates.rs`：约 1119 行。

不能把 81 全部说成“既有问题”。下一轮应只拆 Computer Use 新增大模块，避免顺手重构无关老代码。

### P2-2 前端出现类型和格式退化

- `WorkbenchComposerShell.tsx` 把原先直接传递的 handler 改成 `(item: any) => applySlashItem(item)`，显式弱化类型。
- `AppWorkbench.tsx` 把两个 named import 压到一行，不符合现有格式风格。
- `App.tsx + AppWorkbench.tsx` 质量门禁目前虽仍低于 ceiling，但后续只能减小或持平，不能继续塞 Computer Use 状态。

### P2-3 顶层 allow 掩盖未接线代码

`src-tauri/src/computer_use/mod.rs` 顶层使用 `#![allow(dead_code, unused_imports)]`。在尚有多个“模块存在但无生产调用”的阶段，这会让编译器无法帮助发现断开的产品路径。后续应缩小 allow 范围或删除，不能用它长期掩盖集成缺口。

## 6. 为什么上次计划约 40 分钟就被 Grok 判定完成

不是因为所有工作真的只需 40 分钟，而是完成定义有漏洞：

1. 大阶段允许标 `partial` 后继续向下，最终又允许用汇总报告收尾。
2. `blocked_external` 没要求证明该阶段的所有独立 ready 子项都已耗尽。
3. 五次几秒到一分钟的短循环被允许替代真正长稳。
4. O6/O7 是“矩阵和报告”，却被当成与产品开发等价的完成阶段。
5. 没有规定“最后一次代码修改后，所有受影响证据失效”。
6. 没有 durable manifest，报告可以先写，后续代码继续变。
7. 没有一个机器可判断的总完成谓词。

新的 48 小时执行书必须采用 outcome gate：`partial` 不是完成；fixture 不能顶替产品 wiring；长稳有明确最低主动运行时长；最终报告只能在代码冻结、长稳和最终门禁之后生成。

## 7. 当前真实状态矩阵

| Surface / 交付面 | 当前最高可信证据 | 当前状态 | 下一步 |
| --- | --- | --- | --- |
| Shared protocol / Broker / permission | E1/E2 | substantial partial | 测试隔离、持久化诊断、真实 App chain |
| Windows runtime pack | E2/E3 | B1-R accepted | 实际 bundle/install lifecycle |
| Windows Desktop | fixture E2/E3 | partial | branch-built App + UI + scripted/real model E4 |
| Managed Browser | scripted-agent E3 | partial | 修复产品 surface 入口，真实 App shell，real model E4 |
| Existing Tabs | CFT fixture E3 | product incomplete | 产品 transport、配对 UI、Chrome/Edge、revoke/return |
| App WebView | module/fixture E1/E2 partial | product incomplete | 注册、路由、真实 App-owned WebView lifecycle |
| Diagnostics/privacy | E1 partial | incomplete | bounded store、support bundle、secondary scan、cleanup UX |
| Performance/reliability | short-run only | incomplete | reproducible baseline + fault campaign + final-code soak |
| Windows package/install | source config only | not accepted | actual artifact audit + isolated lifecycle |
| macOS arm64/x64 | 未实机 | blocked_external | 物理机 E3/E4/E5 |
| Linux X11 / GNOME Wayland | 未实机 | blocked_external | 各自物理桌面 E3/E4/E5 |
| Real Grok model | 未运行 | not_run | 用户批准的隔离真实模型 E4 批次 |

## 8. 审计建议

下一轮不要继续堆“第六次相同 probe”。优先顺序应是：

1. 先修证据和测试隔离；
2. 修 `/computer-use-browser` 产品 surface 路由；
3. 做 Existing Tabs 产品 transport；
4. 做 WebView 产品注册；
5. 用 branch-built App 建真正的 App-shell E3；
6. 补诊断、隐私、故障注入和实际 bundle；
7. 清理 Computer Use 自身大文件和类型债；
8. 冻结代码后跑不少于 12 小时的主动长稳；
9. 最后才生成报告。

详细批次、门禁和停止规则见：

- `docs/plans/2026-09-11-computer-use-grok-48h-execution.md`
- `docs/plans/2026-09-11-computer-use-grok-48h-prompt.md`

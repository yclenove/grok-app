# DSH 第三方插件调研：Computer Use、浏览器与任务体验

日期：2026-09-08。范围：从社区目录和 GitHub 搜索筛选 12 个相关项目，下载固定版本源码，核对入口、关键执行路径、许可证与维护信号。**没有安装插件、启动其驱动或进行三平台实测；不是完整安全审计。**

本文补充 [框架对照](2026-09-08-computer-use-harness-comparison.md)，结论已并入 [Computer Use 主方案](../plans/2026-09-08-computer-use.md)。研究分支为 `feat/computer-use-design`，不涉及壁纸分支，也未提交或推送。

## 1. 结论与选型影响

最值得吸收的不是插件数量，而是四组已经有源码的机制：

1. **Tencent BrowserSkill**：真实浏览器的 Agent Window、借用/归还用户标签页、结构化操作、按需预览和会话队列。加入 P0 浏览器连接候选，与既定 Playwright 扩展比较。
2. **Anionex/dsh-computer-use**：macOS 的目标句柄复验、权限租约、重新绑定后的确认失效、动作后重新观察。作为 Broker 及 macOS 适配参考。
3. **Yu-tao-Li/dsh-computer-use-win、ZRui-C/dsh-computer-use**：分别补充 Windows UIA/捕获/输入与 macOS 定向输入、取消协议的实现参考。
4. **Flowglass、Task Graph、Auto Continue**：分别提供执行轨迹、耗时分析与有界恢复思路；按 Grok 自己的事件和副作用规则重写。

主架构仍为 **Grok Build ACP → App Broker → 桌面/浏览器适配器**。桌面优先验证 Cua Driver，受管浏览器优先 Playwright；没有发现已被本次研究验证、可直接交付完整三平台能力的 DSH 插件。DSH 插件入口依赖 Cordis 和 `@deepseek-ai/dsh-*`，不能把 `dsh plugin add` 当成 Grok App 的安装方式。

## 2. 筛选与证据边界

发现入口：[awesome-dsh-plugin][DIR1]、[awesome-deepseek-harness][DIR2]，以及 GitHub 的 `dsh-plugin computer/browser`、`dsh playwright` 等仓库搜索。目录收录不等于审计，目录的 CC0 许可也不覆盖被收录插件。`dsh-external/hub` 本次访问为 404，未作为可核查来源。

每个入选项目都核对了仓库元数据、源码 SHA、包入口及至少一条相关源码路径；没有读取每个文件，也没有用作者的测试通过声明代替本次测试。源码在仓库外的研究缓存中，没有 vendoring 进 Grok App。

截至此次查询，12 个仓库均未归档。11 个非 Tencent 项目在 2026 年 8 月创建；Tencent BrowserSkill 在 6 月创建。多数仍处于很早的开发阶段。下表的日期为 GitHub `pushed_at` 的 UTC 日期，仅表示最近有推送，不能证明发布包、全部分支或所有平台稳定。

| 项目 | 包版本 / 最近推送 | 许可证核对 | 定位与建议 |
| --- | --- | --- | --- |
| [Tencent/BrowserSkill][BS] | DSH 包 0.2.0 / 09-07 | 根与 DSH 包 MIT | **优先验证**：已登录 Chrome/Edge 接入候选 |
| [Anionex/dsh-computer-use][AN] | 0.3.2 / 09-03 | MIT | **优先参考**：macOS 状态、权限与目标复验 |
| [Yu-tao-Li/dsh-computer-use-win][WIN] | 0.1.2 / 08-18 | MIT，含 cgissing 上游来源说明 | Windows 驱动备选及测试参考 |
| [ZRui-C/dsh-computer-use][ZR] | 0.3.0 / 08-14 | Apache-2.0；部分 Cua/yabai MIT 来源 | macOS 后台输入、取消与浏览器参考；包 `private: true` |
| [988hj7tczd-oss/dsh-computer-use][CU] | 0.2.0 / 09-03 | MIT | Cua 接入样例；需重做会话隔离与取消 |
| [ChenyuHeee/dsh-browser-playwright][PW] | 0.1.1 / 08-14 | MIT | 轻量 browser service 与快照 ref 参考 |
| [anweat/dsh-browser][AW] | 0.1.10 / 08-29 | MIT；外部依赖须另核 | 浏览器常驻、站点规则/recipe 参考；整体范围大于首版 |
| [Fisfzy/dsh-ego-browser][EGO] | 0.8.3 / 09-07 | 根 MIT；vendored ego MIT；可选 FFmpeg 构建 GPL | 实时预览/任务空间参考，暂不作为默认运行时 |
| [HsiangNianian/dsh-auto-continue][AC] | 0.11.4 / 09-04 | MIT | 恢复调度参考；其提示词不构成动作幂等保证 |
| [Iwctwbh/dsh-flowglass][FG] | 0.4.5 / 09-02 | MIT | 执行轨迹与增量刷新参考 |
| [KevinZhangNothing/dsh-task-graph][TG] | 0.1.0 / 08-28 | MIT | 只读轨迹图和瓶颈分析参考 |
| [geohotstan/dsh-computer-use][GH] | 0.1.2 / 08-16 | 仓库 MIT，部分 Cua 移植说明 | 独立 Swift 实现参考；不能据此再分发 Codex 私有组件 |

版本取所检出 `package.json`，不是验证过的 npm 最新版本。Fisfzy 的 GitHub license 自动识别为 `NOASSERTION`，实际根 LICENSE 是 MIT 文本加第三方说明；不能把 API 的识别结果当作无许可，也不能忽略 FFmpeg 的不同条款。

## 3. 桌面插件：值得借什么，不能假定什么

### 3.1 Anionex：最贴近 Broker 的状态约束

已读 `src/service.ts`、`leases.ts`、`providers/macos.ts`、`providers/native-helper.ts` 及相关原生文件入口。[AN1][AN2]

- macOS provider 显式检查 `process.platform === 'darwin'`；其他系统为 unsupported，**没有冒充三平台驱动**。
- 动作带观察和目标句柄，检查句柄归属、元素一致性；可重新观察并解析目标。敏感目标重新绑定后，旧的单次确认 token 失效。
- 观察、操作权限分开，取消信号传至 helper；helper 启动前校验平台、文件与打包 hash。
- 动作后等待并重新观察；只报告窗口/树的结构变化，代码明确承认像素、瞬态或远端效果可能不体现在结构 hash 中。

采用：目标身份、快照代际、动作结果证据与 UI 提示。补强：Grok 的 `verified` 必须对应任务后置条件；“树变了”不等于“文件已保存/表单已提交”。`sensitive` 等字段也不能仅由模型决定。其进程内租约不能证明我们需要的跨 App 实例输入互斥。

### 3.2 988hj7tczd-oss：三平台声明背后仍是 Cua

`lib/cua.js` 通过 `spawn(CUA_BIN, ['call', tool, JSON.stringify(args)])` 调用外部 Cua Driver；README 的三平台与虚拟光标能力依赖该驱动及机器环境。[CU1]

源码提供了观察模式、窗口局部坐标和元素 token 的接入参考，但也有需要主动避免的边界：

- `lib/snapshot.js` 是模块级单个 `snapshot`；`CUA_SESSION` 默认固定为 `dsh-computer-use`，不是按 Grok 的会话/run 自动隔离。[CU2]
- `rawCall` 没有接收 AbortSignal，也没有自身超时；不能从 UI 返回“已取消”推断其进程和动作已停止。
- `actions.js` 中无元素的输入使用 `scope: 'desktop'`，按键实现还存在取窗口失败后退到 desktop 的路径；即使入口 guard 限制可达性，也不适合原封不动纳入我们的定向工具契约。[CU3]
- `guard.js` 的密码框/危险标签判断主要依赖已编号元素；源码明确坐标模式无法预知目标。TTL 不是危险动作识别，更不是持续目标身份验证。

结论：**验证了“DSH 薄封装 + Cua”已有社区实现，不是另一套独立跨平台引擎**。不因此跳过 Cua 的版本、Wayland、DPI、取消和错误目标测试。

### 3.3 Windows：Yu-tao-Li 的工具后端可作备选

DSH bundle 通过 MCP stdio 接到 `mcp/server.mjs`，后者维持一个 PowerShell 后端进程；JSON 行请求、超时后 kill 并懒重建都在源码中。`windows-uia.ps1` 包含 UIA、DPI、身份缓存、窗口捕获与输入路径。[WIN1][WIN2]

有用的区分：UIA Invoke/Toggle 可以不切前台；坐标输入走 SetCursorPos/SendInput；PostMessage/WM_CHAR 后台路径明确标为 delivery unverified，部分应用可能丢弃。中文可走剪贴板或 Unicode 输入，但剪贴板恢复本身也有竞态，需单测与实机验收。

采用：Windows fixture、捕获 fallback 和输入分类参考。暂不将整套 PowerShell/C# 动态编译链作为默认发行依赖，也不把超时 kill 当成 OS 已撤回输入。源码中的窗口 homing 思路需经过当前几何复验，不能仅平移旧坐标继续点击。

来源文件说明该项目衍生自 `cgissing/windows-computer-use`，部分 wiki 原样复制。评估应以这个 fork 的源码为准；保留上游 MIT 版权，不能把复制的 wiki 当作 fork 实测报告。[WIN3]

### 3.4 ZRui-C 与 geohotstan：后台输入有真实实现，也有系统依赖

ZRui-C 同时包含 Playwright 与 Swift macOS helper。`AbortableMutex` 提供可取消等待队列；native client 取消时发带请求 id 的旁路 cancel。`TargetedEventPoster.swift` 针对 PID 走 SkyLight 或 `CGEvent.postToPid`，无 PID 分支才走全局 HID。[ZR1][ZR2]

其 NOTICE 标出 Cua 与 yabai 的来源和 MIT 许可。可以借鉴取消、坐标转换和后台/前台能力分类；私有 SkyLight 路径的存在不能证明后续 macOS 版本、所有窗口或所有动作都可靠，也不能迁移成 Windows/Linux 的承诺。

geohotstan 的 README 写“cloned from Codex”，进一步读取发现其交付的是独立 Swift daemon，部分输入代码注明来自 MIT 的 Cua；`docs/codex-parity.md` 则声明基于对专有 Codex 组件的观察，并称不直接引入其原文。[GH1][GH2]

这与“包含可授权再分发的官方 Codex 驱动”是两回事。本文只把独立实现和来源声明作为研究材料，不核准全部来源，也不接受作者的“全部 parity 已完成”作为我们的实测结论。优先采用来源清晰的公开机制，分发时按文件逐项核对。

## 4. 浏览器插件：最有价值的新增候选

### 4.1 Tencent BrowserSkill：重点进入 P0

确认有正式 DSH 子包 `packages/dsh-plugin-browserskill`，不是仓库仅打了 DSH topic。其 `dsh.bundle`、host/client 入口、工具、runner 和扩展均有源码。[BS1]

值得吸收的实现：

- 工具以 session/page/inspect/interact/tabs/assist 组织。DSH 工具面刻意不暴露任意 evaluate 和 record；skill 使用结构化工具，而不是提示模型绕去 CLI。
- Agent Window 和用户 tab 分开。借用前有可见确认，记录原窗口与索引；归还、借用中取消与回滚有实际实现。**会移动被借用的 tab**，因此 UI 要说明位置变化，不能宣传所有操作都毫无干扰。[BS2]
- 每个浏览器 session 一条 FIFO，工具与预览截图共用队列，避免观察与动作相撞。预览订阅为零时取消待采集任务；重新订阅先给 snapshot 再给增量，避免状态接口与事件流乱序。[BS3][BS4]
- runner 使用取消信号和子进程终止路径；完整停止仍需验证 daemon、扩展和页面端是否静止，不能只检查 CLI 退出。

源码也暴露了两个必须进入候选验收的差异：

1. `sessions.ts` 的操作归属检查是“本插件创建的 session”，`resolve()` 不接收调用者 run 身份；DSH owner 元数据还用于清理。Grok 要进一步绑定 **appSessionId/runId/browser/profile/tab**，不能把插件级归属当成多聊天之间的授权隔离。[BS5]
2. `daemon/ws.rs` 的 Origin gate 接受形状合法的 Chrome extension id，源码 TODO 明确尚需真实扩展 id allowlist/配对。仅合法 `chrome-extension://…` 不是我们所需的连接身份校验；本次发现是这一层的限制，不代表已经证明全部配对流程可被绕过。[BS6]

决定：P0 比较 **Playwright 扩展与 BrowserSkill** 的登录复用、选定 tab 控制、借用/归还、停止、来源身份、三 OS 打包和延迟。P7 只交付通过同一 Broker 契约的一条默认连接路径，避免两个引擎同时控制同一个 tab。BrowserSkill 尚未成为锁定依赖。

### 4.2 ChenyuHeee：轻量结构可读，取消必须补齐

`service.ts` 把 provider 注册/选择与会话 acquire/release 分开；`playwright.ts` 按 owner 创建 context，并有空闲回收；`injected.ts` 使用每 document 的 nonce/ref，`tool.ts` 的任意 evaluate 默认关闭。[PW1][PW2]

关键限制在 `withAbort`：信号触发后 reject 外层 Promise，源码注释明确底层 Playwright 操作仍在后台 drain。借鉴其接口和观察 ref 时，不能继承这种“调用返回即视为已停止”的假设。我们要等执行静止、关闭自有 context 或中断自有 worker，才能释放输入/页面租约。

另外 URL 初始域名检查不等价于导航全过程的边界；点击、重定向、iframe、弹窗等要由统一 Broker 与 browser adapter 复验。per-owner Map 也不能代替并发首次 acquire、回收活跃任务的竞态测试。

### 4.3 anweat：适合后续站点自动化，不整体塞进首版

实际是常驻浏览器 service，包含插件本地 Playwright/Patchright、OpenCLI、站点规则、recipe、userscript 与命名 auth profile。`browser-service.ts` 的交互路径使用持久 context/page；`auth-profiles.ts` 有 storageState 文件和允许域名配置。[AW1][AW2]

采用：按需启动、复用浏览器、规则/recipe 的后续扩展思路。暂缓：整套脚本/通用 OpenCLI 工具面、`unrestricted` 模式和认证状态导入导出。它们引入新的执行、凭据、兼容和依赖范围；我们的首版仍是固定 typed 动作和 browser-owned 登录。若后续做 recipe，每一步都必须重新经过 Broker，不能因打包成 recipe 就跳过检查。

### 4.4 Fisfzy ego-browser：纠正对“ego”的简单归类

该插件不只是调用一个 macOS 浏览器 App。检出版本包含 `runtime/ego-linux` 的 Chrome/CDP host、来自 ego-lite 的共享 harness，并有 Windows 路径处理；`src/index.ts` 通过 DSH subprocess 运行封装脚本，传取消信号与输出限额。[EGO1][EGO2]

这证明存在 **ego 的社区浏览器运行时移植**，不证明任意 Linux 桌面、Wayland 原生窗口或官方 ego 的所有能力已支持。主方案中有关官方 ego 产品范围的判断不能直接套到这个 fork 上。

适合参考的是 task space、实时 watch 面板、CDP screencast 和人工步骤提示。暂不作为默认引擎：它维护 vendored 运行时和本地补丁，增加同步成本；可选 FFmpeg 选用构建带 GPL 义务，CDP 预览路径则不要求 FFmpeg。Grok 的首版预览不因此引入录制或视频编码依赖。[EGO3]

## 5. 恢复、上下文与可观察体验

### Auto Continue：可以借恢复调度，不能借“假用户继续”

已核对 host engine 与 shared core。它使用 host 单实例监视事件，区分瞬时/永久错误、用户停止与被阻断，设置宽限期、冷却、连续上限并做启动扫描；内部 loop guard 发起的取消另行标记。[AC1]

其幂等 guard 是构造提示词，让模型检查未知的上一次工具结果。真正恢复时，`fire()` 构造 `source: { kind: 'user' }` 的消息，通过 `agent.followup` 发出。[AC2]

我们的选择：

- 自动恢复保留明确的 system recovery 来源，不伪造成用户新授权；人工停止、权限拒绝、锁屏不能被续跑覆盖。
- 网络恢复与动作重试分离：重连后先查询状态和重新观察；上一输入结果未知时停在 `unknown`，不自动重放。
- 对只读操作允许有界退避；对写操作由 Broker 的 actionId、调用状态和目标后置条件决定下一步，不能仅靠提示词。
- 轨迹记录恢复次数和原因；不为了“保持运行”向同一会话无限注入 Continue。

### Flowglass 与 Task Graph：做“看得懂发生了什么”

Flowglass 现有默认包是静态 host/client，不必连带启用其仓库里的 dynamic-toolbox。源码用 session seq/snapshotEvents 与增量 readFrom 避免无变化时反复全量重建；按 callId 配对调用和结果、显示重试与耗时。[FG1]

Task Graph 的 `graph.js` 将轨迹事件转成节点/边，`analytics.js` 计算执行图的关键路径和耗时。它适合作为诊断视图，不是一个新的 Agent 调度引擎。[TG1]

Grok 采用紧凑轨迹：**观察 → 动作 → 验证 → 等待人工/重试**；展开后查看目标、结果、耗时和错误。工具结束不自动标 `verified`。模型耗时、driver 耗时、等待授权、重试分开统计；并发区间按时间线去重，不把嵌套时长相加当成总耗时，也不将图上的推断关系当成实际依赖。

不直接移植它们的 DSH 事件格式、大块生成 UI 或全部原始日志。只消费 App Broker 的规范事件，敏感参数不进默认轨迹；遵循 Grok 模块拆分、15 locale 和现有面板组件。

## 6. 写回主方案的具体增量

| 批次 | 此次新增要求 | 完成证据 |
| --- | --- | --- |
| P0 / P7 | BrowserSkill 与 Playwright 扩展比较；选一条默认已有 tab 连接路径 | 三 OS 连接、身份/归属校验、借用取消/归还、断连停止与延迟记录 |
| P1 / P2 | 调用状态按 run 隔离；语义后台、定向事件、前台输入分开报告 | 两会话不会共享快照或默认当前 tab；取消后无迟到新动作；不静默扩大到 desktop |
| P3 / P4 | 引入 Windows 与 macOS 插件的边界案例 | 输入路径、DPI、身份复用、结构未变但动作已发生、取消与后台能力实测 |
| P6 / P7 | UI 预览与模型观察分开；按可见订阅采集；借用 tab 明确归还 | 收起预览停止周期截图但不阻止模型主动 observe；断流重连 snapshot + revision 有序 |
| P8 | 规范事件轨迹、有界恢复与真实耗时统计 | 自动恢复来源可识别；stop/deny 不恢复；unknown 不重放；耗时不重复计算 |
| P9 | 第三方来源/许可证与私有系统接口逐项复核 | 固定版本、版权/NOTICE、源码可追溯；浏览器/可选编解码器单独核对 |

不新增“DSH 插件市场兼容层”，不把 DSH/Pi 改成第二套主 Agent 引擎，不增加用户未要求的后台自动续跑。三平台首版共同核心和原有阶段边界保持不变；增量工作进入 P0 后重新估算，不能宣称看过插件就缩短了工期。

## 7. 固定源码版本

所有源码链接固定到以下提交；仓库后续更新不自动成为本报告结论。

| 项目 | 本次检出 SHA |
| --- | --- |
| Tencent/BrowserSkill | `3c5f838c56b5f5f20632804bab83b8a98fea5bf2` |
| Anionex/dsh-computer-use | `97b9731abcccb14d32d3985df971982a56506e1a` |
| Yu-tao-Li/dsh-computer-use-win | `04f4643eba86b28ee26788dbe72e34fa87503852` |
| ZRui-C/dsh-computer-use | `0b0a0844018b56a6a8e95aefea6529004b8341c4` |
| 988hj7tczd-oss/dsh-computer-use | `07477f49e7bbc0a9db83f3b133d13214e7b9394c` |
| ChenyuHeee/dsh-browser-playwright | `57ebe45fb906b6977312a0c7562d407faee4b449` |
| anweat/dsh-browser | `268b658fea54efaad32a4e33de660ef98818780d` |
| Fisfzy/dsh-ego-browser | `6133edfbdb3ceb6a982e0d4147860b3c11e1010c` |
| HsiangNianian/dsh-auto-continue | `592afc0c143c1daf1c4fb3dd01a4547dd0e626c8` |
| Iwctwbh/dsh-flowglass | `1ea3a3c0089b18bec2546a551255e1ddad2a513b` |
| KevinZhangNothing/dsh-task-graph | `7c230e0fe0af8103cfc2d1ee6ecf74a4f0c872aa` |
| geohotstan/dsh-computer-use | `0b623103419706a73c40e3c6dcf1ae6584cdaa06` |

## 来源

[DIR1]: https://github.com/awesome-dsh-plugin/awesome-dsh-plugin/tree/1504ae7bff7bc7824fde968579fa281015533c8c
[DIR2]: https://github.com/0xsline/awesome-deepseek-harness/tree/de3b4ca9b70aabd5147f409416daaa97a4d206c7
[BS]: https://github.com/Tencent/BrowserSkill/tree/3c5f838c56b5f5f20632804bab83b8a98fea5bf2
[BS1]: https://github.com/Tencent/BrowserSkill/blob/3c5f838c56b5f5f20632804bab83b8a98fea5bf2/packages/dsh-plugin-browserskill/README.md
[BS2]: https://github.com/Tencent/BrowserSkill/blob/3c5f838c56b5f5f20632804bab83b8a98fea5bf2/apps/extension/src/tools/tabs.ts
[BS3]: https://github.com/Tencent/BrowserSkill/blob/3c5f838c56b5f5f20632804bab83b8a98fea5bf2/packages/dsh-plugin-browserskill/src/queue.ts
[BS4]: https://github.com/Tencent/BrowserSkill/blob/3c5f838c56b5f5f20632804bab83b8a98fea5bf2/packages/dsh-plugin-browserskill/src/observation.ts
[BS5]: https://github.com/Tencent/BrowserSkill/blob/3c5f838c56b5f5f20632804bab83b8a98fea5bf2/packages/dsh-plugin-browserskill/src/sessions.ts
[BS6]: https://github.com/Tencent/BrowserSkill/blob/3c5f838c56b5f5f20632804bab83b8a98fea5bf2/crates/bsk-cli/src/daemon/ws.rs
[AN]: https://github.com/Anionex/dsh-computer-use/tree/97b9731abcccb14d32d3985df971982a56506e1a
[AN1]: https://github.com/Anionex/dsh-computer-use/blob/97b9731abcccb14d32d3985df971982a56506e1a/src/service.ts
[AN2]: https://github.com/Anionex/dsh-computer-use/blob/97b9731abcccb14d32d3985df971982a56506e1a/src/providers/native-helper.ts
[WIN]: https://github.com/Yu-tao-Li/dsh-computer-use-win/tree/04f4643eba86b28ee26788dbe72e34fa87503852
[WIN1]: https://github.com/Yu-tao-Li/dsh-computer-use-win/blob/04f4643eba86b28ee26788dbe72e34fa87503852/mcp/server.mjs
[WIN2]: https://github.com/Yu-tao-Li/dsh-computer-use-win/blob/04f4643eba86b28ee26788dbe72e34fa87503852/scripts/windows-uia.ps1
[WIN3]: https://github.com/Yu-tao-Li/dsh-computer-use-win/blob/04f4643eba86b28ee26788dbe72e34fa87503852/THIRD_PARTY.md
[ZR]: https://github.com/ZRui-C/dsh-computer-use/tree/0b0a0844018b56a6a8e95aefea6529004b8341c4
[ZR1]: https://github.com/ZRui-C/dsh-computer-use/blob/0b0a0844018b56a6a8e95aefea6529004b8341c4/src/native/client.ts
[ZR2]: https://github.com/ZRui-C/dsh-computer-use/blob/0b0a0844018b56a6a8e95aefea6529004b8341c4/native/macos-helper/Sources/DSHComputerUseCore/Input/TargetedEventPoster.swift
[CU]: https://github.com/988hj7tczd-oss/dsh-computer-use/tree/07477f49e7bbc0a9db83f3b133d13214e7b9394c
[CU1]: https://github.com/988hj7tczd-oss/dsh-computer-use/blob/07477f49e7bbc0a9db83f3b133d13214e7b9394c/lib/cua.js
[CU2]: https://github.com/988hj7tczd-oss/dsh-computer-use/blob/07477f49e7bbc0a9db83f3b133d13214e7b9394c/lib/snapshot.js
[CU3]: https://github.com/988hj7tczd-oss/dsh-computer-use/blob/07477f49e7bbc0a9db83f3b133d13214e7b9394c/lib/actions.js
[PW]: https://github.com/ChenyuHeee/dsh-browser-playwright/tree/57ebe45fb906b6977312a0c7562d407faee4b449
[PW1]: https://github.com/ChenyuHeee/dsh-browser-playwright/blob/57ebe45fb906b6977312a0c7562d407faee4b449/src/playwright.ts
[PW2]: https://github.com/ChenyuHeee/dsh-browser-playwright/blob/57ebe45fb906b6977312a0c7562d407faee4b449/src/tool.ts
[AW]: https://github.com/anweat/dsh-browser/tree/268b658fea54efaad32a4e33de660ef98818780d
[AW1]: https://github.com/anweat/dsh-browser/blob/268b658fea54efaad32a4e33de660ef98818780d/src/browser-service.ts
[AW2]: https://github.com/anweat/dsh-browser/blob/268b658fea54efaad32a4e33de660ef98818780d/src/auth-profiles.ts
[EGO]: https://github.com/Fisfzy/dsh-ego-browser/tree/6133edfbdb3ceb6a982e0d4147860b3c11e1010c
[EGO1]: https://github.com/Fisfzy/dsh-ego-browser/blob/6133edfbdb3ceb6a982e0d4147860b3c11e1010c/src/index.ts
[EGO2]: https://github.com/Fisfzy/dsh-ego-browser/blob/6133edfbdb3ceb6a982e0d4147860b3c11e1010c/runtime/ego-linux/src/chrome.mjs
[EGO3]: https://github.com/Fisfzy/dsh-ego-browser/blob/6133edfbdb3ceb6a982e0d4147860b3c11e1010c/THIRD_PARTY_NOTICES.md
[AC]: https://github.com/HsiangNianian/dsh-auto-continue/tree/592afc0c143c1daf1c4fb3dd01a4547dd0e626c8
[AC1]: https://github.com/HsiangNianian/dsh-auto-continue/blob/592afc0c143c1daf1c4fb3dd01a4547dd0e626c8/src/shared/core.ts
[AC2]: https://github.com/HsiangNianian/dsh-auto-continue/blob/592afc0c143c1daf1c4fb3dd01a4547dd0e626c8/src/host/engine.ts
[FG]: https://github.com/Iwctwbh/dsh-flowglass/tree/1ea3a3c0089b18bec2546a551255e1ddad2a513b
[FG1]: https://github.com/Iwctwbh/dsh-flowglass/blob/1ea3a3c0089b18bec2546a551255e1ddad2a513b/flowglass/lib/index.js
[TG]: https://github.com/KevinZhangNothing/dsh-task-graph/tree/7c230e0fe0af8103cfc2d1ee6ecf74a4f0c872aa
[TG1]: https://github.com/KevinZhangNothing/dsh-task-graph/blob/7c230e0fe0af8103cfc2d1ee6ecf74a4f0c872aa/lib/analytics.js
[GH]: https://github.com/geohotstan/dsh-computer-use/tree/0b623103419706a73c40e3c6dcf1ae6584cdaa06
[GH1]: https://github.com/geohotstan/dsh-computer-use/blob/0b623103419706a73c40e3c6dcf1ae6584cdaa06/docs/codex-parity.md
[GH2]: https://github.com/geohotstan/dsh-computer-use/blob/0b623103419706a73c40e3c6dcf1ae6584cdaa06/native/Sources/dsh-computer-daemon/SkyLight.swift

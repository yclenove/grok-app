# Computer Use：已有标签页动作内核与目标列表隔离

日期：2026-09-20。工作区 `H:\aicoding\grok-app-computer-use`，分支
`feat/computer-use-implementation`，HEAD `30757366`。未提交、推送或创建 PR。

总状态仍为 **partial — not releasable**。本批实现 C3 动作内核的第一部分，
没有把 ExistingTab 的动作能力打开；当前生产 MCP 仍只能观察已有标签页。

## 实现内容

1. `observe-document.mjs` 的模型快照持有原始 WeakRef、元素身份签名和有界
   MutationObserver。同 ID 换元素、原元素移除再插回、属性修改再改回、关联
   form/base 变化均不能恢复旧引用。校验包含当前 document、URL、可见性、
   视口/缩放/滚动、禁用状态及祖先排除条件。收到 blur/visibility/pagehide/
   resize/scroll 时退休快照；不能仅靠这些 DOM 事件证明 tab 未切换（见第 7 项）。
   新模型观察释放旧引用/监听器，preview 不取代模型快照。
2. 新的 `act-document.mjs` 只有固定函数，注入 ISOLATED 主文档，不接受脚本、
   selector、任意属性写入或全局当前元素。严格校验 operation/snapshot/ref 和
   参数，副作用发生前消费整个模型快照。重复操作不再派发。
3. 当前内核支持左键单次语义 click、普通 input/textarea 的 SetValue/TypeText、
   元素垂直 scroll，以及原始引用的有界 nameEquals Wait。输入使用原生 value
   setter 和 beforeinput/input/change 事件；TypeText 尊重当前 selection，
   不访问剪贴板。中文、emoji 和 textarea 换行可保持；不把字段值返回模型。
   focus/beforeinput/input 回调改目标或值时停止后续写入，结果为 unknown。
4. click 仅报告 applied，不能自行证明页面业务成功；输入/scroll/Wait 必须
   满足实际后置条件才 verified。滚动使用 instant，避免返回后仍继续平滑滚动。
   下载、新窗口/继承 base target、file input、脚本/带凭据 URL 不走此 click。
5. Wait 一次只有一个执行者，preview 可读但不能替换模型引用。取消退休快照、
   唤醒并收束等待 timer，`cancelDocumentOperation` 等待该 operation 的 finally。
   **这不等于 Host 已确认远端空闲**：调用者还必须拥有并收束原 executeScript
   Promise；snapshot_missing 也不是完成证明。
6. 回归中发现 `Broker::list_targets` 对已授权浏览器/WebView 仍先枚举 Desktop。
   现已直接向该授权 surface 的 executor 列目标，不读取其他 surface，也不
   继承其枚举失败。Host 未授权发现入口保持独立。新增故障注入测试断言桌面
   枚举调用数不增加，而 ExistingTab MCP 仍返回正确目标。
7. `SharedTabs` 保留实际模型观察的 session/run/grant/document/snapshot、view
   及扩展 activity 版本。真实 tab/window 激活事件立即使旧动作依据失效。
   新 `act()` 在派发前复验这个记录并固定 Chrome documentId；它持有原始
   executeScript Promise。取消、Unshare、reset、deadline 仅请求固定取消函数，
   在两个调用都返回前占用仍保留。没有可验证的结束回复时保留 unknown/busy，
   不用 snapshot_missing 伪造完成。最多保留 8 项，占满时拒绝新动作。

权限、manifest、默认 feature、模型工具 schema、Host extension protocol 均未扩展。
新动作函数已由 SharedTabs 静态导入，但生产 transport 尚未派发动作，Rust
adapter 的动作能力仍为 NONE。
这是分阶段实现，不是已经可让用户操作已有 tab 的宣告。

## 首败与定位

证据目录：`tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/`。

- `existing-action-refs-red.log`：两个断言失败；旧 WeakRef 映射没有 document-bound
  resolver，不能拒绝 remove/reinsert 和 href 修改后恢复。修复后均通过。
- `existing-action-live-first.log` / `second.log`：探针过早使用 worker-created
  对象，Chrome API/timer 绑定尚未可用；未进入动作验收。
- `existing-action-live-third.log`：MV3 禁止 service worker 动态 import。
  探针改从受信扩展页导入固定源码函数、调用真实 chrome.scripting；没有改
  生产 SW、权限或注入假结果。这条证据因此仅是 MV3 DOM 内核，不是 SW/Host 链。
- `existing-action-live-fourth.log`：10 个页面后置条件已通过，但报告版本时
  persistent context 的 browser() 为 null，整个命令 exit 1；已修复报告器。
- `existing-action-observation-app.log` / `diagnostic.log` / `targets.log`：真实
  App 链在 MCP targets 阶段失败，尚未进入此次 DOM 模型观察。分段诊断只输出
  固定检查点和布尔量，不输出凭据/内容。
- `existing-listing-red.log`：独立 Core 回归确定复现不必要的 Desktop 枚举依赖，
  `computer_list_targets` 返回 adapter 错误。它证明依赖缺陷，不把注入错误
  冒充实机 Win32 的完整根因诊断。修复后全 Core 与重建探针的 34 项实机链通过。
- `existing-action-live-complete.log` / `existing-action-focus-diagnostic.log`：
  真实 headless Chrome 切走再切回时 DOM focus/visibility 事件列表为空，直接
  调 DOM 内核仍 applied。没有放宽断言；把准入放到实际 SharedTabs 的扩展
  activity 版本上，生产源方法拒绝旧快照。探针中的 Host 授权 client 是 fixture，
  Chrome tab/document 事件、固定脚本和页面副作用均是真实调用。
- `existing-action-scope-tests-first.log`：并行启动 jsdom 时扫描耗尽真实 200ms
  预算，首个输入节点未进入快照，一项测试失败。串行执行原断言后通过；未
  放宽生产预算或虚构节点。保留首败，不能把该用例作为性能基准。

## 验证记录

| 检查 | 本批结果 | 证据 |
| --- | --- | --- |
| 扩展 + MCP golden | 75 passed，0 skipped，串行，系统 Node 24.15.0 | `existing-action-all-extension-final.log` |
| 完整 Core | 405 passed，0 ignored，31.17s | `existing-action-core-final.log` |
| Driver | 12 passed，0 ignored | 同上 |
| Core strict Clippy | all-targets/all-features，`-D warnings`，exit 0 | `existing-action-core-clippy.log` |
| App strict Clippy | lib + computer-use-probe，`-D warnings`，exit 0 | `existing-action-app-clippy.log` |
| 重建 cu_probe | exit 0；既有 Windows 链接器创建库 stdout 警告保留 | `existing-action-build.log` |
| MV3 内核及 SharedTabs 真浏览器 | 13 项通过，私有 Node 20.18.0 + CfT Chromium 148 | `existing-action-controller-live-final.log` |
| App/MCP/IPC/MV3 观察回归 | 最终 34 项全部通过，exit 0 | `existing-action-observation-app-complete.log` |
| Rust fmt / diff whitespace | exit 0，未更改换行配置 | 本批工具输出 |
| 15 locale / code quality | 一致；final PASS，千行文件 80/80 | `existing-action-quality-final.log`、工具输出 |

本批没有前端行为改动，也未重跑受管 worker Node、source/seed 原生下载/Stop
矩阵。上一批 Core 404 / browser 122 / managed 11 行数据不能写成此次新结果。
没有跨平台、安装版、真实模型或全候选 freeze/长稳证据。

13 项内核/控制器探针不经过 Host 配对授权、生产动作 HTTP 或模型 actionId
账本；它不能替代 C3 端到端动作验收。34 项 App 链仍是观察及取消观察，不能
把两个计数拼成“完整模型操作已通过”。扩展动作的 physicallySettled 是内部
结果，在 Host 回执合同落地前不能将其加入模型工具结果或据此开放能力。

最终 App 观察取消实测约 6ms，tabOpen=true；一万控件的观察 277ms、truncated=true。
都是隔离 fixture 的单次数据，不是用户体验或跨机器性能承诺。当前 8 个涉及的
JS 源/测试文件经过 node --check，均 LF、无尾空格；Rust fmt 与 diff check 通过。

## 工件追溯

局部源码指纹（不是 C6 候选冻结）：283 文件，
`75A867397304CC8829D03B421342CC0162E580B1987E30CB48CC354F1AF38C4B`。
cu_probe SHA256：`2EFFF3FC1C07CEB7679960CA33FEF039D7ED9C963BAA8F431C0CC5994E190294`。
日志：`existing-action-fingerprint.log`。

此次范围明确包含 `tools/computer-use-probe`，不要与上批 268 文件摘要直接比较。
通过 rg 枚举以下范围的 rs/mjs/json/html/css/ts/tsx 文件（遵循 Git ignore）：
Core src、App `src-tauri/src/computer_use`、browser/extension/MCP/probe tools、
`src/components/computer-use`、`src/lib/computer-use`、`src/lib/api/computerUse.ts`
及 authorize/pairing 两个 API 测试。路径改为 `/`，PowerShell Sort-Object 排序，
逐行 `路径 + 空格 + 大写文件 SHA256`，LF 拼接、无末尾 LF、UTF-8 再 SHA256。

最后只读进程核对：没有匹配本轮 owned cu_probe/private Node/临时 Chrome 的
进程残留。探针临时 profile 在核验 owner marker 和绝对路径后清除，保留证据
目录和既有隔离 App home。分支/HEAD 未变，index 空；未操作用户浏览器资料。

## 紧接着做什么

### C3.2：先补生产调度的物理完成合同

- 当前 `host/transport.rs::sweep_requests` 和 pairing invalidation 会直接删除
  取消/到期的 delivered 请求。现有只读结果可丢弃，已派发写操作不能据此 idle。
  必须区分“禁止继续使用结果”与“原始操作物理完成”；先写失败测试再改队列。
- Host 原授权失效时立即取消模型请求，但保留有界的 delivered-write 占用。
  timeout、HTTP 断开、Unshare、换配对、feature-off 不得让同 tab/new run 复用。
  StopRequested 只有在远端结算和本地 admission 都收束后才能 Stopped。
- SharedTabs 的原 executeScript Promise/取消注入 join 已实现；接下来把它
  贯穿 ExtensionTransport reset/epoch 与 Host 回执。不能沿用观察路径的
  Promise.race 5 秒就归还占用，也不能 skip late completion。用户 tab 不能
  被关闭用于清理。
- 先确定重配对后仍能安全送达的完成回执合同。可以评估仅可归还单个 request
  的一次性 completion proof，但这是待设计项，不是已采用/已实现的协议。
  原 pairing key 不得保留或恢复；失效的动作、观测、授权请求仍必须拒绝。
  无法证明远端已结束时保留 unknown/pending，不能靠时钟到期伪造空闲。
- 必测：派发前取消、executeScript 排队中取消、Wait 取消、写入完成但响应丢失、
  导航/permission 失效、SW 重启、替换 pairing、两 session 争抢同 tab、另一
  owner 不受影响。只允许重送幂等清理回执，绝不重新执行动作。

### C3.3：合同通过后再接通动作与能力

- 生产 transport 接到现有 SharedTabs.act；strict Rust/JS 协议同时更新并协商能力。
  protocol 1 的 Observe-only 扩展不能被推断为支持新动作。
- session/run/tab/document/grant/geometry/snapshot 和 cancellation 从 Broker
  准入一直绑定到 executeScript 的 Chrome documentId；不能只依赖 DOM UUID。
- DOM kernel 的 operationId 使用唯一 transport dispatch 身份，模型 actionId
  仍由 Broker 去重；两者不可混淆。表单内容不得进入日志、回执或诊断。
- 接上 adapter.act 和实际节点 actions，只有验证过的动作/目标组合才能被宣告。
  unknown 必须重新 observe，不能补发；页面计数器必须证明副作用确实只发生一次。

### C3.4 及之后：完整范围不缩减

键盘、导航、contenteditable、窗口/页面滚动等完整能力仍在后续范围，当前内核
不支持不代表删需求。不得用单纯 dispatchEvent(KeyboardEvent) 冒充浏览器默认
键盘行为，也不扩大 Cookie/debugger/all_urls 权限来绕过实现。

保留 C1/C2 真实 toolbar activeTab/截图、原生启动内 Pause/Stop、全 App/ACP
生命周期及恢复体验、Chrome/Edge 安装、macOS arm64/x64、X11/GNOME native
Wayland、真实模型、全候选至少 12h 主动长稳。单 writer、重测试串行、只用
隔离资料和自建 fixture；不碰真实账号/日常 profile/代理，不提交推送。

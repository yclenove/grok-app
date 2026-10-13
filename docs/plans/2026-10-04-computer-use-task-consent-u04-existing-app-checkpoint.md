# E4a：ExistingTab 原 App 任务入口与准备查询

状态：**implemented / partial validation — not releasable**。完整 CU 目标 active。
工作区 `H:/aicoding/grok-app-computer-use`，分支 `feat/computer-use-implementation`，
HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`。单 Codex writer；保留全部
dirty/untracked/fix.patch，未启动 Grok/子代理、未提交/push/PR/发布。

## 本批实际改变

1. 原 App picker 严格接受 `desktop` / `existing-tabs`。ExistingTab 在 Core
   begin 前只注册所选 metadata executor；发现沿原 OwnedTasks/blocking handle。
   不初始化 Desktop、driver、浏览器或权限，不改变 default-off。
2. Core 的原 preparation guard 增加只读状态：原 challenge/显式确认状态、
   authenticated registry 中可用/不兼容/已借用的有限数量。它不枚举全部 tabs、
   读取正文/截图、借用页面或生成执行 grant。原 TTL/full-key/epoch/fence 保持。
3. 原 PickerSlot 保留未 discover 的 call。未 Pair、未 Share、只有 legacy
   documentless 分享或全部被占用时，load 返回等待，不提前消费原 call。
   Share 完成后同一 picker 可加载原 opaque candidate receipt。
4. 新的 preparation / pairing-confirm 命令经原 SessionManager→ACP→Core，
   main WebView 限制不变；使用原 MCP entry 的唯一 loopback endpoint，不重新
   bootstrap 全局 runtime。一次性 code 只进入必要 Host UI DTO，不返回 bearer、
   native proof、原 tab ID/URL 或 share ID 给模型。
5. 补测发现旧 picker 原来未保存 App process identity。现在 begin 保留原 process，
   原 App slot publication 边界下验证查询/确认/取消，加载/审批先验原 owner，
   await 后仍复验，最终审批仍沿原 App/ACP/Core publication，不另造授权系统。
   process 变更但 session/agent/ACP 相同时，不能借旧 picker 查询或确认。
6. raw `TaskScopeTarget::parse` 仍 Desktop-only（有意保持 fail-closed）；没有用
   renderer raw `tab:` 绕原 receipt。ExistingTab descriptor、共享 controller、
   UI/15locale 仍未开放/改变，E4b 与 E5 没有冒称完成。

相对 E3 source manifest：19 个既有 source 改变，3 个新增 source。没有依赖、
默认值、App/AppWorkbench、UI 文案或模型 schema 改变。

## 同最终源码证据

证据目录：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。

| 验证 | 实际结果 |
| --- | --- |
| Core default all-targets | 1065 passed / 0 failed / 0 ignored：unit861 + contracts204 |
| Core test-support all-targets | 1081 passed / 0 failed / 0 ignored，含 driver16 |
| 默认 App picker 定向 | 18 passed / 0 failed |
| 默认 App SessionManager preparation | 2 passed / 0 failed（4 owner replacement、3 App slots） |
| 默认 App execution runtime | 4 passed / 0 failed |
| 默认 App CU 完整过滤集合 | **276 passed / 1 failed / 1 intentional ignored**，408.52 秒 |
| 默认 App ACP / routing | 174 passed / 38 passed，各 0 failed |
| 四种 all-targets strict Clippy | Core/default-support、App/default-probe 全部通过 |
| Extension/bundle / protocol-golden | 177 / 38 passed，各 0 failed |
| fmt / quality / diff | 全部通过；App shell + workbench 行数仍13573 |

各过滤集合重叠，不相加。新增1个 Core 与10个 App回归；24个原前后Pair
fence组合增加 preparation-readback 断言，仍是原测试，不当作24个新测试。
所有默认 App harness 构建后按 CI 嵌 Common Controls manifest，确实进入断言。
前端全量、真人 App/MV3/toolbar/Grok、安装态、三平台整链、真人TTL/12h 未运行。

最终3169源 manifest SHA256：
`9467d6b89b930d643ed6c9698243083e141835b045be84d2b8bcbd6c66ad55d8`。
最终 App（`app-artifact-5.json`）SHA256：
`8af6e2bb1b6b000eddf82a0052ea297f4c22e1b0f85a14663bff4ae5cc762d60`。

## 失败证据，不能擦掉或写成全绿

- `app-red-1.txt`：原 App Desktop-only producer 的实际断言失败，0/1。
- `app-build-2`：新增 fixture 误用 legacy shared-candidate 三元 tuple 为二元，
  编译失败；只修测试 fixture，不能算 assertion red 或验证通过。
- `app-red-2.txt`：process replacement 仍可读原 preparation，0/1 实际断言红；
  保留原 process pin 后，最终同名回归通过，没有删除/放宽反例。
- `app-computer-use-1.txt`：
  `managed_native_cleanup_consumer_retains_original_owner_past_reply_deadline` 失败。
  fixture 标明 `close-event` 6793ms、`close-native-settled` 28240ms，native关闭
  duration21530ms，超过测试 release 后20秒的等待。该事实**不证明**生产泄漏，
  也不证明仅是测试期限问题；未取得 same-owner physical-settlement 后 Host 状态
  的足够细证据。没有提高超时、跳过用例、自动重跑刷绿或更改 cleanup 生产代码。
  supervisor/fixture original owners 已退出，失败 root 保留：
  `C:/Users/Administrator/AppData/Local/Temp/grok-cu-managed-task-c606cd95-0c85-48c5-a94c-cd6a9632e655`。
  events 已复制到 evidence 的 `managed-cleanup-failure-events.ndjson`，SHA256
  `a8b2761c5fd79bc604c0493252016c23fba8a87aa9e80e0fa351b63f3f2236b4`。

原 managed 自然 cleanup_pending 长尾仍开放，本批 full App red 不可隐藏。

## 下一步（不缩减完整目标）

1. 无编译/其它 fixture 干扰下，受控复现原 managed close：记录原 worker/native
   settle→consumer retry→Broker Stopped 的身份与时间，核真实 root cause，
   不能仅放大 test timeout。按原 owner 修复或纠正错误断言并保留前后证据。
2. E4b 接共享 controller/card/panel。Pair/Share 阶段需要取消、Stop、busy/empty/
   incompatible/retry、安装入口、同 key/epoch 的迟到回复拒绝，15locale同步。
   必须新增“readiness 与 discover 之间撤销 Share / discover 空结果”的反例与
   同原准备安全重试；本批尚未证明这个竞争下的完整交互，不能冻结空候选后无路可走。
3. descriptor 只在完整后端/UI可用后开放；同最终 candidate 进入 E5 source MV3 +
   authenticated HTTP + 原 App/ACP publication + TaskGrant 联合链，不用 legacy
   `mcp-open` grant 代替。不自动批准真人 Share/系统权限。
4. 原 U00–U09/D00–D31、四surface、Windows/macOS/Linux实机、全输入/恢复、
   安装/签名、真人Grok、12h与完整UI/UX继续；局部定向绿不关闭交付目标。

## 本批构建清理

按 AGENTS 第9条生成**新的**15组 old/current exact cache plan，inspection通过，
逻辑6812943924 bytes（约6.35GiB，可能含hardlinks）。第一次 execution 被仍活跃的
cargo/rustc 安全挡下（`cache-execution-1.txt`），没有删除，不杀其它编译进程。
随后确认原 owners 退出，重新核验15组精确 old/current pairs、3169源冻结及当前
App/Core/seed/fix.patch hashes，第二次 execution 已完成（`cache-execution-2.txt`）。
实际删除15组废弃增量缓存，H盘空闲观察增加6194597888 bytes（约5.77GiB）；
before/report 为 `cache-cleanup-before.json` / `cache-cleanup.json`，保留全部首败和
managed失败root。当前程序、最新缓存、immutable seed、依赖和源码未变。
删除未进回收站，但这些缓存可由 cargo 构建重新生成；不删除整个target/workspace。
此清单已经执行，不得再次复用。下一批须重新检查 owners 和精确目标，另建清单。

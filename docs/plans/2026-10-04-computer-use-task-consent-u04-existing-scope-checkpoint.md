# U04 增量：ExistingTab 原分享身份与任务预留基础

状态：**ExistingTab backend scope foundation verified / full Computer Use active / partial — not releasable**。
工作区 `H:/aicoding/grok-app-computer-use`，分支 `feat/computer-use-implementation`，
HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`。单 Codex writer；原 dirty/untracked/
`fix.patch` 保留；未启动 Grok/子代理，未 commit/push/PR/发布。

## 实际缺口和本批实现

ExistingTab 五种 typed actions 已有生产实现，本批不重做。缺的是新任务的原实例 scope：
原 adapter 无 scope_identity/candidates，Share 的真实 Chrome documentId 只留在 SW。
规范 `existing-tabs` + `observe/input` request 在合法 capture 时实际失败：
`exact task scope identity is unavailable for this backend`（`core-red-3.txt`，0/2，exit101）。
这是实际缺失能力反例，不是已存在的越权漏洞。

- `SharedTabs.share` 上报真正 main-frame documentId，经 PairingClient、原认证 HTTP DTO、
  Broker gate/锁序到 Host registry；不导出网页 Cookie/token。optional 字段兼容旧 Share，
  缺字段可走原 legacy transport，但不能证明新 TaskGrant，不暗补当前身份。
- Host-only `ExistingScopeIdentity` 私有字段、不可 serde、自定义 redacted Debug。绑定原
  pairing instance/connection nonce/generation、tab、原 share nonce、documentId/generation、
  browser/profile；不含 title/URL/认证 token。它不能由模型 UUID 或 liveness 组装。
- 已认证分享 registry 提供 readonly identity/info/bounded candidates；不枚举日常全部 tabs、
  不读正文/截图、不借用、不创建 execution run。过期/占用/无 proof 的行拒绝或跳过，
  discovery 复验 cancellation；仅 canonical JS-safe 数字 tab ID 可进入新 scope。
- 原 Pending `AccessApproval` preparation 依据该 proof 在同 Host lock 内 acquire 借用，
  前后复验原 request/executor/feature epoch/deadline，再沿原 App/ACP publication 提交。
  不借 legacy run，不收养 foreign grant；失败仅精确 release 本次 acquired TabInfo。
  Stop 归还借用，不能关闭用户 tab 或取消其 Share。
- 认证观察回包的 documentId 必须与原 Share 一致，在 consumer/model snapshot 发布前
  拒绝错文档；preview 同样校验。旧无 document proof 的 transport 继续原行为。

## 新回归与真实首败

本批 Core 新增17项，使用生产 Host registry、reservation 和原认证 HTTP，Host gesture/
agent 是 synthetic；**没有真人 Share、实际 MV3 新 TaskGrant、主窗 App/ACP/Grok 整链证明**。

覆盖原 capture 无 borrow/content/grant、原 reservation→commit→Stop、相同 tab/document/
generation 再 Share 换 nonce、prepare 后文档替换回滚、foreign/legacy 占用不收养/不清理、
旧 Share 兼容、非法 document 先拒且不消耗 sequence、取消零 borrow；另含 canonical IDs、
候选限量/取消/占用、连接 replacement/expiry、新 Host 原 proof 拒绝、Debug redaction、
HTTP Share→原 reservation→Stop、HTTP malformed/legacy、model/preview 错 document 零交付。

实际 canonical 候选反例 `core-canonical-red-2.txt` 为0/1，exit101：列表曾把旧 opaque IDs
当新 scope 候选返回。将 canonical 校验放入原 identity producer 后，全量实际通过。
`clippy-core-1.txt` 实际发现新大 identity 令 ApprovalTicket 触发 large_enum_variant；
在 `ExistingPage` 的新大字段用 Box 持有，未豁免警告、未改变权限或分享语义。

所有夹具/运行器错误也保留且不冒称生产反例：最初 `core-red-1/2` 错用 schema；
`core-canonical-red-1` tuple 夹具编译失败；`core-green-3` 不存在的 preview helper 编译失败；
`core-green-1` 错把合法 NoAuthority continuation 当 executable approval；`core-green-4`
错把 `is_closed` 对缺失记录的 true 当关闭证据，已保留原 borrowed record 再验证真实 expiry。
`quality-1.txt` 是运行器单参数被 PowerShell 字符 splat，未进入质量检查；修 runner array，
`quality-2.txt` 实际 RESULT: PASS。无首败删除、结果覆盖或放宽生产安全条件。

## 最终候选验证

证据 `tools/computer-use-probe/.run/codex-u04-existing-scope-20261004/`。
Box 修正后的最终 source manifest2 与 App artifact2，未借用中间候选的绿色结果：

- Core default **all-targets 1021 passed /0 failed /0 ignored**：unit817 + contracts204，
  `core-all-2.txt`，实际 exit0；test-support **1037/0/0**：同集合 + driver16，
  `core-support-2.txt`，实际 exit0，不重复计算16。Mac/Linux 名称的 portable contracts
  在 Windows 上运行，不是其 OS 实机原生验收。
- 同最终默认 App harness 宽 CU **267 passed /0 failed /1 intentional ignored**，134.07s；
  ACP **167**，routing **38**，均 exit0。集合重叠不相加；live_handshake 未运行，真实
  10分钟 TTL 手动项本批未另跑，上一批日志保持原样，不冒领为本批证据。
- Core default/test-support、App default/probe **all-targets strict Clippy**实际通过；
  fmt、quality、diff通过。Core all-targets test-support 的 native fixture test harness
  仍有既有 dead-code warnings，未把普通 test 的 stderr 宣称为无 warning。
- 全 extension unit + bundle audit **173 passed /0 failed**，`node-1.txt`；新增 Share proof
  payload/非法值零 HTTP、source SharedTabs 真 document 及 late replacement 原 proof 断言。
  本批未改 UI/locale/依赖，未重跑前端全量，不冒领旧前端8031为当前新增结果。

冻结3151源（**这次包含 tools/computer-use-extension**），`source-files-2.sha256` SHA256：
`20f8b9e8f7af7864789114c68118751e4acf72276854fc511bc84a5ac2491fe5`。
manifest1 是 Box 修正前的中间候选，仍保留，不能替代最终源码。

默认 App features=[] harness `grok_app_lib-dc611c8f8bc4f678.exe` SHA256：
`107e0b30a36f8e297fbea6dfda06aeda7e4b123295b0b587d0b6a3bd0762194f`。
按既有 CI 后链嵌 Common Controls manifest，实际进入断言；**不是新开发包/发行包**。
Core default/support harness SHA256分别：
`7a3b700c600e7a2044c26bcb603c917e740ca23e5dfbfc18e75fe0b82ec3f6a0` /
`8fe929b54b1189501a9128505e724f197dda368d332d3d5e90401182f2d1cc70`。
immutable seed manifest仍为
`001cf9617ce64b6cb6448c38a848533577a6116f33138bbc242e8e03e873b5ec`。

## 构建卫生

原 Core、App、strict 和各运行器均已实际终态并从原句柄读回退出码。审查13组本批
old/current exact pairs，inspection-only exit0 后只 execute 一次。脚本拒绝 live owners、
reparse、路径逃逸、未完成/换代缓存；未中止无关进程。

删除 **6,681,994,315 logical bytes（6.22GiB，含 hardlink）**；H 空闲实际观察增加
**6,411,382,784 bytes（5.97GiB）**。`cache-cleanup-plan.json`、before、report完整保留；
latest caches、App/Core、immutable seed hashes不变，清理后3151源逐项复核、diff通过。
仅删除精确旧 incrementals，不动 whole target/node_modules/其它 worktree、当前包、依赖、
源码、首败现场或日志/hash。缓存不进回收站，可用 cargo 重建。
清理后观察到另一 session-import 工作的独立 C:Temp target 已开始编译，未触碰/中止；
不将“本批 owners 已退出”写成全机永远没有编译进程。

## 未开放范围与下一步

**本批 backend-only，没有开放新的 App/UI picker gate。** 生产 Core begin_task_scope_picker、
App parser/descriptor仍 Desktop-only；off 首次 Pair/Share 流程仍未接，多成员仅借 active
tab，inactive member switch 尚未接。直接 Host 的 off 夹具不能证明真实首次使用不需设置。

继续按[ExistingTab 后续详细执行顺序](2026-10-04-computer-use-task-consent-u04-existing-next.md)：
E1 原分享 Host picker → E2 多成员切换/恢复 → E3 受原事务约束的首次 Pair+Share →
E4 App共享 UI/15locale → E5 同最终候选 source MV3/App/ACP 联合验收，不借 legacy
`mcp-open` 绿色替代新 TaskGrant。随后继续 WebView、其它U04、完整U05/U06、U07–U09、
D00–D31、Windows/macOS/Linux、真实 App/Grok、签名安装、同最终候选12h与整体UX。
原自然 cleanup_pending 长尾根因仍开放；本批既有 managed fixture 绿不等于已修复。
完整持续目标保持 active / partial — not releasable，不缩减平台/surface/输入范围。

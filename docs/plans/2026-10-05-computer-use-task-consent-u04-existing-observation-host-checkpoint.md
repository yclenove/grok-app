# ExistingTab：观察生命周期的 Host / IPC 基础

本批 source89 只完成 R5 的 Host / IPC 阶段。整体仍 active / partial — not releasable。
source82 的原 Pause / Chat Stop 迟到 PNG 反例仍 OPEN，未拿 synthetic Core 测试代替原 App/native 验收。
单 Codex writer，没有启动 Grok / 子代理，没有 commit / push / PR / 发布。

## 实现与边界

观察协议升至 v2。每个请求携带原 Share 的 documentId 和独立 observation 用途 proof，
严格绑定 request / connection / session / run / tab / document / generation / snapshot。
观察与动作有独立 CSPRNG / MAC authority，双方 proof 不能互换；首次观察不借旧模型 snapshot。

原请求只有 delivered → 一次 claim → 原执行器 physically settle 后才接受结果。
取消、超时、Pair/Share 撤销立即退休原 sender，但保留已 claimed 请求直到 settle。
未 claim 的工作按 NotStarted 退休；重复 claim / 旧 binding / result-before-settle 均拒绝。
settle 可幂等重试，cleanup status / settle / retirement 不依赖 Pair Bearer 或 feature / executionReady。

Host PNG 验证在锁外进行，独立 processing 占用跨取消保留，验证成功/失败都归还。
Broker 不再持 feature mutex 等 PNG 验证，Pause/feature-off 可及时取消；发布前再次验证原 grant。
idle、同 tab 新借用/重连和观察/输入 admission 包含原观察占用。
Pair 撤销不清已执行请求，browser cleanup 只根据原 browser / request / document 记录收束。

新增独立 IPC `/cu/extension-observation/{negotiate,claim,status,settle,retirement,host-lifetime}`，
严格 schema、extension origin、4096-byte 上限和独立 rate bucket；错误不回显凭据。
只 claim / negotiation 需要当前 Pair 和启用状态，cleanup 仅能收束自己原来的占用。
没有增加模型授权工具，没有改变权限、扩展 key、managed seed、默认功能或用户环境。

**扩展仍是 v1 请求 validator，尚未接 observation scope / watcher / journal / recovery。**
当前树是 Host 基础开发检查点，不是可运行的新版整链候选，不应启动/打包作完整 CU 验收。
默认 App EXE 仍为 source86 的 artifact，不把它映射为 source89。

## 本批验证与首败

source89 冻结 3310 files，manifest SHA256
`a070cec95538e077b79ac575e595566925e61b3607c5d02cd8ffde204ed6f856`。
证据根：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。

| 检查 | 实际结果 |
| --- | --- |
| Core default 全量 | 1097 passed / 0 failed，exit0；`existing-observation-lifecycle-core-default-89-1.txt` |
| Core test-support 全量 | 1113 passed / 0 failed，exit0；`existing-observation-lifecycle-core-support-89-1.txt` |
| Core default / test-support strict | exit0；9.90s / 12.51s |
| App / probe strict | exit0；63s / 72s，仅编译检查、不生成新默认App候选 |
| Node / MCP / 固定扩展校验 | 193 / 38 / 7 passed，exit0；扩展仍45文件、15locale，不能表示v2已接线 |
| fmt / quality / diff / source Verify | 实际通过；App+Workbench13573、≥1000行文件80，主fixtures994/983不变 |
| 新 App artifact / 原82 PNG / 完整 native / 宽CU / ACP / routing / 前端全量 | 本批未运行；待完整扩展合同后 fresh 候选验收 |

新增8项 Host、3项 IPC 测试覆盖 claim 竞争、用途隔离、未开始取消、六种 claimed 取消、
unshare/replacement、首次无旧 snapshot、结果先于 settle、Host PNG 验证收尾、原 browser cleanup。
PNG 验证门闩使用 fixture PNG 和 synthetic 执行器；HTTP 是真实生产 IPC，浏览器 peer 仍 synthetic。

87 的定点12项实际通过，但不是冻结候选。88 全量首轮为889 passed / 1 failed、exit101：
公共 HTTP Share fixture 增加 documentId 后，原“缺 documentId 的 legacy Share”反例不再缺该字段。
修复是该反例显式去掉 documentId，保留原断言；没有放宽生产检查。
首败日志 `existing-observation-lifecycle-core-default-88-1.txt` 保留，SHA256
`85cf971a3946ef4cad0da11cefaad4ab90ce0853cbe1a3ae3241d57ae843c10e`；89新增 claim/replacement 等3项后全量通过。
support 全量中 cu-macos-native test harness 有两条已有 dead-code warning；生产 strict 检查独立执行，
不因此增添全局 allow 或删掉原 Mac 合同测试。

## 收尾清理（用户要求每批执行）

本批所有原 Core / strict / Node owners 实际终态后，fresh
`existing-observation-host-cache-89-1` Plan 逐项回读13个 old/current 同variant完整generation，
拒绝越界、reparse、working和活跃owner，再执行原 `cleanup-batch-cache.ps1`。
使用source89/source manifest和保留artifact86只是验证各自身份，没有运行旧App冒充新候选。

13个精确target删除，逻辑3632129417 bytes（3.38 GiB）；
H freeBefore243269304320 / freeAfter246671716352，空闲观察增加3402412032 bytes（3.17 GiB）。
两者分别记录，空闲变化包含并发磁盘活动，不能宣称逻辑大小全部物理回收。
删除未进回收站，Cargo重新构建可生成这些增量缓存；source3310、各variant最新代、默认App86、
两Core EXE、CFT140、immutable seed、fix.patch、依赖及全部失败现场保留且hash复验不变。
working / 跨variant / 归属不明runtime和失败目录继续延期，没有停止无关进程。

清前清后各复验37hash/5原absences、64–66的3log/12hash、67/68的2log/6hash、
71/72/74的3log/9hash、75–77的3log/9hash及82的1log/10profile/PNG hash，全部不变。
83观察单元首败、84固定bundle首败及本批88首败3log也保持原hash。

## 下一阶段（继续开发，不重做本批）

1. PairingClient / ExtensionTransport validator 和 negotiation 同步 v2，检查完整 binding / purpose；旧合同明确拒绝。
2. 观察 scope prepare → 一次 claim → 原 status watcher，把每 request signal 接 source86 ObservationOperation。
   finally 先 join 原 clock/query/script/capture/decoder 和 watcher，再一次 settle；结果不能先于 settle。
3. 单独持久 observation journal / recovery。动作 DOM terminal nonce 不能证明 captureVisibleTab 已结束；
   同 browser worker 死亡不伪造 physical completion，未知原 scope 继续保持占用，按原 browser exit 证据收束。
   cleanup 不依赖当前 Pair / feature，失败重试只收尾、不重新 observe / 输入；保持全局有界。
4. 接生产固定 bundle / 校验 / export，保留权限/key/seed/default。source82 gate 抓真实新 watcher / settle，
   不因不再调用旧 completeExtension 就把 owners 标 joined。
5. fresh source/artifact，原 Pause / Chat Stop 同一真实 PNG 两反例、自然成功截图、权限拒绝、首次正文与 preview；
   再完整 Core / 四strict / native / 宽CU / ACP / routing / Node / MCP / 对应前端门禁。
   失败保留原现场；不重启刷绿、放宽断言或缩减完整四surface/三平台/两Mac架构/安装/真人Grok/12h/UI UX范围。
6. 原 owners 终态后按 AGENTS 第9条精确清理旧代、复验 source/current/全部失败证据并分别记录逻辑量和空闲变化。

# ExistingTab：原文档上的 SW 重启清理检查点

日期：2026-09-20；`feat/computer-use-implementation`，HEAD `30757366`。
工作区 `H:\aicoding\grok-app-computer-use`。默认关闭，动作 capability 仍 NONE。
总状态 **partial — not releasable**，未 commit/push/PR。

后续文档身份修复、18/34/8/13 项真链和 120 项测试的当前结果见
[文档执行身份检查点](2026-09-20-computer-use-document-execution-checkpoint.md)。
本文的 17 项和指纹为该前置批次，不要求重新实施。

设计与明确限制见 [重启 ADR](2026-09-20-computer-use-action-restart-adr.md)。
本批只关闭“同一原文档、同一存活 App、实际 SW 重启”的恢复切片，
不能把它写成整个 R2/R4 或 App 崩溃恢复已完成。

## 实现

- CompletionJournal 在 claim 前将 cleanup-only 记录写入浏览器会话内存，
  收到写入成功才领取。最多 8 条；严格 version/schema/字节限制、完整 proof、
  数字 tabId 和 Chrome documentId。没有配对 Bearer、用户输入或动作参数。
- 存储写入未知时保留占用；物理完成后先保存 physicallySettled 阶段，再
  向 Host settle。Host 已确认但本地删除失败时，保持本地 busy，只重试删除，
  不重发已确认的 settle，更不重发 claim 或动作。
- 原始脚本、取消调用与 watcher 的活进程所有权规则保持。正常控制器等待
  completionScope.prepare，再领取；生产动作 transport 必须显式传入 journal。
  旧 unbound receipt 探针仍为明确的无 journal 历史 fixture。
- SW 启动先退休旧配对，再由 CompletionRecovery 消费初始化时加载的记录。
  之后本 SW 新建的动作由原控制器所有，不能被启动恢复者误取消。
- 对 prepared 记录，只向原 tabId/documentId 注入固定 cancelDocumentOperation，
  核对原 snapshotId/requestId，等待原 operation.finished 及本次真实脚本返回，
  验证 main-frame/documentId/settled 后才归还 Host 占用。缺文档/错误/超时不能
  当完成；已有 physicallySettled 的记录只清理回执，不再注入脚本。
- 恢复记录仍在时，同 tab 不可重新 Share。删除失败也保持这一阻挡；损坏
  的存储拒绝所有新 Share，不静默丢弃。清理不自动恢复配对或授权。
- 每条恢复记录各自持有原脚本 Promise；一个未返回的标签页不阻塞其他
  标签页重试清理。重试只用一个有界退避 timer，不替换尚未结束的原调用。

TRUSTED_CONTEXTS 限制 content scripts，不宣称可信 popup 与同源扩展代码之间
存在秘密隔离。session storage 是跨 SW 生命周期的浏览器会话内存，不是
完整浏览器退出或扩展更新后的持久化恢复。

## 首败与修复

证据根：`tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/`，
以下文件使用 `existing-restart-` 前缀。

- `persistence-red.log`：两个实际断言失败。原控制器在存储尚未确认或写失败
  时仍会 claim/act。增加 prepare 门禁后，原断言通过，取消期间写入也不会执行。
- `recovery-isolation-red.log`：一个未返回的原文档阻止另一 tab 的清理重试。
  原来的整批 Promise 合并完成才安排重试，改为每个 entry 单独保留/调度后通过。
- `live-red.log` 是新增场景重复配对导致的 setup 失败，没到重启断言；改为
  在已配对的当前 fixture 上继续分享，没有放宽配对限流。
- `live-occupancy-red.log`、`live-wake-diagnostic.log`、`live-native-occupancy-red.log`
  是早期 probe 对 facade/target 身份变化的假设失败，未证明产品占用失败。
  后续使用原生 ServiceWorker.stopWorker，并验证 stopped → running；不再
  把 Playwright facade 对象或 targetId 变化当作 worker 执行生命周期的证明。

## 实际重启验证

`cu_probe existing-tab-dispatch` 增至 13 项。`live-final.log` 已全部通过：
前 11 项为上一批生产 SW/v2 故障链；新增两项为实际 storage 写失败零 claim，
以及实际终止/唤醒 SW 后对原 Wait 的恢复。

重启用例的独立后置条件：

1. 原文档真实 Wait 已开始，Host 显示忙。
2. 通过 Chrome ServiceWorker.stopWorker 终止具体 worker，原生状态到 stopped。
3. 在唤醒前，Host 仍忙，session storage 仍有恰好一条 prepared 记录。
4. popup 正常消息唤醒新 SW；原生状态到 running。新 SW 为未配对，旧 Host
   配对被撤销，旧业务结果被拒绝。
5. 恢复后 Host 空闲，清理记录被删除；独立文档脚本看到 snapshot 已退休且
   operation 为空。用户 fixture tab 保留，点击计数未增加。

存储失败用例包装的是实际 SW 的 chrome.storage.session.set，仅让一次
cleanup 写失败；assert 真实 claim 计数不增加，页面点击为零，结果 rejected。
私有管道仍只提供 fixture 授权/入队，不输送 proof。这个证据不是 MCP action
或 Tauri UI、安装版、其他 OS、真实模型验收。

## 验证

| 项目 | 结果 |
| --- | --- |
| 扩展/MCP Node 全量 | `js-final.log`：112 passed，0 skipped |
| 严格 App Clippy | `app-clippy.log`：lib + computer-use-probe、-D warnings 通过 |
| probe 重建 | `build-final.log` 通过；既有 Windows linker stdout 提示保留，无 lint 豁免 |
| 格式、locale、语法 | cargo fmt --check、15 locale check、probe syntax 通过 |
| 质量门禁 | `quality.log`：final PASS，千行文件 80/80 |
| Core/Driver | 本批未改 Core/Driver 源码、未重跑；上一批 Core 445，Driver 历史 12，不记成本批新结果 |

恢复隔离调度修正后，`live-isolation-fixed.log` 的 13 项、
`observation-regression.log` 的 34 项均通过。后续只扩展探针，没有再改生产
扩展代码；当前最终动作探针为下面的 17 项。

## 补齐五个实际 SW 重启时点

`existing-restart-matrix-final.log`：**17 passed**。原前 12 项保持，随后按序
验证以下五个时点；每次都实际执行 Chrome `ServiceWorker.stopWorker` 并核对
原生 stopped → running、新 worker 堆中没有旧故障对象、旧配对已失效。

| 时点 | 终止前的独立证据 | 恢复后 |
| --- | --- | --- |
| 真实 Wait 中 | 文档 operation 存活；领取 HTTP 已接受 | 等待取消、原 snapshot 退休、零点击 |
| claim 前 | session 记录写入，写入回复被持有；领取计数零 | 原排队业务撤销，清理完成、零点击 |
| claim 回复滞留 | Host 已成功接受领取，SW 尚未收到回复 | 业务拒绝、清理完成、零点击 |
| action 注入前 | 领取已接受，executeScript 包装器在调用 native 前阻挡 | 原快照退休、零点击 |
| click 完成但脚本回复滞留 | 页面已恰好增加一次点击，业务结果未发出 | 保持一次点击，不重放、不伪报业务成功 |

每次终止后、唤醒前，Host 仍显示占用，session storage 仍有恰好一个 prepared
记录。恢复后分别等待 Host 空闲和 session 删除确认，检查原文档 retired/无
operation、标签页保留。累计点击数跨新配对持续核对，不能每次重置基线掩盖重放。
此处“注入前”是实际 SW 尚未调用 Chrome scripting API 的切点，**不包含 Chrome
已经接收但迟迟未执行的旧注入**；后者和旧 observe 迟到仍在下一项。

新增探针支持文件 `existing-dispatch-restarts.mjs` 和 `extension-native-worker.mjs`
只做隔离 fixture 的故障注入/观察。Chrome 可保留 target 身份而替换执行环境，
因此重启后通过新的 native attachment 连接运行中的 worker；不是另起控制器，
也没有修改产品能力或生产 transport。

保留的失败日志：

- `matrix-first.log`：Playwright 持有失效执行环境，属于探针连接失败。
- `matrix-native.log`：Host 空闲后立即断言 session 已删除，暴露探针对两个
  异步确认的错误顺序假设；改为分别有界等待。没有把 Host 空闲当作存储删除。
- `matrix-storage-join.log`：中间四重启时点、16 项通过；最终增加 claim 前
  时点和成功领取计数断言，以 `matrix-final.log` 的 17 项为当前证据。

`matrix-clippy.log` 严格 App Clippy 通过，`matrix-final-build.log` probe 重建
通过；cargo fmt、三个探针模块语法检查通过，`matrix-quality.log` final PASS、
千行文件 80/80。生产扩展/MCP 源码未在这段更改，112 项和 34 项不重复记为新跑。

## 当前局部指纹

源码 **317 文件** SHA256：
`926A9F3A50CA57CE3984BB99C45434103CCA88E756D9E01D6DAB9196CDD5F0AA`。
cu_probe SHA256：
`0FBDF9CBC322A69DE6C716CD03189C2061B86A88FEF29C9688F2D09480F3F6DC`。
范围/算法沿用生产动作通道检查点：rg 遵循 ignore；Core/App computer_use、
browser/extension/MCP/probe、Computer Use components/lib/API 及两份 API 测试；
路径转 `/`、排序去重后拼接“路径 大写 SHA256”，LF 无末尾换行，UTF-8 再 hash。
文档和二进制不混入源码 hash；这是局部版本标识，不是发布冻结。

收尾：git diff --check 通过，并额外扫描 18 个本批源码/文档的行尾空白，覆盖
未跟踪文件；最终 JS 日志未发现未处理的异步错误诊断。按明确 executable/
owned profile 标识检查，cu_probe、私有 Node、隔离 Chromium 残留进程为 0。
HEAD 未变、index 为空；没有提交/推送/PR，也没有触碰日常浏览器或代理。

## 下一项

沿 [重启与 MCP 计划](2026-09-20-computer-use-restart-and-mcp-execution.md) 继续。
不要重新开发本批 journal/原文档恢复，也不要把只有一行实际重启通过扩大成全矩阵。

1. 五个同原文档 SW 切点已通过，不再重复从头开发/测试；先处理下面的文档身份。
2. 补旧 observe 迟到、Chrome 已接收的旧注入、新旧 document 退休身份、reload/navigation/close、扩展升级
   世界切换。缺原文档仍保持 unavailable，需要可验证的后续恢复路径。
3. 实现完成 tombstone 64 条/5 分钟淘汰后的 cleanup-only 恢复，不能无限重试。
4. 实际 App 终止/新 endpoint、新实例，以及 App/SW 两种重启顺序；不能用
   空 registry 或旧 403/404 证明物理完成。
5. 关闭重启门禁后才接 adapter.act/真实 session MCP，再做 parity、App/ACP、
   UI、安装、多平台、真实模型及同一最终候选至少 12h 主动长稳。

完整浏览器退出会清 session storage，而旧 Host claimed 不自动消失。这仍是
明确未关闭的缺口。历史 Windows IPC 10053 根因也未在本批解决。

# ExistingTab：批准前自然租约竞争

状态 active / partial — not releasable；单 Codex writer，不启动 Grok/子代理，不提交发布。
接[已启用 Host 的批准前连接替换](2026-10-04-computer-use-task-consent-u04-existing-preapproval-connection-checkpoint.md)。
原 dirty/untracked/fix.patch 和全部失败现场保留，完整交付范围不缩减。

## 实现与证据边界

新增 default-off 的 choose / owned reservation / original App publication 三项自然
租约竞争。沿首次真实 session MCP request、原 full key/picker、固定 A/B 候选与未选 C，
使用生产 source MV3、认证 HTTP、原 owned approval 与 App/ACP publication pin。

Core 的 cfg(test/test-support) getter 原锁内只读原 session-key 的存在性、原单调
deadline 剩余时间与 raw Share 数，不调用 expire/renew/live_connection/session_key
或候选查询。新单元测试分别证明：只读不续租；剩余为零时原存储仍存在；显式维护才退休。
单元测试的受控过期边界不作为 native 自然到期证明。

原 owned MV3 worker 经 CDP pause 停止续租。暂停句柄立即交原失败收尾所有者；
暂停时不调用 worker.evaluate，不读凭据，也不修改时钟/heartbeat/token/registry。
先在批准门闩之外把真实 30 秒租约等到8–10秒剩余，再启动原 reservation/publication。
ready 同时证明原 worker 确实 entered、原 full key、连接仍 stored 且剩余正数。
choose 行固定的是提交旧候选之前的原存储，不冒称其已经进入 reservation worker。

进入后只轮询 raw witness；连接退休与 Share 清零来自原 production HTTP 每秒维护。
先记录退休，再释放/join 原批准 worker、检查实际拒绝与 rollback，最后恢复原 source。
页面 DOM oracle 可在 worker 暂停时读取；恢复后再查原聚合脚本/capture/input计数。
不能把恢复后的403/disconnect误认成自然到期原因。

原 reservation/publication 15秒门闩、18秒 RPC、20秒 Host控制、5秒 discovery、
10分钟 request 与真实30秒 lease 都没改变。错过进入窗口直接失败，不静默重试。
改动仅为 test/acceptance；生产 consent、lease、输入与恢复行为没有改动。
Host手势、App session/ACP peer仍 synthetic；不是真人toolbar、安装态主窗或真实Grok E2E。

## 七项独立检查

1. off且无request/grant/execution，模型正文和目标列表拒绝。
2. 首次实际MCP生成pending，固定原完整key；模型正文/截图/input为零。
3. 原task-bound Pair等待Host确认，错nonce拒绝。
4. 原picker实际Unshare/Share，固定A/B选择，C未选。
5. 原source停止续租，实际进入前仍live；原生产维护在release/resume前退休并清Share。
6. 原candidate/reservation/publication明确拒绝；原owners join、权限/续行为零，
   feature/store恢复初始false，publication原run必须真正Stopped。
7. 原任务正文/截图/list仍拒绝；三页面保留，原模型读/capture/input为零。

## 当前候选与实际验证

source45：3269文件；manifest SHA256
`dda22cd3be513cc052ceace5e9eaa6d566cfef7bd6750e3bb12261e44e0a4547`。
默认App build45 no-run exit0，Windows CI Common Controls manifest已嵌入。
`src-tauri/target/debug/deps/grok_app_lib-63e3432e557be5b6.exe` SHA256
`f7167bc5e14627b9bf6c46a9ec70b7f7bf6ea78ad4e219ab62295fcc73ca4a6d`。
source44及更早同路径App binary已覆盖，旧hash仅为历史记录。

focused `existing-preapproval-natural-lease-focused-1` 实际
**3 passed / 0 failed / exit0，135.19s**，每项7checks。

| 场景 | pause后剩余ms | 原进入前窗口ms | entered剩余ms | pause到物理退休ms |
| --- | ---: | ---: | ---: | ---: |
| choose | 29689 | 9958 | 9958 | 29740 |
| publication | 29705 | 9966 | 9893 | 29731 |
| reservation | 29724 | 9986 | 9912 | 29752 |

三项pause前disconnect requests=0；退休时stored=false/remaining=0/Share=0，
均先退休再恢复原source，未靠强制断开或假时钟。
Core default **1081** / test-support **1097**：实际全量all-targets exit0。
四种strict Clippy、Node177/MCP38、fmt45/quality45/diff45：实际exit0。
App shell+Workbench合计13573行未增长；前端本批未改，未借历史前端全量数字。

完整 `existing-preapproval-natural-lease-native-matrix-1` 实际
**24 passed / 1 failed / exit101，697.06s**。新三项各7checks仍通过；原choose-unshare
在 `source-mv3-start / startup-trusted-local-api` 失败，checks=0，尚未发出首次MCP request。
因此完整native矩阵不绿，不能将本批写成全部通过。

原失败App/store root：`grok-cu-managed-task-9428132c-6ed2-4f28-834c-cc6ce809778e`；
browser root：`grok-cu-pairing-owned-664c1858-8c41-44a1-a05a-4c166351ddac`。
完整父目录为 `C:/Users/Administrator/AppData/Local/Temp/`；browser脱敏证据在
`profile/joint-failure.json`。原native peer已失败退出并保留原profile，不清这个现场。
脱敏只读证据为Error/other、contextClosed=false、初始worker=0、worker event=1、
sourceWorkerMatched=true、elapsed=666ms；捕获失败后另次诊断显示trusted storage/
storage write/execute/capture可用。不能由后次可用反推失败原因，原错误text/stack未记录，
目前无法区分具体Playwright evaluate启动/上下文/协议异常，根因保持开放。
这个失败发生在未改的旧startup路径，不据此声称已证明新lease逻辑导致它，也不据
新三项通过声称它与候选无关。不加sleep/重试、不放宽API要求或忽略失败。

宽App `existing-preapproval-natural-lease-wide-computer-use-1` 实际
**293 passed / 0 failed / 26 manual ignored / exit0，124.03s**。
其中25项joint已另跑（24通过/1失败），原10分钟request deadline门禁本批未重跑。
ACP `existing-preapproval-natural-lease-wide-acp-client-1` **178 passed / exit0，2.69s**；
routing `existing-preapproval-natural-lease-wide-routing-tests-1` **38 passed / exit0，2.33s**。
集合重叠不相加；宽回归没有复现source42清理首败，不作为其根因修复。
Verify45已复核3269源不变，清理后再次通过。

## 本批构建清理

原build/test/browser owners已退出。新 `existing-preapproval-natural-lease-cache-1`
完成inspection、15项target/retained paths和全部preserved hashes回读，再Execute。
已清15组旧App/Core incremental generation，逻辑 **6914590762 bytes（6.44GiB）**；
清理区间H空闲从329982660608增至334944563200 bytes，实际观察增加
**4961902592 bytes（4.62GiB）**。这是清理区间的空闲变化，不是整批构建的磁盘净变化；
硬链接/其它磁盘写入会使逻辑删除量与空闲变化不同。

App/Core可用产物、当前缓存、CFT140、immutable seed、依赖、dirty/untracked/
fix.patch与3269源复验不变。source45新失败root/profile、source42/43及全部历史
失败现场和日志/报告/哈希保留；没有删除整个target/node_modules或其它工作区。
unknown working generation、跨variant缓存与未知runtime/失败文件继续deferred。
没有杀其它进程。缓存不在回收站，原Cargo构建可再生；已执行清单身份不能复用。
完整plan/before/cleanup证据位于本批evidence目录的同名JSON。

## 原失败与下一项

source45上述新startup首败、source42宽CU两个原目录删除失败、source43 off调用enabled-only API的首败，
原B/初始化/seed rename/旧IO首败全部继续保留。新回归未复现不作为根因修复。
尤其事后无owner/DELETE错误不替代失败瞬间的原锁/IO证据，不删除原精确失败根。

下一项按[原App/ACP owner替换小批](2026-10-04-computer-use-task-consent-u04-existing-owner-next.md)，
同时补startup精确且脱敏的错误族/原worker-context退休见证，保留新首败并做有限定位；
未复现不能销项，不无限复跑堵住其它ready工作。继续Stop/取消/Drop与恢复竞争/迟到输出；
不重复开发本批自然lease或上批实际重新Pair。
先完成安装/原任务UX门槛再开放ExistingTab descriptor，raw parser仍Desktop-only。
完整E5、U04/U05/U06/U07–U09、D00–D31、四surfaces、Windows/macOS/Linux、
双Mac架构、全部输入/恢复、签名安装升级回滚、真人Grok与最终同候选12h仍required。

每批按AGENTS第9条：原owners退出后新plan、回读所有targets/retained paths/preserved
hashes再Execute；保留最新产物/依赖/seed/源码/失败证据，已执行清单不可复用。

# ExistingTab：已启用 Host 的批准前连接替换

状态 active / partial — not releasable；单 Codex writer，不启动 Grok/子代理，不提交发布。
接[浏览器/配置替换与宽回归首败](2026-10-04-computer-use-task-consent-u04-existing-browser-replacement-checkpoint.md)。
原 dirty/untracked/fix.patch 和全部失败现场保留，完整交付范围不缩减。

## 实现和前提

新增 choose / owned reservation / original App publication 三个实际 source MV3 重新 Pair
场景。每项仍经首次 session MCP request、原 App picker、完整原 request key、所选 A/B、
未选 C、实际认证 HTTP 和新 TaskGrant 的生产事务入口；没有借 legacy grant。

这三项的初始 Host 功能已经启用。fixture 在首次模型请求前使用真实
`persist_feature_transition_with_broker` 与隔离 `FeaturePreferences`，做 synthetic 显式
Host 设置；不在旧候选或旧 reservation 存续期间切换 feature epoch 以救活权限。
原 reservation probe 保存进入前的 feature 状态；失败 rollback 必须保留用户已有的
true 设置。其它原19场景的默认关闭路径与断言保持原语义。

source43 首版错误地在 off 状态调用 `broker.begin_pairing`。choose/reservation 两项在
新 challenge 前退出，publication 一项通过；实际 **1 passed / 2 failed / 91.21s / exit101**。
源码中该 Host API 明确要求 enabled；off 模式元数据准备只接受原 task-bound challenge，
不能调用普通全局重新 Pair 来构造合法替换。source44 修正测试前提，未放宽生产门槛。
默认关闭期间任意重新 Pair 不属于这三个已经启用的场景，不能将其标为已验。

首败日志 `existing-preapproval-connection-replacement-1.txt` 保留；两组原根：

| 场景 | App/store root | Browser root |
| --- | --- | --- |
| choose | grok-cu-managed-task-d49215fb-1daf-4ef3-ade4-abffb0009c31 | grok-cu-pairing-owned-ee82eb03-2aec-4467-bd57-951349b87661 |
| reservation | grok-cu-managed-task-90724a62-7d77-4109-972d-2d40b4989bb3 | grok-cu-pairing-owned-86c7e240-6c96-4c14-92e4-09072f1f1e6b |

完整父目录为 `C:/Users/Administrator/AppData/Local/Temp/`，browser 脱敏证据在其
`profile/joint-failure.json`。Host 手势、App session/ACP peer 仍 synthetic；程序扩展和
owned Chromium 不是真人 toolbar、安装态主窗或真实 Grok E2E。

## 七项独立检查

1. 已启用但无 request/grant/execution，模型正文和目标列表仍拒绝。
2. 首次实际 MCP request 生成 pending，固定原 full key，零模型正文/截图/input。
3. 原 task-bound Pair 必须等待 Host confirm，错 nonce 拒绝。
4. 原 picker 实际 Unshare/Share，取得 A/B/C 候选，只固定 A/B。
5. 在各原切点从真实 source 重新 Pair 成功一次，原连接退休；同 document 新分享 A/B/C。
   新分享前后独立检查原 full key、零 grant/continuation 和模型正文/截图/input；不导出身份或凭据。
6. 释放原 owner 并 join 后，旧 candidate/reservation/commit 明确拒绝，零迟到权限/续行，
   feature/store 保持初始 true；publication 的原 run 必须真正 Stopped。
7. 原任务读/截图/list 仍拒绝；新连接的三 Share 和用户页面保留，原 owned workers 为0。

原 reservation/publication 15秒门闩、18秒 private RPC 和20秒 Host控制期限没有增长。
普通 browser Pair/Share 不等于新任务授权；模型没有 approve/resume/reconnect 工具。
所有源码改动仅为 test/acceptance，生产 consent、输入、租约与恢复实现未改。

## 当前候选与验证

source44：3267文件，manifest SHA256
`3a88a132c88104f908660eec5198381b9dfb2676b8ff456289b965f80d4ce944`。
默认 App build44 no-run exit0，Windows CI manifest 已嵌入。
`src-tauri/target/debug/deps/grok_app_lib-63e3432e557be5b6.exe` SHA256
`e9e99c0d257b6830d9a00dbae0062b9b5eae384ebe7ae9087fe5aa5601c6ca61`。
旧同路径 source42/43 App binary 已覆盖，旧 hashes 仅为历史记录。

source44 focused `existing-preapproval-enabled-connection-replacement-1` 实际
**3 passed / 0 failed / exit0，67.99s**，每项7checks。
完整 `existing-preapproval-connection-native-matrix-1` 实际
**22 passed / 0 failed / exit0，555.76s**，原19场景与新三项都来自同一候选。
App default/probe 两 strict、Node177/MCP38、ACP178/routing38、fmt44/quality44/diff44
均实际 exit0；Verify44 已复核3267源文件不变。集合重叠不相加。
宽 App `existing-preapproval-connection-wide-computer-use-1` 实际
**293 passed / 0 failed / 23 manual ignored / exit0，135.84s**。其中22项 joint已显式
IncludeIgnored 单独运行，另一个10分钟 request deadline manual gate本批未重跑。
这是当前候选的结果；没有复现 source42 的两个目录删除失败，不能当作其根因已修。
Core/前端未改，本批未全量重跑，不能借旧数字。

## 构建清理

所有原 build/test/browser owners 已退出。新 `existing-preapproval-connection-cache-1`
按 source44/build44 完成 inspection、全部 targets/retained paths/preserved hashes 回读
及 Execute，清3组旧 App incremental generation，逻辑3806304718 bytes（3.55GiB），
H空闲实际观察增加3583635456 bytes（3.34GiB）。最新 cache/App/Core/CFT140/seed/
fix.patch 与3267源文件复验不变，所有 source42/43及历史失败根、日志/报告/哈希保留。
已执行身份不可复用；缓存不在回收站，原 Cargo 构建可再生。不确定的 working generation、
跨variant缓存与失败 runtime 保留，没有删除整个target/node_modules或其它工作区。

加上此前 source42 收尾的4组旧 cache，此次连续工作清7组，逻辑8025357298 bytes
（7.47GiB），两次 H空闲观察增量合计6502199296 bytes（6.06GiB）。这不是开始到结束
整个磁盘的净变化：本批新构建也占空间；C盘其它变化不归因为此清理。
两份清单/报告分别为 `existing-browser-replacement-cache-1-*` 和
`existing-preapproval-connection-cache-1-*`，原 owners/路径检查及逐项hash绑定均保留。

## 下一步

批准前真实 lease 竞争仍待验证；不得扩原15秒门闩硬等30秒，或用伪造时钟/强制断开冒充到期。
继续 App process/agent/ACP replacement、Stop/取消/Drop、恢复并发及迟到输出。
source42 宽 CU 的两处目录删除失败仍开放；事后无 owner 不替代失败瞬间的锁/IO证据。
原 B/初始化/seed rename 等首败不因复测成功销项。

先完成安装/原任务 UX 门槛再开放 ExistingTab descriptor，raw parser 仍 Desktop-only。
完整 E5、U04/U05/U06/U07–U09、D00–D31、四 surfaces、Windows/macOS/Linux、双 Mac
架构、全部输入/恢复、签名安装升级回滚、真人 Grok 和最终同候选12h仍为 required。
每批按 AGENTS 第9条清精确废弃产物；保留最新程序/依赖/seed/源码/失败证据，
原 owners 退出后生成新 plan、全部回读再 Execute；已执行身份不复用。

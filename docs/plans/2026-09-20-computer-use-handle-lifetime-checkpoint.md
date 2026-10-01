# Computer Use：受管浏览器元素句柄生命周期

日期：2026-09-20；工作区：`H:\aicoding\grok-app-computer-use`。
分支：`feat/computer-use-implementation`，HEAD：`30757366`。
承接 [真实受管 Wait 检查点](2026-09-20-computer-use-managed-wait-checkpoint.md)。
总状态：**partial — not releasable**；没有降低最终平台、安装、模型或长稳要求。

## 本批解决的问题

原来 `invalidateObservationState` 只删除 WeakMap 中的映射，没有调用已保留的
Playwright ElementHandle.dispose；持续 re-observe 会留下旧句柄直到页面退出。
不能直接在 invalidate 时全部 dispose：click/drag/upload/download 都会先退休
观察身份，再通过准备好的原始句柄执行操作，直接释放会破坏真实动作。

实现将身份寿命与物理资源寿命分开：

- 退休立即拒绝所有旧 snapshot/ref 解析，不等待异步清理。
- 活动操作通过 `withObservationLease` 暂时持有该观察的私有 handles。准备、
  执行、返回都在同一 try/finally；最后一个使用者结束后才 dispose。
- 同一个观察内重复 handle 只释放一次。success/failure/cancel/replacement/
  navigation/close 都接入；dispose 抛错时仍尝试其他句柄，不产生 unhandled rejection。
- 模型观察将非 truncated handles 的所有权交给观察状态。preview、未发布结果、
  截图期间文档变化、遗漏/截断候选的临时句柄均有明确清理者，避免双重释放。
- 下一份模型结果等待上一份的清理完成；同一页采集未结束时返回 observation_busy，
  防止慢清理积压无限多份退役观察。等待清理后再次验证文档，不能返回迟到结果。
- typed act/upload/download 持有 lease。准备过程中观察被替换时，mutation 的
  expectedObservation 检查拒绝派发，不误伤替换后的新观察。

涉及生产模块：`observation-state.mjs`、`observation-extract.mjs`、`page-state.mjs`、
`server.mjs`。不新增模型 eval/CDP/debug 路由，不改权限或默认关闭策略。

## 首败保留与回归修复

证据目录：`tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/`。

1. `handle-lifetime-red.log`：旧生产实现实际进入断言，`retired model handle
   leaked`，期望 true、实际 false。此定向首败只有 1 个目标测试，12 项是名称
   过滤，不是将它们记为通过。
2. `handle-lifetime-browser-01.log`：真实回收计数与操作通过，但探针错误地只接受
   /disposed/ 错误文案。查明固定 Playwright 1.48 对已销毁 channel 返回
   TargetClosedError。探针增加当前 page/handle 仍可用、旧 channel 已从真实
   connection registry 移除的独立检查，再接受该正确错误类型，避免浏览器崩溃
   被当成回收成功。保留首败；未改生产依赖或降低资源断言。
3. `handle-lifetime-browser-full-01.log`：108 passed / 1 failed。新增 lease 的
   身份检查先于参数验证，把缺 snapshot 的请求从 400 变成 409。生产入口恢复
   preflight-first；原 400/zero-dispatch 断言不变，定向 HTTP 回归已恢复。

## 已执行证据

| 门禁 | 结果 | 日志 |
| --- | --- | --- |
| 生命周期/采集/页面状态 unit | 34 passed，0 skipped | `handle-lifetime-unit-02.log` |
| 真实 Chromium 资源与动作 | 5 passed，0 skipped | `handle-lifetime-browser-02.log` |
| 实际 typed HTTP 动作 | 11 passed，0 skipped | `handle-lifetime-http-02.log` |
| 完整 browser Node | 110 passed，0 skipped，exit 0，98.68s | `handle-lifetime-browser-full-02.log` |
| 完整 Core + Driver | 376 + 12 passed，0 ignored，exit 0 | `handle-lifetime-core.log` |
| Core 严格 Clippy | all-targets/all-features，`-D warnings`，exit 0 | `handle-lifetime-clippy-core.log` |
| App 严格 Clippy | all-targets + computer-use-probe，`-D warnings`，exit 0 | `handle-lifetime-clippy-app-probe.log` |
| 重建 cu_probe | exit 0，23.43s；保留既有链接器 stdout 警告 | `handle-lifetime-probe-build.log` |
| source managed 真链 | exit 0；Wait、preview、原 ref 点击，独立 oracle=2 | `handle-lifetime-source-preview.log` |
| seed managed 真链 | exit 0；确实运行新生成 worker，相同后置条件 | `handle-lifetime-packaged-preview.log` |
| Code quality / fmt | final PASS（千行文件 80/80），fmt exit 0 | `handle-lifetime-quality.log` / 工具输出 |
| 实际 ExistingTab 回归 | 私有 Node/MCP + App Broker/IPC + MV3，34 passed，exit 0 | `handle-lifetime-existing.log` |
| seed 完整 check（实机探针之后） | exit 0，manifest/tree 不变 | `handle-lifetime-seed-check.log` |

真实资源探针通过生产 collector 和 typed action 方法操作隔离 Chromium，
只在测试侧记录实际 ElementHandle.dispose 调用，不在 worker 加诊断逃生口。
25 次模型替换 + 25 次 preview，每轮结束只留当前模型两个句柄；旧 channel 不再
存在，当前 page/handle 仍能读取。门控 click 期间旧 ref 立刻无效，物理句柄仍
存活；放行后独立 DOM 计数器由 0 到 1，所有退役句柄已释放。失败、导航和关闭
后也为零。此项是隔离浏览器证据，不代表 12 小时 soak 或所有 App 路径。

另加真实 `/upload` 成功路径：只从本 run staging 上传自建 fixture 文件，
独立页面 oracle 验证文件名、9 字节及恰好一次 change。重复 actionId 返回原
结果不再触发上传，旧 snapshot 新 actionId 拒绝；没有只测上传拒绝路径。

ExistingTab 34 项仍是配对/租约/分享/观察/Stop/截图权限拒绝，不是已支持 typed
actions 或真实 toolbar 截图 happy path；它只用于检查本批未破坏已有链路。

标准 runtime prepare 已执行 exit 0：`handle-lifetime-seed-prepare.log`。
manifest：`b5f6dc83d3dc70260c01797b949ce92c2d7578ea773ca01bc68ebba90b37b485`。
tree：`08998ee277b58b6231720832d9524b74175d90fa0e7774ab7b3ddb4df72f81ec`。

cu_probe SHA256：`B47A1800426D933CDEA4AAE2E4E9D8A738AA318C66B8769C3B3734685B5DFC0D`。
246 文件局部指纹：`8EBD8D1BECAB85FCB40B864D04628BCB9CF23C713014ED91BE19C90AC3FBE80D`；
日志 `handle-lifetime-source-fingerprint.log`。范围与算法同 Wait 检查点的 244
文件指纹，本批新增两个生命周期测试文件。**不是全仓/C6 发布候选冻结**。

本批未改 Rust/前端产品源码；没有重跑前端 59 项、全仓 pnpm test/build:ui、
安装版或跨平台测试，不继承前一批结果作本批最终验收。

最终 HEAD 未变、index 空；read-only 查询未发现 cu_probe、私有 Node 或本轮
managed-contract/pairing-owned/handle-lifetime/typed-act profile Chrome 进程残留。
没有提交、推送或提 PR，diff whitespace 检查 exit 0；保留既有 LF/CRLF 提示，
未改 Git 全局设置。

## 下一项

1. managed HTTP 的取消仍待实现：`worker_http.rs::bounded_loopback_post` 使用
   off-Tokio 的 blocking reqwest，普通请求最多仍等现有 15 秒，open 为 60 秒。
   `ManagedWorkerAction`/`ManagedTabAction` 尚未携带取消句柄；CaptureOptions
   虽有 cancellation，目前主要检查在请求前后，不能中断进行中的 socket 等待。
2. 改这条链前读 blocking HTTP ADR。必须保持 loopback-only/no-proxy/no-redirect、
   body 上限、错误脱敏、unknown 不重放、StopRequested ≠ 物理已停止。
   用有门控的本地 HTTP 服务覆盖请求发出前、等待 headers、半包 body、取消与
   正常返回竞争；不能用“提早返回但线程仍在跑”伪造 quiescence。
3. 完整授权竞争、profile/process 生命周期、ExistingTab typed actions，
   toolbar activeTab/截图成功、C4/C5/C6 全部继续；不把本批 managed 证据
   充作 ExistingTab、Edge、安装、macOS/Linux/Wayland 或真实模型验收。

仍不提交、推送或提 PR，不碰真实账号、用户日常 profile、代理/VPN。

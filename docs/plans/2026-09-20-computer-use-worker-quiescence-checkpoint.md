# Computer Use：worker 暂停与物理完成检查点

日期：2026-09-20。工作区：`H:\aicoding\grok-app-computer-use`。
分支：`feat/computer-use-implementation`；HEAD：`30757366`。
总状态仍为 **partial — not releasable**，不是最终版验收。

## 本批范围

在上一批可取消 HTTP 基础层之上，新增真实 Playwright worker 的 run 生命周期。
**Rust Host/ManagedBrowserAdapter 尚未调用新暂停/恢复接口，也尚未贯穿生产
请求的取消 token/revision。** 因此本批真实 HTTP fixture 成功不能写成 App
暂停按钮或完整 MCP 生命周期已完成。

新增 `tools/computer-use-browser/run-operations.mjs`，由 `server.mjs` 使用；
`runtime_prepare.rs::WORKER_FILES` 已将它纳入正式 seed 生成清单。
没有新模型工具、任意 JS/eval、调试后门或全局权限。

## 版本 1 协议决定

- `/health` 宣告 `runLifecycle: 1`。新 Host 接线必须先检查能力，旧 worker
  不得默默降级为“暂停成功”。所有生命周期端点仍需原 loopback Bearer，
  并要求 `x-grok-cu-host: 1`。该 header 是用途标记，不能单独作为认证。
- owner 是同一 worker 中不可复用的 run 身份。新 run 的 `runRevision` 为 1。
  业务请求按 owner + revision 准入；已有 actionId 账本仍独立去重，不因恢复清空。
- `/pause-run {owner, runRevision}` 同步将 run 置 paused、撤销本 revision 的
  AbortSignal。退休当前观察，等待其 action-aware handle 回收，并把这段回收
  计入 activeOperations。它不关闭已经打开的 profile。
- `/run-status {owner, runRevision}` 返回 `phase`、`runRevision`、
  `activeOperations`、`idle`。活动业务请求和 Host profile 清理直到 finally
  才归还计数；HTTP 客户端断开不归还正在执行的操作。
- `/resume-run {owner, runRevision, nextRunRevision}` 是 compare-and-swap：
  必须 paused 且 activeOperations=0，next 必须恰好 +1，并建立新的 controller。
  旧请求、旧 pause、旧重复 resume 均不能改变新 revision。
- 兼容现有尚未迁移的 Host：完全未受版本控制的初始 run 可省略 revision。
  一旦显式传 revision 或发生 pause，该 owner 永久要求显式当前 revision；
  恢复后省略字段不能绕过 fence。此兼容行为不授予模型恢复权限。
- `/cancel-run` 是 owner 终态，不可 resume。仍须实际 context.close 成功，
  已准入操作的 finally 收束后才返回成功；一秒内未收束返回
  `run_cleanup_pending/unknown`，可由 Host 清理重试，绝不当作 Stopped。
  run-status 的 idle 不是 context 已关闭的证据，Stop 还必须拿到 cancel-run
  的资源清理确认。
- registry 最多 256 个 owner、64 个活动操作。终态记录保留到 worker 退出，
  不淘汰墓碑后允许旧 owner 复活。超限明确拒绝。错误不回显请求/路径/凭据。

本协议不声称所有 Playwright 原生动作能即时被中断。已进入原生 API 的操作，
若无法撤回，必须一直占用计数直到实际返回；这期间 resume 被拒绝。授权撤销、
取消请求、本地 socket 结束、物理操作完成、context 关闭分别验证。

## 其他实现变化

- open/tabs/new-tab/frames/goto/upload/observe/act/download/close 均纳入 run 计数。
  typed action 的既有取消 controller 与 run signal 组合，准备后/返回前复验。
- 观察接受 signal，异步读取、截图、发布及旧句柄清理后复验；取消时未发布
  handles 回收，回收中已经挂上的新观察也会退休，不能留下取消后的 refs。
- profile 启动有同步 reservation；同一 profile 不能重入启动，clear 不能删
  正在启动的目录。拿到 context 后先登记归属再 await，失败/取消会关闭自己
  的 context；关闭失败保留 slot 供后续 Stop 重试。
- 删除 registry slot 比较对象身份，避免旧 close 删除同名后继 slot；clear
  删除目录前再检查新 reservation/slot。
- shutdown 先封锁后续 run 准入，再关闭 contexts 并等待操作收束；确认失败
  返回错误，不提前返回 shutdown 成功。仍需 Host 侧关闭/进程树矩阵验收。

## 首败与证据

日志根：`tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/`。

`run-quiescence-first-red.log`：真实 Chromium 已开始 Wait，重复 actionId 得到
action_in_flight 后请求 `/pause-run`，断言失败：404 not_found，不是编译失败。
定向 test-name 运行中其他 5 项被过滤，未计入通过数。

`run-quiescence-focused-02.log`：33 passed，0 skipped。包含真实 Chromium
observe → Wait → pause → idle → resume → observe → click，旧/缺失 revision
和旧 pause 拒绝；同一 pageId 仍存在，独立页面 oracle 由无输入到恰好一次点击。
普通的有 Bearer 但无 Host 标记生命周期请求，在最终完整 browser 回归和 seed
定向验收中实际验证为 403，不能以 header 标记替代 Bearer 认证。

单元门控覆盖多操作物理结束前不能 resume、旧 finish 不释放新 revision、
多 run 隔离、先 pause 后迟到 open、终态/容量、清理占用与 shutdown。
观察门控覆盖截图尚未结束、旧 handle 仍在 dispose 时取消；未将这些 Fake
截图/回收用例写成真实浏览器慢截图或原生输入取消验收。

本批首次完整 Core：385 passed / 1 failed，日志 `run-quiescence-core.log`。
失败在 `ipc::tests_transport::real_http_observation_is_bounded_and_replay_or_forged_binding_cannot_complete`，
`tests_sharing.rs::post` 收到 Windows 10053 ConnectionAborted，未到该次 HTTP
状态断言。未修改测试或产品 IPC 源码、未吞异常/增加重试、未放宽 403/413/200
断言。随后定向复测及完整同配置重跑通过。**首败具体发生在该用例哪个 POST
以及根因仍未定位**；复测通过不等于已修复，留作后续门禁/长稳跟踪项。

| 门禁 | 实际结果 | 日志 |
| --- | --- | --- |
| 最终完整 browser Node | 117 passed，0 skipped，45.84s，exit 0 | `run-quiescence-browser-full-02.log` |
| Core 失败用例复核 | 1 passed，385 filtered，exit 0 | `run-quiescence-ipc-recheck.log` |
| 完整 Core + Driver 重跑 | 386 + 12 passed，0 ignored，exit 0 | `run-quiescence-core-02.log` |
| Core / App 严格 Clippy | 两者 exit 0，all-targets，`-D warnings` | `run-quiescence-clippy-core.log` / `run-quiescence-clippy-app.log` |
| seed prepare | 标准生成器 exit 0，新模块已纳入 | `run-quiescence-seed-prepare.log` |
| 实际 seed worker 新生命周期 | 1 passed / 5 filtered，独立 click oracle=1，原 tab 保留 | `run-quiescence-seed-lifecycle.log` |
| 重建 cu_probe | exit 0，28.52s；保留既有 linker stdout 警告 | `run-quiescence-probe-build.log` |
| source / seed managed 探针 | 两者 exit 0；Wait、preview、旧 ref 点击 oracle=2 | `run-quiescence-source-preview.log` / `run-quiescence-packaged-preview.log` |
| 实机探针后的 seed check | exit 0，manifest/tree 未变 | `run-quiescence-seed-check.log` |
| code quality / fmt | final PASS（千行文件 80/80）；fmt exit 0 | `run-quiescence-quality.log` / 工具输出 |

源 worker 的完整测试使用私有 Node 20 + 显式 CfT；seed 新协议定向测试使用同一
私有 Node + 实际 seed Chromium/worker.mjs。后者是通过测试 harness 选定 worker
文件，不是在生产增加调试开关。Rust cu_probe 的两项真实回归仍用于验证既有
受管调用，不能代替 App 已接入新 pause/resume 的证据。

seed manifest：`3dc13d96c63b40066b496028a4bc81e579081eb06bc9d4d54743a1ac2b8b4823`；
seed tree：`c889eb28f7f793452c8d34221aeecc31f6e50cc9630620c759b5a0232bfd21a2`。
cu_probe SHA256：`861907C167EDE8C5532C5C06CA632F9AAD3CA971F5702A0A0CD889C90178E8FD`。
250 文件局部指纹：`41661A84BA7366FF6C9D4EB744B0546E83CCC359A12168CE84EDBB3AD51DD977`，
日志 `run-quiescence-source-fingerprint.log`，范围与算法同 HTTP 基础层检查点。
它不包含全仓依赖/入口，**不是发布候选冻结**。

本批未重跑前端、ExistingTab 34 项、完整模型或安装/跨平台测试。HEAD 未变、
index 空，diff whitespace exit 0（既有 LF/CRLF 提示保留）。read-only 查询未发现
cu_probe、owned Node、typed-act/managed-contract/handle-lifetime/pairing-owned
Chrome 残留。未 commit/push/PR；不碰真实账号、日常 profile、代理/VPN。

## 下一步

1. 补 Host/worker trait 的生命周期状态和版本能力检查，明确 backend revision
   与 Broker target generation 的关系；不能随处 lookup 当前 revision 代替
   同一次准入捕获的身份。丢失 resume 响应按未知处理，不盲目重发/派发动作。
2. 将 ManagedBrowserAdapter 的 abort/is_idle 从常量实现接到真实 worker
   结果。暂停 cleanup 失败必须 pending；resume/authorize 必须在相同 run
   admission 下确认 idle、切换版本和取消 token。旧清理不能影响新版本。
3. 所有业务请求传准入捕获的 revision/cancellation，包括 capture、typed
   Wait/act、navigate、download、upload、new-tab/popup/open。取消本地 HTTP
   不能释放远端 pending；清理用独立的未取消 transport。
4. Host `ExistingTabHost::cancel_run` 目前按已注册 profiles 判断是否发请求，
   opening_profiles 尚未注册时不能漏清理。补真实启动中 Stop、失败回收与
   迟到响应的 Host/worker 联合证据。单个 worker reservation 不是该项完成。
5. 当前真实测试是 Wait 取消，仍需真实 native click/下载/截图执行中的门控
   竞争，以及实际 App Broker/MCP 的暂停/恢复/Stop 两 session 隔离。
   接通可取消 HTTP 前还要覆盖客户端在 request body 未读完时断连：server 的
   body for-await 当前位于路由 try/catch 外，需用真实 socket 验证不会造成
   unhandled rejection/worker 退出；本批未把这个分支当作已验。
6. 接线后生成 seed，source/seed 两条生产链通过，再继续 ExistingTab typed
   actions 与 C4/C5/C6：toolbar 截图、App/ACP、安装/Edge/macOS/Linux/Wayland、
   真实模型与同一候选至少 12 小时主动长稳。继续保持 partial，不提交推送。

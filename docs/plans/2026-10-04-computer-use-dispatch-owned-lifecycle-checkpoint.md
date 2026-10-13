# U06 增量：dispatcher 退出所有权与 managed 生命周期

状态：**dispatcher ownership verified / full Computer Use active / partial — not releasable**。
工作区 `H:/aicoding/grok-app-computer-use`，分支 `feat/computer-use-implementation`，
HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`。单 Codex writer，原 dirty/untracked/
`fix.patch` 保留；未启动 Grok/子代理，未 commit/push/PR/发布。

## 实际生产反例与修复

`session_manager/computer_use/dispatch.rs` 原 join 在 await 前从 registry 取走原句柄：

1. 等待方取消会丢失原 task handle，下一次 shutdown 在原 task 仍阻塞时误报成功。
2. 原 task panic 导致的 join failure 被丢弃，下一次 shutdown 错误改写为成功。

两个反例在默认 App harness 实际进入断言，`dispatch-red-1.txt` 为 **0 passed /2 failed，exit101**；
不是编译失败或 `0xc0000139`。修复让 registry 持有原 `Arc<async Mutex<Option<JoinHandle>>>`，
waiter 只借用，实际 join 后才退休；`AtomicBool` 保留原 join failure。
test-only Notify 精确定位取消边界，不改生产授权、调度、预算、期限或默认开关。
同最终候选 `dispatch-green-1.txt` 为 **4 passed /0 failed**（两个新增回归及两个既有测试）。
synthetic panic 由原 join 捕获并保留错误，不是未处理后台失败。

## 本批 native 联合增量

沿上一批同生产 managed constructor、原 App/ACP publication pin、真实隔离 settings、
原 owned approval worker 和实际 source Node/Chromium，不另造批准通道。

- Feature-off：原 native launch 已创建实际 context、返回被 hold 时，调用真实 feature
  transition，先 fence 再写隔离 settings=false。旧批准不能覆盖明确 off；原 close 一次、
  最终 Stopped，原 approval/cleanup/ACP/Child/Job owners 收束。
- Queued continuation Stop：正常批准后调用真实 `SessionManager::reserve_computer_use_delivery`，
  持原 ACP stdin mutex，实际 prompt 已进入 pending 但未 admission；原 full-key Stop 后释放。
  真实 `prompt_computer_use` 返回 NotStarted，pending 清空、peer 零 prompt、零 authority
  admission，原 prompt task joined。这还不是自动 scheduler/App event settle 整链。
- 实际 request deadline：默认单调 TTL 为真实 10 分钟，手动 ignored 测试独立运行，
  不引入生产测试时钟或缩短产品期限；先等待且零 worker/profile/grant，临近到期才明确启动
  隔离 native context。原请求实际 Expired 时 context 仍 occupied/StopRequested；release 后
  零迟到 grant/continuation，原 close 一次、最终 Stopped，原 worker/consumer/ACP/Child/Job
  完成退出。独立测试 **1 passed /0 failed，601.93s，exit0**。

**Host gesture、App session、ACP peer 及 gates 仍为 synthetic；不是主窗真人、真实 Grok、
安装态或完整三平台 E2E。** 无真实系统/App/Share 自动批准，无用户浏览器、凭据或代理变更。

## 最终候选验证

证据 `tools/computer-use-probe/.run/codex-u06-dispatch-owner-20261004/`；所有首败日志保留。

- 默认 App build1 是红测试候选，build2 为修复及 native 新场景的最终候选，两者编译 exit0。
- Native 普通集合 **7 passed /0 failed /1 intentional ignored**，25.20s；包含上批五项及
  本批 feature-off/queued-stop 两项。真实十分钟 ignored 测试另行显式运行 **1 passed /0 failed**；
  同最终 harness，原 remainingMs=599983，没有缩短产品期限。未把该手动项说成常规 CI 已运行。
- 最终默认 App 宽 `computer_use` filter **267 passed /0 failed /1 ignored**，128.52s；
  ACP **167 passed**，routing **38 passed**，各实际 exit0。过滤集合重叠，不相加；
  `live_handshake` 未运行，不把全部 267 项说成新增。
- 默认 App 与 probe all-targets strict Clippy、fmt、fixture syntax、quality、diff exit0；
  Node context-close/bundle audit **28 passed /0 failed**。
- Core/前端未改，本批未重跑其全量；上批 Core1004/1020、前端703文件8031测试是历史结果。

冻结3084源，`source-files-1.sha256` SHA256
`851441d547a9a8f29a91312b2eb9cd8e64cae50d90dad74c6f21bc651d68dcdd`。
默认 App features=[] harness `grok_app_lib-dc611c8f8bc4f678.exe` SHA256
`f537d47b75a2b838a8355cf87cf653f57a758bf33f0f1c544e1738639eeb2623`；按现有 CI 后链
嵌入 Common Controls manifest，已进入断言。**测试 harness 不是开发包或发行包。**
immutable seed manifest仍为
`001cf9617ce64b6cb6448c38a848533577a6116f33138bbc242e8e03e873b5ec`。

## 构建卫生

原普通 runner 和十分钟原测试均实际 exit0，原测试 PID29856/创建时间已回读缺失；
清理脚本再次拒绝任何 live build/test/browser owner 或 reparse point。先 inspection-only exit0，
再只执行一次本批五组 obsolete/current exact pairs 清理，不重复旧批已完成的清理。

删除 **4,566,134,672 logical bytes（4.25GiB，含 hardlink）**；H 空闲实际观察增加
**4,478,005,248 bytes（4.17GiB）**。`cache-cleanup-plan.json`、`cache-cleanup-before.json`、
`cache-cleanup.json` 保留；最新缓存/App/Core/immutable seed hashes全部不变，
清理后3084源逐项复核不变，diff check通过。成功 native fixture 的临时根在原 owners
join后由测试精准删除；失败根、报告、日志、hash及全部 dirty/untracked源码保留。
删除缓存不进回收站，可通过 cargo 重建。当前程序、依赖和包均未删除，未动整个
target/node_modules、共享缓存、其它 worktree或无关进程。后续每批按 AGENTS 第9条执行。

## 下一步及未关闭范围

1. 继续 U04 ExistingTab/内嵌 WebView 的真实 Host instance proof 和原 reservation 的 task
   scope constructor；现有 ExistingTab 五种 typed actions 已有实现，不重做已完成内容。
   当前 NativeScopeIdentity 生产仅 WindowsRoot/ManagedPage；Existing adapter 尚无 task
   scope_identity/scope_candidates，不能直接开放新 TaskGrant。绑定原 pairing/connection/
   document/share/owner，借用归还不关闭用户 tab，不借 legacy grant 或 model UUID。
2. 原自然 `run_cleanup_pending` 长尾首败根因仍开放；本批 native 快关闭不等于已修复。
   尚需其它 owned 阶段、真正主 WebView/自动 dispatcher 联合验收和相关生命周期矩阵。
3. U04其它 surface/dialog/跨 surface/switch/增量与缩小、U05其它 surface恢复、U06完整矩阵、
   U07–U09、D00–D31、Windows/macOS/Linux、四 surface、真实 App/Grok、签名安装、
   同最终候选12h与完整UI UX均仍开放；不关闭或缩减完整持续目标。

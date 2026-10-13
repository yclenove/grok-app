# U04 增量：独立任务浏览器的共享批准 UI

状态：**managed task UI wired / full Computer Use partial — not releasable**。
工作区 `H:/aicoding/grok-app-computer-use`，分支 `feat/computer-use-implementation`，
HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`。单 Codex writer；原 dirty、
untracked 和 `fix.patch` 保留。未启动 Grok/子代理，未 commit/push/PR/发布。

## 已接通的产品路径

1. `computerTaskManagedApprove` 只向原主 WebView 命令发送 App session 与完整四字段
   Host key，不发送模型给定的 run/profile/tab/native proof；镜像/浏览器客户端拒绝。
2. 每 session 原共享 `ComputerTaskActions`：`managed-browser` 的一次明确 gesture
   调 managed constructor，不弹假窗口 picker；Desktop 仍走原真实 picker。
   card/panel 的重复 gesture 共用 pending；receipt 不生成 grant 或启动 preview。
3. 批准前显示实际固定 policy：独立任务环境、仅当前标签页、不分享日常浏览器。
   挂载、订阅、换 surface 不准备环境；准备状态沿原 Host/readback 与 controller 显示。
4. 原 Host 尚 pending 时，本地批准已开始就显示 Cancel；Stop 不等待准备或取消回复。
   late success/error、Host 已批准、request/owner replacement 由原 epoch 隔离。
   失败短提示与显式 retry，15 locale 同步；隐藏 UI 不撤销已明确批准的任务。
5. `present_view` 在编译支持的 Windows/macOS/Linux 广告 managed constructor descriptor。
   wire `pickerSurfaces` 名称兼容，语义是已编译的选择 producer，不是 runtime ready 或 grant。
   Core 仍空 descriptor；metadata/watch 不创建资源/续行。Desktop parser 与 managed picker
   begin 未放宽；ExistingTab/AppWebView 的任务批准入口未因此开放。
6. 沿既有组件、状态和样式，不增加 App/AppWorkbench 状态，无原生 select/第二套授权 store。

## 红测、修复及未关闭失败

证据根：`tools/computer-use-probe/.run/codex-u04-managed-ui-20261004/`。

- controller 首红 5 failed/18 passed，另有两条 fixture deferred 未消费的 rejection；
  保留 `task-actions-red-1.txt`。补 actual managed-call 断言/fixture catch 后，第二次真正
  首红为 **6 failed/17 passed**，无该 rejection 噪声；实现后 44 条相关 controller/API 通过。
- UI 首红 **19 failed/3 passed**，暴露缺范围文案/local Cancel/短错误和 locale；修复后
  affected suite **31 files /355 tests** 通过。一次 managed label 大小写测试错误已修正，
  `task-ui-green-1.txt` 原失败保留，不当产品失败。
- 默认 App `app-build-1` 是新增测试把 enum 与 string 比较造成的 E0308，未进入断言；
  修复测试类型后 `app-build-2` 真正复现 descriptor 缺失 **0 passed/1 failed**。
  修 descriptor 后 `app-build-3` 编译、descriptor/watch 和后述默认 App 回归通过。
- 首次全前端 **703 files：8029 passed/1 failed**，失败是配对焦点测试在 DOM 出现即检查
  passive effect。改为 `waitFor` 检查实际焦点（不延长产品期限、不改产品焦点逻辑），
  定向 8 条通过；首次失败日志保留。最终全量状态见下表。
- 首次广域默认 App CU **228 passed/1 failed**：source Chromium 的 transport-replaced
  fixture 收尾仍报告 `run_cleanup_pending`，`Fixture::finish` 的立即 unwrap 失败。
  同一 binary、不改 native 源码或超时，退出本批并行重构建后 native 单项及 CU229复测
  通过。**这不是首次清理长尾根因修复**；不以重试覆盖原失败。
  失败现场保留在
  `C:/Users/Administrator/AppData/Local/Temp/grok-cu-managed-task-8d6dc69f-c9c4-44dd-a8a1-84a8773defcd`，
  已读回没有该 root 的 worker/browser 或原 App test owner；未删除该现场、未停无关任务。
- 复核另补 late Host success：先实际 **1 failed/23 passed**，API 回包失败后，稍后原 Host
  已批准仍残留 managed error。仅在原 Host approved 时退休该请求的 managed UI 错误，
  不自造 grant、不重试；controller/managed UI **46** 条转绿。第三轮全门禁/Chromium
  验证这份最终源码，不把修复前8030/78移作最终结果。

## 验证范围

| 实际验证 | 结果 / 边界 |
| --- | --- |
| Core 默认 | 1004 =800 unit +204 integration |
| Core test-support | 1020 =800 unit +16 driver +204 integration |
| 默认 App features=[] | managed6、publication6、task access8、recovery4、native managed1、native scope2、ACP165、routing38；过滤集合重叠，不相加，排除 live_handshake |
| 默认 App CU 复测 | 229，另原生 publication 单项1；原首败未关闭根因 |
| 前端定向 | 31 files/355；配对焦点修复定向8 |
| 最终全前端 | Attempt3：703 files /8031 tests，全部通过 |
| 真隔离 Chromium | 修复前两轮各78；最终第三、四轮各79，含late Host success；synthetic IPC，不是 installed App/native consent |
| 质量 | Core/App/probe strict Clippy、fmt通过；最终Frontend Attempt3的production TS、ESLint、quality、UI build及diff均exit0；probe TS另行通过 |

Chromium evidence：
`tools/computer-use-probe/.run/task-consent-ui-2026-10-03T20-40-42-639Z/` 与
`tools/computer-use-probe/.run/task-consent-ui-2026-10-03T20-43-59-459Z/`。
最终79项两轮分别位于
`tools/computer-use-probe/.run/task-consent-ui-2026-10-03T20-59-08-742Z/` 与
`tools/computer-use-probe/.run/task-consent-ui-2026-10-03T21-02-13-680Z/`。
生产组件有真实浏览器渲染，但 Host/session/IPC 是 synthetic。默认 App/source Node/Chromium
有实际原生 worker/publication 证据，却不是一次实际主窗点击→真实 Grok 的整链验收。
没有把两套 fixture 拼成已完成的真人 E2E。

## 冻结与构建卫生

- 最终 `source-files-4.sha256`：3075 源，
  `4670286c4465c7e53246d806d5e5c033e6ec22e06a96f3505b8d965c1e7e95e6`。
  manifest1/2是修复前历史快照，保留不改写。manifest3与2只差 controller、其测试、
  UI acceptance helper三个前端文件；manifest4与3只差helper中两张错误截图的独立命名，
  避免fail-first与late-approved互相覆盖。最终Frontend Attempt3覆盖当前生产代码，
  Chromium第四轮覆盖最终helper；Rust/worker源及下述App executable未变化。
- 默认 App `grok_app_lib-dc611c8f8bc4f678.exe`（按现有 manifest 后链）：
  `b3d574c0f409fe48d1db12a61109a87646ca2f330507564c0df9ccefedc54bb1`。
- immutable seed manifest 保持
  `001cf9617ce64b6cb6448c38a848533577a6116f33138bbc242e8e03e873b5ec`。
- 前两轮 UI 的原 browser/server close 后分别删除各自 site/browser-runtime，
  每轮 372,647,470 logical bytes（355.38 MiB）；第三、四轮分别删除372,647,523 bytes。
  四份cleanup.json均记录ownersClosed=true，保留 PNG/report/log/hash/失败证据。
- 原 build/test/browser owners 退出后，13组精确旧 App/Core cache实际删除：
  **6,511,822,913 logical bytes（6.06 GiB，含hardlink）**；H空闲观察增加
  **4,266,897,408 bytes（3.97 GiB）**。最新代与App/Core/seed hash不变，3075源码立即
  清理后复验manifest2一致；随后仅上述三份前端文件作有意late-success修复并冻结manifest3/4。
  保留依赖、源、当前程序与证据，不清整个 target/node_modules。
  后续每批继续遵循 AGENTS 第9条；删除的缓存由 cargo 重建，不在回收站。
- 收尾已再次逐项复验manifest4的3075源、默认App及immutable seed hash一致；
  probe TypeScript、两个UI acceptance helper的Node语法检查与git diff --check通过。
  原本批App test/Chromium/runtime owners无匹配live进程；未重复执行历史清理脚本或改写清理报告。

## 下一批，不缩减总范围

1. 针对保留的原 cleanup_pending 首败补原 worker/close 时序诊断与压力对照。
   fixture cleanup 必须确认同 ticket 原 owner 真正 stopped，不能忽略 StopRequested，
   也不能加大输入期限、重放操作或把一次重跑当根因修复。
2. 补真实 owned approval worker 的 managed UI/API→App/ACP→runtime→publication 联合门槛，
   persistence/Stop/Drop/replacement/late preparation 与物理清理全链覆盖。
   真实 App/OS 权限仍需用户明确手势，不自动批准、不动日常浏览器或共享 home。
3. U04 其它 surface、关联 dialog/跨 surface/增量缩小；U05 其它 surface 恢复；
   U06–U09 生命周期/真实验收/全门禁/新开发包继续。
4. D00–D31、四 surface、Windows/macOS/Linux、真实 Grok、签名安装、同候选12h与完整UI/UX
   全部保留。此增量不等于整体可发布，不关闭持续目标。

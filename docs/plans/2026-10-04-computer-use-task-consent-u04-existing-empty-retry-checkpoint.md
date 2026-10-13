# E4a.1：原 ExistingTab 空结果重试与关闭时序

状态：**backend verified / full CU active / partial — not releasable**。
工作区 `H:/aicoding/grok-app-computer-use`，分支 `feat/computer-use-implementation`，
HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`。单 Codex writer；保留原 dirty/
untracked/fix.patch，未启动 Grok/子代理，未提交/push/PR/发布。

## 实际修复及边界

1. 新反例证明：原 preparation 可用性为1，但 discover 前发生 Unshare，得到空
   receipt。重新 Share 后，原 App load 仍固定返回0；实际 **0 passed /1 failed**，
   见 `existing-empty-red-1.txt`。不是参数、编译或测试启动错误。
2. Core 只允许 **当前、空、ExistingTab** receipt 转回原 discovery call。
   permit、full key、cancellation、Pair/Share guard和TTL原样转移，不创建新权限、
   挑选目标、刷新非空列表或延长准备寿命。原 cancel handle 仍能撤销转移后的 call。
3. 原 App slot 在空结果且原 preparation 再次有可用 Share 时重试。没有可用 Share
   时保留空 receipt；有原 Arc borrow 时不丢弃它、不另建call，允许原 borrow退出后
   再试。原 owned discovery worker、App process/session/ACP pin及批准 publication
   边界不变。新测试覆盖 Unshare/借用竞争、取消/Stop/替换和旧 borrow。
4. 相对 E4a manifest：6个既有 source改变、2个新测试source。没有依赖、模型schema、
   默认值、raw target parser、UI/locale或App/AppWorkbench改变。
   **不是完整 ExistingTab UI**：descriptor、共享controller、Pair/Share界面和15locale
   仍待E4b，不能用后端命令的通过宣称真人端到端可用。

## 关闭时序：有新证据，但原长尾尚未销项

沿原 source fixture只观察路径/响应计数、原隔离Chromium子进程退出、Playwright临时
artifact目录清理和原close；不改变参数/结果/deadline，不记录PID/命令/env/token/网页。
Playwright CJS在模块求值时复制spawn/spawnSync，因此观察器必须先安装、后动态import。
**以前没有force-kill日志不证明没调用taskkill**；旧日志没有这层完整观测。

- 原默认App artifact5无其它编译/fixture竞争的定向运行通过，原close自身399ms；
  约5秒是原synthetic gate，不是性能优化。
- 两次新增trace定向运行通过；第一次未捕获process事件，修正上述import顺序后能
  捕获原进程退出/目录清理。没有据此覆盖旧首败或冒称原长尾已修。
- trace第一次宽回归 **273 passed/4 failed/1 intentional ignored**。新增回复记录也
  使用`path`，被原请求计数oracle当作重复请求。只把回复字段改为`route`，保留原
  请求字段和断言，不放宽安全合同。日志和4个明确失败root保留。
- 最终同source、默认App artifact7宽回归 **280 passed/0 failed/1 intentional ignored**。
  关闭测试记录：close-event7278ms、原process exit8238ms、目录清理8240→8242ms、
  原promise8242mssettled、原consumer重试回复8243ms为200，随后同run为Stopped。
  native关闭自身989ms，仍含另约5秒synthetic gate；没有提高20秒等待或生产期限。
- 原自然长尾首败（native21530ms超过测试20秒）及其root完整保留。新的受控运行
  没复现那条自然长尾，**不能断言是生产泄漏，也不能断言只是测试期限**。继续用
  已补齐时序核对真实慢退出/目录清理/consumer；不刷绿后销项。

## 同最终源码验证

证据根 `tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`，旧记录不覆盖。

| 验证 | 实际结果 |
| --- | --- |
| Core default / test-support all-targets | 1068 /1084 passed，均0 failed/ignored；新增3项Core测试 |
| 默认App picker定向 | 20 passed，含新增3项App反例/取消合同 |
| 默认App CU完整过滤集合 | 280 passed /0 failed /1 intentional ignored，126.35秒 |
| 默认App ACP / routing | 177 /38 passed，均0 failed |
| 四种all-targets strict Clippy | Core/default-support、App/default-probe均通过 |
| ContextClose / bundle audit | 28 passed /0 failed，最终fixture语法检查通过 |
| fmt /quality /diff | 通过；App+Workbench仍13573行 |

过滤集合重叠，不相加。前端未改且未重跑全量；Extension/协议亦未改，不将前批
177/38计为本批运行。未运行真人App/MV3/Grok、安装态、三平台整链或真人12h。
全部默认App harness仍按CI嵌Common Controls manifest，真实进入断言。

最终3171源 `source-files-2.sha256`：
`164b2c3fc8f548a8fcd9ed18de699f021d1a390471abffdc7c6fcf968e375dd3`。
默认App `app-artifact-7.json`：
`7eafc46682ca8678a97c48f493abf9198feade814844d0fb553d4fd6225f80f1`。
最终source fixture：
`44f6e5c40f568a8c0fe46276ed7600dde68be1d964e4e5a0765a51e961e36fe1`。

## 下一步：接E4b，不重做E1–E3，不缩减完整范围

1. 前端API加入Started/View的surface及task-bound preparation/confirm DTO；只传原
   session/full key/pickerId和原nonce，严格校验同owner/surface，禁止renderer选endpoint。
2. 共享ComputerTaskActions/card/panel使用同一准备状态。ExistingTab先准备再discover；
   不自动confirm/Share/批准。安装入口、短配对说明、可用/忙/不兼容/空/失效/重试
   全路径；可见时才有一个metadata轮询owner，隐藏/换session/关闭/Stop清理迟到回复。
3. UI覆盖readiness→discover撤销Share/空结果→原任务重试→用户选候选→明确批准；
   取消/Stop始终可用，保留键盘/焦点/缩放、镜像只读和15locale完整键集。
4. 后端和真实UI路径完成后才开放ExistingTab descriptor；E5用同最终source MV3 +
   authenticated HTTP +原App/ACP publication +新TaskGrant联合验收，不用legacy grant替代。
5. 原自然关闭长尾、U00–U09/D00–D31、四surface、Windows/macOS/Linux、所有输入/
   恢复、安装签名、真人Grok、同候选12h、完整UI/UX保持开放；本批不是可交付结论。

## 本批构建卫生

原owners退出后生成了新的15组精确old/current清单（`empty-retry-cache-plan.json`），
与已执行E4a清单分开；本批已完成删除15组旧缓存，逻辑6809858517 bytes（可能含
hardlink），H空闲观察增加6058606592 bytes（约5.64GiB）。`empty-retry-cache-before.json`
及`empty-retry-cache-cleanup.json`保留；原App/Core/最新缓存/seed/fix.patch hashes未变。
4个诊断失败root已分别核原process退出、close-settled及无live owner，仅删除其重复
`.chrome-exec`副本（逻辑1444923812 bytes），C空闲观察增加1444782080 bytes（约1.35GiB）。
`trace-runtime-cleanup-before.json`及`trace-runtime-cleanup.json`保留；root、profile、
events、markers和原自然首败现场仍在，保留证据/seed hashes未变。
删除未进回收站，缓存由cargo重建，副本由隔离fixture从原seed重新生成。没有删除
整个target/workspace/node_modules，也未杀无关进程。已执行清单不可复用。

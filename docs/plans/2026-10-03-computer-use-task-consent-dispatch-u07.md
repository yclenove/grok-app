# Grok U07：真实App任务、批准次数与一次恢复验收

状态：**draft / not completed**。Codex已接手；核U01–U06实际合同/代码及当前候选后继续本批。
单Codex writer，负责必要修复与owned验收；不启动Grok/第二writer/子代理，不自动批准真实权限。
这一阶段补真实用户链路证据，不以合成测试、source页面截图或green CI代替它。

## 0. 冻结候选与事实

确认当前HEAD/dirty inventory/全部新增产品文件，记录各源、测试、oracle、CLI/runtime/
browser/extension/build配置hash。运行与编译都属于同候选；任何相关修复使对应原测试
失效，保留旧结果，冻结新候选并重跑受影响场景，不拼接不同版本数据。
U01独立snapshot/首败、U00原probe/wire、U02–U06日志/checkpoint保持原样。

用独立GROK_APP_HOME和已有合法独立GROK_HOME、owned fixture/profile/浏览器副本。
不读取导出认证、Cookie、用户正文/浏览器profile，不修改共享~/.grok、代理/VPN或日常进程。
真实CLI用normal permission mode，核实际argv/版本/模型/effort与有效tools catalog；
外层开发writer --always-approve绝不能成为产品normal模式授权次数的证据。

读docs/BUILD.md与当前CI。branch-built App使用明确dev identifier/config，不撞用户正式版
single-instance或默认App data。禁止直接运行install-latest：它会切main、fetch并安装，
不是当前dirty feature候选的验收流程。本批不自动安装/升级/发布。
App Windows libtest与实际App启动分开，测试manifest修复未进断言的情况不证明App链。

## 1. 先做可执行验收包，不让用户批准抽象计划

每个需要真人的scene先准备好确切build/source/runtime指纹、owned fixture、命令、界面
状态、预期结果、停止/失败恢复及继续命令。缺真人时写awaiting_external，不自动批准。
App卡、系统TCC/portal/UAC/firewall、ExistingTab真实toolbar Share都分别需要真实手势。
synthetic fixture只验证可自动运行的竞争/负向，报告明确标synthetic，不冒称human。

复用实际已有runner，先读它们接口，不猜参数：

- `tools/computer-use-probe/ui-acceptance.mjs`渲染真实组件但mock IPC，是布局证据；
  它使用immutable seed owned副本，不能把浏览器页面当Tauri App/原生DPI已验。
- `existing-mcp-live.mjs::sessionClient/verifyMcpClickOnce`跨真实MCP child与loopback，
  fixture counter证明动作；该runner自己没有真实Grok决策，不能作为model E4。
- `tools/computer-use-fixtures/windows/wpf.ps1`与App现有Win32 fixture可提供独立效果oracle。
  日常软件不是默认测试目标，两个应用/窗口都应owned且用户明确批准。
- macOS InputFixture、Linux X11/Wayland GTK/portal runner按实际机器和权限分类；
  CLI stub、headless/private GNOME或XWayland不可冒充安装态/真实native Wayland。

正常CLI→真实ACP→当前App Host→实际MCP→Broker→真实surface→独立fixture后置条件
全部有对应事件/效果；成功工具envelope、cancelled bool或截图里按钮文字不足以证明闭环。
oracle由独立端判断，模型不能提前收到答案/counter或被scripted动作替代。

## 2. 按surface运行，每项至少三轮

Windows至少两种owned原生应用/fixture、managed browser、installed Chrome与Edge
ExistingTab、App WebView分别列行。macOS arm64/Intel、Linux X11/native Wayland与
Ubuntu22/24需求保留；缺机器/原生权限只标相关行未验，继续当前机器ready scene。

| 场景 | 真实业务与交互要求 |
| --- | --- |
| 普通聊天 | 无授权卡、无capture/input/browser执行worker；不能因开面板提前申请 |
| Feature off首用 | 模型按需request→卡片→用户一次启用并批准→自动继续；批准前内容/输入0 |
| Busy首次申请 | 请求快速返回pending；原turn/gate结束后票据只发一次；无手工重发原话 |
| 等90秒再批准 | 真正monotonic等待90秒，聊天/Stop可用，原HTTP不悬挂；随后续行一次 |
| 连续20 actions | 初始批准后App额外确认0，真实业务20步；每步fresh身份/结果正确 |
| 两应用五次切换 | 同批准scope免重复App确认；正确target/fresh观察，旧refs拒绝 |
| 第三范围增量 | 恰好一次增量决定；拒绝/取消/失败仍可在原范围完成任务；新增效果0 |
| 暂停/用户接管→继续 | 一个Continue点击，额外重选/授权/手发消息0；新观察后新动作 |
| 请求拒绝/过期 | 无效果、无自动再申请风暴；真实可行非CU替代可继续解释 |
| Stop/feature off | 立即fence、新派发0、原owned resources terminal/join、旧票据/refs不复活 |
| 目标/权限失效 | 不找同名replacement、不假运行；必要系统/Share手势准确、完成后仍用户继续 |
| 其它MCP/shell权限 | CU批准不宽放；normal CLI仍真实请求原通用权限，CU scope内不重复框 |

可自动化的重复请求/双批准/旧事件/跨session/flood/断连unknown另跑合法输入负向。
真人90秒scene不能fake timer替代；等待写异步runner并能每60秒内观察/中断，不以长sleep
阻塞全部工作。实际工具调用次数、业务效果、App/CLI/系统批准次数分别统计，不互相代替。

## 3. 性能与体验证据

拆分模型决定申请、Host request→card paint、用户等待、prepare/必要系统手势、
commit→continuation send→first observe、pause fence、continue→fresh model observe。
本地card paint/范围内control回执目标p95<=500ms；给实际N与原始分布/失败样本。
网络/模型/用户/物理cleanup等待分别记录，不从总时间中暗删失败，也不放大timeout报快。
慢或卡住定位主线程/双轮询/锁/目录RPC，用可复现profile修真正原因再验对应scene。

实际Tauri WebView观察卡片/面板：窄侧栏、light/dark、中文/英文/长语言、键盘焦点、
Stop可达/忙时取消、stale/unknown准确、menu portal/背景/不穿透；OS DPI与CSS zoom分记。
UI fixture截图逐张检视只是辅助；本批Windows本机实机与其它OS未验项不混成全平台通过。
截图只owned fixture，脱敏后在ignored evidence root，产品默认仍不落盘内容。

## 4. 失败与收尾

第一失败保留，日志与report aggregate从实际events/effects生成；缺事件、零必需正向
效果、非terminal EOF、额外post-cancel call都必须红。wrapper0不代表内层CLI/Cargo通过。
timeout/断连按not_started/unknown/verified，unknown绝不重复副作用或双发continuation。
每scene记录原PID/start/executable/argv、context/profile/view/lease/ticket/socket/listener等
精确owner，核其真正退出/closed/join；借用tab不关闭，旧cleanup不影响replacement。

live child观察超时仍核原进程/句柄，不重启同scene；不要按process名结束任何资源。
runner EOF/异常也有owned cleanup；不能用全树kill当业务成功或直接删除unknown状态。
产品失败需要修复就先建有意义红回归，Grok改相关领域代码并重验新hash；Codex不代写。

## 5. 报告与后续

交`2026-10-03-computer-use-task-consent-u07-checkpoint.md`和ignored per-scene manifest：
implementation/verification/evidenceLevel/source-runtime-artifact hashes、真实commands/exit/
assertionCount/ignored/filtered、每轮效果与三类点击数、timings、首败、原owner收尾。
安全计数unauthorizedRead/input/wrongTarget/postFenceDispatch/duplicateContinuation/
unknownReplay/crossSessionGrant/wrongOwnerCleanup必须全部0；未验不写0冒称passed。
不把三轮局部测试当D29全模型任务矩阵或D30同一最终候选12h长稳。

人工缺口给prerequisite artifact、确切必要手势/预期结果/失败恢复/resume command，
不只写“请实测”。ready实现/负向/工具/文档都先完成，再把真有外部依赖的行交接。
Codex接收后派U08当前候选全量门禁，U09独立开发包；完整D00–D31仍持续，非正式release。

不commit/push/PR/merge/tag/release/reset/restore/clean/stash/rebase，不自动点真实CU或
系统/浏览器Share批准，不修改环境/用户数据来迎合验收。

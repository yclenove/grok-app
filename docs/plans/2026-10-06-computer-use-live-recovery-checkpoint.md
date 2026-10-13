# 实时恢复答复与第二次停止记录

2026-10-06；单 Codex writer；`feat/computer-use-implementation`。
本批不 commit / push / PR / 发布；原凭据、登录、会话、dirty 与 `fix.patch` 保留。
完整四 surface、Windows/macOS/Linux、两 Mac 架构及剩余交付计划保持。

## app114 实机：恢复后的新业务操作与主动停止

真人批准当前任务后，真实 Grok 4.7 / xhigh / ask 在独立受管浏览器操作本地测试页。
App 会话 `a968e9ed-9a45-47ea-aa93-8fcd5803c980`，
原 task `30171b35622e43839b337d3eef0dbdca`。
中文备注“继续执行验证 你好”只保存一次；第一次暂停时计数5。

一次 Pause：2026-10-05T15:34:11.347Z；至15:35:10.0778013Z计数仍是5。
一次 Continue：15:35:10.279Z；先重新观察，再产生6、7、8、9四次新计数。
一次 Stop：15:36:09.355Z，尚未到请求的10次；卡片最终“已停止”、控制禁用。
15:38:15.1934829Z及最终只读业务核验均维持9，未产生停止后的新副作用。

工具账本与网页业务记录核验：一次申请、没有额外批准或手工重发、没有重复保存，
没有 `computer_complete_task` 调用。GET `/state` 仅作只读业务 oracle，
模型实际输入通过电脑工具完成，未用 HTTP / CDP / 脚本代替。

原结果 `E/delivery-app-114/active-recovery-result.json`；
`E=tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。
App 原生 Close；fixture 原 session81795收到quit并join exit0；
六个原 App/CLI/MCP/runtime/fixture 创建身份均已退出，日常 App 未停止。
见 `E/delivery-app-114/active-exit-result.json`。

## 新反例与 source115

source114已修复重新打开历史的分组，实机继续发现两处不同入口的问题：

1. 实时 thought/body仍以最后user绑定，恢复文本写入暂停前assistant。
2. FE/Host将最后user之后的任一旧终态当作当前已停止，第二次Stop缺少新记录。

修复 `session/stream.ts`：以user或终态作为当前stream边界；旧实名迟到chunk
不能复活旧答复或写入恢复答复，保留原steer fallback行为。
Host state/runtime的当前新stream先建立空答复，支持尚未出字的立即Stop；
只接受新身份、busy且未被Stop latch抑制的状态，不因旧状态创建新轮次。
`session/rewind.ts` 的busy历史重读也不复活终态前的assistant。

FE终态去重只覆盖最近assistant子轮；同一次本地Stop和Host回执仍保留一个提示。
Rust终态检查在当前新lease开始时间之后查找标记；即使还没有stream flush，
旧Pause也不挡住新Stop/host-exit修复。无lease的旧journal使用最近assistant边界。
普通assistant内的标记文本不作为终态；迟到tool回执不自行开启新轮次。
没有手改原journal、权限范围或网页业务；prepare/commit仍保持原锁外写盘路径。
app115首次启动依正式恢复路径，为旧app114遗漏Stop且遗留active lease的会话
补记 `turn_cancelled|host_exit`。这是原会话的新增恢复记录，不能当作app114当时已正确记录Stop。
原实机报告保留启动前hash，启动后的变化另记；不删除或改写原Pause标记。

## 验证

- 前端最初9项先红；追加busy历史重读回归再次先红。最终12项新回归。
- Host新增3项在原harness中先红，涵盖恢复尚未输出、旧journal与迟到工具/普通文字。
- 最终13个相关前端文件223项通过。
- 当前默认App harness artifact116：turn_interrupt14、stream6、dispatcher4、process5，共29项通过。
  artifact115为修复前红回归harness，不与source/app115混用。
- typecheck最终通过；此前一次测试fixture的上下文类型错误已修，首败日志保留。
- targeted ESLint、App/probe `clippy -D warnings`、Rust fmt、quality final、diff均通过。
- 本批TS源统一LF；未重跑未改变的完整Core/native跨平台矩阵。

source115：3327件，manifest SHA-256
`6a45dc4d933366943e196c68ccc0ec852e843e55f7bdf665e400b2102a7794c6`。
实机业务数字属于app114，不能代替source115新包的真实任务验收。
fresh app115实际UI40.70s、Rust77s，450件ZIP逐文件解包核验通过；源在构建前后复验一致。
未签名本地测试包：

- EXE `dist-portable/Grok_CU_20261006_app115/Grok.exe`，92958208 bytes，
  SHA-256 `3ec07f73bf23ee6292346fb8ab9cc0eda18ba61282256362ee52ddf2c502cdf6`。
- ZIP `dist-portable/Grok_CU_20261006_app115.zip`，208412480 bytes，
  SHA-256 `43c55c10839a49e7490ff55d9daf05fd994346ed7fc92e5841a5e7b8f790d522`。
- 包清单SHA-256 `70372837bbf883388a007128f88ca2c14b0ec9a803451d8e7dcbf40d99196cb6`。
- 入口 `E/delivery-app-115/launch-preview.ps1`，复用独立home/登录/会话与原instance，拒绝重复预览。

实际Grok新任务另记，不能把编译/自动化通过写成产品完整交付。
构建时原Vite大chunk提示保留；MSVC创建import library的本地化stdout被Rust标为linker warning，
未发生link错误，严格源码检查已通过；不因本批修改扩展到构建配置。

整体仍 active / partial — not releasable。

## 预览收尾与清理

首次app115历史核验实例PID71092已正常退出；原App和子CLI/MCP均退出。
新包真实任务使用唯一预览PID76484，创建2026-10-05T16:13:16.3013070Z，
保留同一独立home；未替旧任务点恢复。
新本地fixture PID14632、创建2026-10-05T16:13:17.8693730Z，
原exec session93019，URL `http://127.0.0.1:3028/`。
固定任务已发送，真人批准前网页备注未保存、计数0、业务事件0。
新任务App会话 `18cf2a85-43da-4d9f-a872-6a48ed23161f`，
CLI `01a10cd9-4a71-77c1-a956-c99cb13e7d31`，task `8f06e566a23b4d5680d136670f975478`。
卡片“实时恢复记录验收”已显示“等待你允许”，批准前网页仍0副作用。
用户随后回复“点了”，实际新卡片已变为“申请已过期”。原Host决策是expired、authority=none；
原模型解释权限过期后正常回到就绪，没有再次申请。备注仍未保存、计数0、事件0，
原工具账本仅一次申请，没有其它电脑工具调用。本次不能算source115恢复/第二Stop实机通过。
证据 `E/delivery-app-115/expired-task-result.json`；当前PowerShell执行核验脚本exit0。
Windows PowerShell 5.1读取超长CLI账本路径失败，改用当前PowerShell执行后通过，非应用测试失败。
按原生Close正常退出唯一App，fixture原session93019收到quit并join exit0，
App/CLI/console/MCP/optional MCP/fixture六个原创建身份均已退出，原home/登录/会话保留。
未自动新建重复任务、未自动批准、未复用旧授权。下次新任务需真人批准；source115源码/包保持冻结。

清理已核实过期app114 ZIP，保留其解包目录作为首败证据及可重新打包的输入。
删除绝对路径 `H:/aicoding/grok-app-computer-use/dist-portable/Grok_CU_20261005_app114.zip`。
逻辑208408329 bytes；H空闲观察增208408576 bytes，约199MiB；
其它工作区同时编译，后者只是卷空闲观测，不能作为隔离的物理分配测量。
当前app115 EXE/ZIP hash前后相同，home/源码/凭据/seed/证据未删除。
可对保留app114目录重新执行原portable包装命令恢复ZIP，不在回收站。
证据 `E/live-interrupt-zip-115-cleanup.json`。

缓存清理脚本此前因其它工作区的cargo/rustc而保守延期；没有终止这些进程。
本次全部原owners退出后，新生成并审核`live-interrupt-cache-115-plan.json`，精确清6组已完成旧增量代。
逻辑7358709241 bytes（6.85GiB），H空闲观察增4861943808 bytes（4.53GiB）；
后者是卷空闲观测，未作隔离的物理分配测量。最新cache/source115/App harness116/seed/fix.patch hash复验不变。
证据 `E/live-interrupt-cache-115-cleanup.json`；不在回收站，可由cargo重编译生成。
unknown working、跨variant/未知runtime和失败现场继续保留；当前app115 EXE/ZIP未删除。

## 下一项独立工作

新实机授权未生效不阻塞独立开发：推进D12 App WebView新TaskGrant产品路径。
源码检查确认现有旧`computer_use_authorize_surface`绑定与原native probe已实现，
但新task descriptor/core picker/ACP producer/前端selector均没有App WebView入口；
不可仅加入descriptor把旧授权冒充新任务授权。按新[D12接入分批计划](2026-10-06-computer-use-app-webview-task-next.md)推进。
随后source122已完成[D12多绑定与最终原生提交基础](2026-10-06-computer-use-app-webview-authority-checkpoint.md)：
两个真实内嵌页中文保存/计数、exact释放/转交/刷新拒绝通过，Core与四strict/质量通过。
Host bind仍synthetic，candidate转换/新TaskGrant/前端与真人Grok未接；app115仍冻结且没有新待授权任务。
source118已完成[D12原生选择基础与Windows真实效果](2026-10-06-computer-use-app-webview-selection-checkpoint.md)：
同URL刷新/同label重建旧选择拒绝，中文保存1/计数1通过。Host bind仍synthetic，未接新TaskGrant；
完整交付与source115真实恢复验收均保持开放，不能拿本批fixture替代原过期真人任务。

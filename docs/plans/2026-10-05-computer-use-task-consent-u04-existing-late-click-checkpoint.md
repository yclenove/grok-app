# ExistingTab：原点击动作迟到结果检查点（R5，source78）

日期2026-10-05；工作区`H:/aicoding/grok-app-computer-use`，
分支`feat/computer-use-implementation`，HEAD`8d8b71c262ab222748bca32412eea884d4b892e0`。
单Codex writer；未启动Grok/子代理，未提交/push/PR/发布；dirty/untracked/fix.patch与首败现场保留。
整体active / partial — not releasable；ExistingTab descriptor仍关闭，raw parser仍Desktop-only。

接在[R4恢复owner替换检查点](2026-10-05-computer-use-task-consent-u04-existing-recovery-owner-checkpoint.md)之后。
本批只有click的四个独立fixture完成定点；R5的五动作20行及原截图验收尚未全部完成。
[下一步合同](2026-10-05-computer-use-task-consent-u04-existing-late-actions-next.md)已按实际文本事件与wait语义补充。

## 原调用与两个切点

首次真实session MCP request → source MV3 Pair/三Share → 原App picker选A/B、未选C →
新TaskGrant → 原ACP synthetic peer实际stdin首次delivery。没有legacy toolbar grant。
原App session/Host手势/ACP peer仍synthetic；这不是安装态UI、真人Grok或三平台验收。

原worker固定真实dispatch/request/proof/scope、SharedTabs/ActionCompletions实例与原Promise；
proof始终留在原worker内，不导出或打印。MV3不支持动态import，原静态class通过精确
source module frame的CDP debugger capture；随后立即resume并移除breakpoint/debugger。
原popup仅用被动cu-tab-state cue唤醒SharedTabs.current，不Share/observe或授予权限。

pre切点持有同一SharedTabs.act preflight的真实inspectDocument返回，原actDocument尚未invoke。
绑定实例的facade保留原函数/参数/rows，独立share/observe竞争读取要求0。
post切点先完成真实document-bound actDocument并取得原rows，独立DOM证明click恰好1，再持有原返回。
不回滚DOM、不替换结果、不扩大生产deadline/权限。

分别调用真正manager Pause或fence_computer_use_chat_stop。
原异步Pause waiter保留，先确认同步权限撤销、scope仍claimed/cancelRequested、
原signal aborted且act/completion仍pending，才释放原rows。
原act/completion/cancel/Host waiter自然join，scope settled，两类idle；
pre native invoke/click=0，post native invoke/click=1且原取消script也join。

旧actionId/refs、原finished completion ledger与consumed delivery都不能产生第二次动作或ACP wire。
Pair/三Share及A/B/C原document保留，B零输入、C零读取/输入。
Pause必须在旧owners结束、重放拒绝之后显式Continue，取得fresh正文与一次新ACP delivery；
Chat Stop不能因旧delivery/chat_started复活。
原Host动作返回实际isError，证据outcome=error；已发生click不能称NotStarted或零执行。

## 候选与门禁

证据目录`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。
source75实际编译默认App artifact75，SHA256：
`6a383f92e1f507c09329020733646e21200bec2487439f259c86833d10c3ef43`。
source76–78仅修改明确列出的probe JS，Rust/构建配置与编译source75一致，
原artifact75 hash和映射由`existing-late-action-candidate-artifact-78-1.json`实际复验。
source78为3298 files，manifest SHA256：
`8937f197c11618da48f523d4dc58095ceda7a749da05fd517e88c3d82a075634`。
后续source79 Rust测试扩展已重新编译，默认EXE路径被artifact79覆盖；
75映射只作本批历史证据，后续执行不能使用artifact75。

| 门禁 | 实际结果 |
| --- | --- |
| 默认App build / Windows manifest | source75 exit0，artifact75已记录；不是source78重编译 |
| pre Pause / post Pause定点 | 各1 passed / exit0，17.19s / 17.24s，各13 checks |
| pre Chat Stop / post Chat Stop定点 | 各1 passed / exit0，18.13s / 16.72s，各12 checks |
| 宽CU | 295 passed / 0 failed / 62 manual ignored / exit0，126.39s |
| ACP / routing | 178 / 38 passed，各exit0，2.68s / 2.33s |
| 完整native联合矩阵 | 61 passed / 0 failed / exit0，1273.66s；原session76554已终态，source78 Verify通过 |
| App / probe strict | source75实际exit0，27.54s / 27.05s；之后Rust未改，不称source78重新执行 |
| Node / MCP / 诊断unit | source78实际184 / 38 / 8 passed，均exit0 |
| syntax / fmt / quality / diff / Verify | 已实际通过；新增三个helper syntax另有验证 |

生产Core/前端/权限/manifest本批未改，未全量重跑Core/前端，不移用source73旧数字。
主existing_joint.rs为993行；App+AppWorkbench仍13573行，≥1000行文件budget仍80。
原完整native owners已终态；冻结期间源码未变，原矩阵没有重启或重试刷绿。

## 首次失败与身份纠正

| source | 首次失败的实际位置 | 修复/证据边界 |
| --- | --- | --- |
| 75 | 7 checks之后原worker TypeError，尚未派发动作 | 原MV3动态import不可用；改精确原静态module capture |
| 76 | 7 checks之后等待原module frame超时 | 原current为事件驱动；补原popup被动cue及paused事件排队处理 |
| 77 | 10 checks之后replay断言 | 允许production在finished ledger检查前创建瞬时scope；原scope pin不覆盖 |

三份`existing-late-action-first-failure-75/76/77-1.json`与原log/primary/phase/failure hashes保留。
76/77原recorder曾将artifact默认写成source编号；原文件未覆盖。
权威纠正为`existing-late-action-build-identity-correction-76-77-1.json`：实际均用artifact75。
新的recorder支持显式-Artifact；后续原失败必须使用实际artifact身份。

新增配对fetch诊断只保存networkError/blockedReason/corsError固定枚举，未知值为other。
不记录errorText全文、failedParameter、URL/headers/body/token/requestId，新增canary隐私测试通过。
source74原配对首败仍无新具体错误码，根因OPEN；旧71/72 B切换与初始化/IO/rename首败也仍OPEN。
新通过不代替未知旧失败的根因证明，不自动重试或增加预算。

## 清理状态与接续

本批尚未删除新增缓存。原session76554已exit0，原App/Node/browser owners独立复核为0。
fresh身份existing-late-action-cache-78-1的Plan已识别三组obsolete generations，逻辑3927544078 bytes
（约3.66 GiB），精确target/current/preserved hashes已回读；Execute在写before/删除前被新出现的
其它Rust owner挡住，零删除，before/cleanup report均不存在。
existing-late-action-cache-78-1-deferred.json记录原终态、实际guard与owner witness：
另一个grok-session-import-validation/cargo-target工作区的Cargo43204/rustc74916。
此前77 Plan同样零删除，不能复用；下一候选改源后也不能重用78 Plan。
其它worktree的Cargo/rustc只defer，不停掉它们；working/不明owner/失败现场保留。

清前已复验37旧hash/5原absences、64–66三log/12hash、67/68两log/6hash、
71/72/74三log/9hash及75–77三log/9hash；清后以fresh身份再复验。
最新build/Core/cache/依赖/seed/CFT140、源码/fix.patch和全部首败现场保留。
此前系列累计实际空闲观察增加37022396416 bytes（约34.48 GiB），本轮不能提前计入回收量。
删除缓存不进回收站，Cargo可再生成；实际空闲变化与逻辑文件量分别报告。

原矩阵若失败，先保存原终态log、精确test/root/profile、primary/phase/failure hash，不重跑刷绿。
随后按R5合同扩set_value/type_text/scroll/wait，每动作两个切点×Pause/Chat Stop的四独立fixture；
set_value change=1、type_text change=0，wait真实invoke与零DOM输入分列。
当前source manifest/headless无activeTab真实手势；截图权限拒绝需实测，成功迟到路径缺条件时not_run，
不放宽manifest、不伪造PNG。

完整E5、U04–U09/D00–D31、四surface、Windows/macOS/Linux/两Mac架构、完整native输入/恢复、
签名安装升级回滚、真人Grok、同候选12h及UI/UX全部仍required；本批不宣称可交付。

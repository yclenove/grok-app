# ExistingTab：五动作迟到结果检查点（R5，source79）

日期2026-10-05；单Codex writer，工作区`H:/aicoding/grok-app-computer-use`，
分支`feat/computer-use-implementation`，HEAD`8d8b71c262ab222748bca32412eea884d4b892e0`。
未启动Grok/子代理，未提交/push/PR/发布；全部dirty/untracked/fix.patch与首败保留。
整体active / partial — not releasable，ExistingTab descriptor仍关闭，raw parser仍Desktop-only。

前批[原click检查点](2026-10-05-computer-use-task-consent-u04-existing-late-click-checkpoint.md)的
原session76554已exit0，完整native61 passed、1273.66s；原App/Node/browser owners为0后才改冻结源码。
fresh78清理Plan逻辑3.66 GiB，Execute在删除前被其它worktree的Rust owner挡住，已记录零删除。
source78完整矩阵通过是前批证据，不能算source79的完整回归已完成。

## 本批实现与原生效果

严格按[R5合同](2026-10-05-computer-use-task-consent-u04-existing-late-actions-next.md)，新增
set_value/type_text/scroll/wait各四独立owned fixture，加原click四行，共20个独立native测试。
默认artifact79的--list实际列出20 tests；这是清单证据，实际通过数以运行终态为准。
固定场景集合只接受五动作×pre/post×Pause/Chat Stop，未识别动作/后缀拒绝。

原真实session MCP request/source MV3/认证HTTP/App picker A/B未选C、新TaskGrant与原ACP stdin
首次delivery仍复用原链。Host手势/App session/ACP peer为synthetic，不冒称安装态UI或真人Grok。
原preflight/actDocument门闩保留函数、参数、rows、原native Promise/scope/watcher；proof不离开worker。
没有扩大生产权限/manifest/deadline，没有回滚DOM或清历史ledger刷零。

fixture仅加被动beforeinput/input/change计数，独立原page在中断前和join/replay/Continue后核验：
click post=1，set_value post的input/change=1/1，type_text=1/0且inputType=insertText，
scroll post原scrollTop=60，wait post真实condition_matched与native invoke=1、DOM输入=0。
中文目标值只以实际DOM相等验证，报告不输出字段内容；B/C原页面与事件也必须保持不变。

新见证同时固定原native rows status/detail与原SharedTabs.act自然返回：
pre原actDocument invoke=0，最终rejected/stale_snapshot；
post原动作确已发生，随后unknown/cancelled_after_dispatch，physicallySettled=true。
原act/completion/cancel/Host waiter自然join、scope settled与两类idle均需实际证明；
不能只凭取消ACK或physicallySettled布尔把原执行算结束。
Host退役后的原MCP reply实际isError，outcome=error，不把已发生效果伪称NotStarted。
旧refs/actionId/原finished ledger/consumed delivery拒绝重放；Pause显式Continue只能在原owners结束后，
取得新fresh正文和一次新ACP delivery，旧refs仍拒绝；Chat Stop不能复活旧task。

本批仅改App private native fixture与probe JS，生产Core/前端/权限/manifest未改。
主existing_joint.rs仍993行，late_actions.rs为416行，joint-live.mjs为963行；
App+AppWorkbench仍13573行，≥1000行文件budget仍80。

## source79候选与门禁

证据目录`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。
source79实际冻结3299 files，manifest SHA256：
`d737820b30169dc31c6439fb4f0660d2c87312bc113b4201c800c8fc95eb057f`。
相对78只有四个既有App test/probe文件变化，新增一个private action-spec helper。
Rust测试已变化，实际重新编译默认artifact79（72s），并嵌入Windows test manifest；SHA256：
`2502a66e9b00974c41aeb08e677e8f36940250a6ed89cea7b57aff6887831514`。
默认EXE路径已被新artifact79覆盖，不再使用旧75映射或78 cleanup Plan。

| 门禁 | 实际状态 |
| --- | --- |
| 默认App no-run / manifest | exit0，实际artifact79已记录 |
| 五动作20行定点 | 20独立fixture各1 passed / exit0；原session65137已终态，原source Verify=0 |
| App / probe strict | source79实际exit0，15.21s / 15.83s；原session28706已终态 |
| Node / MCP / 诊断unit | source79实际184 / 38 / 8 passed，均exit0 |
| 五helper syntax / fmt / quality / diff / Verify | source79实际exit0，3299源不变 |
| 宽CU | source79实际295 passed / 0 failed / 78 ignored，124.44s；native另列，不把ignored算验收 |
| ACP / routing | source79实际178 / 38 passed，2.67s / 2.33s，均exit0 |
| 完整native | 原session56119已exit0；source79实际77 passed / 0 failed / 0 ignored，1549.34s，最终Verify=0 |

20行分别为click/set_value/type_text/scroll/wait各4 passed，零首败；
每行Pause为13 checks，Chat Stop为12 checks，含原执行器状态与独立页面事件/效果见证。
Core/前端本批未改，未全量重跑；source73旧Core数字不作为79验收。
冻结期间不得改源码。每项使用新owned profile/root；runner每个过滤器要求恰有实际通过项，
首次失败即停止定点，不重试、不覆盖原log；先record实际artifact79的primary/phase/failure hashes。

## 清理与下一步

原focus/strict/build/browser owners已终态后，fresh身份existing-late-actions-cache-79-1
完成Plan→回读精确target/current/hash→Execute，未复用失效的78 Plan。
仅删3组App旧incremental generations、265文件，逻辑3945920526 bytes（约3.67 GiB）。
H盘freeBefore=244995850240、freeAfter=248550785024，实际空闲观察增3554934784 bytes
（约3.31 GiB）；磁盘其它活动可影响观察值，不将逻辑文件量等同实际回收。
source3299、当前App/Core/seed/CFT140/fix.patch与最新cache hashes均复验不变。
原37hash/5absences、64–66三log/12hash、67/68两log/6hash、71/72/74三log/9hash及
75–77三log/9hash在清前/清后全部复验通过；本批20定点零首败。
保留current App/Core/cache、依赖/seed、source/fix.patch及全部首败；unknown/working/其它
worktree owner只defer，不停掉它们，不删除整个target/node_modules/workspace。
系列累计实际空闲观察增40577331200 bytes（约37.79 GiB）。
删除缓存未进回收站，可通过Cargo重建再生成；plan/before/cleanup JSON记录精确删除路径。

清理与旧证据复验完成后，原session56119顺序跑完本候选宽CU/ACP/routing与完整native，
实际exit0；全部五个runner阶段均exit0，报告`existing-late-actions-rest-79-1.json`。
完整native为77 passed、1549.34s；源码3299 files最终复验不变，不能把宽回归manual ignored算native验收。
原App/browser/probe owners全部退出后，收尾inventory实际检查22组App/Core cache leaves，
无新增完整旧代；本次额外逻辑删除/实际释放均0，未复用已执行过的79清理身份。
原App/Core/seed/CFT140/fix.patch仍不变；未完成working代、cross-variant与未知runtime/失败根继续保留。
收尾报告`existing-late-actions-rest-cache-79-1-inventory.json`记录原四log SHA256及终态摘要。
收尾再次复验37旧hash/5absences、64–66三log/12hash、67/68两log/6hash、
71/72/74三log/9hash、75–77三log/9hash，全部不变。本候选无新增首败。
原74配对与71/72 B切换、早期startup/IO/rename首败仍OPEN，新的通过不构成未知根因修复。

R5 screenshot仍单列required：当前source manifest/headless缺真实activeTab用户手势，
需独立实测原capture权限拒绝、零图像/正文降级发布；成功截图迟到缺真实授权条件则not_run，
不放宽manifest、不伪造PNG/data URL，不用fake截图unit代替成功native证据。
下一批成功截图的CDP action入口研究已补到R5合同；官方master接口可研究，
当前CFT140不支持，尚未下载/运行新版或证明activeTab成功，不将它称真人授权。

随后仍是完整E5、U04–U09/D00–D31、四surface、Windows/macOS/Linux/两Mac架构、完整native输入/恢复、
签名安装升级回滚、真人Grok、同候选12h与UI/UX；本批不宣称完整R5完成或可交付。

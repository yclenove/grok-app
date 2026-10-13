# ExistingTab：独立截图权限拒绝与真实授权条件（source80）

日期2026-10-05；单Codex writer，`H:/aicoding/grok-app-computer-use`，
分支`feat/computer-use-implementation`，HEAD`8d8b71c262ab222748bca32412eea884d4b892e0`。
未启动Grok/子代理，未提交/push/PR/发布；dirty/untracked/fix.patch、全部首败保留。
整体active / partial — not releasable；ExistingTab descriptor仍关闭，raw parser仍Desktop-only。

前批source79的原session56119已exit0，完整native77/0、1549.34s；3299源不变。
原App/probe/browser owners为0后才修改本候选，旧成绩不作为source80完整回归。

## 本批实现

新增独立`active-capture-permission-denied` fixture，经首次真实session MCP request、
原source MV3/认证HTTP、原App picker A/B未选C、新TaskGrant、原ACP首次实际stdin delivery。
Host手势/session/ACP peer仍synthetic，不冒称installed App、真人Grok或跨平台验收。

显式screenshot=true确实到达原Chrome.captureVisibleTab，真实权限拒绝自然返回；
原MCP返回adapter错误，无image或正文成功降级。原capture调用1、拒绝1、settled1、
原createImageBitmap解码0；原extension-result实际HTTP200收束。
用户随后显式screenshot=false才采新正文/refs；沿用原grant/run/group，旧refs拒绝，
不产生新授权或ACP续行。真实semantic三次包含初始、失败截图的document读取及显式新读取，
不能把失败截图的前置正文读取称为正文已发布或没有原native执行。

原Pair/三Share/A/B/C文档保留，B/C无model读取或输入；所有click/input/scroll/文本事件为0。
Stop实际达到Stopped，原页面仍在。定点10checks、1 passed/exit0、16.63s，原session36232已终态。
本批只改private App fixture/probe；生产Core/前端/权限/manifest没有变化。
主existing_joint.rs为998行，joint-live.mjs为974行；App+AppWorkbench13573、≥1000行预算80未增长。

## source与门禁

证据根`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。
source80冻结3301 files，manifest SHA256：
`b6b33da5290653bae4edc8d326ee7a64cb6f86b4aeb73d3d1dfbceb9a835742e`。
Rust fixture已变化，实际重编译默认App并嵌入Windows test manifest，artifact80 SHA256：
`3cffc7d9488f0fbec2ba12323aeffc77c5d66e5803ee06ae1e512712136f755f`。
默认EXE已覆盖79，不能用旧artifact79映射运行新候选。

| 门禁 | 实际状态 |
| --- | --- |
| no-run / 默认App manifest | exit0，原session61106已终态，app-artifact-80.json |
| 独立原生截图拒绝 | 1 passed / exit0、16.63s，existing-capture-permission-focused-80-1.txt |
| App / probe strict | 实际exit0，17.43s / 17.88s；原session57753已终态 |
| Node / MCP / 诊断unit | 实际184 / 38 / 8 passed，均exit0 |
| 新capture/diagnostics/joint syntax、fmt/quality/diff/Verify | 实际exit0；3301源保持不变 |
| 宽CU | source80实际295 passed / 0 failed / 79 ignored，129.12s；native另跑 |
| ACP / routing | source80实际178 / 38 passed，2.70s / 2.33s，均exit0 |
| 完整native | 原session22543已实际exit0；78 passed / 0 failed，1552.15s，existing-late-actions-rest-existing-joint-80-1.txt；最终Verify3301不变 |

本批Core/前端未全量重跑，不移用旧候选数字。冻结期间只读/文档/独立E脚本，不能改冻结源码。

## 真实activeTab授权条件研究

从官方CFT元数据下载固定154.0.8037.92/revision1689415到独立owned runtime，未替换CFT140或managed seed。
实际Browser.getVersion为Chrome/154.0.8037.92。原production extension key/manifest不变，未增加all_urls。
首次metadata探针在粗阶段报native-prerequisite-error，具体原错误未保留；原报告与失败profile80-1仍OPEN。
第二独立探针确认原Extensions.loadUnpacked成功；第三次补逐步phase诊断后取得真实API前置条件证据。
新成功不能凭空解释第一次错误或将它标成已修复。

第三次原capture先permissionDenied，真实tab target唯一；原Extensions.triggerAction调用后，
同一原tab/window返回真实PNG，764×429、16181 bytes，已实际检查图片中的owned中文fixture。
PNG SHA256 `6c8e1f3b1a287b0804d362cc57f92ef7947922ff2156577d524940edbb5b32f0`。
调用来源明确为CDP test-driver，不是真人手势；此为浏览器API授权条件，**不是原App/TaskGrant成功截图或迟到结果验收**。
原浏览器均实际退出，报告capture-action-native-prerequisite-80-3.json/PNG、capture-action-loading-80-2.json。

## 清理

App旧incremental清理80-1在Plan之前被其它worktree的live Rust owner挡住，零删除；
记录existing-capture-cache-80-1-deferred.json，不停止该owner，不复用旧79 Plan。
本任务download/build/test/browser owners退出后，只清固定重复CFT zip及两个成功的owned临时profile，
精确路径、重解析拒绝、原owner、源码和保留hash已核验；原失败profile80-1及原报告/PNG保留。
逻辑211564059 bytes；H freeBefore244754862080、freeAfter244966719488，
空闲实际观察增211857408 bytes（约202.04 MiB）。观察值受其它磁盘活动影响，不等同逻辑文件量。
本系列累计实际空闲观察增40789188608 bytes（约37.99 GiB）。
当前CFT154/CFT140、App/Core/cache/seed/依赖/源码/fix.patch及全部证据保留。
删除未进回收站；固定zip可按官方URL/hash再下载，临时profile按相应probe用fresh identity再生成。
capture-prerequisite-cleanup-80-1.json记录targets/current hashes/恢复方式。
清后37旧hash/5absences、64–66三log/12hash、67/68两log/6hash、71/72/74三log/9hash、
75–77三log/9hash全部实际复验不变。unknown/working/cross-variant/其它owner继续deferred。

原22543终态、所有原build/test/browser owners退出后，fresh existing-capture-cache-80-2
按Plan→回读精确targets/current/hash→Execute完成，清3组旧App incremental。
逻辑3923009695 bytes（约3.65 GiB），H freeBefore244945108992、freeAfter247567659008，
实际空闲观察增2622550016 bytes（约2.44 GiB）；累计观察增43411738624 bytes（约40.43 GiB）。
源码3301/current App/Core/最新cache/seed/fix.patch不变，前述全部旧log/hash/absence清前清后复验通过。
不进回收站；Cargo可重新生成这些缓存。其它working/unknown/cross-variant继续保留。
source80已收束，随后才接入source81测试候选；新候选成绩不能借source80。

## 下一步

原22543完整终态已取得且安全清理完成，未重启刷绿；所有历史首败继续保留。
原owners结束后按[真实成功截图迟到合同](2026-10-05-computer-use-task-consent-u04-existing-late-capture-next.md)继续，
以已证明的CFT154 action条件接回原App/TaskGrant，不用裸capture或fake PNG替代联合链。
Host Pause保留Pair/Share时，v1观察transport缺每request取消、原pending退休可能早于扩展解码的疑点
仍待真实反例；不能据API截图成功宣布R5完成或放开descriptor。
74配对、71/72 B切换及所有未知首败仍OPEN。
完整E5、U04–U09/D00–D31、四surface、三平台/两Mac架构、native输入/恢复、
签名安装升级回滚、真人Grok、同候选12h与UI/UX全部范围保留。

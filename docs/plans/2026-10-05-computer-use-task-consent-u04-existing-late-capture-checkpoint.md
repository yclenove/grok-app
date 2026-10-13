# ExistingTab：原App截图迟到的真实反例（source82）

日期2026-10-05；单Codex writer，`H:/aicoding/grok-app-computer-use`，
分支`feat/computer-use-implementation`，HEAD`8d8b71c262ab222748bca32412eea884d4b892e0`。
未启动Grok/子代理、未提交/push/PR/发布。dirty/untracked/fix.patch及所有历史首败保留。
整体active / partial — not releasable；ExistingTab descriptor关闭，raw parser仍Desktop-only。

## 已收束的上批

source80原session22543已实际exit0；完整native78/0、1552.15s，最终Verify3301通过。
宽CU295/79ignored、ACP178/routing38及其余门禁以[source80检查点](2026-10-05-computer-use-task-consent-u04-existing-capture-checkpoint.md)为准，不能移用到82。
原owners全部退出后fresh80-2清3组旧cache，逻辑3.65GiB、H空闲观察增2.44GiB，累计40.43GiB。
源码/current/全部旧证据清前清后复验不变；完成清理后才修改下一候选。

## 本批代码与编译身份

只改private App fixture/probe/诊断，不改生产Core、前端、权限或manifest。
将原read-only authority组装和成功报告移到现有子模块，保留原检查、空ACP wire见证及返回shape。
主existing_joint.rs为994行，joint-live.mjs为983行；App+AppWorkbench13573、≥1000行预算80不增长。

新增两条独立固定capture场景，不把截图冒充click；复用原Host Pause/Chat Stop、
原consumed ACP delivery检查和显式Continue，其动作scenario parser仍只认原五项。
新增capture gate和live helper，持有原captureVisibleTab Promise真正返回的同一data URL，
绑定原request/run/sequence/tab/document/snapshot/window；实际几何匹配，无fake PNG或all_urls。

source81编译首败E0596：原ACP wire-empty检查需要可变synthetic receiver，拆分时漏了&mut。
原build81 JSON/stderr/source manifest及失败记录保留；没有新artifact或native成绩。
修正两个借用点后fresh source82实际编译成功并嵌Windows test manifest，原session53344已exit0。
此已明确定位的编译错误有82成功证据；不据此关闭其他unknown首败。

source82冻结3303 files，manifest SHA256：
`3c777af60f35f36ca7482fa86e65de9583c2841d6860eaae625e9081b19b04e4`。
artifact82 SHA256：`87b69d3973098993bc5fddcd6c3d8f2363687c6adaf88d8f839ecc2d8e195185`。
默认EXE已覆盖80；后续不能用旧artifact80/81映射当前运行。

## 原App / TaskGrant / 成功截图链的反例

两条分别用独立owned CFT154/profile、原source MV3/认证HTTP、首次实际session MCP request、
原App picker选择A/B未选C、新TaskGrant、原ACP首次实际stdin delivery。Host/session/ACP peer仍synthetic。
原Browser级Extensions.triggerAction授予测试activeTab条件；明确owned CDP test-driver，不是真人工具栏。
原A/window/document保留；原MCP显式screenshot=true经原Broker/HTTP/ExtensionTransport/SharedTabs进入原capture。

两条实际均取得原PNG764×429、9375 bytes、geometryMatched=true；已查看图片中的owned中文fixture A。
PNG SHA256均为`669a73566d78915664eba22439cb7dc37b8f21a0264aaafd032e6b47cec736b3`。
原native调用1/settled1、权限拒绝0；在原data URL返回切点held时observe仍pending且decoder=0。
随后各自执行原Host Pause或Chat Stop同步fence，再释放同一原PNG并自然join。

| 原见证 | Pause | Chat Stop |
| --- | --- | --- |
| 原截图返回仍held时Host existing_operations_idle | true（错误提前空闲） | true（错误提前空闲） |
| held时原观察仍pending / decoder | true / 0 | true / 0 |
| held时Host cleanupPending | false | true |
| 释放后原createImageBitmap | 1（旧图仍解码） | 1（旧图仍解码） |
| 原观察返回成功给transport / completion含图 | true / true | true / true |
| 原MCP最终错误 / image / 旧正文成功 | true / false / false | true / false / false |
| 原capture / native settled / 输入 | 1 / 1 / 0 | 1 / 1 / 0 |
| 原Pair confirm / disconnect | 1 / 0 | 1 / 0 |

首个断言失败是Host提前idle；后续解码/含图提交由同一原被动见证实际记录，不能称为已执行并通过的断言。
模型未收到图或旧正文成功，Host最终拒绝仍有效；但它没有证明扩展物理收尾或解码前取消正确。
不能把已执行的截图称NotStarted/零capture，也不能仅靠模型结果拒绝宣布R5完成。
每条均通过前10checks后失败，原session78187实际exit101，0 passed / 2 failed、34.45s；没有重跑刷绿。

## 本候选门禁与证据

证据根`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。

| 门禁 | 实际结果 |
| --- | --- |
| 默认App no-run / Windows manifest | exit0，app-artifact-82.json |
| 原Pause / Chat Stop迟到截图 | 0 passed / 2 failed，exit101；existing-late-capture-focused-82-1.txt/json |
| 原截图权限拒绝回归 | 1 passed / exit0，15.86s；原session41643已终态 |
| App / probe strict | 实际exit0，18.05s / 16.78s；原session25939已终态 |
| Node / MCP / 诊断unit | 184 / 38 / 8 passed，均exit0；原session23619已终态 |
| 新helper/joint syntax、fmt/quality/diff/Verify | 实际通过，3303源不变 |
| 本批完整native/宽CU/ACP/routing/Core/前端全量 | 未重跑，不移用80数字；先修已取得的真实反例 |

两个失败profile、原阶段/通用primary/额外capture-primary/PNG均保留；
existing-late-capture-first-failure-82-1.json记录10个精确profile/PNG hash与原log/source/artifact。
verify-late-capture-evidence.ps1已复验，不删除原失败现场。其余37hash/5absences、64–66、67/68、
71/72/74、75–77首败仍保留，不能因新通过关闭74配对/B切换或其他初始化unknown。

## 下一步

本批原82 owners全部退出后，fresh existing-late-capture-cache-82-1按Plan→回读→Execute
清3组旧App cache，逻辑3900407862 bytes（约3.63 GiB）；H freeBefore244899860480、freeAfter247545667584，
实际空闲观察增2645807104 bytes（约2.46 GiB），累计观察增46057545728 bytes（约42.89 GiB）。
源码3303、current82/App/Core/最新cache/seed/fix.patch与原失败证据保持不变。
全部历史37hash/5absences、64–66、67/68、71/72/74、75–77及新增82原log/10hash清后复验通过。
删除未进回收站，Cargo可再生成；failed working/unknown/cross-variant及失败profile全部保留。

沿[观察请求的取消与物理完成合同](2026-10-05-computer-use-task-consent-u04-existing-observation-lifecycle-next.md)
修原v1观察生命周期。当前sweep删除cancelled pending使Host提前idle，而扩展无每request取消scope，
Pair/Share保留时live不感知Host Pause/Stop，真实旧图继续解码/提交；这已有实际反例，不再是猜测。
完整E5、U04–U09/D00–D31、四surface、Windows/macOS/Linux、两Mac架构、native输入/恢复、
签名安装升级回滚、真人Grok、同候选12h及UI/UX全部仍开放，不缩减交付定义。

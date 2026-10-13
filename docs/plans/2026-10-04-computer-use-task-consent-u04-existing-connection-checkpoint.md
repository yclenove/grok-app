# ExistingTab：原连接 Forget 撤权联合链

状态active / partial — not releasable；单Codex writer，不启动Grok/子代理，不提交发布。
接[成员精确诊断检查点](2026-10-04-computer-use-existing-member-diagnostic-checkpoint.md)与
[discovery检查点](2026-10-04-computer-use-task-consent-u04-existing-discovery-checkpoint.md)。
原dirty/untracked/fix.patch和全部failed roots保留。本批增加真实连接撤销验收，没有生产行为修复。

## 实际操作与原授权边界

新增choose-forget、reservation-forget、publication-forget三项固定场景。
操作为production source popup的Forget按钮→原PairingClient→认证HTTP
`/cu/extension-disconnect`，不是直接改Host registry或伪造proof失效。
后两项复用原owned reservation/publication锁外门闩，钉住原worker/原App pin，
实际断开完成之后才释放；保持原具体错误断言，不增加重试、sleep或预算。

三项各自检查：原MCP首次request仅一次；原Host完整task key不变；批准前没有正文、
截图或input；popup读回unpaired，Host分享数为0，实际disconnect HTTP200恰好一次；
原document未替换，A/B/C用户页面仍打开。Forget撤销本连接全部Share，与单页Unshare
保留其余Share的语义分开。旧候选或迟到原worker拒绝，grant/continuation均0。
reservation原worker实际join，publication原store rollback且原run达到Stopped。
负向完成后model observe/list仍拒绝，原owned cleanup和退出合同不放宽。

## 候选与已完成验证

source28：3259文件，`source-files-28.sha256`的SHA256：
`47b79f29129032073a1ea5062c06c77a8fd04e777fa49f67000c24e0bc95ca91`。
默认App build29 no-run实际exit0，原CI manifest已嵌入。
原executable：`src-tauri/target/debug/deps/grok_app_lib-63e3432e557be5b6.exe`，SHA256：
`fd6e0cab57317f7d727c06e9cacc92e4a83cf129575ddc11b6ee35da05638e4f`。
build26–28同路径已覆盖，不依据旧hash声称当前可回放。

`existing-connection-forget-native-1`实际3passed/0failed/exit0，57.87s，18checks。
仅使用显式隔离CFT140，不碰日常浏览器或managed Chromium130 seed。
Node177/MCP38、App default/probe两种strict Clippy、fmt29/quality29/diff29与Node syntax
均实际exit0；source28 Verify3259通过。App/Workbench仍13573行。
Core及前端未改，本批未全量重跑，不移用上批计数填本批门禁。

完整`existing-connection-forget-suite-1`实际13passed/0failed/exit0，246.27s，85checks。
含原成功13checks及全部12项各6checks的负向场景；不是只跑新增三项。
默认App宽CU实际293passed/0failed/14manual ignored，124.52s，exit0；14项ignored包含
另跑的13项existing联合场景及原10分钟真实request期限gate，不冒称宽filter自动执行了手动native矩阵。
ACP178/routing38实际passed/exit0，具名日志为`existing-connection-app-acp-1`与
`existing-connection-app-routing-1`。CU与ACP集合有重叠，不相加宣传总数。
本候选未复现旧IO或B动作首败，不能据此称生产根因已修复。

## 失败状态与下一项

source25/build26第二轮B成员五动作首败仍开放；source27精确phase诊断已完成，
三次成功链和一次十场景复测未复现，不是生产根因修复。保留原两个失败根及
脱敏joint-failure.json。旧native目录DELETE access error32和seed rename首败也未关闭。
不重放unknown，不靠复跑绿销项；若新失败出现先看原日志与固定phase/有限outcome。

下一项为真实connection replacement/profile/lease失效，App process/agent/ACP replacement，
Stop/取消/Drop及真实Pause→原Host显式恢复。安装与原任务UX门槛之后才开放ExistingTab
descriptor，从产品入口复跑；raw parser仍Desktop-only。
Host/Share手势、session和ACP peer仍synthetic，不是installed/真人Grok验收。
完整E5/其它U04/U05/U06/U07–U09/D00–D31、四surface、三平台/双Mac架构、完整输入恢复、
签名安装升级回滚、真实Grok及最终同候选12h均在范围内。

## 构建清理

用户要求以后常规清理无用构建，按AGENTS第9条每批收尾处理。
原build/test/browser owners已实际退出；新`existing-connection-cache-1`绑定source28/build29，
inspection实际exit0，读回3组精确old/current路径及保留项后Execute实际exit0。
删除3组旧App incremental generation，逻辑3775438505bytes（约3.52GiB）；
H空闲实际观察增加2563031040bytes（约2.39GiB）。Rust incremental可能共享hardlinks，
因此逻辑大小不能当作实际释放量。没有删除完整target/node_modules或其它工作区。
source3259/current缓存/App/Core/CFT140/immutable seed/fix.patch哈希清后均复验不变；
依赖及全部失败证据保留。旧`grok_app_lib-28mpg1j4ghpco`不完整working代次、
未证明废弃的跨variant/未知runtime保持deferred，没有杀任何无关进程。
已执行身份不能复用。缓存未进回收站，可由原Cargo构建重新生成。

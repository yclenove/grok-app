# ExistingTab：已发布任务的连接替换与真实租约过期

状态active / partial — not releasable；单Codex writer，不启动Grok/子代理，不提交发布。
接[Forget撤权检查点](2026-10-04-computer-use-task-consent-u04-existing-connection-checkpoint.md)。
保留原dirty/untracked/fix.patch与全部failed roots；本批增加实际验收，没有生产行为修复。

## 本批证明的实际边界

原source MV3/认证HTTP/首次session MCP request/Host picker/owned approval/App ACP
publication仍是同一链。先发布一个新TaskGrant并仅向原ACP续行一次，观察所选A，保留
原snapshot/ref；未选C与A/B页面均无input，C正文/截图也没有派发。

`active-connection-repair`在已获批任务中使用与Host配对命令相同的Broker begin/confirm，
随后点击原production source popup Pair，实际认证HTTP再完成一次连接，原document不变。
Host gesture仍synthetic；没有直接改registry、借legacy grant或导出真实凭据。
旧任务在重新Share前及A/B/C同document重新Share后，observe正文/截图、原ref click、
切B均拒绝。source observeDocument/actDocument计数不增加，独立DOM仍为0输入。
重新Share保留为新连接用户状态，旧grant不能借它恢复。Stop旧run达到Stopped，3项
新Share、连接及全部用户tab保留。

`active-lease-expiry`用CDP仅暂停原owned测试浏览器的source service worker；不暂停App、
listener或原expiry consumer。原worker确认paused前disconnect请求计数为0，不改原30秒
单调期限、时钟、token或heartbeat实现。private pipe每次只读Host stored-state witness，
不调用expire/renew；原生产HTTP maintenance每秒自动清原连接。观察到stored=false后
恢复并detach原CDP session，再经production source重新Pair/Share，重复旧grant拒绝及
Stop保留新用户状态的检查。不是browser crash/profile replacement的完成证据。

原pipe18秒/next20秒与reservation/publication15秒门闩都未改。lease独立等待仅按原
CONNECTION_LEASE30秒加2秒消费者结算上限，并要求至少19秒实际耗时（上一次10秒
续租可能早于pause）。没有用延长业务期限或重试把失败刷绿。
pause helper只允许Debugger enable/pause/resume；选择唯一原worker URL，不输出target
ID/原CDP回复/凭据。异常先恢复原worker，最终teardown仍由原context/Child/Job owner完成。

## 当前候选与实际结果

source29：3260文件，`source-files-29.sha256` SHA256：
`96eaad979aaf4c6ff19e557d3f527944a146938fe83948a0b0f9b8350096ef64`。
默认App build30 no-run实际exit0，CI manifest已嵌入；executable仍为
`src-tauri/target/debug/deps/grok_app_lib-63e3432e557be5b6.exe`，SHA256：
`13ffe1dac781f6572a3f37b42536507d121b83cc5b7395ec39ea89f55fa37ecc`。
build29同路径已覆盖，旧hash不能声称当前可回放。

`existing-active-connection-native-1`实际2passed/0failed/exit0，79.99s，共20checks。
真实lease witness：原worker暂停后29119ms，Host stored connection自然退休；原lease=30000ms。
两场景均完成原新TaskGrant及独立拒绝/DOM/Stopped/新Share保留检查。
App default/probe两strict、Node177/MCP38、fmt30/quality30/diff30与两脚本Node syntax
实际exit0；App/Workbench仍13573行。Core/前端未改未全量重跑，不移用旧计数。

同候选完整`existing-active-connection-suite-1`实际14passed/1failed/exit101，292.36s。
两项新增场景仍通过；choose-forget在source-mv3-start失败，0checks，未进入首次MCP
request或任何批准断言。其余14场景通过，不把整套改写为绿色。
原pipeline8273实际exit101，后续宽App项在此pipeline未运行；另起具名原artifact30的
必要回归实际exit0：宽CU293passed/0failed/16manual ignored，120.13s，ACP178/routing38。
16项ignored为另跑的15场景与原10分钟request期限。CU与ACP有重叠，不能相加宣传。
新全矩阵中真实lease witness为29537ms，两次自然过期均通过，不因其它14项绿关闭startup首败。
source29宽回归owners已退出后才开始source30诊断候选；原15场景失败日志保留，没有覆盖。

### 扩展启动新首败

App/store根：`C:/Users/Administrator/AppData/Local/Temp/grok-cu-managed-task-f232f44b-a6ad-4ad8-8c69-6c4cbc9ccb75`。
原browser根：`C:/Users/Administrator/AppData/Local/Temp/grok-cu-pairing-owned-d2e355e5-b6d5-46ea-838b-13e5adb5b549`。
原`profile/joint-failure.json`为stage=source-mv3-start、detail=start、Error/other，0checks，
diagnostic={}。该固定stage当前同时包围worker ready、popup创建/导航、Browser.getVersion
和trusted API查询；现有证据不能认定是service worker超时、popup导航、context退出或API
初始化失败。全部首败现场保留，没有读取其它profile数据库/凭据来猜测。
source29宽回归owners退出后已细分原startup每步phase，记录允许列表error family、
原context退出/worker事件数/源worker是否匹配与原耗时；只采固定布尔/计数，不输出错误正文、
CDP targets、页面或凭据。原Rust control result在teardown前留下固定timeout/node-reported
witness，避免原Child退出断言盖住第一处pipe结果。没有增加启动重试或延长期限。
source30为3260文件，manifest SHA256
`e43a3f78fb44c9a806db7485d0b55fdf3d66765c36bbfd9987dcde4c74f902fb`；默认App build31
实际exit0，CI manifest已嵌入，原同路径executable的SHA256为
`d736265cb06f055420376fc5bb8cf506b2b91f55b3f3c047e55c28f38c7c4617`。
build30同路径已覆盖。`existing-startup-diagnostic-forget-1`实际1passed/exit0，15.55s，
原6checks均完成；本次未复现启动首败，不是生产根因修复，不无限复跑覆盖原矩阵。
当前source30`existing-startup-diagnostic-active-1`实际2passed/exit0，79.16s，20checks，
真实lease witness为29650ms。App两strict、Node177/MCP38、fmt31/quality31/diff31及
source30 Verify3260实际exit0。Core/前端未改未全量重跑。
source30没有重跑完整15场景或宽App集合；这次diagnostic候选的受影响fixture已验，
source29的14passed/1failed矩阵与宽CU293/ACP178/routing38只按各自原候选记录，
不能改标签当作source30全量通过。启动首败仍开放，诊断随后续矩阵保留。

## 产物清理

所有本批原build/test/browser owners退出后，新`existing-active-connection-cache-1`绑定
source30/build31，inspection→读取3组精确old/current路径及保留项→Execute实际exit0。
删除3组旧App incremental generation，逻辑3790826403bytes（约3.53GiB）；
H空闲实际观察增加2455056384bytes（约2.29GiB）。逻辑大小可能包含共享hardlinks，
没有将它充作实际释放空间。source3260/current缓存/App/Core/CFT140/seed/fix.patch
清后哈希均不变。失败根、依赖、不完整working变体和未证明废弃的跨variant/runtime保留。
宽回归期间出现的其它Rust owner未被杀，原owners退出后重新核对才开始清理。
缓存未进回收站，可由原Cargo构建再生成；已执行身份不可复用，每批按AGENTS第9条收尾。

## 首败与完整交付范围仍开放

source25第二轮B五动作首败、native DELETE access error32/未知owner及seed rename首败
根因未证明，本次未复现不能销项。原失败根、脱敏诊断、日志与哈希均保留。
Host/Share手势、App session/ACP peer仍synthetic；源码CFT140不是installed/human/Grok E2E。
ExistingTab descriptor仍关闭，raw parser仍Desktop-only，不因测试通过提前开放。

在启动首败精确诊断之后，继续真实browser/profile退出替换、App process/agent/ACP replacement、Stop/取消/Drop，
再真实Pause→原Host显式resume及fresh observation/原ACP续行一次。
安装与原任务UX门槛之后才开descriptor并从产品入口复跑。
choose/reservation/publication处连接替换和真实lease竞争尚未全部覆盖，不用本批已发布任务
验收替代这些切点；原门闩时钟及share proof完整边界保留。
完整E5/其它U04/U05/U06/U07–U09/D00–D31、四surface、三平台/双Mac架构、完整输入恢复、
签名安装升级回滚、真实Grok和最终同候选12h均仍要求完成。

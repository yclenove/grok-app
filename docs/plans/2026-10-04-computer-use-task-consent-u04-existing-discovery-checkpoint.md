# ExistingTab：原owned discovery期间实际撤销

状态active / partial — not releasable，单Codex writer；原dirty/untracked/fix.patch保留。
接[reservation检查点](2026-10-04-computer-use-task-consent-u04-existing-reservation-checkpoint.md)
和[native清理诊断检查点](2026-10-04-computer-use-native-cleanup-diagnostic-checkpoint.md)。
不启动Grok/子代理，本批不commit/push/PR/发布。

## 当前实现

每Broker一次test-only hook位于原`HostScopePickerCall::discover`实际adapter producer
返回之后、原candidate identity逐项复验之前。门闩钉住真正的原snapshot，不把ready picker
加载完后的choose撤销冒充discovery竞争；锁外等待，正常production无field/module/call。
不替换adapter、Host registry、原生产constructor或原App/ACP transport。

新增`DiscoveryProbe`调用原SessionManager load→原ACP load→原discovery OwnedTasks，
进入门闩时必须捕获三个真实source MV3 Share、一个原owned discovery worker；feature /
execution / grant / preference / continuation仍0，原完整key不变。主线程private child pipe
只协调隔离fixture，不新增生产批准/释放API。

production source popup真实Unshare A或B，认证HTTP/Host读回剩余两Share后释放原门闩。
原load必须具体拒绝并join，不能返回旧receipt，不能把panic/join failure、另一owner replacement
或超时当成功。真实生产discovery仍五秒；fixture要求四秒内release、原result五秒内返回，
没有冻结/扩大产品时钟。其它Share/tab保留，A/B/C正文、截图/input均未提前派发。

新增Core一个原snapshot单位合同：synthetic authenticated registry在hook中Unshare原成员，
必须原proof复验报Adapter(TargetUnauthorized显示文本)，回Pending、无borrow/权限/效果。
首次定向actual0passed/1failed/exit101是oracle把error code误当Display文本，原拒绝本已生效；
修为精确`BrokerError::TargetUnauthorized.to_string()`，不是放宽为任意错误。
第二次定向actual1passed/exit0；首败日志保留，不包装成产品先红后修。

## 初始候选与保留的实际结果

证据根`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。
source24：3259文件；`source-files-24.sha256` SHA256
`8c76f6027b0da160da4d708c904ca2fab123f8c0853606701a1d0c222c30fb5e`。
默认App build25 no-run实际exit0，原CI manifest已嵌入；executable仍
`src-tauri/target/debug/deps/grok_app_lib-63e3432e557be5b6.exe`，SHA256
`e27afeacaa442c945c7b91213360013f5793a334b9fa43128e449ec0057f06ee`。
旧build22/23/24同路径已被覆盖，不借旧record宣称当前可回放。

两项discovery native定向实际2passed/0failed，34.95s，actual exit0；原A/B撤销均
rejected=true、grants=0、continuations=0、discovery_workers=0。仍为original App/
actual source MV3/认证HTTP与独立DOM oracle，Host手势/session/ACP peer明确synthetic。
Core两种全量已分别1079/1095passed、actual exit0；clippy-core5实际exit101，新增
test-only hook field嵌套类型触发type_complexity，随后三项strict未由原pipeline执行。
不能标四strict绿；原pipeline未执行后续core-support5/app11/probe10。
同候选十场景整套两轮实际各10passed/0failed/exit0，202.44s与229.57s；每轮67checks。
宽App CU实际292passed/1failed/11manual ignored，126.43s，actual exit101；原pipeline因此
停止，ACP/routing未运行，不冒称原source24已经通过。原source24最后复验3259文件不变。
Node177/MCP38、fmt9/quality9/diff12与Node syntax已实际exit0。
前端生产未改，不移用旧前端全量数字；未跑完的检查不预填绿色。

## 本批再次捕获native清理首败

原`managed_native_publication_pins_original_session_and_request_transport`在transport-replaced
分支退出后，原remove_dir_all仍报rawOsError5/PermissionDenied，原supervisor witness同时
为原Job ActiveProcesses=0、原Child handle signaled、原status可用。
精确失败root为`C:/Users/Administrator/AppData/Local/Temp/grok-cu-managed-task-7191d080-9889-4a44-bd90-a6493accc04b`，
原226entries；失败36ms后的DELETE access snapshot捕获`.chrome-exec/chrome-win`目录、
`v8_context_snapshot.bin`和`icudtl.dat`的error32/SharingViolation，其余逐项仍保留在原日志。
这证明失败附近仍有不可DELETE的文件，尚不能确定owner，也不把原Job=0当作没有瞬态锁。

之后09:13:35Z只读Restart Manager/DELETE access事后snapshot为225entries/176files，
无可见owner/DELETE access错误；证据`existing-discovery-profile-locks-1.json`保留。
检查无修改/删除/Shutdown/Restart，不据事后无锁销项。新root、原三个root、全部旧failed
profiles及seed rename首败保留；上批source23/build24宽CU绿只证明未复现。

## 修正后的候选与实际结果

原38183 pipeline实际exit101，原owners退出后只提取test-only具名`ScopeDiscoveryHook`，
不加allow、不抬门禁、不改变生产行为。source25仍3259文件，manifest SHA256
`cd987448b570091a63cb44f6cc3e09a6e50083aeaaf6a57d73ea767e08c22a77`。
默认App build26 no-run实际exit0，原CI manifest已嵌入；`app-artifact-26.json`的executable
仍为上文同路径，SHA256 `293ce35099670d5df5ac7af628a4630dc1e1288edc67f9a72942ac8ef2c22422`。
build25同路径已覆盖，不按其旧hash声称当前可回放。

新候选Core default1079/test-support1095实际passed/exit0；四strict均实际exit0，分别
`clippy-core-6`、`clippy-core-support-5`、`clippy-app-11`、`clippy-probe-10`。
Node177/MCP38、fmt10/quality10/diff13及Node syntax均实际exit0；App/Workbench仍13573行。
原默认App ACP178/routing38实际passed/exit0，日志为`existing-discovery-app-acp-2`和
`existing-discovery-app-routing-2`。宽CU实际293passed/0failed/11manual ignored，121.57s，
actual exit0；这是本候选未复现清理失败，不是旧首败根因已修复。
十场景第一轮实际10passed/0failed，192.03s，exit0；第二轮实际9passed/1failed，
191.67s，exit101。两项discovery撤销在两轮都具体拒绝且原worker退出；整套第二轮不绿。
最终App验证pipeline84998实际exit1；没有额外复跑成功场景来覆盖这次失败。
首次新runner将Switch值经powershell -File传成字符串，在参数转换时exit1，未进入测试、
未创建任何上述验证日志；已修成true时仅传`-IncludeIgnored`，不算产品断言首败或覆盖日志。
旧候选结果不冒称新候选验收。当前大构建前H空闲约309GiB，C约153GiB。

### 联合成功路径新首败，先补精确诊断

第二轮在`new-grant-member-b-five-actions`出现AssertionError/ERR_ASSERTION，已完成前
10checks；没有完成B五动作、切回A或最后Stop的整套合同。日志的`detail=scroll-action`
是上一阶段留下的值，memberActions没有逐项刷新detail，不能据此断言是B滚动失败。
真实新TaskGrant/原ACP续行曾已发布，因此这不是discovery撤销晚到grant的失败；但整套
正向链仍failed，不能从其他九项通过推论可用。

保留两个精确失败根：

- App/store root：`C:/Users/Administrator/AppData/Local/Temp/grok-cu-managed-task-e25ac3cb-25ec-48bd-83db-34263e8e0545`。
- 原owned浏览器root：`C:/Users/Administrator/AppData/Local/Temp/grok-cu-pairing-owned-f7833601-a7b4-4d76-82d1-fa9bbb8e1e1a`，
  `profile/joint-failure.json`记录固定stage、前10checks及脱敏诊断；storageWrite/accessLevel均ok，
  totalScripts74/inspections41/fences4。未读取Cookie/Token或原配对记录。

下一小批先给原memberActions逐项记录固定member/action/断言phase、有限outcome kind与
独立DOM后置条件，不记录原MCP reply、身份或错误payload。确认具体反例后才改生产路径；
不把unknown改verified，不重放原操作，不靠重试/sleep/放宽断言或增加预算刷绿。
其它owner/replacement/Pause矩阵仍在后续范围，旧三类IO/rename首败也不销项。

原build/test/browser owners已退出；新`existing-discovery-cache-1`按source25/build26完成
inspection→读取15个精确old/current pair→Execute，实际exit0。删除15组旧App/Core
incremental generation，逻辑6909887984bytes；H空闲实际观察增加3749093376bytes，
约3.49GiB。source3259/current缓存/App/Core/seed/CFT140/fix.patch hash均复验不变。
保留全部原失败根、日志与诊断；unknown working/跨variant/未知runtime不当成废弃物。
缓存不在回收站，原Cargo构建可再生成；已执行旧清单与本清单均不可复用。

## 后续交付范围仍保留

真实connection/profile/lease失效、App process/agent/ACP replacement、Stop/取消/Drop、
真实Pause→原Host恢复；安装与原任务UX门槛之后才开ExistingTab descriptor并从产品入口复跑。
raw parser仍Desktop-only，不顺手扩权；Host手势/session/ACP peer仍synthetic，不是真人
installed/Grok整链。完整E5/其它U04/U05/U06/U07–U09/D00–D31、四surface、三平台/
双Mac架构、输入恢复、签名安装升级回滚、真人Grok/同最终候选12h均仍开放。

# Native cleanup：原失败点诊断

状态active / partial — not releasable，单Codex writer；不启动Grok/子代理。
接[reservation检查点](2026-10-04-computer-use-task-consent-u04-existing-reservation-checkpoint.md)。
本批不提交/push/PR/发布，原dirty/untracked/fix.patch和全部失败现场保留。

## 已知事实与改动边界

上批source21/build22真实native八场景两轮各8通过，宽CU实际290通过/3失败/9手动ignored。
三项失败是原`remove_dir_all`报Windows os error5，并非启动失败。没有证据证明权限配置、
杀软或seed rename首败同一根因，不靠简单重试、sleep、抬预算或跳过断言销项。

只读Restart Manager和DELETE access snapshot已分别检查三根原失败目录，原数据未修改：

- app-scope-355e5506-c945-4615-b18f-8937a1c13fc3：222entries/169files。
- managed-task-800163d5-7d65-41df-97fc-d7ff2f8d7a23：217entries/166files。
- managed-task-40089290-b27c-4153-957e-52450d029d6c：221entries/172files。

精确父目录仍是`C:/Users/Administrator/AppData/Local/Temp/`，前缀均`grok-cu-`。
检查时无DELETE access失败、无Restart Manager可见owner；这是事后snapshot，不能证明
失败瞬间没有瞬态映像/文件句柄或filter driver，不能将此写成“没有占用导致权限错误”。
诊断脚本在本批证据根`inspect-profile-locks.ps1`，只注册资源/查询/打开再关闭访问句柄，
不调用RmShutdown/RmRestart、不删除、不读取文件正文或凭据。

新增默认App test-only witness：原supervisor实际shutdown路径记录原Child handle的
`WaitForSingleObject(handle,0)`、同原Child status、原Job ActiveProcesses和精确owned root。
不重新打开PID、不改生产shutdown、不把未知状态当退出，不额外杀任何进程。

scope与managed原fixture共用test-only精确root inventory/removal helper：保留原
`fs::remove_dir_all`及失败结果；失败时立即记录raw_os_error、kind、原entry数、剩余
路径与只读DELETE access错误。没有重试/延时/输入重放，原失败必须继续使测试exit101。
reparse和父目录检查继续拒绝逃逸，helper未编进正常产品。

## 当前候选与验收

source23：3256文件，manifest `source-files-23.sha256` SHA256
`a30e563394082e8843c176712074c133a920b2f192aa2e6dd79983818420e8b3`。
build24默认App no-run实际exit0，原CI manifest已嵌入；`app-artifact-24.json` executable仍
`src-tauri/target/debug/deps/grok_app_lib-63e3432e557be5b6.exe`，SHA256
`9b2545bb5fee8ca2b1ceace0fe142e02479f26e44802f31e092c85db5a169a42`。
不以旧同路径build22/23 hash宣称可回放。source22/build23只为未运行的初次diagnostic中间产物，当前已修正
test telemetry字段为明确预算，不把不准确elapsed字段当测量证据。

三个原失败fixture分别实际1passed/exit0：scope3.62s、publication三分支10.04s、
owned-drop3.31s。原宽CU完整293passed/0failed/9manual ignored，126.58s，actual exit0。
宽CU记录22个原supervisor witness，全部原Job ActiveProcesses=0、原Child handle wait=0、
原exit status可用；没有removal failure snapshot。此次未复现，不是原三首败根因修复。

default App/probe strict实际exit0（clippy-app10/probe9）；fmt8/diff11/quality8实际exit0，
App/Workbench仍13573行。Core和Node本批未改，未全量重跑，不借旧数字填本批结果；
旧八个joint manual gate在本批宽filter仍ignored，不冒称对本diagnostic候选再次执行。
原build/test owners退出后，新`native-cleanup-diagnostic-cache-1`已完成精确inspection /
读取plan / Execute。最初预检在fmt/quality还live时拒绝，未删任何文件；其原句柄终态后
重新检查新plan。最终清5组旧App incremental generation，逻辑4616492961bytes，
H实际空闲观察增加3176955904bytes，约2.96GiB；source3256/current/artifacts/seed/
CFT140/fix.patch hash复验不变，原失败根与全部证据保留。已执行清单不可复用。
仅缓存不在回收站，原Cargo构建可重新生成；未知working/跨variant/失败runtime保留。

下一步不因局部复测绿销项或无限重试刷绿。失败时诊断继续保留，需原失败路径/句柄/
受控锁和seed rename独立证据；同时推进下一ready项真实discovery过程中撤销联测。

seed publication原rename首败、真实discovery race/其余E5/完整U00–U09/D00–D31、
四surface/三平台双Mac架构、签名安装升级回滚/真人Grok/最终候选12h仍开放。

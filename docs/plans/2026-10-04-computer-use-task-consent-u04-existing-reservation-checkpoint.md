# ExistingTab：原 owned reservation 入口真实撤销联测

状态：active / partial — not releasable。单Codex writer，原dirty/untracked/fix.patch
全部保留，不启动Grok/子代理，本批无commit/push/PR/发布。
接[最终授权撤销检查点](2026-10-04-computer-use-task-consent-u04-existing-revocation-checkpoint.md)。

## 本批实现与证据边界

新增每Broker一次的reservation test hook，位置为reserve_selected_access_approval已验证
原broker/transport/permit之后、逐成员原scope proof复验之前。hook只在test/test-support
编译，锁外等待，不是模型/IPC批准入口；正常生产没有该field/module/call。

默认App native fixture仍调用原load、start_picker_approval、finish_picker_approval、
reservation OwnedTasks、生产scope reservation和原TaskApprovalPin/constructor。fixture
只是挂起真正进入的原worker，并持有原JoinHandle至join，未换adapter或另造grant。
新增test-only owned registry计数核对entered时确有一个原reservation worker，而feature /
execution / grant / continuation / stored preference仍未发布；释放后必须原worker退出。

source MV3 popup实际Unshare active A或inactive B，经认证HTTP/Host读回撤销后才释放
原门闩。要求在原reservation具体错误被拒绝，不把barrier panic/timeout/owner replacement
泛化成成功。A/B/C内容、截图及input均不得提前派发；其它Share/tab保持，原full key不跟随
替换。两场景是独立owned profile/session/run，不重放unknown。

新增Core三项actual reservation入口合同：合成registry撤销A/B拒绝，未选C撤销仍允许
原A/B预留；feature/grant/continuation/content/input仍0。定向实际3 passed，exit0。
Core的Share手势是synthetic，不能冒充真实MV3。真实native场景仍保留旧成功13checks、
choose三反例和publication两反例，总八场景复测，不仅跑新fixture。

## 当前候选与检查状态

证据根：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。
source21，共3255文件；source-files-21.sha256 SHA256：
`a5cacbe5e099f202603893a0bb8c0b9ecb2b9dd3069c3734ca7c84d413f7cb6c`。
默认App build22 no-run实际exit0，原CI manifest嵌入；executable：
`src-tauri/target/debug/deps/grok_app_lib-63e3432e557be5b6.exe`；SHA256：
`bb9a50a8d32ddd651ba69efc331bf7b0d863300fd80a2036291b1ee384edbc1a`。
build21同路径已被覆盖，旧record不是可按旧hash运行的当前程序。

Node177、独立embedded audit7、MCP/golden38、fmt7/quality7/diff10、Node syntax实际exit0。
App/Workbench仍13573行，没有新UI文案或广告ExistingTab descriptor；raw parser不放宽。
真实native完整八场景两轮各8 passed / 0 failed，169.68s / 156.82s，actual exit0。
每轮原成功13checks与七个负向各6checks，共55checks；新active/inactive reservation均
具体拒绝原worker，grants=0、continuations=0、reservation_workers=0。
Core default第二轮实际1078通过、test-support1094通过；四strict分别clippy-core4 /
core-support4 / app9 / probe8实际exit0。首次default失败见下节，不因第二轮绿销项。
App宽CU实际290 passed / 3 failed / 9 manual ignored，189.86s，actual exit101；
八个ExistingTab manual gate已在上述两轮独立执行，其它手动gate不冒称已覆盖。
原pipeline按真实CU失败停止；随后独立ACP178/routing38实际exit0，没有伪造原pipeline全绿。
前端生产本批未改，未全量重跑。局部reservation通过不代表宽native回归或整机产品完成。

## 独立seed publication首败，根因仍开放

source21 Core default第一次全量实际exit101，873 passed / 1 failed，35.92s，集成targets
未执行。唯一失败是原runtime_prepare tiny fixture的first prepare：publish seed /
move prepared staging to seed，Windows“拒绝访问”(os error5)。本批预留测试均通过。
没有改权限/系统代理、添加杀软排除、扩大预算、跳过失败测试或把权限错误改成成功。

独立同source的tiny diagnostic实际1 passed，28.99s，exit0；默认全量第二次也已实际exit0。
这两次绿只证明复测结果，未证明为何第一次rename被拒绝，不能销项。
新增观察到的精确fixture根
`C:/Users/Administrator/AppData/Local/Temp/grok-cu-prep-1d1d1407-c2da-45ca-9928-c287c3e3a407`
其mutation owner PID30660、cache/fixture layout及lock metadata保留；没有按“临时目录”删除。
进一步需要绑定原rename/原import子进程退出、文件句柄/外部锁和具体IO错误的受控witness，
而不是将单独串行复测或“可能是杀软”写成已证明根因。旧首败日志不覆盖。

## App native清理首败，精确现场保留

宽CU三个失败均进入原断言，错误为Windows“拒绝访问”(os error5)，不是测试启动故障：

- `computer_use::playwright_worker::scope_tests::native_worker_host_scope_identity_preserves_instances_and_retires_on_stop`：
  原`scope_tests.rs:168`的owned profile cleanup断言，底层`remove_dir_all`失败。
- `session_manager::computer_use::task_publication::tests::managed_native::managed_native_publication_pins_original_session_and_request_transport`：
  原`managed_task_tests.rs:220`的`remove_owned_root`失败。
- `session_manager::computer_use::task_publication::tests::managed_owned::managed_owned_waiter_drop_during_native_launch_retains_original_cleanup_owner`：
  同原`remove_owned_root`失败。

完整首败日志为`existing-reservation-app-cu-1.txt`，没有跳过、抬预算或只重试刷绿。
独立观察并保留这批新fixture根：

- `C:/Users/Administrator/AppData/Local/Temp/grok-cu-app-scope-355e5506-c945-4615-b18f-8937a1c13fc3`
- `C:/Users/Administrator/AppData/Local/Temp/grok-cu-managed-task-800163d5-7d65-41df-97fc-d7ff2f8d7a23`
- `C:/Users/Administrator/AppData/Local/Temp/grok-cu-managed-task-40089290-b27c-4153-957e-52450d029d6c`

第一根失败后只读观察有222entries / 169files，无readonly/reparse，也没有当时可见的
精确profile argv owner；这不能证明失败瞬间没有文件/映像句柄或外部锁。原supervisor
会核Job ActiveProcesses=0并join原Child，仍须在实际失败点补文件名/raw IO error、
原Job/Child退出与句柄收束witness。不能推定杀软、权限配置或与seed rename同一根因。
旧两根managed首败和本批seed fixture继续保留，不按临时目录或无人运行判定可删除。

## 收尾与下一批

原owners实际退出后，新`existing-reservation-cache-1`绑定source21/build22，已完成
inspection→逐项读取精确plan→Execute。首次Execute在任何删除前被Rust owner预检
安全拒绝；随后只读核验没有Rust owner，原plan尚无before/report、尚未执行，才对原精确
pairs/source/current/artifacts全部重新核验并执行，未杀任何其它进程。已完成清单不再复用。

最终`existing-reservation-cache-1-plan.json`、`-before.json`、`-cleanup.json`均保留。
移除15组旧App/Core incremental generation / 1589files，逻辑6867402367bytes，
H实际空闲观察增加6406660096bytes，约5.97GiB；不把hardlink逻辑大小冒称实际回收。
最新generation/source3255文件/默认App/Core executables/seed/CFT140/fix.patch全部
hash复验不变。源码、依赖、dirty/untracked、全部日志与上述三个新失败根/旧首败根均保留。
working/incomplete、跨variant、未知owner或runtime仍deferred，不将整`target`或其它工作区删除。
删除的仅一次性旧编译缓存，不在回收站；需要时由原Cargo构建重新生成，不需重新下载源码。

先查明本批native目录清理及seed rename首败：绑定原IO失败路径/原owner实际退出/
文件或映像句柄，受控反例与修复后复跑宽CU，不能靠sleep、固定重试或放宽断言销项。
真实discovery进行中撤销仍未覆盖，不把“load已经完成后的choose撤销”当discovery race。
之后connection/profile/lease、App/agent/ACP replacement、Stop/取消/Drop、真实Pause→
原Host恢复；达到安装/原任务UX门槛才开descriptor并从真正生产入口复跑。
完整E5、其它U04、完整U05/U06/U07–U09/D00–D31、四surface、三平台/双Mac架构、
所有输入/恢复、签名安装升级回滚、真人Grok与同最终候选12h仍开放。
旧自然21530ms close/Node TypeText unknown/App worker_timeout等根因与本批seed rename
首败均不能被局部绿替代销项。完整目标未达到可交付状态。

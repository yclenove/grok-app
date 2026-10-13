# ExistingTab：浏览器退出、原配置重开及配置替换

状态 active / partial — not releasable；单 Codex writer，不启动 Grok/子代理，不提交发布。
接[原 Host 暂停/正文恢复检查点](2026-10-04-computer-use-task-consent-u04-existing-host-recovery-checkpoint.md)。
原 dirty/untracked/fix.patch、旧失败现场和完整交付范围保留。

## 本批边界

本批补的是联合 native 验收与探针初始化/退出判断，没有改变生产授权、租约、
ExistingTab 输入能力或恢复逻辑。所有新增 Rust 进程诊断仅编译在 Windows test 中，
原 `ProbeResources`/`ExtensionPeer` 仍持有原 Child/Job/profile 并负责最终退出和清理。
模型工具没有新增批准、恢复、进程查询或配对入口。

三项仍使用实际首次 session MCP request、原 App picker/owned approval/ACP publication，
生成新 TaskGrant；不借 legacy grant。原 App/原 MCP 子进程跨浏览器退出保留。
实际 source MV3、认证 HTTP、Chrome 的浏览器/文档身份与自建页面后置条件均参与验证。
Host 手势、App session 和 ACP peer 仍 synthetic；程序安装扩展不是真人 toolbar，
test harness 不是安装态主窗，也没有真实 Grok 任务证据。

## 物理退出与租约

关闭原 Native Chrome 后，原 popup/A/B/C 必须全部关闭；同时查询原 Windows Job 的
ActiveProcesses、成员和实际镜像类别。只允许原 peer/原 MCP 的两 Node 身份，以及
退出前已存在的原 conhost；任何 Chrome/crashpad/未知成员均不能算浏览器已退出。
不按任意 PID 杀进程，不借一个“页面已关闭”回复声称浏览器已退出。

初版误要求总数为 2。source35 首败保留；source36 只补诊断，实际剩 4 成员。
source37 精确镜像见证确认是两 Node 与两 conhost，而 Chrome 已全部退出。
改为核对原成员身份，原 owner 的最终 teardown 合同不变。

等待 production HTTP maintenance 消费原 30 秒单调租约，轮询只读 stored-state witness。
没有改时钟、续租周期、token 或连接 registry，也没有主动发送 disconnect。
保持原 private RPC 18 秒/Host 控制 20 秒及各业务期限；等待自然租约时每次回执独立完成。

## 重开与替换

重复 `--load-extension` 的 persistent-context 启动不能用作“扩展安装一次后正常重启”证据。
source37 的 startup 轮换等待未完成，失败根保留。现在复用原 `OwnedCdpBrowser`：
首次仅对新隔离 profile 调实际 Extensions.loadUnpacked；原 profile 重开不重新安装，
由实际 onStartup 轮换 BrowserSession。Playwright 只连接这一 owned 浏览器。
新浏览器/连接/文档绝不替换旧 TaskGrant 的绑定。

source38 在初次确认拒绝检查失败；该探针不再依赖英文本，改查 source popup 的
`data-state=pairingRejected/paired`，仍验证原 Host 未确认时不能配对。
新 profile 初次启动曾在初始化阶段失败，保留 source39 现场；后补固定 API readiness
及阶段见证。source41 实际观察 execute/capture/trustedStorage 从 false 到 true，
但这不能唯一证明 source39 未分类 Error 的根因，原首败保持开放。

新增 profile 只建在原 guard 注册的 `profile/replacement-profile`，不建无 owner 的 sibling。
失败保留整个原根；成功最后由原 guard 清理。无日常浏览器/用户配置读取或 Cookie 导出。

| 场景 | 新浏览器条件 | 编号条件 |
| --- | --- | --- |
| active-browser-reopen | 原 disk profile，已安装扩展重开 | 不要求编号相同；实际 native startup、previousId 和新文档必须成立 |
| active-profile-replacement | 全新 owned profile，source 扩展新安装 | 自然随机编号，不能假定从 1 起或自然复用 |
| active-profile-tab-id-collision | 全新 owned profile，仅预置非秘密 Local State 计数器 | 浏览器实际分配的 active A 必须复用旧 A 编号；不伪造 tabs API/Host proof |

Chromium 140 的 SessionIdGenerator 首次取随机值，持久化
`session_id_generator_last_value`，启动加 50；见
[140 源码](https://github.com/chromium/chromium/blob/140.0.7339.207/components/sessions/core/session_id_generator.cc)。
source40/41 对自然新 profile 要求重用编号的断言是测试假设错误，失败原样保留。
自然替换与受控 collision 分开验收，不能把计数器预置称为普通用户配置的自然行为。

collision 仅对刚创建、校验过的空 profile 以 wx 写一个计数器 key，不读取/复制原 Local State，
不修改权限、BrowserSession、document、配对凭据或时间。最多创建 24 个 owned fixture 页面
等待真实计数器到旧 A 编号；这些 padding 页不分享，且正文/input 派发必须为零。
这不是模型动作重试。超出次数或跳过目标编号直接失败，不延长业务期限。

## 每项的实际断言

三项各 12 checks，保持原 full key/run 与一次原 ACP 续行：

- 批准前 model 正文/截图/input 为零；A/B 选中，C 不获任务权限。
- 原模型观察得到 fresh refs，A/B/C 无输入；浏览器真正退出，原 peer/MCP 保留。
- 原租约自然退休、Share 数为 0；旧正文/显式截图/旧 ref click/切 B 均拒绝。
- 新浏览器身份、新文档与未配对状态独立确认；原配置检查 previousId，新 profile 为 null。
- 新 source Pair 后、新 A/B/C Share 后再次拒绝旧 grant；新浏览器正文/截图/input 派发仍为 0。
- Stop 原 run 到 Stopped，不撤销新连接、3 Share 或新页面；padding 页也无副作用。

`lifecycle-authority.grants=1` 在 Stop 前是原历史 TaskGrant 仍在 registry 的见证，
不是新权限已发布或旧页面仍可执行；所有实际旧读写必须独立拒绝。continuations 始终 1。

## 当前候选与验证

source42：3266 文件，`source-files-42.sha256` SHA256：
`430f2c82b3a76d68cf2f2bca9d6f8516c502d463dd180ab44ef15882689b863e`。
默认 App build42 no-run 实际 exit0，CI Windows manifest 已嵌入。
后续 source44/build44 已覆盖同路径 App binary，本节 source42 hashes 和结果为历史冻结，
不能以旧 hashes 声称当前 binary 可回放；当前见[批准前连接替换检查点](2026-10-04-computer-use-task-consent-u04-existing-preapproval-connection-checkpoint.md)。
`src-tauri/target/debug/deps/grok_app_lib-63e3432e557be5b6.exe` SHA256：
`c19f2cafcc4742841f258c2e52d559e2b0b9c6fd79af6b5605392dee5ccdd36c`。
旧同路径 App build 已覆盖，旧 hash 不能称当前可回放。

source39 原配置重开 focused：1 passed/12 checks/57.08s，lease 29325ms。
它是 source39 的实际结果，不借作 source42 最终矩阵结论。
source42 `existing-profile-natural-and-collision-1` 实际 2 passed/exit0，115.61s。
collision 原/新 A/B/C 均为 1369501597/1369501598/1369501599，12 padding 页，
lease 29772ms；自然新 profile 无 padding，编号实际不同。

source42 App default/probe 两 strict Clippy、Node177/MCP38、fmt/quality/diff 已实际 exit0，
App/Workbench 仍 13573 行。Core/前端未改，本批未全量重跑，不挪用上批测试数字。
source42 `existing-browser-replacement-native-matrix-1` 实际 19 passed/0 failed/exit0，
496.89s，包含当前原配置重开、自然新 profile、受控编号复用三项与全部原联合场景。
宽 App CU 实际 **291 passed / 2 failed / 20 manual ignored，132.72s，exit101**；
不能写成全绿。两项业务断言完成，随后原 `remove_fixture_root/remove_dir_all` 在
`managed_task_tests.rs:201` 报 Windows os error5（PermissionDenied）：

- `managed_native_publication_pins_original_session_and_request_transport` 的 session-replaced 分支，
  root `grok-cu-managed-task-a597f672-c061-49e2-8f07-5e1dc69a110a`。
- `managed_owned_session_replacement_at_publication_rolls_back_real_storage`，
  root `grok-cu-managed-task-8b7e5f57-c4fc-478f-8dfc-0b6f28e218a8`。

两处原 supervisor witness 均为 Job ActiveProcesses=0、原 Child handle wait=0、原
Child status=1。第一处紧接失败的 DELETE access snapshot 在 `.chrome-exec/chrome-win`
及其 `v8_context_snapshot.bin` 捕到 error32；第二处剩余233 entries，但事后 probe 未捕到
DELETE access 错误。第一处剩余215 entries。不能据此断言未知锁的 owner，或将第二处
归因于同一个锁。两根现场均原样保留，完整日志为
`existing-browser-replacement-wide-computer-use-1.txt`。

之后独立运行 ACP **178 passed / exit0**、routing **38 passed / exit0**，未被宽 CU
失败跳过；Verify42 实际复核3266源文件不变。两个只读 Restart Manager/DELETE access
复查均无当前 owner/错误，没有 ReadOnly/reparse 属性；这是后置 snapshot，不能证明
失败瞬间没有瞬态句柄或 filter driver，更不是根因修复。对应记录为
`existing-browser-replacement-failure-locks-native-1.json` 和 `...-owned-1.json`。
未重跑宽 CU 刷绿，未延长期限或跳过失败。

证据：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。
首败从 `existing-browser-reopen-first-1` 至
`existing-profile-replacement-owned-tab-diagnostic-1` 的日志、原 App root 和
pairing-owned profile/joint-failure.json 全部保留，只读 curated 诊断，不读取其它 profile DB。

本批失败根均在 `C:/Users/Administrator/AppData/Local/Temp/`，表中为完整叶目录：

| Source | App/store root | Browser root |
| --- | --- | --- |
| 35 | grok-cu-managed-task-bea032b3-efd7-4b73-95c0-4a3a7a96fbde | grok-cu-pairing-owned-1d3b0e2a-e19a-4fe6-828a-6ce78562bd3f |
| 36 | grok-cu-managed-task-0b9a28a9-5834-456b-abb1-662ae950c38b | grok-cu-pairing-owned-07771759-b77b-47d6-8ab4-eab3cefd1ec4 |
| 37 | grok-cu-managed-task-b5329dac-8989-4e1c-9dcd-30a6fce7678b | grok-cu-pairing-owned-d17eae7d-3817-403f-a6a8-53cc72fb3968 |
| 38 | grok-cu-managed-task-9b0e3ab5-1895-4bda-baf1-a2322c0c8391 | grok-cu-pairing-owned-22131199-9ccd-436b-9fb4-0072691bd2b4 |
| 39 | grok-cu-managed-task-3664d1fa-d8f0-422e-a982-43a989a6cfdb | grok-cu-pairing-owned-96599076-05a2-4610-a7ef-cffd86f8f3ca |
| 40 | grok-cu-managed-task-9e37b1c9-0d05-46f2-a43a-0cf66b23ed8f | grok-cu-pairing-owned-245ac0fa-9d21-49d3-8bb2-2ee7bd561033 |
| 41 | grok-cu-managed-task-cbd7509f-ecab-40c0-83fc-f53ee7d667f9 | grok-cu-pairing-owned-dab2f264-cf5b-4393-bfe6-649a20b7005e |

这些测试失败不会因后续通过而消失；已证实的测试假设更正与未唯一归因的初始化首败分别保留。

## 清理与下一批

原 build/test/browser owners 退出后，新 `existing-browser-replacement-cache-1` 已完成
inspection、全部精确 targets/preserved hashes 回读及 Execute；清4组旧 App incremental
generation，逻辑4219052580 bytes（3.93GiB），H空闲实际观察增加2918563840 bytes
（2.72GiB）。current cache/App/Core/CFT140/seed/fix.patch 和3266源文件复验不变，
所有失败根与证据保留。缓存不在回收站，可由原 Cargo 构建再生成；不确定的 working/
跨variant/失败runtime保留。已执行身份不可重用。

继续批准前 choose/reservation/publication 的实际连接替换及真实 lease 竞争，
App process/agent/ACP replacement、Stop/取消/Drop、恢复并发和迟到结果。
原 reservation/publication 准备期限仅 15 秒，不能硬等 30 秒或扩业务预算冒充 lease 场景。
旧 B、IO error32/未知 owner、seed rename 与 startup 首败仍开放，未复现不销项。
descriptor 仍关闭，raw parser 仍 Desktop-only；先完成安装/原任务 UX 门槛再开放并从产品入口复跑。
完整 E5、U04/U05/U06/U07–U09、D00–D31、四 surfaces、Windows/macOS/Linux/双 Mac
架构、全部输入/恢复、签名安装升级回滚、真人 Grok 与最终同候选 12h 仍须完成。

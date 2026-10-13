# ExistingTab：原 Host 暂停、正文恢复与一次续行

状态 active / partial — not releasable；单 Codex writer，不启动 Grok/子代理，不提交发布。
接[连接替换/真实租约及启动首败](2026-10-04-computer-use-task-consent-u04-existing-active-connection-checkpoint.md)。
原 dirty/untracked/fix.patch 与旧失败现场保留，本批包含一个生产恢复修复。

## 实际问题及修复

已有标签页的 normal `computer_observe(screenshot:false)` 能取得新正文和元素引用，
但原 Host 恢复固定请求 `screenshot:true`。在已分享但没有 activeTab 截图权限的原页面上，
Pause 已完成，Continue 却因浏览器截图权限拒绝而无法发布恢复。

本批用原 production source MV3、认证 HTTP、首次 session MCP request、原 picker/owned
approval/App ACP publication 创建新 TaskGrant，选择 A/B、未选 C。随后调用实际
SessionManager 的原地址 Pause/Resume，不借 legacy grant，不直接修改暂停或授权字段。
Host 手势、App session 和 ACP peer 仍 synthetic，测试使用自建 loopback 页面及 owned CFT140。

source31/build32 首次失败：15.30s、8 checks，固定 stage 为
`original-host-resume-fresh-semantic-observation-once`，detail=`original-manager-resume`。
source32/build33 仅补原截图 API 的计数/权限拒绝分类，不更改异常或结果，再次失败：
16.23s、8 checks；实际 `captures=1/capturePermissionDenials=1`，source worker 启动正常，
配对仍在。两次原失败根和日志保留，诊断没有输出异常正文、凭据或任意 CDP targets。

生产修复仅改变 Host recovery 的采集选项：ExistingTab 主动请求新的正文/元素引用，
Desktop 继续请求截图。不是截图失败后自动降级或复用旧观察。原 scope 全组复验、
owner/pin、独占准入、15 秒准备期限及最终原 App/ACP publication 均保留。
显式截图路径仍必须获得截图权限，ExistingTab 的 semantic-only input 能力未扩大；
本批恢复产生的观察没有图像，也不启用坐标输入。

## 原任务联合验收

新增 `active-host-resume` 场景，12 checks：

- 原首次 request/off/pair/confirm/share/picker/TaskGrant，批准前无 model 读写。
- 原 Host Pause 后 grant 暂停；正文/截图/旧 ref click/切 B 全拒绝，source 派发计数不增加。
- 原 Host Continue 仅采集一次新正文，保持同 full key/run/原 A/B scope，发出一个新 continuation。
- 同旧地址重复 Continue 被拒绝，不再次采集或产生 continuation；原 ACP 实际续行一次，重复拒绝。
- 旧 refs 拒绝；新的 MCP 正文与元素 click 生效，重复 actionId 不再次点击；坐标输入无派发。
- 恢复后能切所选 B，未选 C 始终没有正文/input，原 A 只发生一次实际点击。
- 最终 Stop 到 Stopped，原连接/3 Share/A/B/C 用户 tab 保留，无 pending continuation。

`existing-host-recovery-fixed-1` 当前 source34/build34 实际 1 passed/exit0，17.81s，12 checks。
没有真人 toolbar、安装态主窗或真实 Grok 证据；截图 happy path 不由正文恢复通过推断。

## 当前候选及验证

source34：3262 文件，`source-files-34.sha256` SHA256：
`2e5aaebf19c9873a73b52ffe5ef5b69454d9c4ad6ac1ca64196c06298bc9326e`。
默认 App build34 no-run 实际 exit0，CI Windows manifest 已嵌入；executable 为
`src-tauri/target/debug/deps/grok_app_lib-63e3432e557be5b6.exe`，SHA256：
`2b8e59c64e3cc3c6952f6135a9b43c0c4f29986caca3086430a9e896b81c271e`。
build31/32/33 同路径已覆盖，旧 hash 不能声称当前可回放。

Core focused 的第一项失败来自测试错误要求 `visual observation` 文案：实际 ExistingTab
在更早的 semantic-only capability gate 拒绝坐标；生产拒绝行为正确。保留首日志，
精确核对原 gate 后修正为 `unsupported action Click ...; not dispatched`。
第二项 focused 实际 1 passed/exit0，同时检查无图像、fresh refs、旧 refs 拒绝及零坐标派发。
Core default all-targets 实际 1080 passed/0 failed，14 个原日志集合；test-support 为
1096 passed/0 failed，17 个集合。重叠集合与 focused 没有重复相加。
当前完整 `existing-host-recovery-native-matrix-1` 实际16 passed/0 failed/exit0，331.90s；
新增恢复场景的12checks在整矩阵再次通过，真实lease witness为29523ms。
原 source29 startup 首败本轮没有复现；本轮全绿不能关闭原初始化首败。
宽 App CU293 passed/17 manual ignored（125.34s）、ACP178/routing38，均实际exit0。
17 ignored 是另跑的16 native场景与原10分钟request期限，不是17项通过。
四 strict（Core default/test-support、App default/probe）、Node177/MCP38、
fmt34/quality34/diff34及source34 Verify3262实际exit0，App/Workbench仍13573行。
前端没有变化，本批未全量重跑，不能移用旧前端数字。

证据目录：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。

## 保留的本批失败根

首次 App/store：
`C:/Users/Administrator/AppData/Local/Temp/grok-cu-managed-task-69b7af9e-17c9-4f53-a067-3a0b3121cc8f`。
首次 browser：
`C:/Users/Administrator/AppData/Local/Temp/grok-cu-pairing-owned-296c84d7-cc0c-4477-91cd-b95ae7cc1056`。
诊断 App/store：
`C:/Users/Administrator/AppData/Local/Temp/grok-cu-managed-task-c4a42617-3e98-450d-aba7-e465493511a8`。
诊断 browser：
`C:/Users/Administrator/AppData/Local/Temp/grok-cu-pairing-owned-9684dd26-34a3-4f35-b3f6-f3262ec2391e`。
只读取 curated `joint-failure.json`，没有读取其它 profile 数据库/配对凭据。

## 清理与完整剩余范围

本批原 owners 全部退出后，新 `existing-host-recovery-cache-1` 绑定source34/build34；
inspection→读全部15组old/current精确路径、体积与保留项→Execute实际exit0。
清15组旧App/Core incremental generation，逻辑6931234985bytes（约6.46GiB），
H空闲实际观察增加3806511104bytes（约3.55GiB）；没有用包含hardlinks的逻辑体积
充作实际释放。source3262/current caches/App/Core/CFT140/seed/fix.patch清后哈希均不变。
原失败根、依赖、不完整working变体及未证明废弃的跨variant/runtime保留，未杀其它进程。
旧缓存未进回收站，可由原Cargo构建再生成。已执行身份不可复用，每批按第9条收尾。
最后文档修改后 `git diff --check` 复验；源码与用户原改动保留，本批未提交发布。

旧 B 五动作首败、native DELETE error32/未知 owner、seed rename 及 source29 初始化首败
仍未证明根因；本批恢复通过不能关闭它们。ExistingTab descriptor 仍关闭，raw parser
仍 Desktop-only。安装/原任务 UX 门槛之后才能开 descriptor 并从产品入口复跑。

继续真实 browser/profile 退出替换、批准前 choose/reservation/publication 的新连接/真实
lease 竞争、App process/agent/ACP replacement、Stop/取消/Drop 和恢复并发/迟到结果。
本批无并发恢复/原 owner 退出的完整 E2E 证据，不用一个成功场景替代完整矩阵。
完整 E5、其它 U04/U05/U06/U07–U09、D00–D31、四 surfaces、Windows/macOS/Linux/双 Mac
架构、全部输入恢复、签名安装升级回滚、真人 Grok 及最终同候选12h 仍须完成。

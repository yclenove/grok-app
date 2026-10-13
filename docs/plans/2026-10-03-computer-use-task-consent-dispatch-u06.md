# Grok U06：撤权、重连、退出与故障闭环

状态：**draft / not completed**。Codex已接手；复核U01–U05相关合同和实际接口后继续本批。
单Codex writer，不启动Grok、子代理或第二writer，不自行提交发布；完整U/D及三平台范围不变。

2026-10-04 增量见[dispatcher/owned生命周期checkpoint](2026-10-04-computer-use-dispatch-owned-lifecycle-checkpoint.md)：
原dispatcher取消join丢handle/后次join吞失败两反例先红后绿；owned native feature-off、
原ACP stdin queued-stop及真实10分钟request TTL通过。默认App CU267通过/1手动ignored，
该ignored另显式1通过（601.93s）；ACP167/routing38/strict/Node28/fmt/quality通过。
这不是下方完整事件/阶段矩阵或自动dispatcher/真实App/Grok/安装态E2E完成；范围不缩减。

## 目标与入口

把已完成的按需授权、范围和一次继续贯穿所有真实生命周期。request-only会话无run时
也必须能取消/退休，Stop仍有界；撤权后零新派发，原资源清理到真实终态再释放占用。
前端与MCP目录如实区分请求通道、执行权限与cleanup，不用一个enabled或absent代表全部。

先读U06总体计划第4.4节、前批checkpoint/ADR与当前源；保留原首败及所有dirty文件。
已核入口，开工仍用rg回读当前符号：

- `computer_use/{sessions,feature_lifecycle,feature,shutdown}.rs`
- `session_manager/computer_use.rs`与`computer_use/lifecycle.rs`
- `session_manager/{connect,control,events,events_bg,turn,process}.rs`
- `commands/{computer_use,settings,account}.rs`及实际account/profile/root切换入口
- core `task_consent/`或当前领域拆分、`session_grants.rs`、`broker/{lifecycle,resume}.rs`
- current card/controller/provider、browser owned lifecycle、ExistingTab与WebView实例退休

原`fence_process_exit_session`会据desired.run_id及catalog settled提前return；
`sessions::tracked_sessions/clear`来自旧SessionGrants；原shutdown只枚举旧tracked。
不能以旧run不存在、目录absent或旧tracked空表推断新request owner/prepare/票据也不存在。
前批若已修复不重复改；本批对最终生产入口补无run和全owner覆盖的证据。

## 1. 一个真实生命周期矩阵

按下表逐行建立manifest，把实现与验证分开。表中范围记忆永远不是可执行授权。

| 事件 | 请求/执行权威结果 | 原物理资源与下一步 |
| --- | --- | --- |
| User Stop / completed / revoke | 请求、preparation、grant、续行终态退休 | 同步fence；owned cleanup/lease到terminal；新任务重新申请 |
| Feature off | 旧请求取消、旧批准/准备/票据失效；epoch前进 | 先fence再persist；晚批准不可重新启用；申请通道与执行分离 |
| Deny / cancel / request expiry | 本请求无权终态；增量失败保留原范围 | 清理仅本准备资源；NoAuthority一次且不触发执行 |
| Task idle expiry | 不再允许旧任务操作/claim | active action/cleanup不误算idle；真正到期交精确cleanup |
| Pause / physical takeover / lock | 停派发，旧refs无效；只保留已批准范围记忆 | 原action取消、原owner确认idle后用户一次Continue；不抢焦点 |
| Compact / model change / ACP reconnect | 旧execution/decision/transport incarnation退休 | 同App非执行范围可按合同保留；新agent显式用户继续 |
| Account / provider / data-root / remote change | 旧owner及任务权限全退休，不跨身份收养 | drain前fence；cleanup持原endpoint；新local请求重新登记 |
| Fork / App restart / session deletion | 新App任务不继承活grant/token/ticket | 原owner退出；删除先成功清理再忘账；未知资源保留恢复记录 |
| Target death / platform revoke / view-tab replacement | 原短期身份拒绝，不找同名替换 | 精确原资源收束；必要真实权限/Share或增量请求 |
| Timeout / response disconnect | 真实not_started或unknown分型 | 已派发副作用不重放；保留原admission和结果未知状态 |

普通session focus切换、React remount、preview隐藏不是用户Stop；不把另一个chat的任务
无故取消，也不从focus/cwd/CLI agent id猜旧owner。local interactive资格沿用真实Host分类，
SSH/RemoteIM/定时/aux/prewarm不能取得本机执行权或借共享ACP进程继承另一tenant的grant。

## 2. Fence与cleanup所有权

同步fence在所有延迟I/O/锁等待/目录RPC之前发生，并传到最终action admission。
所有run/preparation/worker/context/view/portal/clipboard都有确切原owner与generation；
不得前缀扫描或按process名/端口段清理，也不查询“现在session的endpoint”代替原资源。
Stop准备时run尚不存在仍能取消；迟到候选必须原owner abort/join，不能发布给replacement。

cleanup handoff有界、可回读、失败可显式重试；同票据最多一个consumer，旧cleanup不能
clear新owner/grant/catalog状态。原run in_flight/physical terminal未知时占用保留。
拿到ticket或grantCount=0不算物理收尾，线程完成也不自动证明远端worker/context已退出。
Stop回执不等待慢abort/文件/网络；复用现有协调器，不能每次请求生无限后台线程。
全局/per-owner任务、准备、decision、cleanup和rate账本同时有界；terminal可安全回收，
不可一生只允许固定32个session或丢有效continuation换“内存有界”。保留独立Stop准入。

复核U02实际Host到期watcher：无人轮询/卡片卸载时request TTL及task idle仍正确退休，
所有错误返回路径仍交接精确到期run，不依赖后续成功工具调用才清理。core准入直接验
deadline；timer不授予权限、不续命。原watcher退出/重建不跨App根或替换owner收养票据。

## 3. Catalog、ACP与UI边界

request-only entry存在不代表execution授权；Stop/feature-off后local会话是否保留请求entry
按U02已接收合同处理。真正进程退出/重连退休原transport；不能用旧catalog重新批准。
整表reconcile与base revision保留无关MCP，晚目录回包不能标新generation applied。
共享ACP多个tenant只退休原tenant；实际process退出时覆盖前台/后台/parked所有原owner。

U02票据与发送账本各阶段再次验证，retirement胜出不发送；发送后unknown不重试副作用。
agent绑定前bootstrap可建请求入口，但None agent不能commit/执行，不收养之前的grant。
card/provider事件按owner/task/request/revision排序，晚事件/旧status不覆盖新状态。
stale/error仍保留原任务Stop；隐藏preview停采，迟到帧不闪回或替换模型snapshot。
任务等待批准的end_turn不触发completed；用户明确新任务才换Host task边界。

## 4. Feature首次启用与存储竞争

U02/U03已有首用协调事务，此处覆盖更深竞争：persist失败/调用方取消、旧rollback与
新settings更新交错、off→on→off、prepare前后Stop以及运行时storage失败。
settings存储、App flag、Broker epoch/gate准确回读；任何偏好bool不授予TaskGrant。
off先fence，即使persist失败也无旧执行窗口；迟到enable不能覆盖用户off。
只回滚本事务仍拥有的CU字段并推进epoch，不能保存旧整个settings覆盖其它偏好。
测试不能仅观察一个Mutex bool；读实际Host gate/transport拒绝、存储字段和原cleanup。

## 5. 真实负向、故障回归与验收

每个事件至少测pending无run、preparing、committed/busy及cleanupPending这四个受影响
阶段，按真实依赖标不适用。先建立会失败的实际入口反例再修复，不使用常量true断言。
代表性竞争用barrier定位，不靠sleep赌线程排序；慢owned action/abort/query有明确释放/join。
合法action/observe/Wait身份做拒绝回归，不靠无效JSON/schema失败冒充权限撤销。

独立计数/后置条件必须核：0 unauthorizedRead/input/wrongTarget/postFenceDispatch/
duplicateContinuation/unknownReplay/crossSessionGrant/wrongOwnerCleanup。
检查replacement在旧cleanup后仍能正常执行，并核旧owner lease/profile/tab/listener/key
实际terminal，borrowed tab不被关闭。真实managed worker与loopback/MCP断连覆盖not_started
和派发后unknown；mock cancellation=true不替代原进程/worker idle证据。

S20–S33相关scene、原session/catalog/control/lifecycle/IPC/identity负向、严格协议/golden、
controller/provider/Stop/stale tests受影响全部验证。App Windows libtest按现CI post-link
mt.exe方案，不改机器依赖或增加冲突MANIFESTINPUT；未进入断言不是pass。
fmt/strict Clippy、typecheck/lint/build、15locale键/设置catalog/壳大小按改动范围跑。
不重复未变U00 oracle刷时长；不重写旧失败/独立snapshot来迎合新实现。

已有Wayland/TCC/toolbar外部缺口逐行关联D任务；不在本批重做无关GNOME大实验，也不
自动点系统/App/Share权限。缺某平台真人只阻对应验收，ready的实现/否定路径继续完成。
日志不记录token/Cookie/输入正文/原截图/敏感URL；secret canary由synthetic fixture给出。

交`2026-10-03-computer-use-task-consent-u06-checkpoint.md`：逐事件/阶段矩阵、精确入口、
source/runtime hash、实际命令/count/exit、首败、owned terminal/join与未验项。
提供U07可一次运行的候选/准备步骤，不以本批fixture绿宣布完整CU完成。
Codex接收后派U07，后续U08/U09及原D00–D31继续保留。

不commit/push/PR/merge/tag/release/reset/restore/clean/stash/rebase；不动凭据、共享~/.grok、
代理/VPN/用户profile/日常进程/其它worktree；只清理已核精确原身份的owned测试资源。

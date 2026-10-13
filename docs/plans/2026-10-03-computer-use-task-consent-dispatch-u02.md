# Grok U02：工具预连接、CU 专用 CLI 回执与一次续行

状态：**Codex implementation in progress**。U01 核心已接收，见 `2026-10-03-computer-use-task-consent-u01-codex-finish-checkpoint.md`。
用户已要求 Codex 接手实现；不再派发 Grok。按接收后的 U01 typed 接口接线，不能虚构已存在 API。
单 Codex writer；无提交/push/PR/发布、无真实授权自动点击。

当前自动化实现/验证见 `2026-10-03-computer-use-task-consent-u02-checkpoint.md`。
Host事务、派发与首用回滚已接线；normal CLI无害闭环已通过，production Bound/聊天卡仍归U04/U03。
不能以核心880项或App173项绿宣布U02真人流程或整体CU交付。

目标：用户不用先去面板授权才有工具；模型按需申请；Host决定后正确会话只继续一次；
已批准任务范围内CU工具不再逐次出现同样的CLI确认。shell/文件/其它MCP的现有权限规则保留。
这是接线批次，聊天卡是U03、真实范围resolver是U04；不能据U02合成批准宣布体验已交付。

## A. 先红：当前目录为什么缺工具

当前`computer_use/inject.rs::mcp_acp_entry_with`需feature/session enabled、已有run与
authorized_target；`session_manager/computer_use.rs::build_catalog`只在desired.run_id存在时
插入entry。ACP `open_session_at`先用generic extensions builder，而该builder剔除CU项。
写一条真实session/new目录回归：local interactive、feature-off、无run时应有Host申请entry，
其它已有MCP/official-aux配置按原产品策略保留；旧路径失败输出保留。零adapter/helper/capture/input。

入口按实际路径核实，不向几个千行文件追加大功能块：

- `src-tauri/src/computer_use/mod.rs`、`ipc.rs`、`inject.rs`、`runtime.rs`、`feature_lifecycle.rs`
- `src-tauri/src/acp_client.rs::open_session_at`，connect的warm/cold/new/load/fork路径
- `src-tauri/src/session_manager/computer_use.rs`及`computer_use/lifecycle.rs`
- `src-tauri/src/session_manager/computer_use/tests/catalog.rs`、`tests/lifecycle.rs`

## B. 只预连接申请transport

1. `ensure_request_endpoint`复用同一Broker权威和loopback IPC，只建立请求能力；
   不调用现有会顺带attach managed browser的ensure_host_runtime，不构建native helper、
   不读取窗口/title/DOM/UIA/AX/tab/截图/剪贴板。执行资源只在正确Host批准prepare中启动。
2. 把真正App session的预留owner明确传入connect，不从当前focus/cwd/CLI id猜App id。
   session/new/load/fork完成绑定当前agent前无可执行grant；绑后token仍稳定。
   每个handshake/process incarnation有owner，失败/Stop/晚到回包只退休原owner。
3. 请求entry与无权执行catalog可稳定留在local session，feature-off/Stop不盲目销毁申请通道；
   execution/旧continuation必须撤销。真正session退出/reconnect则退休原transport。
4. `supports_local_computer_use`继续限定local child、非SSH、非empty/aux；RemoteIM/定时
   不得因为共用builder拿到凭据。user自配同名entry仍不能冒充Host登记。
   2026-10-03独立回读发现该函数实际仅检查empty/ssh/child；WSL launch有child、
   ssh_alias=None，也会命中，而同一AcpClient已有owns_local_process_tree=false。
   预连接必须使用真实transport/owned-native证明，不能把“child存在”当本机native。
   native正向，WSL/SSH/TCP（即使localhost）/aux/RemoteIM/scheduler逐项反向：
   零CU credential注入、零request-owner登记、零adapter启动，其它MCP保持原策略。
   这是下一批的源码事实与回归要求，尚未执行负例，不能宣称已验证凭据外泄或已修复。
5. base builder超时/failure不得默默吞掉CU申请项，也不能拖延connect无限等待；
   对runtime缺失给准确unavailable/retry路径，不先启动浏览器下载或全套native运行时。
   保留base revision/desired/applied generation及cleanupPending准确语义。
6. 复用现有整表reconcile锁，entry替换保留无关MCP。Stop/扩展prefs更新/连接重建交错不丢表。
   不能把“清掉执行grant”与“整个目录absent”当同一个状态；旧安全谓词继续测。
7. 冻结供U03调用的首用Host事务：当前`feature_lifecycle.rs::persist_feature_transition`
   同步settings存储、`feature.rs`进程开关、Broker gate，但会调用ensure_host_runtime；
   原入口不能直接用于未批准bootstrap。U01只改Broker状态不是三处同步成功的证明。
   首用用户决策按当前reservation/epoch协调persist→execution prepare→final commit；
   persistence失败无执行，Stop/feature-off与用户更新胜出后晚事务不得重新启用。
   rollback只处理本事务仍拥有的CU状态，epoch只前进；不恢复旧整个settings快照覆盖
   期间其它偏好修改。准确区分“功能偏好启用”与“任务已批准”，任一bool不替代TaskGrant。
   将Host命令边界/API与测试seam写checkpoint，U03接真正卡片手势后完成首用正向链。

## C. CU 专用权限handler，放在通用策略之前

`AcpEvent::PermissionRequest.raw`保存的是完整params，结构在`raw.toolCall`内。
decode_permission_request会对tool_name做title/kind fallback；CU匹配不得用这个fallback。
foreground `events.rs`有load-replay自动allow分支；background `events_bg.rs`也有独立policy。
把CU处理抽到同一领域helper并接两路，不能只修foreground或让replay bypass。

只有实际登记的owner/agent/process/catalog generation且下面六条件全部满足才识别CU：
raw.toolCall._meta["x.ai/tool"].name==use_tool，rawInput是object、variant==UseTool，
tool_name用第一个__拆分非空server/tool，server与Host登记全等，tool是登记具体工具。
不要根据title、泛`use_tool:*`、自由toolInput、公开服务器名单独确认身份。

- `computer_request_access`只能allow-once送到无权Host gate，不写通用allow cache。
- execution工具只有已commit当前grant及该tool所需身份/能力全通过才allow-once；
  status/list/fresh observe不能要求不存在的snapshot；act/wait仍严验原观察完整身份。
- 明确识别但无权限/退休/Stop为reject-once或cancelled，不落回通用允许分支。
  load-replay的CU不能借历史权限回执重建执行。
- 只使用CLI实际提供的allow-once/reject-once选项；缺选项cancelled，不能fallback到
  always-allow/allow-always-mcp，也不能伪造一个成功选项。
- 不调用allow_tool_family/resolve_permission的通用缓存，不改permission-mode，不加
  --always-approve，不触动共享GROK_HOME；其它工具走原产品policy。

测试用保留的真实U00-R权限形状作fixture，缺meta/title-only、错误variant/server/tool、
其它MCP/shell、旧agent/replay、同名用户配置负向；读取真正响应option与adapter计数。

## D. Host 决策排队并只续行一次

依赖U01最终typed权威票据，SessionManager只持调度claim，不另建可执行grant/独立决策真相。
busy、pending_permission/plan/ask gate、连接中/历史load/Stop/cleanup均不发第二prompt。
旧FSM可能早ready；`prompt_in_flight`及真正prompt RPC结束边界不能省。

1. request立即返回pending，不把用户等待留在HTTP/CLI generic permission RPC里。
2. commit才发Continuation；deny/cancel/expire是NoAuthority通知。固定消息只含必要Host id/
   revision/下一步，不重发用户原话、不新建聊天、不靠模型每秒status查询推进。
   当前U01-R2实施中出现take_no_authority(app_session_id, owner_nonce)的typed drain接口，
   派发前以接收后的最终API为准。取出通知不授予权限；Host按原owner持有有界发送账本，
   busy时不丢弃、不按focus改路由、不反复重新生成通知。CU卡片拒绝/取消与整个聊天Stop
   区分：用户已停止的turn/退休连接不能因为收到stopped notice再自动发prompt启动。
   NoAuthority不能套用需要feature-enabled/executable-grant的Continuation验证来假称批准，
   也不能借通知重新建立grant；检查其完整原owner/task/request/revision及一次dispatch身份。
3. claim、发出前、结果入账核完整app/agent/incarnation/connection/task/request/revision/epoch。
   路由精确session（含合法background/parked状态），不把旧会话票据送到新focused会话。
4. ticket入队不提前永久消耗；发送准入原子且只一次。not_started可在仍有效时重试，
   派发后unknown保留准确账本，禁止重复prompt造成双执行。取消/退休不发旧票据。
5. core fence立即阻新派发，物理cleanup继续由原owner收尾；turn结束等用户批准不是任务完成。
6. 申请期限与task idle到期由有界Host生命周期驱动，不靠模型每秒status查询或卡片挂载
   才发现。使用已接收core的monotonic扫描/下一deadline合同，无观察者时也退休到期权限，
   交接精确cleanup并产生typed通知。扫描不枚举窗口/抓屏/刷新活动，不新建第二时钟权威；
   真实物理busy/cleanup尚未terminal不冒称idle。approve、续行准入本身仍直接校验deadline，
   不把timer先赢竞争当正确性前提；Stop/shutdown取消原watcher并收束其consumer。

有界队列、双决策事件、忙时批准、gate关闭后一次发送、两个并行session、Stop与发送竞争、
晚prompt-complete/重连/重复card remount、发送前失败/派发后unknown都检查实际prompt/effect数。

## E. 验证与交审

先针对性红→绿；core回归按接口改动范围，App session目录/permission/turn/lifecycle真实
单元与IPC联测，fmt/strict Clippy/MCP协议。App Windows libtest使用现有CI mt.exe post-link
manifest；未进断言的0xc0000139只记环境失败，不重新修改机器/依赖来假装通过。

再一条owned合成Host决定的真实normal Grok CLI/ACP/MCP无害闭环：当前版本/隔离home，
feature-off无run申请、延后决定、不悬挂HTTP、scope内固定fixture、续行一次与业务计数，
同时其它MCP仍真实请求通用确认。允许的synthetic回执明确不是真人/生产批准，
不使用外层developer writer的always-approve证明normal模式总点击数。
旧首败/原资源terminal/join保留，实际内层exit显式记录。90秒真人/业务三轮仍归U07。

交付`2026-10-03-computer-use-task-consent-u02-checkpoint.md`：每项实现/验证分开、真实
命令/hash/时序/计数、原失败、owned资源收尾、人工缺口、给U03/U04的已冻结Host事件/API。
Codex接收后先派U04生产范围绑定，再派U03完整请求卡；编号不代表跳过Host依赖。
U03完成实际首用手势联合验收。总范围U00–U09/D00–D31不缩减，不能以本批合成CLI成功收尾。

# 新登录账号、首次聊天工具连接与真实 App 申请检查点

2026-10-05；工作区 `grok-app-computer-use`，分支 `feat/computer-use-implementation`。
证据根：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。

## 已确认的进展

官方 Grok Build CLI 的 OAuth 登录成功，凭据只留在当前预览复用的
`delivery-app-98/startup-home/agent-home`。用户指定 Chrome；第二次登录
直接指定本机 Chrome 打开官方页面。登录进程 PID23232 已自然退出，exit0，
独立 auth 存在，共享 auth 在该登录尝试前后保持一致；不记录或公开凭据内容。
原默认浏览器尝试的等待进程已精确退出，失败记录保留。

使用刚登录的独立 home，真实 Grok 4.7 / low / ask 的首次连接验证 attempt3
已 exit0。初始没有 CU runtime；第一次 `open_app_session_at` 自动准备 app102
随包 private Node/MCP，并提供 `computer_request_access`。显式 MCP catalog
更新为0；模型实际调用了 `grok-computer-use__computer_request_access`。
断言 requestPermissions=1、pending=true、featureEnabled=false、grants=0、
observations=0、actions=0、nonfixtureCancelled=0；原 CLI PID7780 自然 EOF
退出0，treeKill=false，Host workers 已 join。

这项验证的 CLI/ACP/MCP 是真实执行，但 Host/FakeAdapter 仍为 synthetic。
它证明新账号认证、首次连接和请求工具调用；不证明真人批准后页面操作完成。
attempt2 使用另一个已有独立 home 也已通过；attempt1 认证失败，原失败现场保留。

## 当前真实 App

唯一任务预览为 app102，EXE/hash 与交付状态一致。启动 PID42300，独立
instance `com.grokapp.desktop.cu-app98-preview`，复用上述 home。
首次设置实际显示“账户已连接”；通过普通“继续”/“进入 Grok”导航进入聊天页，
没有重新执行登录或改用户代理。设置回读 independent、ask、CU默认off、
setupWizardCompleted=true、onboardingDone=true、authSetupDeferred=false。

在该 App 内发送固定本机验收任务：只用受管浏览器访问
`http://127.0.0.1:4761/`，保存“中文验证 你好”，点击增加计数三次并核验结果；
明确禁止终端、文件、其它 MCP、子智能体和修改代码。

- App session：`02f91a4b-2910-40a9-b129-ea4840ff08c3`。
- Agent session：`01a10b96-f72e-7e93-ae9d-f647e5e17d20`。
- 当前 UI 模型 Grok 4.7，推理强度 xhigh，请求批准。
- 真实 Grok 约5秒完成申请，App 显示“使用电脑”卡和
  “启用并允许本次任务”；范围说明为独立受管浏览器当前标签页。
- 批准前 fixture `/state`：未保存、计数0、events为空。

用户已在真实 App 点击允许，并在本聊天确认。Grok 自动续行，实际完成页面导航、
中文输入、保存、三次点击与最后观察；没有由 Codex 代填测试页面。
fixture `/state` 最终为“中文验证 你好”/3，events恰好一条save及三条increment，
分别从0增加至1/2/3；原App记录20条工具消息、toolErrors=0，最终回复结果一致。
最后一条模型消息时间2026-10-05T10:27:18.946342100Z。
这条 Windows受管浏览器/真实Grok/真人批准/中文业务任务已通过；本次未测暂停/恢复/Stop。

新增收尾缺口：模型完成且App“就绪”后，任务授权卡仍显示
“已允许，等待任务继续”，仍提供暂停和停止；执行中也曾显示这段等待文案。
需要检查Host任务状态与ACP轮次结束的映射，不能只改显示字符串来宣称任务已撤权。
后续应分别验证执行中/模型回复完成/后续轮次/用户Stop的权威状态和资源归属。

`real-app-105-result.json` 保存实际结果、原会话/日志hash与验收边界。
验收结束通过App关闭按钮退出PID42300，host_runtime.shutdown=true；
原CLI、MCP worker及受管浏览器进程均已退出。原fixture服务确认无活动请求后退出，
最终业务effects已存证；重新运行其保留的fixture-server.mjs可产生新的本机测试地址。
当前登录、会话、app102和预览runtime保留；没有留下测试窗口。

## 来源与门禁

probe source105：3320文件，manifest SHA-256
`6497692d1a776da9fed19f30c79d24ccd7cfc6efe752ad426863346f65d9aba7`。
新增的 cold-chat 场景与路径注册 helper 仅在 `computer-use-probe` 下编译；
默认产品逻辑未改，当前用户包仍为 source101 / app102，不为 probe 修改重复打包。
`cu_probe.exe` SHA-256
`4e28daa547f189d7940e1d0f5b65f79520e5efbe6498969c1625b88b1ec468df`。

source105 probe strict all-targets、fmt、quality通过；App+Workbench13573，
大文件80不增长；diff检查通过。没有为本批重新运行旧Core/App测试，不能将旧成绩
写成本批新增验证。原完整交付范围和 remaining-plan 保持，未提交/推送/发布。

证据：`cold-chat-cli-105-2.jsonl`、`cold-chat-cli-105-3.jsonl`、
`preview-oauth-105-2/status.json`、`cold-chat-probe-artifact-105.json`。

## 产物收尾

此前103批已实测回收7.88GiB。本批首次遇到活跃Rust owner时延期，未停止其它进程。
原测试owners退出后按已审查计划清理两组旧缓存：逻辑863261851 bytes，H空闲实测
增加863272960 bytes，source105与当前验证产物hash不变。

成功attempt2/3的两个一次性runtime pack也已清理：逻辑881692616 bytes，H空闲实测
增加883150848 bytes。全payload与保留seed/嵌入MCP源一致、规范化manifest经过检查，
原CLI/Host终态已确认、无精确目录owner、无reparse point。
bootstrap会替换两份嵌入MCP并补充组件/架构；不能将这种正常转换误写成原seed损坏。

本批合计逻辑1744954467 bytes，H空闲实测增加1746423808 bytes（约1.63GiB）。
缓存可由cargo重建，runtime可从app102在fresh probe重建，已删除项不在回收站。
报告：`cold-chat-cache-105-cleanup.json`、`cold-chat-runtime-105-cleanup.json`。
旧working缓存、失败attempt1现场和未知目录继续保留；当前app102、独立登录和会话、
预览runtime、源码/未提交改动、seed、依赖及所有日志/hash/验证证据保持。

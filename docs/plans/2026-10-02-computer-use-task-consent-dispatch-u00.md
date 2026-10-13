# Codex 派给 Grok 的第一批：U00 授权链事实与架构决定

用户已明确授权 Codex 通过下任务、审查、再下任务的方式驱动 Grok CLI 完成开发。
Grok 是产品 writer；本批只完成 U00，结束后等 Codex 审查再接实现，不自行越过批次。

工作区 H:/aicoding/grok-app-computer-use，分支 feat/computer-use-implementation。
基线 8d8b71c262ab222748bca32412eea884d4b892e0，重查实时 HEAD/dirty，不回退。
沿用原 Grok 会话 f7013604-7dd2-4a20-805d-626e93f4b08e 的对话，不 restore-code。
新需求覆盖旧“先研究 GNOME backend”任务优先级，不启动/重启旧 VM。

先读 AGENTS.md、CONTRIBUTING.md、相关 wiki，以及
docs/plans/2026-10-02-computer-use-task-consent-plan.md。
本次 dispatch 优先于 task-consent-prompt 中“自动继续 U01–U09”的规则：先交 U00 供 Codex 审查。

## 本批成果

1. 查当前产品会话权限、CU session MCP attach/reconcile、IPC binding、pending permission、
   busy/continue/resume 路径，写出当前真实链路与新链路的差异，引用真实路径与符号。
2. 核 CLI 1.0.46 的真实能力：normal interactive ACP 请求和 session 续行、MCP catalog、
   permission request 的 allow-once/session options、Host 现有 scope 缓存。
   不凭 help 或旧文档宣布真实往返可用。可以运行 owned isolated 子 CLI/ACP probe，
   不借当前外层 writer 的 always-approve 证明产品 normal 模式。
3. 不需要拿真实电脑权限来确认 bootstrap 协议。测试 MCP 只用固定字符串、owned fixture
   及不可执行的权限请求；通过真实 CLI/ACP transport 核 request/decision 的消息时序。
   任何 synthetic/mocked Host decision 必须标为测试数据，不能算真人批准或生产授权成功。
   不自动批准真实桌面/系统/扩展授权；不截用户桌面、不读用户窗口/标签页内容。
4. 写 ADR：任务授意和 execution identity 分层；feature-off 的无权申请入口；稳定目录；
   异步 pending + continuation；Host-only 决策；请求期限/任务完成；scope membership；
   一键 resume；generic CLI confirmation 去重；重连/Stop/feature-off 竞争。
   必须明确现有 normal 会话的 host tool:* 缓存是否会误放行其它 MCP，选择不会扩大权限的方案。
5. 列 U01/U02 的精确 API/类型改动、受影响测试、第一条纵向路径与最小失败场景。
   新协议决定要可直接实现，不留“后面再想”的任务身份、续行或 feature-off 死结。

## 输出位置

- 设计：docs/plans/2026-10-02-computer-use-task-consent-adr.md。
- 交审报告：docs/plans/2026-10-02-computer-use-task-consent-u00-checkpoint.md。
- 新唯一 evidence root 放 tools/computer-use-probe/.run/ 下；不要覆盖旧日志。
- 报告列实际命令/版本/消息时序/退出/断言、implemented vs verified、firstFailure、
  未验证的真人/原生环节、推荐下一批和精确源码入口。

本批允许新增文档与 owned probe；不要修改产品源码，Codex 审查 U00 后会下 U01 实现任务。
不要重新全量构建 App/Cargo/runtime 作为本批“工作量”，使用与假设相关的轻量探针。
若需更长时间先写短状态，实际原 handle 仍 live 时不能启动重复 writer/probe。
同一失败最多三次且有新证据；保留首败，不用成功重试掩盖问题。

## 不变约束

不 commit/push/PR/merge/tag/release；不 reset/restore/clean/stash/rebase/换分支。
不子代理，不碰其它 worktree；不改代理/VPN、日常进程、浏览器 profile、账号/Cookie/token。
不读取/输出/修改启动器 auth.json/config；可使用外部已配置的独立 GROK_HOME 启动 CLI，
不复制凭据、不修改共享 ~/.grok。fixture 不要偷偷继承外部 MCP。
不自动点击 UAC/TCC/portal/firewall、App CU grant 或 browser Share。
default off、Host-only 批准、scope 不跨会话、old refs 拒绝、unknown 不重放、Stop fence 保留。
不要为了通过探针全局 always-approve 或把实际批准路径换成注入。

完成本批后结束当前 turn，给出上述两份文档和证据入口，由 Codex 审查后下下一任务。

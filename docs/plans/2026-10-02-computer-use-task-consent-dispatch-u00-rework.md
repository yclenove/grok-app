# Grok 下一批：U00-R 架构通路与证据返修

延续同一工作区、分支和会话，禁止 restore-code。
你上一批 U00 已退出0，Codex 已读完整 ADR/checkpoint 和原 probe，结论 needs_rework。
先全文读 docs/plans/2026-10-02-computer-use-task-consent-u00-codex-review.md。
本次只解决 R1–R5，不自行开始产品实现。完成后 Codex 再派 U01，目标没有缩减。

## 必须交付

1. 修订 ADR（保留修订说明，不把先前 unresolved 写成早已解决）：
   - CU-only generic CLI permission 的准确 Host allow-once 判断与作用范围。
   - 固定 MCP token 从无权申请到用户批准后执行的实际路由，优先采用 session transport
     credential + Host current TaskGrant/ExecutionBinding gate；不向模型导出 bearer。
   - credential/request/grant/continuation 的完整 App/agent/generation 绑定和退休。
   - feature-off epoch 与 approve reservation，不先开启 feature 再发现旧请求已无效。
   - tagged request/status/cancel schema、真实 kebab-case surface、去重key、typed Host scope。
2. 补一条 normal CLI harmless fixture，使用真实限定名 server__tool，保留必要的 permission
   rawInput/_meta/结构化 server/tool 信息以确定 CU matcher 输入。仅输出固定字符串，
   不注册真实 CU、浏览器或桌面工具；所有 synthetic decision 仍标测试数据。
   如果 CLI 只给 title，明确此限制并给不依赖自由标题的可执行方案，不假设已有字段。
3. 修正 probe-continue 的常量 true 和固定 exit0。新唯一 attempt/root 保留原两轮日志；
   正向重跑受影响的真正 predicates；负向通过只改 test data/省略事件/不符oracle，证明会失败。
   不为补报表刷整套suite，不升级CLI、不重启VM、不改用户代理。
4. 更新 checkpoint 的准确范围，新增
   docs/plans/2026-10-02-computer-use-task-consent-u00-rework-checkpoint.md。
   列 R1–R5逐项结果、真实命令/hash/断言/首败保留、剩余 U01与U02源码变更入口。

## 已知现场，别再花时间找路径

- CLI C:/Users/Administrator/.grok/bin/grok.exe，1.0.46。
- 外部已配置 isolated GROK_HOME：
  H:/aicoding/grok-app-computer-use/tools/computer-use-probe/.run/grok-cli-20261002/home。
  不读取/复制/修改 auth.json/config，不需要新的 fresh-home 认证失败实验。
- 原 probe：tools/computer-use-probe/.run/task-consent-u00-20261002。
- 首败只读快照：tools/computer-use-probe/.run/cu-consent-orchestrated-20261002/batch00-u00/codex-first-probe。
- 第一 configured CLI67024 的 exit1 已核到你自己的 taskkill 工具调用
  call-08e8209c-e63c-43b3-bf00-b9aee69da3b7-65；不是已证产品崩溃。
  真实终止晚于20秒回执，修报告不要照抄命令description的“before”。
- 第二 continue PID75644、fixture28684/32588已退出；wrapper writer37260也已退出0/join。
  新 writer PID不同，以实时owner核对；绝不能结束原数字碰巧复用的进程。

你仍是唯一产品writer，但本批只许修改设计文档与 owned probe。
不 commit/push/PR/merge/tag/release、不 reset/restore/clean/stash/rebase/换分支、不子代理。
不碰用户窗口/屏幕/profile/账号/token、共享 ~/.grok、其它 worktree/代理/日常进程。
不自动批准 App CU、browser Share、TCC/portal/UAC；harmless fixture 的 synthetic decision 非真人。
清理精确 owned 资源需核 PID/start/实际父子/argv，不只按 PID/name批杀。

聚焦已有审查缺口，不重读几天历史日志、不重复认证空home、不夸大CLI成功范围。
写出可执行合同后结束本批，交 Codex 审查，不自动进入 U01。

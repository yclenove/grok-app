# Grok CLI 开发指令：提交后恢复批次

直接执行下面任务，不只复述计划。

你是这轮 Computer Use 的唯一产品代码开发者；Codex 审查你的成果。
当前 repo 为 H:/aicoding/grok-app-computer-use，branch 为 feat/computer-use-implementation，
baseline HEAD 为 8d8b71c262ab222748bca32412eea884d4b892e0。
本轮任务全文定义于 docs/plans/2026-10-02-computer-use-grok-resume-execution.md。
先读 AGENTS.md、相关 wiki、该计划与最新 fork/grant-recovery checkpoint，再实施 A→B→C。
旧文档的未提交/30757366 是历史，不得据此重置当前代码。

当前优先缺陷：GNOME pending-consent 的 dialog cancel + Request.Close 可能对已消费
Start invocation 再次 complete，引发 distro backend SIGSEGV。原 core 和准确符号已留存，
guarded regression client 尚未在 guest 运行。先补真实红色证据，再实现最小正确修复。
特别复核 request/dialog/session ownership、single completion、late-ready callback、Close/join。
不要把加 null 检查、重启服务、成功重试或私有 patched backend 写成 stock 生产支持。

本轮新的忽略目录是 tools/computer-use-probe/.run/grok-cli-20261002/work/。
那里建立 baseline、append-only scenarios.jsonl、state.json、failures、logs、reports。
报告至少写 completed/incomplete/not_run、changedFiles、每条 executed command/exitCode/
assertionCount、source/binary fingerprints、original owners cleanup 和 remainingRequirements。
保留原 `.run/gnome-pending-consent-20261001`，不覆盖旧日志/owner/receipt。
不读取、输出或修改启动器的 auth.json/config；登录已经由外部启动器处理。

禁止：reset/restore/clean/stash/rebase/换分支或覆盖现有修改；commit/push/PR/merge/tag/release；
修改其他 worktree、用户账号/token/Cookie/profile、共享 ~/.grok、代理/VPN/正式安装；
按进程名批量停止服务；自动点击用户系统权限框；启动子 agent。
可以在本轮精确 owned disposable VM 的私有 fixture 做有界 backend 生命周期实验，
但不要自动批准原生 portal consent，不能把 protocol/direct-backend fixture 冒充真人 App 授权。
实际 E4 需要人工授权时标为待验，继续能独立做的否定路径和源码修复。

产品默认关闭。normal/YOLO/always-approve 均不代表 Computer Use 授权。
没有当前授权或身份失效，副作用前 fail closed；各 browser surface 不回退 Desktop。
取消先 fence dispatch；unknown 不重放；stop_requested 不等于 stopped；物理 owner 退出前
不得释放占用；借用 tab 不关闭。App/新会话/fork/换模型不继承旧 grant。
生产命令不能 arbitrary eval/shell/Cookie export，也不向模型暴露 authorize/resume。
不扩大 App.tsx/AppWorkbench 状态，UI 按15 locales和现有组件规范。

按计划连续推进，不把完成一个小 mock 测试就当作任务终点。
若某原生验收因已验证缺少人工/硬件而暂时不能做，保留具体证据并推进独立 B 阶段；
不要反复刷已通过测试，也不要缩小原始最终范围。先红后绿，一次修一个真实问题。
重任务串行，测试需实际到断言；无测试/skip/旧 hash 不算 pass。
完成本批后写 checkpoint 与 report.json，输出变更/验证/仍缺什么，交 Codex 审查。
尚未达到全平台最终版时必须诚实写 partial，不宣布整个 Computer Use complete。


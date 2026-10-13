# Computer Use：提交后继续开发的执行批次

日期：2026-10-02。目标：继续由 Grok CLI 开发、Codex 审查，完成原定 Computer Use 最终版。
状态：**active / partial — not releasable**。本批关闭具体缺口，不替代完整交付。

## 现场基线

- 工作区：`H:/aicoding/grok-app-computer-use`。
- 分支：`feat/computer-use-implementation`。
- 起始 HEAD：`8d8b71c262ab222748bca32412eea884d4b892e0`。
- fork main：`ff65cf9961a206d5b9ce1c126fae3b26f4695333`。
- 已提交 native、UI、tooling、历史计划，并合并源项目 v0.2.38 与 Markdown 安全修复。
- 当前不存在已观察到的 Grok CLI 开发进程；不要把用户的 Grok Bot 当作开发进程。
- 旧 `.run` 位于 `tools/computer-use-probe/.run/`，不是仓库根 `.run/`。
- 本轮由 Codex 添加本执行文档和 prompt；其余现有内容必须保留。

已完成的集成验证不应重写为最终验收：7,918 项前端、276 项 Node、590 项 Windows core
及 Windows App check/Clippy 已有历史证据。已回读 publication 的两个原结果文件：
`computer_use-results.log` 为 146 passed / 0 failed / 0 ignored / 1932 filtered；
`session_manager--control---results.log` 为 15 passed / 0 failed / 0 ignored / 2063 filtered。
它们属于 f509d5bd 集成候选的筛选回归，不是完整 App 或当前新修改的跨平台验收。
Markdown hotfix 的 fork main CI 四个 jobs 已全绿；不能据此称 CU feature CI 全绿。

## 阅读顺序及历史冲突

先全文阅读 AGENTS.md、Computer Use/i18n/dialogs/maintain wiki、本文件与本轮 prompt。
随后读 fork checkpoint、grant-recovery checkpoint 和本阶段所涉及的当前源码。
旧计划中 HEAD=30757366、未提交、禁止任何后续提交等是旧现场记录；本批以这里的基线为准。
旧段落的 NONE/未接线/未测不能覆盖更新的源码和具名实测；同样不能把有实现推导成已验收。

Codex 是审查者，Grok 是本批唯一产品代码 writer。Grok 不启动子 agent。
Grok 完成本批后留下可审查 diff；commit/push 由 Codex 按既有 fork 授权审查后处理。
不在本批创建 PR、合并 CU 到 main、push upstream、tag 或发布。

## A：pending-consent portal 崩溃修复与真实回归

### A1. 先复核原证据与生命周期

输入：

- `docs/plans/2026-10-01-computer-use-grant-recovery-checkpoint.md`。
- `docs/plans/2026-10-01-computer-use-fork-checkpoint.md`。
- `tools/computer-use-probe/.run/gnome-pending-consent-20261001/`。
- 该证据目录下 `upstream/xdg-desktop-portal-gnome-46.2/src/remotedesktop.c`、
  `request.c`、`session.c`、相关生成接口及 `portal-lifetime-client.py`。
- `symbolize-007.log` 与 `core-lifetimes-008.log`，不要假定存在同名 `.txt` 文件。

原 core 为 distro backend PID2988 SIGSEGV。精确 backend SHA256
`cf6839a8003098a7a4d710d520b8fcd9afbba6bbd21931bc0e420ac724b79bc7`。
栈经过 `cancel_start_session` → generated completion；Start invocation 指针已失效，
但 Request.Close 仍进入 handle_close。原始证据支持二次 completion 假设，
还必须检查 request/dialog/session teardown 和迟到 session-ready callback。
不要只给指针加 if 后宣布 UAF 全部修好。

把每个 Start invocation 的持有者、消费点、清空点、每条 close/response/ready 回调及
对象销毁顺序写成一份短生命周期表，引用精确源码行与原 core。先核对官方新版本/修复；
若已有可靠 upstream 修复，记录 commit/version/source SHA 与适用范围，不自动升级主机。

### A2. 先红后绿，保留失败

使原未执行的 guarded regression client 真正运行到断言。至少覆盖：

1. dialog cancellation 已完成 Start 后再 Request.Close。
2. Start 仍 pending 时 Request.Close。
3. Start 仍 pending 时 Session.Close。
4. cancellation/close 与迟到 session-ready 的两个先后顺序。
5. backend/client 断连后清理，重复 close 不重复 completion。
6. 成功 Start 的正常关闭与取消路径不相互污染。

每场景记录 original unique bus owner、PID/create-time、request/session object、方法回执、
退出码、日志中的 GLib critical/UAF、对象最终是否消失。后台自动重启后的新 PID 不算原进程存活。
测试必须核对 reply 恰好一次与资源退休；只断言“没崩”不够。
使用有界等待与实际事件门控，不用放大 sleep、加重试或吞掉错误使测试变绿。

### A3. 实现及交付边界

若修复属于外部 GNOME backend，在 repo 保存最小 patch、基线/来源/许可证说明和可复现
应用/构建/回归办法。不得把第三方完整源码大包或 VM binary 填进产品。
优先消费可用 upstream 修复；私有 backend patch 只能证明修复实验，不能证明 stock 支持。
不覆盖系统 `/usr/libexec`，只在精确 owned VM 使用可回收的私有 executable/service override，
完整记录并还原。涉及实际生效集成时形成 ADR，不能偷偷用私有 patched Mutter 作为公共依赖。

原 VM UUID `812400f8-a6c6-4c38-a735-2b8d0ef8d3e2`，WSL Debian 的 owned root
`/var/tmp/grok-cu-ubuntu-gnome-vm-20261001`；用户为 cuaccept，SSH 仅 loopback42791。
最后 attempt21 已退出；先复核实时 PID、端口、owner 和 canonical disk。
若均确认已退出，可按原 guarded launcher 显式重新启动同一 disposable VM；新增 attempt，
保留原 owner/log，不能仅凭 owner.json 显示 running 去等待或盲目重启。
不要运行旧的 hard-coded attempt21 runner 去操作新进程。
无 host shares/input passthrough/LAN bridge，不改主机代理、VPN、权限、系统包或用户桌面。
只在 owned VM 管理精确测试服务；原包/binary/stock session/helper 状态必须在结束时复验。

## B：把 Host/App 的取消与显式恢复闭环落到生产路径

A 的原生验证正在等待明确缺失的人工动作/环境时，不空等，也不宣布整批 complete；
先推进本阶段独立源码回归。A 的缺口必须继续保留。

阅读 `src-tauri/computer-use-wayland/src/gnome_consent.rs`、`gnome_control.rs`、
`gnome_session.rs`、`portal`/registry 和 `examples/installed_grant/recovery.rs`，
以及 App `src-tauri/src/computer_use/`、commands 和前端授权控制器。
核对当前生产路径是否真正做到：

- App 创建 authorization attempt，系统 consent 未结束时尚未 commit grant。
- 用户 Cancel/Stop、feature-off、session deletion、agent replacement、lock/takeover、
  portal/helper owner loss 都先 fence；迟到成功不能恢复旧权限。
- Start/request/session 清理由唯一 owner 执行；close/join pending 时不得声称 stopped，
  cleanup failure 保持 recoverable 状态，不丢失 endpoint/lease。
- 重新授权必须是显式用户动作、新 attempt/target/snapshot；旧所有身份拒绝副作用。
- 模型 MCP 不包含 authorize/resume/reconnect/扩大目标，unknown 不自动重放。

对实在缺陷先写能到断言的失败回归，再修生产实现。已有正确路径只记录覆盖依据，
不为增加工作量重写。跨 await 校验绑定身份；不要在 Broker/slot mutex 内执行慢 IPC/cleanup。
真实 App/ACP/MCP 成功链必须走生产命令与 Broker，禁止以私有 fixture grant 或例子代替。
本批先完成能独立运行的否定路径、身份竞争和取消闭环；需要真人系统批准的 E4 明确保留待验。

## C：复核、文档和下一批接续

按实际修改运行 focused regressions、fmt、strict Clippy（相关 targets/features）、
pnpm audit:prod/typecheck/lint/相关前端测试、final quality gate。UI/runtime 变化另跑对应构建/fixture。
重任务串行；Windows App test 按原 manifest 后链，不把 0xc0000139 当作断言运行。
新候选的 source fingerprint/binary hash 绑定新结果，不挪用旧 receipt。

产物：实现 diff、先红后绿日志、命令/退出码/断言数、短 checkpoint、未完成矩阵、
一份方便 Codex 回读的 `report.json`。原失败、未知退出与 not_run 都保留。
文档只写已实测结论；不覆盖历史记录，不复制出数十份互相冲突的状态。

原最终范围保持：Windows x64、macOS arm64+Intel、Linux X11/支持的 Wayland/Ubuntu22；
Desktop/managed browser/existing Chrome+Edge/App WebView；全部 input/IME/clipboard；
lock/focus/topology/device/owner/cancel/recovery；签名安装/升级/修复/回滚/卸载；
原生 UX/DPI；真实 Grok E4；同一最终冻结候选连续12h active soak。
本批不具备全部证据时，仍为 partial，不缩小成功定义。


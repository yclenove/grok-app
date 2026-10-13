# E4：ExistingTab 原 App 入口与任务准备

状态：implementation in progress，完整 CU active / partial — not releasable。
接 E3 已验证后端；单 Codex writer，保留 dirty/untracked/fix.patch，不提交发布。

2026-10-04 最新：[新TaskGrant真实联合路径首链](2026-10-04-computer-use-task-consent-u04-existing-joint-checkpoint.md)。
真实MV3/Share/认证HTTP/原App ACP/新grant/MCP/独立DOM9检查两次通过；request/Host/ACP peer仍synthetic。
Chrome/Edge最低140，trusted local保护未降级，managed130 seed不改；继续原MCP首次request和完整联合矩阵。
descriptor仍关闭，真人安装/产品入口与全部交付范围仍未完成，不借legacy路径。

以下为E4b.2历史。

2026-10-04 历史：[E4b.2内置扩展导出与慢准备交互](2026-10-04-computer-use-task-consent-u04-existing-extension-checkpoint.md)。
E4a命令/process pin/空receipt重试、E4b.1共享controller/card/panel/15locale与E4b.2安装包
内置44文件显式导出/慢准备UX已接；前端8107/浏览器153/Node183/fresh App及strict通过。
步骤5仍差真人安装与原任务真实Share联合路径，按[下一批联合入口](2026-10-04-computer-use-task-consent-u04-existing-joint-next.md)
继续，再过descriptor门槛与步骤6新TaskGrant联合验收。不重做E1–E3，不把synthetic IPC当真人链；
自然关闭长尾、本轮Node unknown/App首败根因及完整交付范围仍开放。

## 顺序与不变量

1. 先保留原 App/ACP Desktop-only 的实际失败。App 接受严格的
   `desktop` / `existing-tabs` picker surface；不放宽 raw target parser。
2. ExistingTab 明确选择只注册其 metadata executor，必须在原 Core begin
   之前完成，不初始化 Desktop、worker、浏览器或权限。发现阶段复用原
   OwnedTasks、blocking handle 和 cancel/final readback。
3. 原 PickerSlot 保留未 discover 的 call。新增 task-bound preparation
   readback / confirm；全部绑定 session、agent、full key、pickerId 和原
   ACP endpoint，仅 main WebView。不使用全局设置的 Pair/Share 通道。
4. readback 只读原 authenticated shared registry，返回必要的一次性
   code/loopback endpoint、有限状态和数量，不返回 bearer/native proof。
   未 Pair 或没有可证明且可借用的 Share 时，load 不消费原 call；用户
   Share 后仍能在同一 picker 继续。取消/Stop/替换拒绝旧回复。
5. App 后端回归通过后，接共享 controller/card/panel、短准备状态、
   安装/版本/忙/重试、键盘/焦点与15locale；全部可用后才开放 descriptor。
6. E5 同最终候选进行 source MV3 + authenticated HTTP + 原 reservation
   + App/ACP 联合验收；不能以 legacy grant、unit 或合成批准代替真人链。

## 证据与收尾

新证据放 `tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`，
不得覆盖 E3 首败。默认 App 重新构建后嵌 CI Common Controls manifest；
空 test filter 不算验证。Core all-targets、strict、fmt/quality/diff 及受影响
Node/前端按实际改动范围验证。局部绿不关闭完整交付范围。

每批原 build/test/browser owners 退出后，仅清理新检查确认废弃的精确产物；
保留当前 App/cache、immutable seed、依赖、源码及失败日志/hash。不复用已执行
清理计划。主干/PR/签名安装/三平台/真人Grok/12h/完整UX仍待各自证据。

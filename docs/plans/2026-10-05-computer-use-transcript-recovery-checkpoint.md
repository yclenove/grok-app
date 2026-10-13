# 真实暂停恢复验收与聊天取消标记修复

2026-10-05；单 Codex writer；`feat/computer-use-implementation`。
本批不 commit / push / PR / 发布；全部 dirty、原凭据、登录、会话和 `fix.patch` 保留。
完整四 surface、Windows/macOS/Linux、两 Mac 架构及 E5/U04–U09/D00–D31 范围保持。

## app113 的真实结果

应用来自 source112，真人批准任务 `02f040eeb45d42ea8ab13d78aa87872a`。
实际 App 会话 `c74518b6-adf1-42e8-988e-f64476fa8f38`；真实 Grok 4.7 / xhigh / ask。
中文备注“中文恢复验证 你好”已保存，网页计数恰好 0→1→2→3，与服务端业务记录一致。
一次 Pause、一次 Continue 后，原任务重新观察，确认结果并调用一次 `computer_complete_task`。
无需再次批准或手工重发；卡片已完成、原受管浏览器退出。

23 条工具消息含终态标记；Pause 期间有一次预期的 `computer_observe` 取消。
**三次计数全部发生在 Pause 前**：这只能证明恢复后的重新观察与明确完成，
不能证明恢复后的新业务操作，也不能代替进行中的主动 Stop 验收。

原结果：`E/delivery-app-113/real-recovery-result.json`；
`E=tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。
App/CLI/MCP/worker/browser 已退出，fixture 原 stdin 关闭，按精确 PID/路径/创建身份收尾，
原 session92685 已退出1；该退出码是 fixture 强制收尾，业务断言与结果未改写。
退出证据：`E/delivery-app-113/exit-result.json`。原独立 home 保留。

## 新问题与 source114 修复

真实任务完成后，聊天底部仍显示“本轮中途被取消”。原 journal 正确记录了
暂停前答复→取消标记→恢复后答复；前端只按 user 分组，把两侧答复合并，
并把旧取消标记排在最终答复下面。

`endOfTurn.ts` 导出共享的消息终态识别。`session/tools.ts` 的片段合并、工具编排、
live 工具归属和当前 assistant/tool 选择均以终态标记为边界。
保留取消记录、原顺序与独立工具集合；不修改 journal、Host 状态、权限或恢复逻辑。
普通无终态的片段继续合并，context compact 不形成取消边界；回复中的标记文本不会误分组。

新增 8 项先红回归全部失败，再修复并补 3 项边界回归。
11 个相关测试文件共 201 项通过；typecheck、targeted ESLint、quality final、diff 均通过。
三个本批 TypeScript 文件已统一 LF。原 source112 的 Core/native/strict 数字保留为历史，
本批未重跑未变化的全套 Rust/native 矩阵。

原 app113 journal 的只读生产映射/投影验收另通过 1 项：
两个 assistant、取消标记居中、两侧工具集合分离、原文件 hash 不变。
证据：`E/transcript-real-journal-114-result.json`。
首次 probe 的 include/config/alias 和 raw tool ID 适配失败日志保留；
最后使用正式 `mapStoredMessagesToChat` 映射，不把这些启动失败写成产品失败或通过。

source114：3326 件，manifest SHA-256
`39c59ee318c5f873e8e0b53b0893ddc9fb8884a8661e8bb1ff79424f280c6a7f`。
源在构建前后/包装后均复验一致。

## 新包 app114

fresh frontend 38.52s、Rust 54.81s；实际 App 构建、完整便携 runtime/扩展及
450 件 ZIP 逐件解包校验 exit0。未签名本地测试包，不是正式发行。

- EXE：`dist-portable/Grok_CU_20261005_app114/Grok.exe`，92957696 bytes；
  SHA-256 `266d1799254958058b1692fd34fd662ee315ec29cde81c221529e3d919bdefa3`。
- ZIP：`dist-portable/Grok_CU_20261005_app114.zip`，208408329 bytes；
  SHA-256 `46826028539d2feeec061fbd4fa120c5a9ab3da7f2fb45f3bb0aef1ba1478757`。
- manifest SHA-256 `80b6a103e73ee93730a4203714ed4a16104ecfc9d3458a0131f6ad24427e2141`。
- 入口：`E/delivery-app-114/launch-preview.ps1`；复用原 home/登录/会话/instance，拒绝重复预览。

实际 app114 已重开原会话：旧答复19工具→取消标记→恢复完成答复3工具，
消息导航显示两个独立答复，取消提示不再落在最终完成答复下方。原journal hash不变。
证据：`E/delivery-app-114/transcript-native-ui-result.json`。界面核验后该原App已正常退出。
进行中的新任务已通过 Pause保持5→Continue新计数6–9→active Stop保持9。
实时stream绑定/第二Stop终态去重的新增问题与source115修复另见
[2026-10-06检查点](2026-10-06-computer-use-live-recovery-checkpoint.md)。
app113 的真人批准不用于任何新任务。其它 surface/platform、installed Chrome/Edge、
UI/UX 全矩阵、安装升级回滚、模型矩阵和冻结候选12h长稳仍开放。

## 清理

source112 原 owner 终态后的清理已执行：17 组旧 incremental generations，
逻辑10099996805 bytes（9.41GiB），H 空闲观察增6016630784 bytes（5.60GiB）。
原清单 `E/cleanup-readback-cache-112-cleanup.json`；最新代、source、seed、当前包和证据 hash 不变。
不在回收站，可由 cargo 重新生成；working/跨 variant/未知 owner/首败现场延期。
source114原owners退出后，fresh114清单已审核并执行：
旧app109/app113目录及ZIP，逻辑1484890093 bytes、H空闲观察增1486946304 bytes；
另1组旧App增量代，逻辑2619206229 bytes、H空闲观察增894763008 bytes。
本次两项合计逻辑3.82GiB，H空闲观察增2.22GiB，分别以两个原report为准。
`E/preview-packages-114-cleanup.json` 与 `E/transcript-cache-114-cleanup.json` 记录exact targets/重建方法。
保留app114及其450件完整runtime/扩展、全部home/登录/sessions/失败证据与最新编译代；hash及source复验一致。
不在回收站，旧包依记录的源/config重新build/stage/package生成；未知working/失败现场继续延期。

整体仍 active / partial — not releasable。

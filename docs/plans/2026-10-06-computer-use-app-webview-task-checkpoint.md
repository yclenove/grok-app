# D12：内嵌浏览器候选到任务执行检查点

2026-10-06，`feat/computer-use-implementation`，单 Codex writer。本批未 commit/push/PR/发布。
整体 active / partial — not releasable；用户试用包仍 app115，没有新的待批准卡片。

## 本批完成

原 AppHandle 主线程只枚举该 App 实际拥有的 side browser，冻结 native/document receipt。
picker-local candidateId 与执行 target 分离；选择和 reservation 不 bind、不读 DOM、不输入，
不导出登录信息，也不通过可变 label 重新查找候选。原 discovery callback 保留至实际完成，
取消后的迟到结果不能发布。统一候选 UI 与真实 ACP publication pin 尚未验收。

批准后先创建处于 Pending consent 的新 run，再用同一 receipt 绑定同组选中页。
原选择 revision、executor、native/document、session/run 均复验；最终转换和 grant publication
经过真实 App adapter 的 native group guard。原 candidate 集合独立保留用于幂等重批，
改变选择不能复用原 outcome。混合 selected/executable 集合在创建 preparation 前拒绝。

部分绑定失败、Stop 途中返回或 publication 拒绝精确清理本次新 binding。
RunState 保留全部页面，terminal Stop 清理非活跃成员；切换页面不销毁组内 binding。
HostOwned wrapper 也为每个新成员保存 cleanup claim。内嵌页切换不获取、释放或阻塞
其它桌面任务的输入 lease；该并存反例已加入回归。

WebView 的暂停恢复接入原 HostTaskResume：保留业务值和组内 binding，原操作空闲后
fresh capture，再在原 native document guard 中发布恢复权限。旧 snapshot/ref/permit 拒绝。
这只完成后端合同，不能把程序化 Host 决策当作真人批准或模型操作。

## 当前源码和验证

证据根 `E = tools/computer-use-probe/.run/codex-u04-existing-app-20261004`。

- source124：3340 件；manifest `b8bb8ea36fef1af82345299b013895346c982361b383c7f17862d69bc7b633c0`。
- 默认 App lib harness 实际重编译：`E/app-artifact-125.json`，Windows test manifest 已嵌入。
- Core default 1121 / test-support 1137 全量通过，其中新候选/任务/恢复/桌面 lease 反例 11 项。
- 当前默认 App WebView 86、runtime 4、命令 15、ACP task 42、product approval worker 8 项通过。
- Core default/support、App default/probe 四组 all-targets Clippy `-D warnings` 通过。
- fmt/quality/diff 通过；App+Workbench 13573，≥1000 行文件 80，没有增长。
- source124 默认 debug/custom-protocol 实际 App：`E/webview-task-app-124/artifact.json`；
  EXE SHA256 `5f994de007d8d258bb473202d0e412045c87ebc6b7f238d32a15162c89840623`。
  复用 164 件未修改的前端 dist；未重新构建前端或 portable 包。

## Windows 真实 native 结果

`E/webview-task-app-124b/native-result.json`、`owners.json` 和 stdout/stderr。
实际 AppHandle producer → 原 picker/reservation → product preference/approval worker →
新 run/proof → TaskGrant → Core transport tool dispatch → 原生固定 typed action 已运行。
Host 批准与最后 publication pin 为 synthetic；`realModel=false`、`ACP=false`、`MCP=false`。
没有真人卡片、HTTP MCP、原 ACP continuation 投递或实际 Grok 推理，不能拼接宣称整链通过。

两页各中文保存一次、各计数 3 次，共 10 次 typed business actions，独立 DOM oracle 验证。
切回页仍可用；Pause 拒绝输入；Continue fresh capture 后新增一次计数；旧观察拒绝。
Stop 清全部成员，并拒绝后续输入，两个页面业务值保持。最终无 binding。
原 App PID64256 joined，实际 exit0；所有观察到的 owner 已退出，之后 exact-home/EXE 无残留，
loaded settings 确认为 independent、closeToTray=false、keepTrayForSchedules=false。
runner 只捕获到 App 的历史身份，没有完整 WebView2 子进程历史 ledger；不声称完整子树记录。
原 home/settings 与全部失败证据保留，没有停止其它工作区的预览。

首败没有覆盖：source123 初始反例揭示未检查已 Stop run 和 wrapper 未登记非活跃 binding，
已修；`webview-task-app-123` 的 Windows PowerShell inline JSON 构建失败保留。
123b 程序效果通过但 runner 未保存 exit code；124 使用已启用的旧 fixture settings 导致
“preference changed”正确拒绝，且 exact-home 短暂 owner 首检失败。124b 使用新的 default-off
fixture、保留 process handle 并等待原 exact-home owners 完成，fresh 运行后通过。

## 剩余工作

收尾清理：`E/webview-task-cache-124-plan.json` 与 `webview-task-cache-124-cleanup.json`。
原 build/test/App owners 退出后，复验 exact paths/无 reparse point 和保留物 hashes，
删除 18 个旧增量缓存 generation。逻辑大小 10289225785 bytes（9.58 GiB），
H 空闲实际观察增加 5259341824 bytes（4.90 GiB）；硬链接与同期磁盘活动使两者不同。
删除不经回收站，可由原 cargo build/test/Clippy 重新生成。最新 cache、source/harness/seed/
fix.patch/依赖/原 homes 与失败证据保留且复验不变；用户 app115 包未更换。
failed working generation、跨 variant/未知 runtime 与失败现场继续 deferred。

下一批按 [D12 任务接入计划](2026-10-06-computer-use-app-webview-task-next.md) 最后章节执行：
真实原 ACP picker/TaskApprovalPin 的 WebView 路径及其替换反例、统一选择界面/15 locale/
native cover、Windows 真人 Grok 和新用户包验收。之后完成导航/history/文件合同，
macOS/WebKit 两架构与 Linux/webkit2gtk 分别实测。
四 surface、三平台、installed/升级回滚、模型矩阵、冻结候选 12h 与 UI UX 范围不减少。

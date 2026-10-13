# 受管浏览器任务暂停恢复

2026-10-05；单 Codex writer；完整目标 active / partial — not releasable。
本批未提交、推送或发布；app97/app98原预览及真人fixture保留。

## 修复的产品缺口

新TaskGrant原恢复链只允许Desktop/ExistingTab。ManagedBrowser的Pause正确挂起
原worker，但Host回读不提供Continue，原任务不能恢复。不能通过原legacy resume
绕过任务绑定来解决。

现在Host恢复支持原ManagedBrowser任务：原操作收尾后取得一次恢复占用，推进原
paused worker的revision，在任务授权仍挂起时复核原page/context/worker证明，
通过新run cancellation与managed request采集fresh observation，最后由原App/ACP
端点一次发布恢复授权与续行。恢复期间不新建profile，不扩大scope，不自动重放输入。
UI回读只查看保留的本地worker阶段；unknown/uncertain状态不提供重试恢复。
Stop、取消或端点替换继续使旧恢复不能发布。

## 本批证据

证据根：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。

| 项目 | 实际结果 |
| --- | --- |
| source99 首次测试编译 | E0369：测试直接比较未实现PartialEq的UI view；原日志保留，不计通过 |
| source100 四项定向Core测试 | 4 passed / 0 failed / 0 ignored；正常恢复、unknown不可重试、Stop后不可发布、context替换拒绝 |
| Core strict，source100 | exit0；source101与100只有App native测试文件不同，Core源码一致 |
| source101 全Core默认all-targets | 合计1101 passed / 0 failed / 0 ignored，exit0 |
| source101 默认App lib编译 | exit0，Windows测试manifest嵌入并记录artifact101 |
| 原App/ACP恢复控制回归 | 4 passed / 0 failed / 0 ignored，0.15s，exit0 |
| 原App/ACP/真实Node/Chromium恢复 | 1测试含continue与stop-after-capture两场景，7.34s，exit0 |
| App默认 / probe strict | exit0，34.48s / 30.24s |
| 格式、diff与项目final质量 | 通过；App+Workbench13573行、大文件80件，未增长 |

source101：3319文件，清单SHA-256
`4e7129202901abeb00e4e06459a9b301df9174fa9621d0d6bf6829a63050af19`。
artifact101测试EXE SHA-256
`555cd5fb4edf24df9738dce25fad107c1752615adf7c5c6aad6cdb17008b7153`。
测试记录为 `managed-task-recovery-100.txt`、`managed-recovery-core-101.txt`、
`managed-recovery-app-controls-101.txt`、`managed-native-recovery-101.txt`及三strict日志。

真实native成功场景通过SessionManager的owned恢复调用，确认原target不变、
generation/snapshot更新、fresh screenshot存在，以及一次continuation消费后无第二次。
Stop场景在实际恢复截图准备后撤权，旧call无法提交，零续行。
两原supervisor退出见证均jobActive=0、原句柄退休；成功临时profile由原owner结束后清理。

这些场景使用真实随包Node/Chromium和生产worker，但Host批准、session/ACP peer仍
synthetic；未驱动业务输入、不是真人Grok聊天或安装版的完整使用验收。
不得将其写成已完成全平台、所有输入或12h验收。

## 实际应用与后续

app102从source101构建，包含冷聊天运行时初始化及本恢复修复；状态见
[交付记录](2026-10-05-computer-use-delivery-status.md)。前端/扩展/worker源码未变，
复用app98已通过的前端构建，生成逐文件hash清单并在构建后复验；仅本批build overlay
跳过beforeBuildCommand，生产Tauri配置未改。包中seed/扩展仍需原逐文件校验。

下一项优先完成真实聊天入口、授权后业务操作与等待中Continue/Stop；已有浏览器
入口开放须完成原安装/Share/批准链，不能以本managed结果代替。
App WebView、桌面输入/恢复、三平台/两Mac架构、安装升级回滚、UI UX与12h目标保留。

本批成功native两profile已在原owner退出后清理；未单独测量实际回收空间。
当前可用包、seed、依赖、源码/dirty/fix.patch、编译首败和验证记录保留；
增量cache因本任务原预览/fixture仍活跃而延期，不关闭它们来腾出清理条件。

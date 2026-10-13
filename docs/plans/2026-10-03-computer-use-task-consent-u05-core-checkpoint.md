# U05 Core：原暂停任务的显式 Host 恢复

状态：**Core Desktop recovery implemented / App integration pending / partial — not releasable**。
单 Codex writer，工作区 `H:/aicoding/grok-app-computer-use`，分支
`feat/computer-use-implementation`，HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`。
原 dirty/untracked（含 `fix.patch`）保留；没有 commit/push/PR/发布。
完整 U00–U09 / D00–D31、Windows/macOS/Linux、四类 surface 仍在范围内。

## 实现及边界

Pause 保留原 execution run 作为 cleanup correlation，立即退休旧 continuation。
Stop、完成、transport retirement、agent replacement、feature-off 能找到原暂停 run 并 fence；
原 pause cleanup 未结束时不能提前归还 desktop lease。新语义观察可算 fresh，不能冒充视觉坐标权限。

`reserve_host_task_resume` 只预留精确原 Host/transport/request/run/generation/Bound scope；
`HostTaskResume::complete` 在 owned worker 中等待原 cleanup/native idle，锁外复验所有 scope
成员（含非活动成员）、platform/identity，取得 lease、更新 generation、采新 model observation，
最终再核 scope/geometry/owner/fence 后原子发布 grant、观察和唯一新 continuation ticket。
原暂停地址的重复请求共享 reservation；旧 refs/permit/ticket 不复活，unknown actionId 不重放。
取消只同步 fence；原 worker 持有 admission 到 native cleanup 结束。commit 后的迟到取消无效。
Bound task 禁止进入 legacy resume；非 Desktop surface 暂拒绝，需要各自 ownership transaction。

**未实现 App/ACP/SessionManager/command/UI Resume 接线，未启用 Continue。**
Core synthetic 验收不等同于真实 App 用户点击、原生系统权限、实际 Grok 续行或安装包验收。

## 证据

证据目录 `tools/computer-use-probe/.run/codex-u05-host-resume-20261003/`。

| 验证 | 实际结果 |
| --- | --- |
| `resume-cases-8.txt` | 28 passed；实际前后 synthetic 效果、旧 ref、双击/取消、Stop <250ms、慢 cleanup、非活动成员失效、replacement/geometry/lock、另一个 chat lease、unknown 不重放、一次性票据、语义观察 |
| `core-full-2.txt` | 1000 passed = 764 unit + 16 driver + 220 contracts；0 failed/ignored/filtered |
| `core-clippy-3.txt` | all-targets + test-support，strict exit0 |
| 默认 App no-run + actual harness | features `[]`，build exit0；task-view11 / task-controls3 / owned-tasks4 / CU202 / ACP153 / routing38；过滤集合重叠，不能相加；不含 live handshake |
| `app-clippy-1.txt` / `probe-clippy-1.txt` | strict exit0 |
| `frontend-cu-1.txt` | 26 files / 255 tests passed；本轮没有重跑之前所有 298 项 |
| fmt / typecheck / lint / quality / diff / build-ui | exit0；UI build 保留既有 large-chunk warning |

保留真实红：pause-red-1、resume-red-1、lease-red-actual-1、semantic-red-1、resume-cases-5。
最初未取得 lease 的 fixture 失败不算 lease 回归证据；真正红来自已产生实际效果/取得 lease
后的旧 predicate 对照。错误 bind API、缺 PNG、空 fixture 最小值、拼写及 Clippy large enum
失败均保留并修复，未删断言或豁免门禁。

最终产品编辑后 2983 source files manifest：`source-files-1.sha256`，SHA256
`525366f81257e98e94ea12af8859665f5b150acd0fb6883e6b0e5be7f1d8ecbb`。
清理后逐文件复核 **2983/2983 unchanged**（不包括 docs/ignored evidence）。
默认 App harness `src-tauri/target/debug/deps/grok_app_lib-dc611c8f8bc4f678.exe`，SHA256
`06c3202a6fc8f1b141e2356c74d5b91ecac518e0d82b96dd8d918eaf59bc64ab`；manifest 只做
CI 同款 test harness post-link，不修改产品 linker 设置。同路径历史 App records 属于旧构建。

## 构建清理

所有本批构建/测试 owner 退出后，精确删除十组核实为旧变体的 Core incremental leaf，
共 **2,165,207,136 bytes（2.02 GiB）**。`cache-cleanup-before.json` / `cache-cleanup.json`
记录精确目标和检查；App/Core executable hashes 前后不变。清理脚本第一次解析失败，
修正 PowerShell 运算符换行后实际 exit0；解析失败没有删除文件。
此前 UI 批次已清 22.87 GiB，此处是另一次清理，不覆盖旧记录。

以后每批在所属 owner 退出后清理本批一次性 site/runtime/重复包和已核对的旧缓存，
保留源文件、dirty、当前可运行包、immutable seed、必要依赖及截图/日志/manifest；
先核精确路径、运行进程及 reparse point，不对整个 target/workspace 执行递归清理。
Cargo 缓存可重建；本次不是回收站操作，不声称可从回收站恢复。没有新复制浏览器 runtime/发行包。

## 下一步

1. 将 Core native prepare 与非 native commit 分阶段；App 最终 publication 同时 pin 原
   ACP transport 和 SessionManager endpoint，不能只在 `complete` 前后检查。
   commit 借用 reservation，让失败 Drop/native cleanup 在 ACP/SessionManager 锁释放后执行；
   保持原 ACP → Broker lock order，所有 native query/abort 不持 authority mutex。
2. 接 owned worker/cancel/shutdown/reaper、原 session typed continuation 和 busy prompt 队列，
   验原 transport replacement、Stop、dropped waiter、unknown、不重复派发。
3. 完成后才能接共享 card/panel Continue 和 15 locale，不先做看似成功的按钮。
4. U04 全 producer/组/跨 surface 管理，U05 其它 surface/必要权限恢复，U06 lifecycle，
   U07–U09/D00–D31 真实 App/平台/安装签名/Grok E4/同候选12h 均未关闭。

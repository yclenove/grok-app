# U05：App 原会话恢复后端接线

状态：**Desktop Core + App backend implemented / shared UI pending / partial — not releasable**。
沿用 `H:/aicoding/grok-app-computer-use` / `feat/computer-use-implementation` /
HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`；单 Codex writer。
完整 U00–U09 / D00–D31、Windows/macOS/Linux、四类 surface 范围不变。
原 dirty/untracked 和 `fix.patch` 保留，没有 commit/push/PR/发布、真实 CU/OS 批准或代理/凭据改动。
本批从 10 月 3 日开始；证据目录日期反映开批时间，不是新的发行日期。

## 行为及锁合同

Core 恢复拆成 owned `prepare_native(&mut self)` 和 authority-only `commit(&mut self)`。
采集及原范围复验结束后，grant 仍 suspended，model observation/continuation 尚未发布；
commit 在同一 Broker authority 临界区发布新 grant、观察和唯一票据。
commit 不执行 native query/abort/cleanup；借用 reservation 确保错误 Drop 在 App 锁释放后发生。
原消费式 `complete` 复用两阶段，旧 Core 安全/lease/unknown/refs 合同保持。

`TaskResumeCall` 固定原 ACP transport；native prepare 前后都核原 pointer/agent/live owner。
最终 commit 持原 ACP slot 锁再次核 pointer，并在其内完成非 native Core commit。
不能只在调用前后检查，从而让 replacement 期间旧 grant 短暂复活。

SessionManager 同时 pin 原 live/background/parked slot 的 app session、process、agent 和 ACP。
锁顺序 slot → ACP → Broker；native prepare 和 error Drop 不在这些锁内。
恢复不把别的聊天抢到前台，不重新建 agent/session；原 slot 已失效则拒绝，不跟随 replacement。

`computer_use_task_resume` 只准 invoking `main` WebView，携带精确原 execution address。
并发原暂停地址共享 Core reservation，第二请求返回 `in_progress`；成功 receipt 只含新 address，
不把 model ref/令牌/native proof 交给 UI。

复用 bounded `task_controls` owned worker/reaper。UI waiter 消失仍保留 original worker/handle；
Shutdown fence 准备中的恢复，再等实际 native 返回/cleanup/join；迟到取消不能撤销已 commit 的恢复。
原 typed continuation 已走 U02 队列：实际原 prompt/gates busy 就保留唯一 claim，不偷 focus；
真正 stdin admission 才消费，admitted 之后不重复发 prompt。

## 当前限制

**共享聊天卡/面板的 Continue 尚未接线、未启用。** 本批只是已注册真实后端命令，
不把不可用的按钮或 UI receipt 当成功状态。下批 UI 要消费权威 readiness、共享事务和事件，
保持优先 Stop、remount/late reply fencing，不能因为 paused 就盲目开放所有 surface。

其它 surface 恢复、失效范围/必要系统权限的最小增量恢复、暂占用恢复的完整体验仍未关闭。
真实用户 App 点击、实际 Grok、原生系统权限、其它 OS native、安装签名和最终12h均未验。
本地 NDJSON/TCP + synthetic native witness 是回归证据，不冒称真实 CLI/installed App 链。

## 验证

证据：`tools/computer-use-probe/.run/codex-u05-app-resume-20261003/`。

| 检查 | 实际结果 |
| --- | --- |
| Core task execution | `core-resume-2.txt`：32 passed，含新 prepared 非执行、native-free commit、最终 fence、准备未提交 cleanup 四项 |
| 完整 Core | `core-full-1.txt`：1004 passed = 768 unit + 16 driver + 220 contracts；0 failed/ignored/filtered |
| 默认 App no-run | `app-build-2.jsonl` / stderr：features `[]`，exit0 |
| 原 ACP task view/recovery | `app-task-view-2.txt`：16 passed；原 pointer 前/中/后 replacement、native lock 释放、最终 Stop、实际 local wire 原 agent 一次 prompt |
| owned control workers | `app-task-controls-2.txt`：4 passed；新测试实际阻塞 capture，丢 UI waiter 后仍持 handle，Shutdown fence → native return → original join；已 commit 后迟到 cancel 无效 |
| SessionManager recovery | `app-task-recovery-2.txt`：4 passed；并发一次 capture/claim；live/background/parked；process/agent/ACP/slot replacement；真实 prompt lease 阻塞与原 queue 不抢其它 focus |
| owned task registry | 4 passed |
| App CU / ACP / routing | `app-computer-use-2.txt`212 / `app-acp-2.txt`158 / routing38；过滤集合重叠，不相加；live handshake excluded |
| strict Clippy | Core all-targets + test-support、默认 App all-targets 均 exit0；无抑制新增 |
| fmt / quality final / diff | exit0，quality `RESULT: PASS` |

Core 首编译 E0308 是新测试把 typed phase 与字符串比较；修成 enum。
`core-resume-1.txt` 的31 pass/1 fail是测试误把 resuming 当 cleanupPending；原 metadata 合同
本来区分恢复与清理，因此断言改为真实 Resuming，未改产品去伪造 cleanup。
这两项不作为产品先红证据；上一批真实 pause/lease/semantic/scope race 红日志原样保留。
首 App candidate 定向/相关 suite 已通过；最终接收以上最后 candidate，而非拼接旧绿结果。

本批没有前端/locale/product CSS 编辑，因此没有冒领历史255/298前端或 UI build 为本轮重跑。
CU probe 依赖的公共方法保持兼容，但本轮没有重跑 prior probe Clippy/其它 OS 编译。

最终默认 App test harness（CI 同款 test-manifest post-link，未改产品 linker）：
`src-tauri/target/debug/deps/grok_app_lib-dc611c8f8bc4f678.exe`，SHA256
`5e7e284515aa94305a0195f0b61b28de61a5463d3669502871a854a064cd77da`，记录 `app-artifact-2.json`。
同路径之前 artifact records 是历史构建。验收后已独立复核 hash 相同。

最终2989 source files manifest `source-files-1.sha256`，SHA256
`78abf5c87842ba751cf1e92a6e004e97a423a60e01a9f9cea4ac8d0e63849cf1`。
包含原 dirty 和新增 source，不是只含 HEAD 的 archive；docs/ignored evidence 不在其中。

## 用户要求的构建收尾

开批时实际清理十组精确核实的旧 Core incremental leaf，释放2.02GiB；
旧2983 source files逐项不变、旧 App/Core executable hashes不变，再开始本批开发。
详见上一批 [Core checkpoint](2026-10-03-computer-use-task-consent-u05-core-checkpoint.md)
及该证据目录的 `cache-cleanup-before.json` / `cache-cleanup.json`。
与此前22.87GiB分次记录，本次没有再次声称清理相同内容。

本批所有 owned build/test 已结束，没有新建临时 browser runtime/site 或发行包；
当前缓存/依赖/可运行 harness 保留供下一批，不因容量问题盲删整个 target、workspace、其它 worktree。
后续每批在原 owner 退出后主动清一次性产物/重复副本/已核实旧缓存，保留当前可用产物和证据。
被删 Cargo 缓存可重建，不能从回收站恢复；不干扰用户日常进程。

## 下批可执行入口

1. Host metadata 给准确、非 native 的恢复 availability；未实现 surface/非原批准 Native scope
   不露可点 Continue。共享 `computerTaskResume` API、同 session actions/controller 接一次
   用户 Continue，原 receipt 只等权威事件；早到 running、晚到 error、remount、双击、Stop 竞争逐项验。
2. 同时接聊天卡/面板，短状态/错误和15locale；保持项目现有 chrome、键盘、focus、busy、
   stale preview、目标已关闭与 offline 路径。不要新增 App/AppWorkbench state。
3. 实际 isolated UI/runtime smoke 后记录与新 source/runtime hash绑定；不自动批准真实 App/OS。
4. 完成其它 U04/U05范围管理/必要增量恢复、U06 lifecycle，再推进 U07–U09及D00–D31全平台
   真实输入/生产分发/Grok E4/签名安装/同一最终候选12h，不能按本批 synthetic 绿跳关。

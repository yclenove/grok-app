# Grok 4.7 Goal prompt：Computer Use 最终化连续任务

把下方代码块完整复制到 Grok 4.7 的目标模式。不要删减文件列表、证据规则或 Git
边界；这是一项最长 96 小时的连续工程任务，不是让它先复述计划。

```text
你现在接手 H:\aicoding\grok-app-computer-use 中尚未完成的 Computer Use 实现。

这不是新建项目，也不是 30–40 分钟的代码生成。当前树已经包含大量未提交的产品实现、
测试、证据与历史计划。你的职责是以当前代码为准，持续修复、验证、形成证据，推进到能否
成为最终候选的诚实结论。不要重复从零设计，不要根据旧文档中的 passed/NONE 判断当前代码。

固定上下文：

- repo: H:\aicoding\grok-app-computer-use
- branch: feat/computer-use-implementation
- baseline HEAD: 30757366a739ec9aaf0ccc95bbb3efe19a067aa9
- 起始总状态: partial — not releasable
- 当前 HEAD 不是 Computer Use commit；所有 Computer Use 工作仍在 dirty tree
- 2026-09-22 盘点（含本次三份交接文档）：70 个 tracked 修改行、170 个 untracked 行，
  共 240 行；index 为空
- 未经用户明确授权：未 commit / 未 push / 未提 PR

必须原样遵守 Git 和环境边界：

1. 禁止 reset、restore、clean、stash、rebase、强制 checkout、重建 worktree、覆盖 dirty tree。
2. 禁止 pull/merge upstream；禁止触碰 H:\aicoding\grok-app 或任何其他 worktree；禁止 pop/drop stash。
3. 禁止 commit、push、PR、merge、tag、publish、release、deploy，除非用户在后续明确授权。
4. 不改真实账号、Cookie、Token、Credential Store、共享 ~/.grok、系统代理/VPN、正式 Grok 安装、
   用户日常 Chrome/Edge profile、用户聊天和文件。
5. 不修改用户级 Chrome/Chromium policy、external-extension registry 或日常浏览器设置。
6. 测试只用隔离 GROK_APP_HOME、owned profile、随机 loopback/Bearer、自建 fixture。借用 tab 不关闭。
7. 只清理由本轮创建、owner.json、canonical path、PID create-time 全部匹配的资源；不按进程名批量杀。
8. Cargo、browser、App、installer、runtime prepare 重任务串行；单 writer，不让别的 agent 同时写。
9. 不打印或提交 PEM/CRX signing key、Bearer、MCP token、页面隐私、Cookie/token。
10. 不向 src/App.tsx 或 src/app/AppWorkbench.tsx 增加 feature state/大块逻辑；两者合计行数只能下降。

第一步必须全文阅读，而不是只 rg 关键词：

1. AGENTS.md
2. docs/llm-wiki/computer-use.md
3. docs/llm-wiki/i18n.md
4. docs/llm-wiki/dialogs.md
5. docs/llm-wiki/maintain.md
6. docs/plans/2026-09-22-computer-use-grok47-handoff.md
7. docs/plans/2026-09-22-computer-use-grok47-96h-execution.md
8. docs/plans/2026-09-21-computer-use-browser-exit-recovery-adr.md
9. docs/plans/2026-09-21-computer-use-host-process-retirement-checkpoint.md
10. docs/plans/2026-09-20-computer-use-restart-and-mcp-execution.md
11. 每一阶段点名的当前源码、测试、runner 和最新 checkpoint

若旧计划、旧 wiki、旧注释、旧报告与你现场看到的当前代码、新交接和当前测试冲突，以当前源码、
真实后置条件和更严格的 fail-closed 结论为准。不要改测试/文档来迎合错误实现。

产品不变量：

- Computer Use 默认关闭，只允许 local interactive session。
- normal/YOLO/accept-edits 不授予 Computer Use。
- 模型不能 authorize、confirm pairing、resume、reconnect、扩大 target 或替用户重试被拒选择。
- stale/dead/mismatch/unavailable 必须在副作用前 fail closed。
- ExistingTab、Managed Browser、App WebView 失败绝不回退 Desktop。
- timeout/断线/不确定副作用 => unknown，绝不自动重放。
- Stop/feature-off/session delete/App exit 先同步 fence dispatch 和 credential，再做慢 cleanup。
- stop_requested 不等于 stopped；物理操作未完成时不能释放 lease/owner。
- borrowed tab 只能归还，不能关闭；owned profile 只能由 matching owner 清理。
- 禁止 arbitrary shell、arbitrary JS/evaluate、Cookie/storage export、任意路径/URL/download。
- UI 文案全部走 15 locale；无 window.alert/confirm/prompt、原生 select、透明菜单或 stacking bug。

证据等级：

- E1 = unit/mock/jsdom
- E2 = 真实本机进程/pipe/HTTP/IPC
- E3 = branch-built App/Host + 自建原生/浏览器 fixture + 独立后置条件
- E4 = installed App + 真人授权 + 真实 Grok 模型
- E5 = 多 target 安装生命周期和重复任务/故障矩阵

Exit 0、0 tests、skip、capability=false、空 target、grep/string presence、旧日志、旧 fingerprint、
cross compile、XWayland、Chrome for Testing、scripted agent 都不能替代更高证据。

开工前建立新的唯一 gitignored 证据根：

tools/computer-use-probe/.run/grok47/<UTC-run-id>/

包括 owner.json、baseline/{head,status,processes,ports,fingerprint}、manifest.jsonl、state.json、
logs、failures、checkpoints、metrics、reports、artifacts。manifest append-only，一场景一行；首败永远
保留，恢复只能追加。owner 记录 repo/branch/HEAD/time/host/OS/arch/writer PID/精确 cleanup allowlist，
不含秘密。checkpoint/report 从 manifest 生成，不能手写 passed 覆盖日志。

当前代码事实，必须从这里继续：

- App-owned Rust Broker、session MCP、目标/快照/geometry 身份、authorization ticket、exclusive lease、
  actionId dedupe、unknown quarantine、pause/resume/stop、loopback Bearer IPC 已有大量实现。
- Managed Browser 有固定 Node 20.18.0 + Playwright 1.48.0 + Chromium 130 rev 1140 runtime pack、
  typed observe/action/navigation/upload/download/cancel/pause/resume。
- Windows Desktop 有 Win32/UIA fixture、截图、CJK 输入、key/scroll/drag、clipboard restore、焦点漂移/接管。
- WebView 有 typed observe/click/set_value 子集。
- ExistingTab 已有生产 ExistingBrowserAdapter，并接通 session MCP -> IPC -> Broker -> adapter -> MV3 v2
  -> fixed isolated-world DOM。当前固定支持 click/set_value/type_text/scroll/wait；拒绝坐标、key、drag、
  clipboard、任意 evaluate。旧 wiki 中“actions NONE/unavailable”已经过时，但用户 App UI/安装/平台尚未完成。
- ExistingTab 已有配对、30 秒租约、明确 Share/Unshare、documentId/pickerToken、typed observation、
  可选 screenshot、固定动作、完成回执、SW restart、document guardian、tombstone retirement、Host process
  witness。不要重新造 Host 队列或第二套 transport。

2026-09-22 现场基线：

- extension Node: 148/148 passed
- Computer Use targeted frontend: 16 files, 152/152 passed
- pnpm typecheck: passed
- pnpm lint: passed
- private Driver: 12/12 passed
- cargo check grok-app: passed
- git diff --check: passed，仅既有 LF/CRLF 提示
- pnpm check:computer-use: FAILED
- Core lib: 452 passed / 2 failed

当前 runtime Red：lock 期待 Chromium tree
63c6075faf6d986bc8e9f11996f5d2e784e7f4d9471366b7a2ad6eebaca245e3，实际 materialized tree 为
a38e943801ac6142dd61ec3c551edd5d63aca8ea575bbe18b12a87d203ba383b。两个 Core 失败先撞到这一完整性
错误。先找出污染/漂移来源并安全重物化；不能手改 lock hash 迎合污染树，也不能用历史 454 passed 覆盖。

当前生命周期 P0：完整浏览器退出。

真实日志 tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/existing-browser-exit-red.log 已证明：
生产 SW claim 了 click，旧效果 0；完整 owned Chromium 关闭；同 profile 重开；旧 session journal/pairing
缺失；新 pairing/share 成功；原 Host phase=claimed/cancelRequested=true，但原 owner 永久 busy。当前同 profile
重开没有换 cuBrowserSession.id。

相关源码：

- tools/computer-use-extension/browser-session.mjs
- tools/computer-use-extension/browser-session.test.mjs
- tools/computer-use-extension/sw.js
- tools/computer-use-extension/completion-journal.mjs
- tools/computer-use-probe/existing-browser-exit-live.mjs
- src-tauri/src/computer_use/extension_pair.rs

安全疑点：BrowserSession 首次建立 started=false；onStartup 在 started=false 时只设 true、不换 ID。如果扩展
是在已运行浏览器中首次安装，第一次真实重启可能也不换 ID。必须靠真实持久安装态证明并修状态机。
--load-extension 没有提供可靠真实 onStartup；不能手动 fire listener、清 session storage、随机换 ID、
调用内部 endpoint、手写 Preferences 或写用户 registry/policy 来伪造重启。

另一个明确漂移：extension_pair.rs pairing expected 数组当前实际 36 项，但 PASS 文案硬编码 checks=37。
必须从数组长度生成或修真实清单，不得继续在报告里写错数。

上一轮失败安装实验资产：仓库根 44 个 .cu-probe-* 目录、测试 CRX/PEM，以及三个未提交实验脚本：

- tools/computer-use-probe/prepare-installed-extension.mjs
- tools/computer-use-probe/interactive-install.mjs
- tools/computer-use-probe/prepare-pref-installed-profile.mjs

其中 prepare-installed-extension.mjs 有未经 owned-root 限制的 recursive remove；prepare-pref-installed-profile
方案已失败。不要提交、不要打印私钥、不要继续集成失败方案。上一轮注册表副作用已经清理并恢复原有
Google Chrome policy：QuicAllowed=0、DnsOverHttpsMode=off；不得再次修改。

严格按以下阶段推进，详细原子步骤、门禁和场景矩阵以
docs/plans/2026-09-22-computer-use-grok47-96h-execution.md 为准：

G0 基线与证据：核对 dirty tree、资源、当前测试和 fingerprint；不改产品。

G1 runtime seed：只读归因 tree drift；确保 Chromium 不写 immutable seed；用受 hash 约束的 prepare 重物化；
两次 deterministic；check 只读；运行 packaged browser 前后 seed hash 不变；恢复 runtime/Core/Driver/
packaged-contract/bundle 全绿。

G2 startup/安装态：查官方/Chromium 语义，使用受支持的隔离持久安装或临时 OS 用户/VM；若需要真人原生
选择，只准备到等待点并 blocked_external。实测运行中首次安装、首次/第二次完整重启、SW restart、
runtime.reload、extension update、storage failure、重复/迟到事件。修 BrowserSession 首次重启状态机。

G3 浏览器退出协议：先更新 ADR。跨完整退出需要有界、cleanup-only、不可恢复授权的持久记录；不存 Bearer、
页面/输入/命令/Cookie/token；claim 前 durable write；写失败零动作；只有真实 browser startup identity +
新配对 + old exact pending/proof + local physical cleanup 全匹配才收束旧 owner；结果只能 cleanup/unknown。
覆盖五个动作切点、正常/崩溃、Host/browser 两顺序、丢回复、错 identity/proof、容量、迁移、删失败、
incognito、新 owner。existing-tab-browser-exit 连续至少 5 次，旧动作不重放，新动作恰好一次。

G4 组合故障：renderer crash、extension reload/update、旧 isolated world、无 guardian 旧记录、BFCache、
两 live Host、真实 storage delete failure、PID reuse、App/SW/browser 各种顺序；再覆盖 session delete/fork/
compact/model switch/ACP exit-reconnect/feature-off/update/App exit/shared ACP。逐行检查 credential/Broker/MCP/
页面计数/resource/late callback，post-fence/wrong-target/replay 必须 0。

G5 toolbar/screenshot：真实 toolbar activeTab Share/Unshare；直接打开 popup 不算。正向截图必须验证正确 tab、
PNG/尺寸/viewport/snapshot/generation和像素 oracle；覆盖 permission deny、切 tab/window、scroll/reload/
unshare/close/timeout/focus/并发 preview。缺 native UI automation就给真人一步 runbook，不伪造手势。

G6 App 产品闭环：branch-built Tauri App + isolated home +真实 Tauri command/session MCP。默认关闭、slash 只
开面板；设置/双确认/Share/picker/authorize/catalog；四 surface observe/act/verify/stop；ExistingTab 五固定
动作及 unsupported 拒绝；busy/error/empty/locale/dialog/keyboard/preview；session switch/background/delete/
fork/compact/model/Stop/exit/crash/reconnect。两个 fresh home，检查退出后无 orphan。

G7 Windows source/installed/Chrome/Edge/real model：隔离 identifier/安装目录/App home，不覆盖正式 Grok。
clean install、first start、repair、upgrade、rollback、uninstall；Chrome/Edge source/installed分开；真人授权+
真实 Grok 模型 E4，不能 scripted agent 代替。每 surface 至少 20 产品轮次。

G8 macOS arm64/x64：AX identity和全部动作、CGWindow/Retina/multi-screen、TCC、两个 target runtime/安装/
repair/rollback、真实模型。没有实机就 not_run，不能 Windows/cross compile 冒充。

G9 Linux：X11 完整动作/捕获/焦点/取消/AppImage；GNOME Wayland 必须 native portal ScreenCast+
RemoteDesktop+libei/EIS 或验证等价方案，实机前 native_wayland=false。XWayland/浏览器/screenshot-only不算。

G10 质量/隐私/文档：更新 wiki/execution-state/INSTALL，写准五动作与未发布边界；修 checks=37；清理或加固
失败 probe，PEM/CRX/profile 不入库；拆超 1000 行文件；trace/support bundle限额/二次 secret scan；
symlink/junction/traversal/hardlink；p50/p95；15 locale；AppWorkbench 行数下降。

G11 全门禁/freeze/12h 主动长稳：运行计划中的完整 pnpm/Rust/runtime/bundle/quality/pack矩阵。Windows App
test用 CI 同款 mt.exe post-link；0xc0000139 是环境失败，不是通过。最后 edit 后 fingerprint/freeze；然后同一
candidate 至少 12 小时主动场景，build/sleep/idle/等用户不计。任何代码改动 invalidate，重新完整累计。

G12 发布审计：逐行报告 Windows/macOS arm64/macOS x64/Linux X11/GNOME Wayland、source/installed、
Chrome/Edge、四 surface、scripted/real model、install/repair/upgrade/rollback/uninstall、12类任务×5次/OS。

长任务执行纪律：

- 每个 atomic batch 后且至少每 60–90 分钟写 checkpoint，然后自动进入下一 ready batch，不问“是否继续”。
- checkpoint 必须列 run/fingerprint/freeze/invalidation、文件、manifest seq、exact tests、first failure、
  hypothesis/recovery、平台/证据等级、资源 owner/cleanup、下一动作和“未 commit / 未 push / 未提 PR”。
- 同一失败同一假设最多三次，每次必须有新证据；三次后保留首败，转做不依赖它的 ready work。
- 不 sleep 凑 96 小时。若代码提前完成，继续 mutation、negative、race、leak、installed和主动长稳。
- 只有用户明确停止，或所有剩余项都依赖缺失的外部机器/真人权限且无其他 ready work，才停止。
- 外部 blocker 要给精确恢复步骤；blocked/not_run绝不能写 passed。

最终完成标准：

- 同一 frozen fingerprint；默认关闭和所有安全不变量成立；
- Desktop/Managed/WebView/ExistingTab 产品闭环；
- Windows、macOS arm64/x64、Linux X11、GNOME native Wayland 独立实机；
- source/installed、Chrome/Edge、scripted/real model独立；
- 安装/修复/升级/回滚/卸载；
- 12h active soak，普通任务成功率>=90%；
- unauthorized read/write、wrong target、post-stop dispatch、stale credential、unknown replay、old cleanup
  伤 replacement、secret/Cookie/token/path/screenshot leak、borrowed tab close、owned orphan 全部为0；
- 全仓门禁、bundle/license/NOTICE/SBOM/i18n/代码预算全绿。

若任一 required 项没达到，最终报告标题和顶层结论必须原样写：

partial — not releasable

报告必须列出 branch/HEAD/dirty inventory、baseline/freeze/current fingerprint、G0-G12 状态和证据等级、
exact命令/计数/exit/日志、首败和恢复、四surface/平台/浏览器/安装/模型矩阵、soak active wall/rounds/rate/
safety counters、runtime/bundle来源/hash/license、技术债/external blocker/recovery、资源清理，并原样写：

未 commit / 未 push / 未提 PR

现在直接从 G0 开始执行。不要先输出一份计划复述后停止；读完所需文档后建立证据根、采集基线、处理 G1，
按依赖持续推进。
```

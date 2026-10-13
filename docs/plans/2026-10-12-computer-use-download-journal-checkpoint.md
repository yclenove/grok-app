# D22 Windows 原生下载：持久归属与真实 App 崩溃恢复

完整目标保持 active / partial。Windows/macOS ARM+Intel/Linux X11+Wayland、四 surface、安装升级修复回滚、UI UX、真实模型矩阵、冻结 12h、D00–D31 均未缩减。本批未 commit、push、PR 或发布。

## 实现边界

`src-tauri/src/computer_use/webview/download/windows/journal.rs` 及其领域子模块已接入生产下载，而非独立示例。

- 原 `ICoreWebView2` 安装阶段捕获 App/browser 内核 handle 与 creation FILETIME，安装前后复验原 BrowserProcessId。退出回调还要求原 browser handle 已 signalled，不能只比较 PID。
- 长路径私有 buffer 在派发前发布不可变归属；记录 create-new、flush、readback 成功后才派发。半写保留并拒绝，不事后补归属。
- 当前 App root 的 `computer-use/native-download-buffers` 永久保留 `owner.json` / `store.lock`；跨进程独占锁、最多 64 事务、扁平 `<uuid>.json` / `<uuid>.retired.json`。
- 每项绑定原 App/store/parent/buffer/marker/record FileID、原 PID+birth、native instance/expectation。无 URL、Cookie、token、正文或用户文件内容。
- 维护只读取当前 App journal，不扫描 Temp 前缀。原 App/browser 任一 live、查询 unavailable、破损/半写/未知子项、reparse/hardlink、替换/跨 home 均拒绝并保留。
- 所有候选预检和 pins 完成后才删除；receipt 绑定自身及原 data FileID。删除使用原 held handle，失败保留它，不重新打开同名对象删除。
- `Lease::drop` 不清理；正常清理仍由原 caller + callbacks 联合退休边界执行。显式维护接现有 `computer_use_clear_staging`，启动/status/feature-off 不自动清理，也不向模型增加清理工具。

前端 source140 将 retained/live 提示限定到显式 staging 维护。普通下载动作、runtime repair、其它清理不再被错误显示成“清理未完成”。错误状态只保存 i18n key；关闭失败确认框后仍保留正确分类，不自动重试。已有十五语言文案保持。

## 冻结与实际证据

证据根：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`；原生运行根：`src-tauri/target/codex-test-runtime/`。

- source139：3429 件，manifest SHA256 `4a207a38d573021fda61b2fcc35ece64ef78add8dd0b6e104ed79473865fe26d`。fresh default manifested App 定向 38 下载 + 49 WebView + 66 ACP = **153 passed / 0 failed**，过滤范围 ignored=0；不是全 App suite。三组 Clippy `-D warnings` / fmt / diff / final quality 通过。
- source140：3429 件，`e0c53d6412c4c55bc58a6eae655c44ae72c3eec031f47cb7bcddcb1b4cf2d084`。errors/Settings/i18n **86 passed**，typecheck、定向 ESLint、diff/final quality 通过。报告 `download-journal-frontend-20261012-03/results.json`；原02批因 runner 错写质量脚本参数而 exit1，日志保留，不能称02整批通过。
- source142：3430 件，`8773f2beb6d407689c65981b8a506acec9d9747c2cf5790ec3b86812b3a49d20`。真实 App crash/Host 维护验收通过，见下一节；三组 strict 本身 exit0，但随后 fmt failed，不能报整批 strict 通过。
- source143：仅格式化 source142 的一处 probe 命令调用换行，3430 件，`9b8a89fb8f63da904a9ff887c50f802b46519b661c735a6029f114d81e691297`。fresh default manifested App **153 passed / 0 failed**、三组 strict / fmt / diff / final quality 通过；报告 `download-buffer-journal-20261012-04/results.json` 和 `download-journal-strict-20261012-04/results.json`。manifested copy SHA256 `8dcc0ea2a72abeabe1dc4dd78476ee595481b9f6977a1c8a8d6ae5f88db065bd`。

source143 的前端四个修复/测试文件与 source140 内容未变。原生 crash 证据属于142，不伪称143重新运行了同一原生验收。App + Workbench 13573、≥1000 行文件80，未增长；`FINAL: PASS` 仅表示代码质量。

17 个 journal 回归有真实文件/自己创建的 kernel-process fixture；它们不代替真实 App crash。source137 曾有 **17 passed / 17 failed** 的真实断言：create-new 仅设置 access_mode、漏 read/write 打开选项；修复后通过。source138 曾有 large enum / bool conversion 两项 strict 失败；Box 与直接 bool 修复，无 suppression。原日志和 red copy 保留。

## 真实 App force-kill / 原 browser 维护

新增仅隔离 debug+probe 可用的 `webview-download-buffer-crash` 与 `native-download-maintenance` 入口；普通产品入口、授权及灰度 gate 不变。维护 probe 调用原 Host staging command，不启动 UI/browser/model，也不是新增模型工具。

source142 `app-download-journal-crash-20261012-04/crash-recovery.json`，原 session exit0；App EXE SHA256 `ff39e814762b0b3eb017ee2c06bb664a9baa99dc8fdf33b5107dabaac6c64406`。

1. 原 App PID57584，真实 Wry/COM 下载恰好一次、完整私有1MiB文件、原 native terminal/Wry finished各1、wry processed=0、Broker staging空。不是 synthetic terminal/file。
2. 外部 runner 先持有原 App/browser 内核 handle；原 browser PID57376 的完整 kernel birth 与生产 journal 一致。只 TerminateProcess 原 fixture App handle，原 exit99；没有按名称/整树杀进程。
3. 原 App 已退出、原 browser 尚未 signalled 时，另一个同候选隔离 process 调用 Host maintenance，exit2，明确 `still live; retained`。原 data/record/marker SHA256 不变。
4. 观察到的15个原 App/console/WebView进程均经原 held handle 退休，未放宽30秒门禁；保留 kernel exit times。随后维护 exit0，只删除准确 buffer、record/receipt，journal仅永久两文件。
5. 再次显式维护 exit0，无副作用。三个维护 process 原退出均已 join。

Host approval synthetic；真实原生/Host command 为真，ACP/MCP/model/真人 picker 为假。因此只关闭此崩溃维护场景，不代表完整用户链或发布验收。

原02/03 crash runner分别在共享读取、CIM birth比较处提前失败，均未杀App，随后20秒bounded hold正常 unwind、原App退出。这些失败报告保留；02的 `realHostMaintenanceCommand:true` 是旧 runner 的过度固定标签，实际 `steps=[]`，不能作为维护调用证据。

04 preflight记录证明 CIM 仅微秒精度：`...16.0774860Z`，原 kernel 为 `...16.0774868Z`，生产 FILETIME精确一致。runner 改为原 handle/kernel birth，而非放宽生产校验或用容差。source141首编译的命名空间错误与142 fmt 首败也保留。

## 未关闭的正常退出失败

- source138 held/private-file：业务完成，原App62808 exit0，但原30秒 gate仍有browser62152/crashpad20704。独立 `owner-timeout-followup-20261012-01.json` 证实后来全部11原owner缺失，journal事务0；没有已知准确退出时刻或根因。
- source139 before-start：Pause/Stop/原8015ms grace/Navigate/Close五项业务完成，Close5017ms失败后显式重试成功；原App67696 exit0，但30秒 gate仍有browser59544/crashpad27988。独立 `owner-timeout-followup-20261012-02.json` 证实后来12原owner缺失，journal事务0，原 owners.json SHA256保留。
- 这两份原 `allObservedOwnersExited=false` 不改写。142 crash退出成功不能修复正常退出失败；Chrome_WidgetWin_0 unregister1412也曾见于成功批，尚不能称根因。

下一项应捕获正常 Quit 前的准确 main/side browser roles、原 kernel handles、controller/observer退休阶段，再对原30秒失败采样；不无限复跑等绿，不以force-kill browser作为正常退出修复。

## 本批清理与当前预览

原 owner退出/路径与hash复验后，仅删除138/139两成功 libtest副本与142-02/03两份byte-identical重复EXE。logs/home/失败报告/ready witness保留，142-04完整候选也保留，按cleanup报告可重新复制或从原冻结源码构建。

`download-journal-cleanup-20261012-01.json`：逻辑343542784 bytes；H空闲即时观测增加343547904 bytes（约327.6MiB），并发磁盘活动不能独占归因。诊断EXE/PDB缺独立原owner报告及未逐件审计的incremental/shared caches暂缓；未碰其它工作区构建。

唯一用户预览仍 `dist-portable/Grok_CU_20261012_app133_gray/Grok.exe`，PID10928、原home/login/history不动。已实看正常首页/账号/历史，`http://tauri.localhost/`；本地1421无listener，当前包不依赖开发服务，也不冒充source143。

## 继续工作

- 定位上述正常退出延迟；补真实 App crash场景中的新run并存、受损/替换/半写与访问拒绝等原生组合验证，不能只引用unit。
- 143普通用户包、真实新模型意图/真人选择批准/文件picker、上传下载/暂停恢复Stop完整链仍待验；不复活原132过期请求，不代批权限。
- 其它surface/platform、installed升级回滚、UI UX、模型矩阵、最终冻结12h和D00–D31继续按完整计划关闭。正式app-webview descriptor仍有灰度门禁。

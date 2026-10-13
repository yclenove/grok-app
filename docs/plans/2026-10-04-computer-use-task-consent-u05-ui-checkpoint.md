# U05：Desktop 任务一次 Continue 的共享 UI 接线

状态：**Desktop task resume vertical path implemented / full U05 still partial — not releasable**。
工作区 `H:/aicoding/grok-app-computer-use`，分支 `feat/computer-use-implementation`，
HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`。单 Codex writer；原 dirty/untracked
和 `fix.patch` 保留；没有 commit/push/PR/发布、真实批准、代理/凭据修改或日常浏览器干扰。
完整 U00–U09 / D00–D31、Windows/macOS/Linux及四类 surface 没有缩减。

本批接续 [App backend checkpoint](2026-10-04-computer-use-task-consent-u05-app-checkpoint.md)。
该页“共享 UI pending”是本批之前的历史状态，下面是新增实现和最终证据。

## 产品实现

- Host execution metadata 增加 `resumeAvailable`，只根据锁内原批准 Native Bound、原任务/
  transport/feature身份及当前暂停/终态判断 eligibility；不读取窗口、系统权限或截图。
  不是 native readiness 证明，真正复验仍在 explicit Continue 的 owned worker 中。
  不能恢复的 synthetic-only/无 observe/未实现 surface 不给可点 Continue。
- `computerTaskResume` 只发送精确 key/run/canonical decimal generation，拒绝非本机 Host；
  不调用旧 current-run resume，不从普通网页、镜像或 model resume 进入。
- 聊天卡和面板共用 session `ComputerTaskActions`；一次用户 Continue、双击去重、
  typed receipt 校验、等待原 Host新 generation + fresh observation + executable grant。
  receipt/in_progress不自造 running。早到 native成功可先 settle，晚到 error不覆盖；
  resuming→paused失败有可重试短文案，目标关闭/Stop/feature-off胜出时退出准备。
- shared controller保持优先Stop。换task/connection/Host、stale stream和remount隔离旧回包，
  挂载不自动恢复。相同卡/面板没有各自独立续行prompt或授权副本。
- 卡/面板共用原 chrome 和键盘路径；恢复期间显示“正在恢复”，Stop继续可点，preview
  不在 paused/resuming状态采集。不可恢复的scope不会露可点恢复控件。
- 新恢复错误 `cu.consent.resumeFailed` 同步15locale；已有 resume/resuming短文案复用，
  不把 native/transport原始错误、令牌或 model refs交UI。没有新App/AppWorkbench状态块。

## 验证及失败

主证据目录 `tools/computer-use-probe/.run/codex-u05-ui-resume-20261004/`。

| 项 | 最终实际结果 |
| --- | --- |
| Core恢复/metadata定向 | `core-resume-1.txt`34 passed；新 eligibility原生零查询、无native proof拒绝、准备时不可恢复、feature-off拒绝 |
| 完整 Core | `core-full-1.txt`1006 passed = 770 unit + 16 driver + 220 contracts；0 failed/ignored/filtered |
| 默认App build | `app-build-1.jsonl` / stderr，features `[]`，build-finished success |
| App task-view/controls/recovery/owned | 16 / 4 / 4 / 4 passed；原Backend/ACP/SessionManager/实际local wire单次续行回归保持 |
| App CU / ACP / routing | 212 / 158 / 38 passed；集合重叠，不相加；live handshake excluded |
| 全相关前端 | `frontend-cu-2.txt`：31 files / 329 tests passed，含各旧controller hooks/API/domain/component与43项15locale检验 |
| 新Continue回归 | domain18 / component7，包含receipt非授权、重复请求、Stop、stale、原generation完整性、早成功晚错误、后台失败、无效scope和replacement；已包含在329中 |
| 类型 | `typecheck-3.txt` exit0；单独 fixture tsconfig `fixture-typecheck-1.txt` exit0 |
| lint / build UI | `lint-2.txt` exit0；`build-ui-1.txt`成功，既有large-chunk warning保留 |
| strict Clippy | Core all-targets + test-support、默认App all-targets、probe all-targets均exit0；没有新抑制 |
| fmt / quality final / diff | exit0，quality `RESULT: PASS` |

前端首广选命令把PowerShell数组作为一条字符串传给pnpm，`frontend-cu-1.txt`无testfiles/
exit1，属于runner失败；改用明确目录filters后完整31文件329项，不用空suite报绿。
新test一个未使用type import导致`typecheck-2.txt`失败；删除未使用import后typecheck通过。
原红日志未覆盖；这两项不是产品先红证据，不冒称缺陷先红→修复。先前真实Core pause/
lease/semantic/scope竞争红记录仍保留。

## 实际隔离浏览器及自动清理

`isolated-ui-2.txt`最终源码Chromium40/40；report/screenshots在
`tools/computer-use-probe/.run/task-consent-ui-2026-10-03T18-14-23-772Z/`。
覆盖300/320/900宽、light/dark、en/zh/de/ru、DPR1/1.25/1.5/2、各任务状态、
键盘picker/focus归还、键盘Continue→running以及Continue中Stop→stopped。
实际DOM布局无横向溢出，Stop在视口内；未打开权限/枚举/旧status旁路，原请求一次。
最终run两张paused/恢复后截图的等价首轮已人工查看，button/preview/目标区域无错位。
这是实际产品组件+隔离Chromium的synthetic IPC，不是生产App/真人/OS权限或实际Grok。

browser/server原owner关闭后runner自动删除**本轮**site/browser-runtime，记录全文件hash/
logs/screenshots/reports并保留immutable seed。最终一轮372,641,824 bytes（355.38MiB），
首轮372,641,676 bytes（355.38MiB）；各自`cleanup.json` ownersClosed=true，
两个精确目录实际不存在。两轮不覆盖或复用旧绿report，没有新发行包副本。
与前批清旧incremental2.02GiB及更早22.87GiB分次记录；保留当前缓存/依赖/harness/dist。
已经把原owner终态、精确target、拒绝reparse/live owner、保留当前产物与证据的收尾规则
加入本项目AGENTS第9条，不盲删整个target/workspace/node_modules/shared cache。

## 最终身份

2992 source files manifest `source-files-1.sha256`，包含原dirty/新source及AGENTS，SHA256
`43da03658e6383b24aaed4b67bc6f3d351c8af59ab299544dcc0cf53b0288024`。
冻结后已逐文件复核2992/2992 unchanged；docs/ignored evidence不在清单内。
默认App harness `src-tauri/target/debug/deps/grok_app_lib-dc611c8f8bc4f678.exe`，SHA256
`82cb3aaf3d0a885b3573fa4b88183de7259278dc72cff572683ad8a00f45a5ca`，记录
`app-artifact-1.json`，验收后独立复核一致。只为测试harness嵌CI同款Windows manifest，
没改产品linker；之前同路径artifact记录属于历史构建。

## 剩余与下一步

不能根据本批Desktop/synthetic绿宣称完整CU可交付：

1. U04其它scope producers/关联dialog/跨surface子binding、真正增量与即时缩小；
   managed/ExistingTab/WebView所有ownership/document/readiness合同与三平台仍需完成。
2. U05其它surface恢复、必要系统权限/失效目标的最小增量恢复、暂时lease占用的持续恢复体验；
   本批Continue只复验原Desktop范围，不选同名replacement、不重批所有仍有效范围。
3. U06真实lifecycle矩阵、U07实际App/native/normalGrok纵向动作与用户点击、
   U08完整平台CI/包内能力，U09独立签名安装包/真实安装态/GrokE4/同一最终候选12h。
4. 下批先核ManagedBrowser任务scope producer及native/Desktop关联实例的生产入口，
   采用真实owned context/profile/document证明，先meaningful red再接原Host request
   transaction；原可用Desktop/UI回归保持，不拿旧legacy工具当任务模式已支持。

所有原生/系统/真实App批准需用户完成，不自动点击。单writer继续，不自行提交发布。

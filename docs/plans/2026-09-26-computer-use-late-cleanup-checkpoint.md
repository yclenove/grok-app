# Computer Use：迟到操作与持续物理清理

本机日期：2026-09-26（Asia/Shanghai）。分支 `feat/computer-use-implementation`，
HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。已有大量未提交工作均保留；无 commit/push/PR。
完整目标与 UI/UX 重设计范围不变，未达到最终版完成条件。

## 已复现的产品缺陷

浏览器关闭后，`finishShutdown` 仍需等待已接受的请求结算。原逻辑使用一秒
`waitAllIdle`，到期便退出物理清理函数，导致 `owners.clear()` 永远未执行。
迟到请求随后结束也无法推进清理；只读健康检查一直看到关闭中的 profile。
`cancelRun` 同样存在一秒等待后放弃剩余收尾的问题。

原生回归使用实际生产 worker、Playwright 和隔离 Chromium；仅在测试 preload
通过私有 IPC 延迟原始 `context.close` 与一个已接受请求的 `finish`。
先确认原始 native close 已完成，再让额外 HTTP 观察者到期，随后释放请求，
要求原始后台清理自行退休 profile，不能再发一个 shutdown 来维持它。

- `late-operation-cleanup-red.log`：真实原生断言失败，profile 在原操作结束后仍残留。
- `late-operation-unit-red.log`：三个新单元因缺少物理等待接口失败；这是接口开发基线，
  不替代前一项对实际产品缺陷的复现。
- `late-operation-cleanup-green.log`：生产修复后 **34/34，0 skipped，exit 0**。
  原生三个路径都确认仅一次 native close；shutdown 的迟到请求结束后，
  无额外 shutdown 请求也能完成 inventory 退休，最后显式确认与 owned worker exit 0 通过。

## 生产修复

- `RunOperations.whenIdle/whenAllIdle` 等待真实请求和物理资源双重归零，
  不与某个 HTTP 调用的观察期限共享生命周期。
- 整个 worker shutdown 与每个 owner cancel 共享原始清理 Promise；重复观察不派发第二次关闭。
- 原生失败依然返回 unknown；只有独立的实际关闭证据才允许后续协调完成。
- HTTP 回复预算保持原值：shutdown 1500ms，close/cancel 5000ms。
  没有延长产品期限、重放输入、取消身份检查或把未知状态改成成功。

这只证明上述清理状态缺陷已被定向修复。它不能证明历史 15–28 秒 native 退出长尾、
WebView 20 秒退出失败或操作系统进程查询超时有相同根因。

## 验收程序的独立修复

新增测试专用 `fixtures/cleanup-verified.mjs`：所有清理证据分别尝试并聚合错误，
只有 shutdown、worker exit、fixture close、browser exit 全部确认后才删除隔离 profile。
七项回归覆盖各失败分支、错误不被覆盖和删除失败；与已有协调测试合计 **12/12**。

Observation harness 已接入。R3.4 harness 也移除“请求后立刻 kill worker + finally 删除 profile”，
重复收尾观察共享同一结果，失败不能在下一次调用中变成成功的空数组。
强制终止仅作为该 fixture 自己 child 的失败清理，原失败必须保留，不可据此删除 profile。

## 全量结果与边界

共同产物目录：`tools/computer-use-probe/.run/worker-serial-20260926T091254/`。

1. 更早 UI R13 后的 worker 运行：162/172，10 failed；缺少显式浏览器路径且使用文件并行。
2. 补齐两个显式 Chromium 环境变量并使用 `--test-concurrency=1`：
   `worker-serial-original.log` **176/176，exit 0**。
3. 同样环境、加入清理安全测试后：`worker-serial-final.log` **176 passed / 7 failed / 183 total，exit 1**。
   包含 observation shutdown 未确认、native process query 不可用、三个 8 秒导航期限失败、
   cancel 的立即 200 断言和 R3.4 清理阶段 EPERM。不能将这些都归因于测试启动参数。
4. 第三次运行里 observation 已保留原始 shutdown 和 worker-exit 错误及隔离 profile；
   R3.4 的 EPERM 掩盖问题促成上面的独立 harness 修复。

第三次失败后的内存/CPU读数只是瞬时诊断，不证明失败发生时的机器负载，更不证明网络原因。
之后单个绿色回归不清除这些失败记录。

5. `late-cleanup-native-matrix.log`：observation、精确 PID 与修复后的 R3.4
   **26/26，exit 0**。此时还没有最后的 cancel 只读协调改动，不移用于后续版本。
6. `late-cleanup-unit-final-r2.log`：取消协调、物理等待等 **40/40**。
   本轮三个文件复核 `late-cleanup-unit-current.log` 为 **27/27**，不是完整 worker 套件。
7. `r34-reconciliation-native-current.log`：**19 passed / 2 failed / 21 total**，
   包含父套件失败；实际第 12 子项的最终 `/run-status` 忘记 Host header，返回 403。
   原生 cancel 已返回 200，但这不等于该子项通过。补齐测试调用的 Host header，
   不修改生产路由认证。同时发现该 harness 忽略显式 worker 变量，已改用选择的包路径，
   记录实际源码散列；之前结果仍属于源码 worker，不追溯改名成包验收。
8. 修复测试调用与显式包路径后，`r34-reconciliation-packaged-r2.log`
   **21/21，exit 0，57.15s**，包含 20 个命名子项及父套件。
   诊断明确记录包 `worker.mjs`、SHA-256 `0d8a80d1f008aba6eb81e635def9159e9841c3f27e2ffa00616757d5553c8ff9`
   和包 Node v20.18.0。第 12 项还要求 stopped/idle/active=0、原动作失败和独立 oracle 零效果，
   未放宽断言；第 18 项确认本 harness 的 worker/Chromium PID 退出。
9. `late-cleanup-unit-current-full.log`：当前四组关闭/协调/占用单元 **40/40，exit 0**。

## 源码 / 包 / UI 证据版本

本批生产 worker 改动发生在 R13 App 构建及旧 seed 检查之后。
旧 `importProbe=passed`、18 个模块 source-match、UI 原生启动截图均不能作为本批新 worker 的打包验收。
已经新建隔离 seed `H:/aicoding/cu-late-20260926T0941/seed`，未覆盖运行中的 App。
该包的 Chromium 使用独立可写副本；不直接启动或改写只读 seed。

- 第一次 prepare：`late-cleanup-package-prepare.log` exit 1，publish Access denied (os 5)，
  staging 清理又有 sharing violation (os 32)。遗留 staging 和 owner 标记保留。
  之后 443 个剩余文件独占只读打开成功，未发现匹配存活进程；这只说明检查时未占用，
  不能推断原始原因已修复，亦不能据此归咎杀软。
- 确认原命令终态后，同一路径第二次 prepare：`late-cleanup-package-prepare-r2.log` exit 0，
  `importProbe=passed`。manifest `46939d4fa6fa0dc9179803c50ecad095dd155e6e074b69c74db682bb318dbf83`；
  tree `2143cf14a220a82f777e405f465a934a5d89ce2cab4969fbb44c7928f6896520`。
- 首次测试文件名误写为不存在的 `http-errors.test.mjs`，exit 1、未运行测试。
  正确名称为 `worker-error-http.test.mjs`，不把命令错误当作产品失败或测试通过。
- `late-cleanup-packaged-native-r2.log`：包自带 Node 20.18.0 + 包 worker，**2 passed / 1 failed**。
  第一项真实关闭超过原 30 秒物理观察预算。实际 close event 在 13,328ms；
  Playwright force-kill 在 43,301ms，进程直到 157,387ms 才退出，close Promise 在 157,419ms 结算。
  原生事件发生不等于进程退出，不因此提前释放清理占用。
- 旧测试的 after hook 在错误后没有继续收尾，测试 runner 保持存活。
  本轮先轮询原 session 54138，再核验 worker 原 PID、路径、父进程、创建时间并持有句柄；
  浏览器退出且无浏览器子进程后，仅终止这个已失去调用者的 fixture worker。
  `late-cleanup-owned-worker-recovery.json` 明确记为 forced cleanup，profile 未删除；
  原命令终态 exit 1，不能算正常关闭成功。
  后续只读目录核对保留根为 `C:/Users/Administrator/AppData/Local/Temp/grok-cu-cleanup-http-AFhb2V`。
- 修复该 harness：独立尝试各项清理，真实失败聚合保留；物理 close Promise 也须确认，
  不能只看 close event；未确认时保留 profile。新增仅测量原始 taskkill 调用的脱敏阶段日志，
  不替换依赖行为、不改参数、不增加产品超时、不记录命令、令牌或输出。
- `late-cleanup-packaged-diagnostic-r3.log`：同包重新验证三个关闭接口 **1/1，exit 0**，19.38s。
  三次 native close 各一次，实际进程退出约 136/141/462ms 后确认；本轮未触发 force-kill。
  这是新包功能协调的定向成功，**没有复现或修复 157 秒退出长尾**，不覆盖前一失败。

`late-cleanup-packaged-result-r2.json` 和 hygiene audit 确认失败测试后 seed 内容未变，
444 files、zero hits；digest `0cbebf699e0b6a22ecf68a7b091313fb36eade43d2da5f11ab56abae2670234c`。
`server.mjs → worker.mjs` 及 `run-operations.mjs` 与包散列相同。
这仍不是签名安装版验收。

最终 `late-package-final-check.log` 的 `pnpm check:computer-use` **exit 0**，
明确指定上面的独立 seed，离线 `importProbe=passed`，manifest/tree/Chromium 散列均匹配。
`late-package-final-result.json` 记录 check/audit 均 0、zero hits、seed 前后不变，
digest 仍为 `0cbebf699e0b6a22ecf68a7b091313fb36eade43d2da5f11ab56abae2670234c`。
没有重做 prepare 或覆盖当前 App；该检查不证明历史退出长尾、目录发布失败或签名安装已修复。

R13 UI 的 273 项专项测试、60 个编译浏览器 fixture 保持其原始证明范围：
Host/剪贴板为 mock，原生 App 仅确认启动和初始渲染。键盘、缩放、配对、授权控制与完整安装版验收未完成。

本轮只读重新观察 R13 的唯一匹配原生窗口时，页面已从初始引导变成另一条 `ping` 对话，
并显示 AUTH_FAILED。该消息不是本轮发送；未点击、改帐号、修改权限或撤销其他操作。
截图只证明原生渲染，不把窗口状态变化写成成功的 Computer Use 测试。

## R14 UI：目标发现失败与恢复

实际复现：成功选择目标后刷新失败，旧候选仍保留且授权按钮重新可点；
后续刷新成功也未清除旧错误。`ui-target-refresh-red.log` 先在“授权必须禁用”断言失败。

生产 controller 将目标发现错误与运行/授权错误分离：失败退休旧列表和候选；
重试中显式 busy，成功只清理本次发现错误，仍需重新选择，不能自动授权。
面板区分“加载中 / 发现失败 / 成功但零结果”，不把失败伪装成空列表。
当前运行的 Pause/Stop 不因候选列表失败被锁住。复用现有组件和样式，不增加 App 壳状态。

- `ui-target-refresh-green.log`：首个产品回归 **46/46**。
- `ui-target-refresh-surfaces.log`：后增四个用例把文案误写 `Choose target`，4 failed / 35 passed；
  修正为真实 `Choose a target`，没有修改产品文案迎合测试。
- `ui-r14-domain.log`：**124/124，15 files**；`ui-r14-adjacent.log`：**65/65，7 files**，
  覆盖四种来源、当前任务 Stop、互不覆盖的错误，以及实际存在的 API、全目录 i18n、侧栏缩放测试。
- `ui-r14-typecheck.log`、`ui-r14-probe-typecheck.log`、`ui-r14-lint.log` 均 exit 0。
  新编译浏览器场景须以其终态另记；unit/jsdom 不代表原生 App 授权与桌面操作。

### 文案、真实截图与迟到的发现回包

首次 68/68 编译浏览器场景通过后，截图复核仍发现具体 UX 问题：
加载目标失败被说成“操作失败”，还显示不相关的选目标说明。
`ui-r14-copy-red.log` 先记录 5 failed / 34 passed；之后新增完整 15 语言的
`cu.panel.targetsUnavailable`，明确提示刷新重试，并移除失败/加载时的空候选提示。
`ui-r14-copy-green.log` 为 189/189，类型检查、探针类型检查和定向 lint exit 0。

`ui-r14-copy-browser.log` **74/74，exit 0**，冻结目录
`ui-redesign-2026-09-26T02-13-48-726Z`；增加中文、德文与泰米尔语 200% 文本。
实际查看中文 320×480 深浅两色失败截图和 200% 大字失败截图：
恢复提示清楚、重复说明消失，底部控制固定，大字正文在面板内滚动。
该轮发生在下面的发现期限修补前，不移用为新代码的浏览器证明。

随后新回归证明列表请求一直未结算时，刷新一直禁用；
`ui-target-deadline-red.log` **7 failed / 1 passed**。
只读目标发现新增 **10 秒 UI 期限**，到期清除旧候选、呈现可重试错误；
旧请求成功/失败都不能改变新请求的 busy、错误或候选，卸载取消自己的计时器。
这不声称中止了底层原生请求，不释放输入占用，不重放动作，也不自动授权或截图。
`ui-target-deadline-green.log` 为 **59/59**，含 8 个新增期限/迟到回包测试。
`ui-r14-final-unit.log` **197/197，23 files**；`ui-r14-final-typecheck.log`、
`ui-r14-final-probe-typecheck.log`、`ui-r14-final-lint.log` 均 exit 0。

最终 `ui-r14-final-browser.log` **79/79，exit 0**，冻结目录
`ui-redesign-2026-09-26T02-18-08-886Z`；79 个报告无 page error。
新增 5 个实际浏览器时间的超时场景，覆盖四种来源及中文界面；不替换产品计时器。
回包超时后刷新可用，新请求期间释放旧回包，不得结束新 busy、恢复旧授权或自动截图。
深浅色、窄窗、长译文、大字、固定控制区与恢复后的键盘选择仍全部验证。
本轮再次查看当前中文超时截图，错误文案与版式符合预期。
原 App seed 前后均 `c4430eb59a12744c3a7e8bea7334bc9a4a70bef40d99e4f9ed65fa9384bc88b2`。
`ui-r14-source-receipt.json` 记录此批关键源码/15 语言散列与准确 artifact。
所有上述日志在 `tools/computer-use-probe/.run/worker-serial-20260926T091254/`。
这仍是编译的产品 UI + mocked Host IPC，不等同当前安装 App、原生权限或模型控制成功。

## 未完成的最终版门禁

- Windows/macOS arm64+x64/Linux X11/GNOME native Wayland 的完整实际能力与原生验证；
  macOS 仍存在 root-only 观察、固定坐标元素点击和未实现输入类型，不能称功能齐备。
- WebView 运行中脚本的精确取消、共享 renderer 隔离及完整原生关闭性能。
- Chrome/Edge 正式扩展、签名安装、运行时更新/回滚与 macOS bundle 完整性。
- 重设计 UI 的安装版交互/无障碍/权限路径、真实 Grok E4、冻结 12 小时 active soak。

目标保持 active；任何测试数量都不替代逐要求的最终验收。

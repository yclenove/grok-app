# D12：内嵌页面多绑定与最终原生提交检查点

2026-10-06；`feat/computer-use-implementation`，单 Codex writer，未 commit/push/PR/发布。
整体 active / partial — not releasable。当前用户试用包仍 app115，没有新待授权卡片。

## 本批改变

WebView adapter 按完整执行 target 保留多个 binding，不同页面可以属于不同 session/run。
同一 live native page 不能被另一任务抢走；同 owner 重绑定生成新 ID，旧 release 只清自己的 binding。
按 run 过滤之后才读元数据，避免读取其它任务的页面。原 native operation 的共享占用仍由
同一 ScriptTracker 保留，release/rebind/caller 取消不等于原脚本已经结束。

Core 新增非序列化 WebViewPage proof，包含 target/native instance/document/session/run。
最终 TaskGrant publication 已路由到真实 App adapter：持有 binding registry，再按稳定顺序
锁原 native document，最后执行一次 authority-only callback。刷新、Close、绑定替换、
原脚本仍 pending、外 session/run/executor 均不能发布；不通过 target_alive 降级。
锁顺序为 Broker → App binding → original native documents → existing shares → finalize；
回调中没有 Tauri query、metadata、DOM/AX、脚本、I/O 或重试。

这是已绑定执行身份的基础，不是完整 D12。新任务的 candidate → reservation/run → binding
转换、original picker/ACP publication、前端与真人 Grok 仍未接，descriptor 没有开放。

## 来源与检查

证据根 `E = tools/computer-use-probe/.run/codex-u04-existing-app-20261004`。

- source122：3334 件，manifest `e4d91436926faa1b302e2f063e101d91cbfafa84c0a9b61fb9a8c1c96f968245`。
- 真实 default lib harness：`E/app-artifact-123.json`，编译 exit0，嵌入 Windows test manifest。
- WebView 84、相关命令 15、ACP task 42 通过，分别见 `webview-authority-app-122.txt`、
  `webview-authority-command-tests-122.txt`、`webview-authority-acp-122.txt`。
- Core default 1110 / test-support 1126 通过；新增 Core 路由 6 项全部通过。
- Core default/support、App default/probe 四种 all-targets Clippy `-D warnings` exit0。
- fmt/quality/diff exit0；App+Workbench 13573、≥1000 行文件 80，未增长。
- 无前端代码变更，未重跑完整前端/其它平台/installed/模型/12h矩阵，不借历史数字。

首败保留：artifact120 map 转换遗漏、Core fixture 少 trait methods、artifact122 debug fixture
函数名被局部变量遮蔽，已修复后 fresh source122/artifact123 通过。
`webview-authority-commands-122.txt` 的错误 filter 实际 0 tests，runner 拒绝，不计通过。
随后正确 filter 15 项通过；不覆盖首败证据。

## 真实 Windows App / WebView2

`E/webview-authority-app-122/artifact.json`：默认 debug/custom-protocol/no-bundle App，
EXE SHA-256 `cd2aa13d340b2614354fb8ef4e631188d58ee5aa7dd288756008f20065713a5f`。
复用 164 件未改变 dist，构建前后逐件 hash 相同；没有新 UI build/portable package。

原生结果 `native-result.json` 为 ok=true：原 side_browser_host 创建两个真实 native child，
原页先完成中文保存1/计数1；不同页面 group callback 恰好一次，重复 native/抢占/旧 proof 拒绝。
释放一个成员不影响另一个；转交后旧 release 不影响新 owner，外 run metadata 拒绝。
两个独立只读 oracle 最终分别为 `转交后的页面 你好` / saves2 / count2 与
`独立第二页 中文` / saves1 / count3。第二页 reload 后旧 group 拒绝，第一页值保持，
第二页回到未保存/0/0；所有精确 binding 与 child 正常释放。

Host bind 为 synthetic：`taskGrantPath=false`、`realModel=false`，没有真人批准/ACP/MCP/model。
不能把 native fixture 和 Core 路由测试拼成一条新 TaskGrant 产品验收。
macOS/WebKit、Linux/webkit2gtk、本机完整导航/history/文件流本批 not_run。

原 App PID20172，创建 `2026-10-05T18:00:16.4808810Z`，原句柄已 join exit0。
首 runner 在 App 退出后短暂发现 exact-home owner并拒绝；没有终止进程。
独立 `native-join-verification.json` 最终核对原 owner、exact EXE/home 无残留，source/EXE hash 不变。
轮询只捕获 App 身份，不能声称完整历史 WebView2 child 账本。
native unregister-class 1412 stderr 与 runner 首败保留；后者也曾错误检查 inherited LASTEXITCODE，
改用 PowerShell success 后复验成功，不把 runner 首次 exit1 写成 exit0。

本批真实加载后的 settings 已核对 independent、closeToTray=false、keepTrayForSchedules=false，
home 保留，未导入登录。source118 的旧最小 settings 缺必填字段，退出后实际文件回到 defaults；
此前 owners.json 的 false 值只记了启动声明，不能证明 App 实际加载了这些值。
source118 App 正常退出和局部业务证据仍成立；本批已用完整必填配置与退出后复验修正该验收缺口。

## 收尾与下一步

原 App/测试/编译 owners 均已退出，按 `E/webview-authority-cache-122-plan.json` 执行精确清理：
17 个旧 incremental generations 已删除，逻辑 10105147309 bytes（9.41GiB），
H 空闲观察增加 7703597056 bytes（7.17GiB）。后者为卷空闲变化，非隔离的物理分配测量；
hardlink与并行磁盘活动可能使两者不同。不在回收站，cargo重编译可再生成。
每个精确旧target/保留新generation/重建方式见同名 `cleanup.json`；source122/artifact123、
当前cache/seed/fix.patch清后hash一致。未知working/跨variant/失败现场继续保留。
app115 EXE/ZIP、原 home/凭据/会话、seed/依赖/fix.patch 与全部证据保留。
app115 EXE/ZIP及当前native EXE已单独复验hash；没有停止其它工作区/安装或浏览器。

继续 [D12 接入合同](2026-10-06-computer-use-app-webview-task-next.md) 的候选生产与身份转换。
先让 original task picker 保留未绑定 receipt，再由原 reservation 分配新 run 并转换为执行 proof；
最终用本批真实 document guard 发布。不能将旧 legacy binding 当作新 TaskGrant，也不能先开 descriptor。
四 surface/三平台/两 Mac 架构、installed 升级回滚、模型、12h、UI UX 与 app115 真人恢复仍开放。

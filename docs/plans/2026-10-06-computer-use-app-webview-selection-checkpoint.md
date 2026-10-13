# D12：内嵌页面选择与真实原生效果检查点

2026-10-06；`feat/computer-use-implementation`，单 Codex writer。本批未 commit/push/PR/发布。
整体仍 active / partial — not releasable；四 surface、三平台、两 Mac 架构及原交付范围保持。

## 本批改变

`WebViewSelection` 是 Host 内部的非序列化收据，保留原 `LiveWebView` 对象、原生实例、
document generation 和有界标题。选择只读元数据，不绑定 run、不读 DOM/AX、不执行输入。
读取后再次核对原身份；同 URL 刷新、同 label 重建和选择期间导航都使旧收据失效。

现有 `computer_use_authorize_surface` 在交给 owned worker 前保存真实 side-browser 选择，
worker 使用 `bind_selected`，不在等待之后按 label 重新寻找页面。失败沿原授权事务收尾。
没有复制 Cookie/Token，没有启用新 TaskGrant descriptor，也没有把旧授权写成新任务授权。
原始任意 `LiveWebView` bind 仅保留在 test/probe 编译；产品入口取实际 AppHandle/native view。

source118时旧 singleton 的显式替换语义尚保留。新 TaskGrant 的多 owner 占用、reservation、最终提交原生身份
锁和前端 picker 仍未完成，不能据本批 bind 成功开放新任务入口。
资源工作台 tab 可能由多个聊天共享；应让用户明确选择实际 App-owned 页面，
不能仅凭 label 前缀推断聊天所有权，也不应无依据要求 tab 由当前聊天创建。

## 冻结来源与验证

证据根 `E = tools/computer-use-probe/.run/codex-u04-existing-app-20261004`。

- 当前 source118：3330 文件，manifest
  `f8bf9074ea81f9997ace359d5ad5119ee92d9ccf847f3e4feadc849d41ac9fd9`。
- 默认真实 lib harness `E/app-artifact-119.json`，重编译 exit0，已嵌入原 Windows 测试 manifest。
- `E/webview-selection-118.txt`：WebView 76 passed，0 failed，包含新增 7 项选择竞争/元数据回归。
- `E/webview-selection-commands-118.txt`：相关命令 15 passed，0 failed。
- `E/clippy-app-118.txt` / `E/clippy-probe-118.txt`：两种 all-targets `-D warnings` 实际 exit0。
- `E/fmt-118.txt`、`quality-118.txt`、`diff-118.txt`：实际通过；App+Workbench 13573，≥1000 行文件仍 80。
- Core/前端/完整 native 矩阵本批未全量重跑，不借 source115 的数字。

source116 的 strict 首败（未使用 bind/推测 DTO）保留，已移除未使用占位 API。
source117 的相关回归和 strict 通过后，仅继续完善 debug fixture 的 oracle readiness；
以上结论来自新 source118/artifact119，未冒用历史结果。
首次 native build runner 的 inherited LASTEXITCODE 误判在编译前中止，
`E/webview-selection-app-118/preparation-failure.md` 保留；修正 runner 后实际构建通过。

## Windows 真实内嵌网页

`E/webview-selection-app-118/artifact.json`：实际默认 App 可执行文件，debug/custom-protocol/no-bundle，
没有 computer-use-probe feature。复用未改变的 164 件 frontend dist，构建前后逐件哈希一致；
本批没有新前端构建或新 portable 包，当前用户试用包仍 app115。
原可执行文件 SHA-256：
`3004eb86716dcf37b84997aa6fcf85ad06f48cc375d30815645943a33121ec5a`。

`E/webview-selection-app-118/native-result.json` 实际通过：

- 原 `side_browser_host::create` 创建真实 Tauri/WebView2 child。
- metadata 选择前后备注未保存、saves0/count0、无执行绑定。
- 同 URL reload 后旧收据和 bind 均拒绝；关闭后同 label 重建也拒绝旧收据。
- fresh observe/ref → SetValue → Click Save → fresh observe/ref → Click Increase，
  独立只读 oracle 得到 `内嵌选择验证 你好`、saves1、count1。
- exact run release 后 list 为空，native child 正常 close，adapterIdle=true，原 App 正常 exit0。

这是固定 debug harness 提供的 **synthetic Host bind**，`realModel=false`、`taskGrantPath=false`。
没有真人批准、新 TaskGrant、ACP/MCP 或 Grok 模型参与；不是完整 D12 产品验收。
macOS/WebKit、Linux/webkit2gtk 本批 not_run。

原 App PID33152，创建 `2026-10-05T17:14:38.4740180Z`，原句柄已 join。
`owners.json` 仅捕获到 App owner；短暂 WebView 子进程没有逐一捕获身份，不能声称完整历史子 owner 账本。
退出后按 exact 新 home 查询，除核验 shell 自身外没有 App/WebView2 残留；未终止其它安装或浏览器。
隔离 home `E/webview-selection-app-118/home` 保留，未导入原登录。
source122复查发现source118最小settings缺必填字段而回到defaults；旧owners.json中的
closeToTray/keepTrayForSchedules=false仅为启动声明，未证明实际加载。原App仍正常退出。
新批使用完整必填settings，实际加载/退出后independent与两false已核验，见
[source122检查点](2026-10-06-computer-use-app-webview-authority-checkpoint.md)。
WebView2 的 unregister-class 1412 stderr 保留，不把 exit0 改写成无诊断。

## 收尾与下一步

已构建、回归与原 App owners 退出；本批 cleanup 按 `E/webview-selection-cache-118-plan.json` 精确执行，
6 组过期代已删除，逻辑 7415489608 bytes（6.91GiB），H 空闲观察增 5018562560 bytes（4.67GiB）。
这是卷空闲变化，非隔离测量的物理分配；hardlink/并行磁盘活动可使数字不同。精确目标见同名 cleanup.json。
不在回收站，cargo 重编译可再生成。保留最新 cache、source/artifact、当前 app115 包、native executable、
seed、依赖、fix.patch、原登录 home 和全部首败；未知 working/跨 variant/失败现场继续延期。
source118、artifact119、最新各 cache、seed/fix.patch 清后哈希复验一致；native exe 与 app115 原包亦复验不变。

接 [D12 接入计划](2026-10-06-computer-use-app-webview-task-next.md) §下一批：
先原生 proof/最后提交边界与 run-binding 占用，再 original task picker/reservation/publication，
随后共享 selector/15 locales、Windows 真人新任务与暂停恢复停止，最后其它平台及完整导航/文件流。
app115 新任务 expired 的实机恢复/第二 Stop 行仍 not_run，不用本批 synthetic 结果替代。

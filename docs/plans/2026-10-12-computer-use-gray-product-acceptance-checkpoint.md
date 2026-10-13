# 普通产品入口灰度包与真人请求检查点

2026-10-12；`feat/computer-use-implementation`。完整目标仍 active / partial，不缩减四 surface、Windows/macOS 两架构/Linux X11+Wayland、安装升级回滚、UI UX、模型矩阵、冻结 12h 与 D00–D31。

## 本批实现与来源

- 新增 `tools/computer-use-probe/app-webview-upload-fixture.mjs` 与测试，作为独立、本机、只保留元数据的上传 receiver。不读取本地路径、不落盘、不向外网转发；固定 file input 与提交按钮，选择文件不会自动提交。
- 回环 Host/Origin 与随机提交路径限制；64 KiB/16 条接收记录有界。接收端计算真实 bytes/SHA256/次数，不去重，故能发现重复提交。64 KiB 是此验收页的上限，不是产品的 8 MiB 上限。
- 源冻结132：3416文件，manifest SHA256 `07f26c5592b3a9645a9b5a6359ccb23c7888880202783cf2fc969cbf1b2ab5c7`。与131仅增加这两个 fixture 文件；Rust/正式 UI/权限默认值未修改。
- 初次 lint 因 `no-control-regex` 失败，改为逐字符校验并补并发接收容量复验。后续定向 lint、8项 Node 测试、fmt/diff/final quality 实际通过。随本包私有 Node 重新执行8项也通过；没有把此前131的172/84或 strict 数字冒充本批新执行。
- 正式 `task_view::present_view` 的 app-webview probe gate 未解除。下列测试包显式编译 `computer-use-probe`，但入口是普通 `Grok.exe`，不是固定 `computer-use-app-probe.exe`。

## 新包与唯一窗口

本批原 build/package runner session57104 实际 exit0，完整 fresh 前端/桌面程序/资源及450文件 ZIP 往返回读通过；原构建 owners 已退出。

| 产物 | 实际身份 |
| --- | --- |
| 包目录 | `dist-portable/Grok_CU_20261012_app132_gray` |
| 普通入口 EXE | 96060416 bytes；SHA256 `6e078db488d353bd1c9e8e160c2d9f0754de5179594199d76af7b8dad72581d4` |
| ZIP | 209209754 bytes；SHA256 `59586d275a7765401d20ab2f4553347a349eccc4c0d1442db7fa0a90f8fabbb6` |
| 450件包内 manifest | SHA256 `937d04c18981648a9bf6d2e8d8297557e8ca6c628bf16ecabf238cacf7eff68e` |
| 当前 App | PID51460；创建 UTC `2026-10-11T19:09:34.6558748Z`；真实窗口592270 |
| 隔离 home/instance | 复用 delivery-app-98/startup-home；`com.grokapp.desktop.cu-app98-preview`；原登录/会话保留 |

`GROK_APP_ACP` 和 `GROK_CU_APP_HARNESS`/MODE/OUT/ROUNDS 在启动前明确 unset；未启用 synthetic harness。仍是 unsigned debug gray preview，不是正式发行或 installed 验收。

Computer Use 实际观察到内置前端主页与三个真实 native App 子页；没有依赖 Vite。未自动批准授权、选择文件或导出任何登录凭据。

旧 app129 在真实窗口确认新会话/空闲后正常关窗；App78724、CLI7692及其原 MCP children25416/39284 均缺失后才启动新包。不声称旧 App clean exit0，因为未持有其原退出码。app129 完整包仍作回退保留。

当前权威窗口记录为 `.codex-window-check/current-preview-20261012-gray.json`；旧 portable record 已标 retired。

## 真实 Grok 请求已发起，不等于执行已验收

实际 App 会话：`8f893210-4fcc-4345-b910-45251d2099e8`，标题“内嵌网页与上传验收”。实际 UI 显示 Grok4.7/极高，原 CLI owner1560。

从正常产品输入框只提交一次公开本机验收任务，要求原 `grok-computer-use` 工具，不许终端/任意脚本/外置浏览器代做。实际模型产生 app-webview 的 observe/input 请求，任务卡显示“等待你允许”和“选择本任务可操作的应用页面”；不是编造 DTO 或调用固定 harness。

已请求用户手动选择 A/B/上传三个已打开页面并批准。截止本检查点尚未收到批准确认；批准前独立 receiver 回读：A/B note=未保存、count=0、events=[]，uploads=[]。没有拿任务卡出现等同于允许/输入/上传成功。

原请求已挂起等待，不能在过期后自动重新申请、复活旧 key 或另建聊天。用户若选择/批准，要复验当前 App/会话/原请求及原 fixture owners 后继续。

## 当前验收 owners 与预期后置条件

- notes fixture PID34388、tool session47802；127.0.0.1:1285 的 `/a`、`/b` 与 `/state`；同端口也监听localhost解析的IPv6。
- upload fixture PID16068、tool session60528；127.0.0.1:1689 的 `/` 与 `/state`，仅IPv4明确URL。
- 两者均用本包私有 Node 启动；正常收尾用各原 stdin `quit` 并 join，不停无关 Node/browser。
- 公开 sample：delivery-app-132/upload-fixture-public.txt，198 bytes，SHA256 `5e29526cf2fe5fecd9458f41f3df39f776261731ab5ee3a33600d7601524d598`。只由用户在原 Host 选择框选择。
- A 保存一次“甲页验收：你好，中文输入”，初次计数2；B 保存一次“乙页验收：跨页保持”且计数3；切回 A fresh observe 后只再增加1次，最终A/B均3。独立服务端应为A/B各一次 save、各三次 increment。
- 上传原调用应等待真人文件选择，赋值 verified 后观察，网站提交恰好一次；独立 receiver 应为198 bytes、上述SHA256、sequence=1。赋值 verified 不代表网站已提交。
- 原模型明确 complete 和原 owners 收尾后再单独验 Pause/Continue fresh observe/active Stop、旧 ref 拒绝、选择取消、原 picker/Quit 物理退出；不要用本次正向任务代替这些合同。

## 清理

原 build/test/package owners 已结束，实际检查旧包全部清单/hash、递归无 reparse、无进程引用及 exclusive read 后，删除：

- `dist-portable/Grok_CU_20261005_app114`（过期完整目录）。
- `dist-portable/Grok_CU_20261006_app115.zip`（完整 app115 目录仍保留，ZIP为重复）。

逻辑删除742451140 bytes；H空闲净观测增加743477248 bytes。净差有并行磁盘活动，不能独占归因。旧114的精确历史字节不保证能由当前 dirty source 重造；替代包可用保留脚本按 fresh 身份重建，115 ZIP可从保留目录重新打包。

首次 cleanup preflight 因 CIM 微秒与原 GetProcess StartTime 精度不同拒绝，删除0；回读确认同一精确进程后改用原 API 的完整创建时间比较，未削弱 hash/path/owner/link 检查。首败记录保留。

当前132、正常129回退包、115未闭合恢复验收目录、全部 home/login/session、logs/hash/manifests、依赖/seed保留。活的App/CLI/native页/fixture及必要缓存延期。无整 workspace/target/shared cache 删除。

## 下一工作

等待真人原选择/批准后，核对正向业务和上传的真实原请求、页面效果与物理 owners；不代点权限或系统 picker。失败保留并定位，成功后再做恢复/Stop/导航/文件/Quit合同，最后才解除Windows正式descriptor门禁。继续其它平台/surface、D22、installed升级回滚、真实模型矩阵与同候选12h；完整交付尚未证明。

本批无 commit/push/PR/tag/Release。

# ExistingTab 新 TaskGrant 联合路径：首条实际链与浏览器兼容性

状态：active / partial — not releasable。单 Codex writer；原 dirty/untracked/fix.patch
保留，无 commit/push/PR/发布。接 E4b.2；完整 E5 与全 CU 交付没有关闭。

## 本批实际完成

原 App SessionManager picker、原 AcpClient constructor、reservation/owned approval、
原 App/process/agent/ACP publication 与 production source MV3/认证 HTTP、新 TaskGrant、
实际 session MCP 接通。不调用 legacy `mcp-open`，没有新增 HTTP/模型批准后门。
constructor 仅抽取共用实现，测试注入隔离 preference writer/owned worker；生产依旧调用
原 task_access::approve。私有 child NDJSON 只传所需 fixture DTO，不为内部 CommitOutcome
增加生产 Serialize。原 Job 保留到退出，失败保留 profile，成功退出才删除自己的 profile。

实际 9 检查：

1. off request/readback，grant/execution/页面正文读/截图/input 为零。
2. 扩展在原 App confirm 前 Pair 被拒；错误 nonce 拒绝，原确认后真实 Pair 成功。
3. 真实 Share→Unshare→无 Share load 拒绝→重新 Share，沿同 picker 恢复 discovery。
4. 原 constructor/owned approval/原 App ACP publication 发布一个新 TaskGrant；批准前仍零页面操作。
5. 新 grant MCP 点击使独立页面计数 0→1，重复 actionId 仍 1。
6. 实际 set_value `你好`、type_text `，世界🙂`，页面最终值 `你好，世界🙂`。
7. 实际 wait verified；可观察 textarea 上 scroll verified，独立 scrollTop=60。
8. foreign session 与 stale snapshot 拒绝，不追加点击。
9. Stop 到原 Stopped 后旧 observe 拒绝；用户 Share 和 tab 保留，原 Job/profile 收尾。

两次独立运行 native-11/native-12 各 1 passed（9 checks），实际 13.41s/17.80s、exit0。
不是对 unknown action 的自动重试；每轮新 profile/session/run。Host/Share 点击、request、
ACP peer 为 synthetic；DOM/MV3/认证 HTTP/App publication/MCP/页面效果实际执行。
没有真人 toolbar activeTab、真人 App/真实 Grok、installed/三平台 E2E 或截图权限的证明。
request 本身仍由 fixture submit_access 创建，尚不是模型 MCP 的首次 request。

## 实际首败及兼容性根因

build12 缺 json macro/String，build13 内部 CommitOutcome 无 Serialize，分别 exit101；
只修 fixture 引用/DTO，build14/build15 默认 App no-run 成功并嵌原 CI manifest。
native-1/2 过早在 MV3 target 刚出现时 instrumentation，改为 popup 初始化后再装读数。
native-3～8 真实 Share 前失败：零 executeScript、browser marker 缺失，配对 200；
native-8 证明 local storage write 成功，但 local.setAccessLevel 为 TypeError。

Chromium 官方 130.0.6723.31 renderer 仅 SessionStorageArea 暴露 setAccessLevel；
140.0.7339.16 对 Local/Sync/Managed/Session 四类均暴露。134/136/138/139 对照仍仅一个。
这不是代理、模型参数或用户重复点击问题，也不能靠跳过 TRUSTED_CONTEXTS 过关。
扩展 manifest 与 INSTALL 改最低 Chrome/Edge 140，并增加 bundle 审计回归，旧浏览器在
安装阶段拒绝。managed Chromium130 seed 不加载此扩展，未修改/替换该 immutable seed。

来源（只读核查）：

- https://github.com/chromium/chromium/blob/130.0.6723.31/extensions/renderer/storage_area.cc
- https://github.com/chromium/chromium/blob/140.0.7339.16/extensions/renderer/storage_area.cc
- https://googlechromelabs.github.io/chrome-for-testing/known-good-versions-with-downloads.json

独立官方 CFT140.0.7339.16 位于证据根 native-cft140。下载 zip SHA256：
`7723bc46d9ccd45aa25009834417f9791e3125d8b0aeb810d82ca1dbc7f31678`。
该 CFT executable 的本机 Authenticode 检查为 NotSigned，不能称签名交付候选。
新 gate 明确需要 GROK_CU_JOINT_CHROME 指向隔离测试 binary；Node 复验实际 browser major
与 local API。因标准 managed seed130 不满足此人工 native gate 前提，默认测试标 manual
ignored；native-11/12 明确 IncludeIgnored 实际执行，不当作默认套件绿色证明。

native-9 persistent context.browser() 为 null，测试改用 CDP Browser.getVersion。
native-10 已通过前六项，scroll 指向未提供 element ref 的 generic div 而被拒；改为具有
真实已支持 ref 的 textarea。没有扩大 observation/selector 权限；任意容器/页面级滚动
仍不是此测试的证据，不从一个 textarea 结论外推完整输入支持。
所有编译/实测首败日志与 profile 原样保留，不因复测绿删除失败结论。

## 当前候选与验证

证据根：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。
source13：3250 源文件；manifest SHA256
`17a2d4d31cafe183b86aedd21747a2e8135bc2c5a541d3232abe8590f8082004`。
source10/11/12 为此前候选，不能冒称最终冻结。

默认 App build15 executable：
`src-tauri/target/debug/deps/grok_app_lib-63e3432e557be5b6.exe`；SHA256
`b96714ceae4a914339e20750cc79fc8d7ed1a460648cd3870bd4a9a7ffdbce66`。
固定扩展仍 44 个生产文件/15 locale/version1.1.0/同 ID；bundle SHA256
`be76389bf7dc118eadcdea48d8a539ca58b2011b70e44b154e84bdd5aa11728b`。

本批 actual exit0：native11/12（各1）、宽 CU292（2 manual ignored，其中新 joint 已另跑）、
ACP177、routing38、扩展/bundle Node184（包含新增最低版本回归）、MCP/golden38、
default strict clippy-app5/probe strict clippy-probe4、fmt3/quality3/diff3、Node syntax。
集合有重叠，不相加。App/Workbench 仍 13573 行，无 UI/i18n 新字符串。
Core生产源码和前端本批未改，没有本批全量 Core/前端/browser153 重跑。

## 精准清理

新 joint-cache、joint-runtime 清单经 inspection→execute，实际退出均0，记录均 completed。
5组旧完整 cache 逻辑4626182072 bytes，H空闲实际观察增加3619151872 bytes（约3.37GiB）。
8个失败profile中672个逐项匹配种子的副本文件及1份已解包zip共673文件，逻辑3065544105 bytes；
C/H空闲分别实际观察增加2950541312/175407104 bytes（约2.75/0.16GiB）。
两步清理不并发，观察增量合计6745100288 bytes，约6.28GiB；逻辑量与实际量不混算。
source13全部3250项、当前App/Core、immutable seed/fix.patch及保留的marker/失败记录复验不变。
删除文件不在回收站；cargo重建、从保留seed复制或按原官方URL重新下载可再生成。
不复用 E4b.2 已执行清单。只清旧完整 cache 和逐项匹配 immutable seed 的重复运行文件/
已解包重复 zip；失败 profile、未知 Windows runtime cache、日志、当前 CFT/App/Core、
seed、依赖、dirty/fix.patch 都保留。未知现场不认作垃圾；没有杀其它工作区进程。

## 下一批顺序（不缩减完整范围）

1. 让首次 request 真正经过原 session MCP/HTTP，随后原 App 决定；不由 fixture先 submit。
2. 新 grant 实际 A/B/C 多成员：A↔B fresh observe/五 typed动作，未选 C 零正文/截图/input。
3. Share 在 discover/choose/reserve/publication 各切点真实撤销；reload/re-share、连接/
   profile/lease、App/agent/ACP replacement、Stop/drop/Pause/原 Host 恢复，原 pin 胜出。
4. 满足安装/原任务 UX 门槛之后才开放 descriptor，再从广告产品入口复跑。
   当前 ExistingTab descriptor 保持关闭，raw parser Desktop-only，不扩大模型批准面。
5. 之后 WebView/其它 U04、完整 U05/U06、U07–U09/D00–D31；四 surface、三平台/
   双 Mac 架构、全部输入/恢复、签名安装升级回滚、真人 Grok 与同最终候选12h仍开放。

旧自然21530ms managed close 长尾、E4b.2 Node TypeText unknown、App worker_timeout/
PermissionDenied 首败的根因仍未证明。本批新兼容性结论不能替它们销项。

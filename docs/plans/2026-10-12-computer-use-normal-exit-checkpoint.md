# source144：正常退出取证与最新灰度候选

## 当前结论

同一 source144 原生候选的无下载 baseline、held/private-file、下载开始前五场景分别完成正常退出取证。没有修改产品退出策略，没有杀浏览器，没有放宽原30秒门禁。source138/139 的原超时失败仍保留，根因未证明；本批通过不能将其销项。

source144 fresh 默认 manifested libtest 定向153项和三组 strict、fmt/diff/final quality 通过。完整交付仍 active / partial：Windows/macOS ARM及Intel/Linux X11及Wayland、四surface、安装升级修复回滚、UI UX、真实模型矩阵、最终冻结12h与D00–D31不缩小。

唯一用户预览已更新为 app144 / PID19768（creation UTC `2026-10-11T21:53:09.3899735Z`），原home、登录和历史保持。app133/PID10928 经真实关闭按钮正常 exit0，14 个已观察原owners的held handles均signalled/exit0，不强杀。app144首页、关于页版本标识和历史消息已实看，历史连接到“就绪”，已返回新会话；这些启动检查不冒充真人CU全链验收。

## 新增内容与身份

- `src-tauri/src/computer_use/app_shell/probe_exit.rs`：隔离 debug+probe harness 退出阶段记录。正常产品入口不写该记录。
- `src-tauri/src/computer_use/webview/probe_download/lifecycle/app/idle_exit.rs`：真实 main/side WebView baseline，0下载，两个side view同browser，正常Close后原observer退休。
- 少量原 App shell/lib/WebView exports 接线，不新增产品自动清理、强杀或退出旁路。
- source144：3432源文件，manifest SHA256 `cae93221eb14ba196722b6fce39065f654302b637bd21cca3590f0c4e96b9fd7`。
- native EXE SHA256 `bd94cc9728ee037e21b0812abb458b160d7798f2936d1786cd9f23cfd691ba46`；原构建 session86832 exit0。三场景均用此候选的独立副本和全新profile/home。

证据根为 `tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`；原生run在 `src-tauri/target/codex-test-runtime/`。原始报告不覆盖、不改写。

## 实际正常退出取证

| 原生run | 原App | 原30秒门禁 | 业务及范围 |
| --- | --- | --- | --- |
| `app-normal-exit-idle-20261012-02` | 26636 / exit0 | 14 owners全退出，191ms | 真实main/side、0下载、原native observers正常退休 |
| `app-normal-exit-held-20261012-01` | 33176 / exit0 | 17 owners全退出，618ms | 两真实1MiB/private-file、callback-first与原8秒caller-unknown |
| `app-normal-exit-before-start-20261012-02` | 14600 / exit0 | 20 owners全退出，223ms | Pause/Stop/原8012ms grace/Navigate/Close；Close5009ms失败后显式重试，旧操作不重放 |

三批记录包含 `before-harness-shutdown → after-harness-shutdown → exit-requested → before-exit-cleanup → after-exit-cleanup → after-run-return`，其中exit-requested观察到两次。准确原kernel handles从观察时持有至退休；CIM微秒时间只作辅助，不用容差匹配生产100ns FILETIME。

before-start02的准确main browser23096、side browser43000及两个crashpad44096/60096均退出。原报告把CLI67244标成`browser-root`、profileKind=none；按executable它实际是Grok Build CLI，不计作浏览器root，原报告不重写。runner已对以后批次改为`app-cli`/`owned-child`。

这些是原App/Wry/native业务；Host批准synthetic，realModel/ACP/MCP/真人picker/fullDeliveryAcceptance均false。

## 外部runner失败与取证修正

- idle01的native baseline成功，但runner null-method失败，缺完整owners报告；不能称整批通过。
- before-start01五项native业务成功、退出阶段完整，但runner在`Record-Snapshot`读取 `.ExitTime.ToUniversalTime()`（line98）报null-method，原session24461 exit1。报告保留，不能补报原30秒门禁通过。
- 改为从已持有的原kernel handle直接`GetProcessTimes`读取准确creation/exit FILETIME；仍由原Process的signalled状态判断退出，不重新按PID打开进程，不延长预算。新增owner enrollment日志，减少外部runner失败丢失身份信息。
- 独立自建Process60372正常exit17，验证live时无exit FILETIME必须拒绝、退出后准确时间可读、creation不变及零handle拒绝。没有kill。
- before-start02全部20 owners的managed getter没有再次报错，故**首次null错误的底层原因仍未确证**；不能称已经定位.NET根因。直接kernel读取只是消除对可空属性读取的取证依赖。
- 首次失败的独立后来检查仅证实该fixture App/profile owners已缺失；它不反向证明原门禁。

## source144 回归与质量

`download-buffer-normal-exit-20261012-01/results.json`：默认Cargo唯一libtest副本嵌入并回读项目manifest，38下载+49WebView+66ACP =153 passed/0 failed，所选filters ignored=0；原session30546 exit0。不是完整App suite。副本SHA256 `3b14b690a480c0d26a611edf56c5107457c98c636f31531d8506e2d15a1d2fcc`。

`normal-exit-strict-20261012-01/results.json`：default-lib/default-tests/probe-tests Clippy均`-D warnings`通过，fmt/diff/final quality通过，原session90482 exit0；source前后Verify一致。App+Workbench13573、≥1000行源码文件80未增长。`FINAL: PASS`仅表示质量，不表示发布验收。

## 下一项

1. app144普通入口fresh前端/后端/完整runtime包及ZIP逐件回读已完成，已在复用原home且原133/agent/browser owners退出后更新唯一预览并实看；后续仍须真人CU全链，不把主页正常当作功能全验。
2. 原正常退出延迟仍开放：若再次复现，用准确main/side/controller/observer阶段定位，不无限复跑等绿、不kill browser当修复。
3. D22真实App crash的新run并存、受损/替换/半写、访问拒绝组合仍未完成；只有单元证据不能关闭原生要求。
4. 新模型意图→真人选择批准→真实文件picker→上传/下载独立receiver→Pause/Continue/Stop/Quit的普通产品全链仍待验；过期132请求不复活，不自动批准或选文件。
5. 其它平台/surface、installed升级修复回滚、UI UX、模型矩阵、同一最终候选12h及D00–D31继续；正式app-webview descriptor灰度gate保留，无commit/push/PR/发布。

## 完整普通入口灰度包

原build/package session57903 exit0，源码前后Verify一致。路径 `dist-portable/Grok_CU_20261012_app144_gray/Grok.exe`；EXE96382464 bytes，SHA256 `e9e38e616d172cfe10c78d05d8617678de90faa7c2b8b782e9ed53efaa6d2878`。同名ZIP209314608 bytes，SHA256 `1cb617775a2b69a953bab259a9c3a37c04b6935aa85b0836464c0afc78fd9fd7`。

普通App入口（不是computer-use-app-probe），debug custom-protocol、computer-use-probe灰度feature、独立开发identifier，unsigned。新的前端生产构建、后端、Node/Playwright/Chromium/扩展资源完整；450文件（另manifest一文件），ZIP解压后逐件校验通过。manifest SHA256 `0b7ae2a67c3407687f9294742a9216091622254f6cfee1720d8778b400de8baf`。包不依赖本地前端服务；现已启动实看普通入口，主页 document 为 `http://tauri.localhost/`，1421无监听仍可打开首页/关于/历史；真实模型CU任务与picker仍待验。

证据 `delivery-app-144/artifact.json`、build/stage/package logs。原运行时import/hygiene检查通过。Vite仍报告既有大chunk（App约1.9MB minified），linker输出建库提示warning；未隐藏这些告警，build exit0不等于没有技术债。

启动补证为 `delivery-app-144/startup-process.json`、`previous-preview-retirement.json`、`startup-visual-verification.json`；原artifact构建记录的launched=false保留其当时状态，不覆盖历史。未提交新模型任务、未复活132过期请求、未自动批准/选择文件。唯一预览留在首页供用户主动测试。该启动批没有构建或生成可丢弃副本，额外清理0 bytes；133包/ZIP暂作已知可用回退，原home与失败证据保留。

## 批末清理

原build/test/native owners退出后，删除idle02与held01的两份byte-identical native EXE、obsolete source143成功manifested libtest副本；source144当前native与libtest、app144包/ZIP、app133活动预览、所有logs/reports/home/首败/immutable seeds保留。

`normal-exit-cleanup-20261012-01.json`及逐条removals日志记录准确路径/hash/原owners核验/再生成方法。逻辑266984448 bytes；H空闲即时观测增加266985472 bytes（约254.6MiB），不能独占归因并发磁盘活动。不确定owner的cache/PDB、失败runtime/profile、其它工作区产物暂缓；未杀无关进程，未递归删除工作区/target/shared caches。

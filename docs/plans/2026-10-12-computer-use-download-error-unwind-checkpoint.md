# 原 App 下载错误收尾检查点

2026-10-12，`feat/computer-use-implementation`，HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`。
本批未 commit/push/PR/发布，整体仍 **active / partial — not releasable**。

## 实现与独立负例

新增 `probe_download/lifecycle/app/error_unwind.rs` 和模式 `webview-download-error-unwind`，
仅走原 Windows debug/probe 隔离入口；普通产品入口和五秒 Close deadline 未改变。
收到真实 16,384 字节后，native/Wry 已终止但 wryProcessed=0、原 caller 等待、原 view
保留且隐藏时触发 fixture 错误。保留原 worker 的真实 Receiver，不制造结果。
fault 不显式 release；原 Completion guard 停止原 run、在原 controller 释放 captured
callback，然后原 PendingDownload Drop/join。不按 label 找替代页面、不制造终态、不重播。

负例仍须报告 ok=false、原 App exit2。runner exit0 表示正确验证了预期错误和收尾，
不是把错误改写为成功。原 callback/caller、StopState、idle、native 资源、staging、两次
原 main IPC Close 回执、keeper 和同 label 新 owner 的文件 hash 分别验证。

## 同一候选的实机证据

证据根 `src-tauri/target/codex-test-runtime/`，新构建日志
`app-download-unwind-build-20261012-01.jsonl` / `.stderr.log`，exit0。
四轮 EXE SHA256 均为 `450e2ff094875d6b721a40402f927058a66c9f6176fc938798552c346841813f`。

| 原生轮次 | 结果 | 原 App exit / owners |
| --- | --- | --- |
| app-download-error-unwind-20261012-01 | held 时错误，24ms 收尾；原 callback 一次、caller=interrupted、资源/staging 退休，两 IPC Close 完成，同 label 新 owner 37-byte/hash 正确 | 2 / 10 均退出 |
| app-download-close-regression-20261012-01 | 并发 Close/五秒失败后显式 retry 两场景通过；cutoff5016ms、释放后44/21ms；无重播、keeper/UI 可用 | 0 / 10 均退出 |
| app-upload-native-20261012-01 | adapter8场景/实际4POST；8MiB完整接收1849ms；header前EOF空连接1条 | 0 / 7 均退出 |
| app-upload-negative-20261012-01 | 独立 home closeToTray=true 被 gate 拒绝，result=null | 2 / 1 退出 |

下载走原 TaskGrant/许可，Host approval/publication pin 仍 synthetic。上传只证明 adapter，
文件同意和 TaskGrant 未接。realAcp/realMcp/realModel 均 false，未导入登录、不是 fresh 用户包。
2026-10-11 原首败 report/log/home/EXE/owner barrier 原记录保留，未改写为成功。

## 回归与质量

`app-download-unwind-regression-20261012-01/` 的 Cargo 唯一 libtest 执行副本先做项目
Common Controls v6 manifest embed/readback，不裸跑 harness。
probe 原 harness SHA256 `c1dd0c924cecfa0428320507956e8316edb5b42ee529e7cfb2efb8cdbadd3f14`；
后链副本 `286ce35f0ecfcded528645a56364f3b00f3e288f7325c8da3a8dde74333792c7`。
probe WebView139/Host9/blob4、Core upload2，去重154 passed，零失败/ignored。
默认 feature WebView另跑133/blob4，与probe重叠不累加。最初过窄过滤49/0的日志保留，
不作为完整覆盖；看 default-webview-tests / probe-webview-tests / probe-host-tests.log。
DOM/polyfill/App fixture27 passed。App probe、App默认、独立probe crate三组strict实际exit0。
fmt/PS parser/diff/本批源LF通过，final质量PASS：App+Workbench13573、≥1000文件80/80未增长。

## 清理与用户预览

原 build/test/native owners 已退出。校验精确绝对路径、全部祖先无reparse、原kernel身份
不再live、exact-home无owner、当前候选hash不变后，仅删除以下目录内的重复
`computer-use-app-probe.exe`（目录/home/报告/日志均保留）：

- app-download-close-regression-20261012-01
- app-upload-native-20261012-01
- app-upload-negative-20261012-01

每份94376960 bytes，逻辑283130880 bytes。H空闲实测261298458624→261581598720，
观察增加283140096 bytes（约270MiB）；观察差值可能受其它并发磁盘活动影响。
文件不在回收站，可从保留的 app-download-error-unwind-20261012-01/computer-use-app-probe.exe
按同SHA256精确复制再生，不覆盖旧home/report重新跑。旧已拒绝EXE/cache清理未重试，仍暂缓。
当前两个libtest、native候选、依赖/seed/source/WIP、全部首败和登录数据保持。
用户预览仍app115/PID62200，独立home不变；上一轮已截图确认主页，不是本批新代码包，未退出用户试用窗口。

## 下一项

error unwind独立缺口已关闭，开始[上传完整接线](2026-10-12-computer-use-upload-integration-next.md)。
文件同意/Broker许可账本/UI/ACP/MCP/model/fresh用户包仍开放，不宣称上传产品可用。
D22异常维护、Windows/macOS arm64/Intel/Linux X11/Wayland、四surfaces、安装升级回滚、
完整模型/UI UX/同冻结候选12h active soak/分批交付保持原范围。

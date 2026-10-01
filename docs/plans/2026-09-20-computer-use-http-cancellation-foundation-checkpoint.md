# Computer Use：可取消 HTTP 基础层检查点

日期：2026-09-20（Asia/Shanghai）
工作区：`H:\aicoding\grok-app-computer-use`
分支：`feat/computer-use-implementation`；HEAD：`30757366`
总状态：**partial — not releasable**。

## 本批结论与边界

可取消的 loopback HTTP 基础层已实现并通过真实 TCP 测试；现有生产 HTTP
入口已共用新的异步 exchange，但保持同步 Host API。**尚未将 Broker 取消
句柄接入生产 capture/act/open 等请求，不能宣称停止/暂停已经即时完成。**

原因不是缺少一个参数：`pause_due_to` 与 `fence_stop` 都取消同一个 token；
managed adapter 的 `abort` 目前直接返回成功、`is_idle` 直接返回 true。
若现在直接让 capture/act 提前返回，Host admission 会释放，而 Playwright
可能仍在工作。暂停后恢复可能早于远端操作结束；socket EOF 也不能证明
浏览器已经停止。下一批必须先补远端操作生命周期，再传播 token。

本批没有修改 worker JS、前端或默认开关，没有读取真实账号/profile 或改代理，
没有 commit/push/PR。既有工作树改动全部保留。

## 已完成

1. `ActionCancellation` 保留 sticky 原子状态，增加多监听者 Notify。
   先注册再检查状态，取消不会丢唤醒，晚注册及重复 cancel 均支持。
2. `bounded_loopback_post_cancellable` 在请求所属的 current-thread Tokio runtime
   中 select 完整 send/header/body 与取消。已有非取消 API 委托同一实现。
3. 在 Tokio 调用者中依旧使用 plain scoped thread；runtime/client/socket
   tasks 全部结束、线程 join 后才返回。未采用 detached 阻塞线程或后台 runtime。
4. pre-cancel 返回 `worker_cancelled/not_started`，监听器确认零连接；派发后
   取消返回 `worker_cancelled/unknown`，不重试、不声称远端动作没有发生。
5. loopback-only、no-proxy、no-redirect、JSON/响应字节上限、错误脱敏保留。
   localhost 使用静态 IPv4/IPv6 地址，避免取消后遗留阻塞 DNS 任务。
   半包正文超时现在明确为 `worker_timeout/unknown`，不再泛化为非法响应。
6. 修订 blocking HTTP ADR，说明同步接口与异步 exchange、物理停止边界。

涉及产品源码：`execution.rs`、`browser/worker_http.rs`、`browser.rs`。
新增测试：`browser/worker_http_cancel_tests.rs`、
`broker/tests_http_cancellation.rs`；扩展 `broker/tests_async_stop.rs` 的测试 worker。
测试 worker 的 cancellable capture **不等于生产 LoopbackPlaywrightWorker 已接通**。

## 首败与恢复

日志根：`tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/`。

`http-cancel-first-red.log`：新取消 API 先委托旧 blocking 实现以执行行为断言，
3 项全部失败，而非编译失败。pre-cancel 仍产生连接；等待 headers/body 的
取消没有断开等待，最后分别得到 worker_transport / invalid_worker_response。
随后替换真实实现，没有保留忽略 token 的临时分支，没有放宽首败断言。

## 已执行证据

| 门禁 | 实际结果 | 日志 |
| --- | --- | --- |
| HTTP 定向测试 | 11 passed，0 ignored；含 24 次 completion/cancel 竞争 | `http-cancel-green-02.log` |
| Broker 清理门控 | 1 passed；本地 socket 退出后仍 StopRequested，远端 cleanup 放行后才 Stopped | `http-cancel-broker-01.log` |
| 完整 Core + Driver | 386 + 12 passed，0 ignored，exit 0 | `http-cancel-core.log` |
| Core Clippy | all-targets/all-features，`-D warnings`，exit 0 | `http-cancel-clippy-core.log` |
| App Clippy | all-targets + computer-use-probe，`-D warnings`，exit 0 | `http-cancel-clippy-app.log` |
| 重建 cu_probe | exit 0；保留已有 linker stdout 库创建警告 | `http-cancel-probe-build.log` |
| source managed 真链 | exit 0；旧 ref 点击、Wait、preview、独立 oracle=2、shutdown | `http-cancel-source-preview.log` |
| seed managed 真链 | exit 0；实际 seed worker，同样独立后置条件 | `http-cancel-packaged-preview.log` |
| seed 完整性 check | exit 0，manifest/tree 未变化 | `http-cancel-seed-check.log` |
| Code quality | final PASS，千行文件 80/80 | `http-cancel-quality.log` |
| fmt / diff whitespace | exit 0；保留既有 Git LF/CRLF 提示 | 工具输出 |

TCP 用例覆盖 pre-cancel、等待响应头、半包正文、正常返回后取消、并发返回竞争、
头/正文超时、无 Content-Length 超大正文、Tokio 调用环境、localhost。
独立 server 读取 EOF/reset，确认 socket 关闭，并检查竞争/超时没有第二连接。
Broker 门控使用测试 HTTP worker，不冒充真实 Playwright Stop 竞争。

真实 source/seed 探针证明新 HTTP exchange 未破坏既有受管观察/动作/Wait/preview，
不是取消接线验收，不是安装版、ExistingTab typed actions 或真实模型验收。
本批未重跑完整 browser Node、扩展/MCP 34 项、前端、安装或跨平台矩阵。

248 文件局部源码指纹：
`EE8FFFC1840DBBCEC8F73F0ECB2073AF2C37048B11AF933DF58A94B0E96D3BE7`。
范围/算法同 managed Wait 检查点；本批新增两个测试文件，非全仓 C6 候选冻结。
日志：`http-cancel-source-fingerprint.log`。
cu_probe SHA256：`23309E25CC60C19BC9C5B168AF18CBEAC09522CD029110928C7D57C2C8908C0A`。

实机探针后 read-only 检查无 cu_probe、owned private Node/测试 Chrome 残留；
HEAD 未变、index 空。runtime seed 本批未修改，实机探针之后完整 check exit 0。
manifest：`b5f6dc83d3dc70260c01797b949ce92c2d7578ea773ca01bc68ebba90b37b485`；
tree：`08998ee277b58b6231720832d9524b74175d90fa0e7774ab7b3ddb4df72f81ec`。

## 下一批必须按以下顺序实施

1. **先复现暂停竞争。** 门控真实 worker 的 act/Wait/capture，在 HTTP 结束前后
   触发 pause/resume/Stop。独立页面副作用计数器验证不能提前恢复。分别记本地
   请求退出、远端操作结束和浏览器 context 关闭，不能合并为一个布尔值。
2. **明确远端 operation/quiescence 协议。** 身份须由 Host 产生、绑定 owner 和
   当前 generation，不从模型参数/新 lookup 临时生成。跟踪预检、准备、执行、
   回收、最终完成；任何取消/超时/丢响应都不能在 remote idle 确认前释放权限。
   队列/结果/墓碑有界，旧请求不能重新创建有效 token。能力预检拒绝旧 worker。
3. **实现真实暂停与停止差异。** 暂停撤销当前操作且等待物理结束，保留 owned
   profile；Stop 另有关闭/回收确认。abort/is_idle 必须反映真实状态。失败继续
   pending，晚回调不能复活。open 尚未返回时的取消必须覆盖 opening_profiles
   及 worker launch 后检查，不能因 profiles 尚空就省略清理。
4. **再传播同一次准入的 cancellation。** 覆盖 CaptureOptions、ManagedTabAction/
   ManagedWorkerAction、typed Wait、navigate/download/upload、new-tab/popup/open，
   不只接最容易的 observe。清理请求不能使用已取消的业务 token。避免 owner map
   删后重建或 thread-local 的隐式上下文。timeout/cancel 的 Unknown 永不重放。
5. **验收生产真链。** source + 重新生成的 seed + 实际 App Broker/MCP，覆盖
   dispatch 前取消、headers/body 卡住、执行准备中取消、回收失败、正常完成竞争、
   两 run 隔离、旧 generation、暂停恢复、Stop 清理重试与 profile 正在启动。
   页面 oracle/进程资源验证完毕后才称 managed HTTP 取消接线完成。

之后继续原 C3 ExistingTab typed actions 和 C4/C5/C6，不豁免 toolbar activeTab、
真实截图、完整 App 生命周期、Edge/安装、macOS arm64/x64、Linux X11/GNOME native
Wayland、真实模型和同一候选至少 12 小时主动长稳。保持 partial，不提交推送。

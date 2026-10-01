# Computer Use：Windows 更新独立派发

日期：2026-09-30（本机 +08:00）。工作区：`H:\aicoding\grok-app-computer-use`。
分支：`feat/computer-use-implementation`；HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。

**原始完整 goal 继续 active / partial — not releasable。** 前一轮与本轮均为 progress。
本轮不是再写一份计划：已将生产 Windows 更新的 OS 派发从主 App 转移到先验证、后持久化授权的独立进程，并验证两处真实父进程死亡窗口。该增量不能替代安装完成/回滚对账，更不能替代全平台最终验收。

## 生产链路

1. 原签名包、精确 installer member、原生 UTF-16 launch plan 仍先于 App cleanup 检查。清理成功后，主 App 复制自身绑定/hash 匹配的映像到受保护 store，启动 job-breakaway/no-window 私有副本；不让 helper 映射已安装 App 的原路径。
2. App main 的新保留入口在 Tauri/UI 初始化前提供编译进二进制的 updater key、App 名称/版本、target、mode 与参数。只有 updater-enabled 非 debug 构建可进入；运行时环境变量、IPC、journal 公钥不能成为信任来源。
3. helper 独立复验自身文件名/hash、完整可信配置、原签名 payload、唯一 member 和保留文件，再构造 launch plan。DPAPI Ready 绑定候选指纹、ticket 和精确 PID/创建时间；**Ready 本身不授权派发**。
4. 主 App 原子发布 Prepared→LaunchIntent 的一次性 helper 授权。helper 只消费属于自身 PID/创建时间的授权，只调用一次 ShellExecuteExW，并从该调用开始持有原 OS handle；不再依赖主 App 在 OS 接收后交出句柄。
5. 独立 Accepted/Rejected/Exited 回执不可覆盖，绑定完整候选指纹及 ticket。主 App 导入证据后才走退出清理。若接受回执未成功发布，匹配的原句柄退出证据可证明接受，但**不能证明安装成功**。
6. 只有明确 OS 拒绝可以恢复同一 Prepared nonce；已经有 outcome 的 ticket 不可再次授权。迟到拒绝可修复 blocked 内存记录，但须重新验签/验文件/绑定配置并重新清理，不能因超时或进程消失授予重试。
7. 已授权且仍存活的 helper 不再有十秒 ACK 终止条件：慢 Shell/UAC 交接继续由原任务等待，UI waiter 取消不结束它；准备阶段超时仍不发授权。worker 死亡、证据缺失/冲突和不确定写入保持隔离，不自动重启派发。
8. helper 读取完整原子 journal 快照时允许 READ|DELETE 共享、拒绝 WRITE，避免轮询阻止主 App 原子替换。原包/installer 的严格文件 lease 没有放松。旧 observer 入口和历史 witness 记录继续可读，旧主进程派发/交接仅保留为回归测试。

## 已执行验证

证据目录：`tools/computer-use-probe/.run/windows-update-independent-dispatch-20260930/`。

| 项目 | 已验证结果 | 范围说明 |
| --- | --- | --- |
| 默认 updater 全库 | **64 passed，7 ignored** | 7 个专属 child 入口由父测试真实启动；不是安装版验收 |
| 独立 no-zip updater | **63 passed，7 ignored** | artifact features 精确为 rustls-tls；不包含 zip |
| App 原生 | **38/38** | updater 7、shutdown 3、browser-owner 7、MCP-owner 21；完整编译可用 1733 项，未声称全部执行 |
| 前端 | **135/135，5 文件** | useUpdater 35、updateRecovery 3、appUpdateHonesty 46、About 9、locale 42 |
| 严格检查 | 全部通过、两套 Clippy 无 warning | App/vendor lib/tests + 独立 no-zip，TS、8 文件 ESLint、workspace/vendor fmt、diff |
| 源冻结 | **85 项选定源/依赖零漂移** | 不是整个最终发行候选冻结 |
| 依赖 | App lock 不变 | no-zip 420 包中 419 依赖均已在 App lock 固定；仅本地校验 consumer 新增 |
| 真实新 App debug EXE 副本 | 保留入口测试通过 | 新 dispatch 单参数、附加参数及运行时假 key 均拒绝为 65；旧 observer 仍保存原进程 259 回执并退出 0 |

新增原生测试以 **PUBLIC、确定性、仅测试用 key** 签名现场编译的专属非安装器 PE，运行生产签名验证/launch plan/Shell 派发代码；不跳过签名或把 mock 的成功当作 OS 接收。该测试 key、脚本、barrier 不进入产品运行路径，最终用户不需要 Node/Rust。

- 独立父测试进程在 durable grant 后、OS launch 前实际 `process::exit(73)`；另一场景在 OS 接收后、接受回执发布前实际退出。两者均由仍存活的独立 worker 保存原进程退出 **259**，nonce 保持不变，重派发被拒。
- 完整生产事务使用专属 PE 获得真实 OS 接收；测试退出 callback 必定在 `process::exit` 前 panic，验证 Accepted fence 不变。后续生产 pending 导入真实 **1603**，仍为 blocked，不误报安装成功或允许再次清理/启动。
- 已授权 helper 被测试持有的精确 Child handle 终止后，原 Intent 仍不可重放。未授权 Ready 不能启动，创建时间不匹配不能授权；旧 outcome ticket 不可复用。
- 真实 OS 拒绝恢复原候选；迟到拒绝的生产 pending 分支复验后恢复同 nonce 并重跑 guard。候选内容重绑定、不同 key/config/image、篡改原文件、冲突 outcome/exit 均拒绝。
- 慢交接测试持有真实 helper **超过 11 秒**，确认原等待仍存活而非十秒超时；释放 barrier 后由明确 OS 拒绝决定恢复。
- 上轮 ZIP 重名/ZIP64/截断、11,111 个转义对照、保留原候选、历史 witness 与 read-only process evidence 测试继续通过。

## 修复与证据边界

首轮 App 检查发现新入口需要显式 `Context<Wry>`，以及测试 import/旧 ready 方法仅测试使用造成 warning；已在冻结前修复，失败日志保留。一次验证器过早读取仍在生成的 compiler JSONL 被拒；等待原构建句柄完成后重跑，没有因观察超时重启构建。冻结后无源修改，所有最终测试重新执行。

没有启动真实 NSIS/MSI 安装器，没有关闭用户 App、修改权限/剪贴板/账号、读取 release 私钥、发起远程 CI、commit/push/tag/release。真实 debug App 探针只证明新保留入口拒绝和旧 observer 兼容；**不证明 enabled release worker、UAC 或签名安装态**。

本轮关闭的是已实测的**主 App 死亡导致句柄交接丢失**窗口，不宣称对 worker 死亡、机器断电、同用户恶意代码、installer 子进程委托或文件系统回滚达到 exactly-once 安装。DPAPI/write-through 也不构成这些证明。

## 下一步与完整目标

下一实现依赖仍是权威的已安装文件/版本/发布者与安装完成/回滚回执对账、保留 App/installer 身份的 journal 安全退休和下一轮候选释放；不能把当前持续 blocked 当最终可用更新功能。还需真实签名 NSIS/MSI 的安装、UAC 取消、重启、修复/回滚以及安装版 WebView 流程验收；部分服务恢复、孤儿文件清理、普通 Quit 和原生输入/剪贴板生命周期尚未据此完成。

2026-09-09 唯一执行计划的原始完整范围不变：Windows x64、macOS arm64/x64、Linux X11 与原生 GNOME Wayland；Desktop/受管浏览器/已有 tabs/App WebView；App/ACP/MCP；完整输入/中文 IME/clipboard/取消/恢复；签名安装更新回退；原生窄窗口/DPI/权限 UI；真实 Grok E4；**同一个最终冻结候选 12 小时 active soak**。未在对应真实环境验证的仍是 not_run/unverified，不能调用 complete。

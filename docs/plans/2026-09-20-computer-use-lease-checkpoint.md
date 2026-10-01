# Computer Use：连接租约与分享权限检查点

日期：2026-09-20（Asia/Shanghai）
工作区：`H:\aicoding\grok-app-computer-use`
分支：`feat/computer-use-implementation`；HEAD：`30757366`
总状态：**partial — not releasable**；未冻结候选，未提交/推送/提 PR。

这是上一份安全配对检查点之后的增量记录，不继承历史发布验收。

后续已实现分享 UI、文档绑定和候选版本，见
[分享检查点](2026-09-20-computer-use-sharing-checkpoint.md)。下文是本阶段历史证据，
其中“下一步必须继续”的未实现描述不代表最新源码状态。

## 已实现的行为

- Host 连接使用 30 秒单调时钟租约；只有完整有效的连接身份可以续租。
  每个认证/派发入口先检查到期，IPC 同生命周期的一秒 sweep 清除原始 key、
  候选与借用权限。旧心跳不能复活已过期或已轮换连接。
- MV3 每 10 秒续租；与 popup status 合并同一个五秒超时请求，停止时取消。
  存储操作串行，A 的迟到清理不得删除 B 的新配对。worker 重启不静默恢复。
- 修复浏览器 timer 的 receiver：通过 globalThis 调用 setTimeout/clearTimeout，
  不把 Worker 原生函数当 PairingClient 对象的方法调用。
- 即使定时器启动失败，已经发出的 Host key 也会撤销，本地不能残留 paired。
- Share/Unshare 后端的连接认证、顺序校验与变更在同一个 Host 锁内；Broker
  feature gate 与 feature-off 同序加锁，消除认证后再变更的竞态。
- Unshare 和新的 Share 同时失效旧 borrowed grant、观察和预览，不关闭用户 tab。
  请求携带真实 document/connection generation 与连接内递增 sequence；一个
  高水位拒绝迟到请求，不创建无限增长的逐 tab tombstone。
- 候选最多 64 个；限制标识/标题/URL 长度，严格解析 HTTP(S) URL，拒绝 URL
  用户名/密码、零 generation、零/倒退 sequence。候选授权不再写死 generation=1。
- `grant_picker_tab` 在同一锁内读取候选并登记授权；fixture 的简化分享入口仅在
  test/test-support 编译。客户端分享失败会退役配对，避免不确定的 Unshare 后继续续租。
- App 配对探针创建 owner-marked 私有 profile；Windows Job 在 Node 获准启动
  浏览器前建立。异常退出按 Job handle 清理，不按易复用的 PID 扫描杀进程。

## 首败与恢复

1. 原租约实现的 31 秒真实超时 red：旧连接不发 disconnect 仍有效。
   新实现通过同一断言；另测精确 deadline 前/等于/后的边界。
2. 新真浏览器探针在配对启动心跳时失败（`app-lease-01.log`）。Node 单测没有
   浏览器原生 timer 的 receiver 限制。修复绑定后真实 11 项通过。
3. 定时器异常回归先得到 15 pass / 1 fail，证明失败后本地错误保留 paired；
   修复后通过（后续增加分享客户端覆盖，合计 19 项）。
4. 分享协议的 schema red 后，保留了真正到达权限断言的 semantics red：
   Unshare 后还可 act、旧 document 可删除新候选、超长 tabId 被接收。
   修复后通过，另补容量、feature-off 竞争和新 document 失效旧授权。
5. 主动终止自建 Node 的首轮故障注入中，Job 进程已全部回收，profile 删除
   遇到 Windows sharing violation（os error 32）。原始根目录递归删除先删掉了
   owner marker。已改为保留 marker、只删精确 profile、最多 2.5 秒有界重试，
   最后非递归删除根。复验结果在本记录末尾更新。

## 已取得的证据

| 检查 | 结果与边界 |
| --- | --- |
| Core all-features | 344/344，0 ignored；含 31 秒真实到期断言 |
| Driver 集成 | 12/12，0 ignored |
| 扩展 PairingClient | 19/19；状态/存储/续租/分享失败与晚返回 |
| 实际 Chrome MV3 + 私有 Node + App Broker/IPC | 11/11；配对与租约，不是 tab 控制 |
| 15 locale 生成一致性 | passed；本段未增加 UI 文案 |
| 仓库代码质量 final gate | passed，千行文件 80/80；不是功能发布通过 |
| TypeScript typecheck | passed |
| 配对 API/UI/Panel 与多语言 | 69/69（4 文件）；jsdom/invoke mock，不是 App-shell |
| Core strict Clippy | passed，all-targets/all-features |
| App strict Clippy | passed，default 与 computer-use-probe 两配置 all-targets |
| Windows 探针被强制终止后的资源回收 | 第二轮 passed；原始失败与独立进程/文件 oracle 均保留 |
| 最终重编译的 11 项浏览器检查 | 11/11，退出 0；`app-lease-03.log`，使用下方 SHA256 工件 |
| Rust fmt / git diff whitespace check | passed；Git 对存量文件的 CRLF 转换提示未改配置绕过 |

11 项：stable-extension-id、pair-with-app-confirmation、no-popup-key-message、
extension-revoke、app-revoke、worker-restart、heartbeat-survives-without-popup、
browser-close-expires-host-key、late-heartbeat-rejected、
old-heartbeat-cannot-touch-new-pairing、browser-crash-expires-host-key。

Host oracle 读取原始 key 是否已存储，而不是只看 session_key() 的读时过滤。
正常关闭与 Browser.crash 均不发送 extension-disconnect；过期后实际 HTTP
返回 403。关闭 popup 后运行超过租约期限仍 paired，证明真实 worker 续租。
这些不证明已有 tab 的真实动作归还（生产 adapter 尚未实现）。

证据目录：`tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/`。
新增：`lease-red-core.log`、`lease-scheduler-red.log`、`share-red.log`、
`share-semantics-red.log`、`core-lease-sharing-final.log`、`app-lease-01.log`、
`app-lease-02.log`、`app-probe-interrupted-01.log`。首败不覆盖。

首轮中断残留目录：
`C:\Users\Administrator\AppData\Local\Temp\grok-cu-pairing-owned-f605cbe7-3b68-4f76-9191-72bbb5f729b0`。
已确认 Node/Chrome/父探针退出；手动清理命令被执行策略拒绝，目录保留，
不能报告全量零文件遗留；也未绕过策略重试删除。旧检查点更早遗留不在本次清理范围。

## 下一步必须继续

1. 本段已复验新 Job/profile 清理的正常与故障路径；下一步继续 C1。
2. C1 尚非用户功能：manifest 仍只有 storage + loopback，没有 activeTab/scripting，
   sw/popup 没有 Share 控件；先实现明确用户点击、main-frame document identity、
   navigation/close 取消，再接 App 候选与授权。禁止用测试 helper 冒充实机分享。
3. App picker 当前只传 tabId；增加可核对的候选版本，在真正注册生产 adapter
   前阻止“用户看到 A 文档，点击时授权了 B 文档”。分享序号不等于 document identity。
4. 先把整个 C1 的真浏览器两 tab 授权链跑通，再做 C2 typed transport、C3
   生产 adapter/Broker/MCP 闭环、C4 全入口生命周期。
5. C5 安装/Edge/macOS/Linux X11/GNOME native Wayland 与 C6 冻结、真实模型、
   主动长稳仍是独立未完成项。没有设备写 not_run/blocked_external，不扩大权限。

详细顺序仍见 `2026-09-20-computer-use-remaining-execution.md`。

## Windows 探针中断复验

修复后第二轮只终止校验了 parent PID、专用 profile owner 和命令行的自建
Node（36088）；未使用按进程名批量停止或扫描日常 profile 的操作。
父探针（51712）明确返回功能失败（退出 1），不把中断伪装成配对通过。
Windows Job 内 Chrome（49244）与 conhost（36204）随后均不存在；
`grok-cu-pairing-owned-5a70dfaa-98b7-4a38-8971-537ae03efe52` 整个目录不存在。
这次无 cleanup error。证据：`app-probe-interrupted-02.log` 及
`probe-interruption-oracle.json`（标明是 exec 独立检查的记录，不是自动测试输出）。

当前重编译 `cu_probe.exe` SHA256：
`37A5EC38321558C5FFBAE242F28FB1E53162A4545C87CAD978A53DDBA1EAB109`。
它只标识本段测试工件，不是完整源码/依赖/安装包 freeze。

该工件随后重新跑完全部 11 项（`app-lease-03.log`，退出 0），包含两个真实
浏览器生命周期与正常 profile/Job 清理，未沿用 `app-lease-02.log` 的旧构建。
运行后未发现本段 pairing-live / owned-profile / existing-tab-extension 活进程。
本段没有使用真实账号、Cookie、Token、日常浏览器 profile，也未修改代理/VPN。

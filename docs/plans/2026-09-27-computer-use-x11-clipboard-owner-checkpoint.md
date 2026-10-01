# Computer Use：X11 剪贴板发布、传输与保留式恢复

2026-09-27，Asia/Shanghai。分支 `feat/computer-use-implementation`，HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。原有 dirty/untracked 工作保留。上一阶段有已验证的真实快照实现，分类为 progress；本阶段增加生产代码并运行原生验收，不是只重述计划。完整目标继续 **active**，未写 complete/blocked/paused。

## 已落地的生产实现

- `clipboard_owner.rs`：将原快照和同一条 XFixes 连接交给独立 selection service；不透明、不可克隆 lease 绑定精确服务。先完成任务格式准备，再在短 X server fence 内检查原 owner/epoch 和取消令牌，确认未变化才发布一次。不能跨服务使用 lease，不能重放发布。复用原 `ServerFence`，只扩大 crate 内可见性，没有改变原桌面输入的实现。
- 任务文本提供 UTF8_STRING/TEXT/text/plain 及 UTF-8 MIME；仅能够无损编码时才提供 Latin-1 STRING。空串、NUL、超过 32 KiB、预先取消均在所有权改变之前拒绝。发送后不确定状态保留，而不假设“没有发生”然后重试。
- 恢复不把 owner 设回旧应用窗口：旧 provider 已退出后，服务仍提供快照中的全部受支持格式、原 type、8/16/32 位宽和原始字节。原本没有 owner 时恢复为 None。所有权 epoch 改变后，新复制优先；同窗口重新声明和 A→B→A 不能获得旧快照的恢复资格。
- `clipboard_send.rs`：实现 TARGETS、TIMESTAMP、MULTIPLE、普通转换与发送侧 INCR。MULTIPLE 有界解析，失败 property 标记 None，拒绝递归、重复 property、控制列表碰撞和超量；可同时包含直接结果及 INCR。不执行 DELETE 等副作用目标。
- 每个已经接受的请求和每条传输固定使用当时的数据源，恢复不能把正在读取的任务数据切换成原剪贴板。INCR 分块最多 16 KiB，保留实际位宽/type，等待删除确认，发送同类型零长度结束块并等待最后确认；使用 x11rb 展开的完整请求序号排除旧删除事件。所有权丢失后仍完成已接受的转换。
- 最多 16 条 INCR、256 项排队事件、单条 5 秒软期限；重复 key 不覆盖正在传输的数据。requestor 销毁、拒绝和到期只影响相应传输，不删除外来窗口上的数据 property。连接发送不确定时不重发该块。剪贴板传输到期**不等于原生输入完成**。
- `close` 在仍提供当前选择内容、仍有排队工作或传输时拒绝。调用方必须保留服务，直到所有权真正转移且传输结束；绝不能把 Stop/计时器/Drop 当成恢复证明。生产代码无原始剪贴板日志、Debug/Serialize 输出，也不把数据送入模型。

## 验证结果

证据目录：`tools/computer-use-probe/.run/x11-clipboard-owner-20260927/`。

| 项目 | 结果 | 实际覆盖 |
|---|---:|---|
| Linux Rust | **623/623** | X11 43 + Core 564 + 实际子进程 driver 16，零失败/忽略/筛选 |
| 原生剪贴板 | **33/33 × 3** | 原 19 项不移除，新增 14 项发布/恢复/发送验证；另外完整干净 stdout 复跑 33/33 |
| 原生语义 | **41/41 × 3** | 保留精确 native owner、迟到回执/Unknown、编辑/Wait、明确拒绝 clipboard→AX 替代 |
| 原生坐标 | **19/19 × 3** | 坐标/按键/滚动/拖动/前景/生命周期全部保留 |
| 静态与构建 | 通过 | X11/Core strict Clippy、workspace fmt、3 个 Python fixture AST、构建、git diff --check |

33 项剪贴板测试来自真实 Xvfb、独立 X11 owner/requestor 连接和两个真正 GTK 进程，不是只 mock 内部方法。新增验证包括：foreign lease、预取消/非法文本零所有权写入；独立 GTK 读取发布的 Unicode；不可重复发布/提前退出；原本空选择恢复；原 provider 退出后直接及大块 INCR 的 8/16/32 位格式逐字节恢复；已排队请求跨恢复不串数据；新复制/同窗口/ABA；Latin-1 与时间区间；MULTIPLE 成功/失败/重复/递归/碰撞/边界及混合 INCR；排队 requestor 死亡；Stop 不擅自恢复；中途恢复固定旧传输；16 条并发、重复 key、销毁、超时、所有权丢失和零结束块确认。

做了真正的故障反例：临时去掉“请求固定数据源”保护，让发送端改读当前全局数据源。原生测试实际 exit 1，报 `queued task request silently switched to saved bytes`。随后恢复准确的原文件字节并重建，最终回归全部通过。保留红测源码备份、前后 SHA、构建和失败日志；最终代码没有该变异。

冻结清单 **305 项全部零漂移**，含 X11/Core 依赖与夹具以及既有 CI。CI 本轮未改动，原 `--clipboard` 步骤自动执行扩展探针；远端 CI 未运行。清单不是整个脏仓库冻结，也不是最终发布候选。

`audit.mjs` 校验退出码、旧测试覆盖保留、各轮完整有序门禁、源摘要、红测恢复和文档读回，生成 `final-audit.json`、`receipt.json / receipt.sha256`。中间 strict Clippy 两条 lint 失败保留并修复。第三轮剪贴板 stderr 的 DBus 日志与一个完整 PASS 同行，原日志保留；额外拆分 stdout/stderr 重跑全套 33 项，避免把日志混行误判为缺失门禁。

## 仍须继续实现，不能称最终版

1. **尚未接入 LinuxAdapter 的真实剪贴板粘贴路径。** 这些是生产 crate 中的 Host 底层；显式 clipboard 输入仍明确拒绝，不用 InsertText 冒充。下一步必须把发布服务、精确输入授权、原 native action owner 和实际 paste 生命周期连起来。
2. **异步 native paste 完成仍未证明。** GTK PasteText 的方法返回会早于 clipboard 回调；一次读回匹配、读取数据完毕、INCR 到期都不能证明没有迟到输入。Unknown/Stop/迟到回复要与精确占用和恢复时机共同设计，并用真实 toolkit 故障注入验证。
3. **App/Host 持有与退出交接还未实现。** 当前 `close` 会拒绝不安全退出，但直接 Drop 活服务依然会丢掉其持有的选择数据。因此必须实现真实的持久服务生命周期、clipboard manager 接收/退出交接，及断连/恢复失败的用户可见状态；不能把“拒绝 close”算持久化完成。
4. 快照仍只接受明确支持的格式；未知私有格式拒绝而不悄悄丢弃。所有权事件不证明 provider 在不重新声明 owner 时内容不变。同步 X11 socket/round-trip 不是硬实时取消；5 秒传输期限只约束正常连接上的轮询。
5. 本阶段没有操作用户全局剪贴板、安装 App、权限提示或真实桌面；没有替换用户 runtime、commit/push/创建 PR。证据仅 WSL Debian owned Xvfb/GTK，不代表 native GNOME Wayland 或已安装 App。

完整最终范围不缩减：Windows、macOS arm64+x64、Linux X11+GNOME native Wayland；Desktop/托管浏览器/既有标签页/App WebView；App/ACP/MCP；全输入/IME/取消/恢复；签名安装/升级/回滚；重设计原生 UI 窄窗/缩放/权限；真实 Grok E4；同一最终冻结候选 **12 小时 active soak**。此前 Windows 发布/浏览器长尾、原生 UI 未执行项、无回执/断连恢复等仍开放。当前阶段有可验证进展，但完整目标未达到，继续 active。

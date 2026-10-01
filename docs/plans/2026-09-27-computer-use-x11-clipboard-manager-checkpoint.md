# Computer Use：X11 剪贴板管理器接收与退出交接

2026-09-27，Asia/Shanghai。分支 `feat/computer-use-implementation`，HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。保留现有 dirty/untracked 工作。接续回合中的工具分发失败不算开发进展；本轮重新读取工作树、补齐独立原生夹具、修复真实关闭失败并完成冻结回归，分类为 **progress**。完整目标仍 **active**；实际查询目标状态为 active，没有写 complete、blocked 或 paused。

## 本轮生产变更

- 新增 `src-tauri/computer-use-x11/src/clipboard_manager.rs`，实现一次性的 `CLIPBOARD_MANAGER / SAVE_TARGETS` 握手。只允许精确 lease 请求保存 **Restored 的原始快照**；Prepared、临时任务 Published、空原选择、外来 lease、预先取消均不能经此 API 交给历史管理器。已发送请求不能重放。
- 使用 XFixes 分别跟踪 CLIPBOARD 与 CLIPBOARD_MANAGER 的所有权 epoch，在短 X server fence 内捕获当前管理器身份和原服务所有权。用 XRes resource base/mask 绑定真实 X client，不信任 `_NET_WM_PID`，也不要求管理器的两个 selection 必须用同一个窗口。
- 新建私有 requestor，显式列出快照的所有数据 target，发送前记录 Pending。发送报错、Stop、调用方超时都不推定对方未执行，不自动重试或销毁在途 requestor。
- 只接受 requestor、selection、target、timestamp、property 全部匹配的 SelectionNotify。成功还必须满足：管理器身份及 epoch 未变、CLIPBOARD 恰好从本服务交给该管理器 client 一次、**每种保存格式**都已向该 client 交付。其他应用读取同样的原数据不能替代管理器接收证明。
- 直接转换记录已成功写入的数据 target；INCR 直到有类型的零长度结束块得到删除确认才记录完成。管理器可以像 GTK/GNOME 实现一样先接管 CLIPBOARD、后完成 INCR；已经接受的转换保留固定数据源，不因所有权转移而截断。
- 对外提供 NotRequested、Unavailable、Pending、Acknowledged、Declined、Interrupted、Unknown。过早或仅部分接收后的成功回执为 Unknown；不匹配回执保持 Pending；管理器换代、同窗口重新声明、ABA 或退出中断交接，不能当作成功。Pending/Unknown 都禁止服务关闭。
- 修复真实关闭缺陷：对方已经销毁私有 handoff requestor 时，原 close 先销毁服务窗口、随后在 DestroyWindow 的 BadWindow 上失败。现在先清理 requestor，仅把该精确私有窗口的 BadWindow 视为已清理，连接及其他错误仍上报；成功 close 可重复调用。XFixes manager watch 仅在订阅成功后发布，避免失败后重试跳过实际监视。
- 生产模块不记录或序列化剪贴板原始数据。这里的确认只是 X11 传输协议接收确认，**不是磁盘持久化、原生粘贴完成或 App 生命周期接入**。

## 实际验证

证据目录：`tools/computer-use-probe/.run/x11-clipboard-manager-20260927/`。

| 项目 | 结果 | 范围 |
|---|---:|---|
| Linux Rust | **623/623** | X11 43 + Core 564 + 实际子进程 driver 16，既有测试名单全部保留，无忽略/筛选/失败 |
| 原生剪贴板 | **46/46 × 3** | 原 33 项完整保留，增加 13 项管理器交接门禁 |
| 原生语义 | **41/41 × 3** | 精确授权、原生编辑、Wait、迟到恢复/Unknown、显式 clipboard 不替换为 AX |
| 原生坐标 | **19/19 × 3** | 原有像素、按键/按钮、滚动、拖动、前景及生命周期验证 |
| 静态/构建 | 通过 | 构建、X11/Core strict Clippy、Windows workspace fmt、3 个 Python fixture AST、diff 检查 |
| 选定源冻结 | **307/307 零漂移** | 前一阶段 305 项加 manager 实现与 fixture；不是整仓库或最终发布候选冻结 |

新 `clipboard_manager_fixture.rs` 是独立连接、独立线程、真实 X11 事件驱动的管理器 peer，不复用生产 ClipboardOwner 来伪造接收端。它实际读取 MULTIPLE 结果，接管 CLIPBOARD 后继续读取 INCR，并在生产服务及原 provider 都退出后，通过第三条连接逐字节核对原 8/16/32 位格式。大数据案例实际包含三条 INCR。

新增门禁覆盖直接/INCR 完整退出交接；无管理器时保留原数据；Prepared/Published/foreign lease/预取消零派发；负向、提前和部分回执；无关 client 读取不能满足条件；五类回执元组不匹配；Stop 后保持在途直到正确迟到回执；管理器同 owner 重声明、ABA 和退出；全部失败路径尊重新用户复制。直接成功案例还实际销毁 handoff requestor 并连续调用两次 close。

做了三个真实失败→恢复验证：

1. 管理器销毁私有 requestor 后，原生测试实际 exit 1，报 DestroyWindow / Window 错误；修复清理次序与精确 BadWindow 处理后，冻结回归通过。
2. 临时去掉“所有格式已交付”条件，EarlyAck 门禁实际 exit 1：`manager handoff expected Unknown, got Acknowledged`。
3. 临时去掉“交付给捕获的管理器 client”条件，无关 reader 门禁实际 exit 1，同样阻止了错误 Acknowledged。

两个临时变异都以原文件字节备份和前后 SHA-256 证明准确恢复，再重新构建、冻结全回归；变异没有留在生产代码。初次 fixture 编译的类型/参数错误与中间未使用项警告保留，最终 strict Clippy 未放宽。

`audit.mjs` 核对全部退出码、既有 Rust 名单、三轮完整原生门禁及顺序、307 项 SHA、红测恢复、CI/依赖未变和文档读回，输出 `final-audit.json`、`receipt.json / receipt.sha256`。首轮审计器误把历史“成员清单”当执行顺序而失败；保留 `audit-1.log/exit`，修正为先检查完整清单成员，再核对上一阶段已验证的实际执行顺序，未删除或放松门禁。所有新冻结原生日志分别保存 stdout/stderr，避免 DBus 混行。

本轮未改 CI、Cargo.toml 或 Cargo.lock。既有 CI `--clipboard` 会运行新增原生夹具，本地仍在原 45 秒上限内结束；远程 CI 未运行。

协议依据是本地留档的官方 GTK `gtk-3-24/gdk/x11/gdkdisplay-x11.c` 与 GNOME `gnome-3-18/plugins/clipboard/gsd-clipboard-manager.c`，特别核对了 SAVE_TARGETS 精确回执与“先取所有权、后收完 INCR”的时序。这些是指定历史分支的实现参考，**不是已安装桌面管理器或最新版兼容性验收**。

## 尚未完成，下一步保持原范围

1. **实际 Linux clipboard paste 仍未接入。** `TypeText via: clipboard` 仍显式拒绝，不能用 AX InsertText 代替。还要把精确输入授权、发布服务、真实 toolkit paste、原 native action owner 和恢复时机连接起来。
2. **异步 paste 完成证明仍缺失。** GTK PasteText 返回、选择传输完成、读回恰好匹配、INCR 到期或管理器确认均不能证明没有迟到编辑。Stop/Unknown/无回执/断连必须保留真实占用，不能借本次交接结果释放输入。
3. **Host/App 持有、无管理器时的保留服务和退出 UI 尚未实现。** 这里完成 manager 握手底层，不是端到端 App 退出路径。活服务被直接 Drop 仍可能丢数据；没有 manager 不能拿“拒绝 close”冒充持久化。需接入真实生命周期、helper/退出交接和用户可见失败状态。
4. 同一 X client 绑定已覆盖管理器不同窗口；分拆到其他 helper X client 的管理器未验证。X11 client 不是恶意同 display 客户端之间的安全隔离边界。同步 X11 往返无硬实时取消保证，原格式/传输容量限制仍适用。
5. API 不会主动用 SAVE_TARGETS 保存临时任务文本，但这**不保证桌面历史管理器不会自行监听临时 CLIPBOARD**。不能承诺超出协议能力的隐私或持久性。
6. 没有操作用户全局剪贴板、真实桌面、权限提示或已安装 App，没有替换用户 runtime，没有 commit/push/PR。证据仅 WSL Debian owned Xvfb、原生 X11 独立 peer 及既有 GTK/AT-SPI 夹具；实际桌面 manager、GNOME native Wayland 和其他平台不能据此计为通过。

完整最终要求保持：Windows、macOS arm64+x64、Linux X11+GNOME native Wayland；Desktop/托管浏览器/既有标签页/App WebView；App/ACP/MCP；全输入/IME/取消/恢复；签名安装/升级/回滚；重设计原生 UI 窄窗/缩放/权限；真实 Grok E4；同一最终冻结候选 **12 小时 active soak**。上述缺口及此前 Windows 长尾、原生 UI NOT_RUN 等仍开放，完整完成审计不通过，目标继续 **active**。

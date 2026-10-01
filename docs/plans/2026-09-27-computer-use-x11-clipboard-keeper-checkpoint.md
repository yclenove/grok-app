# X11 原剪贴板跨进程保留检查点

日期：2026-09-27。完整目标仍为 **active**，本阶段不是最终版或全功能完成。

上一阶段：[进程内后台保留](2026-09-27-computer-use-x11-clipboard-retention-checkpoint.md)。本轮将已恢复的原剪贴板交给独立进程，并实际验证模拟 Host 正常退出、被终止后仍可读取原格式；不把控制对象 Drop 当作进程退出，也不把数据交接当作原生粘贴完成。

## 实现与边界

- 新增 `clipboard_keeper.rs / clipboard_keeper_child.rs / clipboard_keeper_transport.rs`。`ClipboardOwner::begin_keeper` 只接受精确 lease、Restored、非空原值、未尝试过 keeper 且无待定 manager 交接的服务。Prepared、临时任务内容、外来 lease、预先取消均不启动子进程。待定 keeper 也禁止反向启动 manager 和重复 keeper，避免竞争交接。
- 通过 `/proc/self/exe` 启动当前运行的同一可执行映像；使用固定内部参数与继承的私有 Unix socketpair，不接受调用方可执行路径，不创建公开监听端点。控制帧固定 32 字节，只含协议标记、窗口、时间戳和 nonce；剪贴板原文通过既有 X11 selection 协议读取，不进入 argv、状态、文件或日志。
- 父服务保留开始交接时的 owner/epoch；子进程检查源窗口 proof、获得时间及快照前后的原生 epoch，完整读取原格式后才发送 READY。父服务在短 fence 内再次验证原 owner、Restored 状态及保留的 epoch，然后记录 Committing 并发送一次 COMMIT。子进程同样在短 fence 内复核原 owner/epoch 才取得 selection。
- 父服务只有读取精确回执、验证子窗口 proof、实际 selection owner 及预期 epoch 变化后才报告 Transferred。启动成功、管道 EOF、计时器、Stop、manager ACK 均不冒充成功。提交后的通信不确定性保留 Unknown，不重试交接或覆盖用户新复制。
- 子进程接管后不依赖父进程、控制管道或 adapter 的寿命；断开控制通道不是退出理由。新复制取代它且已接受的请求/INCR 安全结束后才退休。旧父服务交接后也仍要完成自己的已接受 INCR，不能直接截断另一个读取者。
- 实际 Child handle 交给独立 reaper；只有 `Child.wait` 成功才设置进程结束标志，等待错误不猜测退出。每个 Host 进程最多 8 个尚未终止的 keeper；控制 handle Drop 不提前释放 permit。子进程使用独立 process group；没有按名字或裸 PID 杀进程。
- `RetainedClipboard::request_process_handoff` 与只读 monitor 接入现有后台服务。最后一个控制对象释放仍只请求既有 manager 交接，**不会自动启动 keeper**。keeper 是显式生命周期 API，尚未由实际 TypeText 或 App 退出/升级流程调用。
- `src-tauri/src/main.rs` 已在 Linux 的 App 初始化之前接入专用 keeper 入口；native probe 使用相同生产入口。没有启动 GUI、App singleton 或 App 数据目录逻辑。但本轮未构建或替换已安装 Grok App，不能据此宣称安装包中的入口已验收。

窗口 nonce 用于同一 X server 上的握手关联，不是防御恶意 X11 客户端的安全隔离。选择数据接收与 keeper 交接不确认、不释放未知的 native input slot。

## 当前证据

证据目录：`tools/computer-use-probe/.run/x11-clipboard-keeper-20260927/`。

| 项目 | 结果 | 范围 |
|---|---:|---|
| Linux Rust | **628/628** | X11 48 + Core 564 + 实际子进程 driver 16；旧测试名单保留，无忽略、筛选、失败 |
| 原生剪贴板 | **63/63 × 3** | 前一阶段 54 项完整保留，新增 9 项 keeper 门禁 |
| 原生语义 | **41/41 × 3** | 既有授权、文本、Wait、迟到恢复与显式 clipboard 拒绝行为 |
| 原生坐标 | **19/19 × 3** | 既有图像、输入、焦点与生命周期门禁 |
| 构建与静态 | 通过 | X11 native probe 构建、X11/Core strict Clippy、Windows workspace fmt、3 个 Python fixture AST、diff 检查 |
| 源码冻结 | **315/315 零漂移** | 前一阶段 309 项加四个 keeper 模块/夹具、X11 README、App main；不是整仓或最终候选冻结 |
| 完整 Linux App | **not_run** | 实际 pkg-config 查不到 GTK3、WebKitGTK 4.1、JavaScriptCoreGTK 4.1 开发包；未把 probe 构建代替完整 App |

九项新原生门禁按实际执行顺序为：

1. 缺少继承的私有 bootstrap 时，keeper CLI 拒绝且 selection 不变。
2. Prepared、Published 任务数据、外来 lease、预取消均拒绝，没有启动 keeper。
3. 已确认交接后，模拟 Host **实际正常退出**；独立第三个 X client 仍可读取精确原始 8/16/32 位直接数据。
4. 同样验证大数据 INCR；原 provider 已退出，不借用它持续提供原值。
5. 已确认交接后，通过持有的 Child handle **实际终止模拟 Host**，确认其非成功退出；原始大数据仍可读取。
6. keeper 待定时不能同时启动 manager 或第二个 keeper；正常交接后的数据仍完整。
7. 子进程快照前发生同一 owner 的重新复制，旧交接被拒绝，没有覆盖新复制。
8. 子进程快照前发生 owner ABA，新复制同样优先。
9. 生产 LinuxAdapter 的后台服务交接 keeper 后，所有控制对象和 adapter Drop；旧 reader 的已接受 INCR 完整结束、原线程实际退休，而 keeper 仍提供完整原值。之后独立新复制促使实际子进程退出。

正常退出及被终止的父进程是 `cu-x11-native` 的私有 Host 夹具，运行生产 keeper 代码，**不是安装版 Grok App**。用 `/proc` start-time 绑定孤儿进程观察，不把复用 PID 当作同一进程；仅观察，不据此发送 kill。所有原生测试在 owned Xvfb 内，未操作用户全局剪贴板、桌面或权限提示。

新增四个 Rust 单测分别覆盖固定元数据帧、分段控制读写及 EOF 不是确认、实际子进程容量 permit、终止性 X11 transport I/O 错误分类。错误分类单测不是原生 X server 被终止的验收。OS 创建进程/线程失败、reaper 等待失败、panic 与资源耗尽还未真实故障注入。

### 两个实际反例

- 临时让 keeper 在控制握手后直接退出：变异版本编译成功，原生测试 exit 1，出现 `keeper expected Transferred, got Superseded` 与 `bad parent fixture receipt`，真实父进程寿命门禁未通过。
- 临时移除 manager 对待定 keeper 的排斥检查：变异版本编译成功，原生测试 exit 1，出现 `pending keeper allowed competing manager or duplicate keeper`。

两次均保留变异脚本、stdout/stderr、退出码和原文件字节，finally 中准确恢复；前后 SHA-256 与当前源文件再次一致。随后重建正确二进制，完成冻结三轮回归。没有针对 parent epoch 单独做去保护变异，因此不能声称这一单独分支已被反例独立证明。

初始可见性错误、fixture 缺少 Read trait、Clippy 测试模块位置错误及一次 pkg-config 命令调用错误均保留记录；后续修复与有效命令结果分别留档，不删除失败证据。CI 与 Cargo 清单/锁文件相对上一阶段未改，既有 `--clipboard` 路径包含新门禁；远程 CI 未运行。

`audit.mjs` 核对 315 个选定源码摘要、旧/新 Rust 测试名单、原生完整名单及顺序、三轮退出码、静态检查、两个反例和精确源码恢复、依赖/CI 未变以及文档读回，生成 `final-audit.json`、`receipt.json / receipt.sha256`。它只证明列出的本地阶段，不是完整产品完成审计。

## 未完成与下一步

1. **真实 clipboard paste 及精确异步完成/恢复。** `accessibility_text.rs` 仍明确拒绝显式 clipboard，不偷偷改成 AX InsertText。必须联通授权、一次派发、原始 native owner、持续 selection 服务与恢复时机；GTK PasteText 返回、文本读回、数据交付、keeper/manager 确认都不足以证明没有迟到编辑。
2. **产品生命周期接入。** 仍需实际 TypeText、App quit/update 的有界准备/等待/失败路径，以及用户可见错误、退出控制和恢复动作。已确认交接后跨父进程退出的证明，不覆盖交接完成之前的崩溃或安装包行为。
3. **故障恢复及资源限制。** panic 后保留连接但隔离、停止继续服务，恢复尚未实现；提交后 Unknown 保守保留并非完整 UX。同步 X11 往返没有硬实时取消保证。X server/session 丢失也不是持久化成功，没有宣称跨桌面会话或机器重启保存。
4. **完整 App 与真实桌面。** 需要补齐构建环境并验证真正 App 入口、安装/退出/更新；这里没有已安装 manager/App、GNOME native Wayland 或用户桌面证据。不 commit/push/PR，不替换用户 App/runtime。
5. **完整原范围继续开放：** Windows；macOS arm64+x64；Linux X11 与 GNOME native Wayland；Desktop、托管浏览器、既有标签页、App WebView；App/ACP/MCP；全部输入、IME、剪贴板、取消和恢复；签名安装/升级/回滚；重设计原生 UI 窄窗/缩放/权限；真实 Grok E4；同一最终冻结候选 **12 小时 active soak**。未满足项未被缩减或改成更容易通过的目标，整体目标保持 **active**。

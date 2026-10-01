# Computer Use：X11 剪贴板进程内后台保留

2026-09-27，Asia/Shanghai。分支 `feat/computer-use-implementation`，HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。沿用现有 dirty/untracked 工作树，不覆盖其他工作。上一阶段是已验证的 manager 交接进展；本轮重新轮询真实原生进程、读取源码，完成后台持有与原生验证，分类为 **progress**，不是只复述状态或等待锁文件。

完整目标仍 **active**，实际 `get_goal` 返回 active；没有写 complete、blocked 或 paused。本阶段只是实际 LinuxAdapter 上的**进程内服务持有**，不是整个 clipboard paste 或最终版完成。

## 生产变更

- 新增 `src-tauri/computer-use-x11/src/clipboard_retention.rs`。后台线程取得精确的 `ClipboardOwner + ClipboardLease`，独立处理 X11 selection、MULTIPLE/INCR、manager 交接和安全关闭。只接收已经 Restored 的服务；Prepared、Published 临时文本和外来 lease 在转移前拒绝。发生正常准入、线程启动或通道发送错误时，调用方仍持有原服务对象。
- `LinuxAdapter` 新增保留注册表、`retain_restored_clipboard`、`clipboard_retention_statuses`、`request_clipboard_handoff`。真实 App Host 的 `src-tauri/src/computer_use/linux_adapter.rs` 直接重导出该实现，并非夹具专用替身。但现有 TypeText 实际派发路径**尚未调用这套持有 API**；这里不能表述为剪贴板输入已经自动接通。
- 最后一个控制所有者释放只请求保存交接，不销毁服务。只读 monitor 不算控制所有者；即使 adapter 和所有控制 handle 都被 Drop，没有 manager 时，独立线程仍提供原始 8/16/32 位数据。Abort、release 与 native input slot 不依赖服务线程退出。
- 无 manager 是 Unavailable，不是完成。之后可以显式重新请求发现新 manager；已经真正发送过 SAVE_TARGETS 的握手不能重放。管理器已确认接收，也必须等其他已接受的 INCR、排队请求和不确定状态安全收束，才能关闭服务。
- `ClipboardOwner::try_close` 把“仍需保留”的 `Ok(false)` 与原生连接/清理错误区分；已有 `close` 接口保留拒绝提前关闭的语义。注册表只有在状态 Closed **且实际 JoinHandle 已结束**时才回收记录，不凭单个状态标志宣称线程终止。
- 全进程最多 8 个保留服务，以独立 permit 占用；退出才释放。正常错误保留原连接和快照，并暴露 Faulted。工作循环 panic 时隔离并保留连接，不再执行原生操作、不伪造关闭；panic 后持续服务和显式恢复**还未实现，也未故障注入验证**。
- 原剪贴板为 NONE 时不向 manager 发 SAVE_TARGETS；已接受的旧任务 INCR 仍用固定数据源传到有类型的结束块确认后才退休。这个传输连续性不代表粘贴回调已完成，也不授权把临时任务文本交给历史管理器。
- 状态输出只含状态、错误及线程结束信息；不记录或序列化原始剪贴板字节、格式名称。管理器协议确认不代表磁盘持久化，更不能确认或释放未知的原生输入占用。

## 实际验证

证据目录：`tools/computer-use-probe/.run/x11-clipboard-retention-20260927/`。

| 项目 | 结果 | 验证范围 |
|---|---:|---|
| Linux Rust | **624/624** | X11 44 + Core 564 + 实际子进程 driver 16；旧测试名单全部保留，无忽略、筛选或失败 |
| 原生剪贴板 | **54/54 × 3** | 原 46 项完整保留，增加 8 项后台生命周期验证 |
| 原生语义 | **41/41 × 3** | 授权、文本、Wait、迟到恢复及 clipboard 不冒充 AX 编辑 |
| 原生坐标 | **19/19 × 3** | 原有像素、键鼠、滚动、拖动、焦点及生命周期门禁 |
| 构建与静态 | 通过 | X11 构建、X11/Core strict Clippy、Windows workspace fmt、3 个 Python fixture AST、diff 检查 |
| 源码冻结 | **309/309 零漂移** | 前一阶段 307 项加保留模块及其原生夹具；不是整仓库或最终候选冻结 |

新增 `clipboard_retention_fixture.rs` 使用生产 LinuxAdapter 和独立 X11 请求方、manager peer，在私有 Xvfb 中实际运行后台服务；不在读取端替生产线程手动 pump。8 项验证依次为：

1. Prepared/Published/外来 lease 拒绝，原资产仍在调用方；成功转移不留下双份所有者；终止后 Host 状态回收。
2. Abort/release 不等待保留线程，随后所有控制对象与 adapter Drop，无 manager 仍能逐字节读取原始大数据格式；新用户复制被保留。
3. 最初无 manager，之后真正出现 manager；一次显式请求完成交接，计数恰为 1。
4. 另一个真实 reader 已进入 INCR，manager 接收确认和 adapter Drop 不能截断它；结束块确认后线程实际退出。
5. Silent manager 的 orphan 服务保持 Pending，直到原生所有权变化才 Interrupted；不重放、不覆盖新复制。
6. EarlyAck manager 的 orphan 服务保持 Unknown，同样不能提前退休或覆盖新复制。
7. manager 交接和线程退休不能清除另一个未知 native input slot。这里用软件构造已有 Unknown slot 检验两套生命周期隔离，**不是实际异步粘贴完成验收**。
8. 原值为 NONE 时，恢复后后台继续完成已接受的任务 INCR，最终仍是 NONE，manager 请求数为零。

新增 permit 单元测试覆盖 8 个占用、第 9 个拒绝、释放一个后精确补位及最终计数回零。真实 OS 线程创建失败、通道发送失败、panic 与永久 X11 断连尚未故障注入；不能用该单测替代这些证明。

### 两个真实反例

- 临时让线程在 manager Unavailable 时退出，原生测试 exit 1：`dropping controls killed retained service without manager`。证明“没有 manager 便结束”的错误策略确实被独立读取/生命周期门禁拦住。
- 临时让线程仅凭 Acknowledged 就退出，原生测试 exit 1：`handoff killed another accepted INCR reader`。证明 manager 成功不能替代其他 reader 的完成。

两次变异都先保存原文件字节和 SHA-256，在 finally 中准确恢复，再重建正确二进制并运行冻结三轮回归。保留 `absent-red-*`、`ack-red-*`、原字节和前后 hash；没有修改期望值让反例“通过”，变异不在当前生产代码中。

`audit.mjs` 核对完整原生名单及实际执行顺序、全部退出码、旧 Rust 名单、新增 permit 测试、309 项源摘要、两次反例及源码恢复、CI/依赖未变、文档读回。生成 `final-audit.json` 与 `receipt.json / receipt.sha256`。stdout/stderr 分开保存。既有 CI 的 `--clipboard` 自然包含新夹具，仍在原 45 秒上限内；本轮未改 CI 或 Cargo 依赖，远程 CI 未运行。

## 未完成与下一步

1. **实际 clipboard paste、精确异步完成与恢复仍缺。** `accessibility_text.rs` 对 `via: clipboard` 继续明确拒绝，不用 InsertText 替代。GTK PasteText 返回、匹配读回、选择数据接收、manager 回执都不能单独证明没有迟到编辑。必须把真实授权、一次派发、原 native owner、数据保留及恢复时机连接起来，再开放能力。
2. **这里不跨 App 进程退出。** 线程能跨 adapter/控制句柄 Drop，但 App 进程退出或崩溃会结束线程和 X client。仍需 helper/退出交接、无 manager 下的存续方案、用户可见错误与退出控制；不能把进程内后台线程称作持久化。
3. 队列与快照容量上限、全局 8 个 worker、永久故障恢复仍需完整产品行为。panic 隔离后不继续服务，同步 X11 往返也没有硬实时终止保证；不得将继续占用当成完整恢复实现。
4. 原生证据仅 WSL Debian owned Xvfb 与独立测试 peer，非用户桌面、已安装 manager/App 或 GNOME native Wayland。未操作用户全局剪贴板、权限提示，没有替换用户 App/runtime，没有 commit/push/PR。
5. 原范围保持：Windows、macOS arm64+x64、Linux X11+GNOME native Wayland；Desktop/托管浏览器/既有标签页/App WebView；App/ACP/MCP；全部输入/IME/取消/恢复；签名安装/升级/回滚；重设计原生 UI 的窄窗/缩放/权限；真实 Grok E4；同一最终冻结候选 **12 小时 active soak**。未验证和已有缺口仍开放，完整完成审计不能通过，目标继续 **active**。

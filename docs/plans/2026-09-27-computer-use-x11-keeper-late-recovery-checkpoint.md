# X11 keeper 迟到原回执恢复检查点

日期：2026-09-27。完整目标仍为 **active**，不是最终版或全功能完成。

上一阶段：[原剪贴板独立进程保留](2026-09-27-computer-use-x11-clipboard-keeper-checkpoint.md)。本轮修复真实交接已经完成、但父服务超过软时限才观察原回执时永久停留 Unknown 的缺口。不把 keeper 确认当作异步粘贴完成，不扩展按键协议。

## 实现

- `clipboard_keeper_transport.rs` 将有限写入与只读回执分别封装为 `flush_pending` 和 `read_reply`；正常 `poll` 保留原来的先写后读。Unknown 恢复只读原来的私有通道，**不得发送未发出的 COMMIT，或补发部分 COMMIT**。保留原来的分段接收缓冲，EOF 不等于确认。
- `clipboard_keeper.rs` 在 Unknown 中允许读取原 OWNED 回执。只有 nonce、原 child window、proof、当前原生 selection owner 与精确预期 epoch 全部匹配才恢复 Transferred；不重发输入、不重启 keeper、不释放 native input slot，也不把此前模型结果改为成功。
- 用户新复制仍优先；即使 selection owner 经 ABA 回到旧 child window，也不能把旧回执解释为原始交接。确实缺失的 ACK、EOF、失联不是恢复证据，仍保留不确定状态或由实际 owner/进程变化进入已有退休路径。
- 新夹具仅在 `native-probe` 下可用：向实际子进程送完已经创建的原 COMMIT，但暂不读取回执，等待原来的 **5 秒真实 deadline**。没有改时钟、伪造 ACK 或给生产子进程加入延迟开关。这里验证的是父服务迟到观察回执，不是子进程/网络发送延迟。

变更仅三个源文件：`clipboard_keeper.rs`、`clipboard_keeper_transport.rs`、`clipboard_keeper_fixture.rs`。keeper 子进程、保留线程、CI、Cargo 依赖与锁文件、协议和实际 TypeText 路径均未改。现有 14 个命名按键白名单保留，**没有新增组合键支持**。

## 实测证据

目录：`tools/computer-use-probe/.run/x11-keeper-late-recovery-20260927/`。

| 项目 | 当前结果 | 范围 |
|---|---:|---|
| Linux Rust | **629/629** | X11 49 + Core 564 + driver 16；完整名单，无失败、忽略或筛选 |
| 原生剪贴板 | **66/66 × 3** | 既有 63 项完整保留，新增迟到回执、新复制、ABA 三项 |
| 原生语义 | **41/41 × 3** | 既有语义派发、输入与恢复行为 |
| 原生坐标 | **19/19 × 3** | 既有真实输入及权限/目标生命周期门禁 |
| 静态和构建 | 通过 | native probe 构建、X11/Core strict Clippy、workspace fmt、三个 Python AST、diff 检查 |
| 选定源码冻结 | **315/315 零漂移** | 与上一阶段相同的有序名单；仅上述三项改变，不是整个产品的最终冻结 |
| 完整/安装版 App | **not_run** | 没有用 probe 构建冒充完整 App、安装或退出/升级验收 |

新增三项原生测试使用真实 keeper 进程、原私有通道和独立 X client：

1. 超过真实时限后先观察 Unknown，再凭原回执恢复 Transferred。原格式可独立读取，仍是原来的 child process；关闭父服务后数据继续保留，新复制后实际子进程退出。
2. 原回执被延后观察期间发生新复制，进入 Superseded，旧回执不覆盖新 owner。
3. 在短 X server fence 内发生 user → old-child 的 ABA，旧窗口虽然重新匹配，仍因 epoch 不匹配保持 Unknown；随后新复制使其正常退休。

新增 Rust socket 单测逐个验证未发送及发送到第 11 字节的 COMMIT：只读迟到回执不会继续发送；回执分成 9+23 字节仍可组装。对端真实读取检查零额外数据，不只断言内部状态。

### 初始红测与去保护反例

- 在修复恢复策略前，实际构建成功、原生探针 exit 1：既有 63 项通过，但真实软时限之后仍 `keeper expected Transferred, got Unknown`。修复后 66 项全部通过。
- 临时移除原生 epoch 条件：变异版本构建成功，前 65 项通过，ABA 测试 exit 1，报 `late keeper acknowledgement accepted ABA ownership as original transfer`。
- 临时让 `read_reply` 刷出待发送 COMMIT：实际 socket 对端收到 32 字节，新增单测失败，cargo exit 101。该反例只运行指定单测，48 项被筛选；不冒充完整回归。恢复后完整 49 项无筛选通过。

两个反例均在 finally 准确恢复原始文件字节；前后 SHA-256、备份字节和当前文件一致。之后重建正确二进制，冻结源码并串行执行全部三轮原生和 Rust/静态回归。`audit.mjs` 核对旧/新完整名单与顺序、退出码、315 源码摘要、反例、精确恢复及文档读回，生成 `final-audit.json` 和 artifact receipt；这里只证明本阶段的本地证据。

## 仍未完成

- **真实 clipboard TypeText 与精确异步完成/恢复**：显式 clipboard 仍被拒绝，不用直接 AX 编辑替代；GTK PasteText 返回、文本读回、selection 交付和 keeper ACK 都不是无迟到输入的证明。
- **产品生命周期**：实际 TypeText、App quit/update 的准备/等待/失败路径和用户可见恢复尚未接线；完整 Linux App、安装包及真实桌面仍待验收。上一阶段已记录缺少 GTK/WebKit 开发包，本轮没有安装依赖或重跑完整 App 构建。
- **故障与耐久性**：交接完成前的崩溃、panic 后恢复、真正丢失回执的 UX 和桌面会话耐久性仍未完成；同步 X11 往返没有硬实时取消保证。
- **原始完整范围**：Windows、macOS arm64+x64、Linux X11 和 GNOME native Wayland；Desktop/托管浏览器/既有标签页/App WebView；App/ACP/MCP；全部输入、IME、剪贴板、取消与恢复；签名安装/更新/回滚；重设计原生 UI 的窄窗/缩放/权限；真实 Grok E4；同一最终冻结候选 **12 小时 active soak**。不缩减目标，不用本地子集代替最终版。

原生操作仅在私有 owned Xvfb；没有使用用户全局剪贴板/桌面/权限提示，没有替换用户 App/runtime，没有 commit、push 或 PR。目标继续 **active**。

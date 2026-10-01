# Computer Use：X11 迟到原生回执与精确占用恢复

时间：2026-09-27，Asia/Shanghai。分支 `feat/computer-use-implementation`，HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`；保留原有 dirty/untracked 工作。完整目标继续 **active**，本地恢复阶段不等于最终版。

## 本轮改变

- 上一轮已证明未知原生输入必须隔离，但迟到回执被一秒方法超时丢弃，已完成的输入仍永久 busy。本轮不再以永久 busy 代替可恢复的迟到完成。
- 新增 `accessibility_call.rs`：在派发前注册两条有界 zbus 消息流，分别接收 MethodReturn / Error，保留原调用 serial、原 unique provider 和本地 destination。软期限到达后调用方得到 Unknown，订阅继续存活，没有再次派发、工作线程重启、剪贴板写入或全局 reset。
- 只有精确相关的 provider 布尔回复或正确 provider Error 能证明该同步 RPC 已返回。其他 serial/sender/destination、错误 body、bus 生成的 NoReply、断连、目标消失、健康连接、Stop 或经过一段时间都不是完成证明。消息队列与单次轮询有界；一个流关闭时仍检查另一个流已排队的有效回执。
- `NativeActionSlot` 增加绑定原 owner 的 deferred recovery ticket。必须同时满足 owner 正常返回与后端确认完成，才能回收占用；早到证明不能释放活动 guard，迟到旧证明不能释放新 owner，同 run/generation 的其他 slot 也不能被清除。panic、丢失 ticket 和没有完成证明继续隔离。
- LinuxAdapter 在确认恢复前清空旧 AT-SPI 引用与旧/未发布模型授权，随后才允许释放原 slot。必须重新观察后才能发新动作；旧引用加新 token/generation 不能复活。X11 自身 input cleanup 不确定时，不授予仅靠 AT-SPI 回执恢复的 ticket。
- Click、SetValue、TypeText 的 Delete/Insert/Caret 使用同一回执路径；既有精确范围编辑、原生身份/权限/焦点/物理按键/取消检查不放松。恢复只处理占用，**不把历史 Unknown 升级为 Verified，不继续剩余编辑步骤，不自动重放**。

生产改动：`src-tauri/computer-use-core/src/native_action.rs`；`computer-use-x11/src/accessibility_call.rs`、`accessibility.rs`、`accessibility_text.rs`、`authority.rs`、`lib.rs`；X11 Cargo 声明直接使用已在 lock 中的 `futures-lite 2.6.1`。App 仍重导出同一个 LinuxAdapter，没有另造仅供测试的输入实现。

## 实际原生验证

私有 Xvfb + D-Bus/AT-SPI + 独立 GTK 进程。读取 GTK 自身 text/caret/selection/事件数，不由适配器伪造效果。

1. 延迟 DeleteText 回复：原调用 Unknown；精确迟到回复回收占用，原请求仍被拒绝，无补插入或重复删除。
2. InsertText 与 SetTextContents：在真实 GTK 回调内握手阻塞，等待调用方一秒软超时。Stop/status 不等回调；健康 capability/目标与重复请求不能解除隔离。人工释放私有夹具回调后，精确回复允许重新观察后的独立输入，旧引用不能复活。
3. 生产 Broker：未知删除后保留 Unknown 缓存，同 action ID 不重放；完成后仍必须重新 observe，新的独立 SetValue 才 Verified。
4. 真正杀死处于回调内的 owned GTK provider：bus NoReply/目标死亡不被误当完成证明；隔离仍保留。这是负向证明，不宣称 provider 丢失后的完整恢复已经实现。

## 回归证据

目录：`tools/computer-use-probe/.run/x11-recovery-20260927/`，不覆盖 selection / wait 旧批次。

| 项目 | 当前结果 | 范围 |
|---|---:|---|
| Linux Rust | **614/614** | X11 34 + Core 564 + 真实子进程 driver 16；零失败/忽略 |
| Windows 共用占用模块 | **10/10** | 仅 NativeActionSlot 定向测试，其余 555 项按过滤器未运行；不是 Windows 全套/原生输入验收 |
| 原生语义 | **40/40 × 3 轮** | 每轮逐项核验，包括迟到完成/Stop/新观察/Broker/真实 provider 死亡 |
| 原生坐标 | **19/19 × 3 轮** | 保留先前所有坐标/命名键/滚动/拖动/目标生命周期门禁 |
| 静态检查 | 通过 | X11 all-targets/native-probe 与 Core all-targets/test-support strict Clippy；Windows workspace fmt；Python AST；git diff --check |
| 源码冻结 | **298 项** | 选定 Core/X11/根适配器、依赖、CI 与夹具；最终摘要由 `audit.mjs` 回读核验，不是整个脏仓库 |

`audit.mjs` 从上一批明确门禁加本轮 5 项要求构成期望列表，仅替换“迟到后仍永久隔离”旧要求，其余门禁全部保留；对冻结三轮日志、退出码、测试名/数量、顺序和 SHA-256 逐项检查。输出 `expected-native-gates.json`、`final-audit.json`、`source-before-final.json / source-after-final.json`、`handoff-readback.json`、`receipt.json / receipt.sha256`。

保留的中间失败：
- `initial-core`：未传 Linux 原生种子，误落默认 `node.exe` 路径，548 通过/15 失败。改用既有 `runtime-publication-20260927/linux-seed-path.txt` 指向的实际 Linux 种子后，全套通过；没有放松测试或修改产品种子检测。
- `recovery-native`：恢复后新输入的测试错误假定 SetTextContents 将光标放末尾。真实 GTK 为 caret=0；改为独立 oracle 的实际光标构造精确预期，不移动光标迎合测试。`expanded-native` 和冻结三轮都记录 caret=0 且精确新输入通过。

zbus 5.18.0 已锁定 crate 的 connection/socket reader/message stream/message builder 源文件复制到证据目录，作为 call timeout 丢弃等待与回复广播/本地非 signal 订阅的语义依据。原生行为验证才是功能证据；源码本身不冒充验收。未修改 CI，也不宣称远端 CI 已执行。

## 保留的完整未完成范围

- 本批“完成”只指相关同步 AT-SPI RPC 返回，不证明应用自行启动的任意后台工作完成。一秒为**回复软期限**，同步 socket send/X11 校验仍不保证硬实时抢占。
- provider 真正丢失、永久不回复、无效回复、X11 连接/释放不确定的完整恢复仍未完成，不能把继续隔离算作该功能最终实现。中文 IME composition、clipboard ownership/restore 仍需开发与原生验收。
- 这里是 WSL Debian owned GTK/Xvfb，不是已安装 App、多工具包/多屏/DPI 矩阵、GNOME native Wayland 或其他平台实机。原生文本的非原子步骤与单次 undo 语义仍保留上一批限制。
- 完整目标不缩减：Windows；macOS arm64+x64；Linux X11+GNOME native Wayland；Desktop/托管浏览器/既有标签页/App WebView；App/ACP/MCP；全输入/取消/恢复；签名安装/升级/回滚；重设计原生 UI 窄窗/系统缩放/权限；真实 Grok E4；最终候选源码冻结后的 **12 小时 active soak**。历史 Windows 发布/浏览器长尾与原生 UI 未执行项继续保留。
- 本轮分类 **progress**，没有 complete/blocked/paused 状态写入，没有 commit/push/PR；没有替换用户 App/runtime，没有操作权限提示。下一步继续 G9 的剪贴板、IME 和不可回复原生输入恢复，并推进其他平台与交付验收。

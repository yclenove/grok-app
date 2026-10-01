# Computer Use：X11 剪贴板快照读取与所有权检测

2026-09-27，Asia/Shanghai。分支 `feat/computer-use-implementation`，HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。原有 dirty/untracked 工作保留。上一轮恢复工作有真实源码和回归证据，分类为 progress；本轮同样有实际开发与原生验证。完整目标继续 **active**，没有 complete/blocked/paused 写入。

## 实际改变，不扩大完成声明

- 发现显式 `TypeText via:"clipboard"` 仍落到 AT-SPI DeleteText/InsertText，违背指定输入方式。修复为在直接编辑前明确拒绝，暂不把 AX 输入冒充剪贴板粘贴。独立 GTK 状态/编辑事件证明零修改且占用释放。移除该防护的红测确实失败；恢复防护后原生三轮通过。**拒绝不算剪贴板功能完成。**
- 新增生产模块 `src-tauri/computer-use-x11/src/clipboard.rs`，实现真正 X11 selection 多格式快照读取，不是只保存 UTF-8 文本。保留每个支持格式的 target atom、实际 type、8/16/32 位格式和原始字节；包含 Unicode、HTML、二进制与 ICCCM INCR 分块传输。
- 读取前检查完整 TARGETS：最多 64 个，单格式 4 MiB，总量 16 MiB。协议目标 TARGETS/TIMESTAMP/MULTIPLE/SAVE_TARGETS 不当作用户数据转换；DELETE/INSERT_SELECTION/未知私有格式等拒绝，不为了部分成功悄悄丢弃格式。当前显式支持的文本/图片/MIME 列表不是所有可能的应用私有格式。
- 每次转换创建自己的独立 requestor window；回复匹配 requestor/selection/target/property/time。失败、取消、超时后销毁准确的自有 requestor，迟到回复无法落到后续转换；不销毁其他应用窗口。INCR 验证首部、类型/位宽一致性、下界与累计长度、零长度结束块，拒绝中途变型与不完整传输。
- 独立 X11 连接启用 XFixes selection 事件，维护本连接 epoch。即使同一个 owner window 重新复制、所有权 A→B→A、provider 退出也会使原快照失效；其他 reader 的快照不能充当当前证明。只观察所有权：provider 不重新声明所有权却自行改动数据，不能被 X11 所有权事件证明为内容不变。
- 私有数据类型不实现 Debug/Serialize，公开只提供计数/格式元信息；原始剪贴板内容不进入 Observation、模型结果或错误文案。本模块不声明 CLIPBOARD 所有权、不派发输入、不请求 clipboard manager，不改变用户原有数据。

## 证据与覆盖

证据目录：`tools/computer-use-probe/.run/x11-clipboard-20260927/`。

| 项目 | 实际结果 | 范围 |
|---|---:|---|
| Linux Rust | **622/622** | X11 42（新增 8）+ Core 564 + 实际子进程 driver 16，零失败/忽略 |
| 原生语义 | **41/41 × 3** | 旧 40 项保留，增加显式剪贴板不得静默 AX fallback |
| 原生坐标 | **19/19 × 3** | 旧坐标/命名键/滚动/拖动/生命周期门禁全部保留 |
| 原生剪贴板 | **19/19 × 3** | 独立原生 owner 连接 + 真 GTK 进程，全部在 owned Xvfb |
| 静态检查 | 通过 | X11/Core strict Clippy、workspace fmt、两份 Python fixture AST、git diff --check |
| CI 新增步骤 | 本地通过 | YAML 解析并确认仅新增 owned clipboard 命令；提取实际命令、只替换已构建产物路径后运行 19/19。远端 CI 未运行 |

剪贴板 19 项不是一个笼统 PASS：无 owner；完整 8/16/32 多格式逐字节验证；跨连接身份；同窗口重新声明；ABA；完整 INCR；INCR 改 type/format/截断/过大；错误/超量 TARGETS；危险目标预检且零数据转换；必需格式被拒绝；取消准确清理；沉默 owner 到期；并发新 owner 保留；provider 退出；真实 GTK Unicode 与所有已声明支持格式。每项由独立日志条目核验，失败路径同时断言原 owner 未被本模块替换。

冻结清单 301 项：运行回归期间全部一致；之后只给 `.github/workflows/ci.yml` 加入已验证的新探针步骤。最终审计要求其余 **300 项零漂移**，并单独核对这一项 CI 的精确新增内容、前后 SHA 与实际本地命令。不是整个脏仓库冻结，也不是最终发布候选。

`audit.mjs` 回读退出码、完整测试项、三轮逐项门禁、源摘要与文档，输出 `final-audit.json`、`source-after-final.json`、`expected-native-gates.json`、`receipt.json / receipt.sha256`。保留中间失败：初次 Rust 类型/API 调用编译错误、两条严格 Clippy 建议、去掉防护的真实红测、宿主旧 Python 不支持 `Path.write_text(newline=...)` 的审计脚本错误；均有对应修正与最终通过证据。没有放宽产品检查或吞掉失败。

官方 X.org ICCCM/XFixes 文档及 GNOME GTK 3 的 `gtkentryaccessible.c` 副本保存在证据目录。GTK 源码显示 PasteText 通过 `gtk_clipboard_request_text` 安排异步回调；这帮助定位下阶段的完成边界，但源码本身不代替真实粘贴/恢复验收。

## 下一段必须继续开发

1. 本轮只完成**快照读取与所有权检测**。`ClipboardReader` 是生产 crate 中的 Host 底层，但尚未接入 `LinuxAdapter` 的剪贴板粘贴；显式 clipboard 请求仍明确不支持。
2. 接入有界 selection owner/service，发布任务文本前在短 X server fence 内重新检查快照的 owner/epoch；不能在持有 server grab 时等待其他应用提供剪贴板数据。恢复时要继续提供全部保存格式，不能简单把 owner 设回旧 window——旧应用收到 SelectionClear 后可能已释放数据。
3. 实现真实 paste，绑定原 native owner/精确目标与有效授权。不能使用 InsertText 伪装剪贴板输入，不能把 PasteText 的同步返回、一次匹配读回或单纯数据传输当作任意异步回调全部完成的证明；Unknown、Stop、迟到回复、用户重新复制都要有明确的占用和恢复策略。
4. 覆盖 INCR/MULTIPLE 的发送端、正在进行的请求与所有权丢失、clipboard manager 交接、App 退出时恢复数据的存续、超时/断连/恢复失败。没有这些实测，不能宣称 clipboard save/paste/restore 已完成。
5. 三秒只是轮询/转换的软期限；同步 X11 round trip/socket 等待仍不构成硬实时取消。测试是 WSL Debian 的私有 Xvfb/GTK，不是已安装 App、真实多屏/DPI、GNOME native Wayland 或其他 OS。

完整最终范围不缩减：Windows、macOS arm64+x64、Linux X11+GNOME native Wayland；Desktop/托管浏览器/既有标签页/App WebView；App/ACP/MCP；全输入/IME/取消/恢复；签名安装/升级/回滚；重设计原生 UI 窄窗/系统缩放/权限；真实 Grok E4；同一最终冻结候选 **12 小时 active soak**。先前 Windows 发布/浏览器长尾、未执行的原生 UI 和无回复/断连恢复事项仍开放。未操作用户全局剪贴板或权限提示，未替换用户 App/runtime，未 commit/push/创建 PR。

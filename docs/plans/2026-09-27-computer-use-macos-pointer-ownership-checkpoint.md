# Computer Use：macOS 指针输入的按键/按钮归属

完整目标仍是“接手 grok 的工作，完成 computer use 所有功能开发，推进到最终版”，保持 active。
日期为执行主机 Asia/Shanghai 的 2026-09-27。默认关闭、Host broker、禁止桌面兜底等产品约束未变；未提交、推送、发布、替换用户 App 或授予权限。

## 1. 先失败的真实调用链

检查现有 macOS 点击、滚动、拖拽发现：指针使用 HID event source，未显式清零 flags，也未检查用户/其他输入源已按住的修饰键和鼠标按钮。用户在本次 down 后按下同一按钮时，旧实现仍会发出 up 并报告空闲。

新增 `src-tauri/computer-use-core/tests/macos_pointer_ownership_contract.rs`，直接载入生产 adapter，以确定性 Quartz FFI 替身注入状态。
首次 `red.log` / `red.exit` 为 **1/6 通过、5 项失败、exit 101**：初始修饰键、初始按钮、配置阶段按钮变化、down 后物理按住状态、private source 五类反例均失败；已有正常双击/拖拽正例通过。
这是生产调用链的合同反例，**不是 macOS 真机输入效果证明**。

## 2. 实现变化与边界

生产修改仅在 `macos_adapter.rs` 的点击路径和 `macos_adapter/pointer.rs`：

- 点击、滚动、拖拽统一使用 private event source，并显式设置每个指针事件 flags 为 0，避免借用用户修饰键改变操作含义。
- 初始检查与实际 post 前检查 HID 和 combined-session flags；拒绝 Shift/Control/Option/Command/Fn。Caps Lock 不当作用户正在按住的指针修饰键，仍会清零事件 flags。
- 检查产品支持的左/右/中三个鼠标按钮。物理按住永不豁免；手势内部仅允许 combined state 中本次已发送的那个按钮，避免把自身尚在队列中的 down 当作外来输入。
- 双击保留按钮身份和 click count；拖拽继续在首次输入前分配全部运动及释放事件，逐步校验截图映射/权限/身份/取消。
- 在 down 或 drag 后发现用户/外来输入时，不继续运动、不代用户释放按钮。无法确认安全释放则返回 `pointer release unconfirmed` 并保留 native occupancy；abort 不得伪装成 idle。
- 清理仍只能作用于原身份及最后已发送坐标；没有 global event tap、抢焦点、桌面兜底、权限修改或未知输入自动重放。

Quartz 的状态查询与事件投递之间没有原子归属判定；同一按钮的其他合成发送者竞态仍不能可靠区分。不能声称所有鼠标按钮/敌对并发发送者/原生队列恢复已解决。`CGEventPostToPid` 的排队也不等于目标处理完成，现有 applied/unverified 语义未升级。

FFI 替身增加物理/合成按钮状态、指针 flags 和配置阶段回调，既覆盖拒绝路径，也覆盖正常左/右/中双击、正常拖拽、Caps Lock 中性事件及中途外来输入。最终新文件 **9 项**。

## 3. 本轮验证

证据目录：`tools/computer-use-probe/.run/macos-pointer-ownership-20260927/`。

| 检查 | 当前结果 | 证据 |
| --- | --- | --- |
| 原实现反例 | 1/6，exit 101 | `red.log`、`red.exit` |
| Windows Core lib | 561/561，exit 0 | `windows-final.log` |
| Linux Core lib（当前生产 seed） | 560/560，exit 0 | `linux-final.log` |
| 两宿主各 9 个 Mac FFI/原生验收器合同文件 | 各 173/173，零失败/忽略/过滤 | 同上 |
| Windows 总计 | 734/734 | `final-audit.json` |
| Linux 总计 | 733/733 | `final-audit.json` |
| 两宿主 strict Clippy | all-targets + test-support，`-D warnings`，均 exit 0 | `windows-clippy.log`、`linux-clippy.log` |
| 工作区格式检查 | exit 0 | `format-check.exit`、`format-check-r2.exit` |

9 项新增归属合同已经包含在每宿主 173 中，不重复累加。中间 `green-r1` 仅覆盖当时的 6 项新合同，不能替代最终结果。
各终端句柄均已收取 exit 0；没有因为观察超时重启仍在运行的进程。

四个改动源码/测试在最终回归前冻结，最终哈希和字节数 **零漂移**。前轮 241 项源码清单复核只有本轮预期的三个已有文件变化，新增合同另列，无其他漂移。
`audit-final.mjs` / `final-audit.json` 显式区分该范围与全仓库、真机验收。首次证据读取器因格式检查无输出、Tee-Object 未生成日志而 ENOENT；保留第一次 after snapshot，重新执行格式检查并显式创建空日志后审计通过。这不是产品测试失败，也没有把空日志当作测试数量证据。

Apple 原始 DocC 的 ButtonState、privateState、CGEvent flags JSON 已保存到证据目录；实现参考原生声明，不使用第三方截图或文章冒充 API 证明。

## 4. 仍须完成的原始范围

必须在真实 macOS arm64 和 x64 上跑生产调用链/独立 Cocoa 夹具，核验点击、滚动、拖拽、物理修饰键并发及未知释放恢复；本轮 Windows/Linux 替身不能证明 macOS SDK/ABI、权限或原生效果。
同轮 [R18 原生 UI 构建](2026-09-27-computer-use-r18-native-ui-checkpoint.md) 的快照早于这些 Mac 修改，不能说该 exe 含本轮修复。

Windows、macOS arm64/x64、Linux X11/GNOME native Wayland；Desktop/managed browser/existing tabs/App WebView；App/ACP/MCP；完整输入/IME/剪贴板/取消/生命周期/恢复；签名安装/更新/回滚；原生重设计 UI 的窄窗/系统缩放/权限矩阵；真实 Grok E4 和冻结源码 12h active soak，全部继续纳入最终版验收。
历史浏览器导航 504、worker/native 退出长尾及发布锁/PID 偶发问题没有新根因关闭证据。本轮不申请 complete、blocked 或 paused。

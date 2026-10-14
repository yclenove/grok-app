# 0006 macOS TCC 归属、助手应用与后台点击（S-02 实测）

状态：提议（部分实测；矩阵未齐，见“未完成”）
日期：2026-10-14

> 编号说明：06-roadmap 的 S-02 卡写作 “ADR 0002”，该编号已被接口扩展 ADR 占用，故用 0006。

## 背景（事实 + 证据）

用一次性 Swift 探针（不入库；`LSUIElement` 助手应用 + 普通夹具窗口应用，均用临时签名，经 `open` 在用户桌面会话中启动，结果写 JSON 文件）在真实机器上测量。

| 机器 | 系统 | 架构 | SIP |
|---|---|---|---|
| Mac mini | macOS 27.0.1 (26A434) | arm64 | 开启 |
| MacBook Pro | macOS 15.7.9 (24G830) | x86_64 | **关闭**（结果不可当默认情形） |

缺 macOS 14。**本次只完成了 Mac mini 的完整测量**；MacBook Pro 尚未测量。

## 实测结果（Mac mini / macOS 27 / SIP 开）

**(a) 助手应用归属**：由 `open` 启动的 `LSUIElement` 助手（`ppid=1`）在用户授权后 `AXIsProcessTrusted=true`、`CGPreflightScreenCaptureAccess=true`。同一可执行文件直接从 SSH 运行：均为 false。**TCC 归属跟随“负责进程链”，不跟随可执行文件**；所以从终端/agent 直接运行的 `gcu` 拿不到权限，路线 A 必须有助手应用（04 §macOS helper 的设计成立）。

**(b) 被遮挡窗口截图**：`SCContentFilter(desktopIndependentWindow:)` + `SCScreenshotManager.captureImage` 在助手中成功；浮动层窗口完全盖住目标时，截图内容仍是目标窗口原貌（无遮挡物），`isOnScreen=true`。未授权时返回 `SCStreamErrorDomain -3801`（“用户拒绝了 TCC”），应映射为 `permission_missing{screen_recording}`。注意：授权后必须在进程里先建立 `NSApplication`（连接 WindowServer），否则 `CGS_REQUIRE_INIT` 断言崩溃——驱动实现不能在无 AppKit 初始化的线程里直接调 SCK/CG。

**(c) 后台投递**
- AppKit 窗口：`CGEvent.postToPid` + 通过 `dlsym` 取得的 `CGEventSetWindowLocation` + **`CGEventSetIntegerValueField(event, 51, windowID)`** 才会被路由到目标窗口（事件的 `windowNumber`、窗口内坐标都正确）。缺少字段 51 时，事件虽被应用收到，但 `windowNumber=0`、坐标被当成屏幕坐标，视图收不到点击；只设字段 91/92 或只调 `CGEventSetWindowLocation` 均无效；`SLEventPostToPid` 也无不同。目标窗口在后台且被浮动窗口遮挡、前台应用为他人时，连续 20 次投递精确生效 20 次，前台应用不变。
- Chromium（Chrome，独立 profile）：**后台时 0 次到达**（试过：基础 postToPid、+字段 51、`SLEventPostToPid`、`SLPSPostEventRecordTo` 的 focus-without-raise 事件对、先 `mouseMoved` 再点击）。目标为前台时，字段 51 的 postToPid 能到达（1/1）。结论：macOS 27 上 Chromium 后台点击**不可用**，P1-06 需继续研究或回退。
- 字段 51 属未文档化行为：必须运行时探测并有回退，不得假定跨系统版本稳定。

**(d) 路线 B 子进程**：由已授权的助手应用启动的普通子进程（非 bundle，不同签名）`AXIsProcessTrusted=true`、屏幕录制 true；直接从 SSH 运行同一子进程为 false。**由 Tauri App 以子进程启动 `gcu engine`，权限归属 App，授权一次即对引擎生效**，不需要引擎自带 bundle。

**(e) 替换/未签名**：去掉签名的同 bundle id 应用在 arm64 上无法启动（`launchd spawn failed 163`）；换标识（`com.evil.other`）的副本不继承授权；重新编译（cdhash 变化）的同标识副本在默认临时签名下授权失效。用“按签名标识符匹配”的指定要求（`designated => identifier "…"`）重新签名后，**重编译不丢授权**——这说明开发期可用此做法，也说明：仅按标识符匹配的签名（含临时签名）等于“任何同标识符的临时签名二进制都继承授权”，**正式版必须用 Developer ID + Team ID 要求**。未能构造出同标识符的恶意副本做正向验证（尝试的副本签名标识为 `gcuprobe`，未继承）。

## 决定

1. 路线 A 保留 `LSUIElement` 助手应用，TCC 授权对象是助手；CLI 经 `open -b` 拉起。路线 B 的引擎作为 App 的子进程，权限归属 App，无需额外助手。
2. 后台点击（P1-06）：原生 AppKit 走 `postToPid` + 窗口号字段（运行时探测并验证实际到达，失败不升级）；Chromium 类窗口在 macOS 27 视为“后台不可用”，只能前台路线并需策略放行，直到找到可行做法并实测。
3. 助手/引擎的签名要求：开发用 identifier 要求；发布用 Developer ID + Team ID，并在运行时核验自身身份（06 P4-01）。
4. 驱动在调用 SCK/CG 前必须保证 AppKit/CGS 已初始化。

## 备选与为什么不选

- 引擎自带 bundle 助手（`open`/XPC 启动）：(d) 已证明子进程直接继承 App 的授权，不需要。
- 把 Chromium 点击改成 SkyLight 私有 SPI 全家桶：试过的几种 SPI 组合均无效，且需要 `macos-private-spi` 并承担系统升级风险；未获证据前不采用。

## 影响

- P1-06 以本 ADR 为输入：窗口号字段探测与“实际到达”校验是必需的契约测试；Chromium 后台点击不得标记为可用。
- P4-01 helper 的签名/授权文案要提示“授权对象是助手应用”。
- 错误映射：SCK `-3801` → `permission_missing{screen_recording}`。

## 未完成（须补测后才能把本 ADR 改为“接受”）

- MacBook Pro（macOS 15，Intel，SIP 关）的整套测量；缺 macOS 14。
- (f) 权限被撤销中途、错误 PID/窗口、安全提示出现时的拒绝行为（需要有人在机器上手动撤销授权）。
- 正式签名（Developer ID）下的身份核验与独立 Dev 授权隔离；同标识符恶意副本的正向验证。
- Chromium 后台点击是否存在其他可行做法。

## 退出条件

若后续系统版本使字段 51 路由失效、或发现更稳定的公开做法，则重新评估决定 2。

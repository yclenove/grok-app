# 01 · v1 实现 Review

范围：分支 `feat/computer-use-implementation` @ `5455eff9`（相对 `main` 57 个提交、2341 个文件、+402,735 / −1,933 行）。
方法：源码与文档静态审查、规模统计、与 Codex 官方行为及 12 个开源实现对照（见 [02-research.md](02-research.md)）。
本 review 没有运行 v1 的 native/model 测试；对其运行状态的描述引用 v1 自己的 wiki 与检查点。

---

## 1. 规模画像

| 组件 | 路径 | 文件 | 行数 |
|---|---|---:|---:|
| Host（Tauri 内） | `src-tauri/src/computer_use/` | 221 | 56,895 |
| Core | `src-tauri/computer-use-core/` | 296 | 92,157 |
| Wayland | `src-tauri/computer-use-wayland/` | 66 | 19,150 |
| X11 | `src-tauri/computer-use-x11/` | 33 | 11,111 |
| session_manager / acp_client 接线 | `src-tauri/src/{session_manager,acp_client}/computer_use/` | 68 | 16,315 |
| 前端 | `src/components/computer-use/`、`src/lib/computer-use/` | 68 | 7,364 |
| MCP 门面 | `tools/computer-use-mcp/` | 5 | 808 |
| 受管浏览器 worker（Playwright） | `tools/computer-use-browser/` | 66 | 12,163 |
| 已有标签页扩展（MV3） | `tools/computer-use-extension/` | 69 | 7,997 |
| GNOME Shell 扩展 | `tools/computer-use-gnome-shell/` | 48 | 5,148 |
| 探针 / 验收脚本 | `tools/computer-use-probe/` | 269 | 38,042 |
| vendored 补丁（zbus、tauri-plugin-updater） | `src-tauri/vendor/` | 165 | 37,765 |
| 计划/检查点文档 | `docs/plans/*computer-use*` | 404 | 57,977 |

统计口径：表格为所列目录的文件行数合计（约 36.3 万，含计划文档；扣除文档约 30.5 万），前言为 git diff 新增/删除行数（+40.2 万/−0.2 万）。两者不是同一统计量；静态资源、golden 与 vendor 等的具体差额未经重新归因，不把评审建议的“净代码约 32.6 万”当作已核实事实。

按主题切分（行数）：

| 主题 | 行数 | v2 处置 |
|---|---:|---|
| 运行时打包/导入/回滚/限额（`runtime_*`、`archive_zip`、`playwright_materialize`、`nsis_contract`） | 9,422 | **删除** |
| Core 受管浏览器（`browser*`、`broker/managed*`、`broker/browser_dispatch`） | 15,186 | **删除** |
| Host 浏览器/Playwright/执行运行时 | 7,600 | **删除** |
| 许可/NOTICE/来源审计工具 | 10,853 | **删除**（不再打包 Chromium，义务随之消失） |
| 任务授权 / scope 选择 / 切换 / 恢复（`task_consent`、`broker/scope_*`、`task_*`） | 15,718 | **替换**为按 App 授权 |
| Core broker 测试（`broker/tests*`） | 17,923 | 大部分随被替换的模型删除；**故障类测试思路保留** |
| Windows 适配（`windows_*`） | 8,528 | **择优移植** |
| macOS 适配（`macos_*`、`quartz_frame`） | 3,923 | **择优移植** |

---

## 2. v1 做对了的事（要继承）

这些判断是正确的，v2 原样继承其**语义**（不一定继承代码）：

1. **结果分级。** `applied / verified / rejected / unknown`；超时即 `unknown`，`unknown` 不自动重放。
   这比多数开源实现（成功/失败二元）诚实得多。v2 扩展为 `delivery` × `effect` 两个维度，见 [03 §6](03-architecture.md#6-动作结果语义)。
2. **YOLO / always-approve 不授予桌面控制。** 桌面控制权独立于 shell/文件权限。v2 保持。
3. **默认关闭；SSH / 远程 IM / 定时任务不继承本机 CU。** v2 保持。
4. **目标失效直接拒绝，不回退到全桌面。** v2 保持：窗口关闭后不会自动换成另一个窗口。
5. **剪贴板默认不读入观察；粘贴后按"仍是我们写入的内容"才恢复。** v2 保持（与 zavora 的做法一致）。
6. **截图以 image content 传给模型；无图不允许坐标点击。** v2 保持（与 Codex window2 / opensymph 门禁一致）。
7. **UI 预览与模型观察分离，隐藏时停止采集。** v2 保持。
8. **大量故障注入思路**：进程崩溃、迟到截图、Stop 竞争、owner 退出等。v2 把这些场景压缩成驱动契约测试与 core 状态机测试（[07 §3](07-quality-gates.md#3-故障与生命周期测试l2fake-驱动)）。
9. **Windows UIA + 客户区截图 + 子控件消息优先、失败再 SendInput** 的分层投递方向正确。
10. **Wayland 不冒充 native**（不拿 XWayland/浏览器结果充数）的诚实原则正确。

---

## 3. 核心问题（按严重度）

### P-1 模型接口让模型做服务端的活（致命）

`computer_act` 的每次调用要求模型复制：`version`、`actionId`、`runId`、`targetId`、`targetGeneration`、
`snapshotId`、`geometryRevision`、`target{elementRef|x,y}`，再加 `action`、`parameters`
（[protocol.mjs:8-23](https://github.com/yclenove/grok-app/blob/025670ba/tools/computer-use-mcp/protocol.mjs)）。`computer_wait` 同样要 9 个身份字段。

后果：
- 每步多出几十到上百 token 的参数，模型极易复制错（尤其多目标、多轮后）。错了就是 `identity_mismatch` → 再观察 → 再试，往返翻倍。
- 这些身份**服务端本来就知道**（谁是最新 snapshot、哪个窗口、几何是否变了）。让模型回传只是把一致性校验外包给了最不可靠的一环。
- Codex 的对应调用是 `click {app, element_index}`。opensymph 对齐官方 window2 后也只增加了可选的 `window` 与 `screenshotId`。

**v2：** 服务端持有 per-session `SnapshotStore`；模型只传 `app`（可选 `window`）+ `element_index` 或截图坐标（可选 `screenshot_id`）。
身份不一致时服务端返回 `stale_*` 错误**并附上新状态**，模型无需再调一次观察。

### P-2 观察信息量过小、格式不利于模型

- `OBSERVATION_NODE_CAP = 64`（[protocol.rs:241](https://github.com/yclenove/grok-app/blob/025670ba/src-tauri/computer-use-core/src/protocol.rs)），超出直接截断。
  Codex 默认渲染 **1200** 节点 / 64 层深；opensymph、iFurySt 同样默认 1200。64 个节点在任何真实 App（Finder、微信、VS Code、设置）里
  都只够覆盖工具栏，模型被迫依赖截图坐标，准确率和速度双输。
- 观察是扁平 JSON 节点数组 + 可选正文；没有层级缩进、没有容器上下文、没有"可执行的次要动作"列表、没有焦点元素提示。
  Codex 的文本格式（`0 standard window …, Secondary Actions: Raise` 缩进树 + `The focused UI element is …`）对模型更友好，且经过大规模验证。
- 没有动作后状态：每次动作后模型必须再调 `computer_observe`，往返翻倍。

**v2：** Codex 风格缩进文本树 + 结构化 `structuredContent`；默认 1200 节点；树变换管线（见 [03 §4](03-architecture.md#4-观察树变换与渲染)）；每个动作返回动作后状态（同轮第二次起为 diff）。

### P-3 键盘能力残缺

`allowedKeys` 只有 18 个单键（enter/tab/esc/方向/翻页等，[catalog.json](https://github.com/yclenove/grok-app/blob/025670ba/tools/computer-use-protocol/golden/catalog.json)），
**不支持任何组合键**（⌘C、Ctrl+S、Ctrl+L、Alt+Tab、F 键）。很多日常任务（保存、全选、地址栏、切标签、撤销）无法完成或只能靠多次点击绕路。

**v2：** `press_key` 接受 xdotool 语法（`ctrl+s`、`super+c`、`Return`、`KP_0`、`F5`），与 Codex 一致；
危险组合（如 `ctrl+alt+delete`、`cmd+q` 对未授权 App、`super+l`）由策略层拦截，不靠缩小键表。

### P-4 授权流程打断对话

`computer_request_access` 返回 pending 后，工具描述要求模型**结束本轮**、等待 Host 决定，再由用户触发下一轮
（[protocol.mjs:123](https://github.com/yclenove/grok-app/blob/025670ba/tools/computer-use-mcp/protocol.mjs)）。然后还要 `computer_list_targets` → `computer_open_target` → `computer_observe` 才能开始操作。
用户体验上，一个"帮我在微信里发条消息"要经历：模型请求 → 本轮结束 → 用户在面板里选窗口授权 → 用户再说"继续" → 模型列目标 → 打开目标 → 观察 → 操作。

Codex 的流程：模型直接 `get_app_state {app:"WeChat"}` → 工具调用**阻塞**并弹出"允许 Codex 使用微信？（允许一次 / 始终允许 / 拒绝）" → 用户点一下 → 同一调用返回状态。

**v2：** 授权内联在工具调用里（MCP elicitation 或 Host 拦截），三档持久化。见 [03 §7](03-architecture.md#7-安全与授权模型)。

### P-5 自带 Chromium 运行时把团队拖进许可证泥潭

v1 为"受管浏览器" surface 打包了 Chrome for Testing / 维护版 Chromium + Playwright + Node，
随之需要：四平台 pins、四锁文件、seed、准备/修复/回滚、限额、以及**完整 Chromium NOTICE/SBOM**。
wiki 前 60 行全部是这件事的检查点（SwiftShader、ANGLE、Khronos、Crashpad、Dawn、Skia、FFmpeg 许可……），状态仍是"完整 NOTICE 未齐、R1 红"。

这是和"让模型更好地用电脑"无关的工作，却消耗了最近几十个批次。Codex 用的是自家 App 内的浏览器 + Chrome 扩展；
Windows-MCP 用 UIA 的 DOM 模式控制用户已装的 Chrome/Edge/Firefox；zavora 的浏览器后端是可选的。

**v2：** 删除受管浏览器 surface 与全部运行时打包。网页任务的三层方案：
(1) 默认：通过系统无障碍树直接操作用户已安装的浏览器（Chrome/Edge/Safari/Firefox 都暴露 AX/UIA/AT-SPI 树）；
(2) 路线 B 额外提供 App 内置 WebView 浏览窗（Tauri 自带 WebView，零额外运行时）；
(3) 可选增强：已有标签页扩展（v1 的 MV3 扩展可精简复用）提供 DOM 级语义。见 [03 §10](03-architecture.md#10-浏览器策略)。

### P-6 四类 surface × 任务授权的组合爆炸

`desktop / managed-browser / existing-tabs / app-webview` 四类 surface 各有自己的身份、授权、配对、租约、观察、动作、
下载、上传、导航、恢复语义，再与 task / run / scope / generation / pairing / lease 交叉。core 里 `broker/` 一个目录就有 100+ 文件，
`tests_task_consent_rework2_*` 这类命名说明模型反复重写过。复杂度不是来自平台，而是来自自我设计的状态机。

**v2：** 只有一个一等对象：**App 会话中的窗口**。浏览器标签页是浏览器 App 的窗口内容；内置 WebView 是 Grok App 自己的窗口（路线 B 特殊处理）。
状态机缩减为：`AppApproval`（策略）+ `Session`（MCP 会话）+ `SnapshotStore`（观察记账）+ `ActionLedger`（动作结果）。

### P-7 核心 crate 职责混杂

`computer-use-core` 依赖 `axum`、`reqwest(blocking)`、`zip`、`tar`、`flate2`、`png`、`tokio`、`windows`……
同一个 crate 同时承担：协议、Broker 状态机、HTTP IPC 服务、运行时下载与解压、Playwright 物化、NSIS 合同。
无法被独立 MCP 复用，也无法在没有网络/打包副作用的情况下做纯单元测试。

**v2：** `gcu-protocol`（纯类型）/ `gcu-core`（纯逻辑 + Driver trait，无 IO 依赖）/ `gcu-driver-*`（平台）/ `gcu-mcp`（传输）/ `gcu-cli`（二进制）严格分层，见 [03 §2](03-architecture.md#2-crate-划分与依赖规则)。

### P-8 MCP 门面依赖 Node

`tools/computer-use-mcp/server.mjs` 是 Node stdio 门面，转发到 App 的 loopback HTTP。
这让 CU 依赖用户机器上的 Node（或 App 自带 Node），又多了一个进程和一种语言。

**v2：** `gcu` 单一 Rust 可执行文件同时提供 `mcp`（直连驱动）与 `mcp --connect`（代理到 App Host）两种模式。

### P-9 vendored 上游补丁

`[patch.crates-io]` 替换了 `zbus` 和 `tauri-plugin-updater`（`src-tauri/vendor/`，37,765 行）。fork 上游库是长期维护负担，
升级 Tauri/zbus 时必然冲突。**v2：** 不 fork 上游；需要的修复先尝试上游 PR，期间用包装层规避；确需 patch 必须有 ADR + 退出条件。
（`tauri-plugin-updater` 补丁若与 CU 无关，由主线单独处理，不随 CU v2 合入。）

### P-10 过程失控：检查点文档代替交付

- 404 份计划/检查点文档、近 6 万行，平均每份描述一个"批次"，大量重复的状态句
  （"App11+/受管Mac14+与原R1–R4全部范围active/partial保持"出现了几十次）。
- `docs/llm-wiki/computer-use.md` 1454 行，其中前 1250 行是倒序检查点日志，真正的"架构/入口/工具/禁止"只在最后 200 行。
- 完成定义含混："partial"可以合入，"descriptor 关闭"可以合入，于是一直合入、一直不可发布。
- 大量精力用于"本批清理了多少 GiB 缓存""删除 0 / 实际回收 0"这类环境记账。

**v2：** 见 [08-grok-execution-protocol.md](08-grok-execution-protocol.md)：一个 STATUS 表、任务卡 DoD、禁止检查点散文、禁止 partial 合入、
环境记账不进仓库。

### P-11 macOS 最低版本与能力错配

v1 定为"App 11+ / 受管浏览器 Mac 14+"，但窗口截图若用 `CGWindowListCreateImage`（macOS 15 起弃用并对未授权进程失效），
在 11–13 上还需另一套路径。Codex 官方要求 14.4+，iFurySt/opensymph 要求 14+。
**v2：** CU 功能要求 **macOS 14+**（ScreenCaptureKit `SCScreenshotManager`）；App 本身仍可支持更低版本，只是 CU 入口在 <14 上显示为不可用并说明原因。

### P-12 测试很多，但不是对的测试

v1 有大量协议/状态机/故障测试，但缺：
- **跨平台统一夹具**（同一个带无障碍的测试 App，在三平台验证同一组动作的外部后置条件）；
- **模型任务评测**（固定任务集 + 外部判定 + 指标），v1 只有零星"真人 Grok 4.7 跑一次"；
- **性能预算**（观察/动作延迟、token/图像大小）。
**v2：** 见 [07-quality-gates.md](07-quality-gates.md)。

---

## 4. 与 Codex 体验的差距（逐项）

| 体验 | Codex | v1 | v2 目标 |
|---|---|---|---|
| 开始操作 | 直接说"在 X 里做 Y"，首次弹授权 | 请求 → 结束本轮 → 面板选目标 → 继续 | 同 Codex |
| 授权粒度 | 每 App：一次 / 始终；设置可撤销 | 每任务、每目标，无"始终" | 每 App：本轮 / 本会话 / 始终；设置可撤销；企业策略可禁"始终" |
| 一步的参数 | `app` + `element_index` | 8–10 个身份字段 | `app` + `element_index`（可选 `window`、`screenshot_id`） |
| 动作反馈 | 返回动作后状态 + 截图 | 无，需再 observe | 返回状态（首次全量、之后 diff）+ 按需截图 |
| 观察容量 | 1200 节点缩进树 | 64 节点扁平 JSON | 1200 节点缩进树 + 结构化 |
| 组合键 | xdotool 语法 | 18 个单键 | xdotool 语法 + 策略拦截 |
| 后台操作 | macOS 后台（AX + 定向事件），不抢焦点；Windows 前台 | Windows 子控件消息 + SendInput；macOS 未实测 | macOS 后台优先；Windows UIA/消息优先、前台兜底并如实报告；Linux AT-SPI 优先 |
| 可视化 | 软件光标 + 不进截图 | 面板预览 | 软件光标 + 状态胶囊（不进截图）+ App 任务面板 |
| 代码化编排 | `node_repl` + `@oai/sky` 对象 | 无 | `computer_batch`（P6）+ `computer_script`（QuickJS 沙箱，P6） |
| 禁止对象 | 终端、Codex 自身、系统安全提示 | 宿主窗口、未选窗口 | 终端、Grok 自身、密码管理器、安全软件、系统安全提示（可扩展策略） |

---

## 5. 可复用资产清单（移植到 v2 时逐项评估）

> 原则：**移植语义和经过验证的平台细节，不移植状态机。** 每项移植必须在 v2 的驱动契约测试下重新证明。

| 资产 | 位置 | 价值 | 处置 |
|---|---|---|---|
| Windows UIA 树遍历、Pattern 调用 | `src-tauri/src/computer_use/windows_uia*` | 高 | 移植到 `gcu-driver-windows/src/uia/`，改为 CacheRequest 批量取属性 |
| Windows 客户区截图、HWND 身份（stamp 防复用）、子控件消息投递 | `windows_adapter*`、`windows_identity.rs` | 高 | 移植；截图改为 WGC 优先 |
| Windows 剪贴板保存/恢复 | `windows_clipboard*` | 中 | 移植到 `gcu-driver-windows/src/clipboard.rs` |
| macOS CGWindowList / CGEvent / AX 片段 | `macos_adapter*`、`quartz_frame*` | 中 | 参考；v2 macOS 驱动按 [03 §5.1](03-architecture.md#51-macos) 重写（SCK、postToPid） |
| X11：QueryTree/GetImage/XTEST | `src-tauri/computer-use-x11/` | 中 | 评估后移植到 `gcu-driver-linux/src/x11/`（x11rb） |
| Wayland：portal、PipeWire、libei、会话/帧/设备身份检查 | `src-tauri/computer-use-wayland/` | 高（难做的部分已摸索过） | P7 阶段逐模块移植，必须去掉 vendored zbus 依赖 |
| GNOME Shell 扩展（Wayland 窗口身份） | `tools/computer-use-gnome-shell/` | 中 | P7 可选组件；Wayland 下枚举窗口唯一可行路径之一 |
| 已有标签页 MV3 扩展 | `tools/computer-use-extension/` | 中 | P8 精简复用为"DOM 增强"，配对改为 native messaging |
| 隐私脱敏 | `computer-use-core/src/privacy.rs` | 中 | 移植到 `gcu-core/src/redact.rs` |
| 结果语义、unknown 不重放 | `protocol.rs`、`broker/actions.rs` | 高 | 语义继承，代码重写 |
| 十五语言 CU 文案 | `src/i18n/messages/*/computer-use.ts` | 中 | 路线 B 复用可用键，删除与旧模型绑定的键 |
| 故障注入场景清单 | `broker/tests_*`、`d8_faults.rs` | 中 | 转写为 v2 的 core 状态机测试与驱动契约测试用例表 |
| 导航 URL 拦截规则（javascript/file/data、元数据地址） | `catalog.json` | 中 | 移植到策略层（浏览器 DOM 增强与地址栏输入共用） |

**明确删除（不移植）：** `runtime_*`、`playwright_*`、`browser_*`（受管）、`execution_runtime*`、`task_consent/`、`broker/scope_*`、
`task_picker/`、`task_scope/`、`session_grants/`、`pairing/`（v1 形态）、`nsis_contract.rs`、`tools/computer-use-browser/`、
`tools/computer-use-probe/` 中与上述绑定的脚本、全部 license/NOTICE 工具、`src-tauri/resources/computer-use/*.lock.json`。

---

## 6. 分支处置建议

1. 给当前分支打归档标签（指向远端最新提交，含审查后的 2 个提交）：`git tag archive/cu-v1-2026-10-13 025670ba`，推送标签后**不再向该分支提交**。
2. **不要把 v1 分支合入 `main`**。它包含与 CU 无关但已在 main 上另行修复的改动（v0.2.39/40 合并、composer 等），也包含要删除的大量代码。
3. v2 从最新 `main` 新开 `feat/cu-v2`，按 [06-roadmap.md](06-roadmap.md) 的任务卡逐个 PR 合入（每个 PR 都在 feature flag 后面，可随时发布 main）。
4. 移植资产时用 `git show archive/cu-v1-2026-10-13:<path>` 取原文，PR 描述里写明来源路径。
5. v1 的 `docs/plans/*computer-use*` 与 `docs/llm-wiki/computer-use.md` 不带入 main；`docs/llm-wiki/computer-use.md` 在 v2 的 P5 阶段重写为 ≤200 行的产品规则页。

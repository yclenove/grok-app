# 03 · v2 目标架构

> 本文是 v2 的技术宪法。任务卡（[06](06-roadmap.md)）中的"接口"一栏均以本文为准。本次 v1.1 按总控已批准的[评审处置](reviews/2026-10-13-disposition.md)修订计划；实现卡不得自行扩大这些决定。后续修改本文需要 ADR（[08 §6](08-grok-execution-protocol.md#6-adr-何时写怎么写)）。

---

## 1. 总览

```
                    ┌──────────────────────────── 路线 A ────────────────────────────┐
 任意 MCP 客户端 ──stdio──▶ gcu mcp ─┐                                                   │
 (grok/codex/claude/gemini)         │                                                   │
                                    ▼                                                   │
                    ┌────────── gcu-mcp（MCP 传输 + 工具注册 + elicitation）──────────┐  │
                    │                                                                  │  │
                    ▼                                                                  │  │
            ┌────────────── gcu-core ──────────────┐     ┌─── gcu-host-api ─────┐     │  │
            │ Session · AppResolver · Policy        │◀───▶│ ApprovalProvider     │◀─ A: gcu-helper 审批窗 / elicitation
            │ SnapshotStore · ElementIndex          │     │ OverlayProvider      │◀─ A: gcu-overlay；B: App 覆盖窗
            │ TreePipeline · Renderer · Diff        │     │ EventSink            │◀─ B: 任务面板 / 审计
            │ ActionRouter · ActionLedger · Settle  │     │ PolicyStore          │◀─ A: ~/.config；B: App 设置
            │ Batch · Script(feature) · Redact      │     │ Clock / UserActivity │
            └──────────────┬────────────────────────┘     └──────────────────────┘
                           │ trait Driver
        ┌──────────────────┼───────────────────┬───────────────────┐
        ▼                  ▼                   ▼                   ▼
 gcu-driver-macos   gcu-driver-windows   gcu-driver-linux     gcu-driver-fake
 AX · SCK ·         UIA · WGC/PrintWindow · AT-SPI · X11 ·    确定性内存桌面
 postToPid/SkyLight PostMessage/SendInput   Portal/PipeWire/EI  （单元/契约测试）
                    · worker 隔离
                    └──────────────────── 路线 B ────────────────────┘
 Grok App(Tauri) ── 启动并托管 `gcu engine --host-fd <fd> --stop-fd <fd>`（回退 --host-socket） 进程；
   App 侧只实现 Host 协议的服务端（审批卡/覆盖层/事件/策略/生命周期），不链接 core 与驱动；
   会话注入：MCP 条目指向 gcu 引擎暴露的 loopback MCP 端点（见 §15）
```

**一个核心，两个 Host。** 路线 A 的 Host 是 `gcu` 进程自己（加 macOS helper）；路线 B 的 Host 是 Grok App。
两个 Host 的差别**只**在 `gcu-host-api` 的实现，工具语义、渲染、策略、驱动完全相同，用同一套契约测试。
`gcu-host-api` 有两种实现：进程内（路线 A 的内置 Host）与 **IPC 远程实现**（`gcu-host-protocol`，路线 B 由 Grok App 在另一进程实现）。详见 §15。

---

## 2. Crate 划分与依赖规则

新建独立 Cargo workspace：仓库根下 `computer-use/`（不放进 `src-tauri/`，以便未来 `git subtree split` 成独立仓库）。

```
computer-use/
├── Cargo.toml                 # [workspace]，统一 lints、profile、版本
├── deny.toml                  # cargo-deny：许可白名单 MIT/Apache-2.0/BSD/ISC/Zlib/Unicode/MPL-2.0(仅动态)；禁止 GPL/AGPL/LGPL 静态
├── THIRD_PARTY_NOTICES.md
├── crates/
│   ├── gcu-protocol/          # 纯数据：工具参数/结果类型、错误码、能力声明、版本；serde + schemars；无 OS 依赖
│   ├── gcu-core/              # 纯逻辑：会话、策略、快照、树管线、渲染、diff、路由、账本、批量；依赖 protocol + host-api；无 tokio 以外 IO
│   ├── gcu-host-api/          # Host 需实现的 trait（审批、覆盖层、事件、策略存储、用户活动、时钟）
│   ├── gcu-host-protocol/     # Host 协议：上述 trait 的版本化线协议（消息类型 + 客户端/服务端编解码），供 App 等外部 Host 使用
│   ├── gcu-driver-macos/      # cfg(target_os="macos")
│   ├── gcu-driver-windows/    # cfg(windows)
│   ├── gcu-driver-linux/      # cfg(target_os="linux")：atspi / x11 / wayland 子模块
│   ├── gcu-driver-fake/       # 内存假桌面 + 场景脚本（测试与评测离线模式）
│   ├── gcu-eval/              # 评测 runner（P9-01 创建），任务与报告数据在 eval/
│   ├── gcu-script/            # feature "script"：rquickjs 沙箱，绑定到 core 的 ActionRouter
│   ├── gcu-mcp/               # MCP 服务器（rmcp），stdio 与 loopback streamable-http；代理模式
│   ├── gcu-overlay/           # 路线 A 用：软件光标 + 状态胶囊（各平台原生透明窗，不进截图）
│   └── gcu-cli/               # 二进制 `gcu`：mcp / doctor / call / batch / script / install-* / turn-ended / version
├── apps/
│   ├── gcu-helper-macos/      # 打包脚本 + Info.plist：把 gcu 包成 "Grok Computer Use.app"（LSUIElement，承载 TCC）
│   └── fixture/               # 跨平台夹具 App：eframe 0.36 默认 AccessKit（不另加 accesskit）
├── fixtures-native/           # 平台原生夹具：AppKit（objc2）、Win32/WinForms 等价（windows-rs）、GTK4（gtk4-rs）
├── eval/                      # 任务集 YAML、判定器、runner（见 07 §5）
└── xtask/                     # cargo xtask：打包、签名、生成 schema/golden、预算检查
```

**依赖规则（CI 用 `cargo deny` + 自写 `xtask check-deps` 强制）：**

| crate | 允许依赖 | 禁止 |
|---|---|---|
| gcu-protocol | serde, serde_json, schemars, thiserror | 任何 OS crate、tokio |
| gcu-host-api | gcu-protocol, async-trait | OS crate |
| gcu-core | protocol, host-api, tokio(sync,time,rt,macros), parking_lot, smol_str, smallvec, bitflags, thiserror, async-trait, serde, serde_json, toml(只读解析编入的 denylist), unicode-normalization(敏感词 NFKC), futures-core(事件 Stream), similar(仅渲染行 diff), unicode-segmentation, image(仅缩放/编码), tracing；仅 dev：insta(快照)、criterion(基准)、proptest(属性测试)、gcu-driver-fake(离线场景) | OS crate、网络、文件系统直写（经 Host） |
| gcu-driver-* | protocol, core 中的 `Driver` trait 所在模块（core 以 `pub mod driver` 暴露），平台 crate 与支持库（fake: async-trait, tokio, futures-core, serde, thiserror, ron, png；tokio/test-util 仅测试） | 互相依赖；依赖 gcu-mcp |
| gcu-mcp | protocol, core, host-api, host-protocol（RemoteHost 适配）, rmcp, tokio, axum(仅 loopback), serde/serde_json(参数与 NDJSON), thiserror(传输错误), async-trait(审批适配), base64(脱敏图片封装), tempfile(会话私有文件)；仅 dev：gcu-driver-fake, png(图片断言) | 平台 crate |
| gcu-script | gcu-protocol, gcu-core, rquickjs | OS crate、网络、文件系统、进程权限 |
| gcu-eval | gcu-protocol, gcu-core, gcu-driver-fake, serde, serde_json, tokio, clap, anyhow, thiserror, tracing, libc(仅 cfg(unix)：终止客户端进程组、以 O_NOFOLLOW 打开凭据种子；2026-10-22 真实模式加固)。任务 YAML 用 crate 内子集解析（不引入未维护的 serde_yaml）；HTML 报告不另加模板库。许可均为 MIT/Apache-2.0 | 生产凭据、非隔离运行 |
| gcu-cli | 全部 | — |
| gcu-fixture | 任意（仅测试夹具，不被其他 crate 依赖） | — |
| gcu-host-protocol | gcu-protocol, serde, serde_json, schemars(schema 黄金文件), thiserror(错误), tokio(io/net/sync/time/rt；仅 testing: process), interprocess（UDS/命名管道）, hmac 0.13 + sha2 0.11(双向证明), rand 0.10(随机秘密/nonce), libc(Unix 进程实例/继承), windows 0.62(Windows 进程实例/DACL) | core、驱动 |
| src-tauri（路线 B） | **仅** gcu-protocol、gcu-host-protocol | gcu-core、任何 gcu-driver-*、gcu-mcp、gcu-cli、gcu-overlay（`xtask check-deps` 同时检查 src-tauri） |

---

依赖核实基线（2026-10-13）：上述为运行依赖白名单，测试/构建工具按用途单独校验；不是要求一次性加入全部库。纯数据编解码留在 host-protocol，依赖 core/host-api 的 RemoteHost 适配器放 gcu-mcp。

| 用途 | 核实版本与选择 | 平台/约束 |
|---|---|---|
| MCP/schema | rmcp 3.5、schemars 1.2 | rmcp `default-features=false, features=["server","macros","transport-io","transport-streamable-http-server","elicitation"]`；stdio 只承诺 NDJSON |
| 测试 | insta 1.49、proptest 1.12、criterion 0.8 | 三平台 |
| 检查工具 | cargo-deny 0.20、cargo-llvm-cov 0.9、cargo-fuzz 0.13 | cov 需 llvm-tools-preview；fuzz 仅 Ubuntu/macOS nightly，不在 Windows 运行 |
| 数据/文本 | similar 3.2、smol_str 0.3、smallvec 1.16、unicode-segmentation 1.13、unicode-normalization 0.1.24、toml 1 | similar 仅渲染行序列 diff；树对齐始终按 fingerprint；toml 只读解析编入的 denylist，不写配置；NFKC 仅用于敏感词与执行界面关键词 |
| 通信/CLI | interprocess 2.4、axum 0.8、tokio 1.53、tracing 0.1、clap 4.6 | axum 仅 loopback；interprocess 许可 OR 可选 Apache-2.0 |
| fake CLI / 控制通道 | rand 0.10、tempfile 3、tracing-subscriber 0.3；仅 dev：assert_cmd 2 | MIT/Apache-2.0；Unix libc 0.2、Windows windows 0.62 限私有目录 ACL/命名管道；gcu-cli 依赖规则仍为 Any |
| 配置编辑 | toml_edit 0.25、serde_json（preserve_order） | TOML 保注释；JSON 保键顺序，不保证空白/JSONC 注释；JSONC 另选库需 ADR |
| 审批 UI | wry 0.57、tao 0.37 | Linux 为 GTK3 的 webkit2gtk-4.1；缺库走 zenity/kdialog |
| macOS | objc2 0.6、objc2-app-kit/objc2-foundation/objc2-screen-capture-kit/objc2-core-graphics/objc2-core-services 0.3、accessibility-sys 0.2、security-framework 3.7 | 不用 accessibility 及 servo core-graphics；SecCodeCopySigningInformation 自有 extern C |
| Windows | windows 0.62 | features 至少 Win32_System_WinRT_Graphics_Capture、Graphics_Capture、Win32_Foundation、Win32_UI_Accessibility |
| Linux 树/X11 | atspi 0.30（features=["tokio"]）、上游 zbus 5、x11rb 0.14（composite/xtest/xinput）、xkbcommon 0.9 | 不 fork zbus；xkbcommon 仅 Linux，CI 安装 libxkbcommon-dev |
| Wayland | ashpd 0.13（default-features=false, features=["tokio","screencast","remote_desktop","pipewire"]）、reis 0.7（tokio） | 不单独依赖 pipewire 0.10；沿 ashpd 的 pipewire 0.9；CI 安装 libpipewire-0.3-dev；仅 Linux |
| 脚本 | rquickjs 0.14（default-features=false, features=["std"]） | 禁 rust-alloc/allocator/loader/dyn-load/full；见 §11.5 |
| 打包 | cargo-deb 3.8、cargo-generate-rpm 0.21、cargo-cyclonedx 0.5、cargo-auditable 0.7 | deb/rpm 在 Linux；SBOM/auditable 为发行工具 |
| 夹具 | eframe 0.36（默认 accesskit）、egui 0.36（传递依赖 accesskit 0.24）；gtk4 0.11 仅 fixtures-native | 不另加 accesskit 0.25；无障碍缺口使用原生夹具 |
| Wayland 胶囊 | 待 P4-04 选型后补依赖表与 deny.toml | GNOME 不显示并声明能力，不预设可用 crate |

许可选择遵循 MIT/Apache-2.0/BSD/ISC/Zlib/Unicode 白名单及允许的 OR 分支。系统动态库 WebKitGTK、GTK、libpipewire、libxkbcommon 不由 cargo-deny 判定，需在 deny.toml 注释并单列发行依赖说明。
此规则表与 `computer-use/xtask/src/deps.rs` 的 RULES 必须一致；**代码同步由 P0-05 卡完成**，本次修订仅改计划。

---

## 3. 统一节点模型（gcu-core::model）

驱动把平台无障碍树转换为**统一原始树**（`RawNode`），core 负责变换、编号、渲染。驱动不做渲染。

```rust
pub struct RawTree {
    pub root: Option<RawNode>,       // canvas-only 窗口可无 root
    pub incomplete: Option<Incomplete>, // None = 原生枚举完整；Some = 有未读/不可靠区域
}
// protocol 定义纯数据，core::model re-export；不含原生错误载荷。
pub struct Incomplete {
    pub reason: IncompleteReason,
    pub read_nodes: u32,             // 裁剪前保留的原生节点数
    pub unread_nodes: Option<u32>,   // 精确总数未知时 None，不能猜测子树大小
}
pub enum IncompleteReason { Deadline, NodeCap, PartialError, DepthCap }

pub struct RawNode {
    pub role: Role,                  // 归一化角色（见下）
    pub native_role: SmolStr,        // AXButton / UIA_ButtonControlTypeId / ATSPI_ROLE_PUSH_BUTTON
    pub subrole: Option<SmolStr>,
    pub name: Option<String>,        // AXTitle / UIA Name / ATSPI name
    pub value: Option<NodeValue>,    // 文本/数值/布尔/区间
    pub description: Option<String>,
    pub placeholder: Option<String>,
    pub help: Option<String>,
    pub identifier: Option<String>,  // AXIdentifier / AutomationId / accessible-id
    pub states: States,              // bitflags: ENABLED FOCUSED FOCUSABLE SELECTED SELECTABLE EXPANDED EXPANDABLE
                                     //           CHECKED MIXED EDITABLE SETTABLE PASSWORD OFFSCREEN MODAL BUSY REQUIRED
    pub actions: SmallVec<[NodeAction; 4]>, // 归一化动作 + 原生名
    pub frame: Option<Rect>,         // 窗口相对、物理像素（与截图同一坐标系）
    pub text_extras: Option<TextExtras>, // 选中范围、可见范围、长文本（受 text_limit 约束）
    pub url: Option<String>,         // 链接/网页根
    pub children: Vec<RawNode>,
    pub handle: NativeHandle,        // 驱动私有不透明句柄（Arc<dyn Any + Send + Sync>）
    pub fingerprint: Fingerprint,    // 驱动计算：role + name + identifier + 祖先路径哈希 + frame 量化
}

pub enum Role { Window, Dialog, Sheet, Popover, Menu, MenuItem, MenuBar, Toolbar, Button, Link, CheckBox, Radio, Switch,
  TextField, SearchField, TextArea, ComboBox, PopUpButton, Slider, Stepper, ProgressBar, List, ListItem, Table, Row, Cell,
  Column, Outline, Tree, TreeItem, Tab, TabList, ScrollArea, ScrollBar, Group, SplitGroup, Image, StaticText, Heading,
  WebArea, Document, Canvas, Unknown }

pub enum NodeAction { Press, ShowMenu, Increment, Decrement, Confirm, Cancel, Raise, Expand, Collapse, ScrollIntoView,
  ScrollUp, ScrollDown, ScrollLeft, ScrollRight, Select, Toggle, Native(SmolStr) }
```

**角色映射表**放在各驱动 `src/roles.rs`，并有表驱动单测；未知原生角色映射为 `Unknown` 且保留 `native_role`。

原生完整性见 [ADR 0004](../../../computer-use/docs/adr/0004-raw-tree-completeness.md)：2s / 5000 或深度限制、容忍的字段/子节点读取失败必须上报；首个原因保留，期限中断优先。首节点未读完仍报超时，已有节点则保留部分树；取消/致命错误仍返回错误。`root: None` 本身不代表不完整。

---

## 4. 观察：树变换与渲染

### 4.1 管线（gcu-core::tree，顺序固定，每步纯函数 + 单测）

| # | 变换 | 作用 | 参考 |
|---|---|---|---|
| 1 | `prune_offscreen` | 去掉 OFFSCREEN 且无可见后代的子树（保留滚动区内的被裁剪项计数） | — |
| 2 | `filter_actions` | 去掉噪音动作（ScrollToVisible、ShowDefaultUI 等），保留有意义的 secondary actions | Codex `filterActions` |
| 3 | `associate_title_elements` | 把 label/静态文本与其标注的控件合并为控件名（AXTitleUIElement、UIA LabeledBy、ATSPI LABELLED_BY） | `associateTitleUIElements` |
| 4 | `prune_empty_disabled` | 去掉禁用且无名无值的叶子 | `pruneEmptyDisabledElements` |
| 5 | `prune_non_descriptive` | 去掉无名、无值、无动作、无可交互后代的子树 | `pruneNonDescriptiveSubtrees` |
| 6 | `flatten_into_selectable_ancestor` | 行/单元格内的纯展示子节点并入可选中祖先（行文本作为行名） | `flattenIntoSelectableAncestor` |
| 7 | `merge_single_item_groups` | 只有一个子节点的无名 Group 折叠 | `mergeSingleItemGroups` |
| 8 | `flatten_redundant_hierarchy` | Electron/WebView 的空 wrapper 链压平 | `flattenRedundantHierarchy` |
| 9 | `merge_text_only_siblings` | 相邻纯文本合并 | `mergeTextOnlySiblings` |
| 10 | `flatten_repetitive_static_text` | 大量重复结构的静态文本折叠为"… N more" | `flattenRepetitiveStaticText` |
| 11 | `flatten_links_into_markdown` | 网页区域内链接渲染为 `[text](url)`，减少节点数 | `flattenLinksIntoMarkdownText` |
| 12 | `budget` | 按 `max_tree_nodes`（默认 1200）与 `max_tree_depth`（默认 64）截断；广度优先保证弹窗/焦点祖先链/可交互节点优先；输出 `truncated{omitted_nodes}` | — |
| 13 | `assign_indices` | 先序遍历分配 `element_index`（0 起，窗口为 0），写入 `ElementIndex` | Codex |

**弹窗优先：** 若存在 MODAL dialog/sheet/alert，预算优先分配给它与窗口标题栏；背景内容只保留一级摘要。

`RawTree.incomplete` 在管线入口独立保留，不受裁剪/预算/编号影响；原生未读完仍交付文本与 `truncated` 标记，不能把缺失节点当作敏感区域不存在。

### 4.2 文本渲染格式（模型主读物）

与 Codex 对齐，便于迁移其使用经验；首行与窗口行固定：

```
App=com.tencent.xinWeChat (pid 1432) — 微信
Window: "微信", id 8812, 1280×860 @2x, App: 微信.
0 standard window 微信, Secondary Actions: Raise
    1 split group
        2 list 会话列表
            3 row (selectable) 文件传输助手 — 昨天 21:04
            4 row (selected) 张三 — 好的，明天见
        12 group 聊天
            13 heading 张三
            14 scroll area Secondary Actions: Scroll Up, Scroll Down
                15 static text 好的，明天见
            21 text area (settable, focused) Placeholder: 输入消息
            22 button 发送
The focused UI element is 21 text area.
[truncated: 143 nodes omitted under 2 list]
```

规则：
- 每行：`<index> <role 小写人类可读> <name>`，后接按需属性：`(selected, expanded, settable, string, focused, disabled, checked)`、
  `Value: …`、`Description: …`、`Placeholder: …`、`Help: …`、`ID: …`、`Secondary Actions: A, B`、`URL: …`。
- 每个字符串字段受 `text_limit`（默认 500 字符/字段，`"max"` 不截断）约束，截断处标 `…(+N chars)`。
- 密码字段的值恒为 `•••`（见 §7.6 脱敏）。
- 末尾：焦点行、截断说明、（若动作）动作摘要行。
- 原生未读完另附 `[truncated: native tree incomplete (reason); N nodes read; unknown nodes unread]`；精确未读数可得时替换 `unknown`，不与已知预算遗漏混算。

### 4.3 结构化内容（structuredContent，供 Host/脚本/评测）

```jsonc
{
  "app": {"id": "com.tencent.xinWeChat", "name": "微信", "pid": 1432},
  "window": {"id": "8812", "title": "微信", "frame": {"x":0,"y":0,"w":1280,"h":860}, "scale": 2.0},
  "snapshot_id": "s_7f3a",           // 服务端记账用，模型无需回传
  "screenshot": {"id": "i_92c1", "width": 1280, "height": 860, "mime": "image/png"} | null,
  "focused": 21,
  "truncated": {"omitted_nodes": 143, "native_incomplete"?: {"reason":"deadline|node_cap|partial_error|depth_cap", "read_nodes":5000, "unread_nodes":null}} | null,
  "elements": [ {"index": 21, "role": "text_area", "name": null, "states": ["settable","focused"], "actions": [], "frame": {...}} ],  // 仅在 detail=full 时
  "action": { ... } | null            // 动作结果，见 §6
}
```

`elements` 默认不随文本重复下发（避免双倍 token）；`gcu call --json`、脚本、评测器可请求 `detail: "full"`。
`omitted_nodes` 只数 core 已读后遗漏的节点；原生不完整时 `truncated` 恒非 null，可能 `omitted_nodes: 0`。完整树省略 `native_incomplete`，保持既有输出。
在 Grok 1.0.52 上 `structuredContent` 会被拼进文本末尾，且工具文本只保留约 20,000 字节前缀（超出部分写入会话溢出文件，尾部 JSON 丢失）。因此：文本渲染与 `text_limit` 的总输出必须落在 20 KB 前缀内，结构化 JSON 只放精简事实（动作结果、ID），长内容放文件路径。

### 4.4 截图

- 必经交付管线：原始树先收集密码/敏感区域 → 树裁剪与截图采集 → 按同一状态版本打码 → 缩放/编码 → 模型图片、缩略图、审批裁剪图、临时文件。登录/安全场景无法可靠定位敏感区域或状态不同步时，不输出截图并 `handoff_required`；首次截图交付（P0-07/P1-04/P2-03/P3-02）即验收，不能等 P9。
- 原生树 `incomplete.is_some()` 一律视为敏感区域无法完整定位，沿同一拒图/handoff 路径处理，不依赖是否已读到登录或密码信号；截图前后任一次原生树不完整均不得交付截图。`include_screenshot: false` 仍交付已读文本。
- 窗口级截图（不是全屏），物理像素；长边超过 `max_image_edge`（默认 1568）时等比缩放，记录 `scale_to_window`。
- 坐标动作的 `x,y` 永远是**该截图的像素坐标**；服务端换算为窗口点坐标再到屏幕坐标。
- 默认 PNG；`image_format: "jpeg"`（质量 82）可选；单图上限 1.5 MB，超出自动降质/缩放并在结果中注明。
- **Grok 1.0.52 的图片到达条件（[ADR 0001](../../../computer-use/docs/adr/0001-grok-build-mcp-capabilities.md)）：** MCP image 只有在像素量约 6 万以上（实测 344×160 失败、344×190 成功）时才作为视觉输入，更小的图被摊成文本 data URL，模型读不出。因此 `image_min_short_edge = 256`：窗口截图自然满足；审批裁剪图等小图先放大到短边 ≥256 再下发，仍不满足则不附图并改用文件路径。工具文本同时给出临时图片路径作后备（模型可用 `read_file`；路径只对本会话有效、会话结束删除，且图片经同一脱敏管线）。1.6 MB 级 PNG 会被客户端重编码为 JPEG，故图片目标上限按 ≤300 KB 的 PNG 设计。
- `get_app_state` 默认附图（`include_screenshot` 默认 true）；动作结果默认**仅当视觉可能变化且树无法表达时**附图：
  规则 `attach_image = action_used_coordinates || window_is_canvas_like || diff_is_empty_but_pixels_changed || explicit`。
- 截图与覆盖层：覆盖层窗口设置为不参与截图（macOS `NSWindow.sharingType = .none`；Windows `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`；
  X11 无此能力 → 采集期间隐藏覆盖层）。

### 4.5 Diff（同一轮内的第二次起）

`SnapshotStore` 记录每个窗口本轮上一次**已下发给模型**的渲染行。动作后：
1. 重新取树并渲染；2. 以元素 fingerprint 对齐新旧节点（不是按 index）；3. 输出：

```
Changes since last state (window 8812):
~ 21 text area (settable, focused) Value: 明天下午三点开会   ← was: (empty)
+ 23 button 发送 (enabled)                                  ← was disabled
- 30 static text 正在输入…
Unchanged: 1180 nodes. Indices of unchanged nodes are preserved.
```

- **索引稳定性承诺：** diff 模式下未变化节点保留原 index；新增节点分配新 index（从本窗口历史最大值+1 起），不复用旧 index。
- diff 行数超过 60 行或结构变化 >30% 时退回全量渲染（`[full state: large change]`）。
- 新一轮（turn）开始后，第一次观察总是全量。

---

## 5. Driver trait（gcu-core::driver）

```rust
#[async_trait]
pub trait Driver: Send + Sync + 'static {
    fn platform(&self) -> Platform;
    fn capabilities(&self) -> &Capabilities;           // 见 §5.4，静态 + 运行时探测
    async fn doctor(&self) -> DoctorReport;             // 权限、依赖、显示服务器、私有 API 可用性

    // 发现
    async fn list_apps(&self) -> Result<Vec<AppInfo>>;              // 运行中 + 最近使用；含签名身份（§7.2）
    async fn resolve_app(&self, query: &AppQuery) -> Result<AppRef>; // 名称/bundle id/exe/desktop id/路径 → 唯一 App
    async fn launch_app(&self, app: &AppRef) -> Result<AppRef>;
    async fn list_windows(&self, app: Option<&AppRef>) -> Result<Vec<WindowInfo>>;
    async fn key_window(&self, app: &AppRef) -> Result<WindowInfo>;  // 无窗口时尝试 unhide/restore，不激活前台

    // 观察
    async fn snapshot_tree(&self, win: &WindowRef, opts: &TreeOpts, cancel: &Cancel) -> Result<RawTree>;
    async fn capture(&self, win: &WindowRef, opts: &CaptureOpts, cancel: &Cancel) -> Result<Capture>;
    async fn window_geometry(&self, win: &WindowRef) -> Result<Geometry>;   // 便宜，用于门禁
    async fn focused_element(&self, app: &AppRef) -> Result<Option<NativeHandle>>;

    // 动作（每个方法返回实际使用的投递路径；失败时声明是否"可能已部分执行"）
    async fn perform(&self, req: &DriverAction, cancel: &Cancel) -> Result<Delivery, DriverError>;

    // 订阅（可选能力）：AX/UIA/AT-SPI 事件，用于 settle 与 wait_for
    fn subscribe(&self, win: &WindowRef) -> Option<BoxStream<'static, UiEvent>>;

    // 输入安全
    async fn release_all_input(&self);                  // InputLedger：释放所有 AI 按下未抬起的键/键
}

pub enum DriverAction {
    Press { el: NativeHandle },                          // 语义点击
    SecondaryAction { el: NativeHandle, action: NodeAction },
    SetValue { el: NativeHandle, value: String },
    SelectText { el: NativeHandle, range: TextRange },
    ScrollElement { el: NativeHandle, dir: Dir, pages: f32 },
    PointerClick { win: WindowRef, at: WindowPoint, button: Button, count: u8, route: PointerRoute },
    PointerScroll { win: WindowRef, at: WindowPoint, dx: i32, dy: i32, route: PointerRoute },
    PointerDrag { win: WindowRef, path: Vec<WindowPoint>, route: PointerRoute },
    TypeText { win: WindowRef, focus: Option<NativeHandle>, text: String, route: KeyRoute },
    PressKeys { win: WindowRef, chord: KeyChord, route: KeyRoute },
    Activate { win: WindowRef },                         // 仅当策略允许前台
}

pub enum Delivery { Accessibility, TargetedEvent, Foreground, Clipboard }
pub enum PointerRoute { Auto, TargetedOnly, AllowForeground }   // ActionRouter 决定；驱动不得越级
```

**驱动铁律：**
1. 不渲染、不做策略、不记账。2. 不得静默升级投递路径（`TargetedOnly` 失败就返回错误）。
3. 每个阻塞原生调用必须可被 `Cancel` 打断或有硬超时（默认 5s），超时返回 `DriverError::Timeout { maybe_executed }`。
4. 所有 FFI 位于 `unsafe` 模块并有 `// SAFETY:` 注释；COM/AX 调用在专用线程。
5. 输入前先记账到 InputLedger（参考 Windows-MCP），确保接管/故障/取消时 `release_all_input` 能补发 up 事件。

### 5.1 macOS

| 能力 | 实现 | crate |
|---|---|---|
| App 发现 | `NSWorkspace.runningApplications` + Spotlight `kMDItemLastUsedDate`/`kMDItemUseCount`（`list_apps` 最近 14 天） | objc2-app-kit, objc2-foundation, objc2-core-services（MDQuery） |
| 签名身份 | `SecStaticCode::from_path`；自有 `extern "C" SecCodeCopySigningInformation` 读取 `kSecCodeInfoTeamIdentifier`/`kSecCodeInfoIdentifier` | security-framework 3.7 + 驱动 ffi.rs |
| 树 | `AXUIElementCopyMultipleAttributeValues` 批量取属性；`AXEnhancedUserInterface`/`AXManualAccessibility` 对 Chromium/Electron 开启完整树（记录并在会话结束恢复原值） | 仅 accessibility-sys 0.2（不引入 accessibility） |
| 截图 | ScreenCaptureKit 异步 `SCScreenshotManager.captureImageWithFilter_configuration_completionHandler`（SCContentFilter、SCStreamConfiguration、completion 回调取 CGImage，失败返回驱动错误）（macOS 14+）；窗口被遮挡仍可采 | objc2-screen-capture-kit |
| 语义动作 | `AXUIElementPerformAction`（AXPress/AXShowMenu/AXIncrement/AXRaise/AXScroll*ByPage）、`AXUIElementSetAttributeValue(kAXValue)`、`AXSelectedTextRange` | — |
| 定向事件 | `CGEvent` + `postToPid`；鼠标事件设置 window number、window-local 位置（`CGEventSetWindowLocation` **私有**，`dlsym` 运行时解析） | objc2-core-graphics（CGEvent feature） |
| SkyLight 后台键/点击 | `SLPSPostEventRecordTo` / `SLEventPostToPid`（**私有**，feature `macos-private-spi`，默认开启但运行时探测 + 远程 kill switch 配置项） | dlsym |
| 前台兜底 | `CGEvent.post(.cghidEventTap)`；仅策略 `allow_foreground` 时 | — |
| 文本输入 | 优先 `AXValue` 追加（可设置且非富文本）；否则按 grapheme cluster 分块 `keyboardSetUnicodeString` 定向投递；中文/emoji 不拆分 | unicode-segmentation |
| 用户活动 | 被动 `CGEventTap(listenOnly)`（需"输入监控"权限，缺失时守卫降级并在 doctor 中说明） | — |
| 事件订阅 | `AXObserver`（value changed、focused changed、window created、layout changed）用于 settle/wait | — |
| 不可见窗口 | 可选 `agent_display`（虚拟显示器停放窗口，P10，私有 API `CGVirtualDisplay`） | — |

最低版本：**macOS 14.0**。TCC 身份：路线 A 为 `Grok Computer Use.app`（helper）；路线 B 为 Grok App——`gcu` 引擎由 App 以子进程方式启动，macOS 把子进程的 TCC 责任归到父 App（"responsible process"），系统设置里显示"Grok"（S-02 必须实测确认；若不成立，改为把 `gcu` 作为 App bundle 内的 helper 并以 `open`/XPC 启动，仍属 App 签名身份）。

### 5.2 Windows

| 能力 | 实现 | crate |
|---|---|---|
| App 发现 | `EnumWindows` + 进程映像路径 + Authenticode 发布者（`WinVerifyTrust` + `CryptQueryObject`）；开始菜单 `.lnk` 后台异步索引并缓存（避免 opensymph 的 3.5s） | windows 0.62 |
| 树 | UIA IUIAutomation + CacheRequest 批量属性（Name、ControlType、AutomationId、BoundingRectangle、IsEnabled、HasKeyboardFocus、IsPassword、各 Pattern 可用性属性，不缓存 pattern 对象）；专用 MTA 线程；IUIAutomationStructureChangedEventHandler / IUIAutomationPropertyChangedEventHandler / IUIAutomationFocusChangedEventHandler 三类订阅 | windows |
| 截图 | Windows.Graphics.Capture（`IGraphicsCaptureItemInterop::CreateForWindow(hwnd)`，Win10 1903+；不用 TryCreateFromWindowId；Win11 在 GraphicsCaptureSession 探测并关闭 IsBorderRequired/IsCursorCaptureEnabled）；失败回退 `PrintWindow(PW_RENDERFULLCONTENT)` | windows |
| 语义动作 | Invoke / Toggle / SelectionItem / ExpandCollapse / Value.SetValue / RangeValue / Scroll / Text(选区) / LegacyIAccessible.DoDefaultAction | — |
| 定向消息 | `PostMessage` WM_LBUTTONDOWN/UP、WM_CHAR、WM_KEYDOWN/UP 到命中的子 HWND（对 Win32/WinForms/部分 WPF 有效，对 Chromium/UWP 通常无效——必须检测 effect） | — |
| 前台 | `AttachThreadInput → AllowSetForegroundWindow → SetForegroundWindow → detach` 激活后 `SendInput`；仅策略允许；遵守用户活动守卫 | — |
| 文本 | Value Pattern 优先；否则 `SendInput(KEYEVENTF_UNICODE)` 分块；>8192 UTF-16 拒绝或分批 | — |
| 用户活动 | `WH_MOUSE_LL`/`WH_KEYBOARD_LL` 被动钩子（过滤 `LLMHF_INJECTED`/`LLKHF_INJECTED`），周期替换钩子防静默移除 | — |
| 接管热键 | `RegisterHotKey(Ctrl+Alt+Shift+Backspace)`（可配置） | — |
| 隔离执行 | `ExecMode::InProcess | Worker`：Worker 模式下 UIA 遍历与截图在 `gcu worker` 子进程执行（stdin/stdout 帧协议），崩溃/超时重启（参考 opensymph 浸泡结论）；默认 Worker，基准不达标再评估 | — |
| 虚拟桌面 | `IVirtualDesktopManager::IsWindowOnCurrentVirtualDesktop`；不在当前桌面的窗口标注且前台动作拒绝 | — |
| 完整性级别 | 目标进程完整性高于自身（管理员窗口）时 UIPI 会拒绝输入 → 返回 `elevation_required`，不尝试提权 | — |

生产引擎启动即检查自身完整性与 UIAccess：High/System 或 UIAccess → 拒绝工作，不自行提升或降权绕过。管理员目标、非交互/安全桌面在观察及所有动作路线前统一拒绝（`elevation_required` / `handoff_required`），不只依赖 SendInput 的 UIPI 失败。

最低版本：Windows 10 1903（WGC）；Win7/8 不支持。

### 5.3 Linux

| 能力 | X11 | Wayland |
|---|---|---|
| 树 | AT-SPI2（`atspi = { version = "0.30", features = ["tokio"] }`，zbus 上游版本，不 fork；子接口操作外层 tokio 超时） | 同左 |
| 窗口枚举/身份 | `_NET_CLIENT_LIST` + `_NET_WM_PID`（x11rb） | 无标准 API：GNOME 用可选 Shell 扩展（移植 v1）、KDE 用 KWin 脚本 D-Bus、其他合成器用 AT-SPI frame 近似；能力表如实声明 |
| 截图 | `XComposite` + `GetImage`（x11rb） | xdg-desktop-portal ScreenCast（window source）+ PipeWire 取一帧；restore token 持久化，避免每次弹窗（ashpd 的 pipewire feature，不单独依赖 pipewire） |
| 语义动作 | AT-SPI Action / EditableText / Value / Selection / Text | 同左（最可靠路径） |
| 指针/键盘 | XTEST（全局，等同前台） | portal RemoteDesktop + libei（`reis`），需用户首次同意；坐标需映射到流 |
| 用户活动 | XInput2 原始事件监听 | 无通用方案 → 守卫降级（doctor 说明） |

restore token 仅当前用户可读（目录 0700、文件 0600），绑定授权身份、进程实例、compositor、source/窗口及授权版本；有效期不超过 24h 且不得超过授权期限。关闭功能、撤销、会话结束时关闭实际 portal/输入会话并清除对应 token；恢复时重新验证来源与范围，无法证明仅覆盖授权目标则拒绝输出/输入。

### 5.4 能力声明（`capabilities` 工具 / doctor / initialize instructions 共用）

```jsonc
{ "platform": "macos", "version": "15.1",
  "observe": {"tree": true, "screenshot": true, "events": true},
  "delivery": {"accessibility": true, "targeted_pointer": true, "targeted_keys": true, "foreground": "policy"},
  "background_operation": "full" | "partial" | "none",   // mac=full, win=partial, linux-x11=partial(AT-SPI)/none(pointer), wayland=partial
  "user_activity_guard": true, "overlay_excluded_from_capture": true,
  "private_apis": {"skylight": "available" | "disabled" | "missing"},
  "limits": {"max_tree_nodes": 5000, "max_image_edge": 2560, "type_text_max": 8192} }
```

---

## 6. 动作结果语义

每个动作结果（`structuredContent.action`）：

```jsonc
{
  "tool": "click",
  "target": {"element_index": 22, "label": "发送"} | {"x": 840, "y": 812, "screenshot_id": "i_92c1"},
  "delivery": "accessibility" | "targeted_event" | "foreground" | "clipboard" | "none",
  "outcome": "done" | "refused" | "failed" | "unknown",
  "effect": "changed" | "no_visible_change" | "not_checked",
  "verify": {"expect": "...", "state": "satisfied" | "unsatisfied" | "unknown"} | null,   // 仅 batch/expect 时
  "duration_ms": 182,
  "settle": {"ms": 240, "reason": "tree_stable" | "event_quiet" | "budget"},
  "notes": ["Window was not frontmost; delivered without activation."]
}
```

- `done`：投递成功且未报错（≠ 业务成功）。`refused`：策略/门禁拒绝，**确定未执行**。`failed`：执行前或执行中失败，**确定无副作用**。
  `unknown`：已开始投递但无法确认（超时、驱动崩溃、目标消失）——**禁止自动重放**；结果附新状态，由模型决定。
- `effect` 由 settle 后的 diff + 像素哈希判断；`no_visible_change` 不是错误，但文本会提示"动作后界面无变化，可能未生效"。
- **Settle：** 动作后等待 UI 稳定：订阅事件静默 120ms 或连续两次树哈希一致，硬上限 `settle_budget_ms`（默认 800，`type_text` 1500）。
  不支持事件订阅的平台用 150ms 轮询，上限同上。

---

## 7. 安全与授权模型

### 7.1 三层

1. **系统权限层**（OS）：macOS TCC（辅助功能、屏幕录制、可选输入监控）、Wayland portal 同意。由 doctor/onboarding 引导，不自动点系统对话框。
2. **App 授权层**（gcu 策略）：每个 App 一条授权记录。
3. **动作层**：敏感动作确认、拒绝名单、组合键黑名单、用户活动守卫、Stop。

### 7.2 App 身份（授权的主键）

| 平台 | 主键 | 辅助（防冒名） |
|---|---|---|
| macOS | bundle id | 代码签名 team id（`always` 授权绑定 team id；未签名 App 绑定可执行文件路径 + cdhash） |
| Windows | 可执行文件规范路径（小写） | Authenticode 发布者主体；未签名绑定路径 + 文件哈希（更新后需重新授权） |
| Linux | 实际进程映像规范路径 + 文件身份(dev/inode) + sha256 内容摘要 | desktop id 仅用于发现；每次使用校验进程实例与映像；替换/更新重新授权 |

签名绑定不证明进程内全部代码可信（插件/DLL/可写加载路径仍可能改变代码）。每次使用验证进程实例、规范映像及签名有效性；更新（构建摘要改变）、证书身份/指纹变化、产品身份变化或路径替换均使旧授权失效；同名发布者字符串不是同一证书。插件/DLL 的注入边界在审批中说明，不能仅凭入口签名声称宿主安全。

### 7.3 授权档位与流程

```
模型调用任何以 app 为参数的工具
   │
   ├─ App 在拒绝名单？ ──是──▶ refused: app_denied（说明原因），不弹窗
   ├─ 已有有效授权（always / 本会话 session / 本轮 turn）？ ──是──▶ 继续
   └─ 否 ──▶ ApprovalProvider.request_app(app, purpose, risk) —— **阻塞当前工具调用**（超时默认 120s）
              ├─ 本轮  → turn 授权，turn-ended 时失效
              ├─ 本会话允许        → session 授权，MCP 会话结束时失效
              ├─ 始终允许          → 持久化（PolicyStore），设置页可撤销；企业策略可禁用此档
              └─ 拒绝 / 超时       → refused: approval_denied / approval_timeout
```

- 只读工具 `list_apps`、`list_windows` 不需要 App 授权（只返回名称/标题，标题可按策略脱敏）。
- `get_app_state` 需要授权（读内容即敏感）。

底层枚举固定为 `ApprovalDecision { Turn, Session, Always, Deny }`，wire 为 `turn/session/always/deny`；UI 为“本轮/本会话/始终允许/拒绝”。超时是审批错误 `approval_timeout`，不是第五种授权档位。
审批绑定 App 身份/进程实例/窗口/动作参数（含收件人和提交内容）/snapshot_id+fingerprint/状态版本/策略版本；批准后、投递前重验，变化使决定作废并重新观察、重审。
审批显示 OS 核验的签名状态、发布者、规范路径；名称/图标仅辅助；理由标“模型提供”，纯文本并处理双向控制字符；默认焦点为拒绝，卡片出现不得劫持聊天输入框的 Enter。

- `purpose` 由 MCP 工具参数可选字段 `reason` 提供（模型写一句为什么），显示在审批卡上。

### 7.4 拒绝名单（内置，不可通过"始终允许"覆盖；企业/用户只能追加）

- **Grok 自身：** Grok App、Grok CLI 所在终端窗口（路线 B 记录会话的 TTY/宿主 PID）、`gcu` helper。
- **终端与 shell 宿主：** Terminal、iTerm2、WezTerm、Alacritty、kitty、Ghostty、Warp、Hyper、Windows Terminal、conhost、PowerShell ISE、
  GNOME Terminal、Konsole、xterm 等（规则维护在 `gcu-core/src/policy/denylist.rs`，名称仅提示信号；核验映像路径、签名产品身份、已知二进制哈希，解析符号链接/junction/启动包装器，表驱动测试；未知执行宿主 handoff，名单不是全部终端的完备识别器）。理由同 Codex：防止绕过 shell 沙箱与审批。
- **凭据与安全：** 1Password、Bitwarden、KeePassXC、Dashlane、LastPass、钥匙串访问、Windows 凭据管理器、主流杀毒/EDR 控制台。
- **系统安全提示：** macOS `SecurityAgent`、`coreautha`、`UserNotificationCenter` 授权对话框；Windows `consent.exe`（UAC）、`CredentialUIBroker.exe`、LogonUI；
  Linux polkit 代理窗口。遇到这些窗口出现在目标 App 之上时返回 `handoff_required`（请用户处理）。
- IDE 可允许普通编辑区，但终端/REPL/任务执行/调试执行入口的**所有动作路径**（坐标、菜单、secondary action、拖拽、batch/script）均拒绝，无法确认性质时 handoff；P0-08 通用规则与 fake 场景、P1-03/P2-02/P3-01 角色/名称启发式从首次交付即覆盖。
- 执行环境限制同样适用于 App 内部功能：Web SSH、云终端、容器控制台、远程桌面、调试控制台。窗口标题/URL 关键词表 + 原生角色 terminal/console 的识别是**尽力而为，非完备**；不能可靠隔离的混合界面仅允许观察或 handoff。

### 7.5 敏感动作确认

ActionRouter 在执行变更动作前计算风险：

| 信号 | 例 |
|---|---|
| 目标元素名/描述匹配多语言敏感词表 | 发送/Send/送信、支付/Pay/购买/Buy/下单、删除/Delete/移到废纸篓、提交/Submit、转账、确认付款、Sign/签署、Post/发布 |
| 目标是密码字段 | `set_value`/`type_text` 到 PASSWORD 节点 |
| 组合键 | `cmd+delete`、`shift+delete`、`ctrl+enter`（多数 IM 的发送）、`cmd+q`/`alt+f4` |
| 未知语义 | 浏览器/网页区/未知语义目标中，无可靠动作语义的坐标点击、Enter/Space、含换行 type_text、自定义快捷键，默认高风险 |
| App 类别 | 银行/证券/支付类（策略可配置 bundle 列表） |

`risk = high` 时调用 `ApprovalProvider.confirm_action(app, summary, screenshot_crop)`：卡片显示"Grok 将在「微信」中点击「发送」"及目标区域截图。
设置项 `sensitive_action_confirmation: off | standard(默认) | strict(所有变更动作)`。
web area 默认 `strict`；standard 对上述未知语义信号弹确认，不能因名称伪装/无文字图标降为低风险。企业策略可强制 `strict`。词表放 `gcu-core/src/policy/sensitive_terms/*.txt`（15 语言），有测试。

### 7.6 隐私与脱敏

- 密码节点值恒为 `•••`，且不进入日志/structured。
- 截图打码为 §4.4 的必经阶段：先收集敏感区域再裁树；所有衍生物只能来自脱敏结果；无法可靠定位则不交付并 handoff（A-α 前）。
- `RawTree.incomplete` 必须贯穿到脱敏入口；即使已读密码区域都有 frame 或渲染预算未截断，也无法证明未读区域安全，拒绝出图并 handoff，纯文本观察不受影响（[ADR 0004](../../../computer-use/docs/adr/0004-raw-tree-completeness.md)）。
- 日志默认只记工具名、App id、结果、耗时；不记文本参数与截图（`GCU_LOG_PAYLOADS=1` 才记录脱敏载荷，且仅本地；该开关绝不取消秘密脱敏，错误信息同样脱敏）。
- 剪贴板：不读入观察；跨会话全局互斥，保存 → 写入 → 粘贴前校验序号/内容/目标焦点 → 粘贴 → 若仍为我们写入的内容则恢复（Windows GetClipboardSequenceNumber、macOS changeCount）。变化即取消；敏感文本及高风险目标禁止剪贴板回退。无法消除恶意进程最后一刻替换，剪贴板历史/同步可能保存文本，能力说明必须如实告知。
- 数据清单覆盖原图、缩略图、审批裁剪图、CLI 临时图片、调试载荷、错误日志：内存按会话隔离，临时文件仅当前用户可读；会话删除即清理图片及载荷，元数据日志最多保留 7 天并遵守 5×5MB 上限。截图查询必须核验会话归属；P9-04 审计所有衍生物。

### 7.7 用户活动守卫与接管

- 用户在 `user_idle_ms`（默认 3000）内有物理输入时：需要前台的动作返回 `user_active`（附"等待用户空闲后重试"提示）；
  后台路径（AX/定向事件）不受影响（macOS 主路径因此几乎不打扰用户）。
- **Stop 热键：** macOS `⌃⌥⌘.`、Windows/Linux `Ctrl+Alt+Shift+Backspace`（可配置）。按下 → 立即 `release_all_input` → 当前与后续调用返回 `stopped_by_user`，锁存 Stop。模型下一轮、会话重连、引擎重启均不得自动解除；Host 持久保存停止状态且新引擎默认禁止投递，握手后恢复锁存。只有用户在可信 UI（胶囊“恢复”/App 面板）或 `gcu resume` CLI 显式恢复；模型工具不能调用恢复。
- 覆盖层胶囊上有“停止/恢复”按钮。独立停止控制通道 + watchdog，不排在动作/审批队列后；Stop ≤100ms 锁存、≤500ms 发输入释放，原生阻塞 ≤5s 超时或杀 worker。注册失败/权限撤销/监听失效则禁止启动依赖该停止能力的投递路线；Wayland 无可靠热键时禁用相应输入路线并声明能力。

### 7.8 其他门禁

- 坐标动作：必须引用本会话中该窗口最近一次带截图的观察（`screenshot_id` 省略即用最新）；观察后窗口几何变化（移动/缩放/DPI）→ `stale_geometry` + 新状态；坐标越界 → `out_of_bounds`。观察时记录点击点周围 32×32 物理像素小块的目标区域内容哈希（边缘裁剪，使用同一脱敏/覆盖层处理）；批准后投递前重采核验，内容不一致 → `stale_screenshot` 并附新状态，不能仅验几何。
- 元素动作：`element_index` 映射到 fingerprint；执行前用句柄重新读取 role/name/frame 校验；不一致 → `stale_element` + 新状态。**绝不按名字去找"另一个相似元素"代替。**
- 窗口关闭/App 退出 → `window_gone` + `list_windows` 结果，不自动切换。
- 对自己发出的输入拦截：焦点元素是浏览器地址栏（角色/名称启发式）时，gcu 发出的文本输入/提交返回 `key_blocked`。任意目标输入文本以 javascript:/file:/data:/vbscript:/blob:、已知外部协议开头，或命中元数据主机（例如 169.254.169.254、169.254.170.2、100.100.100.200、metadata.google.internal；含大小写/等价IP/主机写法）均 `key_blocked`，包括 set_value/type_text/粘贴/快捷键提交/batch/script。L1 browser_navigate、L2 自身导航请求用同一规则。此规则只拦截 gcu 自己的输入，不声称 L0 能拦截浏览器内部页面重定向。

---

## 8. 会话与生命周期

| 概念 | 定义 | 由谁维护 |
|---|---|---|
| Session | 一个 MCP 连接（stdio 进程生命周期，或 HTTP `Mcp-Session-Id`） | gcu-mcp |
| Turn | 模型的一个 assistant 轮次。边界来源：路线 B 由 App 从 ACP 流得知并调用 `core.turn_ended()`；路线 A 由客户端 `notifications/turn-ended`（自定义）或 `gcu turn-ended` CLI（Grok/Codex notify 钩子）或 **空闲 45s 推定** | Host |
| AppSession | Session 内对某 App 的工作上下文：授权、该 App 各窗口的 SnapshotStore、索引空间 | gcu-core |
| Snapshot | 一次观察的结果（渲染行、fingerprint 表、截图 id、几何） | SnapshotStore（每窗口保留最近 3 个，内存上限 32MB/会话） |

**turn_ended 时：** 清 turn 级授权、隐藏光标（30s 内无新动作也隐藏）、`release_all_input`、恢复本轮修改过的 `AXEnhancedUserInterface`、下一次观察强制全量。
**session 结束时：** 清 session 授权、停所有订阅、释放所有资源、停 worker、关闭 portal 并清对应 restore token；不清用户 Stop 锁存。

**并发：** 同一会话内工具调用串行执行（MCP 客户端可能并发发送，gcu-mcp 用每会话队列）；不同会话对**同一窗口**的变更动作互斥（窗口级锁，等待上限 5s → `busy`）。

---

## 9. 为什么 A 先行

1. **可测性：** `gcu mcp` 可被任意 MCP 客户端与我们自己的 eval runner 直接驱动；路线 B 需要启动整个 App + Grok CLI + ACP 才能测。
2. **解耦：** 先让 core/driver 在没有 App 的情况下完整工作，可强制保持依赖规则（core 不知道 Tauri 的存在）。
3. **用户价值更早：** Grok CLI 用户与其他 agent 用户可先用上；收集真实兼容性反馈。
4. **B 的工作量主要在 UI 与接线**，有了稳定 core 后 B 是 4–5 周的确定性工作，而不是与驱动调试交织。

风险：macOS helper 的签名/公证需要 Apple 开发者账号——由维护者提供（见 [04 §6](04-route-a-standalone-mcp.md#6-签名公证与分发)）。

---

## 10. 浏览器策略

| 层 | 何时 | 方式 | 阶段 |
|---|---|---|---|
| L0 无障碍 | 默认 | 浏览器也是 App：Chrome/Edge/Brave/Arc（开启 AXEnhancedUserInterface / UIA 自带）、Safari、Firefox（IA2/ATK）。网页区渲染为 `web area`，链接扁平为 markdown | P1–P3 |
| L1 内置浏览窗（仅路线 B） | 用户希望隔离、不碰自己的浏览器 | Grok App 新开一个 Tauri WebView 窗口（系统 WebView2/WKWebView/WebKitGTK，零额外运行时），它本身就是一个可被驱动操作的窗口；另加 `browser_navigate` 便捷工具 | P5 |
| L2 DOM 增强扩展 | 需要 DOM 级精度（隐藏元素、shadow DOM、大页面） | 精简 v1 MV3 扩展：native messaging 连 `gcu`；只在用户点击扩展图标"共享此标签页"后生效；提供更好的树（role/name/value 来自 DOM）与可靠的 `set_value` | P8 |

**明确不做：** 打包 Chromium/Playwright/Node；读取 cookie/存储；任意 `eval`。

---

## 11. 工具面（协议 v2）

> schema 以 `gcu-protocol` 的 Rust 类型 + `schemars` 生成；`computer-use/crates/gcu-protocol/golden/tools.json` 是黄金文件，CI 比对；
> **工具目录字节预算：默认面 ≤ 14,000 bytes**（`xtask budget` 测试）。

### 11.1 工具目录（16 个默认面 + 1 个条件注册，P0–P6 逐步上线）

| # | 工具 | 参数（`?` 可选） | 只读 | 阶段 |
|---|---|---|---|---|
| 1 | `list_apps` | — | ✅ | P0 |
| 2 | `list_windows` | `app?` | ✅ | P0 |
| 3 | `launch_app` | `app`, `reason?` | ❌ | P1 |
| 4 | `get_app_state` | `app`, `window?`, `include_screenshot?=true`, `text_limit?=500\|"max"`, `max_tree_nodes?=1200`, `reason?` | ✅* | P0 |
| 5 | `click` | `app`, `window?`, (`element_index` \| `x`,`y`,`screenshot_id?`), `click_count?=1`, `mouse_button?=left` | ❌ | P0 |
| 6 | `perform_secondary_action` | `app`, `window?`, `element_index`, `action` | ❌ | P0 |
| 7 | `set_value` | `app`, `window?`, `element_index`, `value` | ❌ | P0 |
| 8 | `select_text` | `app`, `window?`, `element_index`, (`text` \| `start`,`length`) | ❌ | P1 |
| 9 | `type_text` | `app`, `window?`, `text`, `element_index?`（先聚焦） | ❌ | P0 |
| 10 | `press_key` | `app`, `window?`, `key`（xdotool 语法，可组合） | ❌ | P0 |
| 11 | `scroll` | `app`, `window?`, (`element_index` \| `x`,`y`), `direction`, `pages?=1` | ❌ | P0 |
| 12 | `drag` | `app`, `window?`, `from_x`,`from_y`,`to_x`,`to_y`, `screenshot_id?` | ❌ | P1 |
| 13 | `wait_for` | `app`, `window?`, `until`: {`text_present`\|`text_absent`\|`element`(role,name)\|`stable`\|`window_title`}, `timeout_ms?=5000`(≤30000) | ✅ | P6 |
| 14 | `computer_batch` | `app`, `window?`, `steps[]`（上列动作 + `wait_for` + `expect`）, `stop_on_error?=true`, `final_state?=true` | ❌ | P6 |
| 15 | `computer_script` | `code`(≤20KB), `timeout_ms?=30000` | ❌ | P6（feature） |
| 16 | `capabilities` | — | ✅ | P0 |
| 17 | `browser_navigate` | `url` | ❌ | P5（仅路线 B 内置浏览窗打开时条件注册） |

\* `get_app_state` 只读但需 App 授权。

`minimal` profile 明确为 9 工具：`get_app_state, click, perform_secondary_action, set_value, type_text, press_key, scroll, list_apps, list_windows`；default 使用已上线的默认面，full 增加已启用 feature/条件工具，browser_navigate 始终受窗口打开条件限制。所有 profile 单独生成黄金文件并检查目录预算。

`app` 接受：显示名、bundle id / exe 名 / desktop id、绝对路径。歧义时返回 `ambiguous_app` + 候选列表。
`window` 接受：`list_windows` 返回的 `id`；省略即 key window。

### 11.2 错误码

`isError: true`，文本首行为 `error: <code>: <说明>`，结构化 `{error:{code,message,hint,retryable,state?}}`。

`app_not_found ambiguous_app app_denied approval_denied approval_timeout window_minimized window_not_found window_gone stale_element stale_geometry
stale_screenshot out_of_bounds element_not_actionable not_settable invalid_key key_blocked user_active stopped_by_user busy
handoff_required elevation_required permission_missing capability_unavailable timeout driver_error internal`

`key_blocked` 包含 §7.8 的地址栏/危险协议/元数据输入拦截；敏感动作被用户拒绝统一 `approval_denied`。协议 golden/errors.json 与 tools.json 需覆盖这些语义与条件工具（P0-03）。

带 `state?` 的错误（stale_*、window_gone、element_not_actionable）**附上新状态**，让模型零额外往返继续。

### 11.3 initialize instructions（服务端指令，≤1,200 字符，按平台生成）

要点：先 `get_app_state` 再操作；优先 `element_index`；坐标只用最新截图的像素；动作返回新状态无需再观察；`unknown` 不要盲目重复；
多步已知流程用 `computer_batch`；遇到 `handoff_required`/登录/验证码请用户处理；不能操作终端与 Grok 自身。

### 11.4 `computer_batch` 语义

```jsonc
{ "app": "TextEdit", "steps": [
    {"click": {"element_index": 5}},
    {"type_text": {"text": "季度总结"}},
    {"press_key": {"key": "cmd+s"}},
    {"wait_for": {"until": {"element": {"role": "sheet"}}, "timeout_ms": 3000}},
    {"expect": {"text_present": "存储为"}} ],
  "stop_on_error": true, "final_state": true }
```

- 预检：所有步骤参数先校验；任一非法则整批拒绝、零执行。
- 每步走与单工具完全相同的 ActionRouter（授权、门禁、敏感确认、守卫、账本）。
- 步骤间 index 解析：每步执行后自动刷新快照（diff 对齐保证未变节点 index 不变）；若步骤引用的 index 在刷新后 stale → 停止并报告是哪一步。
- 结果：每步 `{i, tool, outcome, effect, ms}` + 失败步详情 + 最终状态（全量或 diff）。
- 上限：32 步、总时长 60s。

### 11.5 `computer_script` 语义（feature `script`，P6）

QuickJS（rquickjs）沙箱运行在无文件/网络/进程权限的受限 worker，经窄 RPC 调 ActionRouter，不传原生句柄，不暴露 IO/环境/模块加载/计时器。`rquickjs = { version = "0.14", default-features = false, features = ["std"] }`；禁 rust-alloc/allocator/loader/dyn-load/full。
worker 启动及 OS 权限/资源限额由 gcu-cli 的可信 launcher 实施，gcu-script 仅承担运行时/绑定/窄 RPC，不为此引入平台 crate。
`runtime.set_memory_limit(32 * 1024 * 1024)`；30s 墙钟在 interrupt handler 用 Instant 判断并返回 true 中断；cu.* 绑定计数 ≤200 次并检查取消。宿主回调分配不计入 JS 堆：另限宿主缓冲 32MB、并发 native 调用 1、排队请求 32、总输出 2MB、worker 总内存 128MB、代码 ≤20KB；独立 watchdog 在超时/失控时 ≤500ms 终止 worker，Stop 不依赖 JS interrupt handler。

```js
const app = await cu.app("TextEdit");          // 触发授权（如需）
let s = await app.state({ screenshot: false }); // { text, elements[], focused }
const field = s.find({ role: "text_area" });    // 本地查询，不调用驱动
await app.setValue(field, "季度总结");
await app.press("cmd+s");
await app.waitFor({ element: { role: "sheet" } });
emit(await app.state());                       // 显式回传给模型（文本 + 可选截图）
return { ok: true };
```

- 每次调用都是全新上下文（不依赖内核持久——吸取 ventura 的教训）；需要跨调用的数据由模型通过参数传入。
- `emit()` 次数 ≤5，总输出受 token 预算约束；未 `emit` 时默认回传最后一次状态的 diff。

---

## 12. 配置

`gcu` 读取：`$XDG_CONFIG_HOME/grok-computer-use/config.toml`（macOS `~/Library/Application Support/grok-computer-use/`，Windows `%APPDATA%\grok-computer-use\`）。
路线 B 由 App 通过 `PolicyStore` 提供，不读该文件。

```toml
[approval]
always_allow_enabled = true
sensitive_action_confirmation = "standard"   # off | standard | strict
timeout_secs = 120

[delivery]
allow_foreground = "ask"                     # never | ask | always
macos_private_spi = true

[guard]
user_idle_ms = 3000
stop_hotkey = "default"

[observe]
max_tree_nodes = 1200
max_image_edge = 1568
image_format = "png"

[policy]
extra_denied_apps = []
```

---

## 13. 可观测性

- `tracing` 结构化日志，默认 INFO 只含元数据；文件滚动 5×5MB；路径在 doctor 中显示。
- 每次工具调用记录：`tool app outcome delivery effect duration_ms settle_ms tree_nodes image_bytes`——这些也是评测指标来源。
- `gcu doctor --json`：权限、能力、私有 API 可用性、版本、日志路径、最近 20 次调用摘要（无载荷）。
- 不做远程遥测（与 Windows-MCP 不同）；路线 B 若 App 有遥测，遵从 App 的开关，只发聚合计数。

---

## 14. 性能预算（P9 发布门禁）

具体发布门禁指标见 [07 §4](07-quality-gates.md#4-性能预算)。

| 操作 | macOS p50/p95 | Windows p50/p95 | Linux X11 p50/p95 |
|---|---|---|---|
| `get_app_state`（≤1200 节点，含截图） | 250 / 600 ms | 450 / 900 ms | 400 / 900 ms |
| 元素 `click` 返回（含 settle） | 300 / 900 ms | 400 / 1100 ms | 400 / 1100 ms |
| `type_text` 50 字符（含 settle） | 400 / 1200 ms | 600 / 1500 ms | 600 / 1500 ms |
| `list_apps` | 150 / 400 ms | 300 / 800 ms（缓存后） | 200 / 500 ms |
| 默认工具目录 | ≤14 KB | 同 | 同 |

---

## 15. Host 协议（路线 B 松耦合集成）

### 15.1 目标

- **App 与引擎解耦：** App 不编译 core/驱动；引擎崩溃不拖垮 App；引擎可单独升级、单独测试；路线 A 与 B 运行**同一个 `gcu` 二进制**。
- **保留集成优势：** 审批卡在聊天流、覆盖层由 App 绘制、任务面板有截图时间线、设置页统一管理、turn/session 生命周期精确、
  零安装随 App 更新、按会话类型（本地/SSH/远程 IM/定时任务）决定是否注入、App 内置浏览窗。

### 15.2 进程拓扑

```
Grok App（Tauri 主进程）
  ├─ HostServer（首选 App 创建并继承的匿名管道/socketpair；UDS / 本机命名管道为回退）
  └─ 托管子进程：gcu engine --host-fd <fd> --stop-fd <fd>（首选继承连接；回退 --host-socket <path> --host-token-fd <fd>）
         ├─ 连接 HostServer（握手：协议版本、引擎版本、能力）
         ├─ 驱动、core、MCP 全部在此进程内
         └─ 暴露 loopback MCP 端点（streamable HTTP 127.0.0.1:随机端口）；每个聊天会话一个 token（由 App 经 Host 协议申请）

Grok CLI（某聊天会话）──MCP──▶ 引擎的 loopback 端点（或 `gcu mcp --connect`，同一二进制的 stdio→HTTP 代理）
```

UID/文件权限只隔离其他用户，不证明调用者可信。路线 A macOS helper 通过连接 audit token 取对端代码签名，要求对端可执行文件与 helper 同一 cdhash/同一签名身份，其他客户端不得连接（P4-01）；签名校验不由 getpeereid 或 token 文件替代。同 UID 读写进程内存、修改受信二进制/可信 UI 的攻击不在此认证可保证的边界内。继承连接绑定预期子进程实例；否则用一次性秘密 FD + 双向挑战响应并核验预期进程实例。UDS 防路径替换，命名管道限本机并检测抢先创建；已有未知端点直接拒绝。Stop 使用独立控制通道，不与普通 RPC 排队。

- 一个 App 只有一个引擎进程，服务所有会话；空闲（无启用会话）60s 后 App 让它退出，下次需要时再拉起。
- 引擎异常退出：App 记录、在面板显示"Computer Use 引擎已重启"，进行中的调用返回 `unknown`；10 分钟内连续崩溃 3 次 → 自动关闭功能并提示。

### 15.3 消息（JSON-RPC 2.0，长度前缀帧；双向）

**引擎 → App（请求，App 必须应答）**

| 方法 | 对应 trait | 说明 |
|---|---|---|
| `host.approval.requestApp` | `ApprovalProvider::request_app` | 阻塞直到用户决定或超时；参数含 `session_id`、App 信息（名称、身份、图标 PNG 小图）、`reason`、`risk` |
| `host.approval.confirmAction` | `confirm_action` | 敏感动作确认，含目标区域裁剪图 |
| `host.approval.handoff` | `handoff` | 需要用户处理（登录/验证码/系统提示） |
| `host.policy.load` | `PolicyStore::load` | 返回策略配置（始终允许列表、确认级别、前台策略、热键） |

路线 A 的内置 Host 可在可信用户决定后调用 PolicyStore::grant_always；路线 B 的 RemoteHost 不发送该方法，App 在处理真实审批事件时自行持久化 Always（绑定请求/会话/App 身份/引擎实例/策略版本，单次消费），随后发送 `engine.policy.changed`（含单调策略版本），引擎重新 load 后更新缓存；不提供持久授权回写 RPC。

**引擎 → App（通知）**

| 方法 | 说明 |
|---|---|
| `host.overlay.show` / `host.overlay.status` / `host.overlay.hide` | App 的覆盖窗绘制光标/胶囊（屏幕坐标、DPI、目标窗口 frame） |
| `host.event.emit` | 动作事件（工具、目标、结果、耗时、缩略图引用）→ 面板时间线 |
| `host.engine.health` | 心跳与资源占用 |

**App → 引擎（请求/通知）**

| 方法 | 说明 |
|---|---|
| `engine.session.open {session_id, kind}` → `{mcp_url, token}` | App 提供其本地 session_id，为该会话申请独立 token/审批/快照命名空间；kind 非 local 时引擎拒绝 |
| `engine.session.turnEnded {session_id}` | ACP 一轮结束 |
| `engine.session.close {session_id}` | 会话关闭/fork/切模型 |
| `engine.session.stop {session_id?}` | 独立控制通道，用户点停止（覆盖层/面板/热键由引擎自身也会检测） |
| `engine.session.resume {session_id?}` | 仅可信用户控制通道的显式恢复，核验用户事件，不能来自模型工具或重放 |
| `engine.policy.changed` | 用户在设置页撤销授权或改级别 |
| `engine.system.doctor` → DoctorReport | 设置页显示权限/能力 |
| `engine.screenshot.get {session_id, id}` → bytes | 面板按需取脱敏大图，必须核验会话归属（事件里只带脱敏缩略图） |
| `engine.system.shutdown` | 正常退出 |

方法统一 `domain.subdomain.method`（末段 camelCase）。`engine.session.resume` 仅可信用户控制通道调用，不注册为 MCP 工具；重放/模型请求不得解除 Stop。

### 15.4 版本与兼容

- 握手交换 `host_protocol: {major, minor}`；major 不同 → 引擎拒绝连接，App 显示"引擎版本不匹配"；minor 向后兼容（新增方法/可选字段）。
- App 随包携带经过测试的 `gcu` 版本（Tauri `externalBin`）；正式模式验证引擎签名/构建身份及预期产品，不接受任意外部路径。开发者外部引擎使用独立 Dev App/helper 身份并单独授权，同时显示“使用外部引擎”；S-02 验证替换引擎不能继承正式 App 的 TCC。
- 协议黄金文件：`gcu-host-protocol/golden/*.json`，CI 比对；App 侧 TypeScript/Rust 类型由同一 schema 生成。

### 15.5 为什么不用"进程内链接"

| 维度 | 进程内链接 | Host 协议（采用） |
|---|---|---|
| 驱动崩溃/卡死（COM、AX、DWM 越界写） | 拖垮 App | 只重启引擎 |
| 编译耦合 | App 编译所有驱动，Tauri 升级与驱动依赖冲突 | App 只依赖协议 crate |
| A/B 一致性 | 两套组装方式，测试要跑两遍 | 同一二进制，同一测试矩阵 |
| 独立迭代 | 改驱动必须发 App | 可单独替换引擎（开发者设置） |
| 代价 | — | 一次本地 IPC（<1ms）；多一个进程的生命周期管理 |

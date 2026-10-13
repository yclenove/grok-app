# 06 · 路线图与任务卡

> 执行主文档。每张卡是一个 PR（个别标注"可拆"的卡可拆为 ≤3 个 PR）。卡的 DoD 全部满足才合入；进度只写 [STATUS.md](STATUS.md)。
> 估算单位 **d = 一个 agent 工作日（含测试与自审）**，仅用于排序，不是承诺。
> 所有卡通用 DoD（不再逐卡重复）：`cargo fmt --check`、`cargo clippy -D warnings`（computer-use workspace）、`cargo deny check`、
> 新增/修改代码的单元测试、无 `todo!()`/`unimplemented!()`/`unwrap()`（测试除外）、公开项有文档注释、PR 描述按 [08 §4](08-grok-execution-protocol.md#4-pr-规范) 填写。

## 总览

```
            P0 基础（2–3 周）
               │
     ┌─────────┼─────────┐
     ▼         ▼         ▼
 P1 macOS   P2 Windows  P3 Linux X11      （可并行；各 3–4 周）
     │         │         │
     └────┬────┴────┬────┘
          ▼         ▼
   P4 路线 A 产品化   P6 效率（batch/wait/script）
          │         │
          ▼         │
   P5 路线 B 集成 ◀──┘
          │
          ▼
   P7 Wayland · P8 浏览器 DOM 增强 · P9 加固与 1.0 · P10 长期
```

| 阶段 | 目标 | 退出条件（Exit Criteria） | 估算 |
|---|---|---|---|
| P0 | 工作区、协议、core（fake 驱动）、MCP、CLI、Host 协议、CI、关键事实 spike | 用 fake 驱动跑通全部 P0 工具的 MCP e2e；`gcu engine` 经 Host 协议完成审批往返；Grok Build 能力 ADR 已合入 | 14.5d |
| P1 | macOS 驱动 | 夹具 App 上 9 个核心工具契约测试全绿（自托管 Mac）；TextEdit/Finder/设置 冒烟脚本通过 | 18d |
| P2 | Windows 驱动 | 夹具 + Notepad/资源管理器/设置 契约测试全绿（Windows runner） | 20d |
| P3 | Linux X11 驱动 | 夹具 + gedit/文件管理器 契约测试全绿（Xvfb + AT-SPI CI） | 15d |
| P4 | 路线 A 产品化 | `gcu` 在三平台可安装、可接入 Grok/Codex/Claude；审批/覆盖层/onboarding 完整；A-β 发布 | 18d |
| P5 | 路线 B 集成 | App 内开箱可用，三件套完成；评测基线在 App 内跑通 | 22d |
| P6 | 效率 | batch/wait_for/script/auto 截图策略上线；评测 B 组相对 A 组往返数下降且成功率不降 | 14d |
| P7 | Wayland | GNOME/KDE/wlroots 能力表落地；AT-SPI 动作 + portal 截图/输入可用 | 18d |
| P8 | 浏览器 DOM 增强 | 扩展 + native messaging；网页任务评测成功率提升 | 12d |
| P9 | 加固与 1.0 | 性能预算、评测门槛、安全审查、兼容性矩阵、签名公证全部达标；A-1.0 与 App 正式版发布 | 15d |
| P10 | 长期演进 | 见 P10 列表，按季度排期 | — |

---

## P0 · 基础

### S-01 Grok Build CLI MCP 能力探测（spike）
- **依赖：** 无　**估算：** 1.5d　**产出：** ADR `computer-use/docs/adr/0001-grok-build-mcp-capabilities.md`（spike 代码不合入，放 `computer-use/spikes/` 且 PR 合入后删除）
- **目标：** 用真实 Grok Build CLI 确认 [04 §7](04-route-a-standalone-mcp.md#7-与-grok-build-cli-的配合) 表中 6 个问题，外加：ACP `newSession.mcpServers` 是否接受 `http` 传输。
- **步骤：** 写一个最小 stdio MCP（Rust，rmcp）：工具 `probe_image` 返回一张带随机文字的 PNG；`probe_elicit` 发 elicitation；记录 initialize 中客户端能力；
  在隔离 `GROK_HOME`（临时目录）下用 `grok -p "调用 probe_image 并读出图中的文字"`（隔离 home 中允许 `--always-approve`）验证模型能否读出文字；
  同法测 http 传输与 instructions。
- **DoD：** ADR 中每个问题有"是/否 + 证据（命令、输出摘录、CLI 版本）"；明确 v2 的默认选择；若图片不能抵达模型，ADR 中写出替代方案与对体验的影响。

### S-02 macOS TCC 与 helper 可行性（spike）
- **依赖：** 无　**估算：** 1.5d　**产出：** ADR 0002
- **目标：** 确认 (a) LSUIElement helper 由 CLI `open -b` 拉起后 TCC 归属 helper；(b) SCK `SCScreenshotManager` 在 helper 中可采被遮挡窗口；
  (c) `postToPid` + `CGEventSetWindowLocation`（dlsym）在 macOS 14/15/26 对 AppKit 与 Chromium 后台窗口的到达情况；(d) 路线 B 中由 Tauri App 以子进程启动的 `gcu engine` 的 TCC 是否归属 App（系统设置显示名称、授权一次是否对引擎生效）；若否，验证"bundle 内 helper + open/XPC 启动"备选。
- **DoD：** ADR 含版本矩阵与结论；(c) 不可用的版本列出回退路径。

### P0-01 建立 `computer-use/` workspace
- **依赖：** 无　**估算：** 1d
- **文件：** `computer-use/Cargo.toml`、`rust-toolchain.toml`（与 src-tauri 一致）、`deny.toml`、`.cargo/config.toml`、各 crate 空骨架 `lib.rs`、`xtask/`、`computer-use/README.md`
- **步骤：** workspace lints：`unsafe_op_in_unsafe_fn = deny`、`clippy::unwrap_used = deny`（非测试）、`missing_docs = warn`（公开 crate 为 deny）；
  `xtask check-deps` 实现 [03 §2](03-architecture.md#2-crate-划分与依赖规则) 依赖规则（解析 `cargo metadata`）。
- **测试：** `cargo xtask check-deps` 在故意违规的临时改动下失败（测试用例写在 xtask 的单测里，用伪 metadata）。
- **DoD：** `cargo build --workspace` 三平台通过；依赖规则可执行。

### P0-02 CI 骨架
- **依赖：** P0-01　**估算：** 1d
- **文件：** `.github/workflows/computer-use.yml`
- **内容：** 路径过滤 `computer-use/**`；矩阵 ubuntu-22.04 / windows-2022 / macos-14：fmt、clippy、test、deny、check-deps、golden、budget；
  所有 action pin 到 commit SHA；缓存 `~/.cargo` 与 `target`。自托管 e2e job 占位（`if: vars.CU_SELF_HOSTED == 'true'`）。
- **DoD：** 在 PR 上绿；故意引入 clippy 警告时红。

### P0-03 `gcu-protocol`：类型、错误码、能力、schema 黄金文件
- **依赖：** P0-01　**估算：** 2d
- **文件：** `crates/gcu-protocol/src/{lib,tools,result,error,capabilities,keys}.rs`、`golden/tools.json`、`golden/errors.json`
- **接口：** 每个工具一个参数 struct（`#[derive(Deserialize, JsonSchema)]`，`deny_unknown_fields`）；`ToolName` 枚举；`ErrorCode` 枚举（[03 §11.2](03-architecture.md#112-错误码)）；
  `ActionResult`（[03 §6](03-architecture.md#6-动作结果语义)）；`Capabilities`（[03 §5.4](03-architecture.md#54-能力声明capabilities-工具--doctor--initialize-instructions-共用)）；
  `KeyChord::parse(&str)`：xdotool 语法（修饰键 `ctrl alt shift super cmd meta fn`，命名键表含 `Return Tab Escape BackSpace Delete Home End Page_Up Page_Down Prior Next Up Down Left Right F1–F24 KP_0–KP_9 KP_Enter space` 等，单字符键）。
- **测试：** schema 生成与 `golden/tools.json` 字节一致（`xtask golden --check`；`--update` 重写）；`KeyChord` 表驱动测试 ≥60 例（含非法例）；
  错误码序列化稳定。
- **DoD：** 默认工具面（P0 已实现子集 + 预留）schema 总字节 ≤ 14,000（`xtask budget`）。

### P0-04 `gcu-host-api`
- **依赖：** P0-03　**估算：** 1d
- **接口：**
  ```rust
  #[async_trait] pub trait ApprovalProvider: Send + Sync {
      async fn request_app(&self, s: &SessionCtx, app: &AppInfo, reason: Option<&str>, risk: Risk) -> ApprovalDecision; // Once|Session|Always|Deny|Timeout
      async fn confirm_action(&self, s: &SessionCtx, app: &AppInfo, summary: &ActionSummary, crop: Option<ImageRef>) -> bool;
      async fn handoff(&self, s: &SessionCtx, app: &AppInfo, reason: HandoffReason);
  }
  pub trait OverlayProvider: Send + Sync { fn show_target(&self, s:&SessionCtx, win:&WindowInfo, at: ScreenPoint, kind: CursorKind); fn status(&self, s:&SessionCtx, text: StatusText); fn hide(&self, s:&SessionCtx); fn exclude_from_capture_supported(&self) -> bool; }
  pub trait EventSink: Send + Sync { fn emit(&self, s:&SessionCtx, ev: CuEvent); }
  pub trait PolicyStore: Send + Sync { fn load(&self) -> PolicyConfig; fn always_allowed(&self, id:&AppIdentity) -> bool; fn grant_always(&self, id:&AppIdentity); fn revoke(&self, id:&AppIdentity); }
  pub trait UserActivity: Send + Sync { fn ms_since_physical_input(&self) -> Option<u64>; fn stop_requested(&self) -> bool; fn clear_stop(&self); }
  ```
  另提供 `NoopOverlay`、`MemoryPolicyStore`、`ScriptedApprovals`（测试用，按队列返回决定）。
- **DoD：** 文档注释说明每个方法的阻塞/超时语义；测试实现可用于 core 测试。

### P0-05 `gcu-core`：模型、Driver trait、fake 驱动
- **依赖：** P0-03　**估算：** 2.5d（可拆）
- **文件：** `gcu-core/src/{model.rs, driver.rs}`；`gcu-driver-fake/src/{lib.rs, scene.rs, scenes/*.ron}`
- **接口：** [03 §3](03-architecture.md#3-统一节点模型gcu-coremodel)、[03 §5](03-architecture.md#5-driver-traitgcu-coredriver)。
- **fake 驱动：** 内存中的 App/窗口/节点树（从 RON 场景文件加载）；动作按规则改变树（press 按钮 → 计数+1；set_value → value；
  type_text → 焦点文本追加；scroll → 可见窗口移动；drag → 滑块值）；可注入故障（超时、崩溃 `maybe_executed`、窗口消失、几何变化、慢 settle）；
  截图为按树渲染的简单 PNG（每节点画框+文字），保证像素坐标与 frame 一致。
- **场景：** `form.ron`（表单+提交）、`list.ron`（2000 行列表）、`modal.ron`（弹窗压背景）、`chat.ron`（IM 发送）、`webarea.ron`（链接/文本）、`canvas.ron`（无树）。
- **测试：** 场景加载、每种动作效果、故障注入。
- **DoD：** fake 驱动实现 Driver 全部方法。

### P0-06 树管线与渲染
- **依赖：** P0-05　**估算：** 3d（可拆为管线/渲染两 PR）
- **文件：** `gcu-core/src/tree/{mod, transforms/*.rs, budget.rs, index.rs}`、`gcu-core/src/render/{text.rs, structured.rs}`
- **内容：** [03 §4.1](03-architecture.md#41-管线gcu-coretree顺序固定每步纯函数--单测) 13 步；[03 §4.2](03-architecture.md#42-文本渲染格式模型主读物) 格式。
- **测试：** 每个变换 ≥3 个输入/输出快照测试（`insta`）；6 个 fake 场景的完整渲染快照；预算：2000 行列表在 1200 预算下保留弹窗与焦点链；
  渲染耗时基准（`criterion`）：5000 节点 → 渲染 < 15ms（CI 记录不卡阈值，P9 卡）。
- **DoD：** 快照文件入库并人工审阅过（PR 中贴 2 个渲染样例）。

### P0-07 SnapshotStore、ElementIndex、Diff
- **依赖：** P0-06　**估算：** 2d
- **内容：** [03 §4.5](03-architecture.md#45-diff同一轮内的第二次起)；fingerprint 对齐；未变节点保留 index；diff 退回全量阈值；每窗口保留 3 个快照；会话内存上限。
- **测试：** 属性测试（`proptest`）：随机变异树 → diff 后应用到旧渲染可重建新渲染（语义等价）；index 永不复用；阈值行为。

### P0-08 Session、Policy、ActionRouter、ActionLedger
- **依赖：** P0-04、P0-07　**估算：** 3d（可拆）
- **内容：**
  - `Session`：turn/session 生命周期、per-session 串行队列、窗口级互斥锁。
  - `Policy`：拒绝名单（表驱动，含 [03 §7.4](03-architecture.md#74-拒绝名单内置不可通过始终允许覆盖企业用户只能追加) 全部条目）、授权三档、敏感动作检测（词表 15 语言、密码字段、组合键）、组合键黑名单、前台策略、用户活动守卫、Stop。
  - `ActionRouter`：参数 → 元素解析与 fingerprint 复核 → 门禁（几何/截图/越界）→ 敏感确认 → 选择投递路由（AX 优先 → 定向 → 前台需策略）→ 调驱动 → settle → diff → 结果组装。
  - `ActionLedger`：每次动作记录 `{call_id, tool, target, delivery, outcome, effect, ts}`（内存，会话内）；`unknown` 标记；供 batch 与 Host 事件。
- **测试：** 用 fake 驱动 + `ScriptedApprovals`：
  拒绝名单 12 例；授权三档 + turn_ended/session_closed 失效；敏感词触发（中/英/日各 3 例）；stale_element 返回新状态；stale_geometry；out_of_bounds；
  `maybe_executed` 超时 → `unknown` 且不重试；user_active 仅拦前台路径；Stop 热键 → `stopped_by_user` 并 `release_all_input` 被调用；并发调用串行化。
- **DoD：** 覆盖率（`cargo llvm-cov`）`gcu-core` ≥ 85% 行覆盖。

### P0-09 `gcu-mcp`：stdio 服务器
- **依赖：** P0-08　**估算：** 2d
- **内容：** 基于 `rmcp`；`initialize`（返回 instructions，[03 §11.3](03-architecture.md#113-initialize-instructions服务端指令1200-字符按平台生成)）、`tools/list`、`tools/call`、`notifications/turn-ended`（自定义）、`ping`；
  NDJSON 帧（Grok CLI 要求）与 Content-Length 帧自动识别；结果 = text + image + structuredContent；elicitation 客户端能力检测并接到 ApprovalProvider。
- **测试：** 进程级 e2e：spawn `gcu mcp --driver fake --scene form`，通过 JSON-RPC 跑完"观察→点击→输入→提交"并断言 fake 场景状态（外部判定）；帧格式两种；畸形输入不崩溃（fuzz 1 分钟，`cargo fuzz` 目标入库）。

### P0-10 `gcu-cli`：mcp / call / doctor / version（fake）
- **依赖：** P0-09　**估算：** 1d
- **内容：** clap 子命令；`--driver fake --scene <name>` 仅在 debug 构建或 `GCU_ALLOW_FAKE=1` 下可用。
- **测试：** `assert_cmd` 测退出码、`--json` 输出结构。

### P0-12 `gcu-host-protocol` 与 `gcu engine` 模式
- **依赖：** P0-09　**估算：** 2.5d
- **内容：** [03 §15](03-architecture.md#15-host-协议路线-b-松耦合集成)：消息类型、长度前缀帧编解码、握手与版本协商、`RemoteHost`（把 `gcu-host-api` 各 trait 实现为对 Host 的 RPC，带超时与断线处理）、
  `HostServer` 骨架（供 App 使用的服务端库：回调 trait + 连接管理）、`gcu engine --host-socket` 子命令（引擎经 RemoteHost 运行，并按 `engine.session.open` 暴露 loopback MCP）；
  UDS（unix）/ 命名管道（Windows）权限仅当前用户；一次性连接 token。黄金文件 `golden/*.json`。
- **测试：** 进程级 e2e：测试 Host（Rust）启动 `gcu engine --driver fake`，打开会话、经 MCP 调用触发 `host.approval.requestApp`、测试 Host 应答 session、动作事件到达、turnEnded 使 once 授权失效、
  Host 断开时引擎拒绝新调用并退出；版本 major 不一致握手失败。
- **DoD：** 协议文档（字段、超时、错误）完整；黄金文件入库。

### P0-11 文档与 STATUS 初始化
- **依赖：** P0-10　**估算：** 0.5d
- **内容：** `computer-use/README.md`（开发者向：结构、命令、如何跑测试）；`docs/plans/computer-use-v2/STATUS.md` 填入全部卡（状态 todo）。

**P0 退出评审：** 维护者在 PR 中运行 `gcu call get_app_state --args '{"app":"Fixture"}' --driver fake --scene chat` 并检查渲染可读性。

---

## P1 · macOS 驱动（`gcu-driver-macos`）

> 自托管 macOS runner（预授权 TCC 给测试 helper）跑契约测试；GitHub macos-14 只跑编译与非 TCC 单测。

### P1-01 夹具 App（跨平台，egui + AccessKit）
- **依赖：** P0-01　**估算：** 2.5d
- **文件：** `computer-use/apps/fixture/`
- **内容：** 一个窗口含：文本框、多行文本、密码框、复选框、单选组、下拉、滑块、按钮（计数器）、2000 行列表（虚拟化）、标签页、模态对话框（按钮触发）、
  可拖拽方块（拖到目标区）、画布区（无无障碍，只能看图点击）、中文/emoji 标签、"发送"按钮（敏感词测试）。
  **外部判定通道：** 每次状态变化写 JSON 到 `$GCU_FIXTURE_STATE`（原子 rename），记录事件来源（键盘/鼠标/无障碍动作）；测试通过读文件断言。
- **DoD：** 三平台都能构建运行；AccessKit 在三平台暴露树（手工用 Accessibility Inspector / Inspect.exe / Accerciser 截图入 PR）。

### P1-02 macOS：App/窗口发现与签名身份
- **依赖：** P0-05　**估算：** 2d
- **内容：** `list_apps`（运行中 + 14 天，Spotlight 元数据查询异步 + 缓存 60s）、`resolve_app`（名称/本地化名/bundle id/路径；歧义处理）、`launch_app`（`NSWorkspace.openApplication` 不激活：`activates=false`）、
  `list_windows`（`CGWindowListCopyWindowInfo` layer 0 + AX windows 对齐，得到 CGWindowID↔AXUIElement 映射）、`key_window`（无窗口时 unhide/`AXRaise` 不激活前台）、签名 team id。
- **测试：** 契约测试（自托管）：夹具出现在 list；resolve 歧义；launch 后窗口出现且前台 App 未变。

### P1-03 macOS：AX 树读取
- **依赖：** P1-02　**估算：** 3d
- **内容：** 批量属性读取；角色映射表（`roles.rs`，覆盖 AX 全部常见角色）；Chromium/Electron 开启 `AXEnhancedUserInterface`/`AXManualAccessibility`（记录原值，会话结束恢复）；
  `AXVisibleChildren`/`AXContents` 处理；文本控件 `AXValue`/`AXSelectedTextRange`/`AXNumberOfCharacters` 有界读取；frame 转窗口相对物理像素；
  遍历硬超时 2s + 节点上限 5000；专用线程；`AXObserver` 订阅。
- **测试：** 夹具树渲染快照（契约测试）；对 TextEdit、Finder、系统设置、Safari、Chrome（若装）做冒烟渲染并人工审阅（PR 贴图）。

### P1-04 macOS：ScreenCaptureKit 截图
- **依赖：** P1-02　**估算：** 2d
- **内容：** `SCShareableContent` 取窗口 → `SCContentFilter(desktopIndependentWindow:)` → `SCScreenshotManager.captureImage`；Retina 物理像素；缩放与编码在 core；
  权限缺失 → `permission_missing{screen_recording}`；被遮挡窗口可采（契约测试：夹具被另一窗口完全遮挡仍可截图且像素正确）。

### P1-05 macOS：语义动作
- **依赖：** P1-03　**估算：** 2.5d
- **内容：** Press（AXPress → AXConfirm → AXOpen → 选中行 `AXSelectedChildren` → 子孙可按元素；**不**用 AXRaise/Focus 冒充点击）、SecondaryAction、SetValue（仅 settable）、
  SelectText（`AXSelectedTextRange`）、ScrollElement（`AXScroll*ByPage` 优先，否则定向滚轮事件）。
- **测试：** 夹具每个控件的外部判定契约测试。

### P1-06 macOS：定向事件与 SkyLight 路径
- **依赖：** P1-04、S-02　**估算：** 3d
- **内容：** 指针点击/滚动/拖拽经 `postToPid` + window-local 位置（`CGEventSetWindowLocation` dlsym）；键盘 `postToPid`；SkyLight 路径（feature `macos-private-spi`）
  用于 Chromium 类窗口；运行时探测符号可用性；`PointerRoute::TargetedOnly` 失败不升级。InputLedger。
- **测试：** 夹具在后台（另一 App 前台）时点击/输入/拖拽到达且前台 App 不变；Chrome 被遮挡时点击计数器页面精确一次（50 次循环，契约测试 nightly）；
  记录 p50/p95 到 `computer-use/bench/macos.json`。

### P1-07 macOS：文本输入与剪贴板
- **依赖：** P1-06　**估算：** 1.5d
- **内容：** `type_text` 策略：可设置非富文本 → AXValue 拼接；否则 grapheme 分块定向 Unicode 事件；超长（>2000 字符）且允许时走剪贴板（保存/恢复 changeCount 比较）；
  焦点不是可编辑元素 → `element_not_actionable`（提示先点击输入框）。
- **测试：** 中文、emoji（含 ZWJ 序列）、组合字符、换行；Electron 富文本（VS Code 输入框）冒烟。

### P1-08 macOS：用户活动守卫、Stop 热键、前台路径
- **依赖：** P1-06、P0-08　**估算：** 1.5d
- **内容：** listen-only event tap（过滤自身注入事件：设置 `kCGEventSourceUserData` 标记）；缺"输入监控"时降级；全局热键（`RegisterEventHotKey` 或 event tap）；前台兜底路径（策略允许时）。
- **测试：** 模拟物理输入（自托管机用独立进程 `cliclick` 注入且不带标记）→ 前台动作被拒、后台动作不受影响。

### P1-09 macOS：契约测试套件与冒烟脚本
- **依赖：** P1-01…P1-08　**估算：** 1.5d
- **内容：** `computer-use/crates/gcu-driver-macos/tests/contract.rs`（通用契约测试宏来自 `gcu-core::testing::contract!`，三平台共用同一用例表）；
  `computer-use/scripts/smoke-macos.sh`（TextEdit 写字保存、Finder 侧栏导航、系统设置搜索框）。
- **DoD：** 自托管 runner 全绿；结果 JSON 存档为 CI artifact。

---

## P2 · Windows 驱动（`gcu-driver-windows`）

### P2-01 Windows：发现、身份、虚拟桌面
- **依赖：** P0-05　**估算：** 2d
- **内容：** `EnumWindows`（可见、非工具窗、owner 关系）、进程映像路径、Authenticode 发布者、开始菜单 `.lnk` 后台索引（首次异步，缓存到 `%LOCALAPPDATA%\grok-computer-use\apps.json`，文件监视更新）、
  UWP（`ApplicationFrameHost` 解包到真实进程）、`IVirtualDesktopManager`、HWND stamp（创建时间 + 类名 + pid，防 HWND 复用）。
- **测试：** 夹具、记事本、设置（UWP）的 list/resolve/launch。

### P2-02 Windows：UIA 树（CacheRequest）
- **依赖：** P2-01　**估算：** 3d
- **内容：** 专用 MTA 线程；`IUIAutomationCacheRequest` 一次取全部属性与 pattern 可用性（TreeScope_Subtree 分批）；角色映射；
  Chromium/Electron 的 UIA 树（Chrome 默认暴露）；`IsPassword`；超时 2s / 5000 节点；`IUIAutomationEventHandler`（结构变化、属性变化、焦点）订阅。
  **移植来源：** `archive/cu-v1-2026-10-13:src-tauri/src/computer_use/windows_uia*.rs`（只取经验证的属性与 pattern 用法）。
- **测试：** 夹具快照；记事本、资源管理器、设置冒烟渲染。

### P2-03 Windows：WGC 截图 + PrintWindow 回退
- **依赖：** P2-01　**估算：** 2d
- **内容：** `GraphicsCaptureItem::CreateForWindow` → 单帧；Win11 `IsBorderRequired=false`、`IsCursorCaptureEnabled=false`；DPI（`GetDpiForWindow`）与物理像素；
  最小化窗口 → `window_minimized`（不自动还原，除非前台策略允许）；回退 `PrintWindow(PW_RENDERFULLCONTENT)`。
- **测试：** 被遮挡窗口截图正确；高 DPI（150%）坐标对齐（夹具画网格，点击网格点 → 夹具报告命中格）。

### P2-04 Windows：worker 隔离执行模式
- **依赖：** P2-02、P2-03　**估算：** 2d
- **内容：** `gcu worker`：长度前缀 bincode 帧；主进程只做编排；worker 执行 UIA/截图；崩溃/超时（10s）/坏帧 → 重启并对**只读**操作重试一次，对**变更**操作返回 `unknown`；
  `ExecMode` 配置；句柄跨进程以 runtime-id（UIA RuntimeId）+ HWND 表示。
- **测试：** 注入崩溃（测试 feature 下 worker 收到特定请求 abort）→ 只读重试成功、变更 unknown；浸泡测试（nightly）：窗口拖动中连续观察 300 次无主进程崩溃。

### P2-05 Windows：语义动作（UIA Patterns）
- **依赖：** P2-02　**估算：** 2.5d
- **内容：** Invoke / Toggle / SelectionItem.Select / ExpandCollapse / Value.SetValue / RangeValue / ScrollPattern / TextPattern 选区 / LegacyIAccessible.DoDefaultAction；
  按控件类型选择 pattern 的优先级表（`patterns.rs`）。
- **测试：** 夹具全控件外部判定。

### P2-06 Windows：定向消息与前台输入
- **依赖：** P2-05　**估算：** 3d
- **内容：** 命中测试（`ChildWindowFromPointEx`/UIA ElementFromPoint）→ `PostMessage` 鼠标/键盘到子 HWND；effect 检测（settle diff）；
  Chromium/UWP 检测到 PostMessage 无效（类名 `Chrome_RenderWidgetHostHWND`、`Windows.UI.Core.CoreWindow`）→ 直接走前台路由（需策略）；
  前台：激活序列 + `SendInput`（Unicode 文本分块、组合键、滚轮、拖拽插值）；InputLedger；UIPI/完整性检测 → `elevation_required`。
- **测试：** 夹具后台消息点击；记事本输入中文；前台路径在策略 never 时被拒。

### P2-07 Windows：用户活动守卫、Stop 热键、覆盖层排除截图
- **依赖：** P2-06　**估算：** 1.5d
- **内容：** LL 钩子（过滤 injected 标志）+ 周期替换；`RegisterHotKey`；`SetWindowDisplayAffinity` 能力检测（Win10 2004+）。
- **测试：** 模拟物理输入（测试程序用 `SendInput` 无法模拟"物理"——改为钩子单元测试注入假事件 + 手工验收清单入 PR）。

### P2-08 Windows：剪贴板与文本
- **依赖：** P2-06　**估算：** 1d
- **内容：** 移植 v1 剪贴板保存/恢复（`GetClipboardSequenceNumber` 比较）；>8192 UTF-16 分批或剪贴板。

### P2-09 Windows：契约测试与冒烟
- **依赖：** P2-01…P2-08　**估算：** 1.5d
- **内容：** 同 P1-09 的通用契约用例表；`scripts/smoke-windows.ps1`（记事本写字保存、资源管理器导航、设置搜索）。
- **DoD：** windows-2022 runner（交互桌面会话）全绿。

---

## P3 · Linux X11 驱动（`gcu-driver-linux`）

### P3-01 AT-SPI 连接与树
- **依赖：** P0-05　**估算：** 3d
- **内容：** `atspi` crate（上游 zbus）；`org.a11y.Bus` 获取地址；批量 `GetChildren`/`GetAttributes`/`GetState`/`GetRole`/`Component.GetExtents(WINDOW)`；角色映射；
  GTK3/GTK4/Qt5/Qt6/Electron（`--force-renderer-accessibility` 提示）差异处理；超时。
- **测试：** Xvfb + `at-spi-bus-launcher` + 夹具 + gedit（或 gnome-text-editor）渲染快照。

### P3-02 X11 窗口与截图
- **依赖：** P3-01　**估算：** 2d
- **内容：** x11rb：`_NET_CLIENT_LIST`、`_NET_WM_PID`、`_NET_WM_NAME`、`_NET_ACTIVE_WINDOW`；AT-SPI frame ↔ X 窗口对齐（pid + 标题 + 几何）；
  截图 `XComposite` 命名 pixmap + `GetImage`（被遮挡可采）；无 composite 时 `GetImage` 根窗口裁剪并声明能力。

### P3-03 AT-SPI 动作 + XTEST
- **依赖：** P3-02　**估算：** 2.5d
- **内容：** Action（`do_action` 按名匹配 click/press/activate）、EditableText.SetTextContents/InsertText、Value.SetCurrentValue、Selection、Text 选区；
  XTEST 指针/键盘（全局 → 前台路由，需策略，前先 `_NET_ACTIVE_WINDOW` 激活）；keysym 映射（xkbcommon）。
- **测试：** 夹具外部判定。

### P3-04 用户活动守卫与热键（X11）
- **依赖：** P3-03　**估算：** 1d
- **内容：** XInput2 原始事件（排除 XTEST 设备）；`XGrabKey` 热键。

### P3-05 契约测试与 CI
- **依赖：** P3-01…P3-04　**估算：** 1.5d
- **内容：** CI job：`xvfb-run` + dbus + at-spi + openbox（窗口管理器，提供 EWMH）；通用契约用例表；`scripts/smoke-linux.sh`。

---

## P4 · 路线 A 产品化

### P4-01 macOS helper app 与代理
- **依赖：** P1-09、S-02　**估算：** 3d
- **内容：** [04 §3](04-route-a-standalone-mcp.md#3-macos-helper-与-tcc)；`gcu agent` 子命令；socket 鉴权；版本握手；空闲退出；`xtask bundle-macos` 生成 `.app`（Info.plist、图标、LSUIElement）。
- **测试：** e2e：从终端运行 `gcu mcp`，TCC 只授权 helper 时工具可用；旧版本 helper 被替换。

### P4-02 Onboarding 窗口（macOS）
- **依赖：** P4-01　**估算：** 2d
- **内容：** wry WebView；共享 HTML/CSS（`computer-use/ui/`，与路线 B 不共享 React——独立轻量页面，但视觉 token 对齐 App）；15 语言（i18n JSON 从 App 的 `computer-use` 命名空间导出脚本生成，单一来源）；权限轮询；重启 helper。

### P4-03 ApprovalProvider：elicitation + 审批窗（三平台）
- **依赖：** P0-09、P4-01　**估算：** 3d
- **内容：** [04 §4](04-route-a-standalone-mcp.md#4-审批approvalprovider-实现)；审批窗 HTML 与 onboarding 共用组件；键盘可达；超时；`approvals.json`；`gcu approvals` 子命令。
- **测试：** elicitation 路径用 MCP 测试客户端；审批窗用 `GCU_APPROVAL_TEST_AUTOCLICK`（仅测试构建）验证四种决定。

### P4-04 gcu-overlay（三平台）
- **依赖：** P1-06、P2-06、P3-03　**估算：** 3.5d（可拆三平台）
- **内容：** [04 §5](04-route-a-standalone-mcp.md#5-覆盖层gcu-overlay)；排除截图；光标弹簧动画（60fps，参数可配）；胶囊 + 停止按钮（点击走 `UserActivity::stop`）。
- **测试：** 截图中不出现覆盖层（契约测试：开启覆盖层后截图与关闭时像素哈希一致，X11 除外）。

### P4-05 `gcu install/uninstall`（全客户端）
- **依赖：** P0-10　**估算：** 2d
- **内容：** [04 §2.1](04-route-a-standalone-mcp.md#21-gcu-install-client-写入的配置)；TOML/JSON 无损编辑（`toml_edit`、保留注释）；备份；`--dry-run`；`~/.grok` 写入需确认。
- **测试：** 每个客户端：空文件、已有其他条目、已有 gcu 条目（幂等）、uninstall 精确移除——快照测试。

### P4-06 turn-ended 接入
- **依赖：** P0-09、S-01　**估算：** 1d
- **内容：** `gcu turn-ended`（经 socket/命名管道通知运行中的 `gcu mcp` 会话）；Codex notify 链式包装；空闲推定。

### P4-07 打包与分发
- **依赖：** P4-01…P4-06　**估算：** 3d
- **内容：** `release-computer-use.yml`：三平台四架构构建、macOS 签名公证（有 secret 时）、Windows 签名（有 secret 时）、SHA256SUMS + cosign、SBOM、
  npm launcher + 平台包、Homebrew formula、winget manifest、`.deb/.rpm`（cargo-deb / cargo-generate-rpm）。tag 规则 `cu-v<semver>`。
- **DoD：** 在干净虚拟机（三平台）按 README 安装并接入 Grok CLI 跑通冒烟（录屏/截图入 release PR）。

### P4-08 用户文档
- **依赖：** P4-07　**估算：** 1d
- **内容：** `computer-use/README.md`（用户向）+ `README_ZH.md`：安装、权限、接入各客户端、授权说明、平台差异表、隐私、故障排除、卸载。

---

## P5 · 路线 B 集成（Grok App）

> 所有前端卡：交互逻辑 + 视觉一致 + 功能对等 三件套；15 语言；不改 App.tsx/AppWorkbench.tsx 状态。

### P5-01 ComputerUseHost 骨架 + 引擎托管
- **依赖：** P0-12、P1-09（mac 先行；Windows/Linux 引擎构建合入后自动启用）　**估算：** 2.5d
- **内容：** `src-tauri/Cargo.toml` **仅** path 依赖 `gcu-protocol`、`gcu-host-protocol`；`tauri.conf.json` `externalBin` 加入 `gcu`（构建脚本从 computer-use workspace 产出各平台二进制）；
  `computer_use_v2/{mod,supervisor,host_server}.rs`；按 `computer_use_enabled` 启停；空闲退出；崩溃重启与熔断；开发者设置"外部引擎路径"；`xtask check-deps` 扩展到 src-tauri。
- **测试：** Rust 单测：启停幂等、握手失败处理、熔断；集成测试：用 `gcu engine --driver fake` 跑一次完整审批往返（无 UI，测试回调自动应答）。

### P5-02 Gateway + 注入
- **依赖：** P5-01、S-01　**估算：** 2d
- **内容：** [05 §1](05-route-b-integrated.md#1-进程与数据流)、inject.rs；`engine.session.open` 取端点与 token（引擎侧生成，每会话随机 32 字节，会话结束作废，校验 Host/Origin）；仅本地会话注入。
- **测试：** 未带 token / 错 token / 跨会话 token 被拒；SSH 会话不注入（与现有 session 类型判断共用函数）。

### P5-03 审批卡（App 授权）
- **依赖：** P5-02　**估算：** 3d
- **内容：** approval.rs + `ComputerUseApprovalCard.tsx` + 事件/命令；挂起审批与会话绑定；倒计时；键盘操作（Tab/Enter/Esc=拒绝）；屏幕阅读器标签。
- **测试：** Vitest 组件测试（四种决定、超时、会话切换回来仍在）；Rust：会话关闭自动拒绝。

### P5-04 敏感动作确认卡 + 交接卡
- **依赖：** P5-03　**估算：** 2d

### P5-05 工具调用卡（聊天流渲染）
- **依赖：** P5-02　**估算：** 2.5d
- **内容：** 在现有工具调用渲染管线中识别 `computer-use` server 的调用：显示动作图标、目标（App · 元素名）、结果徽标（done/refused/unknown + effect）、截图缩略图（点击放大，复用现有图片查看器）、耗时；折叠显示原始文本。

### P5-06 Computer 侧栏面板
- **依赖：** P5-05　**估算：** 2.5d
- **内容：** 替换 SideWorkbench 的 Computer 入口为 `ComputerUsePanel`；时间线、当前状态、停止；无任务时的空状态（说明 + 去设置）。

### P5-07 覆盖窗（App 内）
- **依赖：** P5-01　**估算：** 3d
- **内容：** overlay.rs + `cu-overlay` Tauri 窗口（透明、置顶、不可激活、鼠标穿透除胶囊外）；排除截图；多显示器定位；光标动画。

### P5-08 设置页
- **依赖：** P5-03　**估算：** 2.5d
- **内容：** `settings/ComputerUseSection.tsx`；settingsCatalog 登记与关键词；系统权限状态（`cu_doctor`）与去授权；始终允许列表（App 图标、名称、授予时间、撤销）；敏感确认与前台策略 `Select`；热键展示；"清除内置浏览窗数据"（P5-10 后）。
- **测试：** 设置搜索命中；深链；撤销后下一次调用重新弹卡。

### P5-09 turn/session 生命周期接线 + wiki 重写
- **依赖：** P5-02　**估算：** 1.5d
- **内容：** ACP 流结束 → `turn_ended`；会话关闭/fork/切模型 → `session_closed`；`docs/llm-wiki/computer-use.md` 重写（≤200 行）并在 `AGENTS.md` Read first 增加链接；`docs/llm-wiki/settings-ia.md` 登记。

### P5-10 内置浏览窗（L1）
- **依赖：** P5-07　**估算：** 3d
- **内容：** [05 §5](05-route-b-integrated.md#5-内置浏览窗l1p5-可选交付)；`browser_navigate` 动态注册；拒绝名单对 `cu-browser` 窗口放行。

### P5-11 App 内评测与发布开关
- **依赖：** P5-01…P5-09、P9-03 的评测 runner（可先用 P6 前的基础版）　**估算：** 1.5d
- **内容：** 在 App 构建上跑评测基础任务集（见 07 §5）；设置项从"实验"转为正式需满足 07 §8 门槛；CHANGELOG 条目（≤90 字符首句）。

---

## P6 · 效率

### P6-01 settle 引擎与事件订阅调优
- **依赖：** P1-09、P2-09、P3-05　**估算：** 2d
- **内容：** 统一 settle 策略（事件静默/树哈希/预算）；每平台默认参数基准化；`settle` 字段输出。
- **DoD：** bench 显示三平台元素点击返回 p50 不劣于 P1/P2/P3 基线，且 `no_visible_change` 误报率（夹具上真实有变化却报无变化）< 2%。

### P6-02 auto 截图策略
- **依赖：** P6-01　**估算：** 1.5d
- **内容：** [03 §4.4](03-architecture.md#44-截图) 规则 + 树质量评估（可交互节点数、弹窗、canvas 角色占比，参考 bo-516 `_auto_thin` 的思路但阈值由评测确定）；
  `get_app_state` 的 `include_screenshot` 默认值改为 `"auto"`（评测证明不降成功率后）。

### P6-03 `wait_for`
- **依赖：** P6-01　**估算：** 1.5d

### P6-04 `computer_batch`
- **依赖：** P6-03　**估算：** 3d
- **内容：** [03 §11.4](03-architecture.md#114-computer_batch-语义)；预检；逐步结果；expect。
- **测试：** fake + 夹具：全成功、中途 stale、中途敏感确认被拒、超时、预检失败零执行。

### P6-05 `computer_script`（feature）
- **依赖：** P6-04　**估算：** 4d
- **内容：** [03 §11.5](03-architecture.md#115-computer_script-语义feature-scriptp6)；rquickjs；资源限制；取消；`cu` API 类型声明文件（`cu.d.ts`，放入 instructions 链接/skill）。
- **测试：** 沙箱逃逸用例（访问 `globalThis` 未暴露对象、无限循环、内存炸弹、超长输出）；功能用例同 batch。
- **DoD：** 安全审查清单（07 §7）签字。

### P6-06 工具目录与 instructions 优化
- **依赖：** P6-05　**估算：** 1d
- **内容：** 按评测结果精简描述；profile：`minimal`（9 核心）/`default`/`full`。

### P6-07 效率评测（A/B）
- **依赖：** P6-01…P6-06、P9-01　**估算：** 2d
- **内容：** 07 §5.4 的 A 组（单步工具、无 diff、总截图）vs B 组（diff + auto 截图 + batch + script）；报告入 `computer-use/eval/reports/`。
- **DoD：** B 组成功率 ≥ A 组，模型往返中位数下降 ≥30%，输入 token 下降 ≥30%；否则回滚默认值并写原因。

---

## P7 · Wayland

| 卡 | 内容 | 估算 |
|---|---|---|
| P7-01 | Wayland 检测与能力表（GNOME/KDE/wlroots/其他），doctor 输出 | 1d |
| P7-02 | portal ScreenCast 单帧截图 + restore token 持久化（ashpd + pipewire）；**移植** v1 wayland crate 中的帧/会话身份检查 | 4d |
| P7-03 | portal RemoteDesktop + libei（reis）指针/键盘；坐标映射到流 | 4d |
| P7-04 | 窗口身份：GNOME Shell 扩展（移植 v1 `tools/computer-use-gnome-shell`，精简为只读窗口列表与激活 D-Bus 接口）、KWin 脚本、wlroots `foreign-toplevel` | 4d |
| P7-05 | AT-SPI 动作在 Wayland 上的契约测试（GNOME headless：`mutter --headless --virtual-monitor` + portal-gnome，自托管） | 3d |
| P7-06 | 用户文档：各桌面环境能力表与首次授权说明 | 1d |

DoD：不声称"Wayland 全支持"；能力表与 doctor 输出一致；GNOME 上夹具契约测试（AT-SPI 动作 + portal 截图）全绿。

---

## P8 · 浏览器 DOM 增强（L2）

| 卡 | 内容 | 估算 |
|---|---|---|
| P8-01 | 精简 v1 MV3 扩展：只保留"共享此标签页"、DOM → 统一节点模型序列化（role/name/value/states/frame）、set_value/click/scroll/select；删除 v1 配对/租约/HTTP | 4d |
| P8-02 | Native messaging host（`gcu native-host`），三平台 manifest 安装由 `gcu install chrome-extension` 完成；扩展 ID 固定（key） | 2d |
| P8-03 | core 中"浏览器窗口 + 共享标签页"时，用 DOM 树替换 web area 子树（frame 对齐），动作路由优先 DOM | 3d |
| P8-04 | Chrome Web Store / Edge Add-ons 上架材料（隐私说明：不读 cookie/存储，只在共享后读取当前标签页） | 1d |
| P8-05 | 网页任务评测（07 §5 web 类）L0 vs L2 对比 | 2d |

---

## P9 · 加固与 1.0

| 卡 | 内容 | 估算 |
|---|---|---|
| P9-01 | 评测 runner 完整版（07 §5）：任务 YAML、判定器、Grok CLI 驱动（隔离 GROK_HOME）、指标、HTML 报告；nightly | 4d |
| P9-02 | 任务集扩充到 ≥60 个（三平台 × 六类），每个含 oracle 自证与 red-team 变体 | 4d |
| P9-03 | 性能预算门禁（03 §14）：bench 结果超预算 CI 红 | 1.5d |
| P9-04 | 安全审查：拒绝名单、敏感词、脚本沙箱、gateway 鉴权、日志脱敏、截图打码（密码区域）；外部审阅者签字 | 3d |
| P9-05 | App 兼容性矩阵（07 §6）：≥30 个常用 App × 三平台，记录 Stable/Limited/Best-effort 与已知问题 | 3d |
| P9-06 | 长稳：4 小时浸泡（循环任务集）无内存增长 >50MB、无 worker 泄漏、无卡键 | 2d |
| P9-07 | 发布：A-1.0 + Grok App 正式开放（设置项去"实验"标签），CHANGELOG、README、wiki | 1.5d |

---

## P10 · 长期演进（按季度排期，每项开工前写 ADR）

| 项 | 说明 | 价值 | 风险 |
|---|---|---|---|
| OCR 读取 | `read_text{region}`：macOS Vision、Windows.Media.Ocr、Linux Tesseract（可选依赖）——自绘 UI/canvas | 高（微信/飞书/游戏化 UI） | Linux 依赖 |
| 视觉 grounding | 对无树窗口提供 SoM 标号截图（本地生成框与编号）；不默认调用外部模型 | 中 | 准确率 |
| agent_display（macOS） | 虚拟显示器停放窗口，被遮挡/其他 Space 的窗口完整可用，不打扰用户 | 高 | 私有 API |
| 锁屏使用（macOS） | 参考 Codex locked use：活动轮次内临时使用 | 中 | 安全设计复杂，需单独威胁建模 |
| CU 子 agent | 可选：主 agent 把 GUI 子任务交给隔离上下文的子 agent（截图不污染主上下文），返回 ≤2KB 报告 | 中 | 往返增加；需评测 |
| 企业策略 | MDM/组策略下发：禁"始终允许"、App 白名单、强制 strict 确认、审计日志导出 | 中 | — |
| 远程桌面 | 通过 SSH 隧道操作远程主机（`gcu mcp` 在远端运行，本地 App 只做 UI）；与 [ssh-remote.md](../../llm-wiki/ssh-remote.md) 的后续 wave 对齐 | 中 | 授权 UI 跨机 |
| 录制与回放 | 把一次成功的任务录为 batch 脚本，用户可一键重放（仍走授权与确认） | 中 | 脆弱性 |
| Cua 驱动对照适配器 | `gcu-driver-cua`（可选 feature），用于评测 C 组与特定平台补洞 | 低-中 | 依赖外部版本 |
| 多显示器/混合 DPI 全覆盖 | 跨屏拖拽、窗口跨屏 | 中 | — |
| 无障碍改进回馈 | 对暴露树差的常用 App 提交上游 issue/PR（Electron flags 等） | 长期 | — |

---

## 依赖关系速查

```
S-01 ─┬─▶ P0-09 ─▶ P4-03, P4-06, P5-02
S-02 ─┴─▶ P1-06, P4-01
P0-01 ▶ P0-02, P0-03 ▶ P0-04, P0-05 ▶ P0-06 ▶ P0-07 ▶ P0-08 ▶ P0-09 ▶ P0-10 ▶ P0-11；P0-09 ▶ P0-12 ▶ P5-01
P0-05 ▶ P1-02 ▶ {P1-03, P1-04} ▶ P1-05/P1-06 ▶ P1-07, P1-08 ▶ P1-09
P0-05 ▶ P2-01 ▶ {P2-02, P2-03} ▶ P2-04, P2-05 ▶ P2-06 ▶ P2-07, P2-08 ▶ P2-09
P0-05 ▶ P3-01 ▶ P3-02 ▶ P3-03 ▶ P3-04 ▶ P3-05
P1-01（夹具）可与 P0-05 之后任意时间并行，被 P1-09/P2-09/P3-05 需要
```

# 06 · 路线图与任务卡

> 执行主文档。每张卡是一个 PR（个别标注"可拆"的卡可拆为 ≤3 个 PR）。卡的 DoD 全部满足才合入；进度只写 [STATUS.md](STATUS.md)。
> 估算单位 **d = 一个 agent 工作日（含测试与自审）**，仅用于排序，不是承诺。
> 所有实现卡通用 DoD（不再逐卡重复）：本机 `cargo fmt --all -- --check`；远程 `rcargo clippy -p <本卡 crate> --all-targets -- -D warnings`、`rcargo --sh 'cargo deny check'`、
> 新增/修改代码的单元测试、无 `todo!()`/`unimplemented!()`/`unwrap()`（测试除外）、公开项有文档注释、PR 描述按 [08 §4](08-grok-execution-protocol.md#4-pr-规范) 填写。

所有源码/测试落点均限制生产文件≤600行、测试文件≤1000行；超出拆模块。Rust 命令从 computer-use/ 执行，前端和文档命令从仓库根执行。Mac/Windows 原生测试命令仅在对应平台 CI/runner 运行，agent 在本机仍只用 rcargo 跑 build8 可运行的平台无关检查；未实跑原生测试记 not_run。卡内验收命令为最低必跑范围，不替代通用 DoD；远程编译见 08 §3.1，真实平台行为由匹配平台 runner 执行。纯文档卡只运行文档自查，以下实现卡命令是未来验收入口。

## 总览

```
            P0 基础（4.3 周）＋ spike（0.6 周，单列）
               │
     ┌─────────┼─────────┐
     ▼         ▼         ▼
 P1 macOS   P2 Windows  P3 Linux X11      （可并行；3.9 / 3.7 / 2.0 周）
     │         │         │
     └────┬────┴────┬────┘
          ▼         ▼
   P4 路线 A（3.7 周） P6 效率（3.0 周）
          │         │
          ▼         │
   P5 路线 B（5.2 周）◀──┘
          │
          ▼
   P7 Wayland（3.4 周）· P8 DOM（2.4 周）· P9 加固（3.8 周）· P10 长期
```

| 阶段 | 目标 | 退出条件（Exit Criteria） | 估算 |
|---|---|---|---|
| P0 | 工作区、协议、core（fake 驱动）、MCP、CLI、Host 协议、CI（spike 单列） | 用 fake 驱动跑通全部 P0 工具的 MCP e2e；`gcu engine` 经 Host 协议完成审批往返；Grok Build 能力 ADR 已合入 | 21.5d |
| P1 | macOS 驱动 | 夹具 App 上 minimal 9 工具契约测试全绿（自托管 Mac）；TextEdit/Finder/设置 冒烟脚本通过 | 19.5d |
| P2 | Windows 驱动 | 夹具 + Notepad/资源管理器/设置 契约测试全绿（Windows runner） | 18.5d |
| P3 | Linux X11 驱动 | 夹具 + gedit/文件管理器 契约测试全绿（Xvfb + AT-SPI CI） | 10d |
| P4 | 路线 A 产品化 | `gcu` 在三平台可安装、可接入 Grok/Codex/Claude；审批/覆盖层/onboarding 完整；A-β 发布 | 18.5d |
| P5 | 路线 B 集成 | App 内开箱可用，三件套完成；评测基线在 App 内跑通 | 26d |
| P6 | 效率 | batch/wait_for/script/auto 截图策略上线；评测 B 组相对 A 组往返数下降且成功率不降 | 15d |
| P7 | Wayland | GNOME/KDE/wlroots 能力表落地；AT-SPI 动作 + portal 截图/输入可用 | 17d |
| P8 | 浏览器 DOM 增强 | 扩展 + native messaging；网页任务评测成功率提升 | 12d |
| P9 | 加固与 1.0 | 性能预算、评测门槛、安全审查、兼容性矩阵、签名公证全部达标；A-1.0 与 App 正式版发布 | 19d |
| P10 | 长期演进 | 见 P10 列表，按季度排期 | — |
| Spike | S-01、S-02 | 两份事实 ADR | 3d |

逐卡重算：P0–P9 共 **177.0d**，spike 单列 **3d**，总 **180.0d**；周数以 5d/周换算，仅表示串行工作量，非实际日历工期。P0-08 的原 3d 拆为 P0-08a 策略数据 1d + P0-08 集成 2d，不重复计费。P4 为 18.5d（不是旧总览 18d），因此不沿用评审给出的总和。完整依赖以各卡字段为准，流程图是工作量总览；P9-01 可在 P0-10 后提前到 P4 期间执行，供早期发布评测复用。

---

## P0 · 基础

### S-01 Grok Build CLI MCP 能力探测（spike）
- **依赖：** —　**估算：** 1.5d　**产出：** ADR `computer-use/docs/adr/0001-grok-build-mcp-capabilities.md`（spike 代码不合入，放 `computer-use/spikes/` 且 PR 合入后删除）
- **文件：** `computer-use/docs/adr/0001-grok-build-mcp-capabilities.md`、`computer-use/spikes/mcp-probe/{Cargo.toml,src/main.rs,tests/probe.rs}`（package gcu-mcp-probe；不合入探测代码）
- **目标：** 用真实 Grok Build CLI 确认 [04 §7](04-route-a-standalone-mcp.md#7-与-grok-build-cli-的配合) 表中 6 个问题，外加：ACP `newSession.mcpServers` 是否接受 `http` 传输。
- **步骤：** 写一个最小 stdio MCP（Rust，rmcp）：工具 `probe_image` 返回一张带随机文字的 PNG；`probe_elicit` 发 elicitation；记录 initialize 中客户端能力；
  在隔离 `GROK_HOME`（临时目录）下用 `grok -p "调用 probe_image 并读出图中的文字"`（仅一次性 OS/桌面、无用户目录/真实凭据，关闭无关 shell/文件/网络工具后允许 --always-approve）验证模型能否读出文字；
  同法测 http 传输与 instructions。
- **DoD：** ADR 中每个问题有"是/否 + 证据（命令、输出摘录、CLI 版本）"；明确 v2 的默认选择；若图片不能抵达模型，ADR 中写出替代方案与对体验的影响。
- **测试：** 图片随机文字可被模型读出、elicitation四档、turn钩子、instructions、structuredContent、大小上限、ACP HTTP/继承FD；支持与不支持均在ADR记录CLI版本/证据。
  验收命令：`rcargo --sh 'cargo test --manifest-path spikes/mcp-probe/Cargo.toml -p gcu-mcp-probe'`；探测程序按卡中 grok 命令在一次性桌面运行，不在 build8 常驻。

### S-02 macOS TCC 与 helper 可行性（spike）
- **依赖：** —　**估算：** 1.5d　**产出：** ADR 0002
- **文件：** `computer-use/docs/adr/0002-macos-tcc-helper.md`、`computer-use/spikes/tcc-probe/{Cargo.toml,src/main.rs,tests/probe.rs}`（package gcu-tcc-probe；不合入探测代码）
- **目标：** 确认 (a) LSUIElement helper 由 CLI `open -b` 拉起后 TCC 归属 helper；(b) SCK `SCScreenshotManager` 在 helper 中可采被遮挡窗口；
  (c) `postToPid` + `CGEventSetWindowLocation`（dlsym）在 macOS 14/15/26 对 AppKit 与 Chromium 后台窗口的到达情况；(d) 路线 B 中由 Tauri App 以子进程启动的 `gcu engine` 的 TCC 是否归属 App（系统设置显示名称、授权一次是否对引擎生效）；若否，验证“bundle 内 helper + open/XPC 启动”备选；(e) 替换/未签名/外部引擎能否继承正式 App TCC，正式模式签名/构建身份核验与独立 Dev 授权是否有效；(f) 私有 API 在 TCC 撤销、错误 PID/窗口、安全提示出现时拒绝且不升级。
- **DoD：** ADR 含版本矩阵与结论；(c) 不可用版本列回退；(e)/(f) 逐项实测，冲突停下来 needs-decision，不用外部引擎绕过。
- **测试：** Mac14/15/26 helper TCC归属/被遮挡SCK/后台投递、App父进程TCC/外部引擎替换/Dev权限隔离，以及撤销权限/错误PID/安全提示拒绝矩阵。
  验收命令：`cargo test --manifest-path spikes/tcc-probe/Cargo.toml -p gcu-tcc-probe`；真实 TCC/SPI 测试仅在 Mac runner 按 ADR 矩阵执行。

### P0-01 建立 `computer-use/` workspace
- **依赖：** —　**估算：** 1d
- **文件：** `computer-use/Cargo.toml`、`rust-toolchain.toml`（与 src-tauri 一致）、`deny.toml`、`.cargo/config.toml`、各 crate 空骨架 `lib.rs`、`xtask/`、`computer-use/README.md`
- **步骤：** workspace lints：`unsafe_op_in_unsafe_fn = deny`、`clippy::unwrap_used = deny`（非测试）、`missing_docs = warn`（公开 crate 为 deny）；
  `xtask check-deps` 实现 [03 §2](03-architecture.md#2-crate-划分与依赖规则) 依赖规则（解析 `cargo metadata`）。
- **测试：** `cargo xtask check-deps` 在故意违规的临时改动下失败（测试用例写在 xtask 的单测里，用伪 metadata）。
  验收命令：`rcargo test -p xtask`；`rcargo run -q -p xtask -- check-deps`。

- **DoD：** 各 crate 以 `cargo build -p <crate>` 串行在对应三平台 CI 构建通过；依赖规则可执行。
- **DoD 补充：** deny.toml 注明系统动态库不受 cargo-deny 判定（WebKitGTK/GTK/libpipewire/libxkbcommon），发行说明单列。RULES 扩展由 P0-05 承接，不以本次文档修改宣称代码已同步。

### P0-02 CI 骨架
- **依赖：** P0-01　**估算：** 1d
- **文件：** `.github/workflows/computer-use.yml`
- **内容：** 路径过滤 `computer-use/**`；矩阵 ubuntu-22.04 / windows-2022 / macos-14：fmt、clippy、test、deny、check-deps、golden、budget；
  所有 action pin 到 commit SHA；缓存 `~/.cargo` 与 `target`。自托管 e2e job 占位（`if: vars.CU_SELF_HOSTED == 'true'`）。
- **DoD：** 在 PR 上绿；故意引入 clippy 警告时红。
- **测试：** CI路径过滤触发、故意clippy警告导致失败、伪metadata违规导致失败，三平台纯数据测试绿；fuzz仅Unix nightly；自托管未配置标not_run。
  验收命令：`rcargo clippy -p gcu-protocol --all-targets -- -D warnings`；`rcargo test -p xtask`；CI 故障注入验证红绿。

### P0-03 `gcu-protocol`：类型、错误码、能力、schema 黄金文件
- **依赖：** P0-01　**估算：** 2d
- **文件：** `computer-use/crates/gcu-protocol/src/{lib,tools,result,error,capabilities,keys,approval}.rs`、`computer-use/crates/gcu-protocol/golden/{tools.json,errors.json,profiles/*.json}`
- **接口：** 每个工具一个参数 struct（`#[derive(Deserialize, JsonSchema)]`，`deny_unknown_fields`）；`ToolName` 枚举；`ErrorCode` 枚举（[03 §11.2](03-architecture.md#112-错误码)）；
  `ActionResult`（[03 §6](03-architecture.md#6-动作结果语义)）；`Capabilities`（[03 §5.4](03-architecture.md#54-能力声明capabilities-工具--doctor--initialize-instructions-共用)）；
  `KeyChord::parse(&str)`：xdotool 语法（修饰键 `ctrl alt shift super cmd meta fn`，命名键表含 `Return Tab Escape BackSpace Delete Home End Page_Up Page_Down Prior Next Up Down Left Right F1–F24 KP_0–KP_9 KP_Enter space` 等，单字符键）。
- **测试：** schema 生成与 `golden/tools.json` 字节一致（`xtask golden --check`；`--update` 重写）；`KeyChord` 表驱动测试 ≥60 例（含非法例）；
  错误码序列化稳定；golden/errors.json 明确 key_blocked 对自身危险 scheme/地址栏/元数据输入拦截，approval_denied 对敏感拒绝，window_minimized 对最小化；tools.json 含 browser_navigate 条件注册与 minimal 9 工具 profile；ApprovalDecision 仅 Turn/Session/Always/Deny。
  验收命令：`rcargo test -p gcu-protocol`；`rcargo run -q -p xtask -- golden --check`；`rcargo run -q -p xtask -- budget`。

- **DoD：** 默认工具面（P0 已实现子集 + 预留）schema 总字节 ≤ 14,000（`xtask budget`）。

### P0-04 `gcu-host-api`
- **依赖：** P0-03　**估算：** 1d
- **文件：** `computer-use/crates/gcu-host-api/src/{lib.rs,approval.rs,overlay.rs,policy.rs,activity.rs,testing.rs}`、`tests/providers.rs`
- **接口：**
  ```rust
  #[async_trait] pub trait ApprovalProvider: Send + Sync {
      async fn request_app(&self, s: &SessionCtx, app: &AppInfo, reason: Option<&str>, risk: Risk) -> ApprovalDecision; // Turn|Session|Always|Deny；超时独立 approval_timeout，不新增档位
      async fn confirm_action(&self, s: &SessionCtx, app: &AppInfo, summary: &ActionSummary, crop: Option<ImageRef>) -> bool;
      async fn handoff(&self, s: &SessionCtx, app: &AppInfo, reason: HandoffReason);
  }
  pub trait OverlayProvider: Send + Sync { fn show_target(&self, s:&SessionCtx, win:&WindowInfo, at: ScreenPoint, kind: CursorKind); fn status(&self, s:&SessionCtx, text: StatusText); fn hide(&self, s:&SessionCtx); fn exclude_from_capture_supported(&self) -> bool; }
  pub trait EventSink: Send + Sync { fn emit(&self, s:&SessionCtx, ev: CuEvent); }
  pub trait PolicyStore: Send + Sync { fn load(&self) -> PolicyConfig; fn always_allowed(&self, id:&AppIdentity) -> bool; fn grant_always(&self, id:&AppIdentity); fn revoke(&self, id:&AppIdentity); }
  pub trait UserActivity: Send + Sync { fn ms_since_physical_input(&self) -> Option<u64>; fn stop_requested(&self) -> bool; fn clear_stop(&self); // 仅可信用户控制调用，模型/轮次结束不得调用 }
  ```
  另提供 `NoopOverlay`、`MemoryPolicyStore`、`ScriptedApprovals`（测试用，按队列返回决定）。
- **DoD：** 文档注释说明每个方法的阻塞/超时语义；测试实现可用于 core 测试。
- **测试：** ScriptedApprovals 四档与 timeout/取消、MemoryPolicyStore、NoopOverlay；UserActivity 仅用户可 clear_stop。
  验收命令：`rcargo test -p gcu-host-api --test providers`。

- **接口说明：** request_app 超时由调用方 deadline 产生 approval_timeout；ApprovalDecision 四档不含 Timeout。grant_always 仅路线 A 内置受信 Host 调用；路线 B App 真实审批事件持久化，无远程回写方法。

### P0-05 `gcu-core`：模型、Driver trait、fake 驱动
- **依赖：** P0-03、P0-04　**估算：** 2.5d（可拆）
- **文件：** `computer-use/crates/gcu-core/src/{model.rs, driver.rs}`；`computer-use/crates/gcu-driver-fake/src/{lib.rs, scene.rs, scenes/*.ron}`；`computer-use/xtask/src/deps.rs`（RULES与伪metadata测试）
- **接口：** [03 §3](03-architecture.md#3-统一节点模型gcu-coremodel)、[03 §5](03-architecture.md#5-driver-traitgcu-coredriver)。
- **fake 驱动：** 内存中的 App/窗口/节点树（从 RON 场景文件加载）；动作按规则改变树（press 按钮 → 计数+1；set_value → value；
  type_text → 焦点文本追加；scroll → 可见窗口移动；drag → 滑块值）；可注入故障（超时、崩溃 `maybe_executed`、窗口消失、几何变化、慢 settle）；
  截图为按树渲染的简单 PNG（每节点画框+文字），保证像素坐标与 frame 一致。
- **场景：** `form.ron`（表单+提交）、`list.ron`（2000 行列表）、`modal.ron`（弹窗压背景）、`chat.ron`（IM 发送）、`webarea.ron`（链接/文本）、`canvas.ron`（无树）。
- **测试：** 场景加载、每种动作效果、故障注入。
  验收命令：`rcargo test -p gcu-driver-fake`；`rcargo test -p gcu-core`；`rcargo test -p xtask`；`rcargo run -q -p xtask -- check-deps`。

- **DoD：** fake 驱动实现 Driver 全部方法；同步 computer-use/xtask/src/deps.rs 的 RULES 与 03 §2 完整白名单（core 数据依赖、script、host-protocol serde_json 等），伪 metadata 允许/拒绝测试均通过。

### P0-06 树管线与渲染
- **依赖：** P0-05　**估算：** 3d（可拆为管线/渲染两 PR）
- **文件：** `computer-use/crates/gcu-core/src/tree/{mod, transforms/*.rs, budget.rs, index.rs}`、`computer-use/crates/gcu-core/src/render/{text.rs, structured.rs}`
- **内容：** [03 §4.1](03-architecture.md#41-管线gcu-coretree顺序固定每步纯函数--单测) 13 步；[03 §4.2](03-architecture.md#42-文本渲染格式模型主读物) 格式。
- **测试：** 每个变换 ≥3 个输入/输出快照测试（`insta`）；6 个 fake 场景的完整渲染快照；预算：2000 行列表在 1200 预算下保留弹窗与焦点链；
  渲染耗时基准（`criterion`）：5000 节点 → 渲染 < 15ms（CI 记录不卡阈值，P9 卡）。
  验收命令：`rcargo test -p gcu-core tree`；`rcargo test -p gcu-core render`。

- **DoD：** 快照文件入库并人工审阅过（PR 中贴 2 个渲染样例）。

### P0-07 SnapshotStore、ElementIndex、Diff
- **依赖：** P0-06　**估算：** 2d
- **文件：** `computer-use/crates/gcu-core/src/snapshot/{mod.rs,store.rs,index.rs,diff.rs}`、`src/redact/{mod.rs,regions.rs,images.rs}`、`tests/{snapshot.rs,redact.rs}`
- **内容：** 首次截图交付必经 redact 管线：原始树先收敏感区域再裁剪、状态版本一致后打码，模型图/缩略图/审批 crop/CLI 临时图只消费脱敏结果；登录/安全场景无法定位则不交付截图并 handoff。[03 §4.5](03-architecture.md#45-diff同一轮内的第二次起)；fingerprint 对齐；未变节点保留 index；diff 退回全量阈值；每窗口保留 3 个快照；会话内存上限。
- **测试：** 属性测试（`proptest`）：随机变异树 → diff 后应用到旧渲染可重建新渲染（语义等价）；index 永不复用；阈值行为；密码节点被 budget/prune 移除、截图/树不同步、缺失敏感区域时不泄露，衍生物/日志扫描。
  验收命令：`rcargo test -p gcu-core --test snapshot`；`rcargo test -p gcu-core --test redact`。

- **DoD：** A-α 前首次出图即脱敏，快照记录目标区域 32×32 像素哈希供坐标重验。

### P0-08 Session、Policy、ActionRouter、ActionLedger
- **依赖：** P0-04、P0-07、P0-08a　**估算：** 2d（可拆，原 3d 中策略数据 1d 已单列）
- **文件：** `computer-use/crates/gcu-core/src/{session/{mod.rs,queue.rs,stop.rs},policy/{mod.rs,risk.rs,identity.rs,approval.rs},action/{router.rs,ledger.rs,gate.rs}}`、`tests/{policy.rs,router.rs,stop.rs}`、`computer-use/crates/gcu-driver-fake/src/scenes/{ide,web_terminal,submit_unknown}.ron`
- **内容：**
  - `Session`：turn/session 生命周期、per-session 串行队列、窗口级互斥锁。
  - `Policy`：拒绝名单（名称仅提示，核验实际映像/签名产品/已知哈希并解析别名/启动包装器；未知执行宿主 handoff；表驱动，含 [03 §7.4](03-architecture.md#74-拒绝名单内置不可通过始终允许覆盖企业用户只能追加) 全部条目）、授权三档、敏感动作检测（词表 15 语言、密码字段、组合键）、组合键黑名单、前台策略、用户活动守卫、Stop。
  - `ActionRouter`：参数 → 元素解析与 fingerprint 复核 → 门禁（几何/截图/越界）→ 敏感确认 → 批准后投递前重验 App/进程实例/窗口/参数/snapshot_id+fingerprint/状态与策略版本及区域像素哈希（变化则 stale+新状态/重审）→ 选择投递路由（AX 优先 → 定向 → 前台需策略）→ 调驱动 → settle → diff → 结果组装。
  - `ActionLedger`：每次动作记录 `{call_id, tool, target, delivery, outcome, effect, ts}`（内存，会话内）；`unknown` 标记；供 batch 与 Host 事件。
- **测试：** 用 fake 驱动 + `ScriptedApprovals`：
  拒绝名单 12 例；授权三档 + turn_ended/session_closed 失效；敏感词触发（中/英/日各 3 例）；stale_element 返回新状态；stale_geometry；out_of_bounds；
  `maybe_executed` 超时 → `unknown` 且不重试；user_active 仅拦前台路径；Stop 热键 → `stopped_by_user` 并 `release_all_input` 被调用；并发调用串行化；无文字图标/伪装名称/Enter/Space/自定义快捷键/含换行 type_text 在浏览器、web area、未知目标中按高风险，web area 默认 strict；IDE 终端/REPL/任务/调试入口的坐标/菜单/secondary/拖拽/batch/script 均拒绝，无法判断 handoff；Web SSH/云终端/远程桌面等 fake red_team 场景覆盖。
  焦点为地址栏（角色/名称启发式）的自己发出的文本输入/提交，或任意目标危险 scheme/元数据输入，均 key_blocked；同几何替换内容返回 stale_screenshot；审批等待时改变收件人/内容/进程/策略作废。Stop 后跨轮、排队调用、重连、重启仍锁存，可信用户恢复才解除；独立停止通道 ≤100ms 锁存、≤500ms 发释放、原生卡死 ≤5s 超时/杀 worker。
  验收命令：`rcargo test -p gcu-core --test policy`；`rcargo test -p gcu-core --test router`；`rcargo test -p gcu-core --test stop`；`rcargo llvm-cov -p gcu-core --fail-under-lines 85`。

- **DoD：** 覆盖率（`cargo llvm-cov`）`gcu-core` ≥ 85% 行覆盖。

### P0-08a 策略数据（P0-08 新增子卡）
- **依赖：** P0-01　**估算：** 1d
- **文件：** `computer-use/crates/gcu-core/src/policy/{data.rs,denylist.rs,sensitive_terms/*.txt}`、`tests/policy_data.rs`
- **目标：** 已完成的策略数据子卡，提供拒绝类别/多语言敏感词/组合键规则的纯数据与解析验证，不宣称完成 P0-08 Router 门禁。
- **测试：** 规则格式、15 语言条目、拒绝类别与键表、不允许空规则或重复冲突项；与 03 §7.4/§7.5 一致。
  验收命令：`rcargo test -p gcu-core --test policy_data`。

- **DoD：** 数据与单测闭环；分支 `cu2/P0-08a-policy-data` 已完成待合入，STATUS 保持 in_progress；P0-08 消费该数据后另验全集成安全用例。

### P0-09 `gcu-mcp`：stdio 服务器
- **依赖：** P0-08　**估算：** 2d
- **文件：** `computer-use/crates/gcu-mcp/src/{lib.rs,server.rs,stdio.rs,elicitation.rs}`、`tests/mcp_e2e.rs`、`computer-use/fuzz/fuzz_targets/mcp_codec.rs`
- **内容：** 基于 `rmcp`；`initialize`（返回 instructions，[03 §11.3](03-architecture.md#113-initialize-instructions服务端指令1200-字符按平台生成)）、`tools/list`、`tools/call`、`notifications/turn-ended`（自定义）、`ping`；
  rmcp 3.5（default-features=false，server/macros/transport-io/transport-streamable-http-server/elicitation）；stdio 仅承诺 NDJSON，S-01 若发现需要 Content-Length 则自写 codec 并独立验收，不自动识别；结果 = text + image + structuredContent；elicitation 客户端能力检测，在 tools/call 内调用 peer.create_elicitation 并接 ApprovalProvider，正规 JSON Schema decision string enum turn/session/always/deny。
- **测试：** 进程级 e2e：spawn `gcu mcp --driver fake --scene form`，通过 JSON-RPC 跑完"观察→点击→输入→提交"并断言 fake 场景状态（外部判定）；NDJSON 与必要时自写 codec 测试；畸形输入不崩溃，fuzz 目标入库，仅 ubuntu-22.04/macos-14 nightly 跑 1 分钟，不在 Windows 跑 cargo fuzz。
  验收命令：`rcargo test -p gcu-mcp --test mcp_e2e`；Unix nightly另执行 cargo +nightly fuzz run mcp_codec -- -max_total_time=60。

- **DoD：** 本卡内容与测试全部通过，能力/协议/错误语义与03及07相关门禁一致；不注册空实现。

### P0-10 `gcu-cli`：mcp / call / doctor / version（fake）
- **依赖：** P0-09　**估算：** 1d
- **文件：** `computer-use/crates/gcu-cli/src/{main.rs,args.rs,call.rs,doctor.rs,exit.rs,resume.rs}`、`tests/cli.rs`
- **内容：** clap 子命令，包含 gcu resume（仅可信用户CLI，模型不注册恢复工具），与独立停止控制通道/Host锁存连接；`--driver fake --scene <name>` 仅在 debug 构建或 `GCU_ALLOW_FAKE=1` 下可用。
- **测试：** assert_cmd 测退出码映射、--json 结构、可信用户 resume 能解除锁存/模型工具不可调用/未认证连接不可恢复。
  验收命令：`rcargo test -p gcu-cli --test cli`。

- **DoD：** 本卡内容与测试全部通过，能力/协议/错误语义与03及07相关门禁一致；不注册空实现。

### P0-12 `gcu-host-protocol` 与 `gcu engine` 模式
- **依赖：** P0-09　**估算：** 2.5d
- **文件：** `computer-use/crates/gcu-host-protocol/src/{lib.rs,messages.rs,codec.rs,auth.rs,server.rs}`、`golden/*.json`、`tests/{codec.rs,auth.rs}`；`computer-use/crates/gcu-mcp/src/host/{remote.rs,session.rs,stop.rs}`、`computer-use/crates/gcu-cli/src/engine.rs`、`tests/engine_host.rs`
- **内容：** [03 §15](03-architecture.md#15-host-协议路线-b-松耦合集成)：消息类型、长度前缀帧编解码、握手与版本协商、`RemoteHost`（gcu-mcp 中把 gcu-host-api 各 trait 适配为对 Host 的 RPC，host-protocol 本身不依赖 core/host-api，带超时与断线处理）、
  `HostServer` 骨架（供 App 使用的服务端库：回调 trait + 连接管理）、`gcu engine --host-socket` 子命令（引擎经 RemoteHost 运行，并按 `engine.session.open` 暴露 loopback MCP）；
  UDS（unix）/ 命名管道（Windows）权限仅当前用户；一次性连接 token。黄金文件 `golden/*.json`。
- **测试：** 进程级 e2e：测试 Host（Rust）启动 `gcu engine --driver fake`，打开会话、经 MCP 调用触发 `host.approval.requestApp`、测试 Host 应答 session、动作事件到达、turnEnded 使 turn 授权失效、
  Host 断开时引擎拒绝新调用并退出；版本 major 不一致握手失败。
  验收命令：`rcargo test -p gcu-host-protocol`；`rcargo test -p gcu-cli --test engine_host`。

- **DoD：** 协议文档（字段、超时、错误）完整；黄金文件入库；Host 双向认证优先继承连接，否则秘密 FD + 挑战响应核验预期进程实例；UDS 防路径替换、命名管道仅本机/防抢先创建，未知端点拒绝。同 UID 冒充/假 Host/重放/断线测试拒绝。无持久授权回写 RPC，App 真实审批单次消费后自行存 Always 并发 engine.policy.changed；无审批/跨身份/重放不能授权。所有方法 domain.subdomain.method；独立 Stop 控制通道不受审批/动作阻塞。

### P0-11 文档与 STATUS 初始化
- **依赖：** P0-10、P0-12　**估算：** 0.5d
- **文件：** `computer-use/README.md`、`docs/plans/computer-use-v2/STATUS.md`
- **内容：** `computer-use/README.md`（开发者向：结构、命令、如何跑测试）；`docs/plans/computer-use-v2/STATUS.md` 逐卡维护，保留既有状态/PR；P0-11 为 P0-12 后的文档收尾，不重编号。

- **测试：** 开发者README命令与实际CLI一致；STATUS逐卡依赖/工时/既有状态不丢，相对链接/锚点与卡字段自查通过。

  验收命令：`rcargo run -q -p xtask -- check-deps`；`python3 docs/plans/computer-use-v2/reviews/check_plan.py`。

- **DoD：** 本卡内容与测试全部通过，能力/协议/错误语义与03及07相关门禁一致；不注册空实现。

**P0 退出评审：** 维护者在 PR 中运行 `gcu call get_app_state --args '{"app":"Fixture"}' --driver fake --scene chat` 并检查渲染可读性。

---

## P1 · macOS 驱动（`gcu-driver-macos`）

> 自托管 macOS runner（预授权 TCC 给测试 helper）跑契约测试；GitHub macos-14 只跑编译与非 TCC 单测。

### P1-01 夹具 App（跨平台，egui + AccessKit）
- **依赖：** P0-01　**估算：** 2.5d
- **文件：** `computer-use/apps/fixture/`
- **内容：** 依赖 `eframe="0.36"`，默认 accesskit，不另加 accesskit 0.25；一个窗口含：文本框、多行文本、密码框、复选框、单选组、下拉、滑块、按钮（计数器）、2000 行列表（虚拟化）、标签页、模态对话框（按钮触发）、
  可拖拽方块（拖到目标区）、画布区（无无障碍，只能看图点击）、中文/emoji 标签、"发送"按钮（敏感词测试）。
  **外部判定通道：** 每次状态变化写 JSON 到 `$GCU_FIXTURE_STATE`（原子 rename），记录事件来源（键盘/鼠标/无障碍动作）；测试通过读文件断言。
- **DoD：** 三平台构建运行并暴露树（Accessibility Inspector / Inspect.exe / Accerciser 证据入 PR）；密码框为密码/受保护文本；虚拟列表只要求可见行+滚动范围；拖拽源/目标为可操作角色，模态可识别。不满足则改 fixtures-native，不把契约测试适配为残缺树也不直接加不兼容 accesskit。
- **测试：** 表单输入/密码受保护/可见虚拟行与滚动范围/模态/拖拽角色，状态JSON原子写入且counter恰好一次；三平台无障碍检查。
  验收命令：`rcargo test -p gcu-fixture`；Mac/Windows/Linux runner 核验真实无障碍树。

### P1-02 macOS：App/窗口发现与签名身份
- **依赖：** P0-05　**估算：** 2d
- **文件：** `computer-use/crates/gcu-driver-macos/src/{apps.rs,windows.rs,identity.rs,spotlight.rs,ffi.rs}`、`tests/{identity.rs,discovery.rs}`
- **内容：** `list_apps`（运行中 + 14 天，Spotlight 元数据查询异步 + 缓存 60s）、`resolve_app`（名称/本地化名/bundle id/路径；歧义处理）、`launch_app`（`NSWorkspace.openApplication` 不激活：`activates=false`）、
  `list_windows`（`CGWindowListCopyWindowInfo` layer 0 + AX windows 对齐，得到 CGWindowID↔AXUIElement 映射）、`key_window`（无窗口时 unhide/`AXRaise` 不激活前台）、SecStaticCode::from_path（security-framework 3.7），team id/bundle id 用自有 extern C SecCodeCopySigningInformation；Spotlight 用 objc2-core-services MDQuery。
- **测试：** 契约测试（自托管）：夹具出现在 list；resolve 歧义；launch 后窗口出现且前台 App 未变；已签名/未签名 App、签名有效性、路径替换、同名证书、证书/产品/构建更新重审，插件/可写加载路径不把入口签名等同全进程可信。
  验收命令：`cargo test -p gcu-driver-macos --test identity`；`cargo test -p gcu-driver-macos --test discovery`。

- **DoD：** 本卡内容与测试全部通过，能力/协议/错误语义与03及07相关门禁一致；不注册空实现。

### P1-03 macOS：AX 树读取
- **依赖：** P1-02　**估算：** 3d
- **文件：** `computer-use/crates/gcu-driver-macos/src/ax/{mod.rs,tree.rs,roles.rs,events.rs,sensitive.rs}`、`tests/ax_tree.rs`
- **内容：** 批量属性读取；角色映射表（`roles.rs`，覆盖 AX 全部常见角色）；Chromium/Electron 开启 `AXEnhancedUserInterface`/`AXManualAccessibility`（记录原值，会话结束恢复）；
  `AXVisibleChildren`/`AXContents` 处理；文本控件 `AXValue`/`AXSelectedTextRange`/`AXNumberOfCharacters` 有界读取；frame 转窗口相对物理像素；
  遍历硬超时 2s + 节点上限 5000；专用线程；AXObserver 订阅；仅 accessibility-sys 0.2，不引入 accessibility。
- **测试：** 夹具树渲染快照（契约测试）；对 TextEdit、Finder、系统设置、Safari、Chrome（若装）做冒烟渲染并人工审阅（PR 贴图）。
  验收命令：`cargo test -p gcu-driver-macos --test ax_tree`。

- **DoD：** 角色/名称启发式识别 terminal/console、IDE 终端/REPL/任务与调试入口及 Web SSH 等混合区；驱动给出原生角色/名称信号，通用 Router 所有路径拒绝，未知 handoff；夹具回归从首次 IDE 放行起通过，标注尽力而为、非完备。

### P1-04 macOS：ScreenCaptureKit 截图
- **依赖：** P1-02、P1-03、P0-07　**估算：** 2d
- **文件：** `computer-use/crates/gcu-driver-macos/src/capture/{mod.rs,sck.rs}`、`tests/capture.rs`
- **内容：** `SCShareableContent` 取窗口 → `SCContentFilter(desktopIndependentWindow:)` → 异步 `SCScreenshotManager.captureImageWithFilter_configuration_completionHandler`（配置+completion 取 CGImage，失败映射驱动错误）；Retina 物理像素；缩放与编码在 core；
  权限缺失 → `permission_missing{screen_recording}`；被遮挡窗口可采（契约测试：夹具被另一窗口完全遮挡仍可截图且像素正确）。
- **测试：** 被遮挡/Retina/回调失败/取消/权限撤销；原始树先收敏感区域，密码区域被裁树也打码；树/图不同步、登录/安全区无法定位则不输出并 handoff；图片/crop/缩略图像素扫描。
  验收命令：`cargo test -p gcu-driver-macos --test capture`。

- **DoD：** 首次 macOS 图片交付即强制脱敏，A-α 前验收。

### P1-05 macOS：语义动作
- **依赖：** P1-03　**估算：** 2.5d
- **文件：** `computer-use/crates/gcu-driver-macos/src/ax/{actions.rs,value.rs,text.rs,scroll.rs}`、`tests/actions.rs`
- **内容：** Press（AXPress → AXConfirm → AXOpen → 选中行 `AXSelectedChildren` → 子孙可按元素；**不**用 AXRaise/Focus 冒充点击）、SecondaryAction、SetValue（仅 settable）、
  SelectText（`AXSelectedTextRange`）、ScrollElement（`AXScroll*ByPage` 优先，否则定向滚轮事件）。
- **测试：** 夹具每个控件的外部判定契约测试。
  验收命令：`cargo test -p gcu-driver-macos --test actions`。

- **DoD：** 本卡内容与测试全部通过，能力/协议/错误语义与03及07相关门禁一致；不注册空实现。

### P1-06 macOS：定向事件与 SkyLight 路径
- **依赖：** P1-04、S-02　**估算：** 3d
- **文件：** `computer-use/crates/gcu-driver-macos/src/input/{events.rs,skylight.rs,ledger.rs}`、`tests/targeted.rs`
- **内容：** 指针点击/滚动/拖拽经 `postToPid` + window-local 位置（`CGEventSetWindowLocation` dlsym）；键盘 postToPid（objc2-core-graphics CGEvent，不用 servo core-graphics）；SkyLight 路径（feature `macos-private-spi`）
  用于 Chromium 类窗口；运行时探测符号可用性；`PointerRoute::TargetedOnly` 失败不升级。InputLedger。
- **测试：** 夹具在后台（另一 App 前台）时点击/输入/拖拽到达且前台 App 不变；Chrome 被遮挡时点击计数器页面精确一次（50 次循环，契约测试 nightly）；
  记录 p50/p95 到 `computer-use/bench/macos.json`。
  验收命令：`cargo test -p gcu-driver-macos --test targeted`。

- **DoD：** 私有符号缺失、TCC 撤销、错误 PID/窗口、安全提示覆盖目标时拒绝，无静默升级；后台能力按 S-02 结果声明。

### P1-07 macOS：文本输入与剪贴板
- **依赖：** P1-06　**估算：** 1.5d
- **文件：** `computer-use/crates/gcu-driver-macos/src/input/{text.rs,clipboard.rs}`、`tests/text.rs`
- **内容：** `type_text` 策略：可设置非富文本 → AXValue 拼接；否则 grapheme 分块定向 Unicode 事件；超长（>2000 字符）且允许时走剪贴板（保存/恢复 changeCount 比较）；
  焦点不是可编辑元素 → `element_not_actionable`（提示先点击输入框）。
- **测试：** 中文、emoji（含 ZWJ 序列）、组合字符、换行；Electron 富文本（VS Code 输入框）冒烟。
  验收命令：`cargo test -p gcu-driver-macos --test text`。

- **DoD：** 剪贴板跨会话全局互斥；粘贴前检查 changeCount/内容/焦点，改变取消；敏感文本/高风险目标禁回退；仅内容匹配才恢复。测试覆盖并发会话覆盖/焦点变化/历史同步风险说明，不宣称消除最后一刻替换。

### P1-08 macOS：用户活动守卫、Stop 热键、前台路径
- **依赖：** P1-06、P0-08　**估算：** 1.5d
- **文件：** `computer-use/crates/gcu-driver-macos/src/input/{activity.rs,hotkey.rs,foreground.rs,watchdog.rs}`、`tests/stop.rs`
- **内容：** listen-only event tap（过滤自身注入事件：设置 `kCGEventSourceUserData` 标记）；缺"输入监控"时降级；全局热键（`RegisterEventHotKey` 或 event tap）；前台兜底路径（策略允许时）。
- **测试：** 模拟物理输入（自托管机用独立进程 `cliclick` 注入且不带标记）→ 前台动作被拒、后台动作不受影响。
  验收命令：`cargo test -p gcu-driver-macos --test stop`。

- **DoD：** 热键冲突/event tap 禁用/权限撤销/FFI 卡死测试；独立停止通道+watchdog，≤100ms 锁存、≤500ms 发 release，原生≤5s 超时/杀 worker；注册或监听失败禁依赖该停止能力的路线，跨轮/重启不自动恢复。

### P1-09 macOS：契约测试套件与冒烟脚本
- **依赖：** P1-01、P1-02、P1-03、P1-04、P1-05、P1-06、P1-07、P1-08　**估算：** 1.5d
- **文件：** `computer-use/crates/gcu-driver-macos/tests/contract.rs`、`computer-use/scripts/smoke-macos.sh`
- **内容：** `computer-use/crates/gcu-driver-macos/tests/contract.rs`（通用契约测试宏来自 `gcu-core::testing::contract!`，三平台共用同一用例表）；
  `computer-use/scripts/smoke-macos.sh`（TextEdit 写字保存、Finder 侧栏导航、系统设置搜索框）。
- **DoD：** 自托管 runner 全绿；结果 JSON 存档为 CI artifact。

- **测试：** 07通用契约C01–C27、后台能力与skip一致、Stop/截图脱敏/IDE执行入口；TextEdit写字保存/Finder导航/设置搜索，结果artifact。

  验收命令：`cargo test -p gcu-driver-macos --test contract`；自托管Mac另跑 computer-use/scripts/smoke-macos.sh。

---

## P2 · Windows 驱动（`gcu-driver-windows`）

### P2-01 Windows：发现、身份、虚拟桌面
- **依赖：** P0-05　**估算：** 2d
- **文件：** `computer-use/crates/gcu-driver-windows/src/{apps.rs,identity.rs,desktop.rs}`、`tests/identity.rs`
- **内容：** `EnumWindows`（可见、非工具窗、owner 关系）、进程映像路径、Authenticode 发布者、开始菜单 `.lnk` 后台索引（首次异步，缓存到 `%LOCALAPPDATA%\grok-computer-use\apps.json`，文件监视更新）、
  UWP（`ApplicationFrameHost` 解包到真实进程）、`IVirtualDesktopManager`、HWND stamp（创建时间 + 类名 + pid，防 HWND 复用）。
- **测试：** 夹具、记事本、设置（UWP）的 list/resolve/launch。
  验收命令：`cargo test -p gcu-driver-windows --test identity`。

- **DoD：** 启动发现自身 High/System 或 UIAccess 即拒绝工作；管理员目标/非交互/安全桌面在观察及所有输入路线统一门禁；签名有效性、junction/路径替换、同名证书、构建/证书/产品更新重新授权，DLL/插件/可写加载路径说明可信边界。
- **测试补充：** Medium/High/System/UIAccess 测试映像与权限状态、管理员目标、签名有效/失效/同名证书、入口不变的 DLL 加载风险。

### P2-02 Windows：UIA 树（CacheRequest）
- **依赖：** P2-01　**估算：** 3d
- **文件：** `computer-use/crates/gcu-driver-windows/src/uia/{mod.rs,tree.rs,cache.rs,roles.rs,events.rs,sensitive.rs}`、`tests/uia_tree.rs`
- **内容：** 专用 MTA 线程；`IUIAutomationCacheRequest` 一次取全部属性与 pattern 可用性属性（UIA_IsInvokePatternAvailablePropertyId 等，TreeScope_Subtree 分批，不缓存 pattern 对象）；角色映射；
  Chromium/Electron 的 UIA 树（Chrome 默认暴露）；`IsPassword`；超时 2s / 5000 节点；IUIAutomationStructureChangedEventHandler / IUIAutomationPropertyChangedEventHandler / IUIAutomationFocusChangedEventHandler 三类订阅。
  **移植来源：** `archive/cu-v1-2026-10-13:src-tauri/src/computer_use/windows_uia*.rs`（只取经验证的属性与 pattern 用法）。
- **测试：** 夹具快照；记事本、资源管理器、设置冒烟渲染。
  验收命令：`cargo test -p gcu-driver-windows --test uia_tree`。

- **DoD：** terminal/console、IDE REPL/任务/调试入口角色/名称启发式与混合网页 red_team 夹具；所有动作路径通用 Router 拒绝，未知 handoff，首次放行 IDE 即验收（尽力而为、非完备）。

### P2-03 Windows：WGC 截图 + PrintWindow 回退
- **依赖：** P2-01、P2-02、P0-07　**估算：** 2d
- **文件：** `computer-use/crates/gcu-driver-windows/src/capture/{mod.rs,wgc.rs,print_window.rs}`、`tests/capture.rs`
- **内容：** windows 0.62 的 IGraphicsCaptureItemInterop::CreateForWindow(hwnd) → 单帧（不用 TryCreateFromWindowId）；features 至少 Win32_System_WinRT_Graphics_Capture、Graphics_Capture、Win32_Foundation、Win32_UI_Accessibility；Win11 在 GraphicsCaptureSession 探测设 IsBorderRequired=false、IsCursorCaptureEnabled=false，Win10 不支持则忽略；DPI（`GetDpiForWindow`）与物理像素；
  最小化窗口 → `window_minimized`（不自动还原，除非前台策略允许）；回退 `PrintWindow(PW_RENDERFULLCONTENT)`。
- **测试：** 被遮挡窗口截图正确；高 DPI（150%）坐标对齐（夹具画网格，点击网格点 → 夹具报告命中格）。
  验收命令：`cargo test -p gcu-driver-windows --test capture`。

- **DoD：** 首次 Windows 截图交付必经 03 §4.4 脱敏，先收区域再裁树；最小化错误 window_minimized 已登记协议；密码被裁/树图不同步/登录无法定位不出图并 handoff，WGC 与 PrintWindow 回退同要求。

### P2-04 Windows：worker 隔离执行模式
- **依赖：** P2-02、P2-03　**估算：** 2d
- **文件：** `computer-use/crates/gcu-driver-windows/src/worker/{mod.rs,codec.rs,supervisor.rs}`、`computer-use/crates/gcu-cli/src/worker.rs`、`tests/worker.rs`
- **内容：** `gcu worker`：长度前缀 bincode 帧；主进程只做编排；worker 执行 UIA/截图；崩溃/超时（10s）/坏帧 → 重启并对**只读**操作重试一次，对**变更**操作返回 `unknown`；
  `ExecMode` 配置；句柄跨进程以 runtime-id（UIA RuntimeId）+ HWND 表示。
- **测试：** 注入崩溃（测试 feature 下 worker 收到特定请求 abort）→ 只读重试成功、变更 unknown；浸泡测试（nightly）：窗口拖动中连续观察 300 次无主进程崩溃。
  验收命令：`cargo test -p gcu-driver-windows --test worker`。

- **DoD：** 本卡内容与测试全部通过，能力/协议/错误语义与03及07相关门禁一致；不注册空实现。

### P2-05 Windows：语义动作（UIA Patterns）
- **依赖：** P2-02　**估算：** 2.5d
- **文件：** `computer-use/crates/gcu-driver-windows/src/uia/{actions.rs,patterns.rs,value.rs,scroll.rs}`、`tests/actions.rs`
- **内容：** Invoke / Toggle / SelectionItem.Select / ExpandCollapse / Value.SetValue / RangeValue / ScrollPattern / TextPattern 选区 / LegacyIAccessible.DoDefaultAction；
  按控件类型选择 pattern 的优先级表（`patterns.rs`）。
- **测试：** 夹具全控件外部判定。
  验收命令：`cargo test -p gcu-driver-windows --test actions`。

- **DoD：** 本卡内容与测试全部通过，能力/协议/错误语义与03及07相关门禁一致；不注册空实现。

### P2-06 Windows：定向消息与前台输入
- **依赖：** P2-05　**估算：** 3d
- **文件：** `computer-use/crates/gcu-driver-windows/src/input/{messages.rs,foreground.rs,ledger.rs}`、`tests/input.rs`
- **内容：** 命中测试（`ChildWindowFromPointEx`/UIA ElementFromPoint）→ `PostMessage` 鼠标/键盘到子 HWND；effect 检测（settle diff）；
  Chromium/UWP 检测到 PostMessage 无效（类名 `Chrome_RenderWidgetHostHWND`、`Windows.UI.Core.CoreWindow`）→ 直接走前台路由（需策略）；
  前台：激活序列 + `SendInput`（Unicode 文本分块、组合键、滚轮、拖拽插值）；InputLedger；UIPI/完整性检测 → `elevation_required`。
- **测试：** 夹具后台消息点击；记事本输入中文；前台路径在策略 never 时被拒。
  验收命令：`cargo test -p gcu-driver-windows --test input`。

- **DoD：** 本卡内容与测试全部通过，能力/协议/错误语义与03及07相关门禁一致；不注册空实现。

### P2-07 Windows：用户活动守卫、Stop 热键、覆盖层排除截图
- **依赖：** P2-06　**估算：** 1.5d
- **文件：** `computer-use/crates/gcu-driver-windows/src/input/{hooks.rs,hotkey.rs,watchdog.rs}`、`src/overlay.rs`、`tests/stop.rs`
- **内容：** LL 钩子（过滤 injected 标志）+ 周期替换；`RegisterHotKey`；`SetWindowDisplayAffinity` 能力检测（Win10 2004+）。
- **测试：** 模拟物理输入（测试程序用 `SendInput` 无法模拟"物理"——改为钩子单元测试注入假事件 + 手工验收清单入 PR）。
  验收命令：`cargo test -p gcu-driver-windows --test stop`。

- **DoD：** RegisterHotKey 冲突/钩子失效/UAC安全桌面/FFI卡死均覆盖；独立停止控制+watchdog，不在动作或审批队列后；≤100ms锁存/≤500ms发释放/≤5s杀阻塞worker；失效禁依赖能力的输入路线。

### P2-08 Windows：剪贴板与文本
- **依赖：** P2-06　**估算：** 1d
- **文件：** `computer-use/crates/gcu-driver-windows/src/input/{clipboard.rs,text.rs}`、`tests/clipboard.rs`
- **内容：** 移植 v1 剪贴板保存/恢复（`GetClipboardSequenceNumber` 比较）；>8192 UTF-16 分批或剪贴板。
- **测试：** 两会话剪贴板并发/序号与内容被替换/粘贴前焦点变化/高风险与敏感文本拒回退/恢复不覆盖外来内容。
  验收命令：`cargo test -p gcu-driver-windows --test clipboard`。

- **DoD：** 全局互斥且粘贴前校验序号/内容/焦点；最后一刻替换不能消除，历史/同步风险写入说明。

### P2-09 Windows：契约测试与冒烟
- **依赖：** P2-01、P2-02、P2-03、P2-04、P2-05、P2-06、P2-07、P2-08　**估算：** 1.5d
- **文件：** `computer-use/crates/gcu-driver-windows/tests/contract.rs`、`computer-use/scripts/smoke-windows.ps1`
- **内容：** 同 P1-09 的通用契约用例表；`scripts/smoke-windows.ps1`（记事本写字保存、资源管理器导航、设置搜索）。
- **DoD：** windows-2022 runner（交互桌面会话）全绿。

- **测试：** 07通用契约C01–C27、worker超时变更unknown不重试、提升拒绝、热键失效/截图脱敏/IDE执行入口；记事本/资源管理器/设置脚本。

  验收命令：`cargo test -p gcu-driver-windows --test contract`；Windows runner另跑 smoke-windows.ps1。

---

## P3 · Linux X11 驱动（`gcu-driver-linux`）

### P3-01 AT-SPI 连接与树
- **依赖：** P0-05　**估算：** 3d
- **文件：** `computer-use/crates/gcu-driver-linux/src/atspi/{mod.rs,tree.rs,roles.rs,events.rs,sensitive.rs}`、`src/identity.rs`、`tests/atspi_tree.rs`
- **内容：** `atspi = { version="0.30", features=["tokio"] }`（上游 zbus，不 fork；子接口外层 tokio 超时）；`org.a11y.Bus` 获取地址；批量 `GetChildren`/`GetAttributes`/`GetState`/`GetRole`/`Component.GetExtents(WINDOW)`；角色映射；
  GTK3/GTK4/Qt5/Qt6/Electron（`--force-renderer-accessibility` 提示）差异处理；超时。
- **测试：** Xvfb 后用 dbus-run-session 启动 /usr/lib/at-spi2-core/at-spi-bus-launcher，等待 org.a11y.Bus 就绪 + 夹具 + gedit（或 gnome-text-editor）渲染快照。
  验收命令：`rcargo test -p gcu-driver-linux --test atspi_tree`。

- **DoD：** Linux 身份以规范路径+dev/inode+sha256 绑定，desktop id 仅发现，每次授权使用校验进程实例映像，原地替换/更新重审；terminal/console/IDE任务与调试入口角色/名称启发式，所有动作路径拒绝，未知 handoff（尽力而为、非完备）。
- **测试补充：** desktop文件换入口、符号链接/包装器、同路径原地更新、pid复用；IDE/Web终端/远程桌面混合区 fake 与真实夹具。

### P3-02 X11 窗口与截图
- **依赖：** P3-01、P0-07　**估算：** 2d
- **文件：** `computer-use/crates/gcu-driver-linux/src/x11/{mod.rs,windows.rs,capture.rs}`、`tests/capture.rs`
- **内容：** x11rb：`_NET_CLIENT_LIST`、`_NET_WM_PID`、`_NET_WM_NAME`、`_NET_ACTIVE_WINDOW`；AT-SPI frame ↔ X 窗口对齐（pid + 标题 + 几何）；
  截图 `XComposite` 命名 pixmap + `GetImage`（被遮挡可采）；无 composite 时 `GetImage` 根窗口裁剪并声明能力。
- **测试：** 被遮挡窗口/无composite降级/物理像素映射；先收敏感区域再裁树，密码被裁/图树不同步/登录不可定位不出图并handoff；衍生物像素扫描。
  验收命令：`rcargo test -p gcu-driver-linux --test capture`。

- **DoD：** 首次 Linux 截图交付即脱敏，无 composite 时不把根窗口其他应用内容交付模型。

### P3-03 AT-SPI 动作 + XTEST
- **依赖：** P3-02　**估算：** 2.5d
- **文件：** `computer-use/crates/gcu-driver-linux/src/{atspi/actions.rs,x11/{input.rs,keys.rs,ledger.rs}}`、`tests/actions.rs`
- **内容：** Action（`do_action` 按名匹配 click/press/activate）、EditableText.SetTextContents/InsertText、Value.SetCurrentValue、Selection、Text 选区；
  XTEST 指针/键盘（全局 → 前台路由，需策略，前先 `_NET_ACTIVE_WINDOW` 激活）；keysym 映射（xkbcommon="0.9"，仅 Linux；CI 安装 libxkbcommon-dev）。
- **测试：** 夹具外部判定。
  验收命令：`rcargo test -p gcu-driver-linux --test actions`。

- **DoD：** 本卡内容与测试全部通过，能力/协议/错误语义与03及07相关门禁一致；不注册空实现。

### P3-04 用户活动守卫与热键（X11）
- **依赖：** P3-03　**估算：** 1d
- **文件：** `computer-use/crates/gcu-driver-linux/src/x11/{activity.rs,hotkey.rs,watchdog.rs}`、`tests/stop.rs`
- **内容：** XInput2 原始事件（排除 XTEST 设备）；`XGrabKey` 热键。
- **测试：** XGrabKey 冲突/监听失效/用户输入/FFI卡死；Stop独立通道及watchdog；重启仍锁存。
  验收命令：`rcargo test -p gcu-driver-linux --test stop`。

- **DoD：** 依赖的停止能力失效禁相应路线，≤100ms锁存/≤500ms发释放/原生≤5s超时或杀worker。

### P3-05 契约测试与 CI
- **依赖：** P3-01、P3-02、P3-03、P3-04　**估算：** 1.5d
- **文件：** `computer-use/crates/gcu-driver-linux/tests/contract.rs`、`computer-use/scripts/smoke-linux.sh`、`.github/workflows/computer-use.yml`
- **内容：** CI job：xvfb-run + dbus-run-session + /usr/lib/at-spi2-core/at-spi-bus-launcher（就绪后跑测试） + openbox（窗口管理器，提供 EWMH）；通用契约用例表；`scripts/smoke-linux.sh`。

- **测试：** Xvfb+dbus-run-session+AT-SPI就绪后C01–C27；Linux映像替换重审、热键失效/截图脱敏/IDE执行入口，脚本外部oracle。

  验收命令：`rcargo test -p gcu-driver-linux --test contract`；桌面runner按前述Xvfb/dbus启动并运行smoke-linux.sh。

- **DoD：** 本卡内容与测试全部通过，能力/协议/错误语义与03及07相关门禁一致；不注册空实现。

---

## P4 · 路线 A 产品化

### P4-01 macOS helper app 与代理
- **依赖：** P1-09、S-02　**估算：** 3d
- **文件：** `computer-use/crates/gcu-cli/src/helper/{mod.rs,proxy.rs,auth.rs}`、`computer-use/apps/gcu-helper-macos/{Info.plist,assets/}`、`computer-use/xtask/src/bundle.rs`、`tests/helper.rs`
- **内容：** [04 §3](04-route-a-standalone-mcp.md#3-macos-helper-与-tcc)；`gcu agent` 子命令；audit token 取对端代码签名，可执行文件必须与 helper 同一 cdhash/同一签名身份，其他客户端不得连接；UID/文件权限仅隔离其他用户；版本握手；空闲退出；`xtask bundle-macos` 生成 `.app`（Info.plist、图标、LSUIElement）。
- **测试：** e2e：从终端运行 `gcu mcp`，TCC 只授权 helper 时工具可用；旧版本 helper 被替换。
  验收命令：`rcargo test -p gcu-cli --test helper`；`rcargo run -q -p xtask -- bundle-macos`。

- **DoD：** 同 UID 冒充客户端、cdhash/签名身份不匹配、假helper/端点替换被拒；合法同二进制连接成功，新连接不继承其他会话临时授权。

### P4-02 Onboarding 窗口（macOS）
- **依赖：** P4-01　**估算：** 2d
- **文件：** `computer-use/ui/onboarding.html`、`computer-use/crates/gcu-cli/src/ui/{mod.rs,onboarding.rs}`、`computer-use/xtask/src/ui.rs`、`tests/onboarding.rs`
- **内容：** wry WebView；共享 HTML/CSS（`computer-use/ui/`，与路线 B 不共享 React——独立轻量页面，但视觉 token 对齐 App）；15 语言（i18n JSON 从 App 的 `computer-use` 命名空间导出脚本生成，单一来源）；权限轮询；重启 helper。
- **测试：** 单文件 HTML/CSS 构建脚本压缩后 include_str! 编入 gcu 二进制，离线资源/i18n 15语言键锁步，权限轮询与重启按钮/失败路径。
  验收命令：`rcargo test -p gcu-cli --test onboarding`。

- **DoD：** 零外部 HTML/运行时文件；Linux 若复用审批 UI 需 GTK3 的 webkit2gtk-4.1，缺库走已声明回退；HTML默认拒绝与不劫持Enter规则同审批窗。

### P4-03 ApprovalProvider：elicitation + 审批窗（三平台）
- **依赖：** P0-09、P4-01　**估算：** 3d
- **文件：** `computer-use/crates/gcu-cli/src/host/{approval.rs,policy_store.rs}`、`computer-use/crates/gcu-mcp/src/elicitation.rs`、`computer-use/ui/approval.html`、`tests/approval.rs`
- **内容：** [04 §4](04-route-a-standalone-mcp.md#4-审批approvalprovider-实现)；审批窗 HTML 与 onboarding 共用组件；键盘可达；超时；`approvals.json`；`gcu approvals` 子命令。
- **测试：** elicitation 路径用 MCP 测试客户端；审批窗用 `GCU_APPROVAL_TEST_AUTOCLICK`（仅测试构建）验证四种决定。
  验收命令：`rcargo test -p gcu-cli --test approval`；`rcargo test -p gcu-cli --features eval-autoapprove --test approval`。

- **DoD：** GCU_APPROVAL=auto-* 只在 feature eval-autoapprove；发布构建设置此变量报错退出（含任意auto值），测试构建四档/超时/取消通过。OS核验签名状态/发布者/路径，图标名称辅助；模型理由标“模型提供”、纯文本处理双向控制字符；默认拒绝，不劫持聊天Enter；正规schema与tools/call内elicitation验收。

### P4-04 gcu-overlay（三平台）
- **依赖：** P1-06、P2-06、P3-03　**估算：** 3.5d（可拆三平台）
- **文件：** `computer-use/crates/gcu-overlay/src/{lib.rs,macos.rs,windows.rs,x11.rs,wayland.rs,animation.rs}`、`tests/overlay.rs`
- **内容：** [04 §5](04-route-a-standalone-mcp.md#5-覆盖层gcu-overlay)；排除截图；光标弹簧动画（60fps，参数可配）；胶囊 + 停止按钮（点击走 `UserActivity::stop`）。
- **测试：** 截图中不出现覆盖层（契约测试：开启覆盖层后截图与关闭时像素哈希一致，X11 除外）。
  验收命令：`rcargo test -p gcu-overlay --test overlay`。

- **DoD：** Stop/用户恢复的独立通道和锁存、热键冲突失效关闭路线；Wayland胶囊在选定crate后补03 §2/deny.toml，GNOME不显示并声明能力，不假定现成依赖。

### P4-05 `gcu install/uninstall`（全客户端）
- **依赖：** P0-10　**估算：** 2d
- **文件：** `computer-use/crates/gcu-cli/src/install/{mod.rs,toml.rs,json.rs,antigravity.rs}`、`tests/install.rs`
- **内容：** [04 §2.1](04-route-a-standalone-mcp.md#21-gcu-install-client-写入的配置)；TOML（Grok/Codex）用 toml_edit 0.25 保注释；JSON 用 serde_json + preserve_order 按键增删，测试断言键值不断言空白；必须保 JSONC 注释时另选库并写 ADR/deny.toml；备份；`--dry-run`；`~/.grok` 写入需确认。
- **测试：** 每个客户端：空文件、已有其他条目、已有 gcu 条目（幂等）、uninstall 精确移除——快照测试；Antigravity 走 `agy mcp add/remove`，测试中用假的 `agy` 可执行文件记录 argv。
  验收命令：`rcargo test -p gcu-cli --test install`。

- **DoD：** 本卡内容与测试全部通过，能力/协议/错误语义与03及07相关门禁一致；不注册空实现。

### P4-06 turn-ended 接入
- **依赖：** P0-09、S-01　**估算：** 1d
- **文件：** `computer-use/crates/gcu-cli/src/turn_ended.rs`、`computer-use/crates/gcu-mcp/src/lifecycle.rs`、`tests/turn_ended.rs`
- **内容：** `gcu turn-ended`（经 socket/命名管道通知运行中的 `gcu mcp` 会话）；Codex notify 链式包装；空闲推定。
- **测试：** turn-ended hook/重复通知/空闲推定、链式notify不丢原调用；turn授权到期、Stop不解除。
  验收命令：`rcargo test -p gcu-cli --test turn_ended`。

- **DoD：** hook错误返回可见诊断，不恢复锁存，客户端能力按S-01 ADR选择。

### P4-07 打包与分发
- **依赖：** P4-01、P4-02、P4-03、P4-04、P4-05、P4-06　**估算：** 3d
- **文件：** `.github/workflows/release-computer-use.yml`、`computer-use/xtask/src/{release.rs,sbom.rs}`、`computer-use/packages/`、`computer-use/scripts/install-smoke/`
- **内容：** `release-computer-use.yml`：三平台四架构构建、macOS 签名公证（有 secret 时）、Windows 签名（有 secret 时）、SHA256SUMS + cosign、SBOM、
  npm launcher + 平台包、Homebrew formula、winget manifest、`.deb/.rpm`（cargo-deb / cargo-generate-rpm）。tag 规则 `cu-v<semver>`。
- **DoD：** 在干净虚拟机（三平台）按 README 安装并接入 Grok CLI 跑通冒烟（录屏/截图入 release PR）。
- **测试：** 三平台四架构包在干净VM安装、gcu doctor/MCP冒烟、签名缺失/错误包/SBOM/哈希验真、卸载无残留；runner与凭据缺失记not_run。
  验收命令：`rcargo test -p xtask`；`rcargo run -q -p xtask -- release --dry-run`。

### P4-08 用户文档
- **依赖：** P4-07　**估算：** 1d
- **文件：** `computer-use/README.md`、`computer-use/README_ZH.md`
- **内容：** `computer-use/README.md`（用户向）+ `README_ZH.md`：安装、权限、接入各客户端、授权说明、平台差异表、隐私、故障排除、卸载。

- **测试：** 用户安装/权限/四档审批/Stop恢复/撤销/客户端接入/平台差异/隐私/卸载示例逐项对照CLI；Markdown相对链接有效。

  验收命令：`rcargo run -q -p xtask -- check-deps`；`python3 docs/plans/computer-use-v2/reviews/check_plan.py`。

- **DoD：** 本卡内容与测试全部通过，能力/协议/错误语义与03及07相关门禁一致；不注册空实现。

---

## P5 · 路线 B 集成（Grok App）

> 所有前端卡：交互逻辑 + 视觉一致 + 功能对等 三件套；15 语言；不改 App.tsx/AppWorkbench.tsx 状态。

### P5-01 ComputerUseHost 骨架 + 引擎托管
- **依赖：** P0-12、P1-09　**估算：** 2.5d
- **文件：** `src-tauri/src/computer_use/{mod.rs,supervisor.rs,host_server.rs}`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、`src-tauri/src/computer_use/tests/supervisor.rs`、`computer-use/xtask/src/deps.rs`
- **内容：** `src-tauri/Cargo.toml` **仅** path 依赖 `gcu-protocol`、`gcu-host-protocol`；`tauri.conf.json` `externalBin` 加入 `gcu`（构建脚本从 computer-use workspace 产出各平台二进制）；
  `computer_use/{mod,supervisor,host_server}.rs（永久路径；mac 先行，Windows/Linux 引擎就绪后再启用）`；按 `computer_use_enabled` 启停；空闲退出；崩溃重启与熔断；开发者设置"外部引擎路径"；`xtask check-deps` 扩展到 src-tauri。
- **测试：** Rust 单测：启停幂等、握手失败处理、熔断；集成测试：用 `gcu engine --driver fake` 跑一次完整审批往返（无 UI，测试回调自动应答）。
  验收命令：`rcargo test -p gcu-cli --test engine_host`；`rcargo run -q -p xtask -- check-deps`；`cargo test -p grok-app computer_use::supervisor`；匹配平台CI。

- **DoD：** 正式模式拒绝签名/构建身份不符的引擎；外部引擎仅Dev身份单独授权；Host双向认证/防抢占；引擎重启恢复Stop锁存，发布模式Windows提升/UIAccess拒绝。

### P5-02 Gateway + 注入
- **依赖：** P5-01、S-01　**估算：** 2d
- **文件：** `src-tauri/src/computer_use/{inject.rs,gateway.rs}`、`src-tauri/src/session_manager/`（既有 MCP 注入入口）、`computer-use/crates/gcu-mcp/src/http.rs`、`tests/gateway.rs`
- **内容：** [05 §1](05-route-b-integrated.md#1-进程与数据流)、inject.rs；`engine.session.open` 取端点与 token（引擎侧生成，每会话随机 32 字节，会话结束作废，校验 Host/Origin）；仅本地会话注入。
- **测试：** 未带 token / 错 token / 跨会话 token 被拒；SSH 会话不注入（与现有 session 类型判断共用函数）。
  验收命令：`rcargo test -p gcu-mcp --test gateway`；`cargo test -p grok-app computer_use::inject`；匹配平台CI。

- **DoD：** token仅继承匿名管道/FD，不用GCU_TOKEN环境；环境/子进程环境/ACP载荷日志/崩溃报告泄露扫描，会话关闭旧token撤销。Host精确单值127.0.0.1:<实际端口>；Origin缺省仅带token的非浏览器客户端可用，有值仅http://127.0.0.1:<实际端口>；DNS rebinding/恶意Origin/null/重复Host/无token initialize/跨会话图片被拒。S-01若不支持私密headers/FD则needs-decision，不绕回环境。

### P5-03 审批卡（App 授权）
- **依赖：** P5-02　**估算：** 3d
- **文件：** `src-tauri/src/computer_use/{approval.rs,policy_store.rs,commands.rs}`、`src/components/computer-use/ComputerUseApprovalCard{.tsx,.test.tsx}`、`src/lib/api/computerUse.ts`、`src/i18n/messages/*/computer-use.ts`
- **内容：** approval.rs + `ComputerUseApprovalCard.tsx` + 事件/命令；挂起审批与会话绑定；倒计时；键盘操作（Tab/Enter/Esc=拒绝）；屏幕阅读器标签。
- **测试：** Vitest 组件测试（四种决定、超时、会话切换回来仍在）；Rust：会话关闭自动拒绝。
  验收命令：`rcargo test -p gcu-host-protocol`；`pnpm exec vitest run src/components/computer-use/ComputerUseApprovalCard.test.tsx`；`cargo test -p grok-app computer_use::approval`；匹配平台CI。

- **DoD：** App只在真实审批事件中绑定请求/session/App映像/引擎实例/策略版本单次消费后持久化Always并发engine.policy.changed；无审批/跨身份/重放失败；OS核验身份展示，名称图标辅助；理由纯文本标“模型提供”且处理双向控制字符；默认拒绝，聊天Enter不被卡片抢走；组件测试包含冒名/重复决定。

### P5-04 敏感动作确认卡 + 交接卡
- **依赖：** P5-03　**估算：** 2d
- **文件：** `src/components/computer-use/{ComputerUseActionConfirmCard.tsx,ComputerUseHandoffCard.tsx,confirmState.ts,confirmState.test.ts}`、对应 `*.test.tsx`、`src-tauri/src/computer_use/approval.rs`、`src/i18n/messages/*/computer-use.ts`
- **内容：** ComputerUseActionConfirmCard（沿用05组件名，不另造平行组件）与ComputerUseHandoffCard；pending→submitting→allowed/denied/timed_out/stale/closed，host.approval.confirmAction/handoff联动；脱敏crop、理由、拒绝/超时和重复提交禁用。handoff用户“已处理”只触发新消息和重观察，不直接重放/不解除Stop。
- **测试：** Vitest允许/拒绝/超时/会话切换/关闭/重复点击、纯文本双向字符、缺crop/敏感区定位失败；Rust审批后状态变化作废重审、拒绝approval_denied零执行。
  验收命令：`rcargo test -p gcu-core --test router`；`pnpm exec vitest run src/components/computer-use/ComputerUseActionConfirmCard.test.tsx src/components/computer-use/ComputerUseHandoffCard.test.tsx`。

- **DoD：** 状态机与Host往返完整，确认绑定App/进程/窗口/参数/snapshot+fingerprint/版本，批准后投递前重验；15语言与现有卡片视觉/无障碍一致。

### P5-05 工具调用卡（聊天流渲染）
- **依赖：** P5-02　**估算：** 2.5d
- **文件：** `src/components/computer-use/ComputerUseToolCard{.tsx,.test.tsx}`、`src/components/lobe-chat/`（既有工具渲染入口）、`src/lib/computer-use/events.ts`、`src/i18n/messages/*/computer-use.ts`
- **内容：** 在现有工具调用渲染管线中识别 `computer-use` server 的调用：显示动作图标、目标（App · 元素名）、结果徽标（done/refused/unknown + effect）、截图缩略图（点击放大，复用现有图片查看器）、耗时；折叠显示原始文本。
- **测试：** pending/done/refused/failed/unknown+delivery/effect、缺缩略图/展开原文/查看大图、跨会话图片拒绝与删除会话清理。
  验收命令：`rcargo test -p gcu-host-protocol`；`pnpm exec vitest run src/components/computer-use/ComputerUseToolCard.test.tsx`。

- **DoD：** 截图只来自脱敏管线、媒体预览遵循loopback+path resolve，复用既有图片查看器。

### P5-06 Computer 侧栏面板
- **依赖：** P5-05　**估算：** 2.5d
- **文件：** `src/components/computer-use/ComputerUsePanel{.tsx,.test.tsx}`、`src/hooks/useComputerUseEvents.ts`、`src/lib/computer-use/store.ts`、`src/i18n/messages/*/computer-use.ts`
- **内容：** 替换 SideWorkbench 的 Computer 入口为 `ComputerUsePanel`；时间线、当前状态、停止；无任务时的空状态（说明 + 去设置）。
- **测试：** 时间线节流/多会话隔离/空状态/失败状态/Stop和显式用户恢复，图片放大和清理；Vitest键盘无障碍。
  验收命令：`rcargo test -p gcu-host-protocol`；`pnpm exec vitest run src/components/computer-use/ComputerUsePanel.test.tsx`。

- **DoD：** 完整面板可观察/停止/恢复，与聊天状态一致，不增App shell状态。

### P5-07 覆盖窗（App 内）
- **依赖：** P5-01　**估算：** 3d
- **文件：** `src-tauri/src/computer_use/overlay.rs`、`src/components/computer-use/ComputerUseOverlay{.tsx,.test.tsx}`、`src/lib/computer-use/overlay.ts`、`src/i18n/messages/*/computer-use.ts`
- **内容：** overlay.rs + `cu-overlay` Tauri 窗口（透明、置顶、不可激活、鼠标穿透除胶囊外）；排除截图；多显示器定位；光标动画。
- **测试：** 多屏/DPI定位、不可激活与点击穿透、截图排除、按钮Stop/用户恢复，不抢聊天焦点；Vitest光标状态。
  验收命令：`rcargo test -p gcu-overlay --test overlay`；`pnpm exec vitest run src/components/computer-use/ComputerUseOverlay.test.tsx`；`cargo test -p grok-app computer_use::overlay`；匹配平台CI。

- **DoD：** 同路线A停止时限与独立通道，Stop重启锁存，覆盖窗不享有浏览窗远程内容权限。

### P5-08 设置页
- **依赖：** P5-03　**估算：** 2.5d
- **文件：** `src/components/settings/ComputerUseSection{.tsx,.test.tsx}`、`src/lib/settingsCatalog/entries/extensions.ts`、`src-tauri/src/computer_use/commands.rs`、`src/i18n/messages/*/computer-use.ts`
- **内容：** `settings/ComputerUseSection.tsx`；settingsCatalog 登记与关键词；系统权限状态（`cu_doctor`）与去授权；始终允许列表（App 图标、名称、授予时间、撤销）；敏感确认与前台策略 `Select`；热键展示；"清除内置浏览窗数据"（P5-10 后）。
- **测试：** 设置搜索命中；深链；撤销后下一次调用重新弹卡。
  验收命令：`rcargo test -p gcu-host-protocol`；`pnpm exec vitest run src/components/settings/ComputerUseSection.test.tsx`。

- **DoD：** 本卡内容与测试全部通过，能力/协议/错误语义与03及07相关门禁一致；不注册空实现。

### P5-09 turn/session 生命周期接线 + wiki 重写
- **依赖：** P5-02　**估算：** 1.5d
- **文件：** `src-tauri/src/computer_use/lifecycle.rs`、`src-tauri/src/session_manager/`、`docs/llm-wiki/{computer-use.md,settings-ia.md}`、`AGENTS.md`、`src-tauri/src/computer_use/tests/lifecycle.rs`
- **内容：** ACP 流结束 → `turn_ended`；会话关闭/fork/切模型 → `session_closed`；`docs/llm-wiki/computer-use.md` 重写（≤200 行）并在 `AGENTS.md` Read first 增加链接；`docs/llm-wiki/settings-ia.md` 登记。
- **测试：** 跨轮/排队调用/会话重连/引擎重启仍stopped_by_user，只有用户恢复可继续；关闭/fork/切模型撤销旧token/授权/图片/portal，挂起审批拒绝。
  验收命令：`rcargo test -p gcu-core --test stop`；`cargo test -p grok-app computer_use::lifecycle`；匹配平台CI。

- **DoD：** 生命周期幂等且多会话隔离；wiki≤200行与settings登记，恢复不成为模型工具。

### P5-10 内置浏览窗（L1）
- **依赖：** P5-07　**估算：** 3d
- **文件：** `src-tauri/src/computer_use/browser.rs`、`src-tauri/capabilities/`（cu-browser 独立最小权限）、`src/components/computer-use/ComputerUseBrowser{.tsx,.test.tsx}`、`src/i18n/messages/*/computer-use.ts`、`src-tauri/src/computer_use/tests/browser.rs`
- **内容：** [05 §5](05-route-b-integrated.md#5-内置浏览窗l1p5-可选交付)；`browser_navigate` 动态注册；拒绝名单对 `cu-browser` 窗口放行。
- **测试：** 远程网页尝试cu_approval_decide/恢复/事件/媒体token/Host均拒绝；iframe/导航/弹窗不升级capability；页面自报label不放行；危险URL/地址栏输入、删除profile/关闭窗工具退注册。
  验收命令：`rcargo test -p gcu-core --test policy`；`pnpm exec vitest run src/components/computer-use/ComputerUseBrowser.test.tsx`；`cargo test -p grok-app computer_use::browser`；匹配平台CI。

- **DoD：** cu-browser无Tauri命令、事件、媒体token、Host权；放行只据Rust实际窗口身份映射。地址栏可信控制与网页隔离、普通浏览完整；自己输入key_blocked，不承诺L0重定向拦截。

### P5-11 App 内评测与发布开关
- **依赖：** P5-01、P5-02、P5-03、P5-04、P5-05、P5-06、P5-07、P5-08、P5-09、P9-01　**估算：** 1.5d
- **文件：** `computer-use/eval/tasks/app/`、`src/components/settings/ComputerUseSection.tsx`、`CHANGELOG.md`、`src/i18n/messages/*/computer-use.ts`
- **内容：** 在 App 构建上跑评测基础任务集（见 07 §5）；设置项从"实验"转为正式需满足 07 §8 门槛；CHANGELOG 条目（≤90 字符首句）。

- **测试：** App托管引擎评测runner与oracle自证，开关/缺权限/审批拒绝/Stop、跨会话图片、SSH不注入；验证07 §8报告与WhatsNew长度。
  验收命令：`rcargo test -p gcu-eval --test runner`；`pnpm exec vitest run src/lib/whatsNew.test.ts`；一次性匹配平台桌面另运行 cargo run -p gcu-eval -- --suite app。

- **DoD：** 依赖P9-01评测runner；未满足门槛保持实验开关，不能用P9-03性能卡代替runner。

---

## P6 · 效率

### P6-01 settle 引擎与事件订阅调优
- **依赖：** P1-09、P2-09、P3-05　**估算：** 2d
- **文件：** `computer-use/crates/gcu-core/src/settle/{mod.rs,events.rs,poll.rs}`、`tests/settle.rs`、`computer-use/bench/`
- **内容：** 统一 settle 策略（事件静默/树哈希/预算）；每平台默认参数基准化；`settle` 字段输出。
- **DoD：** bench 显示三平台元素点击返回 p50 不劣于 P1/P2/P3 基线，且 `no_visible_change` 误报率（夹具上真实有变化却报无变化）< 2%。
- **测试：** 事件静默/树哈希一致/预算截止/无订阅轮询、超时取消与Stop；有变化夹具的no_visible_change误报<2%，p50不劣于基线。
  验收命令：`rcargo test -p gcu-core --test settle`。

### P6-02 auto 截图策略
- **依赖：** P6-01　**估算：** 1.5d
- **文件：** `computer-use/crates/gcu-core/src/observe/{quality.rs,auto_capture.rs}`、`tests/auto_capture.rs`
- **内容：** [03 §4.4](03-architecture.md#44-截图) 规则 + 树质量评估（可交互节点数、弹窗、canvas 角色占比，参考 bo-516 `_auto_thin` 的思路但阈值由评测确定）；
  `get_app_state` 的 `include_screenshot` 默认值改为 `"auto"`（评测证明不降成功率后）。
- **测试：** canvas/稀树/空diff像素变化/明确请求附图，图树不同步/未知敏感区不出图，auto不取消脱敏。
  验收命令：`rcargo test -p gcu-core --test auto_capture`。

- **DoD：** 相同任务集成功率不降才改默认，失败保留true默认。

### P6-03 `wait_for`
- **依赖：** P6-01　**估算：** 1.5d
- **文件：** `computer-use/crates/gcu-core/src/wait/{mod.rs,condition.rs,deadline.rs}`、`tests/wait_for.rs`、`computer-use/crates/gcu-mcp/src/tools/wait.rs`
- **内容：** wait_for五种条件 text_present/text_absent/element(role,name)/stable/window_title；复用事件订阅并用150ms轮询兜底；单调deadline默认5s上限30s；观察授权/Stop/取消同Router；就绪返回新状态，超时timeout附最后状态，零变更投递。
- **测试：** 五条件即时成功/事件触发/轮询/永不满足/30s上限/窗口消失/取消/Stop/敏感观察授权；fake确定时钟验证无busy-loop、无驱动变更。
  验收命令：`rcargo test -p gcu-core --test wait_for`。

- **DoD：** 状态机 ready/waiting/timed_out/cancelled覆盖；接MCP与batch/script复用入口。

### P6-04 `computer_batch`
- **依赖：** P6-03　**估算：** 3d
- **文件：** `computer-use/crates/gcu-core/src/batch/{mod.rs,preflight.rs,executor.rs}`、`tests/batch.rs`
- **内容：** [03 §11.4](03-architecture.md#114-computer_batch-语义)；预检；逐步结果；expect。
- **测试：** fake + 夹具：全成功、中途 stale、中途敏感确认被拒、超时、预检失败零执行。
  验收命令：`rcargo test -p gcu-core --test batch`。

- **DoD：** 本卡内容与测试全部通过，能力/协议/错误语义与03及07相关门禁一致；不注册空实现。

### P6-05 `computer_script`（feature）
- **依赖：** P6-04　**估算：** 4d
- **文件：** `computer-use/crates/gcu-script/src/{lib.rs,worker.rs,rpc.rs,runtime.rs,bindings.rs,limits.rs,watchdog.rs}`、`tests/sandbox.rs`、`cu.d.ts`、`computer-use/crates/gcu-cli/src/script_worker.rs`、`computer-use/fuzz/fuzz_targets/script_rpc.rs`
- **内容：** rquickjs 0.14 default-features=false/features=["std"]，禁 rust-alloc/allocator/loader/dyn-load/full；set_memory_limit(32MB)、interrupt handler 用 Instant 管墙钟；独立受限 worker 经窄 RPC 调 ActionRouter。[03 §11.5](03-architecture.md#115-computer_script-语义feature-scriptp6)；rquickjs；资源限制；取消；`cu` API 类型声明文件（`cu.d.ts`，放入 instructions 链接/skill）。
- **测试：** 沙箱逃逸用例（访问 `globalThis` 未暴露对象、无限循环、内存炸弹、超长输出）；功能用例同 batch。
  验收命令：`rcargo test -p gcu-script --no-default-features --test sandbox`；Unix nightly另执行 cargo +nightly fuzz run script_rpc -- -max_total_time=60。

- **DoD：** 安全审查清单（07 §7）签字；宿主分配≤32MB、native 并发1/队列32、总输出≤2MB、worker 总内存≤128MB，驱动调用≤200；独立 watchdog 超时 ≤500ms 杀 worker；无文件/网络/进程/环境权限，不传原生句柄。测试未启用 rust-alloc 的内存炸弹、宿主分配炸弹/并发 RPC/绑定与沙箱 fuzz、脚本或 FFI 卡死时 Stop 可达。

### P6-06 工具目录与 instructions 优化
- **依赖：** P6-05　**估算：** 1d
- **文件：** `computer-use/crates/gcu-mcp/src/{catalog.rs,instructions.rs}`、`computer-use/crates/gcu-protocol/golden/profiles/`、`tests/catalog.rs`
- **内容：** 按评测结果精简描述；profile：`minimal`（9 核心）/`default`/`full`。
- **测试：** minimal明确9工具、default已上线默认面、full feature条件工具与browser_navigate仅窗打开；schema golden与目录预算。
  验收命令：`rcargo test -p gcu-mcp --test catalog`；`rcargo run -q -p xtask -- golden --check`；`rcargo run -q -p xtask -- budget`。

- **DoD：** 工具实际能力/instructions一致，所有profile≤预算，无注册空实现。

### P6-07 效率评测（A/B）
- **依赖：** P6-01、P6-02、P6-03、P6-04、P6-05、P6-06、P9-01　**估算：** 2d
- **文件：** `computer-use/crates/gcu-eval/src/ab.rs`、`computer-use/crates/gcu-eval/tests/ab.rs`、`computer-use/eval/reports/`
- **内容：** 07 §5.4 的 A 组（单步工具、无 diff、总截图）vs B 组（diff + auto 截图 + batch + script）；报告入 `computer-use/eval/reports/`。
- **DoD：** B 组成功率 ≥ A 组，模型往返中位数下降 ≥30%，输入 token 下降 ≥30%；否则回滚默认值并写原因。

- **测试：** A/B同模型effort/任务/初态/机器与三次均值，oracle自证、成功率/往返/token与red_team零违规；未达目标默认回滚。

  验收命令：`rcargo test -p gcu-eval --test ab`；一次性桌面另运行 cargo run -p gcu-eval -- --suite core --ab。

---

## P7 · Wayland

### P7-01 Wayland 检测与能力表
- **依赖：** P3-05　**估算：** 1d
- **文件：** `computer-use/crates/gcu-driver-linux/src/wayland/{mod.rs,capabilities.rs,doctor.rs}`、`tests/wayland_caps.rs`
- **目标：** 探测 GNOME/KDE/wlroots/其他；doctor 与 capabilities 同源；守卫/全局 Stop 不可靠的环境禁用依赖该能力的输入路线。
- **测试：** 环境探测/缺 portal/未知 compositor/热键冲突/全局停止不可用，能力表如实降级。
  验收命令：`rcargo test -p gcu-driver-linux --test wayland_caps`。

- **DoD：** 不声称 Wayland 全支持；探测与禁止路线的测试通过。

### P7-02 portal ScreenCast 截图与 restore token
- **依赖：** P7-01　**估算：** 4d
- **文件：** `computer-use/crates/gcu-driver-linux/src/wayland/{portal.rs,capture.rs,restore.rs}`、`tests/portal_capture.rs`
- **目标：** ashpd 0.13 default-features=false/features=["tokio","screencast","remote_desktop","pipewire"]，沿其 pipewire 0.9，不单独依赖 pipewire；CI libpipewire-0.3-dev。移植 v1 帧/会话身份检查，单帧走首次交付脱敏管线。
- **测试：** token 文件0600/目录0700、来源绑定/24h内有效且不超过授权期限；恢复后 compositor/source/窗口/进程/授权版本变化拒绝；过期、关闭功能、撤销、会话结束清 token 并关闭 portal；未知采集范围不出图。
  验收命令：`rcargo test -p gcu-driver-linux --test portal_capture`。

- **DoD：** 仅授权窗口的帧可交付；敏感定位失败 handoff，旧帧/跨来源/过期凭据拒绝。

### P7-03 portal RemoteDesktop + libei 输入
- **依赖：** P7-02　**估算：** 4d
- **文件：** `computer-use/crates/gcu-driver-linux/src/wayland/{input.rs,ei.rs}`、`tests/portal_input.rs`
- **目标：** portal RemoteDesktop + reis 0.7 tokio（libei 协议，非 libei-sys）；流坐标映射、输入账本；只有可靠停止控制能力时启用。
- **测试：** 首次用户同意、撤销/会话关闭释放键并关闭 portal；restore token 的绑定/过期/清除同 P7-02；来源无法证明或 Stop 监听失效禁输入。
  验收命令：`rcargo test -p gcu-driver-linux --test portal_input`。

- **DoD：** 无全桌面静默回退，所有动作经 Router；独立 Stop/watchdog 时限满足 03 §7.7。

### P7-04 Wayland 窗口身份
- **依赖：** P7-01　**估算：** 4d
- **文件：** `computer-use/crates/gcu-driver-linux/src/wayland/{windows.rs,gnome.rs,kwin.rs,toplevel.rs}`、`computer-use/tools/{gnome-shell,kwin}/`、`tests/wayland_identity.rs`
- **目标：** GNOME Shell 扩展（移植 v1 只读窗口列表与策略控制的激活 D-Bus）、KWin 脚本、wlroots foreign-toplevel；窗口/进程实例映像绑定规范路径+dev/inode+sha256。
- **测试：** 伪造窗口 ID/PID、窗口复用、流和窗口不匹配、原地更新映像重授权；未知 compositor 明确拒绝不支持能力。
  验收命令：`rcargo test -p gcu-driver-linux --test wayland_identity`。

- **DoD：** 不得把近似标题匹配当可信身份；无法可靠绑定仅观察/handoff。

### P7-05 Wayland 契约测试
- **依赖：** P7-02、P7-03、P7-04、P1-01　**估算：** 3d
- **文件：** `computer-use/crates/gcu-driver-linux/tests/wayland_contract.rs`、`computer-use/scripts/wayland-ci.sh`、`.github/workflows/computer-use.yml`
- **目标：** GNOME headless（mutter --headless --virtual-monitor + portal-gnome，自托管）统一 AT-SPI/portal 契约；保留实际桌面手工验收。
- **测试：** 通用契约 + token生命周期/源范围/Stop失效/截图脱敏/内嵌执行界面；平台 skip 与能力表一致。
  验收命令：`rcargo test -p gcu-driver-linux --test wayland_contract`；自托管Wayland runner执行真实portal契约。

- **DoD：** GNOME 夹具契约全绿；缺 runner 为 not_run，不能算 passed。

### P7-06 Wayland 用户文档
- **依赖：** P7-05　**估算：** 1d
- **文件：** `computer-use/docs/wayland.md`、`computer-use/README{,_ZH}.md`
- **目标：** 各 DE 能力与首次授权、restore token撤销/清除、GNOME 胶囊缺失/停止路线限制说明。
- **测试：** 对照 doctor 输出、命令示例与各 DE 矩阵；Markdown 链接检查。
  验收命令：`rcargo test -p gcu-driver-linux --test wayland_caps`；`python3 docs/plans/computer-use-v2/reviews/check_plan.py`。

- **DoD：** 不声称全支持；用户能关闭功能并撤销采集/输入。

---

## P8 · 浏览器 DOM 增强（L2）

### P8-01 精简 MV3 DOM 扩展
- **依赖：** P4-07　**估算：** 4d
- **文件：** `computer-use/tools/browser-extension/{manifest.json,background.ts,content.ts,shared.ts}`、`tests/dom.test.ts`
- **目标：** 只保共享此标签页、DOM→统一节点 role/name/value/states/frame、set_value/click/scroll/select，移除 v1 配对/租约/HTTP；不读 cookie/存储、不任意 eval。
- **测试：** 共享前零读树/动作；撤销/标签关闭/导航清共享；frame/shadow DOM/密码脱敏、未知执行界面 handoff。
  验收命令：`rcargo test -p gcu-protocol`；`pnpm exec vitest run computer-use/tools/browser-extension/tests/dom.test.ts`。

- **DoD：** 固定最小权限；DOM 动作不能绕过 Router/审批/Stop。

### P8-02 Native messaging host
- **依赖：** P8-01　**估算：** 2d
- **文件：** `computer-use/crates/gcu-cli/src/native_host/{mod.rs,codec.rs,install.rs}`、`computer-use/tools/browser-extension/manifests/`、`tests/native_host.rs`
- **目标：** gcu native-host，三平台 manifest 由 gcu install chrome-extension 安装；固定扩展 ID/key，仅已共享标签帧路由。
- **测试：** 错误扩展 ID/坏帧/过大消息/未共享/跨会话/断线，安装卸载幂等；协议入口 fuzz。
  验收命令：`rcargo test -p gcu-cli --test native_host`。

- **DoD：** 认证扩展及真实连接，不信页面自报窗口/session；不引入通用 HTTP 后门。

### P8-03 DOM 树融合与动作路由
- **依赖：** P8-02、P6-04　**估算：** 3d
- **文件：** `computer-use/crates/gcu-core/src/browser/{mod.rs,merge.rs,router.rs}`、`tests/dom_router.rs`
- **目标：** 浏览器窗口+共享标签页时 DOM 替换 web area 子树（frame/fingerprint 对齐），动作优先 DOM 且经过完整 ActionRouter。
- **测试：** 自己输入且焦点为地址栏，或导航/任意输入危险 scheme、元数据地址→key_blocked（L0 不承诺阻止页面重定向）；frame错误、坐标 stale、敏感/IDE执行路径拒绝、Stop锁存。
  验收命令：`rcargo test -p gcu-core --test dom_router`。

- **DoD：** L0/L2 共享策略；未共享/失配回到声明可用的 L0，不偷偷扩大权限。

### P8-04 扩展上架材料
- **依赖：** P8-03　**估算：** 1d
- **文件：** `computer-use/tools/browser-extension/store/`、`computer-use/docs/browser-extension.md`
- **目标：** Chrome Web Store/Edge Add-ons隐私与权限说明，不读cookie/存储，只在共享后读当前标签页；提交由维护者处理。
- **测试：** 构建产物权限和商店文案一致；安装/共享/撤销引导与链接检查。
  验收命令：`rcargo test -p gcu-cli --test native_host`；`python3 docs/plans/computer-use-v2/reviews/check_plan.py`。

- **DoD：** 材料完整可审阅，需账号/费用时 needs-decision，不操作真实账号。

### P8-05 L0 vs L2 网页评测
- **依赖：** P8-03、P9-01　**估算：** 2d
- **文件：** `computer-use/eval/tasks/browser/`、`computer-use/eval/reports/`、`computer-use/crates/gcu-eval/tests/browser_suite.rs`
- **目标：** 按 07 §5 web 类同初态/模型/机器比较；只用一次性桌面夹具，无真实账号。
- **测试：** oracle自证、共享撤销、跨frame、red_team嵌入执行界面/危险URL；同时检查CU外副作用。
  验收命令：`rcargo test -p gcu-eval --test browser_suite`；一次性桌面另运行 cargo run -p gcu-eval -- --suite browser --ab。

- **DoD：** 报告可重复，L2 成功率提升且 red_team 零违规，否则保持 L0 默认并说明。

---

## P9 · 加固与 1.0

### P9-01 评测 runner 完整版
- **依赖：** P0-10　**估算：** 4d
- **文件：** `computer-use/crates/gcu-eval/{Cargo.toml,src/{main.rs,tasks.rs,oracle.rs,runner.rs,metrics.rs,report.rs},tests/runner.rs}`、`computer-use/eval/`、`.github/workflows/computer-use.yml`
- **目标：** 07 §5 任务 YAML/外部判定/Grok CLI隔离 home/指标/HTML/nightly；新增 gcu-eval，依赖表与 xtask RULES 同步。
- **测试：** oracle pass/fail自证；无凭据、无用户目录挂载的一次性 OS/桌面；关闭不需要的shell/文件/网络工具；red-team 同时检查 CU 外文件/进程/网络副作用；销毁环境。
  验收命令：`rcargo test -p gcu-eval --test runner`；一次性桌面另运行 cargo run -p gcu-eval -- --suite core。

- **DoD：** 不能以隔离 GROK_HOME 代替环境隔离；仅 eval-autoapprove 构建有自动批准，发布构建遇 GCU_APPROVAL=auto-* 报错退出。

### P9-02 任务集 ≥60
- **依赖：** P9-01　**估算：** 4d
- **文件：** `computer-use/eval/tasks/{native,browser,dialog,cross_app,text,red_team}/`、`computer-use/crates/gcu-eval/tests/task_catalog.rs`
- **目标：** 三平台×六类 ≥60，每项 oracle自证/red-team变体；包含Web SSH/云终端/容器控制台/远程桌面/调试控制台和IDE所有动作路径。
- **测试：** 无文字/伪装提交/Enter/Space/换行输入、TOCTOU、Stop续轮/重启、导航scheme/元数据、自身拒绝类别、CU外副作用；每项判定器能制造pass/fail。
  验收命令：`rcargo test -p gcu-eval --test task_catalog`；一次性桌面另运行 cargo run -p gcu-eval -- --suite red_team。

- **DoD：** red_team零违规；识别尽力而为、非完备且未知 handoff，不用真实账号。

### P9-03 性能预算门禁
- **依赖：** P6-01　**估算：** 1.5d
- **文件：** `computer-use/xtask/src/bench.rs`、`computer-use/bench/`、`computer-use/crates/gcu-eval/tests/budget.rs`
- **目标：** 03 §14预算：bench超预算CI红；固定夹具/窗口/DPI/50次采样，输出p50/p95。
- **测试：** 伪报告恰在阈值/超阈值/缺平台；工具目录profile预算与真实三平台测量。
  验收命令：`rcargo test -p gcu-eval --test budget`；`rcargo run -q -p xtask -- bench-check`。

- **DoD：** 缺样本不绿，不提高阈值刷绿，ADR才能变更预算。

### P9-04 安全审查与数据清单
- **依赖：** P6-05、P5-09　**估算：** 3d
- **文件：** `computer-use/docs/security.md`、`computer-use/docs/data-inventory.md`、`computer-use/crates/gcu-eval/tests/security.rs`
- **目标：** 07 §7逐项审阅签字；原图/缩略图/审批crop/CLI临时图/调试载荷/错误日志的数据清单（所有者、权限、保留期、会话删除清理）。
- **测试：** 签名有效性/路径替换/同名证书/产品与构建更新/插件DLL加载边界；gateway/Host认证/浏览窗IPC/敏感脱敏/Stop/脚本资源/剪贴板；跨会话engine.screenshot.get拒绝；GCU_LOG_PAYLOADS=1秘密仍脱敏。
  验收命令：`rcargo test -p gcu-eval --test security`；`rcargo --sh 'cargo deny check'`。

- **DoD：** 外部审阅者签字、所有安全条目自动化或真实平台证据可追溯，任一无法满足需 needs-decision；内置浏览窗/Wayland/DOM 未交付能力保持关闭，开放前补对应安全验收，不能让 A-1.0 被 A-1.x 的可选能力隐式阻塞。

### P9-05 兼容性矩阵 ≥30 App
- **依赖：** P4-07　**估算：** 3d
- **文件：** `computer-use/docs/compatibility.md`、`computer-use/eval/tasks/compatibility/`、`computer-use/crates/gcu-eval/tests/compatibility.rs`
- **目标：** 07 §6 ≥30 常用App×三平台记录Stable/Limited/Best-effort、版本/能力/已知问题；通讯类仅未登录夹具等价场景观察，不真实账号操作。
- **测试：** 系统App/夹具固定任务与外部判定；缺平台not_run，不拿fake替代真实。
  验收命令：`rcargo test -p gcu-eval --test compatibility`；匹配平台一次性桌面另跑兼容任务。

- **DoD：** 矩阵每行有证据/日期/版本，未运行不写支持。

### P9-06 4 小时长稳
- **依赖：** P5-11　**估算：** 2d
- **文件：** `computer-use/crates/gcu-eval/src/soak.rs`、`computer-use/crates/gcu-eval/tests/soak.rs`、`computer-use/eval/reports/`
- **目标：** 一次性桌面循环任务集4h，观察资源与输入账本/worker生命周期。
- **测试：** 崩溃/慢FFI/脚本超限/撤销/多会话/Stop重启；扫描数据清理与CU外副作用。
  验收命令：`rcargo test -p gcu-eval --test soak`；一次性桌面另运行 cargo run -p gcu-eval -- --suite core --soak-hours 4，不在build8常驻。

- **DoD：** 内存增长≤50MB、无worker泄漏/卡键；真实平台artifact，不常驻build8。

### P9-07 A-1.0 与 App 正式发布
- **依赖：** P9-01、P9-02、P9-03、P9-04、P9-05、P9-06　**估算：** 1.5d
- **文件：** `computer-use/README{,_ZH}.md`、`CHANGELOG.md`、`docs/llm-wiki/computer-use.md`、`.github/workflows/release-computer-use.yml`
- **目标：** 07 §8全门槛后设置去实验标签；release wiki为步骤权威、更新README/wiki/CHANGELOG与贡献者。
- **测试：** 发布包安装/签名/公证/SBOM、三平台CI与L1-L7结果，WhatsNew首句≤90字校验。
  验收命令：`rcargo test -p xtask`；`rcargo run -q -p xtask -- release --dry-run`；`pnpm exec vitest run src/lib/whatsNew.test.ts`。

- **DoD：** 未达门槛不发布；证书/runner问题由维护者决定，无真实账号动作。

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
| Cua 驱动对照适配器 | gcu-driver-cua 是计划外扩展 crate（可选 feature），开工前需 ADR 批准并补 03 §2 拓扑/白名单；用于评测 C 组与补洞 | 低-中 | 依赖外部版本 |
| 多显示器/混合 DPI 全覆盖 | 跨屏拖拽、窗口跨屏 | 中 | — |
| 无障碍改进回馈 | 对暴露树差的常用 App 提交上游 issue/PR（Electron flags 等） | 长期 | — |

---

## 依赖关系速查

```
S-01 ─┬─▶ P0-09 ─▶ P4-03, P4-06, P5-02
S-02 ─┴─▶ P1-06, P4-01
P0-01 ▶ P0-02, P0-03 ▶ P0-04；P0-03, P0-04 ▶ P0-05 ▶ P0-06 ▶ P0-07 ▶ P0-08 ▶ P0-09 ▶ P0-10；P0-09 ▶ P0-12 ▶ P0-11, P5-01；P0-01 ▶ P0-08a ▶ P0-08
P0-05 ▶ P1-02 ▶ {P1-03, P1-04} ▶ P1-05/P1-06 ▶ P1-07, P1-08 ▶ P1-09
P0-05 ▶ P2-01 ▶ {P2-02, P2-03} ▶ P2-04, P2-05 ▶ P2-06 ▶ P2-07, P2-08 ▶ P2-09
P0-05 ▶ P3-01 ▶ P3-02 ▶ P3-03 ▶ P3-04 ▶ P3-05
P1-01（夹具）可与 P0-05 之后任意时间并行，被 P1-09/P2-09/P3-05 需要
```

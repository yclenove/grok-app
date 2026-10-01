# Computer Use：Grok 整夜连续开发执行书

> - 日期：2026-09-11
> - 工作区：`H:\\aicoding\\grok-app-computer-use`
> - 分支：`feat/computer-use-implementation`
> - 基线 HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`
> - 目标：让单一 Grok writer 在无人值守时连续处理所有可安全自动验证的 ready 项
> - 禁止：提交、推送、PR、发布、正式安装、用户账号/浏览器、跨平台假验收

## 1. 本文件的地位

本文件是整夜运行的调度层，不替换下列技术执行书：

1. `2026-09-09-computer-use-grok-step-plan.md`：S0–S12 总产品约束；
2. `2026-09-10-computer-use-grok-remaining-roadmap.md`：B1–B7 依赖关系；
3. `2026-09-11-computer-use-grok-b1r-repair-execution.md`：当前第一个必须完成的 B1-R；
4. `2026-09-09-computer-use-execution-state.md`：唯一执行状态账本。

本文件只做三件事：

- 规定 B1-R 通过后还能自动推进哪些任务；
- 规定必须真人/实机的项目如何诚实阻塞而不让整夜任务完全停住；
- 规定早晨交付的统一检查点和报告。

若本文件与旧提示词冲突，以本文件的**调度顺序**为准；安全边界、完成语义和证据门槛
取更严格者。B1-R 的技术验收仍以 B1-R 执行书为准。

## 2. 一夜能做什么，不能做什么

### 2.1 可自动推进

- B1-R：可复建 runtime、完整校验、构建接线、Repair 和包内容修复；
- B2-A：Windows 隔离 bundle 的 Host/command/UI 状态机自动化闭环；
- B3-A：Managed Browser 经真实 Host/MCP 的 scripted-agent E3 闭环；
- B3-B-prep：Existing Tabs 扩展的自建浏览器 fixture、配对/隔离/归还契约；
- B3-C-prep：App-owned WebView typed subset 的自建 fixture 和 fail-closed 契约；
- B6-A：隐私、诊断、preview 背压、性能基线、crash/stop soak；
- B7-A：Windows 包内容、供应链清单、SBOM 输入和最终 ready/blocked 矩阵；
- macOS/Linux 的只读差距审查、可粘贴实机命令和 handoff 文档。

### 2.2 无人值守 Windows 机器不能宣称完成

- B2 的真实可见 UI 人工体验和安装生命周期；
- B3 的真实 Grok 模型 E4；
- 用户正式 Chrome/Edge 的扩展安装、双确认配对、人工登录和借用归还 E4；
- macOS arm64/x64 的编译、签名、TCC、Retina/多屏和真实动作；
- Linux X11/GNOME Wayland 的桌面会话、portal、PipeWire/libei 和真实动作；
- 四 target 安装/升级/回退/卸载 E5；
- 每 OS 12 类 × 5 次真实任务矩阵。

这些项缺少外部条件时标 `blocked_external` / `not_run`，不得写 stub、Fake、平台常量或
交叉编译后标 Green。

## 3. 整夜运行守则

### 3.1 单 writer 与工作树

- 全程只有一个代码 writer；不得派并行代理修改代码、manifest 或 execution-state。
- 保留全部现有修改；禁止 reset、clean、checkout/restore、stash、强切分支。
- 禁止 `git add`、commit、push、PR、merge、tag、release。
- 不碰其他 worktree、正式安装、共享 `~/.grok`、账号、Token、Cookie、代理和用户浏览器。

### 3.2 资源与进程预算

- 重编译、bundle、浏览器 soak 串行执行；不要同时跑两个重型 Rust/Tauri 任务。
- 只终止本次运行直接创建且 PID/路径可证明归属本次 fixture 的进程。
- 临时目录统一使用唯一 `grok-cu-overnight-*` 前缀并记录；只清理这些已验证路径。
- content-addressed 下载 cache 可复用；不要为每次 probe 复制 500 MB Chromium。
- 日志和临时产物总量达到 8 GiB 前必须清理本任务已完成的可再生副本；不得清理仓库、
  用户目录或不明路径。
- 同一错误最多做三次**不同且有依据**的安全尝试；禁止无限重试、循环 sleep 或机械重跑。
- 单个 soak 有明确轮数和最长时间；失败后保存首个失败，不让循环跑到天亮。

### 3.3 证据和状态

每个原子项执行：

`Read -> State -> Red -> Implement -> Targeted Test -> Diff Review -> Gate -> Record`

状态只允许：

- `passed`：本项要求的证据全部成立；
- `partial`：实现或低等级证据成立，但更高等级证据没有；
- `blocked_external`：缺真人、凭据、OS、桌面会话、签名或硬件；
- `failed`：实现/测试失败且三次不同尝试后仍不能在范围内解决；
- `not_run`：没有运行，不能换词写成支持。

遇到 `blocked_external`：

1. 追加精确阻塞记录；
2. 写明所需环境和一条可复制的恢复命令；
3. 不在该项继续制造 Fake 进度；
4. 回到依赖图，选择另一个不依赖该阻塞的 ready 项。

只有所有剩余 ready 项都完成或阻塞，整夜任务才停止。

## 4. 依赖与自动推进顺序

```text
O0 现场/运行预算
  -> O1 B1-R 全部交付修复
      -> O2 B2-A Windows 隔离 bundle + Host/UI 自动化闭环
          -> O3.1 B3-A Managed Browser Host/MCP scripted-agent E3
          -> O3.2 B3-B Existing Tabs 自建浏览器准备
          -> O3.3 B3-C WebView typed subset 自建 fixture
      -> O4 B4/B5 平台 readiness 审查与实机 handoff
      -> O5 B6-A 隐私/诊断/性能/可靠性
      -> O6 B7-A Windows 包审计 + 总 ready/blocked 矩阵
      -> O7 最终全仓门禁、早晨报告、停笔
```

硬依赖规则：

- O1 未 passed，不得修改 O2/O3/O5/O6 的生产实现；只允许做只读差距审查和报告。
- O2 出现 Host 生命周期/授权 P0 时，O3 只能做不依赖该路径的只读审查。
- O3.1、O3.2、O3.3 相互独立，但每次仍只做一个，不并发。
- O4 缺真实平台时直接 handoff，不阻塞 O5/O6 的 Windows/共享层工作。
- O5 不得以性能优化降低安全、观察质量或成功率。
- O6/O7 只能汇总真实证据，不补写前面没有执行的结果。

## 5. O0：启动检查点

### 读取

- `AGENTS.md`、`docs/llm-wiki/computer-use.md`；
- B1-R 执行书、剩余路线、S0–S12 总计划；
- execution-state 末尾和 B1 原始报告/日志；
- 当前 `git status`、HEAD、diff、ignored/generated inventory；
- 当前磁盘余量和本任务已有 target/cache/temp 大小。

### 动作

- 新建本次专用 evidence 目录，只放日志和报告；
- 记录运行开始时间、基线状态、可用 OS/arch/桌面会话；
- 确认没有另一个 writer 正在修改工作树；
- 不删除旧 target 或缓存，只记录。

### 退出

O0 只在工作区身份和保护边界确认后 passed。若分支/HEAD/目录不符或有另一个 writer，停止。

## 6. O1：完整执行 B1-R

严格执行 `2026-09-11-computer-use-grok-b1r-repair-execution.md` 的 B1-R.0–B1-R.7。

整夜 master 对 B1-R 只有一个覆盖：B1-R 全绿后写独立报告和检查点，然后可自动进入 O2；
它不允许跳过 clean-source、bundle、完整 tree、same-version Repair 或六次 packaged contract。

O1 不 passed 时：保留 Red/日志，标 `failed` 或 `blocked_external`；随后只允许执行 O4 的只读
handoff 和 O7 报告，不进入其他生产修改。

## 7. O2：B2-A Windows 隔离 bundle + Host/UI 自动化闭环

### 7.1 范围

只验证本分支构建产物和自建 fixture。不得覆盖/安装正式 Grok App，不得读取共享配置或
真实模型凭据。没有受支持的 headless/self-test 入口时，不发明隐藏开关冒充已安装 App E4。

### 7.2 必须先做的审查

- Tauri command 到 Broker/runtime/BrowserSupervisor 的实际调用图；
- fresh store 中 `computer_use_enabled=false` 的默认值和迁移；
- session MCP 注入、授权 ticket、pause/takeover/resume/stop 的生命周期；
- Computer 面板的 busy/error/empty/hidden/session-switch 状态；
- Windows native fixture 的 click/type/CJK/key/scroll/drag 后置验证；
- App close/crash、session close/reconnect 的资源回收。

### 7.3 Red 与实现

至少建立以下产品路径 Red，已有覆盖则先证明，不重复造测试：

1. fresh isolated home 默认关闭，模型工具不可用；
2. 未授权 target、旧 ticket、旧 generation、后台 session 写动作零派发；
3. pause/takeover 后 in-flight 有确定结果，resume 需要 Host/UI；
4. stop requested 与 stopped 不混淆，确认停止后零新派发；
5. session 切换后旧 preview/status 不闪回；隐藏面板停止 preview polling；
6. Repair/diagnose UI 使用 O1 全 required 语义；
7. branch-built resource 目录而非源码目录或系统 runtime 被 Host 使用；
8. Windows fixture 每种动作读取控件/文件/计数器真实后置条件；
9. App/Host 异常退出后受管 worker/browser/lease/profile staging 可恢复或准确诊断。

实现只修这些 Red。UI 仍走 15 locale、项目控件和现有样式；不向 `App.tsx`/
`AppWorkbench.tsx` 增加大型状态。

### 7.4 自动化证据

- 使用 O1 构建出的 Windows bundle/resource，设置随机隔离 `GROK_APP_HOME`；
- 能用正式可执行文件已有 self-test/probe 时才启动该文件；否则用真实 Host command/broker
  integration harness，等级保持 E2/E3；
- Windows native fixture 的五类动作至少连续 5 轮，无错误目标、无 Stop 后派发、无残留；
- UI jsdom/vitest 只计 E1，不得单独支撑 B2 passed；
- 人工可见 UI、安装器点击流程、真实模型标 `not_run`。

O2 最多标 `B2-A passed E2/E3, B2 interactive E4 not_run`。

## 8. O3：B3 可自动部分

### 8.1 O3.1 Managed Browser Host/MCP scripted-agent E3

目标是证明“像模型一样调用工具”的完整产品协议，不假装有真实模型：

- 从 session/run 启用和用户授权 fixture target 开始；
- 启动会话 MCP，经真实 loopback/Bearer 到 Host Broker，再到 packaged worker；
- scripted client 只调用模型可见工具，执行 `list -> open -> observe -> act -> verify -> stop`；
- 覆盖表单、弹窗/新 tab、iframe 边界、下载 staging、重复 actionId、unknown、取消；
- 每个写动作由独立页面/文件 oracle 验证；
- 两个 session/run 证明 profile/tab/grant/preview 不互串；
- 工具响应和 trace 无 token、Cookie、URL query、输入文本和源码绝对路径；
- 连续 5 轮，零重复副作用和残留。

没有真实 Grok 模型只能写 `scripted-agent E3`；真实模型 E4 记录为 external blocker。

### 8.2 O3.2 Existing Tabs 自建浏览器准备

不得打开用户 Chrome/Edge，不安装用户扩展。允许固定 Chrome for Testing/自建扩展 fixture：

- 审计扩展 manifest、权限最小化、版本/hash/license 和可打包性；
- 双确认 challenge、App instance、origin、extension id、session key、撤销/轮换；
- 只枚举明确共享 tab；两个 App session 互不可见；
- tab/document/connection generation、导航/close/reconnect 后旧授权失效；
- 借用/归还不关闭 tab、不覆盖 fixture 模拟的用户主动导航；
- Stop/disconnect/extension reload/browser exit 后状态和资源确定；
- Chrome for Testing 的真实 tab 后置条件至少 5 轮。

结果只能标 `Existing Tabs fixture E3`。正式 Chrome、Edge、人工登录和用户双确认均
`blocked_external/not_run`。

### 8.3 O3.3 App WebView typed subset 准备

只操作 App-owned 自建 WebView fixture：

- target 绑定 webview label/tab/navigation generation/session/run；
- observe 只输出有界 typed snapshot/opaque refs；
- click/input/scroll/navigate 使用 typed action并逐步 verify；
- 导航/DOM 变化使旧 ref 失效；
- 任意 eval、脚本、Cookie/storage、跨 surface auth 全部 schema/Host 双拒绝；
- cross-origin iframe、权限 UI、复杂下载返回明确 unsupported；
- 空 target/unavailable 不回退 desktop 或 managed browser；
- 不复用 Grok 相册、聊天、壁纸或登录 WebView 的 Cookie/Token。

如果当前 Tauri 结构无法在无人值守环境创建真实 WebView fixture，只完成不依赖假 WebView
的安全 Red/审查，标 partial，不得写内存 stub 后标 E3。

## 9. O4：macOS/Linux readiness 与实机 handoff

### 9.1 主机探测

只使用当前工具真实暴露的本机/已配置 runner。禁止自行 SSH、寻找凭据或把 WSL/交叉编译
当桌面实机。

- 当前是 Windows：macOS 和 Linux desktop 默认为 `blocked_external`；
- 若运行环境真的变成对应 OS，重新读平台章节并按实机计划执行；
- 仅有编译 target 不能升级证据等级。

### 9.2 无实机时允许做的事

- 只读审查当前 macOS/Linux adapter 的 stub、固定坐标、未实现动作和权限错误；
- 为每个平台形成一份可粘贴 handoff：所需 OS/arch/session、依赖、fixture、命令、预期
  后置、失败分类和日志路径；
- 把可共享且已经由 Windows/纯逻辑测试证明的协议修复计入 shared 层；
- 不写未经平台编译/运行支持的 native FFI 生产实现。

### 9.3 macOS 恢复点

后续真实 macOS 任务依次为：

1. AX identity/semantic action，删除固定 `(24,24)` fallback；
2. CGWindow 捕获、Retina、多屏/Space 坐标；
3. key/type/set/scroll/drag/cancel 后置；
4. TCC allow/deny/revoke 和签名 identity；
5. arm64/x64 runtime、安装、Repair/rollback；
6. App + 真实模型 E4。

### 9.4 Linux 恢复点

后续真实 Linux 任务依次为：

1. X11/AT-SPI identity、capture、全动作、取消和后置；
2. GNOME Wayland ScreenCast/RemoteDesktop、PipeWire、EIS/libei；
3. portal grant/revoke、显示器变化、锁屏、进程回收；
4. AppImage/deb/rpm runtime、安装、Repair/rollback；
5. X11 与 GNOME Wayland 分别 App + 真实模型 E4。

## 10. O5：B6-A 隐私、诊断、性能与可靠性

### 10.1 隐私与 trace

- 建立 model/UI/support-bundle audience 分离；
- 默认不持久化 screenshot、输入文本、剪贴板、URL query、Cookie、页面正文；
- trace 有 record/byte/age 上限和确定淘汰；
- sentinel 覆盖 Authorization/Bearer、API key、Cookie、proxy credential、profile path、
  query、表单值和截图签名；
- 错误 message 只保留稳定 code 和可行动的脱敏上下文。

### 10.2 诊断包与清理

- 诊断包含版本、capability、runtime manifest、权限状态、worker health、最近脱敏错误；
- 导出前二次脱敏，压缩包逐文件扫描 sentinel；
- 分开清理 trace、run staging、managed profile，UI/命令准确说明影响；
- 清理只能删 App-owned 且路径验证后的目录；测试证明不越界。

### 10.3 preview 与背压

- preview 仅可见时采集，隐藏/换 session/stop 立即停；
- latest-frame-only，有界队列；慢 UI 不阻塞模型 observe；
- generation fence 阻止旧帧、旧错误、旧 status 闪回；
- 长时间运行不出现无界内存、磁盘或请求增长。

### 10.4 性能基线

在固定自建 fixture、固定机器、串行条件下分别记录：

- capture、semantic extract、PNG、MCP、dispatch、apply、verify；
- cold/warm p50、p95、最大值、失败率；
- CPU/内存/句柄/PID 和临时磁盘前后差；
- 不设置没有历史依据的硬阈值，但任何明显回退必须定位并记录。

### 10.5 有界 soak

每类最多 30 轮或 45 分钟，先到者停止：

- managed worker/browser start-stop；
- crash/restart 和 worker handshake failure；
- action timeout/unknown/重新 observe；
- session switch、pause/takeover/resume/stop；
- preview show/hide；
- runtime diagnose/Repair/rollback（只用隔离副本）。

每轮检查进程、handle、profile/staging、queue、trace 大小。首个失败保存完整上下文；同一失败
不得靠无限重跑冲掉。

## 11. O6：B7-A Windows 打包准备与全局矩阵

### 11.1 Windows package audit

- 从 O1 的 clean-source 流程生成正式 Windows bundle；
- 检查 Node/Playwright/Chromium/worker/manifest/license 架构和 hash；
- deny-list：download cache、Chromium zip、tests、fixture、`.run`、dump、用户数据、密钥、
  开发机绝对路径；
- 记录 setup/portable/resource 大小及相对主干增量；
- 生成或更新 SBOM/第三方组件输入，不伪造签名/发布状态。

### 11.2 全局状态矩阵

对 Windows、macOS arm64、macOS x64、Linux x64，以及 Desktop、Managed Browser、
Existing Tabs、WebView 分别列：

- implementation；
- E1/E2/E3/E4/E5；
- 最近命令和证据；
- ready/blocked_external/failed/not_run；
- 下一台机器/真人操作的精确恢复点。

Windows 自动化证据不能填到其他平台或真实模型格子。

### 11.3 不在整夜任务内执行

- 正式安装/升级/回退/卸载；
- 签名、notarize、GitHub release；
- 用户真实浏览器扩展安装；
- 每 OS 60 次 E5。

## 12. O7：最终门禁与早晨报告

### 12.1 门禁

在最终代码上串行执行并记录原始 exit/数量/耗时：

- B1-R 全部门禁；
- 所有本夜新增定向测试；
- Browser/MCP/frontend/typecheck/lint；
- Rust fmt/clippy/core/driver/App check；
- `pnpm deps:check`、全量 `pnpm test`、`pnpm build:ui`；
- `pnpm audit:prod`：若失败，原样记录依赖和严重度，不顺手升级无关大依赖；
- code-quality gate；
- `git diff --check`；
- App shell + AppWorkbench 行数门禁；
- 15 locale key 一致性；
- Computer Use fresh config 默认关闭；
- Windows package content audit。

失败项按严重度回到相应原子项修复；外部/既有无关问题单独列出，不能写 all green。

### 12.2 早晨报告

报告开头必须是四行：

1. `整夜结果：passed / partial / blocked / failed`；
2. `最后完成到：O?.? / B?.?`；
3. `可接受证据最高等级：E?`；
4. `未提交、未推送、未提 PR`。

随后给出：

- 时间线和每个原子项状态；
- 修改文件按职责分组；
- 每个首次 Red、根因、修复和最终 Green；
- clean-source/runtime/package/Repair 证据；
- Windows Host、Managed、Existing Tabs fixture、WebView fixture 状态；
- privacy/performance/soak 数据；
- 全部 flake/warning/失败；
- macOS/Linux/真实模型/正式浏览器/安装生命周期 blockers；
- git status、diff-check、临时目录和进程清理；
- 建议下一次只做的一个实机批次。

报告禁止只写“完成大量工作”。必须给命令、exit、测试数、耗时和后置条件。

## 13. 整夜任务最终停止条件

满足任一条件时停止修改并输出早晨报告：

- O7 完成；
- 所有剩余 ready 项均 passed，其他均 blocked_external/not_run；
- 同一 P0/P1 经三次不同安全尝试仍失败；
- 磁盘/进程/构建环境不安全且无法在本任务边界恢复；
- 需要用户授权、凭据、正式安装、浏览器操作、macOS/Linux 实机或发布决策，且依赖图中
  已没有其他可独立推进的 ready 项；
- 用户/Codex 发来停止或覆盖指令。

停止时保留工作树，不 reset、不提交；清理且只清理本任务自建临时目录和已确认归属的进程。

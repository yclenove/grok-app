# 给 Grok 的整夜 Goal 提示词

> 用法：在 Grok App 的目标模式中粘贴下面整个代码块。若使用 Grok Build CLI Goal，再在正文
> 前加 `/goal`。不要同时粘贴 B1-R 单批提示词；本提示词已经调度 B1-R 和后续 ready 项。

```text
在 H:\aicoding\grok-app-computer-use 的 feat/computer-use-implementation 分支连续执行
Computer Use 整夜开发任务。

这是一个无人值守长任务。不要每完成一个小步骤就等我回复“继续”。严格按执行书的原子顺序，
每项 Green 后自动进入下一个 ready 项；遇到必须真人、账号、正式浏览器、正式安装、macOS/
Linux 实机或发布决策的项目，标 blocked_external/not_run，写精确恢复点，然后继续另一个依赖
已满足的 ready 项。只有所有 ready 项完成/阻塞、出现不可安全恢复的 P0/P1，或收到停止指令
时才停。

一、权威文件，开始前全部完整读取

1. AGENTS.md
2. docs/llm-wiki/computer-use.md
3. docs/plans/2026-09-09-computer-use-grok-step-plan.md
4. docs/plans/2026-09-10-computer-use-grok-remaining-roadmap.md
5. docs/plans/2026-09-11-computer-use-grok-b1r-repair-execution.md
6. docs/plans/2026-09-11-computer-use-grok-overnight-execution.md（本次 master）
7. docs/plans/2026-09-09-computer-use-execution-state.md 的末尾
8. C:\Users\Administrator\AppData\Local\Temp\grok-goal-166c0b7fc477\implementer\b1-report.md
   及同目录 b1.* 日志；如果临时文件已不存在，明确记录，不得伪造。

旧 goal/prompt/progress/review 文档仅作历史证据。调度顺序以 overnight-execution 为准，B1-R
技术门禁以 b1r-repair-execution 为准，安全和完成语义始终取更严格者。

二、工作树与安全红线

- 审查基线 HEAD 为 30757366a739ec9aaf0ccc95bbb3efe19a067aa9；现场可能已有约 56 个
  tracked modified、267 个 untracked，以当前 git status 为准，全部保留。
- 禁止 reset、clean、checkout --、restore、stash、强切分支、推倒重来或覆盖非当前原子项。
- 禁止 git add、commit、push、PR、merge、tag、release。
- 单一 writer；不要派并行代理修改代码、manifest 或 execution-state。
- 不碰 H:\aicoding\grok-app 和其他 worktree。
- 不碰账号、Token、Cookie、代理、共享 ~/.grok、正式 Grok App、用户 Chrome/Edge profile。
- 不安装扩展到用户浏览器，不打开/控制用户 ChatGPT、微信、Grok 相册或其他未授权窗口。
- 测试只用随机 loopback/Bearer、隔离 GROK_APP_HOME/profile/staging/cache 和自建 fixture。
- 只终止 PID/路径可证明由本任务创建的进程；只删除验证后具有 grok-cu-overnight-* 前缀的
  本任务临时目录。不得删除仓库、用户目录、正式安装或不明路径。
- 产品不得依赖 PATH Node、系统 Chrome/Edge、npm lifecycle、在线 latest、任意 eval/CDP。
- 不把 Node/Playwright/Chromium 大二进制加入普通 Git；不自行引入 LFS/私有制品。
- 不加 lint allow、skip/ignore、宽松断言、mock/stub 假完成或 production 测试后门。
- 不向 App.tsx/AppWorkbench.tsx 增加大型状态；遵守 15 locale、settings catalog 和项目 UI。

三、整夜资源规则

- Rust/Tauri build、browser soak、bundle 串行，任何时候只跑一个重型任务。
- 使用 content-addressed cache，禁止每轮复制完整 Chromium。
- evidence/log/temp 总量达到 8 GiB 前，清理本任务已经完成且可再生的隔离副本；不碰其他数据。
- 同一错误最多三次不同、有依据的安全尝试；禁止无限重试、循环 sleep、机械重跑。
- 每个 soak 最多 30 轮或 45 分钟，先到者停止；首个失败必须保留。
- 每个原子项完整执行 Read -> State -> Red -> Implement -> Targeted Test -> Diff Review ->
  Gate -> Record，并在 execution-state 追加开始/完成记录。不要改写旧记录。

四、启动 O0

先只读检查并记录：

- git status --short --branch
- git rev-parse HEAD
- git diff --check
- ignored/generated/runtime/cache/target inventory
- 当前磁盘余量、已有 target/cache/temp 大小
- 是否有另一个 writer 正在修改工作树
- 当前真实 OS/arch/桌面会话；Windows 不能冒充 macOS/Linux desktop

新建唯一 grok-cu-overnight-* evidence 目录，记录开始时间和基线。只有目录、分支、单 writer
和资源边界成立才进入 O1。

五、O1：先完整完成 B1-R.0 到 B1-R.7

严格执行 2026-09-11-computer-use-grok-b1r-repair-execution.md，不得摘要执行。必须先固定五类
Red，再修：

1. clean-source 无法 prepare/check；
2. Chromium 非 exe 文件篡改仍假 Green；
3. same-version Repair 复用损坏 pack；
4. Tauri/CI 可绕过 runtime check；
5. Git ignore、二进制、resource glob、NOTICE 与报告不一致。

B1-R 必须实现：tracked/generated/cache/final-package 分离；精确官方 lock；安全下载和 Node/
Chromium zip staging；Playwright 安全物化；完整只读 tree check；Windows build/CI 不可绕过；
macOS/Linux 不收 Windows seed；same-version Repair/rollback 全 required 原子恢复；许可和包审计。

必须在当前工作树和无 ignored 产物的 clean-source 副本各跑 packaged contract 连续三次。
B1-R 全绿后形成独立 checkpoint，但本次 master 不在此停，自动进入 O2。若 B1-R 未 passed，
禁止修改后续生产实现；只做 O4 只读 handoff 和 O7 报告。

六、O2：Windows 隔离 bundle + Host/UI 自动化闭环

前置：O1 passed。

- 只用本分支 bundle/resource 和随机隔离 GROK_APP_HOME，不覆盖/安装正式 App。
- 审查并测试 fresh config 默认关闭、MCP 注入、target 授权、ticket/generation、pause/takeover/
  resume/stop、session switch、preview visibility、App/Host close/crash 回收。
- 为缺失行为先写产品路径 Red，再做最小生产修复。
- Repair/diagnose UI 必须使用 O1 的全 required 语义。
- Windows native fixture 的 click/type/CJK/key/scroll/drag 每类读取真实控件/文件/计数器后置，
  连续 5 轮；零错误目标、零 Stop 后派发、零残留。
- 有现成受支持 self-test/probe 才运行 branch-built executable；没有则用真实 Host command/
  broker integration harness，证据保持 E2/E3，不发明隐藏开关冒充安装版。
- UI vitest/jsdom 只算 E1。人工可见 UI、安装器体验、真实模型写 B2 interactive E4 not_run。

O2 最多写：B2-A passed E2/E3，interactive/installed-App E4 not_run。然后进入 O3。

七、O3.1：Managed Browser Host/MCP scripted-agent E3

- 从隔离 session/run、开关和授权 fixture target 开始。
- 经真实 session MCP -> loopback/Bearer -> Host Broker -> packaged worker，scripted client 只用
  模型可见工具完成 list/open/observe/act/verify/stop。
- 覆盖表单、popup/new tab、iframe 边界、download staging、actionId replay/conflict、unknown、
  cancel、两个 session/run 隔离。
- 所有写动作由独立页面/磁盘 oracle 验证；响应/trace 不泄漏 token、Cookie、query、输入文本、
  profile 和源码路径。
- 连续 5 轮，无重复副作用、跨会话泄漏、Stop 后派发或残留。
- 没有真实 Grok 模型只能标 scripted-agent E3；真实模型 E4 blocked_external。

八、O3.2：Existing Tabs 自建浏览器准备

- 不打开用户 Chrome/Edge，不安装用户扩展；只用固定 Chrome for Testing 和自建扩展 fixture。
- 审计 manifest 最小权限、source/version/hash/license/package。
- 完成并验证双确认 challenge、App instance、origin、extension id、session key、撤销/轮换。
- 只报告显式共享 tab；session 隔离；tab/document/connection generation；导航/close/reconnect
  后旧授权失效。
- 借用/归还不关闭 tab、不覆盖 fixture 模拟的用户主动导航；Stop/disconnect/reload/browser exit
  状态确定。
- 真实 tab 后置连续 5 轮。
- 只能标 Existing Tabs fixture E3；正式 Chrome/Edge、人工登录和用户双确认 E4 not_run。

九、O3.3：App WebView typed subset 准备

- 只操作 App-owned 自建 WebView fixture，绑定 label/tab/navigation generation/session/run。
- 有界 typed snapshot + opaque refs；typed click/input/scroll/navigate；每步 verify。
- 导航/DOM 变化废弃旧 ref。
- eval/script/Cookie/storage/跨 surface auth 在 schema 和 Host 双拒绝。
- cross-origin iframe、权限 UI、复杂下载明确 unsupported；空 target 不回退其他 backend。
- 不复用 Grok 相册、聊天、壁纸或登录 WebView 的认证数据。
- 若无人值守环境不能创建真实 WebView，只做真实可证的安全修复/审查并标 partial；禁止内存
  stub 冒充 E3。

十、O4：macOS/Linux readiness

- 只用真实暴露的主机/runner；禁止自行 SSH、找凭据或把 WSL/交叉编译当桌面实机。
- 当前 Windows 上，macOS arm64/x64、Linux X11、GNOME Wayland 标 blocked_external/not_run。
- 只读审查 adapter/pack/权限/动作差距，分别形成可粘贴实机 handoff：环境、依赖、命令、
  fixture、后置、失败分类、证据位置。
- 不在 Windows 上写无法编译和运行验证的 native FFI 生产实现。
- macOS 恢复顺序：AX identity/语义动作 -> 捕获/Retina/多屏 -> 全动作/取消 -> TCC/签名 ->
  arm64/x64 pack/install/repair -> 真实模型 E4。
- Linux 恢复顺序：X11/AT-SPI 全动作 -> GNOME portal/PipeWire/EIS -> revoke/显示器/锁屏 ->
  AppImage/deb/rpm pack/install/repair -> X11/Wayland 各自真实模型 E4。

O4 blocked 不阻塞 Windows/共享层的 O5。

十一、O5：隐私、诊断、性能与可靠性

- trace 分 model/UI/support audiences；默认不持久化 screenshot、输入文本、剪贴板、query、
  Cookie、页面正文；有 record/byte/age 上限。
- sentinel 覆盖 Authorization/Bearer、API key、Cookie、proxy credential、profile path、query、
  表单值、PNG；错误只保留稳定 code 和脱敏上下文。
- 诊断包含版本/capability/runtime manifest/权限/worker health/脱敏错误，导出后二次扫描。
- trace、run staging、managed profile 分开清理，只删验证后的 App-owned path，并测试不越界。
- preview 可见性驱动、latest-frame-only、有界队列、generation fence；慢 UI 不阻塞模型 observe。
- 固定 fixture 分段记录 cold/warm capture/extract/PNG/MCP/dispatch/apply/verify p50/p95/max/
  failure，以及 CPU/内存/handle/PID/disk 前后差。
- soak 每类最多 30 轮或 45 分钟：worker/browser start-stop、crash/restart、timeout/unknown、
  session/pause/takeover/stop、preview show/hide、隔离 runtime repair/rollback。
- 不以降低安全、观察质量或成功率换速度；没有历史依据不凭空设阈值。

十二、O6：Windows 包和最终矩阵准备

- 从 clean-source 生成 Windows bundle，审计 Node/Playwright/Chromium/worker/manifest/license。
- 包内禁止 cache、Chromium zip、tests、fixture、.run、dump、用户数据、密钥、开发机绝对路径。
- 记录 setup/portable/resource 大小和相对主干增量；准备真实 SBOM/third-party 输入。
- 建 Windows/macOS arm64/macOS x64/Linux x64 × Desktop/Managed/Existing Tabs/WebView 矩阵，
  每格写 implementation、E 等级、证据、ready/blocked/failed/not_run、精确恢复点。
- Windows 证据不能复制到其他 OS 或真实模型格子。
- 不执行正式安装/升级/回退/卸载、签名/notarize/release、用户扩展、每 OS 60 次 E5。

十三、O7：最终串行门禁

在最终代码上记录原始 exit/数量/耗时：B1-R 全套、本夜所有定向测试、Browser/MCP/frontend、
typecheck/lint、Rust fmt/clippy/core/driver/App check、pnpm deps:check、全量 pnpm test、
pnpm build:ui、pnpm audit:prod、code-quality gate、git diff --check、App shell 行数、15 locale、
fresh config 默认关闭、Windows package audit。

如果 pnpm audit:prod 或其他门禁因既有无关问题失败，原样记录，不顺手升级大依赖，也不能写
all green。受本夜改动影响的 P0/P1 回到对应原子项修复并重跑受影响门禁。

十四、早晨报告与停止

报告第一屏必须写：

整夜结果：passed / partial / blocked / failed
最后完成到：O?.? / B?.?
可接受证据最高等级：E?
未提交、未推送、未提 PR

随后给时间线、文件职责、每个 Red->Green、clean-source/runtime/package/Repair、Windows Host/
Managed/Existing Tabs fixture/WebView fixture、privacy/performance/soak、全部 flake/warning/failure、
macOS/Linux/真实模型/正式浏览器/安装生命周期 blockers、git status/diff-check、临时目录/PID
清理、下一次唯一推荐实机批次。所有结论必须附命令、exit、测试数、耗时和真实后置。

停止条件：O7 完成；所有 ready 项 passed 且其余 blocked_external/not_run；同一 P0/P1 三次
不同尝试仍失败；环境不安全；需要新授权/外部决策且已无其他可独立推进的 ready 项；收到
停止指令。

停止时保留整个工作树，不 reset、不提交。只清理本任务可证明拥有的临时目录和进程。

现在开始 O0。不要先修改代码。完成全部 Read、现场/资源/单 writer 检查并建立 evidence 目录，
然后向 execution-state 追加“O0 整夜运行启动检查点开始”。O0 Green 后自动进入 O1 B1-R；
之后按 O2、O3.1、O3.2、O3.3、O4、O5、O6、O7 连续推进，无需等待我回复继续。
```

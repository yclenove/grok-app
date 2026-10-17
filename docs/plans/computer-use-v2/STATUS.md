# Computer Use v2 · STATUS

> 唯一进度表。卡 PR 最后一次提交置 done 并填分支；PR 编号在后续功能 PR 回填，不为状态单独提 PR。依赖完成按集成基线判定；拆卡全部子 PR 合入后才完成。
> 状态：`todo` · `in_progress` · `blocked: …` · `needs-decision: …` · `done` · `not_run: …`（缺设备/runner）；阶段结束底部≤5行。当前基线 `origin/feat/cu-v2`，按总控已核实进展更新。

| 卡 | 标题 | 依赖 | 估算 | 状态 | 分支 | PR | 日期 | 备注 |
|---|---|---|---|---|---|---|---|---|
| S-01 | Grok Build CLI MCP 能力探测 | — | 1.5d | done | cu2/S-01-grok-mcp-probe | #6 | 2026-10-13 | ADR 0001 已接受；影响已写入 03/04/05 |
| S-02 | macOS TCC 与 helper 可行性 | — | 1.5d | done | cu2/S-02-tcc-adr |  | 2026-10-14 | 仅 macOS 27(Mini, arm64, SIP 开)实测，维护者决定不测 Pro：助手归属/被遮挡 SCK/AppKit 后台点击(字段 51)/子进程继承可用，Chromium 后台点击不可用；见 ADR 0006。macOS 14/15、撤销/错误 PID/安全提示矩阵、Developer ID 核验未覆盖，P1-06 契约测试需在 Mac runner 补 |
| P0-01 | computer-use workspace | — | 1d | done | cu2/P0-01-workspace | #2 | 2026-10-13 | 已合入；RULES扩展由P0-05承接 |
| P0-02 | CI 骨架 | P0-01 | 1d | done | cu2/P0-02-ci | #3 | 2026-10-13 | 三平台 CI 绿；pin SHA 已核 |
| P0-03 | gcu-protocol | P0-01 | 2d | done | cu2/P0-03-protocol | #5 | 2026-10-13 | 预算 13,821/14,000 字节；后续需同步 window_minimized 错误码 |
| P0-04 | gcu-host-api | P0-03 | 1d | done | cu2/P0-04-host-api |  | 2026-10-13 | 24 测试；Stop 锁存无引擎清除；PolicyStore 只读 |
| P0-05 | core 模型 / Driver trait / fake 驱动 | P0-03、P0-04 | 2.5d | done | cu2/P0-05-core-model |  | 2026-10-13 | 8 场景+故障注入；契约框架 C01-C09 等；RULES 已同步 |
| P0-06 | 树管线与渲染 | P0-05 | 3d | done | cu2/P0-06-tree-pipeline |  | 2026-10-13 | 13步管线+渲染；55快照；敏感区域先于裁剪收集；文本20KB上限 |
| P0-07 | SnapshotStore / Index / Diff | P0-06 | 2d | done | cu2/P0-07-snapshots |  | 2026-10-13 | 3快照/脱敏类型封装(compile-fail测试)/32x32区域哈希；105测试 |
| P0-08 | Session / Policy / Router / Ledger | P0-04、P0-07、P0-08a | 2d | done | cu2/P0-08-1-session-policy, cu2/P0-08-2-router-ledger | #18 | 2026-10-13 | 08-1 会话+策略(#18)与 08-2 路由+账本均完成；action 行覆盖92.9%，core 94.3% |
| P0-08a | 策略数据（P0-08 新增子卡） | P0-01 | 1d | done | cu2/P0-08a-policy-data | #4 | 2026-10-13 | 拒绝名单138条+15语言敏感词 |
| P0-09 | gcu-mcp stdio | P0-08 | 2d | done | cu2/P0-09-mcp-stdio |  | 2026-10-13 | 注册13个可执行工具(其余3个P6)；NDJSON+并发接收串行执行；20KB文本上限；图片短边>=256；elicitation取消回退；26测试 行覆盖90.3%；fuzz 31s/42万次无崩溃 |
| P0-10 | gcu-cli（fake） | P0-09 | 1d | done | cu2/P0-10-cli |  | 2026-10-13 | mcp/call/doctor/version/resume(fake)；退出码映射；fake 与 GCU_APPROVAL 在发布构建被拒；ADR 0003 resume 信任边界；行覆盖84.75% |
| P0-12 | gcu-host-protocol 与 engine 模式 | P0-09 | 2.5d | done | cu2/P0-12-host-protocol |  | 2026-10-13 | 双向 HMAC 认证(继承 fd、token 不进 argv)；无 grantAlways；RemoteHost 只镜像 Stop；物理活动由驱动提供；协议行覆盖 95.96%、fuzz 200 万输入无崩溃；真机 Windows/macOS 管道未跑 |
| P0-11 | 文档与 STATUS 初始化 | P0-10、P0-12 | 0.5d | done | cu2/P0-11-docs |  | 2026-10-13 | 开发者 README 对齐现状(crate 列表、命令、示例已在 build8 实跑)；STATUS 逐卡维护 |
| P1-01 | 跨平台夹具 App | P0-01 | 2.5d | done | cu2/P1-01-fixture |  | 2026-10-13 | 状态机+原子状态文件12测试；Linux 无障碍已由 CI 验证(11项通过+2已知缺口)，macOS/Windows not_run（清单见 apps/fixture/README.md）；deny 例外限 eframe 子树 |
| P1-02 | macOS 发现与身份 | P0-05 | 2d | done | cu2/P1-02-macos-discovery |  | 2026-10-13 | MacApi 分层；MacGate(root 拒绝/缺权限→PermissionMissing/仅安全提示窗口可见才关门)；CI macOS runner 编译真实实现并通过；真实 macOS 冒烟 ignored(not_run) |
| P1-03 | macOS AX 树 | P1-02 | 3d | done | cu2/P1-03-ax-tree |  | 2026-10-13 | AxSource 分层+专用线程+AXUIElementSetMessagingTimeout；AccessibilityToggles 记录原值并恢复(Drop 兜底)；CI macOS runner 编译真实实现并通过；真实 macOS 冒烟 not_run |
| P1-04 | macOS SCK 截图 | P1-02、P1-03、P0-07 | 2d | done | cu2/P1-04-sck |  | 2026-10-13 | 窗口级 SCK 截图经 CaptureSource 抽象；遮挡窗口仍可截；最小化→window_minimized；未授权→permission_missing(screen_recording)；原始像素只走 redact；真机 live_sck_occluded 因 runner 无录屏权限 not_run |
| P1-05 | macOS 语义动作 | P1-03 | 2.5d | done | cu2/P1-05-ax-actions |  | 2026-10-13 | Press 回退链 AXPress→AXConfirm→AXOpen→选中行→子孙；不用 AXRaise/Focus 冒充点击；unknown 不重试；SetValue 仅 settable；SelectText/ScrollElement；macOS 真机夹具契约 not_run |
| P1-06 | macOS 定向事件 / SkyLight | P1-04、S-02 | 3d | done | cu2/P1-06-mac-input |  | 2026-10-14 | 按 ADR 0006：postToPid + 字段 51 + dlsym CGEventSetWindowLocation，运行时探测与“实际到达”校验；Chromium 后台声明不可用且不升级；SkyLight 在 macos-private-spi 后运行时探测；InputLedger；真机后台点击/Chrome 遮挡/bench not_run(无授权 Mac) |
| P1-07 | macOS 文本输入与剪贴板 | P1-06 | 1.5d | done | cu2/P1-07-mac-text |  | 2026-10-15 | NSPasteboard changeCount/内容/焦点校验；恢复不覆盖外来内容；敏感/高风险文本拒回退(复用 gcu-core policy::clipboard)；Unicode 经 CGEvent 字符串事件分块；Fake 任意平台测试；4 项真机用例待在 Mac 上跑 |
| P1-08 | macOS 守卫 / 热键 / 前台 | P1-06、P0-08 | 1.5d | done | cu2/P1-08-mac-guards |  | 2026-10-15 | 物理/合成输入区分的活动时钟(不可用→None)；Stop 热键；≤100ms 锁存/≤500ms 释放/≤5s watchdog(可注入时钟)；前台激活核验；失效禁依赖路线；真机/TCC 撤销/物理热键 not_run |
| P1-09 | macOS 契约测试与冒烟 | P1-01、P1-02、P1-03、P1-04、P1-05、P1-06、P1-07、P1-08 | 1.5d | done | cu2/P1-09-mac-contract-live |  | 2026-10-16 | Mini(macOS 27, arm64)真机 live 契约表 26 通过/4 跳过/0 失败(跳过均为范围划分：C17/C24/C25 归 core、C21 无排除能力)；夹具就绪等待实测首读 7 节点 incomplete→约 588ms 后 54 节点完整(2 次轮询)；文本框 SetValue 在 egui 上读回不符→NotSettable 并回退 TypeText；非 ASCII 键入经显式允许的剪贴板 targeted cmd+v；拖拽需 32ms 帧间隔；列表滚动=悬停后 line 滚轮。仅 macOS 27 实测，macOS 14/15、Intel、物理键鼠的 Stop 热键未覆盖 |
| P2-01 | Windows 发现 / 身份 / 虚拟桌面 | P0-05 | 2d | done | cu2/P2-01-windows-discovery |  | 2026-10-13 | 34测试；WinGate 统一门禁(引擎提升/高完整性目标/安全桌面)；真实 Windows 冒烟 ignored(not_run) |
| P2-02 | Windows UIA 树 | P2-01 | 3d | done | cu2/P2-02-uia-tree |  | 2026-10-13 | 常驻 MTA + CacheRequest 批量取属性；2s/5000 节点上限并设置 RawTree.incomplete；三类事件订阅 Drop 反注册；终端/IDE 启发式；真机冒烟(记事本/资源管理器/设置)not_run |
| P2-03 | Windows WGC 截图 | P2-01、P2-02、P0-07 | 2d | done | cu2/P2-03-windows-capture |  | 2026-10-13 | WGC 单帧 + PrintWindow(PW_RENDERFULLCONTENT) 回退，经 CaptureSource/FakeCapture 分层；最小化→window_minimized；受保护/全黑不当成功；路径经 CaptureStats；真机遮挡/150% DPI not_run |
| P2-04 | Windows worker 隔离 | P2-02、P2-03 | 2d | done | cu2/P2-04-worker |  | 2026-10-13 | gcu worker + supervisor：崩溃/超时/坏帧重启，只读重试一次、变更返回 unknown；Windows Job Object 随父退出；故障注入仅 test-worker-faults；语义动作(P2-05)也走 worker；真机与 300 次浸泡 not_run |
| P2-05 | Windows 语义动作 | P2-02 | 2.5d | done | cu2/P2-05-uia-actions |  | 2026-10-13 | Invoke/Toggle/SelectionItem/ExpandCollapse/Value/RangeValue/Scroll/Text 选区/LegacyIAccessible；仅明确无副作用才回退，不确定不重放；SetValue 仅 settable 非密码；不 SetFocus；真机夹具契约 not_run |
| P2-06 | Windows 定向消息与前台 | P2-05 | 3d | done | cu2/P2-06-win-input |  | 2026-10-14 | PostMessage 定向投递+effect 检测；Chromium/UWP 不静默升级(Auto/TargetedOnly 仅后台，前台需策略放行)；SendInput 前台路径+分块 Unicode；父进程独立 InputLedger(worker 被杀仍补发 up)；UIPI→elevation_required；真机夹具后台点击/记事本中文输入 not_run |
| P2-07 | Windows 守卫 / 热键 / 排除截图 | P2-06 | 1.5d | done | cu2/P2-07-win-guards |  | 2026-10-14 | LL 钩子过滤 injected、失效→None 并周期重装；父进程侧 RegisterHotKey Stop(≤100ms 锁存/≤500ms 释放/≤5s 杀 worker)；watchdog；覆盖层 SetWindowDisplayAffinity 探测；失效禁用前台输入路线；真机物理输入/UAC/覆盖层截图 not_run |
| P2-08 | Windows 剪贴板与文本 | P2-06 | 1d | done | cu2/P2-08-win-clipboard |  | 2026-10-15 | 全局互斥+序号/内容/焦点粘贴前校验；恢复不覆盖外来内容；敏感/高风险文本拒绝剪贴板回退(gcu-core policy::clipboard 共享分类，供 macOS 复用)；>8192 分批；Stop 接入位待 P2-07 合入对接；真机 not_run |
| P2-09 | Windows 契约测试与冒烟 | P2-01、P2-02、P2-03、P2-04、P2-05、P2-06、P2-07、P2-08 | 1.5d | done | cu2/P2-09-win-contract |  | 2026-10-16 | GitHub windows-2022 受限令牌交互桌面真机(第十九次运行)：契约 15 通过/15 跳过/0 失败，冒烟 0 通过/3 跳过/0 失败。跳过全部带实测原因：8 项(C06/C08/C09/C11/C15/C20/C22/W-HOTKEY)依赖前台投递，同进程 12 种合成鼠标探针(6 类×带/不带标记)全部 sent==requested、无一到达低级鼠标钩子(光标不动)，故 Stop/前台证明不可得，foreground 保持 Unavailable(fail-closed)；C13 列表无 ScrollPattern；C02/C21/C23/C25/C26/C27 为范围/夹具原因；冒烟 Notepad/Explorer 的启动、窗口、UIA 与外部路径断言真实通过，仅前台键入 Skip；Settings 的进程/UIA 断言通过，目标=High、harness=Medium 时按设计返回 ElevationRequired 并 Skip。真机通过：C01/C03/C04/C05/C07/C10/C12/C14/C16/C16-BUTTON/C17/C18/C19/C24/W-UIPI。发布前必须在带物理鼠标的 Windows 补测(清单见 gcu-driver-windows/diagnostics.md)；win-test 实机探针待用户在桌面运行后补记 |
| P3-01 | AT-SPI 树 | P0-05 | 3d | done | cu2/P3-01-atspi-tree |  | 2026-10-13 | AT-SPI 树+Linux 身份(路径+dev/inode+sha256；持久授权绑定映像，pid 复用另判)；CI 实时测试读到夹具 send_button |
| P3-02 | X11 窗口与截图 | P3-01、P0-07 | 2d | done | cu2/P3-02-x11 |  | 2026-10-13 | X11 窗口+XComposite 截图；无 composite 且被遮挡则拒绝(不交付他人内容)；CI Xvfb 实测 live_x11_fixture_window_capture_and_canvas_grid 通过 |
| P3-03 | AT-SPI 动作 + XTEST | P3-02 | 2.5d | done | cu2/P3-03-actions |  | 2026-10-13 | AT-SPI 语义动作+XTEST；TargetedOnly 不升级；InputLedger 先记账后发送并补发 up；CI Xvfb live 动作测试 5 个全过；文本框 SetValue 因 AccessKit 无 EditableText 返回 NotSettable(已登记) |
| P3-04 | X11 守卫与热键 | P3-03 | 1d | done | cu2/P3-04-x11-guard |  | 2026-10-13 | XI2 原始事件活动时钟(排除 XTEST，故障→None)；XGrabKey 停止热键(含 NumLock/CapsLock 组合)，失败则禁用前台 XTEST 路线；看门狗 5s；Xvfb 实机热键测试通过；物理硬件输入 not_run |
| P3-05 | Linux 契约测试与 CI | P3-01、P3-02、P3-03、P3-04 | 1.5d | done | cu2/P3-05-linux-contract |  | 2026-10-13 | C01–C27 表驱动实机契约(Xvfb+dbus+AT-SPI+openbox)：23 通过 / 6 跳过(原因逐条打印) / 0 失败；AT-SPI job 连续重跑稳定；smoke-linux.sh |
| P4-01 | macOS helper 与代理 | P1-09、S-02 | 3d | done | cu2/P4-01-mac-helper |  | 2026-10-16 | `gcu agent` + `gcu-cli::helper::{auth,client,frame,macos,proxy,serve}`：只有同签名身份且同 cdhash 的对端才被服务；同身份不同 cdhash 只能请求旧 helper 退出；另校验 UID、端点 inode、套接字权限、版本精确匹配、10 分钟空闲退出；`xtask bundle-macos` 生成 LSUIElement 的 .app，ad-hoc 签名用 identifier-only designated requirement。30 个 helper 测试+38 个 xtask 测试通过。未做：15 语 InfoPlist.strings；构建机上的 bundle-macos 只写 plist/图标(不打包 gcu、不 codesign)。真机待验(Mini)：TCC 弹窗指向 helper app 而非终端、同二进制代理在只授权 helper 时可用而 SSH 直接执行无授权、重编译后授权仍在、未签名 arm64 app 不能启动 |
| P4-02 | Onboarding 窗口 | P4-01 | 2d | done | cu2/P4-02-onboarding |  | 2026-10-16 | macOS helper 缺权限时的 Onboarding 窗口：权限轮询状态机、页面渲染、文案加载、重启 helper(复用 P4-01 的 open 启动/退出路径)的失败路径放在不依赖 wry 的模块并用 fake 测(gcu-cli onboarding 28 项)；wry WebView 在 gcu-cli 的 `macos-webview` 特性后(默认关)，窗口测试不创建 WebView；零外部 HTML(`xtask ui` 构建单文件 HTML/CSS 经 include_str! 编入)；15 语言文案以 `computer-use/ui/locales/*.json` 为单一来源，`xtask ui` 导出为 App 的 `src/i18n/messages/<locale>/computer-use.ts`(每语言 60 行)。依赖例外见 ADR 0009(deny.toml 的 skip-tree 仅 tao/wry，advisory 仅忽略 RUSTSEC-2024-0429/0370，CI 仅 macOS 作业启用该特性)。构建机无法编 wry(objc2-exception-helper 的 cc 问题)，其首次真编译由 CI 的 macos-14 作业承担。真机待验(Mini)：TCC 授权对象是 helper、设置按钮、权限轮询、屏幕录制授权后经 open 重启 helper、Not now/Esc、Enter 不触发设置或重启 |
| P4-03 | ApprovalProvider（elicitation + 审批窗） | P0-09、P4-01 | 3d | done | cu2/P4-03-approval |  | 2026-10-16 | ApprovalProvider：elicitation 优先，回退到各平台原生对话框(Linux zenity/kdialog、macOS osascript、Windows PowerShell)；`approvals.json` 持久化(policy_store，`gcu approvals` 子命令)；默认拒绝、Enter 不批准；模型理由标“模型提供”并按纯文本处理双向控制字符；`GCU_APPROVAL=auto-*` 仅在 feature eval-autoapprove 可用，发布构建设置该变量(含任意 auto 值)报错退出；审批文案 15 语言放 `computer-use/ui/locales/*.json`(键与 en 锁步，单一来源，待 P4-02 导出为 App 的 computer-use 命名空间)。gcu-cli approval 12 项(带/不带 eval-autoapprove)、gcu-mcp approvals 6 项通过。未做：`approval.html` 的 WebView 窗口(构建机无 WebKit/wry)；真机待验：各平台原生对话框默认焦点是拒绝、Enter 不批准 |
| P4-04 | gcu-overlay | P1-06、P2-06、P3-03 | 3.5d | done | cu2/P4-04-overlay |  | 2026-10-16 | gcu-overlay：状态+用户 Stop 按钮(不可聚焦/穿透/排除截图，macOS/Windows/X11)；Stop 状态机去抖、独立通道；文案表接口待 App i18n 对接；原生 Wayland 覆盖层声明不可用(裁定选项1)；三平台真机 GUI not_run |
| P4-05a | gcu install 客户端安装库（P4-05 前半） | P0-01 | 1d | done | cu2/P4-05a-install-lib | #14 | 2026-10-13 | 服务端名 grok-computer-use；Claude 保留 computer-use |
| P4-05 | gcu install/uninstall（clap 接线 + Windows 钩子） | P0-10、P4-05a | 1d | done | cu2/P4-05-install-wiring |  | 2026-10-13 | gcu install/uninstall 接线(dry-run、0600 备份、幂等、精确移除、~/.grok 需确认)；Windows Stop 钩子 .cmd；Antigravity 走假 agy 测试 |
| P4-06 | turn-ended 接入 | P0-09、S-01 | 1d | done | cu2/P4-06-turn-ended |  | 2026-10-13 | gcu turn-ended(Grok Stop 钩子 stdin / Codex notify argv)经私有端点通知会话；Turn 授权到期、幂等、不解除 Stop、空闲推定；链式 notify 先转发原调用；钩子错误可见且非致命 |
| P4-07 | 打包与分发 | P4-01、P4-02、P4-03、P4-04、P4-05、P4-06 | 3d | todo |  |  |  | 需签名证书（可先无签名） |
| P4-08 | 用户文档 | P4-07 | 1d | todo |  |  |  |  |
| P5-01 | ComputerUseHost 骨架 + 引擎托管 | P0-12、P1-09 | 2.5d | done | cu2/P5-01-host-skeleton |  | 2026-10-16 | 监督逻辑(准入/双向握手/空闲退出/熔断/Stop 锁存恢复/发布模式拒绝 Windows 提权与 UIAccess)放在 `gcu-host-protocol::supervisor`，24 个监督单测+13 个 engine_host 在 computer-use 工作区可测；`src-tauri/src/computer_use/` 只是薄适配(默认关闭，设置行与 UI 归 P5-08)，由 ci.yml 的 Rust 四平台编译(构建机无 webkit2gtk)。`gcu engine` 仍只接受 `--driver fake`，正式启用将失败关闭直到驱动装配。真机待验：macOS TCC 责任进程链、Developer ID 与 identifier-only、外部引擎不继承正式授权；Windows 提权/UIAccess 仅经 windows-gnu 编译 |
| P5-02 | Gateway + 注入 | P5-01、S-01 | 2d | done | cu2/P5-02-gateway |  | 2026-10-16 | 仅本地会话注入（SSH/定时任务/Remote IM 旁路不注入；Remote IM 走独立 agent 路径不经 session_manager::connect）；引擎侧每会话生成 32 字节 token，经 ACP `type:http` 的 `Authorization: Bearer` 头注入（S-01 已确认支持自定义 headers），不进环境变量/argv，`acp_client` 对 stderr/JSON-RPC 错误/非 JSON 行/崩溃详情做 scrub，会话关闭撤销；回环网关 Host 精确 `127.0.0.1:<端口>` 单值、Origin 缺省仅带 token 的非浏览器客户端可用、有值仅 `http://127.0.0.1:<端口>`，DNS rebinding/null Origin/重复 Host/无 token/错 token/跨会话图片被拒。gcu-host-protocol inject 8 项+supervisor 25 项、gcu-mcp gateway 2 项通过。`EngineProcess` 现要求 `Send + Sync`。未做：fork/切模型撤销(P5-09)；真机未验 |
| P5-03 | 审批卡 | P5-02 | 3d | done | cu2/P5-03-approval-card |  | 2026-10-16 | App 的审批卡：审批状态机放在 `gcu-host-protocol`（26 项测试）——App 只在真实审批事件中把请求/会话/App 映像/引擎实例/策略版本绑定，单次消费后才持久化 Always 并发 `engine.policy.changed`，无审批/跨身份/重放失败，外层超时丢弃等待时卡片同步失效，Always 发布失败回滚，会话关闭自动拒绝；`src-tauri/src/computer_use/{commands,policy_store,host_server}.rs` 只是薄适配（靠 ci.yml 编译）；前端 `ComputerUseApprovalCard`（含 `useComputerUseApprovals`、`approvalDesk`、`src/lib/api/computerUse.ts`）挂在聊天输入列，默认拒绝、Esc=拒绝、聊天输入框的 Enter 不触发决定、Tab/Enter 键盘可达、倒计时、会话切换回来仍在，模型理由标“模型提供”并按纯文本处理双向控制字符；15 语言的审批键并入 `computer-use/ui/locales/*.json`（单一来源）并由 `xtask ui export-i18n` 生成 App 的 `computer-use.ts`；`src/App.tsx`/`src/app/AppWorkbench.tsx` 无改动。vitest 56 项（审批卡+i18n 锁步）、tsc 通过。未做：浏览器里真实点选（只跑了 jsdom）；真机/端到端 |
| P5-04 | 敏感确认卡 + 交接卡 | P5-03 | 2d | done | cu2/P5-04-confirm-handoff-cards |  | 2026-10-16 | 复用 P5-03 审批桌：`ComputerUseActionConfirmCard` / `ComputerUseHandoffCard` + `confirmState`（pending→submitting→allowed/denied/timed_out/stale/closed）+ `useComputerUseSensitive`；确认绑定 snapshot/摘要/策略版本，批准后投递前重验，状态变化作废重审；拒绝/超时/会话关闭零执行；交接经 `Session::request_handoff`，用户“已处理”只触发新消息，不重放、不解除 Stop；脱敏 crop 缺失/敏感区定位失败降级为交接；理由纯文本+“模型提供”；15 语言键并入 `computer-use/ui/locales/*.json`。vitest 78 项、tsc、`verify.sh`（含跨目标 clippy）通过。未做：浏览器里真实点选、端到端；`src-tauri` 仅靠 CI 编译；观察路径上不完整树仍直接返回 `HandoffRequired`（不开交接卡） |
| P5-05 | 工具调用卡 | P5-02 | 2.5d | done | cu2/P5-05-tool-card |  | 2026-10-16 | 聊天流里识别 `computer-use` server 的工具调用：`ComputerUseToolCard`（动作图标、目标 App·元素、结果徽标 done/refused/unknown+effect、耗时、脱敏截图缩略图点击放大、原文折叠）接在 `lobe-chat` 既有 Timeline 工具行；事件解析放 `gcu-host-protocol::events`（Rust 测试）+ `src/lib/computer-use/events.ts`；截图只经 `cu_tool_shot` 取脱敏图（跨会话/缺失/拒绝三种错误码，不含路径）；15 语言键并入 `computer-use/ui/locales/*.json` 并 `export-i18n`。本地：`rcargo test -p gcu-host-protocol`、vitest 117 项、tsc、`xtask ui check`、`verify.sh`（含跨目标 clippy）通过。未做：浏览器里真实点击（仅 jsdom）；聊天删除会话界面尚未调用 `sharedShotBook.dropSession`（Rust 侧在 gateway revoke/关闭功能时已 drop）；`src-tauri` 仅靠 CI 编译；真机待验：脱敏截图预览、查看器放大、跨会话 `engine.screenshot.get` 拒绝、app data 下文件 0o600；节流和侧栏属 P5-06 |
| P5-06 | Computer 侧栏面板 | P5-05 | 2.5d | todo |  |  |  |  |
| P5-07 | 覆盖窗 | P5-01 | 3d | done | cu2/P5-07-overlay |  | 2026-10-16 | App 内 `cu-overlay` 窗口（透明、置顶、不可聚焦、不进任务栏，胶囊外鼠标穿透）：布局/安全/截图排除规则在 `gcu-host-protocol::overlay`（多屏/DPI 定位、光标动画参数、状态），`src-tauri/src/computer_use/overlay.rs` 为 292 行薄适配 + 平台 FFI；独立最小权限 capability（`cu-overlay.json` 权限为空，覆盖窗不享有浏览窗远程内容权限）；Stop/用户恢复走独立通道并持久锁存，`gcu-cli` 的 Stop 处理不再被 sessions 写锁延迟（新增 `engine_stop_tests`）；前端 `ComputerUseOverlay` + 独立 Vite 入口 `computer-use-overlay.html`（CSP 收紧）。本地：`rcargo test -p gcu-overlay --test overlay`(16)、`-p gcu-host-protocol`、`-p gcu-cli` 全过、`verify.sh`（含跨目标 clippy）、`xtask ui check`、vitest 85、tsc 通过。未做：原生焦点/穿透/截图排除/Stop 时限的三平台真机验证（not_run，已留 ignored 测试与 `overlay/README.md` 方法）；`src-tauri` 仅靠 CI 编译；浏览器里真实点击 |
| P5-08 | 设置页 | P5-03 | 2.5d | todo |  |  |  |  |
| P5-09 | 生命周期接线 + wiki 重写 | P5-02 | 1.5d | todo |  |  |  |  |
| P5-10 | 内置浏览窗 | P5-07 | 3d | todo |  |  |  |  |
| P5-11 | App 内评测与发布开关 | P5-01、P5-02、P5-03、P5-04、P5-05、P5-06、P5-07、P5-08、P5-09、P9-01 | 1.5d | todo |  |  |  | 评测runner依赖P9-01 |
| P6-01 | settle 调优 | P1-09、P2-09、P3-05 | 2d | todo |  |  |  |  |
| P6-02 | auto 截图策略 | P6-01 | 1.5d | todo |  |  |  |  |
| P6-03 | wait_for | P6-01 | 1.5d | todo |  |  |  |  |
| P6-04 | computer_batch | P6-03 | 3d | todo |  |  |  |  |
| P6-05 | computer_script | P6-04 | 4d | todo |  |  |  | 需安全签字 |
| P6-06 | 工具目录与 instructions 优化 | P6-05 | 1d | todo |  |  |  |  |
| P6-07 | 效率 A/B 评测 | P6-01、P6-02、P6-03、P6-04、P6-05、P6-06、P9-01 | 2d | todo |  |  |  |  |
| P7-01 | Wayland 检测与能力表 | P3-05 | 1d | done | cu2/P7-01-wayland-caps |  | 2026-10-14 | 环境探测(GNOME/KDE/wlroots/其他/未知、portal 接口)；纯 X11/XWayland/原生 Wayland 三类能力表；守卫或全局 Stop 不可靠则禁用依赖路线；doctor 与 capabilities 同源；不声称 Wayland 全支持 |
| P7-02 | portal ScreenCast 截图与 restore token | P7-01 | 4d | done | cu2/P7-02-portal-capture |  | 2026-10-15 | portal/PipeWire 在 trait 后面；restore token 0600/0700、来源绑定、24h、变化即拒、撤销/过期清理；未知范围不出图；仅授权窗口帧；ADR 0007 选项 A(deny 具名例外+CI 用 PipeWire PPA)；真实 PipeWire 仅 CI 编译验证，Wayland 真机 not_run |
| P7-03 | portal RemoteDesktop + libei 输入 | P7-02 | 4d | done | cu2/P7-03-libei |  | 2026-10-16 | portal RemoteDesktop + reis(libei 协议，非 libei-sys)；流坐标映射；输入账本(取消/错误/Stop/会话关闭补 up)；仅有可靠停止控制时启用，无全桌面静默回退；restore token 同 P7-02；Wayland 真机 not_run；PipeWire feature 由 CI Linux 编译验证 |
| P7-04 | Wayland 窗口身份 | P7-01 | 4d | done | cu2/P7-04-wayland-identity |  | 2026-10-15 | GNOME 扩展(只读窗口列表+策略激活 D-Bus)/KWin 脚本/wlroots foreign-toplevel；映像绑定规范路径+dev/inode+sha256；标题近似匹配不当身份；KWin/wlroots 仅观察/handoff；未知 compositor 拒绝；真实桌面 not_run |
| P7-05 | Wayland 契约测试 | P7-02、P7-03、P7-04、P1-01 | 3d | done | cu2/P7-05-wayland-contract |  | 2026-10-16 | 表驱动 Wayland 契约(token 生命周期/源范围/Stop 失效/脱敏/执行界面/skip 与能力表一致)：可移植行三平台通过(17 通过/1 ignored)；真实 GNOME headless(mutter+portal+pipewire)行 not_run，由自托管 Wayland runner 运行(wayland-ci.sh 缺环境如实 not_run)；缺 runner 不算 passed |
| P7-06 | Wayland 用户文档 | P7-05 | 1d | todo |  |  |  | 真实portal/Wayland需匹配桌面runner |
| P8-01 | 精简 MV3 DOM 扩展 | P4-07 | 4d | todo |  |  |  |  |
| P8-02 | Native messaging host | P8-01 | 2d | todo |  |  |  |  |
| P8-03 | DOM 树融合与动作路由 | P8-02、P6-04 | 3d | todo |  |  |  |  |
| P8-04 | 扩展上架材料 | P8-03 | 1d | todo |  |  |  |  |
| P8-05 | L0 vs L2 网页评测 | P8-03、P9-01 | 2d | todo |  |  |  |  |
| P9-01 | 评测 runner | P0-10 | 4d | done | cu2/P9-01-eval-runner |  | 2026-10-13 | gcu-eval：任务 YAML、外部 oracle(含副作用检查)、EnvProvider 接口、指标/HTML 报告/A-B；仅 eval-autoapprove 自动批准；nightly 冒烟(fake，无凭据则跳过)；一次性桌面 provider、真实模型、跨客户端 not_run |
| P9-02 | 任务集 ≥60 | P9-01 | 4d | done | cu2/P9-02-task-catalog |  | 2026-10-14 | 三平台×六类任务集(含 red_team)，每项 oracle 自证 pass/fail，Web SSH/云终端/容器/远程桌面/调试控制台/IDE 入口、伪装提交、TOCTOU、Stop 续轮、CU 外副作用；task_catalog 校验覆盖下限；一次性桌面实跑 not_run |
| P9-03 | 性能预算门禁 | P6-01 | 1.5d | todo |  |  |  |  |
| P9-04 | 安全审查 | P6-05、P5-09 | 3d | todo |  |  |  |  |
| P9-05 | 兼容性矩阵 | P4-07 | 3d | todo |  |  |  |  |
| P9-06 | 长稳浸泡 | P5-11 | 2d | todo |  |  |  |  |
| P9-07 | 1.0 发布 | P9-01、P9-02、P9-03、P9-04、P9-05、P9-06 | 1.5d | todo |  |  |  |  |

## 发现（卡外问题，一行一条）

- ADR-0004：原生树未读区域可能隐藏密码；已批准 `RawTree.incomplete`，core 统一拒图并 handoff、文本照常交付，Linux/macOS 上报上限/期限/部分错误；Windows 由 P2-02 接入。
- 计划 v1.1 逐项决定见 [评审处置表](reviews/2026-10-13-disposition.md)；工时 P0–P9 177d + spike 3d，与 06 一致。
- ~~P0-03 的 ErrorCode 缺 window_minimized~~ 已补（cu2/protocol-window-minimized）。
- 外部 CI：仓库原有 `ci.yml` 已对 `computer-use/**`、`docs/plans/computer-use-v2/**`、`computer-use.yml` 的纯改动加 `paths-ignore`（同时触及其它路径的改动仍会跑）。
- eframe 0.36 在 Linux AT-SPI 上不暴露密码保护(ROLE_PASSWORD_TEXT/STATE_PROTECTED)与模态(ROLE_DIALOG/STATE_MODAL)：契约用例 C14/C20 不能靠 gcu-fixture 验证，需 fixtures-native（GTK4 等）补；脚本以 KNOWN_GAPS 显式登记，非静默跳过。
- AccessKit(eframe) 在 Linux AT-SPI 上的文本框没有实现 EditableText：`SetValue` 返回 `NotSettable`（滑块 `Value.SetCurrentValue` 可用）。模型应改用 `type_text`（需前台策略）；路由层不自动回退，错误提示里写明。真实 GTK/Qt 应用的 EditableText 支持待 P9-05 兼容性矩阵验证。
- 偶发：`gcu-cli/tests/engine_host.rs::engine_lost_stop_channel_denies_approval_and_exits_with_rpc_still_connected` 在 macOS CI（pull_request 运行）失败过 1 次（等待调用结果超过 RPC_TIMEOUT 5s），同一提交的 push 运行与重跑均通过，Linux 本地 30 次加压全过；根因未明。再出现时先读失败日志，不要靠重跑；怀疑点：engine 退出时 `sessions` 关闭路径（`entry.remote.lock().await.close()`）等待在途调用。
- 驱动侧 Stop 锁存（`LinuxDriver::with_stop_latch`）与活动时钟（`ActivityClock`）还未接到 Host 的 `StopMirror`/`UserActivity`：真实驱动组装（gcu engine 装配）卡里必须接上，否则热键只在驱动内生效。
- Linux 树遍历对名称/标识含 "password" 的节点追加 PASSWORD 状态（启发式，宁可多抹）：eframe 不暴露密码角色，夹具靠它通过 C14/C20；真正的原生密码控件仍以 AT-SPI 角色/状态为准。契约表里 6 项跳过（launch_app、set_value 文本框、combobox Selection、stale geometry 快照 ID 等）是驱动能力缺口，不是通过。
- S-02 发现：macOS 后台点击需要给 CGEvent 设置整数字段 51（窗口号），否则 postToPid 事件的 windowNumber 为 0；Chromium 在 macOS 27 后台收不到（详见 ADR 0006）。P1-06 的契约测试必须校验“实际到达”。
- 带 worker 子进程的测试（`gcu-driver-windows` 的 `tests/{input,clipboard,worker}`）对机器负载/冷启动敏感：慢的 macOS runner（#50 稳定失败）和高负载的 build8（P2-08 一次失败）上 worker 启动超过 `DRIVER_DEADLINE` 就返回 `Timeout`。P2-07 正在查根因；原则：这类测试不得依赖真实时间/真实进程冷启动，改为可注入时钟或预热。
- macOS 真机发现(Mini, macOS 27)：P1-02 的发现路径在真机上首次 list_apps 超时、resolve_app 找不到运行中的夹具(逐 App 签名/哈希堵住串行队列)，已修(cu2/fix-mac-discovery)；Windows 在真机 CI 上暴露同类问题，由 P2-09 分支跟进。真机/真桌面才能暴露这类问题，CI 与假驱动测不出。
- P1-09 真机发现(Mini, macOS 27)：egui/AccessKit 的 AX 树是懒激活的——第一次访问只返回 7 节点(驱动如实标 incomplete=partial_error)，约 588ms 后才是完整 54 节点；调用方(Router/套件)必须带界限地重读直到完整，ADR 0004 的“不完整即不出图”因此在首次观察必然触发。
- P1-09 真机发现(Mini)：accesskit_macos 的 `setAccessibilityValue:` 只是把动作入队、AppKit 立即报成功，egui 0.36 的 TextEdit 丢弃该动作，文本不变——AX 写入后必须读回验证(驱动已在注入时钟 320ms 内轮询，窗口后仍不符才 NotSettable 且 maybe_executed=true，避免“假成功”和“假失败”两种错)。同一原因使 Unicode/ZWJ/换行只能走剪贴板粘贴路径。
- P1-09 真机发现(Mini)：egui 拖拽对同一帧内的按下+抬起不响应，两点拖拽需分帧；虚拟列表在 AX 里没有 AXList/AXTable，只有 9 个无名 AXStaticText 与只可按压的滚动条，滚动靠悬停后的 line 单位滚轮。
- P5-01：`interprocess` 2.4.6 需要比 src-tauri 锁里 tokio 1.53.1 更新的 tokio（`AsyncFd::register_with_interest` 缺失），src-tauri 的 Cargo.lock 固定在 2.4.5（与 computer-use/Cargo.lock 一致）；升级 tokio 前不要 `cargo update` 该 crate。
- 构建机的 `rcargo` 只同步 computer-use/ 与 src-tauri 的 Cargo.toml/Cargo.lock，不同步 src-tauri/src，且无 webkit2gtk：src-tauri 的代码只能由 ci.yml 编译，所以任何放进 src-tauri 的逻辑都要尽量薄，可测逻辑放进 computer-use 的 crate。
- CI：Linux 作业依赖的 PipeWire PPA(ppa.launchpadcontent.net)会间歇 503（索引与 .deb 下载都会）；#71 加了 apt 重试并在 PPA 不可用时明确失败，而不是回退到过旧的系统 libpipewire 后在 libspa 里报一串编译错误。
- P4-01：Windows 检出会把 `apps/gcu-helper-macos/Info.plist` 模板转成 CRLF，而生成物是 LF；读模板做比较前要归一化换行(xtask 测试已处理)。
- P2-09：**Windows 上 UIA InvokePattern 本身会激活目标窗口**（不经我们的驱动、裸 Invoke 对照实验已证实），所以“后台 press 不改变前台”不能靠 UIA Invoke 满足；原生 BUTTON 经 `BM_CLICK` 不改变前台（C16-BUTTON 真断言）。驱动对非 BUTTON 元素的后台 press 仍用 UIA Invoke，并在能力/doctor 里诚实声明 `background_operation: Partial`（Invoke 可能激活目标窗口）；显式要求“仅目标消息”的点击不回退到会激活的路径。
- P2-09：GitHub 托管 windows-2022 runner 的受限令牌交互桌面里**没有指针输入栈**：12 种合成鼠标探针无一触发低级鼠标钩子，键盘探针可以（flags=144）。这是环境限制，产品无鼠标证明时 foreground 保持 Unavailable、Stop 语义不放宽；有物理鼠标的 Windows 上是否同样需要真机补测。
- P2-09：受限令牌能在该 runner 启动的前提是令牌的 TokenOwner=用户 SID 且 TokenDefaultDacl 授予用户/登录 SID/SYSTEM，否则进程以 0xC0000142 失败；runner 的 admin 令牌没有 UAC 过滤，Shell 激活的 SystemSettings 是 High 完整性，Medium 的 harness 按设计被拒(ElevationRequired)。
- P2-09：egui 夹具在 UIA 下首读树可能不完整（和 macOS 一样懒激活），调用方要有界重读。
- P4-03：i18n 导出脚本(P4-02)还不存在，审批文案先以 `computer-use/ui/locales/*.json` 为单一来源并有 15 语言键锁步测试；P4-02 要把它导出为 App 的 `computer-use` 命名空间(`src/i18n/messages/<locale>/computer-use.ts`)，而不是再复制一份。
- P5-02：token 必然随 ACP 会话载荷到达 agent 进程（header 方案的代价）；Host 侧所有日志/崩溃路径都 scrub，不要新增会打印 ACP 载荷的路径而不过 `gcu_host_protocol::inject::scrub`。
- 偶发(2026-10-16，PR #75)：Windows 桌面套件 C04(物理截图) 在某次运行里报 `native operation timed out (maybe_executed=false)`——`rpc_capture` 常态约 0.8–1.1s，那次 5s 内没有返回，同一作业总耗时 10m9s(平时约 6m)，同一提交的另一运行 14/15/0 通过、重跑通过。P5-02 没有改 Windows 驱动，判断为 runner 偶发负载；若再次出现要查截图路径(WGC/PrintWindow)是否会挂起，以及是否在 `DRIVER_DEADLINE` 内正确取消。
- 偶发(2026-10-16，PR #75)：既有 src-tauri 测试 `plugin_auth_transport::tests::private_pipe_reaches_legacy_node_parser_on_this_host` 在慢 Windows runner 上 10s 超时("Plugin auth timed out"，node 启动慢)，重跑通过；与 Computer Use 无关，若再出现可单独把它的超时放宽到 30s。
- P4-02：wry/tao 进入 `cargo deny` 解析图后会触发多组跨目标重复版本和两条只经 Linux GTK 路径的 advisory（RUSTSEC-2024-0429 glib、RUSTSEC-2024-0370 proc-macro-error）；已用 ADR 0009 的最小具名例外处理，退出条件是 tao/wry 升级后不再依赖它们或改用其它 WebView 方案。
- P4-01 真机发现(Pro, macOS 15)：`codesign -r <文本>`（两个独立参数）会把需求文本当成**文件名**读，报 “No such file or directory / invalid requirement specification”；必须写成单个参数 `-r=designated => identifier "<id>"`（S-02 当时手工命令里就是带 `=` 的）。`xtask bundle-macos` 一直用独立参数且吞掉了 stderr，单元测试只校验了 argv 形状所以没发现；已修并让错误带出 codesign 的 stderr，真机上签名验证为 `Identifier=ai.grok.computer-use.helper.dev`、`designated => identifier "…"`、`codesign --verify --strict` 通过。

## 阶段记录

-

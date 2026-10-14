# Computer Use v2 · STATUS

> 唯一进度表。卡 PR 最后一次提交置 done 并填分支；PR 编号在后续功能 PR 回填，不为状态单独提 PR。依赖完成按集成基线判定；拆卡全部子 PR 合入后才完成。
> 状态：`todo` · `in_progress` · `blocked: …` · `needs-decision: …` · `done` · `not_run: …`（缺设备/runner）；阶段结束底部≤5行。当前基线 `origin/feat/cu-v2`，按总控已核实进展更新。

| 卡 | 标题 | 依赖 | 估算 | 状态 | 分支 | PR | 日期 | 备注 |
|---|---|---|---|---|---|---|---|---|
| S-01 | Grok Build CLI MCP 能力探测 | — | 1.5d | done | cu2/S-01-grok-mcp-probe | #6 | 2026-10-13 | ADR 0001 已接受；影响已写入 03/04/05 |
| S-02 | macOS TCC 与 helper 可行性 | — | 1.5d | todo |  |  |  | 需 macOS 14/15/26 机器 |
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
| P1-05 | macOS 语义动作 | P1-03 | 2.5d | todo |  |  |  |  |
| P1-06 | macOS 定向事件 / SkyLight | P1-04、S-02 | 3d | todo |  |  |  |  |
| P1-07 | macOS 文本输入与剪贴板 | P1-06 | 1.5d | todo |  |  |  |  |
| P1-08 | macOS 守卫 / 热键 / 前台 | P1-06、P0-08 | 1.5d | todo |  |  |  |  |
| P1-09 | macOS 契约测试与冒烟 | P1-01、P1-02、P1-03、P1-04、P1-05、P1-06、P1-07、P1-08 | 1.5d | todo |  |  |  | 需自托管 Mac runner |
| P2-01 | Windows 发现 / 身份 / 虚拟桌面 | P0-05 | 2d | done | cu2/P2-01-windows-discovery |  | 2026-10-13 | 34测试；WinGate 统一门禁(引擎提升/高完整性目标/安全桌面)；真实 Windows 冒烟 ignored(not_run) |
| P2-02 | Windows UIA 树 | P2-01 | 3d | todo |  |  |  |  |
| P2-03 | Windows WGC 截图 | P2-01、P2-02、P0-07 | 2d | todo |  |  |  |  |
| P2-04 | Windows worker 隔离 | P2-02、P2-03 | 2d | todo |  |  |  |  |
| P2-05 | Windows 语义动作 | P2-02 | 2.5d | todo |  |  |  |  |
| P2-06 | Windows 定向消息与前台 | P2-05 | 3d | todo |  |  |  |  |
| P2-07 | Windows 守卫 / 热键 / 排除截图 | P2-06 | 1.5d | todo |  |  |  |  |
| P2-08 | Windows 剪贴板与文本 | P2-06 | 1d | todo |  |  |  |  |
| P2-09 | Windows 契约测试与冒烟 | P2-01、P2-02、P2-03、P2-04、P2-05、P2-06、P2-07、P2-08 | 1.5d | todo |  |  |  |  |
| P3-01 | AT-SPI 树 | P0-05 | 3d | done | cu2/P3-01-atspi-tree |  | 2026-10-13 | AT-SPI 树+Linux 身份(路径+dev/inode+sha256；持久授权绑定映像，pid 复用另判)；CI 实时测试读到夹具 send_button |
| P3-02 | X11 窗口与截图 | P3-01、P0-07 | 2d | done | cu2/P3-02-x11 |  | 2026-10-13 | X11 窗口+XComposite 截图；无 composite 且被遮挡则拒绝(不交付他人内容)；CI Xvfb 实测 live_x11_fixture_window_capture_and_canvas_grid 通过 |
| P3-03 | AT-SPI 动作 + XTEST | P3-02 | 2.5d | done | cu2/P3-03-actions |  | 2026-10-13 | AT-SPI 语义动作+XTEST；TargetedOnly 不升级；InputLedger 先记账后发送并补发 up；CI Xvfb live 动作测试 5 个全过；文本框 SetValue 因 AccessKit 无 EditableText 返回 NotSettable(已登记) |
| P3-04 | X11 守卫与热键 | P3-03 | 1d | done | cu2/P3-04-x11-guard |  | 2026-10-13 | XI2 原始事件活动时钟(排除 XTEST，故障→None)；XGrabKey 停止热键(含 NumLock/CapsLock 组合)，失败则禁用前台 XTEST 路线；看门狗 5s；Xvfb 实机热键测试通过；物理硬件输入 not_run |
| P3-05 | Linux 契约测试与 CI | P3-01、P3-02、P3-03、P3-04 | 1.5d | todo |  |  |  |  |
| P4-01 | macOS helper 与代理 | P1-09、S-02 | 3d | todo |  |  |  |  |
| P4-02 | Onboarding 窗口 | P4-01 | 2d | todo |  |  |  |  |
| P4-03 | ApprovalProvider（elicitation + 审批窗） | P0-09、P4-01 | 3d | todo |  |  |  |  |
| P4-04 | gcu-overlay | P1-06、P2-06、P3-03 | 3.5d | todo |  |  |  |  |
| P4-05a | gcu install 客户端安装库（P4-05 前半） | P0-01 | 1d | done | cu2/P4-05a-install-lib | #14 | 2026-10-13 | 服务端名 grok-computer-use；Claude 保留 computer-use |
| P4-05 | gcu install/uninstall（clap 接线 + Windows 钩子） | P0-10、P4-05a | 1d | todo |  |  |  | 库部分已由 P4-05a 完成 |
| P4-06 | turn-ended 接入 | P0-09、S-01 | 1d | todo |  |  |  |  |
| P4-07 | 打包与分发 | P4-01、P4-02、P4-03、P4-04、P4-05、P4-06 | 3d | todo |  |  |  | 需签名证书（可先无签名） |
| P4-08 | 用户文档 | P4-07 | 1d | todo |  |  |  |  |
| P5-01 | ComputerUseHost 骨架 + 引擎托管 | P0-12、P1-09 | 2.5d | todo |  |  |  |  |
| P5-02 | Gateway + 注入 | P5-01、S-01 | 2d | todo |  |  |  |  |
| P5-03 | 审批卡 | P5-02 | 3d | todo |  |  |  |  |
| P5-04 | 敏感确认卡 + 交接卡 | P5-03 | 2d | todo |  |  |  |  |
| P5-05 | 工具调用卡 | P5-02 | 2.5d | todo |  |  |  |  |
| P5-06 | Computer 侧栏面板 | P5-05 | 2.5d | todo |  |  |  |  |
| P5-07 | 覆盖窗 | P5-01 | 3d | todo |  |  |  |  |
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
| P7-01 | Wayland 检测与能力表 | P3-05 | 1d | todo |  |  |  | 真实portal/Wayland需匹配桌面runner |
| P7-02 | portal ScreenCast 截图与 restore token | P7-01 | 4d | todo |  |  |  | 真实portal/Wayland需匹配桌面runner |
| P7-03 | portal RemoteDesktop + libei 输入 | P7-02 | 4d | todo |  |  |  | 真实portal/Wayland需匹配桌面runner |
| P7-04 | Wayland 窗口身份 | P7-01 | 4d | todo |  |  |  | 真实portal/Wayland需匹配桌面runner |
| P7-05 | Wayland 契约测试 | P7-02、P7-03、P7-04、P1-01 | 3d | todo |  |  |  | 真实portal/Wayland需匹配桌面runner |
| P7-06 | Wayland 用户文档 | P7-05 | 1d | todo |  |  |  | 真实portal/Wayland需匹配桌面runner |
| P8-01 | 精简 MV3 DOM 扩展 | P4-07 | 4d | todo |  |  |  |  |
| P8-02 | Native messaging host | P8-01 | 2d | todo |  |  |  |  |
| P8-03 | DOM 树融合与动作路由 | P8-02、P6-04 | 3d | todo |  |  |  |  |
| P8-04 | 扩展上架材料 | P8-03 | 1d | todo |  |  |  |  |
| P8-05 | L0 vs L2 网页评测 | P8-03、P9-01 | 2d | todo |  |  |  |  |
| P9-01 | 评测 runner | P0-10 | 4d | todo |  |  |  | 可提前到 P4 期间 |
| P9-02 | 任务集 ≥60 | P9-01 | 4d | todo |  |  |  |  |
| P9-03 | 性能预算门禁 | P6-01 | 1.5d | todo |  |  |  |  |
| P9-04 | 安全审查 | P6-05、P5-09 | 3d | todo |  |  |  |  |
| P9-05 | 兼容性矩阵 | P4-07 | 3d | todo |  |  |  |  |
| P9-06 | 长稳浸泡 | P5-11 | 2d | todo |  |  |  |  |
| P9-07 | 1.0 发布 | P9-01、P9-02、P9-03、P9-04、P9-05、P9-06 | 1.5d | todo |  |  |  |  |

## 发现（卡外问题，一行一条）

- ADR-0004：原生树未读区域可能隐藏密码；已批准 `RawTree.incomplete`，core 统一拒图并 handoff、文本照常交付，Linux/macOS 上报上限/期限/部分错误；Windows 由 P2-02 接入。
- 计划 v1.1 逐项决定见 [评审处置表](reviews/2026-10-13-disposition.md)；工时 P0–P9 177d + spike 3d，与 06 一致。
- ~~P0-03 的 ErrorCode 缺 window_minimized~~ 已补（cu2/protocol-window-minimized）。
- 外部 CI：仓库原有 `ci.yml` 会对 `computer-use/**` 改动也整套运行（Rust/frontend），浪费时间；待 CU 稳定后给它加 `paths-ignore: computer-use/**`。
- eframe 0.36 在 Linux AT-SPI 上不暴露密码保护(ROLE_PASSWORD_TEXT/STATE_PROTECTED)与模态(ROLE_DIALOG/STATE_MODAL)：契约用例 C14/C20 不能靠 gcu-fixture 验证，需 fixtures-native（GTK4 等）补；脚本以 KNOWN_GAPS 显式登记，非静默跳过。
- Windows 上 Grok `Stop` 钩子（.cmd）未实现；安装库当前钩子脚本为 Unix `.sh`，Windows 只装 MCP 条目。
- AccessKit(eframe) 在 Linux AT-SPI 上的文本框没有实现 EditableText：`SetValue` 返回 `NotSettable`（滑块 `Value.SetCurrentValue` 可用）。模型应改用 `type_text`（需前台策略）；路由层不自动回退，错误提示里写明。真实 GTK/Qt 应用的 EditableText 支持待 P9-05 兼容性矩阵验证。
- 偶发：`gcu-cli/tests/engine_host.rs::engine_lost_stop_channel_denies_approval_and_exits_with_rpc_still_connected` 在 macOS CI（pull_request 运行）失败过 1 次（等待调用结果超过 RPC_TIMEOUT 5s），同一提交的 push 运行与重跑均通过，Linux 本地 30 次加压全过；根因未明。再出现时先读失败日志，不要靠重跑；怀疑点：engine 退出时 `sessions` 关闭路径（`entry.remote.lock().await.close()`）等待在途调用。
- 驱动侧 Stop 锁存（`LinuxDriver::with_stop_latch`）与活动时钟（`ActivityClock`）还未接到 Host 的 `StopMirror`/`UserActivity`：真实驱动组装（gcu engine 装配）卡里必须接上，否则热键只在驱动内生效。

## 阶段记录

-

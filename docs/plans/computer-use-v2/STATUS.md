# Computer Use v2 · STATUS

> 唯一进度表。每个 PR 合入时更新对应行。状态：`todo` · `in_progress` · `blocked: …` · `needs-decision: …` · `done` · `not_run: …`（缺设备/runner）。
> 不在此写散文；阶段结束在底部"阶段记录"写 ≤5 行。

| 卡 | 标题 | 依赖 | 估算 | 状态 | PR | 日期 | 备注 |
|---|---|---|---|---|---|---|---|
| S-01 | Grok Build CLI MCP 能力探测 | — | 1.5d | todo | | | |
| S-02 | macOS TCC 与 helper 可行性 | — | 1.5d | todo | | | 需 macOS 14/15/26 机器 |
| P0-01 | computer-use workspace | — | 1d | todo | | | |
| P0-02 | CI 骨架 | P0-01 | 1d | todo | | | |
| P0-03 | gcu-protocol | P0-01 | 2d | todo | | | |
| P0-04 | gcu-host-api | P0-03 | 1d | todo | | | |
| P0-05 | core 模型 / Driver trait / fake 驱动 | P0-03 | 2.5d | todo | | | |
| P0-06 | 树管线与渲染 | P0-05 | 3d | todo | | | |
| P0-07 | SnapshotStore / Index / Diff | P0-06 | 2d | todo | | | |
| P0-08 | Session / Policy / Router / Ledger | P0-04, P0-07 | 3d | todo | | | |
| P0-09 | gcu-mcp stdio | P0-08 | 2d | todo | | | |
| P0-10 | gcu-cli（fake） | P0-09 | 1d | todo | | | |
| P0-12 | gcu-host-protocol 与 engine 模式 | P0-09 | 2.5d | todo | | | 路线 B 松耦合基础 |
| P0-11 | 文档与 STATUS 初始化 | P0-10 | 0.5d | todo | | | |
| P1-01 | 跨平台夹具 App | P0-01 | 2.5d | todo | | | |
| P1-02 | macOS 发现与身份 | P0-05 | 2d | todo | | | |
| P1-03 | macOS AX 树 | P1-02 | 3d | todo | | | |
| P1-04 | macOS SCK 截图 | P1-02 | 2d | todo | | | |
| P1-05 | macOS 语义动作 | P1-03 | 2.5d | todo | | | |
| P1-06 | macOS 定向事件 / SkyLight | P1-04, S-02 | 3d | todo | | | |
| P1-07 | macOS 文本输入与剪贴板 | P1-06 | 1.5d | todo | | | |
| P1-08 | macOS 守卫 / 热键 / 前台 | P1-06, P0-08 | 1.5d | todo | | | |
| P1-09 | macOS 契约测试与冒烟 | P1-01…P1-08 | 1.5d | todo | | | 需自托管 Mac runner |
| P2-01 | Windows 发现 / 身份 / 虚拟桌面 | P0-05 | 2d | todo | | | |
| P2-02 | Windows UIA 树 | P2-01 | 3d | todo | | | |
| P2-03 | Windows WGC 截图 | P2-01 | 2d | todo | | | |
| P2-04 | Windows worker 隔离 | P2-02, P2-03 | 2d | todo | | | |
| P2-05 | Windows 语义动作 | P2-02 | 2.5d | todo | | | |
| P2-06 | Windows 定向消息与前台 | P2-05 | 3d | todo | | | |
| P2-07 | Windows 守卫 / 热键 / 排除截图 | P2-06 | 1.5d | todo | | | |
| P2-08 | Windows 剪贴板与文本 | P2-06 | 1d | todo | | | |
| P2-09 | Windows 契约测试与冒烟 | P2-01…P2-08 | 1.5d | todo | | | |
| P3-01 | AT-SPI 树 | P0-05 | 3d | todo | | | |
| P3-02 | X11 窗口与截图 | P3-01 | 2d | todo | | | |
| P3-03 | AT-SPI 动作 + XTEST | P3-02 | 2.5d | todo | | | |
| P3-04 | X11 守卫与热键 | P3-03 | 1d | todo | | | |
| P3-05 | Linux 契约测试与 CI | P3-01…P3-04 | 1.5d | todo | | | |
| P4-01 | macOS helper 与代理 | P1-09, S-02 | 3d | todo | | | |
| P4-02 | Onboarding 窗口 | P4-01 | 2d | todo | | | |
| P4-03 | ApprovalProvider（elicitation + 审批窗） | P0-09, P4-01 | 3d | todo | | | |
| P4-04 | gcu-overlay | P1-06, P2-06, P3-03 | 3.5d | todo | | | |
| P4-05 | gcu install/uninstall | P0-10 | 2d | todo | | | |
| P4-06 | turn-ended 接入 | P0-09, S-01 | 1d | todo | | | |
| P4-07 | 打包与分发 | P4-01…P4-06 | 3d | todo | | | 需签名证书（可先无签名） |
| P4-08 | 用户文档 | P4-07 | 1d | todo | | | |
| P5-01 | ComputerUseHost 骨架 + 引擎托管 | P0-12, P1-09 | 2.5d | todo | | | |
| P5-02 | Gateway + 注入 | P5-01, S-01 | 2d | todo | | | |
| P5-03 | 审批卡 | P5-02 | 3d | todo | | | |
| P5-04 | 敏感确认卡 + 交接卡 | P5-03 | 2d | todo | | | |
| P5-05 | 工具调用卡 | P5-02 | 2.5d | todo | | | |
| P5-06 | Computer 侧栏面板 | P5-05 | 2.5d | todo | | | |
| P5-07 | 覆盖窗 | P5-01 | 3d | todo | | | |
| P5-08 | 设置页 | P5-03 | 2.5d | todo | | | |
| P5-09 | 生命周期接线 + wiki 重写 | P5-02 | 1.5d | todo | | | |
| P5-10 | 内置浏览窗 | P5-07 | 3d | todo | | | |
| P5-11 | App 内评测与发布开关 | P5-01…P5-09 | 1.5d | todo | | | |
| P6-01 | settle 调优 | P1-09, P2-09, P3-05 | 2d | todo | | | |
| P6-02 | auto 截图策略 | P6-01 | 1.5d | todo | | | |
| P6-03 | wait_for | P6-01 | 1.5d | todo | | | |
| P6-04 | computer_batch | P6-03 | 3d | todo | | | |
| P6-05 | computer_script | P6-04 | 4d | todo | | | 需安全签字 |
| P6-06 | 工具目录与 instructions 优化 | P6-05 | 1d | todo | | | |
| P6-07 | 效率 A/B 评测 | P6-01…P6-06, P9-01 | 2d | todo | | | |
| P7-01…06 | Wayland | P3-05 | 17d | todo | | | 见 06 P7 |
| P8-01…05 | 浏览器 DOM 增强 | P4-07 | 12d | todo | | | 见 06 P8 |
| P9-01 | 评测 runner | P0-10 | 4d | todo | | | 可提前到 P4 期间 |
| P9-02 | 任务集 ≥60 | P9-01 | 4d | todo | | | |
| P9-03 | 性能预算门禁 | P6-01 | 1.5d | todo | | | |
| P9-04 | 安全审查 | P6-05 | 3d | todo | | | |
| P9-05 | 兼容性矩阵 | P4-07 | 3d | todo | | | |
| P9-06 | 长稳浸泡 | P5-11 | 2d | todo | | | |
| P9-07 | 1.0 发布 | P9-01…P9-06 | 1.5d | todo | | | |

## 发现（卡外问题，一行一条）

-

## 阶段记录

-

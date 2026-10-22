# 会话导入迭代说明（2026-09-08 r5）

r4 修了分页阅读、显式根扫描、dispatch 标记失败。本轮记下 **10 个仍开放方向**，并落地本地能修的子集。OBJECTIVE 的「十轮」按十个具名缺口计，不是十次空转。

## 十个方向

| # | 方向 | 状态 | 说明 |
| --- | --- | --- | --- |
| 1 | Cursor locator 含文件系统 `#` | done-this-round | `split_composer_locator` 按 `#composerData:` 切分；预览读对 composer，不截断路径、不落到别的会话 |
| 2 | 续聊 completed/unknown 写失败可见 | done-this-round | `mark_completed` / `mark_unknown` 写旁车失败返回 `SNAPSHOT_UNAVAILABLE`，不再 `let _ =` |
| 3 | 源只读 + `imp:` + 排除项 | done-this-round | 既有约束保持；本轮测试仍覆盖 |
| 4 | 扫描边扫边出 | still-open | 目前 `scan_refresh` 是再扫一轮后进 extra |
| 5 | 向导小窗口 / 200% 缩放实机 | still-open | 本环境未开 Tauri 窗口 |
| 6 | macOS 活库与发现路径实机 | externally-blocked | 无 macOS 实机 |
| 7 | Linux 活库与发现路径实机 | externally-blocked | 无 Linux 实机 |
| 8 | 官方/自定义 × shared/independent ACP | externally-blocked | 未跑真实四组合 |
| 9 | 1 万会话 / 100MB / 1GB 性能数字 | externally-blocked | 不以测试常量冒充成绩 |
| 10 | 杀进程崩溃边界实机 | externally-blocked | 未做杀进程 |

DSH/pi 专用导入、便携包、云历史不在本轮。

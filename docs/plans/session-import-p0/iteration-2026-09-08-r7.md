# 会话导入迭代说明（2026-09-08 r7）

r6 把续聊 completed/unknown 接到发送路径，并拒绝过期 scan epoch。本轮记下 **10 个方向**，并落地本地能修的子集。「10 轮」按十个具名缺口计。

## 十个方向

| # | 方向 | 状态 | 说明 |
| --- | --- | --- | --- |
| 1 | 扫描边扫边出 | done-this-round | `scan_start` 只固化首片；`scan_pump` 把后续发现推进 `extra_candidates`，`select_current` 仍冻结 |
| 2 | extra 候选可预览 | done-this-round | `preview` 在冻结列表和 extra 里查找；向导列出 extra 并可勾选 |
| 3 | 续聊成功/失败写旁车 | done-this-round | r6 已接线；本轮回归测试仍覆盖 Prepared→Completed/Unknown |
| 4 | 过期 scan id/epoch 拒绝 | done-this-round | r6 已接线；本轮 `scan_pump` 同样校验 epoch |
| 5 | 向导小窗口 / 200% 缩放实机 | still-open | 未开 Tauri 窗口 |
| 6 | 杀进程崩溃边界实机 | still-open | 未做杀进程 |
| 7 | macOS 活库实机 | externally-blocked | 无 macOS |
| 8 | Linux 活库实机 | externally-blocked | 无 Linux |
| 9 | 官方/自定义 × shared/independent ACP | externally-blocked | 未跑四组合 |
| 10 | 1 万会话 / 100MB / 1GB 性能 | externally-blocked | 不以常量冒充成绩 |

DSH/pi、便携包、云历史不在本轮。原 53 项 / 三系统矩阵未标完成。

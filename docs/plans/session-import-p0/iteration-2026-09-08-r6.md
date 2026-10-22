# 会话导入迭代说明（2026-09-08 r6）

r5 修了 Cursor `#` locator 与 completed/unknown 写失败返回值。本轮记下 **10 个方向**，并落地本地能修的子集。「10 轮」按十个具名缺口计。

## 十个方向

| # | 方向 | 状态 | 说明 |
| --- | --- | --- | --- |
| 1 | 续聊成功后旁车 completed | done-this-round | `on_continuation_turn_success` 接到发送成功路径；Prepared → Completed |
| 2 | 续聊失败/进程死后旁车 unknown | done-this-round | `on_continuation_turn_failure` 接到发送失败与失败标记释放；Prepared → Unknown |
| 3 | 上述写失败可见 | done-this-round | 只读旁车返回 `SNAPSHOT_UNAVAILABLE`，不是 Ok、不是「找不到旁车」 |
| 4 | 过期 scan id/epoch 拒绝 | done-this-round | `scan_page` / `scan_select_current` epoch 不符报 `PLAN_STALE`；向导丢弃过期响应 |
| 5 | 扫描边扫边出 | still-open | 目前 refresh 是再扫一轮 |
| 6 | 向导小窗口 / 200% 缩放实机 | still-open | 未开 Tauri 窗口 |
| 7 | macOS 活库实机 | externally-blocked | 无 macOS |
| 8 | Linux 活库实机 | externally-blocked | 无 Linux |
| 9 | 官方/自定义 × shared/independent ACP | externally-blocked | 未跑四组合 |
| 10 | 1 万会话 / 100MB / 1GB 性能 | externally-blocked | 不以常量冒充成绩 |

DSH/pi、便携包、云历史、杀进程实机不在本轮。

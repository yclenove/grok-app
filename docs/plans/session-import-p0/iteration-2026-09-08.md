# 会话导入迭代说明（2026-09-08）

本轮只落地**本地能修的首版洞**，并记下仍开放/外部阻塞的优化方向。不是空泛的「继续优化」。

## 本轮完成

| 方向 | 状态 | 落地 |
| --- | --- | --- |
| 交接文本写回 | done-this-round | `continue_create(..., edited_text)` 把调用方编辑后的正文写入 `handoff_draft.txt`；向导把 textarea 内容传给 Host |
| 扫描「全选当前」绑快照 | done-this-round | `scan_select_current(scanId, epoch)` 只返回冻结的 `candidates`；后发现的 `extra_candidates` 不进全选 |
| 忙源/不可读 SQLite | done-this-round | Composer 打开/查询失败不再变成空列表；独占锁不再误报 `SCHEMA_UNSUPPORTED`；`SOURCE_BUSY` / `SNAPSHOT_UNAVAILABLE` 进 scan `issues` |
| 取消/撤销/恢复不复活 tombstone | done-this-round | `undo_job` 隐藏条目并写 tombstone；再 publish 同一身份得到 `tombstoned`，不新增行 |
| Grant 跨会话隔离 | done-this-round | `call_tool` 校验 `appSessionId`；会话 B 的 allow-set 搜不到会话 A 独占正文 |
| 源只读 + `imp:` 隔离 + 排除项不回流 | done-this-round | 既有约束保持；本轮测试仍覆盖 |

## 仍开放（本地可继续，本轮未做）

| 方向 | 状态 |
| --- | --- |
| 向导多根逐根映射 UI | still-open |
| 撤销已有续聊引用时的冲突报告文案 | still-open |
| 扫描增量分页（目前一次扫完再冻结） | still-open |
| 向导小窗口 / 200% 缩放 / 键盘路径 | still-open |

## 外部阻塞

| 方向 | 状态 |
| --- | --- |
| macOS / Linux 活库与发现路径实机 | externally-blocked |
| 官方/自定义 × shared/independent 真实 ACP tools 暴露 | externally-blocked |
| 1 万会话 / 100MB / 1GB 性能数字 | externally-blocked |
| 杀进程崩溃边界实机 | externally-blocked |

DSH/pi 专用导入、便携包、云历史不在本轮。

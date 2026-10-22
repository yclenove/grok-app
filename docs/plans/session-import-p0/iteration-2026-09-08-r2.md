# 会话导入迭代说明（2026-09-08 r2）

上一轮修了交接写回、扫描快照全选、忙源非空列表、tombstone、grant 隔离。本轮继续落地**本地还能修的首版洞**，不是空泛「再优化」。

## 本轮完成

| 方向 | 状态 | 落地 |
| --- | --- | --- |
| 单库失败不丢同根会话 | done-this-round | Cursor composer 单库错误记入 scan `issues`，transcript/store 仍列出 |
| 逐根映射，不用死 `primary` | done-this-round | `propose_mappings` 按 cwd 出多条；`commit` 按 root 匹配项目，不再一律用第一条 |
| 撤销时续聊引用冲突 | done-this-round | 有 `continuation_refs` 时不隐藏条目、不写 tombstone，item 报 `MAPPING_CONFLICT` |
| 扫描分页 | done-this-round | `scan_page` 按 cursor 切片；向导可「加载更多」 |
| 向导问题/撤销/键盘 | done-this-round | 扫描 issues 列表、结果页撤销、Enter 在来源步触发扫描 |
| 源只读 + `imp:` + 排除项 | done-this-round | 既有约束保持 |

## 仍开放（本地可继续）

| 方向 | 状态 |
| --- | --- |
| 向导小窗口 / 200% 缩放实机 | still-open |
| 映射步选已有项目列表（需项目目录 IPC） | still-open |
| 扫描增量发现（目前一次扫完再冻结） | still-open |

## 外部阻塞

| 方向 | 状态 |
| --- | --- |
| macOS / Linux 活库与发现路径实机 | externally-blocked |
| 官方/自定义 × shared/independent 真实 ACP tools | externally-blocked |
| 1 万会话 / 100MB / 1GB 性能数字 | externally-blocked |
| 杀进程崩溃边界实机 | externally-blocked |

DSH/pi 专用导入、便携包、云历史不在本轮。

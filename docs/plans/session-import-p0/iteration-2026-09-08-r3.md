# 会话导入迭代说明（2026-09-08 r3）

r2 修了单库失败不丢会话、逐根映射、续聊撤销冲突、扫描分页。本轮只落地**仍开放且本地能修**的洞。不是「无限迭代」。

## 本轮完成

| 方向 | 状态 | 落地 |
| --- | --- | --- |
| 冻结快照后的增量发现 | done-this-round | `scan_refresh` 把后发现的条目写入 `extra_candidates`；`scan_select_current` 仍只返回冻结列表 |
| 逐根映射到已有项目或暂不关联 | done-this-round | 向导每条 root 可选已有项目 id 或 unassigned；`commit` 按 cwd 匹配，第一条选择不覆盖其他根 |
| 失败项重试不重复目录行 | done-this-round | `retry_failed` → 作业队列重放 durable preview；已提交身份不新增可见目录行。取消后再发布也走同一输入，不另开内存副本 |
| 源只读 + `imp:` + 排除项 | done-this-round | 既有约束保持；本轮测试仍覆盖 |

## 仍开放（本地可继续）

| 方向 | 状态 |
| --- | --- |
| 向导小窗口 / 200% 缩放实机 | still-open |
| 扫描边扫边出（目前 refresh 是再扫一轮） | still-open |

## 外部阻塞

| 方向 | 状态 |
| --- | --- |
| macOS / Linux 活库与发现路径实机 | externally-blocked |
| 官方/自定义 × shared/independent 真实 ACP tools | externally-blocked |
| 1 万会话 / 100MB / 1GB 性能数字 | externally-blocked |
| 杀进程崩溃边界实机 | externally-blocked |

DSH/pi 专用导入、便携包、云历史不在本轮。

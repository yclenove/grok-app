# 会话导入迭代说明（2026-09-08 r4）

r3 修了冻结扫描增量、逐根映射、失败重试。本轮只落地**仍开放且本地能修**的洞。不是「继续迭代」口号。

## 本轮完成

| 方向 | 状态 | 落地 |
| --- | --- | --- |
| 分页阅读不整本加载 | done-this-round | `events_page` 从修订索引按页取 `event_json`；越界 cursor 报错，不伪装成空历史 |
| 显式根扫描 | done-this-round | `scan_start` 带调用方路径时只扫该路径；向导可选自定义目录，不再只能默认根 |
| dispatch 标记失败可见 | done-this-round | `mark_dispatching` 写旁车失败返回 Err；发送路径不再 `let _ =` 吞掉 |
| 源只读 + `imp:` + 排除项 | done-this-round | 既有约束保持 |

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

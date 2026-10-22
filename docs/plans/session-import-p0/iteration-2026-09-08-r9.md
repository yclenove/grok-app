# 会话导入迭代说明（2026-09-08 r9）

r8 修了文件当根伪装零会话、预览分页、源改写 `SOURCE_CHANGED`。本轮记下 **30 个 r8 之后仍开放的方向**，并只修本地能修的子集。「30 轮」按三十个具名缺口计。

## 三十个方向

| # | 方向 | 状态 | 验收/REVIEW |
| --- | --- | --- | --- |
| 1 | 导出选中 leaf 分支，不含兄弟链、不含 system | done-this-round | U06 |
| 2 | 取消后不再启动仍 queued 的项 | done-this-round | R03 |
| 3 | continue 同一 request_id 崩溃后修复旁车，不新开会话 | done-this-round | C10 |
| 4 | 分叉后真实文件变更端到端 | still-open | F04 |
| 5 | 未知块 + 缺图 + 超大输出三类同文件 UI | still-open | F08 |
| 6 | `.code-workspace` 缺 root 向导实机 | still-open | W02 |
| 7 | 同文本路径不同 SSH host 不串项目 | still-open | W03 |
| 8 | 用户映射与 cwd 建议冲突的证据 UI | still-open | W04 |
| 9 | 不覆盖已有项目信任/置顶 | still-open | W05 |
| 10 | 不 checkout / 建分支 / 恢复代码 | still-open | W06 |
| 11 | 失效附件不自动打开同名文件 | still-open | W07 |
| 12 | 已有 Grok 子会话刷新新 revision | still-open | D02 |
| 13 | 同 native 正文分歧不拼接时间线 | still-open | D03 |
| 14 | 跨库 bubble 有界重试 | still-open | S03 |
| 15 | 磁盘满 / 索引权限失败不假成功 | still-open | R07 |
| 16 | 杀进程崩溃边界实机 | still-open | R01–R07 |
| 17 | 未点发送无模型请求的真实日志 | still-open | C02 |
| 18 | 真实 ACP prompt 含钉选约束 | still-open | C03 |
| 19 | 恶意 system 文本 UI 不展开 | still-open | C05 |
| 20 | 同进程 A grant 复用于 B 实机 | still-open | C06 |
| 21 | SSH/WSL 不显示完整检索就绪 | still-open | C08 |
| 22 | CLI 未安装时 setup gate 实机 | still-open | U02 |
| 23 | 向导小窗口 / 200% 缩放实机 | still-open | U04 |
| 24 | 跨机 Windows→macOS 路径实机 | still-open | W01 |
| 25 | 向导预览分页 UI 接线 | still-open | REVIEW 预览 |
| 26 | 标准 graph 全量内存 | still-open | REVIEW |
| 27 | 1 万会话 / 100MB / 1GB 性能数字 | externally-blocked | F12 / U03 / U07 |
| 28 | macOS 活库实机 | externally-blocked | 三平台 |
| 29 | Linux 活库实机 | externally-blocked | 三平台 |
| 30 | 官方/自定义 × shared/independent ACP | externally-blocked | C07 |

r8 已落地的 S02/S04/预览索引分页不重复列为开放项。DSH/pi、便携包、云历史不在本轮。原 53 项 / 三系统矩阵未标完成。本环境未开 Tauri 窗口，向导空白/缩放/杀进程不做静默通过。

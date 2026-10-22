# 会话导入迭代说明（2026-09-08 r8）

r7 落地了扫描首片 + `scan_pump` extra 与 extra 预览。本轮记下 **30 个方向**，并只修本地能修的子集。「30 轮」按三十个具名缺口计，不是三十次空转。

## 三十个方向

| # | 方向 | 状态 | 验收/REVIEW |
| --- | --- | --- | --- |
| 1 | 非 generic 根是文件/不可列目录时报 issue，不伪装零会话 | done-this-round | S02 |
| 2 | 预览后续页从 prepared 索引读取，不把整段 graph 再塞 IPC | done-this-round | REVIEW 预览内存；U01 分页 |
| 3 | 预览后改源内容报 `SOURCE_CHANGED`（content hash，不靠 mtime） | done-this-round | S04 |
| 4 | 分叉后真实文件变更端到端 | still-open | F04 |
| 5 | 未知块 + 缺图 + 超大输出三类同文件 UI | still-open | F08 |
| 6 | `.code-workspace` 缺 root 向导实机 | still-open | W02 |
| 7 | 同名不同 cwd / SSH host 不串项目 | still-open | W03 |
| 8 | 用户映射与 cwd 建议冲突的证据 UI | still-open | W04 |
| 9 | 不覆盖已有项目信任/置顶 | still-open | W05 |
| 10 | 不 checkout / 建分支 / 恢复代码 | still-open | W06 |
| 11 | 失效附件不自动打开同名文件 | still-open | W07 |
| 12 | 已有 Grok 子会话刷新新 revision | still-open | D02 |
| 13 | 同 native 正文分歧不拼接时间线 | still-open | D03 |
| 14 | 跨库 bubble 有界重试 | still-open | S03 |
| 15 | 取消时第 5 项正在 publish | still-open | R03 |
| 16 | 磁盘满 / 索引权限失败不假成功 | still-open | R07 |
| 17 | 杀进程崩溃边界实机 | still-open | R01–R07 |
| 18 | 未点发送无模型请求的真实日志 | still-open | C02 |
| 19 | 真实 ACP prompt 含钉选约束 | still-open | C03 |
| 20 | 恶意 system 文本不展开/不执行 | still-open | C05 |
| 21 | 同进程 A grant 复用于 B 实机 | still-open | C06 |
| 22 | SSH/WSL 不显示完整检索就绪 | still-open | C08 |
| 23 | continue 崩溃后同一 appSessionId 实机 | still-open | C10 |
| 24 | CLI 未安装时 setup gate 实机 | still-open | U02 |
| 25 | 向导小窗口 / 200% 缩放实机 | still-open | U04 |
| 26 | 全局搜索打开导入历史并导出分支 | still-open | U06 |
| 27 | 1 万会话 / 100MB / 1GB 性能数字 | externally-blocked | F12 / U03 / U07 |
| 28 | macOS 活库实机 | externally-blocked | 三平台 |
| 29 | Linux 活库实机 | externally-blocked | 三平台 |
| 30 | 官方/自定义 × shared/independent ACP | externally-blocked | C07 |

DSH/pi、便携包、云历史不在本轮。原 53 项 / 三系统矩阵未标完成。

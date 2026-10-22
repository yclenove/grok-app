# SQLite / WAL 快照实验

更新：2026-10-08。平台：Windows 合成测试。

当前方法：测试创建 WAL 库，保持写连接；应用先复制源 DB/WAL 到自己
新建的私有目录，核对源 `.db` / `-wal` / `-shm` / `-journal` 的存在性、
长度及 SHA-256，然后仅让 SQLite 打开私有副本。`mode=ro` 也可能创建或
修改 SHM，因此当前实现不直接让 SQLite 打开另一应用的源库。
旧文档所述“对源库开 readonly connection 后 backup”已不是当前路径。

| 项 | 结果 |
| --- | --- |
| 已提交 WAL 正文可读 | `committed_wal_is_visible_and_source_files_do_not_change`、`composer_wal_body_is_read` |
| 源文件 hash 前后不变 | `composer_wal_source_files_stay_byte_identical`、`committed_wal_is_visible_and_source_files_do_not_change` 实际通过 |
| 没有源 SHM 的离线 WAL | `offline_wal_without_shm_is_read_without_creating_source_files` 实际通过；私有副本内允许 SQLite 管理自己的临时文件 |
| 长路径 | `long_path_wal_backup_keeps_source_readonly`、`private_snapshot_accepts_long_temp_in_fresh_process` 实际通过 |
| 不 checkpoint / VACUUM / 修库 | 无写 SQL |
| 持续锁 / rollback journal | `locked_file_reports_busy_without_repair`、`rollback_journal_is_busy_and_does_not_repair_source` 实际通过 |
| 跨库更新 | `busy_companion_db_retries_then_partial_without_invented_bubbles`；有界核对，不能假定 workspace/global DB 原子一致 |
| 未知 schema | Store 无 blobs 表 → `SCHEMA_UNSUPPORTED` |

当前执行日志为
`validation/runs/v245-title-fa98e2f14ee04387a64e9d96218f4851/migration-tests.stdout.log`；
完整领域 849 passed / 0 failed / 13 ignored。测试库均为运行时合成，未读
用户 Cursor/Codex/Claude 数据。源码为 `src-tauri/src/session_migration/sqlite_ro.rs`。

尚未验证安装版 Cursor 正在写入的活库、原厂多库更新行为以及 macOS/Linux
原平台文件锁/权限/WAL。跨平台 API 的使用不是跨平台实机证据。

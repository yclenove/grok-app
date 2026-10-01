# Computer Use：Windows journal 并发发布修复

日期：2026-09-30（本机 +08:00）；工作区 `H:\aicoding\grok-app-computer-use`。
分支 `feat/computer-use-implementation`；HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。

**完整 goal 仍为 active / partial — not releasable。** 上轮为 progress；本轮先封存失败/取消回调，再复现并修复 helper snapshot 与 journal 发布的原生竞态。没有把 Windows updater 当作整个 Computer Use 最终版。

## 红灯证据与定位

上一阶段的一次 `worker.authorize` 异常只有 `Access denied (5)`，缺少 API 阶段。五次单项和后续全量通过不能证明它已修复。本轮新增四个真实 reader 与发布者共享私有 journal 的并发测试，在第一次替换即稳定出现相同 Win32 错误。生产诊断增加 `serialize / protect / create-and-flush / inspect-target / replace / readback` 阶段后，失败明确发生在 `journal publish/replace`。

独立 ctypes 原生探针只处理自己创建的惰性文件，结果显示：目标被 READ | DELETE 共享的读句柄持有时，MoveFileEx(REPLACE_EXISTING) 返回 error 5；ReplaceFile 和 FileRenameInfoEx POSIX 操作可保留旧句柄并发布新路径。随后更强的并发 Rust 测试否决了 ReplaceFile 候选方案：reader 会碰到短暂无文件的 error 2。该实验及红灯日志均保留，**ReplaceFile 未进入最终产品路径**。

FileRenameInfoEx 初次回归又明确暴露旧 snapshot 被 unlink 后链接数变成零；原有严格单链接验证将其误拒绝。修复只针对已打开的只读 helper snapshot，不放松安装器、App image 或 owner readback 的文件约束。

这是一个已复现、定位并修复的真实缺陷；由于历史异常未带阶段，不能反向断言此前每次 error 5 都由同一原因导致。

## 已落地行为

- 已有 active record 使用 `SetFileInformationByHandle(FileRenameInfoEx)`，仅 `REPLACE_IF_EXISTS | POSIX_SEMANTICS`。临时文件先 write-through 写入和 `sync_all`；原生 UTF-16 buffer 长度检查并保持 HANDLE 对齐。
- 替换源使用 DELETE / attributes access，不开放 WRITE sharing。初次空 store 创建保留 no-overwrite MoveFileEx + WRITE_THROUGH；不删除旧文件再创建，不在错误后重试或切换旧 API。
- 每个写入阶段保留原始错误诊断；发布不确定时继续 poison 当前 journal、保留证据并阻止下一次派发。readonly 属性和不共享 DELETE 的读句柄仍会拒绝替换，错误不会授权 replay。
- 仅 helper 的只读 snapshot 可接受 POSIX 替换后零链接的旧句柄，仍拒绝多链接、reparse point、目录，且仍验证 DPAPI、schema、原候选和 ticket。旧句柄读取旧完整记录，新路径读取新完整记录。
- 新增原生测试覆盖旧/新记录分离、Unicode/空格/$/emoji 路径、禁止原地写入、readonly 不被覆盖、四 reader 并发发布，以及同一次失败不产生第二次写入。

修改范围为 `windows_journal.rs`、新增 `windows_journal/publication.rs` 和 vendor patch 说明，未改 UI/权限/签名策略、installer dispatch 次数或 journal schema。

## 本轮验证

证据根：`tools/computer-use-probe/.run/windows-journal-publication-20260930/`。
前阶段不可变回执：`windows-update-failure-evidence-20260930/receipt.json`，SHA-256 `71f4c97907c0094bc2d038fa9939e876a86147391d05b48981d8bef633ad45eb`。

| 检查 | 结果 | 范围 |
| --- | --- | --- |
| 原生 journal 定向 | **11 passed** | 包含实际 OS sharing/readonly、DPAPI、旧句柄与并发发布，不是内存 mock |
| 默认 updater library | **99 passed / 0 failed / 9 ignored** | default,rustls-tls,zip |
| 独立 no-zip updater | **98 passed / 0 failed / 9 ignored** | 独立 consumer，仅 rustls-tls |
| 显式 NSIS parser | **两套各 2 passed** | 消费上一阶段成功/失败/取消原生回执；本轮未重新执行 NSIS |
| 重复验证 | **两个用例各 20/20** | 四 reader × 每次 128 次发布；原 `dead_authorized_worker...` 独立子进程用例；不是新业务能力计数或 12h soak |
| App 直接关联 | **38 passed** | updater 7 / shutdown 3 / browser process 7 / MCP session 21，不是完整 App suite |
| 前端恢复链 | **4 files / 99 passed** | 本轮实际重跑 |
| 静态检查 | **两套严格 Clippy、TypeScript、workspace/vendor fmt、diff 通过** | 不代表远程 CI 或发行验收 |
| 选定源和权限 | **116 项源核对、仅 3 项变化** | local main/session-* ACL、上游 IIFE 和 App Cargo.lock 未改变 |

每套 9 个 ignored 中，两个原生 parser 本轮显式运行；其余 7 个为父测试调用的受控子进程入口。默认和 no-zip 使用实际 Cargo compiler-artifact 确定测试二进制，嵌入测试 manifest 后运行；二进制与日志一起归档。重复验证和全部相关测试在同一源冻结后执行。

当前实测机器为 Windows 11 x64 build 26200，C/H 卷均 NTFS。FileRenameInfoEx POSIX 的 SDK gate 为 Windows 10 RS1；不支持的 OS/文件系统应明确失败并继续阻止派发，未加入静默兼容降级。**旧 Windows、其他文件系统、突然断电持久性未验证**，不能外推当前实机结果。

## 剩余完整目标

Windows x64、macOS arm64/x64、Linux X11 与原生 GNOME Wayland；Desktop / managed browser / 已有 Chrome/Edge tabs / App WebView；App / ACP / MCP；完整 actions/input、中文 IME、clipboard、取消及恢复；签名 clean install/update/repair/rollback/uninstall；原生窄窗口/DPI/权限 UI；真实 Grok E4；**同一个最终冻结候选 12h active soak**，均继续按原总计划逐项验收，不能由本轮局部通过替代。

当前下一步仍包括真实安装态失败后的 repair/rollback、支持平台/文件系统的兼容验收及其余产品缺口。没有执行真实 Grok 安装器/App/卸载器，没有修改真实安装注册信息，没有 commit、push、PR、tag、发布或把 goal 标记完成。

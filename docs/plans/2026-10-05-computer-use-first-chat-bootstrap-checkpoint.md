# 首次聊天自动准备 CU 运行时

2026-10-05；`feat/computer-use-implementation`；单 Codex writer。
完整目标仍 active / partial — not releasable，本批未提交、推送或发布。

## 实际问题与修复

app97 从 source96 打出并能启动，但全新 App home 没有随包 Node 和浏览器运行时。
首次聊天的 RequestTransport → request MCP → build_entry 只写入嵌入式 MCP 脚本，
随后解析 js-runtime 失败。缺少工具的聊天不能算 Computer Use 已可用。

source98 在 Tauri setup 记录 resource_dir 与主程序目录；不复制文件、不启动控制
surface，也不授予权限。首次本地聊天构建 MCP 时，从安装包校验并准备私有 runtime。
已健康运行时直接复用；可以迁移内容与清单均核实的精确两件 MCP-only pack。
损坏或未知 active pack、损坏 pointer、遗留非空 staging 均保留原现场，要求明确修复。
缺少安装来源时不使用 PATH Node。

准备、嵌入 MCP、显式修复和 rollback 共用进程内 mutex 与 `.runtime.lock` 文件锁。
guard 内直接使用 RuntimeStore，避免递归取得同一锁。诊断写 MCP 也走同一包装。
功能默认关闭和按任务申请/选择/批准的产品授权保持。

## 已取得证据

证据根：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。

| 检查 | 本批结果 |
| --- | --- |
| source98 冻结与复验 | 3318 文件；SHA-256 `c26689e4b4459f1b5c061d41990fa6b2db458c18343e0f7f19149672554d64f7` |
| 默认 lib 测试编译 | exit0；测试 EXE SHA-256 `142ecc9263c801f4a1385998239c8823f2dc2404d106d954289110a66690f8ea` |
| `computer_use::runtime::` | 10 passed / 0 failed / 0 ignored，60.87s，exit0 |
| App 默认 strict Clippy | exit0，36.23s |
| App probe strict Clippy | exit0，37.52s |
| cargo fmt / git diff 检查 | exit0；已有进度文档 CRLF 提示保留，不扩大修改 |
| 项目 final quality gates | 全部通过；App+Workbench 13573 行，大文件 80 个，未增长 |

新增六项覆盖空 home、精确 MCP-only 迁移、损坏 active、损坏 pointer/staging、
安装来源缺失及两连接并发。正向使用真实随包 seed，执行私有 Node 得到 v20.18.0，
同时核实两件嵌入式 MCP 内容。旧 unwind 测试的预期 panic 被 catch，最终通过。
源码与 source96 相比只涉及 inject/runtime/lib 及新增 bootstrap/tests 五个文件。
Core、前端和扩展源码未变；没有重复旧 81 项矩阵或把旧数字记成本批运行。

## 应用交付与未验边界

实际应用包进展见 [交付状态](2026-10-05-computer-use-delivery-status.md)。
lib 测试和 setup 注册路径不能代替最新应用里的冷聊天、真实模型与授权后业务操作。
app97 原预览进程仍由用户持有，不覆盖或停止。新应用包采用独立输出身份。
真人 fixture 任务结果尚未确认；`http://127.0.0.1:4761/state` 当前仍为未保存、计数0。
不因缺少真人操作而重新启动原 server，也不把自动化批准算作真人批准。

下一步先交付当前 Windows 包，验证真实聊天 → 请求授权 → 操作 → 结果核验 →
暂停/继续/停止。已有浏览器入口目前仍关闭；受管浏览器新 TaskGrant 恢复、
App WebView、全部桌面输入和三平台/两 Mac 架构/安装升级/12h/UI UX仍有未完成项。
完整首版范围保留；Windows 开发试用不等于正式交付。

## 产物卫生

大构建前 C/H 空闲约277/365 GiB，未把同期磁盘活动归因于本任务。
当前应用、便携包、seed、依赖、源及失败证据保留；原 app97 与 fixture server
owner 尚存活，相关产物延期。所有清理须使用新的精确身份、核实 owner 退出、
拒绝 reparse point，并区分逻辑大小和实际回收空间。

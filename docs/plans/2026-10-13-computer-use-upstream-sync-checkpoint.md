# Computer Use：fork / 源仓库同步检查点

2026-10-13，分支 `feat/computer-use-implementation`。本批落实用户的
“合适的时候拉下最新代码”；保持原 CU 开发内容及完整交付范围。

## Git 边界

- fork `origin/main`：`b58b97cd61522a9e92095cfcb1d8d1aa664edda5`。
- 源仓库 `upstream/main`：`9c5eb3b589fa6234258cb0bcbb26e5ee0971d8bc`。
- 原 1,014 份源码 / 测试 / 文档分别保存在 `d2aa14d6`、`1922f446`、
  `d9c192f1`，内容身份核验保持；这些是 WIP 检查点，不是交付声明。
- fork 已通过 `ab1fc5bb` 合入。源仓库冲突与完整本地回归已完成，
  保留其独立合并边界；提交 / push 身份以本分支 Git 记录及同批回执为准。
- 收尾再次 fetch 两个远端，以上两个 main SHA 均未移动。
- `.run`、运行时来源 / seed、用户 home、登录、草稿、预览包及未知来源
  `fix.patch` 不加入本次提交。未创建 PR / tag / Release。

## 合并处理

保留上游 ACP / 会话 / 上下文窗口 / 输入框 / 子代理和 PR monitor 改动，
同时保留 CU 的授权、MCP catalog 协调、异步清理及精确进程归属。
握手 Stop 继续按原 endpoint 清理，并返回上游新增的 `StopReport`。

上游新加的 live 上下文窗口切换原本直接调用 ACP。现已补上同步 CU fence，
在 RPC 之前撤销同一聊天的权限；慢速清理独立进行，RPC 失败也不恢复权限。
无效尺寸、其他聊天和新对话暂存选择不撤销当前聊天。
真实隔离 Broker + TCP ACP 回归覆盖请求顺序、清理延迟、RPC 失败和会话隔离。

上下文窗口菜单 / slash / 用量显示提取至 `useComposerContext`，
保留官方和自定义渠道行为。离开聊天后到达的旧上下文响应不能覆盖新聊天。
App + Workbench 合计 **13,552** 行，比原冻结值 **13,573** 少 21 行；
没有提高质量门禁。≥1,000 行源码数量仍为 80。

主 CHANGELOG 的所有已发布版本章节与源仓库逐字一致，版本标题唯一。
fork 不同的 0.2.39 / 0.2.40 原章节完整归档于
[fork 发行历史](2026-10-13-computer-use-fork-release-history.md)。
fork 的 Markdown 编辑器修复保留在 Unreleased；三个版本字段均为 0.2.40。

## 本批验证

证据根：`tools/computer-use-probe/.run/cu-upstream-sync-20261013-01/`。

| 检查 | 本批结果 |
| --- | --- |
| frozen-lockfile 安装 | exit 0 |
| TypeScript / ESLint | exit 0 |
| 上下文针对性前端回归 | 4 文件 / 68 passed |
| 完整前端回归 | 743 文件 / 8,578 passed |
| 前端生产构建 | exit 0；保留既有大 chunk 提示 |
| Rust fmt / strict clippy，all targets | 最终均 exit 0；两次编译首败日志保留 |
| Rust harness | 当前 Cargo 返回的 EXE 已嵌入 Common Controls v6；上下文 3 passed，完整 App 2,399 passed / 0 failed / 87 ignored，main harness 0 tests / exit 0 |
| website 发布脚本 self-test | 3 passed |
| 质量门禁 / diff whitespace | exit 0 |
| 版本 / CHANGELOG 原文核验 | exit 0，报告保留 |

首次 clippy 暴露 CU 测试 fixture 缺上游 `last_provider_retry` 字段，已补齐。
第二次暴露新增回归的测试模块路径 / 方法可见性问题，已修复；不删除首败。
新上下文回归首跑有一项等待 catalog 超时；单独复现通过，代码回读定位到生产
MCP builder 的冷 CLI 探测超出纯 TCP helper 的 2 秒预算。现按 catalog 路径
单独等最多 15 秒，同时明确断言 retune 收到 ACP 回复后 2 秒内完成，不得等待
未完成的 catalog / 故意阻塞的 surface 清理。三项新版回归通过，首败与复现日志保留。
完整 App 默认并行首跑 2377 passed / 22 failed / 87 ignored：其中一项为 hooks
在缺 `findstr` 的 PATH 下执行，一项为全局撤销测试干扰上下文 peer grant，
其余 20 项是共享 home mutex 被该失败毒化的连锁错误。加现有 feature 测试锁后，
第二轮默认并行 2398 passed / 1 failed / 87 ignored；CU 与连锁错误已消除。
剩余 hooks 单独复现通过；回读确认 CU runtime 测试用 shared home env lock
临时设置 decoy PATH，而 hooks 测试未拿此锁。已补同一环境锁，第三轮默认并行
整套 **2399 passed / 0 failed / 87 ignored**，耗时 188.94 秒。
最终 Cargo 编译、两份准确 harness 和 strict clippy 的原 owner 均已 joined exit 0。
测试进程 PATH 的前置修正只作用于本次 shell，没有修改用户系统环境。

## 尚未完成的发行与产品门禁

fork 新增的 Linux ARM64 发行 leg 保留且明确使用 ARM64 triple，
绝不替换为 x64 CU seed。现有 CU 四目标资源和私有 Linux SDK 只覆盖 x64 Linux，
因此 **ARM64 release leg 仍被阻塞**。需单独完成架构 / SDK / 资源兼容方案
及 native CI，不能将本次本地同步写成所有发行 CI 已通过。

App macOS 11+ / CU 受管浏览器 macOS 14+ 不变。产品仍使用旧运行时；
Node 22 / Playwright 1.64 / Chromium 157 候选未晋升。
完整 NOTICE / 源提供、维护运行时、原 R1–R4 的所有平台 / surface / 安装 /
真实模型 / 同最终候选 12h / D00–D31 门禁继续 active / partial。
本批单元与构建结果不替代实机验证，也不自动发送用户草稿。

## 清理

当前唯一用户预览与原隔离 home 保留。测试 / 编译原 owner 退出并复查无 live
编译 / 本批 harness 后，仅移除下列两份明确更旧的 incremental generation；
最新 successor、当前构建、依赖、source / seed、成功与失败证据均保留。
下列路径相对 `src-tauri/target/debug/incremental/`：

- `grok_app_lib-1f9k1u6zcqfys/s-hn61mga5kq-0ij1t5q-ca2m8l7yecicalcn99a71fw7i`
- `grok_app_lib-209oju03cmede/s-hn61mga8ae-1euae71-a5vnh790kirn7dlgtjw277osf`

逻辑文件大小 **5,849,500,038 bytes**；H 盘即时可用空间实测增加
**2,394,234,880 bytes（约 2.23 GiB）**。二者分别记录，硬链接及并发磁盘活动
可能使实际变化与逻辑大小不同。完整绝对路径、保留 successor、时间和前后空间
在本批 `cleanup-obsolete-incremental-receipt.json`。

首次预检发现第三份拟退休缓存已经不存在，尚未进入删除循环；原失败记录在
`cleanup-first-attempt.txt`。复查后将其记为 `alreadyAbsent`，不计作本次清理成果。
其他工作区或归属不明缓存继续暂缓，未停止任何无关进程。

这些缓存可由 `cargo test --manifest-path src-tauri/Cargo.toml --locked --no-run`
及 `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings`
重新生成；无需恢复源码或用户数据。

# 开发版启动

2026-10-21：已保存到 `handoff/session-import-claude-20261021`，代码检查点
`572ab60a`。从 [Claude 交接入口](../2026-10-21-session-import-claude-handoff.md)
和 [接手提示词](../2026-10-21-session-import-claude-prompt.md) 继续。
V814 已修复分支默认值，通过完整门及10-16限定合成图片桌面/同路径重启；
10-21只读审计通过。固定入口仍V789，候选提升留给下一批，不称全53项交付。

当前开发和验收工作树：
`H:\aicoding\grok-session-import-validation\integration-rehearsal-v243`。
代码与台账已保存为本地交接提交；不要覆盖或清空工作树，接手时先核对新改动。
原 `grok-app-session-import` 路径及下方旧测试数字不代表当前代码状态。

## 用户试用入口

双击固定入口：
`H:\aicoding\grok-session-import-validation\start-current-preview.cmd`。
当前入口指向 V789（2026-10-16），含有界历史版本列表/选择、固定revision阅读/
搜索/导出/续聊及显式重开已完成导入任务。N787导入1299/全库4303通过且strict
Clippy通过（集合重叠不相加，103/107ignored不计通过）；F792七门897文件/
10393项与F793 bundle通过。实际中文Windows合成流程中，新导入3条Claude，创建
未发送的续聊草稿，重启后确认撤销：2条隐藏、1条因引用保留，再重启状态仍在。
复制V762 profile的尝试被绝对路径认证拒绝，原V762未撤销，不能混为同一次成功。
其余旧版本、关联和容量记录保留各自证据；真实来源/渠道、UNKNOWN100、扩展性、
IME/DPI/15语言实机和macOS/Linux等未完成，不代表完整V5/53项交付。尚无实际派发。
版本、程序哈希和验收范围以同目录的 `current-preview.json` 与
`CURRENT-PREVIEW.md` 为准。新版构建及桌面验证通过后才更新这个入口。
不要直接启动内部测试目录里的多个 `grok-app.exe`。

只检查入口、不打开 App：

```powershell
& 'H:\aicoding\grok-session-import-validation\start-current-preview.cmd' -CheckOnly
```

## 开发与测试

需要热更新时，在当前工作树按 `CONTRIBUTING.md` 使用 `pnpm dev`；
不要裸跑 `tauri dev`。必须使用本任务的隔离 `GROK_APP_HOME`、来源根和
版本标识，保留用户的真实会话、凭据及其他对话的开发进程。
遇到端口占用，先核对进程所有者；禁止停掉其他对话或用户的 App。

自动桌面测试同一时间只启动一个 App，窗口标题包含功能和版本。
每轮成功、失败或中断后，使用应用退出命令退出 App，核对托盘和全部
子进程归零，再清理该轮不用的缓存和已替代产物。关闭窗口可能仅隐藏到
托盘，不能当作退出。用户正在试用的预览留给用户结束。

任务本地产物规则与可复用启动、退出、清理脚本位于
`H:\aicoding\grok-session-import-validation\AGENTS.md` 和 `scripts`。
Windows 安全权限弹窗需要用户处理；自动测试不得代点。

## 入口

- 侧栏「导入会话与工作区」
- 项目右键「导入到此项目」
- 设置 → 通用 → 应用 → 导入记录
- 账号页原「导入对话」改为打开同一向导

## 数据目录

`{GROK_APP_HOME 或 %APPDATA%/grok-app}/imports/`

- `catalog.sqlite`
- `entries/<imp_uuid>/<revision>/manifest.json` + `events.jsonl` + `event-index.sqlite`
- 不写 `sessions_index.json` / 伪 `messages.json`

续聊子会话在 `sessions/<uuid>/import_continuation.json`。

## Windows 单测

Tauri 测试二进制需要 Common Controls v6 清单，否则 `0xc0000139`
可能使测试进程在进入断言前退出。每次重新链接后都必须核对清单。
不要按“最后修改的 EXE”猜测测试程序；使用记录了源码、精确二进制路径、
哈希、清单和退出结果的任务 gate。

当前可复用 gate 是产物目录中的 `scripts/session-import-title-gate.ps1`。
全仓检查仍按 `CONTRIBUTING.md` 执行；编译通过、测试通过、合成产品
测试和真实来源/真实 ACP 验收必须分别记录。

## 不要

已获准的仅本地 WIP 检查点与 upstream/main 合入已完成，不要重复执行。
10-21用户已授权交接分支和本地提交；无进一步授权不push、提PR或发布。
后续开发范围由接手会话确认，源应用数据库始终只读。

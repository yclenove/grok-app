# 上传授权正向回归与灰度门禁检查点

2026-10-12；分支 `feat/computer-use-implementation`。整体仍 partial，不缩小跨平台、四 surface、installed、模型、12h 或 D00–D31。

## 本批实际变更

新增 `permission/granted_upload.rs`：原 ACP permission handler 接固定 wire metadata，使用实际 Broker 与 App WebViewAdapter；保留页面和 Host approval 为 synthetic，不声称 live CLI/model/native picker。
复用已有 TestView，只新增 test-only upload 字段与 fixture；不修改正式 adapter、upload schema、权限默认值或 App/AppWorkbench。

正向回归实际验证：

- 原 bound agent、active turn、有效 TaskGrant 和原 observe 身份返回 allow-once，不返回 allow-always。
- 同请求两次权限检查不执行原页面 script、不建立 upload pending、不选文件、不消耗该次文件选择。
- 旧 agent、非 active turn、错误版本/run/generation/snapshot/geometry/ref、模型 path 字段均 reject-once。
- Pause 和 Stop 分别退休执行权限，旧 upload 不重新派发；原 client 与 cleanup tickets 收尾。

## 正式入口不能靠解除编译门禁销账

检查发现 `task_view::present_view` 的 Windows app-webview descriptor 受 `computer-use-probe` 控制。
继续读原规划 `2026-10-06-computer-use-app-webview-task-next.md` §“真实任务验证后再公开入口”，确认这是刻意保留至真人验收的灰度条件，而非单凭 adapter 已编译即可开放。

source130 曾试验无 probe 的 descriptor 开放，分层 171 passed/84 ignored、strict 通过；但不满足上述真人合同，故只撤回本批这一试验（未撤其他 WIP）。该候选不是已认可的产品入口修复或交付证据。
source131 恢复原 gate；现有用户 app129 仍不提供普通编译包的 app-webview 任务选择。不得把其正常首页/历史/设置视为内嵌网页 CU 已可用。

## 本批实际验证

source131：3414 文件，SHA256 `5c437a680e05afb51c3daea4a32f8a62f8a100fc95b6df3614d73bdff00adee4`；冻源回读通过。
默认无 probe 的 Cargo App libtest 构建通过；使用唯一 Cargo executable 副本，嵌入并回读项目 Windows manifest 后执行。

| 分层 | Passed | Failed | Ignored 未运行 |
| --- | ---: | ---: | ---: |
| ACP CU（包含新增正向回归） | 65 | 0 | 0 |
| runtime（含随包 Node 内嵌 catalog） | 11 | 0 | 0 |
| Host 文件 reader | 1 | 0 | 0 |
| WebView adapter | 49 | 0 | 0 |
| SessionManager CU | 46 | 0 | 84 |
| 去重合计 | 172 | 0 | 84 |

原 runner exit0，原测试 owners 已退出。新 runner 分别发现所有测试与 ignored 测试，检查实际 runnable/ignored 数相等；关键正向回归必须存在且不得 ignored。没有把 84 项原生/长时测试计为通过，也没有宣称全 App suite green。
证据 `src-tauri/target/codex-test-runtime/app-upload-permission-unit-20261012-01/verified-results.json`。前批 runner exit1 的发现数误判保留，不回写成成功。
默认 lib strict、默认 lib+tests strict 和 probe lib+tests strict 均实际通过（原 session57646/90043 exit0）。fmt/diff/final quality 通过；13573 行、≥1000 文件80/80未增长。

## 当前窗口与清理

唯一用户窗口仍为 app129 PID78724（创建 UTC `2026-10-11T18:43:13.2130113Z`），路径 `dist-portable/Grok_CU_20261012_app129/Grok.exe`。未改其 home、登录、会话或权限；没有重开其他窗口、提交模型任务或代点授权。

已删本批两个已退出的 App libtest 重复 EXE，逻辑152428032 bytes；H 空闲净观测增加140701696 bytes。现场有其他磁盘活动，净差不是独占回收归因。
保留源 test executable、manifest/readback、logs/results、所有 homes、当前包/依赖及有用增量缓存；无 broad directory/cache 清理。可用保留 Cargo 命令再构建并在 fresh run 复制、embed/readback，不覆盖旧证据。
清理证据 `src-tauri/target/codex-test-runtime/cleanup-upload-permission-20261012-01.json`。

## 下一真实产品链

1. 用带 probe 灰度编译能力但普通产品入口的 fresh 完整测试包验证原 ACP/MCP；不得用固定 probe EXE 冒充。明确 env 不启用 synthetic harness，正式 gate 仍不得据 unit 通过放开。
2. 实际模型自然申请 app-webview 原任务，在需要时请求用户选择页面/文件。用户手动批准；不导出 Cookie/Token，不创建自批入口。
3. 本地固定 receiver 独立验证真实文件 bytes/hash、一次效果；Pause/Continue/Stop/Quit 分别检查原 picker/assignment 物理 owner。`rfd 0.15.4` 不支持公开强制关闭句柄，不能把 waiter 取消说成系统对话框退出。
4. 完成原两页中文保存/计数/切回、fresh observe、旧 ref 拒绝、active Stop 零新效果的真人合同后，才公开 Windows descriptor；其它平台不能借 Windows 结果销账。

本批无 commit、push、PR、tag 或 Release，目标保持 active。

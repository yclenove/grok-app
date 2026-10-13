# Computer Use 下载最终发布与开始前导航/关闭

2026-10-08，`feat/computer-use-implementation`。整体 active / partial；未提交、推送、PR 或发布。

## 本批改动

- `webview/download/publication_probe.rs` 提供 feature-only、原 run + native key 的等待点。
  真实下载生成原 `WebViewStagedDownload` 后才等待；不持有 Broker、binding 或 native authority 锁。
  不替换下载实现，不改原生产超时、文件大小、最终 commit 或 Drop 规则。
- 实际 App fixture 覆盖正常发布、Pause、Stop、原 Wry 导航、解绑、同 run 重绑、原 main close IPC。
  error-path wrapper 明确先 Stop fence、再释放等待点、最后 join 原 caller，避免析构死锁。
- 原开始前 fixture 新增 navigation 和 Close。关闭等待使用原五秒产品 deadline，
  单独观察 IPC 回执，再断言发起到回执在 5–6 秒内。仅观察预算为八秒，未延长产品 deadline。
- 页面就绪失败会保留原 label、URL、readyState、downloadLinks 与 main invoke 状态诊断。

## 同一构建的实际 App 证据

证据根：`src-tauri/target/codex-test-runtime/`。
构建 JSON：`app-download-boundaries-build-20261008-04.jsonl`。
三个实际 App 副本与当前 `target/debug/computer-use-app-probe.exe` 的 SHA256 均为：

`6E6914B405F5884DA55EE926468005236DBA16FFF075AAD4EE7BE734A6B19A64`

| fixture | 场景数 | 原 App joined / exit | 观察到的 owners 全退出 |
| --- | ---: | --- | ---: |
| `app-download-publication-20261008-02` | 7 | true / 0 | 19 |
| `app-download-before-start-20261008-16` | 5 | true / 0 | 13 |
| `app-download-close-path-regression-20261008-16` | 2 | true / 0 | 10 |

publication 每场景在控制前核验 37-byte 原文件内容/hash、原 native/Wry 四种终态回调，
原 caller 仍等待而 native 已 idle。正常发布及原 receipt replay 成功；其它六场景确实到达
原最终 commit 拒绝路径，不返回成功，原 staged 文件和目录清理、重复网络请求为零。
控制到 keeper 确认耗时分别 58、41、3、20、3、7、29ms。
每场景还验证同物理浏览器、同 label 的新 native owner 能下载，旧 Stop/replay 不影响它。

开始前五场景保留原 HTTP request、零 starting/terminal 回调及 original native buffer。
Pause、Stop、Navigate、Close 在释放响应头后均取消/退休，文件未发布；新 owner 成功。
原八秒 grace 实测 8001ms，unknown 期间 occupancy/buffer 保留。
Close 回执 5006ms，原 view hidden、同 label create 拒绝；回调后显式重试成功。
原长路径场景最终 Broker 路径 264 UTF-16 字符，37-byte/hash 通过，native buffer 正常退休。
原 close 回归实测 timeout 5017ms；释放回调后的关闭为 21/19ms。

以上均为实际 App library、原 main IPC、真实 Wry/SideBrowserLiveView 和 TaskGrant/Broker。
Host approval/publication pin 仍 synthetic，`realAcp=false`、`realMcp=false`、`realModel=false`，
`freshFrontend=false`。不代表真人聊天、可安装新版或其它平台通过。

## 失败历史与限界

- `app-download-publication-20261008-01`：前一候选七场景通过，owners 16 全退出；不是本批最终候选。
- `app-download-before-start-20261008-14`：Pause 及 fresh download 后，下一场页面就绪失败，
  未进入新增 navigation/Close 断言。保留原日志/profile/home/EXE；未复现，根因尚未证明。
- `app-download-before-start-20261008-15`：前三控制与 navigation 已走过，Close 的测试观察
  deadline 与原产品五秒 deadline 竞争，未观察到回执。保留原失败，不计整套通过。
  后续候选仅区分观察预算和产品 deadline，仍要求原五秒失败与 5–6 秒 IPC 回执。
- 两次失败的原 App/观察到的 owners 均已退出。其进程 exit 0 与 native `ok=false` 不一致；
  runner 仍通过 native report 拒绝，未将进程 exit 0 当成验收成功。探针退出码传播还需核查。
- WebView2 退出时的 unregister 1412 行保留；原 owner join/实际进程退出另有明确证据，
  不把这行解释成下载成功或权限问题。

## 回归和清理

fresh Cargo-selected、post-link manifest 读回的 App library harness 位于
`app-download-boundaries-regression-20261008-17/`。原 Cargo harness SHA256 为
`EB13F3DB6360E60528D6157ED6AC4299F6E1B4C9E0BE89B19458DAAC18AE8DA1`；
真正执行的 manifested 副本为
`C0E683589823C4D824312545F57FC29454C6DF75D1B613F55F89704F8F8B745C`。
WebView 131、Host 9、side_browser_blob 4，共 **144 passed / 0 failed / 0 ignored**。
这三个过滤组不是全库/跨平台测试；其它 2262 个库测试未在本批执行。

三组 `--locked -- -D warnings` 全部 exit 0，原 exec session74543 已结束：

1. `-p grok-app --lib --tests --bin computer-use-app-probe --features computer-use-probe,tauri/custom-protocol`
2. `-p grok-app --lib --tests`
3. `-p grok-computer-use-probe --all-targets`

最后仅移除 publication fixture 上冗余的 `too_many_arguments` lint 豁免，重新执行第1组 strict
通过，日志 `clippy-final-fixture-lint.log`；没有修改产品执行语义或测试断言，未重建实机副本。
这不是 source/package 的12h冻结验收。

fmt、diff、PowerShell runner parse、LF、final 质量脚本均通过。
App+Workbench 13573、≥1000 行文件 80/80；`webview.rs` 945、`download/windows.rs` 964 行。
新增等待点/fixture 落独立领域模块，无产品前端或用户字符串变更。

自动审批拒绝了两份重复 EXE 的删除命令，工具仅返回 `blocked by policy`，没有更具体原因。
命令未执行，没有移除文件；实际回收 **0 bytes**，暂缓逻辑大小 **188,401,664 bytes**。
只读复验以下 exact paths 无 reparse ancestor、无 exact-home live owner、原 owners 均退出，
hash 与保留的 publication-02/current probe 相同：

- `H:/aicoding/grok-app-computer-use/src-tauri/target/codex-test-runtime/app-download-before-start-20261008-16/computer-use-app-probe.exe`
- `H:/aicoding/grok-app-computer-use/src-tauri/target/codex-test-runtime/app-download-close-path-regression-20261008-16/computer-use-app-probe.exe`

每份 94,200,832 bytes，可从保留的 `app-download-publication-20261008-02/computer-use-app-probe.exe`
复制再生成；跑 fixture 需用新 RunName，不能覆盖已存在的 reports/home。
只读检查时 H 可用 264,207,458,304 bytes，C 可用 387,141,910,528 bytes；不是回收增量。
所有 reports/logs/manifests、原 home/profile 与两次失败现场均保留。
原拒绝 incremental targets 不重试，unknown 首次 owner、失败14/15也继续保留。

收尾只读检查，UUID-format `grok-cu-download-<32 hex>` 私有 native Temp 目录残留为0。
所有本批 App/probe/harness 都已退出；唯一用户预览仍 PID24536，2026-10-07 20:32:07 创建，
`target/debug/grok-app.exe` SHA256 为
`B7CD56212053FE268B5322BC679E24FB2B26D0683A3358F13BE29FA6647D4942`。
再次通过 fresh returned window 亲自查看：首页、侧栏、输入框和 Grok4.7 极高正常。
这不是新版后端验收；更新前仍须核实原 App data-home，保留登录/会话后再替换这个 exact preview。

## 下一步与完整交付

本批实际 fixture、回归/strict/格式质量已验；重复产物清理仅因策略拒绝而暂缓。
接 Host-authorized WebView upload、
D22 私有 buffer 的持久 owner/journal/force-kill 恢复，以及 fresh 用户包与原 ACP/MCP/Grok 实机链。
页面就绪首败与 probe 退出码不一致保留为未关闭调查项。
Windows、macOS arm64/Intel、Linux X11/native Wayland、四个 surface、统一 UI/UX、
安装升级回滚、模型矩阵、同冻结候选 12h 主动长稳、分批提交推送与完整交付范围保持。

# Computer Use 上传 MCP 与原生接线检查点

2026-10-12；工作区 `grok-app-computer-use`，分支 `feat/computer-use-implementation`。
整体仍 partial；本记录不能替代真人、原 ACP/model、真实系统 picker、跨平台或完整交付验收。

## 已实现与验证边界

- `computer_upload` 已进入 Core、Node 与 App 内嵌 catalog；模型只提供原 run/target/generation/snapshot/geometry/ref/action 身份，不提供路径、字节、URL 或脚本。
- 上传使用专属 `/cu/upload`，普通工具仍走 `/cu/tool`、socket idle 仍为 15 秒。上传 HTTP body 以 4 秒心跳等待，最长 600 秒；没有提高普通工具全局超时。
- 原请求 ID 区分数字和字符串，重复 ID 不能覆盖取消者；body-drop、deadline、EOF、shutdown 退休原意图，不能以新 caller 接管原动作。
- App 上传权限识别原 wire metadata/server；无原 TaskGrant、旧 agent/turn 或伪造 metadata 不退回通用 allow-always。上传不进入泛用授权缓存。
- 实际 App 内嵌 MCP 覆盖 stale runtime，并由随包私有 Node 执行 catalog 检查，不依赖 PATH 上的 Node。
- 取消是权限退休，不等于物理 picker/assignment 已结束；StopRequested 保留到原选择 owner 结束。字段赋值 verified 不等于网站提交成功。

## 原生 MCP 验收：实际通过

模式 `webview-upload-mcp` 使用随包 Node → App 内嵌 MCP → 原 HTTP/Broker → 实际 App 侧栏浏览器/native file input；独立本地 receiver 核验数据与重复效果。

证据：`src-tauri/target/codex-test-runtime/app-upload-mcp-native-20261012-01/`，原 runner exit 0、原 App 与全部观察到的浏览器 owners 已退出。

| 场景 | 实际结果 |
| --- | --- |
| 超过普通 15 秒 idle 的原请求 | 等待 16113 ms 后成功赋值；24-byte/hash 正确；重复 action 不重派 |
| 空文件 | 正确赋值和 0-byte/hash；重复 action 不重派 |
| held selection 取消 | 字符串 ID 不取消数字 ID；正确取消退休原意图，未发生额外上传 |
| held selection EOF | 原 caller 结束、意图退休，未发生额外上传 |

4 场景通过、总 receiver POST 2 次、native idle。Host approval 与文件选择为 synthetic；real MCP/HTTP/native App=true，real ACP/model/native picker=false。不能宣称真实用户已经同意文件或完成模型端到端上传。

## 分层检查与失败账本

- 随包 Node 完整 MCP 套件：46 passed，0 failed。
- 默认 App strict 与 probe lib/tests/bin strict 实际通过；首次 probe 编译缺失 import/错误 enum variants 已修复，原失败输出保留。
- App manifested libtest 分层：ACP CU 64、runtime 11、Host reader 1、WebView 49、SessionManager CU 46，共 171 unique passed、0 failed；另有 84 ignored 未运行。
- 上述 libtest runner 实际 exit 1：发现数门禁期待 130 项 SessionManager 全运行，84 ignored 使门禁拒绝。不能写成 runner green 或把 ignored 当 passed；libtest 构建早于新增 probe-only MCP 原生模式。
- Cargo fmt、diff check、final quality 再检通过；App+Workbench 13573、≥1000 行文件 80/80 未增长。FINAL: PASS 仅代码质量。

## 当前候选与下一步

source129 冻结 3412 文件，manifest SHA256 `785339718dd5c1268bfcdd505cd36cc29f010920e4ad0b642423bf1b3dc7507b`。
独立 Windows 测试包实际完成：`dist-portable/Grok_CU_20261012_app129/Grok.exe` 与同名 ZIP。
fresh 前端/实际产品后端/完整资源、450 文件 manifest 和 ZIP 解包逐文件回读通过；不是 probe EXE。
EXE SHA256 `c1b4902ffa4d281b390ad13d057dd7631e8f54494f069f8237a154f5efa74d19`；ZIP SHA256 `561b8ec655169fc8682aeebcde0bc7da8133d534777edc6fc6f20a97bc411a83`。
用户此前的 devUrl 拒绝连接是本机前端地址没有服务接受连接，不代表 CU broker 或 Grok 远端故障。
旧 dev App45164 由窗口正常关闭、原 execution owner14188 结束，随后原 Vite57256 也退出。其终端曾记录 WebView unregister warning/进程4294967295，不能把关闭日志说成 App exit0 验收；最终确切原 PIDs 均不在。
新包 PID78724、实际窗口397944，复用原独立 startup-home；在开发端口1421无 listener 时，实际观察到 `http://tauri.localhost/` 的首页、历史内容恢复/就绪、常规设置和关于页 `CU Preview 20261012 app129`。未提交新模型任务、未代批任何权限；留一个当前预览给用户测试。

清本批原生验收重复 EXE 一份，原 App/browser owners 已 joined/exited；日志、home、源构建与当前包保留。
删除逻辑 87979520 bytes；H 空闲实际观测增 88887296 bytes，后者是现场净变化，非排除并发磁盘活动后的归因。
可由保留的 Cargo 命令重新构建，再使用 fresh RunName 复制，不覆盖旧证据。当前构建/依赖/增量缓存仍有用途，暂缓。

真实文件 picker/同意、原 ACP/model、选择中 Pause/Stop/Quit 的物理 owner 收尾仍待验；本次实际打开包只证明启动/导航，不证明 Computer Use 任务端到端。
`rfd 0.15.4` 没有公开强制关闭 dialog 的句柄，不能承诺 Stop 自动关闭系统选择框。
Windows/macOS arm64+Intel/Linux X11+Wayland、四 surface、installed 升级回滚、模型矩阵、冻结候选 12h active soak 与 D00–D31 全范围仍开放。
本批无 commit、push、PR、tag 或 Release。

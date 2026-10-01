# Computer Use 接手审查与补完账本

日期：2026-09-08。分支 `feat/computer-use-implementation`，起始 HEAD `30757366`，所有改动仍在本地，未 commit/push/PR。用户确认 Grok 已停止修改，可以接手。

权威范围仍为原始 `2026-09-08-computer-use.md` 的 P0–P9：Windows x64、macOS arm64/x64、Ubuntu GNOME Wayland 与 X11。下表以实际接线和实现为准，覆盖旧账本中的过度完成声明。实现和验证分开记录，缺机器不能将未实现归类为“仅待实机”。

## 接手发现

| 范围 | Grok 实际成果 | 主要缺口 | 当前判定 |
| --- | --- | --- | --- |
| P0 | 合成图、Win32 fixture、Cua/Playwright 独立探针及调研 | 四 target、原生 Wayland、扩展配对未验证 | in_progress |
| P1–P2 | Broker/协议/锁/IPC/若干 Fake 测试 | 模型可自授权与自恢复；全局 token；无会话绑定；超时提前释放 in_flight；假停止；迟到观察污染状态 | in_progress，优先修复 |
| P3 Windows | Win32 枚举/客户区截图/消息与 SendInput；单个自建窗路径 | 非 UIA；可抢焦点；元素名称回退；取消/DPI/多屏/控件身份仍不足 | in_progress |
| P4 macOS | CGWindowList、截图和部分 CGEvent | AX 权限存在不等于语义树；元素点击猜固定坐标；key/scroll/drag 明确未实现；截图范围与输入焦点需重做 | in_progress / 原生 not_run |
| P5a X11 | 枚举、XImage、坐标点击 | 无 AT-SPI/中文/完整输入；窗口身份与坐标转换不完整 | in_progress / 原生 not_run |
| P5b Wayland | 环境识别、返回不支持 | 无 portal/helper/PipeWire/libei 操作链 | not_started / not_run |
| P6 受管浏览器 | 单独 Node worker 可跑两 profile 表单 | App 的 browser 工具没有调用 worker；仅返回内存 TabInfo | in_progress |
| P7 已有 tab/WebView/UI | 内存借用模型、面板/任务卡、15 locale 文件 | 无实际扩展连接；WebView 返回不可用；卡片硬编码 running；面板授权前不列目标；预览与模型快照共用 | in_progress |
| P8 模型闭环 | 一个 fixture 点击实测 | 非完整 App 用户流程；无持久轨迹；wait 直接返回成功；handoff 只报错 | in_progress |
| P9 打包 | 一次未签名 NSIS 编译/测试目录安装 | 包夹带探针；MCP 指向开发机绝对路径；其余 target 未交付；更新/回退未验 | in_progress |

接手时真实运行的 Grok App 来自 `%TEMP%/grok-cu-nsis-test/install/grok-app.exe`。不操作其安装/卸载，不终止宿主、用户账号或代理。旧证据只描述当时单个探针，不能直接证明新实现通过。

## 本轮已改动，仍在验证

1. 将平台无关协议/Broker/锁/IPC 拆到 `src-tauri/computer-use-core` workspace crate，保留 Host 原生适配器接口；核心测试不再加载 Tauri/Common Controls。`cu_probe` 改为独立 workspace 包，App 不再声明这个 binary。
2. Broker 拒绝跨会话复用 run、已停止 run 自动重开；授权/恢复代际更新；迟到观察复核代际；UI preview 不改模型 snapshot；所有原生写操作持输入锁；超时后 worker 存活状态不提前清除；停止必须同时确认 worker 结束和 adapter idle。
3. IPC 改用完整 HTTP 解析、并发 worker 和有限请求大小。随机凭据绑定 Host 确认的 session/run，轮换/撤销生效；不信请求 JSON 的会话归属；模型侧不提供授权、resume/reconnect；wait 检查实际条件；handoff 暂停采集与输入。
4. 移除通用 MCP builder 的全局注入，改为目标授权后向已连接的本地会话显式热注入；MCP 脚本嵌入应用并在应用数据目录按内容散列展开，避免依赖开发机源码路径。Node/browser/driver 发行管理仍待补完。
5. Tauri 控制命令改为异步，原生慢工作进入 blocking worker；Host 为每次新任务分配独立 runId，停止确认由真实状态返回。

## 验证记录

- 提取后的原有核心测试：16/16 进入断言并通过。此项只证明原测试，不证明需求完整。
- 修复后首次核心测试：21/21 通过，含全局凭据越权、模型自恢复、token 轮换撤销、分片 HTTP body、慢动作期间查询响应。命令：`cargo test -p grok-computer-use-core --offline --target-dir target-cu-review`。
- App 编译、完整前端、扩展模型实机流程：本轮仍在进行，不能标 passed。
- 三 OS 真实桌面基准和四 target 安装/更新/回退：未完成；目前缺 macOS/Linux 实机，不影响继续独立实现。

## 后续执行顺序

1. 完成本轮核心/Host 接线、严格 Schema、取消传播、会话生命周期及回归；消除旧探针与新协议不一致。
2. 原生驱动回到经过验证的统一适配方案（优先重新接入固定 Cua）；完整 UIA/AX/AT-SPI/Wayland，不能继续以 Win32 控件名或固定坐标冒充语义操作。
3. 将受管 Playwright 和已有 tab 连接真正接入 Broker，完成多 tab、导航/文件权限、停止和会话所有权，再实现 WebView 子集。
4. 完整整理 UI 生命周期、错误/空态、接管、全局停止、预览按需刷新、15 locale、任务轨迹/证据与性能。
5. 依赖安装/升级/回退、打包与门禁、真实模型/OS acceptance、最终整体 review。全部满足原完成定义后才可结束 Goal。

进度输出不得把“有接口”“独立探针成功”或“拒绝不支持动作”写成完整功能已实现。

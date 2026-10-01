# Computer Use：R18 冻结构建与原生 UI 验收边界

完整最终版目标保持 active；本记录不把“能编译/显示初始窗口”当作完整 UI 或交付验收。
执行时间使用 Asia/Shanghai 的 2026-09-27。没有提交、推送、更新用户安装、导入凭据或操作任何权限请求。

## 1. 冻结源码与实际构建

证据目录：`tools/computer-use-probe/.run/ui-native-r18-20260927/`。
独占快照：`H:/aicoding/cu-native-r18-20260927/source`。3519 个普通文件复制后校验哈希，Rust 缓存采用独立普通副本；依赖引用现有目录，不冒充全依赖可复现构建。用户 App、用户运行时和在用目标目录均未替换。

首轮 r2 的 review 配置因 PowerShell `ConvertTo-Json` 默认深度把嵌套窗口项序列化为字符串，构建 exit 1。保留原配置、失败日志和结果；修正为 Depth 12 后 r3 **exit 0**：06:36:38 至 06:43:16。
`native-build-r3.log` / `native-build-result-r3.json` 记录实际 beforeBuild、runtime import/hygiene、Vite 和 Rust 构建。hygiene 444 文件/0 links/0 hits；原有 chunk/linker 警告保留，未宣称 warning-free。

原生可执行文件：

```text
H:\aicoding\cu-native-r18-20260927\source\src-tauri\target-cu-review\debug\grok-app.exe
SHA-256 71ECC608AE000CBC15287237B9B4B23F17D23F642E0FE5478EE9B20D3252D1FF
```

06:43:45 启动记录 PID 26216，仅为该次进程身份记录，不凭 PID 文件推断之后仍存活。独立 instance `com.grokapp.desktop.cu-review-r18-20260927`，独立 App home、中文深色、deferred account setup、新导览、Computer Use 关闭。没有复制凭据；启动器剔除常见 key 环境变量。
启动前重新核对 3519 个冻结文件及 runtime resource 来源；资源摘要相符，没有手工替换包内 worker。

这是一份 debug 原生 UI review，不是签名安装/升级/回滚，也不是首登/鉴权验收。快照早于后续 macOS 指针归属修复，不能作为其编译或原生效果证明。

## 2. 实际界面观察与停止输入

通过规定的 `@oai/sky` 返回列表按精确 exe 唯一选择窗口，读取原生窗口及截图/AX，仅激活并观察，**没有发送点击、按键或滚动**。
06:44 的初始截图元数据为 **962×642**，AX 可见真实工作台、消息输入与首次导览。review 配置的初始尺寸不等于用户拖动缩放或系统 DPI 验收。

实际显示的截图有 Windows 安全中心防火墙权限提示遮挡，提示应用显示名为 `grok_app_lib-e546f23183eb33c6.exe`。未确认其所属进程，不能把显示名当作 R18 进程归属证明。
没有点击允许/取消、关闭权限提示、修改系统设置或杀进程绕过该提示。立即停止 UI 输入；仅继续非交互代码与回归工作。

`native-ui-observations.json` 保存两次观察的时间、精确目标、截图尺寸、AX 与 verdict；没有把截图 base64 写进仓库。视觉依据是本次工具实际显示的图像，而不是仅依据 AX 推断遮挡情况。

| 原生项目 | 状态 |
| --- | --- |
| 冻结 debug 构建、启动、初始工作台显示 | 已观察，范围仅此 |
| 导览关闭后的 CU 设置/入口/状态交互 | NOT_RUN，输入前被权限提示遮挡 |
| 窄窗布局、Tab/Escape/焦点、深浅色矩阵 | NOT_RUN |
| 真实系统缩放、权限拒绝/撤销/恢复 | NOT_RUN |
| 签名安装/更新/回滚、真实 Grok E4、12h active soak | NOT_RUN |

独立夹具设置文件实际回读：`computerUseEnabled=false`、`permissionPolicy=ask`、`sessionDataMode=independent`、`theme=dark`、`locale=zh`。这不是通过 UI 操作权限开关的证据。

## 3. 并行当前源码 UI 回归

当前工作树 CU 组件/controller、设置壳、侧栏入口、domain 与授权/pairing API 的 Vitest：**22 文件、191/191，exit 0**。
见 `ui-domain-current.log` / `ui-domain-current-result.json`，06:42:41 至 06:43:03；测试使用 mocked Host，不是 191 项原生交互或权限通过。
本轮没有修改前端产品代码、用户设置或在用 App。本轮原生 UI 阶段受阻不代表整个开发目标 blocked；后续实际完成了 [macOS 指针归属修复与回归](2026-09-27-computer-use-macos-pointer-ownership-checkpoint.md)。

## 4. 后续要求

权限提示需由用户自行判断处理；不能由自动化代替决策。恢复 UI 检查前必须重新获取窗口/截图/AX，不能复用旧坐标或凭此次 PID 直接输入。继续使用独占 review 实例，不操作用户已安装 App。
之后仍须完成整个原生交互/布局/系统缩放/权限矩阵，不能把本记录或 191 项单元测试升级为 UI 验收通过。
Windows/macOS arm64+x64/Linux X11+GNOME native Wayland、所有既定 surface 与 App/ACP/MCP、完整输入及恢复、签名交付、真实 E4 和冻结 12h active soak 的原始范围不变，目标保持 active。

# Computer Use：App 原生授权入口与取消收尾

日期：2026-09-30。目标仍为 **active / partial — not releasable**，不是最终版。
证据目录使用本地运行标签 `tools/computer-use-probe/.run/wayland-app-consent-20261001/`；
目录标签不是适用日期或未来平台验证的证明。上一目标回合属于 **progress**。

## 生产增量

- App 新增显式开发构建开关 `computer-use-wayland-preview`，默认不启用；
  不开启产品 Computer Use 开关，不安装或启用 GNOME helper。
- Linux factory 与只读 status 使用同一缓存选择：仅显式 preview 构建且原生 Wayland
  会话环境才路由到同一 Host `PortalRegistry`。其余仍走原 LinuxAdapter 与既有
  Wayland fail-closed 检查，不增加 XWayland 回退。`native_wayland` 仍为 false。
- 实际 `computer_use_authorize_surface` 接入系统选择器，parent 是 Tauri 注入的
  调用窗口，不接受 renderer 伪造窗口标签。先建立原 AuthorizationTicket，再准备
  GTK parent、GNOME/logind 监控和 portal；沿用 Broker、回读、MCP attach 和 complete。
- 仅 Host 判断为 Desktop/portal 时允许空目标；此时拒绝 renderer 提供的 targetId。
  浏览器、现有标签页、WebView 的目标选择规则不变。没有 run 的原生 discovery 返回
  空列表且不打开选择器；有 run 时仍校验原 session/run/surface 并回读实际所属目标。
- 新 `PreparedTarget` 在错误、取消或等待者丢弃时撤销未提交 selection；只有全部
  原 native/monitor/GTK owners join 后才清理精确 capability 的元数据。失败或未 join
  仍保留占用；不通过超时、丢 receiver 或新 owner 冒充关闭。已提交 guard 不撤销
  活跃 grant，但仍保留等待真正关闭的回收任务。
- 前端显式系统选择按钮不再要求或显示伪造显示器候选，不把刷新当成授权。
  保留 feature-off、状态过期、cleanup、busy、attempt/revision 和 Stop/unmount 栅栏。
  15 locale 明示是会话级键鼠授权，不承诺限制到单个应用，并说明 helper 前置要求。
  这只是前置说明，**不是已实现 helper 安装/启用/状态/恢复界面**。
- Command 测试从大文件提取到相邻测试模块，保持原模块路径和语义；未提高质量门槛
  上限，也没有向 App.tsx / AppWorkbench 增加状态或功能大块。

## 当前候选验证

冻结 **391 个相关源码记录**，不是全仓库/所有依赖或最终发布冻结。
相对上一 receipt：362 未变、5 修改、24 新纳入记录；“新纳入”不等于本轮新建文件。
新增前端 controller 10、panel 2、App command 2、Linux App guard/factory 6 项用例。

| 检查 | 当前证据与边界 |
| --- | --- |
| Windows App 定向 libtest | **113 passed**：command 15、WebView 69、session MCP 21、feature lifecycle 1、browser process 7 |
| Linux preview App 定向 libtest | **91 passed**：command 15、portal guard/factory 6、session MCP 21、WebView 48、feature lifecycle 1 |
| Linux guard 重复执行 | 同一 preview executable **6 × 3**，线程 1/4/4；包括 Host session 消失而原 parent future 仍保留 |
| Preview factory 环境分支 | 独立进程 X11、Wayland、混合环境各 1 项；只检查真实 factory/status，不创建桌面授权 |
| Linux 默认构建独立回归 | **21 passed**（command 15、guard/factory 6）；Wayland 环境中仍未启用 preview factory，使用单独构建 executable |
| 前端与 i18n | **21 文件 / 234 tests**；全量 typecheck、ESLint 通过 |
| App 编译检查 | Windows、Linux default、Linux preview 三个 all-targets strict Clippy 通过 |
| 格式与结构 | workspace fmt、include 文件 fmt、git diff、quality final 通过 |

所有 Rust 数字都是明确 filter 下的定向用例，日志保留其他 filtered 数量，不能称为
完整 App suite。Linux browser process filter 实际为 **0 tests**：该模块是 Windows-only；
不计入通过数量、不包装为 Linux 对应功能验收。

Windows 仅修改复制后的 libtest executable 的 Common Controls manifest，并回读、
散列核对；原 Cargo executable 未修改，未修改用户安装。Linux executable 固定复制后
核对前后散列，测试脚本、诊断和所选文件清单保留于本轮目录。

初次 Linux guard 用例 **2 passed / 3 failed**：fixture 使用临时 SessionGrants，
其 Drop 按生产语义正确取消 ticket，导致 parent 尚未开始就失败。修正为保留原 Host
owner，并新增 Host owner 消失测试；未放宽取消检查。这是 fixture 错误，**不是生产
漏洞的 red→green 证明**。窗口类型、测试可见性、Clippy、翻译键时序及文件规模的
早期诊断亦保留，不混充产品运行故障。

## 未完成的完整目标

本轮运行的是 App libtest、UI/jsdom 和编译检查，**没有运行实际 App 原生系统选择器**。
先前 Wayland 163×3、真实私有 PW/EIS/PNG、GTK probe、helper/GJS、core 等是历史
证据，本轮没有重新执行；也没有 macOS、安装后 GNOME 或最终长稳证明。

以下原目标继续全部保留：

1. installed Ubuntu 24.04 GNOME Wayland：helper 实际加载、真实输入设备分类、
   锁屏/接管/恢复、session PID 绑定、focus/topology、parented consent，及新增 App
   preview 授权/切换目标/错误恢复的端到端验证，随后才评估默认启用和能力标志。
2. 显式且可见、15 locale 的 helper 安装/启用/状态/停用/恢复 UX；原生窄窗/DPI/权限 UX。
3. Windows x64、macOS arm64/Intel、Linux X11/native Wayland；Desktop、managed
   browser、existing Chrome/Edge、App WebView 经 App/ACP/MCP 的完整功能矩阵。
4. 所有输入、中文 IME、剪贴板、取消/异常恢复、OS 权限与真实物理接管闭环。
5. 签名 clean install/update/repair/rollback/uninstall；Linux 模块/许可和实际
   AppImage/deb/rpm；Ubuntu 22.04 baseline，不能用本轮私有高版本 SDK 替代。
6. real Grok E4、同一最终冻结候选 **12h active soak** 和原需求逐项完成审计。

本轮不安装/启用用户 helper、不发布/签名/安装 App、不 commit/push/tag。
下一阶段应推进 helper 的显式状态/安装/恢复流程和 installed GNOME 的真实 App 验收，
不能以更多 fixture 或部分绿色检查代替最终版。

# Computer Use：GNOME helper 显式管理与恢复

日期：2026-09-30。原目标保持 **active / partial — not releasable**，不是最终版。
本地运行标签为 `tools/computer-use-probe/.run/gnome-helper-management-20261001/`；
标签不改变适用日期，也不表示未来平台已经通过验收。上一目标回合为 **progress**。

## 生产增量

- 新增桌面设置专用 `computer_use_helper_status` / `computer_use_helper_action`。
  Tauri 注入的实际 WebView 必须是 `main`；不接受 renderer 传入身份、路径或扩展 UUID，
  不注册为 MCP、mirror 或代理工具。默认构建及非 Linux 平台拒绝所有 helper 修改。
- Linux preview 增加只读状态和显式 **安装 / 修复 / 启用 / 停用**；只处理随 App
  打包的 `computer-use@grok-app.local`，不下载代码、不自动安装/启用、不改变全局
  扩展开关、不使用 Shell Eval 或不受支持的 ReloadExtension。helper ready 不是桌面授权。
- `GnomeHelperControl` 使用 GNOME 46 的实际 `org.gnome.Shell` owner、
  `/org/gnome/Shell`、`org.gnome.Shell.Extensions`；严格解析 double 类型的
  extension state/type。启用/停用的 bool 只代表接受设置，必须再读真实扩展和 helper
  状态；原调用/回读不确定时不自动重试。helper `GetState` 第四项表示 **blocked**。
- 固定同 UID Shell unique owner，读取 bus ID、总线可信 PID 及
  `boot UUID + PID + /proc start ticks`。GJS 缓存不能靠禁用/启用或重连总线刷新；
  发布新文件后必须观察到 **新的 Shell 进程实例**、实际扩展发现和健康回读。
  单纯重启 App、重新取得总线名、文件存在都不能宣称生效。
- Host 从 XDG/HOME 派生安装目录；检查绝对路径、祖先权限/owner、符号链接、
  regular file、单链接锁文件与有界读取。仅对有明确 App 所有权的固定文件集合发布，
  额外文件、外来目录、损坏所有权信息不被接管或盲删。
- 同一跨进程锁覆盖原始管理操作；持久事务日志、pending/previous 和 fsync 支持
  原发布各中断点的 **显式修复**。损坏日志或不明内容转为 conflict，不静默清理。
  这是 App 管理文件的完整性/恢复边界，**不是抵抗恶意同 UID 进程的沙箱**。
- 与产品开关复用同一 feature transition lock；修改前必须关闭 Computer Use，并
  等待原 PortalRegistry native/monitor/GTK owners 真正 join。Registry 登记前再检查
  Host policy；未确认清理或 owner 失败不放行。IPC 等待者取消不会释放原 mutation
  owner、文件锁或 transition lock，也不会产生第二次管理操作。
- 独立 `ComputerGnomeHelper` 使用现有 GlassModal/设置样式，包含只读状态、明确确认、
  busy、失联、不确定、重新登录、全局关闭、冲突、修复和刷新路径。前端 mutation latch
  跨组件卸载保留；读超时不能解锁修改，晚到读不能覆盖新读。产品开关变化重新取状态。
- 23 个新文案键覆盖 **15 locales**；设置搜索加入 `ext.computer.helper` 和精确 anchor。
  不增加 App.tsx/AppWorkbench 状态，不提高结构质量上限。

## 当前证据

最终记录 **411 个相关源文件**，不是全仓库/依赖或最终发布冻结。
其中 407 条在 Rust/UI 定向回归前冻结；其后只新增 4 个浏览器验收入口/配置记录，
原 407 条再次逐项核对无变化。相对上一 receipt：369 未变、22 修改、20 新纳入范围；
“新纳入”不等于本轮新建。receipt 链接上一阶段
`5af9e5244d344cf36e2f40e3c9b5ceeed8d0ae0652eb704bc5213c976ddc9a98`。

| 检查 | 本轮实际结果与范围 |
| --- | --- |
| Windows App 定向 libtest | **115**：command 15、WebView 69、session MCP 21、feature lifecycle 1、browser process 7、helper 2 |
| Linux preview App 定向 libtest | **37**：helper 15、command 15、portal guard/factory 6、feature lifecycle 1 |
| Linux 默认构建独立 artifact | **16** helper 用例，包括默认构建禁止修改；不是 preview artifact 冒充 |
| 私有真实 D-Bus 管理协议 | **5**：正确 endpoint、bool 不代表激活、拒绝/owner 丢失、严格 variant、缺失/不支持/全局关闭不调用 enable |
| Wayland 完整 crate suite | 同一 executable **169 × 2**，线程 1/4，**0 failed / 0 ignored / 0 filtered** |
| 原生测试产物回读 | 两轮 **130** 个精确 owned 目录；**68** EI peers 均有终止 disconnect；原进程全部 join，无 live child/error；**36** PNG 实际解码 |
| 原生边界回读 | parent 真正导出、monitor 元数据、unmap 后拒绝输入、权限/接管撤销无需外部 Stop、旧授权不可复活；仍是私有 fixture，不是 OS 事件证明 |
| UI / i18n 定向单测 | **24 文件 / 272 tests** |
| 编译后浏览器界面 | **59 场景**：15 locales 明/暗色，11 状态，四种显式操作、键盘确认/取消/焦点恢复、busy Escape、丢响应和刷新不重放；320/400 CSS px、1/1.5/2 像素比、200% 文本 |
| 管理 helper policy / runner | Node **10**；Linux runner contracts **17** |
| 静态检查 | Windows App、Linux default/preview App 和 Wayland all-target strict Clippy；typecheck（含新增 fixture）、ESLint、fmt/include fmt、diff、quality final 通过 |

App 数字均为显式 filter 的定向测试，不是整个 App suite；其他 filtered 数量保留在日志。
Wayland native runner 使用私有 PID/network/tmp、owned labwc、真实 PW/EIS C fixtures；
前后可执行文件散列相同，libei 未作为动态 NEEDED 依赖。独立产物脚本按精确目录检查
PNG、输入事件、parent/撤权数据和收尾，而不是仅相信绿色 summary。

Windows 仅向复制后的 libtest executable 嵌入 Common Controls manifest，随后回读、
核对散列；未修改原 Cargo executable 或用户安装。Linux preview copy 与独立 default
executable 也核对前后散列。测试仅在临时目录写 helper 文件，**没有安装或启用用户 helper**。

浏览器 probe 编译真实组件和产品 CSS，但 Host IPC 是 mock；不是 App WebView、
GNOME Shell 或 OS DPI 验收。59 场景先通过，随后加强按钮纵向溢出断言，再次 **59 通过**；
查看了英/中/德/泰米尔语、明/暗、窄窗及放大文本确认截图，保留完整 PNG 与联系表。
它证明浏览器内指定布局/操作，不证明实际 helper 已经加载。浏览器从 seed 副本运行，
两次测试的原 immutable seed digest 均未改变。

## 诊断记录

- 初次 UI 定向测试 **266 pass / 6 fail** 是原 loading 用例假设页面只有一个 status
  live region；新 helper 合法增加第二个 region。改为按原 runtime 提示精确定位，
  未删除 accessibility 语义；最终 272 通过。初次失败日志保留。
- 早期 strict Clippy 报 owned Path 比较和冗余 async block，已修正；不放宽警告。
  Windows 首次诊断保留；部分早期 Linux日志曾被后续执行覆盖，不伪称保留原失败全文。
- Linux runner contract 曾误在 Windows Python 执行（缺少 `os.killpg`，路径语义不同），
  **1 fail / 3 error / 5 skip**；这是运行平台错误，不是产品失败或通过。
  原日志保留，实际 Linux 上 **17/17** 通过，未改测试去适配错误环境。
- 初期 Linux 构建/测试与最终源冻结前日志以 `initial-` 保留，不计作最终候选验收。

GNOME 46 原始 D-Bus XML、`shellDBus.js` 和 `extensionSystem.js` 已保存到证据目录；
它们解释 endpoint、variant、enable bool 和 GJS restart 规则，不能替代真实 Shell 运行。

## 尚未完成的完整目标

1. **installed Ubuntu 24.04 GNOME Wayland**：helper 实际安装/加载/停用/修复/新登录
   回读，真实物理输入分类、锁屏瞬时栅栏、接管/恢复、PID/session 绑定、focus/topology，
   parented consent，以及实际 App 授权/切换目标/错误恢复的端到端验证。
2. Windows x64、macOS arm64/Intel、Linux X11/native Wayland；Desktop、managed
   browser、existing Chrome/Edge、App WebView，经 App/ACP/MCP 的完整功能矩阵。
3. 所有输入、中文 IME、剪贴板、取消/异常恢复、系统权限、原生窄窗/DPI/确认 UX。
4. 签名 clean install/update/repair/rollback/uninstall；实际 AppImage/deb/rpm、
   Linux 模块/许可和 Ubuntu 22.04 baseline，不能用私有高版本 SDK 替代发布包装证明。
5. real Grok E4、**同一最终冻结候选 12h active soak**，然后原需求逐项完成审计。

显式 helper 管理功能已有实现和上述定向证据；**实际原生管理 UX 尚未验收**，不能把
“界面已写”升级为“已交付最终版”。默认构建和产品开关不启用，`native_wayland=false`。
没有签名/发布/用户安装、commit/push/tag，没有更新 goal 为 complete/blocked/paused。
下一步优先推进真实 GNOME/App 的安装加载与授权/接管闭环；没有原生机器证据时保持
这些项开放，不能用更多 mock、fixture、审计文案或较小矩阵替代原目标。

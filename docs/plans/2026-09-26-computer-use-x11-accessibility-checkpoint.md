# Computer Use：X11 原生语义动作与准确路由

日期：2026-09-26，Asia/Shanghai。分支 `feat/computer-use-implementation`，
HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。保留全部既有工作；未 commit/push/PR。
完整 Computer Use 最终版和整体 UI/UX 重设计目标仍为 active，不把本批当作全平台交付。

## 产品改动

- `src-tauri/computer-use-x11/src/accessibility.rs` 使用实际本地 AT-SPI 总线。
  XRes 提供原生进程归属，不信任可伪造的 `_NET_WM_PID`；匹配唯一 D-Bus owner、
  窗口标题和原生矩形，歧义或绑定缺失不换目标、不回退整个桌面。
- 不透明元素引用绑定到 snapshot、原生窗口、精确 accessible object 和完整 ancestry。
  执行前重新检查原生生命周期、几何、焦点、用户按住输入、控件可操作状态及父子关系。
  旧快照、猜测引用、同名替代控件不能继承授权。
- 实现控件主动作点击、`EditableText.SetTextContents` 和字符光标位置的 Unicode 插入。
  原生独立回读验证文本；不使用剪贴板、全局输入或键盘映射。GTK 返回的 `Click`
  大小写正常识别；`press/release` 不冒充完整点击。密码控件不暴露文本或动作引用。
- 遍历限制节点数、深度和期限；可见名称/角色截断时同时标记节点与整体 observation。
  保存完整名称用于身份核对，不用展示截断值匹配对象。
- 非模型预览只抓取像素，不创建或退休模型引用。没有 AT-SPI provider 的窗口仍能提供
  真实图像，但不会制造语义节点。原来的 XTEST 坐标点击/滚动/拖拽/按键仍走同一产品实现。
- 语义动作与坐标动作共享 `NativeActionSlot`。派发后超时/断连不是物理完成证据，
  保留 uncertain 和占用；不能声称本批已实现未知完成状态的自动恢复。

## 原生测试发现的 Broker 缺陷

实际 GTK 测试复现：坐标命中先找到外层容器，Broker 将其当成语义按钮，
最终出现 `AT-SPI node did not advertise this action`。原始失败保留在 `broker-routing-red.log`。

`broker/observation.rs` 保留节点可用动作；`broker/actions.rs` 只从支持当前动作的节点中
选择唯一、最小的命中控件。相同最小面积的歧义保留原定向坐标；右键/中键/双击不会
被改写成单次主动作。两个 Core 回归和实际 Broker→GTK 点击覆盖本次修复。

## 可复查证据

本批 artifacts：`tools/computer-use-probe/.run/x11-atspi-20260926/`。
WSL Debian 的独立工具链/输出位于 `/var/tmp/grok-cu-linux-20260925.X4zIBt`。

| 验收 | 实际结果 / 证据 |
| --- | --- |
| X11 单元 | `x11-unit-finish.log`：11/11 |
| Linux Core + X11 all-target strict Clippy | `linux-clippy-finish.log`：exit 0，未关闭 warnings |
| 当前原生 binary | `linux-build-finish.log`：exit 0 |
| 真实 GTK/AT-SPI | `semantic-finish-1/2/3.log`：各 12 项通过，连续三轮 |
| 原有坐标输入原生验收 | `coordinate-finish-1/2/3.log`：各 16 项通过，连续三轮 |
| 当前源码隔离 Linux seed | `current-seed-prepare.log` 和 `current-seed-check.log`：importProbe=passed |
| Linux Core 全量 | `linux-core-current-seed.log`：519/519，145.77 秒 |
| 当前源码隔离 Windows seed | `windows-seed-prepare.log` 和 `windows-seed-check.log`：importProbe=passed |
| Windows Core 全量 | `windows-core-current-seed.log`：519/519，121.94 秒，终态 exit 0 |

原生 binary SHA256：
`03d9aa14240c359819f427efccca66155124a1818ccfbbb583fee30bf3bb15f1`。
Linux 新 seed manifest：`60fe6688b0e53b5ad39d390f46d1f18c15761cc703d3f308fda087eb779527cf`，
tree：`194a369b1327adf1329f28788ab885ade621d54aecd5c89aa43431ecadffc27f`。
新 seed 位于独立 `seed-atspi-20260926`，没有更新原 Windows seed 或运行中的 App。

Windows 对照 seed 位于本批 artifacts 的 `windows-seed-current`，同样没有覆盖仓库 seed 或用户资源。
manifest：`e10673cbac4c64cec9c6da23505ea937f0cc1f5546623b2d50848e7760bbc1fe`，
tree：`cec9216bc6c0151a58bc3e0b58e5eae49af914477e3a648b32cb6e291e519255`，
Chromium：`63c6075faf6d986bc8e9f11996f5d2e784e7f4d9471366b7a2ad6eebaca245e3`。
该结果验证共享 Core 回归和独立包完整性，不是 Windows 原生输入或安装版验收。

GTK 夹具是独立原生进程：真实无障碍树、原生文字/点击计数、伪造窗口 PID 属性、
受保护输入、禁用控件、另一自有窗口的焦点、取消、猜测/过期引用、同名替代控件、
预览隔离，以及完整生产 Broker 路由均经过实际检验。
私有 `dbus-run-session` 与 `xvfb-run` 隔离桌面；不是 mock adapter。

**证据边界：** 这是实际 GTK + AT-SPI + Xvfb + 生产适配器/Broker，不是完整桌面 WM、
已安装 App、签名发行版、实际 Grok 模型、GNOME 原生 Wayland 或长稳验收。
CI 增加同一 GTK 门禁和所需依赖，但修改 workflow 不是远端 CI 已运行的证据。

## 保留的失败，不刷绿覆盖

- 初次 Rust 借用编译错误、GTK 窗口就绪竞态、主动作 `Click` 大小写未识别均保留原日志。
  修复实际窗口就绪判断与动作识别，不加入动作重放或关闭权限/生命周期检查。
- `broker-routing-red.log` 保留原生 Broker 真实失败；随后扩展测试通过不能抹去发现过程。
- `linux-core-full-final.log` 首轮为 **518 passed / 1 failed**：旧 Linux seed 的 worker
  与当前产品源码不匹配，在“删除 sibling”检查前触发 `hash_mismatch`。
- Windows 初次全量 `windows-core-current.log` 同样 **518/519**，同一旧 seed 漂移。
  不修改错误断言、不直接改 manifest、不降低 hash 检查，也不更新正在使用的用户资源。
  Linux/Windows 均重新准备独立、当前源码 seed 后重跑全量，各 519/519；旧失败不删除。
- 本地脚本第一次 exit 127：隔离 Linux 工具链没有 rustfmt；日志 `current-seed-run.log`
  保留，改用已安装 Windows rustfmt。新增完整字段后残留的无效 struct update 导致 strict
  Clippy 失败，`linux-clippy-current.log` 保留；删除无效语句后完整原生三轮通过。

## 尚未闭合的范围

X11 仍需：其他 toolkit、带装饰窗口/实际 WM、多显示器/DPI、IME composition、选中文本的
插入语义、AT-SPI 动作超时/断连后的实际占用恢复，以及安装版与模型验收。
macOS AX/完整动作和签名资源交付、GNOME native Wayland、Windows/浏览器清理长尾、
WebView 物理取消、正式 Chrome/Edge 扩展、所有平台安装/更新/回滚、重设计 UI 的原生矩阵、
实际 Grok E4 和冻结 12 小时 active soak 继续保留为完整目标；本批不宣称完成。

上游 API 依据是 GNOME `at-spi2-core` 官方 XML：Accessible/Action/Component/Text/EditableText，
本批目录保存原始 XML。`InsertText` 的位置按字符、length 按 UTF-8 字节处理；
XRes/zbus API 还核对了 lock 中实际 crate 源码，没有引入全局 helper 或凭据。

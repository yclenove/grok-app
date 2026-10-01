# Computer Use：X11 run 授权与精确原生 Wait

日期：2026-09-27（Asia/Shanghai）。这是已实现并验证的局部进展，**不是最终版完成声明**。

## 实际缺口与实现

- 原 X11 `capture_for_run` 丢弃 run ID，AT-SPI 快照也未绑定 run。真实 GTK 红测证实另一 run 可以复用原观察的引用写入控件。现模型观察、快照、原生输入均绑定原 run/target/snapshot/geometry；不是仅在 Broker 外层补过滤。
- 新增最多八项的独立模型授权登记。模型捕获开始即退休同 run/target 的旧授权，发布必须匹配尚存的捕获 ticket；失败、被替换或释放后的迟到捕获不得复活。独立本地锁不等待远端 AT-SPI，Stop/释放/新模型观察可以令等待中的引用失效。
- 坐标输入同样校验本 run 的实际模型截图；猜测 snapshot、跨 run、显示预览、无截图观察不能授权。每个新 XTEST 事件前重查授权；已经拥有的按键/按钮释放仍是取消后必须执行的清理，不因授权退休而跳过。
- 无 run 的 `observe` 变为 display-only；`observe_for_run` 明确请求模型观察。预览既不铸造模型引用也不退休原模型。Stop 退休旧代次和未发布捕获；已进入更新代次的动作不受迟到旧 Stop 影响。未知原生完成依旧保持占用，不强行置 idle。
- X11 opt-in 既有 `wait_uses_retained_reference` 路径，公开 semantic-only Wait。保留原 AT-SPI unique bus owner/object path，校验 XRes 绑定进程、原窗口、角色、完整祖先链/成员关系、可见性和原几何；不重抓树，不按名称或位置替换对象，不读取输入值、不抢焦点、不派发输入。
- 名称允许真实变化；可见但禁用/失焦的公开标签也可只读等待。隐藏、已销毁、同名/同位置替代对象、未知引用、跨 run、旧截图和 desktop fallback 被拒绝。成功为 `applied=false / verifiable=true / postcondition_ok=true`。
- 参数仅 `nameEquals / timeoutMs`，默认 2000ms，范围 1–10000ms。每次远端查询前后、返回匹配前检查取消/期限/授权；过期结果不得成功。25ms 轮询间隔不超出剩余时间。原生同步调用不能强制抢占，**不宣称全链路硬实时 deadline**；AT-SPI 方法设一秒超时，X11 同步服务失去响应仍需进一步故障验收。
- 修正已有状态位误用：AT-SPI wire enum 的 `SENSITIVE=24 / SHOWING=25 / VISIBLE=30`，不是 ATK 的枚举。原输入检查未检查真正的 visible；现在输入要求 enabled+sensitive+showing+visible，Wait 只要求可见和未 defunct。属性读取显式关闭 zbus 属性缓存。
- Wait 期间能力查询不等待 AT-SPI 长轮询；并发动作不得重叠取得 NativeActionSlot。原生语义动作在捕获占用 AT-SPI 时快速拒绝，而不是无限排队。

生产文件在 `src-tauri/computer-use-x11/src/`：`authority.rs`、`accessibility_wait.rs`、`lib.rs`、`accessibility.rs`、`input.rs`。根 App 的 `linux_adapter.rs` 仍直接重导出同一生产实现，不引入 fixture 专用输入路径。

## 先失败、再修复的证据

证据根：`tools/computer-use-probe/.run/x11-wait-20260927/`。旧失败均保留，不覆盖日志。

- `run-isolation-red.log/.exit`：真实 owned GTK 下 **exit1**，明确 `foreign run reused another run's observed AT-SPI authority`；`run-isolation-green` 随实现修复转为通过。
- `wait-first`：Unicode 标签改变实际分配尺寸，正确触发原几何变化拒绝。固定夹具标签分配尺寸以隔离“同对象名称变化”正向用例，未放宽生产几何校验。
- `wait-fixed-layout / wait-state-diagnostic`：禁用标签露出原状态位错误。对照 GNOME `at-spi2-core/atspi/atspi-constants.h` 原始头文件并用真实 GTK 状态验证；原始下载保存为 `atspi-constants-primary.h`，审计逐项核对 wire 数值。
- `wait-wire-state`：夹具 focus-back 会 restack 窗口、使旧 geometry 失效。独立 timeout 测试先取得当前模型观察，不绕过生产 stale 拒绝。
- `expanded-clippy`：unused-mut 导致 **exit101**。审阅时同时发现 clone 共享取消令牌会污染后续隐藏用例；改为独立取消令牌，并用全新令牌重验 Stop 后旧授权确实失效。未删除负向断言或允许 warning。
- `final.sh` 在完成构建/单测/Clippy 后因 shell 拼接缺换行报错，原脚本和失败说明保留；修正为 `frozen.sh`，先 `bash -n` 再运行。生产源码未改变。
- `frozen-format` **exit101** 是独立 WSL 工具链缺少 cargo-fmt，不是源码格式结果。随后宿主 Windows 的 `cargo fmt --all --check` 实际 **exit0**，分别保存证据，不覆盖/伪装 WSL 失败。

## 冻结验证结果

| 项目 | 结果 | 边界 |
| --- | --- | --- |
| X11 Rust 单测 | `frozen-unit`：**17/17，exit0** | 新增授权隔离/ticket/代次/无图/容量与 Wait 参数合同 |
| Linux Core + driver | `frozen-core`：**560 + 16 = 576/576，exit0** | 使用当前隔离种子；driver 是真实 owned 子进程协议 fixture，不冒充发布 driver |
| Rust 合计 | **593/593** | 0 failed / ignored / filtered |
| 原生语义门禁 | `frozen-semantic-1/2/3`：**25/25，各 exit0** | 三个独立 GTK + accessibility bus + Xvfb 运行 |
| 原生坐标门禁 | `frozen-coordinate-1/2/3`：**19/19，各 exit0** | 三个独立 Xvfb，真实像素/事件/窗口生命周期读回 |
| 原生重复范围 | **每轮 44 项，连续三轮通过** | 不是 132 个不同功能，也不是 12h soak |
| 构建 / strict Clippy | `frozen-build / frozen-clippy / frozen-core-clippy`：**exit0** | X11 all-targets+native-probe，Core all-targets+test-support，`-D warnings` |
| 格式 / Python 语法 | `frozen-format-windows / frozen-python-syntax`：**exit0** | Rust workspace、GTK 夹具 AST |
| 冻结源核对 | `source-before-final / source-after-final`：**294 个文件零漂移** | Core/X11/根适配器源码、选定依赖与 CI；不是整个脏工作树 |
| 证据审计 | `final-audit.json / receipt.json / receipt.sha256` | 实际退出码、每项有序原生门禁、单测数量、wire 常量和逐文件 SHA-256 回读 |

本地环境：WSL Debian；`at-spi2-core` 和 `libatk-bridge2.0-0t64` 为 `2.56.2-1+deb13u1`。原生二进制摘要见 `frozen-native.sha256`。已有 CI 的 X11 构建/单测/Clippy/原生探针入口会运行扩展后的测试；本批未修改 CI，也没有宣称远程 CI 已执行。

25 项语义门禁包含真实延时 Unicode 名称变化、即时只读匹配、可见禁用/背景标签、超时和零应用效果、参数/取消/引用拒绝、隐藏/替代标签、活动 Wait 的 Stop/释放/替换观察、即时能力查询和不重叠占用。生产 Broker 还验证真实延时 Wait、已知无副作用的超时拒绝，以及 timeout 后同一原生引用仍可使用。Broker 延续既有 Wait `Verified / executed=true` 的尝试记账语义，不表示输入发生。

## 未完成项与下一步

- 本轮是 **生产 X11 模块在 owned Xvfb/GTK/AT-SPI 的真实执行**，不是已安装 App、真实桌面多应用、GNOME 原生 Wayland、macOS 或 Windows 的新验收。应用 oracle 读取文本/点击/状态；不据此声称覆盖所有全局输入事件、输入法或所有工具包。
- 非响应 accessibility/X server、窗口/对象快速复用、保护态/祖先变化竞态、完整 clipboard/选择替换/中文 IME/恢复仍需扩展；UI/安装包没有因此完成。失败即拒绝和未知占用隔离不能当作已具备完整功能。
- 保留完整目标：Windows、macOS arm64+x64、Linux X11+GNOME 原生 Wayland；Desktop、托管浏览器、既有标签页、App WebView；App/ACP/MCP；签名安装/升级/回滚；重设计原生 UI 窄窗/系统缩放/权限矩阵；真实 Grok E4；冻结源码 **12 小时 active soak**。此前 Windows 发布/浏览器长尾问题没有被此轮关闭。
- 下一阶段继续补 Linux 原生文本选择/IME/剪贴板及恢复缺口，并按原计划完成其余平台和最终交付门禁；不能将这一批 Wait/授权通过重新定义为最终目标。

本轮有生产代码修改、真实红绿验证和冻结回读，分类为 **progress**。目标保持 **active**，没有完成/暂停/阻塞状态写入；没有 commit/push/PR，没有替换用户 App/runtime，没有操作权限提示。

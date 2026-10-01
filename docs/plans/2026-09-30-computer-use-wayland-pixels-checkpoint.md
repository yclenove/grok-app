# Wayland 截图呈现与像素输入检查点 — 2026-09-30

状态：**active / partial — not releasable**。上一可见目标轮没有可验证的代码改动，按 no progress 处理；本轮重新核对实际 HEAD、310 源冻结与旧证据后完成源码、原生测试和文档变更，属于 progress。没有把未知进程当作仍在运行，也没有因观察超时重启任务。

## 实现与边界

- 新增不可伪造、一次性 `PresentedFrame` 与只读 `PresentedImage`。`ObservedFrame::present(max_width,max_height)` 保留原授权、原 capture 时间和原 owner，不允许用外部图片或 dimensions 重建凭据。
- 按 SPA 的 **已施加到 buffer 的变换** 做逆变换；八种旋转/反射全部支持。完整非对称矩阵断言验证方向和反射顺序；实际 PipeWire 非对称像素验证四角与 RGBA 通道。
- 有效 crop 是内容，不是桌面 offset；先转正，再按 1..8192 的输出限制保持整数宽高比例缩小。中心最近邻采样、不放大、不加 letterbox、不把 padding 当画面。
- `PortalSession::move_to_pixel(presented,x,y)` 将呈现图的零起点整数像素中心映射到唯一 mapping-id 配对的 EI logical region；原生 owner 只加一次 region origin，不重复乘 physical scale，也不使用 portal compositor hints 猜像素比例。
- 仍在真正 native dispatch 前检查原 frame epoch、EI generation、owner、撤权和 freshness。跨 geometry/resize、越界、过期和 Stop 后的凭据不能操作；这是 **指针移动提交**，不是点击或应用效果回执。
- 初次实际 EI 测试发现 double API 在协议中编码为 **float32**，114.285714… 实际收到 114.28571320。查验本机精确 libei 1.5.0 `protocol.xml` 后，测试按实际 float32 编码及 8 位小数日志校验，而不是任意放宽误差。
- 同时修复原生绝对输入的真实边界缺口：预检 float32 编码后的坐标，禁止舍入越过 region 边界；像素 API 还拒绝因大 origin/精度不足而落入另一个 screenshot pixel，不作静默 clamp。

语义来源保存在本阶段 `upstream/`：实际 SPA SDK header、PipeWire 的 GStreamer orientation 映射、GStreamer tag 定义、portal XML、Mutter 48.0 crop producer、精确 libei SDK protocol/source。GitHub main 上旧 Mutter 文件路径 404 的初次请求结果保留，后来取得的 48.0 源码只用于语义对照，**不是本机运行了 GNOME 48 的证明**。

## 本轮实际验证

**311 个选定源码冻结**，相对旧 310 源，仅 Wayland crate 内 7 个既有文件改变，新增 `src/presentation.rs`；无依赖升级，Cargo.lock 原 **693 包**版本/checksum 不变，无新增包。

- 同一归档 joint-feature Wayland test executable 三轮 **50 passed / 0 failed / 0 ignored**，线程 1/4/4，包括 **11 个显式 native tests**。
- 新原生端到端用例验证八种真实 SPA metadata 的逆向呈现、crop、resize、缩小中心采样、实际 scale=2 的 EI region、精确 **17 条** absolute wire events；负例没有额外 motion/key/button。单位测试另覆盖多种 physical scale、坏尺寸/长度、float32 大 origin 和边缘舍入。
- Wayland + X11/native-probe 全 targets 严格 Clippy `-D warnings` 通过；原始两处测试 Iterator::last 告警保留，改为 rfind 后通过，没有添加 lint suppression。
- X11 单测 **49**；owned Xvfb **19 PASS 组**；GTK/AT-SPI **41 PASS 组**：本轮重新执行通过。
- Windows App library offline/locked check、workspace fmt、`git -c core.safecrlf=false diff --check` 均退出 0。
- 最终三轮在 **同一 Linux 调用**内用 `TMPDIR=/var/tmp` 执行并立即归档。**78 个最终必需夹具目录**完整；加上初期测试共 **82** 个目录，缺失 **0**，对应 owned fixture 进程残留 **0**。不复制 socket、不触碰用户桌面。
- 静态 libei 1.5.0 archive 与上一阶段一致，最终 ELF 没有 libei 动态依赖；五个归档 executable 前后 hash 一致。

确切 Wayland executable SHA256：
`c9a52eff37729a904c8008d9e1c8cfcb589c9f8d394758a587dd040b4a15315a`

证据目录：`tools/computer-use-probe/.run/wayland-pixels-20260930/`。
之前的 receipt 原字节保留，SHA256：`ccdbfa2975915fae04281cff69c2d35484a0cb3a7e46d43c4182231536e448ea`。
初次浮点断言失败和实际 wire log、随后成功复测、Clippy 初次失败均保留；早期 shell quoting / socket-copy 的辅助命令失败也记录，不计为测试成功。

## 仍须完成

这是 E2 原生 IPC/像素/输入事件，不是 GNOME compositor、实际 App 或 Grok E4。有效内容与 EI region 的真实 GNOME 同步、静态画面、2 秒 freshness 与真实模型延迟必须继续验证并完善；当前严格 TTL 不代表产品可用体验。

下一步：接入 Host 的图片编码、观察 ID 与 model response 绑定、intent/permission/target、复合 action 与效果验证；在真正 GNOME 上验证配对/resize 时序和静态帧生命周期。然后继续完整 IME/clipboard/取消恢复、AT-SPI scope 和 portal parent/restore UX。不把 motion API 冒充完整点击/输入系统。

**App 仍拒绝 Wayland，未设 native_wayland=true**。本轮未改安装态、注册表、签名、发布配置或 App shell；未 commit/push/tag，未跑远端 CI 或完整前端/Core；不冒称这些已验收。

完整目标不变：Windows x64；macOS arm64/x64；Linux X11 和原生 GNOME Wayland；Desktop/managed browser/existing Chrome+Edge/App WebView；App/ACP/MCP；完整输入/中文 IME/clipboard/取消与恢复；签名干净安装/更新/修复/回滚/卸载；原生窄窗/DPI/权限 UI；真实 Grok E4；**同一最终冻结候选 12h active soak**。
没有缩小最终成功条件；goal 保持 active，不调用 complete/blocked/paused。

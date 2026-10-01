# Computer Use：Wayland Host 协议绑定与短复合输入检查点

状态：**active / partial — not releasable**。上一轮是真实 progress；本轮修改了生产
Wayland crate 并完成本检查点的原生回归，不是完成整个 Computer Use 目标。

## 原始范围不变

Windows x64、macOS arm64/x64、Linux X11 **和原生 GNOME Wayland**；Desktop、
managed browser、existing Chrome/Edge、App WebView；App/ACP/MCP；完整输入、
中文 IME、clipboard、取消/恢复；签名后的干净安装、更新、修复、回滚、卸载；
真实窄窗口/DPI/权限 UI；真实 Grok E4；**同一最终冻结候选的 12h active soak**。
局部库回归不能替代这些验收，也不能将目标改写成只有 transport/fixture 的版本。

## 本轮落地

- `host_session.rs`：run-owned `PortalHostSession` 持有实际 portal session 与唯一
  target ID，直接复用 core `Observation` / `CaptureOptions` / `DispatchRequest`。
  精确绑定 Host generation、target generation、snapshot、geometry 和一次性
  `PresentedFrame`，不从 JSON 字符串重建原生授权。
- PNG 来自实际 upright/cropped RGBA；编码超过 core 图片限制时按比例缩小，并继续
  绑定原票据。像素坐标按所属像素取中心，半开边界与 float32 EI 精度检查仍生效。
  固定 monotonic/UTC anchor 保留原始像素交付时间，不给静态图重新盖当前时间。
- preview 不颁发、不替换、不刷新 model ticket；preview ID 不授权输入。每次
  model capture **尝试开始就退休旧票**，包括取消、无截图和编码前失败。
- `sequence.rs` / `ei_owner.rs`：一次观察下的 2..8 个短事件，支持点按/双击、
  命名导航键按下释放、滚动及结束。整个序列在第一条 FFI 前预检 capability、
  坐标、独占 held state、成对状态转移，持有原 EI/frame 锁直到短序列提交完。
  每条事件之间检查 core cancellation；已尝试提交后发生取消/错误，清理本 owner
  的合成输入，不释放别人的状态、不重放。测试覆盖 press/scroll 后取消的清理顺序。
- 对照现有跨平台约定时发现零滚动不能先移动鼠标，已修复：只校验并消费观察授权，
  `applied=false`，无队列/FFI/指针移动。不是把无效参数 clamp 成零。
- Window capture 仍不能冒充逐窗口 keyboard scope；该 Host bridge 拒绝 window
  scope。Monitor 仍诚实标注 session-wide input。语义输入、文字/IME、clipboard、
  timed drag、wait 等未完成路径明确拒绝，没有临时模拟或 desktop fallback。

## 已验证事实与边界

证据目录：`tools/computer-use-probe/.run/wayland-host-20260930/`。

- 冻结 **314 源文件**：对前阶段 311 源修改 11 项、新增 3 项；仅 Wayland crate
  及 Cargo.lock。本次复用已有依赖，**693 个既有包版本/checksum 不变，无新增包**。
- 同一 joint-feature Wayland test executable 三轮，线程数 **1 / 4 / 4**；每轮
  **64 passed / 0 failed / 0 ignored**，其中 **15 个 native tests**。SHA256：
  `30f1e70c010b9ff8ea42cdb1c3d526f2a06ade655426dddf32f1bafc4f223f00`。
- 三个新原生用例使用真实私有 PipeWire daemon/C source、libei/libeis 与 D-Bus
  SCM_RIGHTS，不使用用户桌面。验证真实 PNG 每个像素的非对称 crop oracle；
  right double click、Enter press/release、正负滚动、零滚动、preview、重复/替换
  授权、14 类伪造/越界请求、失败 capture、Stop 与整组预检。
- Node 独立解析 PNG chunk/zlib/filter，还原 **28×20×4=2240 字节**，检查每个
  像素的 G/B/A；不依赖 Rust image decoder。每轮再对照独立 C EIS peer 原始日志：
  **13 个确切输入事件**，3 次坐标、4 个按钮边沿、2 个键边沿、2 次带符号滚动及
  2 次结束；零滚动无输入。报告不能替代 peer，双方必须精确一致。
- **102 个最终 native 夹具目录**全部保留，连同早期共 **108 个**，缺失 0；
  同一 WSL invocation 归档后检查 owned fixture 进程，残留 0。没有全局 kill。
- X11 单测 **49**、owned Xvfb **19 组**、GTK/AT-SPI **41 组**通过；联合
  Wayland+X11/native-probe 全 targets Clippy `-D warnings`、Windows App lib
  `--offline --locked`、全 workspace fmt、tracked whitespace 均通过。
- 静态 libei 1.5.0、PW/EIS C peer、X11 executable 与前阶段相同；检查 ELF 没有
 动态 libei 依赖，五个 executable 在执行前后 SHA256 不变。

## 保留失败，不补造绿色结论

1. 首次 GTK fixture 就绪通道超时（尚无第一个 PASS）。失败原日志保留。
   后续**完全相同 X11 binary**独立复测三轮各 41 组成功，最终回归也成功。
   没有放宽超时或改 X11 实现；**首次超时原因未被证明，不能宣称已修复**。
2. 独立 verifier 首版错误地用 `JSON.stringify` 比较不同字段顺序的等价对象，
   错报找不到对应 peer。原脚本/可复现失败日志保留；修为对象深度相等后，
   三轮 PNG 解码和原始 peer 比对全部通过。没有修改被验证的生产源码/测试结果。

## 尚未完成，下一步不能跳过

`PortalHostSession` 是生产库内的异步协议桥，**不是已接线的同步
`ComputerUseAdapter`**，更不是 App/ACP/MCP 可用功能。后续必须完成：

1. App adapter 的真实 run claim/list/observe/act/abort/idle 所有权和异步 owner
   接线；同步 trait 不得阻塞 Tokio reactor；Host 生成代际/geometry 必须与 broker
   capture normalization 对齐；终止不得只清记录而留下 native owner。
2. Host 显式 intent/permission、lock screen/user takeover、source/target revocation、
   portal-parent/restore UI、效果核验与失败恢复，之后才可启用 capability。
3. 真实 GNOME/Mutter/portal 自动配对及安装后 App E4、全输入/IME/clipboard、
   多平台/安装链/UI、真实 Grok 推理、同一最终冻结候选 12h active soak。

**App 仍拒绝 Wayland，未宣称 `native_wayland=true`。** 原生提交 receipt 只说明
发送到 libei，不是目标应用效果；Streaming/缓存图也不证明 compositor 有响应。
本轮没有 commit/push/tag/release、没有系统安装/注册表或用户桌面操作。
不满足 complete 或 blocked 审计；不调用 goal 状态变更。

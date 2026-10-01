# Wayland 原生观察授权绑定检查点 — 2026-09-30

状态：**active / partial — not releasable**。上一轮和本轮均有真实代码、测试和证据变更，属于 progress；不是已验证等待或最终完成。

## 本轮实现

- 新增 `ObservedFrame` / `PortalSession::observe()` / `input_observed()`：只由原 session 生成、不可从外部帧字段构造、不可 Clone/序列化的一次性原生观察凭据。
  同名 run 字符串不等于同一个 owner；凭据绑定实际 `Arc<FrameStore>` 和 `Arc<InputGate>`，并核对 portal run/node/serial/mapping。
- EI device generation 在提交及原生执行时检查。截图授权在原生 owner **实际 FFI 提交前**再次核对，锁顺序固定为 EI gate → FrameStore，帧锁覆盖整个同步 native action。
  这不是仅在排队前检查一次；clear/revoke/格式、裁剪、尺寸、orientation 改变不能在验证与提交间穿透。
- FrameStore 引入单调 epoch，记录 run/node/serial/format/crop/buffer size/transform 与序号或时间倒退；clear 和几何 ABA 不会恢复旧凭据，计数耗尽关闭授权。
  相同几何的新视频帧可以继续到来，不要求模型赶在下一视频帧前提交；但不会刷新旧观察原始 capture 时间。
- SPA `VideoTransform` 显式协商、读取并保留全部八种值；未知值失败关闭，orientation 变化使观察失效。真实 C 源通过八次受控信号产生 metadata 变化和一次 ABA。
  RGBA **仍为裁剪后的 buffer-oriented 像素**，没有把元数据覆盖当作渲染或截图坐标转换已经完成。
- 输入仍是 EI logical-coordinate / evdev 原语。原有取消提交撤权、Stop/Drop、held-key cleanup 与双 native owner retirement 保留；capture loss 的实际联动测试改用观察绑定 API。

## 真实验证与失败保留

**310 个选定源文件冻结**；相对上一轮 309 个源，仅 Wayland crate 内 10 个既有文件变化、新增 `observation.rs`。其他选定源码未变。

- 同一个归档 joint-feature 二进制六轮，每轮 **42 passed / 0 failed / 0 ignored**；线程顺序 1/4/4、1/4/4。
  包含 10 个显式 native tests；最后三轮是完整日志归档的最终复跑，不等于 12h soak。
- 新原生测试：实际 PipeWire 像素 + private portal/SCM_RIGHTS + libei/libeis，验证跨 owner（相同 run 名）、EI pause/resume generation、八种 transform 与 ABA、resize、旧观察超时全部拒绝。
  新凭据确实发出 key-down/up 和精确逻辑绝对坐标事件；负例没有发出 key 31，Stop 后不能复用凭据。
- 单测覆盖 frame epoch 各身份字段、clear/geometry ABA、旧像素不能通过新帧延长寿命、实际 mutex 持有范围、epoch overflow、未知 transform、grant 与 mapping 不符、不可通过修改像素副本转移 owner。
- Wayland + X11/native-probe 全 targets 严格 Clippy `-D warnings`：通过。
- X11 单测 **49**、owned Xvfb **19 PASS 组**、GTK/AT-SPI **41 PASS 组**：本轮重新执行通过。
- Windows App library offline/locked check、workspace fmt、`git -c core.safecrlf=false diff --check`：通过。Git 检查保留现有换行规范，不修改仓库配置。
- Rust Cargo.lock 原 **693 包**版本/checksum 不变、无新增包；复用上轮已验证的 static libei 1.5.0 SDK。最终 ELF 无 libei 动态依赖。

初次新增测试有编译类型推断错误；修正后运行出现 fixture peer-count 断言错误：两个 transport 均是真实 server，因此 fake socket peers 数为 0，不是 1。
修正后仍验证真实 EI disconnect 和 PipeWire capture node 不再存在，未删掉实际退休检查。严格 Clippy 的 test-module 排序问题亦已修正。原失败日志保留。

首次三轮测试通过，但后续证据收集发现 `/tmp` 为 tmpfs，**120 个早期夹具目录已不可见**；不能补写或声称原日志已归档。
保留缺失路径列表、失败收集日志和挂载检查。对**同一确切二进制**设置 `TMPDIR=/var/tmp` 后完整复跑三轮，在同一 Linux 调用内立即收集：
**72 个最终夹具目录均归档，无最终所需目录缺失**；按本阶段环境/专属路径检查，未发现拥有的 fixture 进程残留。未用进程名全局 kill。

确切 Wayland 测试二进制 SHA256：
`680df2fc6db916a51e73768874db221c30b3a6409aa008d87ed645a1bec2c2d7`

证据：`tools/computer-use-probe/.run/wayland-observation-20260930/`。
上轮 EI receipt 已封存并由 Node/Python 双校验器核对 410 项，原字节 SHA256：
`d8d3e2c3a586bc3e329120af33aac6f326991c610d557cd719e110d27f1c172e`。
当前阶段明确记录源码演进，不声称改完源码后仍与旧阶段 source hashes 一致。

## 未完成与下一步

这仍是 **E2 / 真实 native IPC、帧和输入事件**，不是 native GNOME compositor 或 App 安装态 E3/E4。
一次性凭据不是 Host 用户意图/权限/target scope，也不证明 UI 内容在观察后没有变化或 application effect 已完成。
当前沿用 2 秒 capture freshness 上限，静态画面与真实模型延迟必须继续解决，不能用该严格子集冒充完整产品体验。

下一条实施路径：完成面向模型的帧呈现及八种 orientation/crop/DPI 的截图像素→EI region 坐标映射；在真实 GNOME 上建立静态画面与观察 TTL 规则；
接入 Host desktop adapter 及其 intent/permission/target/action-completion 机制，再补 IME/clipboard、AT-SPI scope、portal parent/restore UX。
**App 仍拒绝 Wayland，未设置 native_wayland=true**。没有以原语通过测试为由打开未完成入口。

完整目标不变：Windows x64；macOS arm64/x64；Linux X11 和原生 GNOME Wayland；Desktop/managed browser/existing Chrome+Edge/App WebView；
App/ACP/MCP；完整输入/中文 IME/clipboard/取消与恢复；签名干净安装/更新/修复/回滚/卸载；原生窄窗/DPI/权限 UI；真实 Grok E4；
**同一最终冻结候选 12h active soak**。本轮没有重跑完整前端/Core、实际安装/发行或远端 CI，没有 commit/push/tag。
最终版验收证据未齐，goal 保持 active，不调用 complete/blocked/paused。

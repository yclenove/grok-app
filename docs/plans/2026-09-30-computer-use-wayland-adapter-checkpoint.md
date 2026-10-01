# Computer Use：Wayland Broker 适配器与精确回收检查点

状态：**active / partial — not releasable**。上轮和本轮均为真实 progress；
同步 adapter 与真实 Broker 路径已实现和回归，完整目标仍未达到。

## 原始范围不变

Windows x64、macOS arm64/x64、Linux X11 **和原生 GNOME Wayland**；Desktop、
managed browser、existing Chrome/Edge、App WebView；App/ACP/MCP；完整输入、
中文 IME、clipboard、取消/恢复；签名后的干净安装、更新、修复、回滚、卸载；
真实窄窗口/DPI/权限 UI；真实 Grok E4；**同一最终冻结候选的 12h active soak**。
以下 private native tests 不能替代这些最终验收。

## 已落地的生产代码

- `PortalAdapter::from_granted` 消费单 run 的 `PortalHostSession`。私有线程及
  runtime 串行执行有界命令；不在 policy、capture、输入或 teardown 期间持有
  adapter 状态锁。原授予 session 的 runtime 必须活到原生 owner 退休。
- plain/blocking 调用可以等待，multithread Tokio 用 `block_in_place`；
  current-thread reactor 在 admission 前拒绝，避免把异步 owner 饿死。
- run-scoped list/claim 隔离，foreign run 不见目标。core 增加向后兼容的
  `capture_for_run_at_generation`；Broker 在 normalize 前传入可信实际 generation，
  不从 agent action 猜测或重新采用代际。preview 不替换 model snapshot。
- Stop 先取消并撤销原生授权，idle 必须等**原始 native owner 与 adapter worker**
  都结束并 join。调用者 5s 超时不释放 busy；未知输入结果退休整个 session；
  release/Drop 不伪造 completion；panic/未证明清理仍保持隔离。
- `PortalInputPolicy` 没有 permissive default。它只是 Host admission 接口，
  **不是已经实现的 OS lock-screen / user-takeover detector**；异步权限丢失
  仍须由 Host 主动撤销。Pause 当前退休 grant，尚无自动恢复/重新授权。

## 实际复现并修复的缺陷

真实 PipeWire/PNG/EI 用例先失败：取消下一次 model capture 后，旧 snapshot
仍能提交输入（`old snapshot survived cancelled`）。修为在 model capture
admission 时先清除 adapter snapshot，再检查 cancellation / policy；成功的新
model capture 且非 stopping 才发布新 snapshot。取消、policy 拒绝、无 image
三类失败均不能保留旧授权；preview 仍不会破坏 model authority。

`red-tests.log`、原错误 adapter 源、初次跟进编译失败以及修正后的
`green2-tests.log`（**7 passed**）均保留，不把重跑当作未解释失败的修复。

## 验证及证据

证据：`tools/computer-use-probe/.run/wayland-adapter-20260930/`。

- 冻结 **317 源文件**，相比前阶段 314：10 项修改、3 项新增；仅 core 的
  adapter/Broker hook 与 Wayland crate。Cargo.lock **693 包版本/checksum 不变**。
- 同一 joint-feature Wayland binary **1 / 4 / 4 线程三轮**，每轮
  **71 passed / 0 failed / 0 ignored**，包括 **18 个 native tests**。
  SHA256 `3a8fb3ddb1b12d91c9557dfd5d76cd3e5e44502fcb5f0f597ecee92ead6484cb`。
- 新增真正 Broker → PortalAdapter → PNG → libei/C EIS 路径，涵盖授权
  generation 2→3、preview、foreign run、重复 action ID 不重放、过期拒绝、
  source loss、Stop/native join。独立 Node PNG 解码逐像素检查 28×20 RGBA，
  新 Broker 用例每轮 **5 条原生事件**与原始 peer 精确匹配；此前 Host 用例
  **13 条事件**仍独立比对。提交不是目标应用效果证明。
- 最终 **120 个唯一原生夹具目录**，连同早期共 **132 个**完整；缺失 0，
  owned fixture 进程残留 0。第二轮原归档只识别 39 个：stdout/stderr 交错
  在 `owned libeis logs:` 后插入换行。保留旧归档/失败 verifier，修正空白
  解析后从**仍存在的原目录**补归档为 40，未重建或伪造 peer 日志。
- 完整 Linux core 在正确独立 native seed 下 **564 passed / 0 ignored**；
  首次 **549 passed / 15 failed** 保留，根因是 WSL 使用默认 Windows seed。
  用生产 prepare/check 和当前 worker 源新建 Linux seed，独立 `GROK_CU_TEST_SEED`
  只影响测试，不修改已打包 Windows seed，不跳过哈希/模块 import 验证。
- Windows core Broker **206**、adapter contract **4**；X11 unit **49**、
  owned Xvfb **19 组**、GTK/AT-SPI **41 组**通过。本阶段 GTK 没有启动失败；
  前阶段的历史启动超时根因仍未被证明，不能因此宣布修复。
- core + Wayland + X11/native-probe 全 targets Clippy `-D warnings`、
  Windows App library check、workspace fmt、tracked whitespace 通过；
  五个原生 executable 执行前后哈希相同，ELF 无动态 libei 依赖。

## 当前缺口与下个实现边界

**App factory 仍使用 X11 adapter 并拒绝 Wayland，`native_wayland=false`。**
直接 Broker 测试不等于产品路径已接线。继续实现：

1. 产品 `HostOwnedAdapter` 的 run-scoped list/claim 和新 generation hook
   透传，再接 run registry、portal negotiation/runtime owner、App parent 和
   显式用户 intent/permission。当前 wrapper 的默认转发仍会丢失这些新语义，
   是下一步可执行的接线工作，不是等待外部条件。
2. OS lock-screen/user takeover、source/target 监控、效果核验、暂停恢复和
   portal restore UI；完整 text/IME/clipboard/timed drag/semantic wait。
3. 真实 GNOME/Mutter/portal 自动配对、安装后 App/ACP/MCP E4、跨平台及
   全安装链/UI/真实 Grok、同一最终候选 12h active soak。

本阶段未 commit/push/tag/release，未执行用户桌面动作或真实安装。
完整目标保持 active；既不满足 complete，也不满足 blocked 审计。

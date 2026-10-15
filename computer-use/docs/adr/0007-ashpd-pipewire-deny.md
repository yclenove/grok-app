# 0007 Wayland ashpd pipewire 依赖许可与构建门禁

状态：接受（选项 A，总控裁定批准）
日期：2026-10-14

## 背景（事实 + 证据链接）
[03 §2](../../../docs/plans/computer-use-v2/03-architecture.md#2-crate-划分与依赖规则) 与 [06 §P7-02](../../../docs/plans/computer-use-v2/06-roadmap.md#p7-02-portal-screencast-截图与-restore-token)
规定 Linux Wayland 截图使用 `ashpd 0.13` 的 `pipewire` feature（沿其 `pipewire 0.9`，不单独依赖 pipewire），CI 依赖 `libpipewire-0.3-dev`。

在执行 P7-02 任务门禁 `cargo deny check` 时发现：
1. `ashpd 0.13` 启用 `pipewire` feature 会传递引入 `pipewire 0.9.2` -> `libspa 0.9.2` / `pipewire-sys 0.9.2` -> `system-deps 7.0.8` -> `cfg-expr 0.20.10` -> `target-lexicon 0.13.5`。
   `target-lexicon` 采用 `Apache-2.0 WITH LLVM-exception` 许可，当前 `deny.toml` 的 `licenses.allow` 仅允许标准 `Apache-2.0`，因此触发 `error[rejected]: failed to satisfy license requirements`。
2. `pipewire-sys 0.9.2` 的构建依赖引入 `bindgen 0.72.1` -> `cexpr 0.6.0` -> `nom 7.1.3`，而 `libspa 0.9.2` 直接依赖 `nom 8.0.0`。两个不兼容的 `nom` 主版本并存，触发 `deny.toml` 的 `error[duplicate]: found 2 duplicate entries for crate 'nom'` 门禁。
3. 共享编译机 build8 未安装系统动态库头文件 `libpipewire-0.3.pc`，`libspa-sys` 构建脚本在探测 `pkg-config --libs --cflags libpipewire-0.3` 时失败退出 101。

根据 08 §2 规则 9、08 §5 及任务卡硬性要求：“先 `cargo deny` 检查新增依赖许可/advisory，有问题写 ADR 并停下问（NEEDS-DECISION），不要放宽 deny.toml”，执行工程师不得在未获授权前擅自放宽 `deny.toml`。

## 决定
采用**选项 A**，实施最小门禁调整，不扩大全局白名单：
1. **deny.toml 变更清单**：
   - `[licenses.exceptions]`：为 `target-lexicon` **具名**允许 `Apache-2.0 WITH LLVM-exception`（构建期传递依赖；无全局白名单扩散）。
   - `[bans.skip-tree]`：增加 `{ crate = "bindgen", reason = "pipewire-sys/libspa-sys build-time bindgen dependency brings in nom 7" }`，精确豁免构建期由 bindgen -> cexpr 引入的 `nom 7`，不整体关闭 duplicate 检查。
2. **驱动实现分层与门控**：
   - 抽象 `PortalScreenCastClient` 与 `PipeWireFrameReceiver` trait，FakeReceiver 覆盖全平台测试。
   - `AshpdSessionWrapper::open_pipewire_remote` 暴露真实 PipeWire remote FD。
   - 真实 `RealPipeWireFrameReceiver` 门控于 `#[cfg(all(target_os = "linux", feature = "pipewire"))]`，接收 PipeWire 视频帧流并转换单帧交付。
   - CI Linux job 安装 `libpipewire-0.3-dev`；Windows/macOS 目标不编译 PipeWire。
   - build8 编译机缺乏 C 头文件时通过门控验证可移植部分，真实 PipeWire 编译验证由 GitHub Actions Linux CI job 承载。

## 影响（对接口、测试、用户）
- `cargo deny --locked check` 全绿（advisories ok, bans ok, licenses ok, sources ok）。
- `tests/portal_capture.rs` 在所有平台保持 100% 契约验证通过。
- 架构安全约束完全满足：无特权提升，无全局许可放宽。

## 补充（总控审阅）

- 选项 A 已批准并落地：`deny.toml` 仅新增 `target-lexicon` 的具名许可例外与 `bindgen` 子树的 `nom` 重复跳过，未放宽其它条目。
- CI 的 Linux job 通过 `ppa:pipewire-debian/pipewire-upstream` 获取新版 `libpipewire-0.3-dev`（Ubuntu 22.04 默认包缺 libspa 0.9 所需符号）。这是**仅 CI 构建期**的第三方包源，不随产品分发、不接触密钥；风险是 PPA 内容未固定版本。退出条件：Linux job 迁到自带足够新 pipewire 的 runner 镜像（如 ubuntu-24.04）时移除该 PPA。
- 真实 PipeWire 采集只由 CI 的 Linux job 编译验证（编译机 build8 没有 libpipewire 头文件）；Wayland 真机运行时测试仍为 not_run。

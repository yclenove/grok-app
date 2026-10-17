# 0009 macOS Onboarding 的 wry/tao 依赖门禁

状态：接受（选项 1，总控裁定批准）
日期：2026-10-16

## 背景（事实 + 证据链接）
[04 §3](../../../docs/plans/computer-use-v2/04-route-a-standalone-mcp.md) 与 [06 §P4-02](../../../docs/plans/computer-use-v2/06-roadmap.md#p4-02-onboarding-窗口macos) 规定 macOS helper 的 Onboarding 窗口（以及后续审批窗）使用 wry WebView。实现把 `tao 0.37` / `wry 0.57` 放在 `gcu-cli` 的 `macos-webview` feature 后面，默认关闭，并且是 `target_os = "macos"` 依赖。wry 关闭 default features，不启用 `os-webview`，因此不拉 Linux WebKitGTK。发布构建只在 macOS CI 打开该 feature。

`deny.toml` 的 `[graph] all-features = true` 会把该 feature 展开，并检查所有目标的依赖图。`cargo deny --locked check`（cargo-deny 0.20.2）因此命中：

重复版本（`[bans] multiple-versions = "deny"`，未整体关闭）：

- `base64` 0.22.1（wry 0.57 的 Android 目标）与 0.23.1
- `sha2` 0.10.9（同上）与 0.11.1；连带 `digest` 0.10.7 / 0.11.3、`block-buffer` 0.10.4 / 0.12.1、`crypto-common` 0.1.7 / 0.2.2、`cpufeatures` 0.2.17 / 0.3.1
- `toml` 0.8.2 与 1.1.8+spec-1.1.0；`toml_edit` 0.19.15 与 0.20.2；`serde_spanned` 0.6.9 与 1.1.2
- `system-deps` 6.2.2 与 7.0.8；`cfg-expr` 0.15.8 与 0.20.10；`target-lexicon` 0.12.16 与 0.13.5；`heck` 0.4.1 与 0.5.0
- `proc-macro-crate` 1.3.1 与 2.0.2

较旧的一侧只出现在 wry 的非 macOS 目标（Android 的 base64/sha2）或 tao 0.37.1 的 Linux GTK 目标（`gtk 0.18` → `glib 0.18.5`、`system-deps 6`、`gtk3-macros` / `glib-macros`）。macOS 发布构建不编译这些路径。

advisory：

- `RUSTSEC-2024-0429`（unsound）：`glib` 0.18.5 的 `VariantStrIter`。依赖树只有 `tao` → `gtk 0.18`。修补版本是 glib >= 0.20.0。https://rustsec.org/advisories/RUSTSEC-2024-0429
- `RUSTSEC-2024-0370`（unmaintained）：`proc-macro-error` 1.0.4。只经 tao 的 Linux GTK 构建依赖 `glib-macros` 与 `gtk3-macros`。https://rustsec.org/advisories/RUSTSEC-2024-0370

wry 未启用 `os-webview`，没有第二条 WebKitGTK 边。08 §2 规则与 ADR 0007 的先例要求：放宽门禁前停下，由总控裁定。

## 决定
采用**选项 1**，沿用 ADR 0007：最小具名例外，不扩大全局白名单。

1. `[bans.skip-tree]` 只增加 `tao` 与 `wry`（与 `eframe` 同类：一个根 crate 的跨目标传递重复）。`multiple-versions` 保持 `"deny"`。不跳过其它 crate。
2. `[advisories] ignore` 只增加 `RUSTSEC-2024-0429` 与 `RUSTSEC-2024-0370`。`unmaintained` / `unsound` 保持 `"all"`。不忽略其它 ID。
3. 退出条件：tao/wry 升级到不再依赖 `glib < 0.20` 与 `proc-macro-error`，或 Onboarding/审批窗改用其它 WebView 方案。届时删除这两条 ignore 和这两个 `skip-tree`。
4. macOS CI 的 clippy 与 test 增加 `--features gcu-cli/macos-webview`。Windows 与 Linux 不加。测试不创建 WebView 窗口。

## 影响（对接口、测试、用户）
- 带 feature 展开的 `cargo deny --locked check` 与不展开该 feature 的检查都应通过。其它 deny 条目不变。
- 接口不变。Linux/Windows 构建不编译 wry。构建机编不了 objc2 的 cc 辅助程序；wry 的编译验证留给 macOS CI。
- 用户侧：只有 macOS helper 在缺权限时打开 Onboarding 窗口。

## 补充（总控审阅）

- 选项 1 已批准并落地：`deny.toml` 仅新增 `tao` 与 `wry` 的 `skip-tree`，以及上述两个 advisory ID。未放宽 duplicate 检查整体，未放宽许可或来源规则。
- macOS 才编译的 `ui/window.rs` 对照 tao 0.37.1 与 wry 0.57.0：`EventLoop::new`、`EventLoopExtRunReturn::run_return`、`ControlFlow::WaitUntil` / `Exit`、`WindowBuilder`、`HasWindowHandle`（tao 的 `rwh_06`）、`WebViewBuilder` 的 html/ipc/navigation/new-window/permission/download 与 `evaluate_script` 签名一致。`prepare_agent_onboarding` 的两个 `cfg` 互斥且覆盖全部目标。现有测试不调用 `window::run`；`gcu agent` 进程测试标了 `not(target_os = "macos")`。

# Computer Use 最终审查（本机检查点）

日期：2026-09-08。工作目录：`H:\aicoding\grok-app-computer-use`。
分支：`feat/computer-use-implementation`。HEAD 仍为设计基线 `30757366` 加未提交实现。
**Goal 未完成。** macOS / Linux 原生与四 target 完整安装矩阵为 `not_run`，准则 3 未闭环。

## 阻断项（Goal 不能标完成）

1. macOS arm64 / x64 原生桌面：适配器只报能力并拒绝 act/observe。无签名 Screen Recording / Accessibility 实机证据。
2. Ubuntu GNOME Wayland 与 X11 原生桌面：Linux 适配器按 `XDG_SESSION_TYPE` 写 notes，拒绝伪称 Wayland。无 AT-SPI / portal 实机证据。
3. 四 target 安装 / 更新 / 回退：Windows 测试目录 `/S /D=` 已安装；二次更新与 testdir 卸载 **not_run**（安装器拉起 testdir `grok-app.exe` 成了当前宿主）。macOS/Linux 包 **not_run**。
4. grok-4.6 桌面多轮：本机已 passed。`p8-desktop-model.mjs` oracle `clicks=1`，未泄漏路径。headless 必须带 `--always-approve`，否则 `PermissionCancelled`。
5. Cua Driver：`where cua-driver` 仍为空。私有 clone 编出 debug `cua-driver.exe` 后，自建窗绑定 + `stop` 取消已 passed。未点 ChatGPT/微信。

1–3 仍阻断 Goal。4 与 5 本机已 passed。

## 本机已验证

| 项 | 结果 | 证据 |
| --- | --- | --- |
| Broker + IPC 门禁 | passed | `cu_probe` 14 + 9 门（含 reconnect/resume 与 browser 隔离） |
| 已有 tab Host 隔离 | passed | `browser_two_session_isolation` / `borrow_return` / `cancel_keeps_user_tab`；实机扩展配对 not_run |
| Windows 自建窗 observe→act→verify | passed | `cu_probe`：image 524×381 dpi=96；element click clicks=1；CJK `你好世界`/`测试`；key sel=2；scroll top 0→13；drag=1；停止后 `Rejected` |
| grok-4.6 桌面多轮 | passed | `tools/computer-use-probe/.run/p8/p8-desktop-model.json`：clicks=1 leaked=false 26.7s |
| Cua 目标绑定与取消 | passed | `tools/computer-use-probe/.run/cua/bind-cancel.json`：绑定 `GrokCuFixture-cua-<pid>`，stop 后 daemon 不在 |
| Playwright 受管 worker | passed | `run-fixture.mjs` → shipped `server.mjs`：两次隔离 profile，`count:2`；owner 409 |
| `pnpm typecheck` / `test` / `lint` / `build:ui` / `deps:check` / `audit:prod` | passed | 7059 tests；eslint max-warnings 0；vite build 成功 |
| `python scripts/check-code-quality-gates.py --mode final` | passed | ≥1000 行文件 79/80 |
| `cargo fmt --all -- --check` | passed | 2026-09-08 本轮 |
| `cargo clippy --all-targets -- -D warnings` | passed | 修完 Computer Use lint 后 `Finished` |
| `cargo test --lib` | failed / 环境 | 无 `mt.exe` 时 `0xC0000139`（缺 comctl32 v6）；嵌入 `windows-test-manifest.xml` 后 harness 可启动。不以环境错误当断言绿 |
| Windows NSIS 测试目录安装 | passed（`/S /D=`）/ not_run（二次更新、testdir 卸载） | 安装到 `%TEMP%\grok-cu-nsis-test\install`；HKCU 已写回 D: 原安装；D: hash 未变。证据 `tools/computer-use-probe/.run/nsis/` |
| 未对 ChatGPT / 微信 SendInput | 遵守 | `cu_probe` 只点 `GrokCuFixture-p8-<pid>` |

## 正确性抽查

- 工具路径经 Broker；死目标不回退桌面；lease 互斥；`stop_requested`≠`stopped`；timeout→`unknown` 不重放。
- 坐标点击要求最近观察含 PNG；MCP observe 有图才带 image part。
- Feature flag 默认关；YOLO 不授权桌面。
- 15 locale + settingsCatalog；`App.tsx`/`AppWorkbench.tsx` 未增长。
- 预览轨迹不进 MCP status。

## 已知限制

- Win32 适配器：BitBlt 客户区；对子控件发 `SendMessage`（click/key/scroll/drag/CJK）；无 UIA 语义树；`postcondition_ok` 恒 false，验证靠 fixture 文件。
- NSIS 仍夹带 `cu_probe.exe` 1.2 MB。
- 轨迹不跨进程持久化。
- 已有 Chrome tab：Host 隔离已测；实机 Playwright 扩展配对 `not_run`。
- `cargo test --lib` 本机需 CI 同款 `mt.exe` 后链，否则不可启动。

## 建议 PR 顺序（未提交）

1. probes + P0 报告  
2. Broker / lease / flag  
3. Windows adapter + macOS/Linux stubs  
4. session MCP inject  
5. settings / i18n / Composer 面板  
6. Playwright worker  
7. agent loop + packaging docs  

本 Goal 不自动 commit / push / PR / tag / Release。

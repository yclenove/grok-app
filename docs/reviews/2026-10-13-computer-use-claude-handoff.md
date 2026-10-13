# Computer Use：Claude 全分支 review 交接

日期：2026-10-13。用户要求把全部开发代码推到fork，交给Claude审阅。

## 1. 仓库与固定基线

- Fork：`https://github.com/yclenove/grok-app.git`
- 分支：`feat/computer-use-implementation`
- 已推完整review提交：`5455eff94b744eff91dcc8280f59039d395205c8`
- 此后的第三方研究/本交接仅为文档，不改变该提交的产品源码。
- Fork main：`b58b97cd`；upstream main：`9c5eb3b5`。本轮已fetch，两者均为review提交祖先；不以旧main引用代替将来合并前fetch。
- 相对fork main为完整累积开发：约2341文件、40.3万新增行，含测试/工具/历史文档及已合入源仓库工作；不是只有最后两个commit。

建议独立checkout固定提交进行review，避免与当前工作区互相覆盖。此交接不要求Claude自动实现、推送、合并或发布。用户此前已授权合到fork main，适合合入的版本仍须完成相应检查；当前CI和完整交付尚未通过，不把本review快照标为可发行。

## 2. 已核验与开放项

启动输出修复 `1aef6bd7`：从启动起drain stderr、端口输入上限与安全诊断、端口后继续drain stdout、原Child/Job/readers保留，failed startup retry。8秒启动deadline不变。

最终本机Windows manifested App：2407 passed / 0 failed / 87 ignored；supervisor15项、源码worker/Chromium任务10项（1 ignored）、plugin auth单独5项通过。Rust1.99 strict Clippy、fmt与质量门禁通过。Jammy未改动startup模块4项通过，不等于完整Linux App或任意Unix后代树验收。

上一终态 [c821 CI](https://github.com/yclenove/grok-app/actions/runs/38394968937) failure：

1. Windows plugin auth测试 `private_pipe_reaches_legacy_node_parser_on_this_host` 10秒timeout；本机通过，远端根因未证，不能只放宽期限。
2. Linux X11第一段native fixture通过，随后GTK provider报D-Bus `a(sss)`序列化fatal，观察返回geometry changed。`Action.GetActions`是待查线索；尚未修复/独立复现，不弱化geometry门禁。
3. ARM64产品runtime prepare拒绝 `aarch64-unknown-linux-gnu`。ARM64 SDK/browser内部诊断绿不代表产品集成完成。

`5455eff9` 的 [run 38407961771](https://github.com/yclenove/grok-app/actions/runs/38407961771) 在本交接首次核查时仍in_progress；之前 `1aef6bd7` run已由workflow concurrency取消，不算通过。后续新增文档push可能产生新run，按准确head SHA查结果。

完整剩余范围见 [R1–R4](../plans/2026-10-13-computer-use-maintained-runtime-next.md)：维护浏览器/Node/PW版本及完整NOTICE/SBOM/源码/安全策略、CPU/header校验、五target资源、Mac原生两架构、各安装/升级回滚卸载、真人ACP/MCP及picker、四surface/IME/clipboard/UI UX、同最终候选12h与D00–D31。不要用局部测试拼成完整交付。

## 3. 优先审阅入口

| 范围 | 入口 |
| --- | --- |
| 协议/授权/账本/预算/生命周期 | `src-tauri/computer-use-core/src/` |
| Host/目标/IPC/运行时/原生适配 | `src-tauri/src/computer_use/` |
| MCP catalog与工具调用 | `tools/computer-use-mcp/` |
| 受管worker/扩展/已有标签页 | `tools/computer-use-browser/`、`tools/computer-use-extension/` |
| X11 / Wayland | `src-tauri/computer-use-x11/`、`src-tauri/computer-use-wayland/` |
| ACP工具注入/模型图像/任务授权 | `src-tauri/src/session_manager/computer_use/`、`src-tauri/src/acp_client/computer_use/` |
| 用户任务/预览/设置 | `src/components/computer-use/`、`src/lib/api/computerUse.ts`、15语言computer-use catalogs |
| prepare/打包/私有SDK/来源 | `scripts/prepare-computer-use-runtime.mjs`、runtime相关scripts与`.github/workflows/` |

重点：授权是否真在执行时生效，旧target/ref/generation是否误复用，未知效果是否重放，Pause/Stop/Quit是否等到原生owner结束，迟到回执是否串run，模型是否实际收到图片及正确坐标，UI是否与真实Host状态一致，文件picker/上传下载是否完整，包是否真正包含维护中的全部运行时。

不要只看测试数量。检查fake/doubles/ignored是否被当作原生验收、test是否覆盖生产路径、夹具是否能独立判定业务结果、cancel是否只结束future、源码通过是否被误报为当前用户包通过。

## 4. 检查与环境约束

先读根AGENTS、CONTRIBUTING、`docs/llm-wiki/maintain.md`及相关产品规则。root只用pnpm；App/AppWorkbench状态与行数禁止增长。按修改范围跑对应检查；提PR前完整本地CI需绿。

```text
pnpm typecheck
pnpm test
pnpm lint
pnpm build:ui
python scripts/check-code-quality-gates.py --mode final
cd src-tauri
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

本机Windows原始App test harness没有Common Controls v6 manifest会在断言前报0xc0000139。使用准确Cargo artifact的任务副本嵌入 `windows-test-manifest.xml`；记录原SHA、manifested SHA及原owner退出，不修改或清理整个Cargo target。

本地证据位于 `tools/computer-use-probe/.run/ci-worker-startup-20261013-01/`，不随Git提交；远端CI链接为可访问证据。用户当前preview仍App144/原隔离home，未替换成review源码。不能动真实登录/history/home、自动发送旧草稿或停止无关App/browser。

未跟踪 `.codex-window-check/`、`dist-portable/`、`fix.patch` 为已有本地项，未夹进review提交；不要删除或自动提交。构建前看空间，收尾仅清原owners退出后的准确废弃产物，保留current/dependencies/seeds/源码和失败证据。

## 5. 给Claude的review提示词

请审阅 yclenove/grok-app 的 feat/computer-use-implementation 完整分支，产品源码固定到5455eff94b744eff91dcc8280f59039d395205c8。先读本交接与项目规则，再看相对fork main的全量变化，区分合入上游的代码与CU新增代码。当前是review任务，请先出问题报告，不自动修改、push、合并或发布。

优先查真实用户流程、授权/身份、原生生命周期、unknown重放、模型图像/坐标、文件操作、运行时维护与跨平台安装、UI UX和未接线/临时实现。每项给优先级、文件/行、具体触发条件、用户影响、证据与修复建议；未经验证的假设单列，已知CI失败不要重复描述成新发现。测试绿、schema完整或大段计划不能替代生产路径和实机结果。

最后给出：阻止合main的问题、可后续处理的问题、尚缺的实证，以及适合分批合入的边界。保留Windows/macOS/Linux首版与四surface完整目标，不通过删功能、skip或放宽门禁制造完成。研究候选见[六项目比较](../research/2026-10-13-open-computer-use-projects.md)和[Cua Driver报告](../research/2026-10-13-cua-driver-and-codex-experience.md)，候选尚未接入，不把它们当现有实现。

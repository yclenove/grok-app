# 壁纸 X 搜索 Responses 预览：跨电脑开发交接

日期：2026-08-28

仓库：`yclenove/grok-app`

当前维护分支：`fix/wallpaper-responses-network-observability`

基线：`91bf92286988ad74708381ee2983a94bf65b625d`

状态：阶段 0–6、渐进式 `3 × 8` Responses 首屏和显式 `1 × 8` 加载更多均已实现并完成确定性测试与真实 Tauri 验收。当前默认仍为 CLI，Responses 只作为手动预览；`auto` 继续隐藏。维护分支尚未 push，也未创建或更新 PR。

## 1. 新电脑如何接手

用户审核通过后，先在当前电脑发布维护分支：

```powershell
git push -u origin fix/wallpaper-responses-network-observability
```

维护分支经用户审核并 push 后，已有仓库：

```powershell
git fetch origin
git switch fix/wallpaper-responses-network-observability
git branch --set-upstream-to=origin/fix/wallpaper-responses-network-observability
git pull --ff-only
pnpm install --frozen-lockfile
rustup component add rustfmt clippy
```

全新克隆：

```powershell
git clone https://github.com/yclenove/grok-app.git
cd grok-app
git fetch origin
git switch --track origin/fix/wallpaper-responses-network-observability
pnpm install --frozen-lockfile
rustup component add rustfmt clippy
```

环境要求：Node.js 22+、pnpm 9+、Rust stable、Windows MSVC Build Tools。桌面开发只运行 `pnpm dev`；不要裸跑 `tauri dev` 或 `pnpm tauri dev`，否则会与正式版抢单实例标识。

接手后先读：

1. [壁纸搜索维护规则](../llm-wiki/wallpaper-search.md)
2. [Responses 灰度开发计划](./2026-08-28-wallpaper-x-responses-preview-plan.md)
3. [最终 QA 与灰度报告](../qa/2026-08-28-wallpaper-x-stage6-final.md)
4. [阶段 4 质量报告](../qa/2026-08-28-wallpaper-x-stage4-quality.md)
5. [阶段 5 生命周期报告](../qa/2026-08-28-wallpaper-x-stage5-lifecycle.md)
6. [Responses 出图数量 / 时间校准](../qa/2026-08-29-wallpaper-responses-output-latency.md)
7. [渐进搜索与加载更多最终验收](../qa/2026-08-29-wallpaper-responses-progressive-load-more.md)

### 1.1 与源项目主线的关系

2026-08-29 最终交接前重新执行了 `git fetch --prune origin` 和 `git fetch --prune upstream`。在最终 QA 文档提交前，`git rev-list --left-right --count` 的快照为：

- `HEAD...origin/main`：本分支 24、`origin/main` 3；最终 QA 交接提交会使本分支侧增加到 25。
- `HEAD...upstream/main`：本分支 24、`upstream/main` 50；最终 QA 交接提交会使本分支侧增加到 25。
- 源项目 `d88dc135 fix(wallpaper): bypass WebView2 loopback fetch when applying local media (#939)` 与本分支的 `7af9caa6` 是同一前置修复的上游落地版本；使用 `--cherry-pick` 计数时双方各减少一个。
- `origin/main` 另外两个独有提交仍是 `cc51db56`（partial fork rewind recovery）和 `ca3203d5`（prompt fallback turn scope）。
- `upstream/main` 已推进到 `939202b0` / `v0.2.28`，包含大量与本功能无关的新变化。

本次没有 merge 或 rebase，因为同步 50 个上游提交会显著扩大当前已验收功能的范围并使本轮 QA 失效。新电脑若要同步最新 `upstream/main`，应单独安排整合提交，注意不要重复应用 `#939`，解决冲突后重跑全部门禁和真实壁纸搜索验收。不要对共享远端分支做未经确认的 force-push。

## 2. 已交付行为

- `cli` 是新安装、旧配置和未知配置的稳定默认值。
- `responses_preview` 只在用户主动选择后启用。
- `auto` 只保留协议值，不在 UI 展示，Host 当前仍按 CLI 执行。
- Responses 固定使用 Grok Build OAuth、官方兼容端点、`grok-4.6`、`low` 和只读 `x_search`。
- 前端不能传 endpoint、model、tool 或 credential；不使用单独计费的 xAI API Key。
- Responses 禁止凭证重定向；token 不跨 IPC，不进入日志、缓存、错误文案或诊断包。
- CLI 与 Responses 共用 normalize、媒体安全校验、去重和排序；CLI 保留自己的条件补搜，Responses 改为三路首屏和显式加载更多，不复用旧的自动补搜。
- Responses 首屏为三路各最多 3 次 `x_search`，单次用户初始搜索上限 9 次；用户主动加载更多再增加一路最多 3 次。429 和工具预算超限不再走 CLI，避免双重消耗。
- 其他可回退错误最多回退一次 CLI；取消不回退、不补搜、不写缓存。
- Host 缓存为 32 项、10 分钟、仅内存；本轮同 key 再搜实测约 1.37 秒稳定显示。
- Responses 每路完成校验后按真实完成顺序发送批次；前端已有真实阶段进度、显式取消、request ID、generation 和迟到结果隔离。
- 初始 Responses 完成后允许一次显式 `1 × 8` 加载更多；它从 Host 初始成功缓存读取排除 identity，追加去重，不自动重试、不回退 CLI，失败或空结果不清空原画廊。
- 旧 `proxyMode = "use"` 会按已保存 URL 安全迁移：合法 URL → `manual`，缺失或非法 URL → `system`；Rust 加载/保存/实际路由和 TypeScript hydrate 使用同一规则。
- `durationMs` 保留为本次总耗时；非缓存回退另带 `responsesDurationMs` / `cliDurationMs`，旧 Host 或不完整数据仍显示旧总耗时文案。
- 缓存命中会清空原请求的分段耗时，避免把历史 provider 成本冒充成本次缓存耗时；本次没有增加 Responses 自动重试或第二次 CLI 回退。

主要实现位置：

- `src-tauri/src/wallpaper_x_responses.rs`：Responses 固定契约、凭证边界、HTTP 与解析。
- `src-tauri/src/wallpaper_x_search.rs`：模式路由、回退、熔断、缓存和请求注册。
- `src-tauri/src/wallpaper_source.rs`：CLI 与共用媒体质量管线。
- `src/hooks/useWallpaperXSearch.ts`：前端请求生命周期、进度、取消和 generation 隔离。
- `src/components/WallpaperSourceModal.tsx`：搜索表单、状态与画廊组合。
- `src/lib/wallpaperXSearch.ts`：模式、错误和路由展示的纯逻辑。

## 3. 本分支提交链

| 提交 | 内容 |
|---|---|
| `7af9caa6` | 前置修复：通过有界 Tauri IPC 应用本地媒体，绕过 WebView2 loopback fetch 限制 |
| `a53ae5f6` | 阶段 0：CLI/Responses 脱敏基线与开发计划 |
| `ed1eeaa2` | 阶段 1：`cli / responses_preview / auto` 设置契约 |
| `959a9a6c` | 阶段 2：固定、只读的 Responses 客户端 |
| `d7770c30` | 阶段 3：手动预览 UI、路由、一次回退与熔断 |
| `81f24bfd` | 阶段 4：统一质量管线与条件补搜 |
| `ef7089f4` | 阶段 5：缓存、进度、取消和迟到结果保护 |
| `a401e4aa` | Windows CI PATH 清理保留系统工具 |
| `e1d3f41a` | 阶段 6 初版维护文档与 QA 报告 |
| `4c54fa1c` | Windows CRLF source guard 跨平台修正 |
| `f0405d66` | 仓库 Rust 格式收口 |
| `111cd7cc` | 严格 Clippy 收口，不使用宽泛 `allow` |
| `e514c689` | 设置 round-trip 测试隔离，避免并行环境污染和触碰真实用户设置 |
| `3f9e56bd` | 最终全绿 QA 结果 |
| `f933e3ae` | 最终报告补齐前置媒体修复范围 |
| `80337538` | 增加跨电脑开发交接文档 |
| `72b95349` | 修复旧 `proxyMode = "use"` 迁移，统一 Rust/TypeScript/IPC 的实际路由语义 |
| `b9f65d6b` | 增加 Responses/CLI 回退分段耗时、旧 Host 与缓存兼容、15 语言文案和传输错误分类测试；不增加自动重试 |
| `29bfc6e2` | 记录脱敏代理 A/B、最终验证和交接结论 |
| `0a40cb50` | 增加渐进搜索批次事件契约与确定性 DTO 测试 |
| `ba93bc30` | Responses 并发三路 `3 × 8`，按完成顺序发送已校验批次 |
| `82f8327c` | UI 渐进渲染批次，保留 request ID/generation 隔离与最终权威 meta |
| `bf2cb867` | 增加一次显式 `1 × 8` 加载更多，失败、空结果和取消均保留原画廊 |
| `f55277a2` | 增加并发/数量 benchmark 支持和脱敏输出延迟校准报告 |

本文档随最终 QA 交接提交更新；以 `git log -1 --oneline` 查看该提交的完整提交号。

## 4. 已验证基线

最终实测：

| 检查 | 结果 |
|---|---|
| `pnpm deps:check` | 通过，pnpm-only |
| `pnpm audit:prod` | 通过，0 个已知 production 漏洞 |
| `pnpm lint` | 通过 |
| `pnpm typecheck` | 通过 |
| `pnpm exec vitest run --maxWorkers=4` | 545 文件、6775 测试全部通过 |
| `pnpm build:ui` | 通过；只有既有动态导入和大 chunk 警告 |
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy --all-targets -- -D warnings` | 通过 |
| `cargo test --no-run` | 通过 |
| Windows manifest 全量 harness | 1574 passed、0 failed、1 ignored |
| 壁纸 Rust 定向测试 | 32 passed、0 failed |
| 最终代码质量闸门 | 除既有 `FILES_OVER_1K_BUDGET`（78 > 69）外全部通过；本轮没有新增源码跨过千行阈值 |

Windows 不要直接把裸 `cargo test` 的 `0xc0000139 STATUS_ENTRYPOINT_NOT_FOUND` 当成断言失败。按 `.github/workflows/ci.yml` 的 Windows `cargo test` 步骤：先 `cargo test --no-run`，再用 Windows SDK `mt.exe` 把 `src-tauri/windows-test-manifest.xml` 嵌入 `grok_app_lib-*.exe` 的 `RT_MANIFEST #1`，最后直接执行 harness。

本轮全量 Vitest 使用 4 workers，以避免该仓库已知的高并发源码扫描超时；6775/6775 全部通过，没有修改无关测试或放宽超时。

新改动提交前至少运行定向测试；准备交付时重跑：

```powershell
pnpm deps:check
pnpm audit:prod
pnpm lint
pnpm typecheck
pnpm test
pnpm build:ui
py -3 scripts/check-code-quality-gates.py --mode final --json
cd src-tauri
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --no-run
```

## 5. 当前产品结论

暂时不要开放 `auto`，也不要把默认值从 CLI 改成 Responses。

同时间窗的阶段 4 数据：Responses p50 63.4 秒，CLI p50 56.8 秒；Responses 7/8 成功，6/8 查询达到至少 6 张有效图，达标率 75%，低于计划的 80%。外部网络波动很大，阶段 0 又曾测得 Responses 比 CLI 快 37.8%，因此只能继续收集交叉样本，不能宣称稳定提速。

阶段 5 真机数据：CLI 首搜 54.8 秒并返回 14 张有效图；相同 key 缓存约 1.1 秒；取消约 0.8 秒恢复空闲，无回退和迟到污染。

2026-08-29 的单路等输出量校准先显示“一次 `x_search`、目标 12 张”在单路组合里平衡最好：8 主题中 6/8 达到至少 6 张有效图，成功样本 p50 69.0 秒、p95 78.6 秒、中位 12 张，成功样本每张有效图约 5.9 秒。目标 16 只把中位数提高到 13 张，p95 增至 89.0 秒且达标率降至 62.5%；两次搜索目标 8 张仅 2/4 达标。该结论随后被并发与每路数量实验继续收敛，不是最终产品契约。

随后按“总时间 + 最终唯一有效图”扩展并发批次。三路各目标 12 张时，8/8 主题达到至少 6 张，6/8 达到 18 张，4/8 达到 24 张；总耗时 p50/p95 为 88.0/93.2 秒，中位 21 张，22/24 个子批可用。四路虽然个别样本达到 35–41 张，但正式矩阵有 2/8 个主题四路同时连接超时、最终 0 张，整体中位降至 15 张。大批量并发拐点因此确定为三路。

三路每路数量继续筛选后，`3 × 8` 被选为并已实现的首屏甜点位：8 主题中 7/8 达到至少 6 张，6/8 达到 18 张，首批 6 张 p50/p95 为 58.9/66.3 秒，最终 p50/p95 为 78.8/91.8 秒，中位 19 张。相对 `3 × 12`，首批 p50 提前 12.6 秒、最终 p50 提前 9.2 秒，但没有主题达到 24 张，且有 1/8 主题三路同时连接超时。`3 × 6` 中位只有 12 张且首批没有更快；`3 × 10` 的成功样本 p50 已到 90.0 秒，因此均未采用。

紧接着的同 8 主题 CLI 邻近窗口中，CLI p50/p95 为 103.0/125.5 秒，中位 28 张且 8/8 成功。`3 × 8` 的成功样本最终 p50/p95 分别快 23.5%/26.9%，首批可用相对等待 CLI 完成的 p50 提前 44.1 秒，但中位少 9 张并有一次整组连接失败。这不是严格交错 A/B，因此只支持在手动预览中落地渐进首屏，仍不足以改变 CLI 默认或开放 `auto`。完整口径见 `docs/qa/2026-08-29-wallpaper-responses-output-latency.md`。

真实 Tauri 最终验收进一步确认：一个未缓存搜索 65.4 秒返回 20 张；另一个搜索在 62.1 秒先显示 5 张，最终 89.3 秒返回 13 张。显式加载更多的失败不会清空原画廊，取消约 0.254 秒恢复且无迟到污染，成功时从 20 张追加到 28 张；相同初始查询缓存约 1.37 秒恢复。完整脱敏证据见 `docs/qa/2026-08-29-wallpaper-responses-progressive-load-more.md`。

## 6. Responses 网络回退 84.9 秒：结论与修复

用户在 2026-08-28 看到：

```text
已回退 Grok Build CLI（Responses 网络不可用）· 84.9 秒
```

准确含义：Responses 请求先得到 `responses_network`，随后完整执行 CLI；84.9 秒是 Responses 尝试、CLI 搜索和图片校验的总耗时，并不是 Responses 单独等待 84.9 秒。

`b9f65d6b` 已补齐分段观测。新 Host 的非缓存回退会显示类似：

```text
Responses 18.4 秒后失败（Responses 网络不可用）；CLI 用时 66.5 秒完成；共 84.9 秒
```

旧 Host、不完整 meta 和缓存命中继续使用总耗时文案；没有为了提速增加不确定是否重复计费的自动请求重放。

当前机器的只读诊断证据：

- Responses 的 `Network` 只表示 reqwest 在收到 HTTP 响应前失败；OAuth、401/403、429、5xx、TLS 和 timeout 均有其他独立分类。
- 直连固定 Build Responses 主机在 10 秒连接超时。
- Windows 系统代理为 `127.0.0.1:10808`；`10809` 是手动配置的家宽出口。本地 Xray 同时监听两端口。
- 交付前不带凭证的最小 POST 均按预期返回 401：10808 约 6.14 秒，10809 约 3.06 秒。这证明两条链路当时都可达，不代表完整搜索耗时。
- v2rayN 日志在 08:48:52 记录过一次 connect timeout，并在 08:59:53 退出、09:00:03 重新启动。它能证明代理链路当时有波动，但不能单独证明哪一条日志就是该次壁纸请求。
- Windows curl 首次还出现过 Schannel 吊销服务器离线；加入 `--ssl-no-revoke` 后代理请求成功。产品 reqwest 使用 rustls，因此不要把这个 curl/Schannel 现象直接归因给产品 TLS。

### 6.1 旧代理模式已修复

旧 `proxyMode = "use"` 已确认来自网络探测的“实际使用代理”决策标签，不是正式设置枚举。`72b95349` 使用保存 URL 做无歧义迁移：合法 URL → `manual`；缺失/非法 URL → `system`。测试全部使用临时 `GROK_APP_HOME`，没有修改真实用户设置。

### 6.2 10808 / 10809 脱敏 A/B

2026-08-28 使用同一固定主题 `ocean-ultrawide`、同一 `grok-4.6 + low` / CLI `low` 契约并启用图片可达性探测。脚本未输出 token、搜索原词、媒体 URL、用户名、出口 IP或原始响应。

| 代理 | 渠道 | 搜索耗时 | 含探测总耗时 | 可达 / 候选 | 规范引用 |
|---|---|---:|---:|---:|---:|
| 10808（系统） | Responses | 62.9 秒 | 73.6 秒 | 3 / 8 | 8 / 8 |
| 10808（系统） | CLI | 85.6 秒 | 93.8 秒 | 26 / 26 | 26 / 26 |
| 10809（家宽） | Responses | 49.8 秒 | 62.4 秒 | 12 / 12 | 12 / 12 |
| 10809（家宽） | CLI | 91.1 秒 | 98.2 秒 | 26 / 26 | 26 / 26 |

本轮两条代理、两个渠道都成功，未触发回退。10809 的 Responses 比 10808 快约 15.2%，有效图从 3 张提高到 12 张；CLI 在 10809 反而慢约 4.7%。只有一个主题、每条路径各一次，不能据此自动选择代理、修改用户设置或宣布家宽稳定更快。代理进程重启和真实断网恢复会影响用户全机网络，本轮未主动执行；失败/恢复矩阵由本地确定性测试覆盖。

## 7. 本轮开发计划与完成状态

| 计划 | 状态 | 落地 |
|---|---|---|
| 批次事件协议与乱序/重复/迟到隔离 | 完成 | `0a40cb50` |
| Host 三路 `3 × 8`、共享 OAuth/client、逐路校验发送 | 完成 | `ba93bc30` |
| UI 渐进追加、15 locale、最终结果权威替换 | 完成 | `82f8327c` |
| 失败策略：有部分结果不回退、无自动单路重试、整组失败沿用一次 CLI 回退 | 完成 | `ba93bc30`、`82f8327c` |
| 真实 Host/Tauri 首批、最终、失败保留、缓存、取消和迟到验收 | 完成 | `docs/qa/2026-08-29-wallpaper-responses-progressive-load-more.md` |
| 显式 `1 × 8` 加载更多，追加去重且不回退 | 完成 | `bf2cb867` |
| 通用 Web 图片搜索 | 后续独立功能，不属于本轮 X 搜索计划 | 必须使用独立来源/标签和引用，禁止静默混入 X |

当前计划内的 Responses 壁纸搜索开发已经完成。后续若继续灰度，只应收集更多跨时间窗真实样本；在达到既有门槛前，不改变 `cli` 默认渠道，也不展示 `auto`。

## 8. 安全与范围红线

- 不打印或提交 token、Authorization、auth 文件内容、原始响应、媒体 URL或用户名。
- 不读取浏览器 Cookie，不实现 X 写操作，不自行刷新 OAuth，不引入 xAI API Key。
- 不把 Web 搜索结果混进“从 X 搜索”；Web 图片搜索应是后续独立来源。
- 不用真实用户数据目录做测试；使用临时 `GROK_APP_HOME` 并持有 `APP_HOME_ENV_LOCK`。
- 不向 `src/App.tsx` 或 `src/app/AppWorkbench.tsx` 增加产品状态块。
- 不 reset/clean 用户工作区；只暂存当前批次文件。
- 未经用户审核，不创建或更新 PR，不改 CHANGELOG，不打 tag，不构建发布安装包。

## 9. 当前工作区说明

本机 `src-tauri/Cargo.toml` 进入本任务前就有行尾状态，`git diff --ignore-space-at-eol` 为空；它从未纳入任何提交。维护分支当前仅在本机，等待用户审核后再 push；未创建或更新 PR。

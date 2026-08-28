# 壁纸 X 搜索 Responses 预览：跨电脑开发交接

日期：2026-08-28

仓库：`yclenove/grok-app`

分支：`feat/wallpaper-responses-search`

基线：`91bf92286988ad74708381ee2983a94bf65b625d`

状态：阶段 0–6 已实现并完成全量 QA；当前默认仍为 CLI，Responses 只作为手动预览。尚未创建或更新 PR。

## 1. 新电脑如何接手

已有仓库：

```powershell
git fetch origin
git switch feat/wallpaper-responses-search
git branch --set-upstream-to=origin/feat/wallpaper-responses-search
git pull --ff-only
pnpm install --frozen-lockfile
rustup component add rustfmt clippy
```

全新克隆：

```powershell
git clone https://github.com/yclenove/grok-app.git
cd grok-app
git fetch origin
git switch --track origin/feat/wallpaper-responses-search
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

### 1.1 与源项目主线的关系

2026-08-28 交接前已执行 `git fetch --prune origin` 和 `git fetch --prune upstream`：

- `origin/main` 仍在本分支基线，本分支相对它有 15 个功能提交。
- `upstream/main` 相对共同基线新增 3 个提交，本分支有 15 个独有提交。
- 源项目 `d88dc135 fix(wallpaper): bypass WebView2 loopback fetch when applying local media (#939)` 与本分支的 `7af9caa6` 是同一前置修复的上游落地版本。
- 另外两个上游提交是 `cc51db56`（partial fork rewind recovery）和 `ca3203d5`（prompt fallback turn scope）。

本次没有 merge 或 rebase，因为那会改变已经完整验证的提交链。新电脑若要先同步 `upstream/main`，应注意不要重复应用 `#939`；完成整合后必须重跑全量门禁。不要对共享远端分支做未经确认的 force-push。

## 2. 已交付行为

- `cli` 是新安装、旧配置和未知配置的稳定默认值。
- `responses_preview` 只在用户主动选择后启用。
- `auto` 只保留协议值，不在 UI 展示，Host 当前仍按 CLI 执行。
- Responses 固定使用 Grok Build OAuth、官方兼容端点、`grok-4.6`、`low` 和只读 `x_search`。
- 前端不能传 endpoint、model、tool 或 credential；不使用单独计费的 xAI API Key。
- Responses 禁止凭证重定向；token 不跨 IPC，不进入日志、缓存、错误文案或诊断包。
- CLI 与 Responses 共用 normalize、媒体安全校验、去重、排序和条件补搜。
- 单次搜索总 X 工具预算不超过 3；429 和工具预算超限不再走 CLI，避免双重消耗。
- 其他可回退错误最多回退一次 CLI；取消不回退、不补搜、不写缓存。
- Host 缓存为 32 项、10 分钟、仅内存；同 key 再搜实测约 1.1 秒稳定显示。
- 前端已有真实阶段进度、显式取消、request ID 和迟到结果隔离。

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

本文档本身是最后一笔交接提交；以 `git log -1 --oneline` 查看其完整提交号。

## 4. 已验证基线

最终实测：

| 检查 | 结果 |
|---|---|
| `pnpm deps:check` | 通过，pnpm-only |
| `pnpm audit:prod` | 通过，0 个已知 production 漏洞 |
| `pnpm lint` | 通过 |
| `pnpm typecheck` | 通过 |
| `pnpm test` | 545 文件、6761 测试全部通过 |
| `pnpm build:ui` | 通过；只有既有动态导入和大 chunk 警告 |
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy --all-targets -- -D warnings` | 通过 |
| `cargo test --no-run` | 通过 |
| Windows manifest 全量 harness | 1560 passed、0 failed、1 ignored |
| 壁纸 Rust 定向测试 | 22 passed、0 failed |

Windows 不要直接把裸 `cargo test` 的 `0xc0000139 STATUS_ENTRYPOINT_NOT_FOUND` 当成断言失败。按 `.github/workflows/ci.yml` 的 Windows `cargo test` 步骤：先 `cargo test --no-run`，再用 Windows SDK `mt.exe` 把 `src-tauri/windows-test-manifest.xml` 嵌入 `grok_app_lib-*.exe` 的 `RT_MANIFEST #1`，最后直接执行 harness。

新改动提交前至少运行定向测试；准备交付时重跑：

```powershell
pnpm deps:check
pnpm audit:prod
pnpm lint
pnpm typecheck
pnpm test
pnpm build:ui
cd src-tauri
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --no-run
```

## 5. 当前产品结论

暂时不要开放 `auto`，也不要把默认值从 CLI 改成 Responses。

同时间窗的阶段 4 数据：Responses p50 63.4 秒，CLI p50 56.8 秒；Responses 7/8 成功，6/8 查询达到至少 6 张有效图，达标率 75%，低于计划的 80%。外部网络波动很大，阶段 0 又曾测得 Responses 比 CLI 快 37.8%，因此只能继续收集交叉样本，不能宣称稳定提速。

阶段 5 真机数据：CLI 首搜 54.8 秒并返回 14 张有效图；相同 key 缓存约 1.1 秒；取消约 0.8 秒恢复空闲，无回退和迟到污染。

## 6. 最新待查问题：Responses 网络回退 84.9 秒

用户在 2026-08-28 看到：

```text
已回退 Grok Build CLI（Responses 网络不可用）· 84.9 秒
```

准确含义：Responses 请求先得到 `responses_network`，随后完整执行 CLI；84.9 秒是 Responses 尝试、CLI 搜索和图片校验的总耗时。当前 meta 没有分别记录两段耗时，所以不能从 UI 反推出 Responses 和 CLI 各花多少秒。

当前机器的只读诊断证据：

- Responses 的 `Network` 只表示 reqwest 在收到 HTTP 响应前失败；OAuth、401/403、429、5xx、TLS 和 timeout 均有其他独立分类。
- 直连固定 Build Responses 主机在 10 秒连接超时。
- 经 Windows 系统 HTTP 代理 `127.0.0.1:10808`，不带凭证的最小 POST 约 2.36 秒返回 401；这是预期的可达性证明，不是接口故障。
- 本地 Xray 同时监听 10808 和 10809；两条代理在诊断时都能约 2.2 秒到达端点。
- v2rayN 日志在 08:48:52 记录过一次 connect timeout，并在 08:59:53 退出、09:00:03 重新启动。它能证明代理链路当时有波动，但不能单独证明哪一条日志就是该次壁纸请求。
- Windows curl 首次还出现过 Schannel 吊销服务器离线；加入 `--ssl-no-revoke` 后代理请求成功。产品 reqwest 使用 rustls，因此不要把这个 curl/Schannel 现象直接归因给产品 TLS。

另有一处需要继续追查的配置兼容性：当前机器持久化的是旧值 `proxyMode = "use"`，并保存了 10809 的手动 URL；现有 Host 和前端都只承认 `system / manual / none`，未知的 `use` 会归一为 `system`，所以产品实际使用 WinINET 的 10808，保存的 10809 没有生效。

不要未经考证就把所有 `use` 映射成 `manual`。先查清旧值是如何被写入的，以及它代表“有效代理决策”还是“用户手动模式”；然后同时修正 Rust 反序列化/迁移、TypeScript hydrate/normalize 和测试，保证 UI 展示与 Host 实际路由一致。

## 7. 建议的下一开发批次

保持一次一提交，建议顺序：

1. 在新电脑复现代理模式读写，确定旧 `use` 的来源和预期语义，增加脱敏 fixture。
2. 实现一次性兼容迁移，覆盖旧值、有无 `proxyUrl`、系统代理、手动代理和直连；不要修改真实用户设置做测试。
3. 为 Responses 回退增加安全的分段耗时，例如 Responses 尝试耗时和 CLI 回退耗时；只传数字与稳定错误码，不传底层错误串、主机、请求头或 URL。
4. 用本地 mock 覆盖 DNS、连接拒绝、连接重置、TLS、timeout、5xx、401、429和取消，确认分类与回退矩阵不漂移。
5. 再做真实代理 A/B：系统 10808、手动 10809、代理进程重启、短暂断网和恢复。
6. 根据实测决定是否缩短 20 秒 connect timeout；不要在无法确认服务端是否已计费时盲目自动重放 Responses 请求。

可以考虑把 UI 总耗时改成类似“Responses 3.2 秒失败；CLI 81.7 秒完成”，这样下次不再把 84.9 秒误认为 Responses 单独等待时长。

## 8. 安全与范围红线

- 不打印或提交 token、Authorization、auth 文件内容、原始响应、媒体 URL或用户名。
- 不读取浏览器 Cookie，不实现 X 写操作，不自行刷新 OAuth，不引入 xAI API Key。
- 不把 Web 搜索结果混进“从 X 搜索”；Web 图片搜索应是后续独立来源。
- 不用真实用户数据目录做测试；使用临时 `GROK_APP_HOME` 并持有 `APP_HOME_ENV_LOCK`。
- 不向 `src/App.tsx` 或 `src/app/AppWorkbench.tsx` 增加产品状态块。
- 不 reset/clean 用户工作区；只暂存当前批次文件。
- 未经用户审核，不创建或更新 PR，不改 CHANGELOG，不打 tag，不构建发布安装包。

## 9. 当前工作区说明

本机 `src-tauri/Cargo.toml` 进入本任务前就有行尾状态，`git diff --ignore-space-at-eol` 为空；它从未纳入任何提交。远端分支只包含已提交内容，新电脑正常 checkout 应为干净工作区。

# 壁纸 X 搜索阶段 6：最终 QA 与灰度报告

日期：2026-08-28

分支：`feat/wallpaper-responses-search`

范围：阶段 0–5 的实现收口、完整自动检查、Windows harness、灰度结论和维护文档

发布状态：仅本地分支；未 push，未创建或更新 PR

## 最终结论

功能已经形成可审核的第一版，但不建议开放自动路由：

- `cli` 继续作为新安装、旧配置和未知配置的默认稳定渠道。
- `responses_preview` 只由用户在外观设置中主动选择；实际失败按安全矩阵至多回退一次 CLI。
- `auto` 只保留协议值，不在 UI 展示，Host 当前仍按 CLI 执行。
- Responses 使用规范 Grok Build OAuth 的只读 access token，不使用单独计费的 xAI API Key，不读取浏览器 Cookie，不刷新 token，不接受前端 endpoint/model/tool/credential。
- 首搜速度没有稳定达到自动灰度线；重复搜索缓存、进度和取消显著改善等待体验，但不能替代首搜质量/速度门槛。

因此本阶段建议是：**保留手动 Responses 预览，继续默认 CLI，不自动灰度**。是否保留该预览入口及后续是否开展更大样本，由用户审核决定。

## 阶段交付

| 阶段 | 本地提交 | 结果 |
|---|---|---|
| 0 基线与计划 | `a53ae5f6` | 8 个固定主题的脱敏 CLI/Responses 基线和六阶段门槛 |
| 1 设置契约 | `ed1eeaa2` | `cli / responses_preview / auto` 持久化；未知值回 CLI；无 UI/路由变化 |
| 2 Responses 客户端 | `959a9a6c` | 固定只读契约、OAuth 过期保护、错误分类、无凭证重定向 |
| 3 手动灰度路由 | `d7770c30` | 设置 UI、一次回退、429 防双花、凭证感知熔断、真实 route meta |
| 4 共用质量管线 | `81f24bfd` | normalize/validate/dedupe/rank、条件补搜、媒体安全与质量报告 |
| 5 生命周期 | `ef7089f4` | UUID、进度、取消、迟到结果保护、32 项/10 分钟内存缓存、真机 QA |
| 6 收口 | `a401e4aa`、`e1d3f41a`、`4c54fa1c`、`f0405d66`、`111cd7cc`、`e514c689` + 本报告提交 | 维护文档、跨平台 source guard、Rust 格式/Clippy、Windows harness 与测试隔离全部收口 |

没有向 `App.tsx` 增加状态；`AppWorkbench.tsx` 只有阶段 3 的既有参数接线，没有新增 `useState` 或大块产品逻辑。搜索状态位于 `useWallpaperXSearch`，Host 逻辑按 `wallpaper_x_search.rs` / `wallpaper_x_responses.rs` 分域。

## 自动检查

| 检查 | 结果 | 说明 |
|---|---|---|
| `pnpm deps:check` | 通过 | 根目录仍为 pnpm-only，没有产生 `package-lock.json` |
| `pnpm audit:prod` | 通过 | 无已知 production 漏洞；Node 只提示既有 `url.parse()` 弃用 |
| `pnpm lint` | 通过 | 全量 ESLint 通过 |
| `pnpm typecheck` | 通过 | TypeScript 类型检查通过 |
| `pnpm build:ui` | 通过 | 8077 modules，16.94 秒；只有既有动态导入与大 chunk 警告 |
| `pnpm test` | 通过 | 545 个文件、6761 条测试全部通过，119.75 秒 |
| `cargo test --no-run` | 通过 | Windows lib/main test harness 编译完成；仅 MSVC linker stdout 提示 |
| Windows manifest 全量 harness | 通过 | 1560 passed、0 failed、1 ignored，最终耗时 6.79 秒 |
| `cargo fmt --all -- --check` | 通过 | 全工作区 Rust 格式检查通过 |
| `cargo clippy --all-targets -- -D warnings` | 通过 | stable Clippy 全 targets、warnings-as-errors 通过 |

### Vitest 收口

首轮全量测试曾有两条 Windows 专属 source guard 失败：

- `src/lib/extensionsSurface.guard.test.ts`
- `src/lib/welcomeIntro.guard.test.ts`

两条 guard 直接用含 `\n` 的字面串/正则读取源码，在 CRLF 工作树上产生假阴性。`4c54fa1c` 只在测试读取时规范成 LF，没有改写被检查的业务文件；定向 4/4 通过，最终全量 6761/6761 通过。首轮还遇到本机 `canvas.node` 缺失；执行 `pnpm rebuild canvas` 后相关 16 条测试恢复，全量结果不再有环境阻塞。

### Rust 格式与 Clippy 收口

- `f0405d66` 只对 `models_aux.rs`、`process_util.rs`、`providers.rs` 应用仓库 rustfmt，没有逻辑改动。
- 安装官方 stable `clippy` 组件后，首轮严格检查发现 7 类告警。`111cd7cc` 用请求上下文结构收敛 wallpaper provider/cache 的长参数列表，并应用等价的标准写法；没有使用宽泛 `allow`，22 个 `wallpaper_x` 定向测试全部通过。
- 最终 `cargo fmt --all -- --check`、`cargo clippy --all-targets -- -D warnings` 和 `cargo test --no-run` 全部通过。

### Windows Rust harness

直接运行未嵌入 manifest 的 harness 会在断言前报 `0xc0000139 STATUS_ENTRYPOINT_NOT_FOUND`。本阶段按仓库 CI 方案执行：

1. `cargo test --no-run`；
2. 使用 Windows SDK `mt.exe` 把 `windows-test-manifest.xml` 写入 `grok_app_lib-*.exe` 的 `RT_MANIFEST #1`；
3. 直接运行完整 harness，而非只过滤 `wallpaper_x`。

首次照抄 PATH 清理逻辑时得到 1558 passed / 2 failed：一个 hook 测试明确提示 `findstr` 不存在，壁纸 CLI 取消测试返回 `search_failed`。原因是规则误删了同样含 `api-ms-win-*.dll` 的 `%SystemRoot%\System32`。在正常 PATH 下两条分别通过，取消测试为 0.83 秒；`a401e4aa` 随后让 CI 保留 Windows 目录、继续删除第三方冲突目录，`findstr.exe`、`ping.exe`、`taskkill.exe` 均解析到 System32。

最终全量复核的首轮又发现既有 `settings_default_factory_and_disk_roundtrip` 并行竞态：1559 passed / 1 failed / 1 ignored，round-trip 前后分别读到 `always_approve` 与 `ask`。该测试原本直接读写当前用户目录，且没有参与仓库已有的 `GROK_APP_HOME` 全局锁。`e514c689` 将它放入现有临时应用目录 helper，既避免测试碰真实用户设置，也避免与并行环境切换互相污染；复跑得到 1560 passed / 0 failed / 1 ignored。

产品 `build.rs` 没有加入第二份 manifest，避免 Tauri 产品链接的 `CVT1100 duplicate resource`。

## 真机体验与性能

阶段 5 Windows Tauri 真机结果：

| 场景 | 结果 |
|---|---|
| 手动取消 | 约 0.8 秒恢复空闲，无回退、无迟到结果 |
| 搜索中切 Imagine | 约 0.8 秒取消，Imagine 控件恢复 |
| 搜索中关闭再打开 | 被取消 generation 没有污染新弹窗 |
| 真实 CLI 首搜 | 54.8 秒，14 张通过校验的图片 |
| 同 key 再搜 | UI 显示缓存、CLI 原路由和 0.0 秒；自动化观察约 1.1 秒内稳定 |
| 深浅主题 | 进度、取消、路由、图库均可读，无裁切/透明/层级问题；结束后恢复深色 |
| 开发日志 | 无新增 panic、auth dump、媒体 URL 或生命周期异常 |

性能基线存在明显波动：

| 时间窗 | Responses p50 | CLI p50 | 判断 |
|---|---:|---:|---|
| 阶段 0 初始 8 主题 | 67.1 s | 107.9 s | Responses 快 37.8%，达到 30% 门槛 |
| 阶段 4 同窗交叉 8 主题 | 63.4 s | 56.8 s | Responses 反而更慢，未达到自动灰度门槛 |

阶段 4 同窗中 Responses 7/8 成功、6/8 达到至少 6 张有效图（75%，低于 80% 目标）；CLI 8/8 成功。所有返回项都通过 HTTPS allowlist、逐跳重定向、大小/MIME/签名和去重校验，且该轮规范 X 引用为 100%。一条 Responses 结果超过首轮两次工具预算，被正确拒绝且没有再调用 CLI。

## 安全与兼容复核

- 默认行为与旧配置保持 CLI，不会把其他用户静默切到实验接口。
- 自定义聊天 provider、API key、agent home 和模型不参与官方壁纸侧路。
- token 不实现 `Debug`/`Serialize`，不跨 IPC；响应、缓存、进度和日志不包含 token、header、auth 路径、原始正文、媒体 URL 或用户名。
- Responses 禁止重定向，正文限 2 MiB，媒体逐跳复核，探测限 64 KiB，完整下载限 200 MiB。
- 429、额度限制或 X 工具超预算不回退 CLI，防止一次用户操作发生双重订阅消耗。
- 取消不回退、不补搜、不写缓存；关闭/切 tab/替换搜索都会使旧 generation 失效。
- 缓存只在当前 Host 内存中保留 32 项、10 分钟；重启清空，Responses key 随凭证文件修订变化。
- 没有加入 xAI API Key、浏览器 Cookie、OAuth 刷新、X 写操作、Web 搜索混源或持久化搜索历史。

## 灰度指标与回滚条件

继续收集样本时建议保持用户主动 opt-in，并按相同时间窗交叉 CLI/Responses。至少连续 3 个独立时间窗、每窗覆盖现有 8 个中英文/横竖屏主题，全部满足以下条件后，才讨论展示 `auto`：

- Responses p50 比 CLI 至少快 30%，p95 没有无解释静默；超过 2 秒的工作有真实进度。
- Responses 成功率不低于 CLI 5 个百分点以上，且至少 80% 查询返回 6 张有效图。
- 坏图率 ≤5%、重复率 ≤5%、规范 X status 引用比例 ≥80%。
- 单次搜索总 X 调用不超过 3；429/预算错误 100% 不双路消耗；回退 mock 矩阵 100% 确定性通过。
- 用户取消到 UI 空闲及 Host 停止均不超过 2 秒，迟到结果污染为 0。
- 日志/诊断/缓存 secret 扫描为 0，且代理、无 OAuth、过期、401、429、5xx、断网场景均有可解释结果。

满足任一条件应停止自动灰度并回到 CLI：成功率相对 CLI 下降超过 5 个百分点、有效图达标率低于 80%、p50 提速低于 30%、429/预算出现双花、取消超过 2 秒或发生迟到污染、出现凭证/隐私泄漏、固定兼容接口发生不可解释契约变化。当前 `auto` 本来就按 CLI 处理，因此回滚不需要迁移用户数据；隐藏预览设置或把用户模式改回 `cli` 即可恢复稳定路径。

## 审核边界

本阶段没有 push、没有创建或更新 PR、没有改 CHANGELOG、没有打 tag 或构建发布安装包。`src-tauri/Cargo.toml` 的本地行尾状态属于进入本阶段前的用户工作区状态，不纳入任何提交。

# 壁纸 X 搜索：Responses API 灰度开发计划

**日期**：2026-08-28
**状态**：阶段 0 已完成，停在 go / no-go 验收点；产品实现尚未开始
**基线**：`main` @ `91bf92286988ad74708381ee2983a94bf65b625d`
**关联设计**：[`2026-07-28-wallpaper-x-imagine-design.md`](./2026-07-28-wallpaper-x-imagine-design.md)
**阶段 0 报告**：[`2026-08-28-wallpaper-x-search-baseline.md`](./2026-08-28-wallpaper-x-search-baseline.md)

## 1. 执行规则

本计划只处理 **设置 → 外观 → 背景图 → 从 X 搜索**。开发时严格按阶段推进：

1. 一次只实施一个阶段。
2. 每阶段完成后运行该阶段测试、报告结果并停止。
3. 上一阶段验收通过后，才进入下一阶段。
4. 不顺手重构无关模块，不在同一批次加入 Web 图片搜索或自动轮换壁纸。
5. 不向 `src/App.tsx` 或 `src/app/AppWorkbench.tsx` 增加新的产品状态或大块功能；新状态进入领域 hook / component / lib 模块。
6. 当前文档落盘不代表授权开始实现；后续由用户明确指定下一阶段。

## 2. 已确认的现状

当前 X 壁纸搜索链路是：

```text
WallpaperSourceModal
  → IPC wallpaper_x_search
  → headless `grok -p`
  → Grok Build 原生 X 工具
  → JSON 解析 / URL 规范化
  → 8 路分批可达性探测
  → 一次性返回画廊
```

关键事实：

- Host 入口是 `src-tauri/src/commands/misc_p1.rs::wallpaper_x_search`。
- 核心实现集中在 `src-tauri/src/wallpaper_source.rs`，文件已经较大；新 Responses 逻辑不继续堆进该文件。
- 当前 headless 调用强制 `--effort low`，没有显式指定模型，模型由 Grok Build 当前官方配置决定。
- 当前搜索预算是 `--max-turns 14`，总超时 150 秒。
- 结果返回前会过滤非允许媒体 URL，并对远程图片做 Range 可达性探测。
- 前端目前只有“搜索中 / 完成 / 错误”的整段等待，没有阶段进度、取消、结果缓存或真实路由说明。
- 当前命令没有 Responses API 直连路径。

## 3. 首版产品决定

### 3.1 灰度开关不是 bool

设置字段使用可演进枚举：

```text
wallpaper_x_search_mode = cli | responses_preview | auto
```

| 模式 | 首版状态 | 行为 |
|---|---|---|
| `cli` | 默认、公开 | 保持现有 Grok Build headless 稳定链路 |
| `responses_preview` | 用户主动选择 | 优先调用 Build OAuth Responses；满足规则时自动回退 CLI |
| `auto` | 先保留协议，不在 UI 展示 | 只有第 6 阶段达标后才考虑开放 |

旧配置、空值或未知值一律归一成 `cli`，保证升级后行为不变。

### 3.2 Responses 预览的凭证与边界

- 只读取 Grok Build 登录产生的规范 `~/.grok/auth.json` access token。
- 首版不使用单独计费的 xAI API Key，也不把 App 官方 API Key 混入该链路。
- 不建立第二套 OAuth，不读取浏览器 Cookie，不模拟 X 登录，不进行发帖、点赞或关注等写操作。
- 首版 **不自行刷新 refresh token**；token 已过期、临近过期或收到 401/403 时，交给 Grok Build CLI 稳定链路处理。
- token 只存在于 Host 内存，不进入前端 DTO、缓存、日志、诊断包或错误文案。
- Responses URL 固定为 Build 官方代理地址，不接受用户输入的 base URL；禁用跨主机凭证重定向。
- 这是依赖 Grok Build 内部兼容接口的实验能力，不承诺接口长期不变，也不能承诺“零封号风险”。默认关闭、只读、限频和自动回退是风险控制手段。

### 3.3 模型与推理强度

- 现有 CLI 路径：模型未显式指定，effort 固定为 `low`。
- Responses 预览候选默认：`grok-4.6` + `low`。
- 第 0 阶段用真实小样本验证速度和画质；若 `low` 的质量不达标，在开放设置前比较 `medium`，不把模型/effort 作为首版用户设置。
- 不继承当前自定义提供商模型，避免 DeepSeek/中转配置污染官方 X 搜索侧路。

### 3.4 X 与 Web 搜索不混源

首版“从 X 搜索”只返回 X 来源。即使 Responses 也支持 `web_search`，也不在 X 空结果时静默塞入网页图片；否则来源、引用和用户预期都会失真。Web 图片搜索作为后续独立来源，见第 10 节。

## 4. 目标架构

```text
AppSettings.wallpaper_x_search_mode
                │
                ▼
      Wallpaper X Search Router
        │                  │
        │                  └─ CLI provider（稳定）
        └─ Responses provider（实验、官方 OAuth）
                │
                ▼
      共用 normalize → validate → dedupe → rank
                │
                ▼
  WallpaperSearchResult { items, error, meta }
                │
                ▼
  UI：结果 + 引用诚实性 + 路由/回退/耗时/缓存说明
```

Host 建议按领域拆分：

- `wallpaper_source.rs`：保留共用 DTO、媒体 URL、下载、图库、Imagine 和已有 CLI 基础能力。
- `wallpaper_x_responses.rs`：固定端点客户端、请求体、响应解析、错误分类。
- `wallpaper_x_search.rs`：模式解析、路由、回退、预算、缓存、取消与进度。

前端建议拆分：

- `src/lib/wallpaperXSearch.ts`：模式、meta、错误与展示文案的纯逻辑。
- `src/hooks/useWallpaperXSearch.ts`：请求生命周期、过期响应丢弃、进度与取消。
- `WallpaperSourceModal.tsx`：只保留表单、画廊和交互组合，不继续堆协议逻辑。

## 5. 返回契约

在现有 `WallpaperSearchResult` 上增加向后兼容的 `meta`：

```text
meta.requestedMode   cli | responses_preview | auto
meta.routeUsed       cli | responses
meta.fallbackReason  可选的稳定错误码
meta.durationMs      Host 总耗时
meta.cacheHit        bool
meta.searchCalls     实际 X 搜索调用数（能可靠获得时才填）
meta.candidateCount  解析出的候选数
meta.validCount      校验后返回数
meta.model           安全的模型 id，可选
meta.effort          low | medium，可选
```

规则：

- `meta` 可以缺失，旧 Host / UI 仍可工作。
- 不把 access token、完整请求头、原始响应正文或 auth 路径放进 `meta`。
- UI 只展示真实路由，例如“Responses 预览”“已回退 CLI”“来自缓存”；不能根据设置值猜测实际路由。

## 6. 回退与熔断规则

一次用户搜索最多发生一次路由回退，禁止递归重试：

| Responses 结果 | 行为 |
|---|---|
| 无 OAuth access token / 已过期 / 临近过期 | 不发送 HTTP，直接走 CLI，记录 `oauth_unavailable` / `oauth_expired` |
| 401 / 403 | 不用同一 token 重试，走 CLI |
| 400 且表明请求契约不兼容 | 打开短期熔断，走 CLI |
| 连接失败 / TLS / 超时 / 5xx / 无法解析 | 走 CLI，并计入熔断失败 |
| 429 或明确额度耗尽 | 不再走 CLI，避免同一订阅重复消耗；诚实返回限流/额度错误 |
| 已得到至少 1 张有效图片 | 不整轮改走 CLI；第 4 阶段可条件补搜 |
| 用户取消 | 立即结束，不回退、不重试 |

熔断建议：10 分钟内连续 3 次协议/网络失败后暂停直连 10 分钟；成功、token 文件变更或冷启动后重新探测。401/403 走凭证状态，不与 5xx 混成同一故障。

## 7. 分阶段实施

### 阶段 0：冻结基线与协议样本

**目的**：先量化现在有多慢、结果有多好，再决定直连是否值得开放。

**范围**：

- 选 8 个固定主题，覆盖中英文、热门/最新、摄影/AI 艺术、宽屏/竖屏。
- 记录 CLI 冷启动、暖启动的总耗时、候选数、可达数、重复数、规范 X 引用数。
- 用已确认可用的 Build OAuth 做最小 Responses 小样本，记录相同指标。
- 保存的 fixture 必须完全脱敏；不提交 token、完整 auth、真实个人信息或未经清理的响应头。
- 确认 Responses 请求的最小兼容字段：固定 endpoint、Bearer、model、input、`tools: [{type: "x_search"}]`、reasoning、JSON schema、`store: false`。

**明确不做**：不改设置、不接入 UI、不改变 `wallpaper_x_search` 行为。

**可能涉及**：

- `src-tauri/src/wallpaper_source.rs` 中现有测试
- 新增脱敏 fixture / 纯解析测试文件
- 本计划的“基线结果”附录

**测试**：

```bash
pnpm test -- src/lib/wallpaperSource.test.ts src/lib/xEvidenceCitation.test.ts
cd src-tauri && cargo test wallpaper_source
```

**验收**：

- 有可复现的 CLI 与 Responses p50 / p95、成功率和质量对比。
- 确认候选 `grok-4.6 + low` 是否够用；不够则给出 `medium` 对照。
- 直接请求和日志检查均无 secret 泄漏。

**停止点**：提交基线报告，先确认 go / no-go；未确认前不进入阶段 1。

---

### 阶段 1：只建立设置数据契约

**目的**：先让模式字段可安全持久化和迁移，但不暴露一个尚不能工作的 UI 选项。

**范围**：

- Host `AppSettings` 增加 `wallpaper_x_search_mode`，默认 `cli`。
- 增加纯函数归一化：只接受 `cli | responses_preview | auto`，其他值回退 `cli`。
- TypeScript `AppSettings` 增加对应 camelCase 字段。
- 此阶段 `auto` 和 `responses_preview` 都不改变生产搜索路由。

**明确不做**：不显示设置控件、不发 Responses 请求、不修改画廊。

**涉及文件**：

- `src-tauri/src/store.rs`
- `src/lib/api/settings.ts`
- 必要时 `src/lib/appSettingsHydrate.ts`
- 对应 Rust / TypeScript 单测

**测试**：

```bash
pnpm typecheck
pnpm test -- src/lib/appSettingsHydrate.test.ts
cd src-tauri && cargo test wallpaper_x_search_mode
```

**验收**：

- 新安装默认 `cli`。
- 老设置无字段时仍为 `cli`。
- 手工写入未知值不会导致崩溃或切到实验路由。
- 保存其他设置不会丢失该字段。

**停止点**：只交付设置契约 diff 和测试结果；不开始 HTTP 客户端。

---

### 阶段 2：实现只读 Responses 客户端，仍不接 UI

**目的**：完成一个可由 mock 完整测试的最小直连垂直切片。

**范围**：

- 新建 `wallpaper_x_responses.rs`，固定 Build Responses endpoint。
- 从账户模块读取规范 CLI OAuth access token 快照，并检查 `expires_at`（建议 60 秒安全余量）。
- 请求使用第 0 阶段冻结的 model / effort / schema；默认 `store: false`。
- 使用现有代理设置和 reqwest 基础设施；Responses 请求不允许携带 Authorization 跨主机重定向。
- 解析最终 output text / structured JSON，并转换成现有 `WallpaperGalleryItem`。
- 对 400、401/403、429、5xx、超时、空响应、坏 JSON 做稳定错误分类。
- 单元测试全部走本地 mock HTTP，不读取开发者真实 auth。

**明确不做**：不刷新 refresh token、不暴露新 Tauri command、不修改默认 CLI 路由。

**涉及文件**：

- 新增 `src-tauri/src/wallpaper_x_responses.rs`
- `src-tauri/src/account.rs`（仅增加安全的 Host 内凭证读取接口）
- `src-tauri/src/lib.rs`（注册内部模块，不一定注册 IPC）
- `src-tauri/Cargo.toml` 仅在现有依赖确实不够时调整；优先零新增依赖

**测试**：

```bash
cd src-tauri && cargo fmt --check
cd src-tauri && cargo test wallpaper_x_responses
```

另做一次人工 live smoke，但只报告状态码分类、耗时和结果计数，绝不打印 token / 请求头 / 原始 auth。

**验收**：

- mock 覆盖成功、401、429、5xx、超时、坏 JSON。
- token 缺失/过期时网络请求计数为 0。
- 任何 DTO 和日志都不含 token。
- live smoke 能获得真实 X 工具结果，或诚实得出当前接口不适合继续。

**停止点**：提交客户端与协议测试结果；未通过不开放设置。

---

### 阶段 3：接入灰度设置、路由与回退

**目的**：形成首个可供用户主动试用的版本，默认用户完全不受影响。

**范围**：

- 新建 `wallpaper_x_search.rs` 路由器，接入 Responses provider 与现有 CLI provider。
- 实现第 6 节的单次回退和熔断规则。
- `WallpaperSearchResult` 增加可选 `meta`。
- 设置 → 外观 → 主题 → 背景图中增加自定义 `Select`：
  - Grok Build CLI（稳定，默认）
  - Responses API（预览）
- `auto` 仍不展示。
- 新设置使用稳定 anchor `settings-anchor-wallpaper-x-search-mode`，登记到 `SETTINGS_ENTRIES`。
- UI 显示实际使用的路由、回退原因和耗时；不能只显示用户选择值。
- 所有新增文案先加 English authority，再同步其余 14 个完整 catalog。
- 设置控件复用项目 `Select`；不使用原生 `<select>`、系统弹窗或透明 menu。

**明确不做**：不做缓存、取消、条件补搜，不开放 `auto`。

**涉及文件**：

- 新增 `src-tauri/src/wallpaper_x_search.rs`
- `src-tauri/src/wallpaper_source.rs`
- `src-tauri/src/commands/misc_p1.rs`
- `src-tauri/src/lib.rs`
- `src/lib/wallpaperSource.ts`
- 新增 `src/lib/wallpaperXSearch.ts` 及测试
- `src/lib/api/wallpaper.ts`
- `src/components/WallpaperSourceModal.tsx`
- `src/components/settings/AppearanceSection.tsx`
- `src/components/settings/types.ts`
- `src/components/SettingsPage.tsx`
- `src/hooks/useAppSettingsPrefs.ts`
- `src/app/WorkbenchSettingsStage.tsx`（只做参数接线，不新增状态块）
- `src/lib/settingsCatalog/entries/appearance.ts`
- `src/i18n/messages/*/settings.ts`（15 个 locale）
- `src/styles/settings.part4.css`（仅在现有样式不足时）

**测试**：

```bash
pnpm test -- src/lib/settingsCatalog.test.ts src/i18n/messages.test.ts src/lib/wallpaperSource.test.ts src/lib/wallpaperXSearch.test.ts
pnpm typecheck
cd src-tauri && cargo test wallpaper_x_search
```

**验收矩阵**：

- 默认 `cli` 与现有行为一致。
- 有效 OAuth + preview：实际 route 为 `responses`。
- 无 token / 过期 / 401 / 5xx：按规则实际 route 为 `cli`，UI 说明回退。
- 429：不双路重试。
- active custom provider 时不读取/修改其 key、route 或 agent-home；壁纸侧路仍只使用官方凭证。
- 重启 App 后设置仍在，未知值安全回退。

**停止点**：首个灰度版本完成后立即停下，先进行真实试用；不顺手做质量和缓存阶段。

---

### 阶段 4：统一质量管线与条件补搜

**目的**：在直连提速后，确保结果不是“快但差”。

**范围**：

- CLI 与 Responses 共用同一套 normalize / validate / dedupe / rank。
- 第一轮最多允许 2 次 hosted `x_search`；仅当校验后少于 6 张时补 1 次，总预算最多 3 次。
- 补搜必须换查询角度并携带已见媒体/帖子 ID，避免重复。
- 结果上限建议 12～18 张；先返回高质量、可达、规范 X 引用的静图。
- 去重至少覆盖规范化媒体 URL、X status id + media index、重复 CDN 变体。
- 排序综合：可达性、真实图片 MIME、尺寸/宽高比（能可靠获得时）、规范引用、互动量；未知值不伪造为 0。
- 加固远程媒体：HTTPS、逐跳 host 复核、流式大小上限、内容类型和实际文件签名检查。
- 少量有效结果优先诚实返回，不因未满 6 张而报空。

**明确不做**：不使用视觉模型逐张审美打分，不抓取 X 登录页面，不生成不存在的帖子链接。

**涉及文件**：

- `src-tauri/src/wallpaper_x_search.rs`
- `src-tauri/src/wallpaper_x_responses.rs`
- `src-tauri/src/wallpaper_source.rs`
- `src/lib/wallpaperSource.ts`
- `src/lib/xEvidenceCitation.ts`（仅当引用契约确需扩展）
- 对应 Rust / TS fixture 测试

**测试**：

```bash
pnpm test -- src/lib/wallpaperSource.test.ts src/lib/xEvidenceCitation.test.ts
cd src-tauri && cargo test wallpaper_x
```

**验收**：

- 任何单次搜索的 hosted X 调用数不超过 3。
- 8 个基准主题中至少 80% 返回 6 张以上有效图，或明确显示真实少结果。
- 坏图率目标 ≤5%，重复率目标 ≤5%。
- 规范 X status 引用比例不低于 CLI 基线，目标 ≥80%。
- Responses 成功率不应比 CLI 基线低超过 5 个百分点。

**停止点**：输出质量对比表；若质量门槛未过，保留 preview，不进入自动模式。

---

### 阶段 5：缓存、阶段进度与取消

**目的**：把“看起来卡住”的体验改成可理解、可停止、重复搜索更快。

**范围**：

- Host 内存 LRU/TTL 缓存，建议 32 个 key、10 分钟；key 包含规范 query、sort、route contract version。
- 缓存只存安全结果 DTO，不存 token、请求头或原始响应。
- 每次请求生成 `requestId`，阶段事件建议：
  - `preparing`
  - `searching_x`
  - `validating`
  - `supplementing`
  - `falling_back`
  - `done`
- 新增取消命令/取消 token：关闭弹窗、切 tab 或点击“取消搜索”时终止 reqwest；CLI 路径要可靠 kill 子进程。
- 前端抽出 `useWallpaperXSearch`，使用 generation/requestId 丢弃迟到结果。
- 命中缓存仍保留原 route meta，并额外标 `cacheHit=true`。

**明确不做**：首版不落盘查询缓存，不跨设备同步，不做后台预搜。

**涉及文件**：

- `src-tauri/src/wallpaper_x_search.rs`
- `src-tauri/src/commands/misc_p1.rs`
- `src-tauri/src/lib.rs`
- `src/lib/api/wallpaper.ts`
- 新增 `src/hooks/useWallpaperXSearch.ts`
- `src/components/WallpaperSourceModal.tsx`
- `src/i18n/messages/*/settings.ts`

**测试**：

```bash
pnpm test -- src/lib/wallpaperXSearch.test.ts src/components/WallpaperSourceModal.test.tsx
pnpm typecheck
cd src-tauri && cargo test wallpaper_x_search
```

**验收**：

- 同 key 二次搜索命中缓存，且不产生新的网络/CLI 调用。
- 过 TTL 或模式变化后不误命中。
- 取消后 2 秒内停止，且不触发 CLI 回退。
- 关闭再打开弹窗不会被上一轮迟到响应污染。
- UI 在浅色/深色主题下进度和路由说明可读，无透明面板、裁切、点击穿透。

**停止点**：完成桌面真机 UX 演示后停止，不直接开放 `auto`。

---

### 阶段 6：完整 QA、灰度结论与文档收口

**目的**：决定是否仍保持手动 preview，还是具备开放 `auto` 的条件。

**完整自动检查**：

```bash
pnpm deps:check
pnpm audit:prod
pnpm lint
pnpm typecheck
pnpm test
pnpm build:ui
cd src-tauri && cargo fmt --check
cd src-tauri && cargo test
```

**真机矩阵**：

- Windows Tauri：有效 OAuth、无 OAuth、过期、401、429、5xx、断网、系统/手动代理。
- CLI 与 Responses 两模式；Top 与 Latest；中英文关键词。
- 当前主路由为 official / custom 两种情况。
- 搜索中取消、关闭弹窗、切 tab、重复搜索、缓存过期。
- 结果预览、打开原帖、下载原图、应用壁纸、重启后壁纸保持。
- 浅色/深色、窗口窄宽、菜单层级、键盘焦点和屏幕阅读器 label。

**性能通过线**：

- Responses 直连 p50 总耗时至少比 CLI 基线快 30%；若外部网络波动大，使用相同时间窗口交叉测试。
- p95 不出现无解释的长时间静默；所有超过 2 秒的阶段都有真实状态。
- 质量达到第 4 阶段门槛，且回退 mock 矩阵 100% 确定性通过。

**文档收口**：

- 新增或更新 `docs/llm-wiki/wallpaper-search.md`，记录模式、凭证、回退和安全边界。
- 在 `AGENTS.md` 的相关产品规则中链接该文档。
- 必要时给旧壁纸设计文档加“由本计划扩展”的说明，不覆写历史结论。
- 只有用户明确要求发布时才改 CHANGELOG、打 tag 或构建安装包。

**灰度结论**：

- 未达标：继续默认 `cli`，保留手动 `responses_preview`，列出具体未达项。
- 达标：可以在后续单独阶段开放 `auto`，但仍不直接把默认值从 `cli` 改为 `auto`；默认切换需要另一次产品确认。

**停止点**：提交 QA 报告和是否开放 `auto` 的建议，由用户决定下一步。

## 8. 全局验收红线

- 所有 UI 文案进入 15 个完整 locale，`en` 是 key authority。
- 新设置登记 `SETTINGS_ENTRIES`，有稳定 anchor，可搜索和深链跳转。
- 不使用 `window.confirm` / `prompt` / `alert`，不新增原生 `<select>`。
- 不向 `App.tsx` / `AppWorkbench.tsx` 增加新产品状态块。
- 不打印或传回 token，不提交 `auth.json`、secrets、真实账户响应。
- 不改变当前自定义 provider route，不把官方 OAuth 写进 custom main agent-home。
- 不把 Web 结果伪装为 X 结果，不伪造 status URL、点赞数、尺寸或搜索次数。
- 工作区有用户无关改动时不 reset / clean；每阶段只提交本阶段范围。

## 9. 预期提速的表达方式

在第 0 阶段完成前不写死“能快几倍”。理论上直连会省掉 CLI 进程启动、agent 初始化和 headless 输出封装，但真实总耗时仍受 X hosted search 与图片探测影响。

对外只使用测得的数据：

- `CLI p50 / p95`
- `Responses p50 / p95`
- 端到端有效图数量
- 缓存命中耗时
- 回退比例与原因

首版性能门槛定为 p50 至少快 30%，而不是先承诺一个无法验证的绝对秒数。

## 10. 后续扩展：网络搜索与其他来源

X 灰度稳定后，可以复用 provider/router 架构增加独立来源，但不并入本轮：

1. **Web 图片发现**：单独的“网页”来源，调用 hosted `web_search`，结果必须显示网页来源和原始页面链接。
2. **授权图库适配器**：Unsplash / Pexels 等有明确 API 和许可元数据的来源，单独配置 API key 与署名规则。
3. **站点安全抓取**：统一走 Host `safe_https_get`，逐跳 HTTPS、阻断私网/回环/元数据地址、限制大小和 MIME。
4. **版权与热链诚实性**：保留作者、来源页、许可字段；不能只保存一条裸图片 URL。
5. **不做**：抓 Google/Bing 图片结果页、绕登录爬 X、自动发布或自动轮换未经用户确认的壁纸。

这些来源应实现同一 `SearchProvider` 契约，但 UI 必须明确分源，不能在“从 X 搜索”里静默混合。

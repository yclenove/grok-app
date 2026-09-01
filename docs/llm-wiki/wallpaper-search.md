# 壁纸搜索

维护“设置 → 外观 → 主题 → 背景图 → 从 X 搜索”时必读。该能力只做只读搜索和壁纸媒体处理；不要把聊天主路由、自定义提供商或通用 Web 搜索混入 X 来源。

## 产品契约

| 设置值 | UI | 实际行为 |
|---|---|---|
| `cli` | 稳定渠道，默认 | 启动本机 Grok Build CLI，使用其官方 X 工具 |
| `responses_preview` | 用户主动选择的预览渠道 | 优先请求固定的 Build Responses 兼容端点；按规则至多回退一次 CLI |
| `auto` | 协议保留，不在 UI 展示 | 当前按 `cli` 处理；没有 QA 决议前不得自动灰度或改成默认 |

缺失、空白、损坏或未知设置值必须归一为 `cli`。UI 必须展示 `meta.routeUsed`、回退原因、耗时和缓存命中等真实结果，不得根据用户选择猜测实际路由。`durationMs` 始终表示本次请求总耗时；非缓存回退可额外提供 `responsesDurationMs` 和 `cliDurationMs`，让 UI 分开说明两段时间。旧 Host 或不完整分段数据继续显示总耗时，不能猜测缺失值。

入口和职责：

```text
AppearanceSection（模式设置）
  → WallpaperSourceModal / useWallpaperXSearch（交互生命周期）
  → wallpaper_x_search IPC（requestId）
  → wallpaper_x_search.rs（缓存、路由、熔断、回退、进度、取消）
      ├─ wallpaper_source.rs（CLI + 共用质量管线）
      └─ wallpaper_x_responses.rs（固定 Responses 客户端）
```

不要向 `App.tsx` 或 `AppWorkbench.tsx` 增加该功能的状态。设置项必须保留稳定 anchor 并登记进 `settingsCatalog`；文案以 English catalog 为权威，同步全部 15 个 locale。

## 两条渠道

### Grok Build CLI

- 默认稳定路径，运行本机 `grok -p`，不显式指定模型；模型由用户当前官方 Grok Build 配置决定。
- 固定 `--effort low`、`--max-turns 14`、结构化 JSON 输出，单轮硬超时 150 秒。
- 第一轮提示最多使用 2 次 X 搜索；有效图片少于 6 张时可再启动一次补搜，补搜预算 1 次。
- Windows 取消使用进程树终止，Unix 使用独立 session/process group；取消不得继续补搜或触发其他渠道。

### Responses 手动预览

- 固定 endpoint `https://cli-chat-proxy.grok.com/v1/responses`、模型 `grok-4.6`、effort `low`、只读 `x_search` 工具和 `store: false`；前端不能传 endpoint、模型、工具或凭证。
- 每次 HTTP 请求超时 90 秒，响应正文最多读取 2 MiB；禁止自动跟随重定向，Bearer 不得跨主机重放。
- 一次搜索并发启动 3 路互补请求：直接主题、视觉变化、发现/双语扩展。每路目标 8 张、最多 3 次 `x_search`，因此单次用户搜索的硬上限是 3 个 HTTP 请求、9 次工具调用和 24 张最终结果；客户端必须同时用提示词限制并核验每路响应中的真实工具调用数，不能只相信 `max_tool_calls`。
- 三路只读取一次 OAuth，并复用同一个 HTTP client；按真实完成顺序处理，每路通过共用验图管线后立即发送一批。跨路按媒体和 status + media index 去重，一路失败不得抹掉其他路的有效结果。
- 当前不做自动单路重试，也不再执行旧的 2+1 补搜。三路全部失败时才进入既有错误优先级和至多一次 CLI 回退；任一路已有有效图即返回真实部分结果，不得为了补满 24 张触发回退。
- 初始搜索成功结束后，Responses 画廊允许用户主动点击一次“加载更多”。该操作仅启动 1 路目标 8 张、最多 3 次 `x_search` 的请求，因此一次 UI 搜索最多约展示 32 张；它不自动重试、不回退 CLI，也不会在初始三路仍活跃时启动。
- 加载更多从 Host 的初始成功缓存读取既有媒体与 status identity 作为排除上下文，并在验图后再次去重；这些 Host-only 字段不由前端回传，也不跨 IPC。空结果只表示“没有更多”，不会计入熔断。
- `responses_preview` 是依赖 Grok Build 兼容接口的实验能力，不是独立 xAI API Key 路径，也不承诺接口长期稳定或零账号风险。

## 凭证与安全边界

- 只读取规范 Grok Build 登录目录中的 OAuth access token；忽略聊天主路由的自定义 provider、`GROK_HOME` 和独立 agent home。
- token 距离过期不足 60 秒时不发 Responses 请求；首版不自行刷新 refresh token。
- token 只能存在于 Host 内存，不得进入 IPC DTO、前端状态、缓存 key 明文、日志、诊断包、错误文案或测试 fixture。
- 不读取浏览器 Cookie，不模拟 X 登录，不执行发帖、点赞、关注等写操作。
- 日志和报告不得保留 auth 内容、Authorization、原始 Responses/CLI 正文、搜索原词、媒体 URL、用户名或个人账号信息。只记录主题 id、稳定错误码、耗时和计数。

## 回退与熔断

Responses 每次用户搜索最多回退一次，禁止递归或双重消耗：

| Responses 结果 | 路由行为 |
|---|---|
| token 缺失、过期、401/403 | 回退 CLI，显示凭证类原因 |
| 400、协议不兼容、坏 JSON、空结果 | 回退 CLI，并按错误类型计入熔断 |
| 连接/TLS/超时/5xx | 回退 CLI，并计入熔断 |
| 429/额度限制 | 直接返回错误，不调用 CLI |
| X 工具调用超过总预算 | 拒绝该结果，不调用 CLI |
| 用户取消 | 立即结束，不回退、不重试、不缓存 |
| 至少一张通过校验的有效图 | 返回真实部分结果；可选补搜失败不能抹掉可用首轮结果 |

同一凭证修订连续 3 次协议/网络类失败后熔断 10 分钟。成功、熔断到期或规范 auth 文件修订变化会恢复探测；`meta.fallbackReason` 使用稳定错误码，UI 再映射为本地化文案。

## 共用质量管线

CLI 与 Responses 的候选必须经过同一套处理：

1. 只接受 HTTPS 且属于明确 X/xAI 媒体 allowlist 的 URL，拒绝 user-info、非默认端口和私网/非媒体目标。
2. 最多跟随 6 跳媒体重定向，每一跳重新验证目标；探测只流式读取最多 64 KiB，完整下载上限 200 MiB。
3. 校验状态码、MIME、真实文件签名和可获得的图片尺寸；拒绝伪装成图片的 HTML/文本。
4. 按规范媒体 URL、twimg CDN 变体、X status id + media index 去重，最多保留 40 个候选；CLI 最多返回 16 张，Responses 初始三路聚合最多返回 24 张，用户主动加载更多后 UI 最多约 32 张。
5. 综合真实图片 MIME、可验证尺寸/像素、宽高比、规范原帖引用和可靠互动量排序；未知数据不能伪造成 0。
6. Host-only `status_id`、`media_index` 和探测质量字段不得跨 IPC。

“从 X 搜索”只能返回 X 来源。通用 Web 图片搜索如需实现，必须成为独立来源和独立 UI 标签，保留来源/引用说明，不能在 X 空结果时静默混入。

## 独立网络与授权图库来源

`网络`、`Openverse` 与 `Pexels` 是独立来源，不参与 X 搜索回退，也不能静默补进 X 结果：

- `网络` 使用固定 Build Responses `web_search` 找公开来源页，再由 Host 解析页面中的结构化图片并走共用安全、签名、尺寸和去重校验。初始两路并发，每个来源页最多保留两张不同的合格图片；结果少不代表请求目标小，常见原因是来源页不可读、候选不是图片、尺寸/质量不足或去重淘汰。
- `Openverse` 使用公开 Images API；`Pexels` 使用用户单独配置且只保留在 Host 的 API Key。两者必须保留提供方返回的来源页、作者和许可信息，禁止把结果描述为“免版权”。
- 初始搜索成功后只在后台预取一页。用户点“加载更多”才把缓冲结果并入画廊，并立即预取下一页；切换来源、替换搜索、取消或关闭弹窗会取消预取并丢弃缓冲。
- 搜索和加载更多只锁定会冲突的搜索按钮。已有卡片必须继续可选、可打开 Lightbox；预取不得显示成前台加载，也不得清空、重排或重新挂载当前画廊。
- `网络` 的主要成本在 Responses 来源发现之后逐页抓取、解析和验图，通常明显慢于直接图库 API。不要通过放松 HTTPS/私网、图片签名、尺寸、来源或去重校验来伪造速度和数量。

完整端点、安全、分页与缓存契约见 [`../plans/WALLPAPER-WEB-SEARCH.md`](../plans/WALLPAPER-WEB-SEARCH.md)。

## 来源弹窗布局

- 宽窗口使用左侧来源导航和右侧单一工作区；X/Web/图库、Imagine、个人相册/本地库用细分隔线分组，不增加解释段落或嵌套卡片。
- 窄窗口把同一组带文字标签横向滚动，不能退回只有图标、无法辨认来源的等宽七宫格；切换来源后当前标签应滚入可见区域。
- 搜索/生成控件、真实进度、路由结果和错误按需出现。来源与许可保留在结果卡片，隐私细节放在可访问 tooltip；不重复展示常驻来源说明、空态说明和页脚操作教学。
- 本地壁纸库默认显示全部媒体，并按静态图优先排序；只有图片和视频同时存在时才展示类型筛选，空库不得出现零计数筛选行。

## Grok Imagine 已保存相册（独立实验来源）

`Grok 相册` 不是 X 搜索的回退渠道，也不使用 Grok Build OAuth。它只在专用远程 WebView 中打开官方 `https://grok.com/imagine/saved` 页面：

- 窗口标签 `grok-imagine-saved` 不得加入任何 Tauri capability。远程页面没有 IPC 权限；主窗口只能调用固定的 `wallpaper_grok_album_*` Host 命令。
- Windows/Linux 使用独立 `data_directory`，macOS 14+ 同时使用稳定的独立 data-store identifier。Cookie 只由 WebView 管理；禁止读取、导出、记录或通过 IPC 返回 Cookie、Token、storage、请求签名和原始 API 响应。
- Windows/Linux 的手动 `http` / `socks5` 代理必须通过 Tauri `proxy_url` 固定到远程 WebView；`socks5h` 规范化为 WebView SOCKSv5。系统/PAC/env 模式继续由原生 WebView 跟随。带认证的代理、不支持的手动协议、Direct 模式及 macOS 手动模式必须结构化失败，禁止静默换路由。代理设置变化时销毁旧相册窗口，下次打开按新路由重建；独立持久 profile 继续保留官方登录态。
- 顶层导航仅允许 HTTPS Grok/xAI 与明确支持的登录提供商；禁止新窗口与下载。新增登录方式时必须先补 allowlist 测试，不能改为任意 HTTPS。
- 未登录时，官方 Saved 路由可能只渲染空壳并返回 401/403；固定启动脚本只检查正常页面壳是否出现，若没有则回到 `https://grok.com/` 展示官方登录入口。脚本不读取 Cookie、storage、响应或账号内容，登录完成后仍由用户打开 Saved。
- Host 只执行仓库内固定的 DOM 快照/滚动脚本，前端不得传入 JavaScript。DOM 脚本在数据离开页面前先剔除未知 URL 查询参数并限制字段长度，Host 再做独立校验和总载荷上限；DTO 只包含 `assets.grok.com/.../generated/...` 媒体 URL、缩略图、类型、尺寸、创建时间和 post id，最多缓存 480 条且不落盘。
- 用户先看到 20 条；当官方相册窗口不在前台时，可用官方页面自身的无限滚动预取下一批 20 条。点击“加载更多”优先瞬时展开缓存，再补热下一批。不得直接调用未公开 `/rest/media/*` 接口。
- `assets.grok.com` 会拒绝主应用 WebView 的跨站 `<img>` 请求。画廊缩略图在严格 `assets.grok.com/.../generated/...` 校验后，并发竞速两条只读路径：隔离 Saved WebView 使用自身登录态抓取并在页内压缩，credential-free Host 使用当前代理抓取并在本地压缩；首个成功结果胜出并取消另一条。输出统一为最长边 480 px、至多 512 KiB 的 JPEG，只以 `data:` URL 暂存在当前前端生命周期内；前端最多 4 路并发，预热范围固定为当前 20 条加下一批 20 条。禁止把这批缩略图写入磁盘或壁纸库。
- 缩略图等待态使用静态占位；不得用无限 shimmer 制造“反复重载”的错觉。轮询和下一批预热必须保留已有卡片及其内存缩略图，不得清空、重排或重新挂载已显示结果。
- 多张未缓存卡片可以共用同一静态占位图，但查看器必须按原始输入索引打开用户点击的媒体，禁止按占位图 URL 反查索引。各 slide 的可显示 URL 并发解析；选中的 lazy 原图在 Lightbox 立即挂载后按需升级，不能被慢兄弟项串行阻塞。
- 搜索/筛选由官方页面自身完成，之后用户同步当前已加载结果；弹窗内的画廊筛选仍只是本地筛选。只有用户预览或应用某条媒体时，才允许下载原始媒体：credential-free Host 当前代理请求与隔离 Saved WebView 的固定 `credentials: include` 请求同时启动，首个成功结果胜出。Host 胜出会协作取消并清理 WebView 作业；WebView 胜出时 Blob 留在远程页内，Host 以最多 512 KiB 二进制分块读取。两路在唯一保存点前汇合，每次 eval 必须低于桥接载荷上限，最终仍执行 URL allowlist、200 MiB 上限、MIME/真实签名校验并写入独立 `grok_album` 壁纸目录；禁止读取或桥接 Cookie、Token、storage、请求头及原始 API 响应。
- 登录中、空相册、桥接失败和窗口关闭必须展示真实状态；顶层导航、关闭窗口或进入非 Saved 页面都会清空内存缓存，不得伪造 CDN 占位图，也不得把历史账号的缓存带入下一页面实例。
- 每次进入相册来源时，首个 Host 快照返回前必须显示明确的加载状态；不得先用默认 `closed` 闪现“窗口未打开”。首次快照失败必须结束加载并显示结构化错误，后续静默轮询失败才允许保留已有画廊。

完整安全契约与分阶段计划见 [`../plans/WALLPAPER-GROK-SAVED-ALBUM.md`](../plans/WALLPAPER-GROK-SAVED-ALBUM.md)。

## 请求生命周期

- 前端生成 UUID `requestId`；Host 注册表、进度事件、搜索结果和取消命令必须使用同一 id。
- 阶段为 `preparing → searching_x → validating → supplementing/falling_back → done`；进度只是提示，最终 invoke 结果仍是权威。
- Responses 实时结果通过 `wallpaper://x-search-batch` 发送，字段固定为 `requestId`、`batchIndex`、`items`、`accumulatedCount`、`done`。批次按完成顺序追加，不要求 `batchIndex` 递增；前端必须忽略重复批次、跨批 URL 去重，并只接受当前 requestId/generation。Host-only 探测字段不得进入批次 DTO。
- 实时 Responses 搜索只在各路完成时发送批次，最后完成的一路标记 `done=true`；若最后一路失败，可以发送空终态批次。缓存命中发送一份终态批次；CLI 路由不发送批次。
- 加载更多复用同一批次事件和 requestId/generation 隔离，但只发送该次新增项；有结果或空结果都必须以 `done=true` 收口。最终 invoke 结果仍负责替换新增项的稀疏批次元数据，原画廊不得被空结果或失败清除。
- 点击取消、关闭弹窗、切到 Imagine/图库、替换搜索或组件卸载都会使当前 generation 失效并请求 Host 取消。
- 前端必须拒绝 generation 或 requestId 不匹配的迟到结果；Host 取消信号为 sticky，覆盖 Responses 请求/正文读取、图片探测和 CLI 两轮搜索。
- 缓存为 Host 进程内 32 项 LRU、TTL 10 分钟，只缓存成功且非空的安全结果 DTO。key 包含规范 query、排序、请求模式、契约版本；Responses 还包含非秘密的凭证文件修订。
- 命中缓存要保留原 `routeUsed`/回退信息，换成本次 requestId，并标记 `cacheHit=true`。分段 provider 耗时属于写入缓存的原请求，命中时必须清空，不能把旧搜索耗时冒充为本次缓存成本。缓存不落盘，重启即清空。

## 测试与排障

常用定向检查：

```bash
pnpm test -- src/lib/wallpaperSource.test.ts src/lib/wallpaperXSearch.test.ts src/hooks/useWallpaperXSearch.test.tsx src/components/WallpaperSourceModal.test.tsx
pnpm typecheck
cd src-tauri && cargo test wallpaper_x --no-run
```

Windows 的 Rust test harness 必须嵌入 `src-tauri/windows-test-manifest.xml`，否则 Tauri/WebView2 依赖可能在断言前以 `0xc0000139 STATUS_ENTRYPOINT_NOT_FOUND` 退出。以 `.github/workflows/ci.yml` 的 Windows `cargo test` 步骤为准：先 `cargo test --no-run`，用 Windows SDK `mt.exe` 写入 `RT_MANIFEST #1`，再直接运行 `target/debug/deps/grok_app_lib-*.exe`。不要在 `build.rs` 添加第二份 `/MANIFESTINPUT`，否则产品链接会报 `CVT1100`。

CI 清理第三方 PATH 中的旧 `api-ms-win-*.dll` 转发器时必须保留 `%SystemRoot%` 及其子目录；否则 `findstr.exe`、`ping.exe`、`taskkill.exe` 等系统命令会消失并制造假失败。

排障顺序：

1. 查看结果 `meta.routeUsed`、`fallbackReason`、`durationMs`、可选分段耗时和 `cacheHit`，不要从设置值推断。分段耗时只增加观测，不引入 Responses 自动重试或额外 CLI 回退。
2. Responses 总是回退时检查 Grok Build 是否已登录、access token 是否临近过期、系统/手动代理是否能访问固定端点。
3. 同查询很快返回属于预期缓存行为；改变 query、Top/Latest、模式或凭证修订后不应误命中。
4. 429 或工具超预算后没有 CLI 回退是防止重复消耗的设计，不是路由遗漏。
5. 取消后 UI 应立即失效旧 generation，Host/CLI 应在 2 秒内结束；若有迟到画廊，先检查 requestId 和 generation guard。

## 灰度规则

当前不得展示 `auto`，也不得把默认从 `cli` 改走。只有新的交叉样本同时满足以下条件，才能另开阶段讨论自动路由：Responses p50 相对 CLI 至少快 30%，成功率不低于 CLI 5 个百分点以上，至少 80% 查询返回 6 张有效图，坏图率和重复率各不高于 5%，规范 X 引用比例至少 80%，工具预算、回退 mock、取消和隐私门禁全部通过。

历史基线与决策见：

- [`../qa/2026-08-29-wallpaper-responses-progressive-load-more.md`](../qa/2026-08-29-wallpaper-responses-progressive-load-more.md)
- [`../qa/2026-08-29-wallpaper-responses-output-latency.md`](../qa/2026-08-29-wallpaper-responses-output-latency.md)
- [`../plans/2026-08-28-wallpaper-x-search-baseline.md`](../plans/2026-08-28-wallpaper-x-search-baseline.md)
- [`../plans/2026-08-28-wallpaper-x-responses-preview-plan.md`](../plans/2026-08-28-wallpaper-x-responses-preview-plan.md)
- [`../qa/2026-08-28-wallpaper-x-stage4-quality.md`](../qa/2026-08-28-wallpaper-x-stage4-quality.md)
- [`../qa/2026-08-28-wallpaper-x-stage5-lifecycle.md`](../qa/2026-08-28-wallpaper-x-stage5-lifecycle.md)
- [`../qa/2026-08-28-wallpaper-x-stage6-final.md`](../qa/2026-08-28-wallpaper-x-stage6-final.md)

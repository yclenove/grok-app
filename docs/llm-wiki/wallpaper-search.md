# 壁纸搜索

维护“设置 → 外观 → 主题 → 背景图 → 从 X 搜索”时必读。该能力只做只读搜索和壁纸媒体处理；不要把聊天主路由、自定义提供商或通用 Web 搜索混入 X 来源。

## 产品契约

### 本地媒体记录与浏览恢复

- `wallpaper_catalog` 在壁纸根目录原子保存 `.catalog.json`，保留收藏、已知图片尺寸、来源/许可及 Host 确认的生成参数和父图 ID。生成接口返回同一持久记录；历史未知字段不推测补齐。元数据写入失败保留生成文件并返回稳定错误码，不能自动重新生成。
- 原图物化的共用前端入口 `ensureLocalWallpaperMedia` 保存来源信息。`wallpaper_library_lookup` 每批最多匹配 96 项，以来源隔离的 URL 哈希关联本地文件，保留查询参数身份，不存原始媒体 URL。查询不下载、不枚举私有相册；路径须在壁纸根目录内，已变更或缺失的文件不复用。来源关闭后的结果不能写回，较新的收藏操作不能被旧查询覆盖。
- 来源历史在同一次弹窗内有界保存。相册内容仍由认证控制器拥有，切回先取得新 Host 快照；临时加载保留筛选、滚动和已展开页数，页面变化或认证失效清除旧导航状态，关闭弹窗重置页数。本地库查询缓存 20 分钟、最多 8 个查询和每查询 2000 项；追加不续期。过期不突然清空当前画廊，下一次分页重新读取快照。

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
- 一次搜索并发启动 3 路互补请求：直接主题、视觉变化、发现/双语扩展。每路目标 8 张、最多 3 次 `x_search`，因此单次用户搜索的硬上限是 3 个 HTTP 请求、9 次工具调用和 24 张最终结果；客户端必须同时用提示词限制并核验每路响应中的真实工具调用数，不能只相信 `max_tool_calls`。结构化输出中没有任何真实 `x_search` 调用也必须按协议错误拒绝，不能接受模型直接编写的结果。
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

- `网络` 使用固定 Build Responses `web_search` 找公开来源页，再由 Host 解析页面中的结构化图片并走共用安全、签名、尺寸和去重校验。初始三路并发，每路目标 8 个来源页，全局最多返回 20 张；每个来源页最多保留两张不同的合格图片。每路请求和提示仍限制 6 次工具调用；若兼容端点无视该限制，Host 会记录并容忍至多 12 次已完成调用，超过独立硬上限仍拒绝。结果少不代表请求目标小，常见原因是 Responses 某路超时、来源页不可读、候选不是图片、尺寸/质量不足或去重淘汰。
- `Openverse` 使用公开 Images API；`Pexels` 使用用户单独配置且只保留在 Host 的 API Key。两者必须保留提供方返回的来源页、作者和许可信息，禁止把结果描述为“免版权”。
- `网络` 加载更多只会把已显示来源页规范成有限的 hostname/site 列表，作为数据交给新的 Responses 请求主动避开已见站点；路径、查询和 fragment 不进入模型提示，Host 的来源页与媒体去重仍是最终权威。
- 远程缩略图与原图 IPC 只接受来源类型、规范媒体 URL 和请求 id，前端不得传 Referer、来源页或任意请求头。搜索结果通过 Host 校验时，Host 才把媒体 URL 与来源页的 HTTPS origin 登记到按来源隔离、最多 512 项、滑动 TTL 30 分钟的内存表；后续请求只能复用这个 origin，不会携带路径、查询、Cookie、Token 或 Authorization。缩略图源文件、最长边、总像素和解码分配都必须有独立上限，防止小体积高像素文件放大 Host 内存占用。
- Pexels 使用其官方 `query` 参数。每次真实上游请求都追加一个 Host 随机生成、与凭证和查询无关的 `_grokapp_cache_bust` 值，避免共享 CDN 把其他鉴权上下文的旧 `200` 响应复用到当前请求。该值不进入 Host 搜索缓存身份；缓存仍按规范查询、凭证修订和契约版本隔离。
- 直连图库请求会从规范查询派生一个 provider-only 查询：移除独立的 `wallpaper` / `wallpapers`、`4K` / `8K` / `UHD` 以及对应的中日韩“壁纸”修饰词，避免把展示用途和分辨率要求误当成图库主题。若移除后为空则继续使用原查询。缓存、分页身份和替换搜索仍使用原始规范查询；该规则不得改写 `网络`、X 或 Imagine 的查询，也不得静默翻译用户主题。修改净化规则时必须提升图库缓存契约版本。
- 初始搜索成功后只在后台预取一页。用户点“加载更多”才把缓冲结果并入画廊，并立即预取下一页；切换来源、替换搜索、取消或关闭弹窗会取消预取并丢弃缓冲。
- 后台预取失败保持静默，不能消耗用户的一次“加载更多”。Host 分页失败通常会正常 resolve 为带 `errorCode` 的结构化结果，而不是抛出 Promise；两种失败形态都必须丢弃隐藏结果。用户明确点击后重新发起一次前台分页请求；若该请求仍失败，再显示真实错误并保留已有卡片与继续加载入口，不做自动循环重试。
- 搜索和加载更多只锁定会冲突的搜索按钮。已有卡片必须继续可选、可打开 Lightbox；预取不得显示成前台加载，也不得清空、重排或重新挂载当前画廊。
- `网络` 的主要成本在 Responses 来源发现之后逐页抓取、解析和验图，通常明显慢于直接图库 API。不要通过放松 HTTPS/私网、图片签名、尺寸、来源或去重校验来伪造速度和数量。

完整端点、安全、分页与缓存契约见 [`../plans/WALLPAPER-WEB-SEARCH.md`](../plans/WALLPAPER-WEB-SEARCH.md)。

## 来源弹窗布局

- 来源导航始终位于工作区上方，让画廊使用完整宽度；七个来源按“搜索 / 生成 / 个人内容”形成三个无标题的紧凑控件簇，靠间距而不是说明文字建立层次。窄屏保持单行并允许横向滚动，不增加组标题、解释段落或嵌套卡片。
- 每个来源始终同时显示图标和文字，不能退回只有图标、无法辨认来源的控件；切换来源后当前标签应滚入可见区域。
- 搜索/生成控件、真实进度、路由结果和错误按需出现。来源与许可保留在结果卡片，隐私细节放在可访问 tooltip；不重复展示常驻来源说明、空态说明和页脚操作教学。
- 本地壁纸库默认显示全部媒体，并按静态图优先排序；只有图片和视频同时存在时才展示类型筛选，空库不得出现零计数筛选行。
- 本地库通过 `wallpaper_library_page` 按 48 项读取 Host 快照，排序为静图优先、修改时间倒序和路径；不再把最近 96 项当作整个库。搜索和类型筛选在 Host 全集合执行。游标绑定查询和页大小，有效期 30 分钟；新增文件刷新后出现，已删除文件跳过，计数表示快照创建时的数量。翻页失败保留已有卡片并允许重试。
- “外观”页的壁纸预览按卡片自身宽度响应，而不是只看 viewport。卡片窄于 520 px 时预览和操作区改为单列，预览继续保持 `16:10`，不得压成竖条。

## Imagine 静图与图片转视频

- 改图通过 `wallpaper_image_edit` 调用受限 CLI 的 `image_edit`，复用来源规范化、取消与结果审计。`auto` 使用单个原图快照；显式 `16:9` / `9:16` / `1:1` / `4:3` 使用同一快照的两次引用，启用上游多参考图请求的原生 `aspect_ratio`。单参考图请求会忽略该参数。不得为改变比例给原图补白边、拉伸或裁切；提示模型按目标比例重新构图和延展场景。两次引用必须都指向本次同一个快照，审计不允许替换第二张图，也不允许额外工具调用或自动重试。
- Imagine 图片模式的 `wallpaper_imagine` 使用独立 requestId、输出目录和受限 `image_gen` 会话，复用改图/视频的取消机制。Host 审计一次成功调用与准确提示词/比例，并从本次 session 复制唯一有效图片；不依据模型最终回复验收，也不扫描当天旧图充当新结果。不自动生成额外变体或重试。所有来源的静态图片卡提供“改图”和“生成视频”按钮，视频卡不显示这两项。
- 点击按钮后先通过来源已有的安全下载路径准备本地图片。网络图库继续走 Host allowlist/签名校验，Grok 相册继续使用隔离 WebView 与 credential-free Host 竞速；不得把 Cookie、Token 或任意请求头导出到主应用。
- 视频模式立即预填本地可编辑的运动/镜头模板，不额外请求模型或网络。只有 `imagine` 来源的原始 prompt 可作为至多 240 个 Unicode 字符的场景上下文；其他来源使用通用模板，不读取远程标题、描述或 Grok Saved 时间戳。净化明确的控制字符和 bidi 控制符，保留 ZWJ/ZWNJ。模板补充慢推镜头、自然运动、主体稳定、构图和风格保持约束；Host 把用户编辑的提示词作为场景数据，不能让它改写工具、路径、时长或分辨率。模式提供 `6` / `10` 秒和 `480p` / `720p`，默认 `6` 秒、`480p`。源图使用 `56 x 36` 紧凑缩略图，长文案截断。
- `wallpaper_image_to_video` 使用本机 Grok Build CLI、`--effort low`、最多 3 turns 和 420 秒硬超时。专用 runner 固定 `--tools image_to_video`、`--disallowed-tools search_tool,use_tool`、`--disable-web-search`、`--no-subagents`，使用 Host 生成的新会话 UUID 和规范官方 `GROK_HOME`。前端不能指定模型、工具、CLI 参数或输出目录。
- AVIF/WebP/GIF 由主应用 WebView 解码为最长边 2048 px 的 PNG。原图通过有上限的 raw IPC 读取，最多 40 MiB；PNG 最多 20 MiB。Host 验证原始路径位于壁纸库，再独立解码图片，限制单边 16384、5000 万像素和 256 MiB 解码分配；缩放后应用 EXIF 旋转或镜像方向，再移除元数据并重新编码为本次任务的临时 PNG，避免手机照片方向丢失或重复旋转。禁止依赖 shell、ffmpeg 或用户安装的图像工具。临时源图不进入图库，成功或失败后清理。
- Host 从已知 CLI session 的 `updates.jsonl` 审计恰好一个成功完成的 `image_to_video`；完整源图路径、提示词、时长和分辨率必须匹配。额外工具、参数变更、缺失或损坏日志一律拒收。此审计是结果验收边界，不保证 CLI 违约前没有产生上游调用；不自动重试。日志读取上限 4 MiB，不把原始日志写入 QA 报告。
- Host 自行从本次会话 `videos/` 目录复制唯一成片，不接受模型指定的复制路径，也不授予 shell 文件操作能力。
- Host 只接受当前 `{app_data}/wallpapers/imagine/<date>/video-*` 输出目录内的 MP4/WebM；落盘结果必须再通过路径 containment、文件签名、MIME、扩展名和 200 MiB 上限校验。Windows 内部可以使用 canonical extended path，但返回前端的本地路径必须移除 `\\?\` 前缀。
- 生成使用 UUID `requestId`；取消信号 sticky，覆盖“取消先于注册”的竞态。取消、关闭弹窗或切换模式会终止进程树、忽略迟到结果并清理失败输出。成片只进入画廊和壁纸库，必须由用户明确选择后才能设为背景。

## Grok Imagine 已保存相册（独立实验来源）

`Grok 相册` 不是 X 搜索的回退渠道，也不使用 Grok Build OAuth。它只在专用远程 WebView 中打开官方 `https://grok.com/imagine/saved` 页面：

- 窗口标签 `grok-imagine-saved` 不得加入任何 Tauri capability。远程页面没有 IPC 权限；主窗口只能调用固定的 `wallpaper_grok_album_*` Host 命令。
- 页面跟踪脚本只在 HTTPS `grok.com` 的顶层文档运行；不得包装 xAI、Google、Apple 等登录页的 History API，也不得在子框架内安装相册状态或自动恢复逻辑。未登录恢复仍仅作用于顶层 `/imagine/saved`。
- Windows/Linux 使用独立 `data_directory`，macOS 14+ 同时使用稳定的独立 data-store identifier。Cookie 只由 WebView 管理；禁止读取、导出、记录或通过 IPC 返回 Cookie、Token、storage、请求签名和原始 API 响应。
- Windows/Linux 的手动 `http` / `socks5` 代理必须通过 Tauri `proxy_url` 固定到远程 WebView；`socks5h` 规范化为 WebView SOCKSv5。系统/PAC/env 模式继续由原生 WebView 跟随。带认证的代理、不支持的手动协议、Direct 模式及 macOS 手动模式必须结构化失败，禁止静默换路由。代理设置变化时销毁旧相册窗口，下次打开按新路由重建；独立持久 profile 继续保留官方登录态。
- 顶层导航仅允许 HTTPS Grok/xAI 与明确支持的登录提供商；禁止新窗口与下载。新增登录方式时必须先补 allowlist 测试，不能改为任意 HTTPS。
- 未登录时，官方 Saved 路由可能只渲染空壳并返回 401/403；固定启动脚本只检查正常页面壳是否出现，若没有则回到 `https://grok.com/` 展示官方登录入口。脚本不读取 Cookie、storage、响应或账号内容，登录完成后仍由用户打开 Saved。
- Host 只执行仓库内固定的 DOM 快照/滚动脚本，前端不得传入 JavaScript。DOM 脚本在数据离开页面前先剔除未知 URL 查询参数并限制字段长度，Host 再做独立校验和总载荷上限；DTO 只包含 `assets.grok.com/.../generated/...` 媒体 URL、缩略图、类型、尺寸、创建时间和 post id，最多缓存 480 条且不落盘。
- 用户先看到 20 条；当官方相册窗口不在前台时，可用官方页面自身的无限滚动预取下一批 20 条。点击“加载更多”优先瞬时展开缓存，再补热下一批；若点击恰好复用了一个因相册窗口获得焦点而跳过的后台 Promise，必须补发一次真实前台分页，不能误判为耗尽。不得直接调用未公开 `/rest/media/*` 接口。
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
pnpm test -- \
  src/lib/wallpaperSource.test.ts src/lib/wallpaperXSearch.test.ts \
  src/hooks/useWallpaperXSearch.test.tsx src/hooks/useWallpaperGrokAlbum.test.tsx \
  src/components/WallpaperSourceModal.test.tsx \
  src/components/WallpaperSourceModal.sources.test.tsx \
  src/components/WallpaperSourceModal.x-paging.test.tsx \
  src/components/WallpaperSourceTabs.test.tsx
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

# 壁纸搜索

维护“设置 → 外观 → 主题 → 背景图 → 从 X 搜索”时必读。该能力只做只读搜索和壁纸媒体处理；不要把聊天主路由、自定义提供商或通用 Web 搜索混入 X 来源。

## 产品契约

| 设置值 | UI | 实际行为 |
|---|---|---|
| `cli` | 稳定渠道，默认 | 启动本机 Grok Build CLI，使用其官方 X 工具 |
| `responses_preview` | 用户主动选择的预览渠道 | 优先请求固定的 Build Responses 兼容端点；按规则至多回退一次 CLI |
| `auto` | 协议保留，不在 UI 展示 | 当前按 `cli` 处理；没有 QA 决议前不得自动灰度或改成默认 |

缺失、空白、损坏或未知设置值必须归一为 `cli`。UI 必须展示 `meta.routeUsed`、回退原因、耗时和缓存命中等真实结果，不得根据用户选择猜测实际路由。

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
- 第一轮最多 2 次 `x_search`，少于 6 张时补 1 次，总预算最多 3 次。客户端必须同时用提示词限制并核验响应中的真实工具调用数，不能只相信 `max_tool_calls`。
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
4. 按规范媒体 URL、twimg CDN 变体、X status id + media index 去重，最多保留 40 个候选、返回 16 张。
5. 综合真实图片 MIME、可验证尺寸/像素、宽高比、规范原帖引用和可靠互动量排序；未知数据不能伪造成 0。
6. Host-only `status_id`、`media_index` 和探测质量字段不得跨 IPC。

“从 X 搜索”只能返回 X 来源。通用 Web 图片搜索如需实现，必须成为独立来源和独立 UI 标签，保留来源/引用说明，不能在 X 空结果时静默混入。

## 请求生命周期

- 前端生成 UUID `requestId`；Host 注册表、进度事件、搜索结果和取消命令必须使用同一 id。
- 阶段为 `preparing → searching_x → validating → supplementing/falling_back → done`；进度只是提示，最终 invoke 结果仍是权威。
- 点击取消、关闭弹窗、切到 Imagine/图库、替换搜索或组件卸载都会使当前 generation 失效并请求 Host 取消。
- 前端必须拒绝 generation 或 requestId 不匹配的迟到结果；Host 取消信号为 sticky，覆盖 Responses 请求/正文读取、图片探测和 CLI 两轮搜索。
- 缓存为 Host 进程内 32 项 LRU、TTL 10 分钟，只缓存成功且非空的安全结果 DTO。key 包含规范 query、排序、请求模式、契约版本；Responses 还包含非秘密的凭证文件修订。
- 命中缓存要保留原 `routeUsed`/回退信息，换成本次 requestId，并标记 `cacheHit=true`。缓存不落盘，重启即清空。

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

1. 查看结果 `meta.routeUsed`、`fallbackReason`、`durationMs` 和 `cacheHit`，不要从设置值推断。
2. Responses 总是回退时检查 Grok Build 是否已登录、access token 是否临近过期、系统/手动代理是否能访问固定端点。
3. 同查询很快返回属于预期缓存行为；改变 query、Top/Latest、模式或凭证修订后不应误命中。
4. 429 或工具超预算后没有 CLI 回退是防止重复消耗的设计，不是路由遗漏。
5. 取消后 UI 应立即失效旧 generation，Host/CLI 应在 2 秒内结束；若有迟到画廊，先检查 requestId 和 generation guard。

## 灰度规则

当前不得展示 `auto`，也不得把默认从 `cli` 改走。只有新的交叉样本同时满足以下条件，才能另开阶段讨论自动路由：Responses p50 相对 CLI 至少快 30%，成功率不低于 CLI 5 个百分点以上，至少 80% 查询返回 6 张有效图，坏图率和重复率各不高于 5%，规范 X 引用比例至少 80%，工具预算、回退 mock、取消和隐私门禁全部通过。

历史基线与决策见：

- [`../plans/2026-08-28-wallpaper-x-search-baseline.md`](../plans/2026-08-28-wallpaper-x-search-baseline.md)
- [`../plans/2026-08-28-wallpaper-x-responses-preview-plan.md`](../plans/2026-08-28-wallpaper-x-responses-preview-plan.md)
- [`../qa/2026-08-28-wallpaper-x-stage4-quality.md`](../qa/2026-08-28-wallpaper-x-stage4-quality.md)
- [`../qa/2026-08-28-wallpaper-x-stage5-lifecycle.md`](../qa/2026-08-28-wallpaper-x-stage5-lifecycle.md)
- [`../qa/2026-08-28-wallpaper-x-stage6-final.md`](../qa/2026-08-28-wallpaper-x-stage6-final.md)

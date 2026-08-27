# 壁纸 X 搜索：阶段 0 脱敏基线报告

**日期**：2026-08-28
**代码基线**：`main` @ `91bf92286988ad74708381ee2983a94bf65b625d`
**范围**：只测现有 CLI 与 Build OAuth Responses 候选链路；未修改设置、IPC、Host 路由或 UI
**基准脚本**：`scripts/benchmark-wallpaper-x-search.mjs`

## 1. 结论

建议 **go：进入阶段 1 的设置数据契约**，但继续保持 `cli` 为默认值，只把 Responses 作为后续手动预览能力，不开放 `auto`。

- Responses 8/8 成功；CLI 7/8 成功。
- 成功样本端到端延迟：Responses p50 `67.1s`、p95 `75.1s`；CLI p50 `107.9s`、p95 `135.2s`。
- Responses 相对 CLI 的 p50 快 `37.8%`，p95 快 `44.5%`，达到计划中“p50 至少快 30%”的门槛。
- Responses 返回 82/88 张可达图片（`93.2%`）；CLI 返回 160/195 张可达图片（`82.1%`）。
- CLI 成功时通常给出更多图片，中位有效图为 26；Responses 为 12。首版体验取舍是“更快、数量够用”与“更慢、候选更多”，不能只宣传提速。
- Responses 全部正式样本实际调用 3 次 `x_search`。后续控制样本证明 `max_tool_calls` 在该兼容端点上不是可靠硬上限：参数设为 1、提示词仍允许 3 次时，服务端实际调用 3 次；当参数和提示词都限制为 1 时，实际降为 1 次。因此正式样本不能单独证明该字段生效，客户端必须双重约束并核验。
- `grok-4.6 + low` 在 8 个主题中都取得至少 6 张有效图，因此阶段 0 不追加 `medium` 对照。当前判断只覆盖结构、来源和可达性，尚未形成可复现的人工审美评分。

这组样本量很小，且依赖未公开承诺稳定的 Build Responses 兼容接口。结果支持“手动灰度”，不支持改默认值，也不构成零封号风险或长期兼容承诺。

## 2. 测试契约

### CLI（现有产品路径）

- 每个样本启动新的 `grok -p` 进程。
- 与 `src-tauri/src/wallpaper_source.rs` 对齐：当前产品搜索提示、JSON schema、`--always-approve`、`--max-turns 14`、`--effort low`、`--output-format json`。
- 产品硬超时为 150 秒；基准脚本用 155 秒，额外 5 秒仅供结束进程。
- 解析对齐产品的 envelope、`structuredOutput`、嵌套/拼接 JSON 与媒体 URL 兜底。

### Responses（候选预览路径）

- 固定 Build endpoint，凭证只读取规范 Grok Build OAuth access token。
- 请求模型 `grok-4.6`，服务端解析为 `grok-4.6-build`；reasoning effort 为 `low`。
- 只注册 `x_search`，请求字段和提示词都要求最多 3 次工具调用，`store=false`，严格 JSON schema。实现不能只信任 `max_tool_calls`，还必须在提示词中声明预算并核验响应中的实际调用数。
- 目标返回 8–16 个候选；这与 CLI 当前 12–28 个候选的目标不同，因此数量差异是契约的一部分，延迟比较不是“完全相同输出量”的模型吞吐测试。

### 共用质量口径

- 规范化并去重媒体 URL。
- 只允许明确的 X/xAI 媒体主机。
- 使用 Range GET；即使 CDN 忽略 Range，也只读取前 2 KiB 后主动取消流。
- 200/206、非文本 MIME、至少读取 32 bytes 才记为可达。
- 原帖引用必须匹配规范的 X/Twitter status URL。
- 探测后 0 张有效图按端到端失败计，不把“快速空结果”算作成功。

## 3. 固定主题

| 主题 id | 语言/类型 | 排序 |
|---|---|---|
| `misty-mountain` | 英文、横屏风景摄影 | Top |
| `cyberpunk-vertical-zh` | 中文、竖屏赛博朋克 | Latest |
| `ocean-ultrawide` | 英文、超宽海景摄影 | Top |
| `ink-landscape-zh` | 中文、横屏水墨风景 | Top |
| `space-nebula` | 英文、星云摄影 | Latest |
| `abstract-ai-prompt` | 英文、AI 抽象艺术 | Top |
| `macro-flower-vertical` | 英文、竖屏微距摄影 | Latest |
| `rainy-anime-street` | 英文、动漫雨街 | Top |

## 4. 汇总结果

延迟 p50/p95 只统计真正返回至少 1 张有效图的成功样本；总候选和有效率统计全部 8 次尝试。

| 指标 | Responses | CLI |
|---|---:|---:|
| 尝试 / 成功 | 8 / 8 | 8 / 7 |
| 成功率 | 100% | 87.5% |
| 成功样本 p50 | 67.1s | 107.9s |
| 成功样本 p95 | 75.1s | 135.2s |
| 中位有效图（成功样本） | 12 | 26 |
| 总有效图 / 总候选 | 82 / 88 | 160 / 195 |
| 总体图片可达率 | 93.2% | 82.1% |
| 规范原帖引用 / 总候选 | 88 / 88 | 194 / 195 |
| 输入 tokens（8 次总计） | 103,561 | 448,786 |
| 实际 X 搜索调用 | 每次 3 | envelope 不可靠提供 |

Responses 的输入 tokens 总量低 `76.9%`（CLI 约为 Responses 的 4.33 倍）。这是接口 usage 指标，不应直接换算成独立 API 账单或订阅费用。

## 5. 逐主题结果

`有效/候选` 中的有效指通过在线图片探测；耗时包含搜索和探测。

| 主题 | Responses 耗时 | Responses 有效/候选 | CLI 耗时 | CLI 有效/候选 |
|---|---:|---:|---:|---:|
| `misty-mountain` | 75.1s | 12/12 | 11.0s | 0/1（失败） |
| `cyberpunk-vertical-zh` | 71.8s | 12/12 | 87.9s | 26/26 |
| `ocean-ultrawide` | 63.5s | 12/12 | 107.9s | 28/28 |
| `ink-landscape-zh` | 74.9s | 10/10 | 92.3s | 28/28 |
| `space-nebula` | 67.1s | 12/12 | 128.8s | 14/28 |
| `abstract-ai-prompt` | 50.7s | 6/6 | 110.1s | 23/28 |
| `macro-flower-vertical` | 68.7s | 6/12 | 135.2s | 13/28 |
| `rainy-anime-street` | 65.5s | 12/12 | 84.2s | 28/28 |

7 个两边都成功的配对主题中，Responses 全部更快，单主题提速约 18%–54%。`macro-flower-vertical` 两条路径的图片可达率都接近一半，说明更多候选不能替代统一的链接校验、去重与条件补搜。

## 6. 波动与代理前提

- 本机访问该 endpoint 必须经过已配置的 Windows 代理。Node 24 的 `fetch` 默认不读取 WinHTTP 设置；无代理预检得到连接超时，正式脚本通过标准 `HTTPS_PROXY` 与 `node --use-env-proxy` 对齐 App 给子进程/reqwest 注入代理的行为。
- 同一 Responses 主题的预检为 62.4 秒，完整集中的重复样本为 75.1 秒，没有可证明的“暖启动必然更快”。
- 第一个不计入正式基线的协议控制样本将 `max_tool_calls` 设为 1、但保留提示词“最多 3 次”，结果仍发生 3 次 X 搜索。第二个控制样本让参数和提示词都限制为 1，结果实际调用 1 次，55.8 秒返回 11 个候选（该样本跳过在线图片探测）。该 Build 兼容端点当前会接受字段但不会可靠地让它覆盖提示词；后续客户端需采用“参数 + 提示词 + 响应核验”，并把超预算视为可观测的协议偏差。
- CLI 每次都是新进程；同一 `misty-mountain` 主题两次对齐产品契约的结果都为 0 张有效图。另有一次早期近似产品参数的 150 秒无输出超时，未纳入正式 8 样本统计。
- Responses 仍需要 50–75 秒，不是即时搜索。阶段 5 的真实进度、取消和缓存仍有明显价值。

## 7. 验证与环境状态

通过：

```text
node --check scripts/benchmark-wallpaper-x-search.mjs
pnpm test -- src/lib/wallpaperSource.test.ts src/lib/xEvidenceCitation.test.ts
  2 files passed, 25 tests passed
```

前端测试依赖通过 `pnpm install --frozen-lockfile --ignore-scripts` 补齐；它不证明 `canvas` 原生模块或完整打包环境可用。普通安装在 Node 24.15.0 下仍受 `canvas` 预编译下载超时和本机缺少 Cairo/GTK 头文件阻塞。

Windows 下直接运行 `cargo test wallpaper_source` 会完成编译和链接，但生成的 test harness 没有 manifest 资源；它因此加载 System32 默认的 Common Controls v5，而二进制又直接导入仅 v6 提供的 `TaskDialogIndirect`，在进入断言前报错：

```text
cargo test wallpaper_source
STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)
```

这不是测试断言或本机 PATH 运行库冲突。仓库已经在 `.github/workflows/ci.yml` 提供 Windows 专用流程：先 `cargo test --no-run`，再用 Windows SDK `mt.exe` 把 `windows-test-manifest.xml` 嵌入 test harness，最后直接运行 harness。按该现有流程验证后：

```text
running 15 tests
test result: ok. 15 passed; 0 failed; 1512 filtered out
```

在恢复原始 PATH 后同一 harness 仍为 15/15 通过，进一步确认根因是缺 manifest。诊断只修改了 `target/` 下的生成物，没有修改产品源码；普通本地 `cargo test` 命令仍不会自动执行 CI 的 post-link 步骤。

## 8. 脱敏与复现

- 脚本只输出主题 id、路由、耗时、计数、模型 id、effort、usage 和稳定错误码。
- 不输出 token、请求头、原始模型正文、搜索原词、媒体 URL、用户名或 auth 文件内容。
- token 只在进程内存中使用；本阶段未保存原始 Responses/CLI fixture。

无系统代理或 Node 能直接联网时：

```powershell
node scripts/benchmark-wallpaper-x-search.mjs --route responses --limit 8
node scripts/benchmark-wallpaper-x-search.mjs --route cli --limit 8
```

需要代理时，先把 `HTTPS_PROXY` 设为用户当前已配置的代理，再使用 Node 24 的环境代理开关：

```powershell
$env:HTTPS_PROXY = '<configured-proxy>'
node --use-env-proxy scripts/benchmark-wallpaper-x-search.mjs --route responses --limit 8
```

## 9. 阶段停止点

阶段 0 到此停止。下一步若获确认，只进入阶段 1：增加默认 `cli` 的设置数据契约与迁移测试；不实现 HTTP 客户端、不显示 UI 设置、不改变生产搜索路由。

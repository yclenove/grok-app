# M0 Preview 2.0 实现计划

> **面向 AI 代理的工作者：** 使用 `subagent-driven-development` 或 `executing-plans` 逐任务实现。所有复选框表示开发待办，文档完成不代表任务已执行。

**目标：** 交付 Host 管理的浏览器任务组、完整 Preview 手动工作流与向后兼容迁移。

**架构：** Rust 持有 BrowserSession/Tab，React 消费投影；原生 WebView adapter 提供真实导航与生命周期。既有 SideWorkbench 只装配领域组件。

**技术栈：** Rust/Tauri、React/TypeScript、现有原子 JSON、Vitest、平台原生 WebView、开发新增 fixture runner。

规格：[m0-preview](../../specs/2026-09-05-browser-rearchitecture/m0-preview.md)。公共类型：[00-contracts](../../specs/2026-09-05-browser-rearchitecture/00-contracts.md)。命令未注明目录时从仓库根运行；`cargo --manifest-path src-tauri/Cargo.toml` 指向本项目。

## 文件职责

| 路径 | 变更类型与职责 |
| --- | --- |
| `src-tauri/src/browser/{mod,protocol,lifecycle,persistence}.rs` | 新建：领域模型、owner、原子 checkpoint |
| `src-tauri/src/browser/preview/{mod,registry,navigation,downloads,tests}.rs` | 新建：WebView adapter，不接 Agent eval |
| `src-tauri/src/browser/preview/platform/{mod,macos,windows,linux}.rs` | 新建：平台原生导航与 capability |
| `src-tauri/src/commands/browser.rs` | 新建：可信 UI command 入口 |
| `src/components/browser/{protocol,store,api,migration}.ts` | 新建：wire、投影、invoke、旧 tab 迁移 |
| `src/components/browser/{BrowserProvider,BrowserWorkbench,BrowserToolbar,BrowserGroupStrip,PreviewSurface}.tsx` | 新建：完整工作台与 Preview surface |
| `src/components/browser/hooks/{usePreviewLifecycle,usePreviewBounds}.ts` | 新建：提取既有 create/close/几何/cover |
| `src/lib/api/browser.ts` | 新建：业务 API barrel，复用领域 api 实现 |
| `src/components/{EmbeddedBrowser.tsx,side-workbench/BrowserTab.tsx,side-workbench/SideTabBody.tsx}` | 修改：兼容装配，不新增第二份真相 |
| `src-tauri/src/commands/session_lifecycle.rs` | 新建：从 `session_p1.rs` 抽出本次涉及路由 |
| `tests/browser-preview/`、`scripts/browser/run-preview-fixtures.mjs` | 新建：UI 与原生 fixture 执行、证据收集 |

现有证据：EmbeddedBrowser 1,070 行；commands/session_p1.rs 1,178 行；BrowserTab 内部状态 10 余项；`side_browser_host.rs` 的 pending 下载以 URL 为 key。实现前重新记录基线。测试文件下文均为计划新增，现有 smoke 路径另行注明。

## 任务清单

### M0-W01：公共模型与幂等迁移

**依赖：** 无。

**负责：** Host/前端领域工程师。

**估算：** 2–3 工程日。

**文件：** 创建 `src-tauri/src/browser/{mod,protocol,lifecycle,persistence}.rs`、`src/components/browser/{protocol,migration}.ts`、`src/components/browser/migration.test.ts`、`tests/browser-contracts/v1.json`；修改 `src-tauri/src/lib.rs`、`src/lib/sideWorkbench.ts`。

- [ ] 在 migration.test.ts 写两个独立 owner、相同 URL、多次迁移用例；Rust 对同一 JSON fixture 验证 wire 字段和无效 revision。
- [ ] 运行 `pnpm exec vitest run src/components/browser/migration.test.ts` 与 `cargo test --manifest-path src-tauri/Cargo.toml browser::protocol::`，确认失败指向未实现的协议/迁移，而非依赖安装问题。
- [ ] 实现 Host UUID/owner 校验、原子迁移记录、SideTab 可选 browserSessionId 和 legacy backup；metadata 写失败不替换 UI 引用。
- [ ] 实现 migration index 重建及 checkpoint 幂等；禁止用 URL 作去重键，未归属旧行只接受可信用户认领。
- [ ] 运行新增测试和现有 `pnpm exec vitest run src/lib/sideWorkbench.test.ts src/lib/sideWorkbenchProject.test.ts`，核对非浏览器行序列化不变。
- [ ] 提交 `feat(browser): add owned browser session model`，同轮推送并记录 fixture digest；不提交真实会话数据。

迁移的最小 API 与测试合同：

```ts
type LegacyBrowser = { id: string; url: string };
type MigrationKey = { appSessionId: string; legacyId: string; version: 1 };
const key = (owner: string, row: LegacyBrowser): MigrationKey =>
  ({ appSessionId: owner, legacyId: row.id, version: 1 });
const row = { id: "side-1", url: "https://example.test/" };
expect(key("app-a", row)).not.toEqual(key("app-b", row));
expect(key("app-a", row)).toEqual(key("app-a", row));
```

验收：重复导入不新增 UUID；owner 不匹配拒绝；模拟原子写失败不丢原始字段。测试需调用真实 migration service，不以以上纯 key 演示替代持久化测试。

### M0-W02：提取 WebView 生命周期与遮挡

**依赖：** `M0-W01`。

**负责：** 前端原生窗口集成工程师。

**估算：** 3–5 工程日。

**文件：** 创建 `src/components/browser/hooks/{usePreviewLifecycle,usePreviewBounds}.ts`、`src/components/browser/PreviewSurface.tsx`、`src/components/browser/hooks/usePreviewLifecycle.test.tsx`；修改 `src/components/EmbeddedBrowser.tsx`；测试复用 `src/lib/nativeWebviewBounds.test.ts` 与 `src/lib/nativeWebviewCover.test.ts`（实现前若现有路径调整，按实际文件定位）。

- [ ] 捕获当前 StrictMode 双挂载、150ms 延迟 close、15s create timeout、45s load timeout 和 trailing single-flight 行为；写 timer-controlled 测试验证快速隐藏/重新显示不会重复 create。
- [ ] 跑 `pnpm exec vitest run src/components/browser/hooks/usePreviewLifecycle.test.tsx`，确认故障 fixture 能复现重挂载误关或覆盖释放缺口。
- [ ] 从 EmbeddedBrowser 提取 create/navigation/listener cleanup 与 bounds/cover 两类 hook；保留旧 export/props，让旧消费者继续工作。
- [ ] 采用 Host binding generation 丢弃晚回调；cover 状态先 hide 后刷新几何，恢复时先有效 bounds 后 show；卸载释放全部 subscription/timer。
- [ ] 跑新增测试及 nativeWebview 相关现有测试；统计 EmbeddedBrowser 小于 700 行，新文件各小于 700 行，千行文件至少减少 1。
- [ ] 提交 `refactor(browser): isolate preview lifecycle and bounds` 并推送；附 20 次快速显隐/拖拽的 native fixture 要求供 W06 验证。

几何请求应保留现有单飞语义：

```text
resize event -> replace pending rect
if request running: return
while pending rect exists:
  rect = take latest pending
  if covered or stale binding: hide and stop
  await setPosition(rect.position)
  await setSize(rect.size)
  if binding still current and not covered: show
```

验收：两个 overlay 依次关闭时 cover 仍遵守引用数；释放函数幂等；旧 binding 的 create 返回后不能重新显示已关 WebView。

### M0-W03：Host Preview 导航与下载身份

**依赖：** `M0-W01`、`M0-W02`。

**负责：** Rust/平台 WebView 工程师。

**估算：** 5–8 工程日，包含真实下载对象与 Wry delegate 接入验证。

**文件：** 创建 `src-tauri/src/browser/preview/{mod,registry,navigation,downloads,tests}.rs`、`src-tauri/src/browser/preview/platform/{mod,macos,windows,linux}.rs`、`src-tauri/src/commands/browser.rs`；修改 `side_browser_host.rs`、`side_browser_blob.rs`、`commands/{mod,terminal}.rs`、`lib.rs`。原生 hook 不足时在本任务评审最小 `src-tauri/Cargo.toml`/`Cargo.lock` 固定适配补丁并记录来源，不以版本漂移绕过验证。

- [ ] 写重定向 A->B 后晚到 A 回调、同 URL 双下载逆序结束、macOS path=None、不同 tab 相同 generation/token、关闭后 callback、popup owner 未确定的用例；原生 fixture 增加 back/forward/stop/title。
- [ ] 运行 `cargo test --manifest-path src-tauri/Cargo.toml browser::preview::`，确认各失败对应真实行为约束。
- [ ] 实现 Host registry 与导航 revision；可信 UI command 只接受 Host UUID，旧 side_browser 命令转发 adapter 并继续验证 label/URL。
- [ ] 为 WKWebView/WebView2 使用原生历史/停止/标题入口，Linux capability 按实现证据声明；原生回调按 generation 校验，不用任意 eval 猜结果。
- [ ] 在 WKDownload/WebView2 DownloadOperation 原生对象上建立 Host downloadId 映射并保有对象至结束；验证既有 Wry delegate/导航兼容，不能仅用 Tauri Finished 的 URL/path 推断身份。覆盖同 URL 与 blob/data 独立 token；身份或保存失败明确报错。
- [ ] 运行 Rust 新增/现有 side_browser 测试、fmt 与 clippy；提交 `feat(browser): add revisioned preview navigation` 并推送。

下载关联约束：

```rust
#[derive(Debug, PartialEq, Eq, Hash)]
struct TransferKey {
    binding_id: String,
    tab_id: String,
    binding_generation: u32,
    native_transfer_token: String,
}
// Token comes from a retained native download object, not the URL-only callback.
```

验收：用户点停止后原生导航停止，而不只是 spinner 消失；title 与 committed URL 都来自后端；一个下载结束不会消费另一个 pending 记录。

### M0-W04：工作台投影、任务分组与完整交互

**依赖：** `M0-W03`。

**负责：** React/UX 工程师。

**估算：** 4–6 工程日。

**文件：** 创建 `src/components/browser/{store,api}.ts`、`src/components/browser/{BrowserProvider,BrowserWorkbench,BrowserToolbar,BrowserGroupStrip,BrowserStateView}.tsx`、`src/components/browser/BrowserWorkbench.test.tsx`、`src/lib/api/browser.ts`；修改 `src/lib/api/index.ts`、`src/components/side-workbench/{BrowserTab,SideTabBody}.tsx`、`src/styles/side-workbench.css`（按当前实际同域 CSS 文件接入）。

- [ ] 写按 AppSession 切换不串 tab、旧 boot/generation 事件洪泛直接丢弃、真实缺号合并重取 snapshot、IME Enter、刷新草稿不提交、关闭失败保留 tab 的交互测试。
- [ ] 运行 `pnpm exec vitest run src/components/browser/BrowserWorkbench.test.tsx` 并逐个确认失败。
- [ ] 实现 Host store + useSyncExternalStore/provider，将地址草稿与 committed URL 分离，渲染 tab strip、导航、状态和后端能力。
- [ ] 接回 Design Mode/SSH 预览/外部打开，所有 busy/error/empty/unsupported 路径可操作；任务组 close 与 pane hide 走不同 action。
- [ ] 在 `src/i18n/messages/en/` 对应领域新增 keys，同步全部 15 目录；复用 tooltip/Select/ContextMenu/native-cover，补 roving focus 与关闭后焦点恢复。
- [ ] 运行交互测试、`pnpm typecheck`、`pnpm lint`，提交 `feat(browser): add preview task workbench` 并推送。

事件投影接受规则：

```ts
type Stamp = { boot: string; binding: number; seq: number };
function classify(prev: Stamp, next: Stamp, authenticatedBoot: string): "drop" | "apply" | "snapshot" {
  if (next.boot !== authenticatedBoot) return "drop";
  if (prev.boot !== authenticatedBoot) return "snapshot";
  if (next.binding < prev.binding) return "drop";
  if (next.binding > prev.binding) return "snapshot";
  if (next.seq <= prev.seq) return "drop";
  return next.seq === prev.seq + 1 ? "apply" : "snapshot";
}
```

验收：authenticatedBoot 只从认证连接/快照更新，event 不能更换它；同一 tab 的 snapshot 请求合并，等待时丢弃旧事件并有界保留新事件。连续 1,000 条旧 boot/generation 回调触发零重取，可信新 boot 或真实缺号才重取。视口 390px 不出现横向页面滚动，原生 surface 不覆盖菜单。

### M0-W05：会话生命周期与兼容清理

**依赖：** `M0-W01`、`M0-W04`。

**负责：** Host 会话工程师。

**估算：** 4–6 工程日。

**文件：** 创建 `src-tauri/src/session_lifecycle.rs`、`src-tauri/src/commands/session_lifecycle.rs`、`src-tauri/src/browser/lifecycle_tests.rs`；修改 `commands/{mod,session_p1}.rs`、`browser/lifecycle.rs`、`session_manager/journal.rs`、`ssh_remote/fs_sessions.rs`、`automation_runner.rs`、`store.rs`、`lib.rs`、`src/hooks/useSideWorkbenchProjectIsolation.ts`；测试 `src/components/browser/lifecycle.test.ts` 与既有 SSH/自动化 Rust 用例。

- [ ] 写 archive/delete/fork/rewind、SSH rail 删除已导入会话、自动化创建失败清理、批量部分失败、shared Runtime ref、cleanup 写失败用例，确认删除不能早于 browser cleanup 完成。
- [ ] 跑 `cargo test --manifest-path src-tauri/Cargo.toml browser::lifecycle` 与 `pnpm exec vitest run src/components/browser/lifecycle.test.ts`，定位失败路径。
- [ ] 抽出经手 commands 并保留 invoke 名称；所有生产 `store::delete_session` 调用迁入共用 `src-tauri/src/session_lifecycle.rs` 服务，store 删除要求 owner/revision 绑定的清理许可，Host 在持久化删除前关闭 admission/fence，空引用也由协调服务证明。
- [ ] fork 建新空 Session、rewind 不还原外部页面、归档不删除 Profile；批量按 owner 收集部分失败，不把其中一项失败变成整体已删除。
- [ ] 保留非浏览器项目 stash；全部生命周期触发浏览器投影更新，后台事件不能写入当前别的 AppSession。
- [ ] 跑相关现有 session fork/rewind 测试和代码质量门，要求 session_p1.rs 小于 1,000 行且总千行文件不高于 77；提交 `feat(browser): bind groups to app session lifecycle` 并推送。

顺序必须由故障注入验证：

```text
close admission -> fence -> revoke bindings/grants -> cancel transfers
-> persist closing/tombstone -> close owned tabs -> release runtime reference
-> verify cleanup -> publish closed -> complete AppSession deletion
```

验收：中途磁盘写失败不恢复动作许可；未清理资源保持 closing/degraded；不会删除项目/命名 Profile，也不会误关其他任务标签。

### M0-W06：Preview 验收、证据与灰度

**依赖：** `M0-W02`、`M0-W03`、`M0-W04`、`M0-W05`。

**负责：** QA/前端/平台工程师。

**估算：** 3–4 工程日。

**文件：** 创建 `tests/browser-preview/{server,scenarios,ui.spec}.ts`、`tests/browser-preview/playwright.config.ts`、`scripts/browser/run-preview-fixtures.mjs`、`docs/qa/browser-rearchitecture/m0-preview/acceptance.md`；修改 `package.json`、`docs/llm-wiki/` 对应规则页。

- [ ] 建立确定性本地站点，包含 redirect、slow navigation、popup、download duplicate、blob export、IME、cross-origin frame；harness 明确区分 UI mock 与 native WebView 证据。
- [ ] 在 UI runner 加入五语言、1440x900/390x844、亮暗主题截图和 bounding-box 检查，运行 `pnpm exec playwright test --config tests/browser-preview/playwright.config.ts`；此命令由本任务安装固定开发版本后可用。
- [ ] 在 macOS/Windows 真机运行 runner，验证 back/forward/stop、非空 native surface、菜单遮挡、20 次快速显隐与下载；runner 使用仅测试构建启用的本地诊断通道，不给生产网页新增 IPC。
- [ ] 运行 `pnpm typecheck`、`pnpm test`、`pnpm lint`、`pnpm build:ui`、`python3 scripts/check-code-quality-gates.py --mode final` 和 Rust fmt/clippy/test；全部通过才记录本包 PASS。
- [ ] 写 acceptance：commit、OS/架构、fixture digest、逐用例结果、截图 hash、迁移前后计数、回滚演练；未跑平台写 not_run，不用 mock 代替真机。
- [ ] 仅通过的平台开放 internal `browser.previewV2`；提交 `test(browser): verify preview task workflows` 并推送，按维护规则先本地 CI 后提出 PR。

证据最小结构：

```json
{"milestone":"M0-W06","kind":"native-webview","status":"not_run","cases":[],"artifacts":[]}
```

验收：M0 Preview 完整流程和仓库质量门均通过；如果 Runtime PoC 未通过，Preview 可保持内部可用，M1 保持未开始。没有真实输入/窗口权限证据时不得把整个浏览器路线标为完成。

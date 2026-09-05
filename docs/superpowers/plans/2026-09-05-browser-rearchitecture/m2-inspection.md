# M2 检查与命名 Profile 实现计划

> **面向 AI 代理的工作者：** 使用 subagent-driven-development 或 executing-plans 逐任务实现。步骤使用未勾选复选框追踪；本文件记录未执行待办，当前文档交付不代表产品完成。实施期间的 commit/push 遵循总索引。

**目标：** 完成 Console/Network/Grok Trace、真实截图、多选标注、响应式验证和 named Profile 管理的可用闭环。
**架构：** Rust Host 保有身份、来源、权限和 Artifact 真相；固定 Playwright Worker 提供受限原语。React 域模块消费有界投影，人工检查 permit 与 Agent lease 分离。
**技术栈：** Rust/Tauri 2、React 19/TypeScript、现有 Vitest/Testing Library、固定 Playwright、JSON/ZIP 标准解析。
**规格：** [M2](../../specs/2026-09-05-browser-rearchitecture/m2-inspection.md)、[共用合同](../../specs/2026-09-05-browser-rearchitecture/00-contracts.md)。
**进入门：** M1-G06、M1-R06、M1-D06；总工作量预估 24..37 工程日，不含外部签名审核等待。
**验证边界：** 下列新路径、脚本、fixture 和 QA 记录均为计划创建，当前尚未运行；不把计划预期当作 pass。

## 代码证据与文件职责

| 文件 | 处理方式与责任 |
| --- | --- |
| `src/components/side-workbench/BrowserTab.tsx`、`BrowserDesignModePanel.tsx` | 现有单选和草稿入口；只改委托，不增加大块领域状态。 |
| `src/hooks/useBrowserDesignMode.ts`、`src/lib/browserDesignMode.ts` | 现有 180 ms eval 轮询和 Canvas/foreignObject 合成图；留给 Preview 兼容，M2 不调用该截图链。 |
| `src-tauri/src/browser/inspection/` | 新建有界订阅、脱敏、人工 permit、Trace 格式和导出校验。 |
| `browser-runtime/worker/src/inspection.ts`、`screenshots.ts`、`design.ts` | 新建 Worker 采集和 Playwright 原语；共用 Worker 根由 M1-R 提供。 |
| `src/components/browser/inspection/`、`src/components/browser/profiles/` | 新建人类面板、列表状态、选区/验证和 Profile 管理 UI。 |
| `src-tauri/src/browser/profiles/named.rs` | 新建命名/绑定/删除服务，复用 M1 的 ProfileGuard 与持久化。 |
| `src/lib/settingsCatalog/entries/runtime.ts`、`src/components/settings/RuntimeSection.tsx` | 修改登记与轻量挂载，使用现有 runtime/tools/privacy tab。 |
| `src/i18n/messages/<locale>/features.ts`、`settings.ts` | 修改所有 15 语言；`<locale>` 明确指 `de,en,es,fil,fr,id,it,ja,ko,pt-BR,ru,ta,uk,zh,zh-TW`。 |
| `tests/browser/m2/`、`docs/qa/browser-rearchitecture/m2/` | 新建真实包场景与证据；共用 `tests/browser/playwright.config.ts` 来自 M1-D04。 |

共用 Host 文件为 M1 的 `protocol.rs`、`gateway.rs`、`origin.rs`、`binding.rs`、`policy.rs`、`lease.rs`、`lifecycle.rs`、`persistence.rs`，均位于 `src-tauri/src/browser/`。
Agent live-tab wire 使用共用 envelope/result；人工检查走可信 Tauri `UserInspectionPermit`，不伪造 AgentBinding/turn/lease。本文 JSON 是 scenario fixture，由用例构造对应入口的完整请求，不能作为少字段的生产请求。
前端根测试仅包含 `src/**`，Worker 使用独立 `vitest.config.ts`；两套测试都必须报告非零用例。
任务依赖：M2-01 -> M2-02/M2-03 -> M2-04；M2-05 可在进入门后独立进行；M2-06 汇总验收。

### M2-01：有界检查流、来源校验与脱敏

**依赖：** M1-G06、M1-R06、M1-D06。
**负责：** Host/安全工程师，Worker 工程师复核数据出口。
**估算：** 4..6 工程日。
**文件：** 新建 `src-tauri/src/browser/inspection/{mod.rs,subscription.rs,redaction.rs,tests.rs}`、`src/components/browser/inspection/{types.ts,redaction.ts,redaction.test.ts}`、`browser-runtime/worker/src/{inspection.ts,inspection.test.ts}`；修改 M1 新增的 `src-tauri/src/browser/gateway.rs`、`browser-runtime/worker/vitest.config.ts`；新建 `tests/browser/fixtures/m2/inspection.json`。

- [ ] **步骤 1：固定拒绝与限流 fixture。** 在 Host 测试中从相同 Runtime 的两个 BrowserSession 生成请求，验证订阅不能跨 Session；加入下列合成网络输入及预期。

```json
{"case":"foreign-body","topOrigin":"https://app.fixture.test","requestOrigin":"https://third.fixture.test","allowedOrigins":["https://app.fixture.test"],"expectedReasonCode":"origin_denied","expectedBodyBytes":0}
```

- [ ] **步骤 2：建立能发现 Worker 测试的红灯。** 在独立 Worker 配置声明 `include:["src/**/*.test.ts"]`，运行 `cargo test --manifest-path src-tauri/Cargo.toml browser::inspection::tests -- --nocapture` 和 `pnpm --dir browser-runtime/worker exec vitest run --config vitest.config.ts src/inspection.test.ts`；预期因尚无订阅/脱敏实现失败，不能为“未发现测试”。

- [ ] **步骤 3：实现入口数据合同与强制脱敏。** 以 Host binding 计算 top/frame/producer/request origin；不执行 console getter，不持有可继续求值的 remote object；参数接口如下。

```ts
export type InspectionBudget = { maxEvents: number; maxBytes: number; maxBatchBytes: number };
export const defaultInspectionBudget: InspectionBudget = { maxEvents: 5000, maxBytes: 8 * 1024 * 1024, maxBatchBytes: 256 * 1024 };
export type RedactedHeaders = Readonly<Record<string, string>>;
export function sanitizeNetworkHeaders(input: Record<string, string>): RedactedHeaders;
```

实现 `sanitizeNetworkHeaders` 时用标准 header 名归一化和 allowlist；Authorization/Cookie/Set-Cookie/代理凭据不进入输出。正文要求独立许可、文本 MIME 与解码/大小预算，未知结构 withheld。

- [ ] **步骤 4：接入订阅生命周期。** Host sequence 和 ring 独立于 React，批量上限 128 条；policy/turn/backend/接管变化撤销 Agent 订阅，返回 gap/drop 计数。人工 `UserInspectionPermit` 由可信 Tauri 入口请求，先设置 TakeoverLatch 并等待 Agent 取消，再限发起面板/Session/Tab/binding/nav revision/来源/有效期，绝不派生 Agent lease；操作集合仅允许检查读取、截图及固定 viewport/overlay 原语。

```json
{"case":"takeover-subscription","initialFenceEpoch":7,"event":"nativeInput","expectedFenceEpoch":8,"expectedAgentEventsAfterFence":0,"expectedUserInspectionNeedsExplicitPermit":true}
```

- [ ] **步骤 5：验证秘密与边界。** 运行 `pnpm test -- src/components/browser/inspection/redaction.test.ts` 及步骤 2 两条命令；预期三组 PASS，包含 5,001 条 overflow、256 KiB 批界、1 MiB body、跨域 iframe、循环对象/BigInt/getter 和旧 nav revision，getter 计数为零。

- [ ] **步骤 6：交付采集合同。** 将 reasonCode、预算、来源判定与暂停行为记录到新增 `docs/qa/browser-rearchitecture/m2/inspection-contract.md`；验收要求未经许可的正文/秘密字节零输出，取消后无新增事件，磁盘失败为 degraded，后续任务可以使用真实订阅接口。

**验收：** Agent 与人工检查权限相互独立，错误来源/过期身份/超限数据不能进入任何消费者。

### M2-02：Console 与 Network 完整面板

**依赖：** M2-01。
**负责：** 前端工程师，Host 工程师联调详情与导出。
**估算：** 3..5 工程日。
**文件：** 新建 `src/components/browser/inspection/{InspectionPanel.tsx,ConsolePanel.tsx,NetworkPanel.tsx,inspectionStore.ts,inspectionStore.test.ts,InspectionPanel.test.tsx,inspection.css}`；修改 `src/components/side-workbench/BrowserTab.tsx` 与 `src/i18n/messages/<locale>/features.ts`；新建 `tests/browser/m2/inspection-panels.spec.ts`、`tests/browser/fixtures/m2/network.html`。

- [ ] **步骤 1：写列表/详情失败用例。** 用 Testing Library 渲染已连接但无事件、筛选无结果、gap、withheld、paused 和 disconnected；模拟同 URL 两次请求必须显示两个稳定 ID。

```json
{"case":"duplicate-url-rows","requests":[{"networkRequestId":"request-a","displayUrl":"https://app.fixture.test/items","status":200},{"networkRequestId":"request-b","displayUrl":"https://app.fixture.test/items","status":503}],"expectedRenderedRows":2,"filterStatus":503,"expectedVisibleRequestIds":["request-b"]}
```

- [ ] **步骤 2：确认交互红灯。** 运行 `pnpm test -- src/components/browser/inspection/inspectionStore.test.ts src/components/browser/inspection/InspectionPanel.test.tsx`；预期组件不存在或筛选/聚焦断言失败，用例必须实际执行。

- [ ] **步骤 3：实现可扫描面板。** Console 完成级别/文本/frame筛选、重复折叠、导航分隔、无 getter 详情；Network 完成排序、redirect chain、timing、允许 headers/body、状态与错误。store 仅保存脱敏投影，按 eventId 去重并校验 sequence。

```ts
export type InspectionView = "console" | "network" | "trace" | "design";
export type NetworkSort = { field: "startedAt" | "status" | "durationMs" | "bytes"; descending: boolean };
export type InspectionFilter = { text: string; frameId: string | null; levels: readonly string[] };
```

- [ ] **步骤 4：接入真实任务区和本地化。** 复用 Tip/icons、Select、ContextMenu、portal/native cover；支持紧凑双栏/窄屏详情替换、箭头/Enter/Escape、焦点恢复和节流 aria-live。新增 15 语言 keys，复制/导出只调用脱敏视图，清空不删除持久 Trace。

```json
{"case":"narrow-panel","paneWidth":360,"fontScale":2,"locale":"de","actions":["openNetwork","filterStatus503","openDetails","escape"],"expectedFocus":"request-b","expectedHorizontalOverflow":false}
```

- [ ] **步骤 5：跑组件和真实页面。** 运行步骤 2 命令、`pnpm test -- src/i18n/messages.test.ts`、`pnpm exec playwright test --config tests/browser/playwright.config.ts tests/browser/m2/inspection-panels.spec.ts`；预期 PASS，fixture 产生 redirect/cache/失败/流式/WebSocket 元数据，非法 body 仍为 withheld。

- [ ] **步骤 6：交付业务与视觉闭环。** 新增 `docs/qa/browser-rearchitecture/m2/panels.md` 记录浅/深色、360 px pane、200% 字号、键盘/读屏与遮挡截图；全部按钮有实际状态变更，unsupported 不显示可点空壳，关闭面板只停订阅。

**验收：** 两类面板从真实流进入列表、筛选、详情、复制/导出和错误恢复，秘密字节不会经任一出口出现。

### M2-03：Grok Trace、真实截图与安全导出重开

**依赖：** M2-01。
**负责：** Worker/Artifact 工程师，前端工程师完成 Trace UI。
**估算：** 5..7 工程日。
**文件：** 新建 `src-tauri/src/browser/inspection/{trace.rs,trace_archive.rs,trace_tests.rs}`、`browser-runtime/worker/src/{screenshots.ts,screenshots.test.ts}`、`src/components/browser/inspection/{TracePanel.tsx,TracePanel.test.tsx,ScreenshotReview.tsx}`；修改 M1 Artifact 模块与 `src/i18n/messages/<locale>/features.ts`；新建 `tests/browser/m2/trace-screenshots.spec.ts`、`tests/browser/fixtures/m2/capture.html`。

- [ ] **步骤 1：定义版本化 Trace 与攻击归档用例。** 创建 schema、路径穿越、重复 zip entry、超限解压、未知版本和秘密字段 fixture；同一用例覆盖导出再重开。

```json
{"format":"grok-browser-trace/v1","traceId":"trace-fixture","events":[{"eventId":"event-1","sequence":1,"type":"action","outcome":"completed"}],"artifacts":[],"gaps":[]}
```

- [ ] **步骤 2：确认格式与像素红灯。** 运行 `cargo test --manifest-path src-tauri/Cargo.toml browser::inspection::trace_tests -- --nocapture`、`pnpm --dir browser-runtime/worker exec vitest run --config vitest.config.ts src/screenshots.test.ts`；预期 exporter/capture 未实现而失败。fixture 中 canvas 色块和字体必须用于像素检查，不能只断言文件非空。

- [ ] **步骤 3：实现实际浏览器捕获和 Trace 归一化。** 截图调用固定 Playwright API，存储 M1 Artifact；Trace 采集 Host 动作/脱敏观察引用，禁止从任意 Playwright zip 直通普通 Artifact。

```ts
const bytes = await page.screenshot({ type: "png", fullPage: false, animations: "disabled" });
const dimensions = { width: 1280, height: 720, deviceScaleFactor: 1 };
// page is the currently authorized M1 Worker page; recheck the fence after await.
```

接入 viewport/fullPage/element/crop，保留 DPR、nav revision、hash 与 mask；不受允许的跨域 frame 整块遮罩。标准 ZIP 库仅写 schema allowlist，拒绝未知字段/格式的原始 Trace，失败删除 quarantine。

- [ ] **步骤 4：实现完整记录与预览交互。** Trace 支持开始/暂停/停止/筛选/定位/导出/重开；截图支持真实预览和人工不可逆遮罩。用户确认后才提升到 Composer/导出，UI 可取消 finalizing，接管暂停的段不会自动继续。

```json
{"case":"trace-roundtrip","operations":["record","capture","pause","stop","export","reopen"],"expectedEventCount":12,"expectedRawHeaders":0,"expectedMissingArtifactCount":0}
```

- [ ] **步骤 5：验证真实像素和秘密字节。** 运行步骤 2 两条命令、`pnpm test -- src/components/browser/inspection/TracePanel.test.tsx`、`pnpm exec playwright test --config tests/browser/playwright.config.ts tests/browser/m2/trace-screenshots.spec.ts`；预期全部 PASS，canvas/字体/sticky/滚动裁剪像素符合 fixture，ZIP 中敏感哨兵出现次数为零。

- [ ] **步骤 6：交付可重开的 Trace。** 新增 `docs/qa/browser-rearchitecture/m2/trace-format.md` 和 `trace-export.md`，记录 schema、预算、像素 hash、恢复/攻击结果；原始 Trace 默认不采集，未经结构化重建不能导出，不承诺 Playwright Viewer 兼容。

**验收：** Grok Trace 录制到导出重开完整通过，真实截图像素与遮罩正确，解压预算及恶意归档均有明确拒绝证据。

### M2-04：Design Mode v2、多选标注与响应式验证

**依赖：** M2-02、M2-03。
**负责：** 前端/Worker 工程师，安全工程师审查人工 permit。
**估算：** 5..8 工程日。
**文件：** 新建 `src/components/browser/inspection/{DesignPanel.tsx,selectionReducer.ts,selectionReducer.test.ts,ResponsivePanel.tsx,ResponsivePanel.test.tsx,validation.ts}`、`browser-runtime/worker/src/{design.ts,design.test.ts}`；修改 `src/components/side-workbench/BrowserDesignModePanel.tsx`、`BrowserTab.tsx` 和 `src/i18n/messages/<locale>/features.ts`；新建 `tests/browser/m2/design-responsive.spec.ts`、`tests/browser/fixtures/m2/{design.html,frame.html}`。

- [ ] **步骤 1：建立选区过期与 viewport 矩阵。** 写 reducer 的替换/增减/排序/撤销/重做/20 项上限、部分失效仍保留说明，以及导航不能重定位旧 ElementRef。

```json
{"case":"stale-selection","selectedAt":{"navRevision":8,"viewportRevision":1},"current":{"navRevision":9,"viewportRevision":1},"expectedReasonCode":"stale_nav_revision","expectedRetainedAnnotation":"Adjust spacing"}
```

- [ ] **步骤 2：验证失败路径存在。** 运行 `pnpm test -- src/components/browser/inspection/selectionReducer.test.ts src/components/browser/inspection/ResponsivePanel.test.tsx` 和 `pnpm --dir browser-runtime/worker exec vitest run --config vitest.config.ts src/design.test.ts`；预期当前没有多选、人工 permit 或恢复 viewport，因此断言失败。

- [ ] **步骤 3：实现人工检查和稳定 ElementRef。** 从可信 App 的 Tauri 入口申请 `UserInspectionPermit`，先 fence Agent 并等待取消完成；固定 overlay 只给候选节点，Host/Worker重新验证 frame、origin、revision，安全提取样式与文本。测试 permit 可设置/恢复 viewport 与安装/移除 overlay，但不能执行一般导航、点击、填表或开启 Agent 观察；结果仅到申请面板。

```ts
export type SelectionItem = { elementRefId: string; ordinal: number; annotation: string; state: "valid" | "stale" };
export type ViewportSpec = { width: number; height: number; deviceScaleFactor: number };
export const validationViewports: ViewportSpec[] = [{ width: 375, height: 812, deviceScaleFactor: 2 }, { width: 1280, height: 720, deviceScaleFactor: 1 }];
```

- [ ] **步骤 4：实现完整用户闭环。** 单击/Shift、多选列表、键盘树、逐项标注到 Composer 草稿；响应式宽高/DPR/preset/旋转，串行最多 8 viewport，有界稳定等待、截图/基准/差异、溢出/重叠断言和恢复原 viewport。用户选择基准，不自动接受失败结果。

```json
{"case":"responsive-overflow","viewport":{"width":375,"height":812,"deviceScaleFactor":2},"fixtureElementWidth":420,"expectedOverflow":true,"expectedRunState":"completed","expectedAssertionStatus":"failed"}
```

- [ ] **步骤 5：运行 frame、像素与接管验收。** 跑步骤 2 两条命令、`pnpm exec playwright test --config tests/browser/playwright.config.ts tests/browser/m2/design-responsive.spec.ts`；预期 PASS，覆盖同源/获双授权跨域 iframe、open shadow、closed shadow unsupported、动画、停机和导航中断。人工读操作不会打开 Agent 事件流，发送草稿不清除 latch。

- [ ] **步骤 6：交付响应式报告。** 新增 `docs/qa/browser-rearchitecture/m2/design-responsive.md`，含原图/基准/差异及判定、viewport 恢复失败回路、200% 缩放键盘路径；保留旧 Preview 单选草稿兼容，旧合成图不得当作真实截图迁移。

**验收：** 多选说明进入草稿，失效选区不能被操作；响应式运行可停止、比较并恢复 viewport，人类检查不恢复 Agent 控制。

### M2-05：Named Profile 创建、复用、绑定与删除

**依赖：** M1-G06、M1-R06、M1-D06。
**负责：** Host/Profile 工程师，设置 UI 工程师。
**估算：** 4..6 工程日。
**文件：** 新建 `src-tauri/src/browser/profiles/{named.rs,named_tests.rs}`、`src/components/browser/profiles/{NamedProfilesPanel.tsx,ProfilePicker.tsx,NamedProfilesPanel.test.tsx}`；修改 M1 Profile 服务、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src/components/settings/RuntimeSection.tsx`、`src/lib/settingsCatalog/entries/runtime.ts`、`src/i18n/messages/<locale>/settings.ts`；新建 `tests/browser/m2/named-profiles.spec.ts`。

- [ ] **步骤 1：写名称、隔离与删除失败用例。** 覆盖 trim/NFC/casefold重名、UUID目录、不同项目显式复用、active ref/锁竞争、解绑为 orphan、静态凭据保护关闭与 epoch 不兼容。

```json
{"case":"delete-active-named-profile","kind":"named","projectBindings":["project-a","project-b"],"activeReferences":2,"expectedReasonCode":"profile_in_use","expectedDeletedDirectories":0}
```

- [ ] **步骤 2：运行红灯验证。** 运行 `cargo test --manifest-path src-tauri/Cargo.toml browser::profiles::named_tests -- --nocapture` 和 `pnpm test -- src/components/browser/profiles/NamedProfilesPanel.test.tsx`；预期命名服务/管理 UI 缺失而失败，不能通过 mock 把删除直接设为成功。

- [ ] **步骤 3：实现原子元数据和切换。** 名称 1..64 字符，路径只用 UUID；创建空 Profile，不导入外部状态。修改 project bindings 前确认共享站点状态，切换活跃 Session 时先 fence/drain，递增 backend generation 再安全恢复页面上下文。

```json
{"profileId":"00000000-0000-4000-8000-000000000041","kind":"named","displayName":"QA","projectBindings":["project-a"],"generation":1,"state":"ready"}
```

重命名不移动目录；删除重新核对 ProfileGuard/descendants/references，应用内确认后 tombstone。Profile epoch 迁移保留旧 generation，失败 needs_repair，不 wipe 或合并。

- [ ] **步骤 4：接入设置和任务选择器。** 完成创建/重命名/设默认/绑定/解绑/活动引用/停止使用/诊断/删除，明确共享 Runtime 单 writer；注册保留与管理条目，所有按钮有 busy/error/cancel。

```ts
{ id: "runtime.browser.profiles", section: "runtime", tab: "tools", anchorId: "settings-anchor-browser-profiles", labelKey: "browser.profile.manage", keywords: ["browser", "profile"] }
```

- [ ] **步骤 5：验证真实持久状态。** 跑步骤 2 两条命令、`pnpm test -- src/lib/settingsCatalog.test.ts src/i18n/messages.test.ts`、`pnpm exec playwright test --config tests/browser/playwright.config.ts tests/browser/m2/named-profiles.spec.ts`；预期 PASS。两个项目显式共享 fixture Cookie，但不可读彼此 tab/Artifact，关会话不删 Profile，symlink 根删除被拒绝。

- [ ] **步骤 6：交付可恢复 Profile 管理。** 新增 `docs/qa/browser-rearchitecture/m2/named-profiles.md`，记录重启、磁盘满、锁持有、orphan、epoch 迁移失败和删除中断续删；静态保护未认证平台只能 ephemeral，入口显示能力原因而非成功提示。

**验收：** 命名/绑定/切换/复用/解绑/删除全部落盘可恢复，共享站点状态不扩大跨 Session 权限，锁冲突和失败迁移不损坏数据。

### M2-06：整包跨平台验收与启用门

**依赖：** M2-02、M2-03、M2-04、M2-05。
**负责：** QA/发布工程师，安全与前端工程师独立复核。
**估算：** 3..5 工程日。
**文件：** 新建 `tests/browser/m2/{package-golden.spec.ts,privacy-recovery.spec.ts}`、`scripts/browser/check-m2-evidence.mjs`、`docs/qa/browser-rearchitecture/m2/{README.md,matrix.json,acceptance.md}`；修改 M1-D04 的 `tests/browser/playwright.config.ts` 接入本包场景、M1 feature capability 配置、`docs/llm-wiki/settings-ia.md` 与 `media-delivery.md`。M3-06 已完成时复用其创建的 `tests/browser/m3/m2-inspection-integration.spec.ts`，本包不重复创建该文件。

- [ ] **步骤 1：建立不可假通过的证据清单。** matrix 每格包含平台、真实签名 App/Runtime tuple、fixture revision、命令、开始/结束时间、status 和脱敏 Artifact hash；不允许以 mock、空测试、缺失签名或 planned 状态通过。

```json
{"milestone":"M2","platforms":["macos-arm64","macos-x64","windows-x64"],"required":["panels","trace-roundtrip","real-pixels","design-responsive","named-profile","takeover","privacy","recovery","accessibility"],"status":"planned"}
```

- [ ] **步骤 2：验证验收门先拒绝。** `node scripts/browser/check-m2-evidence.mjs --matrix docs/qa/browser-rearchitecture/m2/matrix.json` 应在 planned/missing/failed 任一格存在时 exit 1，打印缺少的具体证据；为 checker 写合成 pass/fail 自检并拒绝路径穿越记录。

- [ ] **步骤 3：执行自动验收。** 运行以下命令；预期测试非零且 PASS，typecheck/lint exit 0，代码质量门存量失败必须单独记录，不能记整仓通过。

```bash
pnpm test -- src/components/browser/inspection src/components/browser/profiles src/lib/settingsCatalog.test.ts src/i18n/messages.test.ts
pnpm --dir browser-runtime/worker exec vitest run --config vitest.config.ts src/inspection.test.ts src/screenshots.test.ts src/design.test.ts
cargo test --manifest-path src-tauri/Cargo.toml browser:: -- --nocapture
pnpm typecheck
pnpm lint
python3 scripts/check-code-quality-gates.py --mode final
```

- [ ] **步骤 4：执行三个真实安装包矩阵。** 各目标安装 M1-D06 同来源签名包，运行 `pnpm exec playwright test --config tests/browser/playwright.config.ts tests/browser/m2`，并记录 native cover、键盘/读屏、Input Monitoring/Provider 故障和用户接管人工步骤；截图检查 canvas/字体像素、图像尺寸与无重叠。fixture 不能偷偷加载开发 Runtime。M3-06 已完成时另跑 `pnpm exec playwright test --config tests/browser/playwright.config.ts tests/browser/m3/m2-inspection-integration.spec.ts`，验证 Chrome 上的人工 permit、Console/Network/Trace、Design、响应式和 Artifact；M3 后完成时该组合场景由 M3-06 运行，不阻塞 M2 独立交付。

- [ ] **步骤 5：执行故障和保留演练。** Host/Worker 在 Trace finalizing、Profile 切换、Artifact promotion、ZIP重开时退出；验证旧 lease不恢复、未知副作用不重放、24小时清理、原始 quarantine tombstone、用户导出文件不删。把敏感哨兵扫描结果、App壳总行数不增长写入 acceptance。

- [ ] **步骤 6：完成 M2 出口。** 更新 matrix 后重跑 `node scripts/browser/check-m2-evidence.mjs --matrix docs/qa/browser-rearchitecture/m2/matrix.json`，仅全格证据满足时预期 exit 0，再开放 M2 capability。完整 Chrome 检查单独要求 M2-06、M3-06 和组合实证三者满足；缺一保持组合能力关闭，不用 capability 声明代替测试。更新 wiki 的真实注册路径/15语言/Artifact合同；任何隐私、权限或真实包失败都保持本包关闭，交付清楚的失败记录。

**验收：** 三个真实包的声明能力、安全、保留、恢复和无障碍均有可追溯证据，M2-06 才能标记完成。

## 覆盖自检

M2-01 覆盖来源、预算、脱敏与用户检查 permit；M2-02 覆盖完整 Console/Network；M2-03 覆盖 Trace、像素与导出；M2-04 覆盖多选和响应式；M2-05 覆盖 named 管理；M2-06 覆盖迁移、保留、真实包、安全和视觉验收。
总计划恰好 6 个任务，每项 6 个未执行步骤；M2-06 是唯一对外完成出口，M2 不阻塞 M3 开工。

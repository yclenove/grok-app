# 浏览器重构整套设计与开发计划接续记录

日期：2026-09-05

工作分支：`codex/browser-rearchitecture`

调研基线：`a43e3071`。通过 `git worktree list` 定位现有 `browser-rearchitecture` 工作树，继续使用该分支。

## 目标与当前阶段

本工作继承原任务 `01a06b34-0c9f-71d3-a70b-33834648c72c` 的产品选择与全量规划目标，当前目标是“完成整套浏览器重构的计划和设计文档、开发任务”。原有六节讨论和[浏览器重构总体设计](../superpowers/specs/2026-09-04-browser-rearchitecture-design.md)已落实为 M0–M6 十个工作包，每包独立规格与开发计划，共六十项开发任务。

主入口是[实施总索引](../superpowers/plans/2026-09-05-browser-rearchitecture/README.md)。先阅读[共用合同](../superpowers/specs/2026-09-05-browser-rearchitecture/00-contracts.md)，再按[需求与验收矩阵](../superpowers/plans/2026-09-05-browser-rearchitecture/acceptance-matrix.md)核对范围。[任务索引](../superpowers/plans/2026-09-05-browser-rearchitecture/task-index.json)从 Markdown 自动生成，含依赖、角色、工程量和进度。

当前交付是整套设计与开发计划，产品代码尚未实现。开发任务步骤均未勾选，运行证据均未执行；不能把总体设计的文档验收勾选当成浏览器运行测试通过。

## 已确认的产品决定

| 主题 | 决定 |
| --- | --- |
| 架构 | 渐进式双后端，长期保留系统 WebView Preview；Managed Chromium 承担 Agent 自动化。 |
| Runtime 分发 | 随 App 捆绑固定版本 Chromium、Node 和 Playwright；首次使用不在线下载浏览器。 |
| 首批平台 | macOS arm64、macOS x64、Windows x64 同批交付可用 Agent Browser。 |
| 窗口形态 | M1 使用独立可见 Chromium 窗口，App Workbench 同步任务、标签、状态和控制。 |
| 路由 | 按意图与所需能力自动选择；保留显式覆盖，固定后端后不静默切换。 |
| Profile | M1 默认项目独立，支持 ephemeral；模型预留 named，M2 完成命名和显式复用管理。 |
| Chrome | M3 通过扩展显式认领已有标签或任务组，不自动搬运 Cookie、密码或浏览历史。 |
| 权限 | 复用现有会话 policy，Host 补充 origin、动作、文件和接管约束。 |
| Agent 集成 | 第一方 `grok-browser` 经 ACP 会话级 MCP 注入，Rust Host Gateway 为唯一权限源。 |
| 规划与语言 | 正式规格使用中文，保留协议标识；路线覆盖完整 M0–M6。 |

## 本次收敛的合同

- 接管基于 Host 原生输入来源；DOM `isTrusted` 不作为用户来源证据。M0 验证签名安装包中的监听、权限与故障路径。
- Host 单独保存 TakeoverLatch；lease 失效、turn 结束或重连都不能代替用户交还。
- AgentBinding 以连接为生命周期，turn grant 与 thread grant 分开；新 turn 仍重新校验并申请 lease。
- 补回六种现有会话 permission policy 的映射，明确 `dont_ask` 遇到新确认直接拒绝。
- Runtime 防重放 sequence 与目标版本分离；回滚只使用当前签名策略允许且 Profile 兼容的 tuple，不降低防重放记录。
- 离线首启使用包内有效的签名 bootstrap policy；过期时保留 Preview，不承诺无限期离线自动化。
- 200 MiB 路径上传使用 Runtime-owned UploadSpoolLease；attach 返回不等于浏览器读完，副本保留至 Runtime descendants=0，撤权立即停能力，cleanupPending/空间与显式清理在 Doctor 可见。
- 下载 staged metadata 可恢复，但接收和 export 不自动重放；不确定导出进入 `needs_review`。
- 会话关闭/删除先 fence，再持久化关闭状态并释放资源；持久 Profile 的删除单独确认。
- macOS Framework 链接只接受 manifest 声明且目标受限的相对 symlink，兼顾真实包布局与安全提取。
- Tabshare 只作为窗口标签同步的源码证据，不声称它实现了逐标签认领或安全任务组。
- wire 使用 protocolVersion=1，revision/generation/fence 为不回绕 u32；Host IPC 与 Chrome native messaging 由 bridge 转换帧格式。
- M2 人工检查 permit 由可信 App 入口单独授予，先 fence Agent，检查期间仍保持接管锁存和 Agent 观察暂停。
- M2 交付完整 Grok Trace v1 与真实截图/DOM 引用；原始 Playwright Trace 默认隔离，不能未经结构化校验直接导出。
- M3 必须取得逐 claim 下载归属和能力对等证据，人工保存不能替代 Agent 能力验收。
- M4 Wayland 若无法证明输入来源可保持受限 beta，但不能把完整 Linux 对等目标标完成。
- M5 由 Host/Guard 双持有 loopback listener，复用 OpenSSH `ssh -W` 和身份固定的 Bridge 专属 mux；新 tunnel/revision 使用新隔离 Profile，断线后不暗中继承登录态。
- M6 使用 M1 同一 activation/LKG/最高 Runtime sequence 权威，升级 distribution attestation 支持组件授权；CEF 只输出 Go/No-go，不改变生产 Preview。
- M1 App updater 纳入实际 Host/API/hook：可信资产验证后 Browser 专属 quiesce，再安装或 Windows ready/commit 交接；macOS install 返回失败或 Windows handoffAccepted 前失败均保留 ACP/IM，交接接受并退出后的安装器故障只保证记录与修复路径。

## 开发接续顺序

1. 从总索引选择任务，先完成 M0-W01 的公共类型、ownership、持久化与迁移 fixture。
2. M0-W02–06 交付 Preview 2.0；Runtime 线按 R01 -> R04 -> R02 -> R03 -> R05 -> R06 实施，Guard 和 worker 具备后才汇聚完整签名 tuple。
3. 两条 M0 各自过门后推进 M1 Gateway/Runtime/Delivery。M1-D04 创建共用真实包测试 harness；M1-G06/R06/D06 联合完成才开放首批 Agent Browser。
4. M2 检查、M3 Chrome、M4 Linux、M5 SSH 各按独立任务依赖和发布 flag 实施；M6 完成独立更新、性能与 CEF 决策实验。
5. 每项实际开发记录源码、行为测试、命令与退出码，完成后更新复选框与验收记录并重建 task-index。平台或外部实验未运行，保留 not_run，不能勾选产品通过。

实现前核对 `AGENTS.md` 和相关 `docs/llm-wiki/`，特别是 i18n、设置注册、弹层、媒体交付、模型路由、SSH、维护与发布规则。`App.tsx` 与 `AppWorkbench.tsx` 总行数不得增长，新增状态进入领域模块。

另有 `codex/fix-browser-modal-cover` 独立工作树。浏览器重构开始改 UI 前应核对其原生 WebView 遮挡修复是否已进入基线，避免覆盖该任务的工作。

## 验证边界

本轮只交付设计、任务计划、接续记录及文档校验器/派生 JSON。文档结构、引用、关键决策、需求覆盖与依赖图是本轮验证范围；实际 Runtime 打包、输入监听、跨平台浏览器 E2E 和性能目标都是后续开发验收项。本轮不发布版本或合并到主分支。

可复现的文档校验入口：

```bash
node docs/superpowers/plans/2026-09-05-browser-rearchitecture/verify-plan.mjs --write-index
node docs/superpowers/plans/2026-09-05-browser-rearchitecture/verify-plan.mjs
git diff --check
python3 scripts/check-code-quality-gates.py --mode final
```

2026-09-05 文档校验通过：25 份 Markdown、60 项开发任务、360 个任务步骤、24 条需求、23 个依赖层级，工程量合计 241–365 工程日。每包六项任务的元数据、JSON 示例、相对链接、依赖无环性与进入门可达性、需求覆盖、工程量摘要及派生索引均已校验；所有产品任务仍为 not_started，另有十二项工作包交接检查不计入任务步骤。

五项内存反例均按预期拒绝：非法依赖 ID、缺少 M1-D06 发布进入门、任务仅剩五步骤、README 工程量摘要过期、task-index 过期。反例检查不改写文档。实际浏览器、平台输入、安装包和产品测试均未运行，文档校验不能代替这些后续证据。

本次重新执行质量门，唯一失败项仍是 `FILES_OVER_1K_BUDGET`，源码千行文件 79 个，上限 77 个；App shell + AppWorkbench 合计 15,320 行，上限 15,350 行。文档不参与这两项计数，不能宣称整仓全绿或据此创建 PR。M0-W02/W05 将经手的 EmbeddedBrowser 与会话生命周期拆分列入实际开发任务，不提高预算。

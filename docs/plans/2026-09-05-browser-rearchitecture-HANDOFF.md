# 浏览器重构设计接续记录

日期：2026-09-05

工作分支：`codex/browser-rearchitecture`

调研基线：`a43e3071`。通过 `git worktree list` 定位现有 `browser-rearchitecture` 工作树，继续使用该分支。

## 目标与当前阶段

本工作从浏览器调研、选型和全量规划开始，目标是统一手动预览与 Agent 浏览器工作流。原任务的六节设计讨论已经完成；中文书面规格整理期间因服务中断留下未提交草稿。本次接续恢复了用户决策，补齐一致性复核意见，形成可供总体审查的[浏览器重构总体设计](../superpowers/specs/2026-09-04-browser-rearchitecture-design.md)。

当前交付是总体规格，产品代码尚未实现。M0 至 M6 各自需要规格、实现计划和验收记录，不能把总体文档中的验收勾选当成浏览器运行测试已通过。

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
- 上传 staging 独立销毁；已附着文件可能已被站点读取，撤销不声称回收外部字节。
- 下载 staged metadata 可恢复，但接收和 export 不自动重放；不确定导出进入 `needs_review`。
- 会话关闭/删除先 fence，再持久化关闭状态并释放资源；持久 Profile 的删除单独确认。
- macOS Framework 链接只接受 manifest 声明且目标受限的相对 symlink，兼顾真实包布局与安全提取。
- Tabshare 只作为窗口标签同步的源码证据，不声称它实现了逐标签认领或安全任务组。

## 下一阶段

1. 完成总体书面规格审阅，保留已确认的产品选择。
2. 编写 M0 Workbench、任务分组与 Preview 2.0 的独立规格及实现计划。
3. 编写 M0 Runtime 打包、ProfileGuard 和 NativeInputFenceProvider 的可行性计划，列明双平台真机证据。
4. 两条 M0 工作按各自门槛验收，再进入 M1；M2–M6 保持独立范围。

实现前核对 `AGENTS.md` 和相关 `docs/llm-wiki/`，特别是 i18n、设置注册、弹层、媒体交付、模型路由、SSH、维护与发布规则。`App.tsx` 与 `AppWorkbench.tsx` 总行数不得增长，新增状态进入领域模块。

另有 `codex/fix-browser-modal-cover` 独立工作树。浏览器重构开始改 UI 前应核对其原生 WebView 遮挡修复是否已进入基线，避免覆盖该任务的工作。

## 验证边界

本轮只交付 Markdown 设计与接续记录。文档结构、引用、关键决策覆盖和仓库代码质量门是本轮验证范围；实际 Runtime 打包、输入监听、跨平台浏览器 E2E 和性能目标都是后续验收项。本轮未执行产品编译或浏览器运行测试，也未发布版本或合并到主分支。

本轮已执行的验证：

- Markdown 使用仓库现有 `unified` / `remark-parse` / `remark-gfm` 解析；标题层级、表格列数、代码围栏、相对链接、M0–M6、关键合同和占位符检查通过。
- Git whitespace 检查通过；变更仅为总体规格与本记录，没有修改产品源码。
- `python3 scripts/check-code-quality-gates.py --mode final` 未通过：唯一失败项为 `FILES_OVER_1K_BUDGET`，源码千行文件 79 个，上限 77 个。本轮文档不参与该计数，源码与基线相同；该存量问题需后续单独处理，不能宣称仓库全绿或据此创建 PR。
- App shell + AppWorkbench 的 growth freeze 检查通过：总计 15,320 行，上限 15,350 行。

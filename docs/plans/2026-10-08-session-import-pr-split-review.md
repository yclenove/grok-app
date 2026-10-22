# 会话导入：小 PR 审阅单元与依赖草案

## 2026-10-21：本地保存，不是已完成的 PR 栈

用户要求整理并交给Claude，已建 `handoff/session-import-claude-20261021`。
`572ab60a`保存211个跨层代码/测试路径；后续文档提交保存当前入口与历史台账。
这是完整可恢复检查点，未将后端/API/UI/测试拆成未经验证的中间提交，
也未声称该巨大保存提交就是最终上游PR。无push/PR/release。
[交接文档](2026-10-21-session-import-claude-handoff.md)包含最新测试范围和H0–H6。
以下A–L仍为依赖草案；后续需按实际hunk与每个中间树CI重新证明，不能直接外发。
V814限定桌面/重启及闭库审计已完成，唯一用户入口仍V789，原53项未整体闭合。

## 历史规划和证据（版本/未提交/授权描述仅适用于当时）

2026-10-16源码增加历史版本分页、惰性选择及固定revision的阅读/搜索/导出/续聊，
含15语言、迟到响应隔离、失败重试和最新正文不可读时的旧版恢复。7项hook/
4项真实Host集成通过。N732六命令/全库4289pass/0fail/106ignored及strict Clippy、
F737七命令/895文件/10,376项、F735 bundle和V736嵌入构建119.741098s均通过。
中文Windows实测旧新精确URL阅读、旧版搜索/刷新保留、续聊预览取消、切当前重置和
Esc仅关菜单；两版正文相同，不把截图当不同正文scope证据。闭库39seed/12归档不变、
2revision/2receipt/current不变及8进程正常退出通过。固定入口已提升V736。该切片应
与生产IPC、domain和联动测试一起形成依赖闭包；无新commit/push/PR，原53项/实际
Undo/当前容量/原厂/ACP/三平台/IME-DPI/15语言余项不缩减。下方为历史证据。

最新源码补充：处理数文案、公开历史关联commit及prepared-batch复核界面已接通，
保留batch/version/prepareOrigin/receipt和requestId恢复；metadata每页50条、跨页
确认保留。F723七命令/10,359项通过，之后分页67项专项/typecheck/lint通过；
最终F725七命令/10,365项及F726界面构建通过；N724的3项恢复旧断言失败已严格
修正，N728格式/编译/标题、N729恢复专项及strict Clippy通过，源码/Git/EXE稳定。
新完整门N730全部12命令通过：4,284pass/0fail/106ignored、显式历史87/retention44/
strict Clippy；fresh核验与自身temp清理完成。V727嵌入构建119.24s和限定合成实机
双侧关联复核/取消/清除/确认/实际导入/新正文/任务重开通过，闭库审计及8进程退出
通过，当时固定入口为V727；V710旧包已退役、证据保留。V727当时缺旧revision选择，
现已由顶部V736限定验收取代；原53项及其他余项保留。后续拆分
须保持后端合同→API/domain→界面/15语言→验证的依赖闭包，不拆成空按钮。

2026-10-15 续接：已批准的本地 WIP 检查点和上游合入已完成，当前 HEAD 为
`7eb1b7b9f66b149bf7c6a9e3df189caeec062095`，分支为
`wip/session-import-upstream-integration-20261013`。后续修改仍未提交，
没有新增 push/PR/release。以下体积盘点、测试数字和拆分步骤是 10-08 历史草案，
不是当前分支栈或现行 CI 证明；具体拆分仍须在最终源码上重新核对依赖闭包。
当前交付状态见 `2026-10-08-session-import-delivery-current.md`，唯一用户预览
为 V736 / 固定启动入口，不能沿用下方 V241/V325 的旧描述。V727的prepared关联
及V710的运行中取消/重试和202条去重均为此前证据，未冒认V736重测。当前已验证
版本选择/阅读的限定范围见顶部，实际Undo及完整范围仍待验收。

更新：2026-10-08。仅为最终审核准备，未创建分支、commit、push、PR 或 merge。
当前工作树为 `integration-rehearsal-v243`，HEAD
`632f2a8f56bffe8d1c126fae5794f5c3fda316bc`，全部 WIP 保留。
此草案不是“已能独立编译的分支栈”；尚未生成逐 PR 可应用 patch 或验证其 CI。

## 当前体积与拆分约束

本轮只读盘点：相对 HEAD 有 389 个 tracked changed paths，另有 554 个
untracked files；导入 Rust 目录共 175 文件，导入组件目录共 101 文件。
这些计数包含测试、文档、重构、格式变化以及其他领域接线，不能统称新增功能。
普通 diff 的 145,739 added / 104,058 removed 包含换行噪声，不能当作
真实功能行数。`--ignore-space-at-eol` 的路径/语义复核仍是拆分前置工作。
AppWorkbench 本轮盘点相对 HEAD 净减少 253 行；不能在拆分时重新塞回业务状态。

按行为闭环拆，而不是把所有 tests/docs/i18n 推迟到最后：每个单元携带自身
断言、必要文案和产品规则。机械 LF/格式变更独立审查，不借格式提交掩盖语义。
同一 store/session manager 文件须按 hunks 分配，不整文件选择 ours/theirs。
不能靠生产 stub、默认成功、旁路 SelectionSet 或暴露半成品按钮使中间 PR 编译。

## 建议顺序

| 单元 | 建议标题 | 主要边界 | 依赖及完成证明 |
| --- | --- | --- | --- |
| A | `fix(session): preserve queue and human-gate ownership` | 原生队列、权限/问题/plan 回应、Host event ownership；只有独立于导入的真实修复才能先提 | 对原生行为单独复现并回归；依赖导入的 hunk 留在对应后续单元 |
| B | `refactor(storage): isolate native store and journal operations` | 已有 store/session manager 结构拆分、锁外 I/O、fixture helper 等 | 排除纯 EOL，保证原生日志/用量/连接语义；A 若有依赖则先落 |
| C | `feat(session-import): add read-only history contracts` | 类型/错误/ID guard、归一化图和授权 DTO、独立目录/schema 边界 | 精确声明事件 v1、catalog v2、normalizer 版本；imp ID 不进原生写路径 |
| D | `feat(session-import): snapshot and discover registered sources` | 只读复制、SQLite 私有快照/VFS、来源身份、发现/JSONL 前缀 | C；WAL/锁/无 SHM/长路径源 hash 不变，未知格式非空成功 |
| E1 | `feat(session-import): normalize Codex session histories` | Codex shape、重复投影、fork/rollback/compaction、标题 | C/D；附来源证据及合成 fixture，不能声称全部 Codex 安装版本 |
| E2 | `feat(session-import): normalize Claude Code histories` | Claude 父链、工具、sidechain、标题、typed compact summary | C/D；logicalParent 不拼 live Parent，coverage 未知，isMeta 保持隔离 |
| E3 | `feat(session-import): normalize Cursor and transcript sources` | Cursor Store/Composer/transcript 聚合及通用 JSON/Markdown | C/D；不得用正文相似合并身份；跨库悬空有界 partial；必要时再拆 Cursor 与 generic |
| F | `feat(session-import): persist prepared imports and recover publication` | 固定 preview/SelectionSet、durable plan、job/receipt、发布/恢复/取消/重试/撤销 | C–E；这是原子链，按 compile dependency 继续分成 prepare、publish/recovery 两单元；每段都必须是完整安全路径 |
| G | `feat(session-import): browse and search immutable revisions` | catalog、branch/revision reader、索引、有界搜索/导出、游标 | F；只读入口不做 DDL，tamper/stale/cross-owner 拒绝，不把损坏显示为空 |
| H | `feat(workspace-import): map and relocate source workspaces` | 独立工作区导入、多 roots、显式映射/关联/重定位、原生副本 | C/D/F；不改源命令文本，不 checkout，不覆盖用户项目/信任/置顶；可拆 backend 与 UI |
| I | `feat(session-import): add the import wizard and history reader` | 来源/多候选内容/准备/映射/作业/结果、目录/reader、入口/15 语言/域 CSS | F/G/H；建议分别做向导、目录与 reader 两个 UI PR；每个页面需完整点击/键盘/空/忙/错误闭环，UI hold |
| J | `feat(session-import): create explicit continuation drafts` | 原生 handoff 草稿、固定 revision/leaf、可编辑及回导航 | F/G/H/I；创建不调用模型；未发送草稿可恢复，原生 UUID guard 不误拦 |
| K | `feat(session-import): authorize history tools and first dispatch` | MCP grant/capability、first-send durable claim、bootstrap、UNKNOWN | J；不能拆开授权 evaluator 与派发边界；真实四组合/100 轮 trace 必须先补，Mock 不替代 |
| L | `docs(session-import): publish verified compatibility and acceptance` | 用户说明、兼容/性能/平台限制、最终 53 项及可复现摘要 | 汇总最终来源，前面各 PR 已带必要 docs/tests；不提交本机绝对数据根、原始私人资料、编译包或 secrets |

E1/E2/E3 的逻辑依赖可以并列，但不意味着授权并行开发或同时运行重测试。
F、I、H 的建议二次拆分必须先解决当前模块耦合；若删去未来模块就编译失败，
先调整稳定接口/归属并回归，不在中间版本造临时实现。预计审阅单元比表中行数
更多；最终数量根据可应用 patch 与 reviewer 负担决定，不按日期硬凑。

## 拆分执行前的具体门

1. 当前大容量 owner 完成并退出后，记录最终源码/HEAD/index 与全部 WIP
   清单，确认没有他人并发修改；不覆盖或把 untracked 当缓存删除。
2. 在获准的后续拆分阶段，用可恢复、独立的 review index/patch 编排 hunks。
   本轮不创建 checkpoint commit，不 stash/reset/checkout，不移动原树。
3. 逐单元核对依赖闭包：lib/mod、command registry、API DTO、session manager
   和 hook 的每个引用可解析，前后 schema/optional 字段保持已发版兼容。
4. 每个中间树独立完成适用 CI，最终合成树与已验收 WIP 语义一致。
   不能把最终大树的 3,778/9,537 通过数移作中间 PR 的验证结果。
5. 维护规则中的代码质量、网站脚本 self-test、Rust all-targets/测试和
   前端四检查均列入实际发布前门禁；Windows manifest 必须随测试链接重验。
6. UI 单元提供中文/英文、窄窗/200%/主题与键盘截图或证据；等待维护者
   审核外观。15 语言键齐全不等于译文质量完成。
7. 使用实际复现/前后行为写 description，列测试范围和具体未验证环境；
   不写“所有平台/真实续聊通过”来掩盖外部条件。

## 当前还不能发布的原因

当前 53 项表已有最新断言索引，但仍有部分/未验证：万条结果、V245 完整
桌面链、逐 shape 时间保真、15 语言视觉/译文质量、脱敏原厂来源/活库、
真实 ACP 四组合与 100 轮 exact-once、macOS/Linux 原平台以及外围实机。
详见 `session-import-p0/acceptance-results.md` 和
`2026-10-08-session-import-delivery-current.md`。

Windows Computer Use 创建草稿的绑定错误是验收工具缺口，不是可据此提交
产品修复的已定位 bug。此处是 10-08 的历史工具结论；当前入口已为 V325，
并不代表当前开发源码完成 replacement gate。
先闭合这些已有要求，再请用户审核具体拆分结果；本草案不新增 8–24 小时长稳门。

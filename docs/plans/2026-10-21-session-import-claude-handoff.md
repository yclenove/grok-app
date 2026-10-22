# 会话与工作区导入：Claude 交接入口

更新：2026-10-21。本文件是这次交接的当前状态入口；旧文档中的版本号、
“正在运行”“未提交”“不建分支”按各自日期理解，不是当前进程或授权状态。
本轮只整理、保存代码和证据，不新增功能、不 push、不提 PR、不发布。

## 1. 从哪里接手

- 分支：`handoff/session-import-claude-20261021`。
- 代码检查点：`572ab60a7ac4d06f6b3290bee828e4754930d96f`，211 个代码/测试路径，
  其中 Rust 144、前端 67；随后一个文档提交保存本入口及历史台账。
- 起点：`7eb1b7b9f66b149bf7c6a9e3df189caeec062095`，已合入上游 v0.2.41；
  其前的 `2cdb8d5c` 已保存更早的 WIP。不要重新执行旧上游集成计划。
- 当前工作树：`H:\aicoding\grok-session-import-validation\integration-rehearsal-v243`。
- 任务产物根：`H:\aicoding\grok-session-import-validation`，下文称 `TASK_ROOT`。
  这只是文档简称，不要改写系统 HOME/CODEX_HOME。
- origin 是 `yclenove/grok-app`，upstream 是 `RongleCat/grok-app`。
  本地交接分支没有发布到远端；别假定另一台电脑已能 fetch 到它。

这两个新提交是可恢复的完整交接检查点，**不是**已逐树验证的独立小 PR 栈。
之前 A–L 拆分草案仍可参考，但不能把这次大检查点直接当最终上游 PR。

先读 `AGENTS.md`、`CONTRIBUTING.md`、相关 `docs/llm-wiki/` 规则，再读本文件。
原任务及验收边界见：

- [当前滚动状态页](session-import-p0/current-status.md)（接手后唯一持续更新的状态）
- [V5 原任务](2026-09-10-session-workspace-import-grok-goal-prompt-v5.md)
- [53 项证据台账](session-import-p0/acceptance-results.md)
- [兼容性边界](session-import-p0/compatibility.md)
- [性能台账](session-import-p0/benchmark-baseline.md)
- [历史关联设计](2026-10-13-session-import-historical-identity-association.md)
- [小 PR 依赖草案](2026-10-08-session-import-pr-split-review.md)
- [可直接给 Claude 的提示词](2026-10-21-session-import-claude-prompt.md)

## 2. 结论：可以接着开发，不能宣布全部交付

已有完整生产管线和大量回归，不是只有界面或 mock。限定中文 Windows 合成流程、
持久归档/恢复/阅读及部分撤销已实测。但原 53 项没有整体关闭，真实厂商数据、
真实首次模型派发、多媒体视觉使用、大规模性能及三平台实机仍有缺口。
不能根据测试数量推算完成百分比，不能把“代码存在”换算成“验收通过”。

本轮只补跑轻量检查、核对旧证据，没有重跑 10-16 的全库及桌面流程。

## 3. 已保存的实现与阅读路线

| 范围 | 主要代码入口 | 已有能力与边界 |
| --- | --- | --- |
| 来源发现与保真 | `src-tauri/src/session_migration/adapters/`、`cursor.rs`、`snapshot*` | Codex、Claude、Cursor及通用来源；只读源、归一化图、明确 partial/unknown；不承诺覆盖所有厂商版本 |
| 固定准备与发布 | `prepare*`、`prepared_association.rs`、`job_*`、`jobs_publish*`、`publication_*` | 固定选择、签封输入/回执、普通及历史关联发布、恢复、取消/重试/撤销、身份与归属隔离；不可用默认成功或短路验证替代 |
| 版本和任务历史 | `revision_history.rs`、`job_history.rs`、`engine_commit.rs` | 有界分页、显式重开已完成任务、固定 revision 阅读/搜索/导出/续聊；不改变自动 recovery 策略 |
| 图片保留和人工阅读 | `adapters/codex_content.rs`、`codex_projection.rs`、`assets.rs`、`image_catalog.rs`、`image_reader.rs`、`media_server_import_images.rs` | 有序图文/纯图片、身份认证缩略图/完整原图、摘要 blob 核验、有界并发、缓冲 catalog 读取；不抓远端、不把 file ID 当路径，不扩大模型附件授权 |
| 向导与阅读器 | `src/components/session-import/`、`src/hooks/useSessionImport*`、`useImportedImages.ts`、`useImportedRevisions.ts` | 来源/比较/选择/准备/映射/作业/结果、历史关联复核、旧版本/任务入口、图文阅读，迟到响应隔离和重试 |
| 联动边界 | `src/lib/api/sessionMigration*`、`src/lib/sessionImport/`、`src/i18n/messages/*/session-import.ts`、`src/styles/session-import*` | API/domain、15 语言同键、局部样式；共享 ImageViewer 的惰性原图接入不应回归壁纸/聊天 |

最新 V814 修复了一个已实证的问题：选择分支 A 正确持久化，但摘要和默认人工阅读
仍展示完整归档的来源默认 B。现在摘要跟随已确认选择，已认证发布的 events/context/
search 在 leaf 缺省或空白时取回执中的 leaf。显式人工读别的分支仍允许；完整 A/B
归档、续聊选择和附件权限不变。对应 Host 真交互和离线发布回归已保存。

## 4. 测试证据：日期、源码和范围必须一起看

以下原始日志位于 `TASK_ROOT/runs/`，不在产品 Git 中。换机器时没有这些文件，
就依据本摘要理解历史，再生成新证据；不要造一个假的 terminal.json。
原日志的 HEAD/index 是测试时的 `7eb1b7b9`/原 index，**不能**改成新提交以冒充重跑。

### 2026-10-16 原生与前端全门

- N814：`v814-title-44e2338cf6d4450e97794571fd32a968`。
  fmt、编译、strict all-target Clippy 通过；容量 21 passed/5 ignored，
  migration 1318 passed/103 ignored，全库 4322 passed/107 ignored，均 0 failed。
  集合有重叠，不相加；ignored 不计通过。原执行句柄已消费、进程已退出。
- F814：`v814-branch-reader-frontend`。依赖、typecheck、lint、质量门、网站检查、
  Markdown/更新说明回归、完整前端七门通过；899 文件/10404 项测试。
  terminal SHA256：`d1bb8c5a4f1b31c08e0c94ed5b164b49bd027c075907ed6a6e48b2a8dcd2d65c`。
- UI bundle：`v814-branch-reader-ui-build`，23.1505854 秒通过，输入稳定。
  terminal SHA256：`fcfe75091578145cc9b152075a8432156888c4dd2501e7c48b6a5f45db96918e`。
- 嵌入式 Dev 构建：`windows-preview-v814-eba8708dd7af4fd38a44baa48b3d1e1a`，
  128.9519302 秒通过，绑定最终前端；EXE 99,602,432 B，SHA256：
  `8bc35ed56eb73a03c2ffa513ec3ea4f4a6d1aeb0bb4867a4b9240ef4912d0629`。

### 2026-10-16 Windows 实机限定清单

使用全新隔离合成 profile、mock ACP、禁用默认厂商来源，不是英文网页模拟：

1. 实际选择 Claude 来源，B→A 经应用内确认，摘要与选中 4 条事件一致；
   history-only 发布 1/1，默认正文显示 A，保留来源完整 5 条事件。
2. 实际选择 Codex 来源，3 条消息/2 张图片，混排顺序及纯图片消息可见。
   预览中打开纯图片原图，关闭后附件复选框仍未勾选；人工查看不授予模型。
3. 已发布 Claude/Codex 的缩略图及 3,151,321 B 完整原图显示正常。
4. 正常退出，沿同一绝对路径重启；两条导入、A 默认阅读、两张 Codex 完整原图
   和显式重开已完成 Claude 任务均通过，没有重复导入。
5. 两轮各 8 个所属进程正常全部退出，未强杀、无残留任务窗口。

原观察记录见该 V814 run 的 `desktop-observations.md`；两轮 exit JSON 分别记录
PID 33016/30308 的精确归属。未执行模型/vision dispatch、续聊发送或 Undo。
不能把用户对 V762 三条合成记录的撤销许可套用到 V814。

V789 是另外一次历史实证：原路径全新合成 profile 导入三条 Claude，创建未发送
续聊草稿，重启后实际确认 Undo，2 条隐藏、1 条因引用保留；再重启状态保留。
复制 profile 的失败是路径认证拒绝，不能算该成功测试；原 V762 批次仍未撤销。

### 2026-10-21 本轮 fresh 检查

- N814 的 877 个原生输入、F814 的 2585 个前端门输入，逐文件 SHA256 全部一致。
  这是内容核对，不是全库重新执行；没有新增产品代码。
- `pnpm deps:check` 通过；`check-code-quality-gates.py --mode final` 40 门通过。
  App 21 + AppWorkbench 13223 = 13244 行；≥1000 行文件 80 个，均不可增长。
- 本轮另跑 `pnpm typecheck`、`pnpm lint`、`cargo fmt --all -- --check` 均退出 0。
- 聚焦 Vitest：4 文件/48 项通过，覆盖 Host 内容确认/准备、ImportedImage、
  useImportedImages 和共享 ImageViewer。
- 211 个代码/测试暂存 blob：UTF-8、LF、无 BOM、与工作树字节一致，
  `git diff --cached --check` 通过；特定凭据模式扫描无命中（不是完整安全审计）。
- V814 闭库只读 immutable SQLite/归档/blob 审计通过：quick_check=ok，
  2 entries、2 revisions、2 completed jobs、10 个阶段 receipts（五阶段各 2），
  0 continuation refs；3 个资源出现位置、完整 blob 大小/哈希和 fixture 均一致。
  Claude 归档默认 B/完整 5 条，选择 A/4 条；Codex 3 条/2 图；模型 assetIds 为空。
- 审计 helper 曾因 Python 3.8 API 和 Windows >260 字符路径失败，已修 helper 后通过；
  没有改产品或归档来迎合断言。不要把 helper 调试失败报成产品失败。

## 5. 唯一试用入口及产物状态

`TASK_ROOT/start-current-preview.cmd` 仍指向 **V789**，不是最新代码的 V814。
V814 是上述限定桌面清单已通过的候选；本次交接没有切换固定入口，也没启动新 App。
不要给用户另发多个测试 EXE，不要声称当前入口已含 V814 图片修复。
10-21 已再次通过 plain cmd `-CheckOnly`，核对当前 V789 哈希且未启动 App。

Claude 第一批可做 V814 的发布前收尾：核对 build/桌面/审计证据和 EXE 哈希，
补齐脚本要求的 scoped smoke 记录，再更新 `current-preview.json`/`CURRENT-PREVIEW.md`。
必须保留 `appDataRelative=current-preview-data`，不打开、复制或重写用户数据；
plain cmd `-CheckOnly` 验证后才使用现成 cleanup helper inventory/execute 退役旧成功包。
若 helper 因新 HEAD/index 拒绝历史 gate，生成新验证或明确记录内容绑定，
不得修改旧报告、跳过校验或降低安全条件来过关。

本轮没有大构建/新安装包，也没有删除材料，新增回收量为 0 B。检查时无任务所属
App/build/browser 活进程。已清理过的 compiler-temp/WebView 不重复计入本轮回收。
V789 当前包、V814 候选包/绑定 bundle、共享 cargo-target、依赖、immutable seed、
源码和失败证据保留；V806/V812 失败记录及其他 worktree/未知 owner 不清理。
所有后续批次必须核对磁盘、正常退出自己的测试 App，再清理已确认被替代的产物。

## 6. 剩余路线：按依赖推进，每批给验收证据

### H0：接手与台账归一，不重写已有系统

确认分支/HEAD/干净状态；读取 V5 和 53 项台账，将每项映射为“实现位置/证据日期/
合成或真实/平台/剩余动作”。保留旧证据，不把历史 running 当仍在执行。
先完成上一节候选入口收尾或明确继续保留 V789；本轮只读闭库审计已经通过，勿重报待修。
退出条件：状态表没有用旧全量数字证明新改动，没有把 mock 当真实验收。

### H1：内容保真与多媒体缺口

优先审阅 Cursor 图像/附件的来源字段、顺序、缺失及 unknown 状态、预览和发布后的
人工阅读；查清真实脱敏格式再扩展 adapter，不用合成的错误格式反向定义厂商格式。
另对 Codex 非字符串工具输出、压缩历史叠加 rollback 建立反例；这两项只是待证实
审阅点，先实际复现，不预设产品一定错。用失败测试→最小修复→离线发布/reader/
权限回归闭环，不做第二套图片系统。退出条件：源字节不变、未知内容不静默丢弃，
同 digest 不扩大 asset ID 授权，人工原图展示不触发模型授权或远端抓取。

### H2：当前源码规模与响应能力

之前有发布二次方瓶颈与后续局部优化，旧 DEBUG/Release、64 条与旧万条数据不可
混算。基于新 full native gate，先 64/1000 项再扩大到 10000，覆盖混合来源、
部分选择、关联、真实完整 publication，而不只 prepare。度量扫描/准备/发布时间、
首屏、分页、内存、取消响应、恢复和源 hash。保存固定 seed/命令/构建配置及原始
计时，发现增长异常再优化；不靠缩小验收样本或只跑 reader 掩盖发布成本。
退出条件：各阶段有当前源码可复现数据，取消/离线恢复及有界 UI 仍正确。
不要自行增加 V5 未要求的 8–24 小时长稳门槛来无限推迟交付。

### H3：真实来源、活库和一致性

在用户批准的只读来源或脱敏副本上覆盖 Codex/Claude/Cursor 版本、工作区路径、
活 WAL、锁、多库聚合、并发写入和跨库引用。独立 fixture 留来源说明/脱敏方法。
不关源程序制造无锁，不更改源数据库/SHM/WAL，不读凭据、不把私有正文提交。
退出条件：真实来源/版本/平台表和实际源未改写证据，不用 synthetic 替代。

### H4：真实 ACP 首次派发和权限生命周期

官方/自定义 × shared/independent 四组合、真实首次 dispatch、持久首发 claim、
取消/崩溃恢复、UNKNOWN100、不重发/exact-once、进程复用和实际 vision 使用均需
新实证。目前没有真实模型或视觉派发证据；MCP 文本/base64 成功不证明模型看到了图。
在此阶段前请求具体账户/渠道/费用/测试数据授权，不复用聊天里历史误发凭据；
shared 模式不重写 `~/.grok`，independent 不泄漏 provider key。UNKNOWN 不自动重试。
退出条件：保存脱敏 trace/claim/result，重启和失败边界可追溯，不以 mock 关项。

### H5：产品闭环与原平台 UX

完成 distinct-body 多 revision 的阅读/搜索/导出/续聊和完整桌面 export；现有相同
正文版本截图不能证明正确隔离。整理 Undo 的引用保护原因，避免 generic mapping
conflict 文案误导或把原 completed 状态误当本次撤销结果。profile relocation 当前
不支持，路径认证拒绝正确；需要支持时另定迁移/重认证设计，不放松绝对路径签封。
Windows 真实 IME、系统 DPI、深浅/窄窗、15 语言视觉，以及 macOS/Linux 原平台均
分别验收；浏览器缩放、单纯 key 同步或 Windows 测试不能替代。UI 延续项目设计，
不要无授权再次全盘改皮肤。退出条件：相应截图/交互/空忙错态及原平台证据明确。

### H6：交付审核与小 PR 栈

以 A–L 草案重新编排实际依赖闭包；API/生产注册/consumer/测试/文案一起走。
每个中间树跑同 CI；不能用 stub、默认成功、临时兼容旁路使其能编译。将机械 LF
与语义改动分开审查；不改已发布 CHANGELOG。报告每个小 PR 的范围/依赖/风险/验证，
视觉与 IA 改动保留 maintainer UI hold。用户审核并另行授权后才 push/PR/release。

## 7. 验证命令与环境注意

前端（产品根，pnpm only）：

```powershell
pnpm deps:check
pnpm typecheck
pnpm lint
python scripts/check-code-quality-gates.py --mode final
python scripts/publish-website-downloads.py --self-test
pnpm exec vitest run --maxWorkers=2
pnpm build:ui
```

本次 48 项聚焦命令：

```powershell
pnpm exec vitest run src/components/session-import/SessionImportHost.content.interact.test.tsx src/components/session-import/ImportedImage.test.tsx src/hooks/useImportedImages.test.tsx src/components/ImageViewer.test.tsx --maxWorkers=2
```

Rust 全量标准是 fmt、strict all-target Clippy 和测试，不止编译：

```powershell
# 在 src-tauri 下；只给本次进程设置，不覆盖系统 HOME/CODEX_HOME。
$env:CARGO_TARGET_DIR = 'H:\aicoding\grok-session-import-validation\cargo-target'
$env:CARGO_INCREMENTAL = '0'
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --lib
```

Windows 测试 EXE 要在断言前核对 Common Controls v6 manifest；否则本机可能
`0xc0000139` 启动失败。不能按最新修改时间猜测试 EXE，也不能把退出 0/零断言当通过。
本机现成的 `TASK_ROOT/scripts/session-import-title-gate.ps1` 管理 Cargo JSON 精确 EXE、
清单、source/HEAD/index 绑定、断言数和共享 target；完整模式不要带 `-BaselineOnly`。
使用 `-Version` 新编号、适当 `-Focus`（如 image-reader/capacity-contracts）。
性能脚本是 `session-import-performance-gate.ps1`，必须绑定当前完整 native gate。
这些 harness 是本机辅助，不是产品依赖；搬机器要先审阅并适配路径/工具链，不能
声称只在 H 盘能运行，也不能把编译缓存打包进 Git 解决可复现问题。

## 8. 每批交接格式及禁止项

每批记录：解决了什么/具体代码、原始失败、执行命令/源码与构建标识、实际断言数、
未覆盖什么、进程退出、清理的精确目标及逻辑大小/实际磁盘空闲差。没有外部授权
时继续做其他安全可行项，真正需要权限时说明具体缺什么，不擅自发送真实模型请求。

不要读取聊天误发 token，不改账户/代理，不停其他任务进程，不删除源码/整个 target/
node_modules/未知 owner 产物；不要重写旧归档/签名/授权/数据库以制造通过；
不要新增 agent、worktree 或远端写入来绕过范围。发现并发改动先核对，不覆盖。
编码保持 UTF-8 no BOM/LF；本机 core.autocrlf=true，必要时按单命令指定，
不为交接做全仓换行重写或改全局配置。

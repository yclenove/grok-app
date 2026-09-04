# Grok App 全量代码健康审计修复设计

- **日期：** 2026-09-04
- **状态：** 设计已口头确认；等待书面规格复核
- **审计基线：** `a43e3071a6aafedba203db88376f61b1aa73bd63`
- **规格分支：** `codex/codebase-health-audit`
- **实施状态：** 尚未修改产品代码

## 1. 目的

本设计把全量代码健康审计中确认的缺陷、异常语义、竞态、代码异味和技术债，拆成十个可独立审查、可独立回滚的小 PR。修复顺序首先恢复可信的工程基线，然后处理发布安全、Remote IM、持久化、Session API、文档预览、跨平台 I/O 和剩余封闭债项。

首要约束不是“尽快改完”，而是确保修复缺陷时不破坏现有正确功能。每个 PR 必须先把正确行为固化为 characterization tests，再以失败的回归测试证明问题，最后同时通过自动化门禁和真实设备验证。

## 2. 已确认的决策

### 2.1 交付拓扑

采用十个严格串行的 PR：

| PR | 建议分支 | 主题 |
|---|---|---|
| PR-00 | `codex/health-pr00-ci-baseline` | 恢复可信的绿色 CI 基线 |
| PR-01 | `codex/health-pr01-release-tag` | 让手动 Release 重建严格绑定 tag |
| PR-02 | `codex/health-pr02-remote-im-trust` | 强制执行 Remote IM 入站信任策略 |
| PR-03 | `codex/health-pr03-remote-im-lifecycle` | 统一 Remote IM turn 生命周期和并发预算 |
| PR-04 | `codex/health-pr04-remote-im-storage` | 加固 Remote IM 持久化、前端 Host 语义和 connectors |
| PR-05 | `codex/health-pr05-transactional-settings` | 事务化 settings、credentials 和 provider config |
| PR-06 | `codex/health-pr06-session-api-durability` | 实现耐久且幂等的 Session API |
| PR-07 | `codex/health-pr07-document-preview` | 有界文档预览和安全外链 |
| PR-08 | `codex/health-pr08-io-races` | 修复 SSH、下载、CLI 安装和 PTY 竞态 |
| PR-09 | `codex/health-pr09-residual-debt` | 清理已列明的剩余 UI、工具和进程债务 |

每个 PR 都必须从前一个 PR 已合并后的最新 `main` 创建。不得预先从长期堆叠分支同时开发十个 PR；不得把尚未合并的依赖通过复制提交带入下一 PR。开始下一项前必须：

1. `git fetch --prune` 同步 `upstream` 和 fork；
2. 确认上一 PR 已进入 `main`；
3. 快进 fork 的 `main`；
4. 从该提交新建对应 `codex/health-pr*` 分支；
5. 记录该 PR 的 base SHA；
6. 分支一旦跟踪 `origin`，每次本地 commit 都在同一工作轮次 push；不得把已提交工作只留在本机，也不得 force-push `main`。

这里既有“合并顺序依赖”，也有“功能依赖”。例如 PR-07 在合并顺序上基于 PR-00 至 PR-06，但文档预览不依赖 Session API 的实现；因此发生紧急回退时，必须按功能依赖判断，不得机械地把无关修复一起回退。

### 2.2 范围控制

- 一个 PR 只处理本设计列出的职责，不顺手做无关重构。
- PR-09 是封闭清单，不是兜底桶。实施中发现的新问题进入新的审计记录，不得偷偷扩入 PR-09。
- 不新增产品功能，不改变产品定位，不扩大 Remote IM 支持渠道，不增加 Session API 新建会话/中断/transcript 能力。
- 不改变 `session_data_mode=shared` 的默认值；App 仍不得重写用户的 `~/.grok/config.toml`。
- 不改已发布 CHANGELOG 段落，不以降低门禁、增加忽略项、提高预算或放宽 timeout 作为“修复”。
- 不以改名、空拆文件、移动代码但保留同样耦合的方式通过质量门禁。

### 2.3 回归保护原则

每个行为修复都采用以下顺序：

1. 在未修复基线上新增或补齐 characterization tests，证明相邻的正确行为仍为绿色；
2. 新增能够复现缺陷的回归测试，运行并保存失败证据；
3. 实施最小修复；
4. 运行定向测试并保存由红转绿的证据；
5. 运行完整自动化门禁；
6. 执行真实设备 happy path、fault path、相邻功能、升级和回滚验证；
7. 只有全部证据完整才允许合并。

失败状态只保留在本地 TDD 过程和 QA 证据中；推送前通过 amend/squash 清理，不把故意失败的提交推到共享分支。QA 记录必须包含失败命令、失败断言和修复后同一测试的通过结果；PR 对外可见的每个提交和最终树都必须保持可构建。

## 3. 审计追踪矩阵

截至 2026-09-04，upstream 没有开放 Issue，也没有开放 PR。以下矩阵确保每个已确认 finding 都有明确归属，不因拆 PR 而遗漏。

| 发现 | 级别 | 归属 |
|---|---:|---|
| Tauri Host 错误被前端吞掉并伪装成成功 mock | P1 | PR-04 |
| 配对二维码 payload 被发送给 `api.qrserver.com` | P1 | PR-04 |
| settings 全对象 read-modify-write 丢并发更改 | P1 | PR-05 |
| 稀疏 XLSX 巨大 `!ref` 可冻结主 WebView | P1 | PR-07 |
| 未配置 Remote IM 表单因父重渲染被清空 | P2 | PR-04 |
| XLSX 超链接导航主 WebView、绕过 Host 外链处理 | P2 | PR-07 |
| SSH poll 失败把 last-known-good 会话清空 | P2 | PR-08 |
| SSH refresh 与 load-more 乱序使列表倒退 | P2 | PR-08 |
| Hooks Try 使用 OS 原生 `datalist` | P3 | PR-09 |
| HtmlBrowser loading/empty/error 文案绕过 i18n | P3 | PR-09 |
| Keychain 临时读取失败可导致永久删除凭据 | P1 | PR-05 |
| Session API 15 秒 timeout 可丢弃已提交 turn | P1 | PR-06 |
| Session API idempotency 没有原子 claim，也不绑定 payload | P1 | PR-06 |
| Provider `config.toml` whole-file writers 绕过共享锁 | P1 | PR-05 |
| 外部队列持久化失败仍返回 durable acceptance | P2 | PR-06 |
| Provider ping 把 401/429/500 等状态报告为成功 | P2 | PR-05 |
| Provider ping/list/test 未发送配置的 `extra_headers` | P2 | PR-05 |
| official-aux 等待退出后才 drain pipes，可能死锁 | P2 | PR-09 |
| SSH tunnel 缓存不探活，dedicated forward 无 owner | P2 | PR-08 |
| Side Browser 同 URL 并发下载互相覆盖 | P2 | PR-08 |
| CLI 并发安装共用 `.part`，发布和 link 无锁 | P2 | PR-08 |
| PTY 阻塞写入期间持有全部终端的全局 mutex | P2 | PR-08 |
| PTY 输出队列无界，16ms batching 实际失效 | P2 | PR-08 |
| Remote IM 未执行 `allow_chat` / `group_only` / 真 bot mention | P1 | PR-02 |
| Bridge stop/reload/delete 不拥有或终止在途 turn | P1 | PR-03 |
| Remote IM 绕过 `maxConcurrentAgents` 和同 scope 串行 | P1 | PR-03 |
| Windows Remote IM `/stop` 只杀 leader，不杀 descendants | P1 | PR-03 |
| Release `workflow_dispatch.tag` 声明后完全未使用 | P1 | PR-01 |
| Remote Grok 在 Windows 用 Unix 规则拆接 `PATH` | P2 | PR-04 |
| Remote IM 渠道配置无锁、非原子且损坏即当空 | P2 | PR-04 |
| Bridge watchdog 不观察 connector，状态可长期假在线 | P2 | PR-04 |
| ChatCut pin fetch/checkout 失败后仍可能输出 PASS | P2 | PR-09 |
| LINE/WeCom 裸 TCP HTTP 单次 read、无 timeout/并发上限 | P2 | PR-04 |
| `cargo fmt --check` 在 `cli_update.rs` 失败 | 基线债务 | PR-00 |
| ≥1,000 行文件为 79，超过预算 77 | 基线债务 | PR-00 |
| 仓库扫描型 Vitest 在并发负载下不稳定超时 | 测试债务 | PR-00 |
| `goalOrch.ts` 与 `goalOrchView.ts` 形成 cross-chunk cycle | 构建债务 | PR-00 |
| #998 引入跨会话 stale Review focus 并让冻结区域回涨 | P2/债务 | PR-00 先恢复预算；PR-09 修正语义 |

相关但已经关闭的 upstream Issues：

- `#161`：LINE 显示连接但 webhook 未监听；与 Host 假成功表现相邻，但根因不同。
- `#626`：External session-targeted enqueue/wake API；PR-06 处理其现有实现的耐久性残余问题。
- `#757`、`#870`：AppWorkbench 拆分；PR-00 恢复其增长冻结。
- `#803`：PTY data emit 合并；PR-08 处理其剩余背压和锁问题。
- `#998`：Review changed-files 定位；PR-09 修复最新实现的跨会话 stale focus。

除非实施时发现 upstream 新建了直接匹配的开放 Issue，否则 PR 正文只链接这些历史上下文，不使用 `Fixes #...` 重新关闭已经关闭的 Issue。是否新建公开 Issue 需要单独授权，不属于本设计。

## 4. 所有 PR 共用的验证契约

### 4.1 自动化门禁

每个 PR 在干净 checkout 中至少运行：

```bash
corepack pnpm install --frozen-lockfile
corepack pnpm typecheck
corepack pnpm lint
corepack pnpm test
corepack pnpm build:ui
python3 scripts/check-code-quality-gates.py --mode final
python3 scripts/publish-website-downloads.py --self-test

cd src-tauri
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

此外：

- PR-02 至 PR-04 若修改 `remote-bridge/`，必须执行该目录锁文件对应的 clean install、typecheck 和 build。
- PR-01 必须执行 workflow/tag resolver 的 fixture tests、release notes 测试和三个安装包的本地构建校验。
- PR-09 必须执行 ChatCut 离线 fixture self-test。
- 行为或契约变化必须在同一 PR 更新对应的 `docs/llm-wiki/` 规则；公开 API 变化同步更新公开文档，不能只改实现和测试。
- 触及 UI 的 PR 必须验证完整 interaction、现有 visual chrome 和相邻 feature parity，并附实机截图；任何用户可见的 UI/IA 变化在 maintainer 明确审阅前保持待合并状态。
- GitHub Actions 的 frontend 和 macOS/Windows/Linux Rust legs 必须全部绿色。
- 不允许用 `continue-on-error`、忽略特定失败、提高全局 timeout 或降低质量预算让门禁变绿。

若完整 Vitest、Cargo 或 build 失败，即使定向测试通过也不得合并。环境故障必须有可复核证据并重新运行到真实绿色；“单独跑通过”不能替代完整门禁。

### 4.2 真实设备最低矩阵

“实机”指实际安装并运行应用的物理 macOS、Windows 或 Linux 设备，不包括只在 CI runner、jsdom、mock connector 或静态分析中通过。

| PR | 必须使用的真实环境 |
|---|---|
| 每个 PR | 至少一台 macOS 实机完整烟测 |
| PR-01 | macOS、Windows、Linux 实机安装构建产物 |
| PR-02 | macOS 实机 + 真实受控 bot/tenant + 实际 IM 客户端 |
| PR-03 | macOS、Windows 实机 + 真实受控 bot/tenant；Windows 验证进程树 |
| PR-04 | macOS、Windows 实机 + 真实受控 bot/tenant；手机实际扫码 |
| PR-05 | macOS Keychain、Windows Credential Manager、Linux Secret Service 实机 |
| PR-06 | macOS 实机运行已安装 App 和真实 loopback CLI |
| PR-07 | macOS 实机打开真实 XLSX/Office 文件 |
| PR-08 | macOS、Windows、Linux 实机 + 真实 SSH 测试主机 |
| PR-09 | macOS 实机 + 真实 official-aux fallback 和 ChatCut checkout |

Remote IM 验证不能只调内部 API：必须从真实 IM 客户端向受控 bot/tenant 发送消息。SSH 验证必须连接真实测试主机。Release 验证必须安装产物，不能只验证文件存在。

### 4.3 每个 PR 的七项证据

每个 PR 必须创建：

`docs/qa/YYYY-MM-DD-pr-<number>-<topic>-real-device.md`

记录必须包含：

1. **Characterization：** 基线正确行为、命令、结果；
2. **Red/green：** 修复前失败测试与修复后同一测试结果；
3. **Full gates：** 完整命令、退出码、测试统计和 CI run；
4. **Happy path：** 实机操作步骤、预期和实际结果；
5. **Fault path：** 可控故障注入、预期和实际结果；
6. **Adjacent regression：** 同一表面相邻但原本正确的功能；
7. **Upgrade/rollback：** 前一版升级到候选版，再回装前一版的结果。

记录还必须注明 candidate SHA、base SHA、App/CLI 版本、OS 版本、CPU 架构、设备类型、测试数据、脱敏日志/截图位置和清理步骤。禁止写 `PASS(static)`；没有实测的格子必须写 `NOT RUN`，而 `NOT RUN` 会阻止合并。

### 4.4 升级和回滚协议

每个 PR 都在独立测试 profile 上执行：

1. 备份 profile；
2. 安装 base build，创建该 PR 涉及的代表性状态；
3. 原位升级 candidate build 并验证状态、凭据和配置；
4. 正常退出 candidate；
5. 回装 base build 并验证旧版本仍能读取其原有格式；
6. 恢复备份并确认没有残留测试进程、tunnel 或凭据。

任何持久化格式变更必须保持旧字段可读、未知附加 sidecar 可忽略。若确实需要不可逆迁移，该 PR 必须先补备份/恢复协议并重新获得设计批准；本轮十个 PR 不允许引入不可逆迁移。

## 5. PR-00：恢复可信的绿色 CI 基线

### 5.1 目标

在修改产品行为前恢复主分支的格式、测试、构建和质量预算，使后续 PR 的绿色信号可信。该 PR 只做等价整理和门禁稳定化，不修复后续领域缺陷。

### 5.2 修改边界

1. 对 `src-tauri/src/cli_update.rs` 应用 rustfmt 的唯一格式差异，不改变控制流。
2. 把 App shell + AppWorkbench 恢复到审计前的硬上限：
   - 合计行数不高于 `15,260`；
   - 合计 `useState` 不高于 `153`；
   - `useEffect` 不得增加。
3. 将 #998 新增的 Review focus 桥接状态抽到 side-workbench 的 domain hook/component；PR-00 保持现有点击和定位语义，跨会话 identity 的行为修正在 PR-09 完成。
4. 通过真实职责抽取把 ≥1,000 行文件数从 79 降至不高于 77：
   - 从 `src-tauri/src/commands/worktree_agents_p2.rs` 抽出 Git porcelain/status 解析职责到独立模块；
   - 从 `src-tauri/src/process_util.rs` 抽出系统文件管理器 open/reveal 职责到独立模块；
   - 新模块本身必须低于 1,000 行，调用者 API 和平台行为保持不变。
5. 解除 `goalOrch.ts` ↔ `goalOrchView.ts` 循环：view 消费无环的 core/types；消费者直接从职责模块导入，barrel 不反向 re-export 形成 cycle。
6. 优化仓库扫描测试，而不是放宽 timeout：读取文件一次，先用廉价文本条件筛选候选，再对候选执行同样的 TypeScript AST 断言；覆盖范围仍是全部生产源码。
7. 在完整门禁真实绿色后更新 `CODE-QUALITY-PROGRESS.md` 和相关维护文档，记录实际数字，并把预算 ratchet 到本 PR 的真实结果，不留增长余量。

### 5.3 明确不做

- 不改变 Review focus 的业务语义；跨 session/project stale 问题留在 PR-09。
- 不修 Remote IM、Session API 或 Provider 行为。
- 不删除测试、不排除慢文件、不提高 30 秒测试 timeout。
- 不把 1,000 行门槛从 77 调高，也不通过空行/格式压缩作弊。

### 5.4 测试和实机验证

- Characterization：Review chip 仍同步打开 Review 并定位文件；file manager open/reveal 和 worktree status 解析结果不变；goal orchestration 输出不变。
- Regression：构建日志不得再出现 `goalOrch` circular chunk；完整 Vitest 连续三次均通过；质量门禁报告行数、hook 数和大文件数均在预算内。
- macOS 实机：启动 App、新建/打开会话、发送普通消息、点击 changed-files chip、打开 Review、打开/显示文件位置、打开设置和可靠性中心。
- Fault path：Review 文件不存在、Finder open 失败、无 goal events、仓库扫描在并发完整 suite 中运行。
- Upgrade/rollback：base/candidate 往返后会话、侧栏和本地设置不变。

### 5.5 退出标准

- `cargo fmt --check`、完整 Vitest、UI build 和质量门禁全部绿色；
- 无 circular chunk warning；
- App shell + AppWorkbench `≤15,260 / ≤153 useState`；
- ≥1,000 行文件 `≤77`；
- QA 证据位于 `docs/qa/YYYY-MM-DD-pr-00-ci-baseline-real-device.md`。

## 6. PR-01：手动 Release 重建严格绑定 tag

### 6.1 目标

保证手动输入哪个已有 tag，就只构建该 tag 指向的提交，并让 release notes、上传目标、updater manifest、校验文件和网站下载元数据使用同一个解析结果。

### 6.2 修改边界

1. `workflow_dispatch.tag` 改为必填的 `v<semver>`；空值、非 semver、分支名和不存在的 tag 在 build matrix 启动前失败。
2. 新增一个 resolve/validate job，输出不可变的 `release_tag`、`release_version`、`release_sha`。
3. tag push 使用 `github.ref_name`；manual dispatch 使用已验证 input。二者都解析 annotated/lightweight tag 的最终 commit。
4. 所有 jobs 用同一个 SHA checkout，并使用完整 tag 历史；禁止 manual run 默认 checkout `main`。
5. 校验 `package.json`、Tauri manifest、Cargo package version 和 CHANGELOG 段落都等于 resolved version。
6. GitHub Release body 只通过 `scripts/changelog-for-release.py` 从已校验版本生成；不得在 workflow 内另造 release notes 逻辑。
7. `tauri-action.tagName`、tag-scoped updater archive、Windows portable、checksums 和 Release assets 全部只读取 resolver output，并写入该 tag 的不可变命名空间。
8. tag-scoped build/upload 的 concurrency key 使用 resolved tag，避免同一 tag 的两次重建互相覆盖；不同 tag 的不可变产物构建互不取消。
9. rolling manifest、latest download alias 和 website version metadata 进入独立的全局串行 promotion job。该 job 发布前重新读取当前 latest，以 semver + 当前值做 CAS：旧 tag 重建只能刷新自己的 Release assets，不得降级或覆盖 mutable latest；同版本重建只能在 expected tag 仍为 latest 时刷新。
10. 将 tag 解析/校验放入可离线测试的脚本，workflow 只编排结果，不在五处复制 shell 分支。

### 6.3 明确不做

- 不自动创建 tag，不从当前 package version 猜测手动发布目标。
- 不改变版本号、CHANGELOG 或已发布 release notes。
- 不在 PR 验证中未经授权发布正式 upstream Release；使用 fork/dry fixture 和本地产物验证。

### 6.4 测试和实机验证

- Fixture：tag push、合法 manual tag、空 tag、非法 tag、不存在 tag、tag/manifest mismatch、tag/CHANGELOG mismatch。
- 静态契约测试：release workflow 中不存在绕开 resolver output 的 tag 推导。
- 并发 fixture 让新旧两个 tag 反序完成，证明旧 tag 只能更新自己的不可变 assets，不能回退 rolling manifest、latest alias 或 website version。
- 受控 fork 验证 manual rebuild checkout SHA 等于 tag SHA，并上传到对应测试 release。
- macOS、Windows、Linux 实机分别安装本地产物，核对 About 版本、启动、打开会话、退出和卸载/回装。
- Fault path：故意输入错误 tag 和 mismatch fixture，必须在任何发布上传前停止。
- Upgrade/rollback：安装 base release、升级 candidate、回装 base，用户 profile 可读。

### 6.5 退出标准

- workflow 中 `inputs.tag` 被唯一 resolver 消费；
- 下游没有从默认分支 package version 推导 manual tag 的路径；
- 旧 tag 重建不能改写任何 mutable latest surface；
- 三平台真实安装完成；
- QA 证据位于 `docs/qa/YYYY-MM-DD-pr-01-release-tag-real-device.md`。

## 7. PR-02：强制执行 Remote IM 入站信任策略

### 7.1 目标

所有 Remote IM 输入在触发命令、创建/续跑 session 或启动 Grok 前，统一执行 sender、chat、会话类型和真实 bot mention 策略。

### 7.2 设计

1. channel adapters 输出规范化的 `InboundIdentity`：channel/instance、sender ID、chat ID、direct/group 类型、bot ID、被提及的主体集合和原始事件 ID。
2. engine 中建立唯一的纯函数 `evaluate_inbound_policy`，输出 `allow` 或结构化拒绝原因。
3. 判定顺序固定为：
   - instance 启用且允许 chat；
   - sender 满足 `allow_from`；
   - chat 满足 `allow_chat`；
   - `group_only` 与实际 conversation type 一致；
   - 按现有产品规则需要群聊 mention 时，mentions 中必须包含当前 bot identity，而不是“出现任意 mention”；明确允许的 DM 继续沿用现有免 @ 语义。
4. 自由文本、slash command、数字选择、卡片回调和 `/stop` 等入口共用同一判定，不允许旁路。
5. 拒绝发生在项目路径解析、session 创建、进程 permit 和用户 prompt 落盘之前。
6. 审计日志只记录 instance/chat/sender 的稳定哈希和 reason code，不记录消息正文、token 或 secret。
7. adapters 无法证明 chat type 或 bot identity 时 fail closed，并呈现可诊断的 connector configuration error。

### 7.3 兼容性边界

- 已被明确允许的 DM、群聊和真实 @bot 行为保持不变。
- 不改变 `allowRemoteYolo`、项目 trust、presenter 模式和 outbound 格式。
- 不新增 ACL UI；沿用现有配置字段和 i18n。
- 不在这一 PR 处理进程取消、并发、存储或 connector watchdog。

### 7.4 测试和实机验证

- 表驱动测试覆盖 sender/chat/group-only/mention 的允许和拒绝笛卡尔矩阵。
- 每个生产 adapter 都有 identity contract fixture；每种输入形态均证明不能绕开统一 predicate。
- Slack/Discord fixture 区分提及 bot、提及其他用户和无提及；企业 IM fixture 覆盖群白名单和私聊。
- macOS 实机至少覆盖真实 Slack、Discord，以及 Feishu/Lark 或 Telegram 三类受控 bot/tenant：允许用户+允许群+真 @bot 能启动；未授权群、未授权用户、`group_only` 配置下的 DM、@其他人都不能启动 turn。
- 实施前建立 adapter-to-evidence 表；除上述最低矩阵外，任何被修改了生产 chat type、bot identity 或 mention 解析语义的 adapter，都必须增加对应真实渠道客户端验证。缺少该渠道凭据时不得用 fixture 顶替，必须停止该 adapter 改动或阻止 PR 合并。
- Fault path 验证拒绝不会创建 session、不会占 agent slot、不会落 prompt、不会泄漏正文。
- 相邻回归覆盖合法 slash command、卡片回调、普通回复和远程 YOLO 仍按原配置工作。
- Upgrade/rollback 使用原有 ACL 配置，无需迁移且旧版本仍可读取。

### 7.5 退出标准

- 所有入站入口只有一个授权决策点；
- 未授权矩阵为零 turn、零 session、零进程；
- 每个被改动 adapter 的真实 bot/tenant 证据位于 `docs/qa/YYYY-MM-DD-pr-02-remote-im-trust-real-device.md`。

## 8. PR-03：统一 Remote IM turn 生命周期和并发预算

### 8.1 目标

让 Bridge 对自己启动的每个 turn、进程和 permit 拥有完整生命周期；Bridge stop/reload/delete 的 UI 结果必须与真实执行状态一致，同时遵守桌面 App 的全局进程预算和同 scope 串行规则。

### 8.2 设计

1. `RuntimeHandle` 持有 root cancellation token、connector handles、message pump 和 turn `JoinSet`；不再丢弃 `spawn_cancellable` 返回的 handle。
2. 每个 turn 使用 RAII owner 持有：scope key、全局 process permit、child process tree、outbound capability 和 completion state。
3. stop/reload/delete 顺序固定为：停止接受新消息 → signal cancellation → 终止进程树 → 等待/abort 有时限的 tasks → 丢弃 outbound credentials → 返回完成。
4. Unix 继续使用独立 session/process group；Windows 使用 kill-on-close Job Object，使 leader、工具 descendants 和持管道进程一起终止并被 reap。
5. `/stop` 只取消对应 scope 的当前 turn；Bridge stop/reload 取消该 runtime 全部 turn；删除 instance 只取消该 instance 的 turn。
6. Remote IM 申请与 SessionManager 同源的 `maxConcurrentAgents` permit；无 permit 时进入有界 FIFO，而不是绕开预算 spawn。
7. 同一 scope/session 只允许一个执行者。并发首轮消息先原子绑定唯一 App session，其余消息按到达顺序排队，不能创建两个 session 再互相覆盖 binding。
8. FIFO 达到容量时返回现有本地化 busy/degraded 语义，并保证该消息零 session、零 prompt 落盘、零 child process；不得无界增长或静默丢弃旧消息。
9. permit、scope lock 和 queue item 在成功、错误、取消、panic 和 shutdown 路径全部释放。
10. stop 成功后不得再发送旧 outbound 消息；取消状态使用原有非技术性文案，不伪装成完成。

### 8.3 明确不做

- 不改入站 ACL；PR-02 已负责。
- 不重写 Remote IM 为 ACP 长连接，不改变 presenter/session continuity 产品规则。
- 不在本 PR 修 connector storage、QR 或前端 Host 假成功。

### 8.4 测试和实机验证

- fake Grok 生成 child/grandchild 和持 stdout/stderr pipe descendant，stop 后全部退出。
- bridge stop、reload、instance delete、`/stop` 各自验证正确取消范围。
- 并发测试证明总进程不超过配置预算，同 scope 严格串行，不同 scope 在预算内可并行。
- 两个同步首轮请求只创建一个 App session，后续顺序稳定。
- macOS 与 Windows 实机通过真实 bot 启动长 turn，依次验证 `/stop`、Bridge stop、改配置 reload 和删除 channel。
- Windows 用进程查看工具记录 descendants 在停止后消失；macOS 记录 process group 回收。
- 相邻回归覆盖正常完成、连续两条消息、桌面本地会话与 Remote IM 共享预算。
- Upgrade/rollback 保持 binding/session 格式兼容并清理所有测试进程。

### 8.5 退出标准

- runtime 中没有 detached turn；
- stop 返回时没有对应 child/descendant、permit 或 outbound capability；
- 同 scope 不并发、全局不超预算；
- QA 证据位于 `docs/qa/YYYY-MM-DD-pr-03-remote-im-lifecycle-real-device.md`。

## 9. PR-04：加固 Remote IM 持久化、前端 Host 语义和 connectors

### 9.1 目标

消除 Remote IM “界面显示成功但 Host 未成功”、渠道配置丢失、二维码数据外发、connector 假在线和 webhook 解析脆弱等剩余问题。

### 9.2 持久化和前端语义

1. channel metadata、bridge config 和 credentials 写入共用进程锁、sidecar file lock、temp write、flush/fsync 和 atomic rename。
2. 解析错误不再返回空列表；损坏文件隔离为带时间戳备份并向 UI 返回可诊断错误。
3. metadata 与 credentials 采用可恢复的提交顺序：先准备新 secret、原子提交 metadata、再清理旧 secret；失败时可回滚或保留旧可用版本。
4. `bridgeClient` 只在非 Tauri 浏览器开发环境提供 mock。只要 `isTauri()` 为真，invoke rejection 就必须作为 tagged Host error 传播。
5. save/delete/test 只有收到 Host 明确成功才更新本地列表、清空 secret 草稿或显示成功；失败保留表单、实例和重试入口。
6. 未配置 channel 的 default instance 按 channel/identity 稳定 memoize；子表单只在真实 instance identity/version 变化时重置，父状态轮询不得清空草稿或扫码状态。
7. 配对二维码用已有 `qrcode` 依赖在本地生成 canvas/data URL；DOM、日志和网络中不得出现第三方 QR endpoint。

### 9.3 Connector 和平台语义

1. runtime 单独跟踪每个 connector 的 starting/ready/failed/stopped 状态和 handle。
2. 只有协议握手/监听真正 ready 后才加入 connected list；connector 退出必须立即撤销 connected 状态并触发有界 backoff/restart 或明确 degraded。
3. watchdog 同时观察 pump 和 connector handles，不能由 keepalive sender 掩盖 connector 死亡。
4. LINE/WeCom webhook 改用仓库兼容的成熟 HTTP server 栈，完整读取 header/body，并设置：body 上限、header 上限、读写 timeout、连接并发上限和 graceful shutdown。
5. 签名验证继续在业务解析前执行；分片请求、慢客户端和超限 body 均有确定响应且不占无限 task。
6. Windows Remote Grok `PATH` 使用 `std::env::split_paths/join_paths`，保留盘符、空格、顺序并去除精确重复项。

### 9.4 明确不做

- 不新增渠道或重做 Remote IM 设置 IA。
- 不改变 PR-02 的授权规则或 PR-03 的 lifecycle/permit owner。
- 不把真实 Host 错误重新降级为 soft success。

### 9.5 测试和实机验证

- 并发 save/delete/error update 不丢 instance；中断写入和损坏 JSON 可恢复。
- 在存在 `__TAURI_INTERNALS__` 且 invoke reject 时，test/save/delete 均显示失败且本地状态不变。
- 父组件多次轮询重渲染后，未保存名称、ACL、secret 和 scan 状态仍在。
- QR DOM 和网络请求不包含 `qrserver.com` 或任何远程 QR 服务。
- connector ready/fail/restart 状态机、HTTP 分片、slow client、oversize body 和 graceful stop 有自动化测试。
- Windows PATH fixture 覆盖盘符、空格、重复目录。
- macOS/Windows 实机连接真实 bot/tenant；手机实际扫码；断网、错 token、connector crash、保存失败和删除失败均显示真实状态。
- LINE 与 WeCom 各使用真实受控账号完成签名事件接收、真实回复、断网恢复、connector kill/restart 和 graceful stop；再在已安装 candidate 上用真实 socket fault harness 覆盖分片、slow client 和 oversize body。任一渠道 `NOT RUN` 都阻止合并。
- 相邻回归覆盖正常保存连接、自动启动、合法 webhook、Bridge status 和现有 channel switching。
- Upgrade/rollback 保留全部实例、ACL 和凭据；损坏恢复文件可识别。

### 9.6 退出标准

- Tauri Host failure 为零假成功；
- 配对 payload 零外发；
- channel storage 在并发和中断下零静默清空；
- connected list 与真实 connector 一致；
- QA 证据位于 `docs/qa/YYYY-MM-DD-pr-04-remote-im-storage-real-device.md`。

## 10. PR-05：事务化 settings、credentials 和 provider config

### 10.1 目标

让设置、凭据和 provider 配置的读改写在并发、临时读取错误和跨后端迁移时保持原子且可恢复，同时保留现有字段校验、软重启和 UI 语义。

### 10.2 Settings 事务

1. 新增 Host `settings_patch`。请求只包含要修改的字段：字段缺失表示不修改；可空字段只有显式 `null` 才清空；未知字段拒绝。`settings_get` 和写入响应同时返回由已提交内容导出的 opaque revision token。
2. 所有产品前端 `settingsGet() → {...snapshot, field} → settingsSet()` 改为最小 patch。UI optimistic update 失败时恢复 Host 返回的已提交值并显示现有风格错误。
3. Rust 内部 settings writer 进入统一 `store::update_settings`：持锁读取最新对象、执行 mutation、规范化/校验、原子 replace，并返回 `{before, after, changed_fields}`。
4. `settings_set` 不再作为产品写入口；若为兼容保留，只能要求调用方传回 `settings_get` 的 `expected_revision`，锁内内容摘要不匹配即拒绝，不允许覆盖新值。
5. launch-at-login、keychain、profile sync、agent respawn、tray/menu refresh 等副作用以已提交的 before/after diff 计算，只执行一次。关键外部副作用失败时按当前产品承诺回滚对应字段；回滚本身也走字段级 CAS，只有该字段仍等于本次提交值时才能恢复旧值，不能覆盖随后到达的更新。非关键刷新失败不得回滚无关字段。
6. JSON 文件继续使用既有 `store_lock` 的跨进程锁和 atomic replace；不引入新设置文件格式。

### 10.3 Credential 事务

1. Keychain read 返回 `Found(value)`、`Missing` 或 `BackendError`，禁止把 `BackendError` 折叠成 `None`。
2. backend error 不写 cache、不更新 presence flag、不删除任何源/目标 secret。
3. 切换存储后端使用两阶段迁移：完整读取源 → 写目标 → 回读验证 → 提交 preference/presence → 删除源。
4. 任一步失败都保留源值和旧 preference，并返回可重试错误。删除失败时保留 presence 标记，不能报告迁移完成。
5. 日志只记录 account kind 和错误类别，不记录 secret 值。

### 10.4 Provider config 事务

1. 扩展 `agent_home_config` 为唯一 `config.toml` mutation owner；provider upsert/remove/default、relay repair、models/official-aux repair 都经同一锁。
2. 锁内读取最新文本，解析/UTF-8/IO 错误立即中止，不能以空配置继续。
3. 写入使用唯一 temp、flush/fsync 和 atomic replace；保留注释、未知 top-level key、无关 table 和 API key。
4. shared mode 仍拒绝写 `~/.grok/config.toml`。
5. Provider ping 只有预期成功 HTTP status 才 `ok:true`；401/403/429/5xx 映射为不含 secret 的结构化失败。
6. ping、list models、test model 都应用经过换行校验的 `extra_headers`；Authorization 的现有规则保持不变，header 值不得进入日志或错误。

### 10.5 明确不做

- 不重做 Settings/Providers UI，不新增设置项，不改变默认模型或 session data mode。
- 不改变 Provider inference wire format。
- 不借机格式化整个 `config.toml` 或删除用户未知配置。

### 10.6 测试和实机验证

- 两个 deferred setting updates 交错完成后两个值都保留，副作用各执行一次。
- revision mismatch 不覆盖已提交设置；非法字段和非法值保持原配置。
- Keychain read/delete/write/verify 各阶段故障注入均零凭据丢失。
- 两个 provider/repair writer 并发后所有 table 和无关 key 保留；读取失败不写文件。
- HTTP status matrix 和受控 extra-header server 验证 probe 语义。
- 相邻回归覆盖主题、语言、启动项、CLI 路径、Provider activate、shared/independent 和 soft-respawn。
- macOS、Windows、Linux 分别验证系统 credential backend 的成功、拒绝授权、暂时不可用、迁移和恢复。
- Upgrade/rollback 验证旧 settings JSON、secret fallback 和 config TOML 均可读。

### 10.7 退出标准

- 产品代码不存在旧快照 whole-object settings write；
- 故障注入下 settings 零 lost update、credentials 零删除、TOML 零截断；
- QA 证据位于 `docs/qa/YYYY-MM-DD-pr-05-settings-credentials-real-device.md`。

## 11. PR-06：耐久且幂等的 Session API

### 11.1 目标

外部调用者收到 `turn_started` 或 `queued` 时，Host 必须已经取得可恢复的执行所有权；相同 idempotency key 的安全重试不能产生重复 turn。

### 11.2 Idempotency 协议

1. claim 在 dispatch 前原子落盘，至少记录 schema version、key、稳定 operation ID、session ID、prompt digest、created/updated time、phase 和 result。
2. key 与 `(session_id, prompt_digest)` 绑定。同 key 同 payload：
   - completed：回放原结果；
   - in-flight：返回当前确定状态，不启动第二次 dispatch。
3. 同 key 不同 payload 返回 `idempotency_conflict`（HTTP 409），不回放旧结果也不处理新 prompt。
4. claim/update/eviction 在一个锁定事务内完成；cap 200 只能清理已终结的最旧记录，不得淘汰 in-flight claim。若达到上限且没有可清理终态记录，新 claim 返回 `busy`（HTTP 409，reason=`idempotency_capacity`），不允许无界增长。
5. 新 schema 使用独立、带版本的 sidecar；旧版本可忽略它，不能把解析失败解释成空并覆盖。
6. operation ID 贯穿 claim、queue item 和 SessionManager 的持久化 dispatch fence。SessionManager 在调用有副作用的 prompt RPC 前原子写入 `prepared`，跨越调用边界前写入 `sending`，收到确定结果后写入 `acknowledged` 或终态；同一 operation ID 只能有一个执行者跨越该 fence。
7. 崩溃恢复或重试先查 fence：`prepared` 可由唯一 owner 继续；`acknowledged`/终态只回放；`sending` 若没有可验证的下游 receipt 则转为 `outcome_unknown` 并停止自动派发。只有能够证明 prompt RPC 从未开始时才允许重新 dispatch，不能以“没有完成证明”为由盲目重发。

### 11.3 Dispatch 和 timeout 所有权

1. 15 秒上限只覆盖尚未提交的 prepare/connect admission 阶段。
2. 一旦写入用户消息、设置 `prompt_in_flight`、创建 lease 或 spawn side process，任务交给 SessionManager-owned handle；HTTP future 被取消不能丢弃它。
3. 能证明 turn 已由 SessionManager 接收时返回确定的 started/queued 状态；无法证明提交边界时返回 `outcome_unknown`，附同一 idempotency key，调用者只能携带该 key 做查询式重试。
4. pre-commit timeout 可以返回 `retry_later`；必须证明没有消息、lease、busy state 或 child process 残留。
5. 所有错误/取消路径恢复 session state、释放 connect lock 并清理 side process。

### 11.4 Durable queue

1. `queued` 只在 queue item 原子落盘成功后返回 202。
2. 读取、解析、锁、写入或容量错误都返回明确失败，不能重建为空队列。
3. 队列满时拒绝新 item，不淘汰已确认接受的旧 item。
4. drain 使用 `pending → leased → completed` 状态；take/requeue/complete 都持锁持久化。
5. 崩溃恢复按 operation fence 分类 lease：明确未开始的回到 pending；已 acknowledged 的完成/回放；停在 `sending` 且无法核实 receipt 的转为 `outcome_unknown` 并保留诊断记录，不自动重新入队。
6. GUI `session://send_queue` 仍只展示 Host 已持久化 item，不成为第二个发送 owner。

### 11.5 API 兼容性

- 现有 `turn_started`、`queued`、`busy`、`not_found`、`app_not_running`、`retry_later` 和 `error` 保持其已文档化的正常语义。
- 只新增 `idempotency_conflict`（HTTP 409）和 `outcome_unknown`（HTTP 503）；CLI JSON 始终包含机器可读 status，二者均为 exit 1。`outcome_unknown` 的重试必须携带原 key，直到回放出确定状态或继续返回同一 unknown。
- Host 为每个请求生成 operation ID；`idempotencyKey` 为兼容仍可省略，但 CLI 默认在发请求前生成并复用。直接 HTTP 调用若省略 key，响应会返回 Host 生成的 key；若调用方在收到响应前断线，则该次调用不可安全重试，文档必须明确禁止无 key 重放。
- 不新增会话创建、中断或 transcript API；不监听非 loopback。

### 11.6 测试和实机验证

- 并发同 key 只有一个 dispatch；同 key 不同 session/prompt 为 409。
- pre-commit timeout 零副作用；post-commit HTTP cancellation 后 owner 正常完成或清理。
- queue 写失败不返回 202；capacity 不淘汰；corrupt store 可诊断且不覆盖。
- 在 claim、enqueue、lease、send-before-fence、`sending`、acknowledged、complete 各阶段模拟 crash，验证只有明确未开始的操作会恢复派发，模糊边界返回 unknown 且不重复。
- characterization 覆盖现有 status/HTTP/CLI exit code。
- macOS 实机使用已安装 App 的真实 loopback endpoint 和 `grok-app --session-send`，验证空闲发送、忙时排队、重复 key、慢 Host vision、强退重启和磁盘不可写。
- 相邻回归覆盖 GUI send queue、同一会话普通发送、session list 和 token auth。
- Upgrade/rollback 只在没有活动 turn 时回装，并验证旧会话/索引可读、新 sidecar 被旧版安全忽略。

### 11.7 退出标准

- 每个 accepted request 只有一个 durable owner；
- 相同 key 零重复 turn；
- `queued` 零未落盘确认；
- timeout 后零 sticky busy 和 orphan process；
- QA 证据位于 `docs/qa/YYYY-MM-DD-pr-06-session-api-real-device.md`。

## 12. PR-07：有界文档预览和安全外链

### 12.1 目标

恶意或稀疏 XLSX 不能阻塞主 WebView；工作表链接不能在应用内导航或绕过统一外链策略。

### 12.2 XLSX 处理

1. XLSX parse/render 放入可终止 Web Worker；主线程只接收有界的 sheet metadata 和已清洗 HTML。
2. 同一 buffer 只解析一次并缓存 workbook；切换 sheet 不再重新 `XLSX.read`。
3. 在调用 `sheet_to_html` 前 decode `!ref` 并裁剪。单 sheet 预览同时受以下硬上限约束：
   - 2,000 行；
   - 200 列；
   - 200,000 个矩形单元格。
4. 任一上限超出时只渲染满足全部上限的前部区域，返回原始范围、显示范围和 truncated flag；UI 展示本地化提示与“外部打开”。
5. Worker 有解析 wall-clock timeout、消息大小上限和 generation token。文件变化、卸载或新请求到达时终止旧 worker，旧结果不得覆盖当前文件。
6. 保留现有 40 MiB 输入限制；它与 used-range 限制共同生效。

### 12.3 链接处理

1. sanitizer 只允许可解析的绝对 `http:`/`https:` anchor；相对 URL、`javascript:`、`data:`、`file:` 和其他 scheme 去除可点击能力但保留单元格文本。
2. sheet container 捕获 anchor click，执行 `preventDefault`/`stopPropagation` 后调用 `openExternalUrl`。
3. Host open 失败显示本地化、可关闭错误，不回退到当前 WebView 导航。
4. 主 WebView location 在任何 sheet click 后都保持不变。

### 12.4 明确不做

- 不增加完整电子表格编辑、公式执行或无限滚动虚拟表格。
- 不改变 PDF、DOCX、PPTX/ODF 的正常渲染语义。
- 不修改原始 Office 文件。

### 12.5 测试和实机验证

- 正常小表、多 sheet、空表、恰好边界值和超边界值。
- `A1:XFD1048576` 稀疏 `!ref` 在 `sheet_to_html` 前被裁剪。
- Worker timeout、快速文件/工作表切换、unmount 和 stale response。
- http/https 外链走 Host；所有其他 scheme 不可导航；Host failure 留在 App。
- 相邻回归覆盖 PDF 翻页/缩放、DOCX reflow、external open 和 reveal。
- macOS 实机打开真实业务 XLSX 与构造稀疏 XLSX，解析期间持续操作侧栏和会话，确认无黑屏、无冻结、内存受控。
- 点击真实工作表外链只打开系统浏览器。
- Upgrade/rollback 验证原始文件未改、无新持久化格式。

### 12.6 退出标准

- 主线程不执行无界 workbook expansion；
- 三项范围上限和 timeout 可测试；
- 所有 sheet link 零主 WebView 导航；
- QA 证据位于 `docs/qa/YYYY-MM-DD-pr-07-document-preview-real-device.md`。

## 13. PR-08：SSH、下载、CLI 安装和 PTY 竞态

### 13.1 组织方式

这是一个 PR，但内部保持五个可独立 review/revert 的提交：SSH watch、SSH tunnel、Side Browser download、CLI install、PTY。提交之间不得共享一个泛化的“大 runtime manager”。

### 13.2 SSH watch

1. per-alias state 增加 last-success、stale 和 error；请求失败保留 last-known-good sessions/total。
2. 只有成功返回的空列表能清空 UI。
3. 每个 alias 使用递增 generation；只有最新 generation 可提交。
4. refresh 使用当前已加载 page limit，旧的 20-item response 不能覆盖已完成的 40-item load-more。
5. disable/remove alias 时取消请求并清理该 alias state。

### 13.3 SSH tunnel

1. 缓存命中前对本地 forward 端口做有时限探活；失败立即淘汰并重建。
2. dedicated `ssh -N -L` 注册到 Host lifecycle owner，记录 alias、local port、child handle 和状态。
3. 删除 host、disable watch、remote browser 关闭、App quit 都终止并 reap 对应 forward。
4. 重建使用单飞锁，多个并发请求共享一个成功 tunnel，不能启动重复 `ssh`。

### 13.4 Side Browser download

1. 每次 download 创建唯一 token；pending key 是 token，URL 只作为元数据。
2. staging filename 包含 token，完成事件携带相同 token；同 URL 并发互不覆盖。
3. success/cancel/failure/close 各自只清理自己的 staging 和 pending entry。
4. publish 到目标继续使用原子 move/copy fallback，并保留现有文件选择体验。

### 13.5 CLI install

1. GUI、Mirror RPC 和 updater fallback 共用 process-level install coordinator。
2. 同一版本/平台并发请求 single-flight，共享结果；不同安装目标返回明确 busy，不并发发布。
3. 每次下载使用 UUID 临时文件；checksum 成功后 fsync，再原子 publish/link。
4. 旧可用 CLI 在新文件完整校验并发布前保持不变；失败清理本次 temp，不删除其他请求或旧 binary。

### 13.6 PTY

1. 全局 sessions mutex 只用于查找/插入/移除 per-session handle，不包围 `write_all`、flush 或等待。
2. 每 session 建立有界 input queue 和专用 writer；生产者等待容量并可被 close/kill 取消，channel 关闭时返回明确错误，不能静默丢输入。PTY write 阻塞不影响其他 session，也不阻止 kill 取得 killer。
3. output 使用有界 channel，容量固定为 32 个最多 64 KiB 的 batch；满时施加背压，不丢字节、不无限增长内存。
4. emitter 在 16ms elapsed 或累计 64 KiB 时发送一次；不能因为单次 8 KiB read 就立即 flush。
5. close/kill 关闭 channel、终止 child 并让 reader/writer/emitter 退出；旧 tab event 不再发送。
6. Windows PTY 的 kill 需验证 shell descendants 一并退出；若 portable-pty killer 不能保证，则复用平台 process-tree owner，而不是调用 `taskkill` 的散落副本。

### 13.7 明确不做

- 不新增 tmux、远程 agent wave 或 SSH 文件编辑能力。
- 不重做 Side Browser 下载 UI 或改变 CLI 信任链/checksum 策略。
- 不改变 terminal ANSI 内容或静默丢弃输出。
- Remote IM Windows process tree 已归 PR-03，不在本 PR 重复修改。

### 13.8 测试和实机验证

- Deferred SSH responses 证明失败保留数据、stale 标记和 latest-generation wins。
- fake dead tunnel、并发 tunnel 请求和 lifecycle cleanup。
- 同 URL 双下载以相反顺序完成，两个目标内容正确且无 staging 残留。
- 并发 CLI install、checksum failure、publish interruption、旧 binary 保留。
- PTY blocking stdin 和持续大输出测试证明其他 terminal 可 write/resize/kill，pending memory 有界，输出无丢失。
- macOS、Windows、Linux 实机连接真实 SSH test host，验证断网恢复、kill tunnel、load more 和退出清理。
- 三端执行普通/同 URL 下载、CLI 安装中断与重试、`yes` 或等效持续输出、阻塞 stdin 和另一 terminal 操作。
- kill/close 目标为 2 秒内返回控制权；App 退出后没有测试 tunnel/PTY/installer process。
- 相邻回归覆盖 SSH rename、普通 browser、checksum/link、terminal resize/tab persistence。
- Upgrade/rollback 验证旧 CLI、SSH host 配置和 terminal 设置仍可用。

### 13.9 退出标准

- SSH 错误零数据清空、response 零倒退；
- App 退出零 orphan tunnel；
- 同 URL 下载和并发安装零互相覆盖；
- PTY 全局操作不被单 session I/O 阻塞，内存有界且输出不丢；
- QA 证据位于 `docs/qa/YYYY-MM-DD-pr-08-io-races-real-device.md`。

## 14. PR-09：封闭式剩余债务清理

### 14.1 唯一允许的五项

#### A. Review focus identity

- focus request 携带 session ID、project identity、path 和单调 token。
- 只有当前 session/project 匹配时消费一次。
- 切换 session/project、关闭 Review 或完成消费后清除。
- 状态留在 side-workbench domain hook/component，不向 `AppWorkbench` 新增 `useState`。
- 测试会话 A 点击后立即切到 B、相同路径跨项目、重复 token、正常单会话定位。

#### B. official-aux pipes

- child 运行时并发持续 drain stdout 和 stderr，不等进程退出后再读。
- stdout 最多收集 8 MiB；上限内的成功 stdout 逐字节保持当前返回语义，超过上限立即取消并返回结构化 `output_too_large`，不得把截断内容当成功结果。
- stderr 持续 drain，只保留最后 256 KiB 的有界 ring buffer；现有面向用户的错误预览长度继续保持不变。
- timeout 同时 kill、reap child 并 join/drain reader tasks。
- 测试 stdout 大于 OS pipe 但低于上限、stdout 超过 8 MiB、stderr 超过 256 KiB、两路同时大、超时和正常小输出。

#### C. ChatCut pin fail-closed

- fetch、checkout、rev-parse 等每个 Git 命令都检查 spawn error 和退出码。
- 最终强制 `git rev-parse HEAD === vendor/chatcut-agent-plugin.pin` 中的 commit。
- 旧 clone、离线 fetch、缺 pin 和 checkout failure 都返回失败，不能输出 `RESULT: PASS`。
- 使用本地 Git fixture 测试，不依赖网络才能覆盖失败路径。

#### D. Hooks portal combobox

- 原生 `datalist` 替换为项目 portal combobox，保留自由输入。
- 支持 keyboard navigation、Escape、Enter、outside click、busy/error、空选项和稳定层级。
- 使用现有 solid menu chrome，不新增平行 skin，不被 panel clip。
- guard 扩展为禁止产品路径中的 `datalist` 和 native select。

#### E. HtmlBrowser i18n

- `loading`、`empty`、`read failed` 使用 `createT(locale)` 和 15 locale lockstep keys。
- 原始内部错误作为脱敏 detail，不直接替代用户文案。
- `HtmlBrowser` 从 ResourceViewer 接收 locale；正常 `srcDoc` 行为保持不变。

### 14.2 明确排除

- Remote IM draft reset 属于 PR-04。
- SSH watch/tunnel 属于 PR-08。
- goalOrch cycle 和质量文档属于 PR-00。
- 不做其他大文件拆分、CSS 整理、依赖升级或泛化 lint cleanup。

### 14.3 测试和实机验证

- 跨会话/项目 Review focus 不串线；正常 focus 仍同步打开并滚动。
- fake child 产生超过 OS pipe capacity 的输出仍按时完成；timeout 后无 child。
- ChatCut local fixture 覆盖正确 pin 和所有 Git 失败。
- combobox 键盘、鼠标、portal、scroll/stacking 和自由路径。
- HTML loading/empty/failure/ready 与 15 locale catalog 对齐。
- macOS 实机执行双会话快速切换、真实 official-aux fallback、ChatCut 正常/断网启动、Hooks 菜单和 HTML 错误态。
- 相邻回归覆盖普通 Review、official-aux 小输出、正确 pin、Hooks 自定义路径和 HTML 正常脚本页面。
- Upgrade/rollback 不改变 checkout、settings 或 persisted side-workbench state 格式。

### 14.4 退出标准

- 五项回归全部有红绿证据，且 PR diff 不包含第六类清理；
- App shell/AppWorkbench 指标不增长；
- QA 证据位于 `docs/qa/YYYY-MM-DD-pr-09-residual-debt-real-device.md`。

## 15. 错误语义总则

为避免“修了 bug 但用户看到新假象”，所有 PR 遵守统一错误语义：

- **已提交才成功：** Host/磁盘/凭据/队列/进程 owner 明确成功后才显示成功。
- **未知不是成功：** 无法判断结果时使用明确 unknown/degraded/stale，不猜测成功或清空状态。
- **失败保留可恢复状态：** 表单草稿、last-known-good 列表、旧凭据、旧 binary 和旧配置在失败时保留。
- **取消有 owner：** timeout、stop、reload、delete 和 unmount 必须能追踪并终止所创建资源。
- **不泄漏敏感内容：** 日志和 QA 证据脱敏 prompt、message、token、API key、extra headers 和二维码 payload。
- **不使用系统默认 UI：** 新错误、选择和确认沿用现有 App dialog/menu/control。
- **所有用户文案 i18n：** `en` 为 key authority，15 locale 保持 lockstep。

## 16. 合并、回退和停止条件

### 16.1 合并条件

一个 PR 只有同时满足以下条件才可合并：

- scope 与本设计一致；
- characterization 和 red/green 回归证据完整；
- 全自动门禁和 GitHub Actions 全绿；
- 对应真实设备矩阵全部执行；
- happy/fault/adjacent/upgrade/rollback 全部通过；
- QA 文档已由 reviewer 检查，且没有 secrets；
- branch 相对最新 `main` 无未解决冲突；
- 未通过调高预算、删除测试或降低错误强度达标。

### 16.2 回退单位

- PR-00 至 PR-07、PR-09 以整 PR 为回退单位。
- PR-08 可按五个职责提交独立 revert，但仅在依赖测试仍通过时使用；否则整 PR 回退。
- PR-03 回退时必须确认没有新 runtime 正在运行；PR-05/06 回退前必须正常关闭 App 并确保无迁移/活动 queue。
- 回退后重新执行该 PR 的相邻功能和 profile 可读性验证。

### 16.3 立即停止并重新设计的条件

- 发现会造成真实凭据、用户配置、会话或文件丢失的不可逆迁移；
- 修复必须改变已确认的产品行为或公开 API，而本设计没有定义；
- 无法在真实设备安全注入故障；
- 需要新外部服务、付费账号或生产 tenant 权限；
- 任一 PR 无法保持可独立回滚；
- upstream 在实施期间合入重叠修复，导致本设计的根因或边界改变。

## 17. 完成定义

本修复计划只有在 PR-00 至 PR-09 全部依次合并、各自 QA 证据齐全、最终 `main` 全门禁绿色，并完成一次最终 macOS/Windows/Linux 综合 smoke 后才算完成。

完成后执行 branch hygiene：确认 PR 状态和提交已在 `main`，移除空闲 worktree，删除已完成的 local/remote `codex/health-pr*` 分支，再 `git fetch --prune` 核验。规格分支 `codex/codebase-health-audit` 只有在其文档已经进入 `main` 且没有独有工作时才能删除。

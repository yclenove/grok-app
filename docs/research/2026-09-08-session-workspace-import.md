# Codex、Claude Code、Cursor 会话与工作区导入调研

日期：2026-09-08。状态：源码与公开文档调研完成；尚未实现导入器，未读取或迁移本机私人会话。

工作分支：`feat/session-workspace-import-design`。基线：上游 `RongleCat/grok-app` 的 `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。独立 worktree，不修改壁纸、Computer Use 或其他 PR 分支。

配套：[产品方案与分批开发计划](../plans/2026-09-08-session-workspace-import.md)、[实施契约](../plans/2026-09-08-session-workspace-import-contracts.md)、[验收矩阵](../plans/2026-09-08-session-workspace-import-acceptance.md)。

## 1. 结论

可以做，而且项目已有可复用的基础，但没有找到 Codex、Claude Code、Cursor 的专用会话及工作区迁移实现。不能只在现有文件选择器里增加 `.jsonl` 后缀：三家消息格式、工具记录、分支关系和目录信息不同，Cursor 还有多代数据库存储。

建议交付一个统一的“导入会话与工作区”入口，完成发现、预览、目录映射、批量导入、历史阅读和 Grok 续聊。导入保留来源记录；继续工作时创建新的 Grok 会话，引用导入快照，不把外部 UUID 冒充 Grok Build 会话 ID。

最有价值的参考是：

| 参考 | 已核实的能力 | 应借鉴什么 |
|---|---|---|
| OpenAI Codex | 已有 Claude Code / Cursor 迁移模块、检测、进度、导入账本、严格后缀追加 | 适配器划分、预览与导入分离、来源指纹、重复导入处理 |
| DeepSeek Harness（用户所指 dsh） | 会话树与附件 ZIP 导出、持久化格式迁移、独立 workspace registry | 原始事件与显示内容分离、完整迁移包、分版本解析、工作区与历史解耦 |
| pi | JSONL 会话树、`id/parentId`、分支/压缩事件、JSONL 导入导出 | 保留分支与来源关系，避免把整棵树线性拼接 |
| cursor-history | Composer SQLite、Store SQLite、JSONL 的发现与合并 | Cursor 多格式覆盖、来源优先级、一致读取和完整性报告 |

这里的 Claude 首版指 **Claude Code**，包括兼容该存储格式的 SDK 会话。Claude 网页聊天、Cowork 私有存储、各家云端任务不因产品名称相同就被视为已支持；它们可通过受支持的导出文件进入通用导入，专用格式另列版本。

## 2. Grok App 当前实现

以下均核对了本分支源码，而非仅依据旧规划。

| 位置 | 已有行为 | 扩展边界 |
|---|---|---|
| [`session_import.rs`](../../src-tauri/src/session_import.rs) | JSON 消息数组；对象下的 `messages/conversation/items`；Markdown `User/Assistant`；创建本地 journal | 仅提取 user/assistant 文本；统一使用导入时间；附件、工具、来源 ID、分支与去重均未建立 |
| [`commands/account.rs`](../../src-tauri/src/commands/account.rs) | `session_import_transcript` 与文件选择命令；支持 md/txt/json/markdown | 选文件后一次读入并导入，没有批量预览、进度、取消、尺寸预算或来源识别 |
| [`AccountPanel.tsx`](../../src/components/AccountPanel.tsx) | 官方账号已登录区域中的“导入对话”入口 | 本地迁移不应依赖官方账号登录，入口应独立于账户 |
| [`cli_sessions.rs`](../../src-tauri/src/cli_sessions.rs) | 读取 Grok Build history，按 `agent_session_id` 避免重复，按 cwd 关联/添加未信任项目 | 原生 Grok 恢复专用；不能直接用于外部厂商的 ID |
| [`CliSessionsPanel.tsx`](../../src/components/settings/CliSessionsPanel.tsx) | Grok CLI 列表、单个导入并打开、批量导入 | 批量 UI 调用上限为 50；不能原样当作“全部外部会话导入” |
| [`store.rs`](../../src-tauri/src/store.rs) | App 会话索引与 journal；项目 path/trusted/ssh_alias；会话 worktree 元数据 | 项目以一个执行目录为中心，尚无通用外部来源/快照/多根工作区模型 |
| [`projectSpaces.ts`](../../src/lib/projectSpaces.ts) | 侧栏分组；一个项目归属一个 Space | Space 是 UI 分组，不代表复制仓库或多根执行环境 |
| [`session_attach.rs`](../../src-tauri/src/session_attach.rs) | 将其他 App 会话作为发送上下文 | 可借鉴引用和上下文预算；不是全量历史迁移格式 |
| [`cc_switch_import.rs`](../../src-tauri/src/cc_switch_import.rs) | CC Switch 提供商配置导入 | 不是 Claude/Cursor 会话导入，不应误复用其账号配置路径 |

现有导入器还存在两项应在本功能接入时解决的问题：不识别的 JSON 对象可能落回 Markdown 解析；Markdown 识别未区分代码块中的 `User:`。新检测流程须用格式证据，未知格式明确报错；保留旧格式能力并补真实回归样本。

### 续聊的实际限制

[`session_manager/types.rs`](../../src-tauri/src/session_manager/types.rs) 的 `build_history_bootstrap` 当前只传最近 **16 条** user/assistant 消息，每条最多 **2,000 字符**，正文合计最多 **14,000 字符**。[`turn.rs`](../../src-tauri/src/session_manager/turn.rs) 在首次发送时加入该前缀。

所以“页面能看到几百轮历史”不等于模型能继续那几百轮工作。新功能必须增加可检查的交接上下文、来源引用和按需读取；不能以成功写入 `messages.json` 作为完成条件。

## 3. Codex

### 官方读取接口

[官方 App Server 文档][codex-doc]提供 `thread/list`、`thread/read`、原生 resume/fork，以及实验性的历史分页接口。`thread/read` 可以读历史而不 resume；`thread/list` 支持 cwd、归档等过滤。运行时可生成对应版本的 TypeScript / JSON Schema。

但官方文档当前明确写到：`historyMode=paginated` 的创建尚不支持，已有 paginated 记录可以列摘要，完整历史读取、分页和恢复可能返回不支持。**不能因为接口名字存在，就承诺所有 Codex 存储版本均能完整迁移。**

推荐本地离线快照解析作为基础能力；App Server 作为可选的、经版本探测的兼容路径。不要为扫描启动模型请求，也不要调用 `thread/resume`、`turn/start` 或修改原线程。启动 App Server 本身可能触发本地维护，若使用此路径，应在隔离的源快照上验证行为，而非承诺源目录零写入却直接启动原配置。

### 文件与工作区

传统本地记录通常在 `CODEX_HOME`（默认 `~/.codex`）下的 `sessions/`、`archived_sessions/`；JSONL 包含 `session_meta`、`response_item`、`event_msg`、`turn_context` 等不同记录。消息正文与 UI 事件可能重复描述同一次输出，需要按事件身份/来源规则归一，不能按文字相同一概去重。

当前源码还有 SQLite 元数据、`thread_turns/thread_items` 投影和 `projects/project_roots` 表。必须识别支持的 schema 与 `historyMode`；文件、数据库可能是正文、摘要或部分投影，不能无条件拼接。工作区优先使用显式项目根和 cwd；只有会话 cwd 时，可生成“从会话推断的项目”并标明来源，不能宣称恢复了 App 全部收藏和工作区设置。

实现前需固定代表性版本，核对 rollout 身份、分叉、回退和压缩事件。遇到只有摘要、正文已删除、未知分页存储的情况，应显示“仅发现元数据/格式暂不支持”，不能计入完整导入成功。

### Codex 自身的跨工具迁移值得参考，但不能全盘照搬

固定源码中 `external-agent-migration` 明确包含 `cla` 与 `cur` 适配器。App Server 对应 `externalAgentConfig/detect`、`externalAgentConfig/import` 和进度/完成通知，导入项包含 `SESSIONS`。

源码核实到的具体限制：

- 默认检测最近 **30 天、最多 50 个**会话；这是该实现的筛选策略，不是外部记录不存在。
- Cursor 检测主要扫描 `.cursor/projects/*/agent-transcripts/**/*.jsonl`，排除 `subagents`，并非全代 SQLite 历史扫描器。
- Claude/Cursor 解析跳过 `isMeta/isSidechain`；把工具调用/结果转换为文本提示，跳过 thinking，未知内容块用占位提示。因此它是有损迁移参考，不是全保真目标。
- 账本记录来源路径和 SHA-256；仅对未运行、未分歧、未归档的目标追加完全匹配的缺失后缀。借鉴这些边界；Grok App 使用只读导入快照加续聊子会话，可进一步避免外部追加污染本地续聊。

## 4. Claude Code

[官方会话文档][claude-sessions]说明：默认 `~/.claude/projects/<project>/<session-id>.jsonl`，可由 `CLAUDE_CONFIG_DIR` 和新版本的 `CLAUDE_CODE_PROJECT_DIR_NAME` 改变。目录名经过不可逆替换，长路径还可能截断加 hash；应读取记录中的 cwd，不能把目录名里的 `-` 全部还原为 `/`。

官方同时明确 JSONL 是内部格式、可能随版本变化；`/export` 导出供人阅读的文本。SDK 的 `listSessions/getSessionMessages` 提供列举与读取接口，可以作为可选读取桥，但不应为了桌面导入强制安装另一套 Node/Python/Claude 运行环境。

适配器需要覆盖：

- 标题、sessionId、cwd、时间、消息 UUID/父关系；不把索引缓存当正文。
- `user/assistant` 内的内容块、`tool_use/tool_result` 配对、图片引用、可用的压缩摘要。
- 子代理 transcript 独立关联，不并成主会话的用户输入；元数据、系统提醒保留类别。
- 原生 resume/rewind 能力属于 Claude 运行时；导入后不恢复 shell、权限、checkpoint 或仍在运行的子任务。
- 缺失正文可能来自保留期限或禁用持久化，不能通过重新登录凭空恢复。

[SessionStore 文档][claude-store]支持远端持久化与 subkeys，但恢复需要匹配 cwd，且摘要旁车是 SDK 不透明状态。它适合研究备份/迁移边界，不是 Grok App 直接接管 Claude 原生运行状态的依据。

## 5. Cursor：需要同时考虑三种来源

Cursor 官方[共享 transcript 文档][cursor-share]描述链接分享和 Fork to Cursor；这不是本机历史批量读取 API，也不应要求用户先公开聊天。以下本地结构由 Codex 开源迁移器和社区读取器源码佐证，属于 **版本相关兼容格式**，不是 Cursor 官方稳定契约。

| 来源 | 候选位置/结构 | 处理要求 |
|---|---|---|
| Composer / 旧 Chat | `User/workspaceStorage/*/{workspace.json,state.vscdb}`、`User/globalStorage/state.vscdb` | 工作区库索引与全局正文可能分离；识别 `ItemTable`、`cursorDiskKV`、`composerData:<id>`、`bubbleId:<composerId>:<bubbleId>` 等版本 |
| Store | `.cursor/chats/<hash>/<id>/{meta.json,store.db}`、`.cursor/acp-sessions/<id>/...` | `store.db` 有元数据及 blob 关系，不能作为普通 JSON 文件；按支持版本读取关系和完整性 |
| Agent transcript | `.cursor/projects/<encoded>/agent-transcripts/` 的 JSONL；部分历史导出为文本 | 常含 `role` + `message.content`；缺失 cwd 时用可信工作区元数据或用户映射 |

Composer 根目录：Windows `%APPDATA%/Cursor/User/`，macOS `~/Library/Application Support/Cursor/User/`，Linux 通常 `~/.config/Cursor/User/`；Store 根通常为 `~/.cursor/`。自定义 user-data-dir、便携版、多个 profile、WSL 均需手动选根覆盖，不能只写死默认路径。

同一会话可在多处出现。社区读取器以可用 Store DB 为该 Store 会话的主正文，JSONL 为替代来源；跨栈合并另外检查身份与内容冲突。Grok App 应先按 ID 与可核实 lineage 聚合来源，再选择正文；冲突保留两个版本并提示，禁止按标题/首句自动合并。

数据库需要 SQLite 一致读取，涵盖 WAL 中已提交数据。不要只复制主 `.vscdb` 文件；不要对活库使用忽略 WAL 的 `immutable=1`，不要 VACUUM、checkpoint 或“修复”源数据库。锁定或快照设施错误不应静默变成“没有会话”。

## 6. DeepSeek Harness 与 pi

已核实用户所指为 [`deepseek-ai/deepseek-harness`][dsh]。

DSH 的 `session-log-export` 导出 root、后代会话及附件到 ZIP；通用文件按摘要组织，Host 分块压缩，浏览器不先接收整个归档到内存。`workspace` 独立管理目录、名称、顺序与会话归属，删除工作区不删原目录和会话。

它的持久化系统具有显式版本分类（current / migration-required / unsupported / malformed）、相邻格式迁移、只读恢复与写入发布分离。当前默认压缩为 Zstandard，文件可能是 `session.v2.jsonl.zstd`，因此未来增加 DSH 导入时不能只靠 JSONL 文本读取。调研到上述原生能力；未将它描述为已支持 Codex/Claude/Cursor 的现成导入器。

[pi 会话格式][pi-format]明确以 `id/parentId` 组成树，版本 1 为线性、2 引入树、3 调整 custom 消息角色，记录包含工具、分支摘要、压缩与模型变更。README 提供 `/export` 到 HTML/JSONL 及 `/import` JSONL。应保留源树，按一个选定叶节点生成续聊上下文；不能把其他分支上的操作视为已经发生。

首版必需来源仍为 Codex、Claude Code、Cursor。DSH/pi 作为设计参考并保留 adapter 接口，专用导入另排后续切片，避免扩大首版验收范围。

## 7. 主要决策与未验证边界

1. 历史归档、项目关联、Grok 续聊是三个独立能力；同一次向导可完成前两个，用户开始续聊时才调用模型。
2. 默认导入目录关系和可选择的会话附件，不复制整个源项目、不自动 checkout、不恢复原工具进程。便携工程文件包作为明确的后续功能设计。
3. 工具历史是可读记录，不是可执行 ACP 通知。原 system/rules/config 作为来源信息，不自动激活。
4. JSONL/SQLite 都需版本适配、损失报告与合成 fixture。不能承诺未知版本或云端未下载内容可恢复。
5. 三平台的发现、路径和解析从首版设计；本轮没有 Windows/macOS/Linux 私人数据实测，性能目标也尚未测量。
6. 复用现有 store、项目和 UI 控件；新状态进入导入领域模块，遵守 AppWorkbench 增长冻结和 15 语言要求。

## 8. 第二轮源码复核：接入风险与方案修正

本节继续核对同一基线的应用源码；没有修改业务代码或实际导入数据。以下改变了 v0.1 的部分设计，而不是新的已实现能力。

| 已核实的行为 | 对方案的影响 |
|---|---|
| [`setupGatePro.ts`](../../src/lib/setupGatePro.ts) 与 [setup.md](../llm-wiki/setup.md) 要求 CLI 可用才能进入工作台，账号可以跳过 | “离线导入”不等于“未安装 CLI 即可进入应用”；首版沿用启动门槛 |
| [`commands/session_p1.rs`](../../src-tauri/src/commands/session_p1.rs) 的 session_messages 返回完整数组，默认还可能 reconcile；通用 delete 直接删除普通会话目录 | 不能把超大导入历史直接塞 messages.json；选择独立导入目录、专用分页/删除接口 |
| [`store.rs`](../../src-tauri/src/store.rs) 的 archived 只是显示标志；create_session 自行生成 UUID、先写索引再建 journal | 导入类型与 archived 分开；子会话创建需要可预分配 ID 的发布接缝，不能靠重复调用现有函数实现幂等恢复 |
| [`store_lock.rs`](../../src-tauri/src/store_lock.rs) 具有全局进程互斥量并配合文件锁 | 导入跨 catalog/project 操作不得嵌套获取 store 锁，也不能长时间持锁解析文件 |
| [`acp_client.rs`](../../src-tauri/src/acp_client.rs) 的 open_session_at 按 cwd 构建 MCP，SSH 时直接使用空列表；配置构造失败也可能返回空列表继续连接 | 需要显式传 App session 范围和工具状态；“ACP 连接成功”不能证明历史工具存在 |
| [`extensions.rs`](../../src-tauri/src/extensions.rs) 自定义主模型 + official-aux 时可省略其他用户 MCP；热更新还会重建 MCP | 应用启用的历史只读工具需要独立合成，并覆盖 new/load/fork/热更新；不能藏在会被省略的普通扩展配置里 |
| [`session_attach.rs`](../../src-tauri/src/session_attach.rs) 能展开 App 内部聊天 token | 来源正文中的类似 token 要保留为文字，不能让外部记录触发其他本地会话读取 |
| [`commands/doctor_p1.rs`](../../src-tauri/src/commands/doctor_p1.rs) 的 export_session_bundle 是诊断包；session_cli_export 依赖原生 agent ID | 现有诊断 ZIP 不当作用户便携会话迁移格式；导入页不调用 Grok CLI 导出冒充跨厂商导出 |
| [`session_content_search.rs`](../../src-tauri/src/session_content_search.rs) 扫描原生 journal，最多 200 个会话并跳过超过 2 MiB 的 journal | 导入历史要有独立检索索引与 typed search hit；直接复用原扫描会让大量旧会话/长历史查不到 |

因此 v0.2 将导入记录放在独立 catalog，以同一侧栏 UI 展示；只有续聊创建普通 Grok 子会话。对历史授权要覆盖 revision、分支、记录和字段，避免用户排除内容后仍被检索工具读到。模型请求断线时需要明确“不知道是否已执行”的状态，不能将本地幂等创建误写为模型请求恰好一次执行。

仍需实验的问题有：源 SQLite 辅助文件是否变化、三平台逐会话 MCP 的 CLI 支持、多个来源库之间的一致性、派发状态核对、冷/热缓存性能。没有本轮运行证据，不标记通过。

## 9. 证据与固定版本

公开文档查阅日均为 2026-09-08。源码链接固定 commit；动态文档与源码若出现差异，以实际版本探测和验收样本收敛，不按文档中的新字段推断用户本机已支持。

| 来源 | 固定版本 / 许可 | 核实文件 |
|---|---|---|
| OpenAI Codex | `6750f5bd1356fe1553c0fcc9f2632704f3055946` / Apache-2.0 | [迁移模型][codex-model]、[Cursor 发现][codex-cur]、[内容提取][codex-extract]、[账本][codex-ledger]、[追加策略][codex-append]、[项目 schema][codex-projects]、[历史 schema][codex-history] |
| DeepSeek Harness | `b0a7d2ce3b4c19d7452e364b2d7acbfa87e707ed` / MIT | [导出][dsh-export]、[格式目录][dsh-format]、[持久化][dsh-storage]、[工作区][dsh-workspace] |
| badlogic/pi-mono | `c1d4c801114545f47c440921d8b3e04aeb1e565d` / MIT | [格式][pi-format]、[README][pi-readme] |
| S2thend/cursor-history | `aa0d4698289bd3801d2bac07895fe7b6adbc5ce0` / MIT | [来源说明][cursor-history]、[Store 路径][cursor-paths]、[Store DB][cursor-db]、[Composer 读取][cursor-storage] |
| Anthropic 官方 | 动态文档 | [会话][claude-sessions]、[SDK 会话][claude-sdk]、[SessionStore][claude-store] |
| Cursor 官方 | 动态文档 | [共享 transcript][cursor-share] |

本轮只借鉴架构和格式，没有复制第三方实现或引入运行依赖。后续若移植源码，须保留对应版权和许可说明；Apache-2.0 源码同时检查 NOTICE 义务。

[codex-doc]: https://learn.chatgpt.com/docs/app-server
[codex-model]: https://github.com/openai/codex/blob/6750f5bd1356fe1553c0fcc9f2632704f3055946/codex-rs/external-agent-migration/src/model.rs
[codex-cur]: https://github.com/openai/codex/blob/6750f5bd1356fe1553c0fcc9f2632704f3055946/codex-rs/external-agent-migration/src/detect/sessions/cur.rs
[codex-extract]: https://github.com/openai/codex/blob/6750f5bd1356fe1553c0fcc9f2632704f3055946/codex-rs/external-agent-migration/src/sessions/records_common.rs
[codex-ledger]: https://github.com/openai/codex/blob/6750f5bd1356fe1553c0fcc9f2632704f3055946/codex-rs/external-agent-migration/src/sessions/ledger.rs
[codex-append]: https://github.com/openai/codex/blob/6750f5bd1356fe1553c0fcc9f2632704f3055946/codex-rs/external-agent-migration/src/sessions/append.rs
[codex-projects]: https://github.com/openai/codex/blob/6750f5bd1356fe1553c0fcc9f2632704f3055946/codex-rs/state/migrations/0049_projects.sql
[codex-history]: https://github.com/openai/codex/blob/6750f5bd1356fe1553c0fcc9f2632704f3055946/codex-rs/state/thread_history_migrations/0001_thread_history.sql
[claude-sessions]: https://code.claude.com/docs/en/sessions
[claude-sdk]: https://code.claude.com/docs/en/agent-sdk/sessions
[claude-store]: https://code.claude.com/docs/en/agent-sdk/session-storage
[cursor-share]: https://cursor.com/help/ai-features/shared-transcripts
[cursor-history]: https://github.com/S2thend/cursor-history/blob/aa0d4698289bd3801d2bac07895fe7b6adbc5ce0/README.md
[cursor-paths]: https://github.com/S2thend/cursor-history/blob/aa0d4698289bd3801d2bac07895fe7b6adbc5ce0/src/core/store-stack/paths.ts
[cursor-db]: https://github.com/S2thend/cursor-history/blob/aa0d4698289bd3801d2bac07895fe7b6adbc5ce0/src/core/store-stack/store-db.ts
[cursor-storage]: https://github.com/S2thend/cursor-history/blob/aa0d4698289bd3801d2bac07895fe7b6adbc5ce0/src/core/storage.ts
[dsh]: https://github.com/deepseek-ai/deepseek-harness
[dsh-export]: https://github.com/deepseek-ai/deepseek-harness/blob/b0a7d2ce3b4c19d7452e364b2d7acbfa87e707ed/packages/session-query/session-log-export/README.md
[dsh-format]: https://github.com/deepseek-ai/deepseek-harness/blob/b0a7d2ce3b4c19d7452e364b2d7acbfa87e707ed/packages/session/session-format/README.md
[dsh-storage]: https://github.com/deepseek-ai/deepseek-harness/blob/b0a7d2ce3b4c19d7452e364b2d7acbfa87e707ed/packages/session/session-persistence-jsonl/README.md
[dsh-workspace]: https://github.com/deepseek-ai/deepseek-harness/blob/b0a7d2ce3b4c19d7452e364b2d7acbfa87e707ed/packages/workspace/workspace/README.md
[pi-format]: https://github.com/badlogic/pi-mono/blob/c1d4c801114545f47c440921d8b3e04aeb1e565d/packages/coding-agent/docs/session-format.md
[pi-readme]: https://github.com/badlogic/pi-mono/blob/c1d4c801114545f47c440921d8b3e04aeb1e565d/packages/coding-agent/README.md

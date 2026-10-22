# 当前格式兼容矩阵

更新：2026-10-22（Claude 接手 H1–H5 进行中）。分支 `handoff/session-import-claude-20261021`，
代码检查点 `572ab60a`。基线原生门为 N814（1318 passed / 0 failed / 103 ignored）与
前端门 F814（899 文件 / 10404 项通过）；用户入口 V814 已于 2026-10-21 提升并通过；
Windows 原生基线 `e6081365`（fmt/compile/strict clippy 通过，4318 library ok / 107 ignored）；
fork CI 快照 `3f799b67` 三平台全绿（Linux x64/arm64 全库、macOS 4,333 ok、Windows 4,353 ok、
前端 902 文件 / 10,418 项全过）；10k 容量门（`77ccf141`）全部发布并通过逐项核验（`90486ae9` 一轮正确性通过但冷读作业页 647 ms 超预算，已修）；
实机运行覆盖 Windows V815/V816/V817/V818（CDP 交互自动化）与 Linux arm64 XFCE（L1/L2 真机窗口）。

验证级别定义：
- `verified-by-real-data`：通过真实厂商软件导出或生成的实际资料（脱敏或原样录制）并在生产管线/测试中严格验证。
- `verified-by-fixture`：仅通过依据公开规范、逆向源码或社区参考构造的合成 fixture 验证，未在真实厂商生成资料上实测。
- `unverified`：尚未取得真实厂商活库/资料，或提取脚本尚未运行，格式行为未实测验证或明确 fail closed 拒绝。

---

## 1. 真实厂商资料实测状态（2026-10-21/22 H1 实证）

| 来源 / 格式 | 软件版本 / 平台 | 资料来源与 Fixture | 验证级别 | 当前处理与已验证特性 | 证据与测试位置 |
| --- | --- | --- | --- | --- | --- |
| Codex rollout JSONL | codex-cli 0.160.0 / Windows | owner 录制演示会话（A1–A8），经 `scrub.py` 白名单脱敏生成 `src-tauri/tests/fixtures/session-migration/codex-0.160/`（3 个文件） | `verified-by-real-data` | 1. 工具输出为 content-item 数组：文本与图片（`view_image`）按序保留为 Asset，不误作空串；<br>2. 显式失败标志（`success: false`）映射为 `ToolOutcome::Error`；未知形状保留为 opaque 占位并标 partial；<br>3. 注入上下文（AGENTS.md、`<environment_context>`）与 developer 消息归入隐藏系统事件（System role，不作标题、不计入轮次）；<br>4. 回退分支：0.160 不写 rollback 记录，而是经 resume 生成带 `history_base` 的新文件；独立会话 `<thread>_<branch>` 经两遍流式校验拼接已校验的父前缀（父缺失则作为独立 partial 不拼接）；<br>5. 加密推理项如实披露为 UnsupportedItem 并标 partial，绝不解码 | `codex_tool_output_tests`、`engine_codex_native_tests`（scan→preview→publish→reader 完整生产链路）；Windows V815/V816/V817 桌面实机及 Linux arm64 L1/L2 |
| Cursor agent transcript | Cursor 3.23.12 / Windows | owner 录制，原样 fixture `src-tauri/tests/fixtures/session-migration/cursor-3.23/`（`cdfdffc1-…` agent 与 `204eedd5-…` ask） | `verified-by-real-data` | 1. `{"type":"turn_ended","status":"<status>"}` 识别为轮次边界元数据（如 `turn_ended:success`），不再报 `unknown-role` 导致 partial；<br>2. `<timestamp>`/`<user_query>` 精确包装拆出用户提示词作为消息正文与会话标题，时间解析为 RFC3339 来源时间（星期不符保留 Unknown）；<br>3. 包含未识别包装标签（如 `<attached_files>`）时保留原文不丢弃；<br>4. 无 id 的 `tool_use` 块与对应回答正常配对保留 | `cursor_transcript_323_tests` |
| Cursor Composer / globalStorage DB | Cursor 3.23.12 / Windows | 待 owner 审阅运行提取脚本 `native-demo/extract-cursor-demo-chats.py`（只读，取两 composer 及引用 blob） | `unverified`（真实原厂活库未验证） | 真实格式未验证（workspace DB 新增 `composerHeaders` 表，聊天在 globalStorage）；目前仅完成合成 ItemTable WAL 与 temporal 回归 | 真实原厂证据：—（合成基线见下方矩阵） |
| Claude Code JSONL | — | owner 未登录，测试机无真机导出资料 | `unverified`（真实厂商数据未验证） | 真实格式未验证；目前仅依据 Anthropic SDK 公开源码（commit `23bb0157`）构建的合成 fixture 通过测试 | 真实原厂证据：—（合成基线见下方矩阵） |

版本与协议锁定说明：
Codex evidence 的 `adapter_version` 独立记录为 `1.0.7`（`CODEX_ADAPTER_VERSION`）。全局 `NORMALIZER_VERSION` 保持 `1.0.6`，因为 Cursor 历史身份关联协议与已存 provenance 记录按值锁定它；Cursor transcript、Claude Code 及 Generic 的 evidence 版本同理保持 `1.0.6`。只有 3.x / 0.160 新形状（如包装、`turn_ended`、数组工具输出）的输出改变，旧形状逐字节不变。重导入因规范化内容摘要不同生成新 revision，旧归档不原地改写。

---

## 2. 格式族完整兼容矩阵

| 格式族 | schema fingerprint | 验证级别 | 支持状态 | 格式特性与边界说明 | 对应证据与测试 |
| --- | --- | --- | --- | --- | --- |
| Codex legacy JSONL | `codex-rollout-jsonl-v1` | `verified-by-real-data`（0.160 特性）<br>`verified-by-fixture`（旧版与合成） | 支持（Complete / Partial） | session_meta / response_item / event_msg；流式增量与 final 合并去重，真实重复保留；0.160 数组/图片工具输出、系统级环境/AGENTS 注入、`history_base` resume 分支父前缀拼接；旧版合成 rollback/fork 分支保留；加密推理标记 partial | `src-tauri/tests/fixtures/session-migration/codex-0.160/`、`codex/g1-main.jsonl`、`codex/g4-dup.jsonl`、`temporal-matrix.json`；`codex_tool_output_tests`、`engine_codex_native_tests`、`codex_tests` |
| Codex metadata/paginated | `codex-thread-history-metadata` | `verified-by-fixture`（拒绝逻辑）<br>`unverified`（真实原厂 DB 格式） | 明确拒绝 (Unsupported) | `metadata_only_db` 拦截，报 `ErrorCode::SchemaUnsupported` / `ImportOutcome::Unsupported`，不作为空会话导入 | `codex.rs::metadata_only_db`、`codex_tests::metadata_only_projection_is_unsupported_not_an_empty_session` |
| Claude Code JSONL | `claude-code-jsonl-v1` | `verified-by-fixture`<br>（真实厂商数据 `unverified`） | 合成测试通过 | UUID / 父链恢复、tool_use 与 tool_result 明确分型、sidechain 隔离、首条有效用户消息作为标题；1.0.2 typed compact summary（`logicalParentUuid` 切断 live parent）；目录名不作为工作区路径 | `src-tauri/tests/fixtures/session-migration/claude/`（`g1-tools.jsonl`、`sidechain.jsonl`、`compaction.jsonl`，参考 SDK `23bb0157`）；`fixture_tests::claude_tool_result_not_user`、`claude.rs` 单测、`claude_compaction_tests` |
| Cursor agent transcript | `cursor-transcript-jsonl-v1` | `verified-by-real-data`（3.23.12 特性）<br>`verified-by-fixture`（基础结构） | 支持（Complete / Partial） | `{role, message.content}` 基础解析；3.23.12 `turn_ended:<status>` 边界识别；`<timestamp>`/`<user_query>` 精确包装剥离并还原 RFC3339 来源时间；无 id 工具调用保留；未识别包装保留原文；末尾不完整行标记 partial | `src-tauri/tests/fixtures/session-migration/cursor-3.23/`、`cursor/transcript.jsonl`；`cursor_transcript_323_tests`、`fixture_tests::cursor_transcript_two_messages`、`cursor_tests` |
| Cursor Composer ItemTable | `cursor-composer-itemtable-v1` | `verified-by-fixture`<br>（真实厂商活库 `unverified`） | 合成测试通过 | SQLite `ItemTable` 中 `fullConversationHeadersOnly` 结构；数字 type 1/2，epoch-ms / RFC3339 混合时间戳，从伴随数据库补齐缺失 bubble，WAL 只读并发。3.23.12 新增 `composerHeaders` 表及 globalStorage 活库未实测 | `temporal-matrix.json`、`cursor_tests.rs::composer_wal_*`、`engine_source_fidelity_tests` |
| Cursor legacy Chat ItemTable | `cursor-legacy-chat-v1` | `verified-by-fixture`<br>（真实厂商活库 `unverified`） | 合成测试通过 | 扫描 `ItemTable` 内 `chatData:` 前缀 key 族，定位内联 id 与正文，不把内部内联 id 误作为外部气泡关联 | `temporal-matrix.json`、`cursor_tests.rs::unknown_keys_are_ignored_and_legacy_chatdata_reads`、`engine_source_fidelity_tests` |
| Cursor Store JSON blobs | `cursor-store-blobs-v1` | `verified-by-fixture`<br>（真实厂商活库 `unverified`） | 合成测试通过 | 从 sidecar selected root 出发遍历 JSON blob 树，识别树状分叉并选取活动叶节点；成环或多父节点告警；缺失子节点记 partial；拒绝无 root 的 all-blob fallback，不代表原厂实测 | `cursor_tests.rs::store_tree_*` |
| Cursor Store DB/Merkle wire | `cursor-store-merkle-v1` | `verified-by-fixture`<br>（真实厂商活库 `unverified`） | 合成测试通过 | 社区开源逆向协议（S2thend `aa0d4698`）；解析 DB meta hex、selected root、有序 0a/12 Merkle 引用、plain JSON/protobuf field4 叶节点；只读可达节点；工具归属与现代 results 块配对；截断或坏节点标记 partial；非 Cursor 官方契约，无原厂实测 | `store-wire.json`、`cursor_store_wire_tests.rs`、`engine_source_fidelity_tests` |
| 通用 JSON 数组 | `json-array` | `verified-by-fixture` | 合成测试通过 | 消息对象数组，提取 role/content/timestamp 保留来源时间 | `src-tauri/tests/fixtures/session-migration/generic/messages.json`、`generic_stream.rs`、`generic_spool_raw.rs` |
| 通用 messages 对象 | `json-object` | `verified-by-fixture` | 合成测试通过 | 包含 `messages` 数组字段的顶层 JSON 对象；不代表任意 JSON 对象受支持 | `generic.rs::bom_and_crlf_json_and_markdown_parse`、`generic_tests_v10.rs` |
| 通用 Markdown | `markdown` | `verified-by-fixture` | 合成测试通过 | `## User` 与 `## Assistant` 标题分段；代码围栏内的 `User:` / `Assistant:` 不作角色拆分；单条消息上限 32 MiB | `src-tauri/tests/fixtures/session-migration/generic/fenced.md`、`adapters::fixture_tests::generic_fenced_and_illegal`、`generic_spool.rs`、`generic_stream.rs` |
| 非法 JSON 对象 | — | `verified-by-fixture` | 明确拒绝 (FORMAT_UNSUPPORTED) | 顶层为非法/不符合格式要求的 JSON 对象时，直接返回 `FormatUnsupported` 错误，绝不静默回退到 Markdown 解析 | `src-tauri/tests/fixtures/session-migration/generic/illegal.json`、`generic.rs::illegal_json_object_does_not_fall_back_to_markdown` |

---

## 3. 架构版本定义与演化事实

- 当前核心版本常量：
  - `SCHEMA_VERSION = 1`（导入事件版本）
  - `CATALOG_SCHEMA_VERSION = 2`（目录数据库版本；只读入口不执行 DDL，受控升级至 v2，高于此版本返回 `IMPORT_SCHEMA_TOO_NEW`）
  - `NORMALIZER_VERSION = "1.0.6"`（全局规范化器基线版本；Cursor 历史身份关联协议与已存 provenance 记录按值锁定）
  - `CODEX_ADAPTER_VERSION = "1.0.7"`（Codex 独立适配器版本，记录于 `SourceEvidence.adapter_version`；Cursor、Claude Code、Generic 均写入 `"1.0.6"`）
  - `SELECTION_SCHEMA_VERSION = 2`、`DURABLE_SCHEMA_VERSION = 2`、`LOCATION_REFERENCE_VERSION = 1`

- 历史版本演进与基线门记录：
  - **1.0.0～1.0.3**：初始黄金样本登记（2026-09-08 `fixture-manifest.json`）；temporal 矩阵 10 项生产管线保真回归通过（角色、直接时间、倒退/缺失偏移时间）；V248 边界补丁验证通过（聚焦 22、领域 872、全库 3801 passed / 0 failed）。
  - **1.0.4**：Cursor transcript 身份/标题修复通过完整合成 native 门（V249 聚焦 31、领域 881、全库 3810 passed / 0 failed）：正式 JSONL 发现与归一化共用有界声明 ID 与相对 locator fallback，不将消息 ID 或裸 stem 当会话 ID。
  - **1.0.5**：新增 typed session/composer/legacy-chat 身份与未知 Store 根内 locator 作用域；生产双发布碰撞反例通过；修复 16 个 fixture 隔离消除 HMAC home 依赖（V251 完整 gate 通过：聚焦 41、领域 889、全库 3818 passed / 0 failed）。
  - **1.0.6**：首批 Cursor/local 关联支持：附属根独立 UUID/扩展 schema 1，primary 身份保留 1.0.5 算法，fallback/subagent/未知 Store 按附属 location scope 隔离。Store JSON/wire 在 graph 生成前接固定身份，原始来源与旧归档不改写。扩展首次显式关联才事务性建表，只读/schema 检查无 DDL。N787/N797/N808 及 N814 完整原生门相继通过。
  - **1.0.7（2026-10-21/22）**：Codex 适配 codex-cli 0.160.0 真实厂商资料（数组/图片工具输出、显式失败标志、AGENTS.md/环境注入转系统事件、`history_base` resume 分支两遍流式校验拼接父前缀）；Cursor 适配 3.23.12 真实 transcript（`turn_ended` 边界元数据、`<timestamp>`/`<user_query>` 精确拆出正文与 RFC3339 来源时间）。Codex 独立适配器版本升为 `1.0.7`，Cursor 与全局规范化版本保持 `1.0.6`。

- 当前完整基线与多平台验证证据（2026-10-22）：
  - **Windows 原生基线**（`e6081365`，claudetest，Rust 1.99.0）：fmt、编译、strict Clippy 通过；migration 1318 passed / 0 failed / 103 ignored（与 N814 同数）；全库 JSON 事件运行 4318 passed / 107 ignored（4 个非导入的环境专属测试挂起按规范排除并在 CI runner 覆盖）。
  - **fork CI 全绿**（快照 `3f799b67` = 源 `cbf34895`）：Linux x64 全库、Linux arm64 全库、macOS（4,333 passed）、Windows（4,353 passed，含环境排除的 4 项全部通过）、Frontend（902 文件 / 10,418 项全过）。
  - **容量门**：10,000 条合成测试负载在安静机器上优化构建实测（`77ccf141`；`90486ae9` 一轮冷读作业页超预算，已修），全部发布并通过逐项核验（条目、修订、回执、阅读事件全量一致），冷读作业页单次头解码且无逐项 BLOB 句柄，作业终态 Completed。

---

## 4. 可追溯证据、Fixture 清单与未闭合限制

### 4.1 真实厂商资料 Fixture 清单

1. **codex-cli 0.160.0 演示会话（Windows，owner 导出）**：
   - 来源：`C:\grok-drop\native\codex`，演示工作区（`notes.md`、`calc.py`、两张 PNG 图片，PROMPTS A1–A8）。
   - 脱敏脚本：`src-tauri/tests/fixtures/session-migration/codex-0.160/scrub.py`。白名单保留记录结构与 ordinal，重新计算脱敏后 `end_byte_offset`（保持 75 条记录边界），拒绝输出包含原始路径或凭证关键词。
   - 脱敏后 Fixture SHA-256（`src-tauri/tests/fixtures/session-migration/codex-0.160/`）：
     - `b71dc60229107a8726f6bd2e144c3a7bfec033813c5397fbe2e6cf3eda8afaf3` `rollout-2026-10-22T00-19-04-01a15f42-520d-7f22-8aa3-b764cfccc141.jsonl`（主会话：读图失败说明、3 次 exec 调用含 exit 1、view_image 图片输出、apply_patch、/compact 压缩）
     - `8a12f02c9440a7409d49d373ab60f96b6e0c0b5de9dc637362ec16a137353844` `rollout-2026-10-22T00-28-11-01a15f42-520d-7f22-8aa3-b764cfccc141_01a15f4a-aa3e-7781-8d92-dee133706e94.jsonl`（回退分支：由 resume 生成，携带 `history_base {thread_id, end_ordinal_exclusive: 75, end_byte_offset}`）
     - `b975498554be4690fdea0ff47eee1f1075a7488913e60007dd88fd9c8279025b` `rollout-2026-10-22T00-20-29-01a15f43-a066-7351-b5de-316fdc562f9f.jsonl`（独立 `codex exec` 会话）

2. **Cursor 3.23.12 Transcript（Windows，owner 原样导出）**：
   - 来源：`C:\grok-drop\native\cursor`，无隐私原样录制入库。
   - Fixture SHA-256（`src-tauri/tests/fixtures/session-migration/cursor-3.23/`）：
     - `185c2801067521c48a1fa6e1ed724325d50cab80e95e992704f9a90eba8b0570` `cdfdffc1-49a8-4b40-b6cb-2025940216eb.jsonl`（Agent 会话：5 轮对话，含 `turn_ended:success`、Glob/Shell/Read/StrReplace 工具调用、`<timestamp>`/`<user_query>` 包装）
     - `795404043400f5cd9ed66d0e99c75e747cb2b1256fa2506d263471abe5877d61` `204eedd5-b641-4465-b81d-7f1d42f8bf09.jsonl`（Ask 会话：1 轮对话，含 Read 工具调用与回答）

### 4.2 合成 Fixture 清单与公开依据

依据 `src-tauri/tests/fixtures/session-migration/README.md`：
- **Claude Code**：`claude/g1-tools.jsonl`（F03 工具结果分型）、`claude/sidechain.jsonl`（F05 sidechain 标记）、`claude/compaction.jsonl`（P3 typed compact summary 与 live 分支隔离）。公开依据为 Anthropic SDK commit `23bb0157f51f83c21a7c47fbed1e5741ad581082`（`sessions.py` 第 259–285、935–941、1029–1036 行；`test_sessions.py` 第 801–828 行）。所有正文、UUID、时间戳均为合成。
- **Cursor Temporal / Store Wire**：`cursor/transcript.jsonl`、`temporal-matrix.json`（10 项生产管线合成用例）、`store-wire.json`（DB hex meta、有序 Merkle 引用、plain/protobuf JSON 叶节点）。公开依据为社区开源项目 S2thend/cursor-history commit `aa0d4698289bd3801d2bac07895fe7b6adbc5ce0`（`parser.ts`、`storage.ts`、`store-db.ts` 及单测）。所有文本、ID、时间戳均为合成，不构成 Cursor 官方格式契约。
- **Codex 合成用例**：`codex/g1-main.jsonl`（G1 消息/调用/结果计数）、`codex/g4-dup.jsonl`（G4 流式与 final 消除、真实重复保留）。
- **通用文件**：`generic/fenced.md`（代码围栏内角色保护）、`generic/illegal.json`（非法 JSON 拒绝）、`generic/messages.json`（标准 JSON 消息数组）。

### 4.3 桌面实机与跨平台验收演进

已完全替代早期历史时点仅合成 transcript 的 V245 预览：
- **V789**：Windows 实机验证已完成作业重开、Undo 绝对路径签名认证、草稿续聊引用项安全保留。
- **V814（2026-10-21 提升）**：已确认选择摘要、阅读器默认分支修复、中文 Windows 纯净重启并通过只读审计。
- **V815（2026-10-22，Administrator 会话 CDP）**：真实加载 Codex 0.160 三会话，发现 3、准备 3/3、导入 3/3；验证分支阅读器含父前缀、环境/AGENTS 上下文展示为纯文本系统事件、工具输出 exit_code 1 / 失败详情保留。
- **V816（2026-10-22）**：修复主会话标题图片包装污染；支持带 `covered_event_ids` 的压缩事件“查看压缩前对话（N 条）”；Undo 保留项归入中性“未撤销”分组。
- **V817（2026-10-22，CDP 交互）**：压缩前阅读深度叶节点跳转；追加新轮次后重导入形成 revision 1 与 revision 2 并存；撤销第 1 次导入时识别“已有更新导入因此保留”；15 语言截图（据此修复长文本布局）。实测中发现被撤销会话无法再导入，修复后由 V818 与 Linux L2 验证（默认不勾选的显式重新导入选项）。
- **Linux arm64 实机（XFCE 桌面，L1/L2）**：fork CI 预览二进制，隔离 profile + mock ACP，5 项全部导入；验证压缩前对话阅读；追加重导形成 revision 2；撤销后回到 revision 1；重导入撤销会话；系统托盘正常退出，两 WebKit 进程无信号干净退出。

### 4.4 明确未闭合限制与未验证项（不臆造支持）

1. **Cursor Composer / Store 真实活库**：`unverified`。提取脚本 `native-demo/extract-cursor-demo-chats.py` 待 owner 审阅运行；workspace DB 新增 `composerHeaders` 表及 globalStorage 活库结构未实测。
2. **Claude Code 真实资料**：`unverified`。owner 未在测试机登录，真实安装版导出文件及目录结构未实测。
3. **Codex metadata/paginated DB**：`unverified`。格式明确拒绝（`ErrorCode::SchemaUnsupported` / `ImportOutcome::Unsupported`），真实数据库结构未实测。
4. **D04 跨格式身份聚合与旧身份 Adoption**：Store / Composer / transcript 跨来源完全聚合去重尚未完成；旧裸 ID / entry / revision 不自动 adopt。
5. **真实 ACP / Vision / MCP 进程复用（H4）**：实际多模态理解与真实模型交互仍待用户配置 provider 账号与费用上限；目前仅在 synthetic / mock 边界通过断言。

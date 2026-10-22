# 当前格式兼容矩阵

## 2026-10-21 真实厂商资料（Claude 接手 H1）

| 来源 | 版本 / 平台 | 资料 | 当前处理 | 证据 |
| --- | --- | --- | --- | --- |
| Codex rollout JSONL | codex-cli 0.160.0 / Windows | owner 录制演示会话，脱敏 fixture `src-tauri/tests/fixtures/session-migration/codex-0.160/` | 工具输出数组（文本+图片）、显式 success 标志；注入上下文与 developer 归 system；resume 分支 `history_base` 作为独立会话拼接已校验的父前缀 | `codex_tool_output_tests`、`engine_codex_native_tests`（scan→preview→publish→reader） |
| Cursor agent transcript | Cursor 3.23.12 / Windows | owner 录制，原样 fixture `cursor-3.23/` | `turn_ended:<status>` 元数据；精确 `<timestamp>`/`<user_query>` 包装拆出问题文本与来源时间 | `cursor_transcript_323_tests` |
| Cursor Composer/global DB | Cursor 3.23.12 | 待 owner 审阅运行提取脚本 | 未验证（workspace DB 新增 `composerHeaders` 表，聊天在 globalStorage） | — |
| Claude Code | — | owner 未登录，无资料 | 仍仅合成证据 | — |

版本记录：Codex evidence `adapterVersion` 为 `1.0.7`（`CODEX_ADAPTER_VERSION`），
全局 `NORMALIZER_VERSION` 仍为 `1.0.6`，因为 Cursor 历史身份关联协议与已存
provenance 记录按值锁定它；Cursor transcript 的 evidence 版本同理不变。只有
3.x 新形状（含包装或 `turn_ended`）的输出改变，旧形状逐字节不变。
新内容摘要不同，重导入生成新 revision，旧归档不改写。

2026-10-21交接：代码检查点`572ab60a`已在
`handoff/session-import-claude-20261021`本地提交；当前完整范围见
[Claude交接文档](../2026-10-21-session-import-claude-handoff.md)。N814/F814及
V814限定Windows合成图片/分支/重启已通过，不等价真实厂商格式/vision或三平台通过。
下方HEAD、未提交、running和版本状态均为历史快照，不覆盖上述最新结论。

更新：2026-10-15。当前工作树为 `integration-rehearsal-v243`，HEAD 为
`7eb1b7b9f66b149bf7c6a9e3df189caeec062095`，包含合入上游后的未提交修改。
当前完整原生证据为 N696；实现与交付余项见
`../2026-10-08-session-import-delivery-current.md`。下文版本演进中的
“仍 live / 尚未完成”仅描述对应旧 run 时点，不能覆盖当前总状态。
本表的通过证据来自 Windows 原生测试进程中的合成资料；不能推广为
任意安装版本或原厂活库兼容。macOS/Linux 原平台发现、WAL/MCP 尚未验证。

| 格式族 | schema fingerprint | 状态 | 说明 |
| --- | --- | --- | --- |
| Codex legacy JSONL | `codex-rollout-jsonl-v1` | 合成测试通过 | session_meta / response_item / event_msg；delta/final 去重，真实重复保留；fork/rollback/compaction |
| Codex metadata/paginated | `codex-thread-history-metadata` | 明确拒绝/仅元数据 | `metadata_only_db` → Unsupported |
| Claude Code JSONL | `claude-code-jsonl-v1` | 合成测试通过 | UUID/父链、tool_use/result、sidechain、来源标题；1.0.2 保留 typed compact summary，历史 logicalParent 不拼接到 live Parent |
| Cursor transcript | `cursor-transcript-jsonl-v1` | 合成测试通过 | role + message.content |
| Cursor Composer ItemTable | `cursor-composer-itemtable-v1` | V248 1.0.3完整原生合成回归通过 | 数字type 1/2、直接epoch-ms/RFC3339时间；缺失/倒退/offset保留；此前WAL合成基线保留 |
| Cursor legacy Chat ItemTable | `cursor-legacy-chat-v1` | V248正式生产链合成回归通过 | 正式扫描chatData、精确key族定位、inline id/content；不会把inline id当外部bubble |
| Cursor Store JSON blobs | `cursor-store-blobs-v1` | V248完整原生合成回归通过 | sidecar selected root/JSON节点；拒绝无root的all-blob fallback，不代表原厂实测 |
| Cursor Store DB/Merkle wire | `cursor-store-merkle-v1` | V248新wire与边界合成回归通过 | DB meta hex、selected root、有序0a/12引用、plain JSON/protobuf field4；只读reachable叶，工具归属/现代results/截断边界通过 |
| 通用 JSON 数组 | `json-array` | 合成测试通过 | 保留 timestamp |
| 通用 messages 对象 | `json-object` | 合成测试通过 | 已识别的 messages 对象；不代表任意 JSON 对象受支持 |
| 通用 Markdown | `markdown` | 合成测试通过 | 围栏内 `User:` 不分角色 |
| 非法 JSON 对象 | — | FORMAT_UNSUPPORTED | 不落回 Markdown |

当前源码 `NORMALIZER_VERSION = 1.0.6`，事件 `SCHEMA_VERSION = 1`，
`CATALOG_SCHEMA_VERSION = 2`。三者不是同一个版本号。
旧归档保留自己的 adapter/normalizer 版本；重导入不会原地改写旧 revision。
1.0.4 Cursor transcript身份/标题修复通过完整合成native门：正式JSONL
发现/归一化共用有界声明ID与相对locator fallback，不将消息ID或裸stem
当会话ID。V249聚焦31、领域881、全库3810passed，均0failed；路径/子代理
和可读标题回归实际通过，Unix专属断言未执行。旧stem entry身份不自动迁移，D04跨
Store/Composer/transcript聚合尚未完成，不能据此宣称完整跨来源去重。
1.0.5新增typed session/composer/legacy-chat身份与未知Store根内locator作用域；
生产双发布碰撞反例已在37项聚焦回归通过，领域首次884passed/3failed是
尚未同步的旧契约断言。断言已更新，不沿用1.0.4的3810passed作为新证据。
随后39项契约聚焦通过，但领域886passed/
1failed暴露未隔离搜索fixture的HMAC home依赖；V251 fresh-process回归
实际复现继承home污染。修16个fixture隔离后V251完整gate通过：聚焦41、
领域889、全库3818passed，均0failed，fmt/compile/clippy exit0，665输入及
HEAD/index稳定；产品cursor/HMAC/权限规则未改。旧裸ID/entry/revision不自动adopt；
source locations、来源组及版本选择仍是D04未闭合范围。
1.0.6关联首批仅Cursor/local：附属根独立UUID/扩展schema1，primary身份
保留1.0.5算法，fallback/subagent/未知Store按附属location scope隔离。
Store JSON/wire在graph生成前接固定身份，原始来源与旧归档不改写。
扩展首次显式关联才事务性建表；只读/schema检查无DDL，登记漂移/缺索引/
未知版本fail closed。V255并发关联61passed/1failed后修一致性快照，V256
当前聚焦63passed/0failed，领域/Clippy/全库仍live，未当作完整兼容通过。
source groups/member、完整固定版本durable链与UI/原厂验收仍未完成。
旧 catalog 的只读入口不执行 DDL；受控重建验证归档后升级到 v2。
比当前更高的 catalog 版本返回 `IMPORT_SCHEMA_TOO_NEW`。

## 可追溯证据与限制

- 1.0.2 native基线：`validation/runs/v245-title-fa98e2f14ee04387a64e9d96218f4851`，
  `migration-tests.stdout.log` 中 849 passed / 0 failed / 13 ignored。
  通过判定来自实际 `... ok`，不把测试名称存在或 ignored 当通过。
- 资料范围与公开形状来源：
  `src-tauri/tests/fixtures/session-migration/README.md`。
  Claude compact-summary 资料依据 Anthropic SDK 公共源码 SHA
  `23bb0157f51f83c21a7c47fbed1e5741ad581082`；所有正文、UUID、时间均为合成。
- 2026-09-08 的 `fixture-manifest.json` 是 1.0.0 初始黄金样本登记，
  不是 1.0.3 的完整资料清单，也不能据它认定原厂版本已经实测。
- 1.0.3 temporal链：`v247-title-1daad9a35ee7403cb60328e636879d36` 中13项
  保真回归通过；同次新增Store wire回归失败1项，整个gate不算通过。
  后续`v247-title-70df268594ee489283d8f8c18deb6806`完整native门通过：
  聚焦19、导入869、全库3798passed，0failed，fmt/compile/clippy均exit0。
  2026-10-11核对661份source-after输入无drift；无任务owner。
  新边界补丁以`v248-title-d0f458e81d42435f8f038287df236a06`完整验证：聚焦22、
  领域872、全库3801passed，0failed；fmt/compile/clippy exit0，661输入与
  HEAD/index稳定。D04正式跨格式聚合仍未接入，不能按辅助测试称已支持。
- 1.0.4完整native门：`v249-title-d36851c8e6bc49459fba2196ba262a52`，
  聚焦31、领域881、全库3810passed，0failed，fmt/compile/clippy exit0；
  sourceStable/gitStable=true，终态后逐哈希复核663输入无drift。不是
  来源组、原厂数据、真实provider、桌面或跨平台验收。
- 1.0.5完整native门：`v251-title-e6dd303e572443b6b67472f42e56b11b`，
  聚焦41、领域889、全库3818passed，0failed；fmt/compile/clippy exit0，
  sourceStable/gitStable=true；终态后逐哈希复核665输入无drift，原owner退出。
  13/15ignored不计通过；来源位置关联/组/版本选择及真实验收仍未完成。
- Windows V245 仅完成合成 Cursor transcript 的部分桌面链：扫描、预览、
  重导入、阅读及搜索；这不替代 Composer/Store 活库、Claude/Codex
  原厂 smoke、完整续聊和跨平台验收。
- 源软件具体版本、脱敏原厂生成资料及 fixture hash/期望输出登记仍需补齐。
  未识别 shape 明确 partial/unsupported，不按品牌猜测兼容。

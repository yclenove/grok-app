# 会话导入验收矩阵与 P0 样本计划

2026-09-08。**所有验收项尚未执行。** 本文是可复现的输入/期望清单，不是通过报告。依据：[主方案](2026-09-08-session-workspace-import.md)、[实施契约](2026-09-08-session-workspace-import-contracts.md)。

## 1. 如何认定一个格式受支持

兼容登记记录 provider、source version（如果可得）、schema fingerprint、adapter version、平台、fixture hash、期望输出与来源证据。软件品牌和一个文件扩展名不足以匹配 parser。

先用公开格式构造不含真实对话的测试资料；需要真实版本差异时只在用户授权后采集，脱敏后再进入 fixture。每个 fixture 的 README 写明结构来源、修改内容和已知缺失，不把合成样本称为三平台真实数据实测。

| 格式族 | P0 必备样本 | 当前证据 / 缺口 |
|---|---|---|
| Codex legacy JSONL | metadata、response/event 重复、工具、归档、fork/rollback/compaction | 已调研固定源码；尚无解析/端到端结果 |
| Codex metadata / paginated | projects roots、只有摘要、正文版本不支持 | 已见 schema 与官方限制；验证拒绝/partial 行为，不承诺未知正文 |
| Claude Code JSONL | UUID/父关系、内容块、工具结果、sidechain、summary/custom title | 已调研官方文档和参考 parser；需明确运行版本和 shape |
| Cursor Composer / legacy Chat | workspace/global 两库、bubble 指针、旧 chat data | 社区源码证据；需覆盖参与 schema 与 WAL |
| Cursor Store | meta/root blob、children/leaf、ACP 根、缺 blob | 社区源码证据；尚未确认本机安装版本，不能声称本机实测 |
| Cursor transcript | 字符串/块内容、空窗口、未知 cwd、与 DB 重复 | Codex/社区源码证据；纯 transcript 不代表 Cursor 全历史 |
| 通用 JSON/Markdown | 原有支持格式、围栏代码、BOM/CRLF、无角色/格式歧义 | 现有实现可读；新管线须保持正常输入兼容并明确异常 |

## 2. 行为验收用例

每个 ID 在后续报告填写实际命令、fixture 版本、输出位置、平台和通过/失败；本文件保持期望定义。`P` 表示规划切片，所有行当前均为“待执行”。

| ID | 输入 / 操作 | 必须观察到的结果 | 切片 |
|---|---|---|---|
| F01 | 三家各 2 条 user、2 条 assistant，带不同原时间 | 输出 4 条消息，角色/顺序/原时间保留；importedAt 独立 | P2–P4 |
| F02 | Codex 一次回答同时有流事件和最终 response；之后真的再回答同一句 | 首次输出只出现一次，第二次真实回答仍存在 | P2 |
| F03 | Claude 1 user、1 assistant 含 tool_use、1 tool_result、1 final | 3 条可见自然语言消息、1 调用、1 结果；结果不成为用户气泡 | P3 |
| F04 | 主链 A→B；从 A 分出 C；B/C 产生不同文件变更 | 阅读树保留 B/C，选 C 续聊不含 B 的变更事实 | P2–P4/P6b |
| F05 | 1 主会话 + 2 sidechain；只选主链 | 子会话关联仍可发现；上下文及工具不能读未选 sidechain | P3/P6 |
| F06 | 文件包含 1 条坏中间记录，后面还有完整回合 | partial + 明确位置；不能当未完成尾行忽略，也不能报告全部导入 | P1–P4 |
| F07 | 活 JSONL 最后半行，之后追加完成 | 第一次只固定完整前缀并提示；刷新补齐，旧 revision 保持不变 | P1–P4 |
| F08 | 未知 block + 缺失图片 + 超大输出 | 3 类问题分别可解释；占位/分页可用；不静默计为完整成功 | P1–P4 |
| F09 | 原时间缺失、时间倒退、跨时区 | 因果以 ordinal/graph 为准，不以导入时间伪造来源时间 | P1–P4 |
| F10 | Markdown 围栏内含 `User:` / `## Assistant`；非法 JSON 对象 | 围栏不分角色；非法格式明确错误，不落回文本胡乱解析 | P1 |
| F11 | 已签名/加密或不可解的 reasoning 数据 | 标记不支持/不可读，不解密、不编造内容 | P2–P4 |
| F12 | 100MB 会话和 1GB 批次，含长工具输出 | 流读、页边界与顺序正确；峰值/吞吐记录；UI 不拿完整数组 | P1/P7 |
| W01 | Windows 中文空格目录，导入到 macOS 的新目录 | 源路径不改写，用户映射生效，历史命令文本不被替换 | P5 |
| W02 | `.code-workspace` 2 个相对 roots，其中 1 个缺失 | 保留 2 roots，分别映射，执行明确选 primary root | P4/P5 |
| W03 | 两会话同名不同 cwd；两个路径同文本但不同 SSH host | 不串项目；Host 身份参与匹配 | P5 |
| W04 | 用户手选目标与源 cwd 建议冲突 | 用户选择优先，原建议仍有证据记录 | P5 |
| W05 | 同源有项目 A，无会话；已有 App 项目 B 信任/置顶已自定义 | A 可单独导入，B 状态不被覆盖、不被挪 Space | P5 |
| W06 | 缺 worktree；历史有 branch/commit | 仅显示事实，不 checkout/建分支/恢复代码 | P5 |
| W07 | 附件绝对路径已无效，新 root 有同名文件 | 不自动打开同名文件；显式映射/摘要校验后才关联 | P5 |
| D01 | 同一批点击两次导入、多窗口同 requestId | 一个 job、一个 entry/revision，无重复项目 | P1 |
| D02 | 相同会话新增一轮后刷新，已有 Grok 子会话 | 归档出现新 revision，子会话引用原 revision | P1/P7 |
| D03 | 源截断/同 native ID 正文分歧 | 冲突版本可选，不拼接两个时间线 | P2–P4 |
| D04 | Cursor 同 ID 在 Store、Composer、transcript 都存在 | 来源聚合，主正文选择有证据；对不一致字段报告冲突 | P4b |
| D05 | 两个会话文本完全相同但原始 ID 不同 | 仍为两个候选，不按内容 hash 自动合并 | P1 |
| S01 | SQLite 正文仅在已提交 WAL 中，源应用未退出 | 在受支持 readonly 条件下读到正文；所有源文件 hash/存在性前后不变 | P0/P4 |
| S02 | 数据库只读目录且没有必要 SHM，或源锁持续占用 | SNAPSHOT_UNAVAILABLE / SOURCE_BUSY；不修库、不显示“零会话” | P0/P4 |
| S03 | workspace DB 引用的 bubble 在另一 DB 更新中 | 有界核对/重试；仍悬空则 partial，不能假定跨库原子一致 | P4 |
| S04 | 预览后源替换/改写，mtime 刻意保持相同 | revision 校验发现变化；提交固定旧快照或要求重新预览 | P1 |
| R01 | receipt、publish、project link、catalog commit 各边界退出 | 按契约恢复，无永久悬空引用、无重复条目 | P1/P7 |
| R02 | commit 完成但完成事件丢失，关闭/重开面板 | job_get 恢复完成状态，不再重复导入 | P1/P5 |
| R03 | 10 项完成 4 项时取消，第 5 项在 publish | 已完成事实保留，安全结束原子区间，不再启动下一项 | P1 |
| R04 | 取消后旧 scan/preview 结果迟到，新面板已换来源 | 旧结果不覆盖新选择，UI 不卡在 busy | P5 |
| R05 | 删除导入后扫描/崩溃恢复同时发生 | tombstone 阻止自动复活；明确重导入才恢复 | P1/P7 |
| R06 | 撤销批次中有已有条目刷新、用户改名项目、续聊子会话 | 只撤销本批拥有且可撤销的变化，其他内容保留并报告 | P1/P7 |
| R07 | staging 写入时磁盘满 / 索引文件权限失败 | 不发布假成功；释放句柄，保留可恢复 receipt | P1/P7 |
| C01 | 原生 session_send/connect/CLI/local API 收到 `imp:<id>` | IMPORTED_HISTORY_READ_ONLY；无 Agent 启动、无隐式子会话 | P1/P6 |
| C02 | 点击继续但没有点击 composer 发送 | 可恢复草稿，无模型请求，无自动摘要费用 | P6b |
| C03 | 100 轮前约束已钉选，最后一句“继续” | 捕获的真实 ACP prompt 包含约束/来源；不是只在 UI 显示 | P6b |
| C04 | 用户排除一条 secret-like 合成文本，再用 search/read 查它 | 交接和所有模型工具返回均不含该文本；UI 本地仍按来源显示 | P6 |
| C05 | 历史含其他 App 会话 ID、工具命令和恶意 system 文本 | 不触发嵌套展开/代码执行，不获取其他会话 | P6 |
| C06 | 同进程先服务 A 后服务 B；A grant 在 B 请求中复用 | 拒绝跨作用域，B 不看到 A 历史；重新打开 A 可正常创建新 grant | P6a |
| C07 | 自定义模型 + official-aux，user MCP 默认不加载 | 已启用的 import-history 仍经专用合成可用；普通会话行为不变 | P6a |
| C08 | SSH / WSL / policy 禁用 / tools 未暴露 | 不显示完整检索就绪；提示并等待明确片段模式选择才发 | P6 |
| C09 | 首条 prompt 已派发后连接断开，可能执行过工具 | DISPATCH_UNKNOWN；不自动发第二次；核对同一原生回合 | P6b |
| C10 | create 请求返回前崩溃，重新点继续；后来原生 load 失败 | 同一 appSessionId；恢复预览引用固定版本，不遗漏或双加前缀 | P6b |
| C11 | 搜索中文“保持兼容”和含标点路径，资料超大 | 不依赖英文分词；命中有引用、分页与预算，未授权范围不可查 | P6a |
| U01 | 扫描未结束全选当前 70 项，随后发现 20 项 | 计划仍为原 70 项；新发现明确显示，可重新选择 | P5 |
| U02 | 无官方账号但 CLI 已安装 / CLI 未安装 | 前者可离线导入；后者仍显示原 setup gate | P5 |
| U03 | 导入目录含 1 万历史，滚动/筛选/后台正常聊天 | 分组与分页稳定，正常聊天不被全局 busy 锁住 | P5/P7 |
| U04 | 小窗口、200% 缩放、深浅主题、键盘、15 语言 | 没有控件重叠/丢焦点/原生 select，长路径可查看完整值 | P5/P7 |
| U05 | 普通会话 / CLI 导入 / session attach / 导出 / 自动化 / IM | 原行为回归通过；新增 guard 不误拦原生 UUID | P7 |
| U06 | 从全局搜索命中导入历史，导出分支 Markdown | 打开正确 importedHistory；导出选中分支/来源/损失，不把 imp ID 发给 CLI | P5 |
| U07 | 一次导入 1 万条旧会话，包含原来源 token 数 | 不刷未读/运行通知，不计入 Grok 新增用量；原始统计单独标记 | P5/P7 |

## 3. 最小黄金样本与预期数

以下为标准层的示意，原厂 fixture 需各自按真实格式编码。每例须验证结构与输出内容，不只验证存在某关键字。

```text
G1 主链：U1 -> A1(text + call T1) -> result T1(success) -> A2(final)
   期望：message=3, tool-call=1, tool-result=1, missing=0
G2 分支：U1 -> A1 -> U2 -> A2；从 A1 分出 U3 -> A3
   leaf=A3 交接：U1,A1,U3,A3；U2,A2 只在来源树中，不进入此交接
G3 局部损坏：U1 -> unknown_block -> A1(image missing)
   期望：message=2, unsupported=1, asset missing=1, outcome=partial
G4 重复投影：stream delta(A1) + final(A1)；下一轮 final(A2) 文本相同
   期望：A1 一次 + A2 一次；同文本不等于同事件
G5 排除：U1("alpha") -> A1("beta") -> U2("synthetic-secret")
   allowed={U1,A1}；prompt/search/read/export-to-model 都只可见 alpha/beta
```

每个 adapter 同时输出 golden graph、inventory 和 issues；资产使用本地生成小文件，不访问网络素材。SQLite fixtures 用测试时创建的临时库，并保持写连接以制造 WAL；禁止用用户真实 `state.vscdb` 作为 CI 测试库。

## 4. 平台与运行时发布门槛

| 平台 | 本地文件/DB 导入 | UI 阅读/映射/恢复 | Grok 完整续聊 | 当前 |
|---|---|---|---|---|
| Windows | 三家代表性格式与 WAL | 含中文、UNC、缩放 | 官方/自定义 × shared/independent | 未验证 |
| macOS | 三家代表性格式与 WAL | 含卷大小写、权限 | 官方/自定义 × shared/independent | 未验证 |
| Linux | 三家代表性格式与 WAL | 含 XDG/路径权限 | 官方/自定义 × shared/independent | 未验证 |

每个平台至少完成原厂生成的无隐私 smoke 资料导入；可复用数据验证 parser 跨平台，但不能代替原平台发现路径/SQLite 活库/MCP 测试。每种真实 CLI 组合要记录具体版本、tools 暴露证据与发送 trace；无账号的纯 fixture 测试不能证明真实续聊。

WSL/SSH 不在上述“三平台本地完整”计数内，另测明确降级与映射。测试机缺失时保留缺口，不把 CI 编译成功当实机通过。性能记录 CPU/内存/磁盘、资料规模、冷/热缓存、计时范围、峰值 RSS、源库前后 hash；不沿用上一批修改前的结果。

## 5. P0 交付包和发布检查

P0 交付格式：`fixture-manifest.json`（版本/证据/期望）、`compatibility.md`（支持 shape 与排除）、`snapshot-experiment.md`、`mcp-capability-experiment.md`、`benchmark-baseline.md`、schema v1 评审记录。当前这些是**待产出的文件**，尚未创建或运行。

P0 退出条件：能区分格式、原厂身份与元数据；能证明 SQLite snapshot 的源文件行为；能确定本地逐会话 MCP 支持版本；数据契约不再含未决读写所有权。若读工具不可接入，先调整完整续聊承诺并说明差距，不能跳过实验直接标完成。

发布前每个首版必需用例有可追溯结果；所有 partial/error 字段与用户文案一致；全套仓库检查通过；无私人 fixture/原始库/凭据入库；UI/数据迁移/回退限制写进开发文档和用户说明。报告分别列“实测通过、合成测试通过、仅源码证据、未验证”，不混称全部完成。

# 当前交付状态与剩余验收

## 2026-10-21：已保存到 Claude 交接分支

当前权威入口为 [Claude 交接文档](2026-10-21-session-import-claude-handoff.md)。
分支 `handoff/session-import-claude-20261021`，代码检查点 `572ab60a`；
其后的文档提交保存全部台账。上游 v0.2.41 已集成，不重复合入。
V814 构建、10-16 限定中文 Windows 图片/分支/任务重开及同路径重启均已完成，
10-21 闭库只读审计通过。固定试用入口仍 V789，候选 V814 未提升。
本轮匹配原 877 native/2585 frontend 输入哈希，补跑 48 项聚焦回归、依赖及
40 项质量门及typecheck/lint/Rust fmt；没有重跑旧全库/桌面。原53项、真实dispatch/来源/性能/平台余项
按交接 H0–H6 继续；无 push/PR/release、无运行中测试 App。

## 历史增量记录（下方 running/未提交仅代表当时）

2026-10-16 v814分支显示修复：预览摘要跟随已确认的分支；未指定分支的已发布阅读、
搜索和上下文定位使用回执中的选择，完整来源归档和续聊/附件授权保持原契约。
真实Claude分支离线发布回归已覆盖默认/空leaf及完整A/B归档。N814容量21项、导入
1318项/全库4322项和strict all-target Clippy通过，5/103/107ignored不计通过，
重叠集合不相加。F814七门899文件/10404项通过，绑定界面包已生成。V814原生预览
正在构建，桌面验收待完成，
唯一可用入口仍V789；不得将这轮状态解释成原53项或三平台交付。

2026-10-16 图片阅读仍未交付：有序图文、缩略图/原图认证路由和身份隔离已实现。
DTO按职责拆出后大文件仍80个；20条密集图文的IPC前缀预算、真实40MiB文件和
Windows junction回归已补齐。N809专项7项、N808导入1316项/strict all-target Clippy
通过，103ignored不计通过。F810七门899文件/10403项通过，F805 bundle及V806嵌入
Dev构建通过。N800全库4318项是较早源码基线，不冒称覆盖之后的修复，集合不相加。
V806首轮图片在观察期间持续loading；重启诊断最终出现缩略图/大图按钮，但原图
仍在读取，未验收完成。实际8.4MiB附件目录暴露了JSON解析未缓冲、逐字节读取的
性能问题；v811已加缓冲及4MiB读取次数/尾部校验回归，N811专项9项通过。严格
Clippy已通过（N813）；V812嵌入构建和同fixture实机缩略图、3151321B完整原图、
Codex/Claude发布已完成。但实机发现分支摘要/阅读默认值矛盾：A选择正确持久化，
阅读器却默认采用完整归档的来源默认B。v814修复并补回归，原图/附件选择独立性及
重启清单尚未全部完成，不提前提升入口。所属进程均已正常全退出，失败证据保留。
当前唯一可用入口仍V789；原53项、真实视觉派发及大容量等余项保留。
下方旧“Stored显示缺失”和“无图片实现”是历史状态，当前交付以固定入口为准。

2026-10-16 Codex图片保留/权限后端已完成限定验证：图文顺序、纯图片消息、双投影
保守配对和压缩历史中的图片按来源出现位置保留；未知块明确Opaque/Partial，不把
私密payload放进正文/模型；file ID不当本地路径，远端不抓取。无效base64不再伪造
Stored，Codex normalize_file不在资源落盘前永久降级；schema/catalog/normalizer及
普通文字归档形状不变。N794真实断言4fail及N796测试预期错误日志均保留。
N797完整原生门9专项/1309导入/4313全库通过、0fail，fmt/compile/strict all-target
Clippy通过；103/107ignored不计通过，集合不相加。N798显式3项旧发现/续聊/授权
兼容通过，0fail/0ignored、26seed不变；不将N784历史88项重标为本轮覆盖。
fresh绑定871native/2579frontend/196LF-noBOM文本/40质量门，Git/EXE稳定且原owner
退出，F792前端哈希可复用。N794–N798五个自身临时目录已核验清理，其中772文件/
10500855逻辑B；观察空闲变化合计10919936B，单独计量，不宣称排他的物理回收。
日志、失败证据、源码/依赖/种子/当前包保留，无本轮延期目标。

尚不能称多媒体已交付：当前阅读器将Stored图片也显示缺失；预览丢失图文位置；
无扩展名blob直接走现有媒体接口会落到2MiB分段；MCP文本/base64不证明视觉理解。
下步按任务根plans/v799-imported-media-reader-plan.md完成准确状态、缩略图/原图、
交互/大图回归及中文桌面验收后再提升唯一入口。另保留非字符串工具输出和“压缩
后回滚”组合的待证实审阅问题，以及Cursor/真实来源/ACP/扩展性/原平台等原53余项。
当前可用预览仍V789，未嵌入本轮native；本轮未新建包、提交、推送或PR。详见v785。

2026-10-16 最新状态：v786已完成“导入任务”只读分页及显式重开已完成任务；
不改自动recovery/schema/codec。N787完整原生门3history/1299migration/4303library
通过、0fail，fmt/compile/strict all-target Clippy通过；103/107ignored不算通过，
集合重叠不相加。F792七门897files/10393tests通过，F793最终bundle与V789嵌入Dev
构建123.599862秒通过，3498build/162frontend输入与Git稳定。提交成功的prepare
不再锁住历史入口，新未提交prepare仍阻止切换；实际Host回归覆盖。

V789中文Windows实机已通过重启后重开任务及实际Undo。先复制V762到profile1，
实际点击确认被原publication绝对dir_path认证拒绝；catalog完全未变，原V762批次
未撤销，不能重标成功。独立profile2用V727合成seed重新扫描/准备/导入3条Claude，
编辑创建prepared空消息续聊；重启后重开实际新批次0b18ae41-67f4-8fbc-89ec-7812f94f3af8，
沿用用户授权确认撤销：2条undone/hidden，1条因续聊引用保留。再次重启后列表、
结果和编辑草稿都保留；未发送/无dispatch。闭库前后/重启审计均通过，4entry/
5revision/5committed receipts、39seed/12旧Cursor归档不变。5轮各8个所属进程正常
退出、原guard消费且WebView清理。复制profile拒绝与正常路径明确分开，详见V789
desktop-observations/smoke及profile-specific审计。通用“项目映射冲突”文案和任务
仍显示原导入完成状态是待整理UX，不影响引用保护的正确结果。

唯一固定入口已选V789且plain-cmd CheckOnly通过：任务根start-current-preview.cmd，
current-preview-data路径保持，提升不打开/复制用户数据。N787/F792 fresh核对
866native/2579frontend/26seed/189LF-noBOM文本/40质量门及owner退出通过。
V736/V762以下记录是历史，不代表当前仍缺任务入口或正常Undo。原53、Codex/Cursor
媒体、千条二次方性能/混合容量、原厂/live-WAL/ACP/UNKNOWN100、distinct-body/
export、IME/DPI/15语言/原平台余项保持。本轮无commit/push/PR；收尾旧包精确清理
后继续任务根plans/v785。

2026-10-16 分支/附件专项及产品修复已通过当前完整原生门：N783容量19、导入1296、
全库4300项通过，0失败，fmt/strict all-target Clippy通过；集合重叠不相加，
5/103/107ignored不计通过。N784另显式运行88项历史兼容，0失败/0ignored，
26份不可变seed稳定。真实Claude双分支/4图片流程覆盖扫描、分页预览、准备、
离线持久计划发布、完整人工阅读及实际MCP授权；同摘要不同asset ID仍不能越权。
修复图片待落盘时recount过早把Complete永久降成Partial；最终及无source_root
路径仍严格检查，真实截断/缺失/远端问题保留。无schema/codec/version变化或旧归档
重写。864native输入/HEAD/index/EXE稳定；详见任务根plans/v780及N783/N784日志。

用户已许可V762的3条合成导入撤销，无需重问。实际重启后发现阻塞：已完成任务
不会由recovery恢复，向导没有重新打开它的入口，因此没有点击确认撤销；预期
2撤销/1引用冲突尚未实测。此行为由engine_recovery及既有测试明确规定，需新增
显式“导入任务”查看入口，不能通过恢复旧提交计划或后台改状态冒充实机通过。
V762闭库restart-without-undo审计通过，catalog hash与原before-undo相同，
4entry/5revision/5receipt、39seed/12旧归档及prepared空消息草稿不变。
本次8个所属进程正常退出，WebView193文件/11872779逻辑B已清理，观察H空闲增加
12005376B，单独计量。N783先前清理10499557逻辑B，观察增加11444224B；N784与
N780–N782的空temp清理0B。源码/包/日志/seed保留，先前被拒绝目标未重试。
V762是旧包，不代表新native桌面验收；V736唯一入口不变。下步详见任务根plans/v786，
然后继续v785的Codex非文本保真；原53、容量/真实来源/ACP/三平台/UX余项未缩减。

2026-10-16 混合普通导入/历史关联的有界流程已通过：N778编译与13项标题回归，
N779显式执行3项prepared关联测试，0fail/0ignored，strict all-target Clippy通过。
新增72份普通来源（Codex/Claude/Cursor各24）＋1个历史Cursor关联，37份部分选择；
真实扫描/准备/持久计划/普通来源离线/清缓存/公开提交/正常worker发布后，218条
归档、逐项回执/成员/选择及草稿一致，旧import ID/版本/回执/job/归档和源字节保留，
重复请求发布0。N777初始失败是测试比较了不同范围的哈希，已修正断言，未改产品。
相对N775只增加测试与注册，861native输入；只补齐有界混合正确性，不代表容量/
真实性/实机或完整53项完成。历史关联仍仅支持Cursor。唯一预览仍V736，V762未提升；
实际Undo、分支/附件、扩展性、原厂/ACP、原平台和UX余项保留。证据见任务根plans/v776。

2026-10-16补齐实际工具字段筛选的合成全流程：N774专项17pass/0fail/5ignored，
N775历史MCP12项、续聊创建58项及strict all-target Clippy通过。12份三来源JSONL
经过真实扫描/固定组/异步准备/持久计划/来源离线/正常worker发布；60条完整归档、
工具参数JSON与调用结果配对、回执/source-member/selection均逐项核对。全关/仅名称/
仅参数/仅结果分别约束真实续聊草稿、可读事件ID及持久化本地会话授权；实际MCP
搜索查完全部分页、隐藏字段无命中、完全隐藏工具按ID读取被拒绝。本地阅读前后
完整，源字节不变、同请求重复发布0；本地续聊prepared/空消息/无dispatch。
本轮仅新增3个测试文件和2行注册，858native输入与Git稳定；没有产品修复、App包、
提交/推送/PR。三次初始测试合同错误及最终证据保留在任务根plans/v771。只闭合有界
合成合同，不等于原53项交付；mixed普通/历史关联容量、分支/附件、实际Undo、原厂/
ACP、三平台及IME/DPI/15语言实机等原余项继续。V736仍唯一入口，V762未提升。

2026-10-16 原始输入校验改为每次完整快照一次范围查询，仍逐条流式校验所有原始
内容；没有跨写入/事务的 SQL 授权缓存，也未改 schema/codec。新增 NULL/BLOB/
非法文本/字节篡改及相邻作业保护回归，N766 验收36项通过。N767完整六命令通过：
标题13、导入1293、全库4297，均0fail，strict Clippy通过；102/106ignored不计通过。
855native输入和Git稳定。N768实际Release opt3编译/发现817.180秒通过，不是App包。
N769普通prepared64条逐项通过，发布9031ms；单项查询46208→1024，完整原始内容
hash仍47940次/132809448B。此前9094ms与本次基本相同，不宣称稳定或因果提速。
N770当前源码不间断千条完整发布逐项通过：发布537096ms，最后验证18129ms，
整轮596.366秒；所有1000entry/revision/committed回执/reader一致，518次并发查询
最大61ms，测试进程峰值working set50806784B。仍有11002次完整快照、27019000次
成员校验，二次方扩展未解决；不与N761恢复799条直接比较，也未机械启动万条。
原owner均已退出且原session消费，自身临时目录已精确清理。唯一预览仍V736，V762
未提升，原53项、mixed/scoped容量、实际Undo、真实来源/ACP及原平台等余项保持。
本轮未新增提交/推送/PR。详细结果见benchmark-baseline与任务根plans/v766。

2026-10-16 N764/N765补齐有界混合来源完整流程回归：12份合成Codex/Claude/Cursor
JSONL（各4份），实际扫描→固定组分页→6份部分内容选择/6份默认选择→异步准备→
持久计划→来源离线/清缓存→发布→逐项回执/双项分页reader/续聊草稿全部通过。
36条归档消息完整保留，6个部分选择草稿排除未选回复，固定事件及字段开关保留；
源文件前后字节一致，同一请求重放无新增发布。N764格式/编译/容量专项16pass、
0fail、5ignored；N765新回归单验1pass与严格all-target Clippy通过。仅新增测试，
生产输入相对N757未变；不是当前全库重跑、实机、真实厂商资料或容量测速。
N763首轮失败是把Cursor专用member pin错误要求于其他来源，已按来源合同修正，
没有修改生产安全校验。mixed/scoped千条/万条、关联与普通混合发布、分支/附件/
工具字段实际裁剪等原余项仍开放。原53项/V762 Undo待确认/V736唯一预览保持。
具体记录和后续边界见任务根plans/v763-mixed-scoped-contract.md。

2026-10-16 V762当前源码嵌入构建117.016秒通过（Dev，非Release），fresh3480
输入0drift。中文Windows合成实机完成3条Claude来源扫描/准备/history-only导入，
结果3/3；工具事件分型、当前压缩链、摘要搜索1命中、编辑并创建续聊和重看上下文、
重开完成任务通过。草稿prepared/messages=[]，未发送。闭库核对4entry/5revision/
5committed回执、6/5/4条Claude事件及来源/归档hash一致，旧Cursor12归档文件与39
seed文件不变。8个所属进程正常退出，清理WebView逻辑13139488B、观察free增加
13357056B（分别记录）。实际Undo仅展开确认，action-time确认等待用户；有续聊引用
的一条预期保留冲突，另外两条预期撤销，尚未实测。唯一入口仍V736，V762未提升，
未碰用户数据/提交/推送/PR。详见任务根V762 desktop-observations/smoke及before-undo
audit。原53项、真实渠道/来源/三平台等余项保持，不把合成流程当最终交付。

2026-10-16续验已结束：N759当前源码优化编译/发现817.0007383s通过；N760的64条
完整发布/逐项reader通过，发布9094ms。N761千条取消/源离线恢复实际1pass/0fail：
201条后取消87ms返回、1546ms停止，源离线后75ms重新排队799条，最终1000条归档/
版本/回执/reader全部一致且再次恢复不重复。整轮641.2332073s，恢复发布466925ms、
最后逐条验证17537ms，恢复查询最大320ms。不能当作不间断千条、mixed/scoped、
当前万条或UI结果；完整集合校验仍有扩展性问题，不声称统计或因果提速。
原session均exit0并消费，源码/Git稳定，自己的空temp已清理（0logicalB/观察0B）。
fresh853native/2574前端/26seed/Git/EXE/166LF文本/40质量门及owner退出通过。
旧N746对照EXE约62MiB因清理脚本被自动审批拒绝暂留，未删除，不报回收空间。
未启动App、未更新V736固定入口、未新增commit/push/PR；原53项和实机/真实渠道等
余项保持。详见benchmark-baseline与任务根plans/v756-member-commitment-review.md。

2026-10-16当前源码新增测试归因及有界纯计算复用。N751/N752计时器与发布专项
通过；N753定位成员集合重复commitment计算，N754新旧root篡改控制在内35项、
N755发布96项及strict Clippy通过。N756同64条Debug负载逐项归档/回执/reader
通过，成员range3.982→2.810s，发布24.370→23.381s，worker23.448→22.901s；
单次对照不代表正式版收益，也未解决二次方扩展。SQL完整读取、root/原始input/
allocation及全部写后认证保留；仅缓存已认证精确字节的纯commitment计算结果。
N757当前源码完整六命令已通过：导入1291/全库4295，0fail，102/106ignored不算
通过，strict all-target Clippy通过，853native/source/Git稳定。显式旧格式N758
87项全部通过，26seed不变；fresh853native/2574前端/Git/EXE/166LF-noBOM文本/
40质量门和owner退出核对通过。N757自身temp及已完成对照的旧EXE已按owner/hash
清理，日志/源码保留；N758自身空temp也已清理，0logicalB/观察0B。
旧EXE逻辑104049664B、观察free+104050688B，temp逻辑10500335B、观察+11452416B。
原53项和实机/真实渠道等余项保持；唯一V736预览仍未嵌入这些后端改动。
具体证据、当前owner及后续顺序见任务根plans/v756-member-commitment-review.md。

以下同事务writer复用结果是本次纯commitment改动之前的已完成基线。

2026-10-16本轮后端优化已通过完整原生回归。签名发布复用同一未写入事务中的
已认证acceptance，只减少重复header/member-set读取；仍重新核对具体事务、连接、
job、total_changes、所选成员原始输入和allocation，写后仍完整重新认证。普通创建
每条减少5次重复header读取，去重减少1次；这不是跨事务授权缓存，也未改变Legacy。
N743发布专项96pass/0fail/1ignored（含新增4项）、N744显式历史87pass/0fail/
0ignored及26seed不变通过。N745六命令全部通过：导入1289pass/0fail/102ignored、
全库4293pass/0fail/106ignored和strict all-target Clippy；集合重叠，忽略不算通过。
853native输入/source/Git稳定，HEAD/index仍为已批准集成检查点。F737的2574份
前端输入hash不变，保留其原七命令/10,376项证据。N746完成相同源码的release
编译/发现（opt3，858.814s），不等于完整release回归。N747/N748/N749各64条
发布/逐项归档回执reader验证1pass/0fail；发布10.118/10.252/10.077s，中位数
10.118s，未优于历史N739的9.041s。worker header由2052减至1732、member由
132288减至111808（约15.6%），完整snapshot仍706次；只证明重复工作减少，
不声称实际提速、因果性能回退或解决二次方扩展。原owner均exit0/消费，自身
空runtime已清理，各0logicalB/观察delta0B；详细构建/日志/hash及计时限制见
benchmark-baseline。下一切片先补发布阶段耗时归因，见任务根v750计划；不机械
重复千条/万条。下方N739/N740仍是优化前N732基线。唯一试用入口仍为V736，
尚未嵌入本轮后端改动；本轮复用该包补存中文1282×802的当前阅读器、版本菜单及
旧版阅读器3组JPEG/TXT，新旧精确URL与2事件核对通过，不替代新版源码桌面验收。
8个owned进程正常退出；自身compiler-temp/WebView按owner/hash核对后清理，
保留源码/日志/失败证据。原53项及实际Undo、真实来源/ACP、容量与平台余项继续OPEN。

2026-10-16历史性能续验：同N732源码的N738优化编译/发现通过；N73964条、N740
1000条普通prepared发布各实际1pass/0fail/0ignored，最终归档/版本/回执/reader
逐项一致。发布9.041s/639.413s，千条完整696.110s、峰值working set50,655,232B；
status5ms/首50项6ms/terminal35ms/catalog2ms。并发查询最大772ms仍是性能问题，
worker重复32004次header/32019000次member校验；暂不直接跑万条。原session30791/
1180均exit0并消费，source/Git稳定、owner退出、自身空runtime已清理（0logicalB/
观察delta0B）。详见benchmark-baseline及任务根v738/v741计划。不含发现/厂商解析、
mixed/scoped、当前取消恢复或UI/release全套；这些和其余原53项继续OPEN。当时仅文档/
外部runner改动；该段N732证据已由顶部N745后端门补充，F737/V736仍沿用。

## 当前切片（2026-10-16，V736 历史版本导航限定验收通过）

源码已接入公开只读metadata分页接口、惰性版本选择和15语言。每页最多50条，
游标绑定历史/列表代际；显式选版本清除旧leaf/anchor，阅读/搜索/导出/续聊固定到
所选revision。列表关闭隔离迟到响应，翻页失败保留原页并重试原目的地；最新正文
不可读时仍可选择完好的旧版本。7项hook、4项真实Host/Select集成通过；原生导入
1285通过/0失败/102忽略（含新增版本测试5项）。N732六命令完整通过：全库4289/
0失败/106忽略及strict all-target Clippy，集合不相加、忽略不算通过。F737最终冻结
前端七命令/895文件/10,376项通过，F735 bundle及V736嵌入构建119.741098s通过，
输入/资源/Git绑定。F734因源码变动不是最终稳定门；N730的显式历史87/retention44
沿用此前证据，未冒认本轮重跑。中文Windows合成实机实际选择/阅读精确旧新URL，
旧版搜索1命中、metadata刷新保留旧版/query、续聊预览/取消、切回当前清搜索及
Esc仅关Select均通过。两版合成正文相同，不能声称画面正文发生变化；不同正文和
精确动作scope由真实Host联动覆盖。闭库核对39seed/12归档文件不变、2版本/2回执、
current未变。8个App/WebView进程正常退出，WebView精确清理。固定入口现为V736；
未打开/移动用户current-preview-data。原53项、实际Undo、当前mixed/scoped容量与
万条性能、UNKNOWN100、原厂/live-WAL/ACP、三平台/IME-DPI/15语言余项
仍OPEN。本轮未实测桌面导出、实际续聊创建/派发、窄窗或多页版本列表。
证据：任务根V736 desktop-observations.md、desktop-smoke.json、desktop-data-audit.json。
下方V727及以前为历史，缺旧版本入口或旧current版本的描述已被本段取代。

## 历史 V727（2026-10-16，关联导入限定验收）

固定入口 `H:\aicoding\grok-session-import-validation\start-current-preview.cmd` 已更新V727，
plain-cmd CheckOnly及EXE哈希通过，未另开App或改用户试用数据。中文Windows合成
实机完成原已登记Cursor来源的扫描/准备、双侧正文复核、取消、确认/清除未提交关联、
重新显式确认及实际公开导入；打开新正文和重开已完成任务通过。退出后只读核对同一
entry共2个revision/committed receipt，新增1个completed job/2条事件；旧归档6文件、
不可变seed26文件和登记来源hash未变。精确8进程正常退出，guard69425及quit11065
均exit0并消费。证据见任务根V727的desktop-observations.md、desktop-smoke.json及
desktop-data-audit.json。V710的2/40/160与cancel/retry实测仍是此前证据，未冒认本版重测。

当时发现reader没有旧revision选择入口；V736已补有界版本列表/选择、固定revision
阅读/搜索/续聊和迟到响应隔离，实际桌面结果及证据限制见顶部。后续仍需完成
实际Undo与原厂/live-WAL/真实ACP验收。原53项、mixed/scoped
容量与优化构建/万条性能、UNKNOWN100、三平台/IME-DPI/15语言实机范围不缩减。
V727已嵌入下述F725/F726/N730源码，构建119.24s/exit0、输入/Git稳定；无新提交或PR。
清理已退出的WebView缓存13,905,397logicalB、旧V710包98,975,744logicalB；观察H空闲
分别+14,118,912B/+98,971,648B，不能完全归因于删除。源码/数据/日志及N724失败证据保留。

### 本轮源码和完整回归

prepared-batch的复核界面、公开inspect/plan和commit已接通。后端按batch/version/
candidate/preview校验实际prepared selection，保留prepareOrigin及原批次receipt；
UI双侧只读复核后显式确认，禁止同批重复目标、迟到响应和过时版本。恢复的immutable
plan显示真实关联摘要，并在首个IPC前保存requestId。来源metadata每页最多50条，
翻页保留确认；空页、busy、失败重试及batch/version复位路径已覆盖。
N719真实pinned-seed的prepare→review→public commit→worker专项2项、N721准备回归
36项与strict Clippy通过；N722格式/编译/标题通过。F723七命令/892文件/10,359测试
通过，源/Git稳定；之后分页调整的5文件67项、typecheck、受影响lint与diff检查通过。
最终分页源码F725七命令/893文件/10,365测试全部通过，源码/Git稳定；F726界面
构建通过，资源及输入绑定F725。N724完整库4,281通过/
3失败/106忽略：两处旧恢复测试仍要求只有planId/hash，第三项为子进程复用失败；
已严格更新为核对4字段及其真实值，保留原防篡改断言。N728格式/编译/标题通过；
N729恢复专项（storage2/recovery1/inherited-home父1及其子进程）与strict Clippy
55.58s通过，原session26283 exit0并消费，源码/Git/EXE稳定。N730新完整门全部
12命令通过：全库4,284pass/0fail/106ignored，显式历史87、retention44与strict
Clippy通过，集合不相加。原session55381 exit0并消费；fresh核对850native/
2570frontend/26seed/Git/index/EXE、40质量门和154份LF-noBOM文本通过，owners退出。
自身temp按inventory→Execute清理380文件/5,254,458logicalB，观察H空闲增加
5,328,896B另列。N724失败日志/sandbox保留。V727嵌入构建及上述限定桌面验收现已完成。
V710旧包已按替代物/精确owner/hash核对后退役，保留全部日志和合成归档。原53项、mixed/scoped容量与优化构建/
万条性能、UNKNOWN100、实际Undo、原厂/WAL/ACP、三平台/IME-DPI/15语言实机仍OPEN。
以下段落为保留的历史记录，V710入口及“界面待接入”不代表最新状态。

### 此前公开提交专项

15语言已把作业终态计数改为“已处理”，与成功导入数分开；相关前端3文件75项通过。
公开commit已把明确确认的历史关联plan送入既有原子接受链，普通lane仍拒绝关联；
关联加explicit reimport仍拒绝，原请求重放早于重开preview。N714历史接受60项通过；
N715新专项3通过1失败为普通单事件fixture误用双事件断言，证据保留，修正后N716
格式/编译及原始发现1项通过，N717公开commit4项和严格all-target Clippy通过。
847native文件、HEAD/index与N717记录fresh核对一致；26seed稳定，原owner退出。
N717自身空temp经两步清理：0logicalB/观察空闲变化0B。以上是专项，不替代完整门。
当时预处理批次界面尚待接入；最新实现见本页顶部，保留prepareOrigin和原批次receipt。
唯一试用包仍V710，未包含本段源码改动；不提升current-preview.json或宣称实机通过。
下面的“仅文档修改”“公开关联guard仍在”“处理数文案待修”仅代表此前状态。

## 当前进展（2026-10-15，公开作业控制）

取消、重试、撤销的公开 API 已接入原合同要求的 requestId 与
expectedStateVersion。真实写事务检查版本并持久化请求身份；重复请求不再次
执行，状态冲突刷新后由用户复核。撤销的原始清理意图与内容回滚一同提交，
进程重启只接续原清理；用户修改过的项目保留。界面保留不确定响应的请求身份，
15 语言错误文案齐全，作业读取已移入 blocking pool。

N706 专项92通过/0失败/1专用子进程helper忽略（父测试另行执行并通过）。
F705 全前端890文件/10,348测试通过，types/lint/quality等七命令通过；F709
界面构建通过，输入、Git及资源哈希已绑定。N707显式历史81项、retention44项
通过，但严格Clippy发现8个只剩测试调用的旧入口及1处多余转换；完整库尚未
执行，不能称完整门通过。旧入口现限定cfg(test)，未放宽检查；N712的格式、
编译及92项专项通过。N713全部12命令通过：历史81项、retention44项、严格
Clippy和完整库4,282通过/0失败/100忽略。Fresh核验846native/2564frontend/
26seed/Git/index/EXE、40质量门及125份UTF8-noBOM/LF文本通过，精确owner
均退出。自身temp经inventory→Execute移除380文件/5,254,431logicalB；观察
H空闲增加5,853,184B另列，不能全归因于删除。
详细run/原owner/清理记录见任务根 `plans/v699-public-control-writers.md`。

唯一试用入口已提升为 V710 / `start-current-preview.cmd`（2026-10-15）。
F705/F709绑定的嵌入构建通过；中文Windows合成实测2/40/160条导入、阅读/
搜索、任务重开、跨页全选和运行中取消/重试。160条作业取消保留20条成功项，
重试最终160条；退出后只读核对新增202个entry/revision/committed receipt，
408事件无重复，旧归档6文件/不可变seed26文件未变。精确8进程家族正常退出，
原guard49441和显式quit41871均exit0并消费。固定入口plain-cmd CheckOnly通过，
不另启动App；旧V325包已按replacement/owner/hash校验退役，数据与日志保留。
本轮UI仅验证撤销二次确认/取消，未实际删除。已记录取消后“已完成160/160”
实际表示终态处理数（当时仅20条成功）的文案歧义；下步先修处理数/成功数表达。
公开历史身份关联、mixed/scoped与万条容量/优化构建性能、UNKNOWN100、原厂/
live-WAL/真实ACP、三平台、IME/DPI及15语言实机余项仍开放，原53项范围保留。
无新commit/push/PR/release。完整桌面证据及清理量见任务根V710 run，后续仅有
文档/外部工具收口，产品构建输入未改。
以下均为保留的历史进展，包括文末旧“剩余工作”段落；旧入口名、
“公开控制待实现”和测试数字仅代表对应历史源码，不作为当前启动指引。

续接N698（2026-10-15）：当前普通合成1GiB两轮独立导入/热读搜索实际1pass/
0fail/0ignored，原session53050 exit0并已消费。总771.368s，单轮preview
178.777-180.347s、commit+worker203.667-204.715s；首批后端页14-15ms，热读12ms/
搜索1111ms。峰值working set46,600,192B，完整imports/source约2.0264倍，小于3倍。
末尾10ms取消来自独立小排队作业，不是1GiB发布中取消；没有UI/release计时。
fresh837native/2562F621frontend/26seed/Git/index/EXE/N696字节稳定，精确owner退出，
task构建/原生/App owners0。不可变大样本CheckOnly复核通过；自身runtime-temp
已清理0logicalB/观察free delta0B，不作回收量声明。后续按任务根
`plans/v698-gib-and-public-controls-next.md`闭合原合同的公开cancel/retry/undo
requestId去重与expectedStateVersion；保留撤销事务及项目清理恢复边界，随后接UI。
万条、genuine mixed/scoped、公开身份关联、UNKNOWN100、原厂/WAL/ACP/三平台/
IME-DPI/15语言及当前源码嵌入桌面预览仍OPEN。唯一V325入口未提升，无新提交/发布。

续接（2026-10-15）：N697 原 session43015 已 exit0 并消费；千条取消/离线恢复
实际1pass/0fail/0ignored。200条时取消，返回342ms，worker停止2165ms；原合成
来源离线后retry292ms，余800条恢复，最终1000 entry/revision/committed receipt/
reader event逐项核对，原回执字节不变且重复retry不再发布。DEBUG总3824.417s，
恢复发布3038.470s、终验22.006s，峰值working set61,444,096B；不是UI/release计时。
fresh核对837native/2562未变F621frontend/26seed/Git/index/EXE与N696原记录通过，
精确owner退出、task原生/构建/App owners为0。wrapper已移除自身runtime-temp：
0logicalB、观察free delta0B，无空间回收声明。千条取消恢复正确性闭合，性能仍慢；
未另跑完整千条publish，不能据此将万条、原厂解析或整个交付写成完成。
profile确认余800条仍有25,604次整批header及25,616,000次member校验；先独立
区分优化构建与DEBUG的开销，再测万条，不原样重跑这一小时。所有认证与断言保留。
本轮只修正文档中的旧入口/结论：START 与 CURRENT-PREVIEW 统一为实际 V325；
验收结论区分已实现的内部 signed 链与仍有 guard 的公开关联提交；兼容矩阵和
小 PR 草案明确旧 HEAD/旧 run 状态为历史。固定入口 CheckOnly exit0、哈希核对通过，
未启动 App。当前原生源码/EXE未改，N696 仍为最新完整门；精确原 owner 和终态
见任务根 `plans/v697-thousand-cancellation-run.md`。原 53 项及其余交付范围保持。

## S240（2026-10-15，批量发布优化及完整原生回归通过，尚未交付）

已批准的检查点与上游合并 7eb1b7b9 已完成，不重复。N681/N682 的完整回归和
100 MiB 成绩只证明当时源码；后续改动不能沿用旧 EXE 哈希作当前验收。
N684/N685 定位了64条合成预处理批次→真实plan/acceptance/worker发布中的重复认证。
该负载不包含来源扫描/厂商格式解析，不能称为端到端来源导入或UI测量。
N686 编译通过，N687 acceptance 34 项及 authenticator 4 项通过（集合重叠）；
N688 相同完整负载 1 pass，发布从 48.014s 降至 30.582s，单轮减少 36.3%。
64 entries/revisions/committed receipts/readers 全核对，最大并发 poll 29ms。
缓存只保留精确原字节和已加载密钥上下文对应的纯 codec/MAC 事实，有界且每次
仍核 expected header/candidate；不缓存 SQL 授权或跳过原 input/root/owner/ACK。
N688 own 空 temp 经 inventory→Execute 删除，0 logical B / 观察 H free delta 0B。

千条性能尚未修复验收。第二轮将完整成员读取合为单次有界 SQL 范围扫描，复用
同一次只读校验刚读到的成员，减少重复 candidate 列表构造；写入 witness 的
事务指针/total_changes 及各项当前事实检查保留。增加复制/改名、暖缓存损坏、
邻近任务隔离和 8192/8193B 边界控制，预算/BOM 拒绝早于缓存 key 哈希。
N689 编译/77outcome、N690 acceptance35/authenticator4 与 strict Clippy 通过；
N691 同一 64 条完整负载通过，发布 25.379s（较最初单轮减少 47.1%），最大 poll21ms。
fresh837native/2562未变F621frontend/26seed/Git/index/EXE/40quality/84LF-noBOM 核对通过。
以上均为 DEBUG 原生测量，不代表 release 或 UI 性能。N692 另行通过81seed/44retention/
Clippy，但全库201.65s时主动中止，以补批量查询异常超长 key 的读取前限界；native-1/
wrapper1是中断，不是断言失败或通过，原现场和日志保留，原owners已退出。
现 SQL 只投影 prefix+64 字节的 TEXT key，并新增1MiB异常key负控。
N693 fmt/compile/77outcome通过（compile129.38s），N694 acceptance35/authenticator4
及strict Clippy通过。N695修补后同一64条完整负载通过，发布24.389s、总测试27.63s、
最大poll30ms，64项entries/revisions/committed receipts/readers全核对。
较N685原单轮DEBUG发布49.2%更短，不外推其他容量或UI性能。
N694/N695 fresh核对837native/2562未变F621frontend/26seed/Git/index/EXE/
40quality/84LF-noBOM通过，owners已退出。N696修补后的完整12命令门已通过：
全库4267pass/0fail/99ignored，1088.90s；显式历史seed81pass/0fail/0ignored，
228.70s；retention44pass及strict all-target Clippy通过。集合不累加，ignored不算通过。
原session70639已exit0并消费，source/Git/index/EXE稳定。N696 fresh核对837native/
2562未变F621frontend/26seed/40quality/85UTF8-noBOM-LF修改文本通过，原owners退出。
其自身temp经inventory→Execute删除380文件/5,254,431logicalB，观察H free增加
5,853,184B（包含并发磁盘活动影响，不作独占物理回收归因）。原gate可重建。
N692中断temp仅10文件/295,627logicalB，与原中止记录保留；N683原现场也保留。
不得重启已消费的N693/N694/N695，也不能沿用N692作为完整通过。
最新完整native/Clippy本身不代表容量、桌面或三平台验收；千条取消恢复已由上方
后续N697闭合，其他容量/桌面/平台范围仍OPEN。
原始 N683 中断现场保留。唯一用户预览仍为 V325 / start-current-preview.cmd。
其余原交付范围不缩减；详细证据见任务根 v683-signed-batch-performance-triage.md。
成功专项空temp已两步清理（0logicalB，不作空间回收声明）；中断现场保留。
本轮没有启动 App、修改真实数据或新增 commit/push/PR/release。以下为历史记录。

## S239（2026-10-15，完整原生回归与 100 MiB 导入通过，尚未交付）

已批准的本地检查点与上游 v0.2.41 合并保持为 7eb1b7b9，无需重复。
N681 完整门实际通过 12 个命令：完整库 4264 pass / 0 fail / 98 ignored，
1104.19s；另行显式执行历史 seed 81 pass / 0 fail / 0 ignored，234.20s；
retention 44 pass，strict all-target Clippy 通过。各集合重叠，不累加。
fresh 核对 834 native / 2562 未变 F621 frontend / 26 seed / Git / index /
EXE，40 项质量检查和 78 个修改文本 UTF8-noBOM/LF 通过，原 owners 已退出。
原 N671/N672/N674/N676/N678 RED 证据保留；未放松 signed 认证或历史合同。

N682 在相同源码和 EXE 上完成 100 MiB source→plan→acceptance→worker 三次
独立导入、三次 warm 读取/搜索，实际 1 pass / 0 fail / 0 ignored，115.92s。
单轮 API 阶段合计 37.63–37.85s，其中 preview 17.62–17.69s、发布
19.86–20.13s；首批读取 12–14ms，warm 11–12ms，搜索 445–455ms。
采样峰值 working set 42,905,600B（约 40.92MiB）；完整 imports 占用约
2.029 倍 source，未超过原 3 倍约束。cold 指独立 App home，不清 OS 文件缓存。
这不是 UI 首屏时间，也未单独测量 encoded durable input 的超大容量。

N681 own compiler-temp 先 inventory 再独立 Execute，删除 380 文件，
5,254,431 logical B；观察 H 可用空间增加 5,853,184B，不作独占物理回收归因。
N682 owners 退出后移除自身 runtime-temp（末步 0 logical B / 观察 delta 0B），
保留 logs/metrics；immutable 大样本 CheckOnly 复核通过。原命令可重建临时目录。

N683 千条取消/恢复在 528.57s 因明显退化被主动中止：最后 worker 进度
481.98s 仅 committed=58，未达到 200 项取消点；native exit=-1，wrapper exit=1。
这是中止而非取消断言失败/通过，取消响应、停止、离线恢复均尚未测到。
源码/Git 稳定，原 owners 已退出，失败日志及隔离现场保留。重复整批认证为代码
可见热点，需采样确认贡献后优化，不能从部分发布外推完整耗时或削减认证。
下一步按任务根 plans/v683-signed-batch-performance-triage.md 定位。1GiB、万条容量/取消、
真实 mixed/scoped 批量、public requestId/version、UNKNOWN100、UI15语言、
vendor/live-WAL、ACP四组合、三平台、IME-DPI、当前源码嵌入预览与小PR审阅仍 OPEN。
唯一用户预览仍为 V325 / start-current-preview.cmd，不能冒充当前源码构建。
本轮无 App、真实数据改写、新 commit/push/PR/release。下文 S238 及以前为历史。

## S238（2026-10-15，启动认证修复通过专项，完整回归待完成）

本地检查点2cdb8d5c与上游v0.2.41合并7eb1b7b9已完成，不重复集成。
N670 fmt/compile/77outcome通过。N671输入损坏专项通过：missing/malformed/
oversized-title均在启动前两次零写拒绝，健康job继续，恢复原字节后成功。
后续兼容分组60/1/1/1/2通过，但撤销fault组1pass/2fail；ownership与Clippy未执行。
分组有重叠，不能累加为完整门。834native/Git/index/EXE与26seed稳定。

撤销失败源于旧测试将signed任务当历史协议：缺失回执预期写conflict，以及
malformed/oversized回执的旧解析错误码。正在分离真实历史v2与正常signed接受路径；
原历史断言保留，新版必须拒绝且全部catalog行/类型/原字节及归档不变。
N672 fmt通过、compile缺helper导入失败，无测试执行；修正后N673 fmt/compile/
77outcome通过，N674关联Undo全部5项通过，普通导入缺回执同类旧断言0pass/1fail。
现保留真实Legacy冲突并验证signed两次全行零写；N675 fmt/compile/77outcome通过。
N676完整旧seed命名空间81项实际80pass/1fail/0ignored，231.52s：撤销/ownership全过，
发现流程仍把新plan断言为v2而收到v3，Clippy未运行。现明确断言v3/descriptor/协议，
历史未接受input解析探针使用独立真实Legacy plan，不将伪造signed行当正例；
signed原codec/上下文/拒绝断言保留。N677 fmt/compile/77outcome通过；N678发现流程
0pass/1fail是更后方未接受signed发布仍期待旧PlanStale。signed缺认证精确拒绝与
Legacy无上下文原断言已分离。N679 fmt/compile/77outcome通过，N680发现流程单项
通过（4.79s）。准备执行N681完整原生门，包含全部81seed、retention、严格Clippy
与完整library；前述失败终态保留，不能声称当前81全绿或完整native已过。
不通过放宽认证或把signed故障任务降级来修绿。详细终态见任务根v664报告。

N670/N673/N675空compiler-temp经inventory→独立Execute清理：各0logicalB，观察H free各-4096B，
无空间回收声明；原gate可再生。失败证据和源码/依赖/seed/当前EXE保留。
唯一V325预览未提升；本轮无App、真实数据改写、新commit/push/PR/release。
完整native、容量/取消/UNKNOWN、UI15语言、vendor/live-WAL、ACP四组合、三平台、
IME-DPI、性能、当前源码嵌入桌面预览与小PR审阅仍待完成。以下为历史进展记录。

## S237（2026-10-15，显式旧 seed 回归修复中，尚未交付）

N664在N662 EXE上显式执行第一组57项：48pass/9fail/0ignored，101.1810777s；
后面五组未执行。833native/Git/index/EXE与26seed稳定，失败证据保留，不计65全过。
无完整native门。失败涉及旧无ACK/事务内测试入口与signed原子ACK/事务外crypto的
协议混用，以及接受后修改ordinary input的历史正例。不能放宽signed校验解决。

本轮只修测试夹具：独立历史v1/v2计划按原payload/lifetime重算hash，走真实加载和
接受，不覆盖signed计划或制造grant；保留原Legacy容量/错误/恢复断言。正常signed
metadata/fence/独立发布走实际worker slot；新增损坏input零写拒绝且健康任务继续、
exact-original恢复、单次原子ACK及mixed取消/恢复控制。signed超大input接受容量尚未
验证，不能用Legacy容量或篡改后拒绝代替。N665测试helper两处作用域编译失败，已改；
N666fmt/compile/77outcome通过。N667首组60项59pass/1fail，165.72s：原九项已过，
新增signed input损坏队列控制预期拒绝却收到Ok(2)，原因未定；后六组/Clippy未执行。
两种mixed取消及signed单次ACK控制通过，不把新负例直接放宽。N668正重编增加具体
fault/协议/状态和零写诊断，fmt/compile/77outcome通过。N669单项0pass/1fail：
missing两次正确零写拒绝；malformed却完成2job并把坏job写成completed_with_issues。
原因是启动writer只核header/member集合，不核原input body；reader随后又拒绝坏记录。
现start_job在同一Immediate事务先验证全部原input，再调用原change/save路径；只在
启动一次做全量，不给每item/progress增加全body扫描，crypto仍在事务外加载，Legacy
保持原行为。N670重编验证中；这次已包含产品修复，未运行完整门。逐项见任务根
plans/v664-signed-seed-protocol-compatibility.md。唯一V325未提升，无App/真实数据或发布。

## S236（2026-10-15，正常 signed producer 回归修复中，尚未交付）

系统 plan/job producer 已在本地 WIP 切换 SignedOutcomeV1；正常 V3/V8 不再处于
下述历史记录的 fence 状态。public association guard 仍保留。真实 source→plan→
acceptance→worker 的 create/match/fallback、dedup/restart/Undo 正负控已开始运行，
原 N558 foreign 项目保护按原断言通过，不等于完整交付。

N647 扩大回归实际1199pass/50fail/88ignored（实际执行1249项），未运行Clippy。
已定位并修复 proof1 adoption 的旧 import allocation CAS，以及 accepted ordinary
job 错读可变 draft 的问题：恢复只核接受事务保存的 original snapshot，不从 draft
修补缺失原件。原 input/owner/member/allocation 校验、Legacy容量与字节契约不放宽。
N648 legacy专项66pass/5fail/75ignored；N649 acceptance专项32pass；N650七组
回归98pass，但 strict Clippy四处未使用代码/过期expect报错，整个门为RED。
N651 复现 explicit reimport 在 Undo 后错误复用旧 signed ACK 的真实失败。
修复后N652fmt/compile/71outcome通过（含真正signed重新导入正例）；N653完整
qualified legacy组46pass/0fail/75ignored，strict all-target Clippy通过，已fresh核
830native/2562未变F621frontend/26seed/Git/index/EXE与40quality、57文本LF/noBOM。
以上集合重叠，不累加。N654fmt/compile后73pass/1fail：真正signed late dedup
trigger回滚/离线重试和saved-workspace引用保留通过；ACK改写拒绝及零写也通过，
但测试最后把job_get派生progress与过期DTO比较而失败。应改为精确持久header比对，
不改拒绝逻辑。N655最终212pass/2fail/79ignored，两个失败来自历史fixture复用
signed plan ID。Legacy helper现创建独立ID和真正v1 payload/hash，走真实commit，
不覆写signed计划。N656fmt/compile/77outcome全通过，包括修正后的ACK零写拒绝、
真实mixed Undo后项损坏写前拒绝、late-trigger整批回滚、初始failed成员及重复Undo/
离线retry。N657engine214pass/79ignored、job-control50pass/1ignored，但mapping
5pass/1fail；后续retention/protocol/Clippy未执行。N658fmt/compile/77outcome通过，
N659单项0pass/1fail。N660/N661调试器定位到mapping测试后半段手工改completed
ACK再retry：首次发布其实通过，原“首次发布失败”判断撤回。调试器exit0但测试
进程101，不能计绿。该历史ACK重放现分离到独立v1 producer；signed保留真实正常
发布/错误source零写拒绝，并新增实际第二成员receipt故障→源离线→原allocation
retry控制，不把篡改signed ACK变正例。N662 fmt/compile/77outcome通过；N663
mapping8pass、retention crypto4pass、publication protocol15pass，strict all-target
Clippy55.6843089s通过。fresh核833native/2562未变F621frontend/26seed/Git/index/
EXE与40quality、66文本LF/noBOM，owners已退出；27项与其他集合重叠，不算full gate。
当前测试EXE SHA256为81bc78015a8b806e73ed3b2c5f26b7d42516be34df7c2e84cc40051758884c18。

N649/N652/N653/N656/N658/N662/N663成功专项的空compiler-temp经inventory→独立Execute删除：
各0logicalB，观察H free依次-4096/-4096/+4096/-4096/-4096/-4096/0B，无回收声明；原gate可再生。
N647/N648/N650/N651/N654/N655/N657/N659失败证据保留。
未启动App、未更新唯一V325预览、未再commit/push/PR/release，也未碰真实数据。
上游v0.2.41合并仍为7eb1b7b9；无需重复检查点或合并。详细失败分组、后续顺序见
任务根 plans/v647-signed-producer-regression-triage.md。完整native/Clippy、真实
mixed rollback、容量/取消/UNKNOWN、UI15locale、live-WAL、ACP四组合、三平台、
IME-DPI、性能、唯一新嵌入预览与小PR审核仍OPEN。下方S235及以前为历史记录。

## S235（2026-10-15，Undo writer 接线切片通过，非整体交付）

真实 undo_job 现在接 UndoSnapshot：同一具体 writer 在首次 target 写前，验证
所有初始 committed 的 signed 成员（含 dedup）的原 input/owner/acceptance/
member/fence/allocation、完整 terminal 与精确 ACK/version；固定真实 receipt
供循环和后 COMMIT native 清理，不重新取可变 latest receipt。dedup 不获得创建
所有权。save_job 后、COMMIT 前再核原 immutable 链、revision/previous 和预期
更新后的 phase/error/version ACK，并按实际顺序合并同 import 的预期显示变化、
核完整 entry/tombstone。归属验证没有 archive/native/preview I/O 或现场补签。
Legacy 行为保留；catalog→store 引用锁序及 native 完整Project CAS未改。

新增三项 result/effect primitive 验证移走preview/重开后原链与新ACK、十次零写、
十二种 late result/revision/entry/tombstone/ACK 改写拒绝及 conflict/既有tombstone/
共享条目效果；原dedup测试增加不能转成Undo所有权的断言。它们没有制造accepted
source/proof grant，不能算正常signed Undo或mixed事务正例，也没修绿原N558。
N638 原89037 exit0：fmt/compile/66outcome通过；N639 原92367 exit0：27组
408pass/0fail/2既有ignored，strict all-target Clippy56.093022s通过。N640 原
52993 exit0：六组显式--ignored实际65旧seed全通过。408含66及新增三项，不叠加。
两份fresh各829native/2562未变F621frontend/26seed/HEAD/index/EXE、22产品文本
LF/noBOM、40quality，owners退出；App13244/大文件80不增长。当前EXE SHA256：
09b7f093f6a74e66621d89467d0bc8ac517907f0f55e528d9daf83a4da0ff759。

N638/639/640 own temp均inventory→独立Execute清理；N639删169files/1924922
logicalB，观察H free+2174976B（非独占回收），两空temp0B/delta-4096/0B。
相同源码重跑原gate可再生；精确target/hash/owner见cleanup JSON。失败证据/
源码/WIP/依赖/seed/当前EXE/符号/V626/唯一V325保留，未启动或提升App。

正常V3/V8仍fenced；当前只覆盖初始committed成员，其他phase/重复Undo、真正
signed mixed/late-trigger全事务回滚仍需正常producer验收。下一不再只加primitive：
闭合正常source→plan→acceptance→worker正负控及Legacy无结果证明的项目保留
语义，完整单元中切换系统producer/new_job_id，再保持原断言跑N558、真实create/
match/fallback/dedup/crash/Undo和当前完整native门。不能以永久禁用合法Undo、
caller/test授权开关、旧mutable结果补签或改N558预期代替。详细见任务根
next-protocol§21/B5§27。完整原范围保留，无checkpoint/commit/push/PR/release。

## S234（2026-10-15，旧 publisher / Reuse 接线切片通过，非整体交付）

Reuse 与独立 dedup recovery 现在准备实际 receipt publisher 的 crypto，并在
当前 SQL snapshot 重选原 durable input/member/owner，核 accepted allocation、
完整 terminal、唯一 ACK slot/version 和精确 receipt/revision/selection。dedup
不能充当旧 publisher 或继承 created 所有权；prepared crypto 不是跨事务 grant。
Reuse 拆为 SQL 捕获→事务外 archive 实际验证→新 snapshot 重核，旧 semantic
fallback 只先比较 SQL metadata，不在 snapshot 内读取 archive。final writer
仍在写前、写后核旧 owner。正常 require_publisher fence 没有解除。

新增四项测试覆盖真实 Legacy archive/semantic fallback 的 autocommit 边界、
entry/receipt/revision 改写后真实文件验证及 SQL 拒绝、重开/移走 preview/十次
零写 terminal/唯一 ACK、foreign/重复 slot/unknown/version drift 拒绝。后两项
是 terminal primitive，不是 accepted signed source/grant 正例或 Undo 证明。
N635 fmt/compile/63outcome 通过；N636 原45719 exit0：27组405pass/0fail/
2既有ignored、strict all-target Clippy54.7170189s通过；N637 原68448 exit0：
六组显式 --ignored 实际65旧seed全部通过。405含63及新增四项，不叠加。

两份 fresh 各绑定827native/2562未变F621frontend/26seed、HEAD/index/EXE，
17产品文本LF/noBOM、40quality及exited owners；App13244/大文件80不增长。
EXE SHA256：6d1da02a624e13c8ea213390a232f44bf29e9e7616d48c15c38468cc22c28eff。
N635/636/637 own temp 均inventory→独立Execute收尾：N636删除169files/
1924922logicalB，观察H free+2174976B，非独占物理回收；两空temp0B，观察
delta-4096/0B。原gate可再生；精确target/hash/owner见各cleanup JSON。源码/
依赖/seed/当前EXE/符号/失败证据/V626/唯一V325保留，没有启动或提升App。

下一直接实现同链 Undo 的首次写前全成员归属验证、固定结果消费及提交前重核，
保持引用/原生项目 CAS 锁序；原N558须保持原断言修绿，不能永久禁用合法Undo。
正常 signed producer/prior-owner 正负控、Undo/mixed和完整原范围仍OPEN，V3/V8
继续fenced。无新checkpoint/commit/push/PR/release或真实数据调用。见任务根
next-protocol§20及B5§26。以下为历史记录，不代表当前仍停在旧读侧拒绝接缝。

## S233（2026-10-15，独立结果恢复读侧切片回归通过，非整体交付）

本地上游合并已提交为 7eb1b7b9，双亲为 WIP 2cdb8d5c 与 upstream/main
ebc9ff5a（v0.2.41）；没有重复合并。本轮 9 个已有 native 文件及 3 个新增
native 文件未 stage/commit，没有 frontend 改动。

恢复不再依赖 CommitItem/prepared selection 或保留的 preview：RecoveryInput
借用原 DurableInput 元数据，同 snapshot 核 crypto、owner、acceptance/member/
input/fence/allocation，再读完整 allocation→stored→committed 或 allocation→
dedup 链、稳定 receipt 前缀、revision/selection 和精确原 ACK。SQL snapshot
退出后验 archive/projection，重开 snapshot 比对原事实。signed 返回 Recorded
及已核版本，不重复 setter；Legacy 保留原 Pending、校验顺序和显示标题行为。
新链重建 ACK 使用原 input title/outcome，不使用后来编辑的 entry.title。
dedup 使用真实旧 revision，不消费预留新 ID、不继承 created 所有权。

新增 4 个 result/receipt/ACK primitive 和 2 个真实 Legacy reader 测试，覆盖
preview 已释放、重开 catalog、十次零写、缺行/协调 mutable 改写/未知 ACK
字段拒绝和旧 dedup。它们不是 accepted signed source/grant 或正常 producer
正例。旧 signed prior-owner 仍明确拒绝，不能 Legacy 降级或把读结果当 Undo grant。

N632 原 87096 exit0：fmt/compile/59 outcome tests；N633 原 33425 exit0：
27 非零 filters 共 401pass/0fail/2既有ignored，strict all-target Clippy
55.0706734s 通过。401 已含 59 和上述 6 个新增测试，不叠加。N634 原 6980
exit0：六 filters 显式 --ignored 实际执行旧 seed 65pass/0fail/0ignored。
两份 fresh-closeout 各核 826native/2562未变frontend/26seed、HEAD/index/EXE
稳定，12 产品文本 UTF8-noBOM-LF、40quality 通过，owners 退出；App13244/
大文件80不增长。前端仅重新绑定 F621，不是本轮重测或 full native/桌面验收。
当前测试 EXE SHA256：
2cbcc8bf7380171e00275ab0538b0dc54a5e2160ada10abe4655d03fd28fface。

成功 N632/633/634 own compiler-temp 已 inventory→独立 Execute 清理，可由
相同源码重跑原 gate 再生。N633 删除169files/1924922logicalB，观察 H free
+2174976B；另两空 temp0B，观察 delta-4096/0B。观察值不是独占物理回收量。
精确 target/hash/owner/再生方法见各 run cleanup JSON；源码/WIP、依赖、seed、
当前 EXE/符号、失败证据、V626 bundle 和唯一 V325 均保留；未启动 App。

下一先接旧 signed prior-owner 的事务外 crypto 准备、当前 snapshot 原结果/
receipt/revision/selection/exact ACK 校验和 Reuse seam，并将其 archive I/O
移出 SQL snapshot。再接同链 Undo/mixed rollback、保持原断言修复 N558。
正常 source→plan→enqueue→worker 全部正负控闭合才开放 V3/V8；原容量/取消/
UNKNOWN/UI/15locale/live-WAL/ACP四组合/三平台/IME-DPI/性能/唯一嵌入预览/
小 PR 审阅义务不变。下一接线见任务根 next-protocol §19。无 push/PR/release、
真实数据/账号/模型调用或预览提升；原 goal active。以下为历史阶段记录。

## S232（2026-10-15，上游 v0.2.41 集成回归完成，非整体交付）

用户已明确批准仅本地 WIP 检查点及分步集成。136 个原 dirty/untracked 路径
逐字节备份、凭据扫描与漂移检查后保存为 2cdb8d5c；当前分支为
`wip/session-import-upstream-integration-20261013`。本轮集成
`upstream/main=ebc9ff5a`（v0.2.41），远端再次核对未前进；origin/main 仍为
b58b97cd。原 98e3b524/a0883e7d/12f37c74 不重复或覆盖。

六处接缝均保留双边语义：PR MCP 绑定实际 App chat/cwd，仍使用 captured
main runtime 的官方/第三方隔离；connect 保留 import history grant、resume
拒绝及 owner 释放；control 保留一致捕获的 app/agent/cwd/acp tuple；外部消息
保留 durable claim/outcome、幂等和 UNKNOWN 不重发，不恢复旧 take/requeue。
Host origin 改为单次 send 携带，在真正写 user journal row 时标记；排队、取消、
拒绝及穿插的手动消息不能污染下一条消息。旧外部队列格式不变，实际 drain
才写 Host 标记。composer 继续用领域 hook 接 repo PR 列表和 session-owned
watches，不把大块功能状态塞回 App/AppWorkbench。

F621 全量前端 888 files / 10337 tests、typecheck、whole lint、40 quality、
website selftest 及 27 markdown/notes tests 全部通过。V626 生产 Vite build
通过，162 产物 hash 无漂移，尚未嵌入桌面 EXE。N620 synthetic client 缺新
app_session_id 导致编译失败，已补 None，失败证据保留。N622 fmt/compile/
55 outcome tests 通过；N625 11 非零 filters 共 267pass/0fail/2既有ignored，
strict all-target Clippy 82.3978938s 通过；N627 原发布链 27 非零 filters 共
395pass/0fail/2既有ignored；N628 六 filters 显式 --ignored 真执行 65 旧 seed
发布/恢复/final ACK/Undo/refresh/continuation/fault tests，65pass/0fail/0ignored。
395 已含 55，各专项可能重叠，不累加为完整 native 验收。N623 owner guard、
N624 零匹配 filter 拦截记录保留；正确模块名运行后才计入 N625。

N625/N627/N628 各独立 fresh-closeout 绑定 823 native/2562 frontend/26 seed、
原 HEAD/index/EXE，40 quality 通过、owners 退出，App/AppWorkbench 13244、
千行源文件 80 不增长。首次 fresh 被 session-api 文档 CRLF 拦截，已统一 LF，
未放宽 guard；三份最终 fresh 均成功。当前测试 EXE SHA256：
9f3febf0caa42b46c1702a7186ccfee6904ea684b32563b704a1e1a5cf83503c。
这不是 full native、桌面、三平台或导入最终验收。

成功 N622/N625/N627/N628 的 own compiler-temp 均经 inventory→独立 Execute
删除，可由同源码重跑原 gate 再生；日志/当前 EXE/符号/依赖/seed/源码保留。
N627 删除 169 files / 1924922 logicalB，观察 H free +2174976B；其余三个
空 temp 各为 0 logicalB，观察 delta 依次 -4096/+4096/0B。观察值不等于独占
物理回收量。失败 N620、拦截 N623/N624、最新 V626 ui-dist 和未替换的旧预览
均保留，不绕过 owner/replacement 规则清理。

本轮仅本地合并：提交前逐 staged blob 核工作字节、UTF8/noBOM/LF、敏感路径
及凭据，提交后核准确双亲、tree 和 clean；记录在任务根 v630 审计。v629 因
审计脚本异步 void 返回混入 byte[] 失败，已修脚本并保留失败，不覆盖证据。没有 push、
PR、release、模型/真实账号调用或真实应用数据迁移。唯一固定入口仍为
`start-current-preview.cmd` / V325，plain cmd -CheckOnly 通过，未启动或提升。

**仍未交付**：独立 durable reader/recovery、signed prior-owner、Undo/mixed
回滚及原 N558 foreign 项目记录误删 RED 仍 OPEN；正常 V3/V8 继续 fenced。
原容量、取消、UNKNOWN、UI/15locale、live-WAL、ACP 四组合、三平台、IME/DPI、
性能、唯一新嵌入预览和小 PR 审阅义务全部保留。下一接线见任务根
next-protocol §18；以下 S231 及更早为历史记录，不表示上游仍未合入。

## S231（2026-10-15，已提交结果的 ACK 重放修正回归通过，未交付）

本轮重新确认本地检查点98e3b524及两次集成a0883e7d/12f37c74已存在，fetch前
两main本地ref均为HEAD祖先。随后只读远端核对发现源项目新增8提交，已fetch到
upstream/main=ebc9ff5a（v0.2.41），尚未合入；origin/main仍为b58b97cd。82个
上游改动路径未与当前dirty/untracked路径重叠，仍需在本轮gate/清理结束后的
独立集成边界处理，不能将v0.2.40 EXE冒认为v0.2.41验证。merge-tree预演发现6个
committed集成接缝冲突（不是dirty路径重叠），未改工作区/index或开始实际merge。
具体文件和双边保留要求见任务根next-protocol§17。未重复旧检查点/合并。

signed publisher 自身 committed 重入此前校验原 ACK 后仍返回 Pending，worker
会再次 setter、推进 stateVersion。现改为返回同一校验 snapshot 中读取的版本和
Recorded progress；dedup replay也直接携带该校验返回值，不另查版本。Legacy
恢复/普通 Pending 仍走原 setter；没有新增可由调用者跳过认证的公开接口。

新增两个进度分派测试使用正常 Legacy enqueue/worker：重开 catalog 后安装
拒绝 UPDATE 的 trigger，十次 Recorded finish 必须零写、原 JSON/版本/phase/
durable input 不变；Pending 必须推进一次版本/phase且不重写 input。原 signed
ACK primitive 还校验十次读取零写，以及后续 header 版本一致推进时返回当前版本。
它们不是正常 signed source/accepted producer 正例。N615编译因新测试遗漏jobs
模块导入失败，已补导入、保留原证据。N616原80904 exit0：fmt/compile/55结果链
专项通过；N617原45370 exit0：27非零filter共395pass/0fail/2既有ignored，
strict all-target Clippy57.5669584秒通过。395已含55和两个新分派测试，不叠加。
N618原29700 exit0：6非零filter显式--ignored实际执行65旧seed测试，0fail/
0ignored，含发布/crash恢复/final ACK/Undo/refresh/continuation/fault；26seed稳定。
这不是正常signed producer、完整native/桌面/平台或最终交付验收。

N617/N618各独立fresh-closeout：815native/2556frontend/26seed/HEAD/index/EXE
稳定，136产品文本LF/noBOM，40quality通过，owners退出，App13244/大文件80
不增长；前端只是重新绑定，未重测。最终测试EXE SHA256为
9db54de80aff6b57e584a5c4b6eb2684f825b5e469dfa6d83f0e7194dfef867f。
成功N616 own空temp经inventory→独立Execute清理，0logicalB，观察H free delta
-4096B；N617 own169files/1924922logicalB同样清理，观察H free+2174976B，
非独占物理回收；N618 own空temp同样0logicalB/delta0B清理。精确targets/hash/
owner/再生方法保存在各run cleanup JSON，再生为同源码重跑相同gate；N615失败
日志及空temp保留，不借成功门删除。源码/WIP/依赖/seed/当前EXE/符号和唯一V325
保留。plain cmd固定入口-CheckOnly通过，launched=false，无运行preview。

独立 durable reader/recovery、signed prior-owner、Undo/mixed回滚与原N558仍
OPEN；正常V3/V8继续fenced。原全部交付范围保留，未提交、推送、PR、发布或
提升/启动唯一V325预览；用户真实数据不变。下一接线详见任务根next-protocol§17。

## S230（2026-10-15，去重切片回归通过，未交付）

当前 worker 的 fenced signed 路径已接独立 Deduplicated terminal producer，
而不是将“复用旧记录”伪装成新发布。结果绑定原 allocation、真实旧 import/
revision、实际校验的旧 receipt/revision/selection、entry identity 和最终 ACK item。
预留新 revision 保持未使用，不创建新回执/归档/native 项目，也不继承 created
所有权。Published 与 Deduplicated 两条 terminal 分支互斥，缺失/冲突不修复。

preflight 验真实旧归档、原 receipt/provenance/selection，并在同 snapshot 重查
semantic duplicate，避免先查去重再捕获 SQL 之间的改写。writer 无归档/native/
key I/O，首写前消费具体事务认证，前后核原 SQL 与当前接受事实。ACK、显式重导入
清 tombstone/恢复 hidden/current、原自动标题 CAS 和预制 result 同 COMMIT；别名、
用户标题和非 explicit 的较新 current 不覆盖。原结果重放只读/不再 setter ACK；
已完成 ACK 而结果缺失或残留完成字段不得被当作新条目补签。

正常 V3/V8 仍 fenced。旧 revision 属于 signed job 时明确拒绝 Legacy 授权，
需接独立 durable outcome reader 后才能支持。真正正常 source→plan→enqueue→
worker 正控、durable recovery/Undo/mixed rollback 与原 N558 仍未闭环；本批不
制造 test-only grant 或称整个 signed dedup 已验收。历史 input2/3、receipt2、
proof1/2 bytes/capacity 与 Legacy 行为保留。

N608 fmt/compile/51专项通过。N609 53pass/1fixture-setup failure：新增纯标题
投影夹具改 provider 后调用 identity-key upsert，撞原 import 主键，未进入断言。
改为正确的隔离行初始化，不修改原断言或产品 SQL。N610 原1561 exit0，fmt/
compile/54专项通过（12项新增 synthetic archive/result/ACK primitives），0failed。
N611 原55993 exit0，26非零filter392pass/0fail/2既有ignored及strict all-target
Clippy通过。随后只改两个 dedup native 路径：补接受时 previous current 的 CAS，
防止 preflight 捕获已在 acceptance 后推进的 current；它仍允许真正接受较新
current 后复用非 current 旧 revision。N611 的392不是最终树重测，不能冒认。
最终 N612 原14829 exit0，fmt/compile/55专项通过（共13项新增primitive）；N613
原98700 exit0，6非零filter112pass/0fail/1既有ignored，strict all-target Clippy
通过。N614 原66954 exit0，6非零filter显式--ignored实际执行旧seed65pass/0fail/
0ignored，含发布/crash恢复/final ACK/Undo/refresh/continuation/fault。112已含55，
65单列；不叠加中间392。没有 full-native/desktop/平台/交付验收通过的声明。

N611/N613/N614 各独立fresh-closeout.json：814native/2556frontend/26seed/Git/
index/EXE稳定，135产品文本UTF8-noBOM-LF、40quality通过，owners退出，App13244/
大文件80不增长。最终EXE SHA256为
ca3d5b1232e0f3fb43e89fa2dcb85078ea55481c259370755e08dd06100fe489。
这不是前端重测。成功 N608/610/612 own空temp各inventory→独立Execute删除：
0logicalB，观察H free delta各-4096B。N611/N613各own169files/1924922logicalB
删除，观察H free各+2174976B，不能视为独占物理回收；N614空temp同样两步删除
0logicalB/delta0B。再生为相同源码重跑对应gate；精确target/hash/owner/再生指令
保存在各run cleanup JSON。N609失败日志及空0B temp保留；它的成功门cleanup
请求被guard拒绝，未删除。N610早期inventory在terminal未出生时也被拒绝，原owner
退出且门通过后才完成正式inventory/Execute。源码/WIP/依赖/seed/当前EXE/符号/
唯一V325保留，没有启动App、改真实用户数据或commit/push/PR/release。

文档终稿后独立复核814native哈希/HEAD/index/当前EXE不变，135产品+4外部文本
共139路径LF/noBOM、40quality通过，task owners=0。固定入口plain cmd -CheckOnly
通过，V325 hash有效、launched=false；不是本轮新桌面验收。C/H空闲分别约
415.28GB/177.52GB（十进制，非本轮独占回收量）。原goal active，未整体交付。

以下 S229 及更早文字为对应历史输入的证据，不代表 S230 的完整当前验收。

S229（2026-10-15，阶段回归通过，未交付）：真实 publisher 已接原 allocation IDs、
确定性 phase receipt、实际 native disposition、linked receipt/store-result 同事务、
final receipt/outcome/catalog/mapping/worker ACK 同事务。最终 envelope 在首次 ACK
写入前构造；写后重新核 current acceptance/source/proof/fence、完整 phase 前缀、
实际保存的 ACK slot/item/phase/version 和 catalog/revision。publisher 重入只读原结果，
不从缺失的 linked/committed 行补签；独立 durable recovery 仍待。Legacy receipt2
bytes、随机追加回执及原错误校验
顺序保留，不新增普通 receipt 容量限制。

native Target/Provenance/Store 错误已区分：只有明确允许的目标解析失败可以落到
unassigned-reported；坏 origin 或实际存储写失败不能被吞掉。Create 解析到已有
同目录不同拼写时，独立 MatchedAlias 结果保存 actual/target 两个 raw path digest，
不得重写旧项目或冒认 created。该 codec 是未开放 V3 私有 metadata 扩展，不改历史
input2/3、receipt2、proof1/2。结果还必须与 receipt 的项目身份/路径/所有权字段一致。

正常 V3/V8 仍 fenced；signed dedup 尚未接，不得声称新发布链已整体可用。旧去重
行为保留，新路径在真实去重结果 producer 完成前明确拒绝，不能伪造新 revision。
本批新增 synthetic receipt/native/result/ACK primitive 负控不制造 source/proof grant，
不能作为正常注册 producer 正控、恢复/Undo 修复或交付证据。原 N558 保持 OPEN。
N601 只验证早期源码的 33 项；N602 验证中间源码的 40 项。N603 重建及42专项
通过；N604 常规366pass/0fail/2既有ignored，但 strict Clippy 失败：native 接口
仍 cfg(test) 导出，正常 lib 无法引用；另有 manual_inspect 告警。已真实修正导出
和 inspect_err，且移除已接线 outcome/native 模块的旧 dead-code 豁免，不加 suppression。
原失败证据保留。N605（原40272 exit0）修正后 fmt/compile/42专项通过；N606
（原60471 exit0）23非零常规filter共366pass/0fail/2既有ignored，strict all-target
Clippy55.7683866s通过。N607（原61368 exit0）6非零filter显式--ignored实际执行
旧seed65pass/0fail/0ignored，含发布/crash恢复/final ACK/Undo/refresh/continuation/
fault；26seed稳定。366已含42，不叠加，65单列。source/Git/index/EXE稳定，
fullNativeGate/finalAcceptance=false。当前EXE SHA256为
3b61987b29b3f70e4375711538544f003bfd2647498ef7fbaf07cdfcb773cb63。

fresh（原9027 exit0）：N606/N607各812native/2556frontend/26seed/Git/index/EXE
稳定，132文本UTF8-noBOM-LF、40quality通过，owners退出、App13244/大文件80不增长。
独立记录见各run fresh-closeout.json，不是前端重测、实机或完整native/最终验收。
N601/602/603/605 own空temp各inventory→独立Execute删除（各0logicalB、观察H delta
各-4096B）；N606 own169files/1924922logicalB删除，观察H free+2174976B，非独占
回收；N607 own空temp删除（0logicalB/delta0B）。精确target/hash/owner/再生方法见
各cleanup JSON。N604失败资料及其compiler-temp保留；不重试N528/V224清理，不删
源码/WIP/依赖/seed/当前EXE/符号或V325。固定预览CheckOnly通过，无App启动/提升。
无commit/push/PR/release；goal active，原全部交付范围保留。

以下 S228 及更早为历史记录；未启动 App、升级唯一 V325 预览或发布远端。

S228（2026-10-15，阶段回归通过）：allocation已移到接受作业的同一事务，
不是worker发现缺行时重新分配。新member必填allocationDigest，完整member root
绑定原input与canonical allocation；历史input2/3、receipt2、proof1/2不改。
preflight捕获catalog目标/previous和Create/Match/Unassigned意图；先退出读snapshot
再读取native项目和加载crypto。writer纯SQL重核目标，插入后重新读全部allocation，
已有acceptance缺allocation明确拒绝，不修复。精确producer对象可无写重试；另行
capture的新UUID不能替换原接受事实。具体Transaction-bound allocated reader
只读原分配，不从mutable receipt/JobItem推导。它不是source/SQL/native授权。

本批新增7项合成primitive测试；N596首次编译因兼容路径PreparedScope initializer
漏填新protocol字段失败，证据保留。N597编译通过，专项30pass/1fail：新trigger
测试按显示index猜插入顺序，实际是candidate digest排序，删除了尚不存在的行。
修正为真正最后插入时删除已存在的前一行；不改产品校验或预期。
N598原15622 exit0，fmt/compile及31接受/分配专项通过。N599原52921 exit0：
23个非零常规filter共357pass/0fail/2既有ignored，strict all-target Clippy
53.6078043s通过。N600原90171 exit0：6个非零filter显式--ignored实际执行65项
旧seed发布/crash恢复/final ACK及Undo/refresh/continuation/fault，65pass/0fail/
0ignored，26seed文件稳定。357已含31，不叠加；65旧seed专项单列。两个run的
source/Git/index/EXE稳定，fullNativeGate/finalAcceptance=false。当前测试EXE为
0f8f00eb9caca1ccbf3789d729c5e9687bee5b61f4d736d65373a7910e24b70a。

fresh原68150 exit0：N599/N600各808native/2556frontend/26seed/Git/index/EXE
稳定，128文本UTF8-noBOM-LF、40quality通过，owners退出、App13244不增长。
这是未变前端重新绑定，不是前端重测、桌面或完整native/最终验收。
N598 own空temp经inventory→独立Execute删除（0logicalB/观察H free delta-4096B）；
N599 own169files/1924922logicalB删除，观察H free+2174976B，非独占回收；
N600 own空temp同样删除（0logicalB/delta0B）。精确target/owner/hash/再生方法在
各run cleanup JSON；日志/EXE/符号/源码/WIP/依赖/seed/V325保留。N596/N597失败
资料及其空temp（各0logicalB）暂留。V325 CheckOnly通过，无运行preview。

原N558仍OPEN；真实native/store-result/final/recovery/Undo和正常V3/V8未接通，
fence保留，原全部交付范围不缩减。未启动App、升级V325或提交远端。
下一发布切片还必须显式覆盖去重返回旧revision的结果，不能强制消费预留新revision
或改变旧去重行为；具体函数接缝与顺序在任务根next-protocol§14。

以下S227及更早记录为历史证据，不能当本批改动后的当前源码已通过。

S227 历史（2026-10-15，阶段回归通过）：requestId幂等查找、durable
input/committed恢复及retention/reconciliation已接显式事务前crypto。准备只选择
依赖，不携SQL授权；真正snapshot重新核owner及当前input/context。新出现的owner
不得当unowned回收；坏owner只使其自身preview延期。Legacy不增加key加载或
完整成员scan；新点读仍核完整membership，只hash选中的原input。

N587缺分号编译失败已修；N588专项通过后N589的350项回归通过，但Clippy发现
测试专用wrapper及无谓clone问题，均真实修正、未加lint suppression。N590重建
及24接受专项通过；N591的350pass/0fail/2既有ignored和strict Clippy通过。
随后N592显式旧seed专项实际53pass/4fail，余filter未执行：暴露旧publisher
测试wrapper在writer内错误prepare，以及恢复在schema校验前先报禁止archive I/O。
保持原预期修正，新增常规future-receipt错误顺序断言；N593 fmt/compile及24
接受专项通过。上述旧绑定不冒认修正后的源码已通过。

N594原22834 exit0：6个非零filter实际65pass/0fail/0ignored，明确--ignored执行
旧seed publication/crash recovery/final ACK、Undo/refresh/continuation/fault专项。
26文件seed前后hash稳定，source/Git/index/EXE稳定；不是full native或最终验收。
N595原44951 exit0：相同源码的23个非零常规filter共350pass/0fail/2既有ignored，
strict all-target Clippy55.50s通过；source/Git/index/EXE稳定，fullNativeGate/
finalAcceptance=false。350已含24接受专项，不与其叠加；65项旧seed专项单列。
当前测试EXE：
85d27df36a4b802fd53ff0b86c3bd9f719016717db827f058ff344da46e052f5。

N588/N590/N593 own空temp均经inventory→独立Execute清理（各0logicalB/观察H
delta-4096B）；N591 own169files/1924922logicalB同样清理，观察H free+2174976B，
非独占回收。N594 own空temp清理（0logicalB/观察delta0B）。精确targets/hash/
owner/再生说明保存在cleanup记录；N587/N589/N592原失败资料保留。当前EXE、
符号、源码/WIP、依赖、seed和唯一V325预览保留；N595 own169files/1924922logicalB
经同样两步清理，观察H free+2174976B，非独占回收。V325固定入口CheckOnly通过，
无运行预览。本轮未启动App、提升预览或commit/push/PR/release。
fresh原14990 exit0：N594/N595各805native/2556frontend/26seed及Git/index/EXE
稳定，124文本UTF8-noBOM-LF和40quality通过，owners退出、App13244未增长。
独立终态在各run fresh-closeout.json；只是重新绑定未变前端，不是前端重测、
完整native/实机/全平台或最终交付验收。

原N558项目记录误删RED、真实allocation/store-result/final/recovery/Undo及正常
V3/V8 producer仍OPEN/fenced。点读/分页/千条metadata成本尚未测完，不声称消除
所有平方工作。下一顺序见任务根next-protocol§13；原全部容量/控制/B3–B6/UI/
15locale/原厂-WAL/ACP/UNKNOWN/三平台/IME-DPI/小PR范围不缩减。

以下S226及更早记录为历史证据，不代替当前源码或完整验收。

S226（2026-10-15）：operation-local JobCrypto和只读/具体Transaction-bound
writer证据分离已接entry、状态、取消、worker发布owner/final ACK、Undo和分页读。
crypto只持job ID和已加载auth，不是SQL grant；writer在首次写入前核精确事务/
connection/job/total_changes，发生修改或savepoint回滚后旧证据不能复用。
Legacy不新增key加载；缺job保留not-found；正常v3/v8 producer仍fenced。
requestId snapshot、durable input和retention/recovery的完整显式依赖尚未接齐，
真实allocation/store-result/final/recovery/Undo及原N558误删RED仍OPEN。

N583 fmt/compile和23接受专项通过；N584扩大回归发现测试专用publisher wrapper
提前prepare导致无表时错误顺序变化，保留失败证据并恢复原协议preflight，未改
FormatUnsupported预期。N585重新fmt/compile及19协议/crypto专项通过。N586原
句柄87718 exit0：19非零filter共333pass/0fail/2既有ignored，strict all-target
Clippy55.21s通过；333已含6项新crypto与23接受专项，重叠不累计。source/Git/
index/EXE稳定，fullNativeGate/finalAcceptance=false。当前测试EXE SHA256为
33d6f06ab1eeb2e4bbdfe6c9e343d64b60fff96d1521293740ee96eef6c5e3c9。

N585 own空temp经inventory→独立Execute清理，0logicalB，观察H free delta
-4096B；N586 own169files/1924922logicalB按相同步骤清理，观察空闲增加2174976B，
不称独占回收。精确路径/hash/owner及再生方法保存在各run cleanup记录。N584
失败资料/临时数据保留；N583旧EXE绑定已被重链，未绕过绑定做清理。保留当前
EXE/符号/源码/WIP/依赖/seed/V325；未启动App、提升预览、commit/push/PR/release。
V325固定入口CheckOnly通过。下一接线顺序见任务根next-protocol§12；原全部
容量/控制/UI/locale/原厂WAL/ACP/三平台/IME-DPI/小PR范围不缩减。
fresh原63860 exit0：797native/2556frontend/26seed及Git/index/EXE稳定；115文本
UTF8-noBOM-LF、40quality通过，owners退出，App13244不增长。独立记录在N586的
fresh-closeout.json；不是新一轮前端测试、实机、全平台或最终验收。

以下S224/S225及更早记录为历史证据，不是当前完整验收或当前EXE绑定。

最新 S224/S225（2026-10-15）：native原子创建出处已实施，且outcome binding已直接
接到当前verified acceptance header/member；正常v3/v8 producer、完整发布/恢复/Undo
仍未开放，N558原误删RED仍待闭环。不是已完成会话导入交付。

native签封只接受存储Snapshot checked-new分支的opaque NewImportProject，不能
给任意existing Project补签。项目与出处同一次atomic replacement；同ID无出处、
同路径/规范别名和另一合法producer只match，不认created或改写。精确出处才恢复
created归属；remove保留用户全部typed定制、unknown字段和workspace/session引用，
只移除项目记录、不删目录。旧writer保留新字段，旧remove仍按未知字段保护。
真实publisher后续必须先核完整accepted/source/member/fence/committed链和SQL引用；
当前native API不是catalog授权，未接正常pipeline。

接受证据绑定原Transaction/total_changes以及同一个预加载auth引用。结果Binding
只取verified原header/member，acceptanceDigest固定为canonical signed member
envelope的SHA256（其headerDigest追溯完整scope/member集合），不取mutable结果。
该Binding只是codec身份、不是SQL grant。测试涵盖精确字段、不同candidate、可变
结果不改变原Binding、canonical空白、换事务/修改原input拒绝和loader失效。

N578 fmt/compile及32专项通过；capability/typed定制/正常session引用和原home
key-loader阻断下store全流程加强后，N579 fmt/compile及33专项通过。N580的17
非零filter共282pass/0fail/2既有ignored，但strict Clippy因非测试构建unused
re-export失败；保留原记录，只把测试alias限定cfg(test)，没有加入全局suppression。
随后N581 fmt/compile及21接受专项通过（旧19加2，不累计）。N582原23300已exit0：
17非零filter共284pass/0fail/2既有ignored，strict all-target Clippy57.85s通过；
284已含33和21，不叠加。source/Git/index/EXE稳定；不是full native或最终验收。
当前测试EXE SHA256为a2ef33348463bf3a63499f9b6612e4d064f98f42de714a75763b0c3a1ff18024。
App/AppWorkbench未增长，无新依赖；fresh/清理以本轮独立记录为准。

fresh原32904已exit0：795native/2556frontend/26seed及Git/index/EXE稳定，113文本
UTF8-noBOM-LF和40quality通过，owners退出、App13244不增长；V325 CheckOnly通过。
N578/N579/N581 own空temp各经inventory→独立Execute清理，0logicalB，每次观察H
free delta -4096B。N582 own169files/1924922logicalB经相同步骤清理，观察H空闲
增量2174976B（约2.07MiB，不称独占回收）。精确目标/owner/hash/再生方法保存在
各run cleanup记录。N580失败run的169files/1924922logicalB暂留待逐项分类；
旧失败证据、当前EXE/符号/依赖/seed/V325保留，不删共享target、不终止他人进程。

下一步实施顺序详见任务根next-protocol§11：writer前crypto显式贯穿read/state/
publish/Undo；真实allocation先耐久再消费UUID；store-result/final/恢复/Undo；
原N558和正常新链正负控通过后才开放producer。原容量/控制/B3–B6/UI/15locale/
原厂WAL/ACP/三平台/IME-DPI/小PR全部范围仍保留。上游与fork main的本地集成已
完成，不重复；唯一V325预览不变。未启动App、commit/push/PR/release。

以下S223及更早结果是历史证据，不代替当前完整门或当前EXE绑定。

最新 S223（2026-10-14）：signed acceptance 的固定元数据 codec、private 输入
捕获和同事务写入/读取基础已实现；N576 的 19 项专项通过。N577 的14个非零
filter共236passed/0failed/2既有ignored和strict all-target Clippy66.84s通过。
236包含19，不叠加；真实v3正常接受链仍未启用，N558仍开放。

header以独立HMAC域绑定job/context、原plan字节、scope字节、精确成员集合；
member以另一个域绑定header、candidate和完整原input字节。8KiB只约束新元数据，
旧input/body格式、bytes和容量不变。捕获先核真实plan codec、selection/snapshot/
mapping和全部输入集合，再在writer前load opaque crypto；旧路径不增加该load或
全文扫描。scope writer在原scope/accepted marker写完之后录入新记录。

新记录insert-only或byte-identical重试；writer在同事务核当前job初始queued/
version1、原plan/scope/marker/全部input，并在全部新行写完后重读完整元数据集合
和逐项原字节hash。reader不加载key、不读source/preview/archive/native store，
只对请求candidate流式hash原input。借用具体Transaction并核total_changes，不能
作为跨事务SQL grant；原source/proof/fence/target权限仍须保留。

专项证据明确是synthetic producer primitive/SQL事务测试：包含原字段篡改、
跨job/header/member复制、missing/extra/重复成员、不可修复冲突、late trigger整笔
回滚、超过16MiB的原输入、阻断key loader后的预加载crypto writer，以及运行中
job可读但不可重新签发。synthetic scope不是source授权，不当作真实注册正控。
正常v3/v8 producer和公开association入口仍关闭，不宣称N558或交付已完成。

下一步仍按任务根next-protocol计划§10：显式writer前crypto贯穿entry/read/state/
publish/Undo，再接真实allocation、native原子创建出处、final/recovery/Undo；
完整正常链正负控和N558原RED通过后才开放新producer。原完整验收范围保持。
已批准的本地checkpoint和两条main集成已完成，不重复；唯一预览仍V325。

N574原41514 exit1，fmt/compile通过、5pass/11fail是fixture lifetime超过现有
30分钟policy；保留失败证据，只修fixture，未放宽产品检查。N575原71772 exit0，
fmt/compile和16项通过。N576原36464 exit0，fmt/compile和19项9.75s通过。
N577原76410 exit0，14定向全部非零、236pass/0fail/2ignored、strict Clippy通过。
当前EXE SHA256为5a80eb0bbaa8a5fe877ce5be943282feda773085d538298956bf390927eb3753；
source/Git/index/EXE稳定，fullNativeGate/finalAcceptance=false。fresh/cleanup证据
在本轮各run中记录；未启动App、升级预览或commit/push/PR/release。
fresh原93753已exit0：792native/2556frontend/26seed/Git/index/EXE稳定、107文本
UTF8-noBOM-LF和40quality通过，owners退出，App13244未增长；V325 CheckOnly通过。
N576 own空temp经inventory→独立Execute清理，0logicalB/观察H free delta-4096B；
N577 own temp169files/1924922logicalB经相同步骤清理，观察H free delta+2174976B，
不称独占回收量。精确目标/owner/hash/再生方法在cleanup记录。N574失败与N575旧
EXE绑定的empty temp保留待后续完整replacement gate（各0文件），失败资料/当前
EXE/符号/依赖/seed/唯一V325保留，未删除共享target或其他聊天产物。

以下S222及更早内容为历史切片证据，不能代替当前完整门。

最新 S222：协议绑定的作业ID和新路径拒绝已接到读取/发布/控制/兼容入口；
217项相关定向回归及strict Clippy通过。正常新producer、恢复/Undo接线及N558
仍开放（2026-10-14），不是完整交付或当前full native门。

旧正常producer继续UUIDv4；新SignedOutcomeV1预留系统随机canonical UUIDv8，
由原plan协议选择，原v3 require_publisher仍关闭。ID仅选择校验路线，不提供
输入接受/结果归属/SQL grant。既有job按主键确认存在后，在读取正文/scope/
old_owner前分流；v8缺全部新事实和原plan也不能进入legacy，未来UUID版本及
v8非canonical别名拒绝。历史opaque ID和旧v4原校验、not-found语义保留。
enqueue/publish在preview/store/archive I/O前拒绝未实现新协议；compatibility
不为无scope的v8/未来job补标签。原plan/ID错配拒绝，无caller/test许可开关。
这些是拒绝和旧路径正控证据，尚非真正新接受链；协调改主键/结果的完整攻击
仍需后续MAC acceptance/native创建出处/committed结果验证，N558原RED未修。

N570原96185已exit1，fmt通过，compile因测试访问私有module失败，0断言；
测试移回模块内部，未扩大生产可见性。N571原47032已exit1，fmt/compile通过，
19项17passed/2failed：无I/O测试误判fixture初始化的空目录。保留失败证据，
改为操作前后精确文件/目录digest快照相等，未放宽无写预期。N572原54974已
exit0，fmt/compile及19passed/0failed1.82s。N573原12652已exit0：13非零
定向217passed/0failed/2既有ignored，strict all-target Clippy63.26s通过。
217已含19，不叠加。当前784native inputs，EXE
e7e88257edcb3e673be94381c5d9b71b336b4a542ff9374ae24fffed3208a3a5；source/Git/
index/EXE稳定，fullNativeGate/finalAcceptance=false，旧N557不当当前完整门。
本轮fresh/严格cleanup的终态另见N573记录；V325 CheckOnly通过，未启动App。
fresh原36418已exit0：784native/2556frontend/26seed/Git/index/EXE稳定、99文本
UTF8-noBOM-LF/40quality通过，owners退出，App13244未增长。N572自身空temp已
inventory→独立Execute移除，0logicalB/观察H free delta-4096B；N573自身temp
169files/1924922logicalB已同样清理，观察H free delta+2174976B，非独占回收量。
旧失败资料/共享EXE/符号/依赖/seed/V325保留，不删除shared target或其他对话产物。

下一按next-protocol计划§9接private DurableInput的signed acceptance producer，
并显式贯穿writer前加载crypto；entry writer里不调用load，也不用secret cache
冒称零文件I/O。随后耐久allocation、native原子创建出处、final/recovery/Undo，
全部接线和正常链正负控完成才启用v3/v8。原全部容量/控制/B3–B6/UI/15locale/
原厂-WAL/ACP/三平台/IME-DPI/小PR范围开放。唯一V325不变，无新包/commit/
push/PR/release，不重复已完成checkpoint/main集成。

下面S221是已完成基础切片的历史证据；本节分流拒绝不代替真实接受/结果授权。

最新 S221：独立三阶段签封结果 codec 和点日志已实现，定向兼容绿色；真实 producer/恢复/Undo 接线仍未完成，N558仍开放（2026-10-14）。

新增独立 outcome v1 envelope/binding/allocated→stored→committed，精确绑定原接受
摘要与分配/真实项目结果/linked和最终回执/JobItem摘要。固定元数据8KiB限制不施加
给普通 input/receipt；旧plan1/2/input2/3/receipt2/proof1/2 bytes/digest不变。
每阶段独立HMAC域；私有seal未暴露给reader/Undo。点日志同Transaction按主键读，
先核存储type/字节长度，不在writer加载key或读取source/preview/archive/native store。
行只能首次插入或byte-identical重试，不可覆盖/补签；stored/committed插入和完整
reader均核精确前序链。删行/复制/未来版本/损坏拒绝只证明该codec日志的行为，
不是整个产品已具备抗缺失降级分流。match保留旧native非UUID ID和获准fallback。

N567原34573 exit0：fmt/compile+15passed。审阅扩完整链写入/读取、固定canonical
fixture及负面形状；N568原62390 exit0：fmt/compile+19passed/0failed0.89s。
N569原91744 exit0：10非零定向165passed/0failed/1既有ignored、strict all-target
Clippy60.38s通过。165已含19新测试，不与N568相加。当前782native inputs，EXE
384aa73c6c9b44a272497ffebd5d67fc000e62599712148e4e01a20c1c03a9e8；source/Git/
EXE稳定，fullNativeGate/finalAcceptance=false，不冒认旧N557完整门。
N567/N568/N569自身空temp inventory→独立Execute已清，各0logicalB；观察H free
delta分别-4096/-4096/+4096B，不称独占回收。共享EXE/符号/依赖/seed/失败资料保留。
后续fresh精确绑定记录于N569/fresh-closeout.json；V325 CheckOnly通过，未启动App。
该fresh原22941已exit0：782native/2556frontend/26immutable seed/Git/index/EXE
稳定，97文本UTF8-noBOM-LF、40quality通过，owners退出且App13244未增长。

仍无正常accepted signed outcome producer；新plan producer仍1/2，v3和public
association guard保持关闭。真实store创建后/linked前崩溃不能从同UUID猜created。
下一按任务根next-protocol计划§8接真实acceptance、防降级、allocation和native
原子创建出处，再接final/recovery/Undo并使N558原RED变绿。原全部容量/控制/
B3–B6/UI/15locale/原厂-WAL/ACP/三平台/IME-DPI/小PR范围开放。唯一预览仍V325；
无新包/预览提升/commit/push/PR/release，不重复已完成checkpoint/main集成。

以下S220为前一基础切片，不代表新的journal尚不存在；它的producer未接线状态仍成立。

最新 S220：plan版本分流与opaque HMAC基础已实现，定向兼容绿色；结果journal/producer/恢复/Undo未接线，N558待修复（2026-10-14）。

plan3必填publicationOutcome.version1并把协议/planId/生命周期纳入canonical hash。
旧plan1/2 bytes/digest不加wrapper，不改input2/3、receipt2、proof1/2；普通旧计划
不增加全文扫描或32KiB/16MiB输入cap。实际plan producer仍发1/2；public/internal
commit与scope/compatibility捕获拒绝3，public association guard保留，DTO不变。
现有HMAC wrapper真实委托opaque InstallationAuthenticator，MAC算法/域/字节不变。
load保留现有secret/cache文件I/O，加载后sign/verify纯crypto，无SQL授权事实。
合成home真实writer内移动精确key、原路径建空目录并清loader cache后，验证仍成功，
路径不被访问/重建且SQL不变。没有账号、模型、私人资料访问或key/MAC输出。

N561原18635 exit0：fmt/compile+10passed。N562/N563错误module alias0match，被
inventory拒绝，0断言，不计产品结果。N564原73680 exit1：11cursor/28member/
21group-selection通过，原association codec11passed/1failed，dispatch3误接收
正文version2。保留原预期，生产parse_record补精确version一致性及新错配矩阵。
N565原67993 exit0：fmt/compile+11passed1.55s，BaselineOnly非完整门。N566原31639
exit0：9定向146passed/0failed/1ignored和strict all-target Clippy通过；146已含
11新测试，不与N565重复相加，control50实际passed/1既有ignored未冒算执行。
当前776native、EXE360ea27a62c07f246f7183b7672dc1b069f9076e253967705655326625155b5c；
source/Git/EXE稳定，fullNativeGate/finalAcceptance=false，不冒认N557原完整绿。
N561/N565/N566 own空compiler-temp均inventory→Execute清理，0logicalB；观察H
free delta分别-4096/-4096/+4096B，非独占回收量。当前EXE/符号/依赖/seed/日志保留，
N558/N562–N564失败材料defer。独立fresh收尾记录于N566的fresh-closeout.json。
该fresh已exit0：776native/2556frontend/26seed与Git/index/EXE稳定，91WIP文本
UTF8-noBOM-LF、40quality通过，owners退出；App13244未增长。V325 CheckOnly通过，未启动。
新结果协议只有上述基础，不是accepted outcome journal或已签项目结果，N558未修好。
任务根next-protocol计划§7记录真实后续接缝；原全部范围开放，无App/新包/预览
提升/commit/push/PR/release，唯一V325不变，已完成的checkpoint/main集成不重复。

下面S219及更早状态为历史；其“协议未实施”只由S220上述有限基础进展替代。

最新 S219：N557完整原生门已通过；N558确认新的create=true项目归属RED，仍未整体交付（2026-10-14）。

N557原27255已exit0：全库4078passed/0failed/89ignored，1117.52s；historical
seed72、retention/recovery44和strict all-target Clippy通过，fullNativeGate=true。
fresh核772native/2556frontend/26seed/85文本UTF8-noBOM-LF/40quality、Git/index/
EXE稳定且owners退出。这是N555 EXE78fc4b65...的历史完整绿色，不是当前EXE验收。
N557自身380编译临时文件移除：5246233logicalB、observedHFreeDelta5844992B；
N555空临时目录也已按精确Focus映射清理（0logicalB，观察free delta -4096B为盘活动）。

N558原80296已exit1，fmt/compile通过、ownership专项8passed/1failed22.35s。
正常accepted createProject=true→worker创建own项目后，正常store另建无引用foreign
项目；仅协调latest receipt与mutable item的项目ID/精确项目快照，公开Undo返回undone
并误删foreign项目记录。不是目录或归档删除；own项目和两个目录marker/archives保留。
新genuine same-path match及真实许可unassigned-reported fallback正控均通过。
未改失败预期、未恢复被删对象来伪造原子性，也未用永久禁用正常Undo凑绿色。
当前773native、EXE198345849c4403386501dd2dd902d3ff2d206f20742b91781b11ce6efec5a918。

N559原入口误把ignored正控交给不带--ignored的FocusedFilters：0passed/1ignored，
脚手架非零断言门正确拒绝；不是产品验收。N560原82355已exit0，7published-auth
实际passed及strict Clippy通过。其fresh-closeout核773native/2556frontend/26seed/
86WIP文本/40quality、Git/index/EXE稳定、owners退出；passing-subset自身空compiler-temp
inventory→Execute移除，0logicalB/观察free delta0B。N558/N559失败证据保留。

下一按任务根plans/publication-outcome-protocol-next.md实施独立durable outcome协议：
accepted意图≠allocation≠store实际结果≠committed结果；新协议由原plan/hash及接受事实
约束，不能删journal/marker或降级字段后自动走legacy。真实producer发签封、store前耐久
分配、恢复复用原UUID、Undo同snapshot核完整结果，不在reader/Undo补签可变结果。
旧plan1/2、input2/3、receipt2、proof1/2 bytes/digest和普通容量边界保持。
本节未声称该协议已实施或N558已修复。原全部B3–B6/容量/控制协议/UI/15locale/真实
原厂-WAL/ACP/三平台/IME-DPI/小PR范围继续；public guard和唯一V325预览不变。
无App启动/新包/预览提升/commit/push/PR/release；已完成的checkpoint/main集成不重复。

下面S218为前一轮过程记录，其中N557 RUNNING已由S219上述terminal结果替代。

最新 S218：两处真实Undo缺陷已窄修复，5项兼容回归及相关定向复验通过；当前全库复验中，仍未整体交付（2026-10-14）。

N549真实RED：accepted createProject=false，却可凭损坏的createdProject回执
让公开Undo返回undone并移除独立合成项目记录。修复在同writer使用已认证
mapping限制创建权限，并比较item/receipt项目ID。N551另有真实普通Undo RED：
只将mutable item改指另一正常发布entry/revision，公开Undo仍成功并改catalog。
新增共享目标边界核job/candidate/item-receipt IDs、entry/source identity、
revision snapshot及previous必须属于同import，在实际target写入前执行。
保留missing-receipt/later-revision conflict、dedup/tombstone既有行为；不增加
普通记录大小上限、不改public DTO/schema，不在writer做source/preview/archive/
native-store I/O，不输出凭据或触碰真实资料。

当前N553原64292 fmt/compile及6passed/0failed（19.759s）。包括四种协调关联
结果、N8四项目fault与真实create正例、普通在前的mixed原子拒绝、真实普通
durable-v2 changed-body refresh Undo/missing receipt及三个foreign负面fault。
所有负面fault提交后才调用公开Undo，foreign对象均真实domain创建；raw catalog、
archive hashes/native stores按边界核对。N548和N551的reader/已GC preview脚手架
错误及N552 E0609编译错误原日志保留；missing凭据后的reader不假称健康。

N554原65916已exit1：全库4073passed/5failed/89ignored（1118.09s），
historical seed72、retention recovery44和strict Clippy通过，但fullNativeGate=false。
5项失败均是共享Undo边界直接比较旧receipt与已adoption entry身份所致，
不是忽略失败或更改预期。已沿用现有registration_provenance::authorize精确
accepted bridge，保留目标/revision/previous校验，不凭parsed proof或UUID授权。
N555原86621已exit0：fmt/compile和6项专项通过（20.129s）；BaselineOnly的
terminal.passed=false，不冒充完整门。772 native inputs，当前EXE78fc4b65...。
N556原13636已exit0：5项原失败、control50/jobs31/published-auth7均通过，
strict Clippy60.529s/exit0；source/Git/EXE稳定，但不是fullNativeGate。
本轮169个临时编译文件按inventory→Execute移除，1924922logicalB，
observedHFreeDelta2174976B；原证据/EXE/依赖/seed保留，V325 CheckOnly通过。
N557原27255正在同当前EXE执行seed-bound 12命令FullNative/RequireGreen；
当前全库仍须terminal确认，不能把旧N554或定向绿色当作当前full green。
N555专项临时容器因现有cleaner未映射该Focus暂留，不绕过。N8这里只关闭
createProject=false的误授权，create=true生成ID的foreign项目归属、真实match/
fallback及普通全协调项目/same-import结果仍开放，不把item/receipt自洽当权限。
原万条取消/stop/吞吐、public accepted桥、B6 UI/15语言/原53/三平台/真实ACP等
原范围继续，public guard保留。没有App/新包/预览升级或commit/push/PR/release，
唯一用户入口仍V325；checkpoint/两main集成已完成，不重复。精确结果与下一
执行点见任务根plans/b5-undo-ownership-baseline.md §10。

下面S217为前一轮G2/G3历史证据，不是当前EXE的完整库验收。

最新 S217：真实关联 Undo 的固定续聊与后续正文刷新验证通过，仍未整体交付（2026-10-14）。

本轮只增加隔离合成测试和任务harness过滤入口，未再改产品行为、公开协议或UI。
G2经真正continue_prepare/create分别建立旧prefix和新窄选择的两个native会话。
关联发布、公开Undo和runtime reset后，ref各列/JSON bytes、sidecar bytes/hash、
selection hash、handoff bytes/hash及native row不变。原grant与重新解析的grant
均经本地MCP读取实际选中事件，返回revision/citation保持各自固定，实际未选
event ID明确拒绝、未选正文搜索为空；不输出/序列化grant token，不调用模型。
独立旧draft在真正current移动后create返回HISTORY_ACCESS_UNAVAILABLE，
catalog逐rowid/type/bytes不变，0session/ref/intent；允许既有advisory lock-only
目录，不把它算作已创建会话。仅两条真实合成消息，不冒称完整secret/assets矩阵。
G3先真正公开Undo，再通过正常source注册API把隔离catalog的primary移到新合成
副本，只修改该副本正文。registered scan→preview→普通input2 commit→worker
完成：同canonical import、共3revision、changed-body非dedup，全部旧archive与
receipts/proof保留。重启/同request retry无重复，旧association Undo不覆盖最新，
原仍committed creator明确conflict/later revision kept；未改seed/source或SQL root。

N543原92883因新测试误用JSON阅读结果为内部event产生4个E0599，未进入断言；
修正测试后N544原44560 G2两项通过。当前N545原25814 G3一项通过1.642s，
N546原82200加强MCP返回revision/citation核对后的G2两项通过3.652s。
N547原48381当前EXE的58续聊创建/4scope/12MCP/7published-auth全部通过，
strict all-target Clippy exit0/59.281s，过滤inventory逐项非零；不是全库门。
19:14:27fresh768native/2556frontend/26seed/HEAD/index/EXEacb28906...一致，
5个原owner退出、task owners0；81WIP文本UTF8/noBOM/LF及原40quality通过，
App13244/85state/60effects/80大文件不增。N547自己的空compiler-temp按
inventory→Execute清理，0logicalB/0observedB；N543-N546空容器和N540失败
门的169文件暂留，不绕过清理资格。无App/新包/preview promotion，唯一V325。

下一B5仍要补完整N4/N5协调receipt+item替换、多项原子性、N8项目归属及普通
durable-v2真实refresh Undo正负对照；G3的later-revision conflict不是这些覆盖。
当前全库、原万条取消RED/吞吐、public accepted桥、B6 UI/15语言、原53/三平台/
真实ACP等全部原范围仍开放，public guard保留；未commit/push/PR/发布。
原始结果及精确续接点见任务根plans/b5-undo-ownership-baseline.md §9；
runs/v547-continuation-regression/g2-g3-batch-closeout.json记录fresh绑定与清理。

下面S216为前一轮Undo窄修复历史，不是当前EXE的全库验收。

最新 S216：真实关联 Undo 的错误版本回退已复现并窄修复，专项与相关回归通过，仍不可整体交付（2026-10-14）。

N534原46028真实RED：正常撤销通过；隔离副本的latest receipt.previousRevision
改成另一UUID后，公开Undo仍返回undone并装入foreign current。修复复用同一writer
的accepted成员、完整input/proof、bounded exact receipt及catalog revision归属，
先验证全部关联项再写入；不增加caller授权flag，不跨COMMIT缓存，不在writer
重新哈希归档。普通项仍走既有receipt读取顺序与missing-receipt conflict。
当前N538原77135专项5passed/0failed（15.337s），含正常撤销/重启/重复请求、
16种坏凭据、独立真实发布entry保护、缺失凭据明确冲突；两份immutable archive
及旧receipt/proof保留，重复Undo版本4→5→6但不新增revision。不是全部B5通过。
N539原17317旧关联57passed（129.007s）；N540原29510 control50/jobs31通过，
第三过滤路径漏engine匹配0条，被保护门拒绝，Clippy未执行，不能算整轮通过。
N541原1443纠正后的授权reader7passed、strict all-target Clippy exit0/58.999s。
任务harness增加当前EXE的filter inventory；N542真实负控制在执行任何断言前
拒绝0匹配。N535的模块路径编译错误及N536的测试快照非法UTF8 panic原证据保留；
后者发生在该fault的公开Undo调用前，改用type+raw bytes快照，未弱化产品断言。
18:44:59fresh766native/2556frontend/26seed/EXE10077d54.../HEAD/index稳定，
9个原owner退出、task build/test/App owners0；原40quality与App规模不增长。
N541自身空compiler-temp经inventory→Execute清理，0logicalB/0observedB；
N540的169文件/1924922logicalB因失败门绑定暂留，其他本轮旧temp为空，不绕过。
无App/包/预览promotion；唯一V325不变，未commit/push/PR，public关联guard保留。
下一B5 G2真实固定refs/grants、G3 changed-body刷新、完整协调反例/N8项目归属及
普通durable-v2对照；当前全库、原万条取消RED、public桥/B6/UI/平台/真实ACP/
原53等仍开放，不能把N520旧EXE全库绿灯或N531旧容量冒认为本轮成绩。
具体原始run和下一执行点见任务根plans/b5-undo-ownership-baseline.md §8。

下面S215为取消切片历史，不代表B5仍未执行。

最新 S215：万条取消原始门失败，冗余写入窄修复通过定向门，仍不可交付（2026-10-14）。

N528原20198已exit1/native101，total2171.800s；cancelReply790ms超过原<500ms，
workerStop19713ms。请求时2001committed、停止时2002；其receipt保留及reader检查
已执行，但失败在源离线/reset/retry之前，不是万条离线续传通过。原失败日志/
samples/terminal保留，source/Git稳定；精确原owner退出后fresh核764inputs与EXE。
原cancel复用save_job，给未改变的10000条job_items各发一次UPDATE。真实61条
prepare/plan/commit→public cancel的abort-trigger回归N529实际RED，修复只写jobs
头部后N530定向15passed/0failed/4ignored（16.444s），fmt/compile通过；包括
重复取消、原始inputs/phase保留、header写失败回滚、completed结果/receipt保留。
仍走同snapshot get_job认证及commit后通知，未改变公开协议/普通输入容量。
这是3-command focused gate，非新的全库绿灯；N520旧EXE不冒认新源码通过。
下一先N530当前EXE做千条取消诊断，再扩大回归/定位stop；790ms/19713ms尚未
复测关闭，不放宽阈值。万条已发布的N527原正例仍保留，不冒充新EXE成绩。
本地checkpoint/两main集成已完成，WIP保留；未push/PR/发布。唯一V325不升级，
B3-B6/UI/15语言/原53/平台/真实ACP等原范围仍开放。空runtime容器删除被tool
policy拒绝，0entries/0logicalB暂留，不绕过；原失败证据、seed、依赖/源码均保留。

续接结果：N531原28364诊断exit0，千条cancel76ms/stop1683ms/retry99ms，停止时
201committed、离线恢复余799；最终1000entry/revision/receipt/reader逐项与无重复
均通过，旧receipt bytes保留。Total291.716s，terminal29ms/catalog3ms；76ms与旧
79ms接近，不宣称显著提速或原万条门已关闭。N532的control50通过，但错误过滤名
jobs_tests匹配0条被非零门拒绝；N533纠正为jobs::tests::31通过、strict Clippy
exit0/61.772s，不重跑已绿50或掩盖0条。仍非新全库验收，N520旧EXE不冒认。
17:29fresh764native/EXE91854dc.../HEAD/index稳定、task owners0；77WIP文本均
UTF8/noBOM/LF、diff-check与原40quality门通过，App13244/85state/60effect/80大文件
不增。N531最终空runtime已清0logicalB/0observedB；N533inventory→Execute清169
编译临时文件1924922logicalB，观察H空闲增2174976B单列。N529/N530/N532空temp
按失败/三命令门清理资格暂留，不绕过。未启动App或新增包；唯一V325保留。
下一真实B5 Undo/归属基线与worker-stop/IO性能、完整native/public桥/B6及原余项。

最新 S214：万条完整发布正确性通过，批次级重复处理成为已测热点，仍非整体交付（2026-10-14）。
N527原22443已exit0，实际10000独立普通会话×1 event；10000 entry/revision/
committed receipt/reader event逐条验证，完整文件和重复run_pending=0断言保留。
总9353.143s，publish8439.668s、verification341.883s；terminal276ms/catalog32ms，
原500ms门不变，并发poll最大562ms。程序是unoptimized+debuginfo的native test，
不是正式App速度、UI首屏或全平台性能成绩；测试通过不代表吞吐已合格。
与同源码/EXE的N524千条publish213.609s相比，十倍数量实际耗时约39.51倍；
仅本机单次观察，不冒称普遍比例或完整归因。worker实际20003次读取/哈希
原计划98,431,542,517B，仍仅1次纯codec miss；job-header解码37,511,758,777B，
20000次exclusion解码/哈希各50,403,060,000B。批次字节随数量变大且每项重复
校验，纯codec缓存未消除该规模成本。各profile为inclusive，不能相加当独占时间；
publish还含6191次并发poll及502次周期报告/目录采样，未覆盖成本不全归产品JSON。
15:52:08已逐SHA复核764native/EXE/HEAD/index一致、原owner全退出；Rust隔离home
已收尾，wrapper删除最终空runtime容器，记录0logicalB/0observedB，不冒称整轮
物理回收量。保留原日志/样本/种子/WIP，无App/包/预览升级。下一真实万条
取消与源离线续跑，原计数/预算不缩；然后按实测归因收口性能及B5撤销归属/
公开关联桥/B6完整UI，所有原53/V5/平台/真实ACP/IME-DPI/小PR余项仍未完成。
本轮上游和fork main集成已在本地完成，不重复merge或再commit/push/PR。
唯一用户入口仍V325。详细原始结果见任务根runs/v527-performance-e2e4f47c85c8477a931af388e657213c。

下面S213保留优化与千条/万条准备基线，不代表万条完整发布仍未运行。

最新 S213：普通原始计划 codec 优化通过全库和两项千条实测，仍非整体交付（2026-10-14）。
真实采样先确认重复完整解码热点；每线程最多16个精确内容小键，不缓存计划正文、
SQL事实或授权。每次仍读取并SHA当前原始字节，关联member/input/proof/fence及
同事务、连接/job/total_changes绑定不变；没有schema变更或ordinary-allow flag。
N517新增测试误调私有入口导致编译失败，未进入断言；改用既有scan公开入口，
不放开产品可见性。N518关联57项、N519缓存6/恢复12/采样3及strict Clippy通过。
N520原49887已exit0，原12命令/26固定种子执行，全库1128.46s：4075passed/
0failed/75ignored；重叠和ignored不加算。764native/2556前端/seed/HEAD/index/EXE
绑定、76文本UTF8-noBOM-LF及原40质量门通过，App13244/85state/60effect/千行80不变。
N521原24898已exit0：1000独立会话完整发布及逐条结果/回执/reader/无重复通过，
总301.153s（N511485.267s，减少37.94%）、publish219.746s（404.307s，减少45.65%）。
worker完整原plan解码2003次降为1次，仍实际读/哈希2003次；不是整条worker线性证明。
terminal29ms/catalog5ms，但并发poll最大730ms，较旧354ms出现尖峰，须同输入复测。
N524原57032已exit0，同源码/EXE新隔离home完整1000复测：总290.368s/publish213.609s，
全部逐项结果/无重复通过；terminal29ms/catalog3ms/maxpoll183ms，未复现730ms。
保留首轮异常；一次复测不能证明所有负载下无偶发延迟，也不据此宣布稳定性验收。
N526原44814已exit0，实际10000普通独立会话准备/分页/计划/恢复/入队检查通过，
总529.771s；status9ms/items-page8ms、响应425B/12269B，原500ms与IPC门槛不变。
计划输入63.966s/plan48.121s/recovery956ms/commit-setup301.357s，入队准备仍重。
这不是万条完整发布、取消恢复或mixed/scoped通过；测试home/runtime-temp已退出清理。
N522原68430已exit0：总301.920s；203条时请求取消，204条时停，reply79ms/stop1894ms；
源离线/运行缓存重置后retry102ms，余796条publish174.059s，旧receipt字节不变，
最终1000条唯一且再次retry不重发。不可把796条阶段与完整1000或旧798条直接比。
这只是本机单次普通合成输入结果，不是mixed/scoped/10000/UI/平台验收或普遍提速保证。
N515/N519空temp及N520的380文件已退役；N520逻辑5246239B，观测H空闲增加5844992B，
不冒称独占物理回收。两千条runtime-temp已移除，最终空容器清理各0logicalB/0observedB。
N514/N517/N518空temp因现有清理门绑定范围而defer；保留全部日志/失败证据/种子/WIP。
无新App/包/预览promotion，唯一V325不变。详细交接及下一步见任务
plans/v523-original-codec-cache-and-capacity-next.md；先收尾与复测poll，再实际万条和
真实mixed/scoped；原B3-B6/public关联guard、完整UI UX/15语言/53/V5/vendor-liveWAL/
ACP四组合/100roundUNKNOWN/IME-DPI/三平台及小PR范围全部保留，goal继续active。

下面S212是原始千条基线，不代表当前源码尚未完成这次codec优化。

最新 S212：当前普通千条发布、取消/源离线续跑实测通过，性能仍待优化（2026-10-14）。
N511原94891已exit0，全程485.267s、发布404.307s；1000 entry/revision/committed
receipt/reader event各逐项核对，无重复。入队准备29.714s、结果核对26.730s；
terminal113ms/catalog4ms/并发job poll最大354ms。N512原75900已exit0，全程
490.680s；202条真实提交后取消，259ms返回、2470ms停worker；源离线/运行缓存
重置后retry266ms，余798条发布323.467s，旧receipt字节不变，最终1000条唯一。
两批复用未改N509源码/EXE/HEAD/index；原counts/IPC/页面/取消断言未削弱。
这只证明普通合成1000会话后端，不是mixed/association/10000/UI/平台验收，也没有
重新跑4066全库或优化吞吐。完整计划重复解码是疑似热点，先测贡献再审计优化，
不得缓存跨COMMIT授权、弱化原canonical plan hash、member/input/proof或fence验证。
无App/新包/预览替换；两runtime-temp已收尾移除，最终空容器清理记录各0logicalB/
0observedFreeDeltaB，不冒称独占物理回收。40质量及新鲜绑定收尾记录由任务
scripts/capacity-batch-closeout.ps1生成；详细证据和下一片见任务
plans/v513-capacity-baseline-and-hot-path-next.md。原B3-B6/UI全面重设计/15语言/
53/V5/vendor-liveWAL/ACP四组合/100roundUNKNOWN/真实IME-DPI/三平台/大容量/
小PR范围全部保留；唯一V325保持可用，public关联commit guard继续保留，非交付。

下面S211为全库修复基线，不代表最新大容量或整体交付已经完成。

最新 S211：上游集成后的9项恢复失败已闭合，当前全库GREEN；仍非整体交付（2026-10-14）。
已接受普通任务从同一acceptance事务捕获的原始计划恢复，不再依赖可变/缺失草稿；
空路由标记仍须用原始canonical plan hash验证，不是普通授权flag。关联任务继续验证
原retained-plan字节、完整成员/input/proof，部分新格式事实拒绝且reader不迁移。
原始计划codec/hash损坏为job-local，SQLite/BLOB I/O仍shared。Clock仅测试可冻结/
推进，排序在真实入队前设定，过期测试证明fresh submission拒绝且原JSON未变。

N503编译129.93s/4控制；N504早期29专项+strict Clippy通过。N505编译125.63s、
关联123.41s=56passed/1failed：模拟旧格式只删两条旧事实，留下新intent/snapshot，
真实retry撞唯一键。保留原RED日志，补齐全部四条新routing事实的negative删除和
真实absence断言，原closure race断言未削弱；产品增加partial事实前置拒绝。
N507编译126.42s/关联123.80s=57passed，N508当前30恢复/边界/真实child专项+
strict Clippy通过。新增原snapshot损坏、missing/malformed普通草稿恢复、12种
关联intent负例/36次0解码拒绝，包括协调空标记和checksum-correct计划降级。

N509原53510已消费exit0；v509-native-association-green经v396 seed wrapper执行。
103集成控制/58历史种子/44retention/strict Clippy以及full library1080.01s：
4066passed/0failed/75ignored；重叠集合不相加，ignored不计通过。12命令、761native/
2556未改F398前端/26immutable seed/HEAD/index/EXE绑定稳定；fresh-closeout的
71产品WIP文本UTF8-noBOM-LF、40质量门通过，App13244/85state/60effect/千行文件80
未增长。fullNativeGate=true，但finalAcceptance=false；无新App、包或预览promotion。

原始计划snapshot按job保存，普通entry仍有完整canonical intent校验成本；不宣称
整条worker线性或大容量通过。容量脚本已合法支持association完整gate+原编译focus+
seed/fresh binding，下一片先当前1000-publish/1000-cancel，再mixed/scoped和10000。
所有B3-B6、完整UI UX/两只读视图/15locale、53/V5/vendor/live-WAL、ACP四组合/
100轮UNKNOWN不重发、真实IME/DPI、macOS/Linux及小PR范围仍在；public关联commit
guard保留。用户唯一入口仍start-current-preview.cmd/V325，不把test EXE当新App。
清理/下一片实际执行记录见任务plans/v510-s211-closeout-and-capacity-next.md。

下面S210b及更早条目为历史记录，不代表当前源码仍有那9项失败。

最新 S210b：真实逐项进度写入已修复；全库新发现9项失败，仍不可交付（2026-10-14）。
set_item先在自身IMMEDIATE事务认证；正常worker发布的ACK复用最终publisher
已有的同快照认证，与catalog/committed receipt一起提交。私有结果在COMMIT后
才允许worker跳过第二次ACK；取消/去重/旧回执恢复/错误仍用认证setter。
连接、job及total_changes绑定在首个SQL写入前消费；没有跨事务缓存grant。
拆出jobs_publish_item域文件，低层无ACK入口仅用于测试，不增加dead-code抑制。

N496原62452编译失败（测试缺Connection导入），无断言；N497原28943退出0：
compile135.75s/focus124.19s，56passed/0failed。N498原10363退出1：50worker/
31publisher/41lifecycle通过，但strict Clippy发现生产/测试边界4项未使用诊断。
整理私有结果消费和cfg(test)入口后，N499原84612退出0：compile137.68s/
focus124.75s，56passed/0failed，758native/HEAD/index稳定。
原item writer10负例均0解码/原catalog字节不改；5种after-capture删改/错job/
错连接拒绝。header/catalog/receipt三种SQL abort全部回滚，真实retry同IDs且
只ACK一次。2/25项final writer均3次完整扫描/6次plan读取，ACK只额外解码1项；
这只是最终发布阶段、不增加扫描的证据，不是整条worker O(N)或10k性能完成。

N500原94534退出1，run v500-native-association-green的名称不是通过声明：
103项集成控制、57历史种子、44保留恢复及strict all-target Clippy58.11s通过。
full library1093.16s：4054passed/9failed/74ignored；ignored不是pass，子进程
0/1失败是其中一条父检查的证据，不再相加。source/Git/EXE稳定，但
fullNativeGate/finalAcceptance均false。9项按根因分为4个任务时间字段设置冲突、
4个已接受计划正文变更冲突和1个派生home-contract失败。原失败断言/日志保留，
尚未判定所有项目都是fixture问题；不能放宽关联认证或普通恢复合同来换绿。
下一片执行单：任务plans/v501-item-progress-closeout-and-scope-recovery-reds.md。

无新App/包/预览替换，唯一V325保持可用；原owners退出。N496/N497/N499
compiler-temp各0files，N498169files/1924922logicalB、N500584files/8099799logicalB
为有用失败证据及临时数据，因完整replacement未通过而保留。本批未删除物料，
回收0B；共享target、源码/WIP/seed/现有EXE及其他聊天产物均未清理。
原B3-B6、完整UI UX/15语言/原厂WAL/ACP/平台/当前容量/53验收/小PR范围仍保留。

下面S210a及更早条目是历史记录，不代表当前源码全库通过。

最新 S210a：真实批量取消的事务前置认证已修复，逐项进度写入仍red（2026-10-14）。
N492原49826已消费exit1：compile135.36s/focus98.39s，50passed/2failed。
两项新测试真实复现set_item及cancel_unstarted_queued遇到坏绑定仍写入；
原50项通过，不把新增red归因环境或修改断言。批量取消现已在自身SAME
IMMEDIATE事务内、完整正文/JSON投影之前认证一次，原phase/header/CAS仍不变。
N493原33368已消费exit1：compile139.85s/focus106.11s，52passed/1failed。
批量取消10种负例全部精确拒绝、0 JobItem decode、故障态全catalog字节不变，
仅恢复fault bytes后真实retry成功。2项/25项混合任务均只做1次完整成员认证、
1次header写入、分别2/25次item解码；真实已发布项回执、restart恢复及Undo通过。
这不是大容量性能或竞态全部验收；set_item原red未削弱，也未用每item全扫描绕过。

N494原43871已消费exit0，同N493 EXE的job-control50项122.68s、jobs31项
27.40s及strict all-target Clippy58.22s通过；另两项ignored不算通过。
756native/2556frontend/26seed/HEAD/index/EXE新鲜绑定稳定；当前全库未跑，
fullNativeGate/finalAcceptance均false。public关联commit guard及原范围仍保留。
原owners退出，无新App/包/预览替换，V325 CheckOnly通过且未launch。
N492/N493失败compiler-temp各0files/0logicalB及日志保留；只删除N494成功
subset自己的169份临时文件/1924922logicalB，观察H空闲+2174976B单独记录。
下一片执行单：任务plans/v495-batch-cancel-closeout-and-item-progress-next.md。

下面S209c及更早条目为历史记录，不是当前源码完整验收。

最新 S209c：发布及队列两个实际隔离 red 已闭合，仍未交付（2026-10-14）。
本地 WIP 检查点、upstream/main 与 origin/main 集成已完成；HEAD12f37c74、
index未变，后续WIP全部保留。没有重复合并、commit、push、PR或发版。
原registered enqueue在原事务接受绑定精确tuple/input的可选发布排除证据；
真正后续adoption在同事务旋转原tuple fence，删除/替换selector不能复活旧ticket。
最终publisher writer只重验metadata/stored-input digest/fence，不读取preview文件。
worker只跳过启动前明确job-local的拒绝，健康后续任务完成后仍返回原错误；
坏job原bytes/stateVersion不改。schema/SQLite/BLOB I/O仍shared，不按公开错误码跳过。

N487原66954已消费exit0：fmt3.23s、compile130.79s、专项88.06s，
50passed/0failed，755native/HEAD/index稳定。N488/N489错误过滤执行0项并被
harness拒绝，原失败日志保留；先核实当前EXE --list后才运行N490。
N490原46998已消费exit0：plan8、job-control50、jobs31、error-origin2、
scope-I/O1分别通过；另两项ignored不计通过。strict all-target Clippy57.83s
通过。source/Git/EXE稳定，fullNativeGate/finalAcceptance均false，不是全库门。
fresh755native/2556frontend/26不可变seed/HEAD/index/EXE绑定通过，40静态质量
门通过；App13244/千行文件80未增长。没有新App/包/预览替换，唯一V325
CheckOnly通过且未launch。无plan旧任务仍strict/unsealed，不宣称全形态兼容。

原owners全部退出。N481/N483-N489八个compiler-temp各0files/0logicalB，
空目录及失败证据保留。inventory→Execute仅删除N490自己的compiler-temp：
169files/1924922logicalB；观察H空闲增量2174976B单独记录，不宣称独占物理回收。
下一步真实set_item/cancel_unstarted_queued同IMMEDIATE前置认证及容量复杂度，
不能把每item全plan/member扫描引入O(N²)。原B3-B6/UI全面重设计/15语言/
原厂WAL/ACP/平台/capacity/53验收/小PR范围全部保留。
最新交接及下一片执行顺序：任务plans/v491-publication-worker-closeout-and-progress-next.md。

下面S209b及更早条目为历史记录，不是当前源码完整验收。

最新 S209b：旧任务兼容读取已收敛，队列隔离仍未完成（2026-10-14，非交付）。
本地检查点及两次main集成已经完成；HEAD12f37c74/index未变，WIP全部保留，
未重复合并、push、PR或发版。N474原句柄67432已消费终态exited1：编译128.60s、
专项77.10s，39passed/1failed，750native/HEAD/index稳定。真正V243旧任务由
现有enqueue acceptance writer原子补齐两份scope，此后不再被无关坏proof1
拖累；读取不补写，原job/input/receipt/archive/history不变。两种迁移写入
abort也通过完整rollback/真实retry。无retained plan的旧任务仍保留严格fallback，
不宣称全形态旧数据兼容。剩余red是坏关联队首使后续健康任务queued。

N473曾暴露历史plan含prepareOrigin、临时batch已disposed的回归。产品helper
已区分retained intent与fresh acceptance：历史保留plan校验结构/hash，不重开
disposed prepare batch；新commit仍验finalized receipt。N476复用N474 EXE，
实际plan_tests八项全部通过3.85s、source/Git/EXE稳定。N475用错测试过滤名
执行0项，被harness拒绝，不能计通过。N477新增竞态测试因测试常量/可见性
编译失败（54.39s），没有断言证据；只修测试常量与内部test hook导出。
N478原句柄55866已消费终态exited1，run v478-title-254a1c26aaec4b519b1056ff8b44018c：
编译131.19s、专项77.55s，41passed/1failed，751native/HEAD/index稳定。
新增六种after-capture竞态通过：old plan bytes/hash、old job context、单份
scope插入、真正proof1丢失/正文损坏。写入拒绝精确错误码；故障期间全catalog
原bytes/rowids不变，恢复故障后同request真正retry成功，无部分scope/acceptance。
仍只剩原队列隔离red，未削弱其断言。N479原句柄80381已消费终态exited0：
同N478 EXE的plan8项3.81s、job_control50项121.18s、jobs31项27.02s全部通过；
另两项ignored不算通过。strict all-target Clippy56.30s通过，source/Git/EXE
稳定，但fullNativeGate/finalAcceptance均false，不是全库或交付验收。
当前751native哈希与N478一致，56个WIP文本UTF8/noBOM/LF、40质量门及
whitespace通过；App13244/千行文件80未增长。原owners已退出；N473-N478
六个compiler-temp各0files/0logicalB，空目录/失败日志保留。只删除N479成功
subset的compiler-temp：169files/1924922logicalB，观察H空闲增量2174976B
单独记录，不宣称独占物理回收。相关测试脚本可重建这些临时运行数据。

publication旧tuple absence仍走全局proof认证，不能只调度绕过坏head；真实
progress/cancel/Undo及原全部B3-B6/UI全面重设计/15语言/原厂WAL/ACP/平台/
capacity/小PR验收保持原范围。仅V325预览保留，CheckOnly通过且未launch。
新交接：任务plans/v480-compatibility-cas-closeout-and-worker-next.md。

下面S209a及更早条目为历史记录，不是当前源码完整验收。

最新 S209a：已改产品，部分故障隔离修复验证通过（2026-10-14，仍非交付）。
本地检查点98e3b524、upstream合入a0883e7d及fork合入12f37c74已经完成，
没有重复合并。当前HEAD/index未变，全部后续WIP保留，不push/PR/发版。
现有registered enqueue在同事务写job-local scope和独立digest；读取仅点查
本job的proof，仍校验canonical input、bounded header、完整retained-plan成员。
记录只负责路由，不是新grant；缺失两份scope仍走严格旧路径，不自动认定普通。
N467现代proof2和proof1的两个跨任务隔离red通过。新增extra-job-witness负例
在N469真实失败（get/page/retry解码1/1/2次），已补同job认证记录完整性拒绝。
N470 original36686 exited1/run v470-title-46610e0982aa4a7aac045d0b274b128b：
编译131.68s、专项72.10s，38passed/2failed；749native/HEAD/index稳定。
45次scope篡改exact-code拒绝且zero JobItem decode，原34项、普通坏/缺失input
worker控制和现代跨任务隔离继续通过。剩余两个是真正产品red：
1. 无scope的真正V243旧任务仍被无关损坏proof1拖累。只读核实其版本1计划
   实际保留，之前“该样本无plan”是假前提，已纠正；未删除plan或制造旧样本。
2. 损坏关联任务位于队首时，run_pending/restart仍使后续健康任务queued。
publication的旧tuple absence也仍依赖全局proof认证；不能只改队列忽略报错。
未跑已知red的全库门，N461 full/Clippy只是历史绿。40质量门通过，App13244、
千行文件80未增长，scope380/test319/job_control936/jobs_publish994。
原owners退出，无native/App owner；N466至N470 compiler-temp精确inventory
各0files/0logicalB，空目录和失败日志保留，无删除或回收空间宣称。唯一V325
CheckOnly通过，未launch/更新。后续工作单：任务plans/v471-scoped-entry-progress-and-next-slice.md。
public新关联commit guard、真实worker/progress/cancel/Undo、B3-B6/UI全面重设计、
15语言/真实平台/vendor/capacity/全部原验收范围仍保留，goal保持active。

以下为此前批次历史记录，不代表当前源码的最新完整验收。

S209 已建立真实隔离性 red（2026-10-14，尚未修复，非交付）：
仅新增真实生产流程测试及模块接线，没有改产品逻辑。N463 original27041
exited1/run v463-title-b5be83bfd7704491aa1be2a4a17229f8：34passed/1failed，
63.89s专项/132.16s编译。扩展后的N464 original28874 exited1/run
v464-title-bc703fe4b7c248eca0ac9d8205a8ecee：35passed/3failed，
69.34s专项/127.35s编译；747native/HEAD/index稳定。三个失败都是产品red：
1. 7类proof2/selector/witness故障，各拖累2个无关正常任务的3个公开入口，
   共42个无关读取失败；坏关联本身27次exact-code拒绝/zero JobItem decode。
2. 真实旧注册archive→registered preflight产出的proof1有3类损坏时，
   无关普通任务和健康关联任务的3个公开入口共18次被拖累。
3. 队首关联proof损坏时，真实run_pending及restart均报SchemaUnsupported，
   后续健康任务仍queued。关联job/input/receipt未变；恢复proof后2任务完成，
   不能拿这个恢复后控制冒充损坏期间的队列隔离。
原34项继续通过。普通坏/缺失input真实worker控制通过：失败可见、未合成
input、后续健康任务完成、restart不重发。各负例先恢复捕获的原bytes/rowids
再断言健康重试、全catalog rows、lease、两方history/archives；不是前提错误。
两轮原owners均退出，无活动task native/App owner。两轮compiler-temp精确
inventory各0files/0logicalB；空目录与失败日志保留，无删除/回收空间宣称。
cheap检查40质量门、48产品UTF8-noBOM-LF文本、whitespace通过；App13244、
千行文件80未增长。V325 CheckOnly通过，未launch/未更新唯一预览。
没有跑已知red输入的全库门；N461只是S208历史绿，不是当前字节的full green。
下一工作单：任务plans/v465-job-scoped-absence-design-and-fix.md。需先确定
同snapshot、job-scoped的完整归属/absence closure；不能忽略坏proof、只信
marker/count/plan intent或给普通job/input全局加16MiB。旧proof1归属仅存于
可能损坏的proof正文，当前owner/accepted键不能独立覆盖它；这是设计依赖，
不是已经安全解决。仍保留public新关联commit guard、全部原B3-B6/UI/平台范围。

S208 full/fresh/cleanup CLOSED - 2026-10-14 (synthetic native, NOT delivery):
N460 original2090 exited0/run v460-title-70472dbb0f3847c084f1808d24f863b5:
34/0,60.57s focus,134.07s compile,746native/HEAD/index stable. All39 real
public retry/page/state-entry faults refuse with exact codes and zero JobItem
decode; original rowids/bytes/inputs/proofs/markers/history and healthy retries
remain. One complete membership scan per entry, snapshot-borrowed witness; public
new-association commit stays disabled. Ordinary large job/mixed input controls pass.

N461 original73966 exited0/run v461-native-association-green:12 commands green;
full library4061 passed/0 failed/52 ignored,1078.12s;103integration controls,
35 historical-seed,44 lifecycle and strict all-target Clippy55.75s pass. Sets
overlap; ignored are NOT passes. Full source/Git/EXE bindings stable. Fresh
746native/2556frontend/26seed/HEAD/index/EXE/40quality/47product UTF8-noBOM-LF
texts pass, precise original harness57592/test23776 exited. No active native
handle to keep polling; freeze has ended. App13244/files>=1000=80 unchanged;
job_control930 (down from953), jobs_publish994. No ceiling increases.

Inventory->Execute removed ONLY N460/N461 compiler-temp:380files/5246239logicalB.
ObservedHfree+5705728B separately, NOT exclusive physical reclamation. Task title
and v396 scripts regenerate temp. N457/N458/N459 compiler-temp each inventory0
files/0logicalB; all failure logs/empty dirs and unclassified evidence retained.
Source/WIP/deps/immutable seeds/logs/currentEXE/symbols/UI bundle/soleV325 remain.
FixedV325 CheckOnly passed: no launch/no running preview family. No new worktree/
agent/commit/push/PR/real account/provider/history changes; integration not repeated.

Next order: plans/v462-job-entry-isolation-and-worker-next-slice.md. FIRST prove
present ordinary-job isolation from damaged unrelated legacy/association proofs.
Current global selector closure is an AUDIT RISK, not certified isolation.
Then actual worker/progress/cancel/Undo predecode. Sentinel change_job is still
not full worker/cancel evidence. Multi-authentic-old-target/mixed projects,
changed-source refresh/conflicts, B5/B6/UI/15locales/Windows IME-DPI/macOS/Linux/
vendor/live-WAL/ACP/contracts/capacity/small-PR and ALL original scope remain.
A green S208 is NOT complete B3/B4, UI/platform acceptance or goal completion.
Post-handoff cheap recheck:51 UTF8-noBOM-LF texts (47product+4external),
40quality/App13244/files>=1000=80 and unchanged HEAD/index all pass.

S208 focused green; full gate RUNNING - 2026-10-14 (not delivery):
Product job_entry authenticates before get_job/find-by-request and public
job_items_page SQL JSON projections, in the caller's SAME read/writer snapshot.
Optional selector closure streams bounded proofs/map/digest witnesses; selectors
choose candidates, never grant access. Exact canonical association inputs, bounded
headers, complete retained-plan members and accepted proof2 bind the read.
AcceptedMembers is non-serializable and snapshot-borrowed; one complete membership
scan per entry, shared plan context checks/indexed selections, not per proof.
Ordinary bodies/inputs are not loaded/capped to classify absence. Missing jobs
return absence without depending on unrelated corrupt candidate records. Keep the
public new-association commit guard and both deliberate non-test dead-code notes.

N457 original41664 exited1: E0603 visibility compile failure52.83s, no assertions.
N458 original97030 exited1: compile129.60s; wrong explicitly supplied seed path
(missing df) caused0/34 in0.08s, not product evidence; corrected environment only.
N459 original36799 exited1:29/5,58.86s,cache compile0.92s. Original15 predecode
negatives now pass. Four old membership tests incorrectly used the newly guarded
reader to inspect their deliberate coherent negative fixtures; replace only the
premise with raw SQL/Value, ALSO require product get_job refusal/zero decode.
One introduced regression: absent job depended on unrelated corrupt proof; fix
metadata-only existence-first, leaving present association authentication intact.
All failure logs retained. Expanded real entry tests cover13 faults x3=39, exact
codes/zero JobItem decode, restored rowids/raw job/input/proof/markers/old-owner
facts, source/entry/plan/lease/receipt/history and healthy same-request retries.
N460 original2090 exited0/run v460-title-70472dbb0f3847c084f1808d24f863b5:
34/0,60.57s focus,134.07s compile,746native/HEAD/index stable. Ordinary large
job and mixed ordinary large-input controls pass. 40 quality gates pass;
App13244/files>=1000=80 unchanged (not an increased ceiling).
N461 full same-input is RUNNING in original73966/run
v461-native-association-green. Strict Clippy/full library/fresh/cleanup NOT closed.
Freeze native/fixtures/HEAD/index; continue the original owner, do not restart for
silence. No App/preview update/new worktree/agent/commit/push/PR/real data.
Global allocation/large-ledger latency, multiple authentic historical targets,
worker/cancel/progress/Undo predecode, B5/B6/UI/platform and ALL original scope
remain open. Do not call this full B3/B4 or goal completion.

S208 entry-point product reds recorded - 2026-10-14 (not a green gate):
N456 original4923 exited1/run v456-title-a0840cc9500443d3805f56074570e4b5:
31 passed/3 failed,56.57s focus,128.34s compile,744native/HEAD/index stable.
Real acceptance -> public same-request commit, public job_items_page and actual
change_job post-read error closure reproduce >16MiB job acceptance/body decoding,
header binding and deleted-member refusal gaps. Five faults per entry (15 log
records); reset owned negative JSON before complete state/history assertions.
change_job uses its own IMMEDIATE transaction; its sentinel errors before saving.
This is predecode evidence, NOT full worker/cancel/Undo coverage. N456 owners
exited; compiler-temp empty (0 files/0 logical bytes), failure logs retained.
N454 remains historical evidence for S207, NOT green for current S208 bytes.
Next: authenticate proof2/accepted digest/canonical input/bounded header/full
retained membership in the SAME snapshot before full JobRecord or SQL JSON reads.
Ordinary capacity, corrupt-input failure reporting, public guard and all original
B3-B6/UI/platform delivery requirements stay. No App/preview promotion/commit/
push/PR/real account/provider/history changes; goal remains active.

S207 本批后台验证已收尾（2026-10-14，非交付）：新增隔离旧-owner点读边界测试及
真实普通导入/当前身份dedup控制，没有改产品行为。N452原16855退出1，run
v452-title-68af906f8b924b3ca22d3b5281c20971：30passed/1failed、54.55s，
compile131.98s，743native/HEAD/index稳定。7种超长/非法UTF8故障回滚及健康
PlanStale拒绝、当前身份真实rescan/dedup均通过。无关普通来源已实际worker
committed且历史保护通过；其最终读取却误用了固定两消息seed的text助手，
新来源实际一消息，是测试前提错误，不是产品red。改为真实events_page验证
一消息及其内容，并加强边界错误消息断言；保留N452失败证据。
N453原95490已退出0/run v453-title-a779686a55c5489f8e383c99113697f5：
31passed/0failed、53.83s、compile127.94s，743native/Git/index稳定。
N454同输入完整门原25384已退出0/run v454-native-association-green：12commands
全绿，全库4061passed/0failed/49ignored、1077.52s；103集成专项、32历史样本、
44生命周期、严格all-target Clippy55.02s通过，集合重叠，ignored不算passed。
fresh743native/2556frontend/26seed/HEAD/index/EXE、40质量门、44个产品WIP
UTF8-noBOM-LF文本通过，原harness39028/test45404已退出，native冻结结束。
inventory→Execute只清N453/N454的精确compiler-temp：380files/5246236logicalB，
观测H可用空间+5705728B单列，不算独占物理回收量。title/v396脚本可重建。
N452失败日志与空compiler-temp、其他未分类失败证据保留；源码/WIP/依赖/seed/
日志/当前EXE和symbols/UI bundle/唯一V325保留。固定V325 CheckOnly通过，
未launch、无运行preview family。不能用N454后台绿冒充实机/跨平台/完整交付。
后续工作单为plans/v455-association-public-predecode-next-slice.md，包含实际
public job_items_page在SQL JSON投影之前的缺口，不能只修get_job就宣称全部覆盖。
本批未启动App/更新V325/commit/push/PR/操作真实数据，S208尚未新增源码。
同内容dedup不冒充源内容变化的later refresh；公开predecode/B5/B6/全原范围仍开放。

S206 本批后台验证已收尾（2026-10-14，非完整交付）：N451 原52232已退出0，
12 commands全绿；全库4061 passed/0 failed/46 ignored、1075.40s，29seed、
44lifecycle与严格all-target Clippy通过，集合重叠、ignored不算passed。
fresh742native/2556frontend/26seed/HEAD/index/EXE、40质量门、43个UTF8-noBOM-LF
文本通过，原owners均退出。inventory→Execute只清N449/N450/N451的精确
compiler-temp：380files/5246227logicalB，观测H可用空间+5705728B单列，
不算独占物理回收量。title gate/v396脚本可重建临时文件。N448失败证据与临时
目录保留；源码/WIP/依赖/seed/日志/当前EXE和symbols/UI bundle/唯一V325保留。
固定V325 CheckOnly通过，未launch、无运行preview family。N451不要重启。
native冻结已结束；下一批S207补oversize/UTF8点读与无关普通导入/当前身份dedup控制。
真实多历史目标需要第二份可验证的旧输入，不能编辑当前adapter/native/normalizer
伪造；现有固定seed仅一个目标。更大ledger延迟、公开predecode、B5/B6及全部原始
验收仍未完成。新增源码后N451不再是当前字节完整绿证据；不启动App/更新V325/
commit/push/PR/操作真实数据，goal保持active。

最新 S206（2026-10-14，专项通过、完整门正在运行，公开入口仍关闭）：原始
hash-pinned OLD_JOB input2 在隔离 home 仅 relocate previewPath；metadata roundtrip、
三份归档哈希、normalizer1.0.0/raw selection 验证，lease 标为 synthetic fixture
restoration，不复活原 seed preview。真实 ordinary worker 控制及原 committed receipt
无需 preview 恢复通过。old-first 在真实 detached preflight hook 内发布，普通和
explicit-reimport 均产生新 revision，原确认 CAS PlanStale 拒绝并保留新 revision。
N448 原44133退出1/run v448-title-61ff7031dee6483bbea92ad0f80328a7：
26/1、49.82s、compile132.27s，741native/Git/index稳定。association-first 两种
模式实际新建第二条 entry，是产品 red；该版 new-receipts 日志只统计原 import，
不代表全局无写入。修复后的控制改用全表 receipts 快照。
独立 optional old-owner ledger1 在同一接受 writer 写精确 tuple selector/配对 marker，
store1/proof2/input/receipt格式不改。索引只选候选；副作用前及最终 writer 仍认证
proof2/accepted digest/input/header/full plan/原 anchor-cutoff/registered live target。
未命中必须同快照流式核对有界 proof 的完整映射与 accepted digest，拒绝协调 key
substitution，不能从 count/marker 授权。无 JSON extraction/路径或 native-prefix
猜测/读源/重开 preview/新 App state/容量放宽。
N449 原31683退出0/run v449-title-4f0d3b39a90f457584b2ebe9fa9c8d2b：
28/0、50.84s、compile130.38s，742native/Git/index稳定。加强 absence 闭合后的
N450 原46588退出0/run v450-title-c6a6e3aff3894973a77f8e568205174a：
28/0、51.07s、compile131.96s，742native/Git/index稳定。14类索引异常明确拒绝、
rollback/健康重测通过；association-first 真实 worker 旧任务 failed、新任务 committed，
只有原 entry，副作用前全局 receipts 不变，原历史可读。集合不相加。
N451 同输入完整门正在原52232运行/run v451-native-association-green，harness33896
created2026-10-14T03:04:07.095101+08:00。不得因等待重启；native/fixture/HEAD/index
冻结。N451已通过29seed（55.12s）、44lifecycle（38.82s）与严格all-target Clippy
（55.91s）。全库仍在原test75544（created2026-10-14T03:06:43.469138+08:00）运行；
全库/fresh/清理尚未完成，N447不代表当前源码绿。N448证据保留。
未启动 App/改唯一V325/commit/push/PR/操作真实数据。旧-owner全局 allocation、
大规模 absence 延迟、混合多目标/项目/oversize控制及 B3/B4/B5/B6/原范围仍未闭环。
下一步续 poll 原52232，完整绿后 fresh→owners退出→精确 inventory/Execute 清理，
再推进剩余控制、公开入口、Undo、UI和原平台验收；goal未完成。

最新 S205（2026-10-14，内部发布与恢复的本批后台验证已完成，公开入口仍关闭）：N440 原96469
退出1，739native/Git/index稳定，19 passed/1 failed、39.42s。真实接受后
worker返回FORMAT_UNSUPPORTED，原entry/历史/job/回执/归档完整，是实际发布缺口。
新增非序列化AcceptedPublication，由真实stored proof2、accepted digest witness、
精确canonical input、有界job/header和完整确认plan成员派生。现有publisher锁后、
ID和项目副作用前认证目标，最终短writer再次认证；新receipt固定原importId、
原previousRevision和created_entry=false。复用stage/rename/link/commit，不另建
publisher、不从intent/receipt授权。anchor抽为共用private helper；关联receipt
使用metadata-first有界点读，ordinary容量与legacy注册限制不变，公开commit仍关闭。
N441 原70676退出1，20 passed/1 failed、40.81s，compile141.87s。
真实worker已committed、原历史完整，12个owner负例通过；失败位于测试助手在
完成后重建已回收preview，非worker发布失败。已改用同request空items恢复原job。
新增原child manager六个exit77发布断点和十个receipt边界。N442 原82466退出1，
编译因新增测试漏catalog模块导入失败，54.10s，未运行断言；已补精确import。
N443 原4038退出1，740native/Git/index稳定，22 passed/1 failed、45.71s。
六个真实exit77发布恢复断点全部通过，同import/revision、旧历史及完成后无需
preview恢复通过；新receipt边界测试却先被已回收lease拒绝，是测试前提错误。
已改用test-only薄入口调用真实load_durable_input→recover_committed_input，不复活
preview、不改生产GC、不弱化断言。N444 原30555退出0：23 passed/0 failed、
45.85s、compile130.04s，740native/Git/index稳定。12个owner负例和10个真实
completed receipt边界错误均核对，健康恢复/旧回执/entry不变；集合不相加。
N445 原31321退出1：103集成专项通过，historical seed为23 passed/1 failed，
49.75s。旧protocol断言仍预期统一FormatUnsupported，实际为receipt元数据不匹配的
PlanStale；不是已接受worker再度失败。精确校验已改为元数据mismatch消息与真实
未接受worker的HistoryAccessUnavailable/缺失accepted owner消息，仍要求无新条目/
receipt。test-only helper重命名为validate_committed_input_metadata，避免当成授权。
Clippy/全库未运行。N446 原30265退出0，run
v446-title-ce22c1220dd744b7869e5c068b636ecb：23/0、46.37s、compile130.27s，
740native/Git/index稳定。N447 同输入完整门原50106退出0，run
v447-native-association-green，12 commands全绿：全库4061 passed/0 failed/
41 ignored、1078.57s；103集成专项、24seed/接受、44lifecycle和严格all-target
Clippy55.94s通过，集合重叠，不相加。fresh740native/2556frontend/26seed/
HEAD/index/EXE、40质量门/40个UTF8-noBOM-LF文本通过，原owners已退出。
inventory→Execute仅清N444/N446/N447的精确compiler-temp：380files/
5246239logicalB；观测H可用空间+5705728B单列，不算独占物理回收量。
失败N440/N441/N442/N443/N445证据暂留；WIP/依赖/seed/日志/当前EXE和symbols/
UI bundle/唯一V325保留，title gate/v396集成脚本可重建临时文件。
固定V325 CheckOnly通过，未launch且无运行preview family。N439不是当前源码
证据，N447后台绿也不替代以下完整产品验收。
下一实现工作单在v425 section2/S206：公开入口、late old
producer两种先后顺序、worker/status/cancel/Undo提前认证、later refresh、fixed
refs/grants、混合项目、UI/15语言和原平台验收仍未闭环。未启动App/改唯一V325/
提交/push/PR/操作真实账号或历史；goal未完成。

最新S204（2026-10-14，完整后台验证完成、发布链未接入）：新增真实pre-COMMIT子进程exit73、回滚write/
sync/rename/回执remove四点，以及实际混合计划的协调extra/remove/substitute成员
负例。复用现有child/retention/fault RAII；正向来自真实scan/preview/确认/接受，
负向额外input来自另一个真实queued普通producer。N434原会话50096退出1，738native/
Git/index稳定，14 passed/3 failed、29.09s。崩溃与四点rollback I/O已通过；三种
协调成员故障均被真实reader/retry/owner接受，JobItem解码3/1/2，是实际产品red。
错误断言之前的精确行/rowid恢复、原plan/receipt/history与健康重试/lease均验证。
完整成员校验已改为同一读快照绑定确认plan的唯一preview/selection/snapshot集合，
不重开preview/套用TTL/从count授权，不统一缩小ordinary v2容量。新加重复preview
拒绝和混合计划>16MiB普通input容量对照。N435原会话99118退出1，18/1、38.68s；
原三种产品负例均reader/retry/owner提前SnapshotUnavailable拒绝，JobItem0/0，
混合普通大input容量对照通过。重复preview新测试却改了candidate而未改其原始pin，
被既有SourceMember校验提前PlanStale拒绝，是新增测试前提错误，不是新产品red。
已保留失败证据；现改为保留第二条真实candidate/pin/native，只重复preview元数据，
并通过确定的candidate顺序与精确错误消息确认到达唯一覆盖检查。N436原会话9993
退出0：19 passed/0 failed、39.35s；fmt/compile/manifest通过，compile132.32s，
738native/Git/index稳定。四种成员故障均reader/retry/owner拒绝，错误明确，
retry/owner实际JobItem解码0/0；duplicate到达精确唯一覆盖拒绝，大普通input对照通过。
同一输入的N437原会话62595退出1：103集成专项、20种子/接受和44 lifecycle通过，
严格Clippy因新增测试的err_expect写法失败，全库尚未执行；26seed和738native/
Git/index/EXE稳定。已改为语义等价expect_err，不屏蔽lint或放宽错误预期；
N438原会话12657退出0：修改后19/0专项、38.73s、compile130.51s，738native/
Git/index稳定；错误预期/实际0/0解码未变。N439同一输入原27377退出0，12 commands
全绿：全库4061 passed/0 failed/37 ignored、1079.92s；103集成专项、20种子/接受、
44 lifecycle和严格all-target Clippy58.73s通过，集合重叠，不相加。fresh738native/
2556frontend/26seed/HEAD/index/EXE、40质量门与35个UTF8/noBOM/LF文本验证通过，
原owners已退出。inventory→Execute仅清N436/N438/N439的三个精确compiler-temp：
380 files/5246239logicalB；观测H可用空间+5705728B单列，不算独占物理回收量。
源码/WIP/依赖/seed/日志/当前EXE和symbols/UI bundle/唯一V325保留；N434/N435/N437
失败证据暂留。title gate及v396集成脚本可重建临时文件。固定V325 CheckOnly通过，
未launch且无运行预览家族。完整后台绿不代替关联publisher、前台/跨平台或交付验收。
N433只绑定此前输入；N434/N435/N437失败证据与临时数据保留。
关联前台/publisher仍关闭，未启动App/替换V325/提交/push/PR。

本批结论：本地检查点和双 main 集成已完成。历史身份关联新增了真实的内部接受
事务和旧历史读取路径，前台提交、worker/publication 仍关闭。N424 后新增事务/
reader 边界测试的N428/N429批次已全绿收尾。其后N430发现真实的任务JSON提前
读取问题；读取顺序修复在N431专项转绿，新增拒绝控制的N432专项12/0通过。
N433同一冻结输入的12-command完整后台绿门和fresh/清理收尾已完成，旧N429
不代替本次证据。B3/B4 完整验收、publisher/
recovery/late producers、Undo/fixed refs/grants、UI/15 locales 与原始实机/跨平台
等验收仍按原计划继续，整个功能尚未可交付。
唯一用户预览仍 V325，本批后台 WIP 未提交、未推广到预览，没有新 App/安装包。

S203 retention/retry提前读取修复（2026-10-14，后台验证完成）：N430实际9 passed/2 failed，
是产品red，不是测试前提错误。真实已接受关联任务只改display title超16MiB，
最终虽然返回ResourceLimit，重试/retention owner在此前已解析2/1次JobRecord.items。
精确恢复、健康重试/owner、旧历史/receipt/lease保留和普通大任务对照均通过。
生产路径改为request只查stored job ID；逐项在完整JSON之前认证durable input/
proof及有界header，只缓存identity digests。proof精确绑定被分类的input/context，
已有accepted markers必须和真实已认证输入对应；marker数量不是独立授权。
普通任务仍保持原完整body容量，无新全局16MiB限制或并行rollback owner。
N431 11项专项通过（133.21s compile、15.72s focus、736native/Git/index稳定）：
两条拒绝路径实际JobItem解析计数均0，原普通大记录仍通过。随后marker查找
改为meta.key的BINARY索引前缀范围，避免扫描不相关记录；增加8种negative fault
分别检查真实retry和owner拒绝。N43212/0专项通过，compile141.58s、focus19.15s，
736native/Git/index稳定；8种fault各自真实retry/owner均提前拒绝，错误码明确，
JobItem解析计数0/0，精确恢复及健康重试/旧历史/lease都验证。原owner已退出，
N433原执行会话14111退出0，12 commands全绿：full library4061 passed/0 failed/
30 ignored，1089.99s；103集成专项、13种子/接受、44 lifecycle和严格all-target
Clippy通过，集合重叠，不相加。fresh736native/2556frontend/26seed/HEAD/index/
EXE、40质量门与32个WIP文本UTF-8/noBOM/LF验证通过，所有原owners已退出。
inventory→Execute仅清N431/N432/N433三个精确compiler-temp目录：380 files/
5246230logicalB；观测H可用空间+5181440B单列，不算独占物理回收量。
源码/WIP/依赖/seed/日志/当前EXE和symbols/UI bundle/唯一V325均保留；N430等失败
与未分类产物暂留。title gate与v396完整验证脚本可重建这些临时文件。
本批未启用关联发布或启动App，完整后台绿不代替实机/跨平台/交付验收。
S204最新实现/验证状态见本文件顶部和外部v425；公共commit同request
入口仍需接入同一stored-owner认证，本次内部修复不冒充全入口覆盖。
拒绝控制不冒充任意恶意
SQLite全局分配保证。association crash、immutable plan完整成员集合、publisher/
restart/late producers与B5/B6/原始全范围仍未闭环。

S202 真实事务与回执边界补证（2026-10-13开发，2026-10-14后台验证完成）：新增独立 test-domain
模块，不改生产行为。实际 immutable V243 seed 经 scan/preview/确认计划和真实
内部接受，不手写 positive proof/job/input/entry 行。6 个实际 SQL 写入阶段分别
中断（schema marker、proof witness、entry identity、job、input、retention witness），
确认没有部分 proof/身份/任务/input/witness，旧 entry/source/alias/receipt/job/lease
保留，移除唯一故障后同一 request/job 正常接受并在 reset 后读旧历史。
9 种确认后 race 覆盖精确 plan bytes/hash/created/expiry、过期字节、anchor 内容/
row、cutoff 和 old normalizer。12 种已接受 receipt fault 保持 SchemaUnsupported/
HistoryAccessUnavailable/ResourceLimit 的明确分类；SQL NULL JSON 则由原始 NOT NULL
约束阻止，不能冒充 reader 对实际不可注入 NULL 行的覆盖。
N426 6/1 是新测试漏掉 retention 既有错误包装；N427 7/1 是新测试未考虑 NOT NULL
约束。原始失败日志保留，未改变生产/原始种子断言。N428 8/0 专项通过，fmt/
compile/manifest、735 native/Git/index 稳定，compile140.18s、focus13.76s。
N429 原owner退出0，12 commands全绿：full library4061/0/26，1096.57s；
103项集成专项、9项种子/接受、44 lifecycle与严格all-target Clippy通过，集合重叠。
fresh735native/2556frontend/26seed/HEAD/index/EXE、40质量门和31个UTF8/noBOM/LF文本
验证通过。
仅清成功N428/N429的compiler-temp：380 files/5246239logicalB；观测H可用空间
+5705728B单列，不算独占物理回收量。N426/N427失败日志与其他未分类产物暂留，
源码/WIP/依赖/seed/当前EXE与symbols/UI bundle/唯一V325全部保留。
3个新增测试含上述28个控制子检查，不是28个独立Rust测试。发布/恢复/late producer、allocation
audit、association pre-COMMIT/rollback-I/O crash 与 B5/B6/原始全范围仍继续。

S201 真实关联接受与旧历史 reader（2026-10-13，后台验证完成）：内部 producer
从真实 inspect_witness、确认计划、canonical DurableInput 和不可序列化的
VerifiedInput 构造 proof2；沿用 lifecycle、PendingInputs、文件绑定和非阻塞
publication lock。短事务比较完整 entry、source/alias、tombstone、revision、
anchor/cutoff、已有 proof 与精确 plan bytes；原子 insert proof/accepted digest
witness、只改指定 native identity、保存普通 job/input/retention。旧 importId、
revision、archive 和 receipt 保留；reader 不重开 preview/source。普通/v1 容量
路径不统一缩成 16MiB，新的 association point reads 使用 scoped bounds。

N415 是真实接受失败，N416 1 项和 N417 3 项接受测试转绿；N418 5 项通过，含
真实 SQL COMMIT rollback、publication busy/native collision、子进程 COMMIT 后
ACK 前 exit74，以及 checksum-correct proof/input/job/membership/witness 拒绝。
N419 原种子测试发现新 receipt reader 错误分类回归：5 passed/1 failed，exit101。
已恢复 malformed receipt 的 SchemaUnsupported，不改旧断言。N420 缺少明确
seed 环境路径、0 passed/5 prerequisite failed，非产品断言，不冒充 green。
N421 5 项通过；N422 原种子加接受为 6/0、lifecycle 44/0，但严格 Clippy 失败，
未跑 full library。修正无用 import、boxing 大枚举和 cfg(test) ordinary lane。
未接入的内部入口使用仅 non-test 的 expect(dead_code) 并说明关闭原因，接入
后该 expectation 必须删除；没有开启半成品前台或忽略 Clippy 大枚举告警。
N423 重新编译/fmt/manifest 与 5 项接受专项已通过、734 native/Git/index 稳定。
N424 相同冻结输入的 103 项集成专项、6 项原种子/接受、44 lifecycle 和严格
all-target Clippy 已通过；原 owner 退出0，full library 4061 passed/0 failed/
23 ignored，1097.61s，12 commands 全绿。734 native、2556 frontend、26 seed、
HEAD/index/EXE fresh bindings 与40项质量门通过，30个WIP文本 UTF-8/no BOM/LF。
fresh脚本首次排序前提错误已记录并修复，26个path/hash未变，不算产品red。
仅清本批已退出的 compiler-temp：380 files/5246239 logicalB，观测H可用空间
+5181440B单列；失败数据、源码/seed/日志/EXE/symbols及唯一V325全部保留。
N415/N419/N420/N422 原失败日志
保留，各集合有重叠，不能相加。下一步详见外部 v425 continuation work order。
还缺真实阶段失败/plan-anchor-cutoff races、发布/restart/late producers、混合
计划/项目映射及 B5/B6，不写成 B3/B4 完全交付。HEAD/index 不变，未访问真实数据。

S200 普通导入的 durable retention 恢复（2026-10-13）：两边 main 已集成本地，
98e3b524/600584fa 检查点与 v387 精确快照保留；当前 HEAD 仍为 12f37c74。
本批后台实现是未提交 WIP，不 push、PR、发版或修改真实 App 数据/provider。
最新 N411 文件身份绑定实现因 Windows verbatim 路径字符串比较失败（12/41/0），
原 owner 已退出，失败证据保留。N412 已修正路径解析、保留文件身份/VFS 校验，
并新增正常 SQL 写入/sharing、原文件 rename-back 测试。N412 原 owner 已退出0，
56 passed/0 failed/0 ignored、82.94s；fmt/compile/manifest 和 730 native/Git/index
稳定。自有空 temp inventory→Execute 清理 0 logicalB，H delta -4096B 不算回收。
N413 相同输入的完整绿门已完成，原 owner 退出0：全库 4061 passed/0 failed/
18 ignored（1116.00s），103 项上游集成专项、实际 1 项原 immutable seed、44 项
lifecycle 和严格 all-target Clippy，12 commands 全绿。集合重叠，不相加。
730 native/Git/index/EXE 稳定，26 seed hashes 不变，checksFinished true。
仅清成功 N413 compiler-temp：380 files/5246221 logicalB，观测 H delta +5181440B
单列，不视为独占回收量；N410/N411 及早期失败 evidence/temp 保留。
N414 当前源码 1000-publish 实际 1 项测试通过、原 child 23980 已退出：全链路
253.749s、publish 168.186s、verification 28.246s，峰值 RSS 48648192B。
status/items 5/8ms、mapping 1387ms、plan-inputs 9060ms、plan 4775ms、commit setup
32836ms；terminal/catalog 8/4ms，并发 job poll 峰值 439ms。1000 条 entry/revision/
committed receipt/reader event 均验证（每会话 1 event），重复 worker run 为 0。
source/Git 稳定；自有 runtime 清理 0 logicalB/observed H delta 0B，logs/metrics 保留。
比 N409 单轮全链路多约 9.359s（3.8%）、publish 多 6.202s，不能据此归因安全
校验开销或宣称提速；未清 OS cache、未测 UI 首屏，mapping/acceptance 等仍待优化。
下一步按 v375 实现真实 B3/B4，不启用半成品发布或替换唯一预览。
恢复 metadata 当前为独立 format3；未发布 recovery2 缺少文件绑定时保留并 defer，
不是 association receipt2/proof2。UUID/witness 改为同快照 rowid/type→只读 BLOB
字节长度→有界 UTF-8，错误类型/长度/编码不视为缺失。仍不声称 same-inode
原地回滚字节可检测。以下 N407/N408 为历史绿门，不绑定当前新实现。
恢复 intent 原子写入并刷盘后才增加 retained bit，绑定完整 job/input/preview
和 immutable lease 摘要。新 catalog UUID 与同一 job/input SQL COMMIT 的摘要
witness 防止外国空 catalog 或残缺 SQL 行被误当成拒绝。它不是 reader grant，
也不是 association proof2；旧 v1 archive/普通 input2 容量不变，input3 发布仍拒绝。
恢复保留最新 TTL/batch；未知、未来、重叠或 I/O 未决 metadata 保留并显式 defer。
损坏任务完成后的 GC 不阻断后续健康任务；显式 cleanup 仍报告未决并保留坏快照。

实际 red/green：N404 全库为 4046 passed/2 failed/18 ignored、exit101；两项 queue
回归在 N405 的 43 项专项中转绿。N406 真实 accepted child 暴露空 catalog 替换
误释放（43 passed/1 failed），原失败证据保留。N407 修复后 50 项专项全绿，
涵盖 catalog epoch/witness 损坏、两次 lease 写入间崩溃、竞争 accepted owner
和重叠 receipt。N408 同 729 native inputs/EXE 完成 12-command 全绿门：103 项
集成专项、实际 1 项原 immutable seed、38 lifecycle、严格 all-target Clippy、
全库 4055 passed/0 failed/18 ignored（1117.45s）。这些集合重叠，不相加。
source/Git/index/EXE 稳定、26 seed hashes 不变，原 harness/full child 均已退出。
quality 40/40，App 编排 13244、千行文件 80 不增，未提高任何质量阈值。

N405/N407 各自空 temp 清理 0 logicalB，H 盘观测 delta 各 -4096B，不称为空间回收。
N408 全绿后 inventory→Execute 仅清 401/402/403/408 compiler-temp，380 files /
5246221 logicalB，观测 H 可用空间 +3112960B 单列；失败 399/404/406 temp 暂留。
全部日志、seed、WIP/依赖、当前 EXE/symbols、UI bundle 与唯一 V325 保留。
N409 用同一全绿输入完成实际 1000-publish（每会话 1 event），1 项测试通过。
全链路 244.390s、publish 161.984s、verification 26.770s；status/items 5/8ms，
mapping 1251ms、plan-inputs 8530ms、plan 4499ms、commit setup 30221ms。
terminal/catalog page 5/4ms，并发 job poll 峰值 574ms，峰值 RSS 48390144B。
这些是调试构建、fresh App Home，未清 OS cache、未测 UI 首屏、无同源码旧版
对照；不宣称无性能开销，mapping/acceptance 等阶段仍可优化。source/Git 稳定，
原 child 15196 已退出，runtime cleanup 完成：0 logicalB/observed H delta 0B，
logs/metrics 留存。729 输入的绿门是 N407/N408；后续 probe 不冒用它作当前证据。

同 UUID 旧 catalog 备份恢复的 N410 真实 child 负例已失败：50 passed/1 failed，
76.90s、EXE exit101，原 harness 已退出1。它证明旧备份会误退休 accepted recovery
intent；不是 fixture 或编译错误。fmt/compile/manifest 和 source/Git/index 稳定，
失败日志/temp 保留；尚未加产品修复。当前源码为 RED，N408 只绑定此前输入。
新 CASE/length
仅限界应用结果，不声称覆盖 SQLite 全部恶意大 cell 分配。先完成这两处边界复核，
再进入 v375 的 B3 accepted reader+B4 真 CAS、publisher/recovery、late producers、
Undo/fixed refs/grants 与 UI/15 locales。原 V5/contracts/53、vendor/live-WAL、ACP
四组合、100 round/UNKNOWN、Windows IME/DPI、macOS/Linux、容量与小 PR 范围不缩。
唯一固定入口仍 start-current-preview.cmd/V325，不含本批后台代码且未启动。
完整后台绿不等于整体验收或可交付；goal active。最新原始证据及下一步以外部
plans/v404-retention-recovery-current.md、v406-catalog-retention-binding.md 为准。

S199两main本地集成（2026-10-13）：已授权的WIP检查点及精确v387快照保留；
upstream9c5eb3b5通过本地merge a0883e7d合入，origin b58b97cd的8独有提交纳入
本轮普通三方合并。fork的5个文档冲突已解决；ARM64 CI/release、平台更新器/
manifest、xdg-utils、Node heap和Markdown安全回归保留。源头已发布章节不改写，
fork同名版本的两节原文独立归档在docs/releases/fork-2026-10-04.md，避免popup
重复版本键；README保留更新的贡献者记录。未push/PR/tag/release或改真实App数据。
N397724native inputs fmt/compile/Windows manifest及4context tests通过；N399
同输入/binary的103专项、1原immutable seed test、严格Clippy通过。实际全库
4035passed/3genuine failed/18ignored，EXEexit101、1155.78s，精确只有N385原三红；
wrapper核对预期失败集后exit0不等于产品全绿。26seed files/hash前后不变，源码/
Git/index/EXE稳定。F3982556frontend inputs：依赖卫生/typecheck/完整lint/quality
40/40、下载清单3tests、Markdown/notes27tests和全库883files/10324tests通过；
专项与全库重叠，不相加。F400同前端输入生产Vite构建通过（先前typecheck），
162输出文件/24459148logicalB，无App/安装包或preview promotion；大chunk与
mixed static/dynamic import警告仍待优化，没有调高阈值。App13244/千行80不增。
原397/398/399/400 owners均退出，无本批遗留App；唯一V325 CheckOnly通过、未
启动且不含本轮后台改动。397成功专项只清自己的空temp，0logicalB、H盘观测
delta -4096B单列（并发磁盘活动）；未作为full replacement去删旧EXE或失败证据。
N399失败temp377files/5242083B及旧失败证据暂留，当前EXE/162文件UI bundle/
seed/WIP/依赖/预览数据保留。完整原始日志与本地merge SHAs见外部集成audit。
两main同步不是会话导入交付；下一步按v386闭合三红的durable接受epoch恢复，
再按v375实现accepted CAS+旧reader、publisher/recovery、Undo/ref/grant和UI。
V5/contracts/53、vendor/live-WAL、ACP四组合、100round/UNKNOWN、三平台、IME/DPI、
容量、15locale及小PR完整范围不缩，goal active，不以本轮合成回归替代实机验收。

S198上游集成检查点（2026-10-13）：用户已明确授权仅本地WIP检查点及分步
集成，不push、不PR、不发版、不改应用数据。1086路径精确字节/index/两类
patch与hash快照在外部runs/v387-local-checkpoint；WIP98e3b524与clean-filter
规范化600584fa保留，工作文件字节不因规范化改变。固定upstream9c5eb3b5已
完成普通三方合并的22文件冲突处理；上下文控制/skills重扫迁入明确owner域，
composer本地键和上游PR-monitor键并存，新增状态不进入AppWorkbench。
V390依赖卫生/typecheck/相关lint/quality通过，8文件106tests通过；V394的
15locale目录42tests通过。N393的724native inputs fmt/compile/manifest及4
context tests通过；N396同binary完成91专项、1原immutable seed test、严格
Clippy，source/index/EXE稳定，26seed files前后hash相同。retention仍6passed/
3genuine failed（精确N385三项，EXEexit101），并非产品全绿或完整native门。
N388/N391/N392失败及N395缺seed环境变量的原日志保留，后续成功不覆盖历史。
App合计13244（较13294下降50）、千行80、quality40/40；未抬质量门。当前
只完成upstream输入验证、尚待本地merge commit；origin8独有提交下一步分开
集成。唯一V325预览仍不包含本轮后台改动，未启动App或创建包，不改私人数据。
accepted CAS+reader/publisher/recovery/B5/B6及原完整验收未完成，goal active。
完整集成状态以外部plans/2026-10-13-upstream-integration-audit.md的最新段为准。

S197诊断（2026-10-13）：真实 ordinary enqueue 的 crash/orphan 与 rollback-I/O
缺口已复现，尚未修复。只改2个既有测试模块、增加1个4-test模块，产品实现与
N383相同。N384原44093 exit1：5passed/1genuine failed；N385原66358 exit1：
6passed/3genuine failed/0ignored，fmt/compile/manifest通过，均719inputs/Git稳定。
3失败均为清理删除未接受且未过期的预览：COMMIT前子进程退出、同candidate的
其他已完成任务、真实SQL失败后Windows lease rollback I/O失败。COMMIT后退出的
真实accepted queued owner被保留并正常worker完成（新正例），原5controls仍绿；
不是手插accepted行或association正例。错误来自jobs.rs把无retryable引用的
retained bit一概当completed；无引用不能独立证明完整owner。需原接受epoch的
durable恢复证据，保持已接受任务/drop安全，不按title/candidate/boolean猜归属。
fresh719native/2529frontend/26seed drift0，quality40/40、App13294/千行80、owners0。
N384/N385空temp0logicalB按失败replacement策略暂留，日志/当前EXE/seed/WIP保留。
共享EXE已relink，不再是N383的binary；未跑当前全库/Clippy/seed，不冒称全绿。
soleV325未改，无App/包/私人数据/provider/安全修改/commit/push/PR。上游checkpoint
授权仍待答复，未集成；真实accepted CAS+reader/publisher/recovery/B5/B6及原全部
范围继续。下一细节与证据见外部plans/v386-retention-crash-reconciliation.md和N385
closeout；这不是crash recovery交付，goal active。

S196（2026-10-13）：association stored_job读取前限界已通过当前完整回归，仍不是
B3 accepted reader/B4 CAS或关联交付。N380实际5红（63passed/5failed/1ignored）
覆盖SQL整列分配、重复items被隐藏、未知字段、createdAt漂移和超限错误分类；
原失败日志保留。独立job_association_header先rowid/typeof、只读BLOB长度，
同snapshot流式strict header；复用shared HeaderReader的key/scalar/depth保护。
原job16MiB cap/普通input2容量/v1 bytes不变，requestId不另加小限额。duplicate/
missing/unknown/wrong-type/SQL-header漂移拒绝；mutable items/progress/cursor
只忽略投影，非authority，不证明全部正文语义，不读source/preview或改stored bytes。
N381原29471 exit0，同718inputs的71专项passed/0failed/1ignored（含9新实际tests）
及fmt/compile/manifest通过；quality40/40、App13294/千行80不增，五native路径
LF/noBOM。N382原21194 exit0，同718inputs旧seed实际1test通过，28B1/22plan/
54input/91draft仍为子检查；旧archive及same-request retry不变。两个空temp在
owner退出后inventory→Execute清0logicalB，observed H delta0/-4096B分别记录。
N383原session31064 exit0，六门完整通过：71专项/1058领域/3987全库passed，0failed；
1/14/16ignored不计通过，集合重叠；fmt/compile/manifest/严格Clippy通过。
全库864.8905384s；fresh718native/2529F344frontend-owned/26seed drift0，N382/N383
输入相同，HEAD/index不变、original/task owners0、quality40/40、App13294/千行80。
原owner退出后inventory→Execute清自身765files/10483322logicalB，observed H
delta+11440128B单列；N380空失败temp清0logicalB/observed delta+4096B。
日志/源码/WIP/seed/deps/current EXE/symbols/soleV325/data保留；N363非空失败证据
仍待逐项分类，不删除其他聊天产物。固定入口CheckOnly通过/no launch。
较N379只有3既有native路径变化、2新增；无App/新包/私人provider/安全修改/commit/push/PR。
两main再次fetch成功，refs仍为upstream9c5eb3b5/originb58b97cd，分叉35/8；源头
较HEAD领先28提交、246变化路径中79与当前1085唯一WIP路径重叠，尚未集成。
本地WIP checkpoint授权仍待答复，不自动commit/stash/换worktree。后续真实accepted CAS+
reader、crash/orphan/rollback-I/O、publisher及UUID旧producer、Undo/ref/grant和
UI仍按v375/v380；原V5/contracts/53及完整平台/provider/容量/15locale不缩。
证据：外部N383 terminal/fresh-closeout/两个cleanup JSON/closeout，N380–382原记录，
plans/v380-bounded-association-job-header.md；完整后台回归不等于整体验收或交付。

S195（2026-10-13）：完成普通入队的 retention 失败回滚前置修复，不是 B4
关联接受/发布交付。N376 用真实 rollback-journal 读锁复现最终 SQL COMMIT
失败后未接受预览仍被占用（1passed/1failed）。当前入队在 detached preflight
后、publication/writer 前取得进程及 OS acceptance owner，持有到 SQL 回滚及
lease 修复结束；仅恢复本次 false→true 的 retention，核对不可变身份并保留
更新后的 expiry/batch，不清其他已接受任务。内存 retention 仅随成功 COMMIT。
N377 五项实际专项通过：提交失败同 request 重试、已有 owner、新 expiry/batch、
第二 lease Windows rename 失败和跨进程 owner busy/退出后重试。N378 同716inputs
的旧 seed 实际1test通过，保留28B1/22plan/54input/91draft子检查，不算195tests。
N379 原95857 exit0，完整六门通过：5专项/1049领域/3978全库passed，0failed；
0/14/16ignored不计通过、集合重叠。fmt/compile/manifest/Clippy通过，全库889.189s。
fresh716native/2529F344frontend-owned/26seed drift0，N378/N379输入相同；HEAD/index
不变，owners0，quality40/40、App13294/千行80不增长，六个修改native文件LF/noBOM。
退出后精确 inventory→Execute 清自身770files/10486224logicalB，observed H
delta+11431936B单列；另清N376空temp0B/observed delta0。原失败日志、源码/WIP、
seed、依赖/current EXE/soleV325/data保留；N363非空失败证据暂留，其他聊天不动。
没有新App/包、私人数据/provider、安全设置、commit/push/PR。固定入口CheckOnly
通过/no launch；V325不包含最新后台切片。crash/orphan与rollback-I/O修复失败
恢复仍未证明，失败修复保留保护并报告deferred，不能称重启恢复已交付。
两main refs已fetch、尚未合入；当前1083唯一WIP路径。upstream/origin分叉35/8，
79路径上游交叠审计不是实际merge冲突。未收到本地WIP checkpoint授权，不自动
commit/stash/换worktree。下一计划仍为外部v375：先有界精确job header、真实
accepted CAS+旧history reader，再publisher/recovery、Undo/ref/grant及UI。
原V5/contracts/53、厂商-WAL/ACP四组合/100round/三平台/IME-DPI/容量/15locale/
小PR范围不缩；goal active，整体验收未完成。证据：N379 terminal/fresh-closeout/
两个cleanup JSON/closeout，N376 red与N377/N378原日志。

S194（2026-10-13）：完成registration_input保留原16MiB硬限界、strict proof1/2
协议分发、共享InspectionWitness及未接受proof2拒绝；不是B4接受/发布或UI交付。
v1真实接受记录的flat JSON字节/digest不变，store marker1/input3/receipt2独立。
共享witness保留operation guard，不带SQLite事务、不放idle缓存，不是grant；
test-only draft绑定实际plan/job/input与原确认，不作为产品接受producer。
N368读取真实红已修；N369漏跑worker是测试前提错误；N370真实复现matching-current
身份绕过新版proof，已修。N371新模块import/字段引用编译失败未到断言，修正后
N37262专项通过；清未使用import并补类型负例后N373同714inputs固定旧seed实际
1test通过：原28B1/22plan/54input及91proof-draft子检查，不算195独立tests。
N374原68751 exit0、完整六门通过：62专项/1044领域/3973全库passed、0failed；
1/14/16ignored不计通过、集合重叠；fmt/compile/manifest/Clippy通过，全库849.155s。
fresh714native/2529F344frontend-owned/26seed drift0、N373/N374输入相同；HEAD/index
不动、owners0、quality40/40、千行80/80、App13294不增长。唯一V325/data不改，
plain cmd CheckOnly通过/no launch；无新App/包、私人数据/provider、安全设置、
提交/push/PR。退出原owners后inventory→Execute清763files/10484006logicalB，
observed H delta+11460608B；N372/N373及N368–371空temp各0B，观测量单独记录。
源码/日志/seed/依赖/current EXE保留；N363非空失败证据继续待分类，其他聊天不动。
用户要求合适时更新代码：origin/main与upstream/main现已fetch成功；首轮upstream
TLS失败以本次命令OpenSSL后端重试恢复，验证证书、未改全局Git/代理。源头9c5eb3b5
较当前HEAD领先28提交，246变化路径中79与1081 WIP路径重叠；origin有8/7分叉、
113变化路径中50重叠。未merge/checkout/stash；抓取不等于已合入开发代码。
下一步先安排安全上游集成检查点，再按外部plans/v375-accepted-association-next-slice.md
闭合B3 reader/B4真实accepted CAS-publisher-recovery，随后B5 Undo/ref/grant和B6 UI。
receipt等SQL CAST仍未全面限界；原V5/contracts/53、厂商-WAL、ACP四组合/100round/
三平台/IME-DPI/容量/15locale/小PR范围不缩，goal active，整体验收未完成。
证据：外部N374 terminal/fresh-closeout/cleanup/closeout、N373 seed、N368–372原日志。

S193（2026-10-13）：完成 v359 计划第0节的读取前置修复，不是 Proofv2/
accepted association/UI。worker 大尺寸 input3 在同一只读 snapshot 内先取
rowid、BLOB 元数据和有界流式 header，再决定是否读取正文；普通 input2 的
原容量/字节语义保持。preview retention 只流式投影路径，不是授权；损坏引用
拒绝清理，不能当空引用删除。proof 点读用 rowid/typeof 和只读 BLOB 长度，
存在但超限/损坏的行明确拒绝，缺表/缺行才是 None；schema1/v1 不放宽。
N359 两个真实产品红、N360 cleanup 整行读取红已修；N359 另有 JSON 字段顺序
测试前提错误，其日志收尾中断且没有原 terminal，不伪造完整通过。N363 是
明确诊断中断，不计 full pass。N364 58passed/1failed 是 EXPLAIN P2 索引错误；
两个查询计划测试修正并加整列读取对照，N365 59passed/0failed/1ignored。
N366 固定旧 seed 实际1test通过，28 B1+22 plan+54 input 子检查不算104独立tests。
N367 原 session3862 exit0，711 inputs 完整六门通过：59专项、1041领域、
3970全库passed/0failed，各1/14/16ignored不计通过，测试集合重叠；fmt/compile/
manifest/Clippy通过。fresh711native/2529F344frontend-owned/26seed drift0，
N366/N367输入完全相同，HEAD/index不动，owners0；quality40/40，千行80/80、
App13294不增长。原owner退出后inventory→Execute清759files/10482846logicalB
（observed H delta+10924032B）、两个空temp0B（+8192B），以及旧8a测试EXE/PDB/
depfile三文件839238192logicalB（+839237632B）和其空temp；观测量分列，不当作
独占物理回收。N363非空205files/3175537B暂留分类；完整失败日志/源码/WIP/
依赖/当前EXE/唯一V325/data保留，未停止其他聊天进程。入口plain cmd CheckOnly
exit0/hash有效/no launch；无新App/包、私人数据/provider、安全修改或远端发布。
下一切片见外部plans/v368-proof-protocol-next-slice.md：保留registration_input
原16MiB cap并复用读取前限界，然后strict Proofv2/共享witness/reader；原v359
后续B4原子accepted CAS/publisher/recovery、B5 Undo/ref/grant、B6 UI继续。
receipt等其他SQL CAST读取未在本批全面审计，不能称全应用已限界。原V5/contracts/
53、厂商-WAL、ACP四组合/100round/三平台/IME-DPI/容量/15locale/小PR范围不缩，
goal active，本轮合成后台结果不是整体验收或交付。
证据：外部N367 terminal/fresh-closeout/三个cleanup JSON/closeout，N366 seed，
N359 interrupted-log-closeout，N363 diagnostic-interruption，N364/N365 terminal。

S192（2026-10-13）：B2b 的实际 plan → CommitItem → durable input v3 传递、
严格版本/字段解析、raw 选择绑定和重启 metadata 校验已实现。普通 input v2
字节保持兼容，receipt 仍为 2；关联意图不是 grant，所有未完成的 enqueue/
publisher/worker/committed recovery 通道仍拒绝。没有开放关联提交或 UI。
N355 编译后真实复现丢确认/选择顺序/序列化意图；N354/N356 是未进入断言的
编译前提失败，日志都保留。N357 旧 seed 实际 1 test 通过，保留 28 B1、22
plan 并新增 52 input 子检查，不是 102 个独立 Rust tests。
N358 原 session95649 exit0，709 输入完整六门通过：51 专项、1033 领域、
3962 全库 passed，0 failed；各 1/14/16 ignored 不计通过，测试集合重叠。
fmt/compile/manifest/Clippy 通过，full 797.637 秒。fresh 709 native、2529
F344 frontend-owned、26 immutable seed drift0；N357/N358 输入完全相同，
HEAD/index 不动、task owners0。质量 40/40、千行 80/80、App13294 不增长。
原 owner 退出后 inventory→Execute 清自身 755 文件/10483161 logicalB 和
四个旧失败空 temp/0B；observed H delta +11436032/+4096B 分列，不能当作
独占物理回收量。日志/存档/依赖/当前 EXE/WIP 保留，没有停止其他聊天进程。
唯一 V325/data 不变，plain cmd CheckOnly exit0/hash 有效/no launch；该包
不含最新 B1/B2a/B2b。没有新 App/包、真实 provider、安全设置或远端发布。
下一前置修复：worker 大尺寸 v3 在 materialize 前限界、proof 超限行不能当
缺失；随后 B3 strict Proofv2/reader → B4 accepted CAS/publisher/recovery →
B5 Undo/ref/grant → B6 UI。详细步骤：外部 plans/v359-proof-v2-reader.md。
原 53/厂商-WAL/ACP 四组合/100round/三平台/IME-DPI/容量/15locale/小PR
范围不缩；这些合成后台结果不是整体验收或交付。goal active。
证据：外部 N358 closeout/terminal/fresh-closeout/两个 cleanup JSON、N357 seed。

S191（2026-10-13）：完成B2b前置的行为字段拒绝修复，不是input v3/关联发布。
N351真实回归：fmt/compile通过，46passed/1failed/1ignored；旧v2输入会忽略
options.identityAssociation（包括null）。共享guard现在拒绝options中的单/复数
关联字段，覆盖plan加载、CommitItem入队、durable shape和direct publisher，
在普通writer/retention/项目副作用前拒绝；worker重启不会静默独立发布。
N352当前706inputs的fmt/compile/manifest和50专项passed/0failed/1ignored；
N353同706inputs旧seed实际1test/0failed/0ignored（仍28B1+22plan子检查），
额外Clippy all-targets -D warnings exit0/51.126秒；不冒称新全领域/全库重跑。
quality40/40、千行80/80、App13294不增；fresh706native/2529frontend/26seed
drift0、HEAD/index不动、task owners0。普通input/receipt版本仍2，v1授权不改；
唯一V325/data不改，plain cmd CheckOnly通过/no launch，没有新App/包/发布。
退出owners后inventory→Execute清N352/N353空temp：0logicalB；observed H
delta0/-1495040B分列，不称实际独占回收。N351空失败temp0B按full-replacement
策略暂留，所有失败日志/seed/依赖/current EXE/WIP保留；其他聊天不动。
下一完整B2b步骤见外部plans/v354-durable-association-input.md；完整input v3、
Proofv2、accepted CAS/publisher、Undo/ref/grant和UI仍待做，原53/厂商-WAL/
ACP四组合/100round/三平台/IME-DPI/容量/15locale/小PR范围不缩。goal active。
证据：外部N353 closeout/fresh-closeout/supplemental-clippy和N351/N352 terminal。

S190（2026-10-13）：B2a严格Confirmation/durable plan v2已验证，不接UI/身份
变更。v1夹带关联字段（含null/空数组/options）拒绝，普通payload不变；v2保留
raw选择并绑定planId/created/expiry/confirmation。失效确认在lease/plan写入前
拒绝；新关联plan拒绝普通commit，已接受request查同plan/job的顺序不动。
N345两个真实红保留；N348原50252 exit0/full六门passed：46专项/1028领域/
3957全库passed、fmt/compile/manifest/Clippy通过；1/14/16ignored不算通过。
N348后只拆纯test模块：4完整测试正文逐字不变，父1158→941、子222；仅2条
test路径与N348不同，产品实现hash全不变。N349当前705inputs的46专项、fmt/
compile/manifest及额外Clippy49.30s全过；质量40/40，千行文件80/80，不改上限。
N350同705输入旧seed实际1test通过，保留28B1+22plan子检查，不是50个独立test。
不冒称705输入重跑全库。fresh705native/2529frontend/26seed drift0，Git与App13294
不动，task owners0。F344全前端10176/0failed/0pending已过；唯一V325/data不动。
清F344 tmp1312files/11216160logicalB、N348 temp758files/10482728logicalB；
observed H delta+14475264/+11452416B分列，不归因成独占回收。其余自身空temp
清0B；旧失败日志、bundle/EXE/依赖/seed/WIP保留。无App/新包/真实provider/安全
修改/发布/agents。B2b input→Proof v2→accepted publication→Undo/ref→UI仍待做，
原53/厂商-WAL/ACP/三平台/IME-DPI/容量/15locale/小PR不缩；goal active。
证据：外部N350 closeout，历史身份设计第9节是下一步。

S189 证据补正：同轮也新增前端只读 inspect wrapper、9 个测试与公共 export，
不是“没有改前端”。N343 仅覆盖 native；F344 原37117 exit0，types/全仓lint/
UI build 全过，10176测试/0failed/0pending（含9wrapper），3232 inputs/Git稳定。
原owner退出后清自身tmp1312files/11216160logicalB，observed H delta+14475264B
另列，不归因成独占回收量。bundle/日志/依赖/V325/data保留。B2a仍实施中。

最新 S189（2026-10-13）：显式身份关联只读domain与异步Tauri检查已实现，未接
UI/关联plan-job/Proofv2/提交。N342既有V243合成archive正常路径与28个故障/
摘要子检查通过（1实际test/0failed/0ignored）；逐次catalog schema/rows与lease
hash不变，旧历史/普通独立发布/同request重试保持。source-target同snapshot，
登记alias绑定digest，既存proof含超限行拒绝；v1授权不放宽。N337alias和
N340默认WAL是错误测试前提，失败保留，只修隔离fixture，产品日志模式不改。
N341专项38/领域1020passed，但Clippy接口dead-code失败，未执行全库；原失败
保留。N342注册spawn_blocking只读命令后同seed验证通过，command/domain错误
一致；不加allow(dead_code)。N343完整六门原54361 exit0/terminal.passed=true：
39专项、1021领域、3950全库passed/0failed，fmt/compile/manifest/Clippy通过。
1/14/16ignored不算通过；native门是合成后端验证，不是整体验收。
fresh701native/26seed drift0，N342与N343输入集合完全相同，Git不动，原task
App/build/test owners0；V325入口CheckOnly exit0/hash有效/no launch，data不改。
自身4成功空temp清0logicalB，observed H deltas各-4096B另列，不称独占回收。
B2确认-plan/input→B3Proofv2→B4accepted-publisher→B5Undo/ref→B6UI详细步骤
见2026-10-13-session-import-historical-identity-association.md。原53/厂商-WAL/
ACP/三平台/IME-DPI/容量/15locale/小PR不缩；不发布、不派agents。
N343自身755files/10483173logicalB及7个旧temp370files/5234072logicalB已
inventory→Execute清理，observed H delta+11436032/+5701632B分列，旧日志/
失败证据/seed/当前EXE/唯一V325/源码/WIP/依赖保留。其他聊天与不明owner
不动。证据：外部N343closeout/terminal及两cleanup JSON，N342外部seed日志。

此前 S188：N332旧V243/normalizer1.0.0合成archive实际通过当前
strict reader→原UUID目录scan→preview→durable plan→runtime reset→enqueue→
worker publish→reload与同request重试，1passed/0failed/0ignored。旧entry/job/
5receipt/不可变文件不改，两边各2消息；新1.0.6 scoped identity独立发布，
无显式关联时不自动合并。关联功能仍未实现，设计见
2026-10-13-session-import-historical-identity-association.md。
N33332旧兼容专项passed/0failed/1ignored（该外部seed测试已在N332执行），
Clippy all-targets -D warnings exit0/49.637s；697native fresh drift0，较N328
仅测试模块链接/新增测试改变，没有产品实现变化，未重跑全领域/全库。
原seed26 drift0/Git稳定，App/build/test owners0；未launch桌面/新包。
V325唯一入口/data不改，CheckOnly exit0/hash有效/不launch。严格清自身两成功
空temp0logicalB，observed H delta-4096/0B另列；3失败空temp0B按full-replacement
策略暂留，原失败日志保留。原53/契约/真实vendor-WAL/ACP/三平台/IME-DPI/
容量/15语言/小PR仍开；没有private/provider/product App commit-Undo/安全设置/
发布/agents。证据：外部N332closeout.md与N333测试/Clippy日志。

此前 S187：唯一start-current-preview.cmd已提升V3256f79ce5d，
SHA45d9677f；原数据路径不动，plain cmd -CheckOnly exit0/hash正确/未launch。
原build48089 exit0/112.4755秒，3275source/162assets/Git稳定；准确复用F312
2527未变前端输入。N328当前原生六门32专项/1014领域/3943全库及Clippy通过，
696native fresh drift0；重叠不相加、13/15ignored不算验收，锁修复已进当前包。
限定中文1282×802实机通过旧历史2消息只读、A/B四位置/缺尾/93字节完整字段、
采用不自动勾选、手动B prepare1/0/0、关闭重开/Back/Refresh保持单项选择、
深浅色与Escape可见比较按钮focus ring。UIA只报告RootWebArea，不冒称其焦点
身份通过。没有产品commit/Undo/send。原65241/退出78474 exit0；App26120与
捕获8进程正常Quit/family0，seed26/archive6/source4 drift0，entries1/jobs1/
receipts5不变。严格清自身WebView262files/13916246logicalB（observed H
delta14147584B）和被替代V313包1file/94038528logicalB（delta94035968B另列）。
源码/WIP/deps/current包/seed/失败记录保留，无其他聊天进程操作。
旧Cursor源仅metadata观察：原UUID保留，当前scoped transcript-locator与旧
1.0.0 bare transcript不同，未prepare/adopt/commit；不等同真实历史身份迁移。
继续隔离domain旧seed replay/显式关联设计；原53/契约/原厂WAL/真实ACP/三平台/
IME-DPI/当前容量/15语言/小PR证据仍开，不缩范围、不发布、不派agents、不用私源。
证据：外部runs/windows-preview-v325-6f79ce5d7d944f97996be2ca34de47a0的
desktop-observations.md、prepare-inspection.json、desktop-smoke.json、exit/cleanup。

此前 S186：N324原43710已exit0，27专项/1009领域/3938全库均0failed，
Clippy通过；695native/Git稳定、fresh drift0、owner45944/task App-build-test0。
领域13ignored/全库15ignored不算验收通过；重叠集合不相加。严格inventory→
Execute清自身752files/10482367logicalB，observed H delta10915840B另列；
EXE/日志/WIP/deps/seed/失败及中断证据保留。未启动桌面App/新包，V313不改。
N326 5f50369f原6491已exit1：30专项28passed/2failed，实际复现最终拒绝前
旧producer创建孤立项目或把已保存映射version2覆盖成旧项目version1；正常对照
通过，非夹具错误。696native/Git稳定、owners全退，空temp0B/失败日志保留。
现按plans/v327-legacy-adoption-publication-lock.md只在实际身份adoption前取得
既有进程/OS publish owner（SQLite writer/store锁外），忙则SourceBusy、不等待/
不迁移/不建job/不变lease；同一plan/request显式retry。普通UUID入队/发布/Undo
契约与最终guard保留。N327 5697383e原63851已exit0：32专项0failed/0ignored，
fmt/compile/manifest通过，696native/Git稳定、owner74048退出，严格FocusedOnly
inventory→Execute清自身空temp0B。两合法顺序、实际进程/OS owner与retry均通过。
N328 a7ea2927原69945已exit0：32专项/1014领域/3943全库均0failed；
领域502.342秒、Clippy exit0/51.568秒、全库794.652秒。13/15ignored
不算验收通过，重叠集合不相加。696native/source/Git稳定、退出后fresh
drift0；owner31804/test68904/task owners0，EXE SHA835dc05f匹配原记录。
严格inventory→Execute清自身1559files/13147717logicalB，observed H
delta14217216B另列；仅compiler-temp，EXE/日志/seed/deps/WIP/失败证据保留。
一个V325候选所需6个strict guard仅允许准确版本，去重后parse0errors/
LF-noBOM；真实AST边界允许325、拒绝326及路径逃逸。尚未build/launch。
F312前端2527inputs fresh drift0，162assets与V313嵌入记录完全一致；
可复用的只是前端subset，不代表旧native或完整source manifest当前一致。
唯一候选windows-preview-v325-6f79ce5d7d944f97996be2ca34de47a0已开始build，
原48089/owner29104（08:03:05.942+08:00）；尚待build终态，不启动第二包。
唯一V313入口/data路径未改，没有桌面App；候选成功后做一次隔离mock cycle。
原53/契约/范围不缩，无private/provider/product App commit-Undo/security/publication。

此前 S183：N317原完整六门已通过并清自身758个临时文件，但其694输入
不是此后源码的全库证明。新增实际worker/注册边界专项N318、校正N319都
19passed/4failed；N319确认3项真实红例：带adapter前缀的普通/verbatim路径
冲突被接受，旧queued producer在adoption后及Undo后都生成第二条legacy entry。
另外N318前提/错误码预期及N319规划续期前捕获lease的时序错误已纠正，失败原证据保留。
现sources只对已知5个adapter路径前缀做至多3种精确拼写，不匹配其他family/
opaque ID；publisher在分配receipt前及最终事务内点查旧registration/entry
ownership，失主旧任务含explicit reimport均PlanStale；新UUID来源及旧committed
lost-ACK恢复路径不改。归档/receipt/ID不改写，无新增任意路径/完成系统。
N320因新测试把error String当对象编译失败，断言未执行；已改用既有error parser。
N321 caa4ac40原91109已exit0：fmt/compile/manifest及26专项全绿（23.057秒），
695native/Git稳定、owner22184退出；严格inventory/Execute清自身空temp（0B）。
N322新增第27项因相同options合法dedup未到第二probe而失败：26passed/1failed，
是夹具前提错误；已改真实rawRetention选项并断言digest不同，不改产品dedup。
N323原85039诊断中断exit1：静态发现新测试在发布锁内调用Undo会重入同一锁，
其第27项尚未执行，不当产品死锁。只中断精确测试owner65852、finally完成，
原owners全退；4files/249980logicalB中断证据保留。Undo已移到publisher返回后。
N324 85869361原43710已exit0：27专项/1009领域/3938全库均0failed及Clippy通过，
695native稳定、fresh drift0/Git不改、owners全退。完整结果仅绑定这695输入，
不代替后续项目副作用测试或真实原厂/ACP/平台验收；一次桌面replacement仍待。
F312的2527非src-tauri frontend-owned输入fresh drift0；旧完整manifest不复用。
唯一V313预览不提升、未含本轮native修复；无桌面App/新包/私源/provider/
产品App commit/Undo/安全设置/发布。HEAD/index/App13294/原V5/contracts/53不缩。
具体设计与失败证据见validation/plans/v319-legacy-producer-ownership.md。

此前 S182：S180/S181的原子注册入队及精确历史凭据关联保留，旧归档/
receipt不改写，reader/recovery严格授权不放宽。N316原12734实际exit1：
16项legacy专项通过；领域994passed/4failed/13ignored，Clippy/full未执行。
4项capacity失败共用未注册的占位sourceId夹具；现已用sources::register
登记一次canonical root/UUID，全部原分页/取消/离线恢复/crash/reader断言保留。
N317 e8fa7707原82163实际exit0：六门全绿，fmt/compile/manifest通过；
容量专项12passed/0failed/4ignored、领域998passed/0failed/13ignored（487.216秒）、
Clippy exit0（53.174秒）、全库3927passed/0failed/15ignored（783.551秒）。
专项/领域/全库重叠不相加，ignored不算通过。退出后fresh694native drift0，
HEAD/index/App13294不变，原owner28792及task App/build/test owners全部退出。
F312旧3220manifest中11个已记录Rust路径变更，另3个新增native文件未记录；
只有2527个frontend-owned输入fresh drift0，不能冒称旧全manifest当前一致。
唯一V313入口CheckOnly exit0/hash正确/未launch，尚不含本轮native修复。
本轮无桌面App/新包/私源/provider/产品App commit/Undo/安全动作/发布。
严格inventory后Execute清自身compiler-temp758files/10483047logicalB，
observed H free delta+11472896B单列，不当独占物理回收；精确路径/hash/再生
见本轮native-gate-cleanup.json。当前EXE/日志/源码/WIP/依赖/seed保留。
失败N316原owners已退出；temp840entries/370files/5234072logicalB仍保留分类，
未用成功门清理器绕过。用户确认无安全弹窗，不据此改安全设置。原53不缩。
下一按plans/v318-capacity-registration-repair.md补跨locator、foreign preflight、
retention/workspace/rebuild及
真实旧queued producer晚到proof，再单一replacement桌面验证后提升预览。
真实原厂/旧normalizer/WAL/ACP/IME-DPI/macOS/Linux等原V5余项仍待验收。

此前 S180：入队原子性修复已实施。N315 d51ecac4 原32854实际exit1，fmt/
compile/manifest通过，5项专项4passed/1failed/0ignored；后项snapshot拒绝、
job_items存储失败均回滚身份迁移，正常retry/dedup及collision对照通过。剩余
legacy_registration_adoption_keeps_old_revision_readable仍红，旧归档hash不变。
692native inputs/Git稳定、退出后fresh drift0；owner8960退出，task App/build/test0。
compiler-temp为空0logicalB、失败日志/当前测试EXE保留，无材料清理或新桌面包。
本轮修改正式engine/enqueue/sources事务边界，不再是仅cfg(test)；尚未跑当前
domain/Clippy/full-library，不能复用N309宣称当前全门绿。唯一预览仍V313。
下一执行书外部plans/v316-legacy-receipt-provenance.md：精确历史凭据关联、锁外
preflight/短事务CAS，以及reader与recovery共同严格授权。原V5/契约/53/平台等
完整余项不缩，不发布；HEAD/index/App13294保持。

本文件按 2026-10-08 当前工作树更新，不把历史计划中的“待做”或旧测试
数字直接视为现状。完整范围仍以 V5 执行书、原始契约及 53 项验收为准。
源码/WIP 保留，不提交、不推送、不提 PR。

最新S179：V314最终56f4b1f9原41441 exit1，fmt/compile/manifest通过，实际
2正常对照passed/2真实红例failed。旧local注册迁移后旧revision因catalog/receipt
身份不一致拒读，归档hash不变；后项prepared hash在enqueue前被拒、无新job
且表计数不变，前项身份却已迁移、restart仍保持。前两轮fixture/前提错误均
保留，不计产品bug。692inputs/source-Git稳定、fresh drift0；三原owners退出，
task build/test/App0，三个temp均空/0logicalB、失败logs/当前测试EXE保留，无新
桌面包或本轮材料回收。新增cfg(test)及门禁focus，未改产品行为。当前schemas/
normalizer1.0.6历史注册layout不冒称旧normalizer/原厂。V313入口不变，原全门
仅代表各自钉选输入，不覆盖新测试。下一外部plans/v315-registration-compatibility-repair.md
先精确旧凭据关联与短enqueue原子边界，再两个红例转绿；V311旧stem关联另待。
原V5/契约/53及平台/真实ACP等余项不缩，HEAD/index/App13294不改、不发布。

此前S178：F312原22660已exit0，10167passed/0failed/0pending，types/whole lint/
UI build通过，3220inputs/Git稳定、fresh drift0、owner已退出。V31324fa3a33
原52101 embedded exit0/122.9653549秒，3270inputs/162assets/Git稳定，复用F312。
N309691native inputs fresh drift0，只复用其未变输入，不虚构新全库。中文synthetic
实际A/B采用/dirty取消确认/手动native prepare exact B4events及原故障路径完成
prepare→close→Light→reopen→Back现恢复2逻辑行、B选择保留，Refresh/再重开正常；
浅深主题与比较Escape焦点返回通过。窄窗未确认尺寸、真IME/系统DPI等未验收。
guard20281/helper45489 exit0，App68988及8所属进程正常Quit归零、无force kill；
旧6archive/26seed/3sources hashes不变，无产品commit/Undo/私源/provider/安全动作。
唯一入口已提升V313，plain cmd -CheckOnly/hash/args校验通过且未launch，用户
数据路径不改。6旧EXE243/244/245/246/284/288已精确退役；V310 FALSE失败包、
V241旧proof不全包与未知owner仍保留。临时1675files+6EXE清603091751logicalB，
observed H delta606339072B另列；日志/源码/WIP/deps/seed保留，有精确再生记录。
HEAD/index/App13294不改，不发布；预览可用不等于原V5/契约/53整体交付。下一
外部plans/v311-legacy-session-identity-discovery.md先证明旧normalizer身份缺口，
已有legacy local注册迁移和workspace adoption不重复实现；所有原厂/WAL/ACP/
Windows真IME-DPI/原macOS-Linux/容量/小PR依赖原余项保持。

此前S177：实际Host/controller正式red afdcc2fa复现V310返回空组列表；新增2个晚到
read红例也修复。最终222f751e原53709 exit0：9聚焦/751相关（不累加）、types/
定向lint/quality通过，2526inputs/Git稳定、fresh drift0、owner退出。discard后
同scan只读reopen；cancel失败/关闭/新来源/unmount守卫，no-live-scan回Source不
隐式发现。缺scope Refresh恢复但不绕批次/adoption锁，失败scope只显式重试。
4文件LF/noBOM，App13294不增；native N309691manifest drift0。六轮cache清1748
files/15180236logicalB、observed H delta8560640B单列，红例/logs/WIP/deps保留。
当前真正full前端F312 7b31b450原22660仍运行，输入冻结；V313单一replacement
原故障实机/Quit family0前不提升V241。未将V310失败或focused勾成整体交付。

此前S176：V310 embedded原28840 exit0（122.850秒），source3268/frontend2525/Git
稳定，复用F308/162assets。中文synthetic桌面实际A/B采用、dirty cancel/confirm、
无自动勾选、手动prepare1成功及readonly exact B member/4events已验；但completed
prepare close/reopen/Back持续空组列表，Refresh无效，theme因果未证，smoke=false。
V241入口不变，V310保留失败证据、不退役旧包。guard78869/helper6609 exit0，
App44960及8所属进程全退、无force kill；旧6archive/26seed/3sources hashes不变。
本批四类缓存共2485files/59035784logicalB、observed H delta62119936B另列；
失败/拒绝/未知owner保留。先外部plans/v310-prepare-return-recovery.md红例→安全
reopen→新全前端/embedded/实机，之后V311旧身份。全V5/53余项不缩、不发布，goal active。

此前S175：F308原92397已exit0，全10158passed/0failed/0pending、types/whole
lint/UI build通过；3218输入/Git稳定、fresh drift0、原owner退出。N309原92229
完整六门exit0：14专项/982领域13ignored/3911全库15ignored，均0failed，fmt/
compile/manifest/Clippy通过；691输入/Git稳定、fresh drift0、原owners退出。
自身tmp清1312files/11216336logicalB、compiler-temp清755files/10483170logicalB；
observed H delta分别12771328/11436032B单列，保留ui-dist/当前EXE/日志/WIP/deps/
seed。单一V310 2e0b24a774824726a53483e3b9af5659原28840构建中，复用F308，
未launch App或提升V241。之后按plans/v310实机验收与正常Quit/family0；旧身份
后续plans/v311先验证真实缺口。全门通过不代表原厂/真实ACP/原平台或53整体。

此前S174（2026-10-13）：真实Host A/B采用UI已接上实际selection/groups/content
事务与prepare overrides。Host保有一个comparison/dirty consent/retry request，
精确owned ACK/read gap不卸载；独立A member、实际B member、原preview/adapter与
native summary授权采用。shared确认/同步lock、明确read-only Retry与uncertain/
stale/expired Compare again、窄preview reveal和15语言已实现；不自动勾选、
prepare/commit或继承grants。最终F307 0c8c92bd原68917 exit0：196聚焦/784相关
（有重叠不相加）、tsc/定向lint/quality通过，2524输入/Git稳定、fresh drift0、
PID52012已退出。30个关闭/晚到组合与真实Host prepare链是合成IPC证据，不是
native桌面/原厂/真实ACP/原平台或53整体。九轮精确cache已清2055files/
18590803logicalB，observed H delta22683648B单列；源码/日志/deps/seed保留。
F308全前端原92397/run ed31c2f42e814d78907129b25cf9e90b和N309完整原生原92229/
run1b8679de8e9e442e8e468585d419c9e6仍运行，未记录full终态；源码/config/lock/index
冻结，不重复跑。之后按外部plans/v310-current-source-delivery.md构建一次embedded
replacement，实机验收与正常Quit family0后才提升唯一入口。当前仍V241；无App/
私源/provider/产品commit/Undo/安全设置/发布，App13294行与HEAD/index原样。

此前S173（2026-10-13）：固定采用selection/groups/content实际hook事务接通，
合成IPC验证同步reserve、单次精确CAS、ACK/read分离、失败保留旧编辑、prepare
私有eligible projection、review-only+eligible合计200上限、exact binding撤销及
内容提交receipt。后读失败仅显式reconcile读取固定events与membership，不再
choose；丢ACK uncertain、superseded/expired stale需重比。取消pending命令精确
tombstone保留到原ACK终态；source/wizard/unmount失效，prepare批次锁仍有效。
最终F306 7dbbce17原12528 exit0：179聚焦（143旧+36新）/326相关、tsc/定向
lint/quality五门通过，2521输入/source-Git稳定、fresh drift0、owners0。
六轮cache清4044files/32007856logicalB，observed H delta39116800B单列。
7检查源码LF/无BOM、限定diff-check0，App21+13273不增，HEAD/index不变。
V241 CheckOnly/hash通过且未launch，没有App/私源/provider/Undo/安全设置/发布。
Host采用UI未接，不是全前端/native/桌面/原厂/53整体交付。下一执行书
外部plans/v307-fixed-adoption-ui.md，原V5契约/53项和原平台余项均不缩范围。

此前S172（2026-10-13）：groups.chooseAcknowledged区分已校验detached ACK与
后续coherent membership-read，原choose boolean兼容；失败/重置不CAS回退或重发，
外部writer推进后保留原ACK身份，采用方仍须验证最新精确descriptor/member/revision。
F305最终c4991d92原4395 exit0，96聚焦（含旧78+新18）/218相关、tsc/定向lint/
quality40通过，2517输入/Git稳定、fresh drift0，owners退出。正式red78passed/
1failed缺入口已转绿；中途2个新mock choiceVersion误置0失败保留并修DTO。
本轮V304/V305合计清2796files/22464857logicalB，observed H delta29442048B另列。
未完成selection reservation/Host transaction/真实A-B采用UI或最终验收；下一
plans/v306-fixed-adoption-transaction.md，不再将旧boolean当唯一ACK事实。
10检查TS/TSX UTF8无BOM/LF；App21+13273不增，原53ID保持。V241 CheckOnly/hash
通过且未launch；没有App/私源/provider/Undo/安全设置/发布，HEAD/index原样。

此前S171：固定内容transfer域入口与Host完整scope已接。只读精确
preview/leaf首20条events，捕获身份/确认并同步reserve，独立authority通过且owner
有效才提交fresh默认选择；失败保留原编辑，旧page/迟到success/error/finally不会
复活。143聚焦（含旧46）/218相关回归、tsc/定向lint/quality40五门通过，run
v304-fixed-content-4a4a75a9986d48fdae8c07607065dc73原16517 exit0，2516输入/Git
稳定。authority尚未接真实CAS/selection reservation/UI，不称固定采用端到端完成。
首例red+成功run清1348files/10668649logicalB，observed H delta14028800B单列；
PS5错误在snapshot/owner前未跑断言，pwsh7实际五门通过，无owner空temp留作证据。
本轮未启动App/使用私源/provider，唯一V241不提升，不发布。下一ACK+membership
状态分离→selection事务/Host→明确A/B采用UI→当前全门与单一replacement桌面。

此前S170：精确A member async命令/独立metadata owner/真实dialog
正式接线通过。N303原62556 `v303-title-d4a689d5569d4c08afe287f4f5c4e489`
exit0，fmt/compile/manifest/120聚焦passed，691输入/Git稳定、fresh drift0、
owners0；BaselineOnly没有新领域/Clippy/full。F302最终02e48836原34609exit0，
56聚焦/61相关、tsc/定向lint/quality40全部通过，2510输入/Git稳定、drift0。
正式test-first分页漏A已转绿；保留两次测试fixture/查询失误的失败日志。
5个新增native test实际验证离线精确member、选择/schema/refs/来源字节不改、
跨组/scan/epoch/缺失/路径ID拒绝、group/pin篡改与旧schema/byte bounds。
F302四轮cache清1549files/12927494logicalB、observed H delta12513280B另列，
N303空compiler-temp清0files/0B/observed0B。没有桌面App，唯一V241 CheckOnly
仍可用；固定preview采用、旧身份与原53项余项仍待。下一按plans/v304。

此前S169：N301原2629已exit0，完整六门14聚焦/977领域13ignored/
3906全库15ignored、fmt/compile/manifest/Clippy均通过，690输入与Git稳定；
不是其后新增point member切片的full证明。原owners退出，自身temp清763files/
10486077logicalB，observed H delta11452416B另列。精确A member async只读命令/
独立metadata owner/真实dialog已接线：不依赖lazy B页，不 fallback/repreview。
实际F302 test-first分页漏A失败已复现并修，当前门结果见S170。
显式固定preview adoption仍未实现，不据此勾选D03/D04/原53。

字段切片S168：完整typed字段阅读已实际接线，正式async命令
`session_import_source_comparison_field`与D1共用固定pair/RAM-disk-pin/lease/
hash/lifecycle及返回前校验。top-level字段名、全文body/field digest、准确JSON
分段、UTF-8 byte offset、缺字段与JSON null、独立signed cursor（不是行cursor）
已实现；默认4096B、原生<=64KiB/响应<=256KiB，不取asset、不读live source。
N298首轮11passed/2failed是在fixture分组阶段，未到两项字段断言：非Cursor
不是typed-native组，大首记录超过listing采样。只修fixture、保留策略；N299
原84404窄门fmt/compile/manifest/14聚焦passed，仍不是六门full证明。
N301原2629/run `v301-title-23c83967b9254e11a0268ddda1fd5e9e`：fmt/
compile/manifest、14聚焦、977领域/13ignored、Clippy通过，均0failed；全库
3906passed/0failed/15ignored，完整六门已收；690 native fresh drift0。
前端独立field owner和实际dialog inspector/15语言已实现：只存当前段、独立
分页与原cursor重试、切row/side/field/pair/scope/close/unmount失效；field列表
在读取另一字段时仍可操作，缺字段可明确查看，焦点/IME/copy/CSS合成覆盖。
最终F300 `v300-comparison-field-39d7a3bddd5546f3812ad3483faf7ec1`五门49
聚焦/96相关passed，类型/定向lint/quality40通过，2508输入/source-Git稳定。
不是full frontend/bundle/pixel/native desktop或显式A-B导入证明。typed compaction
仅字段序列化unit覆盖，不冒称Claude同组命令链。明确采用固定preview、完整A
metadata和旧身份adoption/原53余项仍待，下一执行书`plans/v302-fixed-preview-adoption-next-slice.md`。
public wrapper仅复制Side四个身份字段，frozen扩展对象不透传UI/summary字段
已有回归。F300六轮退出缓存4069files/33428895logicalB已清；观察H空闲增量40292352B
另列，非独占physical回收。N299空temp已清（0files/0logicalB/observed-4096B）；
N301 temp已由原finally清理，N298/旧失败证据保留。App13294行
未增，无本轮测试App，唯一V241不提升、不发布，HEAD/index原样，goal active。
用户最新确认无安全弹窗；未因此执行Undo、访问私人源或改安全设置。
下方S166及更早记录是历史，不覆盖本轮结果与N301真实运行状态。

历史S166（2026-10-13）：N295原62248已exit0，完整六门通过：6聚焦、969领域/
13ignored、3898全库/15ignored，均0failed，fmt/compile/manifest/Clippy通过；
688 native输入稳定。F296双preview owner五门38聚焦/142相关通过；最终F297
`v297-comparison-ui-8019bf73637a4b99802c23a78208f76b`五门18聚焦/175相关通过，
2499前端输入/Git稳定，包含getPair和行级A/B标签。集合有重叠不累加。
真实Host已接只读比较入口：明确打开A/选择B才读正文，独立owner、分支、
有界替换分页/原cursor重试、缺尾/uncertain/excerpt与15语言。合成回归证明
不勾选/choose/prepare/plan/commit/provider，不等于像素、桌面或最终验收。
仍缺完整typed字段阅读、显式A/B固定preview导入；A的格式/root只有当前
member页含A时才显示。下一切片按外部`plans/v298-fixed-field-reader-contract.md`。
本批缓存已精确清4636files/41640449logicalB；观察H空闲增量53522432B单列，
含并发活动不称独占物理回收。所有owner退出，旧失败证据/源码/依赖/seed保留；
旧失败native temp、不明owner缓存及未验收替换的旧preview包继续deferred。
HEAD/index不变、App13294行本批未动，唯一V241不提升，不发布，goal active。
用户确认无安全弹窗，仅确认当前状态，不是实际Undo删除或安全设置授权。
下方S165及更早记录是历史，不据其running状态重启已经通过的门。

历史S165（2026-10-13）：D1固定preview比较已注册正式async命令；同组双member、
有序signed cursor、RAM/disk/member/lease守卫、全文typed字段摘要/来源span/
缺尾与uncertain均已实现，不拼正文，不变选择。V291窄门6passed、688输入/
Git稳定。V292扩大门6聚焦/969领域0failed（13ignored），Clippy在新增测试
len()>0处failed，未进入full；失败证据保留。只改为等价!is_empty()后，
原session62248/run `v295-title-064c9df1157345b38ce8401b0723d5f2`正在重跑完整
六门，已fmt/compile/6聚焦exit0，领域/full终态待收；Rust输入继续冻结。
独立TS DTO/wrapper与分页owner已实现，尚未接成paired UI。F293五门11聚焦/
134相关、2484输入稳定；F294五门59聚焦/113相关、2489输入稳定；类型/
定向lint/quality40通过，测试集合重叠不相加。旧回复/切scope/分支/关闭/
卸载、原cursor重试、DTO字节/身份/缺尾守卫有合成覆盖，不当桌面或full通过。
两轮退出frontend精确缓存共2496files/19079063logicalB已清；观察H空闲增量
合计24436736B另列，含并发活动不称独占物理回收。native失败temp/日志保留。
App/AppWorkbench本轮未动，无live测试App，唯一V241不提升；完整字段阅读、
双preview UI/15locale/显式A-B导入与旧身份adoption/原53余项仍未闭合。
下一执行书：`H:/aicoding/grok-session-import-validation/plans/v296-paired-comparison-ui-next-slice.md`。
用户报告桌面无安全弹窗不等于实际Undo删除授权，安全设置和私人来源不动。
下方S164/S163与S162仍是历史记录，不覆盖本轮运行状态。

最新S164（2026-10-12）：V288原48767构建exit0/129.832s，嵌入F287、
3220source/162assets/Git稳定。cycle1与restart1各8个owned进程正常Quit
归零，无强杀；原restart guard68993/显式quit4865均exit0。来源摘要在实际
1282×712/931×712的滚轮揭示、最后目录按钮Tab/Return及Esc焦点恢复通过。
restart五次放大输入已观察，数值200%未独立核对；Settings随后最小化/检测
用户输入，停止UI输入，浅色未验，不记pass。无新导入/发送/实际撤销。
fresh3220输入drift0、旧6归档hash不变、HEAD/index保持；restart缓存已清
200files/11300866logicalB，观察H增量11448320B单列，不当独占物理回收。
完整记录在V288 run的cycle/restart observation与exit/cache/guard JSON。
唯一V241不提升，旧包deferred；D正文比较/adoption与原53项余项继续。
下方S163 building和S162 running均为历史检查点，当前无本轮live App。

最新S163（2026-10-12，桌面复测待收）：F287原99056已exit0，9761passed/
0failed/0pending/0失败suite，typecheck/整仓lint/UI build通过，3170输入/
sourceStable/gitStable=true，fresh685 native drift0，HEAD/index保持。Vitest
620.356秒/UI build23.174秒；原owner10268运行中完整绑定补录、已退出。精确
tmp清理1312files/11216416logicalB，观察H增量13950976B单列，保留ui-dist/
证据，不当独占物理回收。单一V288原48767/run
`windows-preview-v288-7bb0d726844843ab8af982b93d27aa25`复用F287正在构建。
外部helper只新增288白名单并通过parse；原685 native未变不重复跑全库。
当前CSS仍需新嵌入实机裁切/滚轮复测，唯一V241不提升；53项其余范围不减。
下方S162 running属于历史检查点。

最新S162（2026-10-12）：V284 reuse-profile补验已结束，浅色目录管理/reader及
关联根保留可用；实际缩窗931×802复现来源摘要挤掉第三登记行且滚轮无法揭示。
来源向导CSS限定修复自然内容高度与外层滚动传递，未改产品TS/Rust/i18n/
App/AppWorkbench；原content flex、有界root页/列表与固定底栏保留。新增CSS
契约红例保留，后续两次测试label/focus设置错误保留。F286首轮相关230passed/
1failed，旧metadata测试点击search-busy下disabled续聊；只修为确定性等待并
明确断言busy不prepare、settled后固定scope可继续。当前原12679/run
`v286-source-scroll-7012185d942f4f5faeb15b71bd9c4af1`五门已exit0：20聚焦/231相关
（重叠）、类型/lint/quality40通过，2482输入/Git稳定；不是像素证明。
F287全量原99056/run `v287-frontend-441d257807624d94991c7431bc7a0d73`正在执行，
其源输入冻结，终态待收。5个退出F286 run精确缓存已清，1649files/
14056988logicalB，观察H增量16457728B单列，不能当独占物理回收；日志保留。
fresh685 native drift0，App/AppWorkbench仍13294行，3改动前端文件为LF/noBOM。
新3个前端输入尚无成功当前全量/
嵌入候选/桌面复测，下面F285/V284是S161旧基线，不能据此提升V241。
重启owner61664及8个owned进程正常Quit归零，guard80316/closeout70248均exit0；
WebView177files/10985451logicalB已清，观察H增量11100160B单列。restart只读
审计r2按正确owner绑定通过，无发送，旧归档未改。实际撤销仍等action-time
答复，200%/真实IME/DPI与53项其余真实证据不减少。
下一修复/验收书：`H:/aicoding/grok-session-import-validation/plans/v286-source-scroll-fix.md`。

已关闭基线检查点（2026-10-12，S161）：C1b已通过V272完整合成原生门；C2独立
固定组选择现已通过V279完整六门。原6318/run
`v279-title-cc3412b5d3d642ef885bade69b286106`已exit0：115聚焦、963领域/
13ignored、3892全库/15ignored均0failed，fmt/compile/manifest/Clippy通过；
full790.76秒，terminal.passed/sourceStable/gitStable=true。fresh685输入
drift0，HEAD/index原样，原owners和native artifact owners均退出，原finally
精确清自身compiler-temp。EXE SHA为
`1853646299cd33a26fa8bf3aced313537abbcd24fb9c2fb14d45a5726ee33388`。
V276全库真实失败和V278窄复测证据保留；重签游标测试仅修契约断言，
生产TTL/MAC未变，不将失败历史改写为通过。

C3a独立前端域已实现：fixed-groups selector与candidate域分离；同步CAS
flight、版本协调、页/扫描/卸载owner；按需单页50版本元数据、前后页/retry，
关闭/切组丢弃晚到成功/错误/finally。正式红例复现并修复callback换scope
抢新page owner、可变filter、空live尾页堆cursor及身份/DTO守卫缺口。
S155当时F280 `v280-group-domain-d934eedc403b4a53990a6c9329705faa`原78268
exit0：104域测试（56组+6mutation边界+32members+10validator）、208相关
Host/content/prepare回归、tsc/定向ESLint/quality final40通过；2468输入与
Git稳定。缺模块时的collection failure和一次测试provider拼写造成的类型
失败均保留，不能当行为红例或完整通过。测试数据为合成，只mock IPC。
本批9个新前端域/测试文件均UTF8无BOM/LF；App/AppWorkbench没有本批改动。

C3b typed选择/prepare桥现已接入生产：新扫描默认fixed-groups，候选descriptor
不被组域覆盖；只查实际预览的有界override membership，保留显式旧候选路径。
choose/clear/refresh/其他writer改变member时窄失效旧preview。新来源（含工作区
预览失败）同步清旧scan；关闭/切来源在prepare discard或scan ACK等待中的
晚到结果不能复活组选择。prepare与读/改选择同步互斥，ACK必须匹配原版本，
不匹配时请求取消且不adopt；不宣称取消一定成功。完成/恢复batch锁定live edits。
实际Host反例修复了ACK→membership间列表行消失：保留上一份一致显示快照，
配对旧计数并禁用stale控件/Next，ready=false不伪装rollback。

F281原87455/run `v281-selection-bridge-3a382e6873cc404aa7047ecd0f421780`
exit0：103聚焦（含21真实Ui hook及2真实Host集成）、485相关回归、tsc、定向
ESLint、quality final40全部通过；两组测试存在重叠，不相加为唯一测试数。
2473前端输入/sourceStable/gitStable=true，fresh drift0；685 native输入drift0，
HEAD/index不变。真实行为红例及旧mock/测试fixture/类型接线失败日志均保留，
不是整仓全量或桌面/原厂/provider验收。5个新TS文件UTF8无BOM/LF；App/
AppWorkbench本批未改。13个退出run的39个精确缓存子目录已清：2457files/
25,803,096logicalB；观察H空闲delta合计+61,874,176B单列，含并发磁盘活动，
不称独占物理回收；源码/依赖/日志/当前V241入口不动，可重跑F281再生。

C3c实际lazy版本菜单和显式候选/组域入口已落生产，15语言同批覆盖。成员
元数据按需单页50，portal实心表面、互斥菜单、busy/error/retry/empty、IME/
箭头/Tab/Escape/outside与原组焦点恢复均有覆盖，不按文件名猜格式或称首项最佳。
原45874实跑157passed/3failed，正式复现Tab双向不前进和独立域旧preview残留；
修为关闭scope后按trigger逻辑Tab顺序前进，两个域共同登记最多200实际预览owner，
close/域切换窄失效。失败CAS保留原preview；ACK后membership失败仍失效旧授权，
菜单显示复核而非假装rollback，Refresh后恢复当前choice。
F282原60011/run `v282-member-menu-ec3a9c667cd548f1bef00ef6bf2a5756`全部五门
exit0：165聚焦、493相关（重叠不相加）、tsc、受影响lint（含15locale）、quality40；
2476输入/sourceStable/gitStable=true。含23真实Ui/8真实Host和12菜单交互，只mock
IPC，不代替整仓/桌面。685 native fresh drift0，App/AppWorkbench与F281哈希相同。
5个已退出run精确15缓存子目录inventory→Execute清理：1655files/14077304logicalB；
观察H空闲delta总18235392B另列，包含并发磁盘活动；日志/source/seeds/依赖均保留。
C3d位置关联现已落生产界面，限定已有Cursor/local契约。默认可读来源只有
显式点击才register（无sourceId、不搬迁）；选文件夹后再次明确关联。
固定source/provider/local/path/expectedPrimaryRoot，丢ACK或关闭保留原请求，
重开只复核同一幂等操作，不声称回滚。ACK成功而list失败保留成功状态。
关联不自动勾选；每页50元数据替换、retry保留attempted cursor、显式选最多
200根。SourceChoice保存完整位置，ScanSource wire使用顶层provider/path/
sourceId/locationId/host，不能塞嵌套location。13新键已覆盖15locale。
正式复现并修默认登记ACK晚到打开旧来源弹窗；source intent/installation/
primary/lifetime/serial隔离success/error/finally和选择take。
F283原18120/run `v283-source-location-ui-5e2cf8f2df454c948c53769a670a5ea7`
五门exit0：84聚焦/586相关（重叠不相加）、tsc/定向lint/quality40通过；2482
输入/Git稳定。之后两文件改为layout提交/卸载失效owner。F284原42429/run
`v284-frontend-f06fbb9129ee4b328443115ebf1f2016`已exit1：typecheck/整仓lint/
UI build通过，3170输入/Git稳定，9745断言passed/0failed；但旧ReviewGallery
缺pickDirectory mock使1suite未collection，success=false，不称完整门通过。
只补旧测试的picker/host fail-closed mock，产品未改。F285-gallery原92154/
`v285-gallery-mock-73c0e01466bd480ab504d770525b0250`已12passed/0failed，
输入/Git稳定；100缓存文件/1127488logicalB已清，观察delta1384448B单列。
F285整仓原74334/run `v285-frontend-b2b24d570a6f4f469ac33a0b60a7f4db`已
exit0：9757passed/0failed/0pending；typecheck/整仓lint/UI build通过，3170
输入/sourceStable/gitStable=true，fresh3170/native685均drift0。质量final40、
deps:check通过；audit:prod达到moderate门槛，仍3low，不说零漏洞。精确tmp
inventory→Execute已清：1312files/11216288logicalB，观察H空闲delta14475264B
单列，含并发活动不能当独占物理回收。成功ui-dist/日志/homes保留，F284失败
日志和tmp保留，不改写为绿。单一候选windows-preview-v284-c44de11d6d0543b69be8761e09603843
复用F285，原90384已exit0，114.655秒；3220source/162frontend/Git稳定。
EXE SHA `4bf21dde6db415eb5bbce16553bd066e855a9c525cbbfb4d50029865615189b8`。
V284 cycle1隔离原生Mock实机完成两根关联/显式勾选、5候选3组及A/B/C版本
选择、B预览与两域切换、只导入历史1/1、阅读/搜索/复制导出、预检/未发送
续聊草稿、重开结果、撤销确认取消、旧seed阅读。r4只读审计exit0，逐事件
正文/来源时间/sourceMember/唯一receipt/revision、旧6文件及creation intent
摘要/结构化draftHash/正文持久绑定均正确；messages=[]/prepared，无dispatch。
catalog/scan及伴随文件未改。r1字段、r2错误hash语义、r3过严shm守卫失败保留，
只修外部审计脚本。临时来源banner下登记行裁切待查；浅色import表面/真实
窄窗/IME/DPI尚未验完，实际撤销等待action-time确认，不称replacement通过。
fresh685 native drift0，App/AppWorkbench与F282哈希相同；26受影响TS/TSX含
15locale全部UTF8无BOM/LF。7个退出F283 run精确缓存已清：1856files/
16340287logicalB；观察H空闲delta20955136B单列，含并发活动。全部日志保留。
本轮PID11636及8个owned进程正常Quit归零，无强杀；原guard67840/显式收尾62125
均exit0。精确WebView缓存190files/11062917logicalB已清，观察H空闲增量11194368B
单列，含并发盘活动；归档/seeds/source/日志/包保留。完整观察与未验项见该run
`desktop-cycle-1-observation.md`；数据证据见`desktop-cycle-1-readonly-audit-r4.log`。
下一步仅单一reuse-profile补验并fresh绑定；不重跑未变Rust全门。不改安全设置/
读私人来源/调真实provider；用户确认和fresh截图均无安全遮挡。其他CU分支
App未动。replacement未完成前旧包清理deferred，不累积额外用户入口。
唯一V241入口不提升，53项真实余项仍未关闭。
下一UI细化书：`H:/aicoding/grok-session-import-validation/plans/v283-source-location-ui-next-slice.md`。
下方逐轮段落是历史记录，以本检查点及后续真实终态为当前状态。

前端检查点（S152，下列native live状态为当时记录，S153已收失败终态）：V276领域已963passed/0failed/13ignored，Clippy
exit0；全库原owner PID25904/parent71304自05:56:22本地继续，完整门终态
仍待收。native685输入fresh drift0、HEAD/index保持；没有重启原门或App。
经确认native测试不读取该前端hook，限定独立修复已完成：hook-lifetime
单调requestSeq跨close/scan/slot重建不归零，unmount失效、单candidate失效，
只由active candidate采用active preview；其他slot/普通A-B-A保留语义不变。
正式红基线5failed后，当前`v277-content-lifecycle-115465c991d5475998ea62800a19a6b6`
实跑旧22+新24=46passed，Host/content/prepare相关208passed；tsc/noEmit、
定向ESLint、quality final40均exit0，2459前端输入和HEAD/index稳定。
这是当前前端合成回归，不是新UI接线或桌面验收；R04仍部分。
原前端harness退出后已inventory→Execute删除1248缓存文件/9540424logicalB，
观察H空闲增量11030528B另列；启动器路径选择失败的run保留诊断证据，其
空temp移除0files/0logicalB、观察-4096B。新缓存可重跑外部v277 gate再生。
旧正式red缓存缺精确owner记录暂缓；V276live temp归原finalizer。唯一V241
入口不变，未提交/推送/PR；C3a-d、D及原53项真实余项继续。

S151额外确认一个现有前端缺口：外部当前production hook隔离诊断实际
3failed/0passed（exit1），同candidate/revision关闭重开或scan generation
清store后请求owner归初始值，旧preview或错误污染新pending。原F全量没有
覆盖这个重置反例，R04已改部分/失败。run为
`v276-c3-owner-audit-1fc3772d5c214a4bb162f020b0bd3d9e`；只mock IPC，不mock
owner守卫、不启动App/模型。首assert失败后的finally/计数断言未到，不
冒称全边界均已独立复现。源码两hash/native685输入未变；等V276真实终态
后先正式test-first修此C3前置，再接组UI。该诊断自身104缓存文件/
2222145logicalB已退出owner/拒绝reparse检查、inventory→Execute清理，
观察free delta+2420736B单列；spec/config/失败日志/JSON和native live temp保留。

2026-10-11补核：V248完整native门已结束，聚焦22passed、领域872passed/
13ignored、全库3801passed/15ignored，均0failed；fmt/compile/clippy exit0。
终态sourceStable/gitStable=true，661份输入逐哈希复核无漂移，HEAD/index
未变。静态质量40项通过；这仍是合成Rust证据，唯一预览入口仍V241。

2026-10-12：D04身份第一步已写入正式Cursor扫描/归一化，normalizer为1.0.4。
V249 test-first实际22passed/4failed，复现裸stem碰撞、声明ID丢失及混合
会话仍被接受；随后补源根就是subagents时的隔离、路径边界和显示标题回归。
V249完整native门已通过：聚焦31、领域881、全库3810passed，均0failed；
fmt/compile/clippy exit0，663份输入与HEAD/index稳定。Unix专属断言未在
Windows执行，不能据此宣称Unix原平台通过。
跨Store/Composer/transcript来源组、默认两根明确关联、冲突版本选择及
旧stem身份显式迁移仍待实现，不能把身份修复写成D04整体完成。

2026-10-12继续：normalizer现为1.0.5，新增生产双发布反例已真实复现并
修复三种错误归并（DB keyfamily、transcript声明family、未知Store同名目录）。
`v250-title-0e5b0bb81df04ad7951a568c1049b209`聚焦37passed；领域884passed/
3failed，失败为旧裸ID路径断言及其隔离子进程、旧normalizer版本断言。
契约断言已更新；`v250-title-8b29a8db7d3042fcb2e5b740c04f7314`聚焦39通过，
领域886passed/1failed，唯一失败为未隔离搜索fixture所见HMAC目录拒绝访问。
V251 fresh-process回归稳定证明旧fixture写入继承home，修16个fixture的
RAII目录/环境锁隔离后，新`v251-title-e6dd303e572443b6b67472f42e56b11b`
实际41项聚焦、领域889、全库3818passed，均0failed；13/15ignored仍单列。
fmt/compile/clippy全部exit0，665份输入与HEAD/index稳定；未改产品HMAC或
权限规则。原进程退出后本轮compiler-temp已清理，不将此门当桌面/跨平台验收。

2026-10-12 B1a：新增主目录位置查询、稳定 `primary:<sourceId>` 引用及正式
scan/preview 校验；读登记不 DDL，固定 alias 不替换成后来搬迁的主目录。
V252新契约实际42passed/3failed后实现，V253聚焦50passed/0failed，fmt/compile
exit0，667输入与HEAD/index稳定；前端API6项、typecheck/lint和静态40门通过。
这是新的窄切片证据，未跑其领域/Clippy/全库；旧N251的3818不冒充当前全库。
附属根独立关联登记、location scope、group/member及来源选择UI仍未完成。

2026-10-12 B1b：独立 source-location 扩展 schema1、Cursor/local 显式关联
命令与 registry/TS API、有界签名分页、完整位置 resolve 已写入正式管线。
关联不改 primary/profile/aliases，重复关联返回同 UUID；其他来源已占用或
根重叠明确冲突。合法主根搬迁同步 stamp，旧二进制登记漂移明确报错，
旧归档保持可读且不重写。primary 保留1.0.5身份计算，associated fallback/
subagent/未知Store同接稳定location scope，normalizer现为1.0.6。

V254是编译引用错误，未进入断言；修正后V255实际61passed/1failed，正式
并发关联稳定暴露marker/结构读跨越提交的竞态。修一致性读快照后V256
聚焦63passed/0failed，包括201附属根分页、JSON/wire双根发布与重启读取。
V256终态：领域911passed/0failed/13ignored；Clippy被两处release dead-code
阻断（normalize_in_root、file_candidate），全库未执行，不能写完整通过。
已保留原入口并改为正式default分派调用；修后Clippy仍待实际验证。
669输入fresh哈希无drift，HEAD/index未变；前端API8项/typecheck/lint和
静态40门通过，App合计13294行不增。唯一预览仍V241。

V257 test-first终态63passed/1failed/0ignored，失败于正式scan缺少
source_groups表；670输入/sourceStable/gitStable=true，fmt/compile通过。
B2正式磁盘groups/member与只读签名分页命令已接线。V258聚焦70passed/
1failed，真实暴露200项完整元数据超过256KiB预算；保持原预算，改为同时
按条数/字节推进keyset，下一页不漏成员。V259终态聚焦71passed、领域
919passed/0failed/13ignored，Clippy exit0；全库3847passed/1failed/15ignored，
695.65秒。673输入/sourceStable/gitStable=true，但terminal.passed=false。
唯一失败为receipt_review取消测试的100ms时序断言，不是已证实的产品重发。
V260复用旧二进制单跑3次均通过、receipt领域20通过，仅作诊断，不抵销全库失败。
现将测试改为callback readiness同步后显式drop，保留UNKNOWN/不得重发断言；
修后正式回归待执行。前端新增groups API后12项通过。本批未启动桌面App。

这是后端/API切片，不是完整B1/D04验收：关联/组UI尚未接线，
固定版本选择→prepare/lease/durable plan/receipt/recovery全链、
比较/adoption及单App桌面闭环仍未完成；原厂/真实ACP/原平台范围不缩减。

C1a（2026-10-12）：V261 receipt短正式20项通过。V264固定引用正式反例
71passed/3failed；修后V265聚焦77passed/0failed，fmt/compile通过，675输入
与HEAD/index稳定。Cursor新sourceMember独立HMAC固定完整成员/目录/组/
proof/listing revision，同pin进入新owned manifest hash、lease和resident。
common selection拒绝篡改/高版本/unknown/lease-manifest或旧provenance不一致。
已执行resident/disk、源离线发布与reader、旧缺pin兼容；这是C1a，不是
canonical plan、prepare input/override或durable job/receipt完整绑定。
V266原session3269已exit0：完整六门通过，聚焦77、领域925/13ignored、
全库3854/15ignored，均0failed；fmt/compile/Clippy exit0。675输入逐hash
复核drift0、HEAD/index不变，精确原owners已退出。原取消测试在默认并发
全库实际ok；这仍是合成原生证据，不是C1b/D04或最终53项交付。
V267 test-first已exit1：77passed/5failed，真实复现计划丢member摘要、
rehashed snapshot被commit忽略、prepare input及cached mapping漏验pin、
只读group/member漏验MAC。旧disk/其余mutation循环在第一失败前未执行，
不冒称所有旧反例均已单验。修后V268原31004 exit0，85聚焦/0failed(51.32s)，
fmt/compile通过、676输入与HEAD/index稳定。新plan_snapshot的严格typed
bindings进入canonical hash、load保留、resident/disk共同commit边界实际核对；
新prepare input/override/mapping与lease验同pin，旧无pin契约保留。metadata
页验MAC/scan/epoch/SQL group，纯只读不读离线源。合法另一次扫描pin替换、
删pin/unknown/high-version、离线prepare→mapping→plan→commit→reader与
同正文新scan dedup且旧revision/receipt/ownership不改的断言全部执行通过。
V268仍只是窄门；V269完整六门已启动，source冻结。完整job/receipt显式
pin绑定尚未实施；具体顺序见validation/plans/v250，之后才C2/C3/D。本批无App。
V266自身临时754文件/10196916 logicalB已清，观察空闲变化-787922944B受
并发写盘影响，不宣称实际回收该值。旧4个空failed temp已清（logical0B，
观察free delta+12288B）；V256失败合成资料370文件/4967832B保留。
V260与以前tool policy拒绝目标均未重试。
V268自身空compiler-temp已按成功窄门专用限制盘点/退出owner后移除，
0files/0logicalB、observed0B；该模式不能作为旧artifact完整replacement。
唯一V241入口不提升，不发布。

V269原session9145已exit0（2026-10-12 04:40）：完整六门通过；85聚焦、
933领域/13ignored、3862全库/15ignored，均0failed，fmt/compile/Clippy exit0。
676输入fresh逐hash drift0，HEAD/index未变，原75196/23808与task native
owners均已退出。原finally清自身compiler-temp：756files/10196232logicalB，
observed free delta+11153408B；并发盘活动下不把这个观察值当独占回收量。
这证明C1b plan/prepare/metadata切片，不是durable input/receipt已完成。
随后只新增6个正式durable row/receipt/warm及persisted reader反例，并加入
历史v1合法签封proof的绕过反例；生产实现尚未改，V270窄门开始test-first。
原53项交付范围、唯一V241入口与不发布约束保持。

V270原79878 exit1：85passed/6failed，6个独立record/reader反例均实际复现。
现DurableInput/Receipt保存完整optional pin，旧version2缺字段兼容；共用
纯metadata RecordIdentity验签与旧身份，reload/retry/recovery完整Option
比较，published manifest与receipt比较。缓存key纳入Some pin；proof升v2，
旧v1 proof需完整重验。V271原30308 exit0：97聚焦/0failed、fmt/compile
通过，677输入/HEAD/index稳定。离线/preview已正常清理的committed恢复、
合法另一scan替换、历史v1proof与旧无pin记录字节保留、真实worker dedup
结果保存失败→离线重放且旧ownership不改均已执行；未增completion系统。
补4个receipt插入边界重试与篡改回归后，V272完整六门开始，生产输入冻结。
这仍不是完整C1b/full当前通过或C2/C3/D04/53项交付，无桌面App。

V272原12346：fmt/compile/manifest与99聚焦已通过（69.38s）；领域正在跑，
Clippy/全库没有终态。4个实际receipt中断边界及去掉注入trigger之后的
篡改拒绝均执行通过。当前677输入冻结，不启动另一Rust build/test/App。
下一C2实际入口/独立schema/固定member快照/prepare resolver/局部失效细化到
外部`validation/plans/v273-fixed-group-selection-next-slice.md`，只是计划，
未把C2实现计为完成；先收V272原handle全门、hash/index/owners/finalizer。

V272继续：领域947passed/0failed/13ignored（410.90s），Clippy exit0
（55.34s）。全库原PID76132/parent56812从05:03:59本地继续执行，无终态。
fresh677输入drift0、HEAD/index原样；冻结保持，不能把通过的5步写成6门。
C2外部设计补同组合法member替换的选择授权边界；生产尚未动，无App。

V272最终（05:16）：原12346 exit0，完整六门terminal.passed=true。
99聚焦、947领域/13ignored、3876全库/15ignored均0failed；fmt/compile/
manifest/Clippy通过，full715.29s。fresh677输入drift0、HEAD/index稳定，
原76132/56812及task native owners退出。原finally清自身747个临时文件，
10193061 logicalB、observed free delta+11173888B，不称独占回收量。
C1b record/receipt/recovery/reader合成原生切片通过；C2/C3/D04和真实验收
仍未完成。随后只新增正式C2独立selector初始化失败基线，V273原5372
BaselineOnly正在编译，生产源码未实现组选择。唯一V241不提升，不发布。

## 当前基线

- 唯一开发工作树：
  `H:\aicoding\grok-session-import-validation\integration-rehearsal-v243`。
- HEAD：`632f2a8f56bffe8d1c126fae5794f5c3fda316bc`，有大量未提交修改。
- 用户固定入口：
  `H:\aicoding\grok-session-import-validation\start-current-preview.cmd`。
  当前指向 V325，已通过 S187 的限定桌面验收；S189新增未接UI的只读domain
  检查，没有替换该包，也不是全部交付。
  内部测试包不提供多个入口，原 V310 失败证据保留。
- 导入历史保持只读，续聊是用户明确发送的 Grok handoff；不冒充原厂 resume。
- 1.0.1 的来源标题修复及 1.0.2 的 Claude compaction 处理已有当前源码
  回归。旧 revision 保持原 normalizer，不重写旧归档。
- 1.0.3的Cursor数字角色/来源时间、legacy发现和Store wire完整门已通过。
  1.0.4 transcript身份/显示标题修复已通过完整合成native门；这不是D04
  来源组、原厂、桌面或跨平台验收。旧stem身份不自动重写旧entry/revision，
  迁移仍需明确设计和回归。
- 历史1.0.5 typed-family/Store位置身份与fixture隔离已有完整native证据：
  聚焦41、领域889、全库3818passed，fmt/compile/clippy通过；已证实composerId可关联同安装Composer key，
  但两正文保持独立revision，不按目录名推断Store关系或自动改写旧身份。
- 当前normalizer1.0.6、location扩展1、group schema1；B2正式分组/API已通过
  后续C1b完整V272原生门。V259取消测试时序已用readiness修复并在后续全库
  执行通过；独立group-selection扩展1已实施，当前C2全门V276仍待终态。

## 已核对的当前证据

| 项目 | 结果与范围 | 证据目录（均在 validation/runs 下） |
| --- | --- | --- |
| Rust 全库 | 3,778 passed，0 failed，15 ignored；真实执行断言 | `v245-title-fa98e2f14ee04387a64e9d96218f4851` |
| 导入领域 | 849 passed，0 failed，13 ignored | 同上 `migration-tests.stdout.log` |
| Compaction 聚焦 | 19 passed，0 failed | 同上 `compaction-tests.stdout.log` |
| Rust 格式/编译/Clippy | 退出码均 0；Common Controls 清单已核对 | 同上 `terminal.json`、`executable.json` |
| 补充静态门禁 | 代码质量 final 40 项通过，网站下载脚本 self-test 3 项通过；不是 native/真实渠道测试 | 同上 `supplemental-static-audit.json` |
| 100 MiB / 1 GiB | 两个 opt-in 精确断言各 1 passed；有计时/RSS/存储记录 | `v245-performance-fc36e95375bf417a8bafc1ea41402838` |
| 中等及以上生产依赖审计 | markdown-it 修至 14.3.2 后 `audit:prod` 通过，仍有 3 个低级 advisory | native gate 目录中的 `dependency-audit.md` |
| 更新 lockfile 后的前端全量 | 9,537 passed，0 failed，0 pending；类型/lint/UI 构建通过 | `v245-frontend-91c7f86f6b5d48f687903eddfc209d79` |
| 日语修复后的语言回归 | 47 passed，0 failed；类型/lint/UI 构建通过，其他输入与全量基线一致 | `v245-frontend-9a9c22569eff4a3588a3af3ec4890072` |
| V245 原生候选 | 构建 exit 0，126.343 秒，输入/前端/HEAD/index 稳定；桌面验收部分完成 | `windows-preview-v245-ee9c2c6789ca4c778f9402b4cd5d8d38` |
| 千条完整发布 | 精确 opt-in 断言 1 passed；1,000 entry/revision/receipt/reader 全核对，重启不重复 | `v245-performance-b8a591b89a9d4d89ac7739c3236b174a` |
| 千条取消/离线恢复 | 精确 opt-in 断言 1 passed；202 条完成处取消，43 ms 返回、1,086 ms worker 停止；798 条恢复后全部唯一 | `v245-performance-db0797623d514cd1969c93a9e267faef` |
| 万条完整发布 | 精确 opt-in 断言 1 passed；10,000 entry/revision/receipt/reader 全核对，无重复；全程 3,153.830 秒 | `v245-performance-10511fc0e1bd4660a3410a4f0c76e07e` |
| 万条取消/源离线恢复 | 精确断言1passed；2007条处取消、2008条时停止，余7992条恢复后全部唯一；cancel323ms/stop10280ms/retry619ms | `v245-performance-dcf7826311b14bd1a833d4d767bb8938` |
| V245 Windows 部分实机 | Cursor 扫描/重导入/阅读/搜索；Cycle 3 成功创建续聊、保存草稿、打开固定历史和复制导出，未发送 | V245 原生候选目录 `desktop-cycle-1-observation.md`、`desktop-cycle-3-observation.md` |
| 退出与清理 | 三轮 App 各 8 个进程归零，无强杀；Cycle 3 暴露同时收尾竞态，guard 失败证据保留 | 同上各轮 exit/cache-cleanup JSON 与 Cycle 3 guard JSON |
| V246 当前前端全量 | 9,537 passed，0 failed，0 pending；类型/lint/UI 构建通过，输入/HEAD/index 稳定 | `v246-frontend-6ec2638b3ebf4f29bae3e23e6e081eef` |
| V246 原生候选与部分实机 | 构建 exit 0，125.105 秒；中文窄窗 200% 搜索框修复、键盘搜索/清除、中文草稿进程重启恢复通过，未发送 | `windows-preview-v246-2ec85b92c33441228dbf0788ffba00a7/desktop-cycle-1-observation.md` |
| V246 收尾并发回归 | 初次及重启各 8 个家族进程正常退出；guard/显式 quit 均 exit 0，排他锁下缓存仅删除一次 | 同上 cycle/restart 1 的 guard/exit/cache-cleanup JSON |
| V246 Claude 多选与主题 | 合成来源扫描、多选、预览、准备2/2、history-only映射、导入2/2、中文摘要搜索和高亮、深浅主题、重开结果通过；撤销仅确认/取消 | 同上 `desktop-cycle-2-observation.md`、`desktop-cycle-2-archive-audit.json` |
| V247 1.0.3原生基线 | 聚焦19passed；导入869passed/13ignored、全库3798passed/15ignored；fmt/compile/clippy全部exit0，661输入/HEAD/index稳定；后续边界修改需V248门 | `v247-title-70df268594ee489283d8f8c18deb6806` |
| V248 1.0.3边界完整回归 | 聚焦22passed；导入872passed/13ignored、全库3801passed/15ignored；均0failed，fmt/compile/clippy exit0；661输入/HEAD/index稳定 | `v248-title-d0f458e81d42435f8f038287df236a06` |
| V249 1.0.4身份/标题完整回归 | 聚焦31passed；导入881passed/13ignored、全库3810passed/15ignored；均0failed，fmt/compile/clippy exit0；663输入/HEAD/index稳定；不包含D04分组 | `v249-title-d36851c8e6bc49459fba2196ba262a52` |
| V251 1.0.5类型身份/fixture隔离完整回归 | 聚焦41passed；导入889passed/13ignored、全库3818passed/15ignored；均0failed，fmt/compile/clippy exit0；665输入/HEAD/index稳定；不包含D04来源组或桌面验收 | `v251-title-e6dd303e572443b6b67472f42e56b11b` |
| V259 B2当前正式后端/API | 聚焦71passed，导入919passed/13ignored，Clippy通过；全库3847passed/1failed/15ignored，receipt取消时序失败；673输入/HEAD/index稳定，完整门未通过 | `v259-title-5a813e24862b4b4e97996af7f2c22fd2` |
| V272 C1b完整合成原生门 | 99聚焦、947领域/13ignored、3876全库/15ignored，均0failed；fmt/compile/Clippy通过，677输入与HEAD/index稳定；不是C2/UI/真实验收 | `v272-title-5085871d3fc94f10975d29c5075b69f2` |
| V275 C2聚焦门 | 115passed/0failed，fmt/compile/manifest通过，685输入与HEAD/index稳定；BaselineOnly，不包含领域/Clippy/full；后续测试加强断言由V276验证 | `v275-title-61d00091145e4c489181901e7b563831` |

C2实施范围：独立group selector/schema、每组完整固定CandidateRef、choice与
descriptor签封、CAS/原子更改、有界只读分页、legacy candidate保留，以及
prepare selector resolver。115聚焦包含离线prepare→plan→commit→重启reader、
A/B正文分歧生成不同revision、真实暂停发现后的晚到成员/组、同组合法B
替换A/删行/旧choice重放拒绝、legacy与10k元数据页、读页后确定性更改。
10k没有正文文件，不是原厂资料或UI性能验收；没有单独公开计时数字。
8条新command已注册并有TS wrappers，尚未接产品UI。未把group descriptor
塞进旧scan hook的candidate adopt，也没有UI假按钮。
V275自身空compiler-temp已inventory→FocusedOnly Execute移除，0文件/
0logicalB、观察空闲增量0B；当前EXE/依赖/日志/失败证据/V241保留。

Rust gate 与大输入 gate 均记录 654 份输入哈希、HEAD/index 稳定。
大输入通过只证明后端合成场景；详见 `session-import-p0/benchmark-baseline.md`。
V241 的四轮原生 Mock 耐久证据继续保留，但不能替代后续代码的桌面验收。

## 此轮发现的产品缺口

Cursor公开格式审阅和production回归确认：数字type1用户曾落成assistant；
legacy chatData只在测试辅助发现函数支持，正式扫描返回0；直接毫秒时间
此前不保留，inline conversation id可能误作外部指针。第一轮V247固定10项
回归6passed/4failed；修复后扩至13项保真回归均passed，覆盖scan→preview→
plan→commit→cache reset→双项分页reader，以及独立导入时间/来源字节不变。

随后新增公开DB meta/ordered Merkle/protobuf field4 Store样例，复现失败：
预览只返回1条而非期望4条。原实现没有selected sidecar root时遍历全部blob，
实际会错误导入废弃叶。新实现已接正式normalize_store：bounded selected-root
DFS，不读取非reachable正文；工具call/result保持类型、未知块有span/诊断，
metadata时间不补消息时间。V247完整native门已通过（19聚焦/869领域/3798全库）。
复审新增边界：截断已声明32-byte引用应partial而不是静默丢掉；无DB或sidecar
selected root不能退回遍历全部JSON叶；工具调用所属消息仅来自同一叶的文本，
不能借前一用户消息；role=tool的现代result块保留各callID。补丁和新回归已写入，
V248完整native门已通过，不把合成结果升级成原厂、桌面或平台验收。
生产跨Store/Composer/transcript聚合（D04）仍未接入；不能以cfg(test)聚合
测试替代该项。真实原厂/WAL/ACP/平台门保持原有范围。

日语 `session-import.ts` 的来源品牌名和纯占位元数据之外，仍有 24 条
内容选择、授权和工具状态文案曾沿用英语。完整语言键集合不能证明翻译完成。
这 24 条已补齐；变量、来源品牌和既有 key 保留。修复后语言回归、类型/
lint 和新 UI 构建通过。复用全量基线的 gate 已核对其他源码与 lockfile
未变，并会拒绝存在非语言输入变化的旧基线。

## 剩余工作及所需证据

1. V246 新构建已结束。候选 EXE SHA-256 为
   `ce5d9e1cf770d4a79a2da73bc8abb57c6377ec6cfc1db4e411d7cc757b6ffb1c`，
   92,215,296 字节。当前前端全量通过；replacement desktop gate 尚未完成。
2. 在单个隔离 Windows App 上完成来源→扫描→多选→内容→映射→导入→
   阅读→搜索→导出→草稿→撤销，核对旧 revision 不变、标题/compaction、
   键盘/IME、窄窗、200% 与深浅主题。不得在安全权限弹窗仍阻挡时发送输入；
   Windows `node_repl`/sky 工具可用；每次重新绑定都先观察当前窗口，
   不复用旧截图/坐标，也不根据窗口列表缺项判断安全弹窗已消失。
   用户确认安全遮挡已消失，Cycle 3 实际截图也未见阻挡。新鲜绑定后
   成功创建续聊并保存中文草稿，导出通过隔离输入框粘贴核对且未发送。
   窄窗 100% 正常，约 886×802 px 窄窗放大到 200% 时搜索框异常增高。
   已对纵向工具栏的搜索框取消 240px flex-basis；V246 全前端及窄窗 200%
   实机复测通过，Enter 搜索、Tab/Enter 清除并恢复输入焦点也通过。
   V246 保存中文 marker 后正常退出并重新启动，既有会话编辑器恢复草稿；
   messages=[]、prepared 且未派发。这里只验证进程重启，不是 OS 重启。
   Cycle 2 的 Claude 合成两会话已走完多选/准备/只导入历史/导入/阅读；
   压缩摘要搜索得两条命中、点击高亮正确，浅深主题及重开作业面板通过。
   撤销确认/取消已操作，实际确认删除尚未执行，已请求 action-time 确认。
   退出后逐事件及独立导入时间核对通过、旧归档不变，原厂资料兼容仍不据此通过。
   Unicode type_text 不代表真实中文 IME，应用缩放也不代表系统 DPI 测试。
3. 运行当前源码的千条/万条准备、完整发布和约 20% 处取消/恢复容量。
   每项核对全部 entry/revision/receipt/reader，无重复发布；确认响应与
   worker 停止分别计时。历史容量结果保留，不直接写成当前结果。
   千条完整发布已通过并包含准备 API/plan/recovery/enqueue 断言：全程
   258.307 秒，发布 176.047 秒，逐条复核 33.802 秒，峰值 RSS 36,757,504
   字节；status 6 ms、items 8 ms、catalog 首页 4 ms。这里不是 UI 首屏
   计时，也不是单独 prepare-only 测试。千条取消/离线恢复通过：202 条
   已提交事实保持不变，剩余 798 条全部恢复，最终 1,000 entry/revision/
   committed receipt/reader event 无漏项或重复；取消调用 43 ms，worker
   停止 1,086 ms，retry 53 ms。全程 274.579 秒，峰值 RSS 36,257,792 B。
   万条完整发布 run `v245-performance-10511fc0e1bd4660a3410a4f0c76e07e`
   已 exit 0，sourceStable/gitStable 为 true。publish 2,239.798 秒，
   verify 419.301 秒，峰值测试进程 RSS 148,037,632 B；catalog 首页
   23 ms，terminal 页 69 ms，并发 job poll 最大 513 ms，如实保留。
   PID 50920 已退出，runtime-temp 清空后移除。万条取消随后已通过：
   `v245-performance-dcf7826311b14bd1a833d4d767bb8938`，exit0，真实断言
   1passed，sourceStable/gitStable均true；PID75716已退出。cancel reply
   323ms、worker stop10280ms，源离线后恢复余下7992条，最终全部唯一。
   全程3019.409秒，RSS峰值146886656B；runtime-temp清空后移除，删除量
   与观察free delta均0B。源码冻结限制结束，可以继续产品修复。
   上述是1.0.2容量证据，不能直接写成后续normalizer修复后的性能实测。
4. 使用用户明确授权的脱敏原厂资料和受控来源活库补兼容/WAL/跨库写入
   核对。不能读取私人会话或凭据；公开 shape 合成样本仍标合成通过。
5. 补真实 ACP official/custom × shared/independent、tools 暴露、首次
   prompt 的 100 轮 exact-once 和 UNKNOWN 不重发；Mock 不替代该项。
   真实机器/账号/渠道准备不足时标明具体缺口，继续独立本机工作。
6. macOS/Linux 原平台的导入、发现、WAL、UI 与续聊门禁仍需对应机器或
   CI/受控执行环境。交叉编译不能写成原平台实机通过。
7. 53 项验收表已重写为当前证据，53 个 ID 唯一，87 个 Rust test suffix
   已核对实际执行为 ok；保留各条完整期望与当前断言的差距。
   compatibility/snapshot/MCP 文档也已修正版本、私有快照及合成/真实边界。
   万条和后续桌面结果返回后继续更新，不将文档整理等同功能交付。
   尚需外围实机回归、15 语言完整视觉/译文质量与小 PR 拆分草案。

小 PR 审阅边界草案已记录在
`2026-10-08-session-import-pr-split-review.md`：14 个初始审阅单元，部分
大单元仍需按可编译依赖继续拆分。尚未生成逐 PR patch 或中间树 CI，
不能写成已可发布分支栈。没有创建分支或提交远端。

15 语言的 session-import catalog 静态核对各有 360 个字面键。
菲律宾语 `handoffMeta` 的 `leaf` 标签已改为 `dulo`，V246 全前端门禁通过；其余
非英语 catalog 没有发现长度超过 30 的整条英语同值。
这个检查不代替自然语言质量和原生视觉验收。

Cycle 3 guard 与显式 quit 同时清理 WebView，第二调用方校验已删除文件时
失败。退出记录仍证明家族归零；保留原失败，不改写为通过。收尾 helper
新增每 run/cycle 排他锁，V246 初次及重启真实并发收尾均 exit 0，未重现竞态。
重启使用独立的 restart 进程/
退出/缓存记录和同一 profile，并由 guard finally 收尾；不可覆盖初次证据。

V5 没有要求额外跑 8–24 小时空泛长稳。历史自加的时长不作为硬性交付门；
保留已有耐久证据，优先闭合原始范围中的功能和真实验收。

每轮桌面测试结束（包括失败或中断）立即退出 App/托盘/子进程，确认归零
后清理不用的临时包和缓存。当前可用包、源码/WIP、必要依赖和有用证据
保留；不终止其他对话的进程。

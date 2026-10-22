# 性能基线

2026-10-21已保存到交接分支，当前入口见
[Claude交接H2](../2026-10-21-session-import-claude-handoff.md)。本轮未重新跑性能，
下方千条/64条/DEBUG/Release记录只证明其各自源码和配置，不代表最终V814规模验收。
接手需基于新完整门补混合/部分选择/关联的实际完整发布与取消/恢复，不拼接旧数字。

初始目标定义：2026-09-08。以下目标表保留；历史实测不能直接代表后续源码。

## 2026-10-16：原始输入范围校验与当前千条完整发布

仅将完整快照中的逐成员SQL查询改为一次范围读取，所有原始内容仍逐条流式hash，
成员/root/类型/写后认证不减，无跨事务授权缓存或schema/codec变化。N766验收36项
通过；N767完整六命令通过，标题13/导入1293/全库4297、0fail和strict Clippy，
102/106ignored不计通过。N768同855输入实际opt3编译/发现817.1797999s通过；
这是优化测试程序，不是Release App或完整Release测试套件。

| 指标 | N769普通prepared64条 | N770普通prepared1000条 |
| --- | ---: | ---: |
| wrapper | 12.0033152s | 596.3662102s |
| 完整发布 | 9031ms | 537096ms |
| 发布后逐条验证 | 513ms | 18129ms |
| terminal页 / catalog页 | 2 / 0ms | 29 / 2ms |
| 并发job查询次数 / 最大耗时 | 9 / 3ms | 518 / 61ms |
| 峰值测试进程working set | 21929984B | 50806784B |
| 唯一entry/revision/committed回执/reader | 各64，逐条核对 | 各1000，逐条核对 |

两项精确opt-in测试均实际1pass/0fail/0ignored，sourceStable/gitStable均true。
N769与N760相同64负载：worker单项输入查询46208→1024，新范围读取706次；
完整快照706/header1732及原始BLOB open/hash47940次、132809448B完全相同。
lookup inclusive145.7039→5.7373ms，full-snapshot659.7502→540.5982ms；新range
163.499ms包含读行/hash。worker8866.6793→8310.2285ms，发布9094→9031ms，
后者受1秒轮询粒度影响；单次诊断不证明稳定或因果提速，嵌套时间不能相加。

N770是不间断千条，不与N761取消后恢复799条直接比较。worker仍验证27004header/
27019000member/11002full-snapshot；单项input lookup16000次，所有BLOB hash
11045004次/31459900152B。full-snapshot208.280915s与input-range125.7573379s
是inclusive时间，不能相加。整体仍有二次方扩展问题，未机械启动当前万条。
每会话仅1个合成事件；不是混合来源/部分选择全链、UI首屏、原厂或OS重启证明。

N768/N769/N770原session均exit0并消费、原owner退出，自身空temp都已移除，
各0logicalB/观察free0B。N766空目录0logicalB/观察-4096B；N767自身780临时文件
10502503logicalB/观察free+11464704B，分别记录，不把逻辑大小当实际回收量。
源码/WIP/共享target和依赖/V736/V762/种子/日志保留；已拒绝的N746/V224清理未重试。
run/hash/复现边界与后续计划见任务根plans/v766-input-range-streaming.md。

2026-10-16 N764/N765新增12条混合来源/半数部分选择的完整生产流程合成回归通过，
不再只靠直接写prepared行的容量fixture推断scan/选择语义。这是Debug正确性验证，
不是容量基线或提速数据；没有重测Release64/千条/万条。新2个测试模块与既有测试
模块接线使native输入变为855份；N759优化EXE不含新回归，不能用它声称已跑混合
来源。生产输入相对N757未变，旧优化数据保留原scope。详见外部plans/v763。

## 2026-10-16：当前优化版64条与千条取消/离线恢复

N759绑定完整原生门N757，按仓库release配置编译/发现通过，817.0007383s，
实际opt3/debug_assertions=false；不是完整release测试套件或App包。N760/N761
均实际1pass/0fail/0ignored，853native/source/Git稳定，原进程退出并消费。

| 项目 | N760普通prepared64条 | N761千条取消/源离线恢复 |
| --- | ---: | ---: |
| wrapper | 12.4394566s | 641.2332073s |
| 发布 | 9094ms | 恢复799条466925ms，已有201条保留 |
| 最后逐条验证 | 592ms | 17537ms |
| terminal页 / catalog页 | 3 / 0ms | 32 / 2ms |
| 并发查询最大值 | 29ms | 恢复阶段320ms，取消前58ms |
| 峰值测试进程working set | 21864448B | 50524160B |

N761在201条已提交后取消：87ms返回、1546ms worker停止，原回执逐条不变；
合成源目录离线、缓存重置后75ms完成retry排队，最终1000个entry/revision/
committed receipt/reader event逐条核对一致，再次retry/run_pending不重复。
不是OS重启、原厂扫描、mixed/scoped或UI验收，也不是不间断千条发布耗时。

N760 worker1732header/111808member/706full-snapshot不变，commitment仅64次；
N761恢复阶段仍21577header/21588985member/8791full-snapshot，扩展性问题未解。
单次9094ms比此前N747–N749中位10118ms短10.1%，也接近更早N739的9041ms；
计时插桩/环境不同，不据此声称稳定或因果提速，不重复64或机械启动万条。
八个不重叠publication阶段N760合计8042.0366ms，接近worker-item8043.8035ms；
archive4258.9643ms最大。嵌套计时仍inclusive，不能相加。

N759自身compiler-temp、N760/N761自身runtime都已在owner退出后清理，均0logicalB/
观察free delta0B。fresh再核853native/2574前端/26seed/Git/EXE/166LF文本/40质量门
与owner退出通过。N746的65379328B旧对照副本因清理脚本被自动审批拒绝而暂留，
没有删除或空间回收量；其README有N745/N746重建说明。不重试被拒绝动作。
完整run/hash和限制见任务根plans/v756-member-commitment-review.md。唯一V736不提升；
下节Debug结果属于此前证据，Release“待测”描述由本节的限定结果取代。

## 2026-10-16：发布归因与纯commitment复用，Debug限定对照

N751增加八个顺序且不重叠的publication阶段，子级acceptance/archive计时仍为
inclusive，不能与父级相加。N752 recorder4/timing5/8会话发布1和strict Clippy
通过。N753的主要阶段为archive7459ms、identity4156ms、link3766ms；输入
metadata lookup419ms/open140ms，不支持把小buffer/SQL准备当作主瓶颈。

当前候选只在已验证精确member字节中保留纯commitment结果；当前SQL完整range、
完整root、原始input/allocation和全部写后认证不变，无格式/schema变更。
N754验收35项、N755发布96项/strict Clippy通过；N757完整六命令现已通过，
导入1291/全库4295、0fail、strict Clippy；102/106ignored不算通过。显式旧格式
N758的87项全部通过，26seed及fresh853native/2574前端/Git/EXE/40质量门通过，
原owner退出；当前源码Release性能仍待测。
N753/N756各实际1pass/0fail/0ignored、64个归档/版本/回执/reader均逐项一致，
853native/source/Git稳定，原owner/session均已结束/消费。两轮均Debug诊断。

| 指标 | N753归因基线 | N756纯计算复用 |
| --- | ---: | ---: |
| wrapper / 发布 | 27.9499501s / 24370ms | 27.2592306s / 23381ms |
| worker-run-pending / item | 23447.711 / 21143.237ms | 22900.566 / 20703.164ms |
| 成员range inclusive | 3981.928ms | 2810.085ms |
| 完整snapshot inclusive | 6064.964ms | 5671.463ms |
| header / member / snapshot次数 | 1732 / 111808 / 706 | 1732 / 111808 / 706 |
| 验证 / terminal页 / catalog页 | 723 / 13 / 1ms | 713 / 12 / 1ms |
| 并发查询次数 / 最大耗时 | 24 / 27ms | 23 / 38ms |

N756 worker实际只计算64个commitment、0.7839ms。旧110848次是由1732×64
推导，旧版没有独立commitment计时，不冒认直接测得。range单次减少29.4%，
观察发布减少4.1%、worker减少2.3%；publishMs含observer轮询尾部，不能把
完整4.1%都归为计算收益。没有统计分布，不声称Release/UI提速或解决扩展性。
保留小改动，不重复未变Debug64；等待当前完整门和精确源码优化构建再测。
原N746 Release基线保留；N753精确Debug对照程序在N757通过后已核owner/hash退役，
比较日志/README保留。旧EXE104049664logicalB/观察free+104050688B；N757自身
temp10500335logicalB/观察+11452416B，其他自身空temp已清理，不声称独占物理
回收量。当前V736不提升。详细run/hash/阶段表见任务根plans/v756-member-commitment-review.md。

## 2026-10-16：同事务writer复用，调用减少但未测得提速

本轮只减少同一未写入事务中重复的acceptance header/member-set校验，保留所选
成员原始输入/allocation及写后完整认证。普通创建每条少5次header读取，
去重少1次。N743发布96项、N744显式历史87项及N745六命令/全库4293项均通过；
853native/source/Git稳定，F737前端输入未变。N746在共享target完成相同源码的
release编译/发现，858.8136169s，实际opt_level=3、debug_assertions=false；不是
完整release套件。EXE SHA256：
`283be44f1d26ac0cb24f62b5b934201274c5025513c021982916b2cb69db06eb`。
N746 terminal SHA256：
`59fcd7ff697bd87a8a7273021a00c1af550e6facd6a8e33cedb93130e6f2e3d6`。

N747/N748/N749使用该精确EXE，各实际1pass/0fail/0ignored，sourceStable/gitStable
均true，原exec47081/68642/28839均exit0并消费。每轮64个唯一entry/revision/
committed receipt/reader event均通过真实发布后逐项核对，保留分页、重开不重复和
cache-reset reader断言。仅合成普通prepared批次；冷指全新隔离App home，未清
OS缓存，不含来源发现/厂商解析、UI、mixed/scoped或取消恢复。

| 指标 | 历史N739 | N747 | N748 | N749 |
| --- | ---: | ---: | ---: | ---: |
| 完整测试wrapper | 12.0254873 s | 13.589828 s | 13.7028329 s | 13.6236059 s |
| 发布 | 9041 ms | 10118 ms | 10252 ms | 10077 ms |
| 发布后验证 | 457 ms | 606 ms | 577 ms | 592 ms |
| terminal页 / catalog页 | 3 / 0 ms | 3 / 0 ms | 3 / 0 ms | 3 / 0 ms |
| 并发job查询次数 / 最大耗时 | 9 / 10 ms | 10 / 54 ms | 10 / 191 ms | 10 / 21 ms |
| worker header校验次数 | 2052 | 1732 | 1732 | 1732 |
| worker member校验次数 | 132288 | 111808 | 111808 | 111808 |
| worker完整snapshot次数 | 706 | 706 | 706 | 706 |

新三轮发布中位数10.118s，范围10.077–10.252s；均未优于单次历史9.041s。
header减少320次、member减少20480次，均约15.6%，与审阅推导一致。这个结果
证明少做了重复工作，没有证明端到端提速；缺少同时间窗交替对照分布，也不能
仅凭旧一次/新三次把墙钟差距归因为该代码。二次方扩展仍未解决，不机械重复
同负载或启动千条/万条。N740千条保持旧源码基线，不升级为当前实现验收。

新worker的header inclusive时间1.070–1.081s，完整snapshot inclusive时间
0.705–0.714s，worker-publish-item inclusive时间8.330–8.760s；span重叠，不相加
或相减推导独占CPU。47940次acceptance-blob-hash计时仅0.106–0.107s，且该span
从blob_open之后开始，未包含逐行SQL准备或打开BLOB。先补实际发布阶段和输入
metadata/open的计时，再选择优化；不能将小buffer调整当成已确认主瓶颈。具体
下一步见任务根`plans/v750-publication-cost-attribution-next.md`。

三个runtime-cleanup.json均removed=true，目录实查不存在；各0logicalB/观察
free delta0B。N746自身空compiler-temp同样已删除，0logicalB/观察0B。当前EXE、
共享依赖、immutable seeds、V736、全部日志/失败证据与WIP保留。V736未嵌入此
优化，原53项及真实来源/ACP/三平台/桌面余项继续开放。

## 2026-10-16：优化前基线，普通预处理批次发布

N738在共享target内编译N732相同源码的release测试程序，实际opt_level=3、
debug_assertions=false，源码/HEAD/index/manifest与发现记录已绑定；编译1026.469s，
不是完整release回归或新的桌面构建。精确EXE SHA256：
`df4ca0738a1746f36583e7d576fe3c18689558f1b9579e87da2249cb78edacbd`。
N739/N740各实际1pass/0fail/0ignored，4391filtered不算通过；原session30791/1180
均exit0并消费，源码/Git稳定。每条是单事件合成会话，从已完成prepare批次开始，
执行真实plan/acceptance/worker/归档/回执及cache-reset reader；不含来源发现、厂商
解析、UI或mixed/scoped/取消恢复。冷指隔离App home，未清OS缓存。

| 指标 | N739：64条 | N740：1000条 |
| --- | ---: | ---: |
| 完整测试wrapper | 12.025 s | 696.110 s |
| 发布 | 9.041 s | 639.413 s |
| 发布后验证 | 457 ms | 17171 ms |
| terminal页 / catalog页 | 3 / 0 ms | 35 / 2 ms |
| 并发job查询次数 / 最大耗时 | 9 / 10 ms | 612 / 772 ms |
| 唯一entry / revision / committed receipt / reader event | 各64 | 各1000 |
| 测试进程峰值working set | 21,848,064 B | 50,655,232 B |
| 采样CPU | 7 s | 640.016 s |

千条fixture4577ms、status5ms、首50项6ms、mappings950ms、plan inputs6447ms、
plan4030ms、recovery24ms、commit setup21678ms；IPC分别有界，未枚举preview目录。
terminal/catalog的500ms断言通过；测试未断言并发查询最大500ms，772ms仍是性能
问题。worker有32004次acceptance-header、32019000次member校验、11002次完整
snapshot及48007次job-header解码。相关inclusive时间重叠，不能相加当独占CPU。
单次64/1000测量不能外推准确万条时间，也不能与不同源码/取消负载的N697计算
严格加速比。先减少同一有效snapshot内重复工作，保留完整认证，再决定万条。

证据：任务根`plans/v738-optimized-capacity.md`及N738/N739/N740 run；下一步
`plans/v741-same-snapshot-acceptance-reuse.md`。千条stdout SHA256：
`afb3a9530f6dcf04ca86bc25dcb10f9a85f1d66488cae14b86a1a73ff71e5137`。
运行中做过少量只读catalog计数诊断；保留原始采样，不宣称无干扰基准。
成功运行自身runtime均在owner退出后清理，fixture已清空：各0logicalB/观察free
delta0B；优化编译自身空temp0logicalB/观察+4096B，不宣称大空间回收。必要依赖、
当前EXE/V736/seed/证据保留。原53项及真实来源/平台等范围仍开放。以下为历史：

下方N697/N698均是此前N696源码/DEBUG EXE的实测，不能直接作为当前公开请求
版本校验、持久撤销清理、优化构建或桌面体验的时间。

## 2026-10-15：当前普通1GiB批次，两轮独立导入/一轮热读

N698复用下节同N696源码与精确DEBUG EXE。实际1pass/0fail/0ignored，原session
53050 exit0并消费，总771.368s。两轮各从四份约256MiB合成JSON来源完整复制、
扫描、预览、plan/acceptance/worker发布；未复制imports缓存，未清OS文件缓存。

| 指标 | cold0 | cold1 |
| --- | ---: | ---: |
| 来源字节 | 1,073,774,098 | 1,073,774,098 |
| 扫描 | 88 ms | 82 ms |
| 四份完整预览合计 | 178.777 s | 180.347 s |
| 计划 | 65 ms | 63 ms |
| commit及worker发布 | 204.715 s | 203.667 s |
| 首批后端读取 | 15 ms | 14 ms |
| 发布归档字节 | 1,074,308,566 | 1,074,308,566 |
| 完整imports字节 | 2,175,915,522 | 2,175,874,562 |
| imports/source | 2.0264 | 2.0264 |

热读首页12ms，合成needle搜索1111ms；全测试采样峰值working set46,600,192B。
末尾10ms取消来自单独小排队作业，不是活跃1GiB发布停止/句柄释放验收。上述均为
DEBUG原生后端计时，不代表UI首帧、release或原厂历史格式。没有放宽原3倍磁盘断言。

证据：任务根 `runs/v698-performance-42cb9f7476e440668630ae69b2c2ce84`，完整
scope/owner/收尾见 `plans/v698-gib-and-public-controls-next.md`。fresh原生/前端/
seed/Git/index/EXE不变，owners退出，大样本seed再次CheckOnly通过。自身runtime-temp
在fixture teardown后为空，wrapper移除0logicalB/观察free delta0B；证据和seed保留。

## 2026-10-15：当前 signed 发布路径的千条取消/离线恢复

当前开发树 HEAD 为 `7eb1b7b9f66b149bf7c6a9e3df189caeec062095`，含未提交修改。
N696完整原生门与N697容量测试使用同一837份原生输入及精确EXE：
`257e68e27178909841b555ae0e09eaed344806d1588b7463ed39701ecf085636`。
N697原session43015退出0并已消费，实际1pass/0fail/0ignored；源码/Git/index/EXE
未变，进程已退出。不是下方normalizer1.0.2旧基线的重新标注。

| 指标 | N697，DEBUG 原生合成千条取消/恢复 |
| --- | ---: |
| 完整测试（含计划、取消前发布、恢复、验证） | 3824.417 s |
| 请求取消时 / worker停止时已提交 | 200 / 200 |
| 取消返回 / 实际worker停止 / retry返回 | 342 / 2165 / 292 ms |
| 恢复余800条发布 / 最终验证 | 3038.470 / 22.006 s |
| entry / revision / committed receipt / reader event | 各1000，逐项核对 |
| terminal页 / catalog页后端API | 175 / 3 ms |
| 恢复发布期间最大并发job poll | 358 ms |
| 测试进程采样峰值working set | 61,444,096 B |

原合成来源移为离线后恢复；取消前的committed回执字节保持不变，重复retry和
run_pending不重复发布。fixture从合成已完成prepare批次开始，实际执行plan/
acceptance/worker/归档/回执/reader；不包含发现或厂商解析。计时不代表UI首屏、
release、原厂历史导入或OS重启。没有另跑当前源码的完整千条publish或万条容量。

DEBUG性能仍慢。余800条的worker profile记录25,604次整批header验证和
25,616,000次member校验；缓存命中并未消除当前SQL事实、JSON和整批root的
重复处理。各阶段为inclusive时间，不可累加。下一步需独立记录优化构建的
源码/flags/EXE与原断言，再决定万条测量或具体优化；不得放松认证换吞吐。

证据：任务根 `runs/v697-performance-aacef26b94544ef7a86f01ba1fe516e5` 的
terminal/results/stdout/stderr/samples/cleanup，以及 `plans/v697-thousand-cancellation-run.md`。
自身runtime-temp在owner退出后移除；最终0logicalB/观察free delta0B，无回收量声明。
现有日志、失败证据、源码、依赖、不可变seed和当前EXE保留。以下均为历史基线。

| 指标 | 目标 | 本轮 |
| --- | --- | --- |
| 1 万会话元数据首批 50 条 | ~2s SSD | 未测 |
| 已索引首屏 | ~500ms | 未测 |
| 取消 UI 确认 | ~300ms | 未测 |
| 100MB 单会话 / 1GB 批次峰值 RSS | 记录 | 未测 |

实现已按流式 JSONL、分页 50、索引 sqlite、backup 短事务编写。数字需在发布前用本机 SSD 重测。

## 2026-10-08：当前源码的大输入后端测量

工作树为 `H:\aicoding\grok-session-import-validation\integration-rehearsal-v243`，
HEAD `632f2a8f56bffe8d1c126fae5794f5c3fda316bc`，含未提交修改。
当前 normalizer 为 1.0.2。测试前后 654 份 Rust 输入与 Git HEAD/index 保持不变。
精确测试二进制 SHA-256：
`80421318ce07f65812c217466c7936ea4108306969d7219c1220ec46ca5123c9`。

| 输入和轮次 | 扫描 | 完整预览 | 发布 | 首页后端 API | 进程峰值 RSS |
| --- | --- | --- | --- | --- | --- |
| 100 MiB 单会话，3 轮冷导入 | 65–180 ms | 18.429–18.565 s | 14.407–14.648 s | 14–16 ms | 33.2 MiB |
| 1 GiB 批次（4 × 256 MiB），2 轮冷导入 | 78–81 ms | 188.277–192.334 s | 148.509–149.920 s | 14–17 ms | 36.3 MiB |

- 100 MiB 热读 3 轮：首页 14–15 ms，合成关键词搜索 486–497 ms。
- 1 GiB 热读 1 轮：首页 12 ms，合成关键词搜索 1,153 ms。
- 1 GiB 已排队作业的取消调用确认 9 ms；该小取消样本不是正在发布
  1 GiB 数据时的 worker 停止/清理耗时。
- 100 MiB 发布 revision 104,912,567 B，完整 imports 存储 212,679,379 B，
  源文件 104,860,767 B。1 GiB 发布 1,074,308,566 B，完整 imports 存储
  2,175,800,828–2,175,804,924 B，源文件 1,073,774,098 B。
  完整 imports/source 比例约 2.03，低于测试要求的 3 倍。
- 两个精确测试各进入断言并通过 1 项，退出码 0；未把默认 ignored 或
  错误过滤后运行 0 项算通过。总运行时间分别为 102.828 s 与 683.243 s。
- 冷导入指全新的隔离应用数据和完整发布，未复制导入缓存；没有清空 OS
  文件缓存。RSS/CPU 是被测原生测试进程的测量，不是整个桌面 App 的总量。
- **未测 UI 首帧**。首页后端 API 时间不代表 UI 首次可见时间，亦不证明
  活 Cursor/Claude/Codex 来源兼容性、万条会话容量或真实 ACP 续聊。

当前日志位于：
`H:\aicoding\grok-session-import-validation\runs\v245-performance-fc36e95375bf417a8bafc1ea41402838`。
查看 `terminal.json`、`results.json`、`100mib.stderr.log`、`1gib.stderr.log`
及各自的 `*-samples.jsonl`。进程退出后运行临时目录已移除；保留测量日志
和一份经哈希核对的合成源 seed。回放命令：

```powershell
& 'H:\aicoding\grok-session-import-validation\scripts\session-import-performance-gate.ps1' `
  -NativeRun 'v245-title-fa98e2f14ee04387a64e9d96218f4851' -Version 245 -Focus all
```

只有 Rust 输入仍与该 native gate 一致时才能复用它；源代码变化后须先
生成对应的新 native gate，不要引用旧二进制当作当前性能结果。

## 2026-10-08：千条完整发布和取消/恢复

两项均使用上节同一当前源码 EXE；各精确 opt-in 测试实际 1 passed、exit 0，
sourceStable/gitStable 均为 true。每条会话仅有一个合成事件，不代表真实
长历史。fixture 使用硬链接（不支持时复制）；记录的是逻辑存储字节，
不是物理磁盘占用或实际释放空间。未 flush OS cache，未测 UI 首帧。

| 指标 | 完整发布 P | 取消/离线恢复 K |
| --- | ---: | ---: |
| 全程（含准备/计划/验证） | 258.307 s | 274.579 s |
| 发布阶段 | 176.047 s | 恢复剩余 798 条为 152.407 s |
| 最终逐条验证 | 33.802 s | 35.474 s |
| 取消确认 / worker 停止 | 未取消 | 43 ms / 1,086 ms |
| retry 返回 | 未重试 | 53 ms |
| 最终 entry/revision/committed receipt/reader event | 各 1,000 | 各 1,000 |
| catalog 首页后端 API | 4 ms | 4 ms |
| terminal job 首页后端 API | 5 ms | 6 ms |
| 峰值 RSS（测试进程） | 36,757,504 B | 36,257,792 B |
| 最大并发 job poll | 196 ms | 取消前 57 ms，恢复发布 86 ms |
| 最终逻辑 imports 存储 | 38,959,954 B | 38,935,512 B |

P 包含 prepare status/items/mapping/plan/recovery/enqueue 断言，不能称为
另跑了 prepare-only。K 在 202 条已 committed 时请求取消，worker 停止时
仍为 202 条；原 committed receipts 保持字节相同。将合成源移为离线后
清空应用运行缓存并重试，使用已持久的固定快照恢复余下 798 条；最终
全部结果及 reader 核对，不只检查计数。再次 retry/run_pending 不重复发布。
这是应用缓存重置/worker 恢复，不是 OS 重启。

P run：`v245-performance-b8a591b89a9d4d89ac7739c3236b174a`。
K run：`v245-performance-db0797623d514cd1969c93a9e267faef`。
查看各 run 的 terminal/results/stdout/samples/cleanup JSON。进程均已退出，
runtime-temp 在 harness RAII 清空后由 wrapper 移除，逻辑删除量和观察的
空闲增量均为 0。保留当前测试 EXE、seed 与日志，不生成重复包。

## 2026-10-08：万条完整发布

Run `v245-performance-10511fc0e1bd4660a3410a4f0c76e07e` 已结束，精确
opt-in 断言 1 passed、exit 0，sourceStable/gitStable 均为 true。全部
10,000 entry/revision/committed receipt/reader event 已核对且无重复。
全程 3,153.830 s（52.6 分钟），publish 2,239.798 s，verify 419.301 s。
catalog 首页 23 ms、terminal job 页 69 ms；并发 job poll 最大 513 ms，
不能声称每个查询都小于500 ms。峰值RSS 148,037,632 B、采样最大private
139,460,608 B；最大逻辑存储572,547,416 B，完成387,496,957 B。
每会话一个合成事件，未flush OS cache，也未测UI首帧或真实来源。

PID50920退出后RAII已清空runtime，wrapper移除空runtime-temp；逻辑删除
与观察free delta均为0。保留terminal/results/stdout/stderr/samples与cleanup
证据和当前native测试EXE。万条取消的后续独立结果如下。

## 2026-10-08：万条取消与源离线恢复

Run `v245-performance-dcf7826311b14bd1a833d4d767bb8938` 已结束，精确
opt-in 断言 1 passed / 0 failed / 0 ignored，exit 0，sourceStable/gitStable
均为 true。取消请求时 committed 2,007，worker 停止时 2,008；cancel reply
323 ms，worker stop 10,280 ms，retry 619 ms。来源离线后，使用持久固定
快照恢复剩余 7,992 条，最终逐条核对 10,000 entry/revision/committed
receipt/reader event，无漏项或重复。停止不是立即完成，不能混同两个计时。

全程 3,019.409 s；恢复 publish 1,740.771 s，最终 verify 356.209 s。
catalog 页 22 ms，terminal 页 42 ms；恢复期间最大 job poll 379 ms，
取消前最大 262 ms。peak RSS 146,886,656 B，peak observed private
137,404,416 B；最大逻辑存储 535,248,589 B，完成 387,476,509 B。
每会话一个合成事件；不代表 UI 首帧、原厂兼容或 OS cache 冷启动。
这些是 normalizer 1.0.2 的证据，不直接替代后续行为修复后的验证。

PID75716 已退出，runtime-temp 经 RAII 清空后由 wrapper 移除；cleanup
记录 removed=true、逻辑删除量 0 B、观察 free delta 0 B。保留 EXE、
immutable seed、stdout/stderr、terminal、metrics、samples 和清理证明。

# U04 checkpoint：批准范围清单

后续 App 精确范围选择/原 ACP 批准接线与包装器修复见
[App scope checkpoint](2026-10-03-computer-use-task-consent-u04-app-scope-checkpoint.md)。
本页历史源码/失败/原生证据仍属于各自候选，不改写为后续验证。

状态：**U04 in progress，未交付**。单 Codex writer；未 commit/push/PR/发布。
本检查点记录已完成的范围清单、所选 surface 准备、selector/世代、批准重试、Windows 单根选择回执及本页末尾的组范围/桌面切换核心子步骤，不能替代全范围生产绑定、跨 surface 切换、
增量/缩小授权、真实 App 卡片或全平台验收。U02 的 normal CLI 证据属于其旧冻结候选。

## 实现与边界

严格 transport 路径的 `computer_list_targets` 从当前 owner/agent/epoch 的已 commit、
仍可执行且包含 Observe 的范围生成清单。返回保存的 runId、targetId、title、surface、
targetGeneration；input-only、pending、其它 owner 的范围不进入读取清单。
每个原 identity 只调用其 retained executor 的 `target_alive`，不枚举桌面/标签页，
不读取 DOM/AX/截图，也不根据标题或焦点找替代目标。失效成员省略。

每次外部 liveness 回调前后复验；完成所有回调后，在同一 Broker lock 内复验整表
permit、target/executor generation 和 surface。Stop 或后一个成员回调中的重新授权
不能发布前一个成员的旧记录。permission preflight 使用同一清单路径。
legacy dispatch 的原边界保留；本批没有创建 scope、改变 input-only 权限或放宽跨 surface fence。

全量回归发现 IPC 省略 arguments 时默认 Value::Null，新严格对象校验会拒绝合法的无参 list。
IPC 现将省略 arguments 解析为空对象；显式 null、数组和字符串仍拒绝。
已在真实 IPC/transport 回归中明确断言 list 成功及非法参数失败。

## 验证

证据根：`tools/computer-use-probe/.run/codex-u01r2-20261003/`。
所有批准/目标/回调为明确 **synthetic/FakeAdapter**，不是真人、原生平台或安装版验收。

| 检查 | 实际结果 |
| --- | --- |
| inventory 原三条反例 | `u04-inventory-red-1.txt`，0 pass / 3 fail，保留原输出 |
| 首次实现编译 | `u04-inventory-green-1.txt`，private live_context 导致失败；未算成功 |
| 定向回归 | green-2 三条、green-3 六条通过；后加入第七条晚成员回调/早成员世代反例 |
| 首次 Core 全量 | `u04-core-full-inventory-4.txt`，666 pass / 1 fail，省略 arguments 问题；未覆盖旧日志 |
| 修复后 Core 全量 | `u04-core-full-inventory-5.txt`，667 unit + 220 integration = **887 pass**，0 failed/ignored，Cargo exit0；doc/bin 0 项不计 |
| Core strict Clippy | `u04-core-clippy-inventory-5.txt`，all-targets/test-support/offline/-D warnings，exit0 |
| 默认 App 生产 check | `u04-app-production-inventory-5.txt`，cargo check --lib --offline，exit0 |
| MCP | `u04-mcp-inventory-4.txt`，37 pass，0 failed/skipped，exit0 |
| quality | `u04-quality-inventory-4.txt`，PASS；最近 diff check exit0 |

inventory 候选源子集：`src-tauri/computer-use-core/src`、`src-tauri/src`、
`tools/computer-use-mcp`、`src-tauri/cu-probe`、`tools/computer-use-probe/consent-fixture.mjs`。
`git ls-files --cached --others --exclude-standard` 后 PowerShell Sort-Object -Unique，
每行 `<lowercase SHA256>  <repo path>`，LF结尾，共642文件，清单 SHA256：
`c129ace1644810c2119edee4be0a1d917979a8a79778f4ff5b4d90bea73c9991`。
这是 runtime 准备改动之前的源码子集，不是 frontend/packaging/完整产品清单。
inventory 的 App strict Clippy `u04-app-clippy-inventory-5.txt` 已 exit0。
后续候选 App 回归分别记录，不据旧 U02 测试算新候选通过。

## 所选 surface 准备（第二子步骤）

AccessApproval/PrepareTicket 现在冻结所选 surface；实际准备走同一 SurfaceRouter，
只有完整匹配当前预约且处于 Pending 的原 run 能进入 staging 分支。模型和 legacy
仍走原 require_owner，未提交前读取/动作继续拒绝，cross-surface 检查未取消。
最终 commit 也复验冻结 surface。

App 新增 execution_runtime 领域模块。任务批准只准备所选 Desktop、ManagedBrowser、
ExistingTab 或 WebView；复用原 Desktop singleton，注册/worker 启动失败返回错误，
不再把 ensure_host_runtime 的日志当成功。不预建其它执行器、枚举目标或开 listener。
首用写偏好、发布、准备、commit 和原 worker rollback/join 的顺序保留。
新增失败回归核三处 feature 状态、零 grant/continuation/读取/动作。
新的多 surface synthetic helper 仍只在 test/test-support 编译；生产 Bound 仍未完成。

| 检查 | 实际结果 |
| --- | --- |
| WebView 原反例 | `u04-surface-red-1.txt`，0 pass / 1 fail，原路径误用 Desktop |
| 首个定向绿 | `u04-surface-green-1.txt`，该反例通过；该命令前 fmt 因新 tests 模块尚缺文件失败，未计 fmt 成功 |
| 首次全量 | `u04-core-full-surface-2.txt`，668 pass / 2 fail；两条旧 Stop-gap 测试误要求已退休预约还能绑定成功 |
| 修订 Stop-gap | 保留原 late-run cleanup/epoch/no-authority 全部断言，增加 bind 返回 StopRequested 和零 discovery，未删测试 |
| Core 全量 | `u04-core-full-surface-3.txt`，670 unit + 220 integration = 890 pass，0 failed/ignored，exit0 |
| Core / App strict Clippy | `u04-core-clippy-surface-3.txt`、`u04-app-clippy-surface-3.txt`，各 exit0 |
| App 默认 lib harness | `u04-app-build-surface-3.jsonl/.stderr.txt`，exit0；从此 invocation 唯一 grok_app_lib artifact 嵌入现有 Windows manifest |
| App CU | `u04-app-computer_use-surface-3.txt`，153 pass，0 failed/ignored；默认 features，非旧 U02 probe features 的173项 |
| App ACP / routing | `u04-app-acp_client-surface-3.txt` 130 pass（skip live_handshake）、`u04-app-session_manager-routing_tests-surface-3.txt` 38 pass；各 exit0，filters不能加总 |
| MCP / quality / diff | `u04-mcp-surface-3.txt` 37 pass；`u04-quality-surface-3.txt` PASS；diff check exit0 |

surface 候选同一清单算法，共644文件，SHA256：
`5a86fb4aed09f0f0cda9c4e89a1f3ac7e24326519c7ab07a7f2a6e771051a6e2`。
App harness `target/debug/deps/grok_app_lib-dc611c8f8bc4f678.exe`，manifest 后 SHA256：
`9f5c25cf92de1a51c83046d803d2fc82de2a3f51c1040bd6f1aa89f103fa9dcd`。
此候选后续已被 selector/准备世代改动推进，不用它替代新候选验证。

## 明确目标选择与准备世代（第三子步骤）

MCP 目录给 status/list/open/observe/handoff/stop 和 browser 路径增加可选 runId/targetId。
单 grant 无参兼容；多个 grant 不猜焦点，模型从批准清单复制明确身份。
共享 typed claim parser 在 permission/实际 dispatch/inventory 三处一致地拒绝 null、
非法类型、空白、超长及重复 aliases。否则 malformed 参数曾被当成 absent，借用唯一 grant。
请求工具仍禁止携带模型目标身份。open_target 只是原已授权目标回执，未实现持久切换/激活。

新增实际两 grant 回归：模糊默认拒绝，A-run/B-target 错配拒绝，明确B的 preflight 不读取，
真实 transport dispatch 只观察B一次。它不是 A→B→A 输入/租约切换验收。
prepared native binding 现在保存确切 target/executor generation；最终原子 commit 核对
这份回执，不能使用后来同 targetId 的重新授权。晚回包仍归原预约清理。

| 检查 | 实际结果 |
| --- | --- |
| selector 编译首败 | `u04-selectors-red-1.txt`，测试误用不存在 cleanup helper；修成原 Broker tickets 循环 |
| selector 业务红 | `u04-selectors-red-2.txt`，0 pass / 1 fail；null selector 实际获得 preflight 允许 |
| selector 绿及全量 | `u04-selectors-green-1.txt` 1 pass；`u04-core-full-selectors-2.txt` 672 unit + 220 integration = 892 pass，0 failed/ignored，exit0 |
| MCP schema | `u04-mcp-selectors-2.txt` 38 pass，0 failed/skipped，exit0 |
| prepared 世代红 | `u04-prepared-generation-red-1.txt`，0 pass / 1 fail；旧决定实际 commit 到新的同ID绑定 |
| 最终 Core 全量 | `u04-core-full-prepared-2.txt`，673 unit + 220 integration = **893 pass**，0 failed/ignored，exit0 |
| 最终 Core strict Clippy | `u04-core-clippy-prepared-2.txt`，all-targets/test-support/offline/-D warnings，exit0 |

prepared-2 历史候选同一清单算法，共645文件，SHA256：
`fc8104c1a28c4ec872787867fccb6a95186055df5ddf9e2546633d82b4b3db49`。
此清单仍只覆盖上述源码子集。

| prepared-2 候选联合验证 | 实际结果 |
| --- | --- |
| App 默认生产 check | `u04-app-production-prepared-2.txt`，exit0 |
| App strict Clippy | `u04-app-clippy-prepared-2.txt`，all-targets/offline/-D warnings，exit0 |
| App 默认 lib build | `u04-app-build-prepared-2.jsonl/.stderr.txt`，exit0，取该 invocation 唯一 grok_app_lib artifact 后嵌入现有 manifest |
| App CU / ACP / routing | `u04-app-computer_use-prepared-2.txt` 153 pass、`...acp_client...` 130 pass（skip live_handshake）、`...session_manager-routing_tests...` 38 pass；各 exit0，0 failed/ignored；不同 filters不能相加 |
| MCP / quality / fmt / diff | `u04-mcp-prepared-2.txt` 38 pass；`u04-quality-prepared-2.txt` PASS；fmt --check/diff --check exit0 |
| normal CLI 构建 | `u04-normal-cli-build-prepared-2.jsonl/.stderr.txt`，exit0，取此 invocation 的唯一 cu_probe artifact 后嵌 manifest |
| normal CLI strict Clippy | `u04-normal-cli-clippy-prepared-2.txt`，all-targets/offline/-D warnings，exit0 |
| normal CLI 执行 | `u04-normal-cli-attempt1.jsonl/.stderr.txt`，exit0 / oracle pass；新隔离目录 `codex-u04-cli-20261003-attempt1`，未覆盖 U02 |

App harness manifest 后 SHA256：
`3386e867cff787580ad5b1befbad3cd8907a6d063485994353e661cd2085da71`。
cu_probe `target/debug/cu_probe.exe` manifest 后 SHA256：
`bbf0f0c065dc669d31144c191cddf2de5dda8a1867836d14e98000c321fb7bbd`。
CLI 仍为 1.0.46 (2765805b9442)、grok-4.7 / low、default/ask；无 always-approve，
leader/subagents关闭。实际普通工具两次确认/两次效果；未批观察一次 reject、零读取；
申请 pending 时零 grant/读取；synthetic staged commit 一份 grant/一张 continuation；
typed stdin admission 后两次 fresh observe、零 actions；重复原 delivery NotStarted，
ledger 不可再领取。continuationRecords=1 包括已消费记录，不代表 pending。
原 CLI EOF退出0且原 child.wait/join，无 tree kill；原 Host workers joined。
原 CLI PID66436及普通 fixture PID56196只读回查均缺失。
仍属于 normal CLI + production ACP/MCP 接线、**synthetic Host/FakeAdapter**，普通确认
使用固定无害 policy，不是真人卡片、真实生产 Bound、完整 App dispatcher 或原生/安装版验收。

## 下一小步与完整剩余范围

1. 收齐当前 App/CLI 自动化证据，保留原失败和二进制来源。
2. 生产 Host Bound 和 picker 候选凭证：身份来自真实 adapter，不从字符串或 synthetic=false 产生。
3. 一次多成员批准、真实 membership、切换退休旧 refs/geometry 与原 admission 收尾；
   scope 外拒绝、lease 冲突、增量失败不伤已有范围、缩小立即 fence。
4. observe catalog 已支持明确 selector；open_target 仍没有持久切换事务。
   当前多 grant 默认严格歧义，不能宣称多目标完整可用。
5. U04 后 U03 请求卡/15 locales/实际手势联合验收，再 U05–U09 / D00–D31 全范围。

额外生命周期待查：App shutdown 的 fence_all 仍可能通过 set_feature_enabled(false)
同步等待 native cleanup；feature_lifecycle orphan spawn_blocking 需确认原句柄 join。
目前只读发现，尚无 native hang 证据，不称已修；必须保留 cleanup tickets/原 owner。

## 同一请求批准失败后的显式重试（第四子步骤）

实际复现：第一次所选 adapter 绑定失败，批准回滚后 request 回到 pending；
再次手动批准同一 request/revision，却复用了原已停止的 execution run，返回 StopRequested。
无论原 cleanup 已完成还是尚未完成，两条反例均失败；原日志保留。
每个 preparation 现在拥有唯一 execution run identity，原失败 run 和 cleanup ticket 保留。
已 commit 的重复批准仍返回原 run/continuation，不另分配尝试，也没有自动重试副作用。

| 检查 | 实际结果 |
| --- | --- |
| 批准重试原红 | `u04-retry-red-1.txt`，0 pass / 2 fail，两个场景均实际返回 StopRequested |
| 批准重试绿 | `u04-retry-green-1.txt`，2 pass；原 cleanup/cancel/abort 晚到后，新 run 仍可两次严格 transport observe，零输入；重复批准只有一份 grant/continuation |
| Core 全量 | `u04-core-full-retry-1.txt`，675 unit + 220 integration = **895 pass**，0 failed/ignored，exit0 |

上述仍是 synthetic Host/FakeAdapter，不能据此称生产 Bound 已完成。prepared-2 的 App/CLI
二进制来自修改前候选，新源码的 App/原生联测随后单独记录。

## Windows 出生身份与不可伪造选择回执（第五子步骤）

Windows targetId 现在冻结原 process creation FILETIME、PID、原根 HWND 与 window stamp。
不可访问/已退出进程不能生成 identity；GetProcessTimes/活性检查使用同一打开的 process
handle，查询后关闭。原生 WindowInstance 也保存出生身份，在原回调前后复验，并保留
原 TID/root/window-instance 检查。旧 `win:pid:hwnd:stamp` 格式需要重新选择，不能继续执行。
这不是强制 PID/HWND 回收的实测，也不是 Win32 原子执行保证。

Core 新增 typed NativeScopeIdentity 和私有 HostScopeSelection。Host 显式选中目标后，
只向所选 adapter 取得原生 identity metadata；没有枚举/DOM/AX/截图/输入，不开 run 或启用
feature。选择回执冻结原 Broker、完整 transport owner/request/revision、选择轮次、feature
epoch 和 executor generation；不能从模型 JSON 反序列化、复制或构造。
刷新选择会退休旧回执；跨 Broker/owner、Stop、过期及旧 epoch 不可使用。
接受选择与生成原 approval reservation 在同一权威锁内，不存在另一选择偷偷替换目标的空隙。

native proof 在 prepare 前后、最终 commit 前以及严格模型调用的 preflight/dispatch 复验。
所有 native 查询在 Broker lock 外，随后重新检查权威状态；Stop 仍可收束死目标。
pending/准备中不生成 grant/continuation/scope memory，native bind 失败仍由原尝试清理。
重复已 commit 的批准继续返回原结果。synthetic scope witness 只在 test/test-support 编译，
仍输出 SYNTHETIC_DECISION_CLASS；真实 native witness 的 classification 是 host-native-scope，
它不是“真人验收通过”声明。

当前仅 WindowsRoot 已提供真正 platform producer；其余 adapter 的默认 scope_identity 明确
unavailable，不用 target_alive、title 或 bool 替代 proof。App 卡片/picker 的实际手势接线、
多成员事务及完整 native commit/执行链仍开放。真实隔离 Win32 测试只覆盖原生选择/预约及
退休拒绝；该组没有执行 native grant commit，不冒称安装版 App 或真人授权。

| 检查 | 实际结果 |
| --- | --- |
| Win32 原反例 | `u04-native-birth-red-1.txt`，25 PASS / 1 FAIL，exit1；旧 PID-only 格式实际写入 owned EDIT；原构建 `...red-build-1.jsonl`、manifest 后 binary SHA256 `001f0e139fdad8c84f930dc6ea78403ef1f35c2509f6469962f8ec9fc0e704e9` |
| 出生身份首绿 | `u04-native-birth-green-1.txt`，27 PASS，exit0；修改出生值拒绝，不声称实际 PID 回收；此候选随后被选择回执推进 |
| 首次 App strict Clippy | `u04-app-clippy-native-birth-1.txt` exit101，生产构建的旧三字段 parser 无调用；将该 helper 限于 test/probe 编译后 `...native-birth-2.txt` exit0，没有关闭 warning |
| 选择事务定向 | `u04-scope-receipt-tests-1.txt`，6 pass，exit0；全为明确 synthetic witnesses；覆盖无 witness、刷新、跨 Broker、Stop、prepare/commit 替换、commit 后零越界读取 |
| Core 全量 | `u04-core-full-scope-receipt-1.txt`，681 unit + 220 integration = **901 pass**，0 failed/ignored，exit0 |
| Core / App / probe strict Clippy | `u04-core-clippy-scope-receipt-1.txt`、`u04-app-clippy-scope-receipt-1.txt`、`u04-probe-clippy-scope-receipt-1.txt` 各 exit0；all-targets/offline/-D warnings，Core/test-support |
| 最新原生 probe | `u04-scope-receipt-native-build-1.jsonl/.stderr.txt` 构建 exit0；`u04-scope-receipt-native-1.txt` **28 PASS**，exit0；真实 WindowsRoot metadata → Core 私有回执/预约、未启用/零输入，以及原 root stamp 退休后拒绝 |
| normal CLI | `u04-scope-receipt-normal-cli-attempt1.jsonl/.stderr.txt`，exit0 / oracle pass；独立目录 `codex-u04-scope-receipt-cli-20261003-attempt1`；普通确认两次/两次效果、未批 observe 一次 reject、批准后两次 fresh observe/零 actions、原 delivery 重复 NotStarted/ledger empty；原 CLI/Host workers joined，treeKill=false |
| MCP | `u04-mcp-scope-receipt-2.txt`，38 pass，0 failed/skipped，exit0；首个 golden-only 命令 `...receipt-1.txt` 仅6项，未当作全量 |
| quality | `u04-quality-scope-receipt-2.txt` PASS；首个命令误用不存在的 mjs 文件，`...receipt-1.txt` exit1 保留，没有把它当业务绿 |

scope-receipt-1 源子集共649文件，同前述算法，SHA256：
`977cb6684f33791ff6cb263579dabd7c7c8abc90c1ab8a3328ca5f5f316be5d2`。
原生 probe / normal CLI 共用该构建 `target/debug/cu_probe.exe`，manifest 后 SHA256：
`7fca2848be10c3354641dc6d58cf5fb50096b1d03f4d7925fbd158cc10592a6c`。
CLI 仍为 1.0.46 / grok-4.7 / low / default-ask，无 always-approve，leader/subagents off；
Host 决定和执行器仍是 synthetic/FakeAdapter，不是此处的真实 Win32 producer。
原 CLI PID69272、ordinary fixture PID55972 执行后只读回查缺失。

### App 并发回归暴露的测试隔离问题

同一 scope-receipt-1 候选的默认 features App harness
`u04-app-build-scope-receipt-1.jsonl/.stderr.txt` build exit0，manifest 后 SHA256
`9563087fa2e1ac98f6780afe2f1f3f9e19290f783438d424bc79d65202ee1186`。
宽 filter `computer_use` 在四 test threads 下，`u04-app-computer_use-scope-receipt-1.txt`
实际177 pass / 2 fail，exit101：feature_lifecycle/task_access/feature 用例共享进程级 feature
bool、choice revision，彼此的显式关开导致 superseded 或 rollback 等待超时。
不是 native proof 测试通过后假报 App 绿，也没有增加 timeout/忽略测试/删原断言。

新增 test-only 统一 Tokio mutex，独立 fixture 持有到原 worker 结果及 Drop cleanup 收束；
feature 的同步测试和 feature_lifecycle 的两组用例也使用同一锁。每个测试场景内部原本的
异步关开、Stop、native barrier 和 caller cancellation 仍真实并发，生产控制路径未修改。

| 修订后 App 验证 | 实际结果 |
| --- | --- |
| 默认 lib build | `u04-app-build-scope-receipt-2.jsonl/.stderr.txt` exit0，artifact features=[]；取此 invocation 唯一 grok_app_lib test artifact 嵌 manifest |
| 宽 CU filter | `u04-app-computer_use-scope-receipt-2.txt`，**179 pass**，四 threads，0 failed/ignored，exit0；filter 比旧 checkpoint 的 CU 模块过滤更宽，不把数量增长全算新增 |
| ACP / routing | `u04-app-acp_client-scope-receipt-2.txt` 130 pass（skip live_handshake）、`...session_manager-routing_tests...` 38 pass；各四 threads、0 failed/ignored、exit0；filters不能相加 |
| App strict Clippy | `u04-app-clippy-scope-receipt-2.txt`，all-targets/offline/-D warnings，exit0 |

仅测试隔离修订后的源子集仍649文件，SHA256：
`53b4d0acc6581c393d652e6bbe7d32370c0dfe3a247a7afb0fbf1de007877576`。
默认 harness 仍为 `target/debug/deps/grok_app_lib-dc611c8f8bc4f678.exe`，manifest 后 SHA256：
`6fb21ab70dea8bf9245a45975143769b853f113f95f227a4d943748b2539bfa1`。
上述原生/CLI 运行来自隔离修订前构建，未冒称它们从修改后的 test source 重新构建/重跑。
当前没有未收的构建或 CLI；未提交/发布。U04 及完整 goal 继续 active。

### 后续范围合同注意

Managed 的 profile config 选择是不可执行的候选，首次 owned context 的真实证明需要在
同一 preparation 中创建并验证；不能把 config id 直接标成 native context witness。
ExistingTab 要核真实 pairing/Share/实例，WebView 要核原 NativeView instance，不能靠 label。
它们的稳定 scope member identity 与 document-scoped execution binding 必须分开，普通导航
应退休旧 refs/文档并 fresh observe，不能因此让用户重新批准整个范围。
这些合同与一次多成员批准/切换、增量拒绝保留旧范围、缩小立即 fence 一起继续实现，
U03 实际卡片/15 locales 仍在 U04 完整生产范围之后；没有缩减 U05–U09/D00–D31。

## 组范围与当前执行目标分离（Codex 接手续行）

这是 U04 的核心增量，**不是完整 U04 或产品交付**。没有新开 writer、提交或发布。

Request 的 Bound、Preparation、私有 HostScopeSelection 和最终 ScopeMemory 现在保存完整成员组。
`capture_task_scope_group` 一次冻结最多 8 个明确实例，拒绝空组、重复项、超上限、未请求的
surface 和分类不符。所有成员都要有真实 adapter witness；每次准备前后及最终 commit 前
复验整组。没有用标题、同 PID 或 liveness 代替原生身份；未选成员不进入 native 查询。
已有单成员 Host API 走同一组事务。准备没有 grant/continuation，最终一次 commit 产生
一份含全部成员的 grant 和一张原 typed continuation；重复批准返回原结果。

Grant 的 `active_target_id` 与批准 `members` 分开。严格模型清单从成员组读精确 identity
metadata；不枚举 ambient targets/读取内容。只给当前可执行成员 execution generation；
非当前成员 `active=false`、`targetGeneration=null`，不会拿当前 run 的截图身份冒充它们。
不活动成员暂用 opaque target id 作展示名；真实 picker 的显示名接线仍在 U03/U04 后续。

严格 transport 的 `computer_open_target` 现在执行 Desktop→Desktop 范围内切换，而不是回执。
先检查成员与 owner，再在 Broker 锁内占有切换事务，取消旧 admission、推进 run generation、
退休 snapshot/refs/geometry。原 retained adapter 清理、原 worker/admission 与 native occupancy
都终态之后才释放旧 target/lease，并按原 file lease 合同取得新目标。原生工作不持 Broker 锁。
失败保留批准范围，但执行保持隔离；下一次明确切换可在原工作确认终态后重试。
Stop/feature-off/owner retire 胜出时，迟到 claim/绑定不能发布。不会拿超时或 abort 返回当作 idle。
用户暂停时模型不得借 open_target 恢复或抢焦点。普通 Host/legacy authorize/resume 也不能绕过
尚未收束的切换。范围外 C 在 metadata/capture/input/cleanup 前拒绝，当前 A 不受影响。

一个 run 保留全程 action ledger、预算和 grant revision；切换不增加票据或新 run。
ExecutionPermit 新增冻结的 execution generation，A→B→A 之后原 A permit 仍失效。
已有 actionId 重复交付遵守原 idempotency 合同：回读历史结果及历史 generation，物理效果不再发生。
新目标在 fresh observe 前没有 snapshot/ref/input。旧两份独立 grant 的歧义默认仍拒绝。

### 实际失败与修正

- `u04-group-red-1.txt` 是新测试 helper 参数错误造成的编译失败，不算业务红。
  修正参数后 `u04-group-red-2.txt` 实际 1 pass / 1 fail；旧清单对第二成员返回 revision mismatch。
- `u04-group-green-1.txt` 是遗漏 LeaseError 映射的编译失败，`...green-2.txt` 两条业务回归通过。
- `u04-group-core-lib-1.txt` 实际 682 pass / 1 fail：新增 permit-generation 读取中的 `?`
  提前返回，跳过过期 run 的原 cleanup handoff。将查询局限在结果闭包内，所有错误仍先交出清理。
  没有删掉到期断言，也没有把 Running 改成通过。
- `u04-group-switch-stress-1.txt` 实际 5 pass / 2 fail。lease release 的 Held 被错误映射为 Adapter，
  改为准确 lease_held；另一个新测试误以为 actionId 去重必须返回错误，校正为原历史结果合同，
  并检查旧 generation 和独立物理效果计数。原效果断言保留，不放开重放。

### 本候选验证

| 检查 | 实际结果 |
| --- | --- |
| 定向 scope | `u04-group-scope-2.txt` 24 pass，0 failed/ignored，exit0；含既有用例，不能把 24 全算新增 |
| Core 全量 | `u04-group-core-full-2.txt` **691 unit + 220 integration = 911 pass**，0 failed/ignored，exit0；含原过期 cleanup、清单世代及两个独立 grant 回归 |
| Core strict Clippy | `u04-group-core-clippy-1.txt`，all-targets/test-support/offline/-D warnings，exit0 |
| MCP | `u04-group-mcp-1.txt`，38 pass，0 failed/skipped，exit0 |
| quality / diff | `python scripts/check-code-quality-gates.py --mode final` PASS；`git diff --check` exit0 |
| 默认 App build | `u04-group-app-build-1.jsonl/.stderr.txt` exit0，唯一 grok_app_lib test artifact，features=[]；已嵌 Windows test manifest |
| App CU / ACP / routing | `u04-group-app-computer_use-1.txt` **179 pass**；`...app-acp_client-1.txt` 130 pass（skip live_handshake）；`...app-routing-1.txt` 38 pass；各四 threads、0 failed/ignored、exit0，重叠 filters 不能相加 |
| App strict Clippy | `u04-group-app-clippy-1.txt`，all-targets/offline/-D warnings，exit0 |

上述 source 子集同本页前述算法，共651文件，SHA256：
`6b19a81e54197b0342d1854a7c670d003cab57251914e320405a642e6be41da2`。
默认 App harness `target/debug/deps/grok_app_lib-dc611c8f8bc4f678.exe`，manifest 后 SHA256：
`c8412a4a6b3c3fc0e46501b103b0b1816ba3935a19af16895e05c2766604053e`。
没有冒称 prior native probe/normal CLI 从此候选重跑；本批 source/runtime 证据与之前候选分开。

所有新 scope/switch 业务回归都是明确 synthetic Host/FakeAdapter，不是真人、native Windows
效果、安装版 App 或真实 Grok 任务验收。Windows producer 的真实整组 native commit/动作仍待
隔离纵向验收；真实 App picker/请求卡、关联 dialog、其它平台 witness、三个浏览器 surface 与
跨 surface 子 binding、增量合并/执行中缩小和 U05–U09/D00–D31 完整范围继续开放。

## 精确目标信息与原生组拒绝验证（本次接手）

发现一个真实范围泄漏入口：所选 Desktop 的 preparation 仍调用旧 `list_targets_for_run`，
为了取一个标题重新枚举整个桌面。已有组回归加强为：Host 已选 A/B 后关闭 discovery，
准备/commit、两成员清单、切换和 Host readback 全程仍应成功，枚举调用数必须为 0。
`u04-selected-metadata-red-1.txt` 实际 0 pass / 1 fail，原准备返回 enumeration unavailable。

新增 adapter `scope_target_info(run_id, target_id)`，默认明确 unavailable，绝不默认枚举。
生产 Windows 只读取所选原根的 caption 和原进程的必要名称；查询前后复验出生身份、根、
window stamp/TID、可见性、UIPI/Host 保护。Broker 核原 witness、executor generation、
target/surface/backend 与有界标题。标题只是展示数据，不成为授权 identity。
有生产 witness 的 Desktop preparation、批准清单、切换与 Host readback 走同一精确路径。
Legacy picker/测试 Bound 保留自身兼容合同；其它平台的精确 metadata 未接时明确拒绝。
真实 witness 的非当前成员不再用 opaque targetId 冒充可读标题。

身份查询期间 Stop 也可能胜出。metadata helper 现在在 proof 查询前、标题读取前及回包后
复验原 preparation/permit/binding，回包不能重新建立权限。独立反例在第二次 proof 查询中
Stop：暂缺标题前 fence 时 `u04-selected-metadata-fence-red-1.txt` 实际 0 pass / 1 fail，
selected metadata 调用数 **1 而非 0**；恢复 fence 后保留零读取断言，全量绿。
另有 metadata 回调期间 Stop/原 witness 退休的回归，均不产生 grant/continuation/输入。
它们是 synthetic Host/FakeAdapter；不声称 Win32 查询和异步 Stop 物理原子。

### Windows 隔离环境的实际限制与失败记录

私有非交互 station 不支持可见窗口。`ShowWindow` 与创建时 `WS_VISIBLE` 均无法令
`IsWindowVisible` 返回 true（实际 style `94000044`），生产校验正确拒绝。
未切换到 WinSta0、用户桌面或关闭可见性/input gate，没有造一个 native happy path。

- `u04-selected-metadata-native-1.txt/.stderr.txt`：旧 28 PASS / 新 2 FAIL，exit1；
  对应 build-2，manifest 后 SHA256 `cb06fe683f7a4abb3fcf5dac1fba8932447ac48f4a7e223f08d45cc484d44d4b`。
- `...native-2.txt/.stderr.txt`：28 PASS / 2 FAIL，exit1；改为创建时 WS_VISIBLE 仍不可见；
  对应 build-3，SHA256 `c488abf77552008b85866678dfce6f2a4e3bf2165ccfc61c975f91f51a5f9587`。
- `...native-3.txt/.stderr.txt`：错误地在 build-4 尚未结束时执行了同一 build-3 二进制，
  同 hash / 28 PASS / 2 FAIL / exit1。保留原记录，**不计新候选验证**。
- 将隔离探针明确改为其能验证的生产负向合同：真实两根 Host selection/整组 reservation，
  隐藏根准备准确拒绝/回滚；非当前根 stamp 在原 HWND 上实际退休后整组 reservation 拒绝。
  独立原窗口 procedure 计数：所选/范围外 caption 读取与输入均为 0；无 grant、scope memory
  或 continuation，原 run cleanup 必须 Stopped，所有 owned HWND/私有 station 由原 guard 关闭。
  这 **不是整组 native commit/五次执行切换/fresh capture/输入成功**，这些验收仍开放。
- probe strict Clippy 首次 `...probe-clippy-1.txt` exit101（测试回调签名 type_complexity）；
  按现有 probe 模式提取 `GateRunner` type alias 后通过，没有 suppress warning。

### 最终源码与验证

沿本页相同算法，source 子集共 **653 文件**，SHA256：
`87edfa7731e762070312bec2db9626022c0667700c6813700fc1b48d0dfaffe5`。
仍不是全产品/全部资源清单；此 hash 后只编辑文档，不改源码。

| 检查 | 实际结果 |
| --- | --- |
| Core 全量 | `u04-selected-metadata-core-full-4.txt`，**693 unit + 220 integration = 913 pass**，0 failed/ignored，exit0；较前一组新增 2 个业务测试，已有组回归加强 |
| Core / App / probe strict Clippy | `...core-clippy-2.txt`、`...app-clippy-2.txt`、`...probe-clippy-3.txt`，各 all-targets/offline/-D warnings，exit0；Core/test-support，App 默认 features |
| 最终 probe build / 原生 | `...probe-build-5.jsonl/.stderr.txt` exit0；取 invocation 唯一 cu_probe artifact 嵌 manifest；`...native-5.txt/.stderr.txt` **30 PASS**，exit0；包含原 28 与新增 2 个真实负向组检查 |
| 默认 App build | `...app-build-2.jsonl/.stderr.txt` exit0；唯一 grok_app_lib test artifact，features=[]，已嵌 manifest |
| App CU / ACP / routing | `...app-computer_use-2.txt` **179 pass**，`...app-acp_client-2.txt` 130 pass（skip live_handshake），`...app-routing-2.txt` 38 pass；各四 threads、0 failed/ignored、exit0；filters 重叠不能加总 |
| fmt / diff / quality | `...fmt-final-1.txt`、`...diff-final-1.txt` exit0；`...quality-2.txt` final PASS |
| normal CLI | 最终候选 `...cli-2.jsonl/.stderr.txt` exit0/oracle pass；普通确认两次/两次效果，未批 observe 一次 reject，批准后两次 fresh observe/零 actions，原 delivery 重复 NotStarted/ledger empty，原 CLI/Host workers joined、treeKill=false |

表中 `...` 前缀均为 `u04-selected-metadata-`，日志根仍为本页既有 `.run/codex-u01r2-20261003/`。
最终 `target/debug/cu_probe.exe` manifest 后 SHA256：
`d23b3652e02c17b9c573e354139fd1986034e14ed155044b623ee27ce49a7627`。
最终默认 App harness `target/debug/deps/grok_app_lib-dc611c8f8bc4f678.exe` manifest 后 SHA256：
`94185ba3b85c3a4901a503711ab5cc2e2bb5879b643e4be1bde9a77e0cc18844`。

最终 CLI evidence 独立目录 `codex-u04-selected-metadata-cli-20261003-attempt2`；
CLI 1.0.46 / grok-4.7 / low / default-ask，leader/subagents off，无 always-approve。
它是生产 ACP/MCP + synthetic Host/FakeAdapter 单目标兼容回归，不是组切换、native 输入
或真人 App 验收。原 CLI PID73976 与普通 fixture PID77756 均已原 owner wait/join，
结束后只读回查也缺失。此前 group/native-1 与 metadata/cli-1 属各自旧构建，未冒领最终产物。
当前没有未收的构建、Clippy、测试或 CLI session。

U04/完整目标保持 active，未提交/push/PR/发布。下一步优先接 App 的原 ACP transport 到
私有 HostScopeSelection/整组批准入口，再完成真实可见窗口纵向链；其它平台 witness、关联
dialog、浏览器稳定范围/document 子 binding、增量原子合并/缩小与 U03、U05–U09 继续保留。
App shutdown 同步 native cleanup 与 feature_lifecycle orphan blocking worker join 的遗留审查项
也未在本批宣称解决。完整安装版、真人授权、跨平台、Grok 联合验收和最终长稳仍不具备。

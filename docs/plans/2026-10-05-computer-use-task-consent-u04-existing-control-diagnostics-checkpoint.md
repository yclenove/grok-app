# ExistingTab：control首败跨Node退出的窄诊断检查点

本批已完成验证与清理，原session90856已exit101；权威终态见
[source54结果](2026-10-05-computer-use-task-consent-u04-existing-control-results.md)：
native20/18失败，宽CU290/3失败，ACP178/routing38；清三组旧cache，H实际观察增1.64GiB。
下文running段落是该原pipeline尚未退出时的历史记录，不代表当前状态。

本批接source52 [启动检查点](2026-10-04-computer-use-task-consent-u04-existing-bootstrap-checkpoint.md)。
source52完整native33通过/5失败、exit101，整套未绿；原owners已退出后精准清理三组旧cache，
逻辑3.54GiB、H空闲观察增2.34GiB，旧源/current/所有首败证据保留。此记录不覆盖旧首败。

当前单Codex writer，HEAD仍8d8b71c262ab222748bca32412eea884d4b892e0，分支
feat/computer-use-implementation；无Grok/子代理/提交/push/PR/发布，不碰凭据/用户
浏览器/代理或共享Grok home。产品consent、permission、lease、input和恢复规则未改。
整体仍active / partial — not releasable；本文件后续验证待原owners实际终态填写。

## 本批窄改动

- 原已验证owner/profile内追加`joint-phases.ndjson`：固定phase、replacement布尔与elapsed，
  最多128行；只本地同步写，不增加RPC、重试、sleep、生产预算或浏览器权限。
- 原失败先同步独占写`joint-primary-failure.json`，只保留固定error family、phase、
  数量/布尔与elapsed；先于worker resume和所有popup/worker辅助await。后来的旧
  enriched `joint-failure.json`不覆盖它，辅助挂起不能使主阶段记录随Node退出丢失。
- 原ExtensionPeer首败直接报告自己持有的profile路径、已消费command数量、固定类别
  和elapsed；不记录payload/binding/任意child stdout，消除靠Temp创建时间猜映射的缺口。
- browser replacement消费原native附件后立即清除全局旧worker handle；dispose错误仍
  可见，原browser/Job仍负责物理退出；后面的finally不会再次消费已释放的原附件。

这提升诊断可靠性并修复确切fixture所有权缺口，尚不能称source52五项根因已修。
不会借source51 dormant证明历史source50 startup的确切原因。

## 诊断持久化验收与首次错误

真实单独Node子进程先落主阶段记录，然后卡在模拟的辅助查询；parent明确kill这一个
child并等待原exit，文件仍有share-host-ready/3checks，phase亦保留。另验证128行上限、
任意payload不进入文件、第二次写不能覆盖首败。
第一次本地测试1通过/1失败，仅新测试的预期newline转义错误；实际child已写主记录，
原log `existing-control-phase-tests-1.txt`及根
`C:/Users/Administrator/AppData/Local/Temp/grok-cu-phase-evidence-uxgCgK`保留。
修正该测试预期后，两项实际2通过/exit0，`existing-control-phase-tests-2.txt`；没有
改诊断行为来迁就断言。不是原MV3或产品权限验证。

## source53候选与验证

source53为3276 files，manifest SHA256：
`ece2f38c4574ea9009be72392765f8fc9a6be579d1f57fad3762ecc6fa819821`。
证据目录仍为 `tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`，跨日不改旧目录身份。

| 项目 | 实际结果 / 当前状态 |
| --- | --- |
| 主诊断持久化、界限与首败保留 | 2 passed / exit0；原首次错误保留 |
| 默认no-run build / manifest | exit0，1m32s；`app-artifact-53.json`，binary SHA256 `c9e4145f9bdb6292a0cfe78ca7dec4904f1e30f52ee500bcfbbecc4f20a1d65d` |
| 原碰撞/repair各一次定位 | 各1 passed / exit0，57.48s / 26.99s；分别12/10checks；仅non-reproduction，旧根因OPEN |
| App / probe strict | 各exit0，26.01s / 22.42s |
| Node / MCP / quality / diff | 177 / 38 passed及质量/diff各exit0 |
| fmt | exit1：新peer witness的println需多行格式；原`fmt-53.txt`保留，未在owners运行时修改源码 |
| 同候选完整native / App CU/ACP/routing | not_run；先修已确定的格式失败、冻结新候选，不借source52总数 |
| 原owners退出后的本批新精准清理 | source53新Plan已回读；Execute被另一个Rust owner安全defer，未删除、未写before/report；不复用旧候选Plan，最终候选退出后新身份重列 |

原session14020/3017/29870/2272均已退出。source53一次碰撞与一次repair均通过；仅未复现，
不会称旧五项根因已修。当前只纠正println格式，冻结新source54，后续必须使用新候选
实际全矩阵和门禁，不借53的局部数字。其它Rust owner的实际out-dir为另一个session-import
任务的隔离target；未停止或清理它。AGENTS第9条保守清理门禁仍生效。

## 当前source54：仅修格式，完整验证仍运行

source54为3276 files，manifest SHA256：
`f16dcf17f1c37a230abc6d7ee063cdbe42aa9affc94cd1bafdc5e01989b28388`。
相对53仅peer witness的println格式变化；先fmt实际exit0，再freeze/build，不修改
运行中的源。默认no-run build exit0，40.17s，Common Controls manifest已嵌入。
`app-artifact-54.json` executable仍为 `src-tauri/target/debug/deps/grok_app_lib-63e3432e557be5b6.exe`，
SHA256 `090774006b8e5a8c5c23b919fae6693be19fbc349fa9ffdbd81df80d17076614`。

| 项目 | 当前实际状态 |
| --- | --- |
| 诊断持久化两项 | 2 passed / exit0，`existing-control-phase-tests-3.txt` |
| Node / MCP | 177 / 38 passed，各exit0，`node-54.txt` / `protocol-54.txt` |
| fmt / quality / diff | 各实际exit0，fmt先于freeze，其后源未再改 |
| App strict | exit0，30.43s，`clippy-app-54.txt` |
| probe strict | exit0，28.96s，`clippy-probe-54.txt`；原session73410已退出 |
| 完整native joint | running，原session90856，38 expected，`existing-control-native-matrix-1.txt`；无最终总数 |
| 宽App CU / ACP / routing | 原90856串行pipeline内待native退出后执行；即使native失败也继续宽回归，保留原native exit |
| 最终精准清理 | 待全部本批owners退出后新身份Plan→回读/hash→Execute；不执行source53旧Plan，不清正在运行的产物 |

原session86828非Rust门禁、73410两strict和build86450均已exit0。source53的两次原定位不得
移称source54 focused，更不能提前把running全矩阵称绿。下一次先续原90856
到真正终态、核直接peer/curated phase首败、清本批旧缓存并更新权威入口，
随后才写choose/discovery五项终态，不重开原session或修改冻结源码。

Core/前端未改，未全量重跑，不借source52或更早数字。source52及历史失败根全保留。
无安装版App、人类toolbar/真实Grok或macOS/Linux证据。

下一步按[原control/终态合同](2026-10-05-computer-use-task-consent-u04-existing-control-terminals-next.md)
补choose Cancel/Stop、discovery Cancel/Stop/原outer future Drop与具体registry owner
物理join，再reservation/publication终态、queued continuation撤销和恢复竞争。
完整E5、U04–U09、D00–D31、四surface、三平台/双Mac架构、输入/恢复、签名安装升级
回滚、真人Grok与最终同候选12h均保留；缺设备/人类授权not_run，descriptor关闭，raw parser仍Desktop-only。

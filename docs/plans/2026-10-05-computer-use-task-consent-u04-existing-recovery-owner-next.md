# ExistingTab：恢复中的原 App / agent / ACP owner 替换

这是[原恢复竞争合同](2026-10-05-computer-use-task-consent-u04-existing-recovery-races-next.md)
R1–R3之后的R4小批。先核对
[恢复竞争检查点](2026-10-05-computer-use-task-consent-u04-existing-recovery-races-checkpoint.md)
的实际结果；原build/test/browser owners全部终态后，才开始source74+的新候选。
单Codex writer，不启动Grok/子代理，不提交、push、PR或发布；全部dirty/untracked、
fix.patch和失败现场保留。整体active / partial — not releasable，ExistingTab descriptor仍关闭。

## 已有路径与本批边界

沿原first session MCP request、source MV3 Pair/三Share、App picker选A/B未选C、
新TaskGrant publication与原ACP stdin首次delivery。先对原run取一次语义观察、
保存old refs，然后实际原Host Pause。原设置/feature保持true。

复用`armOriginalRecoveryReturn`和原`host-resume-race-start/held`：只暂缓原
`observeDocument`对原A document的真实`chrome.scripting.executeScript`结果，
其它script/poll/action/status不hold。不换grant、document、rows或adapter；
门闩必须实证matched=1、documentMatched=true、原registry UUID和原waiter pending。

先单独实现和定点证明process replacement，再agent，再实际独立ACP Arc，最后
移除原session slot。各自新建独立owned fixture，不能在一次原fixture里串联四种替换。
不要套用批准前owner替换的false settings/无execution合同：这里已有原批准、
原run及历史ticket，保留metadata不等于可执行权限。

## 四种替换的固定原身份

- Process：只改本fixture的原synthetic App slot process id；记录替换后的固定字段见证。
- Agent：只改同slot的synthetic agent id；原reader仍来自原transport，不跟随新agent。
- ACP：创建实际独立`TaskTransportFixture`/TCP peer，替换slot的ACP Arc。
  原和新peer分别保留至各自自然终态，不能以`acp=None`冒称实际Arc替换。
- Removed：移除原synthetic slot并保留其原值作只读见证；不改日常App会话。

原reader、原request key/run、原manager endpoint/process/agent pin、原ACP Arc、
原TaskControls registry entry和原capture promise均在替换前固定。
禁止替换后调用current-slot getter重建所谓原identity；禁止把新slot的address、
grant、claim或receipt拿来完成原恢复。

## 每项必须成立的后置条件

1. 替换发生在原native返回门闩已进入、未released、原waiter仍pending时。
   原grant仍suspended、零executable/available continuation、没有fresh observation。
2. 替换方用独立只读见证保存process/agent/ACP Arc状态；新peer没有原request/grant/run，
   不因旧结果得到权限。原broker和新broker证据分开，不能加总掩盖迁移。
3. 交回同一真实原rows，自然await原waiter/worker，不abort、不reset registry或
   stopping/join_failed、不抢live waiter结果。原worker实际退休。
4. 按实际生产切点断言固定精确错误：原scope/capture重验或原owner/publication退休。
   不以任意`is_err`销项；若路径不同，先保留实际反例，再调整切点解释和候选。
5. 原零executable、零available claim、旧claim仍validate失败、旧delivery不能重放。
   历史invalid/consumed ticket数单独报告；不删除历史ticket刷零。
6. 原fresh refs/observation不发布，旧refs不能输入，独立原ACP peer零wire；
   有新peer时也检查独立零wire。只读witness证明新slot未被旧结果改写。
7. 原Pair、三Share、A/B/C document/page保留，C无读取或输入；A/B无输入。
   script/capture计数只增加该一项原正文capture，截图0、动作0。
8. 最终原worker/原nativePromise/各peer及owned browser按原ownership结束。
   所有join均成功才清成功临时fixture；失败先保留primary/phase/log/profile/hash。

不得通过旧会话重新批准或新task request来补全原late result。外部真实Grok、
安装态UI或真人手势仍`not_run`，synthetic Host/ACP联测不是这些证据。

## 门禁与清理

每个原子源码候选fmt→freeze→默认App artifact；先四个新定点，再原并发Continue、
Pause/Chat Stop和顺序恢复回归。最终同候选完整native、宽CU/ACP/routing、
App/probe strict、Node/MCP/诊断unit、syntax/fmt/quality/diff/source Verify。
Core若改变，补default/test-support全量与两Core strict；前端若改变则补全量前端门禁。
不移用旧候选数字；不增原15秒/HTTP/control/lease预算、不自动重试。

AGENTS第9条每批常规收尾：原owners退出后新cleanup身份Plan→回读→Execute；
仅删精确obsolete cache/可再生临时runtime，保留current build、seed/依赖/源码、
fix.patch与全部失败现场。清前后复验原37hash/5absences、source64–66、67/68、
source71及本批新增首败。分别报告逻辑bytes与实际空闲观察变化。
未知owner/working generations/其它worktree产物deferred，不杀无关进程。

之后R5为独立下一小批：显式screenshot与五动作在invoke前/后被退休的迟到结果。
invoke后的真实DOM效果必须保留，不能回滚伪造零效果；Unknown/去重不能重放。
无activeTab环境只能证明准确拒绝，截图成功路径记`not_run`。
完整E5、U04–U09、D00–D31、四surface、三平台/双Mac架构、原生输入、
签名安装升级回滚、真人Grok、最终同候选12h及UI/UX仍required，范围不缩减。

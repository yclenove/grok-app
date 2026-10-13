# U04 E1：ExistingTab 原分享列表的 Host picker

状态：**E1 backend picker verified / full Computer Use active / partial — not releasable**。
工作区 `H:/aicoding/grok-app-computer-use`，分支 `feat/computer-use-implementation`，
HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`。单 Codex writer，原 dirty/untracked/
`fix.patch` 保留；没有启动 Grok/子代理，没有 commit/push/PR/发布。

本批接续[原分享身份基础](2026-10-04-computer-use-task-consent-u04-existing-scope-checkpoint.md)，
只完成[ExistingTab 计划](2026-10-04-computer-use-task-consent-u04-existing-next.md)的 E1，
不是全部 ExistingTab、完整 U04 或三平台交付完成。

## 真实缺口与生产改动

原 `begin_task_scope_picker` 拒绝合法 ExistingTab request；认证 HTTP Share→Host picker
的反例 `core-picker-red-1.txt` 实际进入断言，0/1、exit101。接通入口后，原 Desktop
标题过滤又拒绝空标题或名为 Grok 的合法分享网页；`core-title-red-1.txt` 同样0/1、exit101。
这两项是缺失能力，不是已证实的越权或原用户浏览器被操作。

- `begin_task_scope_picker` 现在接受 Desktop / ExistingTab，ManagedBrowser / WebView
  仍拒绝；managed 已有独立 constructor，不制造假 picker。
- `skip_desktop_target` 仅用于 Desktop。网页标题不是窗口权限规则，空标题可保留
  browser metadata fallback；原 metadata 的512字符上限和脱敏 view 合同不变。
- 原 authenticated shared registry→bounded candidates→Host-only receipt→原 selection/
  reservation→publication→Stop 接通。discovery 不读取正文/截图、不借用、不发布 grant。
  model 不取得原 share/document/connection proof，不枚举日常未分享 tabs。
- `select` 仅复制原 opaque receipt，不在 App registry 锁内增添阻塞 native 查询。
  真正 proof 复验仍在原 owned reservation preparation/最终 publication；不能把
  `select` 返回成功当作已批准或已证明文档仍有效。

本批 production delta 只有 `broker/scope_picker.rs` 两处 gate/filter；其余三处为测试：
注册新 picker 模块、新10项回归，以及把既有 HTTP Share 回归接到真实 Host picker。
相对上一批最终源清单，三个既有文件改变、一个新增文件，无其它 source delta。
没有改 App/UI 入口、feature 默认值、模型工具、Pair/Share gate、locale 或依赖。

## 新测试及保留的失败

10项新 Core 回归使用原 Host registry/receipt/reservation；user gesture/agent 为 synthetic。
覆盖 readonly metadata/秘密不出 view/512字符、空标题和 Desktop 特殊标题、不列 legacy/
occupied 候选、subset批准/原结果幂等重试、reshare/document/unshare/expiry/connection
换代、旧 picker/drop 不干扰新 picker、非法/重复/foreign/超限 choices、Cancel/Drop/
Stop 多切点零迟到 grant、discover 后 foreign borrow 不收养或释放、full request key/
requested surface/原 transport，以及空标题页面真实 prepare/commit 零 Desktop/content IO。

既有 `existing_task_http_document_share_flows_into_original_reservation` 现在走认证 HTTP
Share→discover→select→原 reservation→commit→Stop，仍是原测试，不额外加到新增计数。
每项 cleanup 只归还本次借用；不 close/unshare 用户页面，foreign owner 不受影响。

`core-picker-green-1.txt` 实际25通过/1失败、exit101，是新测试提交后再次读取已退休
picker 的 `views()` 导致 StopRequested。修测试在批准前保存原候选 refs，再用原结果
验证幂等；没有修改生产退休规则。green2 为26/0，最后加第10项后的 green3 为
**27/0/0、exit0**（包含17项既有相关测试）。原两个 red、green1 失败均保留。

## 同最终源的验证

证据：`tools/computer-use-probe/.run/codex-u04-existing-picker-20261004/`。
所有原 Core/App/strict runners 均从原执行句柄读回终态和退出码。

- Core default **all-targets 1031 passed /0 failed /0 ignored**：unit827 + contracts204，
  `core-all-1.txt`，exit0；test-support **1047/0/0**：同集合 + driver16，
  `core-support-1.txt`，exit0；16不重复计算。Windows 上的 portable Mac/Linux contracts
  不等于这些 OS 的实机验收。
- 重建默认 features=[] App harness并按 CI 嵌 Common Controls manifest：宽 CU
  **267 passed /0 failed /1 intentional ignored**，126.34s；ACP **167/0/0**，
  routing **38/0/0**，均exit0。重叠过滤集合不相加；live_handshake未运行，原10分钟
  手动TTL项本批未另跑，不冒领历史结果。
- Core default/test-support、App default/probe 四种 **all-targets strict Clippy**通过；
  fmt、quality、diff通过。普通 test-support all-targets 编译仍有既有 native fixture
  两项 dead-code warnings，不把它的stderr宣称为零warning。
- Extension unit + bundle audit **173/0**；MCP协议/drag/golden **38/0**，均exit0。
  没有UI/locale变更，未重跑前端全量/type/lint/build，不冒领旧8031前端结果。

冻结3152源（含 extension），`source-files-1.sha256` SHA256：
`04177ac6ff46e1b4f0aed7a8147c349284722b98d353473562b12d1517fe4ff0`。
验证和清理后逐项复核相同；上一批3151/hash不是本批候选。

默认 App harness `grok_app_lib-dc611c8f8bc4f678.exe` SHA256：
`cab4318c2dc0fedc5cf60e28e5a62f0e83add8c785947e90d2d163356423e52d`。
Core default/support harness SHA256：
`07b701717c291f0f2d04466030a420cf82d042b7da485e70c1dc263af466ddca` /
`f897178945f9e84af660046019088b4803e8525d0f718f38d891c0833ca2e0c4`。
immutable seed manifest仍为
`001cf9617ce64b6cb6448c38a848533577a6116f33138bbc242e8e03e873b5ec`。
这是测试程序，不是新的开发安装包/发行包。

## 构建卫生与中断记录

原 owners 退出后审查15组 exact old/current pairs，inspection-only实际exit0。
首个 execute 原句柄36047实际exit1：删除两组后，独立C:Temp工作区新一轮 Rust
编译启动，逐目标安全检查自动停止；不杀该进程、不放宽全局保护、不覆写原 before。
`cache-cleanup-interrupted.json` 独立复核两组已删、13组未删，所有保留哈希和3152源不变。

重新只读核对剩余13组，使用**新** plan/script/before/report2，未重放首个计划。
第二次 inspection-only exit0、execute原句柄78693 exit0，删除剩余13组；首个中断
和前后记录都保留。最终逐项确认原15个旧路径缺失、最新代所有原hash entries不变。

合计删除 **6,787,421,705 logical bytes（6.32GiB，可能含 hardlink）**。
两个清理窗口的H空闲观察增量分别1,315,876,864和2,611,617,792 bytes；总和
3,927,494,656 bytes，首个before→最终after为 **3,924,799,488 bytes（约3.66GiB）**。
全程有其它工作区活动，空闲增量不等于逐文件物理占用的精确归因。

`cache-cleanup-final.json` 为两次记录的汇总，不覆盖原失败。未删 whole target/
node_modules/共享cache/其它worktree/当前程序/依赖/seed/源码或报告、日志、hash。
旧缓存不在回收站，可用 cargo 重建。每批按 AGENTS 第9条继续精准清理。

## 下一步与未验边界

**Core picker已支持 ExistingTab；App TaskScopeTarget/parser/picker descriptor仍
Desktop-only，新的 App ExistingTab UI入口尚未开放。** managed 独立流程保持原行为。
E2 inactive member switch/recovery、E3 feature-off 首次 Pair+Share 事务、E4共享UI/
15locale、E5 source MV3/App/ACP/Grok联合验收继续按原顺序，不放宽旧claim准入或
借 legacy `mcp-open` 绿色代替新 TaskGrant。

本批无实际用户浏览器、真实toolbar Share/activeTab点击、真人主窗或新MV3 TaskGrant
整链验收。Core/HTTP与JS分层绿不能拼成真人、installed、Edge或macOS/Linux证明。
原 managed 自然 cleanup_pending 长尾根因仍开放；本批原 fixture 绿不关闭它。
WebView/其它U04、完整U05/U06、U07–U09、D00–D31、三平台、真实App/Grok、签名
安装、同最终候选12h和整体UX范围全部保留。完整目标保持 active / partial — not releasable。

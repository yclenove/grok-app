# U04 E2：ExistingTab 多成员切换与原 Host 恢复

状态：**E2 backend verified / full Computer Use active / partial — not releasable**。
完整 Computer Use **active / partial — not releasable**。工作区
`H:/aicoding/grok-app-computer-use`，分支 `feat/computer-use-implementation`，
HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`。单 Codex writer，原 dirty/untracked/
`fix.patch` 保留；未启动 Grok/子代理，未 commit/push/PR/发布，未操作用户浏览器或权限框。

接续 [E1 原分享 picker](2026-10-04-computer-use-task-consent-u04-existing-picker-checkpoint.md)，
完成 [ExistingTab 顺序](2026-10-04-computer-use-task-consent-u04-existing-next.md)的 E2 后端。
不是 App ExistingTab UI、首次 feature-off Pair+Share 或真实 MV3/App/Grok 整链完成。

## 生产变化与先红后绿

原 inactive 成员因尚未借用被 inventory 隐藏，scope switch 仍拒绝 ExistingTab；
`core-group-red-1.txt` 实际1通过/2失败。接通后原 adapter.abort 归还整个 run，连刚借用
的新页面一起退休，`core-group-green-1.txt` 保留实际失败。切换中的 Stop 提前报告
Stopped、原 Host resume 拒绝 ExistingTab，`core-group-red-3.txt` 实际7通过/2失败。
修恢复后 Pause 的额外 cancel_actions 仍归还 active，green4 实际8通过/1失败。

- 同 surface ExistingTab 切换进入专属 `scope_switch/existing.rs`，跨 surface 仍拒绝。
  使用原 Bound 的 opaque share/document/connection proof 和 `acquire_scope_tab`，
  不从 raw tab/run/model JSON 创建准入，不借 legacy run 再迁移。
- 复用原 existing_authorization 串行，新借用 RAII 按捕获的 TabInfo/generation 精确
  回滚。成功原子提交新 member 后才归还旧 active；失败保留旧 active。inactive 已被
  独立借用，即便 run/session 相同也不收养、不释放；foreign successor 不被回滚误删。
- `quiesce_existing_operations` 退休观察/预览、取消排队工作，不归还已批准 borrow，
  不改 borrow generation。adapter.abort 使用它；旧公开 cancel 方法保留原撤权语义。
  terminal Stop 仍走原 cancel_run/release；Bound ExistingTab Pause 保留 active，legacy
  Pause 保持原 cancel_actions 行为。未关闭网页或扩大 Share。
- inactive 已批准且 proof 复验通过的成员可以列出，不用要求已借用的 runtime liveness
  隐藏它。切换退休旧 snapshot/ref；fresh 合法 ref 的真实 Host typed packet 正向派发
  和旧 ref 零派发一起验证，extension peer 的实际页面效果仍是 synthetic。
- Stop 回读纳入 switch/resume 原 owner，原 owner 未退出时不能冒称 Stopped；Stop
  不等待 existing_authorization。ExistingTab switch/recovery 不占用全局 Desktop lease。
- 原 Host resume 保留 active borrow，复验原 suspended group 的全部 native proofs，
  fresh capture 后沿原 authority-only publication 恢复并生成新 continuation。旧票据
  和旧观察继续无效，不复用旧 grant。

额外实测发现失败 switch 的 pending fence 使原 Host Pause 返回 TargetUnauthorized，
red4 实际12通过/1失败；分离准确 control identity 后，red5 又证明 resume_available
被残留 fence 卡住，实际12通过/1失败。只对 Bound ExistingTab 的 Host Pause 允许
精确原地址执行撤权，不放宽 preview/model admission；Desktop 旧准入合同不变。

ExistingTab 在原 switch owner 已退出后允许预留一次 Host recovery 等待；只有原
pause/model/native 工作确认 idle、取得原 recovery admission 后才能退休残留 switch
fence。physical completion 未 settle 时超时仍 paused/pending，零 capture/grant；真实
settle/join 后新一次 Continue 可以成功。不是无条件清 pending 或超时重置占用。

相对 E1 最终 manifest：**9个既有 source 改变、10个新增 source**；没有 App/UI/locale、
feature 默认值、Pair/Share gate、模型工具 schema 或依赖变化。

## 新回归与证据边界

**23项新增 Core 回归**，最终 unit850 = E1 unit827 +23。使用原 Host registry、picker
receipt、task reservation、typed transport、physical completion 和 Host recovery API；
用户选择/agent/extension outcome 为 synthetic，不是真人授权或实际浏览器完整链。

覆盖 A→B→A、未选 C、inactive readonly inventory、foreign/same-run 独立 borrow、
reshare/document/unshare/connection/expiry、取得 B 后 Stop/feature/agent/transport 换代、
foreign successor 精确回滚、Pause 与原 switch owner 交错、旧 capture 迟到、fresh
typed packet 正向及 stale refs 零派发；foreign Desktop lease 在 approve/switch/Pause/
resume/Stop 前后原 holder 不变。原 public executor registration 拒绝替换，而不是
伪造一个不存在的生产 replacement API。

恢复负向覆盖 inactive proof 变更/foreign active 零 capture，prepare 前后 Cancel/Drop/
Stop/agent/transport/task/feature 变化零迟到 grant/continuation，旧 generation/request/
foreign owner 的 Host Pause/preview 拒绝。全程独立核对原 borrow、grant、有效票据和
排队请求，不仅检查 error 文案。

所有首败保留，包括测试编译错误（ambiguous ActionOutcome、match arm 返回类型、
private fixture helper）、invalid outcome detail；测试一度把 retained invalid ticket 的
ledger count 误当有效 continuation、把显式 feature-off 误当不取消 Share、以及错误
假定可 public 替换 executor。按真实合同改为有效 claim/旧票据拒绝/零新 publication
和 feature-off 原配对退休断言，没有为这些测试放宽生产权限。green14 为22/0/0，
green15 为23/0/0；最后格式整理后的同 frozen 源已在 core-all 重新执行全部23项。

## 同最终源的验证

证据：`tools/computer-use-probe/.run/codex-u04-existing-switch-20261004/`。
每个 runner 唯一日志、原执行句柄读回实际退出码；不覆盖任何旧红/绿日志。

- Core default all-targets **1054 passed /0 failed /0 ignored**，unit850 + contracts204，
  `core-all-1.txt`，exit0；portable Mac/Linux contracts 不等于这些 OS 实机。
- Core test-support all-targets **1070/0/0**，同集合+driver16，`core-support-1.txt`，
  exit0；16不重复计算。普通构建保留既有 native fixture 两项 dead-code warnings。
- 默认 features=[] App重新构建并按 CI 嵌 Common Controls manifest：宽 CU **267 passed /
  0 failed /1 intentional ignored**，120.41s；ACP **167/0/0**、routing **38/0/0**，
  均exit0。过滤集合有重叠不相加，live_handshake未运行。
- Core default/test-support、App default/probe 四种 **all-targets strict Clippy**通过。
- Extension unit/bundle audit **173/0**，MCP协议/golden/drag **38/0**，exit0。
- final fmt、quality、diff通过。无前端 source/locale 变化，未重跑前端全量/type/lint/
  build，不冒领既有前端或10分钟手动TTL证据。

冻结 **3162源（含extension）**，`source-files-1.sha256` SHA256：
`ebcb6a1c87a90d1b1799de0986b2df8d595deab6ee64321feb3ccf302710810a`。
原 runners 全部实际终态/exit0；最后再次逐项复验3162源相同。默认 App harness
`grok_app_lib-dc611c8f8bc4f678.exe` SHA256：
`35e0779de140c1dde0852e8254583a34c8f1a75785a07fec34ec467aa1a1c5a5`。
Core default/support harness SHA256：
`4275de4f679085c0055e3c7c2962ba36d15ba76a0441d4d7af1285a476361f6b` /
`a8e542d31e606ab8e12be47f9c235e0ee492a857b4f0ead8f4f664f7069fbcdc`。
immutable seed manifest仍为
`001cf9617ce64b6cb6448c38a848533577a6116f33138bbc242e8e03e873b5ec`。
`fix.patch`仍为
`441b6807747a7102860394a17f908b99e9b2fe1e5754167f5fb8e730743830ab`。
E1旧manifest不是本批候选；本批未产生新的开发安装包/发行包。

## 构建卫生与终态记录

原build/test runners全部实际退出后，按AGENTS第9条审查15组exact obsolete/current
incremental pairs，新清单只读检查原句柄19816 exit0；execute原句柄31834 exit0。
不复用E1两份旧计划。删除 **6,743,993,121 logical bytes（约6.28GiB，可能含hardlink）**；
H空闲窗口观察增加 **4,846,305,280 bytes（约4.51GiB）**，不冒称逐文件物理占用的精确归因。

`cache-cleanup-before.json`保留各目标、current文件和保留产物原哈希；`cache-cleanup.json`
确认15个旧路径缺失、最新代原hash entries与当前App/Core/immutable seed/`fix.patch`不变。
执行前和最终再验证3162源，manifest不变。保留下批依赖、源码/uncommitted、日志/失败
证据/hash；未删whole target/node_modules/共享缓存/其它工作区，未杀任何无关进程。
旧缓存未进回收站，可用cargo重建。**本批清理计划已执行，下批必须新建实际检查过的计划。**

## 下一步与未验边界

E2后端验证/清理收尾后进入 **E3小ADR + 正式 Broker/HTTP feature-off 首次 Pair+Share
事务**，之后 E4 原App命令/shared controller/card/panel/15locale，最后 E5 同最终
候选的 source MV3+authenticated HTTP+原 task reservation+App/ACP 联合链。
App TaskScopeTarget/parser/picker descriptor 当前仍 Desktop-only，不能放开 UI 多选
后用 legacy `mcp-open` 代替新 TaskGrant。

没有真人 toolbar activeTab Share、用户主窗/Grok、installed、Edge 或 macOS/Linux
整链验收；分层 Core/Node 绿不能拼成这些结论。原 managed 自然 cleanup_pending
长尾根因仍开放，本批 fixture 通过不关闭它。其它 U04 surfaces、完整U00–U09/
D00–D31、Windows/macOS/Linux、完整输入/恢复/安装修复回滚、签名包、同一最终候选
12h与整体 UI/UX 范围全部保留。单writer，未提交发布；目标保持active/partial。

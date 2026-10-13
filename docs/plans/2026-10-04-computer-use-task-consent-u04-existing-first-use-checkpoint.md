# U04 E3：首次 feature-off Pair / Share 的原任务事务

状态：**E3 backend verified / full CU active / partial — not releasable**。
工作区 `H:/aicoding/grok-app-computer-use`，分支 `feat/computer-use-implementation`，
HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`。单 Codex writer，保留原 dirty/
untracked/`fix.patch`；未启动 Grok/子代理，未 commit/push/PR/发布，未操作日常
浏览器、凭据、代理、共享agent home或真人权限。

接续 [E2](2026-10-04-computer-use-task-consent-u04-existing-switch-checkpoint.md)，
按 [E3 ADR](2026-10-04-computer-use-task-consent-u04-existing-first-use-adr.md) 实现。
这不是 App ExistingTab 入口/真人MV3/Grok/安装态/三平台整链交付。

## 生产变化

- 原 Host `begin_task_scope_picker` 的 ExistingTab 手势在同 authority lock 建立
  唯一有界 metadata preparation。绑定原 full key、owner/agent/transport、selection
  revision、feature epoch与五分钟硬期限；原request期限独立生效。挂载/status/模型
  申请/错surface/错key不配对、不启动浏览器、读内容或建立执行run。
- 复用仍活的原连接，不覆盖 foreign pending challenge。原picker guard提供只在
  Host保留的code与显式确认，HTTP只给public challenge，complete仍核全身份、MAC、
  协议和一次性消费。旧nonce、旧receipt和旧cancel不能批准或清理新owner。
- off下只有原准备绑定的连接可以保活、登记browser metadata、显式Share/Unshare；
  token/Origin/connection/sequence/document校验不变。poll、observation、action、
  completion claim/result、v2输入negotiation及模型执行gate仍关闭。
- Pair是用户确认的设备连接，Share是用户activeTab手势产生的浏览器状态，二者不是
  TaskGrant。普通任务取消只关metadata admission、退休确切未完成challenge，不
  反向取消已完成Pair/Share；无准备者且feature仍off时不能保活，原30秒lease有界。
  显式feature-off全局revoke合同不变，既有foreign borrow/Share不被普通Drop清理。
- discover移交原guard给picker。进入原reservation后禁止off Share更新，仅有效
  原preparation绑定的status/heartbeat桥接reservation→enable短空隙。fence/abort/
  Stop/replacement/过期立即失去桥接准入，不借此建立执行grant。
- 原扩展先读取认证status的`executionReady`再启动两个执行loop。false只等待，
  不误判断线，也不发观察/动作poll；原promise保留至真实退出。旧epoch迟到响应和
  reset不能启动新loop。旧enabled Host不带该字段仍兼容；字段类型不符则拒绝。
- 最终feature + TaskGrant + continuation仍由原reservation/Host publication提交，
  未另造批准系统、未使用legacy grant冒充新TaskGrant。默认CU仍关闭。

相对E2最终manifest：**16个既有source改变、4个source新增**；无App/UI/locale、
模型工具schema、依赖或默认值变化。新的App功能入口尚未放开。

## 先红后绿和测试边界

证据目录：`tools/computer-use-probe/.run/codex-u04-existing-first-use-20261004/`。
日志不覆盖；所有原执行句柄实际读回终态，不能用文件mtime代替owner退出。

- `core-red-1`：原首次off picker没有challenge，实际0通过/1失败。实现后green2通过。
  green1保存误将guard初始化放进candidate解构的真实编译错误，未称为断言回归。
- `node-red-1`保存fixture遗漏action journal的失败；修正fixture后的`node-red-2`
  实际4通过/1失败，证明准备期提前poll。readiness接线后green1实际40通过。
- `core-green-4`实际8通过/1失败：fixture错误尝试用只允许原agent的bind方法替换
  agent。改用正式`bind_agent_session`，不放宽生产bind合同；green5实际9通过。
- 审查另发现reservation前后关闭保活的空隙。`core-red-2`实际0通过/1失败，正式
  HTTP status返回403而不是有效桥接200。修原preparation复验后green6实际9通过；
  新增approval fence桥接回归后最终all-targets重新覆盖全部10项新Core测试。
- ACP首filter错用`acp_client::tests`，`app-acp-1`实际0项，**不计作验证**。
  保留旧日志，改用真实`acp_client`后`app-acp-2`实际167项；runner拒绝空filter结果。

新增 **10项Core回归**，含24种before/after Pair原fence组合和16轮原lock竞争；它们
是测试内部case，不重复加成测试数量。覆盖full key/surface/model状态零准备、原
challenge/confirm、24种Cancel/model-cancel/Deny/Stop/request/revision/agent/
transport/epoch/feature-off/硬期限/request TTL、foreign pending/borrow/Share、
旧Drop与新nonce、原HTTP Pair/保活/Share→picker→reservation→commit→Stop、
旧token/connection/document、off执行拒绝、reservation保活桥接及原approval fence。
独立核对run/grant/有效continuation/原registry/零内容与动作，不只检查错误文案。

用户gesture、extension proof和页面为synthetic；五分钟硬期限及request期限由明确
test clock/helper验证，不冒称本批另跑了真人TTL。未操作用户toolbar或权限弹窗。

## 同最终源的验证

冻结 **3166源（含extension）**，manifest `source-files-1.sha256` SHA256：
`527833dd32034d512a9a18f62cb6d1f4be2a497f8121c9f802f66ee6abd9b8d1`。

- Core default all-targets **1064 passed /0 failed /0 ignored**：unit860 + contracts204。
- Core test-support all-targets **1080/0/0**：上述集合+driver16，不重复计数；Mac/Linux
  portable contracts不是这些OS的实机验收。
- 默认features=[] App重新构建并按CI嵌Common Controls manifest。CU **267 passed /
  0 failed /1 intentional ignored**，118.87秒；ACP **167/0/0**、routing **38/0/0**。
  过滤集合有重叠不相加，真实Grok live_handshake未运行。
- Core default/test-support、App default/probe四种all-targets strict Clippy全部通过。
- Extension/bundle **177/0**（4项新增），MCP协议/golden/drag **38/0**。
- fmt、quality、diff通过。没有前端UI/locale source变化，未重跑前端全量/type/lint/
  build；不冒领历史真人App、installed或全平台结果。

默认App harness SHA256：
`359d9beac00cd9e455999f2315af712cb506b813932147c90e37058abb5ac969`。
Core default/support harness：
`550fbf3dfe73c36b0f3a60d8faf8b83fc1e9d32f83020e744b6da32567a38e09` /
`4c631fe94cab1839ee1f9de3cbd78699d74cf12f406af3d980d74ad390717f04`。
immutable seed manifest仍为
`001cf9617ce64b6cb6448c38a848533577a6116f33138bbc242e8e03e873b5ec`；
`fix.patch`仍为`441b6807747a7102860394a17f908b99e9b2fe1e5754167f5fb8e730743830ab`。

## 清理与下一步

本批全部build/test原owner已读回终态。新清理清单只读检查15组exact obsolete/current
pairs，原inspection句柄49353 exit0；执行原句柄86340实际exit0。
删除 **6,785,101,630 logical bytes（约6.32GiB，可能含hardlink）**，H空闲窗口观察
增加 **6,396,743,680 bytes（约5.96GiB）**，不冒称逐文件物理占用的精确归因。
before/report保留原目标及current hashes；当前cache、App/Core、seed、fix.patch哈希
全部不变。旧缓存未进回收站，可用cargo重建。**该计划已执行，下批不得复用。**
不复用E2已执行清单，不删whole target/node_modules、seed、依赖、源码或失败证据。

下一批E4仍需原App命令/ACP pin/shared controller/card/panel/15locale接线，再E5同
最终候选source MV3+authenticated HTTP+原reservation+App/ACP联合验收。特别是：

1. App原`TaskScopeTarget::parse`及picker descriptor仍Desktop-only。不要仅放宽raw
   `tab:`入口；ExistingTab选择必须来自原picker-local UUID和Host-retained receipt。
2. App `task_picker::discover`目前调用Desktop-only `prepare_scope_selection`。先增加
   typed selected-surface metadata preparation，在begin前注册原ExistingTab executor，
   不启动Desktop worker、用户browser或系统权限，不让缺backend fallback到Desktop。
3. 原Host保留尚未discover的call，分步显示必要Pair/Share准备，显式confirm绑定原
   full key及guard；不能先把空候选discover冻结后用新call覆盖仍待确认的challenge。
4. 卡片/panel共用一个controller和一次批准。短空态/版本不符/忙/error/retry、Cancel/
   高优先Stop、聚焦/键盘/缩放、切卡片迟到结果防闪回及15locale按en authority同步。

其它U04 surfaces、全U00–U09/D00–D31、三平台实机/全输入/恢复、安装修复回滚、签名、
真人主窗/Grok、同最终候选12h与整体UX范围保留。原managed自然cleanup_pending长尾
根因仍开放；本批fixture绿不关闭它。目标保持active，不标全功能完成或交付。

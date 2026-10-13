# U03 下一批：Host 状态订阅合同与卡片接线

状态：**Core/Host Channel/共享领域源及卡片/面板已接线；完整native整批尚未交付**。
最新[UI checkpoint](2026-10-03-computer-use-task-consent-u03-ui-checkpoint.md)记录pending转绿、
前端298/Core982/隔离UI37及最终默认App证据。以下为原接线合同，不是当前待实现状态。
接续 [显式 Host picker](2026-10-03-computer-use-task-consent-u03-scope-picker-checkpoint.md)。
唯一 writer 是 Codex；本稿不派发 Grok/子代理、不授权提交或发布。完整 U00–U09 / D00–D31 不变。

本批新增 `computer_use_task_subscribe/unsubscribe/acknowledge`；最后一个是传输接收回执，
不是用户许可或 task 决定。每条 Channel 只保留一个未确认状态，4秒收不到回执则退出，
最多补一个 retired；worker由有界registry保留，统一reaper自动join，App shutdown也join。
每秒纯metadata heartbeat与100ms变更合并分开；不在Broker变更线程等待renderer。
前端先挂载后Host才连接的情况由既有 `session://state/runtime` 可用事件触发一次新的
metadata订阅。事件只提示重新核原endpoint，不授予权限，不启动runtime，不重放request；
没有新的Host可用事件就不循环重试。旧subscription/generation晚回包仍只取消旧观察者。
实际旧卡 pending 无run测试先红，`ui-pending-red-1.txt`保留；同一断言已转绿，
没有删除/skip。最新证据与剩余项见UI checkpoint，events checkpoint保留历史阶段结论。

## 状态源合同

以真实 Core authority 的变更通知唤醒 Host 的 bounded metadata readback；不以 legacy
computer_use_status、800ms双轮询或 adapter liveness 猜 task。事件只说明需要回读，不能作为
TaskGrant、批准回执、执行权限或物理清理完成的证据。最终事实仍来自 HostTaskView。

建议 Core 提供仅 Host 可达的 change handle：有界原子序号 + Notify，订阅不会启动 runtime
或读取桌面；无 bearer/candidate/native proof。先注册 notified/enable，再检查 sequence，
避免 subscribe/read 之间丢更新。序号仅控制观察顺序，不参与 authority admission。
publish 必须不等待 UI，不能在 Broker 锁内回调前端或读取原生。

publisher 审计至少覆盖：

- connection reserve/bind/retire、request submit/deny/cancel/complete、任务 fence 和到期。
- selection begin/fence、approval reserve/publish/abort/final commit；准备 run 出现及取消。
- feature epoch、open/target binding/switch、pause/resume、观察完成、busy/in-flight 结束、
  unknown、Stop fence 和原 generation cleanup 终态。

不能只对 trace 发布：没有 run 的 request 和部分 preparation 没有 trace。
不能每次 authority read 都发更新：HostTaskView 的期限 sweep 会导致自触发无限回读。
不把修改 `Mutex` 的所有借用自动标脏，避免只读 sweep/分类调用引入循环或锁序问题。
若选择 RAII notice，应显式放在真正 mutator，并证明 unlock 和通知的顺序；忙闲的 atomic
变化及原工作 Drop 也要有通知。遗漏必须由测试暴露，不能靠更短轮询隐藏。

## Host 订阅生命周期

subscribe/unsubscribe/acknowledge Tauri commands 已实现；最后一个只是传输回执。
控制基础见 [controls checkpoint](2026-10-03-computer-use-task-consent-u03-controls-checkpoint.md)。
优先使用主 WebView 的 Tauri Channel，避免全局 broadcast 把其它 session metadata 散播给
无关观察者。订阅绑定原本地 endpoint、原 ACP transport 的 connectionId 和 Host instance；
原 agent/process 或连接变化时只退休原流，不跟随同字符串 replacement。

每条消息带 Host subscriptionId 和十进制字符串序号；typed view/heartbeat/retired 分开。
所有 view 都走已实现的 metadata-only readback；不读取 target liveness/正文/截图/AX/UIA。
初始回读与订阅注册必须消除 race，临时晚回包核原订阅身份再丢弃。必要的一秒 metadata
health tick 可以确认 Host 可达及期限，准确标为 heartbeat，不冒称每次都是业务状态变更。

registry 有界、原 worker handle retained；unsubscribe 只关闭自己的观察者并真正 join，不
fence task、不消耗决定、不停止执行。最后一个前端观察者离开也不改变 TaskGrant。
channel 断开/命令 waiter Drop/Shutdown 都必须退场，不能只删 map 后遗留匿名后台任务。
Host shutdown 先阻止新订阅，再取消并 join 原异步 workers；不在回读线程执行 native cleanup。
没有本地 endpoint 时如实 unavailable，不调用 ensure_request_endpoint/runtime 来造假流。

## 前端每 session 共享领域源

领域模块持有单一 session 订阅和 immutable snapshot；卡片/面板用 useSyncExternalStore 或
等效 subscription 共享，而不是各自拉取。首次消费者创建流，最后消费者取消原观察者。
StrictMode remount、快速切聊天、Channel 消息先于 subscribe ack、卸载后晚 ack 都须处理。
一次 start/stop 使用独立前端 generation；旧 cancel/错误不能影响新 source。

验证 subscriptionId、sessionId、connectionId 和单调序号后接受消息。Host reset/retired
立即失效缓存权限视图；不得从旧前端缓存重建 grant。保留最后确切 key 供原 task Stop，
unknown/stale 不保持绿色，不从当前 legacy run 补猜新 task。事件健康超时采用有界 watchdog
和明确 metadata recovery，不新增后台截图。mirror/browser 不能创建本地订阅或批准。

批准/deny/cancel/Stop 与 picker 都调用同一领域 controller；Stop 不受 approval busy 阻断，
收到 acknowledgement 后继续等真实 Host cleanup 状态。批准卡不显示运行直到执行事实成立。

## 先红测试与验收顺序

1. Core 真 request-only broker：订阅前后 pending 无 run 可见，零 adapter/credential/continuation
   副作用；submit/prepare/commit/deny/Stop/expiry/连接退休各自唤醒；无通知失联/无限回读。
2. Core 原 busy handle 和 cleanup：原 worker 完成改变观察状态；通知前后 Stop 精确、没有
   action replay，原 native handle 尚未 join 不显示 stopped。
3. Host 原 endpoint/ACP：同 agent 的重连、并行 session、取消 observer、channel failure、
   subscription overflow 和 App shutdown 原 handle 退场，且 observer 退出不撤 TaskGrant。
4. 前端 source：两个消费者仅一条流；完整序号/晚 ack/切聊天/remount/stale，原 key Stop
   和 terminal state 不被旧订阅覆盖；没有新的 request/retry/enable 副作用。
5. 先运行并保留旧 ComputerTaskCardLive 在 Host pending/executions=[] 下不出卡的真实 red；
   再接新 card/picker/同首用事务。保留现有 legacy controls/Stop 回归的原安全合同；若改变
   展示合同明确说明，不删除保护断言取绿。
6. 卡片和面板按原 U03 draft 完成所有状态、15 locales、键盘/返回焦点、300/320px侧栏、
   900px窗口、深浅/长语言和 100/125/150/200%DPI。隔离实际组件截图逐张检视，不能把
   mock 成功算真人 App/OS 批准；未知能力不挂无效按钮。

建立下一批独立 .run 证据目录，不覆盖 scope-picker 日志。当前源码/最终 invocation/artifact
绑定后执行必要 Core/Host/frontend 和 strict 检查，记录 actual exit/断言/源及 binary hash。
本批仍不关闭 U04 其它平台/surface、U05恢复、U06清理、signed installed/Grok E4/12h等缺口。

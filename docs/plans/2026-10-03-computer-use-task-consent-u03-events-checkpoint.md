# U03：Host 状态订阅与前端共享源检查点

状态：**已实现订阅基础链；卡片/面板接线未完成，整体 active / partial — not releasable**。
2026-10-03，唯一 writer：Codex。继续完整 U00–U09 / D00–D31，没有启动 Grok、子代理、
新线程或自动化；未 commit/push/PR/release，既有 dirty/untracked 和 fix.patch 保留。

## 本批实现

Core `broker/host_changes.rs` 提供原子版本 + Notify 的合并 invalidation。
订阅先注册 notified 再比较版本，避免丢唤醒；没有逐事件队列，也不在 Broker 锁内
回调前端。它不是 TaskGrant、决定或物理完成证明，实际状态仍来自 HostTaskView。
publisher 接在 request/connection/selection/preparation/commit/feature/Stop/pause/switch
及原 admission guard 的实际变更处。直接 commit 的 preparation 在阻塞 enable hook 前
也会通知。metadata read 本身不发更新，避免回读触发自己。

ACP `TaskTransportWatch` 保留原 transport，SessionManager 同时核原 endpoint 的
agent/process/Arc；read 再核原 ACP transport 指针，不能跟随同 agent ID replacement。
没有原本地 endpoint 就返回 unavailable，不调用 ensure/bootstrap 或执行准备。
token/native identity 都不进入 observer DTO、Channel 或 renderer。

新增主 WebView 命令：

- `computer_use_task_subscribe`：Channel 接收 typed view/heartbeat/retired。
- `computer_use_task_unsubscribe`：只取消原 subscription，保留 task/grant/continuation。
- `computer_use_task_acknowledge`：接收流控回执，**不是用户许可**。

每个流固定 subscriptionId/sessionId/connectionId/hostInstanceId；sequence 为十进制 u64
字符串。变更最多每100ms合并回读，health每秒只读 metadata并明确标记heartbeat。
每流最多一个未确认view，4秒不确认则退出、最多补一个retired；Broker不等待UI。
registry最多32流，原异步handle retained，取消unsubscribe waiter也不丢原handle。
统一的owned reaper自动join已退出observer，包括Channel失败/超时；shutdown也取消并
join原observer及reaper。probe join不再因前一个registry失败跳过其余owners。
这没有完成其它registry的全部U06清理缺口。

`src/lib/computer-use/taskSource.ts` + `useComputerTask.ts` 实现每session单一共享源：
首次消费者启动、最后消费者释放；StrictMode/切聊天/早消息/晚ack分别核原generation。
pre-ack buffer只保留最后一条完整状态。接受消息前核全部owner、严格单调u64顺序及
view内connection；4秒失联变stale，保留最后确切key供Stop，不保留绿色。
retired/failed不会自行申请权限或重放动作。既有Host session://state/runtime可用事件
可以触发新的metadata订阅，解决先挂载后连接；事件仅提示重核endpoint，不是权限事实。
失败subscribe期间到来的可用事件保留一次，避免丢恢复，也不循环重试。
mirror/browser由typed API门禁拒绝本地订阅。

## 实际验证与失败保留

目录：`tools/computer-use-probe/.run/codex-u03-task-events-20261003/`。

| 检查 | 实际结果与证据 |
| --- | --- |
| Core 原red→green | `core-red-1.txt`真实pending无run通知断言失败；`core-green-1.txt`同断言转绿；早期输出不改写 |
| Core事件专项 | `core-events-2.txt`，8 pass/0 fail，exit0；覆盖无run、无丢唤醒、回读无循环、prepare/abort/Stop/expiry、原busy handle、直接commit准备发布 |
| Core全量 | `core-full-1.txt`，734 unit +220 integration =954 pass，0 fail/ignored，exit0 |
| 默认App最终构建 | `app-build-6.jsonl/.stderr.txt`，唯一features=[]测试artifact，exit0；从此次JSON取产物，再嵌现有windows-test-manifest.xml，没有改产品linker |
| App observer专项 | `app-events-2.txt`，10 pass，exit0；含真实流控、初始read/start竞争、同agent退休、无run、授权保留、overflow、取消waiter原handle、Channel自动join |
| App CU过滤 | `app-computer-use-2.txt`，193 pass，exit0 |
| App ACP过滤 | `app-acp-2.txt`，149 pass，exit0；明确skip live_handshake，不是实机CLI认证验收 |
| App routing过滤 | `app-routing-1.txt`，38 pass，exit0 |
| 前端最终领域/API | `frontend-events-3.txt`，4文件31 pass，exit0：source16/events3/task6/picker6 |
| 前端旧CU兼容回归 | `frontend-cu-1.txt`，18文件214 pass，exit0；这是新增endpoint-ready两项之前的运行，不冒称最终全前端套件通过 |
| 前端新UI缺口 | `ui-pending-red-1.txt`，**0 pass/1 fail，exit1**：实际ComputerTaskCardLive收到Host pending、executions=[]仍无卡；源码断言保留、尚未转绿 |
| TS/lint/UI构建 | `typecheck-4.txt`、`lint-3.txt`、`build-ui-2.txt` exit0；Vite原有大chunk提示保留 |
| strict Clippy | `core-clippy-1.txt`、`app-clippy-3.txt`、`probe-clippy-2.txt` exit0 |
| 格式/质量 | `fmt-1.txt`、单独include!命令`fmt-commands-1.txt`、`quality-2.txt`、`diff-1.txt` exit0 |

App过滤集合重叠，不能相加。所有测试决定、picker/adapter均synthetic，不是用户App/OS
批准、主WebView手势、真实Grok、其它平台或已安装发行包证据。

中间失败同样保留：app-build-1语法缺括号；app-build-2两个测试pattern guard移动Option
和unused import；typecheck-2的测试helper union类型；app-clippy-1的question_mark。
分别修复后再检查，不把构建失败写成断言通过。最终候选只用build-6，早期artifact不同。

## 源与产物绑定

`app-artifact-2.json`记录默认测试产物及最终invocation。嵌manifest后harness SHA256：
`ee3536d2f07d38e8ba226cc2a8f572a88a05bcb0b9d62616e7cc86ce1f49c38f`。

`source-files-1.sha256`为823文件：git cached +非ignored untracked，Core/App/probe/MCP/
protocol，以及task API、CU组件/领域和useSessionHostEvents；Sort-Object -Unique，
lowercase SHA256 +两个空格 +repo path，UTF-8无BOM、LF终行。manifest SHA256：
`138c651ccca4baaeba34a913d4f946935a42b96e3899edf970e0de95b13b9c71`。
这是受检子集、包含尚红的UI断言，不是全产品发布清单。后续源码变化需要新证据，
不能冒用本批结果。未使用先前scope-picker的binary/hash冒充本批。

## 下一步，继续完整目标

1. 让保留的pending无run UI断言转绿；ComputerTaskCardLive和面板接同一source/controller，
   退掉旧双800ms轮询。先核原legacy Stop/lifecycle回归：晚reply、精确key、Stop优先、
   ack不等于物理stopped、catalog清理和native清理分开，不能删保护断言取绿。
2. 接显式picker begin→load→原成员approve；首用沿Host preference/prepare/commit事务，
   准备可取消，迟到begin/load/approve不能影响新请求。打开卡/范围说明不枚举或截图。
3. Host只给metadata；现有legacy卡的liveness/trace/catalog展示须逐项迁移，缺字段先补
   metadata合同和回归，不从legacy当前run猜新task，不用前端字符串把unknown画成running。
4. 卡片/面板完成各状态、15locale、keyboard/focus和隔离真实组件视觉验收，包含窄侧栏、
   深浅、长语言、DPI。当前只写了hook，产品组件**还没有消费它**。
5. 继续U04其它平台/four surfaces/真实native组/增量缩小；U05恢复；U06其它owner收尾；
   U07–U09和D00–D31、signed installed、真实Grok E4及同一最终候选12h长稳全部保留。

本批只完成基础链，整体不能称可交付，更不能据这份checkpoint创建PR或发行包。

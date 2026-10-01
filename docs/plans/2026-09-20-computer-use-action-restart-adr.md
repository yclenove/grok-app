# ADR：ExistingTab 动作的 SW 重启恢复

状态：分阶段实施，完整 App/SW 重启隔离仍未验收；动作 capability 保持 NONE。

最新 R3 决策见
[完成墓碑淘汰后的只读退休证明](2026-09-21-computer-use-completion-retirement-adr.md)：
同一 Host 进程用 authority 派生 completion key，tombstone 淘汰后可认证查询
`absent`；查询只读且不能释放 pending。新 App authority 拒绝旧 proof，实际
App/浏览器重启仍属于 R4，本 ADR 其余保守边界继续有效。

## 首个切片：同一原文档的完成回执恢复

当前 SW 丢失内存后，Host 的 claimed 记录仍正确保持占用，但新 SW 没有旧
completion proof 和原 Promise，无法归还它。原 key 失效不是物理完成证明。

在 claim 前，将有界 cleanup-only 记录写入 chrome.storage.session：精确
loopback endpoint、完整 completion proof、prepared/physicallySettled 阶段。
最多 8 条，严格版本/结构/长度；先收到 storage 写成功再领取。存储写失败或
格式损坏时不领取、不动作。该存储是浏览器会话内存，TRUSTED_CONTEXTS，
不是磁盘恢复，也不包含 pairing Bearer、命令参数、输入文本、页面内容。

正常执行仍由原 ActionCompletions 拥有原始脚本、取消调用与 watcher；全部
收束之后才能把记录标记 physicallySettled。完成记录持久到 session storage
确认，再发送原 settle，最后删除记录。删除失败只重试清理，不重发业务动作。

新 SW 先退休旧配对，再恢复初始化时已有的记录；不得把本 SW 后来产生的
记录混入重启清理者，避免两个 owner 同时取消/归还正在执行的动作。恢复者
没有 claim/业务结果权限，不恢复授权。

prepared 记录只向原 tabId + Chrome documentId 注入固定取消函数，必须匹配
原 snapshotId 和 operationId。函数退休原 snapshot，等待原 operation.finished。
延迟尚未进入的旧 actDocument 会在入口拒绝该已退休 snapshot；正在 Wait 的
旧操作由原 finally 完成。恢复者必须等待自己实际 executeScript 原 Promise。
结果需要精确 main-frame/documentId/settled 回执，才可标记物理结束和发送 settle。

同一文档下，成功 action 的原 observation 必须已返回，Host 才可能入队/领取；
不能重放已发布 observation，且 action 占用禁止新的 model observation。
恢复记录阻止新 SW 对同 tab Share，直到上述清理确认。此切片不把 missing
snapshot、未知脚本错误、页面导航、读不到文档或超过期限解释为已完成。

## 不属于本切片的结论

- 不宣称新 App 空 registry 能恢复旧占用。
- 不把恢复记录缺失当作原文档已销毁。
- 完整浏览器退出会清 session storage；旧 Host claimed 占用仍须额外证明。
- 文档已导航/被替换、App endpoint 失效、完成 tombstone 淘汰时可保持 unavailable，
  但这只是诚实保留缺口，必须在后续 R2–R4 补齐，不能当成最终可用状态。
- 在正式开放 adapter actions 前，仍需证明旧 observe 的迟到注入、新旧 App/SW
  组合、文档退休身份不可倒退、安装/升级世界切换。此处没有用随机 nonce
  冒充单调身份，也没有放宽原 snapshot/grant 检查。

计划及完整门禁见 [重启与 MCP 执行计划](2026-09-20-computer-use-restart-and-mcp-execution.md)。

## 已有证据与下一条合同

同原文档五个 SW 重启切点的实际证据见 [检查点](2026-09-20-computer-use-worker-recovery-checkpoint.md)
末节，当前动作探针 17 项；没有改变上面的限制。下一项先建立“恢复已退休
snapshot 后，迟到的旧 observe 又试图创建相同 snapshot”的失败测试，再决定
不可倒退的执行身份设计。仅校验随机 snapshotId 或当前 retired 字段不能解决
这个问题；不得通过重复观察重建旧身份来使重启门禁通过。

## 文档执行身份：本轮实施决策

首败 `existing-document-fence-red.log` 已确认：退休 snapshot 后，同一观察函数
可再次创建同名 snapshot。选用扩展私有 IndexedDB 的单条非敏感 epoch 计数，
每个 SW 启动通过 readwrite/strict transaction 领取一个递增 epoch，只有事务
complete 才可用于 Share；并发领取依靠数据库事务串行，不用随机数猜先后。
记录缺失/损坏、数据库错误、超时、溢出均拒绝新 Share，不静默重建旧计数。
新库在升级事务中初始化；已有库缺记录不是“第一次运行”。不存 Bearer、页面
内容、动作输入或 cleanup proof。计数允许跳号，不允许回退。

每个 worker 的本地 sequence 递增；Share 和 observe 在异步注入前领取自己的
`epoch/sequence`。显式 Share 向精确 Chrome documentId 安装文档执行身份，
只能前进，不能越过活动 operation。observe 必须属于该身份的 epoch，且
sequence 严格前进；没有 Share 初始化的文档或没有身份的旧调用直接拒绝。
preview 推进调用顺序但不替换模型 snapshot，也不使仍有效的模型 ref 失效。
新 Share 退休旧 snapshot，旧 Share/observe 迟到不能覆盖更新的身份。

SW 恢复只在已核对的原 snapshot 上关闭其所属文档执行身份，再等待原 operation；
因此旧 worker 尚未进入文档的 observe 也不能复活它。新的 worker epoch 更大，
用户重新配对/Share 后可继续。正常动作的 snapshot 退休仍允许同 worker 的
后续新观察；恢复关闭与普通 snapshot 退休是两种不同操作。

本决策不扩展“原文档不存在”的完成证据，也不宣称扩展升级前旧版本序列化
函数会自动服从新检查。不同 isolated world、数据库人工清理/扩展重装、App
重启与整个浏览器退出仍须各自验证；动作能力继续 NONE。

## 原文档销毁的证据

**2026-09-21 实测更正：本节最初的 null 推论已被推翻，禁止据此释放。**
Chrome 148.0.7778.96 中，原 documentId 查询为 null，后退却恢复相同 documentId
及点击计数。`WebNavigationTabObserver::RenderFrameHostChanged` 无条件调用
`RenderFrameHostPendingDeletion(old_host)`，删除 FrameNavigationState；DocumentId
本身仍可存活于 BFCache。下文关于显式 null 的推论仅保留为失败方案记录。

修订采用两种明确证据：原文档固定取消函数的匹配完成回执；claim 前安装的
原文档守卫在可信、非 BFCache pagehide 时退休 snapshot/执行身份并等待原
operation.finished 后，由 Chrome runtime 的原生 sender(documentId/tab/frame)
确认的终态回执。回执不携带 cleanup key、
页面内容或动作文本。null、API 异常、权限丢失与超时均不释放。BFCache pagehide
不发送销毁回执；恢复同一原文档后仍用固定取消函数证明完成。

普通 controller 必须先收束自己仍持有的所有原始 Promise，再消费该证据；
重启恢复可消费已保存的终态。onRemoved 仅触发已知原 tab 的清理重试，
不扫描其他标签，也不单凭该事件宣称原 JS 已停止。回执丢失且原文档不再可达时
仍保留占用，不虚报完成。扩展升级
前没有安装守卫的记录、完整浏览器退出以及 App 重启另行处理。

[对应版本 Chromium observer](https://github.com/chromium/chromium/blob/148.0.7778.96/chrome/browser/extensions/api/web_navigation/web_navigation_tab_observer.cc)

**发送通道增量**：实际 SW 停止后 reload，原文档已完成并发起 runtime.sendMessage，
但导航销毁发送方时消息未交付，直接回执方案仍卡住。当前 guardian 先将独立的
256-bit 一次性 terminal nonce 写 extension storage.local，再发 runtime nudge。
nonce 由 SW 创建，预先保存在 trusted session journal v3 并仅交给精确原文档的
固定 guardian；不复用 completionKey、不含 Host endpoint/proof 或页面/动作数据。
local 暂存的形状仅为随机 requestId 键与 nonce 值。只有匹配当前 journal owner
的 nonce 才能将其转为 physicallySettled；不能 claim、恢复配对或制造业务成功。

storage.onChanged 唤醒及 startup/provePhysical 补读使用同一验证。先确认 session
终态落库，再删 local receipt；删除失败仍持有原 journal owner。旧 key 无法完成
新 owner，历史 v1/v2 记录 terminalToken=null，不伪造守卫证据。最多 8 个准入
owner；已完成的旧 guardian 可能短暂写入孤立 key，onChanged 和启动均只清理
本协议前缀的孤立键，其他扩展本地数据保留。local 区域须保持 content-script 可写，
不得在其中放 pairing Bearer/full proof；session 区域继续 TRUSTED_CONTEXTS。
缓存文档不会写 terminal receipt；仍等待恢复后的原文档取消完成回执。

下面为已否决方案的原始分析，不是当前实现依据：

本轮采用 `webNavigation.getFrame({documentId})` 的原始文档查询，不用 tab 当前
frameId、不枚举 getAllFrames，也不把 scripting 异常文本当成销毁证明。Chromium
的 documentId map 以 DocumentUserData 注册/析构；按 documentId 查询包含
BFCache 文档，而 getAllFrames 明确跳过它。cached/prerender/pending_deletion
均仍是存活文档，不能因导航或超时释放。

新增 webNavigation 扩展权限只用于已明确分享且参与 cleanup 的原 documentId。
claim 前先验证同 tab/main-frame、活跃 HTTP(S) 文档能被该 API 读取，并记录
incognito 属性。原文档查询得到明确 null 才可作为销毁证明；undefined、错误、
畸形回包、身份不符不能。私密窗口还必须在查询前后均有 incognito 访问权限，
权限被撤回不等于文档被销毁。当前 tab 不再含该 frame 也不是证明，清理查询
故意只带 documentId，避免 tab/frame 匹配失败被当作不存在。

journal v2 追加只含 incognito 布尔值的 documentContext。v1 记录明确迁移成
unattested，不据 null 释放，仍可凭原文档取消完成回执清理。记录不保存 URL、
页面内容、输入或新执行权限。正常物理完成仍等待原 script/cancel/watcher；
未知完成后的后台工作只重试固定取消/原文档销毁查询，不再 claim/act/发业务结果。
本进程的 SharedTabs 保留对应 request 身份，证据成立后只释放它自己的旧槽位。

官方依据（只读核对，2026-09-20）：

- [webNavigation getFrame](https://developer.chrome.com/docs/extensions/reference/api/webNavigation#method-getFrame)
- [Chromium getFrame 与 getAllFrames 实现](https://github.com/chromium/chromium/blob/main/chrome/browser/extensions/api/web_navigation/web_navigation_api.cc)
- [DocumentUserData 注册/析构与查找](https://github.com/chromium/chromium/blob/main/extensions/browser/extension_api_frame_id_map.cc)
- [FrameNavigationState 的文档级生命周期](https://github.com/chromium/chromium/blob/main/chrome/browser/extensions/api/web_navigation/frame_navigation_state.cc)

仍须实机证明 reload/navigation/close/BFCache 的差别；源码依据不能替代真实
浏览器证据。扩展升级旧 world、整个浏览器退出和 App 重启仍分别处理。

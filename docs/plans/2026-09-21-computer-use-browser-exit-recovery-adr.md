# 浏览器整体退出恢复：先验证启动事件，再接持久化清理

状态：实施前置验证，未修复整体退出卡死，不开放 ExistingTab actions。

## 实际首败

`existing-browser-exit-red.log` 已执行：生产 SW 的 claimed click 回包持有；旧点击
为零；整个隔离 Chrome 关闭；同一个 owner profile 重新启动；session journal
和 pairing 均不存在；重新配对和 Share 成功。原 Host 返回 `phase=claimed`、
`cancelRequested=true`，随后等待原占用归还超时。测试没有替扩展提交旧 proof。

这是原生完整浏览器退出，不是只重启 SW；当前首败为正常关闭，崩溃仍待单独验证。

## 证据与分层

Chrome 官方文档：<https://developer.chrome.com/docs/extensions/reference/api/runtime#event-onStartup>
（2026-09-21 实际读取）定义：profile 第一次启动时触发，incognito profile 启动
不会触发。它没有把普通 SW 启动、runtime.reload 或 storage.session 丢失定义为
完整 profile 启动。`onSuspend` 中的异步清理不保证完成，不能用它承担最终回执。

先建立 BrowserSession 的非敏感 storage.local 标记。同步注册浏览器 onStartup
监听。SW 重启和 runtime.reload 保留身份。空存储上本 worker 新建的 started=false
只在第一次真实 onStartup 绑定为 started=true，不发明 previous owner。已经持久化的
started=false（扩展装进正在运行的浏览器）以及之后每一次 onStartup 都换成新 id，
previousId 指向换代前的 id。写失败不发布这次换代。缺标记、重复回调无法和另一次
真实启动区分时保持确定性代际，不能据此退休旧文档。新动作总是标记当前身份，即使
onStartup 稍后到达，也不应被当成旧动作。

本阶段仅验证标记与事件，不能让该标记单独释放 Host pending。需要真实门禁：
完整浏览器重启时新身份且 started=true；SW 重启身份不变；扩展 reload/update 后
不得伪造 startup；缺事件、持久化失败和迟到事件保持保守。必须保留 BFCache 负例。

## 后续持久化方案的要求

选择扩展自身 origin 的 IndexedDB 保存有界 cleanup-only journal，以避免把
completion proof 放入页面 content script 可用的 storage.local 区域。不能保存
pairing Bearer、动作输入、页面内容或完整命令。原 completion key 只允许清理自己
的请求，不恢复读取/执行权限；仍须原 Host 的完整认证和最终物理完成证据。

这将明确修订当前“Host proof 只在 session”的存储决策，而不是假装原有内存日志
已经支持浏览器退出。先实现原子、可迁移、容量有界的私有持久层与测试，再修改
生产 journal；写入必须在 claim 前确认，删除失败不能释放本地 owner。

旧浏览器身份与新 onStartup 的关系必须来自上述原生事件门禁，不得仅因随机 ID
不同、session 为空、连接断开、超时、重新配对或找不到 documentId 而清占用。
Host 只在 previous browser id、request id、document id 与扩展给出的
prepared/physicallySettled 清理记录完全一致时移除该条占用，结果只能是
unknown 或 cleanup。写失败不注入脚本。错 id、错 document、applied 相位
或其他 owner 一律保持占用。如果事件语义/顺序的实际证据不足，应继续保留
busy，不把不确定性写成成功。

后续完成要求仍包括 App/浏览器双重退出、renderer crash、扩展升级旧 world、
旧版无 guardian 记录、真实 session MCP actions，以及三 OS/安装/模型/长稳验收。

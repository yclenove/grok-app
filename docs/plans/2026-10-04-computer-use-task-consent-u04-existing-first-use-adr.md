# E3：默认关闭时的 ExistingTab 首次配对 / 分享

状态：implementation ADR，完整 CU active / partial — not releasable。
接续 E2；不改变默认值、不替用户批准，不创建平行 TaskGrant 或 continuation。

## 决定

1. `begin_task_scope_picker` 是 Host 用户已选择 ExistingTab 的原手势边界。
   只有这一入口在原 authority lock 内建立 metadata preparation；Desktop、挂载、
   status 和模型申请都不建立它。记录绑定 full HostRequestKey、原 TransportContext、
   selection revision、feature epoch 和原取消句柄，另有不超过五分钟的硬期限，
   原十分钟 request 期限仍独立生效。一次 App instance 至多一个准备 owner。
2. preparation 只保存原 challenge 或完整 connection 身份，不保存 bearer。它不能
   observe、借用 tab、发布 feature / run / grant / continuation。开始时原连接尚活则
   复用，不重新配对；外国 pending challenge 返回 busy，不覆盖或撤销它。
3. 原 picker receipt 提供 Host-only challenge 读取与显式确认方法。确认必须复验原
   receipt 和 nonce。HTTP challenge 不返回 code；HTTP complete 仍核协议、全身份、
   HMAC 和一次性消费，只有原准备绑定的 challenge 可在 off 时完成。
4. off 时只允许该准备的原 connection 进行 status/heartbeat、browser lifecycle
   metadata、Share/Unshare。Origin、token、connection、sequence、document 的既有
   校验不变。observe/action/poll/result/completion/模型执行 gate 不放宽；v2 输入
   negotiation 也不在准备期放行。最终 enable + TaskGrant + App/ACP publication
   仍走原 reservation 和最终 commit。
   原扩展先经认证status等到executionReady再启动两个原执行loop；准备期false不被
   误当配对失败，不发poll/observe/action。旧enabled Host未带此字段仍兼容；新Host
   显式false严格等待。旧epoch迟到status和reset不能启动新loop。
5. Pair 是用户明确确认的设备连接，Share 是用户 activeTab 手势产生的浏览器分享，
   不是 task execution grant。已完成 Pair / Share 不因取消 picker 而由 Host 反向
   取消；此规则同样适用于准备期间新完成的用户 Pair / Share。准备拥有的是有限的
   metadata admission 和未完成 challenge，而不是用户已完成的设备设置或 Share。
   Cancel/Drop/Stop/请求或连接换代/过期只退休本次 admission 和确切 nonce 的 pending
   challenge。off 后无其它准备者则不能保活，原30秒 connection lease自然到期。
   显式 feature-off 的既有全局 revoke 合同保持不变。
6. discover 移交同一个 guard 给原 picker；发现、选择和最终审批不能暗中刷新或替换
   pairing/share proof。进入原 reservation 后 Share更新通道关闭；仅原有效preparation
   的精确connection status/heartbeat桥接reservation→enable空隙，仍返回false，避免
   扩展在批准持久化期间误断开。原reservation被fence/abort则桥接也关闭。最终 TaskGrant
   成功不会因旧 picker Drop 被撤销。旧 Drop 不得清理新 preparation 或 foreign
   challenge / connection。维护和各正式 endpoint 都复验原请求，过期即拒绝。

## 验证顺序

先证明正式 Broker feature-off 的首次 picker 没有 challenge，保留原断言失败。
再通过正式 Broker + authenticated HTTP 完成显式确认、Pair、保活、Share、原
picker → selection → reservation → feature publication → TaskGrant → Stop。
用户 / extension proof 为 synthetic，不等于真人MV3/主窗/Grok/安装态。

取消矩阵包括：未选择/错 surface/full key、confirm前后Cancel/Drop、完成Pair后
Stop/换request/agent/transport/feature epoch、原deadline、stale nonce/proof/connection、
foreign pending/已有connection和Share、旧guard对新owner的Drop、reservation前后。
独立核对 feature、run、grant、有效continuation、原 registry 与零内容/动作请求。
之后才接 E4 App命令/UI/15locale、E5同候选source MV3/App/ACP联合验收。

不启动日常浏览器、不读凭据、不改代理，不commit/push/PR/发布；新日志不覆盖旧
日志，每批原owners退出后按AGENTS第9条精确清理，不复用已执行的旧清理计划。

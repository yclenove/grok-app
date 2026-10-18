# 0015 持久授权使用完整应用身份

状态：已接受（G1。03 §7.2 不改。用户已授权总控启动本卡。）
日期：2026-10-18

## 背景

`AppIdentity::grant_eq` 对有效签名只比较 bundle/路径加 Team 或发布者，忽略路径、内容哈希、cdhash、证书和产品/build。`always_grant_follows_platform_identity_and_can_be_disabled` 把这种宽匹配写成了断言。驱动自己的 `needs_reauth` 更严，但引擎持久链不调用它。`host_app` 又把签名固定成 `Unknown`，所以线上始终允许实际走的是结构相等，不是那条宽匹配。

## 决定

持久授权的比较只有 `gcu_protocol::grant::persistent_eq`。路线 A 的 `AppIdentity::grant_eq` 调用它。路线 B 的 `persistent_grant_matches` 也调用它。`gcu-host-protocol` 不依赖 `gcu-host-api`（03 §2）。缺任何必需证据都不匹配。

- macOS 有效签名：bundle id、路径、内容 SHA-256、cdhash、证书 SHA-256、发布者主体、Team、产品、build。
- Windows 有效签名：路径（key 与路径必须相同）、内容 SHA-256（这就是 build 摘要；没有单独的版本字段。若记录里有 build 字符串，两边必须一致）、证书 SHA-256、发布者主体、产品。
- Linux：路径、内容 SHA-256、设备号、inode。key 仍必须相同。没有证书时两边都缺才算一致。
- `Invalid` 和 `Unknown` 永不匹配。未签名要求路径、内容哈希，以及该平台仍能证明的代码身份（macOS 的 cdhash，Linux 的 dev/inode）。

旧的窄记录不升级。缺的证据无法从旧行证明，所以旧行对任何身份都不匹配，包括它自己。行还在，按存储键撤销仍然有效。之后用户再次批准，同一存储键被完整身份替换。那是一次新批准，不是静默升级。

驱动把证书、cdhash、产品和签名状态放在现有 `publisher` JSON 里。Linux 把 dev/inode 放在同一字段。`host_app` 只在 JSON 有明确 `state` 时才设置签名，不从一串 Team 文本发明 `Valid`。解析后发布者字段只留主体，审批卡不显示原始 JSON。

`Identity.proof` 和 `ImageBinding.proof` 缺省为空，空证明不写出。协议版本不升。旧对端遇到非空 `proof` 会因 `deny_unknown_fields` 拒绝报文，不会退回宽匹配。同一版本的引擎和 Host 一起发。

存储去重可以是 `grant_eq` 或逐字段相等，以免相同的旧行被追加两次。去重不是授权。

## 后果

应用更新如果改了路径、内容哈希、cdhash、证书、发布者、Team、产品或 build，「始终允许」会再次询问。这是 03 §7.2 的要求。无签名的 Windows 映像现在也带上 `state`，否则引擎无法把它和 `Unknown` 分开。可写加载路径仍由驱动的 `needs_reauth` 报告，不单独进入这条授权键。真机产品链尚未跑。

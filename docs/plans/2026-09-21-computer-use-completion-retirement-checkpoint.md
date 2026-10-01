# ExistingTab：完成墓碑淘汰后的 cleanup-only 恢复

日期：2026-09-21（America/Los_Angeles）。
工作区 `H:\aicoding\grok-app-computer-use`；分支 `feat/computer-use-implementation`；
HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。未提交/推送/PR。
总状态 **partial — not releasable**；默认关闭，ExistingTab action capability 仍 NONE。

前置：[文档生命周期检查点](2026-09-21-computer-use-document-lifetime-checkpoint.md)。
决策：[完成退休 ADR](2026-09-21-computer-use-completion-retirement-adr.md)。

## 首败与修复

R3 首先复现：Host 已接受 `settle`，扩展丢失回复，随后 64 条容量或 5 分钟 TTL
淘汰 tombstone。旧 `settle` 重试只能得到 unavailable，本地
`physicallySettled` journal 永久 busy。

加入 retirement 后，首次实机回归暴露两个独立问题：

1. 旧 live 断言仍要求第二次 `settle`。新协议正确行为是一次 `settle` 已被 Host
   接受，再用一次只读 `retirement=settled` 清理本地 journal。断言已改为检查
   settle/retirement 各一次及 storage 删除，不放宽动作次数。
2. tombstone 真实过期后，retirement 已返回认证的 `absent`，但 Host 的业务结果
   仍调用旧 `status()`，因此把已执行一次的动作判成错误。Host 结果准入已改用
   retirement；仍要求原 action pending、完整 request/proof 和 claimed 状态，
   `pending` 继续拒绝成功，重复结果继续失败。

实现包含：

- completion key 改为当前 Host 进程 authority 对完整 binding 的 HMAC 派生；
- 新增严格、只读的 `/cu/extension-completion/retirement`；
- 扩展仅在本地 `physicallySettled` 持久化后使用 settle→retirement fallback；
- 新增容量、TTL、错 proof、旧 App authority、pending/settled/absent、畸形回复、
  无 journal、业务结果单次提交的 Core/HTTP/Node/真实 Chromium 回归；
- 私有 probe 的 tombstone 过期入口只在 test/test-support 构建存在。

## 当前验证

日志根：`tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/`。

| 检查 | 结果 / 文件 |
| --- | --- |
| 扩展/MCP Node 全量 | `existing-retirement-node-full.log`：142 passed，0 skipped；串行 |
| Core 全量 | `existing-retirement-core-full.log`：448 passed，0 failed |
| Driver integration | `existing-retirement-driver.log`：12 passed，0 ignored；真实私有 worker |
| 定向 Host 结果 | `existing-retirement-host-result.log`：墓碑过期后单次正向结果通过 |
| 生产 SW/Host 动作 | `existing-retirement-live-final.log`：23 passed；含真实 TTL 淘汰、零重放和后续新 click |
| 观察/MCP | `existing-retirement-observation.log`：34 passed |
| 真实 BFCache | `existing-retirement-bfcache.log`：4 passed |
| 历史 completion fixture | `existing-retirement-completion.log`：8 passed |
| strict Core Clippy | `existing-retirement-clippy-core.log`：all targets/features，`-D warnings` |
| strict App Clippy | `existing-retirement-clippy-app.log` 与 `existing-retirement-clippy-app-probe.log`：默认/probe 均通过 |
| 格式/locale/质量 | `existing-retirement-fmt-final.log`、`existing-retirement-locale.log`、`existing-retirement-quality.log`：15 locales，final PASS，千行文件 80/80 |
| whitespace | `existing-retirement-diff-check.log`：exit 0；仅既有 LF/CRLF 提示 |

容量证据在 Core registry/HTTP 合同中覆盖：finished tombstone 始终最多 64 条，旧 proof
经同一 authority 认证后为 `absent`；pending 不被容量淘汰。实机行使用产品 sweep
路径淘汰 tombstone：私有 probe 将其时间戳回拨到超过产品 5 分钟 TTL，再调用
生产 sweep；没有等待五分钟墙钟时间。随后释放被暂停的
retirement 请求；原 click 恰好一次，本地 journal 删除，下一次 click 恰好一次。

## 局部指纹与清理

源码 328 文件 SHA256：
`7F759295D734B350564440C889AE08EFDAF759BE92D29E16C430A6001DF41CC9`。
cu_probe SHA256：
`D51F6AE3BAA7C01E53C0C7AF195224D65184851B89031E3254244D3ECB9F13B1`。

范围/算法沿用文档生命周期检查点；新增一个实机 retirement 源文件。日志见
`existing-retirement-fingerprint.log`。这是局部追溯，不是 C6 发布冻结。

最终 owner/App home/profile 精确检查未发现 cu_probe、私有 Node 或 Chrome 残留。
没有操作日常浏览器、真实账号、Cookie、代理或 VPN；没有新增子代理。

## 下一项与边界

R3 已关闭，不重做 retirement 或放宽 proof。继续 R4：

1. 建立实际 App crash/new endpoint 的失败测试，证明新进程 authority 拒绝旧 proof，
   且不能把新 registry 的 `absent` 当作旧动作完成。
2. 分别验证 App→SW、SW→App 两种重启顺序；每个切点核对动作次数、Host occupancy、
   journal owner、旧 endpoint/旧 connection 和新授权隔离。
3. 覆盖完整浏览器退出后 session journal 丢失、Host claimed 仍存在的情况；不把
   local terminal nonce 当成磁盘恢复协议。
4. 回到未关闭的 renderer crash、扩展 reload/update 旧 isolated world 和旧版无
   guardian 记录；这些缺口不能由 R3 证据代替。
5. R1–R4 全部关闭后才接 `adapter.act` 和真正 session MCP actions；随后仍有 parity、
   原生 toolbar + 成功截图、App/ACP/UI、安装 Chrome/Edge、三 OS、真实模型、
   ≥12h 主动长稳及历史 Windows IPC 10053 根因。

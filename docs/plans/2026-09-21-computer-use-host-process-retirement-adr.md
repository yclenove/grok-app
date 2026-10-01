# ADR：用原 Host 内核进程身份完成跨 App 清理

状态：已实施，真实 Host 五切点及 Host/SW 三种顺序共 15 个场景通过。
完整验证与剩余项见 [检查点](2026-09-21-computer-use-host-process-retirement-checkpoint.md)；
不开放 ExistingTab actions，不等于 R4 全部通过或完整 Tauri UI 崩溃验收。

## 首败

`existing-app-restart-red.log` 已真实终止独立 App Host 子进程，保留原浏览器。
accepted claim 的回包被持有，旧 click 为 0；新 endpoint 拒绝旧 completion proof
和 connection（403），原 endpoint 不可达。本地物理完成后仍永久 busy，失败点
为 `physically-finished-old-instance-releases-journal`。不是替换内存 registry 的模拟。

## 决策与信任来源

1. claim 前，生产 CompletionScope 用原 completion proof 请求原 Host 的只读
   `host-lifetime`。原 Host 先认证完整 proof，再返回自身内核 PID、创建标识、
   平台/进程命名空间及原 App instance。扩展核对原 binding，把它写入 trusted
   session journal v4，然后才安装 guardian 和 claim。不可读取时零 claim。
2. v1–v3 迁移为 `hostLifetime:null`，不可事后根据新 endpoint 猜出旧进程。
   记录不落磁盘、不含 pairing Bearer、页面/动作内容。只增加非敏感内核元数据。
3. 原 settle/retirement 失败后，且当前 owner 已持久化 `physicallySettled`，
   扩展可经用户新配对的 App 请求 `host-retirement`。请求带当前 connection
   和原记录中的内核 witness；响应必须回显完全相同的 witness。
4. 新 Host 校验当前配对/feature/Origin，读取内核中的精确进程身份。相同创建
   标识且进程活着为 live；明确不存在、已 signaled，或 PID 已由不同创建标识
   的进程占用才为 retired。权限错误、平台或命名空间不一致、读失败为 unavailable。
   Windows 用 process handle/GetProcessTimes/零等待；Linux 用 boot + PID namespace
   + proc starttime，并用 ESRCH 复核缺失；macOS 用 proc_pidinfo 的启动时间。
5. 这份响应只证明客户端事先保存的原进程已消失，不认证旧 completion proof，
   不发布动作成功，不释放任何 Host pending，不恢复授权。新 Host 的原 R3
   retirement/claim/status/result 仍拒绝旧 proof。客户端严格使用自己的原 journal，
   不提供任意 witness 覆盖入口。当前 connection 变化会丢弃迟到回复。
6. live/unavailable、无新配对、无原 witness、畸形回复或删除失败均保留 owner。
   普通控制器必须先 join 原 act/cancel/watcher；恢复控制器先取得原文档完成证明。
   新配对本身不释放 busy，内核证明也不能跳过物理完成。

进程 witness 不是可转移的签名凭据。它的来源是原 Host 已认证 completion scope
的响应，归属于扩展私有 journal；新的查询仅作内核事实判断。不能把该只读端点
单独当作任意 proof 的完成验证器。测试必须证明篡改/错 owner 无法替换 journal
里的 witness，并证明新实例中正在运行的占用不会被此通道改变。

## 门禁

- 实际 App 退出：claim 前、claim 回复滞留、原生注入前、Wait 中、click 回包滞留。
- App→SW、SW→App；每行有旧/新动作计数、journal、两个 Host/connection 的证据。
- 存活的另一 App、PID 复用、权限/命名空间异常、无新配对、旧 journal、畸形回复。
- Host reply 丢失与 session 删除失败；恢复后原 tab 新 click 恰好一次。
- 原 R3 HTTP/Core、生产 dispatch/观察/BFCache、Node、Clippy、fmt/质量门禁。

完整浏览器退出、renderer crash、扩展升级旧 world、无 guardian 旧记录和真实
session MCP actions 仍为后续项，不用这份进程证明代替文档销毁证明。

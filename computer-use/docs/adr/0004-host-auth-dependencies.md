# 0004 Host wire authentication and schema dependencies
状态：接受（P0-12 派工授权的最小依赖与物理活动决定）
日期：2026-10-13

## 背景（事实 + 证据链接）
[03 §15](../../../docs/plans/computer-use-v2/03-architecture.md#15-host-协议路线-b-松耦合集成)
及安全评审 #1/#2/#13/#18/#21 要求独立双向认证、进程实例绑定、秘密 FD、严格 schema
与独立 Stop。原 host-protocol 规则缺少实现这些门禁所需的密码学/平台元数据库。

## 决定
只扩展 03 §2 与 xtask RULES 的依赖表，不改变 §15 消息表或 core/Host 公共接口。
使用 hmac 0.13 + workspace sha2 0.11 计算/常数时间核验 HMAC-SHA256，rand 0.10
生成秘密与 nonce；serde_json 编解码帧，schemars 从 Rust 类型生成实际 wire schema，
thiserror 为库错误。interprocess 2.4 提供 UDS/本机命名管道；tokio 的 io/net/sync/time/rt
支持非阻塞连接、背压和期限（process 仅 testing）。Unix libc 与 Windows windows 0.62
只用于进程启动时间、继承描述符、所有者/DACL，所有 FFI 集中在 unsafe_platform。
interprocess 2.4.5 的 doctest-file/recvmsg 传递依赖实际声明 0BSD，属于03 §2
允许的 BSD 家族；deny.toml 仅对这两个具名 crate 允许该变体，不扩大全局许可证范围。
CLI 的 reqwest 0.13 关闭默认 TLS/代理/重定向，只供 loopback 代理及测试使用；rmcp
启用已有上游 HTTP client feature，MCP 服务端复用同一 dispatcher。

物理活动按总控方案 3 由引擎驱动提供，RemoteHost 组合可注入 PhysicalActivity；
默认不可用在 doctor 既有 capabilities/issues 表达。不新增活动 RPC，不改公共 DoctorReport。
Host 只镜像其持久 Stop。恢复仍受 ADR 0003 的可信用户事件边界约束。

## 备选与为什么不选
手写密码学或 JSON schema：难审计且易偏离 Rust 类型；将 core/host-api 链入协议：
违反 App 依赖边界；把物理活动放入 Host 消息：与驱动职责和获批消息表冲突。

## 影响（对接口、测试、用户）
App 仍只链接两个协议 crate。新依赖受 check-deps/deny/跨目标 clippy 校验；帧、认证、
端点替换、恢复重放、断线及 HTTP 来源校验有自动化测试。无持久授权回写 RPC。

## 退出条件（什么情况下推翻本决定）
上游提供等价且更小的认证/进程实例 API，或经批准变更 Host 信任边界/平台启动方式。

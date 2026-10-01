# Computer Use：Windows 独立派发前置校验

日期：2026-09-30（本机 +08:00）。工作区：`H:\aicoding\grok-app-computer-use`。
分支：`feat/computer-use-implementation`；HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。

**完整 goal 继续 active / partial — not releasable。** 上一阶段为 progress，现有独立 observer 已证明 ACK 后 custody。本轮把未来 dispatcher 必需的原生启动计划和原签名产物验证从 App `Update` 中分离，并修复相应产品路径；**没有把前置校验当作独立派发完成，也没有关闭父 App 的 ACK 前崩溃窗口**。原始范围仍以 2026-09-09 总计划及执行状态账本为准。

## 生产改动

- 新 `launch_plan.rs` 将绝对安装器路径、系统目录 MSI 路径、三种 install mode、原 App restart args 和自定义 installer args 编译为不可从 journal 反序列化的原生计划。首次 staging **在服务清理和 journal.prepare 之前**构造；Prepared 恢复在原签名/文件复验之后构造。launch 只使用保留的计划，不再在已清理 App 中临时拼参数。
- 拒绝路径、argv[0]、restart/custom 参数中的嵌入 NUL，以及超过 UTF-16 命令长度边界的数据。确定的预检失败使用专门错误，原有 catalog 发出 **Failed / staging** 终态；清理计数为零，没有活动候选落盘，纠正数据可以开始新 check，不误归类为未知派发阻断。已有 LaunchIntent/Accepted 的不可重放约束不变。
- NSIS/MSI 参数转义直接处理原生 UTF-16 code units，避免 `to_string_lossy` 把 Windows 未配对 surrogate 改成另一条路径。对有效 Unicode 保持 pinned upstream 的转义/顺序与三个 mode，不宣称已完成真实 NSIS/WiX 安装器重启参数验收。
- `ShellExecuteExW` 增加 `SEE_MASK_NOASYNC`，保留系统 msiexec、NO_UI 与原 process handle。该标志只针对 Shell 交接，不把 OS 接收或 installer exit 当作安装成功。
- 新 `verified_artifact.rs` 复用原签名 payload、可信调用者 key、payload/member digest、精确 archive member 与文件 lease 校验；当前生产恢复链已接入。它不从 journal 读取一个新的“可信公钥”，不启动安装器、不授予新授权。
- 验证发现 pinned ZIP reader 的 `file_names` **和 `by_index/len` 都会折叠重名条目**。仅换成 by_index 不足以修复。本轮检查 crate 索引 central headers 对物理目录的完整覆盖，拒绝被隐藏的重名 member；解压/CRC/ZIP64 仍由原 crate 完成，不另写 ZIP 解压器。
- 只改 vendor 的 7 个源/说明文件（含 2 个新模块）；App main/UI/15 locales/权限与 Cargo.lock 不变。observer 仍没有派发安装器能力。

## 实测及修复记录

首次编译发现测试访问旧 `installer` 字段，已同步为保留 temp-file 生命周期的字段。首次全库回归发现预检错误错误地复用 pending 类型，catalog 因此保持 blocked；没有把测试改成接受阻断，而是增加明确 preflight 错误并验证 Failed 终态及修复数据后的新 check。首次局部 Clippy 暴露仅测试用 Kind 导入，已移至测试模块。所有首次失败/警告日志保留；最终冻结后检查重新执行。

## 当前冻结验证

证据根：`tools/computer-use-probe/.run/windows-update-dispatch-preflight-20260930/`。

| 项目 | 当前证据 | 限定范围 |
|---|---:|---|
| 默认 updater | **53 passed，5 ignored** | 全部该库原生测试；5 个专属 child 入口由父测试真实执行 |
| 独立 no-zip updater | **52 passed，5 ignored** | artifact features 仅 rustls-tls，同跑所有适用恢复/见证/新 preflight 测试 |
| 转义对照 | **11,111 个语法样例 × NSIS/MSI** | 有效 Unicode 对比 pinned upstream；另测未配对 UTF-16 code units，不冒充安装版往返验收 |
| 首次预检 | 通过 | durable/非 durable × argv[0]/restart/custom NUL/超长，零 cleanup；Failed receipt、拒绝 resume、修复后新 nonce |
| 签名与保留产物 | 通过 | 空/错 key、signature、payload/member hash、kind/member 改绑均拒绝；原文件只读 lease 和 journal 不变 |
| ZIP 验证 | 通过 | 双物理 installer 被旧 map 折为 1 的明确复现；新路径拒绝；正常 ZIP64/中文/central extra/comment 及逐字节截断测试 |
| 原进程防重放回归 | 通过 | 父进程真实退出后 1603、独立 custody/259、ready/exit 错绑、Accepted fence 等在两配置继续运行 |
| App lib | 编译 + **选定 38/38** | 1733 可用测试，只执行 updater 7 / shutdown 3 / browser owner 7 / MCP owner 21 |
| 真正 App debug EXE | 构建与副本生产入口通过 | 额外参数拒绝 65；原 handle 私有交接、ready、真实 owned fixture exit 259、helper exit 0；未执行真实安装 |
| 前端 | **135/135，5 文件** | Hook35、协议3、honesty46、About/Sidebar9、locale42 |
| Clippy | 两套无 warning | App+vendor lib/tests；独立 no-zip lib/tests；检查日志，不只看 cap-lints 下退出码 |
| TS / ESLint / Rust 格式 / diff | 5 项退出 0 | ESLint 8 文件；worktree 已有 CRLF 提示不是失败 |
| 选定源冻结 | **80 项零漂移** | 更新链及直接依赖，不是全产品最终冻结 |
| 来源与依赖 | 通过 | 19 pinned 原件、API bundle/生成 ACL 对账；App 673 包 lock hash 不变；nozip 420 包中 419 个依赖均与 App lock 一致 |

Cargo 全程 offline + locked，未修改 registry 依赖、新增依赖版本或降低 lint。实际 App 副本、观察夹具和测试子进程均属于本轮创建的私有对象；不按名称终止用户进程，结束后清理私有副本。没有改用户权限/剪贴板、账号或运行安装器。远程文档读取尝试被策略拒绝，未绕过，也未把它记为验证通过。

新 receipt 会封存本轮源、日志/审计和四份文档，随后独立逐项 SHA-256 复核。前一封存目录与检查点不回写，历史索引可追加本次入口。

## 下一项与完整范围

1. **独立 dispatcher 尚未实现。** 下一项须让 worker 成为唯一安装派发者：从编译内 App 配置取得可信 key/模式，在清理成功后授予一次性持久 ticket；worker 自己持有 OS 返回的原 process handle；只发布绑定同候选/身份的不可替换 dispatch/exit receipt。不能只是把父 App 的句柄转移再包一层，也不能通过缺失 PID/超时自动换 helper 或重放。
2. 必须真实注入父 App 在授权、OS 接收、ACK 前后的死亡，证明不是把窗口搬到 pipe 写入/主 journal 多写者竞争。当前 helper/断电不确定性仍安全阻断，不能借“blocked”宣称最终可用。
3. 原签名候选与实际安装文件、安装完成/失败/回滚 receipt、结果对账、journal 退休、下一轮更新释放仍待完成；exit 0、259、版本一致均不足。
4. 真实签名 NSIS/MSI/UAC、安装版重启/修复/回滚/卸载、部分清理服务恢复、ordinary Quit、orphan 身份安全 GC 和完整 input/IME/clipboard 生命周期仍需完成。
5. Windows x64、macOS arm64/x64、Linux X11 与原生 GNOME Wayland；Desktop/managed browser/existing tabs/App WebView；App/ACP/MCP；完整输入/取消/恢复；签名交付/更新/回滚；窄窗/缩放/权限原生 UI；真实 Grok E4；**同一最终冻结候选 12h active soak** 全部保留，未改成较小首版。

未 commit/push/PR/release，没有开始新的 soak，没有把 goal 标为 complete/blocked/paused。本轮是实际代码、回归和证据进展，不是最终完成审核。
前一检查点：`2026-09-30-computer-use-windows-update-independent-witness-checkpoint.md`。
前一 receipt SHA-256：`98a92b5ff5243dbbc5b3ecb4a381fabd56d1b59b43e85e16db8fae6f24648212`。

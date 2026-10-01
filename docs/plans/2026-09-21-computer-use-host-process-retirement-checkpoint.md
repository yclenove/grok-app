# ExistingTab：真实 Host 崩溃后的清理恢复

日期：2026-09-21。工作区 `H:\aicoding\grok-app-computer-use`；
分支 `feat/computer-use-implementation`；HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。
未提交、推送或 PR。总状态 **partial — not releasable**，默认关闭，
ExistingTab action capability 仍为 NONE。

前置：[R3 检查点](2026-09-21-computer-use-completion-retirement-checkpoint.md)。
决策：[原 Host 进程身份 ADR](2026-09-21-computer-use-host-process-retirement-adr.md)。

## 首败与实现

`existing-app-restart-red.log` 真实终止了产品 Host 子进程，保留浏览器及原文档。
新 Host 拒绝旧 connection/proof；旧点击没有发生，但原操作结束后本地清理记录
仍永久 busy。失败不是内存 registry 的模拟，也不是整个浏览器一起退出。

修复增加两份独立证据：原文档/原生调用确实结束，以及原 Host 内核进程确实
不再存在。claim 前从原 Host 认证的 completion scope 捕获进程创建身份，写入
扩展 trusted session journal v4；旧版记录迁移为无 witness，不猜测旧进程。
原 settle 与同进程 retirement 都不可用后，只有本地物理完成的原 owner 才能
经当前新配对的 Host 查询该进程是否消失。查询不认证旧 proof、不修改 Host
pending、不产生业务成功、不恢复授权，也不增加持久化 Bearer/签名密钥。

Windows 使用进程句柄、创建时间与零等待；Linux 使用 boot/PID namespace/proc
starttime；macOS 使用 proc_pidinfo。权限或身份查询不确定时保留占用。
本机验证的是 Windows，Linux/macOS 原生运行仍为 `not_run`。

探针把浏览器和可崩溃 Host 分给独立所有者。私有 stdio 控制明确的子进程，
Windows Job 负责确认自有进程树退出；产品没有故障注入 HTTP 端点。

## 验证记录

日志根：`tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/`。

| 检查 | 结果 / 日志 |
| --- | --- |
| 单一 Host crash 首次修复 | `existing-app-restart-live-fixed.log`：3 passed，含一个 held-claim 场景 |
| 15 个切点/顺序矩阵首轮 | `existing-app-restart-matrix-first.log`：13 个场景通过后触及旧整批 180 秒上限；整批失败，不能算 15 项通过 |
| 最终实际 Host/SW 重启矩阵 | `existing-app-restart-matrix-final.log`：17 passed；15 个重启场景和 2 个启动检查 |
| 生产 SW/Host 动作回归 | `existing-app-restart-dispatch.log`：23 passed |
| 实际 BFCache | `existing-app-restart-bfcache.log`：4 passed |
| 观察/MCP 回归 | `existing-app-restart-observation.log`：34 passed |
| 历史 completion fixture | `existing-app-restart-completion.log`：8 passed |
| 扩展/MCP Node 全量 | `existing-app-restart-node-final.log`：148 passed，0 skipped |
| Core 全量 | `existing-app-restart-core.log`：450 passed，0 failed |
| Driver integration / 格式 | `existing-app-restart-driver.log`：12 passed，0 ignored；`-fmt.log`：exit 0 |
| strict Clippy | `existing-app-restart-clippy-core.log`、`-clippy-app.log`、`-clippy-probe.log`：Core all-targets/all-features、默认 App 和 probe App 均通过，`-D warnings` |
| 最终 probe 构建 | `existing-app-restart-build-final.log`：成功；仅 MSVC 导入库创建信息 |
| 15 locales / 质量 | `existing-app-restart-locale.log`、`-quality-final.log`：通过；千行文件 80/80 |
| whitespace | `existing-app-restart-diff-check.log`：exit 0，仅既有 LF/CRLF 提示 |

完整矩阵及本批涉及的既有回归全部通过，Cargo/浏览器/App 重型验证串行执行。
矩阵新增十四个场景，整批预算按检查数计算；每个请求、动作和后置条件的超时
未放宽。每行独立检查旧/新点击次数、Host 占用、journal、原 tab 存活及新旧
权限隔离。持有原 act 回包时，单独释放 cancel 回包仍必须保持 prepared。

| 时点 | 只重启 Host | Host→SW | SW→Host | 原动作效果 / 恢复后新点击 |
| --- | --- | --- | --- | --- |
| claim 回包滞留 | 通过 | 通过 | 通过 | 0 / 1 |
| claim 前 session 写入回包滞留 | 通过 | 通过 | 通过 | 0 / 1 |
| native 注入前 | 通过 | 通过 | 通过 | 0 / 1 |
| 真实 Wait 中 | 通过 | 通过 | 通过 | 0 / 1 |
| click 已应用、原生回包滞留 | 通过 | 通过 | 通过 | 1 / 1 |

十五行均保持原 tab 存活，旧 endpoint 不可达，新 Host 拒绝旧 proof/connection；
重新配对后清理自己的记录，再显式 Share。SW 重启使用 Chrome 原生 stopped→running
事件和新 worker 上下文证据；不是调用 reset 或替换 map。

Node 覆盖 live/unavailable 保留 owner、错误 witness/旧配对/迟到回包拒绝、
capture 失败零 reservation、旧 journal 迁移、删除失败，以及旧退休查询不能
释放同 tab 新 owner。Core HTTP 还验证进程查询绝不释放正在运行的 Host pending。
PID 复用为创建身份不匹配的模拟；尚无强迫操作系统复用 PID 的实机证据。

## 局部指纹与收尾

源码 335 文件 SHA256：
`23DB24F9C1AE61C5A4F9DD86522172B02E475AE875AE716E21BA8E553A10FD55`。
cu_probe SHA256：
`EE2F99FCC12140EA8310E01B60EC0E5328F8A88A625676EE3E3B286E1ABFCCBF`。
范围和算法沿用前置检查点，记录在 `existing-app-restart-fingerprint.log`；
验证结束复核源码 hash 未变。这是局部追溯，不是 C6 发布冻结。

最终进程检查没有发现 cu_probe 或本批私有 Node/Chrome 残留。本批自有 profile
已由探针收尾。另发现一个 9 月 20 日创建且无 owner.json 的旧测试目录
`grok-cu-pairing-owned-f605cbe7-3b68-4f76-9191-72bbb5f729b0`，未读取 profile 内容，
未删除或改动；它不能归入本批已确认的资源所有权。

没有操作日常浏览器、真实账号、Cookie、代理或 VPN，没有新增子代理。

## 下一项与证据边界

本批验证与指纹已完成；不要重做 R3、本文已通过的矩阵或重新设计队列。
继续未关闭项：

1. 另一真实 Host 仍存活的跨实例查询、Host retirement 回复丢失与真实 storage
   删除失败；目前相应 Core/Node 合同不等于双 Host/真实扩展故障证据。
2. 完整浏览器退出造成 session journal 丢失、Host claimed 仍存在；需要独立的
   浏览器退出/清理协议，不能拿本批 Host 进程证明代替文档销毁证明。
3. renderer crash、扩展 reload/update 后旧 isolated world、旧版本没有 guardian
   的记录，以及 Chrome 已接受但尚未执行的旧注入。
4. R1–R4 关闭后接 ExistingTab adapter.act 和真正 session MCP actions；之后仍有
   原生工具栏与成功截图、parity、App/ACP/UI、安装/升级/回滚、Chrome/Edge、
   三 OS（包括原生 Wayland）、真实模型、冻结候选至少 12h 主动长稳及 Windows
   IPC 10053 根因。

本批 crash 证据来自独立的真实产品 Host 进程，不是完整 Tauri UI 退出验收。
授权仍由私有 fixture 提供，不是用户在 App 点击授权或真实模型调用动作。

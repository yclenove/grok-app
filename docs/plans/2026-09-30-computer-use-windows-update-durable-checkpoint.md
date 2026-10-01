# Computer Use：Windows 更新候选跨进程恢复

日期：2026-09-30。分支：`feat/computer-use-implementation`。
HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。保留既有工作区；没有 reset、提交、推送、真实安装或替换用户运行中的 App。

**完整目标继续 active；这是实际代码与原生验证进展，不是最终版或发布验收。**
上一阶段归类为 progress：已有 process-local 原候选恢复。本阶段重新核对当前源码，补上 App 崩溃/重启后 Prepared 候选丢失，以及安装器启动前缺少持久化防重放记录两个缺口。

## 已实现的生产路径

- 每个 App 的 Windows updater owner 使用可信 AppLocalData 下的 `update-recovery-v1`。不再让生产路径依赖进程全局 catalog；原 Update 的 owner 回指被移除，避免 Arc 环。
- 元数据使用 **Windows 用户作用域 DPAPI**，绑定 store 路径；保留原候选随机 UUID、签名包摘要、安装文件摘要、源/目标版本、安装参数及原重启参数。回调与 App 句柄只从当前可信 App 重建，不从磁盘反序列化。
- 独占 OS owner handle 与祖先目录 handle 固定存储位置；拒绝 reparse/junction、非法 UUID 和多硬链接文件。持有安装文件只读 lease，拒绝写入/删除替换；运行受控 PE 的测试确认该 lease 不阻止正常执行。
- 元数据先写入、flush，再用 Windows write-through 原子替换并读回。永久初始化标记防止“以前的 journal 丢失”被误当成没有候选；真正空目录的中断初始化可以恢复。
- 重启后 Prepared 只恢复 **同一个 nonce、原包和原文件**。再次核对当前 App 路径/二进制摘要、源版本、公钥与安装配置，重新验原包签名，并将保留的安装文件与已签名包的唯一根目录安装器逐字节比较。没有重新下载、重新解压或替换候选。
- resume 重新执行当前可信 App 的清理门禁；原生更新发现也检查 owner，不能绕过 UI 另开候选。pending/resume 的磁盘工作以及 check 初始化放在 blocking worker。
- **LaunchIntent 在调用 OS 启动之前持久化。** 只有明确 OS 拒绝允许退回 Prepared；panic、写入结果不明或已接受的启动不能授权重放。接受启动时，在有进程句柄的情况下记录 PID 与创建时间。
- 两个真实进程死亡测试启动独占的测试子进程，先验证其 OS 锁仍存活，再只终止该精确 Child handle：Prepared 可恢复同一候选，LaunchIntent 不会再次启动。没有终止用户 App 或任何按名字猜测的进程。

## 冻结验证

证据目录：`tools/computer-use-probe/.run/windows-update-durable-recovery-20260930/`。
本地 `receipt.json` 封存测试、依赖图、74 项选定源、本文与两个当前索引的摘要；这是选定变更冻结，**不是整个最终产品候选冻结**。

| 检查 | 结果 | 证明边界 |
|---|---:|---|
| 产品默认 feature 的 updater 库 | **36 passed，2 ignored** | 两个 ignored 是父测试实际启动、验证并回收的专用子进程入口 |
| 独立 no-zip consumer 的 updater 库 | **35 passed，2 ignored** | Cargo artifact features 精确为 `rustls-tls`；没有 zip；未改 App workspace 或 root lock 来制造配置 |
| 完整 Windows App lib 测试程序 | 编译通过 | **1733** 个可用测试，不是全部执行 |
| App 选定原生回归 | **38/38** | updater 7、shutdown 3、browser owner 7、MCP owner 21 |
| 冻结前端回归 | **133/133，5 文件** | Hook 34、协议 2、helper 46、About/Sidebar DOM 9、locale 42 |
| 默认及 no-zip Clippy | 通过，无 warning | 默认 App + updater lib/tests；no-zip updater lib/tests 另检查日志，防止 dependency lint cap 隐藏 warning |
| TS、8 文件 ESLint、Rust 格式、正常 diff check | 5 项通过 | 不代替打包、签名、真实安装或原生 UI 验收 |
| 选定源/直接依赖 | **74 项零漂移** | 最后编译、默认/no-zip 测试、静态检查使用同一选定源冻结 |
| 来源、锁与 ACL | 通过 | 19 个 pinned 原件、JS API bundle 未变、原生恢复权限仅 local main/session；no-zip 包版本/校验和来自既有 App 锁 |

本轮仅扩大已锁定 `windows-sys` 的 Windows 功能选择，没有新增依赖版本。App `Cargo.lock` 与本轮开始保持相同摘要；从前置 vendor 基线逐块比较，仍只有既有 updater 定位及 sha2/uuid 依赖块差异。

覆盖的反例包括：修改签名包/安装文件/源版本/公钥/参数、DPAPI 损坏/截断/跨路径、旧 journal 丢失、junction/hardlink/目录替换、发布前后故障、启动 panic、明确 Shell 拒绝、并发 owner 与进程死亡。OS 拒绝不退出；未知和已接受启动不重复。

早期失败日志保留且不冒充最终通过：DPAPI provider header 的某字节不属于可依赖的认证失败测试，已改为破坏认证数据；junction 夹具的 cmd quoting 已修正；no-zip 的 Cursor 条件 import 已修正，最后严格检查无 warning。v1 冻结/日志与最终冻结明确分开。

测试使用临时公钥/签名和 **MZ 前缀惰性文本（不是可运行安装器）**，其清理必然拒绝。唯一实际执行的 PE 是临时目录中的测试程序副本；真实进程死亡夹具不启动安装器。没有读取/输出产品私钥，也没有把测试密钥落盘。

## 发布阻断项：不能误称安装恢复完成

1. **Accepted / LaunchIntent 的结果对账仍未完成。** 重启后继续显示 blocked；PID/创建时间不是安装完成证明，也不是回滚回执。目前没有已验证安装完成/回滚的 receipt、journal 退休或下一轮更新释放路径。真实更新成功后不能永久卡在此处，必须继续实现，不能靠删除 journal 或自动重放绕过。
2. **真实安装态验收仍缺。** 签名 NSIS/MSI、UAC 取消、安装完成/重启/修复/回滚/重装、安装版真实 WebView 重载均未执行。DOM、库测试与受控 PE 不能替代这些门禁。
3. **中断清理/产品恢复仍缺。** interrupted prepare/publication 与旧 updater 临时文件需要基于身份的安全回收；部分服务先停止后清理失败的恢复、普通 Quit、native input/IME/clipboard 生命周期仍需完成。
4. **耐久与安全边界有限。** write-through 与读回不是所有文件系统/断电行为的证明；DPAPI 不防已经取得同用户/admin 任意代码执行能力或恶意回滚。损坏 journal 不会自动变为空白成功，也没有声称已实现所有修复路径。
5. **原始完整范围保持不变。** Windows、macOS arm64/x64、Linux X11/**native GNOME Wayland**；Desktop/managed browser/existing tabs/App WebView；App/ACP/MCP；全部输入、取消和恢复；签名安装/更新/回滚；原生窄窗/缩放/权限 UI；真实 Grok E4；**同一个最终冻结候选 12 小时 active soak**。

没有操作用户剪贴板、权限提示、真实安装器、用户 App 或远端 CI；没有新启动长时 soak，也没有改变 goal 状态。下一步必须继续推进接受/未知启动的真实结果对账与 journal 退休，并使用实际签名安装态完成相应门禁，不将本次跨进程 Prepared 恢复缩减为完整目标。

前一封存证据保持原样：`windows-update-recovery-20260927/receipt.json` SHA-256 为 `45012c204a45cf024a7d5cd87f3319f7314084584d93a4962436e141e3901d09`；其检查点不回写。

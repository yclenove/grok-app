# Computer Use：Windows 安装器精确进程退出证据

日期：2026-09-30。分支：`feat/computer-use-implementation`。
HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。保留既有未提交改动；没有 reset、提交、推送、替换用户 App 或运行真实安装器。

**完整 goal 继续 active；这是结果对账的必要增量，不是已完成安装/回滚对账或最终版。**
前一阶段为 progress：跨进程 Prepared 恢复和持久化启动意图已封存。本阶段继续处理其中的真实缺口：仅有 Accepted PID/创建时间，没有查询原进程、保存准确退出证据或区分 PID 消失与退出的路径。

## 生产行为

- 原生恢复轮询对 **原 PID + 原创建时间** 打开只含 query/synchronize 权限的句柄；没有 terminate、注入或 VM 写权限。匹配后一直持有同一 OS handle，避免 attach 后 PID 复用。
- 零等待 OS 查询区分仍在运行、确已发出退出信号、PID 不存在、创建时间不匹配以及无权限/观察错误。PID 消失、查询失败和观察超时均不是安装完成或重试许可；不会重启或终止任何被观察进程。
- 使用句柄 signal 判断进程终止，不将 `GetExitCodeProcess` 的数值 259 一概当成 STILL_ACTIVE：**真实退出码 259 可以是终态**。
- 已匹配的退出身份、OS 退出时间和退出码通过现有原子 DPAPI journal 保存。必须对应原候选 nonce、Accepted 阶段和原进程身份；拒绝冲突证据，重复轮询不再重写文件。旧 schema-1 无该可选字段的记录仍可读取。
- 原生 `pending_install` / discovery 返回更准确的进程观察信息；写入失败继续隔离。刷新后的 App 可从持久化退出证据恢复，不依赖仍存在的 PID。运行中的原始清理/启动事务不会被后台观察覆盖。
- **状态仍为 blocked，阶段仍为 Accepted。** 退出 0、259 或任何失败码都不会触发 resume、新候选、清理、重启、journal 清除或“已安装”。安装器可能委托其他进程，单个进程退出不能证明文件安装或回滚完成。
- 前端新增退出 0 的实际 Hook 轮询回归，确认 About/Sidebar 所用状态不变为 installed/restartPending，也不调用任何安装、恢复、发现、下载或 relaunch。未新增用户文案或权限，沿用既有 15 locale 状态提示与诊断详情。

## 当前冻结验证

证据目录：`tools/computer-use-probe/.run/windows-update-outcome-observation-20260930/`。

| 检查 | 结果 | 范围 |
|---|---:|---|
| 默认 updater 原生库 | **41 passed，3 ignored** | 三个 ignored 是真实父测试调用的专用受控子进程入口 |
| 独立 no-zip updater 原生库 | **40 passed，3 ignored** | artifact features 仅 `rustls-tls`；同样运行所有适用的进程/持久化测试 |
| 真正受控 Windows 进程 | 通过 | 实际退出码 **0 / 259 / 1603**、attach 前创建身份不匹配、attach 后保留句柄、原 Child handle 丢弃后准确读取退出 |
| 生产 owner + journal 恢复 | 通过 | 同 nonce；持久化退出后重新构造 owner 不查 PID；仍 blocked；写入被文件 lease 阻止时不释放/重放 |
| 前端 | **135/135，5 文件** | Hook 35、协议 3、helper 46、About/Sidebar DOM 9、locale 42 |
| Windows App lib | 编译通过；选定 **38/38** | 可用 1733 个测试，不是全部执行；updater 7 / shutdown 3 / browser owner 7 / MCP owner 21 |
| 默认与 no-zip Clippy | 无 warning 通过 | App + updater lib/tests；no-zip 单独检查 dependency lint cap 下的日志 |
| TS / 8 文件 ESLint / Rust 格式 / 正常 diff check | 5 项通过 | 不冒充已打包、签名或全 App 原生 UI 验收 |
| 选定源冻结与来源 | **75 项零漂移** | 19 个 pinned 原件、生成后 ACL、unchanged JS API bundle；App 锁仍为同一 673 包锁，无新依赖版本 |

no-zip consumer 的 420 个包中，唯一不在原 App lock 中的是本地验证 consumer 自身；419 个依赖的版本、source 和 checksum 均与已锁定项一致。所有 Cargo 命令离线且 locked，未通过变更 App workspace/lock 制造该 feature 图。

真实进程测试只执行私有临时目录中的测试程序副本。1603 是该夹具实际返回的 Windows 进程退出码，**不是一次真实 MSI 安装失败验收**；创建时间不匹配是对真实存活 PID 提供错误创建身份，未声称强迫 OS 实际发生 PID 复用。父进程只在失败清理时终止自己的精确 Child handle，不按名称寻找或杀用户进程。

源文件在编译、两套库测试、App 选定回归、前端、Clippy 与静态检查期间保持一致；`receipt.json` 封存 75 项源、日志、依赖审计及文档摘要，再用 PowerShell 独立校验。前一 receipt 与前一检查点原样保留，不回写旧封存日志。

## 尚不能交付最终版

1. **没有连续独立进程见证。** 当前 witness 依附重新打开的 App；若安装器在它 attach 前已经消失，结果仍不可证明。没有将 PID 消失、时间经过或版本字符串提升为成功，也没有删除记录解除阻断。
2. **没有真实安装产物/回滚回执。** Accepted/LaunchIntent 的完整对账、installer delegation、已签名候选对应的安装产物验证、完成/回滚 receipt、journal 退休及下一轮更新释放仍是发布阻断项。本增量不会绕过它们。
3. **签名安装态和服务生命周期仍待验收/完成。** NSIS/MSI/UAC/完成重启/修复/回滚/重装与安装版 WebView、部分服务清理后的恢复、journal orphan GC、普通 Quit、native input/IME/clipboard 生命周期尚未由本阶段解决。
4. **完整原始范围不变。** Windows、macOS arm64/x64、Linux X11 和 native GNOME Wayland；Desktop/managed browser/existing tabs/App WebView；App/ACP/MCP；全部输入、取消和恢复；签名安装/更新/回滚；原生窄窗/缩放/权限 UI；真实 Grok E4；**同一最终冻结候选的 12 小时 active soak**。

没有操作用户剪贴板、权限弹窗、真实安装器或远端 CI，没有启动新的 soak，没有修改目标为 complete/blocked/paused。下一步继续完善独立见证/安装结果与签名产物的权威对账，再接通经证明的 journal 退休及真实安装验收；不将“安全阻断”当成最终可用更新功能。

前一阶段：`docs/plans/2026-09-30-computer-use-windows-update-durable-checkpoint.md`；其 receipt SHA-256：`22b342c3dc5155a742af7ba8184778b23660784ed3d428e8897721063e95995d`。

# Computer Use：Windows 更新事务跨界面重载恢复

日期：2026-09-27。分支：`feat/computer-use-implementation`。
HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。保留既有工作区改动；没有 reset、提交、推送、真实安装或替换用户运行中的 App。

**完整目标继续 active；这是实际代码进展，不是最终版或发布验收。**
上一目标阶段归类为 progress：安装前清理门禁已有冻结测试和检查点。本阶段从当前文件重新核对，修复其中明确未实现的界面重载恢复。

## 修复的真实缺口

此前原生只保留 prepared transaction 的 Arc；原 Update/config 和界面资源句柄丢失后，新的 Update 被拒绝，却没有入口恢复原候选。原生全局锁还跨越清理过程，不能提供即时只读状态。

- 新增 process-owned recovery catalog，保留原 Update、配置、清理 guard、摘要和受保护的原安装文件，以随机 candidate nonce 标识；不依赖旧 WebView resource id。
- 新增 `pending_install` 与 `resume_install(candidateId)` IPC。前者只读短锁快照；后者在 owned blocking task 上恢复原事务，**不重新检查 manifest、不重新下载、不重新解压、不切换版本、不重放已经接受的安装器启动**。
- 状态明确区分 `running`、`retryable`、`blocked`、`failed`。`failed` 仅在未准备成功且未进入清理/启动时生成，是允许重新检查的明确终态；缺失状态、传输失败、未知结果均不等于该终态。
- 原生运行期间快照不被长清理阻塞；并发恢复被拒绝；调用方消失不取消 blocking owner。未知启动、owner 异常退出或不可验证的状态禁止重放。
- 界面在新版本发现和 developer simulation 之前查询原生 owner；实际 React 卸载/重挂载后可找回原候选。只读轮询确认原事务状态，旧轮询不能覆盖新的恢复尝试。
- 查询失败和插件可用性不明时，不旁路到 GitHub 或新候选。`resume_install` 的 fulfilled Promise 不被当作安装完成，也不额外调用 JS install、prepare/relaunch。
- About 与 Sidebar 区分可重试拒绝和未知结果隔离；后者禁止安装动作，说明文案覆盖 **15 locales**。
- 新 ACL 仅授予本地 `main` / `session-*`；没有开放给 pet、theme-editor 或远程 WebView。原 JS updater API bundle 不变。

## 实际验证

证据目录：`tools/computer-use-probe/.run/windows-update-recovery-20260927/`。

| 检查 | 结果 | 边界 |
|---|---:|---|
| 冻结前端回归 | **133/133** | Hook 34、协议 2、既有 helper 46、About/Sidebar DOM 9、locale 42 |
| updater 原生库 | **20 passed，1 ignored** | ignored 为由受控父测试实际执行并验证的 owned child 专用入口 |
| 生产 staging / 资源关闭恢复 | 通过 | 真正 Tauri ResourceTable 关闭原资源后，同一文件仍拒绝写入/删除；原 guard 第二次执行，新候选被拒绝 |
| 真实 Windows owned child | 通过 | 临时目录中的测试程序副本，精确 HANDLE/PID/回执/退出码；不是安装器 |
| 完整 Windows App lib 测试程序 | 编译通过 | **1733** 个可用测试；未执行全部 1733 |
| App 选定原生回归 | **38/38** | updater 7、shutdown 3、browser owner 7、MCP owner 21 |
| 严格 Clippy | 通过 | updater 和 App 的 lib/tests，产品默认 feature |
| 全量 TS、8 文件 ESLint、Rust 格式与正常 diff check | 通过 | 不代表完整打包/签名/安装或原生 UI 矩阵 |
| 选定源/直接依赖冻结 | **72 项零漂移** | 不冒充整个最终候选冻结 |
| 来源与权限审计 | 通过 | 19 个 pinned 原件核对，API bundle 未变；生成后的 ACL 只开放本地 main/session |

生产 staging 夹具是用临时测试密钥签名的 **MZ 前缀惰性文本，不是可运行 PE**；清理始终拒绝，测试绝不允许启动它。私钥没有落盘、输出或读取产品密钥。仅新的 fixture/generator 新增，上一阶段公开签名向量和封存证据保持不变。

`Cargo.lock` 与上阶段 vendor 前基线逐块核对：只改变 updater 定位及依赖块（已有的 sha2，加上本轮已在锁内的 uuid），其余全文未变；无新包版本解析或网络取依赖。

前端验证包括真正 React remount、运行中只读轮询、并发点击、迟到旧回包、短暂查询失败后原身份恢复、准备失败终态、未知启动隔离、模拟模式不能遮盖原事务，以及缺失/被替换 owner 不误报成功。原生验证同时覆盖元数据短锁和生产资源/文件所有权，未只用 mock 声称完成原生恢复。

## 尚未完成，不能据此发布

1. **跨进程持久化与对账**：当前 catalog 是 process-local；App 崩溃/重启后的 journal、未知启动结果的真实 OS/安装器对账、部分服务已停止后的产品恢复仍未完成。禁止重复启动不等于已完成未知结果恢复。
2. **真实安装态与真实 WebView**：签名 NSIS/MSI 更新、UAC 取消、安装完成、重启、修复/回滚/重装，以及安装版真实 WebView reload 验收仍缺。React DOM + ResourceTable 测试不替代这些验收。
3. **其他配置/生命周期**：no-zip 配置仍未验证；上游提取目录崩溃回收、普通 Quit、native input/IME/clipboard 退出交接仍是独立必需项。
4. **完整原始范围不变**：Windows、macOS arm64/x64、Linux X11/native GNOME Wayland；Desktop/managed browser/existing tabs/App WebView；App/ACP/MCP；全部输入、取消和恢复；签名安装/更新/回滚；原生窄窗/缩放/权限 UI；真实 Grok E4；**同一最终冻结候选 12 小时 active soak**。

没有操作用户剪贴板、点击权限提示、运行真实安装器、终止用户 App、执行远端 CI 或启动新的长时 soak。下一步继续补跨进程原候选恢复与未知启动对账，并以真实安装态验收收敛，而不是将本次局部通过重新定义为最终完成。

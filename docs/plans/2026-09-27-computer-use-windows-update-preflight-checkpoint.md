# Computer Use：Windows 更新安装前清理门禁

日期：2026-09-27。分支：`feat/computer-use-implementation`；HEAD：
`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。工作区保留其他既有改动，没有 reset、提交、推送或替换用户安装版。

**完整目标继续 active；这不是最终版或发布验收。**

## 本轮接续与真实变更

前一阶段确认 pinned updater 2.10.1 的 Windows `install()` 会直接退出，前端后置 `prepare_for_app_update` 无法覆盖。本轮不采用禁用更新、仅手动下载或提前停止服务的替代方案，而是保留签名更新通路并修复原生安装边界。

- 将 crates.io **2.10.1** 最小必要内容 vendoring 到 `src-tauri/vendor/tauri-plugin-updater/`，通过 `[patch.crates-io]` 使用，不修改 Cargo registry 缓存。保留许可、ACL、API bundle；`UPSTREAM.json` 记录 19 个原始文件的 SHA-256 和 registry checksum。
- 仅 5 个复制来的上游文件改变：`Cargo.toml`、`src/{commands,error,lib,updater}.rs`；新增安装事务、Windows 实现、公开签名测试夹具与补丁维护说明。
- 新 Windows 顺序是：**验签 → 准备并核对文件 → 保持拒绝写入/删除共享的读句柄 → 可失败 App 清理 → 检查真实 OS 启动结果 → Tauri 退出清理 → 退出**。MSI 解析系统目录下的 `msiexec.exe`，不从 PATH 查找。
- App 在 webview 加载前登记 `WindowsInstallGuard`，共享已有的 `UPDATE_SHUTDOWN`。JavaScript 安装与 download-and-install 都交给原生 blocking owner；调用者取消/卸载不取消原生 owner。
- 准备失败不停止服务；清理或已知启动失败不退出。原生保留同一份准备结果，重试先重新清理，不换更新包、不重复解压。已确认启动后不再次启动；启动结果未知保持隔离，不自动重放或退出。
- 前端区分“下载完成但安装未确认”和“已安装但清理/重启未完成”。前者复用原 Update 安装事务；后者仍只重试清理/重启。About/Sidebar 不让普通检查或偏好重置覆盖待恢复事务。两条新增文案已写入并回读 **15 locales**。

## 实际验证

证据目录：`tools/computer-use-probe/.run/windows-update-preflight-20260927/`。

| 检查 | 结果 | 证据边界 |
|---|---:|---|
| 前端冻结回归 | **113/113** | Hook 17、纯逻辑 46、About/Sidebar DOM 8、locale 42；不是原生 WebView 全矩阵 |
| updater 库 | **14 passed，1 ignored** | ignored 是仅供受控父测试启动的子进程入口，不单独冒充通过 |
| 真实 Windows 启动夹具 | 通过 | 私有临时目录中的测试程序副本；精确 HANDLE/PID、标记和退出码核验；未运行真实安装器 |
| 签名/格式准入 | 通过 | 临时生成测试公钥/签名，私钥从不落盘；篡改字节与签名拒绝，合法签名但非法格式也在清理前拒绝 |
| 文件保护 | 通过 | 真实 Windows 文件写入和删除在保留读句柄时失败，释放后删除成功 |
| 完整 Windows App lib 测试程序 | 编译通过 | 含 1733 个可用测试；**没有执行全套 1733** |
| App 选定原生回归 | **38/38** | updater 7、shutdown 3、browser owner 7、MCP 21，冻结后复跑 |
| 严格 Clippy | 通过 | updater 与 App 的 lib/tests，默认产品 feature |
| 全量 TS、7 文件 ESLint、格式和正常换行规则的 diff check | 通过 | 不包含全 App UI build、安装包签名或发行构建验收 |
| 选定源与依赖冻结 | **63 项零漂移** | 仅本阶段和直接依赖，不是全产品最终候选冻结 |

`Cargo.lock.before-vendor` 的逐块比对确认：只有 updater 从 registry 定位改为 local、增加已经锁定的 `sha2` 依赖；其他 lock 内容完全不变。`api-iife.js` 与同版本上游原件一致。

### 保留的失败与纠正

- 初次 vendor 构建缺少 build.rs 必需的 `api-iife.js`；从同一 pinned 原包补齐后完整编译通过。
- 上轮 Python stdin 的多语言编码错误没有写入 locale；本轮改为 UTF-8 脚本文件，写入后回读并通过全 locale 门禁。
- provenance 脚本最初依赖本机旧 Python 不具备的 `tomllib`；改为精确锁文件块/其余全文比较，不安装额外解析器、不放宽比较。
- no-zip 配置尝试未通过环境准入：dependency-only package 不能从父 workspace 选择 feature；临时加入 workspace 后又要求 offline 缓存缺失的可选 `hyper-tls`。已撤回该成员变更，lock 无漂移，**no-zip 明确未验证**。
- 一次人为 `core.autocrlf=false` 的全树 diff 检查将既有 CRLF 当成行尾空白；保留失败日志，移除该命令级覆盖后按仓库原换行规则通过。未批量改写无关 CSS/源码。

## 未完成项与下一步

1. **安装态验收仍缺**：实际签名 NSIS/MSI、UAC 取消、启动接受后的安装完成、修复/回滚/重装、用户安装版 App 退出与重启。库测试与 owned child 不等价于这些行为。
2. **恢复仍需实现**：process-local owner 不是持久化 journal；UI 重挂载/刷新丢失原 Update 后的原候选恢复、启动结果未知的对账、部分服务已停止后的可恢复产品状态。当前不同候选被拒绝，不声称已完成恢复。
3. 原生准备目录仍沿用上游提取生命周期，尚无完整崩溃后临时目录回收机制。
4. 普通 Quit、native input/IME/clipboard 退出交接和跨 App 生命周期仍未因本轮更新门禁自动解决。
5. 完整原目标不变：Windows、macOS arm64/x64、Linux X11/native GNOME Wayland；Desktop/managed browser/existing tabs/App WebView；App/ACP/MCP；全部输入/取消/恢复；签名安装更新回滚；原生窄窗/缩放/权限 UI；真实 Grok E4；**同一最终冻结候选 12 小时 active soak**。

本轮没有点击权限提示、改系统剪贴板、运行真实安装器、终止用户 App、执行远端 CI 或启动新的长时 soak。所有本轮测试结束后，完整 goal 保持 active。

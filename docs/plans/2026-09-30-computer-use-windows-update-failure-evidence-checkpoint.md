# Computer Use：Windows 安装失败与取消的原生证据

日期：2026-09-30（本机 +08:00）；工作区 `H:\aicoding\grok-app-computer-use`。
分支 `feat/computer-use-implementation`；HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。

**完整 goal 仍为 active / partial — not releasable。** 上轮与本轮均为 progress。本轮补上真实失败/取消回调与恢复查询，不把诊断证据当作回滚完成，也不把 Windows updater 当作完整目标。

## 已落地的生产链路

- 发布者签名 inventory 的 NSIS completion contract 可显式声明 `failure_protocol: grok-nsis-install-failed-v1`。旧 contract 不推断失败；显式 null、非字符串、未知协议拒绝，不静默降级。Node producer 新增明确 opt-in 参数，release workflow 已接线；本轮未执行发布或生产签名。
- checked launch plan 在清理前绑定原 nonce 的独立失败回执路径；保留原成功回执参数，阻止自定义安装参数覆盖保留前缀，原生 UTF-16 路径引用与命令长度检查继续生效。预存失败回执不允许派发。
- 生产 NSIS `.onInstFailed` 写入 `failed`；MUI 的 `MUI_CUSTOMFUNCTION_ABORT` 写入 `cancelled`。不抢占 MUI 自己的 `.onUserAbort`，保留任何已配置取消确认的顺序。两个回调复用严格小写 UUID v4 检查，只以 CREATE_NEW 写原生进程绑定记录，不修改注册信息、不执行 App。
- Rust 同时校验发布者签名声明、原候选 nonce/version/App/executable/bundle/install path、PID/创建 FILETIME、持久化的确切非零退出。成功/失败回执冲突、字段变化、无退出、零退出、截断、超长、硬链接和非普通文件均不能通过。
- 原生恢复状态只把 **Blocked** 的诊断细化为 `installer_failed` / `installer_cancelled`。重开仍绑定原候选，不清空 journal、不授权 resume/replay、不替换候选、不创建完成归档。回执缺失时恢复到一般退出观察，不从旧标签推断失败。前端协议继续保持这些阶段为 Blocked，不能伪装成 Retryable / Failed / Completed。

回调文件**没有独立数字签名**，不是针对同用户/管理员恶意代码的隔离边界；信任来自签名 payload/contract、原进程身份与既有本地用户 journal 边界。失败后的文件和注册信息可能只更新了一部分，必须另行验证修复/回滚。

## 本轮原生与回归验证

证据根：`tools/computer-use-probe/.run/windows-update-failure-evidence-20260930/`。

| 检查 | 本轮结果 | 证据范围 |
| --- | --- | --- |
| 默认 updater library | **96 passed / 0 failed / 9 ignored**（后续完整复跑） | default,rustls-tls,zip；首次完整运行存在下述未定根因异常，未隐去 |
| 独立 no-zip updater | **95 passed / 0 failed / 9 ignored** | 独立 consumer、features 仅 rustls-tls，锁文件固定 |
| 显式原生 parser | **两套配置各 2 passed** | 一个成功回调测试、一个同时消费失败与取消真实回调的测试；不是忽略不跑 |
| 无注册写入的 NSIS 失败夹具 | **10/10 cases** | 真正 Section Abort → `.onInstFailed`，真正 MUI 自有窗口 Cancel → abort callback；退出码分别 2 / 1 |
| 原成功 callback 兼容回归 | **21/21 私有 hive + 5/5 共享 writer** | 本轮重新编译/执行；64 位私有注册视图，宿主唯一键保持不存在 |
| Node 汇总 | **59 passed / 0 skipped** | 包括 producer/metadata、静态接线和原生子用例；不是 59 项实际安装验收 |
| App 直接关联 | **38 passed** | updater 7 / shutdown 3 / browser process 7 / MCP session 21，不是全部 App |
| 前端恢复链 | **4 files / 99 passed** | useUpdater、updateRecovery、AboutUpdateRow、appUpdateHonesty |
| 静态检查 | **两套严格 Clippy、TypeScript、workspace/vendor fmt、diff、YAML/PowerShell 解析通过** | PowerShell workflow 仅解析；远程 CI 未执行 |
| 权限/依赖/选定源 | **115 项核对** | recovery 仍只属于 local main/session-*，未加默认权限；原 IIFE 与 App Cargo.lock 保持原哈希 |

每套 9 个 ignored 中，两个原生 parser 测试本轮显式运行；其余 7 个是由父测试调用的受控子进程入口，不另计顶层通过。

失败夹具只向自己的窗口发送 Cancel，父进程先持有精确 Process.Handle/创建时间，再释放私有 permit；超时只终止自己的持有进程。负例覆盖非更新模式、大写 nonce、错误 nonce 版本/variant、缺失路径、已存回执、硬链接、目录。没有启动真实 Grok 安装器、安装态 App 或卸载器，没有改动用户真实安装注册信息。

## 必须保留的未解决观察

首次默认 library 全量运行 **95 passed / 1 failed / 9 ignored**：既有 `dead_authorized_worker_is_unknown_not_refusal_or_replay_permission` 在 `worker.authorize` 返回 Windows `Access denied (5)`。原始日志为 `updater-first-regression-denied.log`。随后原样单项五次和完整默认 suite 通过，未更改该分支生产代码、未增加重试或放宽断言。

**根因仍未证明，不能把后续通过当作修复。** 下轮应优先增加可定位的 journal publication 诊断并复现授权写入竞态，保持不确定状态阻止再次派发。该观察也是不能宣称最终版稳定的具体依据。

开发中的夹具失败也保留：NSIS 重复 `.onGUIInit`、不支持的 PostMessage 指令，以及过早异步关闭未退出；最终夹具使用正常 MUI、页面一次性 timer 与同步自有窗口 Cancel，实测收到回调并退出。no-zip consumer 初次复制遗漏 target 的构建错误已保留，补齐原有测试 target 后才重新验证。没有把这些夹具修正描述为产品缺陷修复。

## 剩余完整目标

仍须完成并逐项验证 Windows x64、macOS arm64/x64、Linux X11 与原生 GNOME Wayland；Desktop / managed browser / 已有 Chrome/Edge tabs / App WebView；App / ACP / MCP；完整 actions/input、中文 IME、clipboard、取消及恢复；签名 clean install/update/repair/rollback/uninstall；原生窄窗口/DPI/权限 UI；真实 Grok E4；**同一个最终冻结候选的 12h active soak**。

本轮回执、源码哈希及测试二进制属于一个有边界的实现/验证阶段，不是全产品最终冻结候选。签名安装态失败恢复/实际回滚、多平台 E4/E5 和以上未解决授权异常均仍在完整目标内。没有 commit、push、PR、tag、发布或将 goal 标记完成。

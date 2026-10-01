# Computer Use：Windows 更新签名安装文件清单

日期：2026-09-30（本机 +08:00）；工作区 `H:\aicoding\grok-app-computer-use`。
分支 `feat/computer-use-implementation`；HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。

**完整 goal 仍为 active / partial — not releasable。** 前一轮与本轮均为 progress。
已新增发行方签名的文件预期，并接通发布脚本、安装前验证、恢复/独立派发复验和候选版本 App 的实际文件观察。文件匹配只是必要证据，不能代替安装完成、回滚、journal 安全退休或下轮更新。

## 生产增量

1. Windows 元数据可携带 `windows_install_inventory = { document, signature }`。独立 minisign 签名覆盖 exact UTF-8 document，不重新序列化。受信公钥来自编译后的 updater 配置，不从 envelope/journal/IPC 获取。
2. 严格 schema/domain 绑定 App 名称、候选版本、target、architecture、NSIS/MSI kind、完整 payload SHA-256 和主 EXE 文件名；清单限制 256 KiB/4096 文件、单文件 16 GiB/合计 64 GiB。相对路径拒绝 traversal、ADS、设备名、大小写别名、短文件名和文件作为目录。
3. 验证先于 `journal.prepare` 与 App cleanup。非法/显式 null 清单不会被降级忽略。静态多平台元数据按已选 URL 和 payload signature 精确选取；同候选别名的清单冲突拒绝。旧发行候选缺清单仍可读取，但不产生已安装文件匹配结论。
4. 原签名 envelope 持久化到 DPAPI record，纳入独立 dispatcher 的不可变候选指纹。保留候选恢复和 worker 派发都复用编译 key 再验签；不改写旧无清单 record 的序列化形状。
5. 已有准确进程退出证据后，仅当前 App 版本等于候选、App/target 一致且当前 EXE 位于原绑定路径，才观察已安装文件。逐级 directory lease 和所有 read-only file handle 保持到整体校验结束；拒绝 junction/reparse、hardlink、已有 writer、重复卷/文件身份，核对大小和流式 SHA-256，绝不创建缺失安装目录。
6. 即使签名与文件全部匹配，production pending 仍为 `installer_exited` / Blocked；只增加诊断证据，不重派发、不退休 nonce、不释放下一候选、不宣称回滚/安装成功。
7. 发布 producer 从实际 NSIS 解包目录生成清单；显式 NSIS 模式仅排除 root `$PLUGINSDIR` 与动态 uninstaller，应用子目录同名文件仍包含。payload/output 禁止位于 image 内；使用 create-new 防止覆盖已有结果。producer 不执行安装器、不读取签名私钥。
8. release workflow 接入实际 bundle → 7z 只解包 → producer → 现有 Tauri signer → versioned sidecar 上传；assembly 将 exact signed bytes 嵌入对应平台。严格检查平台别名、版本、installer kind、payload hash，同时保留 `.nsis.zip` / `.msi.zip` 元数据兼容。CI 代码存在不代表远程 CI 已跑通。

## 已执行验证

证据根：`tools/computer-use-probe/.run/windows-update-install-inventory-20260930/`。

| 项目 | 结果 | 证明范围 |
| --- | --- | --- |
| 默认 updater 全库 | **76 passed，7 ignored** | 7 个专属 native child 入口由父测试实际启动；不是安装态验收 |
| 独立 no-zip 全库 | **75 passed，7 ignored** | artifact features 精确为 `rustls-tls`；无 zip |
| App 选定原生回归 | **38/38** | updater 7、shutdown 3、browser-owner 7、MCP-owner 21；1733 项编译可用，未全部执行 |
| 前端现有回归 | **135/135，5 文件** | 更新恢复、退出码不等于成功、UI 与 15 locale；不是安装版 UI |
| producer/attachment/CLI | **13/13** | 实际临时文件/hardlink/junction、输出保护、exact document、候选匹配和 archive 元数据兼容 |
| 真实 NSIS archive | 只编译/解包成功，2 应用文件 hash 一致 | NSIS 3.11 + 7-Zip 24.09；含中文资源名，bootstrap 被排除；从未执行该安装器 |
| 严格检查 | 全部通过 | App/vendor lib/tests 和 no-zip Clippy 无 warning；TS、8 文件 ESLint、fmt、diff、YAML、PowerShell/bash syntax |
| 最终选定源冻结 | **94 项零漂移** | 仅本阶段相关源/依赖；不是整个最终发行候选冻结 |
| 依赖审计 | App lock SHA 不变 | no-zip 419 依赖均在 App lock；无新增依赖版本；19 pinned 上游原件与 ACL 继续核对 |

12 项新增 Rust 测试覆盖签名/全部候选绑定、native 文件替换/缺失/hardlink/writer/junction、持久化复验/dispatch 指纹、metadata 选择冲突、生产 staging 与 pending。Node producer 输出与测试专用签名脚本跨边界进入真实 Rust verifier；脚本使用 **PUBLIC deterministic TEST-ONLY key**，不是发行私钥，也不是把 mock 返回值当密码学通过。

`current_binary_file_match_remains_blocked_until_separate_completion_reconciliation` 实际 hash 当前 native 测试 EXE，但为隔离状态迁移使用合成 process facts；它只证明文件匹配不会放行，不是安装器退出/安装完成实测。旧真实 OS dispatch、父进程死亡和退出 evidence 回归仍在全库中实际执行。

## 红绿过程与证据边界

- 首个回归在改动前失败：不可信清单仍调用 cleanup（actual 1 / expected 0）；现已通过。
- 发布脚本追加 3 个实际失败回归后，修复 JSON hash 类型强制、image 自引用、平台/kind 边界，完整通过。
- 最终复核发现新后缀检查误拒历史 NSIS/MSI ZIP 元数据。保留 red 日志，修复并新增独立兼容测试。旧 source freeze 与先前库结果移入 `pre-archive-compatibility/`，重新冻结 94 项，再执行两套库、App 选定回归、producer 和静态检查；不是覆盖失败证据伪装一次通过。
- 本轮未重建/实跑最终 App GUI 或签名发行 EXE；此前独立派发 checkpoint 的 debug App 探针只属于此前源码阶段，不扩张为本清单实现的产品二进制验收。
- PowerShell 发行步骤只被解析；未运行签名/upload/installer。没有 release 私钥访问、远程 CI、用户 App 终止、权限/账号/剪贴板修改、commit/push/PR/tag/release。
- 清单是发行方声明的应用文件列表，不是 registry/uninstaller/服务/所有文件穷尽性证明，不防任意 same-user/admin 代码或所有文件系统/断电回滚攻击。

## 下一实现与完整目标

下一步仍须将精确 installer outcome、签名文件事实、发布者/安装布局、运行版本与安装完成/回滚回执进行权威对账，完成安全 journal 退休及下一候选释放。worker/机器故障、委托 installer、真实签名 NSIS/MSI、UAC 取消、重启、修复/回退和安装版 WebView 都未据此验收；不能把持续 Blocked 当最终可用更新。

本轮另已读取现行 `scripts/package-windows-portable.sh`：仍仅复制主 EXE 并改名 `Grok.exe`，而 `tauri.windows.conf.json` 的安装包包含 Computer Use runtime resources。便携版资源完整性及其与 `grok-app.exe` 安装版之间的更新布局/身份策略仍是明确缺口；本轮没有修复或宣称通过。

全部原始范围保留：Windows x64；macOS arm64/x64；Linux X11 与原生 GNOME Wayland；Desktop/受管浏览器/已有 tabs/App WebView；App/ACP/MCP；完整输入、中文 IME、clipboard、取消/恢复；签名安装、更新、回退；原生窄窗口/DPI/权限 UI；真实 Grok E4；**同一最终冻结候选 12 小时 active soak**。仍未满足最终完成审计，禁止标记 complete。

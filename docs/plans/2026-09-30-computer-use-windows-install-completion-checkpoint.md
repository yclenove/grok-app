# Computer Use：Windows 更新完成回执与下一轮更新

日期：2026-09-30（本机 +08:00）；工作区 `H:\aicoding\grok-app-computer-use`。
分支 `feat/computer-use-implementation`；HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。

**完整 goal 仍为 active / partial — not releasable。** 前一轮为 progress；本轮续查两个仍存活的原始测试 handle，均正常退出 0，并完成权限复核和证据封存，没有重复启动原测试。
本阶段实现 publisher 声明的 current-user NSIS 成功回执、原生对账后的 journal 退休，以及 Rust/IPC/React 的明确 completed 状态；不是签名安装态或所有更新结果的最终验收。

## 实现

1. 签名安装清单可显式声明 `grok-nsis-install-complete-v1`、bundle ID 和 `currentUser`。只允许 NSIS；拒绝 null、未知字段、其他 scope/protocol、MSI 声明和签名不匹配。历史无声明清单继续兼容，但文件匹配或退出 0 **不能**自动升级为完成。
2. 原候选 UUID 和受保护回执路径由 native launch plan 绑定，调用者不能通过 installer 参数覆盖保留的 `/GROKUPDATE*` 参数；预先存在的回执路径拒绝派发。独立 dispatcher 重建计划时使用同一原候选。
3. 生产 NSIS `.onInstSuccess` 在既有可选重启之前执行回调。只读核对本安装器的注册路径、产品、版本、publisher、主 EXE 和 uninstaller，再以 `CREATE_NEW` 写 UTF-16LE 回执，包含 nonce、布局、bundle ID、当前 PID 和原生创建时间。新增回调不写注册表、不启动 App、不删除路径；真正安装器仍有其既有安装副作用，本轮未执行。
4. Rust 严格核对完整回执、Accepted 阶段、原进程创建身份、原生退出证据和退出码 0；再验 signed inventory 的真实安装文件。回执、文件与目录的读 lease 保持到持久化结束，而非哈希完便释放。
5. 只能用私有构造的 `VerifiedCompletion` 发布 DPAPI terminal archive。完整原 record CAS、archive 回读和 active-slot 原子清空完成后，才发布 Completed。archive 已落地但 active 清空失败仍保留原候选；重开后必须重新验证才能幂等收尾，不靠等待或退出码猜测成功。
6. Completed 明确表示原候选已完成，不能 resume/replay；后续真实候选可以新建 nonce。修复上一安装器的内存 process witness 缓存阻碍下一安装器的问题；缓存被替换不代表放宽 PID + creation 身份检查。
7. 原界面可能晚于另一窗口查询。增加只读历史 completion DTO，重新认证原签名清单、DPAPI archive 和原回调，可按原 nonce 返回历史完成；它不能构造 journal 写权限，不能退休或释放新候选，也不宣称后续版本的当前文件树仍等于旧版本。
8. React 只对精确原 nonce/version、phase=completed、message=null 的显式结果释放原恢复状态；null、错误、他人的 nonce/version 仍不能被当作完成。下一次检查重新发现其他窗口的新候选，不能跳过它。没有新增未本地化用户文案。
9. release producer 接入 bundle ID 声明，并断言 NSIS currentUser / hook / template 一致；Windows CI 加入必须有真实 NSIS 编译器的回调编译检查。仅验证本地脚本/配置，本轮没有执行远程 CI、签名或上传。

## 当前冻结源码上的验证

证据根：`tools/computer-use-probe/.run/windows-install-completion-20260930/`。

| 项目 | 实际结果 | 边界 |
| --- | --- | --- |
| 默认 updater library | **86 passed / 0 failed / 7 ignored** | features=`default,rustls-tls,zip`；7 个 ignored 是专用 owned-child 入口，不把它们计作顶层 passed |
| 独立 no-zip updater library | **85 passed / 0 failed / 7 ignored** | 实际单独编译，features 仅 `rustls-tls`；独立 consumer manifest/lock 与前阶段 pinned 输入相同 |
| App 直接依赖块 | **38 passed** | updater 7、shutdown 3、browser process 7、MCP session 21；不是 1,735 项全 App library 测试 |
| 前端更新相关 | **141 passed / 5 files** | useUpdater 40、updateRecovery 4、honesty 46、About 9、i18n 42 |
| producer / portable / hygiene / NSIS | **58 passed / 0 skipped** | 15 + 29 + 12 + 2；没有把 fixture 签名字符串当作发行密钥证据 |
| 生产 NSIS 回调宏 | 真实 makensis 3.11 编译通过，0 compiler warnings | 私有 inert fixture **仅编译、未执行**；没有原生注册安装/回调执行验收 |
| 完成路径 / 崩溃窗口 | native owned-process + protected-file tests 通过 | 回调 bytes 为合成 fixture；真实 Windows sharing violation 阻断 active 清空，重开幂等完成，不是实际 installer 崩溃测试 |
| App + updater / 独立 no-zip Clippy | 均通过 `-D warnings` | offline / locked；分别对应实际构建 feature 集合 |
| TypeScript / 选定 ESLint | 通过 | `tsc -b --pretty false`；相关 UI 文件 `--max-warnings 0` |
| fmt / diff / workflow | 通过 | workspace 和 vendor 分别 fmt；CI/release YAML；4 个 PowerShell block **只做语法解析，不执行** |
| 生成后的 ACL | 本地 `main` / `session-*` 专属 | 两条 recovery command 权限不在 updater default、不授予 remote 或其他窗口；上游 api-iife.js hash 未改变 |
| 输入冻结与依赖 | **106 项选定源零漂移** | Cargo.lock 保持原 hash；不是全产品最终候选冻结 |

最终日志：`vendor-tests-final.log`、`nozip-tests-final.log`、`app-*-tests.log`、`frontend-tests-final.log`、`node-tests-final.log`、`clippy-final.log`、`nozip-clippy-final.log`。
构建及执行参数：`app-vendor-build-complete.jsonl`、`native-artifacts.json`、`nozip-artifact.json`、`run-native.ps1`、`run-nozip.ps1`。`final-review.json` 记录实际 feature、test executable hash、生成 ACL 和冻结源核对。
先前失败的编译、fixture warning、格式和缺失 consumer src 日志均保留；最终 runner 不引用这些失败构建作为成功证据。

## 可复查产物

- 当前源冻结：`source-freeze.json`；最后 source verify 和 receipt 绑定 106 项当前源。较早 `source-freeze-before-ui.json` / `source-freeze-before-format.json` 仅作历史，不说成当前零漂移。
- 最终 NSIS compile-only fixture：`grok-nsis-completion-5WzsFI/`。生产 hook SHA `bf451881514aec425d4a51a6ed268f7a80fcf3ed75ecbf8e04df295194e81b97`；编译 EXE SHA `e45684146143858fc36f708280e31a6a78b82dced45e213f965bc62cdbaeb6be`。不执行该 EXE，不宣称 NSIS 产物可重现。
- App Cargo.lock SHA `11242806aa4ea001a591207ad49b850551d632d5ef71f115368350f8e8da1ec6`。
- 前一便携包 receipt SHA 保持 `168f610839aaf7b071feea984fc909ddbde318b0d5c2c0f84414a733bb4fa5c9`；前一 inventory receipt 保持 `d650ddc13f44206b025e676cf38f879628f8ce6eaa2eb28570f805436cbba315`。它们的历史源码快照不应重新写成当前状态。
- `receipt.json` 只封存本阶段选定源、本文/3 个当前索引、日志与测试产物；独立 Python 逐项回读。它不是 publisher 签名、不代替完整发布验收。

## 仍未完成 / 下一项

1. **真实 current-user NSIS 安装回调仍未执行验证。** 现有运行测试使用真实 owned Windows 进程与合成 callback；宏编译不能证明真实注册安装、回调写文件、安装版 App 启动和下一次更新的端到端效果。下一项应在隔离的安装验收环境验证该链，而非改成只凭文件匹配或永久 blocked。
2. 无 completion contract 的历史安装器、MSI、失败/回滚/dispatcher 或机器失效后的未知结果尚未形成完整可用对账；签名 clean install → update → rollback → uninstall 和便携更新身份亦未全面验证。
3. 普通 NSIS 回调不是单独签名文件，依赖 signed payload/contract、原生进程 custody、受保护 nonce/root 和既有 DPAPI threat model。同用户恶意代码/管理员仍在既有信任边界之外；不宣称绝对防篡改或任意电源故障可恢复。
4. 完整原始范围不变：Windows x64、macOS arm64/x64、Linux X11 **与原生 GNOME Wayland**；Desktop / managed / existing tabs / App WebView；App / ACP / MCP；完整动作、中文 IME、clipboard、取消/恢复；签名安装更新回滚；窄窗/DPI/权限 UI；真实 Grok E4；**同一最终冻结候选的 12h active soak**。任何必需项缺当前权威证据都不能 complete。
5. 没有 commit/push/PR/tag/release、发行私钥读取、真实安装器/主 App 执行、注册表变更或用户 App/浏览器进程终止。只编译和运行有明确所有权的测试程序。

**阶段判断：progress；完整目标仍 active，尚未满足 complete 或 blocked 审计。**

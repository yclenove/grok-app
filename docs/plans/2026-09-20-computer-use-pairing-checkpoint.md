# Computer Use：安全配对收尾检查点

日期：2026-09-20（Asia/Shanghai）；分支：`feat/computer-use-implementation`。
HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`，已有脏改动全部保留。

结论：**partial — not releasable**。真实源码扩展配对已通过；已有标签页
Share/控制功能没有完成。这不是最终验收，不是安装版、跨平台或真实模型结论。

后续增量见 [连接租约检查点](2026-09-20-computer-use-lease-checkpoint.md)。
下文“心跳尚未实现”以及六项测试数量保留为本检查点的历史状态，不代表最新代码。

## 本段完成

1. 修复 App 配对探针的 Rust Result 错误类型推断，完成重新编译。
2. 为 process-wide Broker 探针加入 owner-marked 隔离 App home 校验、
   精确六项检查名校验、失败信息脱敏和离开时 Host cleanup。
3. 删除无人调用的旧 `pairing_ok` 宽松辅助判断，避免后续误作权限边界。
4. 把 App-shell 的「DTO 是否包含 secret 字样」改成结构化字段检查、
   App 确认、撤销和撤销后再次确认应拒绝。该 App-shell 场景本段仅编译，
   **未运行完整 Tauri shell**，不把源码修改当运行通过。
5. 补上扩展存储删除失败时仍须尝试 Host 撤销，以及 HTTP 200 但 status
   正文无效时不得报告 paired 的处理和回归测试。
6. 改正探针名称为 `no-popup-key-message`：它不证明可信扩展 popup 无法
   直接访问同源 session storage，只证明消息 API 不导出 key。
7. 重写配对 ADR/源码安装说明，纠正 wiki 与旧 transport ADR 的完成度描述。

上一段已实现的 App-only 80 位验证码、HMAC 绑定、严格 HTTP 校验、单次消费、
nonce 对应确认、feature-off 和撤销失效、15 locale 在当前代码中保留并验证。

## 本段首败与定位

真实 App Broker + 私有 Node 20.18.0 首次运行在启动 Chromium 前失败：
`read ENOTCONN`，没有进入配对断言。保留了 `app-pairing-01.log`。

隔离对比证据：

- 相同 Node/Chromium，经普通启动环境能完成六项配对检查。
- Rust `env_clear` + SystemRoot/TEMP/TMP，可复现绝对路径子进程的
  ENOENT/ENOTCONN；补 WINDIR、USERPROFILE 或 SYSTEMDRIVE 均不能修复。
- Node 作为父进程会补入多项系统环境，所以 Node 的“最小环境”测试
  与 Rust `env_clear` 并不等价；检查的是环境变量名称，未输出账号/密钥值。
- Rust 环境只增加 `PATH=SystemRoot\System32` 即可通过相同真实配对场景。

修复只在 probe 启动环境增加系统目录 PATH，仍使用私有 Node 绝对路径；
不继承用户 PATH、代理、密钥或 NODE_OPTIONS。之后重编译，真正的
`cu_probe existing-tab-extension` 通过，不用独立 fixture 替代它。

## 本段验证

| 验证 | 当前结果 | 边界 |
| --- | --- | --- |
| Core `--all-features` | 332/332，0 ignored | 单元/模块测试，不是平台验收 |
| Driver 集成测试 | 12/12，0 ignored | 私有管道进程；不等于 native OS 能力 |
| 配对 UI/API/Panel + locale | 69/69 | jsdom/invoke mock + 15 locale 目录 |
| 扩展 PairingClient | 11/11 | 包含新增存储故障和无效 status |
| 真实 Chrome MV3 + 私有 Node + App Broker/IPC | 6/6 | E2 配对切片；无 tab 控制 |
| TypeScript + 定向 ESLint | passed | 本段配对相关前端 |
| 扩展 locale generator `--check` | passed | 15 locale |
| Core strict Clippy | passed | all-targets/all-features |
| App strict Clippy | passed | default 与 computer-use-probe 两种配置，all-targets |
| Rust fmt check | passed | 当前 workspace |
| 仓库代码质量 final gate | passed | 千行文件 80/80；不是功能完成 |
| 全新完整 App-shell | not_run | 修改后的 typed pairing 场景尚缺运行证据 |
| Windows 安装 / Edge / macOS / Linux / Wayland | not_run 本段 | 不继承历史或其他平台通过 |
| 真实模型、候选冻结、长稳 | not_run 本段 | 未建立最终候选版本 |

Core doctest 发现 0 项，不计作有覆盖的测试。

真实配对六项为：stable-extension-id、pair-with-app-confirmation、
no-popup-key-message、extension-revoke、app-revoke、worker-restart。
其中双端确认包含扩展在 App 未确认时应失败的独立断言。

## 可定位的证据

忽略目录：
`tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/`

- `owner.json`：本段 home 身份；`app-home/`：无真实账号的私有运行时。
- `app-pairing-01.log`：首败，未覆盖。
- `app-pairing-02.log`：修复后实际 App Broker/IPC + 扩展通过。
- `node20-pairing-01.log`：隔离 Node20 fixture 对比，不当成 App 路径。
- `core-final.log`：332 + 12 的实际执行结果。
- `app-probe-clippy-final.log` / `app-default-clippy-final.log`：两种 App 配置。
- `env-probe.mjs` / `spawn-probe.rs`：仅本段诊断复现，不是产品代码。

通过时 cu_probe.exe SHA256：
`EF3583B00A7E86982B875CD3A1CB9B155AAD24BB0EA250CB28D6B8F3FEB52220`。
这只是该工件身份，不冒充完整工作树 freeze fingerprint。

完成后按本段 home/profile 标识查询，没有关联的 Node/Chrome 活进程。
首败留下的临时 profile 目录仍保留，未扫描删除全体同名前缀目录；不能报告
“全部临时文件已清零”。已有更早测试目录也未动。

## 剩余关键缺口

- abrupt browser death 后 Host 缺 heartbeat/lease expiry；当前只知道
  浏览器 session storage 会消失，不能承诺 Host 立即收回授权。
- 没有真实 Share、typed transport 或生产 ExistingTab adapter。
- 源码扩展未进入安装工件；源码稳定 ID 不是商店签名或源代码完整性证明。
- 全部 OS、安装、真实模型、App-shell 和最终长稳证据仍需独立补齐。

下一步执行 [剩余开发顺序](2026-09-20-computer-use-remaining-execution.md)：
先 C0 连接到期/异常退出清理，再 C1 分享授权、C2 通道、C3 Broker/MCP
闭环，不先堆 UI 或把 Core 状态机当现成用户功能。

**未 commit / 未 push / 未提 PR**。

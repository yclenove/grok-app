# Computer Use：Linux SDK 双架构检查点

2026-10-13，`feat/computer-use-implementation`。前一批同步提交 `8107bb07`
已包含 fork 与源仓库最新 main；本批继续完整交付目标，不调整平台、surface、
安装/升级、真人模型、UI UX、12h 或 D00–D31 范围。

## 修复的阻塞

fork 新增 ARM64 Release 使用 `ubuntu-24.04-arm`，而 CU 的 SDK 审计、
stager、产品 manifest parser 和 build link policy 写死 x64；release SDK
步骤也要求所有 Linux runner 都是 Ubuntu22.04 / glibc2.35。
这些问题与 ARM64 受管浏览器资源缺失是两条独立链路。

本批将 SDK 链路按实际目标绑定：

| 目标 | 原生 runner / ABI 上限 | ELF machine / 唯一允许的动态 loader |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | Ubuntu22.04 / glibc2.35 | X86-64 / `ld-linux-x86-64.so.2` |
| `aarch64-unknown-linux-gnu` | Ubuntu24.04 / glibc2.39 | AArch64 / `ld-linux-aarch64.so.1` |

- auditor 逐件核对目标架构、loader、SONAME、依赖、GLIBC 和固定许可/补丁；
  不接受提高该目标 ABI 上限来放过不兼容库。
- stager 的清单记录实际选择的目标和 ABI；不同架构资源不能复用或覆盖。
- checker 的可选第四参数指定目标，要求匹配原生机器，保留旧 x64 调用兼容。
- 产品 loader 按编译架构检查 manifest，未知架构、跨架构或错误 ABI 拒绝。
  DT_RPATH、资源/hash/链接边界、原加载库身份和拒绝系统 fallback 合同保留。
- Tauri before-build 显式向 SDK stager 传递 bundle triple；ARM64 浏览器映射
  仍缺失，因此 before-build 仍会准确拒绝不完整的 ARM64 发行资源。
- CI SDK job 变为 Jammy-x64 / Noble-ARM64 原生矩阵，release SDK baseline
  跟随各自已声明的 runner。CU 分支 push 也执行 CI，不创建 PR/tag/Release。

## 实际验证

证据根：`tools/computer-use-probe/.run/linux-sdk-targets-20261013-01/`。
使用已有隔离 Ubuntu22.04 用户态与私有工具链；不安装到 Windows/WSL 全局库，
不修改共享 seed、用户预览、home、登录或草稿。

| 检查 | 实际结果及范围 |
| --- | --- |
| Linux Python auditor/stager/checker | 18 + 10 + 12 = 40 passed，0 skipped；含真实 cc/readelf、FIFO、symlink 和跨架构反例 |
| 真实私有 SDK smoke | 原 SDK 与迁移 runtime 均通过，16件许可/ABI清单通过；未连接 portal/桌面 |
| 当前产品 Rust loader module | 10 passed，0 ignored；包含双架构 parser 合同，原生执行目标为 x64 |
| 当前产品 module strict clippy | all targets，exit0 |
| 同一真实 release ELF 的安装/迁移/故障 | 18/18 passed，binary SHA256 `75e48e09f7dc73e880c4c46b1cda529ce74849a9c22dbbc31daa71c6b51299bd` |
| Node target / runtime-command | 15 passed，0 skipped |
| Rust fmt / targeted ESLint | exit0 |
| 六份 Python AST | passed |
| CI / release YAML 与两条 SDK shell 实例 | runner/target/ABI/stager/fallback 路径匹配，bash语法通过 |
| ARM64 原生 SDK | 本地 not_run；交给已接入的原生 CI，实际 run 身份与结果记录在后续回执 |

第一次 native runner 已完成 40 Python / 10 Rust / strict / SDK smoke，
随后 fault runner 因隔离环境不存在指定系统库而在 preflight 拒绝。
保留该 exit1 的 `native.log`；复查已保留的 PipeWire1.0.5 SDK 后，使用同一
已编译 ELF、同一冻结源码及新 fault 输出继续，18项全部通过。
未重编换候选、删除断言或将首败改成成功。

显式脚本 lint 还暴露 before-build 既有 implicit `process` 的29项 no-undef，
已补 `node:process` 导入，没有关闭规则。初始错误摘要和最终日志均保留。
本批不重记前一批 8578 前端 / 2399 Windows 的结果为新执行。

## 例行清理

两个原 native owners 已 joined；再次确认隔离 root 无 live owner后，
只删除 `.../work/native-sdk-targets-20261013-01/loader-proof/case-00` 至
`case-12` 及 `case-17` 的14份规则文件运行副本。
完整绝对路径见 `cleanup-receipt.json`；链接/FIFO反例 `case-13`–`case-16`
共4份继续暂缓，不绕过链接/特殊文件规则。

逻辑删除 **65,796,778 bytes**；WSL ext4 可用空间即时观测增加
**67,620,864 bytes**，Windows VHDX 的物理回收量未知，二者不混报。
当前 binary/target、SDK、冻结源码、原来源、报告/log/hash及首败证据保留。
可用保留的 ELF/resources 重新运行 `jammy-product-loader-faults.py` 到新目录
再生这些副本；没有删除整个 target/workspace，也没有停止无关进程。

## 仍需完成

本批证明 x64 SDK 与当前产品 loader 无回归，并将 ARM64 同一链路送入原生 CI；
不证明 ARM64 SDK、完整 App、安装或桌面输入已经验收。
ARM64 Node/Playwright/Chromium 资源、compiled pins/source locks/pack mapping
和资源来源仍开放；整体 ARM64 release 尚不可用。

维护版浏览器来源/完整 NOTICE/SBOM/源提供、安全与稳定性策略、R2 最终 pack
晋升和 R3/R4 全部原生/安装/真人/四 surface/同最终候选12h门禁继续 active/partial。
App macOS11+、CU受管浏览器macOS14+保持；当前产品仍是旧运行时。

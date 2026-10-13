# Linux 产品私有装载接线检查点

分支 `feat/computer-use-implementation`；整体 active / partial，未新增提交、push、PR 或正式发布。
本批没有替换当前 Windows app144 或其登录/会话/home，也未发新模型任务、自动批准或代选文件。

## 已实现

- `linux_native_runtime.rs` 在 Linux main 的最早入口检查安装资源，`run()` 使用 OnceLock 同一结果；在 session/UI/clipboard keeper 之前执行。
- 与产品/探针共用的 `build/linux_native.rs` 写入实际 DT_RPATH，而不是 RUNPATH；两个 executable-relative 布局覆盖正常安装、迁移与 Cargo 输出，优先于 host-WebKit 的 LD_LIBRARY_PATH。
- manifest 编译嵌入并与安装副本逐字节比较；严格16文件/允许目录、长度/SHA/版本/目标/基线，拒绝额外模块、空目录、配置 drop-in、链接和特殊文件。FIFO 检查不等待读取；文件读取有上限。
- 已装载 DSO 使用 `pw_get_library_version`/dladdr 核对实际路径和版本，保留非 UTF-8 路径；不调用 pw_init、portal、远端或桌面。
- 固定进程环境的私有 lib/SPA/modules/client.conf，覆盖继承的错误 NAME/PREFIX。LD_PRELOAD/执行前签名/恶意并发替换与已映射inode屏障不在本批证明范围；这是装载完整性诊断，不能称为安全沙箱。
- Python stager 先审计并在复制后重审；新副本目录0755、普通文件0644，源SDK不改。既有不同内容/权限拒绝覆盖或修复；生成 manifest 不含构建机路径。
- Linux before-build 必须指定审计过的 `GROK_CU_PIPEWIRE_RUNTIME`；release 缺 manifest 硬失败。生成的原生资源/探针输出不提交进 Git。
- Ubuntu22 CI 加入真实产品模块探针/严格Clippy；release 配置接私有SDK准备、ABI/许可/实际模块门禁与环境。本地YAML已解析；远端工作流未运行、没有发布资产。

## 实际证据

原 WSL Debian 只提供内核，全部本批原生编译/执行位于独立 Ubuntu22.04/glibc2.35 用户态：
`/var/tmp/grok-cu-jammy-sdk-20261012-01`。固定 Rust1.98 的 rustc/cargo/std 和 Clippy 官方组件在执行 installer 前核对官方 metadata 的 SHA，安装仅在 `/work/rust-toolchain-1.98`。

最终冻结输入：`tools/computer-use-probe/.run/linux-jammy-product-loader-20261012-04/`。

- stager8 + SDK auditor16 + 实际产品 Rust 模块9，均通过，0skip；不是完整App测试。
- 同一实际 Rust ELF 的18个场景全部按指定exit0/2通过，不把crash/signal计成拒绝。
- 正向：安装布局、Cargo布局、空格/中文迁移、非UTF8路径、旧SDK前置、继承错误环境。
- 负向：缺/旧DSO、缺module、组件改变、错manifest、额外配置/资源、manifest链接/FIFO、component FIFO、资源祖先链接、缺资源。
- 实际 ELF 含精确 DT_RPATH；最终 probe SHA256：`dcf60d5b5c703661ac164c7ab34997ac81178b20638d803ba777460ac0e44ba5`。loader-02/03/04一致。
- loader-03 的严格 Clippy `--all-targets -- -D warnings` 通过，产品模块与04相同。
- Node target/before-build 门禁7通过；全项目 fmt、lint、fresh前端 build:ui 和 final quality通过。App+Workbench13573、≥1000行文件80不增长。
- Vite 仍有既有动态/静态 import 与大chunk警告，不能称为零warning。

产品模块SHA `6e054ebae5a97752c87da917b58e9781462fae3766a4cee28152a7d32ad3cc65`；共享link策略SHA `9ce5e309e6a0e6cdbc372a15f55358fe214cb722afcce9d5402d172886dd0653`。
最终stagerSHA `0426f1fc69394121755a51049d65069351e0ac6518c93506e18d36de6dc72b5a`，auditorSHA `c694a6ff8e67f1c46c743766624fde4b46864d4cda1e9423f97b2b86d719ace5`。

## 完整 App 与尚未通过项

fresh生产前端已构建，完整 App 的冻结源位于 `/work/product-app-01`。
原 session72293 已 exit1，缺少 Linux seed manifest；原日志在 `linux-jammy-product-app-20261012-01/app-build.log`。
续行增加原公共 worker 并用生产 prepare/check，明确开启 `tauri/custom-protocol`；session69470 已 exit1，官方 Node 下载请求失败。
不能把这些失败或 Cargo release 名称写成完整生产 App 已通过。

后续离线输入复核、实际平台编译错误与新候选见
[完整 Jammy App 续行检查点](2026-10-12-computer-use-jammy-app-build-checkpoint.md)。

下一步为完整App的实际链接/ABI/私有库身份、正常退出与安装态；再做三格式包/资源许可/依赖/升级修复回滚和portal验收。
现有 `linux_webkit.rs` 未在入口接线；外部host-WebKit wrapper仍需独立验证，不能借本批探针称AppImage helper生命周期已完成。
真人ACP/MCP/Grok/picker、四surface、Windows installed、macOS原生ARM/Intel、Linux X11/Wayland、UI UX、模型矩阵、同最终候选12h及D00–D31完整范围仍保留。

## 产物归属与清理

工具链/SDK/当前probe/完整App编译依赖、Windows唯一预览及原home保留。
新增精确退休工具只针对已完成loader-01/02/03的case复制品，拒绝链接/特殊项和任何live/uncertain private-root owner；不删整workspace、target或共享cache。
`linux-jammy-product-loader-20261012-04/cleanup.json` 与 `removals.jsonl` 已记录：
删除738个退休复制文件，逻辑192469688 bytes；WSL可用量观察增197791744 bytes。
Windows物理VHDX回收未知；12个带故意链接/FIFO的case目录暂缓，原失败证据、当前04、SDK/工具链及 App 编译缓存保留。

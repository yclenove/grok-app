# ADR：Computer Use 默认 desktop backend

日期：2026-09-09  
阶段：S2.1  
工作区：`H:\aicoding\grok-app-computer-use`  
分支：`feat/computer-use-implementation`

## 决定

**产品默认 desktop backend 是 Grok App Host 适配器**（Windows Win32、macOS CG、Linux X11），经 `computer-use-core` Broker 调度。

Cua（`https://github.com/trycua/cua.git`）**不是**发行运行时，也不是根 Cargo/pnpm 依赖。本机 gitignored clone 只作研究与将来 S3.2 对照。

缺能力时：**补 App adapter 或阻断该 OS 首版**，不得静默混用 Cua 与 Host 双轨冒充完成。

## 来源与 pin

| 项 | 值 |
| --- | --- |
| URL | https://github.com/trycua/cua.git |
| License | MIT（clone `LICENSE.md`，Copyright Cua AI, Inc.） |
| 磁盘 clone | `tools/computer-use-cua/`（gitignore） |
| 2026-09-09 HEAD | `18217d0f03f3f9d9347f3c78d0e002d5fd6fdece` |
| 旧 NOTICE 声称 | `c5a15f3df3b29ffbe774de9f33d632fe75afec75`（作废，不得当产品 pin） |
| 进入发行包 | **否** |
| 复现 | 不在本任务执行第三方 README / 安装；S3.2 若接入须另写固定 commit 与构建命令 |

## 能力缺口（相对首版门槛）

| 面 | Host 现状 | Cua 角色 | 结论 |
| --- | --- | --- | --- |
| Windows | EnumWindows + BitBlt + 子控件/SendInput；非完整 UIA | 研究对照 | 产品继续 Host；UIA 在 S4 |
| macOS | CGWindowList/CGEvent；AX 不完整；实机 not_run | 研究对照 | 不引入 Cua 冒充 AX；S5 补 AX |
| Linux X11 | QueryTree/GetImage/XTEST；无 AT-SPI | 研究对照 | S6.1 补 AT-SPI |
| GNOME Wayland | `native_wayland=false` | 若有 portal 实现也不得未验证就接 | S6.2 单独做；禁止 XWayland 冒充 |
| 私有 worker | `computer-use-core/src/driver` 协议+Job Object | 可选未来 worker | S2.2/S3.2 再接线；现在产品桌面不经过 Cua |

## 不做

- 不把 Cua 加进 `Cargo.toml` / 根 `package.json`
- 不把 clone 提交进 git
- 不把当前 HEAD 写成“已验证产品 runtime”

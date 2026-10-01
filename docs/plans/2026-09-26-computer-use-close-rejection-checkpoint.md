# Computer Use：关闭拒绝不能被关闭事件改写成成功

实际日期：2026-09-26，Asia/Shanghai。继续完整 Computer Use 与 UI/UX 重设计目标；未发布。
分支 `feat/computer-use-implementation`，原有 dirty/untracked 工作保留，当前用户 App 未替换。

## 原生失败仍保留

日志目录：`tools/computer-use-probe/.run/close-diagnostics-20260926T1027/`。
此前独立包的 `cleanup-round-1.log` 再次失败，实际 95.84 秒退出 1，循环立即停止，未运行第 2/3 轮。
`/close` 第一段通过；`/cancel-run` 在 20,137ms 出现 context close 事件，
50,138ms 开始强制关闭，53,374ms 返回，原生调用自身耗时 **3,236ms**。
期限内未确认原始 close Promise 和浏览器进程退出，94,978ms 仅用原始 Node 子进程句柄做失败收尾。
`/shutdown` 正常验证阶段未到达；不能将收尾后的“没有匹配进程”倒推为期限内已退出。

保留隔离目录：`C:/Users/Administrator/AppData/Local/Temp/grok-cu-cleanup-http-TYgJ2c`。
之前 `grok-cu-cleanup-http-AFhb2V` 的 157 秒失败也保留。禁止清理不确定退出的 profile。
3.236 秒诊断排除了“全部等待都由同步 taskkill 调用本身占用”的简单解释，并未证明根因。
新增诊断只记原始调用的 status/signal/error code；不改参数、返回结果或抛出，不记录原始输出/PID/凭据。
失败 after hook 现在额外打印保留目录和错误数量，避免 Node 主测试错误掩盖清理失败。

## 本批产品修复

确认 `managed_run_idle` 信任精确 worker 的 run-status；其他层没有弥补本次错误释放。
原 `ContextClose` 在原始 close Promise **拒绝** 后，只要收到 context close 事件，
也会释放 cleanup lease；下一次 close 被转换为成功，可能清除 profile 并错误恢复 idle。
真实测试已经证明关闭事件可以明显早于浏览器进程退出，因此事件不是拒绝收尾的补充成功证明。

- 分开记录原始 Promise 的 fulfilled 与 settled，拒绝状态不能被事件“恢复成功”。
- 原始 Promise、错误和 cleanup lease 保持同一对象身份，不重复调用 Playwright close。
- 真正 fulfilled 但事件迟到的收尾仍可正常对账，只释放一次。
- Pause/Stop 回读继续保留物理占用，拒绝 Resume；独立 owner 的任务不被全局锁住。
- 未改变 HTTP 期限，未自动重放输入，未新增 PID 重新发现式终止权限。

`close-rejected-red.log`：**12 passed / 4 failed**，先证明缺陷。
`close-rejected-green.log`：ContextClose + RunOperations **26/26**。
`worker-unit-final.log`：显式选定 19 个 worker 单元文件 **133/133**，不是全量原生套件。

## 原生验证范围

新增 `cleanup-rejected.test.mjs` 使用生产 server/ContextClose 和真实 Chromium。
仅测试 preload 在真实原生 close 事件发生后注入拒绝，仍独立观察原始 native close Promise。
断言 close/cancel 不能转为成功、profile 不被遗忘、占用为 1、Resume 拒绝、独立 profile 可正常关闭。

首个测试脚本把已 Pause 后的 `/close` 误期望为 500，实际正确拒绝为 409；
`close-rejected-native-source.log` 保留。修正测试顺序，先在运行态验证 close 重试，
再验证 Pause/Resume/cancel；没有为了测试放宽产品 admission。
`close-rejected-native-source-r2.log`：**1/1，exit 0**。
`cleanup-source-r1.log`：三个清理接口与迟到完成对账 **1/1，exit 0**，18.63 秒。

故障测试最后故意终止自己的原始 Node 子进程句柄，并验证已观察到的原生关闭后才删自身 profile。
这是测试收尾，不代表产品能自动恢复受损 worker，更不代表正常 shutdown。
本批不覆盖自然 close 无原始调用的全部情况，或精确原生进程句柄恢复的完整产品设计。

## 独立运行时包

新 seed：`H:/aicoding/cu-close-rejection-20260926T1054/seed`。
离线 prepare 成功、importProbe passed；源码与包内 worker/ContextClose SHA256 一致。

- manifest：`e10673cbac4c64cec9c6da23505ea937f0cc1f5546623b2d50848e7760bbc1fe`
- tree：`cec9216bc6c0151a58bc3e0b58e5eae49af914477e3a648b32cb6e291e519255`
- Chromium：`63c6075faf6d986bc8e9f11996f5d2e784e7f4d9471366b7a2ad6eebaca245e3`
- worker：`e4ff3eea7113d79626c785dbd541cfbdb9afa84cba8720475cb820fb8977f5e4`
- ContextClose：`1315c70a2f076c403d4a54b15b513f1624022177d5ecd876799f735635749c1e`

包前卫生检查 444 files、0 hits，digest
`ecb90adb6661f12e88ec828913b6a2f4910b0bd01228422cd39c7641b7ab8dbb`。
用新包的 Node、worker 和依赖跑两个原生测试：`rejection-package-native.log` **2/2，exit 0**，22.97 秒。
Chrome 来自此前同一固定版本的独立可写副本，不改 seed 内浏览器，不接触普通用户 profile。
新包包含产品修复，不包含故障注入 preload 或测试文件；这不是签名安装版验收。

`rejection-package-final-check.log`：离线 `pnpm check:computer-use` exit 0、importProbe passed。
`rejection-package-final-result.json`：check/audit 均 exit 0、444 files、0 hits、前后 digest 一致。
随后用同一新包完成 `rejection-package-r34.log` **21/21，exit 0**（20 个具名子场景 + 父测试），
53.60 秒，日志记录实际 worker SHA256 与 Node v20.18.0。
覆盖观察、输入、去重、未知结果不重放、取消、owner/staging 隔离、截图和正常退出；
没有用测试次数代替签名安装版或跨平台验收。

只读核对现有重设计面板：`stop_requested` 仍锁定切换/授权/Resume，
“本机控制已停止，工具清理仍未完成”必须来自 fresh Host 的 `stopState=stopped`，
不会仅凭 MCP cleanupPending 宣告物理控制已停。本批没有另外修改 UI 产品源码。

## 未关闭的最终版要求

Windows 原生退出长尾及历史发布拒绝访问仍未证明根因修复，绿色回归不覆盖原失败。
完整 Windows/macOS arm64+x64/Linux X11/GNOME Wayland、四种 surface、App/ACP/MCP、
正式扩展、签名安装/更新/回滚、真实 Grok E4 和冻结 12 小时 active soak 均按原计划继续。
UI/UX 整体重设计保持必需项；R14 的 197 项回归、79 个编译浏览器场景不等于完整安装版交互验收。
目标保持 active，不因为本批关闭状态修复而宣布最终版。

# Computer Use R16：更新源码的原生 UI 复验

日期：2026-09-26，Asia/Shanghai。完整目标仍为 Computer Use 最终版和整体 UI/UX 重设计。
未 commit/push/PR；不会用 UI 局部通过替代原生控制、安装版和跨平台验收。

## 本批产品修复

实际 ExtensionsPanel 进入 Computer Use 时，之前仍自动执行 skills/MCP/plugins 的 CLI 检查，
并显示不属于 CU 的检查错误。现在 CU 只加载自身 Host 状态，不启动这三个检查；
从其他扩展 tab 切入后，既有错误和迟到错误不会污染 CU 页面。
切回 CLI 扩展 tab 仍刷新并显示真实错误，不清除或吞掉它们。
相关 path/action 提示也只出现在 CLI 扩展页；没有改变 CU 授权、默认关闭或 Host 错误展示。

- `ExtensionsPanel.computer.test.tsx` 新增三条回归，先 3 failed / 2 passed，修复后与设置页联合 35/35。
- 根目录 TypeScript 和上述两项组件/测试的 ESLint 均 exit 0。
- 最新编译浏览器：85/85、terminal exit 0；六个真实扩展外壳场景另检查 CLI inspect 调用数为零。
- Browser IPC 是 mock，不能冒充实际 Native/CLI 调用跟踪；原生证据另列。
- artifacts：`tools/computer-use-probe/.run/ui-native-r16-20260926/`，保留红测试。
- browser：`ui-redesign-2026-09-26T03-50-20-005Z`；运行前后原 seed digest 均为
  `c4430eb59a12744c3a7e8bea7334bc9a4a70bef40d99e4f9ed65fa9384bc88b2`。

## 更新原生构建的边界

新冻结源码：`H:/aicoding/cu-native-r16-20260926/source`，3464 项文件哈希逐项复制核对。
使用独立 app identifier、构建目录和新的 App/agent home；不复制账户、会话或授权。
从 R15 构建缓存普通复制 6387 个文件（不是硬链接），不会在 R15 输出上编译。
当前 R15 exe 的 SHA256 仍为
`4399F1C57ECA8F4830C87E542DA0BCBA6B1E5E4B317D9718C9968EE244E5F610`。
现有用户 App、R13、R15、其他工作区进程均未关闭或替换。

构建命令：

```text
pnpm exec tauri build --debug --no-bundle --config src-tauri/tauri.review-r16.conf.json
```

首轮 `build-review.ps1` 被 Windows PowerShell 5.1 的 NativeCommandError 中断：
pnpm.ps1 的 stderr 中正常 Tauri INFO 被 ErrorActionPreference=Stop 当作异常。
该轮终态 exit 1、输出日志为空、没有成功 receipt；即使产生 exe 也不计为通过。
原脚本和 `native-build-wrapper-failure.txt` 保留。只在确认该轮及其子进程终止后重跑。

第二轮 `build-review-r2.ps1` 由 cmd 负责 stdout/stderr 重定向：
**terminal exit 0**，`native-build-result.json` 记录 11:59:12–12:01:47 +08:00。
前端、准备/导入与资源审计、Rust 编译均完成；Rust 33.57s。
保留原有大 chunk 和 linker stdout warning，不把它们改写成无 warning 构建。
最终 exe SHA256：
`1F5EF6C58A8B5D5E896920A54583103E1B4B872AB70B9B03AF76FC7D2B621FBD`。
源 seed / exe 旁资源 manifest SHA256 相同：
`E10673CBAC4C64CEC9C6DA23505EA937F0CC1F5546623B2D50848E7760BBC1FE`。

## 原生启动与尚未通过的交互

首次启动 PID 72952 的验收 settings.json 被 PowerShell 5.1 写成 UTF-8 BOM，
与 R15 的无 BOM 夹具不同，App 显示账户 setup 而不是预期的 deferred-account 主界面。
没有点击登录、复用登录或任何认证步骤。保留初始 home、日志和 launch receipt；
只在匹配精确 exe 和进程创建时间后停止该 owned R16 PID，没有关闭其他 App。

替代启动只修正**验收夹具编码**，未改变产品源代码或 exe，也没有制造登录状态。
`launch-review-r2.ps1` 使用显式 UTF8Encoding(false) 写入全新 `app-home-r16-r2`。
`native-launch-r2.json`：PID **56096**、创建时间 **2026-09-26T12:06:00.42025+08:00**，
3464 个冻结文件重新核验；资源审计记录 `launch-resource-audit-r2.json`。
实际资源审计为 444 files / 0 hits，digest
`ecb90adb6661f12e88ec828913b6a2f4910b0bd01228422cd39c7641b7ab8dbb`。
使用窗口前仍需重新核验 PID/时间/路径和唯一返回窗口，不能直接复用这些旧标识。

真实 WebView2 已显示新 App 主界面和产品导览，点击“跳过导览”后截图显示主界面。
同次 AX 树仍含已消失的导览：截图与 AX 不一致的事实保留，不宣称 AX 全面可靠。
之后进入设置的输入工具失败：

1. 使用返回截图引用时报 `unknown screenshotId screenshot-0`。
2. 重新观察后使用新的设置按钮索引，报 `call get_window_state before using this window`。
3. 重新 list_windows / get_window / get_window_state，唯一绑定后只重试一次，仍报同一错误。

按工具恢复规则停止原生输入；不改用 PowerShell UIA 或私有驱动规避。
`native-observations.json` 保存观察文本、截图元数据和上述失败；没有转储图片载荷。
**更新后原生设置、维护弹窗/焦点、窄窗口和浅色交互均未完成**。
不能把本批 85 个 browser mock 场景或 R15 旧 exe 的原生证据代替这些项。
当前独立 R16 App 保留在主界面，CU 未启用；无授权、删除确认、修复/安装或模型任务。

## 全目标仍开放

Windows 原生退出长尾/发布拒绝访问、macOS AX 与签名资源交付、X11 完整动作、
GNOME Wayland、WebView 物理取消与关闭性能、Chrome/Edge 正式扩展、全平台安装/更新/回滚、
UI 权限/缩放/跨平台矩阵、真实 Grok E4、冻结 12 小时 active soak 未由本批覆盖。
目标保持 active。
